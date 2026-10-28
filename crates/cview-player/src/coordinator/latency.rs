//! `StreamCoordinator+LowLatency.swift` · `+MatchWebV2.swift` · `+RoleLatency.swift` 포팅 —
//! 저지연 PID 싱크 배선, 엔진 자체 정지 재정합, 멀티라이브 선택 싱크, matchWeb v2 백오프 루프.

use std::sync::{Arc, Weak};

use futures_util::future::BoxFuture;

use super::{
    same_engine, CoordinatedEngine, HlsEngineControl, Inner, LatencyInfo, LowLatencyActivation,
    SharedEngine, StreamCoordinator, StreamEvent, MAX_TARGET_LATENCY_MARGIN_SECONDS,
    SECONDARY_PANE_STABILITY_TARGET_SECONDS,
};
use crate::low_latency::latency_provider;
use crate::low_latency::{LowLatencyBindings, LowLatencyConfiguration, LowLatencyController};
use crate::low_latency::{MatchWebV2DisplaySample, SampleProvider};
use crate::policy::latency_snap;
use crate::policy::LatencyPresetPairing;

/// 콜백이 쥐는 약한 참조 쌍.
#[derive(Clone)]
struct Binding {
    coordinator: Weak<Inner>,
    engine: Weak<dyn CoordinatedEngine>,
    generation: u64,
}

impl Binding {
    /// 세대·엔진·게이트·phase 가 여전히 현재면 (코디네이터, 엔진).
    fn current(&self) -> Option<(StreamCoordinator, SharedEngine)> {
        let coordinator = StreamCoordinator::from_weak(&self.coordinator)?;
        let engine = self.engine.upgrade()?;
        coordinator
            .low_latency_command_is_current(self.generation, &engine)
            .then_some((coordinator, engine))
    }
}

impl StreamCoordinator {
    /// 현재 엔진이 matchWeb v2 세션을 돌리는가.
    pub(crate) fn match_web_v2_engine_active(&self) -> bool {
        self.engine()
            .as_ref()
            .and_then(|e| e.hls().map(|h| h.is_match_web_v2_active()))
            .unwrap_or(false)
    }

    /// 저지연 sync 루프를 구동해야 하는지 — automatic 은 설정 그대로, managed 는 외부 활성 요청이 있을 때만.
    pub fn should_run_low_latency_sync(&self) -> bool {
        let (time_shift, managed) = {
            let s = self.st();
            (s.time_shift_active, s.managed_sync_active)
        };
        if !self.inner.config.enable_low_latency || self.inner.low_latency.is_none() || time_shift {
            return false;
        }
        match self.inner.config.low_latency_activation {
            LowLatencyActivation::Automatic => true,
            LowLatencyActivation::Managed => managed || self.match_web_v2_engine_active(),
        }
    }

    /// 비선택 패인이면 안정성 목표, 아니면 0(주입 없음). 단일 라이브·저지연 꺼짐·v2 는 대상이 아니다.
    pub fn idle_latency_target_seconds(&self) -> f64 {
        let config = &self.inner.config;
        let managed_active = self.st().managed_sync_active;
        if config.low_latency_activation != LowLatencyActivation::Managed
            || !config.enable_low_latency
            || self.inner.low_latency.is_none()
            || managed_active
            || self.match_web_v2_engine_active()
        {
            return 0.0;
        }
        SECONDARY_PANE_STABILITY_TARGET_SECONDS
    }

    /// 목표 지연 여유(초) — 멀티라이브 메인 패인이 다른 패인과 대역폭을 나눠 쓸 때 기본 목표(웹과 같은 3초)에
    /// 더한다. 밴드(목표~seek 문턱) 폭은 그대로 두고 함께 민다. 0 이면 기본 설정으로 되돌린다.
    /// 늦추기 문턱(slow_down_threshold, 웹 프리셋 0.55초)도 여유×0.25 로 좁힌다 — 그대로 두면 지금 위치가
    /// "목표 − 문턱" 안이라 컨트롤러가 아무것도 하지 않아 여유가 실제 전방 버퍼로 바뀌지 않는다(실측: 목표 4.0초에
    /// 지연 3.6초가 2분 동안 그대로). 좁히면 0.97배속으로 몇 초 늦춰 목표 가까이로 물러난다.
    /// 컨트롤러가 없거나(저지연 끔) v2 가 지연을 소유하면 아무것도 하지 않는다. 같은 값이면 PID 를 새로 만들지 않는다.
    pub fn set_target_latency_margin(&self, margin: f64) {
        let Some(controller) = &self.inner.low_latency else {
            return;
        };
        if self.match_web_v2_engine_active() {
            return;
        }
        let margin = if margin.is_finite() {
            margin.clamp(0.0, MAX_TARGET_LATENCY_MARGIN_SECONDS)
        } else {
            0.0
        };
        let mut config = self.inner.config.low_latency_config;
        config.target_latency += margin;
        config.max_latency += margin;
        if margin > 0.0 {
            config.slow_down_threshold = config.slow_down_threshold.min(margin * 0.25);
        }
        if (controller.current_configuration().target_latency - config.target_latency).abs() < 0.01
        {
            return;
        }
        tracing::info!(
            "LowLatency: 목표 지연 여유 {margin:.1}s — 기본 {:.1}s → {:.1}s",
            self.inner.config.low_latency_config.target_latency,
            config.target_latency
        );
        controller.update_configuration(config);
    }

