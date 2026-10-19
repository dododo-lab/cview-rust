//! `Sources/CViewPlayer/LowLatencyController.swift` 포팅 — LL-HLS 저지연 컨트롤러 (PID + EWMA).
//!
//! Swift actor 를 `Arc<Mutex<State>>` 로 옮겼다. 콜백(rate·seek·측정·목표)은 락을 놓은 뒤에
//! 호출한다 — 콜백이 컨트롤러를 다시 불러도 교착하지 않는다. 공급자(버퍼 건강도·세그먼트
//! 길이·seek 권한)는 락 안에서 동기 호출되므로 컨트롤러를 다시 부르면 안 된다.
//!
//! 시간은 [`crate::clock::Clock`] 에서 읽는다(Swift `Date()`·`ContinuousClock` 둘 다).

mod backoff;
pub mod config;
mod match_web_v2;
mod sync;

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use cview_core::utils::{EwmaCalculator, PidController};
use futures_util::future::BoxFuture;
use tokio::task::JoinHandle;

use crate::clock::{self, SharedClock, DISTANT_FUTURE, DISTANT_PAST};

pub use backoff::{
    LATENCY_BACKOFF_FLOOR_RELEASE, LATENCY_BACKOFF_HOLD, LATENCY_BACKOFF_MAX, LATENCY_BACKOFF_STEP,
    SEGMENT_TARGET_MARGIN,
};
pub use config::LowLatencyConfiguration;
pub use match_web_v2::{MatchWebV2DisplaySample, SampleProvider};
pub use sync::latency_provider;

/// 동기화 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SyncState {
    Idle,
    Synced,
    CatchingUp,
    SlowingDown,
    SeekRequired,
}

/// 모니터링용 스냅샷.
#[derive(Debug, Clone, PartialEq)]
pub struct LatencySnapshot {
    pub current_latency: f64,
    pub target_latency: f64,
    pub ewma_latency: f64,
    pub playback_rate: f64,
    pub sync_state: SyncState,
    pub pid_output: f64,
    pub timestamp: f64,
    /// 최근 60초 내 재버퍼 진입 횟수.
    pub oscillation_count: usize,
    pub is_oscillation_capped: bool,
}

pub type RateCallback = Arc<dyn Fn(f64) + Send + Sync>;
pub type MeasuredCallback = Arc<dyn Fn(f64, f64, f64) + Send + Sync>;
pub type F64Provider = Arc<dyn Fn() -> f64 + Send + Sync>;
pub type BoolProvider = Arc<dyn Fn() -> bool + Send + Sync>;
/// sync 루프가 매 tick 부르는 지연 공급자.
pub type LatencyProvider = Arc<dyn Fn() -> BoxFuture<'static, Option<f64>> + Send + Sync>;

/// 한 재생 세션의 배선 묶음 (`installBindings`).
#[derive(Clone)]
pub struct LowLatencyBindings {
    pub owner_generation: u64,
    pub debug_tag: String,
    pub on_rate_change: RateCallback,
    pub on_seek_required: RateCallback,
    pub on_latency_measured: MeasuredCallback,
    pub buffer_health_provider: F64Provider,
    pub on_target_latency_changed: RateCallback,
    pub segment_duration_provider: Option<F64Provider>,
}

pub(crate) enum Effect {
    Rate(f64),
    Seek(f64),
    Measured(f64, f64, f64),
    Target(f64),
}

