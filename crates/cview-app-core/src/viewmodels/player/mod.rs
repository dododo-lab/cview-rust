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
//!
//! Swift 확장 파일처럼 주제별 하위 모듈로 나눴다: 스트림 시작/정지(stream) · 이벤트·엔진 콜백(events) ·
//! 라이브 복귀·되감기(rewind) · 녹화(recording) · 스크린샷(screenshot) · 오디오·비디오 보정(adjust).
//! 이 파일에는 상태·스냅샷 정의와 엔진 주입·설정·볼륨·화질 같은 작은 조작이 남는다.

mod adjust;
pub mod engine;
mod events;
pub mod helpers;
mod recording;
mod rewind;
mod screenshot;
mod stream;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Utc;
use futures_util::future::BoxFuture;
use serde::Serialize;
use tokio::sync::watch;
use url::Url;

use cview_core::models::player::{PlayerEngineType, PlayerToolStatus};
use cview_core::models::settings::{PlayerSettings, ScreenshotFormat};
use cview_core::models::stream_proxy::{StreamProxyMode, StreamProxyRouteStatus};
use cview_core::protocols::PlayerEngineCapabilities;
use cview_player::coordinator::{
    BufferHealth, LatencyInfo, SharedEngine, StreamCoordinator, StreamEvent, StreamPhase,
    StreamQualityInfo, TimeshiftRecordingConfig,
};
use cview_player::hlsjs::HlsJsForcedQualityTarget;
use cview_player::manifest::parser::MasterPlaylist;
use cview_player::policy::occlusion_cap::should_skip_background_resync;
use cview_player::policy::{LatencyPresetPairing, PlaybackKeyAccess};

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

/// 코디네이터 이벤트 탭.
pub type StreamEventTap = Arc<dyn Fn(&StreamEvent) + Send + Sync>;

/// 사용자 이벤트 콜백.
#[derive(Default, Clone)]
pub struct PlayerCallbacks {
    pub on_playback_state_changed: Option<Arc<dyn Fn() + Send + Sync>>,
    pub on_check_stream_ended: Option<Arc<dyn Fn() -> BoxFuture<'static, bool> + Send + Sync>>,
    pub on_volume_changed: Option<Arc<dyn Fn(f32) + Send + Sync>>,
    pub on_mute_changed: Option<Arc<dyn Fn(bool) + Send + Sync>>,
    /// 코디네이터 이벤트 탭 — 뷰모델이 처리하기 직전에 현재 세션 이벤트를 그대로 넘긴다
    /// (코디네이터 이벤트 채널은 구독자가 하나뿐이라 셸이 따로 받을 수 없다).
    pub on_stream_event: Option<StreamEventTap>,
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
    /// 멀티라이브 메인 패인의 목표 지연 여유(초) — 매니저가 패인 수로 정한다. 새 코디네이터에도 이어 준다.
    pub latency_margin: f64,
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
            latency_margin: 0.0,
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

    /// 멀티라이브 패인으로 쓴다 — 다음 재생 시작부터 저지연을 매니저가 관리(Managed: 선택 패인만 정밀 싱크,
    /// 비선택은 안정성 목표 10.5초)하고, 화질은 역할 목표(매니저의 `set_multi_live_quality_target`)를 따르며,
    /// 로컬 되감기 녹화는 기본으로 끈다. 단독 시청 플레이어는 그대로(false).
    pub fn set_multi_live(&self, on: bool) {
        self.state.lock().is_multi_live = on;
    }

    /// 멀티라이브 메인 패인 목표 지연 여유(초). 같은 값이면 아무것도 하지 않는다.
    pub fn set_latency_margin(&self, seconds: f64) {
        let coordinator = {
            let mut s = self.state.lock();
            if (s.latency_margin - seconds).abs() < 0.01 {
                return;
            }
            s.latency_margin = seconds;
            s.coordinator.clone()
        };
        if let Some(c) = coordinator {
            c.set_target_latency_margin(seconds);
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
}

impl PlayerViewModel {
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
