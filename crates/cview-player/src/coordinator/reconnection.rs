//! `StreamCoordinator+Reconnection.swift` 포팅 — 재연결 트리거·시도·소진·방송 종료.

use std::sync::Arc;
use std::time::Duration;

use url::Url;

use super::{StreamCoordinator, StreamEvent, StreamPhase, RECONNECT_COOLDOWN};
use crate::reconnection::{AttemptCallback, ExhaustedCallback};

/// 재연결 시도가 첫 프레임을 기다리는 최대 시간.
const RECONNECT_FIRST_FRAME_WAIT: Duration = Duration::from_secs(8);

impl StreamCoordinator {
    /// 쿨다운 중 들어온 신호(대표적으로 fatal 오류)는 버리면 엔진이 죽은 채 남는다 — 이 신호를 다시
    /// 보내 줄 감시자가 없다. 쿨다운이 끝났을 때 그사이 다른 재연결·세션 교체가 없었고 엔진이 아직
    /// 오류 상태면 한 번 더 시도한다. 같은 쿨다운 창의 신호 여럿은 시도 하나로 모인다.
    fn retry_after_cooldown(&self, last: tokio::time::Instant, reason: &str) {
        {
            let mut s = self.st();
            if s.cooldown_retry_for == Some(last) {
                return;
            }
            s.cooldown_retry_for = Some(last);
        }
        let weak = self.weak();
        let reason = reason.to_string();
        let wake = last + Duration::from_secs_f64(RECONNECT_COOLDOWN);
        tokio::spawn(async move {
            tokio::time::sleep_until(wake).await;
            let Some(this) = StreamCoordinator::from_weak(&weak) else {
                return;
            };
            {
                let mut s = this.st();
                if s.cooldown_retry_for != Some(last) || s.last_reconnect_time != Some(last) {
                    return;
                }
                s.cooldown_retry_for = None;
            }
            if this.engine().is_some_and(|e| e.is_in_error_state()) {
                this.trigger_reconnect(&format!("{reason} (쿨다운 뒤 재시도)"), false);
            }
        });
    }

    /// 재연결 트리거 — 현재 URL 로 재시도한다. 일시정지·종료·정지 의도·재연결 중·쿨다운이면 무시.
    /// `bypass_cooldown` 은 이미 재연결 루프가 끝난 뒤의 즉시 재시작(네트워크 복구)용.
    pub fn trigger_reconnect(&self, reason: &str, bypass_cooldown: bool) {
        let engine = self.engine();
        let suspend = engine.is_some_and(|e| e.is_suspend_intent_active());
        let raw_url = {
            let mut s = self.st();
            if s.user_paused
                || matches!(s.phase, StreamPhase::Idle | StreamPhase::StreamEnded)
                || suspend
            {
                return;
            }
            let Some(raw_url) = s.stream_url.clone() else {
                tracing::warn!("StreamCoordinator: 재연결 실패 — 저장된 URL 없음");
                return;
            };
            if s.phase == StreamPhase::Reconnecting {
                return;
            }
            let now = tokio::time::Instant::now();
            if let Some(last) = s.last_reconnect_time.filter(|last| {
                !bypass_cooldown && now.duration_since(*last).as_secs_f64() < RECONNECT_COOLDOWN
            }) {
                tracing::info!("StreamCoordinator: 재연결 쿨다운 중 — 끝나면 다시 판단 ({reason})");
                drop(s);
                self.retry_after_cooldown(last, reason);
                return;
            }
            s.last_reconnect_time = Some(now);
            raw_url
        };
        self.update_phase(StreamPhase::Reconnecting);
        tracing::info!("StreamCoordinator: 재연결 시작 ({reason})");

        let weak = self.weak();
        tokio::spawn(async move {
            let Some(this) = StreamCoordinator::from_weak(&weak) else {
                return;
            };
            // 종료된 방송에 재연결하지 않는다.
            if this.check_stream_ended().await {
                this.handle_stream_ended();
                return;
            }
            // stale 데이터 오염 방지 — 저지연 서브시스템 정지, 프록시 연결 풀 정리.
            this.stop_latency_subsystems();
            if this.st().is_proxy_active {
                this.inner.proxy.reset_session();
            }
            this.refresh_master_manifest().await;
            // 준비 중 stopStream/streamEnded 가 끼어들었으면 좀비 재시작을 막는다.
            {
                let s = this.st();
                if s.phase != StreamPhase::Reconnecting || s.stream_url.is_none() {
                    tracing::info!(
                        "StreamCoordinator: 재연결 준비 중 세션 정리 감지 — 재시작 취소"
                    );
                    return;
                }
            }
            let attempt_weak = weak.clone();
            let on_attempt: AttemptCallback = Arc::new(move |attempt, delay| {
                let weak = attempt_weak.clone();
                let raw_url = raw_url.clone();
                Box::pin(async move {
                    if let Some(this) = StreamCoordinator::from_weak(&weak) {
                        this.run_reconnect_attempt(attempt, delay, raw_url).await;
                    }
                })
            });
            let exhausted_weak = weak.clone();
            let on_exhausted: ExhaustedCallback = Arc::new(move || {
                let weak = exhausted_weak.clone();
                Box::pin(async move {
                    if let Some(this) = StreamCoordinator::from_weak(&weak) {
                        this.handle_reconnect_exhausted();
                    }
                })
            });
            this.inner
                .reconnection
                .start_reconnecting(on_attempt, on_exhausted);
        });
    }

