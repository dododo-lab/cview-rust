//! 멀티라이브 매니저·세션이 기대는 외부 표면.
//!
//! 원본에는 대응 파일이 없다. Swift 는 `HLSJSPlayerEngine`·`PlayerViewModel`·`ChatViewModel`·
//! `MultiLiveEnginePool`·`ChzzkAPIClient`·`AppState`·`PiPController` 를 직접 잡았다.
//! 여기서는 매니저/세션 로직이 실제로 쓰는 메서드만 트레이트로 두고, 셸(Tauri)이나 테스트
//! 가짜가 구현한다.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::future::BoxFuture;
use uuid::Uuid;

use cview_core::models::settings::PlayerSettings;
use cview_core::models::settings_ui::MultiLiveSettings;
use cview_core::models::LiveInfo;
use cview_player::coordinator::StreamPhase;
use cview_player::hlsjs::network_metrics::StreamTransferSample;
use cview_player::hlsjs::profile::HlsJsForcedQualityTarget;
use cview_player::policy::SessionTier;

/// `HLSJSPlayerEngine` 중 멀티라이브가 직접 만지는 부분.
pub trait MultiLiveHlsEngine: Send + Sync {
    fn is_playing(&self) -> bool;
    fn is_suspend_intent_active(&self) -> bool;
    fn is_in_error_state(&self) -> bool;
    fn is_loading_suspended(&self) -> bool;
    fn is_playback_desired(&self) -> bool;
    fn buffer_ahead_seconds(&self) -> f64;
    fn current_bitrate(&self) -> f64;
    fn rate(&self) -> f64;
    fn transfers(&self) -> Vec<StreamTransferSample>;
    fn measured_segment_duration(&self) -> f64;
    fn force_highest_quality(&self) -> bool;
    fn forced_height_active(&self) -> bool;
    fn max_adaptive_height(&self) -> i64;
    fn max_adaptive_bitrate(&self) -> i64;
    fn set_max_resolution_height(&self, height: i64);
    fn set_max_bitrate(&self, kbps: i64);
    fn set_gpu_render_tier(&self, tier: SessionTier);
    /// `false` 는 `reason: .paneOffscreen`.
    fn set_video_surface_visible(&self, visible: bool);
    fn pause(&self);
    fn resume(&self);
    fn resume_loading(&self);
    fn suspend_loading(&self);
    fn set_buffer_caps(&self, max_len: f64, max_max_len: f64, max_size: i64);
    fn arm_surface_presented_probe(&self);
    fn reset_retries(&self) {}
}

pub type SharedHlsEngine = Arc<dyn MultiLiveHlsEngine>;

/// 스트림 종료 확인 프로브 (`onCheckStreamEnded`).
pub type StreamEndedProbe = Arc<dyn Fn() -> BoxFuture<'static, bool> + Send + Sync>;

/// `startStream` 요청.
#[derive(Debug, Clone, PartialEq)]
pub struct MultiLiveStreamRequest {
    pub channel_id: String,
    pub stream_url: String,
    pub channel_name: String,
    pub live_title: String,
    pub plan: LaunchPlan,
    pub match_web_v2_source_eligible: bool,
}

/// 세션의 `PlayerViewModel` 중 멀티라이브가 쓰는 부분.
#[async_trait]
pub trait MultiLivePlayer: Send + Sync {
    fn engine(&self) -> Option<SharedHlsEngine>;
    fn detach_engine(&self) -> Option<SharedHlsEngine>;
    fn is_muted(&self) -> bool;
    fn set_muted(&self, muted: bool);
    /// 세션 볼륨(음소거와 별개) — 여러 소리 모드의 보조 패인 볼륨에 쓴다.
    fn set_volume(&self, _volume: f32) {}
    fn stream_phase(&self) -> StreamPhase;
    fn presented_frame_tick(&self) -> u64;
    fn error_message(&self) -> Option<String>;
    fn set_precision_sync_active(&self, active: bool);
    fn rewind_recording_desired(&self) -> bool;
    fn set_rewind_recording_desired(&self, desired: bool);
    fn multi_live_quality_target(&self) -> HlsJsForcedQualityTarget;
    /// 바뀌었으면 `true`.
    fn set_multi_live_quality_target(&self, target: HlsJsForcedQualityTarget) -> bool;
    fn apply_picture_enhancement(&self, auto: bool, upscale: bool);
    fn recover_from_background(&self);
    fn promote_for_in_app_selection(&self);
    fn sync_network_activity(&self);
    fn resume_watch_heartbeat_for_menu_return(&self);
    fn suspend_watch_heartbeat_for_menu_leave(&self);
    fn restore_surface_after_occlusion(&self);
    fn set_gpu_render_tier_hidden(&self, keep_alive: bool);
    /// `streamCoordinator?.lowLatencyController?.setMaxRateOverride`.
    fn set_max_rate_override(&self, rate: Option<f64>);
    /// 메인 패인 목표 지연 여유(초) — [`crate::viewmodels::multi_live::policy::main_pane_latency_margin`].
    fn set_latency_margin(&self, _seconds: f64) {}
    fn configure_time_machine_access(
        &self,
        is_cheat_key_user: bool,
        is_time_machine_available: bool,
    );
    fn is_time_machine_available(&self) -> bool;
    /// `applySettings(ps, preserveSessionVolume: true)` + 샤프 픽셀 + 고급 설정.
    fn apply_started_settings(&self, settings: &PlayerSettings);
    fn set_on_playback_state_changed(&self, callback: Option<Arc<dyn Fn() + Send + Sync>>);
    fn set_stream_ended_probe(&self, probe: Option<StreamEndedProbe>);
    async fn start_stream(&self, request: MultiLiveStreamRequest);
    async fn stop_stream(&self);
    async fn toggle_play_pause(&self);
}

