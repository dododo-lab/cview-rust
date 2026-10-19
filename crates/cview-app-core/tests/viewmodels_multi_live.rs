//! 멀티라이브 매니저·세션·정책 테스트.
//!
//! 원본: `Tests/CViewAppTests/MultiLiveFocusQualityTests.swift`, `MultiLiveTileDropTests.swift`,
//! `MultiLiveAdmissionPolicyTests.swift`, `MultiLiveAudioRoutingTests.swift`,
//! `MultiLiveDecodeSuspensionOwnershipTests.swift`, `MultiLiveNetworkBudgetTests.swift`,
//! `MultiLiveEmergencyDowngradeTests.swift` 의 소스 비의존 케이스. 실제 hls.js 엔진 대신
//! 상태만 기록하는 가짜 엔진/플레이어를 쓴다.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use cview_app_core::viewmodels::following::FollowingWorkspaceHost;
use cview_app_core::viewmodels::multi_live::deps::{
    ChatConnectionKind, LaunchPlan, LaunchPlanError, LiveStatusSnapshot, MultiLiveBackendError,
    MultiLiveChat, MultiLiveEnginePool, MultiLiveHlsEngine, MultiLivePlayer,
    MultiLiveSessionBackend, MultiLiveSessionFactory, MultiLiveSettingsSource,
    MultiLiveStreamRequest, SharedHlsEngine, StreamEndedProbe,
};
use cview_app_core::viewmodels::multi_live::layout::{
    layout_mode_transition, tile_drop, LayoutPrefsStore, MemoryLayoutPrefsStore,
    MultiLiveFocusDirection, MultiLiveGridLayoutMode, MultiLiveLayoutPrefs, MultiLiveLayoutRatios,
};
use cview_app_core::viewmodels::multi_live::policy::{
    self, focus_quality, DecodeRankInput, DecodeSuspensionReason, EvictionCandidate, MruLedger,
};
use cview_app_core::viewmodels::multi_live::{
    AddSessionOptions, MultiLiveAddSessionResult, MultiLiveDeps, MultiLiveLoadState,
    MultiLiveManager, MultiLiveSession,
};
use cview_core::models::settings::PlayerSettings;
use cview_core::models::settings_ui::{MultiLiveLayoutMode, MultiLiveSettings};
use cview_core::models::{ChannelInfo, LiveInfo, LiveStatus};
use cview_player::bandwidth_coordinator::{BandwidthAdvice, BufferHysteresisPhase};
use cview_player::coordinator::StreamPhase;
use cview_player::hlsjs::network_metrics::StreamTransferSample;
use cview_player::hlsjs::profile::HlsJsForcedQualityTarget;
use cview_player::policy::SessionTier;

// MARK: - 가짜

#[derive(Debug, Default)]
struct EngineState {
    max_height: i64,
    max_bitrate: i64,
    forced: Option<HlsJsForcedQualityTarget>,
    loading_suspended: bool,
    playing: bool,
    tier: Option<SessionTier>,
    surface_visible: Option<bool>,
    paused: bool,
    playback_desired: bool,
}

#[derive(Debug, Default)]
struct FakeEngine {
    s: Mutex<EngineState>,
}

impl FakeEngine {
    fn st(&self) -> std::sync::MutexGuard<'_, EngineState> {
        self.s.lock().unwrap()
    }
}

impl MultiLiveHlsEngine for FakeEngine {
    fn is_playing(&self) -> bool {
        self.st().playing
    }
    fn is_suspend_intent_active(&self) -> bool {
        self.st().loading_suspended
    }
    fn is_in_error_state(&self) -> bool {
        false
    }
    fn is_loading_suspended(&self) -> bool {
        self.st().loading_suspended
    }
    fn is_playback_desired(&self) -> bool {
        self.st().playback_desired
    }
    fn buffer_ahead_seconds(&self) -> f64 {
        4.0
    }
    fn current_bitrate(&self) -> f64 {
        2_000_000.0
    }
    fn rate(&self) -> f64 {
        1.0
    }
    fn transfers(&self) -> Vec<StreamTransferSample> {
        Vec::new()
    }
    fn measured_segment_duration(&self) -> f64 {
        2.0
    }
    fn force_highest_quality(&self) -> bool {
        false
    }
    fn forced_height_active(&self) -> bool {
        self.st().forced.is_some_and(|f| f.height > 0)
    }
    fn max_adaptive_height(&self) -> i64 {
        self.st().max_height
    }
    fn max_adaptive_bitrate(&self) -> i64 {
        self.st().max_bitrate
    }
    fn set_max_resolution_height(&self, height: i64) {
        self.st().max_height = height;
    }
    fn set_max_bitrate(&self, kbps: i64) {
        self.st().max_bitrate = kbps;
    }
    fn set_gpu_render_tier(&self, tier: SessionTier) {
        self.st().tier = Some(tier);
    }
    fn set_video_surface_visible(&self, visible: bool) {
        self.st().surface_visible = Some(visible);
    }
    fn pause(&self) {
        self.st().paused = true;
    }
    fn resume(&self) {
        self.st().paused = false;
    }
    fn resume_loading(&self) {
        self.st().loading_suspended = false;
    }
    fn suspend_loading(&self) {
        self.st().loading_suspended = true;
    }
    fn set_buffer_caps(&self, _a: f64, _b: f64, _c: i64) {}
    fn arm_surface_presented_probe(&self) {}
}

struct FakePlayer {
    engine: Mutex<Option<Arc<FakeEngine>>>,
    muted: AtomicBool,
    target: Mutex<HlsJsForcedQualityTarget>,
    phase: Mutex<StreamPhase>,
    error: Mutex<Option<String>>,
    rewind: AtomicBool,
    precision: AtomicBool,
    rate_override: Mutex<Option<f64>>,
    starts: Mutex<Vec<MultiLiveStreamRequest>>,
    stops: AtomicUsize,
}

impl FakePlayer {
    fn new(engine: Option<Arc<FakeEngine>>) -> Arc<Self> {
        Arc::new(Self {
            engine: Mutex::new(engine),
            muted: AtomicBool::new(false),
            target: Mutex::new(HlsJsForcedQualityTarget::MULTI_LIVE_1080P_8MBPS),
            phase: Mutex::new(StreamPhase::Idle),
            error: Mutex::new(None),
            rewind: AtomicBool::new(false),
            precision: AtomicBool::new(false),
            rate_override: Mutex::new(None),
            starts: Mutex::new(Vec::new()),
            stops: AtomicUsize::new(0),
        })
    }

    fn height(&self) -> i64 {
        self.target.lock().unwrap().height
    }
}