    async fn check_stream_ended(&self) -> bool {
        let callback = self.st().on_check_stream_ended.clone();
        match callback {
            Some(cb) => cb().await,
            None => false,
        }
    }

    async fn run_reconnect_attempt(&self, attempt: u32, delay: f64, raw_url: Url) {
        tracing::info!("StreamCoordinator: 재시도 {attempt} — {delay:.1}초 대기");
        self.inner.reconnection.wait_for_backoff(delay).await;
        // 백오프는 수십 초일 수 있다 — 그 사이 세션이 정리됐으면 시도하지 않는다.
        {
            let s = self.st();
            if s.phase != StreamPhase::Reconnecting || s.stream_url.is_none() {
                tracing::info!("StreamCoordinator: 백오프 중 세션 정리 감지 — 재시도 취소");
                return;
            }
        }
        if self.check_stream_ended().await {
            self.handle_stream_ended();
            return;
        }
        // 매 시도마다 매니페스트 재갱신 → 최신 CDN 토큰. 화질을 고정했으면 첫 시도부터.
        let pinned = self.st().user_selected_variant.is_some();
        if attempt > 1 || pinned {
            self.refresh_master_manifest().await;
        }
        let target = self.reconnect_playback_url(&raw_url);
        let url = if self.st().is_proxy_active {
            self.inner.proxy.proxy_url(&target)
        } else {
            target
        };
        self.perform_reconnect_attempt(&url).await;
    }

    /// 재연결에 실을 URL — 사용자가 화질을 고정했고 현재 variant 가 그것이면 그 URL, 아니면 마스터.
    pub fn reconnect_playback_url(&self, fallback: &Url) -> Url {
        let s = self.st();
        match &s.user_selected_variant {
            Some(pinned) if s.current_variant_url.as_ref() == Some(&pinned.uri) => {
                pinned.uri.clone()
            }
            _ => fallback.clone(),
        }
    }