    /// 새로 설치된 엔진이 비선택 패인이면 재생 시작 **전에** 안정성 목표를 심는다.
    pub(crate) fn apply_idle_latency_target(&self, hls: &dyn HlsEngineControl) {
        let target = self.idle_latency_target_seconds();
        if target <= 0.0 {
            return;
        }
        let token = hls.invalidate_latency_control_session();
        hls.set_controller_target_latency(target, token);
        tracing::info!(
            "LowLatency[{}]: 비선택 패인 안정성 목표 {target:.1}s 주입",
            self.inner
                .config
                .channel_id
                .chars()
                .take(6)
                .collect::<String>()
        );
    }

    /// PID 소유 세대를 무효화하고 같은 토큰으로 1.0x 복원을 건다.
    pub(crate) fn invalidate_low_latency_control(&self) -> Option<u64> {
        let engine = {
            let mut s = self.st();
            s.low_latency_sync_generation = s.low_latency_sync_generation.wrapping_add(1);
            s.active_hls_latency_control_generation = None;
            s.engine.clone()
        };
        let engine = engine?;
        let hls = engine.hls()?;
        let token = hls.invalidate_latency_control_session();
        hls.set_rate_for_latency(1.0, token);
        Some(token)
    }

    /// 세대·엔진·게이트·phase 가 현재인가.
    pub(crate) fn low_latency_command_is_current(
        &self,
        generation: u64,
        engine: &SharedEngine,
    ) -> bool {
        {
            let s = self.st();
            if generation != s.low_latency_sync_generation
                || !s.engine.as_ref().is_some_and(|e| same_engine(e, engine))
                || s.network_activity_suspended
                || s.time_shift_active
                || !s.phase.is_playing_or_buffering()
            {
                return false;
            }
        }
        self.should_run_low_latency_sync()
    }

    // MARK: - 네트워크 활동 재정합

    /// 엔진 자체 정지(hls.js `suspendLoading`) 구간의 폴러 게이트. 원하는 상태를 인자로 받지 않고
    /// 매번 엔진에서 다시 읽는다 — 빠른 정지↔재개 토글이 어떤 순서로 실행돼도 최신 값으로 수렴한다.
    pub async fn reconcile_network_activity(&self) {
        let generation = {
            let mut s = self.st();
            s.network_activity_generation = s.network_activity_generation.wrapping_add(1);
            s.network_activity_generation
        };
        // 최대 2패스: await 중 상태가 한 번 뒤집히면 다시 수렴시킨다.
        for _ in 0..2 {
            let (engine, flag, active) = {
                let s = self.st();
                if generation != s.network_activity_generation {
                    return;
                }
                (
                    s.engine.clone(),
                    s.network_activity_suspended,
                    s.active_hls_latency_control_generation,
                )
            };
            let Some(engine) = engine else { return };
            let Some(hls) = engine.hls() else { return };
            let suspended = hls.is_loading_suspended();
            if flag == suspended {
                // 빠른 suspend→resume 에서 플래그만 보면 폐기된 PID 토큰이 영구히 남는다.
                let token_current =
                    active.is_some_and(|t| hls.is_latency_control_session_current(t));
                if !suspended && self.should_run_low_latency_sync() && !token_current {
                    self.start_low_latency_sync().await;
                }
                return;
            }
            self.st().network_activity_suspended = suspended;
            self.sync_timeshift_fetch_policy();
            if suspended {
                self.invalidate_low_latency_control();
                if let Some(c) = &self.inner.low_latency {
                    c.stop_sync(true);
                }
                if generation != self.st().network_activity_generation {
                    return;
                }
                tracing::info!("네트워크 폴러 정지(엔진 자체 정지)");
            } else {
                if generation != self.st().network_activity_generation {
                    return;
                }
                if self.should_run_low_latency_sync() && self.phase().is_playing_or_buffering() {
                    self.start_low_latency_sync().await;
                }
                tracing::info!("네트워크 폴러 재개");
            }
        }
    }