/// 컨트롤러 내부 상태 (Swift actor 의 저장 프로퍼티).
pub struct State {
    pub(crate) config: LowLatencyConfiguration,
    pub(crate) pid: PidController,
    pub(crate) ewma: EwmaCalculator,
    pub(crate) state: SyncState,
    pub(crate) current_rate: f64,
    pub(crate) active_binding_generation: Option<u64>,
    pub(crate) last_process_instant: Option<f64>,
    pub(crate) is_paused_for_buffering: bool,
    pub(crate) buffering_pause_started_at: Option<f64>,
    pub(crate) latency_backoff_enabled: bool,
    pub(crate) latency_backoff_seconds: f64,
    pub(crate) latency_backoff_decay_at: f64,
    pub(crate) latency_backoff_floor: f64,
    pub(crate) latency_backoff_floor_release_at: f64,
    pub(crate) seek_grace_auto_resume_at: Option<f64>,
    pub(crate) debug_tag: String,
    pub(crate) cooldown_until: f64,
    pub(crate) recent_buffering_timestamps: Vec<f64>,
    pub(crate) oscillation_max_rate: Option<f64>,
    pub(crate) oscillation_reset_time: f64,
    pub(crate) buffer_health_provider: Option<F64Provider>,
    pub(crate) segment_duration_provider: Option<F64Provider>,
    pub(crate) seek_authority_provider: Option<BoolProvider>,
    pub(crate) post_grace_sample_count: i64,
    pub(crate) post_grace_last_raw_latency: Option<f64>,
    pub(crate) initial_seek_done: bool,
    pub(crate) max_rate_override: Option<f64>,
    pub(crate) buffer_starvation_cap: Option<f64>,
    pub(crate) on_rate_change: Option<RateCallback>,
    pub(crate) on_seek_required: Option<RateCallback>,
    pub(crate) on_latency_measured: Option<MeasuredCallback>,
    pub(crate) on_target_latency_changed: Option<RateCallback>,
    pub(crate) published_target_latency: f64,
    pub(crate) outbox: Vec<Effect>,
    pub(crate) clock: SharedClock,
}

pub(crate) const MAX_BUFFERING_PAUSE: f64 = 30.0;
pub(crate) const SEEK_GRACE_AUTO_RESUME_DELAY: f64 = 8.0;
pub(crate) const COOLDOWN_DURATION: f64 = 12.0;
pub(crate) const COOLDOWN_MAX_RATE: f64 = 1.03;
const EWMA_ALPHA: f64 = 0.15;

impl State {
    fn new(config: LowLatencyConfiguration, clock: SharedClock) -> Self {
        Self {
            pid: PidController::new(config.pid_kp, config.pid_ki, config.pid_kd),
            config,
            ewma: EwmaCalculator::new(EWMA_ALPHA),
            state: SyncState::Idle,
            current_rate: 1.0,
            active_binding_generation: None,
            last_process_instant: None,
            is_paused_for_buffering: false,
            buffering_pause_started_at: None,
            latency_backoff_enabled: false,
            latency_backoff_seconds: 0.0,
            latency_backoff_decay_at: DISTANT_PAST,
            latency_backoff_floor: 0.0,
            latency_backoff_floor_release_at: DISTANT_FUTURE,
            seek_grace_auto_resume_at: None,
            debug_tag: String::new(),
            cooldown_until: DISTANT_PAST,
            recent_buffering_timestamps: Vec::new(),
            oscillation_max_rate: None,
            oscillation_reset_time: DISTANT_PAST,
            buffer_health_provider: None,
            segment_duration_provider: None,
            seek_authority_provider: None,
            post_grace_sample_count: 0,
            post_grace_last_raw_latency: None,
            initial_seek_done: false,
            max_rate_override: None,
            buffer_starvation_cap: None,
            on_rate_change: None,
            on_seek_required: None,
            on_latency_measured: None,
            on_target_latency_changed: None,
            published_target_latency: 0.0,
            outbox: Vec::new(),
            clock,
        }
    }

    pub(crate) fn now(&self) -> f64 {
        self.clock.now()
    }

    pub(crate) fn emit_rate(&mut self, rate: f64) {
        if self.on_rate_change.is_some() {
            self.outbox.push(Effect::Rate(rate));
        }
    }

    /// 현재 실효 최대 가속률 (config, 쿨다운, 진동, 외부 오버라이드 중 최소값).
    pub(crate) fn effective_max_rate(&self) -> f64 {
        let mut rate = self.config.max_playback_rate;
        if let Some(o) = self.max_rate_override {
            rate = rate.min(o);
        }
        if let Some(s) = self.buffer_starvation_cap {
            rate = rate.min(s);
        }
        if self.now() < self.cooldown_until {
            rate = rate.min(COOLDOWN_MAX_RATE);
        }
        if let Some(o) = self.oscillation_max_rate {
            rate = rate.min(o);
        }
        rate
    }

