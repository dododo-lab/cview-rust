//! `Sources/CViewApp/ViewModels/PlayerViewModel.swift` (+StreamStart, +EventHandling, +Recording,
//! +Screenshot, +AudioVideoAdjust) 포팅 — 라이브 플레이어 뷰모델.
//!
//! 재생은 [`StreamCoordinator`] 가 맡고, 뷰모델은 코디네이터 이벤트를 관찰 상태로 옮긴다.
//! 엔진은 [`LivePlayerEngine`] 트레이트(hls.js 설정 면 포함)로 받는다. hls.js 엔진 콜백
//! (`onStateChange`·`onPlaybackStalled`·`onRebufferStateChanged`·`onRecordingEvent`·
//! `onSurfacePresented`)은 셸이 `engine_*` 메서드로 넘긴다.
//!
//! Swift 에서 가져오지 않은 것(앱 셸·macOS 몫):
//! - PiP 표면 게이트, `NSView` 렌더 표면, 전체화면 `NSWindow` 호출, 저장 패널, 토스트.
//! - 시청 하트비트 센터(`WatchHeartbeatCenter`) 등록 — 셸이 시작/정지에 맞춰 붙인다.
//! - 가림 해상도 캡 타이머(`OcclusionResolutionCap` 스케줄) — 정책 함수만 cview-player 에 있다.
//! - 로컬 타임시프트 레코더 — 기록 설정([`PlayerViewModel::rewind_recording_config`])은 계산해
//!   코디네이터에 넘기지만, 레코더 팩토리를 등록하지 않으면 코디네이터는 기록하지 않는다.

pub mod engine;
pub mod helpers;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Utc;
use futures_util::future::BoxFuture;
use serde::Serialize;
use tokio::sync::watch;
use url::Url;

use cview_core::models::equalizer::EqualizerPreset;
use cview_core::models::player::{
    PlayerEngineType, PlayerPhase, PlayerTool, PlayerToolStatus, PlayerToolStatusKind,
};
use cview_core::models::settings::{ExperimentalPlaybackFlags, PlayerSettings, ScreenshotFormat};
use cview_core::models::stream_proxy::{StreamProxyMode, StreamProxyRouteStatus};
use cview_core::protocols::PlayerEngineCapabilities;
use cview_player::coordinator::{
    BufferHealth, Configuration, LatencyInfo, LowLatencyActivation, SharedEngine,
    StreamCoordinator, StreamEvent, StreamPhase, StreamQualityInfo, TimeshiftRecordingConfig,
};
use cview_player::hlsjs::{HlsJsForcedQualityTarget, HlsJsMatchWebV2Config};
use cview_player::manifest::parser::MasterPlaylist;
use cview_player::policy::occlusion_cap::should_skip_background_resync;
use cview_player::policy::{LatencyPresetPairing, PlaybackKeyAccess, RecoverySignal};
use cview_player::LowLatencyConfiguration;

use super::support::{cancel_slot, slot_is, Observed, ObservedGuard, TaskSlot};
use crate::logic::effective_playback_policy::EffectivePlaybackPolicy;
pub use engine::{LiveEngineFactory, LivePlayerEngine, SharedLiveEngine, VideoAdjust};
pub use helpers::{
    apply_quality_intent, equalizer_bands_fitting_frequencies, format_time_interval, format_uptime,
    screenshot_file_base_name, selectable_qualities, write_screenshot, PlayerOverlayState,
};

/// 레이턴시 이력 최대 길이.
pub const MAX_LATENCY_HISTORY: usize = 60;
/// 되감기 창 상한(1시간).
pub const MAX_TIME_MACHINE_WINDOW: f64 = 60.0 * 60.0;
/// 메인에서 벗어난 세션의 기록을 지우기까지의 유예(초).
pub const REWIND_RELEASE_GRACE: f64 = 120.0;
/// 방금 옮긴 위치를 뒤처짐으로 믿는 시간(초).
pub const REWIND_SEEK_HINT_LIFETIME: f64 = 4.0;
/// 되감기 컨트롤을 보일 최소 기록 길이(초).
pub const MINIMUM_REWIND_WINDOW: f64 = 10.0;

/// 되감기 출처.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RewindSource {
    None,
    ServerTimeMachine,
    LocalTimeshift,
}

/// 설정에서 온 되감기 값.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RewindSettings {
    pub enabled: bool,
    pub window_minutes: i64,
    pub budget_mb: i64,
    pub proxy_mode: StreamProxyMode,
}

impl Default for RewindSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            window_minutes: 60,
            budget_mb: 5_120,
            proxy_mode: StreamProxyMode::LocalProxy,
        }
    }
}

impl RewindSettings {
    pub fn from_settings(s: &PlayerSettings) -> Self {
        Self {
            enabled: s.live_rewind_enabled,
            window_minutes: s.live_rewind_window_minutes,
            budget_mb: s.live_rewind_disk_budget_mb,
            proxy_mode: s.stream_proxy_mode,
        }
    }
}

/// 사용자 이벤트 콜백.
#[derive(Default, Clone)]
pub struct PlayerCallbacks {
    pub on_playback_state_changed: Option<Arc<dyn Fn() + Send + Sync>>,
    pub on_check_stream_ended: Option<Arc<dyn Fn() -> BoxFuture<'static, bool> + Send + Sync>>,
    pub on_volume_changed: Option<Arc<dyn Fn(f32) + Send + Sync>>,
    pub on_mute_changed: Option<Arc<dyn Fn(bool) + Send + Sync>>,
}

/// `startStream` 요청.
#[derive(Clone)]
pub struct StartStreamRequest {
    pub channel_id: String,
    pub stream_url: Url,
    pub channel_name: String,
    pub live_title: String,
    pub thumbnail_url: Option<Url>,
    pub prefetched_manifest: Option<MasterPlaylist>,
    pub player_settings: Option<PlayerSettings>,
    pub playback_key_access: PlaybackKeyAccess,
    pub match_web_v2_source_eligible: bool,
}

impl StartStreamRequest {
    pub fn new(channel_id: impl Into<String>, stream_url: Url) -> Self {
        Self {
            channel_id: channel_id.into(),
            stream_url,
            channel_name: String::new(),
            live_title: String::new(),
            thumbnail_url: None,
            prefetched_manifest: None,
            player_settings: None,
            playback_key_access: PlaybackKeyAccess::anonymous(),
            match_web_v2_source_eligible: true,
        }
    }
}

// MARK: - 상태

/// 관찰 상태 + 내부 상태.
pub struct PlayerState {
    pub stream_phase: StreamPhase,
    pub current_quality: Option<StreamQualityInfo>,
    pub available_qualities: Vec<StreamQualityInfo>,
    pub latency_info: Option<LatencyInfo>,
    /// `(타임스탬프, 지연초)`.
    pub latency_history: Vec<(chrono::DateTime<Utc>, f64)>,
    pub buffer_health: Option<BufferHealth>,
    pub playback_rate: f64,
    pub volume: f32,
    pub is_muted: bool,
    pub is_fullscreen: bool,
    pub is_audio_only: bool,
    pub show_controls: bool,
    pub error_message: Option<String>,
    pub is_live_stream: bool,
    pub is_cheat_key_user: bool,
    pub is_time_machine_available: bool,
    pub is_time_shifting: bool,
    pub has_rewindable_recording: bool,
    pub timeshift_recorded_duration: f64,
    pub presented_frame_tick: u64,
    pub is_recording: bool,
    pub recording_duration: f64,
    pub recording_path: Option<PathBuf>,
    pub recording_status: Option<PlayerToolStatus>,
    pub screenshot_save_path: String,
    pub screenshot_format: ScreenshotFormat,
    pub screenshot_status: Option<PlayerToolStatus>,
    pub channel_name: String,
    pub live_title: String,
    pub thumbnail_url: Option<Url>,
    pub viewer_count: i64,
    pub uptime: f64,
    pub current_channel_id: Option<String>,
    pub is_multi_live: bool,
    pub precision_sync_requested: bool,
    pub current_engine_type: PlayerEngineType,
    pub current_proxy_mode: String,
    pub proxy_route_status: Option<StreamProxyRouteStatus>,
    pub is_equalizer_enabled: bool,
    pub equalizer_preset_name: String,
    pub equalizer_pre_amp: f32,
    pub equalizer_bands: Vec<f32>,
    pub is_video_adjust_enabled: bool,
    pub video_brightness: f32,
    pub video_contrast: f32,
    pub video_saturation: f32,
    pub video_hue: f32,
    pub video_gamma: f32,
    pub aspect_ratio: Option<String>,
    pub picture_enhancement_desired: (bool, bool),
    // 내부
    rewind_recording_desired: Option<bool>,
    multi_live_quality_target: HlsJsForcedQualityTarget,
    multi_live_source_qualities: Vec<StreamQualityInfo>,
    latency_sample_sequence: u64,
    has_initialized_session_volume: bool,
    coordinator: Option<StreamCoordinator>,
    engine: Option<SharedLiveEngine>,
    is_preallocated: bool,
    session_id: u64,
    start_generation: u64,
    event_task: Option<TaskSlot>,
    uptime_task: Option<TaskSlot>,
    control_hide_task: Option<TaskSlot>,
    buffering_debounce: Option<TaskSlot>,
    last_playing_time: Option<Instant>,
    buffering_started_at: Option<Instant>,
    rewind_settings: RewindSettings,
    rewind_seek_hint: Option<(f64, Instant)>,
    rewind_request_serial: u64,
    rewind_release_task: Option<TaskSlot>,
    rewind_window_task: Option<TaskSlot>,
    recording_timer: Option<TaskSlot>,
    recording_status_reset: Option<TaskSlot>,
    screenshot_status_reset: Option<TaskSlot>,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            stream_phase: StreamPhase::Idle,
            current_quality: None,
            available_qualities: Vec::new(),
            latency_info: None,
            latency_history: Vec::new(),
            buffer_health: None,
            playback_rate: 1.0,
            volume: 1.0,
            is_muted: false,
            is_fullscreen: false,
            is_audio_only: false,
            show_controls: true,
            error_message: None,
            is_live_stream: true,
            is_cheat_key_user: false,
            is_time_machine_available: false,
            is_time_shifting: false,
            has_rewindable_recording: false,
            timeshift_recorded_duration: 0.0,
            presented_frame_tick: 0,
            is_recording: false,
            recording_duration: 0.0,
            recording_path: None,
            recording_status: None,
            screenshot_save_path: "~/Pictures/CView Screenshots".into(),
            screenshot_format: ScreenshotFormat::Png,
            screenshot_status: None,
            channel_name: String::new(),
            live_title: String::new(),
            thumbnail_url: None,
            viewer_count: 0,
            uptime: 0.0,
            current_channel_id: None,
            is_multi_live: false,
            precision_sync_requested: false,
            current_engine_type: PlayerEngineType::Hlsjs,
            current_proxy_mode: "pending".into(),
            proxy_route_status: None,
            is_equalizer_enabled: false,
            equalizer_preset_name: String::new(),
            equalizer_pre_amp: 0.0,
            equalizer_bands: Vec::new(),
            is_video_adjust_enabled: false,
            video_brightness: 1.0,
            video_contrast: 1.0,
            video_saturation: 1.0,
            video_hue: 0.0,
            video_gamma: 1.0,
            aspect_ratio: None,
            picture_enhancement_desired: (false, false),
            rewind_recording_desired: None,
            multi_live_quality_target: HlsJsForcedQualityTarget::MULTI_LIVE_1080P_8MBPS,
            multi_live_source_qualities: Vec::new(),
            latency_sample_sequence: 0,
            has_initialized_session_volume: false,
            coordinator: None,
            engine: None,
            is_preallocated: false,
            session_id: 0,
            start_generation: 0,
            event_task: None,
            uptime_task: None,
            control_hide_task: None,
            buffering_debounce: None,
            last_playing_time: None,
            buffering_started_at: None,
            rewind_settings: RewindSettings::default(),
            rewind_seek_hint: None,
            rewind_request_serial: 0,
            rewind_release_task: None,
            rewind_window_task: None,
            recording_timer: None,
            recording_status_reset: None,
            screenshot_status_reset: None,
        }
    }
}

