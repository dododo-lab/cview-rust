//! `StreamCoordinator+Timeshift.swift` 포팅 — 라이브 로컬 타임시프트(되감기) 제어.
//!
//! 레코더 구현은 [`TimeshiftBackend`] 로 받는다. 코디네이터는 앱이 넘긴 팩토리로 레코더를 만들어
//! 프록시에 붙이고, 되감기 진입(`/_ts/<sid>/index.m3u8?start=초`)·이동·라이브 복귀를 맡는다.
//! Swift 의 `retargetTimeshiftAfterAuthFailure`(레코더 토큰 갱신)는 레코더 쪽 API 라 옮기지 않았다.

use std::sync::Arc;

use super::{StreamCoordinator, StreamEvent, StreamPhase};
use crate::proxy::timeshift::TimeshiftBackend;
use crate::proxy::LocalStreamProxy;

/// 로컬 타임시프트 기록 설정.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeshiftRecordingConfig {
    /// 되감을 수 있는 최대 길이(초).
    pub max_duration: f64,
    /// 디스크 한도(바이트).
    pub budget_limit_bytes: i64,
}

/// 기록 설정과 프록시로 레코더를 만든다.
pub type TimeshiftRecorderFactory = Arc<
    dyn Fn(&TimeshiftRecordingConfig, &LocalStreamProxy) -> Arc<dyn TimeshiftBackend> + Send + Sync,
>;

impl StreamCoordinator {
    /// 레코더 팩토리 등록(없으면 기록하지 않는다).
    pub fn set_timeshift_recorder_factory(&self, factory: Option<TimeshiftRecorderFactory>) {
        self.st().timeshift_factory = factory;
    }

    /// 기록을 켜거나(설정 변경 포함) 끈다. 끄면 되감던 중이면 라이브로 돌아가고 기록을 지운다.
    pub async fn set_timeshift_recording(&self, config: Option<TimeshiftRecordingConfig>) {
        self.st().timeshift_recording_config = config;
        let Some(config) = config else {
            if self.st().local_timeshift_playing {
                self.exit_timeshift().await;
            }
            // 복귀를 기다리는 사이 다시 켜졌으면 새 설정을 따른다.
            if self.st().timeshift_recording_config.is_none() {
                self.inner.proxy.detach_timeshift();
            }
            return;
        };
        match self.inner.proxy.timeshift_backend() {
            Some(recorder) => {
                recorder.update_limits(config.max_duration, config.budget_limit_bytes)
            }
            None => self.attach_timeshift_recorder_if_needed(),
        }
    }

    /// [실험] 치지직 공식 타임머신(DVR) 스트림 — 프록시가 `__bgda__` 를 붙이고 로컬 기록은 하지 않는다.
    /// `start_stream` **전에** 부른다.
    pub async fn set_server_time_machine_stream(&self, enabled: bool) {
        self.st().is_server_time_machine_stream = enabled;
        self.inner.proxy.set_smp_segment_token_rewrite(enabled);
        if enabled {
            if self.st().local_timeshift_playing {
                self.exit_timeshift().await;
            }
            self.inner.proxy.detach_timeshift();
        }
    }

    /// 되감기가 아닌 경로(재연결·화질 전환)로 엔진이 라이브를 다시 불러왔다 — 게이트를 거두고 UI 에 알린다.
    /// 되감기 게이트를 실제로 거뒀으면 true.
    pub fn end_time_shift_after_live_reload(&self, reason: &str) -> bool {
        let was_active = {
            let mut s = self.st();
            let was_local = s.local_timeshift_playing;
            s.timeshift_transition = s.timeshift_transition.wrapping_add(1);
            s.local_timeshift_playing = false;
            let was_active = s.time_shift_active;
            s.time_shift_active = false;
            if was_local {
                drop(s);
                if let Some(r) = self.inner.proxy.timeshift_backend() {
                    r.clear_served_position();
                }
            }
            was_active
        };
        if !was_active {
            return false;
        }
        if let Some(e) = self.engine() {
            if let Some(h) = e.hls() {
                h.set_live_latency_correction_enabled(true);
            }
        }
        self.emit_event(StreamEvent::TimeShiftEnded);
        tracing::info!("[TIMESHIFT] 라이브 재로드({reason}) — 되감기 해제, 자동 보정 복원");
        true
    }

    /// 프록시가 떠 있고 기록 설정이 있으면 레코더를 붙인다.
    pub(crate) fn attach_timeshift_recorder_if_needed(&self) {
        let (config, factory) = {
            let s = self.st();
            if s.is_server_time_machine_stream || !s.is_proxy_active {
                return;
            }
            let (Some(c), Some(f)) = (s.timeshift_recording_config, s.timeshift_factory.clone())
            else {
                return;
            };
            (c, f)
        };
        if self.inner.proxy.timeshift_backend().is_some() {
            return;
        }
        let recorder = factory(&config, &self.inner.proxy);
        self.inner.proxy.attach_timeshift(recorder);
        self.sync_timeshift_fetch_policy();
        tracing::info!(
            "[TIMESHIFT] 기록 시작 (최대 {}초)",
            config.max_duration as i64
        );
    }