#[async_trait]
impl MultiLivePlayer for FakePlayer {
    fn engine(&self) -> Option<SharedHlsEngine> {
        self.engine
            .lock()
            .unwrap()
            .clone()
            .map(|e| e as SharedHlsEngine)
    }
    fn detach_engine(&self) -> Option<SharedHlsEngine> {
        self.engine
            .lock()
            .unwrap()
            .take()
            .map(|e| e as SharedHlsEngine)
    }
    fn is_muted(&self) -> bool {
        self.muted.load(Ordering::SeqCst)
    }
    fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::SeqCst);
    }
    fn stream_phase(&self) -> StreamPhase {
        self.phase.lock().unwrap().clone()
    }
    fn presented_frame_tick(&self) -> u64 {
        0
    }
    fn error_message(&self) -> Option<String> {
        self.error.lock().unwrap().clone()
    }
    fn set_precision_sync_active(&self, active: bool) {
        self.precision.store(active, Ordering::SeqCst);
    }
    fn rewind_recording_desired(&self) -> bool {
        self.rewind.load(Ordering::SeqCst)
    }
    fn set_rewind_recording_desired(&self, desired: bool) {
        self.rewind.store(desired, Ordering::SeqCst);
    }
    fn multi_live_quality_target(&self) -> HlsJsForcedQualityTarget {
        *self.target.lock().unwrap()
    }
    fn set_multi_live_quality_target(&self, target: HlsJsForcedQualityTarget) -> bool {
        let mut t = self.target.lock().unwrap();
        if *t == target {
            return false;
        }
        *t = target;
        if let Some(e) = self.engine.lock().unwrap().as_ref() {
            e.st().forced = Some(target);
        }
        true
    }
    fn apply_picture_enhancement(&self, _auto: bool, _upscale: bool) {}
    fn recover_from_background(&self) {}
    fn promote_for_in_app_selection(&self) {}
    fn sync_network_activity(&self) {}
    fn resume_watch_heartbeat_for_menu_return(&self) {}
    fn suspend_watch_heartbeat_for_menu_leave(&self) {}
    fn restore_surface_after_occlusion(&self) {}
    fn set_gpu_render_tier_hidden(&self, _keep_alive: bool) {}
    fn set_max_rate_override(&self, rate: Option<f64>) {
        *self.rate_override.lock().unwrap() = rate;
    }
    fn configure_time_machine_access(&self, _a: bool, _b: bool) {}
    fn is_time_machine_available(&self) -> bool {
        false
    }
    fn apply_started_settings(&self, _settings: &PlayerSettings) {}
    fn set_on_playback_state_changed(&self, _callback: Option<Arc<dyn Fn() + Send + Sync>>) {}
    fn set_stream_ended_probe(&self, _probe: Option<StreamEndedProbe>) {}
    async fn start_stream(&self, request: MultiLiveStreamRequest) {
        *self.phase.lock().unwrap() = StreamPhase::Playing;
        self.starts.lock().unwrap().push(request);
    }
    async fn stop_stream(&self) {
        self.stops.fetch_add(1, Ordering::SeqCst);
        *self.phase.lock().unwrap() = StreamPhase::Idle;
    }
    async fn toggle_play_pause(&self) {}
}

#[derive(Default)]
struct FakeChat {
    disconnects: AtomicUsize,
}

#[async_trait]
impl MultiLiveChat for FakeChat {
    fn connection_state(&self) -> ChatConnectionKind {
        ChatConnectionKind::Other
    }
    fn has_last_connection_config(&self) -> bool {
        false
    }
    fn set_background_mode(&self, _background: bool) {}
    fn set_user(&self, _uid: Option<String>, _nickname: Option<String>) {}
    fn set_live_metadata(&self, _v: Option<i64>, _a: Option<i64>, _o: Option<DateTime<Utc>>) {}
    fn restore_owner_presentation(&self, _background: bool) {}
    async fn disconnect(&self) {
        self.disconnects.fetch_add(1, Ordering::SeqCst);
    }
    async fn reconnect(&self) {}
}

#[derive(Default)]
struct FakePool {
    acquired: AtomicUsize,
    released: AtomicUsize,
    discarded: AtomicUsize,
    drained: AtomicUsize,
    max: AtomicUsize,
}

#[async_trait]
impl MultiLiveEnginePool for FakePool {
    async fn set_max_pool_size(&self, size: usize) {
        self.max.store(size, Ordering::SeqCst);
    }
    async fn acquire(&self) -> Option<SharedHlsEngine> {
        self.acquired.fetch_add(1, Ordering::SeqCst);
        Some(Arc::new(FakeEngine::default()))
    }
    async fn release(&self, _engine: SharedHlsEngine) {
        self.released.fetch_add(1, Ordering::SeqCst);
    }
    async fn discard(&self, _engine: SharedHlsEngine) {
        self.discarded.fetch_add(1, Ordering::SeqCst);
    }
    async fn release_slot(&self) {}
    async fn total_active_count(&self) -> usize {
        0
    }
    async fn trim_idle(&self, _keep: usize) {}
    async fn drain(&self) {
        self.drained.fetch_add(1, Ordering::SeqCst);
    }
    async fn warmup(&self, _count: usize) {}
}

/// 풀이 준 엔진을 감싸지 않는 팩토리 — 매니저 경로의 플레이어는 엔진 없이 둔다.
struct FakeFactory;

impl MultiLiveSessionFactory for FakeFactory {
    fn make_player(&self, _engine: Option<SharedHlsEngine>) -> Arc<dyn MultiLivePlayer> {
        FakePlayer::new(None)
    }
    fn make_chat(&self) -> Arc<dyn MultiLiveChat> {
        Arc::new(FakeChat::default())
    }
}

#[derive(Default)]
struct FakeBackend {
    closed: Mutex<HashSet<String>>,
    hang: AtomicBool,
    status_closed: AtomicBool,
    plan_error: AtomicBool,
    detail_calls: AtomicUsize,
}

fn live(channel_id: &str, status: LiveStatus) -> LiveInfo {
    let mut l = LiveInfo::new(1, format!("{channel_id} 방송"));
    l.status = status;
    l.concurrent_user_count = 1234;
    l.chat_channel_id = None;
    l.channel = Some(ChannelInfo::new(channel_id, format!("이름-{channel_id}")));
    l
}

#[async_trait]
impl MultiLiveSessionBackend for FakeBackend {
    async fn live_detail(&self, channel_id: &str) -> Result<LiveInfo, MultiLiveBackendError> {
        self.detail_calls.fetch_add(1, Ordering::SeqCst);
        if self.hang.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
        let status = if self.closed.lock().unwrap().contains(channel_id) {
            LiveStatus::Close
        } else {
            LiveStatus::Open
        };
        Ok(live(channel_id, status))
    }
    fn build_plan(&self, live: &LiveInfo, channel_id: &str) -> Result<LaunchPlan, LaunchPlanError> {
        if self.plan_error.load(Ordering::SeqCst) {
            return Err(LaunchPlanError::NoHlsUrl);
        }
        Ok(LaunchPlan {
            stream_url: format!("https://example.invalid/{channel_id}.m3u8"),
            is_audio_only: false,
            channel_name: live
                .channel
                .as_ref()
                .map(|c| c.channel_name.clone())
                .unwrap_or_default(),
            live_title: live.live_title.clone(),
            is_time_machine: false,
        })
    }
    async fn live_status(
        &self,
        _channel_id: &str,
    ) -> Result<LiveStatusSnapshot, MultiLiveBackendError> {
        Ok(LiveStatusSnapshot {
            is_closed: self.status_closed.load(Ordering::SeqCst),
            concurrent_user_count: 99,
            accumulate_count: 500,
            open_date: None,
        })
    }
    async fn connect_chat(
        &self,
        _chat: Arc<dyn MultiLiveChat>,
        _a: &str,
        _b: &str,
        _c: &str,
    ) -> Result<(), String> {
        Ok(())
    }
}

struct FakeSettings {
    ml: MultiLiveSettings,
}

impl MultiLiveSettingsSource for FakeSettings {
    fn multi_live(&self) -> MultiLiveSettings {
        self.ml.clone()
    }
    fn player(&self) -> PlayerSettings {
        PlayerSettings::default()
    }
}

struct Toasts(Mutex<Vec<String>>);

impl cview_app_core::viewmodels::chat::deps::ToastPresenter for Toasts {
    fn show(&self, message: String, _t: Option<String>, _u: Option<Box<dyn FnOnce() + Send>>) {
        self.0.lock().unwrap().push(message);
    }
}

fn manager_with(pool: Arc<FakePool>, prefs: Arc<MemoryLayoutPrefsStore>) -> MultiLiveManager {
    let mut deps = MultiLiveDeps::new(pool, Arc::new(FakeFactory));
    deps.layout_prefs = prefs;
    MultiLiveManager::new(deps)
}

fn manager() -> MultiLiveManager {
    let m = manager_with(
        Arc::new(FakePool::default()),
        Arc::new(MemoryLayoutPrefsStore::default()),
    );
    m.lock().is_terminating = true;
    m
}

struct Pane {
    session: Arc<MultiLiveSession>,
    player: Arc<FakePlayer>,
    engine: Arc<FakeEngine>,
}