    // MARK: - 멀티라이브 선택 싱크

    /// 선택 세션 정밀 싱크 토글 — `Managed` 전용. 비활성화하면 소유권을 반납하고 비선택 안정성 목표를 넣는다.
    pub async fn set_low_latency_sync_active(&self, active: bool) {
        if self.inner.config.low_latency_activation != LowLatencyActivation::Managed {
            return;
        }
        let Some(controller) = &self.inner.low_latency else {
            return;
        };
        if self.st().managed_sync_active == active {
            return;
        }
        if self.match_web_v2_engine_active() {
            // v2 는 백오프 루프를 유지한다.
            self.st().managed_sync_active = active;
            return;
        }
        self.st().managed_sync_active = active;
        if active {
            if self.phase().is_playing_or_buffering() {
                tracing::info!("LowLatency[P2-4]: 선택 세션 정밀 싱크 시작");
                self.start_low_latency_sync().await;
            } else {
                tracing::info!("LowLatency[P2-4]: 정밀 싱크 예약 — 재생 시작 시 자동 개시");
            }
        } else {
            let reset = self.invalidate_low_latency_control();
            controller.stop_sync(true);
            if let (Some(engine), Some(token)) = (self.engine(), reset) {
                if let Some(h) = engine.hls() {
                    h.set_latency_owned_by_controller(false, token);
                    h.set_controller_target_latency(self.idle_latency_target_seconds(), token);
                }
            }
            tracing::info!("LowLatency[P2-4]: 비선택 전환 — 정밀 싱크 중지, rate 1.0 복원");
        }
    }

    /// 타임머신/되감기 동안 자동 라이브 캐치업(PID·JS 자기보정)을 중지/복원한다.
    pub async fn set_time_shift_active(&self, active: bool) {
        {
            let mut s = self.st();
            if s.time_shift_active == active {
                return;
            }
            s.time_shift_active = active;
        }
        if let Some(e) = self.engine() {
            if let Some(h) = e.hls() {
                h.set_live_latency_correction_enabled(!active);
            }
        }
        if active {
            self.invalidate_low_latency_control();
            if let Some(c) = &self.inner.low_latency {
                c.stop_sync(true);
            }
            tracing::info!("LowLatency: 타임머신 재생 — 자동 라이브 보정 일시 중지");
            return;
        }
        self.apply_pending_proxy_mode().await;
        if self.should_run_low_latency_sync() && self.phase().is_playing_or_buffering() {
            self.start_low_latency_sync().await;
            tracing::info!("LowLatency: 실시간 복귀 — 자동 라이브 보정 재개");
        }
    }

    // MARK: - PID 싱크