impl PlayerState {
    pub fn engine(&self) -> Option<&SharedLiveEngine> {
        self.engine.as_ref()
    }

    pub fn coordinator(&self) -> Option<&StreamCoordinator> {
        self.coordinator.as_ref()
    }

    pub fn multi_live_quality_target(&self) -> HlsJsForcedQualityTarget {
        self.multi_live_quality_target
    }

    pub fn rewind_recording_desired(&self) -> Option<bool> {
        self.rewind_recording_desired
    }

    pub fn is_rewind_recording_release_pending(&self) -> bool {
        self.rewind_release_task.is_some()
    }

    fn capabilities(&self) -> PlayerEngineCapabilities {
        self.engine
            .as_ref()
            .map(|e| e.capabilities())
            .unwrap_or_default()
    }

    pub fn can_take_screenshot(&self) -> bool {
        self.capabilities()
            .contains(PlayerEngineCapabilities::SCREENSHOT)
    }

    pub fn can_record(&self) -> bool {
        self.capabilities()
            .contains(PlayerEngineCapabilities::RECORDING)
    }

    pub fn can_use_equalizer(&self) -> bool {
        self.capabilities()
            .contains(PlayerEngineCapabilities::EQUALIZER)
    }

    fn live_edge_tolerance(&self) -> f64 {
        5.0_f64.max(self.latency_info.map(|l| l.target).unwrap_or(3.5) * 1.5)
    }

    pub fn rewind_live_edge_tolerance(&self) -> f64 {
        self.live_edge_tolerance()
    }

    pub fn time_machine_window_duration(&self) -> f64 {
        if !self.is_time_machine_available {
            return 0.0;
        }
        match self.engine.as_ref().and_then(|e| e.seekable_range()) {
            Some(r) => MAX_TIME_MACHINE_WINDOW.min((r.end() - r.start()).max(0.0)),
            None => 0.0,
        }
    }

    pub fn rewind_source(&self) -> RewindSource {
        if self.is_time_machine_available && self.time_machine_window_duration() > 0.0 {
            RewindSource::ServerTimeMachine
        } else if self.has_rewindable_recording {
            RewindSource::LocalTimeshift
        } else {
            RewindSource::None
        }
    }

    pub fn rewind_window_duration(&self) -> f64 {
        match self.rewind_source() {
            RewindSource::ServerTimeMachine => self.time_machine_window_duration(),
            RewindSource::LocalTimeshift => {
                MAX_TIME_MACHINE_WINDOW.min(self.timeshift_recorded_duration)
            }
            RewindSource::None => 0.0,
        }
    }

    pub fn seconds_behind_live(&self) -> f64 {
        let window = self.rewind_window_duration();
        let source = self.rewind_source();
        let uses_window = source == RewindSource::ServerTimeMachine
            || (source == RewindSource::LocalTimeshift && self.is_time_shifting);
        let latency = self.latency_info.map(|l| l.current).unwrap_or(0.0);
        if window <= 0.0 || !uses_window {
            return latency;
        }
        if self.is_time_shifting {
            if let Some((behind, at)) = self.rewind_seek_hint {
                if at.elapsed().as_secs_f64() < REWIND_SEEK_HINT_LIFETIME {
                    return window.min(behind);
                }
            }
        }
        match self
            .engine
            .as_ref()
            .and_then(|e| e.seekable_range().map(|r| (*r.end(), e.current_time())))
        {
            Some((end, current)) => window.min((end - current).max(0.0)),
            None => latency,
        }
    }

    pub fn is_behind_live(&self) -> bool {
        if self.stream_phase != StreamPhase::Playing {
            return false;
        }
        if self.is_time_shifting {
            return true;
        }
        if self.rewind_source() == RewindSource::ServerTimeMachine
            && self.rewind_window_duration() > 0.0
        {
            return self.seconds_behind_live() > self.live_edge_tolerance();
        }
        match self.latency_info {
            Some(info) => info.current > info.target * 1.5,
            None => false,
        }
    }

    /// 이 세션이 기록해야 할 로컬 되감기 설정.
    pub fn rewind_recording_config(&self) -> Option<TimeshiftRecordingConfig> {
        let desired = self.rewind_recording_desired.unwrap_or(!self.is_multi_live)
            || self.is_rewind_recording_release_pending();
        let r = &self.rewind_settings;
        if !desired
            || self.is_time_machine_available
            || !r.enabled
            || r.proxy_mode != StreamProxyMode::LocalProxy
        {
            return None;
        }
        let minutes = r
            .window_minutes
            .min((MAX_TIME_MACHINE_WINDOW / 60.0) as i64);
        Some(TimeshiftRecordingConfig {
            max_duration: (minutes * 60) as f64,
            budget_limit_bytes: r.budget_mb * 1_048_576,
        })
    }

    pub fn formatted_latency(&self) -> String {
        if let Some(info) = self.latency_info {
            return format!("{:.1}초", info.current);
        }
        if let Some(e) = self.engine.as_ref().filter(|e| e.is_playing()) {
            let (d, c) = (e.duration(), e.current_time());
            let lat = d - c;
            if d > 0.0 && c > 0.0 && lat > 0.0 && lat < 60.0 {
                return format!("{lat:.1}초");
            }
        }
        "-".into()
    }

    pub fn formatted_playback_rate(&self) -> String {
        if (self.playback_rate - 1.0).abs() < 0.01 {
            "1.0x".into()
        } else {
            format!("{:.2}x", self.playback_rate)
        }
    }
}

/// 프런트엔드로 보내는 스냅샷.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerSnapshot {
    pub channel_id: Option<String>,
    pub stream_phase: StreamPhase,
    pub overlay: PlayerOverlayState,
    pub current_quality: Option<StreamQualityInfo>,
    pub available_qualities: Vec<StreamQualityInfo>,
    pub latency: Option<LatencyInfo>,
    pub latency_history: Vec<f64>,
    pub buffer_health: Option<BufferHealth>,
    pub playback_rate: f64,
    pub volume: f32,
    pub is_muted: bool,
    pub is_audio_only: bool,
    pub error_message: Option<String>,
    pub is_time_shifting: bool,
    pub is_behind_live: bool,
    pub rewind_source: RewindSource,
    pub rewind_window: f64,
    pub seconds_behind_live: f64,
    pub is_recording: bool,
    pub recording_duration: f64,
    pub recording_status: Option<String>,
    pub screenshot_status: Option<String>,
    pub channel_name: String,
    pub live_title: String,
    pub uptime: String,
    pub proxy_mode: String,
    pub formatted_latency: String,
    pub is_equalizer_enabled: bool,
    pub equalizer_preset_name: String,
    pub is_video_adjust_enabled: bool,
}

// MARK: - 뷰모델