/// 세션 채팅 연결 상태 중 매니저가 보는 값.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatConnectionKind {
    Disconnected,
    Other,
}

/// 세션의 `ChatViewModel` 중 멀티라이브가 쓰는 부분.
#[async_trait]
pub trait MultiLiveChat: Send + Sync {
    fn has_foreground_presentation_lease(&self) -> bool {
        false
    }
    fn connection_state(&self) -> ChatConnectionKind;
    fn has_last_connection_config(&self) -> bool;
    fn set_background_mode(&self, background: bool);
    fn set_user(&self, uid: Option<String>, nickname: Option<String>);
    fn set_live_metadata(
        &self,
        viewer_count: Option<i64>,
        accumulate_count: Option<i64>,
        open_date: Option<DateTime<Utc>>,
    );
    /// 멀티챗 관리 해제 시 복원 — 병합 예산 해제·소유자 표시·버퍼 크기.
    fn restore_owner_presentation(&self, background: bool);
    async fn disconnect(&self);
    async fn reconnect(&self);
}

/// 엔진 풀 (`MultiLiveEnginePool`).
#[async_trait]
pub trait MultiLiveEnginePool: Send + Sync {
    async fn set_max_pool_size(&self, size: usize);
    async fn acquire(&self) -> Option<SharedHlsEngine>;
    async fn release(&self, engine: SharedHlsEngine);
    async fn discard(&self, engine: SharedHlsEngine);
    async fn release_slot(&self);
    async fn total_active_count(&self) -> usize;
    async fn trim_idle(&self, keep_per_type: usize);
    async fn drain(&self);
    async fn warmup(&self, count: usize);
}

/// 세션 플레이어·채팅 생성 (Swift `MultiLiveSession.init` 의 `PlayerViewModel()`·`ChatViewModel()`).
pub trait MultiLiveSessionFactory: Send + Sync {
    fn make_player(&self, engine: Option<SharedHlsEngine>) -> Arc<dyn MultiLivePlayer>;
    fn make_chat(&self) -> Arc<dyn MultiLiveChat>;
}

/// 재생 계획 (`PlaybackLaunchPlan` 중 세션이 읽는 값).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LaunchPlan {
    pub stream_url: String,
    pub is_audio_only: bool,
    pub channel_name: String,
    pub live_title: String,
    pub is_time_machine: bool,
}

/// `PlaybackLaunchPlanError`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchPlanError {
    MissingPlaybackJson,
    NoHlsUrl,
}

impl LaunchPlanError {
    pub fn message(self) -> &'static str {
        match self {
            Self::MissingPlaybackJson => "재생 정보를 찾을 수 없습니다.",
            Self::NoHlsUrl => "HLS 스트림 URL을 찾을 수 없습니다.",
        }
    }
}

/// 백엔드 호출 실패.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MultiLiveBackendError {
    /// Swift `CancellationError`.
    Cancelled,
    Message(String),
}

/// `ChannelHeaderHub.liveStatus` 결과 중 세션이 쓰는 값.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveStatusSnapshot {
    pub is_closed: bool,
    pub concurrent_user_count: i64,
    pub accumulate_count: i64,
    pub open_date: Option<DateTime<Utc>>,
}

/// 세션 메타데이터 적용 입력 (LiveInfo·LiveStatus 공통).
#[derive(Debug, Clone, PartialEq)]
pub struct LiveMetadata {
    pub concurrent_user_count: i64,
    pub accumulate_count: i64,
    pub open_date: Option<DateTime<Utc>>,
}

impl From<&LiveInfo> for LiveMetadata {
    fn from(i: &LiveInfo) -> Self {
        Self {
            concurrent_user_count: i.concurrent_user_count,
            accumulate_count: i.accumulate_count,
            open_date: i.open_date,
        }
    }
}

impl From<&LiveStatusSnapshot> for LiveMetadata {
    fn from(s: &LiveStatusSnapshot) -> Self {
        Self {
            concurrent_user_count: s.concurrent_user_count,
            accumulate_count: s.accumulate_count,
            open_date: s.open_date,
        }
    }
}

/// 시청 기록 시작 입력.
#[derive(Debug, Clone, PartialEq)]
pub struct WatchRecordStart {
    pub channel_id: String,
    pub channel_name: String,
    pub thumbnail_url: Option<String>,
    pub avatar_url: Option<String>,
}

