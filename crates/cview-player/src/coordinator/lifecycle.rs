//! `StreamCoordinator+Lifecycle.swift` · `+QualityABR.swift` · `StreamCoordinator.swift`(엔진 주입·
//! 상태 보고) 포팅 — 시작/종료/일시정지/화질 전환과 엔진 phase 보고.

use std::time::{Duration, Instant};

use chrono::Utc;
use cview_core::models::player::PlayerPhase;
use url::Url;

use super::{
    select_1080p60_variant, SharedEngine, StreamCoordinator, StreamCoordinatorError, StreamEvent,
    StreamPhase, StreamQualityInfo, BACKGROUND_RECOVERY_MANIFEST_TTL,
};
use crate::low_latency::LowLatencyConfiguration;
use crate::manifest::parser::Variant;
use crate::manifest::MasterPlaylist;

impl StreamCoordinator {
    // MARK: - 엔진

    /// 엔진 주입. 이전 엔진의 PID 소유권·주입 목표를 반납하고, 되감기 게이트와 역할별 목표를 새 엔진에 옮긴다.
    pub async fn set_player_engine(&self, engine: SharedEngine) {
        let previous = self.engine();
        let reset_token = self.invalidate_low_latency_control();
        let replacement_generation = self.st().low_latency_sync_generation;
        if let Some(c) = &self.inner.low_latency {
            c.stop_sync(true);
        }
        // 세대 검사는 이전 엔진 정리에만 의미가 있다. 설치는 무조건 한다.
        if replacement_generation == self.st().low_latency_sync_generation {
            if let (Some(prev), Some(token)) = (&previous, reset_token) {
                if let Some(h) = prev.hls() {
                    h.set_latency_owned_by_controller(false, token);
                    h.set_controller_target_latency(0.0, token);
                }
            }
        }
        let time_shift_active = {
            let mut s = self.st();
            s.engine = Some(engine.clone());
            s.time_shift_active
        };
        if let Some(h) = engine.hls() {
            h.set_live_latency_correction_enabled(!time_shift_active);
            self.apply_idle_latency_target(h);
        }
    }

    /// 런타임 지연 설정 변경 — 컨트롤러를 교체하지 않고 설정만 바꾼다.
    pub fn update_low_latency_config(&self, config: LowLatencyConfiguration) {
        match &self.inner.low_latency {
            Some(c) => c.update_configuration(config),
            None => tracing::info!("LowLatencyController 미활성 — 설정 업데이트 스킵"),
        }
    }

    /// 실제 엔진 상태 변화(버퍼링·재생·일시정지)를 저지연 컨트롤러에 전달한다.
    pub fn handle_playback_engine_state(&self, phase: &PlayerPhase) {
        if self.st().user_paused {
            return;
        }
        let Some(c) = &self.inner.low_latency else {
            if matches!(phase, PlayerPhase::Paused) {
                self.invalidate_low_latency_control();
            }
            return;
        };
        match phase {
            PlayerPhase::Playing => c.resume_from_buffering(),
            PlayerPhase::Buffering { .. } => c.pause_for_buffering(true),
            PlayerPhase::Paused => {
                self.invalidate_low_latency_control();
                c.stop_sync(true);
            }
            _ => {}
        }
    }

    /// 엔진이 실제로 `.playing` 에 들어갔다 — phase 의 단일 권위 경로. 재연결 성공 판정도 여기서 한다.
    pub async fn report_engine_playing(&self) {
        let phase = {
            let s = self.st();
            if s.user_paused {
                return;
            }
            s.phase.clone()
        };
        if phase == StreamPhase::Playing {
            self.apply_pending_proxy_mode().await;
            return;
        }
        let (starts_pipeline, resumes_manual_pause) = match phase {
            StreamPhase::LoadingInfo
            | StreamPhase::LoadingManifest
            | StreamPhase::Connecting
            | StreamPhase::Reconnecting => (true, false),
            StreamPhase::Buffering => (false, false),
            StreamPhase::Paused => (false, true),
            // 늦은 첫 프레임 신호가 stop/error/방송종료 상태를 되살리지 못한다.
            _ => return,
        };
        self.update_phase(StreamPhase::Playing);
        // `play(url:)` 반환은 접수일 뿐 — 실제 첫 프레임만이 재연결 성공 신호다.
        if starts_pipeline {
            self.inner.reconnection.handle_success();
        }
        self.apply_pending_proxy_mode().await;
        if self.phase() != StreamPhase::Playing {
            return;
        }
        if (starts_pipeline || resumes_manual_pause) && self.should_run_low_latency_sync() {
            let weak = self.weak();
            tokio::spawn(async move {
                if let Some(this) = StreamCoordinator::from_weak(&weak) {
                    this.start_low_latency_sync().await;
                }
            });
        }
    }