    /// 레코더의 상류 직접 요청 — 재생·버퍼링 중이고 앱이 로딩을 멈추지 않았을 때만 허용.
    pub(crate) fn sync_timeshift_fetch_policy(&self) {
        let Some(recorder) = self.inner.proxy.timeshift_backend() else {
            return;
        };
        let enabled = {
            let s = self.st();
            s.phase.is_playing_or_buffering() && !s.network_activity_suspended
        };
        recorder.set_upstream_fetch_enabled(enabled);
    }

    fn is_rewindable_phase(&self) -> bool {
        matches!(
            self.st().phase,
            StreamPhase::Playing | StreamPhase::Buffering | StreamPhase::Paused
        )
    }

    /// 지금 되감을 수 있는 기록 길이(초).
    pub fn timeshift_recorded_duration(&self) -> f64 {
        self.inner
            .proxy
            .timeshift_backend()
            .map(|r| r.recorded_duration())
            .unwrap_or(0.0)
    }

    pub fn is_local_timeshift_active(&self) -> bool {
        self.st().local_timeshift_playing
    }

    /// 기록 엣지 기준 `seconds_behind` 초 전부터 재생한다. 이미 되감는 중이면 재로드 없이 옮긴다.
    /// 진입(또는 이동)했으면 true.
    pub async fn enter_timeshift(&self, seconds_behind: f64) -> bool {
        let Some(engine) = self.engine() else {
            return false;
        };
        let Some(hls) = engine.hls() else {
            return false;
        };
        if self.st().local_timeshift_playing {
            hls.seek_timeshift(seconds_behind);
            return true;
        }
        // 방송 종료·오류·재연결 중에는 되감지 않는다.
        if !self.is_rewindable_phase()
            || !self.st().is_proxy_active
            || self.timeshift_recorded_duration() <= 0.0
        {
            return false;
        }
        let generation = {
            let mut s = self.st();
            s.timeshift_transition = s.timeshift_transition.wrapping_add(1);
            s.timeshift_transition
        };
        self.set_time_shift_active(true).await;
        let current = |this: &Self| this.st().timeshift_transition == generation;
        // 기다리는 사이 기록이 꺼졌을 수 있다 — URL 은 지금 레코더로 만든다.
        let url = match self.inner.proxy.timeshift_playlist_url(seconds_behind) {
            Some(url) if current(self) => url,
            _ => {
                if current(self) {
                    self.set_time_shift_active(false).await;
                }
                return false;
            }
        };
        self.st().local_timeshift_playing = true;
        match hls.play_timeshift(&url).await {
            Ok(()) => {
                if !current(self) {
                    return false;
                }
                // 기다리는 사이 방송이 끝났다 — 되감기 표시를 거둔다.
                if !self.is_rewindable_phase() {
                    self.st().local_timeshift_playing = false;
                    self.set_time_shift_active(false).await;
                    return false;
                }
                self.reconcile_network_activity().await;
                if self.st().user_paused {
                    engine.pause();
                }
                tracing::info!("[TIMESHIFT] 되감기 진입 ({}초 전)", seconds_behind as i64);
                true
            }
            Err(e) => {
                if !current(self) {
                    return false;
                }
                self.st().local_timeshift_playing = false;
                self.set_time_shift_active(false).await;
                tracing::warn!("[TIMESHIFT] 되감기 진입 실패: {e}");
                false
            }
        }
    }

    /// 라이브로 돌아간다 — 원래 라이브 URL(고정 화질이면 그 variant)을 다시 싣는다.
    pub async fn exit_timeshift(&self) {
        let (generation, engine, stream_url, proxy_active) = {
            let mut s = self.st();
            if !s.local_timeshift_playing {
                return;
            }
            s.timeshift_transition = s.timeshift_transition.wrapping_add(1);
            s.local_timeshift_playing = false;
            (
                s.timeshift_transition,
                s.engine.clone(),
                s.stream_url.clone(),
                s.is_proxy_active,
            )
        };
        let clear_position = || {
            if let Some(r) = self.inner.proxy.timeshift_backend() {
                r.clear_served_position();
            }
        };
        clear_position();
        let (Some(engine), Some(stream_url)) = (engine, stream_url) else {
            self.set_time_shift_active(false).await;
            return;
        };
        let target = self.reconnect_playback_url(&stream_url);
        let url = if proxy_active {
            self.inner.proxy.proxy_url(&target)
        } else {
            target
        };
        let current = |this: &Self| this.st().timeshift_transition == generation;
        match engine.play(&url).await {
            Ok(()) => {
                // 기다리는 사이 다시 되감았다 — 그 진입이 게이트의 주인이다.
                if !current(self) {
                    return;
                }
                clear_position();
                self.reconcile_network_activity().await;
                if self.st().user_paused {
                    engine.pause();
                }
            }
            Err(e) => tracing::warn!("[TIMESHIFT] 라이브 복귀 재로드 실패: {e}"),
        }
        if !current(self) {
            return;
        }
        self.set_time_shift_active(false).await;
        tracing::info!("[TIMESHIFT] 라이브 복귀");
    }
}