    pub(crate) fn is_seek_authorized(&self) -> bool {
        self.seek_authority_provider.as_ref().is_none_or(|p| p())
    }

    /// 실효 목표가 직전 전송값과 0.01s 이상 다를 때만 통지.
    pub(crate) fn publish_effective_target_latency_if_changed(&mut self) {
        if self.on_target_latency_changed.is_none() {
            return;
        }
        let target = self.effective_target_latency();
        if (target - self.published_target_latency).abs() < 0.01 {
            return;
        }
        self.published_target_latency = target;
        self.outbox.push(Effect::Target(target));
    }

    pub(crate) fn update_configuration(&mut self, config: LowLatencyConfiguration) {
        self.config = config;
        self.pid = PidController::new(config.pid_kp, config.pid_ki, config.pid_kd);
        self.publish_effective_target_latency_if_changed();
        tracing::info!(
            "LowLatency: configuration updated in-place (target={}s, kp={})",
            config.target_latency,
            config.pid_kp
        );
    }

    pub(crate) fn install_bindings(&mut self, b: LowLatencyBindings) -> bool {
        if let Some(active) = self.active_binding_generation {
            if b.owner_generation < active {
                return false;
            }
        }
        self.active_binding_generation = Some(b.owner_generation);
        self.debug_tag = b.debug_tag;
        self.on_rate_change = Some(b.on_rate_change);
        self.on_seek_required = Some(b.on_seek_required);
        self.on_latency_measured = Some(b.on_latency_measured);
        self.buffer_health_provider = Some(b.buffer_health_provider);
        self.on_target_latency_changed = Some(b.on_target_latency_changed);
        self.segment_duration_provider = b.segment_duration_provider;
        self.published_target_latency = 0.0;
        self.publish_effective_target_latency_if_changed();
        true
    }

    /// 버퍼링 시작 — rate 1.0 리셋, PID 리셋, 조정 일시 중지. `is_rebuffer` 가 false(수동 정지)면
    /// 백오프·진동 통계에서 제외한다.
    pub(crate) fn pause_for_buffering(&mut self, is_rebuffer: bool) {
        self.seek_grace_auto_resume_at = None;
        if is_rebuffer {
            let now = self.now();
            self.bump_latency_backoff(now);
        }
        if self.is_paused_for_buffering {
            return;
        }
        self.is_paused_for_buffering = true;
        self.buffering_pause_started_at = Some(self.now());
        self.pid.reset();
        if self.current_rate != 1.0 {
            self.current_rate = 1.0;
            self.emit_rate(1.0);
        }
        if !is_rebuffer {
            return;
        }
        let now = self.now();
        self.recent_buffering_timestamps.push(now);
        self.recent_buffering_timestamps.retain(|t| now - t <= 60.0);
        if self.recent_buffering_timestamps.len() >= 3 {
            self.oscillation_max_rate = Some(COOLDOWN_MAX_RATE);
            self.oscillation_reset_time = now + 120.0;
            tracing::warn!(
                "LowLatency: 진동 감지 — 60초 내 {}회 버퍼링, 가속 상한 1.03 적용 (120초간)",
                self.recent_buffering_timestamps.len()
            );
        }
    }

    /// 버퍼링 종료 — rate 조정 재개 (쿨다운 적용).
    pub(crate) fn resume_from_buffering(&mut self) {
        self.seek_grace_auto_resume_at = None;
        if !self.is_paused_for_buffering {
            return;
        }
        self.is_paused_for_buffering = false;
        self.buffering_pause_started_at = None;
        self.cooldown_until = self.now() + COOLDOWN_DURATION;
    }