    /// 단일 재연결 시도 — stop → play. 성공 판정은 엔진의 실제 `.playing` 보고가 소유한다.
    pub async fn perform_reconnect_attempt(&self, url: &Url) {
        let Some(engine) = self.engine() else { return };
        // 직전의 네트워크 await 들 사이에 stopStream 이 완주했을 수 있다 — 세션이 살아 있는가만 본다.
        {
            let s = self.st();
            if s.user_paused
                || engine.is_suspend_intent_active()
                || s.stream_url.is_none()
                || matches!(s.phase, StreamPhase::Idle | StreamPhase::StreamEnded)
            {
                tracing::info!("StreamCoordinator: 재연결 시도 직전 세션 정리 감지 — 시도 취소");
                return;
            }
        }
        self.end_time_shift_after_live_reload("재연결");
        engine.stop();
        match engine.play(url).await {
            Ok(()) => {
                if self.st().user_paused {
                    engine.pause();
                    return;
                }
                self.reconcile_network_activity().await;
                tracing::info!(
                    "StreamCoordinator: 재연결 재생 요청 수락 — 실제 첫 프레임을 최대 8초 대기"
                );
                // 실제 첫 프레임(report_engine_playing)이 핸들러를 끝낼 시간을 준다.
                let deadline = tokio::time::Instant::now() + RECONNECT_FIRST_FRAME_WAIT;
                loop {
                    {
                        let s = self.st();
                        if s.phase != StreamPhase::Reconnecting || s.stream_url.is_none() {
                            break;
                        }
                    }
                    if tokio::time::Instant::now() >= deadline {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                // 대기 중 세션이 정리됐으면 되살린 엔진도 함께 세운다(좀비 재생 방지).
                let (ended, paused, phase) = {
                    let s = self.st();
                    (
                        s.stream_url.is_none()
                            || matches!(s.phase, StreamPhase::Idle | StreamPhase::StreamEnded),
                        s.user_paused,
                        s.phase.clone(),
                    )
                };
                if ended {
                    engine.stop();
                    tracing::info!(
                        "StreamCoordinator: 재연결 대기 중 세션 정리 — 되살린 엔진 정지"
                    );
                    return;
                }
                if paused {
                    return;
                }
                if phase != StreamPhase::Playing {
                    self.update_phase(StreamPhase::Reconnecting);
                }
            }
            Err(e) => tracing::warn!("StreamCoordinator: 재연결 시도 실패 — {e}"),
        }
    }

    /// 재연결 시도 모두 소진 — 오류 카드 뒤에서 hls.js 가 재생을 이어가지 않게 엔진을 세운다.
    pub fn handle_reconnect_exhausted(&self) {
        {
            let s = self.st();
            if s.user_paused || s.stream_url.is_none() || s.phase != StreamPhase::Reconnecting {
                return;
            }
        }
        if let Some(e) = self.engine() {
            e.stop();
        }
        self.invalidate_low_latency_control();
        let message = "재연결 최대 횟수 초과".to_string();
        self.update_phase(StreamPhase::Error(message.clone()));
        self.emit_event(StreamEvent::Error(message));
        tracing::error!("StreamCoordinator: 재연결 소진");
    }

    /// 방송 종료 감지 — 재연결 중단, 엔진 정지, `StreamEnded` 방출.
    ///
    /// 재연결 핸들러 태스크 안에서도 불린다. `cancel()` 이 자기 태스크를 abort 하므로 이 함수는
    /// `await` 없이 끝까지 실행돼야 한다.
    pub fn handle_stream_ended(&self) {
        self.invalidate_low_latency_control();
        self.inner.reconnection.cancel();
        if let Some(e) = self.engine() {
            e.stop();
        }
        if let Some(c) = &self.inner.low_latency {
            c.stop_sync(true);
        }
        self.update_phase(StreamPhase::StreamEnded);
        self.emit_event(StreamEvent::StreamEnded);
        tracing::info!("StreamCoordinator: 방송 종료 감지 — 재연결 중단");
    }

    /// 저지연 PID 를 멈춘다 — 재연결 전 stale 데이터 오염 방지.
    pub(crate) fn stop_latency_subsystems(&self) {
        self.invalidate_low_latency_control();
        if let Some(c) = &self.inner.low_latency {
            c.stop_sync(true);
        }
    }
}