/// `ChzzkAPIClient` + `AppState` 중 세션과 매니저가 쓰는 부분.
#[async_trait]
pub trait MultiLiveSessionBackend: Send + Sync {
    async fn live_detail(&self, channel_id: &str) -> Result<LiveInfo, MultiLiveBackendError>;
    /// `PlaybackLaunchPlan.timeMachinePlaybackEnabled`.
    fn time_machine_playback_enabled(&self) -> bool {
        false
    }
    /// `channelViewerInfo(channelId:).cheatKey` (실패는 `None`).
    async fn viewer_cheat_key(&self, _channel_id: &str) -> Option<bool> {
        None
    }
    /// `timeMachinePlayback(channelId:)` 가 타임머신이면 그 playback JSON.
    async fn time_machine_playback_json(&self, _channel_id: &str) -> Option<String> {
        None
    }
    fn build_plan(&self, live: &LiveInfo, channel_id: &str) -> Result<LaunchPlan, LaunchPlanError>;
    fn enabling_time_machine(
        &self,
        _plan: &LaunchPlan,
        _playback_json: &str,
    ) -> Option<LaunchPlan> {
        None
    }
    async fn live_status(
        &self,
        channel_id: &str,
    ) -> Result<LiveStatusSnapshot, MultiLiveBackendError>;
    /// `ChatSessionBootstrap.prepareConnection` → `connect` → 후원 순위 폴링.
    async fn connect_chat(
        &self,
        chat: Arc<dyn MultiLiveChat>,
        chat_channel_id: &str,
        stream_channel_id: &str,
        channel_name: &str,
    ) -> Result<(), String>;
    fn restart_chat_header_polling(&self, _chat: &Arc<dyn MultiLiveChat>, _channel_id: &str) {}
    fn player_settings(&self) -> PlayerSettings {
        PlayerSettings::default()
    }
    /// `appState.updatePlaybackActivity()`.
    fn note_playback_activity(&self) {}
    /// 데이터 저장소가 없으면 `None`.
    async fn start_watch_record(
        &self,
        _record: &WatchRecordStart,
    ) -> Option<Result<DateTime<Utc>, String>> {
        None
    }
    async fn end_watch_record(
        &self,
        _channel_id: &str,
        _started_at: DateTime<Utc>,
    ) -> Option<Result<bool, String>> {
        None
    }
    async fn checkpoint_watch_record(
        &self,
        _channel_id: &str,
        _started_at: DateTime<Utc>,
    ) -> Option<Result<bool, String>> {
        None
    }
    async fn nudge_watch_history_sync(&self) {}
    /// `WatchHistoryAggregationPolicy.sleepUntilNextCheckpoint` 간격.
    fn watch_checkpoint_interval(&self) -> Duration {
        Duration::from_secs(60)
    }
    /// 전경 상태 폴링 간격 (`PowerAwareInterval.scaled(60, batteryMultiplier: 1.5)`).
    fn foreground_poll_interval(&self) -> Duration {
        Duration::from_secs(60)
    }
}

/// `SettingsStore` 중 멀티라이브가 읽는 값.
pub trait MultiLiveSettingsSource: Send + Sync {
    fn is_loaded(&self) -> bool {
        true
    }
    fn multi_live(&self) -> MultiLiveSettings;
    fn player(&self) -> PlayerSettings;
}

/// `PiPController.shared`.
pub trait MultiLivePip: Send + Sync {
    fn stop_pip(&self);
    fn active_session_id(&self) -> Option<Uuid>;
    /// 패널 픽셀 크기 (없으면 0).
    fn panel_pixel_size(&self) -> (f64, f64);
}

/// PiP 없음.
#[derive(Debug, Default)]
pub struct NoPip;

impl MultiLivePip for NoPip {
    fn stop_pip(&self) {}
    fn active_session_id(&self) -> Option<Uuid> {
        None
    }
    fn panel_pixel_size(&self) -> (f64, f64) {
        (0.0, 0.0)
    }
}

/// `MultiChatSessionManager` 중 멀티라이브가 부르는 부분.
pub trait MultiLiveChatSessions: Send + Sync {
    fn remove_session_if_matching(&self, channel_id: &str, expected: &Arc<dyn MultiLiveChat>);
    fn remove_borrowed_sessions_all(&self);
}

/// `ChannelIdentityAssignments.shared.update(owner:priority:channelIds:)`.
pub trait ChannelIdentitySink: Send + Sync {
    fn update(&self, priority: i32, channel_ids: &[String]);
}

/// 대역폭 루프 간격 배율 (`PowerAwareInterval`·`RenderBudgetController`).
pub trait BandwidthPacing: Send + Sync {
    /// `PowerAwareInterval.scaled(updateIntervalSecs) * economyMul`.
    fn base_interval_secs(&self, update_interval_secs: f64) -> f64 {
        update_interval_secs
    }
}

/// 기본 배율 1.
#[derive(Debug, Default)]
pub struct PlainPacing;

impl BandwidthPacing for PlainPacing {}