fn pane(channel_id: &str) -> Pane {
    let engine = Arc::new(FakeEngine::default());
    let player = FakePlayer::new(Some(engine.clone()));
    let session = MultiLiveSession::new(channel_id, player.clone(), Arc::new(FakeChat::default()));
    Pane {
        session,
        player,
        engine,
    }
}

fn panes(n: usize, prefix: &str) -> Vec<Pane> {
    (0..n).map(|i| pane(&format!("{prefix}-{i}"))).collect()
}

fn heights(p: &[Pane]) -> Vec<i64> {
    p.iter().map(|x| x.player.height()).collect()
}

fn ids(p: &[Pane]) -> Vec<Uuid> {
    p.iter().map(|x| x.session.id()).collect()
}

fn focus_setup() -> (MultiLiveManager, Vec<Pane>) {
    let m = manager();
    let p = panes(4, "focus-test");
    for x in &p {
        x.session.set_muted(true);
    }
    {
        let mut st = m.lock();
        st.set_sessions(p.iter().map(|x| x.session.clone()).collect());
        st.set_selected_session_id(Some(p[0].session.id()));
        st.set_grid_layout(true);
        st.set_grid_layout_mode(MultiLiveGridLayoutMode::Focus);
        st.set_focus_main_session_id(Some(p[0].session.id()));
        st.report_stage_size((1600.0, 900.0));
        st.reconcile_focus_quality(None);
    }
    (m, p)
}

fn advice(id: Uuid, bitrate: f64, height: i64, emergency: bool, measured: bool) -> BandwidthAdvice {
    BandwidthAdvice {
        session_id: id,
        max_allowed_bitrate: bitrate,
        capped_max_height: height,
        emergency_downgrade: emergency,
        buffer_phase: if emergency {
            BufferHysteresisPhase::Buffering
        } else {
            BufferHysteresisPhase::Playing
        },
        has_bandwidth_estimate: measured,
    }
}

// MARK: - 포커스 화질 (MultiLiveFocusQualityTests)

#[test]
fn focus_roles_control_decoder_target() {
    let (_m, p) = focus_setup();
    assert_eq!(heights(&p), vec![1080, 360, 360, 360]);
    assert!(p[1..]
        .iter()
        .all(|x| x.engine.st().forced.map(|f| f.height) == Some(360)));
}

#[test]
fn focus_swap_restores_main_immediately() {
    let (m, p) = focus_setup();
    p[2].engine.set_max_bitrate(900);
    p[2].engine.set_max_resolution_height(360);
    m.lock().set_focus_main_session_id(Some(p[2].session.id()));
    assert_eq!(heights(&p), vec![360, 360, 1080, 360]);
    assert_eq!(
        p[2].engine.st().forced,
        Some(HlsJsForcedQualityTarget::MULTI_LIVE_1080P_8MBPS)
    );
    assert_eq!(p[2].engine.max_adaptive_bitrate(), 0);
    assert_eq!(p[2].engine.max_adaptive_height(), 0);
    assert!(
        p.iter().all(|x| x.player.is_muted()),
        "화질 전환이 오디오를 바꾸면 안 된다"
    );
}

#[test]
fn main_and_pip_are_protected_from_emergency_downgrade() {
    let (m, p) = focus_setup();
    {
        let mut st = m.lock();
        st.set_selected_session_id(Some(p[1].session.id()));
        st.set_pip_session_id(Some(p[3].session.id()));
        let advices: Vec<_> = [0, 3]
            .iter()
            .map(|i| advice(p[*i].session.id(), 900_000.0, 360, true, true))
            .collect();
        st.apply_bandwidth_advices(&advices);
    }
    assert_eq!(heights(&p), vec![1080, 360, 360, 1080]);
    assert_eq!(p[0].engine.max_adaptive_height(), 0);
    assert_eq!(p[3].engine.max_adaptive_bitrate(), 0);
}

#[test]
fn removal_and_missing_main_fall_back_to_first() {
    let (m, p) = focus_setup();
    let mut st = m.lock();
    let rest: Vec<_> = p[1..].iter().map(|x| x.session.clone()).collect();
    st.set_sessions(rest);
    assert_eq!(st.quality_main_session_id(), Some(p[1].session.id()));
    assert_eq!(p[1].player.height(), 1080);
    st.set_focus_main_session_id(Some(Uuid::new_v4()));
    assert_eq!(st.quality_main_session_id(), Some(p[1].session.id()));
}

#[test]
fn layout_exit_and_single_session_restore_full_quality() {
    let (m, p) = focus_setup();
    let mut st = m.lock();
    st.set_selected_session_id(Some(p[2].session.id()));
    st.set_grid_layout_mode(MultiLiveGridLayoutMode::Preset);
    assert_eq!(p[2].player.height(), 1080);
    assert!(p[0].player.height() <= 720);
    st.set_grid_layout(false);
    assert_eq!(p[1].player.height(), 360);
    st.set_sessions(vec![p[1].session.clone()]);
    assert_eq!(p[1].player.height(), 1080);
}

#[test]
fn repeated_focus_switch_uses_latest_role() {
    let (m, p) = focus_setup();
    for i in [1, 3, 0, 2, 1, 2] {
        m.lock().set_focus_main_session_id(Some(p[i].session.id()));
    }
    assert_eq!(heights(&p), vec![360, 360, 1080, 360]);
    let forced: Vec<_> = p
        .iter()
        .map(|x| x.engine.st().forced.map(|f| f.height))
        .collect();
    assert_eq!(forced, vec![Some(360), Some(360), Some(1080), Some(360)]);
}

#[test]
fn audible_peer_keeps_480_floor() {
    let (m, p) = focus_setup();
    m.lock().route_audio(p[2].session.id());
    assert_eq!(p[2].player.height(), 480);
    m.lock().route_audio(p[0].session.id());
    assert_eq!(p[2].player.height(), 360);
    assert_eq!(p[0].player.height(), 1080);
}

#[test]
fn thermal_serious_caps_secondaries_only() {
    let (m, p) = focus_setup();
    let mut st = m.lock();
    st.route_audio(p[2].session.id());
    assert_eq!(p[2].player.height(), 480);
    st.set_thermal_secondary_quality_cap(true);
    assert_eq!(heights(&p), vec![1080, 360, 360, 360]);
    st.set_pip_session_id(Some(p[3].session.id()));
    assert_eq!(p[3].player.height(), 1080);
    st.set_thermal_secondary_quality_cap(false);
    assert_eq!(p[0].player.height(), 1080);
    assert_eq!(p[2].player.height(), 480);
    assert_eq!(p[3].player.height(), 1080);
}

#[test]
fn mute_before_append_avoids_audible_floor() {
    let (m, p) = focus_setup();
    let mut st = m.lock();
    st.set_sessions(p[..3].iter().map(|x| x.session.clone()).collect());
    let unmuted = pane("focus-unmuted");
    st.push_session(unmuted.session.clone());
    assert_eq!(unmuted.player.height(), 480);
    st.set_sessions(p[..3].iter().map(|x| x.session.clone()).collect());
    let muted = pane("focus-muted");
    muted.session.set_muted(true);
    muted.session.set_background_mode(true);
    st.push_session(muted.session.clone());
    assert_eq!(muted.player.height(), 360);
}

#[test]
fn focus_quality_policy_geometry_and_hysteresis() {
    let h = focus_quality::secondary_pane_size((1920.0, 1080.0), 4, true, true, 0.7);
    let v = focus_quality::secondary_pane_size((1920.0, 1080.0), 4, true, false, 0.7);
    assert!((h.0 - 576.0).abs() < 0.01 && h.1 == 360.0);
    assert!(v.0 == 640.0 && (v.1 - 324.0).abs() < 0.01);
    let target = |hgt: f64, prev: i64| {
        focus_quality::secondary_target(Some((hgt * 16.0 / 9.0, hgt)), prev, false, false).height
    };
    assert_eq!(target(365.0, 360), 360);
    assert_eq!(target(420.0, 360), 480);
    assert_eq!(target(350.0, 480), 480);
    assert_eq!(target(300.0, 480), 360);
    assert_eq!(target(1200.0, 720), 720);
    let unknown = focus_quality::secondary_target(None, 1080, false, false);
    assert_eq!((unknown.height, unknown.target_bitrate_kbps), (360, 1_000));
    let grid = focus_quality::secondary_pane_size((1800.0, 1000.0), 6, false, true, 0.7);
    assert_eq!(grid, (600.0, 500.0));
}