#[derive(Clone)]
pub struct PlayerViewModel {
    state: Arc<Observed<PlayerState>>,
    callbacks: Arc<Mutex<PlayerCallbacks>>,
    engine_factory: Option<LiveEngineFactory>,
    start_lock: Arc<tokio::sync::Mutex<()>>,
}

fn spawn_detached<F>(fut: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    if let Ok(h) = tokio::runtime::Handle::try_current() {
        h.spawn(fut);
    }
}

fn upcast(engine: &SharedLiveEngine) -> SharedEngine {
    engine.clone()
}

impl PlayerViewModel {
    /// `engine_factory` 가 없으면 시작 전에 [`Self::inject_engine`] 으로 엔진을 넣어야 한다.
    pub fn new(engine_factory: Option<LiveEngineFactory>) -> Self {
        Self {
            state: Arc::new(Observed::new(PlayerState::default())),
            callbacks: Arc::new(Mutex::new(PlayerCallbacks::default())),
            engine_factory,
            start_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub fn read(&self) -> ObservedGuard<'_, PlayerState> {
        self.state.read()
    }

    /// 상태를 직접 고친다(셸·테스트용 — Swift 의 `public var` 대입).
    pub fn lock(&self) -> ObservedGuard<'_, PlayerState> {
        self.state.lock()
    }

    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.state.subscribe()
    }

    pub fn set_callbacks(&self, callbacks: PlayerCallbacks) {
        *crate::services::support::lock(&self.callbacks) = callbacks;
    }

    fn callbacks(&self) -> PlayerCallbacks {
        crate::services::support::lock(&self.callbacks).clone()
    }

    fn notify_playback_state_changed(&self) {
        if let Some(cb) = self.callbacks().on_playback_state_changed {
            cb();
        }
    }

    pub fn snapshot(&self) -> PlayerSnapshot {
        let s = self.state.read();
        PlayerSnapshot {
            channel_id: s.current_channel_id.clone(),
            stream_phase: s.stream_phase.clone(),
            overlay: PlayerOverlayState::from(false, Some(&s.stream_phase), false),
            current_quality: s.current_quality.clone(),
            available_qualities: s.available_qualities.clone(),
            latency: s.latency_info,
            latency_history: s.latency_history.iter().map(|p| p.1).collect(),
            buffer_health: s.buffer_health,
            playback_rate: s.playback_rate,
            volume: s.volume,
            is_muted: s.is_muted,
            is_audio_only: s.is_audio_only,
            error_message: s.error_message.clone(),
            is_time_shifting: s.is_time_shifting,
            is_behind_live: s.is_behind_live(),
            rewind_source: s.rewind_source(),
            rewind_window: s.rewind_window_duration(),
            seconds_behind_live: s.seconds_behind_live(),
            is_recording: s.is_recording,
            recording_duration: s.recording_duration,
            recording_status: s.recording_status.as_ref().map(|t| t.message.clone()),
            screenshot_status: s.screenshot_status.as_ref().map(|t| t.message.clone()),
            channel_name: s.channel_name.clone(),
            live_title: s.live_title.clone(),
            uptime: format_uptime(s.uptime),
            proxy_mode: s.current_proxy_mode.clone(),
            formatted_latency: s.formatted_latency(),
            is_equalizer_enabled: s.is_equalizer_enabled,
            equalizer_preset_name: s.equalizer_preset_name.clone(),
            is_video_adjust_enabled: s.is_video_adjust_enabled,
        }
    }

    // MARK: 엔진 주입

    /// 미리 만든 엔진 주입(멀티라이브 엔진 풀).
    pub fn inject_engine(&self, engine: SharedLiveEngine) {
        let mut s = self.state.lock();
        s.engine = Some(engine);
        s.is_preallocated = true;
        s.current_engine_type = PlayerEngineType::Hlsjs;
    }

    /// 주입된 엔진 분리 — stop 은 부르지 않는다.
    pub fn detach_engine(&self) -> Option<SharedLiveEngine> {
        let mut s = self.state.lock();
        s.is_preallocated = false;
        s.engine.take()
    }

    // MARK: 멀티라이브 화질

    /// 바뀌었으면 `true`.
    pub fn set_multi_live_quality_target(&self, target: HlsJsForcedQualityTarget) -> bool {
        let mut s = self.state.lock();
        if !s.is_multi_live || s.multi_live_quality_target == target {
            return false;
        }
        s.multi_live_quality_target = target;
        if let Some(e) = &s.engine {
            e.set_forced_quality(Some(target));
        }
        let sources = s.multi_live_source_qualities.clone();
        Self::update_multi_live_quality_options(&mut s, sources);
        true
    }

    fn update_multi_live_quality_options(s: &mut PlayerState, qualities: Vec<StreamQualityInfo>) {
        let next = selectable_qualities(&qualities, s.is_multi_live, s.multi_live_quality_target);
        s.multi_live_source_qualities = qualities;
        if s.available_qualities != next {
            s.available_qualities = next;
        }
        if s.is_multi_live && s.current_quality.as_ref() != s.available_qualities.first() {
            s.current_quality = s.available_qualities.first().cloned();
        }
    }

    /// 멀티라이브 선택 세션 정밀 웹 싱크.
    pub fn set_precision_sync_active(&self, active: bool) {
        let coordinator = {
            let mut s = self.state.lock();
            if s.precision_sync_requested == active {
                return;
            }
            s.precision_sync_requested = active;
            if let Some(e) = &s.engine {
                e.set_match_web_v2_seek_priority(if active { 1 } else { 0 });
            }
            if !s.is_multi_live {
                return;
            }
            s.coordinator.clone()
        };
        if let Some(c) = coordinator {
            spawn_detached(async move { c.set_low_latency_sync_active(active).await });
        }
    }

    // MARK: 설정

    /// 기본 볼륨은 첫 엔진 시작 전에 한 번만 가져온다.
    pub fn initialize_session_volume(&self, settings: &PlayerSettings) {
        let mut s = self.state.lock();
        Self::initialize_session_volume_locked(&mut s, settings);
    }

    fn initialize_session_volume_locked(s: &mut PlayerState, settings: &PlayerSettings) {
        if s.has_initialized_session_volume {
            return;
        }
        if settings.volume_level.is_finite() {
            s.volume = settings.volume_level.clamp(0.0, 1.0);
        }
        s.has_initialized_session_volume = true;
    }

    pub fn apply_settings(&self, settings: &PlayerSettings, preserve_session_volume: bool) {
        let policy = EffectivePlaybackPolicy::new(settings);
        self.apply_rewind_settings(settings);
        {
            let mut s = self.state.lock();
            if preserve_session_volume {
                Self::initialize_session_volume_locked(&mut s, settings);
            } else {
                if settings.volume_level.is_finite() {
                    s.volume = settings.volume_level.clamp(0.0, 1.0);
                }
                s.has_initialized_session_volume = true;
            }
            if let Some(e) = &s.engine {
                e.set_volume(if s.is_muted { 0.0 } else { s.volume });
                apply_quality_intent(
                    e.as_ref(),
                    &policy,
                    s.is_multi_live,
                    s.multi_live_quality_target,
                );
            }
        }
        self.apply_latency_settings(settings);
    }

    pub fn apply_force_highest_quality(&self, enabled: bool) {
        if let Some(e) = &self.state.read().engine {
            e.set_force_highest_quality(enabled);
        }
    }

    /// 설정의 지연 프리셋 → 코디네이터 저지연 설정.
    pub fn apply_latency_settings(&self, settings: &PlayerSettings) {
        let s = self.state.read();
        let Some(coordinator) = &s.coordinator else {
            return;
        };
        let v2 = s
            .engine
            .as_ref()
            .and_then(|e| e.hls().map(|h| h.is_match_web_v2_active()))
            .unwrap_or(false);
        coordinator
            .update_low_latency_config(LatencyPresetPairing::configuration_for(settings, v2));
    }

    pub fn apply_sharp_pixel_scaling(&self, enabled: bool) {
        if let Some(e) = &self.state.read().engine {
            e.set_sharp_pixel_scaling(enabled);
        }
    }

    pub fn apply_picture_enhancement(&self, auto: bool, upscale: bool) {
        let mut s = self.state.lock();
        s.picture_enhancement_desired = (auto, upscale);
        if let Some(e) = &s.engine {
            e.set_picture_enhancement(auto, upscale);
        }
    }

    // MARK: 가림·백그라운드

    /// 창이 완전히 가려졌다 — 표면만 숨긴다(재생 유지).
    pub fn set_surface_hidden(&self, hidden: bool) {
        let (engine, coordinator) = {
            let s = self.state.read();
            (s.engine.clone(), s.coordinator.clone())
        };
        let Some(engine) = engine.filter(|e| e.is_hlsjs()) else {
            return;
        };
        engine.set_video_surface_visible(!hidden);
        if let Some(c) = coordinator {
            spawn_detached(async move { c.reconcile_network_activity().await });
        }
    }

    /// hls.js 로딩 정지 상태를 코디네이터 네트워크 폴러에 반영.
    pub fn sync_network_activity(&self) {
        let s = self.state.read();
        if !s.engine.as_ref().is_some_and(|e| e.is_hlsjs()) {
            return;
        }
        if let Some(c) = s.coordinator.clone() {
            spawn_detached(async move { c.reconcile_network_activity().await });
        }
    }

    /// 앱 백그라운드 복귀 — hls.js 하드 리싱크 + 코디네이터 복구.
    pub fn recover_from_background(&self) {
        let s = self.state.read();
        if !matches!(
            s.stream_phase,
            StreamPhase::Playing | StreamPhase::Buffering
        ) {
            return;
        }
        let Some(engine) = &s.engine else {
            return;
        };
        if !s.is_time_shifting && engine.is_hlsjs() {
            engine.force_live_resync();
        }
        if let Some(c) = &s.coordinator {
            c.recover_from_background();
        }
    }

    /// 인앱 선택 승격용 경량 복구.
    pub fn promote_for_in_app_selection(&self) {
        let coordinator = {
            let s = self.state.read();
            if !matches!(
                s.stream_phase,
                StreamPhase::Playing | StreamPhase::Buffering
            ) {
                return;
            }
            s.coordinator.clone()
        };
        self.force_live_resync_if_hlsjs();
        if let Some(c) = coordinator {
            c.recover_from_background();
        }
    }

    /// 드리프트가 작으면(목표 대비 2초 미만) 왕복을 생략한다.
    pub fn force_live_resync_if_hlsjs(&self) {
        let s = self.state.read();
        if !matches!(
            s.stream_phase,
            StreamPhase::Playing | StreamPhase::Buffering
        ) || s.is_time_shifting
        {
            return;
        }
        let Some(engine) = s.engine.as_ref().filter(|e| e.is_hlsjs()) else {
            return;
        };
        let latency = engine
            .hls()
            .map(|h| h.live_latency_seconds())
            .unwrap_or(0.0);
        if should_skip_background_resync(latency, engine.effective_live_target_latency_seconds()) {
            return;
        }
        engine.force_live_resync();
    }

    // MARK: 라이브 복귀·되감기

    fn snap_to_live_edge(engine: &SharedLiveEngine) {
        if !engine.is_hlsjs() {
            return;
        }
        if let Some(h) = engine.hls() {
            h.set_live_latency_correction_enabled(true);
        }
        engine.seek_to_live_edge();
    }

    /// 수동 '실시간으로 이동'.
    pub fn jump_to_live(&self) {
        let (engine, coordinator, paused) = {
            let mut s = self.state.lock();
            let ok = matches!(
                s.stream_phase,
                StreamPhase::Playing | StreamPhase::Buffering
            ) || (s.stream_phase == StreamPhase::Paused && s.is_time_shifting);
            if !ok {
                return;
            }
            let Some(engine) = s.engine.clone() else {
                return;
            };
            s.is_time_shifting = false;
            s.rewind_seek_hint = None;
            s.rewind_request_serial = s.rewind_request_serial.wrapping_add(1);
            (
                engine,
                s.coordinator.clone(),
                s.stream_phase == StreamPhase::Paused,
            )
        };
        let Some(coordinator) = coordinator else {
            Self::snap_to_live_edge(&engine);
            if paused {
                engine.pause();
            }
            return;
        };
        if coordinator.is_local_timeshift_active() {
            spawn_detached(async move { coordinator.exit_timeshift().await });
        } else {
            Self::snap_to_live_edge(&engine);
            if paused {
                engine.pause();
            }
            spawn_detached(async move { coordinator.set_time_shift_active(false).await });
        }
    }

    pub fn configure_time_machine_access(&self, is_cheat_key_user: bool, available: bool) {
        let coordinator = {
            let mut s = self.state.lock();
            let was = s.is_time_machine_available;
            s.is_cheat_key_user = is_cheat_key_user;
            s.is_time_machine_available = is_cheat_key_user && available;
            if was && !s.is_time_machine_available && s.is_time_shifting {
                s.is_time_shifting = false;
                s.coordinator.clone()
            } else {
                None
            }
        };
        if let Some(c) = coordinator {
            spawn_detached(async move { c.set_time_shift_active(false).await });
        }
    }

    pub fn note_presented_frame(&self) {
        let mut s = self.state.lock();
        s.presented_frame_tick = s.presented_frame_tick.wrapping_add(1);
    }

    /// 서버 타임머신 — 최근 1시간 안에서 과거 위치로 seek.
    pub async fn seek_time_machine(&self, seconds_behind: f64) {
        let (engine, coordinator, range) = {
            let s = self.state.read();
            if !s.is_time_machine_available {
                return;
            }
            let Some(engine) = s.engine.clone() else {
                return;
            };
            let Some(range) = engine.seekable_range() else {
                return;
            };
            (engine, s.coordinator.clone(), range)
        };
        let window = MAX_TIME_MACHINE_WINDOW.min(range.end() - range.start());
        if window <= 0.0 {
            return;
        }
        let behind = seconds_behind.max(0.0).min(window);
        if behind <= self.state.read().live_edge_tolerance() {
            self.jump_to_live();
            return;
        }
        self.state.lock().is_time_shifting = true;
        if let Some(c) = &coordinator {
            c.set_time_shift_active(true).await;
        }
        let same = self
            .state
            .read()
            .engine
            .as_ref()
            .is_some_and(|e| Arc::ptr_eq(e, &engine));
        if !same {
            self.state.lock().is_time_shifting = false;
            if let Some(c) = &coordinator {
                c.set_time_shift_active(false).await;
            }
            return;
        }
        engine.set_rate(1.0);
        engine.seek(range.start().max(range.end() - behind));
    }

    /// 되감기 — 출처에 따라 서버 타임머신 seek 또는 로컬 타임시프트 진입.
    pub async fn seek_rewind(&self, seconds_behind: f64) {
        let (source, window, tolerance, coordinator) = {
            let s = self.state.read();
            if !matches!(
                s.stream_phase,
                StreamPhase::Playing | StreamPhase::Buffering | StreamPhase::Paused
            ) {
                return;
            }
            (
                s.rewind_source(),
                s.rewind_window_duration(),
                s.live_edge_tolerance(),
                s.coordinator.clone(),
            )
        };
        let behind = seconds_behind.max(0.0).min(window);
        match source {
            RewindSource::None => {}
            RewindSource::ServerTimeMachine => {
                self.state.lock().rewind_seek_hint =
                    (behind > tolerance).then(|| (behind, Instant::now()));
                self.seek_time_machine(seconds_behind).await;
            }
            RewindSource::LocalTimeshift => {
                let Some(coordinator) = coordinator else {
                    return;
                };
                if behind <= tolerance {
                    self.jump_to_live();
                    return;
                }
                let serial = {
                    let mut s = self.state.lock();
                    s.is_time_shifting = true;
                    s.rewind_seek_hint = Some((behind, Instant::now()));
                    s.rewind_request_serial = s.rewind_request_serial.wrapping_add(1);
                    s.rewind_request_serial
                };
                let entered = coordinator.enter_timeshift(behind).await;
                let mut s = self.state.lock();
                if !entered && serial == s.rewind_request_serial {
                    s.is_time_shifting = false;
                    s.rewind_seek_hint = None;
                }
            }
        }
    }

    /// ←/→ 한 칸.
    pub async fn step_rewind(&self, delta: f64) {
        let behind = {
            let s = self.state.read();
            if s.rewind_source() == RewindSource::None {
                return;
            }
            s.seconds_behind_live()
        };
        self.seek_rewind(behind - delta).await;
    }

    pub fn apply_rewind_settings(&self, settings: &PlayerSettings) {
        let rewind = RewindSettings::from_settings(settings);
        {
            let mut s = self.state.lock();
            if s.rewind_settings == rewind {
                return;
            }
            s.rewind_settings = rewind;
        }
        self.apply_rewind_recording();
    }

    /// 메인 포커스 여부(멀티라이브). true → false 는 유예 뒤에 기록을 끈다.
    pub fn set_rewind_recording_desired(&self, desired: Option<bool>) {
        let (old, jump) = {
            let mut s = self.state.lock();
            let old = s.rewind_recording_desired;
            if old == desired {
                return;
            }
            s.rewind_recording_desired = desired;
            (old, s.is_time_shifting)
        };
        if old == Some(true) && desired == Some(false) {
            if jump {
                self.jump_to_live();
                self.state.lock().is_time_shifting = false;
            }
            self.schedule_rewind_recording_release();
        } else {
            cancel_slot(&mut self.state.lock().rewind_release_task);
            self.apply_rewind_recording();
        }
    }

    fn schedule_rewind_recording_release(&self) {
        let me = self.clone();
        let mut s = self.state.lock();
        cancel_slot(&mut s.rewind_release_task);
        s.rewind_release_task = Some(TaskSlot::spawn(move |id| async move {
            tokio::time::sleep(Duration::from_secs_f64(REWIND_RELEASE_GRACE)).await;
            {
                let mut s = me.state.lock();
                if !slot_is(&s.rewind_release_task, id) {
                    return;
                }
                s.rewind_release_task = None;
            }
            me.apply_rewind_recording();
        }));
    }

    pub fn rewind_recording_config(&self) -> Option<TimeshiftRecordingConfig> {
        self.state.read().rewind_recording_config()
    }

    fn apply_rewind_recording(&self) {
        let (coordinator, config) = {
            let mut s = self.state.lock();
            let Some(c) = s.coordinator.clone() else {
                return;
            };
            let config = s.rewind_recording_config();
            if config.is_none() {
                cancel_slot(&mut s.rewind_window_task);
                Self::apply_timeshift_window(&mut s, 0.0);
                if s.is_time_shifting && !s.is_time_machine_available {
                    s.is_time_shifting = false;
                }
            }
            (c, config)
        };
        let me = self.clone();
        spawn_detached(async move {
            coordinator.set_timeshift_recording(config).await;
            let start = {
                let s = me.state.read();
                config.is_some()
                    && s.rewind_window_task.is_none()
                    && s.stream_phase != StreamPhase::Idle
            };
            if start {
                me.start_rewind_window_updates(coordinator);
            }
        });
    }

    fn start_rewind_window_updates(&self, coordinator: StreamCoordinator) {
        let me = self.clone();
        let mut s = self.state.lock();
        let session = s.session_id;
        cancel_slot(&mut s.rewind_window_task);
        s.rewind_window_task = Some(TaskSlot::spawn(move |id| async move {
            loop {
                tokio::time::sleep(Duration::from_secs(2)).await;
                let duration = coordinator.timeshift_recorded_duration();
                let mut s = me.state.lock();
                if !slot_is(&s.rewind_window_task, id) || s.session_id != session {
                    return;
                }
                Self::apply_timeshift_window(&mut s, duration);
            }
        }));
    }

    fn apply_timeshift_window(s: &mut PlayerState, recorded: f64) {
        s.timeshift_recorded_duration = recorded;
        let rewindable = recorded >= MINIMUM_REWIND_WINDOW;
        if rewindable != s.has_rewindable_recording {
            s.has_rewindable_recording = rewindable;
        }
    }

    // MARK: 스트림 시작/정지

    /// 스트림 시작. 앞선 시작은 끊고, 정지와 교차하면 결과를 버린다.
    pub async fn start_stream(&self, request: StartStreamRequest) {
        let generation = {
            let mut s = self.state.lock();
            s.start_generation = s.start_generation.wrapping_add(1);
            s.start_generation
        };
        let _guard = self.start_lock.lock().await;
        if self.state.read().start_generation != generation {
            return;
        }
        self.perform_start_stream(request, generation).await;
    }

    fn is_current_start(&self, generation: u64) -> bool {
        self.state.read().start_generation == generation
    }

    async fn perform_start_stream(&self, req: StartStreamRequest, generation: u64) {
        let settings = req.player_settings.clone().unwrap_or_default();
        let policy = EffectivePlaybackPolicy::new(&settings);
        let (is_multi_live, time_machine, precision, multi_target) = {
            let mut s = self.state.lock();
            Self::initialize_session_volume_locked(&mut s, &settings);
            s.current_quality = None;
            s.available_qualities.clear();
            s.multi_live_source_qualities.clear();
            s.latency_info = None;
            s.latency_history.clear();
            s.latency_sample_sequence = 0;
            s.buffer_health = None;
            s.playback_rate = 1.0;
            s.error_message = None;
            s.is_time_shifting = false;
            s.channel_name = req.channel_name.clone();
            s.live_title = req.live_title.clone();
            s.thumbnail_url = req.thumbnail_url.clone();
            s.current_channel_id = Some(req.channel_id.clone());
            s.aspect_ratio = req
                .player_settings
                .as_ref()
                .and_then(|p| p.aspect_ratio.clone());
            s.proxy_route_status = None;
            s.current_proxy_mode = "pending".into();
            (
                s.is_multi_live,
                s.is_time_machine_available,
                s.precision_sync_requested,
                s.multi_live_quality_target,
            )
        };

        let match_web_v2 = LatencyPresetPairing::is_match_web_v2_eligible(
            req.player_settings.as_ref(),
            ExperimentalPlaybackFlags::match_web_v2(None),
            time_machine,
            req.match_web_v2_source_eligible,
        );
        let low_latency_config = if match_web_v2 {
            LowLatencyConfiguration::MATCH_WEB_V2_HLSJS
        } else {
            req.player_settings
                .as_ref()
                .map(|p| LatencyPresetPairing::pairing(Some(p)).configuration)
                .unwrap_or(LowLatencyConfiguration::WEB_PARITY_HLSJS)
        };
        let mut config = Configuration::new(req.channel_id.clone());
        config.enable_low_latency = policy.low_latency_enabled;
        config.low_latency_activation = if is_multi_live {
            LowLatencyActivation::Managed
        } else {
            LowLatencyActivation::Automatic
        };
        config.preferred_quality = policy.preferred_quality;
        config.low_latency_config = low_latency_config;
        config.force_highest_quality = policy.force_highest_quality();
        config.quality_policy = Some(policy.quality_policy);
        config.stream_proxy_mode = settings.stream_proxy_mode;

        let old = self.state.lock().coordinator.take();
        if let Some(old) = old {
            old.stop_stream().await;
        }
        if !self.is_current_start(generation) {
            return;
        }
        let coordinator = StreamCoordinator::new(config, req.playback_key_access.clone());
        let session = {
            let mut s = self.state.lock();
            s.session_id = s.session_id.wrapping_add(1);
            s.coordinator = Some(coordinator.clone());
            s.rewind_settings = RewindSettings::from_settings(&settings);
            Self::apply_timeshift_window(&mut s, 0.0);
            s.rewind_seek_hint = None;
            s.session_id
        };
        coordinator
            .set_server_time_machine_stream(time_machine)
            .await;
        let rewind_config = self.state.read().rewind_recording_config();
        coordinator.set_timeshift_recording(rewind_config).await;
        if is_multi_live && precision {
            coordinator.set_low_latency_sync_active(true).await;
        }
        if let Some(check) = self.callbacks().on_check_stream_ended {
            coordinator.set_check_stream_ended_callback(check);
        }
        if let Some(manifest) = req.prefetched_manifest.clone() {
            coordinator.set_prefetched_manifest(manifest);
        }

        let engine = {
            let mut s = self.state.lock();
            let engine = match (&s.engine, s.is_preallocated) {
                (Some(e), true) => e.clone(),
                _ => {
                    let Some(factory) = &self.engine_factory else {
                        drop(s);
                        self.fail_start(&coordinator, "재생 엔진이 없습니다".into())
                            .await;
                        return;
                    };
                    let e = factory();
                    s.engine = Some(e.clone());
                    e
                }
            };
            s.current_engine_type = PlayerEngineType::Hlsjs;
            engine.set_volume(if s.is_muted { 0.0 } else { s.volume });
            engine
        };

        if engine.is_hlsjs() {
            engine.set_streaming_profile(
                LatencyPresetPairing::pairing(req.player_settings.as_ref()).profile,
            );
            engine.set_match_web_v2_config(match_web_v2.then(|| {
                HlsJsMatchWebV2Config::new(
                    cview_core::models::match_web_v2::MatchWebV2Flag::resolve_ingest_target(None),
                )
            }));
            engine.set_match_web_v2_seek_priority(if !is_multi_live || precision { 1 } else { 0 });
            apply_quality_intent(engine.as_ref(), &policy, is_multi_live, multi_target);
            let tag: String = req.channel_id.chars().take(6).collect();
            engine.set_parity_tag(&tag);
        }
        coordinator.set_player_engine(upcast(&engine)).await;

        self.start_event_listening(&coordinator, session);
        self.start_configured_coordinator(
            coordinator,
            req.stream_url.clone(),
            rewind_config.is_some(),
            generation,
        )
        .await;
    }

    async fn fail_start(&self, coordinator: &StreamCoordinator, message: String) {
        coordinator.stop_stream().await;
        {
            let mut s = self.state.lock();
            cancel_slot(&mut s.event_task);
            s.coordinator = None;
            let msg = format!("스트림 시작 실패: {message}");
            s.error_message = Some(msg.clone());
            s.stream_phase = StreamPhase::Error(msg);
        }
        self.notify_playback_state_changed();
    }

    async fn start_configured_coordinator(
        &self,
        coordinator: StreamCoordinator,
        url: Url,
        records_rewind: bool,
        generation: u64,
    ) {
        let result = coordinator.start_stream(url).await;
        let route = coordinator.proxy_route_status();
        let still_current =
            self.is_current_session(&coordinator) && self.is_current_start(generation);
        match result {
            Ok(()) => {
                if !still_current {
                    coordinator.stop_stream().await;
                    return;
                }
                Self::apply_proxy_route_status(&mut self.state.lock(), route);
                self.start_uptime_timer(coordinator.clone());
                if records_rewind {
                    self.start_rewind_window_updates(coordinator);
                }
            }
            Err(e) => {
                if !still_current {
                    coordinator.stop_stream().await;
                    return;
                }
                Self::apply_proxy_route_status(&mut self.state.lock(), route);
                tracing::error!("스트림 시작 실패: {e}");
                self.fail_start(&coordinator, e.to_string()).await;
                // 실패한 세션의 프록시 상태는 남겨 진단에 쓴다.
                Self::apply_proxy_route_status(&mut self.state.lock(), route);
            }
        }
    }

    fn is_current_session(&self, coordinator: &StreamCoordinator) -> bool {
        // 코디네이터는 Arc 핸들이다 — 같은 세션인지는 우리가 넣은 핸들의 설정 채널로 확인한다.
        self.state
            .read()
            .coordinator
            .as_ref()
            .is_some_and(|c| std::ptr::eq(c.configuration(), coordinator.configuration()))
    }

    /// 정지 — 시작 중이면 끊고, 녹화·타이머를 거두고, 엔진을 멈춘다.
    pub async fn stop_stream(&self) {
        {
            let mut s = self.state.lock();
            s.start_generation = s.start_generation.wrapping_add(1);
        }
        let _guard = self.start_lock.lock().await;
        if self.state.read().is_recording {
            self.stop_recording().await;
        }
        let coordinator = {
            let mut s = self.state.lock();
            cancel_slot(&mut s.recording_timer);
            cancel_slot(&mut s.uptime_task);
            cancel_slot(&mut s.event_task);
            cancel_slot(&mut s.rewind_window_task);
            cancel_slot(&mut s.rewind_release_task);
            Self::apply_timeshift_window(&mut s, 0.0);
            cancel_slot(&mut s.control_hide_task);
            cancel_slot(&mut s.buffering_debounce);
            s.last_playing_time = None;
            s.session_id = s.session_id.wrapping_add(1);
            s.coordinator.take()
        };
        if let Some(c) = coordinator {
            c.stop_stream().await;
        }
        {
            let mut s = self.state.lock();
            if s.is_preallocated {
                if let Some(e) = &s.engine {
                    e.stop();
                }
            } else if let Some(e) = s.engine.take() {
                e.stop();
            }
            Self::apply_proxy_route_status(&mut s, None);
            s.uptime = 0.0;
            s.stream_phase = StreamPhase::Idle;
            s.current_quality = None;
            s.available_qualities.clear();
            s.multi_live_source_qualities.clear();
            s.latency_info = None;
            s.latency_history.clear();
            s.latency_sample_sequence = 0;
            s.buffer_health = None;
            s.playback_rate = 1.0;
            s.is_cheat_key_user = false;
            s.is_time_machine_available = false;
            s.is_time_shifting = false;
        }
        self.notify_playback_state_changed();
    }

    pub async fn toggle_play_pause(&self) {
        let (coordinator, phase) = {
            let s = self.state.read();
            (s.coordinator.clone(), s.stream_phase.clone())
        };
        let Some(c) = coordinator else {
            return;
        };
        match phase {
            StreamPhase::Playing => c.pause().await,
            StreamPhase::Paused => c.resume().await,
            _ => {}
        }
    }

    // MARK: 볼륨

    pub fn set_volume(&self, volume: f32) {
        if !volume.is_finite() {
            return;
        }
        let clamped = volume.clamp(0.0, 1.0);
        {
            let mut s = self.state.lock();
            s.has_initialized_session_volume = true;
            if s.volume == clamped {
                return;
            }
            s.volume = clamped;
            if let Some(e) = &s.engine {
                e.set_volume(if s.is_muted { 0.0 } else { clamped });
            }
        }
        if let Some(cb) = self.callbacks().on_volume_changed {
            cb(clamped);
        }
    }

    pub fn toggle_mute(&self) {
        let muted = self.state.read().is_muted;
        self.set_muted(!muted);
    }

    pub fn set_muted(&self, muted: bool) {
        {
            let mut s = self.state.lock();
            if s.is_muted == muted {
                return;
            }
            s.is_muted = muted;
            if let Some(e) = &s.engine {
                e.set_volume(if muted { 0.0 } else { s.volume });
            }
        }
        if let Some(cb) = self.callbacks().on_mute_changed {
            cb(muted);
        }
    }

    // MARK: 화질·배속·프록시

    pub async fn switch_quality(&self, quality: &StreamQualityInfo) {
        let coordinator = {
            let mut s = self.state.lock();
            if s.is_multi_live {
                let allowed =
                    selectable_qualities(&s.available_qualities, true, s.multi_live_quality_target);
                if allowed.first() == Some(quality) {
                    s.current_quality = Some(quality.clone());
                } else {
                    tracing::info!("멀티라이브 수동 화질 전환 차단: {}", quality.name);
                }
                return;
            }
            let Some(c) = s.coordinator.clone() else {
                return;
            };
            s.error_message = None;
            c
        };
        let result = coordinator
            .switch_quality_by_bandwidth(quality.bandwidth)
            .await;
        let mut s = self.state.lock();
        match result {
            Ok(()) => s.current_quality = Some(quality.clone()),
            Err(e) => s.error_message = Some(format!("품질 전환 실패: {e}")),
        }
    }

    pub fn set_audio_only(&self, enabled: bool) {
        let mut s = self.state.lock();
        if s.is_audio_only != enabled {
            s.is_audio_only = enabled;
        }
    }

    pub fn set_playback_rate(&self, rate: f64) {
        let mut s = self.state.lock();
        s.playback_rate = rate;
        if let Some(e) = &s.engine {
            e.set_rate(rate as f32);
        }
    }

    pub fn toggle_fullscreen(&self) {
        let mut s = self.state.lock();
        s.is_fullscreen = !s.is_fullscreen;
    }

    /// 컨트롤을 보이고 3초 뒤 숨긴다.
    pub fn show_controls_temporarily(&self) {
        let state = self.state.clone();
        let mut s = self.state.lock();
        s.show_controls = true;
        cancel_slot(&mut s.control_hide_task);
        s.control_hide_task = Some(TaskSlot::spawn(move |id| async move {
            tokio::time::sleep(Duration::from_secs(3)).await;
            let mut s = state.lock();
            if slot_is(&s.control_hide_task, id) {
                s.show_controls = false;
                s.control_hide_task = None;
            }
        }));
    }

    pub async fn apply_stream_proxy_mode(&self, mode: StreamProxyMode) {
        let Some(coordinator) = self.state.read().coordinator.clone() else {
            return;
        };
        coordinator.apply_stream_proxy_mode(mode).await;
        let status = coordinator.proxy_route_status();
        if !self.is_current_session(&coordinator) {
            return;
        }
        let mut s = self.state.lock();
        Self::apply_proxy_route_status(&mut s, status);
        if let Some(e) = &s.engine {
            e.set_volume(if s.is_muted { 0.0 } else { s.volume });
        }
    }

    fn apply_proxy_route_status(s: &mut PlayerState, status: Option<StreamProxyRouteStatus>) {
        s.proxy_route_status = status;
        s.current_proxy_mode = match status {
            None => "pending".into(),
            Some(st) => st
                .effective
                .map(|m| m.raw_value().to_string())
                .unwrap_or_else(|| "unavailable".into()),
        };
    }

    pub fn current_time(&self) -> f64 {
        self.state
            .read()
            .engine
            .as_ref()
            .map(|e| e.current_time())
            .unwrap_or(0.0)
    }

    pub fn duration(&self) -> f64 {
        self.state
            .read()
            .engine
            .as_ref()
            .map(|e| e.duration())
            .unwrap_or(0.0)
    }

    pub fn seek(&self, position: f64) {
        if let Some(e) = &self.state.read().engine {
            e.seek(position);
        }
    }

    // MARK: 이벤트

    fn start_uptime_timer(&self, coordinator: StreamCoordinator) {
        let state = self.state.clone();
        let mut s = self.state.lock();
        let session = s.session_id;
        cancel_slot(&mut s.uptime_task);
        s.uptime_task = Some(TaskSlot::spawn(move |id| async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                let next = coordinator.uptime();
                let mut s = state.lock();
                if !slot_is(&s.uptime_task, id) || s.session_id != session {
                    return;
                }
                if s.uptime as i64 != next as i64 {
                    s.uptime = next;
                }
            }
        }));
    }

    fn start_event_listening(&self, coordinator: &StreamCoordinator, session: u64) {
        let mut rx = coordinator.events();
        let me = self.clone();
        let mut s = self.state.lock();
        cancel_slot(&mut s.event_task);
        s.event_task = Some(TaskSlot::spawn(move |id| async move {
            while let Some(event) = rx.recv().await {
                {
                    let s = me.state.read();
                    if !slot_is(&s.event_task, id) || s.session_id != session {
                        return;
                    }
                }
                me.handle_stream_event(event);
            }
        }));
    }

    fn handle_stream_phase(&self, phase: StreamPhase) {
        {
            let state = self.state.clone();
            let mut s = self.state.lock();
            match &phase {
                StreamPhase::Buffering => {
                    if s.stream_phase == StreamPhase::Playing {
                        let recently_playing = s
                            .last_playing_time
                            .is_some_and(|t| t.elapsed() < Duration::from_millis(1_500));
                        if !recently_playing {
                            cancel_slot(&mut s.buffering_debounce);
                            s.buffering_debounce = Some(TaskSlot::spawn(move |id| async move {
                                tokio::time::sleep(Duration::from_secs(1)).await;
                                let mut s = state.lock();
                                if slot_is(&s.buffering_debounce, id) {
                                    s.buffering_started_at.get_or_insert_with(Instant::now);
                                    s.stream_phase = StreamPhase::Buffering;
                                    s.buffering_debounce = None;
                                }
                            }));
                        }
                    } else if s.stream_phase != StreamPhase::Buffering {
                        s.buffering_started_at.get_or_insert_with(Instant::now);
                        s.stream_phase = StreamPhase::Buffering;
                    }
                }
                StreamPhase::Playing => {
                    cancel_slot(&mut s.buffering_debounce);
                    s.last_playing_time = Some(Instant::now());
                    Self::end_buffering_event(&mut s);
                    if s.stream_phase != StreamPhase::Playing {
                        s.stream_phase = StreamPhase::Playing;
                    }
                    if s.error_message.is_some() {
                        s.error_message = None;
                    }
                }
                other => {
                    if &s.stream_phase != other {
                        s.stream_phase = other.clone();
                    }
                }
            }
            if let StreamPhase::Error(msg) = &phase {
                if s.error_message.as_ref() != Some(msg) {
                    s.error_message = Some(msg.clone());
                }
            }
        }
        self.notify_playback_state_changed();
    }

    fn end_buffering_event(s: &mut PlayerState) {
        if let Some(started) = s.buffering_started_at.take() {
            tracing::debug!(
                "buffering.end ch={} duration_ms={}",
                s.current_channel_id.as_deref().unwrap_or("-"),
                started.elapsed().as_millis()
            );
        }
    }

    /// 코디네이터 이벤트를 관찰 상태로 옮긴다. 같은 값이면 쓰지 않는다.
    pub fn handle_stream_event(&self, event: StreamEvent) {
        match event {
            StreamEvent::PhaseChanged(phase) => self.handle_stream_phase(phase),
            StreamEvent::ProxyRouteChanged(status) => {
                Self::apply_proxy_route_status(&mut self.state.lock(), Some(status));
            }
            StreamEvent::QualitySelected(q) => {
                let mut s = self.state.lock();
                if s.current_quality.as_ref() != Some(&q) {
                    s.current_quality = Some(q);
                }
                if let Some(c) = s.coordinator.clone() {
                    let qualities = c.available_qualities();
                    Self::update_multi_live_quality_options(&mut s, qualities);
                }
            }
            StreamEvent::QualityChanged(q) => {
                let s = self.state.read();
                if s.current_quality.as_ref() != Some(&q) {
                    drop(s);
                    self.state.lock().current_quality = Some(q);
                }
            }
            StreamEvent::LatencyUpdate(info) => {
                let mut s = self.state.lock();
                if s.latency_info != Some(info) {
                    s.latency_info = Some(info);
                }
                s.latency_sample_sequence += 1;
                let n = s.latency_sample_sequence;
                if n == 1 || n.is_multiple_of(10) {
                    s.latency_history.push((Utc::now(), info.current));
                    if s.latency_history.len() > MAX_LATENCY_HISTORY {
                        s.latency_history.remove(0);
                    }
                }
            }
            StreamEvent::BufferUpdate(health) => {
                let s = self.state.read();
                if s.buffer_health != Some(health) {
                    drop(s);
                    self.state.lock().buffer_health = Some(health);
                }
            }
            StreamEvent::Error(msg) => {
                let s = self.state.read();
                if s.error_message.as_ref() != Some(&msg) {
                    drop(s);
                    self.state.lock().error_message = Some(msg);
                }
            }
            StreamEvent::StreamEnded => {
                let mut s = self.state.lock();
                cancel_slot(&mut s.buffering_debounce);
                Self::end_buffering_event(&mut s);
                if s.stream_phase != StreamPhase::StreamEnded {
                    s.stream_phase = StreamPhase::StreamEnded;
                }
            }
            StreamEvent::Stopped => {
                let mut s = self.state.lock();
                Self::apply_proxy_route_status(&mut s, None);
                Self::end_buffering_event(&mut s);
                if s.stream_phase != StreamPhase::Idle {
                    s.stream_phase = StreamPhase::Idle;
                }
            }
            StreamEvent::TimeShiftEnded => {
                let s = self.state.read();
                if s.is_time_shifting {
                    drop(s);
                    self.state.lock().is_time_shifting = false;
                }
            }
        }
    }

    // MARK: 엔진 콜백 (hls.js → 코디네이터)

    fn current_coordinator(&self) -> Option<StreamCoordinator> {
        self.state.read().coordinator.clone()
    }

    /// `onStateChange`.
    pub async fn engine_state_changed(&self, phase: &PlayerPhase) {
        let Some(c) = self.current_coordinator() else {
            return;
        };
        match phase {
            PlayerPhase::Playing => {
                c.handle_playback_engine_state(phase);
                c.report_engine_playing().await;
            }
            PlayerPhase::Error(_) => {
                c.handle_recovery_signal(RecoverySignal::EngineError {
                    engine: PlayerEngineType::Hlsjs,
                    detail: "HLS.js 재생 오류".into(),
                })
                .await;
            }
            _ => {}
        }
    }

    /// `onPlaybackStalled`.
    pub async fn engine_playback_stalled(&self) {
        if let Some(c) = self.current_coordinator() {
            c.handle_recovery_signal(RecoverySignal::PlaybackStall {
                source: "HLS.js playback".into(),
            })
            .await;
        }
    }

    /// `onRebufferStateChanged`.
    pub fn engine_rebuffer_changed(&self, stalled: bool) {
        if let Some(c) = self.current_coordinator() {
            c.handle_playback_engine_state(&if stalled {
                PlayerPhase::Buffering { progress: 0.0 }
            } else {
                PlayerPhase::Playing
            });
        }
    }

    /// `onMatchWebV2Fallback`.
    pub async fn engine_match_web_v2_fallback(&self, reason: &str) {
        let (c, e) = {
            let s = self.state.read();
            (s.coordinator.clone(), s.engine.clone())
        };
        if let (Some(c), Some(e)) = (c, e) {
            c.handle_match_web_v2_fallback(reason, &upcast(&e)).await;
        }
    }

    /// `onSurfacePresented`.
    pub fn engine_surface_presented(&self) {
        self.note_presented_frame();
    }

    // MARK: 녹화

    /// 녹화 시작. 경로를 주지 않으면 `<영상 폴더>/CView/<채널>_<시각>.ts`.
    pub async fn start_recording(&self, path: Option<PathBuf>) {
        let (engine, channel) = {
            let s = self.state.read();
            if s.is_recording {
                return;
            }
            let Some(e) = s.engine.clone() else {
                return;
            };
            (e, s.channel_name.clone())
        };
        let path = path.unwrap_or_else(|| {
            cview_player::recording::StreamRecordingService::default_recording_path(
                dirs_movies().as_deref(),
                &channel,
                chrono::Local::now(),
            )
        });
        self.state.lock().recording_path = Some(path.clone());
        match engine.start_recording(&path).await {
            Ok(()) => {
                {
                    let mut s = self.state.lock();
                    if let Some(actual) = engine.recording_destination() {
                        s.recording_path = Some(actual);
                    }
                    s.is_recording = true;
                    s.recording_duration = 0.0;
                }
                self.start_recording_timer();
            }
            Err(e) => {
                let reason = e.to_string();
                tracing::error!("녹화 시작 실패: {reason}");
                self.set_recording_status(
                    PlayerToolStatusKind::Failure,
                    format!("녹화 시작 실패 · {reason}"),
                );
            }
        }
    }

    pub async fn stop_recording(&self) {
        let engine = {
            let s = self.state.read();
            if !s.is_recording {
                return;
            }
            let Some(e) = s.engine.clone() else {
                return;
            };
            e
        };
        engine.stop_recording().await;
        let path = {
            let mut s = self.state.lock();
            s.is_recording = false;
            cancel_slot(&mut s.recording_timer);
            s.recording_path.clone()
        };
        if let Some(reason) = engine.recording_failure_reason() {
            self.set_recording_status(
                PlayerToolStatusKind::Failure,
                format!("녹화 저장 실패 · {reason}"),
            );
        } else if let Some(path) = path {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.set_recording_status(
                PlayerToolStatusKind::Success,
                format!("녹화 저장됨 · {name}"),
            );
        }
    }

    /// 엔진의 녹화 오류 이벤트.
    pub fn engine_recording_failed(&self, reason: &str) {
        let was_recording = {
            let mut s = self.state.lock();
            let was = s.is_recording;
            if was {
                s.is_recording = false;
                cancel_slot(&mut s.recording_timer);
            }
            was
        };
        let message = if was_recording {
            format!("녹화 중단됨 · {reason}")
        } else {
            format!("녹화 실패 · {reason}")
        };
        self.set_recording_status(PlayerToolStatusKind::Failure, message);
    }

    pub fn set_recording_status(&self, kind: PlayerToolStatusKind, message: String) {
        let state = self.state.clone();
        let mut s = self.state.lock();
        s.recording_status = Some(PlayerToolStatus::new(kind, message, PlayerTool::Recording));
        cancel_slot(&mut s.recording_status_reset);
        let secs = if kind == PlayerToolStatusKind::Failure {
            6
        } else {
            3
        };
        s.recording_status_reset = Some(TaskSlot::spawn(move |id| async move {
            tokio::time::sleep(Duration::from_secs(secs)).await;
            let mut s = state.lock();
            if slot_is(&s.recording_status_reset, id) {
                s.recording_status = None;
                s.recording_status_reset = None;
            }
        }));
    }

    fn start_recording_timer(&self) {
        let state = self.state.clone();
        let mut s = self.state.lock();
        cancel_slot(&mut s.recording_timer);
        let start = Instant::now();
        s.recording_timer = Some(TaskSlot::spawn(move |id| async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let mut s = state.lock();
                if !slot_is(&s.recording_timer, id) {
                    return;
                }
                s.recording_duration = start.elapsed().as_secs_f64();
            }
        }));
    }

    pub fn formatted_recording_duration(&self) -> String {
        format_time_interval(self.state.read().recording_duration)
    }

    // MARK: 스크린샷

    /// 현재 프레임을 캡처해 설정 폴더에 저장한다. 결과는 `screenshot_status` 로 알린다.
    pub async fn take_screenshot(&self) {
        let (engine, format, dir, base) = {
            let s = self.state.read();
            let Some(engine) = s.engine.clone() else {
                drop(s);
                self.set_screenshot_status(
                    PlayerToolStatusKind::Failure,
                    "재생 중인 스트림이 없습니다".into(),
                );
                return;
            };
            if !engine
                .capabilities()
                .contains(PlayerEngineCapabilities::SCREENSHOT)
            {
                let name = s.current_engine_type.display_name();
                drop(s);
                self.set_screenshot_status(
                    PlayerToolStatusKind::Failure,
                    format!("{name} 엔진은 화면 캡처를 지원하지 않습니다"),
                );
                return;
            }
            (
                engine,
                s.screenshot_format,
                helpers::expand_tilde(&s.screenshot_save_path),
                screenshot_file_base_name(&s.channel_name, Utc::now().timestamp()),
            )
        };
        let result = match engine.capture_screenshot(format).await {
            Ok(frame) => {
                let written = tokio::task::spawn_blocking({
                    let frame = frame.clone();
                    move || write_screenshot(&frame, &dir, &base)
                })
                .await;
                match written {
                    Ok(Ok(dest)) => Ok((frame, dest)),
                    Ok(Err(e)) => Err(e.to_string()),
                    Err(e) => Err(e.to_string()),
                }
            }
            Err(e) => Err(e.to_string()),
        };
        match result {
            Ok((frame, dest)) => {
                let name = dest
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.set_screenshot_status(
                    PlayerToolStatusKind::Success,
                    format!(
                        "저장됨 · {}×{} · {name}",
                        frame.pixel_width, frame.pixel_height
                    ),
                );
            }
            Err(reason) => {
                tracing::error!("스크린샷 실패: {reason}");
                self.set_screenshot_status(PlayerToolStatusKind::Failure, reason);
            }
        }
    }

    pub fn set_screenshot_status(&self, kind: PlayerToolStatusKind, message: String) {
        let state = self.state.clone();
        let mut s = self.state.lock();
        s.screenshot_status = Some(PlayerToolStatus::new(kind, message, PlayerTool::Screenshot));
        cancel_slot(&mut s.screenshot_status_reset);
        let secs = if kind == PlayerToolStatusKind::Failure {
            6
        } else {
            3
        };
        s.screenshot_status_reset = Some(TaskSlot::spawn(move |id| async move {
            tokio::time::sleep(Duration::from_secs(secs)).await;
            let mut s = state.lock();
            if slot_is(&s.screenshot_status_reset, id) {
                s.screenshot_status = None;
                s.screenshot_status_reset = None;
            }
        }));
    }

    // MARK: 오디오·비디오 보정

    pub fn equalizer_presets(&self) -> Vec<&'static str> {
        if self.state.read().can_use_equalizer() {
            EqualizerPreset::names()
        } else {
            Vec::new()
        }
    }

    pub fn equalizer_frequencies(&self) -> Vec<f32> {
        if self.state.read().can_use_equalizer() {
            EqualizerPreset::FREQUENCIES.to_vec()
        } else {
            Vec::new()
        }
    }

    pub fn apply_equalizer_preset(&self, name: &str) {
        let Some(preset) = EqualizerPreset::preset_named(name) else {
            return;
        };
        let mut s = self.state.lock();
        let Some(engine) = s.engine.clone().filter(|e| e.is_hlsjs()) else {
            return;
        };
        s.equalizer_preset_name = name.to_string();
        s.is_equalizer_enabled = true;
        s.equalizer_pre_amp = preset.pre_amp;
        s.equalizer_bands = preset.gains.to_vec();
        engine.set_equalizer_enabled(true, &preset.gains, preset.pre_amp);
    }

    fn enable_equalizer_graph(s: &mut PlayerState, engine: &SharedLiveEngine) {
        if s.equalizer_bands.is_empty() {
            s.equalizer_bands = vec![0.0; EqualizerPreset::FREQUENCIES.len()];
        }
        s.is_equalizer_enabled = true;
        engine.set_equalizer_enabled(true, &s.equalizer_bands, s.equalizer_pre_amp);
    }

    fn refresh_preset_name(s: &mut PlayerState) {
        s.equalizer_preset_name =
            EqualizerPreset::matching(&s.equalizer_bands, s.equalizer_pre_amp)
                .map(|p| p.name.to_string())
                .unwrap_or_default();
    }

    pub fn set_equalizer_pre_amp(&self, value: f32) {
        let mut s = self.state.lock();
        s.equalizer_pre_amp = value;
        let Some(engine) = s.engine.clone().filter(|e| e.is_hlsjs()) else {
            return;
        };
        if !s.is_equalizer_enabled || s.equalizer_bands.is_empty() {
            Self::enable_equalizer_graph(&mut s, &engine);
        }
        engine.set_equalizer_pre_amp(value);
        Self::refresh_preset_name(&mut s);
    }

    pub fn set_equalizer_band(&self, index: usize, value: f32) {
        let mut s = self.state.lock();
        let had_no_graph = s.equalizer_bands.is_empty();
        if had_no_graph {
            s.equalizer_bands = vec![0.0; EqualizerPreset::FREQUENCIES.len()];
        }
        if index >= s.equalizer_bands.len() {
            return;
        }
        s.equalizer_bands[index] = value;
        let Some(engine) = s.engine.clone().filter(|e| e.is_hlsjs()) else {
            return;
        };
        if !s.is_equalizer_enabled || had_no_graph {
            Self::enable_equalizer_graph(&mut s, &engine);
        }
        engine.set_equalizer_band(index, value);
        Self::refresh_preset_name(&mut s);
    }

    pub fn disable_equalizer(&self) {
        let mut s = self.state.lock();
        s.is_equalizer_enabled = false;
        s.equalizer_preset_name.clear();
        s.equalizer_pre_amp = 0.0;
        s.equalizer_bands.clear();
        if let Some(e) = s.engine.as_ref().filter(|e| e.is_hlsjs()) {
            e.set_equalizer_enabled(false, &[], 0.0);
        }
    }

    fn push_video_adjust(s: &PlayerState) {
        let Some(engine) = s.engine.as_ref().filter(|e| e.is_hlsjs()) else {
            return;
        };
        engine.apply_video_adjust(VideoAdjust {
            enabled: s.is_video_adjust_enabled,
            brightness: s.video_brightness as f64,
            contrast: s.video_contrast as f64,
            saturation: s.video_saturation as f64,
            hue: s.video_hue as f64,
            gamma: s.video_gamma as f64,
            sharp_pixel: engine.sharp_pixel_scaling_enabled(),
        });
    }

    pub fn set_video_adjust_enabled(&self, enabled: bool) {
        let mut s = self.state.lock();
        s.is_video_adjust_enabled = enabled;
        Self::push_video_adjust(&s);
    }

    pub fn set_video_brightness(&self, v: f32) {
        let mut s = self.state.lock();
        s.video_brightness = v;
        Self::push_video_adjust(&s);
    }

    pub fn set_video_contrast(&self, v: f32) {
        let mut s = self.state.lock();
        s.video_contrast = v;
        Self::push_video_adjust(&s);
    }

    pub fn set_video_saturation(&self, v: f32) {
        let mut s = self.state.lock();
        s.video_saturation = v;
        Self::push_video_adjust(&s);
    }

    pub fn set_video_hue(&self, v: f32) {
        let mut s = self.state.lock();
        s.video_hue = v;
        Self::push_video_adjust(&s);
    }

    pub fn set_video_gamma(&self, v: f32) {
        let mut s = self.state.lock();
        s.video_gamma = v;
        Self::push_video_adjust(&s);
    }

    pub fn reset_video_adjust(&self) {
        let mut s = self.state.lock();
        s.is_video_adjust_enabled = false;
        s.video_brightness = 1.0;
        s.video_contrast = 1.0;
        s.video_saturation = 1.0;
        s.video_hue = 0.0;
        s.video_gamma = 1.0;
        Self::push_video_adjust(&s);
    }

    pub fn set_aspect_ratio(&self, ratio: Option<String>) {
        self.state.lock().aspect_ratio = ratio;
    }

    /// 고급 설정(화면비·스크린샷·비디오 보정·EQ) 복원.
    pub fn apply_advanced_settings(&self, settings: &PlayerSettings) {
        let preset_to_apply = {
            let mut s = self.state.lock();
            s.aspect_ratio = settings.aspect_ratio.clone();
            s.screenshot_save_path = settings.screenshot_path.clone();
            s.screenshot_format = settings.screenshot_format;
            s.is_video_adjust_enabled = settings.video_adjust_enabled;
            s.video_brightness = settings.video_brightness;
            s.video_contrast = settings.video_contrast;
            s.video_saturation = settings.video_saturation;
            s.video_hue = settings.video_hue;
            s.video_gamma = settings.video_gamma;

            let has_stored =
                !settings.equalizer_bands.is_empty() || settings.equalizer_pre_amp != 0.0;
            let stored_bands = equalizer_bands_fitting_frequencies(&settings.equalizer_bands);
            if has_stored {
                s.is_equalizer_enabled = true;
                s.equalizer_pre_amp = settings.equalizer_pre_amp;
                s.equalizer_bands = stored_bands.clone();
                Self::refresh_preset_name(&mut s);
            }
            let Some(engine) = s.engine.clone().filter(|e| e.is_hlsjs()) else {
                return;
            };
            engine.apply_video_adjust(VideoAdjust {
                enabled: settings.video_adjust_enabled,
                brightness: settings.video_brightness as f64,
                contrast: settings.video_contrast as f64,
                saturation: settings.video_saturation as f64,
                hue: settings.video_hue as f64,
                gamma: settings.video_gamma as f64,
                sharp_pixel: settings.sharp_pixel_scaling,
            });
            if has_stored {
                engine.set_equalizer_enabled(true, &stored_bands, settings.equalizer_pre_amp);
                None
            } else {
                settings.equalizer_preset.clone().filter(|p| !p.is_empty())
            }
        };
        if let Some(name) = preset_to_apply {
            self.apply_equalizer_preset(&name);
        }
    }
}

/// `~/Movies` (macOS) 또는 `$XDG_VIDEOS_DIR`/`~/Videos`.
fn dirs_movies() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    if cfg!(target_os = "macos") {
        return Some(home.join("Movies"));
    }
    if let Some(x) = std::env::var_os("XDG_VIDEOS_DIR") {
        return Some(PathBuf::from(x));
    }
    Some(home.join("Videos"))
}