    /// Seek 실행 후 버퍼 재구축 보호 — PID/EWMA 리셋 + 자동 pause + 쿨다운.
    pub(crate) fn enter_post_seek_grace(&mut self) {
        self.pid.reset();
        self.ewma = EwmaCalculator::new(EWMA_ALPHA);
        self.is_paused_for_buffering = true;
        let now = self.now();
        self.buffering_pause_started_at = Some(now);
        self.seek_grace_auto_resume_at = Some(now + SEEK_GRACE_AUTO_RESUME_DELAY);
        self.post_grace_sample_count = 0;
        self.post_grace_last_raw_latency = None;
        self.current_rate = 1.0;
        self.last_process_instant = None;
        self.emit_rate(1.0);
        self.cooldown_until = now + COOLDOWN_DURATION;
    }

    /// 모니터링 스냅샷.
    pub(crate) fn snapshot(&self, current_latency: f64) -> LatencySnapshot {
        let now = self.now();
        LatencySnapshot {
            current_latency,
            target_latency: self.config.target_latency,
            ewma_latency: self.ewma.current(),
            playback_rate: self.current_rate,
            sync_state: self.state,
            pid_output: self.pid.last_output(),
            timestamp: now,
            oscillation_count: self
                .recent_buffering_timestamps
                .iter()
                .filter(|t| now - **t <= 60.0)
                .count(),
            is_oscillation_capped: self.oscillation_max_rate.is_some(),
        }
    }

    pub(crate) fn reset(&mut self) {
        self.pid.reset();
        self.ewma = EwmaCalculator::new(EWMA_ALPHA);
        self.current_rate = 1.0;
        self.state = SyncState::Idle;
        self.initial_seek_done = false;
    }
}

/// 저지연 컨트롤러 핸들 (복제하면 같은 컨트롤러를 가리킨다).
#[derive(Clone)]
pub struct LowLatencyController {
    pub(crate) inner: Arc<Mutex<State>>,
    pub(crate) sync_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    pub(crate) sync_interval: Arc<Mutex<Duration>>,
}

impl std::fmt::Debug for LowLatencyController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LowLatencyController")
            .field("state", &self.sync_state())
            .finish()
    }
}

impl Default for LowLatencyController {
    fn default() -> Self {
        Self::new(LowLatencyConfiguration::STANDARD)
    }
}

pub(crate) fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}

/// 상태를 바꾸고, 락을 놓은 뒤 쌓인 콜백을 호출한다.
pub(crate) fn with_state<R>(state: &Mutex<State>, f: impl FnOnce(&mut State) -> R) -> R {
    let (result, effects, rate, seek, measured, target) = {
        let mut s = lock(state);
        let result = f(&mut s);
        let effects = std::mem::take(&mut s.outbox);
        (
            result,
            effects,
            s.on_rate_change.clone(),
            s.on_seek_required.clone(),
            s.on_latency_measured.clone(),
            s.on_target_latency_changed.clone(),
        )
    };
    for effect in effects {
        match effect {
            Effect::Rate(v) => {
                if let Some(cb) = &rate {
                    cb(v)
                }
            }
            Effect::Seek(v) => {
                if let Some(cb) = &seek {
                    cb(v)
                }
            }
            Effect::Measured(a, b, c) => {
                if let Some(cb) = &measured {
                    cb(a, b, c)
                }
            }
            Effect::Target(v) => {
                if let Some(cb) = &target {
                    cb(v)
                }
            }
        }
    }
    result
}

impl LowLatencyController {
    pub fn new(config: LowLatencyConfiguration) -> Self {
        Self::with_clock(config, clock::system())
    }

    pub fn with_clock(config: LowLatencyConfiguration, clock: SharedClock) -> Self {
        Self {
            inner: Arc::new(Mutex::new(State::new(config, clock))),
            sync_task: Arc::new(Mutex::new(None)),
            sync_interval: Arc::new(Mutex::new(Duration::from_secs(5))),
        }
    }