    // MARK: - 수명

    /// 스트림 재생 시작. PID 싱크는 엔진의 실제 `.playing` 보고 뒤에 시작한다.
    pub async fn start_stream(&self, url: Url) -> Result<(), StreamCoordinatorError> {
        let generation = {
            let mut s = self.st();
            s.proxy_route_generation = s.proxy_route_generation.wrapping_add(1);
            s.user_paused = false;
            s.stream_url = Some(url.clone());
            s.start_time = Some(Instant::now());
            s.proxy_route_generation
        };
        self.update_phase(StreamPhase::Connecting);
        let owns = |this: &Self| {
            let s = this.st();
            s.proxy_route_generation == generation && s.stream_url.is_some()
        };

        let result: Result<(), StreamCoordinatorError> = async {
            let playback_url = self.configure_proxy_route(&url).await?;
            let Some(engine) = self.engine() else {
                self.update_phase(StreamPhase::Error("Player engine unavailable".into()));
                return Ok(());
            };
            engine.play(&playback_url).await?;
            if !owns(self) {
                return Err(StreamCoordinatorError::Cancelled);
            }
            // hls.js `play(url)` 은 `isLoadingSuspended` 를 false 로 되돌린다 — 다시 파생시킨다.
            self.reconcile_network_activity().await;
            if self.st().master_playlist.is_none() {
                self.load_manifest_info(url.clone());
            }
            tracing::info!("Stream started");
            Ok(())
        }
        .await;

        match result {
            Err(StreamCoordinatorError::Cancelled) => Err(StreamCoordinatorError::Cancelled),
            Err(e) => {
                if !owns(self) {
                    return Err(StreamCoordinatorError::Cancelled);
                }
                self.update_phase(StreamPhase::Error(e.to_string()));
                Err(e)
            }
            Ok(()) => Ok(()),
        }
    }

    /// 스트림 정지 — 프록시(기록 포함)·재연결·저지연을 모두 거둔다.
    pub async fn stop_stream(&self) {
        {
            let mut s = self.st();
            s.proxy_route_generation = s.proxy_route_generation.wrapping_add(1);
            s.stream_url = None;
            s.is_proxy_active = false;
            s.user_paused = false;
        }
        self.inner.proxy.stop();
        self.invalidate_low_latency_control();
        self.inner.reconnection.cancel();
        self.unsubscribe_network_monitor();
        if let Some(e) = self.engine() {
            e.stop();
        }
        if let Some(c) = &self.inner.low_latency {
            c.stop_sync(true);
        }
        {
            let mut s = self.st();
            s.local_timeshift_playing = false;
            s.master_playlist = None;
            s.current_quality = None;
            s.start_time = None;
            s.user_selected_variant = None;
            s.current_variant_url = None;
            s.last_reconnect_time = None;
        }
        self.update_phase(StreamPhase::Idle);
        self.emit_event(StreamEvent::Stopped);
        tracing::info!("Stream stopped");
    }

    /// 일시정지 — 재버퍼 신호가 아니므로 저지연 루프 자체를 끝낸다.
    pub async fn pause(&self) {
        self.st().user_paused = true;
        self.update_phase(StreamPhase::Paused);
        if let Some(e) = self.engine() {
            e.pause();
        }
        self.invalidate_low_latency_control();
        self.inner.reconnection.cancel();
        if let Some(c) = &self.inner.low_latency {
            c.stop_sync(true);
        }
    }

    /// 재개 — 실제 `.playing` 전환은 엔진 콜백([`Self::report_engine_playing`])에서만 확정한다.
    pub async fn resume(&self) {
        self.st().user_paused = false;
        if let Some(e) = self.engine() {
            e.resume();
        }
    }