// MARK: - 타일 끌어 놓기 (MultiLiveTileDropTests)

#[test]
fn tile_drop_resolve_rules() {
    let (a, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let order = [a, b, c];
    use tile_drop::Outcome::*;
    assert_eq!(
        tile_drop::resolve(&order, c, a, Some(a)),
        Some(Swap(2, 0, c))
    );
    assert_eq!(
        tile_drop::resolve(&order, a, b, Some(a)),
        Some(Swap(0, 1, b))
    );
    assert_eq!(
        tile_drop::resolve(&order, b, c, Some(a)),
        Some(Move {
            from: 1,
            to_offset: 3
        })
    );
    assert_eq!(
        tile_drop::resolve(&order, c, b, Some(a)),
        Some(Move {
            from: 2,
            to_offset: 1
        })
    );
    assert_eq!(
        tile_drop::resolve(&order, c, a, None),
        Some(Move {
            from: 2,
            to_offset: 0
        })
    );
    assert_eq!(tile_drop::resolve(&order, a, a, Some(a)), None);
    assert_eq!(tile_drop::resolve(&order, Uuid::new_v4(), a, Some(a)), None);
    let mut v = vec![1, 2, 3];
    tile_drop::apply_move(&mut v, 1, 3);
    assert_eq!(v, vec![1, 3, 2]);
    tile_drop::apply_move(&mut v, 2, 0);
    assert_eq!(v, vec![2, 1, 3]);
}

fn drop_setup(mode: MultiLiveGridLayoutMode) -> (MultiLiveManager, Vec<Pane>) {
    let m = manager();
    let p = vec![pane("drop-A"), pane("drop-B"), pane("drop-C")];
    {
        let mut st = m.lock();
        st.set_sessions(p.iter().map(|x| x.session.clone()).collect());
        st.set_selected_session_id(Some(p[0].session.id()));
        st.set_grid_layout(true);
        st.set_grid_layout_mode(mode);
    }
    (m, p)
}

#[test]
fn drop_onto_focus_main_promotes() {
    let (m, s) = drop_setup(MultiLiveGridLayoutMode::Focus);
    let mut st = m.lock();
    st.set_focus_main_session_id(Some(s[0].session.id()));
    let mut double_tap = None;
    assert!(st.drop_tile(s[2].session.id(), s[0].session.id(), &mut double_tap));
    assert_eq!(st.focus_main_session_id(), Some(s[2].session.id()));
    let order: Vec<_> = st.sessions().iter().map(|x| x.id()).collect();
    assert_eq!(
        order,
        vec![s[2].session.id(), s[1].session.id(), s[0].session.id()]
    );
    assert_eq!(double_tap, None);
    assert_eq!(st.selected_session_id(), Some(s[0].session.id()));
}

#[test]
fn drag_main_onto_secondary_and_empty_main_fallback() {
    let (m, s) = drop_setup(MultiLiveGridLayoutMode::Focus);
    let mut st = m.lock();
    st.set_focus_main_session_id(Some(s[0].session.id()));
    let mut dt = None;
    st.drop_tile(s[0].session.id(), s[1].session.id(), &mut dt);
    assert_eq!(st.focus_main_session_id(), Some(s[1].session.id()));
    let order: Vec<_> = st.sessions().iter().map(|x| x.id()).collect();
    assert_eq!(
        order,
        vec![s[1].session.id(), s[0].session.id(), s[2].session.id()]
    );
    drop(st);

    let (m, s) = drop_setup(MultiLiveGridLayoutMode::Focus);
    let mut st = m.lock();
    st.set_focus_main_session_id(None);
    st.drop_tile(s[1].session.id(), s[0].session.id(), &mut dt);
    assert_eq!(st.focus_main_session_id(), Some(s[1].session.id()));
}

#[test]
fn double_tap_main_swaps_without_touching_focus_main() {
    let (m, s) = drop_setup(MultiLiveGridLayoutMode::Preset);
    let mut st = m.lock();
    let mut dt = Some(s[1].session.id());
    assert!(st.drop_tile(s[2].session.id(), s[1].session.id(), &mut dt));
    assert_eq!(dt, Some(s[2].session.id()));
    assert_eq!(st.focus_main_session_id(), None);
    let mut none = None;
    assert!(st.drop_tile(s[2].session.id(), s[0].session.id(), &mut none));
    let order: Vec<_> = st.sessions().iter().map(|x| x.id()).collect();
    assert_eq!(
        order,
        vec![s[2].session.id(), s[0].session.id(), s[1].session.id()]
    );
}

#[test]
fn layout_mode_transition_and_apply() {
    use layout_mode_transition::{resolve, Outcome};
    use MultiLiveGridLayoutMode::*;
    assert_eq!(resolve(Preset, Preset, false), Outcome::Noop);
    assert_eq!(resolve(Focus, Focus, true), Outcome::Enter(Focus));
    assert_eq!(resolve(Preset, Custom, false), Outcome::Enter(Custom));
    let (m, s) = drop_setup(Preset);
    let mut st = m.lock();
    st.set_layout_ratios(MultiLiveLayoutRatios {
        horizontal_ratio: 0.3,
        vertical_ratio: 0.3,
        bottom_row_ratio: 0.3,
        focus_ratio: 0.6,
    });
    st.apply_grid_layout_mode(Custom);
    assert_eq!(
        st.layout_ratios(),
        MultiLiveLayoutRatios::with_focus_ratio(0.6)
    );
    st.apply_grid_layout_mode(Focus);
    assert_eq!(st.focus_main_session_id(), Some(s[0].session.id()));
    assert_eq!(st.grid_layout_mode(), Focus);
}

// MARK: - 오디오 라우팅 (MultiLiveAudioRoutingTests)

fn audio_setup() -> (MultiLiveManager, Vec<Pane>) {
    let m = manager();
    let p = vec![pane("a"), pane("b"), pane("c")];
    {
        let mut st = m.lock();
        st.set_sessions(p.iter().map(|x| x.session.clone()).collect());
        st.set_selected_session_id(Some(p[0].session.id()));
        st.route_audio(p[0].session.id());
    }
    (m, p)
}

fn audible(p: &[Pane]) -> Vec<bool> {
    p.iter().map(|x| !x.player.is_muted()).collect()
}

#[test]
fn selection_reconciles_all_audio() {
    let (m, p) = audio_setup();
    let mut st = m.lock();
    st.route_audio(p[2].session.id());
    st.select_session(p[1].session.id(), false);
    assert_eq!(st.audio_session_id(), Some(p[1].session.id()));
    assert_eq!(audible(&p), vec![false, true, false]);
    let enabled: Vec<_> = p.iter().map(|x| st.is_audio_enabled(&x.session)).collect();
    assert_eq!(enabled, vec![false, true, false]);
}

#[test]
fn rerouting_current_audio_and_audible_vs_enabled() {
    let (m, p) = audio_setup();
    let mut st = m.lock();
    p[0].session.set_muted(true);
    assert!(st.is_audio_enabled(&p[0].session));
    assert!(!st.is_audible(&p[0].session));
    assert!(!st.is_audio_enabled(&p[1].session));
    st.route_audio(p[0].session.id());
    assert!(!p[0].player.is_muted());
}

#[test]
fn multi_audio_selection_preserves_enabled_set() {
    let (m, p) = audio_setup();
    let mut st = m.lock();
    st.toggle_multi_audio_mode();
    st.toggle_session_audio(p[2].session.id());
    st.select_session(p[1].session.id(), false);
    assert_eq!(audible(&p), vec![true, false, true]);
}

#[test]
fn toggle_semantics_single_and_multi() {
    let (m, p) = audio_setup();
    let mut st = m.lock();
    st.toggle_session_audio(p[0].session.id());
    assert!(p[0].player.is_muted());
    assert_eq!(st.audio_session_id(), Some(p[0].session.id()));
    st.toggle_session_audio(p[0].session.id());
    assert!(st.is_audible(&p[0].session));
    st.toggle_session_audio(p[1].session.id());
    assert_eq!(st.audio_session_id(), Some(p[1].session.id()));
    assert_eq!(audible(&p), vec![false, true, false]);
    drop(st);

    let (m, p) = audio_setup();
    let mut st = m.lock();
    st.toggle_multi_audio_mode();
    st.toggle_session_audio(p[2].session.id());
    assert!(st.is_audible(&p[2].session));
    p[2].session.set_muted(true);
    assert!(st.is_audio_enabled(&p[2].session) && !st.is_audible(&p[2].session));
    st.toggle_session_audio(p[2].session.id());
    assert!(st.is_audible(&p[2].session));
    st.toggle_session_audio(p[2].session.id());
    assert!(!st.is_audio_enabled(&p[2].session));
    assert!(st.is_audible(&p[0].session));
}

#[tokio::test]
async fn removing_selection_keeps_independent_audio_and_manual_mute() {
    let (m, p) = audio_setup();
    m.lock().route_audio(p[2].session.id());
    m.remove_session(p[0].session.id()).await;
    {
        let st = m.read();
        assert_eq!(st.audio_session_id(), Some(p[2].session.id()));
        assert_eq!(st.selected_session_id(), Some(p[1].session.id()));
    }
    assert!(p[1].player.is_muted());
    assert!(!p[2].player.is_muted());

    let (m, p) = audio_setup();
    p[0].session.set_muted(true);
    m.remove_session(p[2].session.id()).await;
    assert!(p[0].player.is_muted());
    assert!(p[1].player.is_muted());
}

// MARK: - 디코드 정지 소유권 (MultiLiveDecodeSuspensionOwnershipTests)

fn decode_setup() -> (MultiLiveManager, Vec<Pane>) {
    let m = manager();
    let p = panes(4, "test");
    {
        let mut st = m.lock();
        st.set_sessions(p.iter().map(|x| x.session.clone()).collect());
        st.set_selected_session_id(Some(p[0].session.id()));
        st.reconcile_decode_budget();
    }
    (m, p)
}

fn suspended(p: &[Pane]) -> Vec<bool> {
    p.iter().map(|x| x.engine.is_loading_suspended()).collect()
}

#[test]
fn occluded_panes_stay_suspended_across_budget_refreshes() {
    let (m, p) = decode_setup();
    let mut st = m.lock();
    st.suspend_non_selected_decode(DecodeSuspensionReason::WindowOcclusion);
    for _ in 0..10 {
        st.reconcile_decode_budget();
    }
    assert_eq!(suspended(&p), vec![false, true, true, true]);
    assert_eq!(
        st.pane_suspension().get(&p[1].session.id()).copied(),
        Some(policy::PaneSuspensionCause::Occlusion)
    );
    st.restore_decode_from_budget(DecodeSuspensionReason::WindowOcclusion);
    assert_eq!(suspended(&p), vec![false; 4]);
    assert!(st.pane_suspension().is_empty());
}

#[test]
fn thermal_and_occlusion_reasons_are_independent() {
    let (m, p) = decode_setup();
    let mut st = m.lock();
    st.suspend_non_selected_decode(DecodeSuspensionReason::WindowOcclusion);
    st.suspend_non_selected_decode(DecodeSuspensionReason::ThermalPressure);
    st.restore_decode_from_budget(DecodeSuspensionReason::ThermalPressure);
    assert!(suspended(&p)[1..].iter().all(|s| *s));
    st.restore_decode_from_budget(DecodeSuspensionReason::WindowOcclusion);
    assert_eq!(suspended(&p), vec![false; 4]);

    st.suspend_non_selected_decode(DecodeSuspensionReason::ThermalPressure);
    st.suspend_non_selected_decode(DecodeSuspensionReason::WindowOcclusion);
    st.restore_decode_from_budget(DecodeSuspensionReason::WindowOcclusion);
    assert!(suspended(&p)[1..].iter().all(|s| *s));
    assert_eq!(
        st.pane_suspension().get(&p[1].session.id()).copied(),
        Some(policy::PaneSuspensionCause::Thermal)
    );
    st.restore_decode_from_budget(DecodeSuspensionReason::ThermalPressure);
    assert_eq!(suspended(&p), vec![false; 4]);
}

#[test]
fn selection_and_pip_remain_protected() {
    let (m, p) = decode_setup();
    let mut st = m.lock();
    st.suspend_non_selected_decode(DecodeSuspensionReason::WindowOcclusion);
    st.set_selected_session_id(Some(p[1].session.id()));
    st.set_pip_session_id(Some(p[2].session.id()));
    st.reconcile_decode_budget();
    assert_eq!(suspended(&p), vec![true, false, false, true]);
}

#[test]
fn repeated_round_trips_restore_every_pane() {
    let (m, p) = decode_setup();
    let mut st = m.lock();
    for _ in 0..20 {
        st.suspend_non_selected_decode(DecodeSuspensionReason::WindowOcclusion);
        st.reconcile_decode_budget();
        assert!(suspended(&p)[1..].iter().all(|s| *s));
        st.restore_decode_from_budget(DecodeSuspensionReason::WindowOcclusion);
        st.restore_decode_from_budget(DecodeSuspensionReason::WindowOcclusion);
        assert_eq!(suspended(&p), vec![false; 4]);
    }
}

#[tokio::test(start_paused = true)]
async fn memory_pressure_limits_budget_then_recovers() {
    let (m, p) = decode_setup();
    m.lock().handle_memory_pressure(true);
    assert_eq!(suspended(&p), vec![false, true, true, true]);
    assert_eq!(
        m.read().pane_suspension().get(&p[3].session.id()).copied(),
        Some(policy::PaneSuspensionCause::MemoryPressure)
    );
    tokio::time::sleep(Duration::from_secs(21)).await;
    assert_eq!(m.read().memory_pressure_override(), None);
    assert_eq!(suspended(&p), vec![false; 4]);
}

#[test]
fn watch_stage_limits_decode_to_keep_alive_set() {
    let m = manager();
    let p = panes(5, "watch");
    let mut st = m.lock();
    st.set_sessions(p.iter().map(|x| x.session.clone()).collect());
    for x in &p {
        st.select_session(x.session.id(), false);
    }
    st.select_session(p[0].session.id(), false);
    st.set_stage_session(Some(p[0].session.id()));
    // MRU: p0(최신) > p4 > p3 > p2 > p1. 유지 = p4, p3 (2개).
    let keep = st.watch_keep_alive_session_ids();
    assert_eq!(
        keep,
        [p[4].session.id(), p[3].session.id()].into_iter().collect()
    );
    assert_eq!(st.watch_warm_candidate_id(), Some(p[4].session.id()));
    assert!(!st.watch_mode_allows_decode(p[1].session.id()));
    assert!(st.watch_mode_allows_decode(p[3].session.id()));
    // 스테이지 밖 패인은 화면에서 내린다.
    assert_eq!(p[1].engine.st().surface_visible, Some(false));
    assert_eq!(p[0].engine.st().surface_visible, Some(true));
    st.set_grid_layout(true);
    assert_eq!(st.stage_session_id(), None);
    assert!(st.watch_mode_allows_decode(p[1].session.id()));
}

// MARK: - 네트워크 예산·긴급 강등

fn network_setup(force_pinned: bool) -> (MultiLiveManager, Pane) {
    let m = manager();
    let main = pane("network-main");
    let peer = pane("network-peer");
    {
        let mut st = m.lock();
        st.set_sessions(vec![main.session.clone(), peer.session.clone()]);
        st.set_selected_session_id(Some(main.session.id()));
    }
    peer.engine.st().forced = if force_pinned {
        Some(HlsJsForcedQualityTarget::MULTI_LIVE_1080P_8MBPS)
    } else {
        None
    };
    (m, peer)
}

#[test]
fn unknown_capacity_releases_network_cap_and_resets_smoothing() {
    let (m, peer) = network_setup(false);
    let id = peer.session.id();
    let mut st = m.lock();
    st.apply_bandwidth_advices(&[advice(id, 2_000_000.0, 480, false, true)]);
    assert_eq!(peer.engine.max_adaptive_bitrate(), 2_000);
    st.apply_bandwidth_advices(&[advice(id, 0.0, 480, false, false)]);
    assert_eq!(peer.engine.max_adaptive_bitrate(), 0);
    assert_eq!(st.last_applied_hls_kbps(id), None);
    assert_eq!(peer.engine.max_adaptive_height(), 480);
    st.apply_bandwidth_advices(&[advice(id, 6_000_000.0, 480, false, true)]);
    assert_eq!(peer.engine.max_adaptive_bitrate(), 6_000);
}

#[test]
fn measured_exhaustion_requests_lowest_level() {
    let (m, peer) = network_setup(false);
    let id = peer.session.id();
    let mut st = m.lock();
    st.apply_bandwidth_advices(&[advice(id, 6_000_000.0, 480, false, true)]);
    st.apply_bandwidth_advices(&[advice(id, 0.0, 480, false, true)]);
    assert_eq!(peer.engine.max_adaptive_bitrate(), 1);
    assert_eq!(st.last_applied_hls_kbps(id), Some(1.0));
    st.apply_bandwidth_advices(&[advice(id, 0.0, 480, false, false)]);
    assert_eq!(peer.engine.max_adaptive_bitrate(), 0);
}

#[test]
fn smoothing_ignores_small_changes() {
    let (m, peer) = network_setup(false);
    let id = peer.session.id();
    let mut st = m.lock();
    st.apply_bandwidth_advices(&[advice(id, 4_000_000.0, 0, false, true)]);
    st.apply_bandwidth_advices(&[advice(id, 4_200_000.0, 0, false, true)]);
    assert_eq!(
        peer.engine.max_adaptive_bitrate(),
        4_000,
        "12% 미만 변화는 무시"
    );
    st.apply_bandwidth_advices(&[advice(id, 2_000_000.0, 0, false, true)]);
    assert_eq!(peer.engine.max_adaptive_bitrate(), 3_200);
    assert_eq!(
        policy::smoothed_bitrate_cap_kbps(500_000.0, true, Some(4_000.0)),
        Some(500.0)
    );
}

#[test]
fn emergency_downgrade_reaches_pinned_pane_and_releases_after() {
    let (m, peer) = network_setup(true);
    let id = peer.session.id();
    assert!(peer.engine.forced_height_active());
    let mut st = m.lock();
    st.apply_bandwidth_advices(&[advice(id, 3_000_000.0, 720, true, true)]);
    assert_eq!(peer.engine.max_adaptive_bitrate(), 3_000);
    assert_eq!(peer.engine.max_adaptive_height(), 720);
    st.apply_bandwidth_advices(&[advice(id, 6_000_000.0, 1080, false, true)]);
    assert_eq!(peer.engine.max_adaptive_bitrate(), 0);
    assert_eq!(peer.engine.max_adaptive_height(), 0);
    drop(st);

    let (m, peer) = network_setup(true);
    m.lock()
        .apply_bandwidth_advices(&[advice(peer.session.id(), 3_000_000.0, 720, false, true)]);
    assert_eq!(peer.engine.max_adaptive_bitrate(), 0, "평상시는 바이패스");
    let (m, peer) = network_setup(false);
    m.lock()
        .apply_bandwidth_advices(&[advice(peer.session.id(), 2_500_000.0, 480, false, true)]);
    assert_eq!(peer.engine.max_adaptive_bitrate(), 2_500);
    assert_eq!(peer.engine.max_adaptive_height(), 480);
}

#[test]
fn rate_budget_splits_twelve_percent() {
    let m = manager();
    let p = panes(3, "rate");
    let mut st = m.lock();
    st.set_sessions(p.iter().map(|x| x.session.clone()).collect());
    st.set_selected_session_id(Some(p[1].session.id()));
    st.apply_rate_budget();
    let rates: Vec<f64> = p
        .iter()
        .map(|x| x.player.rate_override.lock().unwrap().unwrap())
        .collect();
    assert!((rates[1] - 1.072).abs() < 1e-9);
    assert!((rates[0] - 1.024).abs() < 1e-9 && (rates[2] - 1.024).abs() < 1e-9);
    st.set_sessions(vec![p[0].session.clone()]);
    st.apply_rate_budget();
    assert_eq!(*p[0].player.rate_override.lock().unwrap(), None);
    assert!(policy::rate_budget_plan(&[Uuid::new_v4()], None).is_none());
}

#[test]
fn pane_sizes_follow_roles() {
    let m = manager();
    let p = panes(3, "pane");
    let mut st = m.lock();
    st.set_sessions(p.iter().map(|x| x.session.clone()).collect());
    st.set_selected_session_id(Some(p[0].session.id()));
    st.set_grid_layout(true);
    st.set_grid_layout_mode(MultiLiveGridLayoutMode::Focus);
    st.set_focus_main_session_id(Some(p[0].session.id()));
    st.update_pane_sizes(1000, 600);
    assert_eq!(policy::grid_pane_size(1, 100, 50), (100, 50));
    assert_eq!(policy::grid_pane_size(2, 100, 50), (50, 50));
    assert_eq!(policy::grid_pane_size(4, 100, 50), (50, 25));
    assert_eq!(policy::grid_pane_size(6, 99, 50), (33, 25));
}

// MARK: - 순수 정책

#[test]
fn decode_rank_and_budget_policies() {
    let ids: Vec<Uuid> = (0..5).map(|_| Uuid::new_v4()).collect();
    let mut mru = MruLedger::default();
    mru.touch(ids[1]);
    mru.touch(ids[3]);
    mru.note_added(ids[4], true);
    assert_eq!(mru.tick(ids[4]), 0);
    let inputs = policy::rank_inputs(&ids, &mru);
    assert_eq!(
        inputs[3],
        DecodeRankInput {
            id: ids[3],
            insertion_index: 3,
            tick: 2
        }
    );
    let audible: HashSet<Uuid> = [ids[2]].into_iter().collect();
    let ranked =
        policy::rank_for_decode_budget(&inputs, Some(ids[4]), Some(ids[0]), None, &audible);
    assert_eq!(ranked, vec![ids[4], ids[0], ids[2], ids[3], ids[1]]);
    let ranked = policy::rank_for_decode_budget(&inputs, None, None, Some(ids[1]), &HashSet::new());
    assert_eq!(ranked, vec![ids[1], ids[3], ids[0], ids[2], ids[4]]);

    assert_eq!(policy::effective_decode_budget(2, 4, None), 4);
    assert_eq!(policy::effective_decode_budget(6, 4, None), 6);
    assert_eq!(policy::effective_decode_budget(6, 4, Some(0)), 1);
    assert!(policy::has_session_capacity(3, 0, 4));
    assert!(!policy::has_session_capacity(3, 1, 4));
    assert!(policy::has_session_capacity(-2, -5, 0));
    assert!(policy::should_bypass_bandwidth_caps(false, true));
    assert!(!policy::should_include_in_bandwidth_active_set(
        &StreamPhase::Playing,
        true,
        false
    ));
    assert!(policy::should_include_in_bandwidth_active_set(
        &StreamPhase::Buffering,
        false,
        false
    ));
}

#[test]
fn eviction_victim_and_restart_policies() {
    let ids: Vec<Uuid> = (0..4).map(|_| Uuid::new_v4()).collect();
    let ordered = vec![
        EvictionCandidate {
            id: ids[0],
            tick: 1,
            is_audible: false,
        },
        EvictionCandidate {
            id: ids[1],
            tick: 0,
            is_audible: true,
        },
        EvictionCandidate {
            id: ids[2],
            tick: 3,
            is_audible: false,
        },
        EvictionCandidate {
            id: ids[3],
            tick: 1,
            is_audible: false,
        },
    ];
    assert_eq!(
        policy::watch_eviction_victim(&ordered, &HashSet::new()),
        Some(ids[0])
    );
    let pinned: HashSet<Uuid> = [ids[0], ids[3], ids[2]].into_iter().collect();
    assert_eq!(policy::watch_eviction_victim(&ordered, &pinned), None);

    use MultiLiveLoadState as L;
    let playing = L::Playing {
        channel_name: "a".into(),
        live_title: "b".into(),
    };
    assert!(policy::needs_restart(&L::Idle, &StreamPhase::Playing));
    assert!(policy::needs_restart(
        &L::Error("x".into()),
        &StreamPhase::Playing
    ));
    assert!(!policy::needs_restart(&L::Loading, &StreamPhase::Idle));
    assert!(policy::needs_restart(&playing, &StreamPhase::StreamEnded));
    assert!(!policy::needs_restart(&playing, &StreamPhase::Reconnecting));
    assert_eq!(policy::proactive_refresh_delay_secs(2.0), 3300.0);
    assert_eq!(policy::proactive_refresh_delay_secs(0.0), 3000.0);
}

#[tokio::test(start_paused = true)]
async fn zap_requests_are_capped_and_expire() {
    let m = manager();
    let mut st = m.lock();
    for i in 0..10 {
        st.note_zap_requested(&format!("ch-{i}"));
        tokio::time::advance(Duration::from_millis(10)).await;
    }
    assert!(!st.has_zap_request("ch-0") && !st.has_zap_request("ch-1"));
    assert!(st.has_zap_request("ch-9"));
    st.complete_watch_zap("ch-9", "ready");
    assert!(!st.has_zap_request("ch-9"));
    st.forget_zap_request("ch-8");
    assert!(!st.has_zap_request("ch-8"));
}

// MARK: - 레이아웃 선호

#[test]
fn layout_prefs_round_trip_and_restore() {
    let store = MemoryLayoutPrefsStore::default();
    let prefs = MultiLiveLayoutPrefs::new(
        MultiLiveGridLayoutMode::Focus,
        MultiLiveLayoutRatios {
            horizontal_ratio: 0.95,
            vertical_ratio: 0.1,
            bottom_row_ratio: 0.4,
            focus_ratio: 0.99,
        },
        MultiLiveFocusDirection::Top,
    );
    prefs.save(&store);
    prefs.save(&store);
    assert_eq!(store.write_count(), 1, "같은 값은 다시 쓰지 않는다");
    let json: serde_json::Value =
        serde_json::from_slice(&store.get(MultiLiveLayoutPrefs::USER_DEFAULTS_KEY).unwrap())
            .unwrap();
    assert_eq!(json["gridLayoutMode"], "focus");
    assert_eq!(json["focusDirection"], "top");

    let prefs_store = Arc::new(store);
    let m = manager_with(Arc::new(FakePool::default()), prefs_store.clone());
    let ml = MultiLiveSettings {
        remember_last_layout: true,
        ..MultiLiveSettings::default()
    };
    m.lock()
        .configure(None, Some(Arc::new(FakeSettings { ml })), None, None, None);
    let mut st = m.lock();
    st.apply_initial_layout_prefs_if_needed();
    assert_eq!(st.grid_layout_mode(), MultiLiveGridLayoutMode::Focus);
    assert_eq!(st.focus_direction(), MultiLiveFocusDirection::Top);
    let r = st.layout_ratios();
    assert_eq!(
        (r.horizontal_ratio, r.vertical_ratio, r.focus_ratio),
        (0.8, 0.2, 0.85)
    );

    let m2 = manager_with(Arc::new(FakePool::default()), prefs_store);
    let ml = MultiLiveSettings {
        remember_last_layout: false,
        default_layout_mode: MultiLiveLayoutMode::Focus,
        ..MultiLiveSettings::default()
    };
    m2.lock()
        .configure(None, Some(Arc::new(FakeSettings { ml })), None, None, None);
    let mut st2 = m2.lock();
    st2.apply_initial_layout_prefs_if_needed();
    assert_eq!(st2.grid_layout_mode(), MultiLiveGridLayoutMode::Focus);
    assert_eq!(st2.focus_direction(), MultiLiveFocusDirection::Left);
    assert!(st2.has_applied_layout_prefs());
}

// MARK: - 세션 추가·제거 (MultiLiveAdmissionPolicyTests)

#[tokio::test]
async fn watch_entry_preflight_and_api_unavailable_keeps_sessions() {
    let m = manager();
    let existing = pane("existing");
    m.lock().set_sessions(vec![existing.session.clone()]);
    assert_eq!(
        m.read().watch_entry_failure("new"),
        Some(MultiLiveAddSessionResult::ApiUnavailable)
    );
    assert_eq!(m.read().watch_entry_failure("existing"), None);

    let m = manager();
    let p = panes(4, "capacity");
    {
        let mut st = m.lock();
        st.set_grid_layout(true);
        st.set_sessions(p.iter().map(|x| x.session.clone()).collect());
        st.set_selected_session_id(Some(p[1].session.id()));
    }
    let r = m
        .add_session("one-more", AddSessionOptions::default(), None)
        .await;
    assert_eq!(r, MultiLiveAddSessionResult::ApiUnavailable);
    let order: Vec<_> = m.read().sessions().iter().map(|s| s.id()).collect();
    assert_eq!(order, ids(&p));
    assert_eq!(m.read().selected_session_id(), Some(p[1].session.id()));
    assert!(!m.read().is_adding_channel("one-more"));
}

fn live_manager(backend: Arc<FakeBackend>, pool: Arc<FakePool>) -> (MultiLiveManager, Arc<Toasts>) {
    let toasts = Arc::new(Toasts(Mutex::new(Vec::new())));
    let mut deps = MultiLiveDeps::new(pool, Arc::new(FakeFactory));
    deps.toast = Some(toasts.clone());
    let m = MultiLiveManager::new(deps);
    let ml = MultiLiveSettings::default();
    m.lock().configure(
        Some(backend),
        Some(Arc::new(FakeSettings { ml })),
        Some("uid".into()),
        Some("닉".into()),
        None,
    );
    m.lock().is_terminating = true;
    (m, toasts)
}

#[tokio::test]
async fn add_session_selects_first_and_evicts_lru_in_watch_mode() {
    let backend = Arc::new(FakeBackend::default());
    let pool = Arc::new(FakePool::default());
    let (m, toasts) = live_manager(backend.clone(), pool.clone());
    let opts = AddSessionOptions {
        start_immediately: false,
        ..AddSessionOptions::default()
    };
    let r = m.add_session("ch-0", opts, None).await;
    let MultiLiveAddSessionResult::Added { session_id: first } = r else {
        panic!("{r:?}");
    };
    {
        let st = m.read();
        assert_eq!(st.selected_session_id(), Some(first));
        assert_eq!(
            st.stage_session_id(),
            Some(first),
            "시청 모드 첫 세션은 스테이지"
        );
        assert_eq!(st.sessions()[0].channel_name(), "이름-ch-0");
        assert!(!st.sessions()[0].is_muted());
    }
    assert_eq!(
        m.add_session("ch-0", opts, None).await,
        MultiLiveAddSessionResult::ActivatedExisting
    );
    for i in 1..4 {
        assert!(m
            .add_session(&format!("ch-{i}"), opts, None)
            .await
            .is_new_session());
    }
    assert!(m.read().sessions()[1..]
        .iter()
        .all(|s| s.is_muted() && s.is_background()));
    assert!(!m.read().can_add_session());
    // ch-1 을 가장 오래 안 본 세션으로 만든다.
    let id_of = |c: &str| m.read().session_for_channel(c).unwrap().id();
    for c in ["ch-2", "ch-3", "ch-0"] {
        let id = id_of(c);
        m.lock().select_session(id, false);
    }
    let r = m.add_session("ch-4", opts, None).await;
    assert!(r.is_new_session(), "{r:?}");
    let channels: Vec<String> = m
        .read()
        .sessions()
        .iter()
        .map(|s| s.channel_id().to_string())
        .collect();
    assert_eq!(channels, vec!["ch-0", "ch-2", "ch-3", "ch-4"]);
    assert_eq!(
        toasts.0.lock().unwrap().as_slice(),
        ["채널 상한에 도달해 이름-ch-1 채널을 닫았습니다".to_string()]
    );
    assert_eq!(
        pool.released.load(Ordering::SeqCst),
        0,
        "팩토리 플레이어는 엔진을 쥐지 않는다"
    );

    // 그리드에서는 회수하지 않는다.
    m.lock().set_grid_layout(true);
    assert_eq!(
        m.add_session("ch-5", opts, None).await,
        MultiLiveAddSessionResult::MaxSessionsReached
    );
    // FollowingWorkspaceHost 표면.
    let host: &dyn FollowingWorkspaceHost = &m;
    assert_eq!(host.session_channel_ids().len(), 4);
    assert_eq!(host.session_id_for_channel("ch-4"), Some(id_of("ch-4")));
    assert!(host.is_grid_layout());
}

#[tokio::test]
async fn watch_transition_rejects_closed_and_superseded_requests() {
    let backend = Arc::new(FakeBackend::default());
    backend.closed.lock().unwrap().insert("off".into());
    let (m, _) = live_manager(backend.clone(), Arc::new(FakePool::default()));
    let opts = AddSessionOptions {
        start_immediately: false,
        watch_transition: true,
        ..AddSessionOptions::default()
    };
    assert_eq!(
        m.add_session("off", opts, None).await,
        MultiLiveAddSessionResult::NotLive
    );
    let stale = || false;
    assert_eq!(
        m.add_session("on", opts, Some(&stale)).await,
        MultiLiveAddSessionResult::Cancelled
    );
    let r = m.add_session("on", opts, None).await;
    assert!(r.is_new_session());
    let st = m.read();
    assert_eq!(
        st.selected_session_id(),
        None,
        "시청 전환은 자동 선택하지 않는다"
    );
    assert!(st.sessions()[0].is_muted());
}

#[tokio::test(start_paused = true)]
async fn live_detail_timeout_returns_engine() {
    let backend = Arc::new(FakeBackend::default());
    backend.hang.store(true, Ordering::SeqCst);
    let pool = Arc::new(FakePool::default());
    let (m, _) = live_manager(backend, pool.clone());
    let r = m
        .add_session("slow", AddSessionOptions::default(), None)
        .await;
    assert_eq!(r, MultiLiveAddSessionResult::LiveDetailTimeout);
    assert_eq!(pool.released.load(Ordering::SeqCst), 1);
    assert!(m.read().sessions().is_empty());
}

#[tokio::test]
async fn stop_all_tears_down_and_resets_layout() {
    let backend = Arc::new(FakeBackend::default());
    let pool = Arc::new(FakePool::default());
    let (m, _) = live_manager(backend, pool.clone());
    let opts = AddSessionOptions {
        start_immediately: false,
        ..AddSessionOptions::default()
    };
    m.add_session("a", opts, None).await;
    m.add_session("b", opts, None).await;
    {
        let mut st = m.lock();
        st.set_grid_layout(true);
        st.apply_grid_layout_mode(MultiLiveGridLayoutMode::Focus);
        st.toggle_multi_audio_mode();
    }
    m.stop_all().await;
    let st = m.read();
    assert!(st.sessions().is_empty());
    assert_eq!(st.selected_session_id(), None);
    assert!(!st.is_grid_layout() && !st.is_multi_audio_mode());
    assert_eq!(st.grid_layout_mode(), MultiLiveGridLayoutMode::Preset);
    assert_eq!(st.stage_session_id(), None);
    assert!(!st.is_stopping_all());
    assert_eq!(pool.drained.load(Ordering::SeqCst), 1);
    drop(st);
    let snap = m.snapshot();
    assert!(snap.can_add_session && snap.sessions.is_empty());
}

// MARK: - 세션 수명주기

#[tokio::test]
async fn session_start_plays_and_stop_resets() {
    let backend = Arc::new(FakeBackend::default());
    let player = FakePlayer::new(Some(Arc::new(FakeEngine::default())));
    let chat = Arc::new(FakeChat::default());
    let session = MultiLiveSession::with_live_info(
        "s1",
        "이름",
        None,
        live("s1", LiveStatus::Open),
        backend.clone(),
        player.clone(),
        chat.clone(),
    );
    session.start(1).await;
    assert_eq!(
        session.load_state(),
        MultiLiveLoadState::Playing {
            channel_name: "이름-s1".into(),
            live_title: "s1 방송".into()
        }
    );
    assert_eq!(
        backend.detail_calls.load(Ordering::SeqCst),
        0,
        "미리 받은 LiveInfo 를 쓴다"
    );
    assert_eq!(
        player.starts.lock().unwrap()[0].stream_url,
        "https://example.invalid/s1.m3u8"
    );
    {
        let st = session.read();
        assert_eq!(st.viewer_count(), 1234);
        assert_eq!(st.formatted_viewer_count(), "1.2천");
        assert!(st.has_poll_task());
    }
    session.stop().await;
    assert_eq!(session.load_state(), MultiLiveLoadState::Idle);
    assert_eq!(chat.disconnects.load(Ordering::SeqCst), 1);
    assert!(player.stops.load(Ordering::SeqCst) >= 1);

    session.invalidate_for_removal();
    session.start(1).await;
    assert_eq!(
        session.load_state(),
        MultiLiveLoadState::Idle,
        "무효화 뒤에는 시작하지 않는다"
    );
}

#[tokio::test]
async fn session_plan_failure_and_missing_backend() {
    let backend = Arc::new(FakeBackend::default());
    backend.plan_error.store(true, Ordering::SeqCst);
    let session = MultiLiveSession::with_live_info(
        "s2",
        "이름",
        None,
        live("s2", LiveStatus::Open),
        backend,
        FakePlayer::new(None),
        Arc::new(FakeChat::default()),
    );
    session.start(1).await;
    assert_eq!(
        session.load_state(),
        MultiLiveLoadState::Error("HLS 스트림 URL을 찾을 수 없습니다.".into())
    );
    let bare = MultiLiveSession::new("s3", FakePlayer::new(None), Arc::new(FakeChat::default()));
    bare.start(1).await;
    assert_eq!(
        bare.load_state(),
        MultiLiveLoadState::Error("API 클라이언트 없음".into())
    );
}

#[tokio::test(start_paused = true)]
async fn session_poll_marks_offline_when_closed() {
    let backend = Arc::new(FakeBackend::default());
    let player = FakePlayer::new(None);
    let session = MultiLiveSession::with_live_info(
        "s4",
        "이름",
        None,
        live("s4", LiveStatus::Open),
        backend.clone(),
        player.clone(),
        Arc::new(FakeChat::default()),
    );
    session.start(1).await;
    backend.status_closed.store(true, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_secs(61)).await;
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert_eq!(session.load_state(), MultiLiveLoadState::Offline);
    let st = session.read();
    assert!(st.is_offline);
    assert_eq!(st.viewer_count(), 99);
    assert_eq!(st.accumulate_count, 500);
}

#[test]
fn background_mode_and_snapshot() {
    let p = pane("snap");
    p.session.set_background_mode(true);
    p.session.set_chat_presentation_managed_by_multi_chat(true);
    assert!(p.session.is_background());
    let snap = p.session.snapshot();
    assert!(snap.is_background);
    assert_eq!(snap.formatted_viewer_count, "0");
    let json = serde_json::to_value(&snap).unwrap();
    assert_eq!(json["loadState"]["state"], "idle");
    let err = serde_json::to_value(MultiLiveLoadState::Error("x".into())).unwrap();
    assert_eq!(err["detail"], "x");
}