    pub(crate) fn with<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        with_state(&self.inner, f)
    }

    fn read<R>(&self, f: impl FnOnce(&State) -> R) -> R {
        f(&lock(&self.inner))
    }

    // MARK: 조회

    pub fn sync_state(&self) -> SyncState {
        self.read(|s| s.state)
    }

    pub fn current_rate(&self) -> f64 {
        self.read(|s| s.current_rate)
    }

    pub fn is_pid_active(&self) -> bool {
        matches!(
            self.sync_state(),
            SyncState::CatchingUp | SyncState::SlowingDown
        )
    }

    pub fn current_configuration(&self) -> LowLatencyConfiguration {
        self.read(|s| s.config)
    }

    pub fn current_target_latency(&self) -> f64 {
        self.read(|s| s.config.target_latency)
    }

    pub fn effective_max_rate(&self) -> f64 {
        self.read(|s| s.effective_max_rate())
    }

    pub fn debug_tag(&self) -> String {
        self.read(|s| s.debug_tag.clone())
    }

    /// PID sync 루프 주기(Swift `PowerAwareInterval.scaled(5.0)`). 기본 5초.
    pub fn sync_interval(&self) -> Duration {
        *self.sync_interval.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set_sync_interval(&self, interval: Duration) {
        *self.sync_interval.lock().unwrap_or_else(|e| e.into_inner()) = interval;
    }

    pub fn has_sync_task(&self) -> bool {
        self.sync_task
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    // MARK: 설정·배선

    pub fn set_debug_tag(&self, tag: impl Into<String>) {
        let tag = tag.into();
        self.with(|s| s.debug_tag = tag);
    }

    pub fn set_max_rate_override(&self, max_rate: Option<f64>) {
        self.with(|s| s.max_rate_override = max_rate);
    }

    pub fn update_configuration(&self, config: LowLatencyConfiguration) {
        self.with(|s| s.update_configuration(config));
    }

    pub fn set_on_rate_change(&self, f: impl Fn(f64) + Send + Sync + 'static) {
        self.with(|s| s.on_rate_change = Some(Arc::new(f)));
    }

    pub fn set_on_seek_required(&self, f: impl Fn(f64) + Send + Sync + 'static) {
        self.with(|s| s.on_seek_required = Some(Arc::new(f)));
    }

    pub fn set_on_latency_measured(&self, f: impl Fn(f64, f64, f64) + Send + Sync + 'static) {
        self.with(|s| s.on_latency_measured = Some(Arc::new(f)));
    }

    pub fn set_buffer_health_provider(&self, f: impl Fn() -> f64 + Send + Sync + 'static) {
        self.with(|s| s.buffer_health_provider = Some(Arc::new(f)));
    }

    pub fn set_seek_authority_provider(&self, f: impl Fn() -> bool + Send + Sync + 'static) {
        self.with(|s| s.seek_authority_provider = Some(Arc::new(f)));
    }

    /// 실효 목표 지연 소비자 등록 — 등록 즉시 현재 값을 1회 내보낸다.
    pub fn set_on_target_latency_changed(&self, f: impl Fn(f64) + Send + Sync + 'static) {
        self.with(|s| {
            s.on_target_latency_changed = Some(Arc::new(f));
            s.publish_effective_target_latency_if_changed();
        });
    }

    /// 한 재생 세션의 배선을 한 번에 교체한다. 오래된 owner 는 최신 배선을 덮을 수 없다.
    pub fn install_bindings(&self, bindings: LowLatencyBindings) -> bool {
        self.with(|s| s.install_bindings(bindings))
    }

    // MARK: 버퍼링·seek

    pub fn pause_for_buffering(&self, is_rebuffer: bool) {
        self.with(|s| s.pause_for_buffering(is_rebuffer));
    }

    pub fn resume_from_buffering(&self) {
        self.with(|s| s.resume_from_buffering());
    }

    pub fn enter_post_seek_grace(&self) {
        self.with(|s| s.enter_post_seek_grace());
    }

    pub fn snapshot(&self, current_latency: f64) -> LatencySnapshot {
        self.read(|s| s.snapshot(current_latency))
    }

    pub fn reset(&self) {
        self.with(|s| s.reset());
    }
}