    /// 백그라운드 복귀 시 재생 상태 점검 및 복구. 의도적으로 정지된 패인은 건드리지 않는다.
    pub fn recover_from_background(&self) {
        let (phase, engine, lease) = {
            let s = self.st();
            (
                s.phase.clone(),
                s.engine.clone(),
                s.current_manifest_lease.clone(),
            )
        };
        if !phase.is_playing_or_buffering() {
            return;
        }
        let Some(engine) = engine else { return };
        if engine.is_suspend_intent_active() {
            tracing::debug!("recoverFromBackground: 정지 의도 활성 — 복구 판정 생략");
            return;
        }
        let weak = self.weak();
        if engine.is_in_error_state() {
            self.trigger_reconnect("background recovery: engine in error state", false);
        } else if !engine.is_playing() {
            engine.resume();
            tokio::spawn(async move {
                if let Some(this) = StreamCoordinator::from_weak(&weak) {
                    this.refresh_master_manifest().await;
                }
                tokio::time::sleep(Duration::from_secs(3)).await;
                let Some(this) = StreamCoordinator::from_weak(&weak) else {
                    return;
                };
                if !engine.is_playing() && !engine.is_in_error_state() {
                    this.trigger_reconnect("background recovery: resume failed after 3s", false);
                }
            });
        } else {
            if let Some(lease) = lease {
                let age = (Utc::now() - lease.fetched_at).num_milliseconds() as f64 / 1000.0;
                if age < BACKGROUND_RECOVERY_MANIFEST_TTL {
                    tracing::debug!(
                        "recoverFromBackground: manifest fresh (lease gen={}) — refresh skipped",
                        lease.generation
                    );
                    return;
                }
            }
            tokio::spawn(async move {
                if let Some(this) = StreamCoordinator::from_weak(&weak) {
                    this.refresh_master_manifest().await;
                }
            });
        }
    }

    // MARK: - 화질

    /// 수동 화질 전환. 같은 variant 재요청이면 재로드를 생략한다.
    pub async fn switch_quality(&self, variant: &Variant) -> Result<(), StreamCoordinatorError> {
        let Some(engine) = self.engine() else {
            return Ok(());
        };
        let play_url = {
            let mut s = self.st();
            if s.current_variant_url.as_ref() == Some(&variant.uri) {
                tracing::debug!(
                    "switchQuality: 동일 variant({}) — 재로드 생략",
                    variant.quality_label()
                );
                return Ok(());
            }
            s.current_variant_url = Some(variant.uri.clone());
            if s.is_proxy_active {
                self.inner.proxy.proxy_url(&variant.uri)
            } else {
                variant.uri.clone()
            }
        };
        // 화질 전환은 라이브 엣지에서 다시 시작한다 — 되감기 상태를 재로드 전에 거둔다.
        self.end_time_shift_after_live_reload("화질 전환");
        engine.play(&play_url).await?;
        self.reconcile_network_activity().await;
        let quality = StreamQualityInfo::from_variant(variant);
        self.st().current_quality = Some(quality.clone());
        self.emit_event(StreamEvent::QualityChanged(quality));
        Ok(())
    }

    /// bandwidth 로 variant 를 찾아 전환하고 그 화질로 잠근다(토큰 갱신·재연결이 덮지 못한다).
    pub async fn switch_quality_by_bandwidth(
        &self,
        bandwidth: i64,
    ) -> Result<(), StreamCoordinatorError> {
        let variant = {
            let mut s = self.st();
            let variant = s
                .master_playlist
                .as_ref()
                .and_then(|m| m.variants.iter().find(|v| v.bandwidth == bandwidth))
                .cloned()
                .ok_or(StreamCoordinatorError::QualityNotFound)?;
            s.user_selected_variant = Some(variant.clone());
            if s.current_quality
                .as_ref()
                .is_some_and(|q| q.bandwidth == bandwidth)
            {
                tracing::info!(
                    "수동 화질 선택: 동일 화질({}) — reload 생략",
                    variant.quality_label()
                );
                return Ok(());
            }
            variant
        };
        self.switch_quality(&variant).await
    }

    /// 초기 화질: 원본 선호면 1080p60, 아니면 사용자 선호 라벨, 아니면 중간.
    ///
    /// # Panics
    /// variant 가 하나도 없으면(파서는 빈 마스터를 만들지 않는다).
    pub fn select_initial_quality(&self, master: &MasterPlaylist) -> Variant {
        let config = &self.inner.config;
        if config.effective_quality_policy().prefers_source_quality() {
            if let Some(v) = select_1080p60_variant(&master.variants) {
                return v.clone();
            }
        }
        if let Some(preferred) = config.preferred_quality {
            if let Some(v) = master
                .variants
                .iter()
                .find(|v| v.quality_label() == preferred.display_name())
            {
                return v.clone();
            }
        }
        master.variants[master.variants.len() / 2].clone()
    }
}