    /// PID 싱크 시작 — 재생 중이고 엔진이 정지하지 않았을 때만.
    pub(crate) async fn start_low_latency_sync(&self) {
        if !self.should_run_low_latency_sync() || !self.phase().is_playing_or_buffering() {
            return;
        }
        if self.st().network_activity_suspended {
            return;
        }
        let Some(controller) = self.inner.low_latency.clone() else {
            return;
        };
        let Some(engine) = self.engine() else { return };
        let Some(hls) = engine.hls() else { return };
        self.invalidate_low_latency_control();
        let generation = self.st().low_latency_sync_generation;
        let lcg = hls.begin_latency_control_session();
        self.st().active_hls_latency_control_generation = Some(lcg);
        // 이전 세션의 1.03~1.05x 가 실제 playbackRate 에 남지 않게 새 토큰으로 1.0x 를 먼저 건다.
        if !hls.set_rate_awaiting_application(1.0, lcg).await
            || !self.low_latency_command_is_current(generation, &engine)
        {
            return;
        }
        if self
            .start_match_web_v2_sync_if_needed(&controller, &engine, generation, lcg)
            .await
        {
            return;
        }
        let binding = Binding {
            coordinator: self.weak(),
            engine: Arc::downgrade(&engine),
            generation,
        };
        let bindings = LowLatencyBindings {
            owner_generation: generation,
            debug_tag: self.inner.config.channel_id.chars().take(6).collect(),
            on_rate_change: {
                let b = binding.clone();
                Arc::new(move |rate| {
                    if let Some((_, e)) = b.current() {
                        if let Some(h) = e.hls() {
                            h.set_rate_for_latency(rate as f32, lcg);
                        }
                    }
                })
            },
            on_seek_required: {
                let b = binding.clone();
                Arc::new(move |target| {
                    if let Some((_, e)) = b.current() {
                        if let Some(h) = e.hls() {
                            h.seek_to_latency(target, lcg);
                        }
                    }
                })
            },
            on_latency_measured: measured_callback(binding.clone()),
            buffer_health_provider: buffer_health_provider(binding.clone()),
            on_target_latency_changed: {
                let b = binding.clone();
                Arc::new(move |target| {
                    if let Some((_, e)) = b.current() {
                        if let Some(h) = e.hls() {
                            h.set_controller_target_latency(target, lcg);
                        }
                    }
                })
            },
            // 실측 세그먼트 길이를 목표 지연의 하한으로 준다.
            segment_duration_provider: Some({
                let e = Arc::downgrade(&engine);
                Arc::new(move || {
                    e.upgrade()
                        .and_then(|e| e.hls().map(|h| h.measured_segment_duration()))
                        .unwrap_or(0.0)
                })
            }),
        };
        if !controller.install_bindings(bindings)
            || !self.low_latency_command_is_current(generation, &engine)
        {
            return;
        }
        // 가려진/비신선 스냅샷 구간에서는 seek 만 억제한다.
        let e = Arc::downgrade(&engine);
        controller.set_seek_authority_provider(move || {
            e.upgrade()
                .and_then(|e| e.hls().map(|h| h.latency_sample_is_actionable()))
                .unwrap_or(false)
        });
        self.start_hlsjs_low_latency_sync(&controller, &engine, generation, lcg, binding);
    }

    fn start_hlsjs_low_latency_sync(
        &self,
        controller: &LowLatencyController,
        engine: &SharedEngine,
        generation: u64,
        lcg: u64,
        binding: Binding,
    ) {
        let Some(hls) = engine.hls() else { return };
        if !self.low_latency_command_is_current(generation, engine) {
            return;
        }
        // 재버퍼마다 목표를 올리고 무사고 재생마다 되돌리는 적응형 백오프.
        controller.set_latency_backoff_enabled(true);
        if !self.low_latency_command_is_current(generation, engine) {
            return;
        }
        // Swift PID 가 이 세션의 지연 보정을 소유 — JS 자기보정의 discrete seek 를 끈다.
        hls.set_latency_owned_by_controller(true, lcg);
        let target = controller.current_effective_target_latency();
        if !self.low_latency_command_is_current(generation, engine) {
            return;
        }
        // 측정 전·JS 허용 폭 안에서는 스냅하지 않는다.
        if latency_snap::should_snap_on_sync_start(
            hls.live_latency_seconds(),
            hls.measured_segment_duration(),
            target,
        ) {
            tracing::info!(
                "HLS.js initial seek-to-target: latency={:.2}s target={target:.2}s",
                hls.live_latency_seconds()
            );
            hls.seek_to_latency(target, lcg);
        }
        if !self.low_latency_command_is_current(generation, engine) {
            tracing::info!("HLS.js 정밀 싱크 시작 취소 — 준비 중 패인이 정지됨");
            return;
        }
        controller.start_sync(
            Some(generation),
            latency_provider(move || {
                let (this, _) = binding.current()?;
                this.hls_live_latency()
            }),
        );
    }

    /// hls.js `latency` — PID 입력. 0 이하·120초 이상은 버린다.
    pub(crate) fn hls_live_latency(&self) -> Option<f64> {
        let engine = self.engine()?;
        let hls = engine.hls()?;
        let lat = hls.live_latency_seconds();
        if !(engine.is_playing() || lat > 0.0) {
            return None;
        }
        (lat > 0.0 && lat < 120.0).then_some(lat)
    }

    // MARK: - matchWeb v2

    /// true = v2 배선을 맡았다. false = v1 경로 진행.
    async fn start_match_web_v2_sync_if_needed(
        &self,
        controller: &LowLatencyController,
        engine: &SharedEngine,
        generation: u64,
        lcg: u64,
    ) -> bool {
        let Some(hls) = engine.hls() else {
            return false;
        };
        if !hls.is_match_web_v2_active() {
            // 폴백된 v2 세션이 v2 설정(목표 2.0·게인 0)으로 PID 를 켜지 않게 한다.
            if hls.match_web_v2_config().is_some() {
                controller.update_configuration(
                    LatencyPresetPairing::match_web_v2_fallback_configuration(),
                );
            }
            return false;
        }
        self.start_hlsjs_match_web_v2_sync(controller, engine, generation, lcg);
        true
    }

    /// v2 배선: 배속·시크 콜백은 no-op, 시작 위치 보정 없음, 백오프 전용 루프.
    fn start_hlsjs_match_web_v2_sync(
        &self,
        controller: &LowLatencyController,
        engine: &SharedEngine,
        generation: u64,
        lcg: u64,
    ) {
        let Some(hls) = engine.hls() else { return };
        controller.update_configuration(LowLatencyConfiguration::MATCH_WEB_V2_HLSJS);
        let Some(cfg) = hls.match_web_v2_config() else {
            return;
        };
        if !self.low_latency_command_is_current(generation, engine) {
            return;
        }
        let offset = cfg.ingest_target_seconds - cfg.hls_target_seconds;
        let binding = Binding {
            coordinator: self.weak(),
            engine: Arc::downgrade(engine),
            generation,
        };
        let bindings = LowLatencyBindings {
            owner_generation: generation,
            debug_tag: self.inner.config.channel_id.chars().take(6).collect(),
            on_rate_change: Arc::new(|_| {}),
            on_seek_required: Arc::new(|_| {}),
            on_latency_measured: measured_callback(binding.clone()),
            buffer_health_provider: buffer_health_provider(binding.clone()),
            on_target_latency_changed: {
                let b = binding.clone();
                Arc::new(move |target| {
                    if let Some((_, e)) = b.current() {
                        if let Some(h) = e.hls() {
                            // 백오프 δ 는 두 목표에 똑같이 더해진다.
                            h.set_match_web_v2_targets(target, target + offset, lcg);
                        }
                    }
                })
            },
            // 구조적 하한 = 파트 길이.
            segment_duration_provider: Some({
                let e = Arc::downgrade(engine);
                Arc::new(move || {
                    e.upgrade()
                        .and_then(|e| e.hls().map(|h| h.measured_part_duration()))
                        .unwrap_or(0.0)
                })
            }),
        };
        if !controller.install_bindings(bindings)
            || !self.low_latency_command_is_current(generation, engine)
        {
            return;
        }
        controller.set_latency_backoff_enabled(true);
        if !self.low_latency_command_is_current(generation, engine) {
            return;
        }
        hls.set_latency_owned_by_controller(true, lcg);
        tracing::info!("matchWeb v2 백오프 루프 시작");
        let provider: SampleProvider = Arc::new(move || {
            let sample: Option<MatchWebV2DisplaySample> = binding
                .current()
                .and_then(|(_, e)| e.hls().and_then(|h| h.match_web_v2_display_sample()));
            let fut: BoxFuture<'static, Option<MatchWebV2DisplaySample>> =
                Box::pin(async move { sample });
            fut
        });
        controller.start_backoff_only_sync(Some(generation), provider);
    }

    /// JS `fallback` → v1(현행) PID 경로로 재시작. 다른 엔진의 늦은 콜백은 무시한다.
    pub async fn handle_match_web_v2_fallback(&self, reason: &str, engine: &SharedEngine) {
        if !self.engine().is_some_and(|e| same_engine(&e, engine)) {
            return;
        }
        let token = self.invalidate_low_latency_control();
        if let Some(c) = &self.inner.low_latency {
            c.stop_sync(true);
        }
        if let (Some(token), Some(h)) = (token, engine.hls()) {
            h.set_latency_owned_by_controller(false, token);
            h.set_controller_target_latency(0.0, token);
        }
        if let Some(c) = &self.inner.low_latency {
            c.update_configuration(LatencyPresetPairing::match_web_v2_fallback_configuration());
        }
        tracing::warn!("matchWeb v2 폴백({reason}) — v1 PID 경로로 전환");
        if self.should_run_low_latency_sync() && self.phase().is_playing_or_buffering() {
            self.start_low_latency_sync().await;
        }
    }
}

fn measured_callback(b: Binding) -> crate::low_latency::MeasuredCallback {
    Arc::new(move |current, ewma, target| {
        if let Some((c, _)) = b.current() {
            c.emit_event(StreamEvent::LatencyUpdate(LatencyInfo {
                current,
                target,
                ewma,
            }));
        }
    })
}

fn buffer_health_provider(b: Binding) -> crate::low_latency::F64Provider {
    Arc::new(move || {
        let Some(engine) = b.engine.upgrade() else {
            return 1.0;
        };
        let level = engine.hls().map_or(1.0, |h| h.buffer_health());
        if let Some((c, _)) = b.current() {
            c.publish_buffer_health(level);
        }
        level
    })
}
