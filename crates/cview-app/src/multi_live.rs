//! 멀티라이브 셸 — app-core `MultiLiveManager` 에 Tauri/WebView 어댑터를 붙인다.
//!
//! 원본: `Sources/CViewApp/ViewModels/MultiLive*.swift`, `Sources/CViewPlayer/MultiLiveEnginePool.swift`.
//! 세션·선택·오디오 라우팅·대역폭 분배·레이아웃 판단은 매니저가 하고, 여기서는
//!
//! - 패인 엔진([`PaneEngine`]) — 슬롯 번호가 붙은 [`WebViewHlsEngine`]. 명령은 `multilive://command`
//!   `{ slot, script }` 로 나가고 프런트엔드의 같은 슬롯 iframe 이 실행한다.
//! - 엔진 풀([`PanePool`]) — 슬롯을 만들고 되돌려 받으면 멈춰 두었다가 다시 쓴다.
//! - 패인 플레이어([`PanePlayer`]) — 패인마다 `PlayerViewModel`(코디네이터·프록시·저지연 제어 포함).
//! - 채팅([`PaneChat`]) — 채팅 채널 ID 만 기억한다. 실제 연결은 프런트엔드가 선택 패인의 채널로
//!   기존 채팅 패널(`chat_connect`)을 다시 붙인다(오디오와 채팅이 포커스를 따라간다).
//! - 백엔드([`ShellBackend`]) — 라이브 상세·재생 계획·상태 조회.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cview_app_core::viewmodels::multi_live::deps::{
    ChatConnectionKind, LaunchPlan, LaunchPlanError, LiveStatusSnapshot, MultiLiveBackendError,
    MultiLiveChat, MultiLiveEnginePool, MultiLiveHlsEngine, MultiLivePlayer,
    MultiLiveSessionBackend, MultiLiveSessionFactory, MultiLiveSettingsSource,
    MultiLiveStreamRequest, SharedHlsEngine, StreamEndedProbe,
};
use cview_app_core::viewmodels::multi_live::layout::LayoutPrefsStore;
use cview_app_core::viewmodels::multi_live::{
    MultiLiveAddSessionResult, MultiLiveDeps, MultiLiveFocusDirection, MultiLiveGridLayoutMode,
    MultiLiveLayoutRatios, MultiLiveLoadState, MultiLiveManager,
};
use cview_app_core::viewmodels::player::{PlayerCallbacks, PlayerViewModel, StartStreamRequest};
use cview_auth::CookieManager;
use cview_core::models::live::LiveStatus;
use cview_core::models::settings::PlayerSettings;
use cview_core::models::settings_ui::MultiLiveSettings;
use cview_core::models::LiveInfo;
use cview_core::protocols::PlayerEngineProtocol;
use cview_networking::ChzzkApiClient;
use cview_persistence::SettingsStore;
use cview_player::coordinator::StreamPhase;
use cview_player::hlsjs::network_metrics::StreamTransferSample;
use cview_player::hlsjs::profile::HlsJsForcedQualityTarget;
use cview_player::hlsjs::JsCommand;
use cview_player::policy::{PlaybackCookie, PlaybackKeyAccess, SessionTier};
use serde::Serialize;
use url::Url;

use crate::engine::{CommandSink, WebViewHlsEngine};
use crate::playback::{self, PlaybackPlanError};
use crate::state::PLAYER_PROFILE;

/// 패인 슬롯 스크립트 싱크 — `(slot, script)`.
pub type SlotCommandSink = Arc<dyn Fn(u32, String) + Send + Sync>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// `Arc<dyn Trait>` 와 구체 `Arc<T>` 가 같은 객체인가(데이터 포인터 비교).
fn same_object<A: ?Sized, B>(a: &Arc<A>, b: &Arc<B>) -> bool {
    std::ptr::eq(Arc::as_ptr(a) as *const u8, Arc::as_ptr(b) as *const u8)
}

// MARK: - 엔진

/// 매니저가 패인 엔진에 건 화질·버퍼 상한.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct PaneCaps {
    max_height: i64,
    max_bitrate_kbps: i64,
}

/// 슬롯 번호가 붙은 WebView hls.js 엔진.
pub struct PaneEngine {
    slot: u32,
    engine: Arc<WebViewHlsEngine>,
    caps: Mutex<PaneCaps>,
    playback_desired: AtomicBool,
    /// 숨김 단계(`SessionTier::Hidden`)가 멈춘 로딩 — 이 단계가 멈춘 것만 되돌린다(디코드 예산 정지는 건드리지 않는다).
    tier_suspended: AtomicBool,
}

impl PaneEngine {
    fn new(slot: u32, sink: SlotCommandSink) -> Self {
        let command: CommandSink = Arc::new(move |script| sink(slot, script));
        Self {
            slot,
            engine: Arc::new(WebViewHlsEngine::new(command, PLAYER_PROFILE, false)),
            caps: Mutex::new(PaneCaps::default()),
            playback_desired: AtomicBool::new(true),
            tier_suspended: AtomicBool::new(false),
        }
    }

    pub fn slot(&self) -> u32 {
        self.slot
    }

    pub fn engine(&self) -> &Arc<WebViewHlsEngine> {
        &self.engine
    }

    /// 매니저가 마지막으로 건 높이 상한(0 = 없음).
    pub fn max_height_cap(&self) -> i64 {
        lock(&self.caps).max_height
    }

    fn send_caps(&self, caps: PaneCaps) {
        self.engine
            .set_managed_quality_caps(caps.max_height, caps.max_bitrate_kbps);
    }

    /// 풀로 돌아올 때 — 재생을 멈추고 상한을 푼다.
    fn reset(&self) {
        PlayerEngineProtocol::stop(self.engine.as_ref());
        *lock(&self.caps) = PaneCaps::default();
        self.engine.set_managed_quality_caps(0, 0);
        self.engine.clear_suspend_intent();
        self.playback_desired.store(true, Ordering::SeqCst);
        self.tier_suspended.store(false, Ordering::SeqCst);
    }
}

impl MultiLiveHlsEngine for PaneEngine {
    fn is_playing(&self) -> bool {
        PlayerEngineProtocol::is_playing(self.engine.as_ref())
    }
    fn is_suspend_intent_active(&self) -> bool {
        PlayerEngineProtocol::is_suspend_intent_active(self.engine.as_ref())
    }
    fn is_in_error_state(&self) -> bool {
        PlayerEngineProtocol::is_in_error_state(self.engine.as_ref())
    }
    fn is_loading_suspended(&self) -> bool {
        PlayerEngineProtocol::is_suspend_intent_active(self.engine.as_ref())
    }
    fn is_playback_desired(&self) -> bool {
        self.playback_desired.load(Ordering::SeqCst)
    }
    fn buffer_ahead_seconds(&self) -> f64 {
        self.engine.snapshot().buffer_health
    }
    fn current_bitrate(&self) -> f64 {
        0.0
    }
    fn rate(&self) -> f64 {
        f64::from(self.engine.snapshot().rate)
    }
    fn transfers(&self) -> Vec<StreamTransferSample> {
        Vec::new()
    }
    fn measured_segment_duration(&self) -> f64 {
        self.engine.snapshot().segment_duration
    }
    fn force_highest_quality(&self) -> bool {
        use cview_app_core::viewmodels::player::engine::LivePlayerEngine;
        LivePlayerEngine::force_highest_quality(self.engine.as_ref())
    }
    fn forced_height_active(&self) -> bool {
        false
    }
    fn max_adaptive_height(&self) -> i64 {
        lock(&self.caps).max_height
    }
    fn max_adaptive_bitrate(&self) -> i64 {
        lock(&self.caps).max_bitrate_kbps
    }
    fn set_max_resolution_height(&self, height: i64) {
        let caps = {
            let mut c = lock(&self.caps);
            if c.max_height == height {
                return;
            }
            c.max_height = height;
            *c
        };
        tracing::debug!(slot = self.slot, height, "멀티라이브 패인 높이 상한");
        self.send_caps(caps);
    }
    fn set_max_bitrate(&self, kbps: i64) {
        let caps = {
            let mut c = lock(&self.caps);
            if c.max_bitrate_kbps == kbps {
                return;
            }
            c.max_bitrate_kbps = kbps;
            *c
        };
        self.send_caps(caps);
    }
    /// 원본 GPU 렌더 단계 — 이 앱에서는 숨김(메뉴 이탈 뒤 해제)만 의미가 있다: 로딩을 멈추고(뒤 버퍼 유지),
    /// 보이는 단계로 돌아오면 그 정지만 푼다.
    fn set_gpu_render_tier(&self, tier: SessionTier) {
        if tier == SessionTier::Hidden {
            if !self.is_loading_suspended() {
                self.tier_suspended.store(true, Ordering::SeqCst);
                self.suspend_loading();
            }
        } else if self.tier_suspended.swap(false, Ordering::SeqCst) {
            self.resume_loading();
        }
    }
    fn set_video_surface_visible(&self, _visible: bool) {}
    fn pause(&self) {
        self.playback_desired.store(false, Ordering::SeqCst);
        PlayerEngineProtocol::pause(self.engine.as_ref());
    }
    fn resume(&self) {
        self.playback_desired.store(true, Ordering::SeqCst);
        PlayerEngineProtocol::resume(self.engine.as_ref());
    }
    fn resume_loading(&self) {
        self.engine.resume_loading();
    }
    fn suspend_loading(&self) {
        self.engine.suspend_loading();
    }
    fn set_buffer_caps(&self, max_len: f64, max_max_len: f64, max_size: i64) {
        self.engine.send(JsCommand::SetBufferCaps {
            max_len,
            max_max_len,
            max_size,
        });
    }
    fn arm_surface_presented_probe(&self) {}
    fn reset_retries(&self) {
        PlayerEngineProtocol::reset_retries(self.engine.as_ref());
    }
}

// MARK: - 엔진 풀

/// 슬롯 엔진 풀 (`MultiLiveEnginePool`).
pub struct PanePool {
    sink: SlotCommandSink,
    next_slot: AtomicU32,
    max: Mutex<usize>,
    active: Mutex<Vec<Arc<PaneEngine>>>,
    idle: Mutex<Vec<Arc<PaneEngine>>>,
}

impl PanePool {
    pub fn new(sink: SlotCommandSink) -> Self {
        Self {
            sink,
            next_slot: AtomicU32::new(1),
            max: Mutex::new(MultiLiveSettings::default().max_concurrent_sessions.max(1) as usize),
            active: Mutex::new(Vec::new()),
            idle: Mutex::new(Vec::new()),
        }
    }

    /// 사용 중인 슬롯 엔진(구체 타입).
    pub fn find_active(&self, engine: &SharedHlsEngine) -> Option<Arc<PaneEngine>> {
        lock(&self.active)
            .iter()
            .find(|e| same_object(engine, e))
            .cloned()
    }

    fn take_active(&self, engine: &SharedHlsEngine) -> Option<Arc<PaneEngine>> {
        let mut active = lock(&self.active);
        let index = active.iter().position(|e| same_object(engine, e))?;
        Some(active.remove(index))
    }

    pub fn active_slots(&self) -> Vec<u32> {
        lock(&self.active).iter().map(|e| e.slot).collect()
    }
}

#[async_trait]
impl MultiLiveEnginePool for PanePool {
    async fn set_max_pool_size(&self, size: usize) {
        *lock(&self.max) = size.max(1);
    }
    async fn acquire(&self) -> Option<SharedHlsEngine> {
        if lock(&self.active).len() >= *lock(&self.max) {
            return None;
        }
        let engine = lock(&self.idle).pop().unwrap_or_else(|| {
            let slot = self.next_slot.fetch_add(1, Ordering::SeqCst);
            Arc::new(PaneEngine::new(slot, self.sink.clone()))
        });
        lock(&self.active).push(engine.clone());
        Some(engine)
    }
    async fn release(&self, engine: SharedHlsEngine) {
        if let Some(e) = self.take_active(&engine) {
            e.reset();
            lock(&self.idle).push(e);
        }
    }
    async fn discard(&self, engine: SharedHlsEngine) {
        if let Some(e) = self.take_active(&engine) {
            e.reset();
        }
    }
    async fn release_slot(&self) {}
    async fn total_active_count(&self) -> usize {
        lock(&self.active).len()
    }
    async fn trim_idle(&self, keep_per_type: usize) {
        lock(&self.idle).truncate(keep_per_type);
    }
    async fn drain(&self) {
        let engines: Vec<_> = lock(&self.active).drain(..).collect();
        for e in engines {
            e.reset();
        }
        lock(&self.idle).clear();
    }
    async fn warmup(&self, _count: usize) {}
}

// MARK: - 플레이어

/// 암호화 방송 채널 — 재생 키 요청이 인증 프록시를 거쳐야 한다(백엔드가 계획을 만들 때 기록).
type AuthKeyChannels = Arc<Mutex<HashMap<String, bool>>>;

/// 재생 상태 변화 콜백 자리(세션이 나중에 건다).
type StateChangedSlot = Arc<Mutex<Option<Arc<dyn Fn() + Send + Sync>>>>;

/// 패인 플레이어 — 패인마다 `PlayerViewModel` 하나.
pub struct PanePlayer {
    vm: PlayerViewModel,
    engine: Mutex<Option<Arc<PaneEngine>>>,
    cookies: Arc<CookieManager>,
    auth_key_channels: AuthKeyChannels,
    on_state_changed: StateChangedSlot,
    presented_ticks: AtomicU64,
}

impl PanePlayer {
    fn new(
        engine: Option<Arc<PaneEngine>>,
        cookies: Arc<CookieManager>,
        auth_key_channels: AuthKeyChannels,
        settings: &PlayerSettings,
    ) -> Arc<Self> {
        let vm = PlayerViewModel::new(None);
        // 패인은 멀티라이브 플레이어다 — 빠뜨리면 모든 패인이 단독 시청처럼 각자 3초를 쫓는 PID 를 돌리고
        // (Automatic), 비선택 안정성 목표(10.5초)·선택 전환·역할별 화질 목표가 하나도 걸리지 않았다.
        vm.set_multi_live(true);
        if let Some(e) = &engine {
            vm.inject_engine(e.engine.clone());
        }
        vm.apply_settings(settings, false);
        let on_state_changed: StateChangedSlot = Arc::new(Mutex::new(None));
        let slot = on_state_changed.clone();
        vm.set_callbacks(PlayerCallbacks {
            on_playback_state_changed: Some(Arc::new(move || {
                let cb = lock(&slot).clone();
                if let Some(cb) = cb {
                    cb();
                }
            })),
            ..PlayerCallbacks::default()
        });
        Arc::new(Self {
            vm,
            engine: Mutex::new(engine),
            cookies,
            auth_key_channels,
            on_state_changed,
            presented_ticks: AtomicU64::new(0),
        })
    }

    pub fn slot(&self) -> Option<u32> {
        lock(&self.engine).as_ref().map(|e| e.slot)
    }

    pub fn pane_engine(&self) -> Option<Arc<PaneEngine>> {
        lock(&self.engine).clone()
    }

    pub fn view_model(&self) -> &PlayerViewModel {
        &self.vm
    }

    /// 플레이어 페이지 메시지를 엔진에 반영하고 뷰모델에 신호를 넘긴다.
    pub async fn handle_page_message(&self, message: &cview_player::hlsjs::JsMessage) {
        let Some(engine) = self.pane_engine() else {
            return;
        };
        let signal = engine.engine.handle_message(message);
        if matches!(
            message,
            cview_player::hlsjs::JsMessage::SurfaceFirstFrame { .. }
        ) {
            self.presented_ticks.fetch_add(1, Ordering::SeqCst);
            self.vm.engine_surface_presented();
        }
        crate::commands::player::apply_engine_signal(&self.vm, signal).await;
        if matches!(message, cview_player::hlsjs::JsMessage::PlayStarted { .. }) {
            self.presented_ticks.fetch_add(1, Ordering::SeqCst);
        }
    }
}

#[async_trait]
impl MultiLivePlayer for PanePlayer {
    fn engine(&self) -> Option<SharedHlsEngine> {
        lock(&self.engine).clone().map(|e| e as SharedHlsEngine)
    }
    fn detach_engine(&self) -> Option<SharedHlsEngine> {
        self.vm.detach_engine();
        lock(&self.engine).take().map(|e| e as SharedHlsEngine)
    }
    fn is_muted(&self) -> bool {
        self.vm.read().is_muted
    }
    fn set_muted(&self, muted: bool) {
        self.vm.set_muted(muted);
    }
    fn stream_phase(&self) -> StreamPhase {
        self.vm.read().stream_phase.clone()
    }
    fn presented_frame_tick(&self) -> u64 {
        self.presented_ticks.load(Ordering::SeqCst)
    }
    fn error_message(&self) -> Option<String> {
        self.vm.read().error_message.clone()
    }
    fn set_precision_sync_active(&self, active: bool) {
        self.vm.set_precision_sync_active(active);
    }
    fn rewind_recording_desired(&self) -> bool {
        false
    }
    fn set_rewind_recording_desired(&self, _desired: bool) {}
    fn multi_live_quality_target(&self) -> HlsJsForcedQualityTarget {
        self.vm.read().multi_live_quality_target()
    }
    fn set_multi_live_quality_target(&self, target: HlsJsForcedQualityTarget) -> bool {
        self.vm.set_multi_live_quality_target(target)
    }
    fn apply_picture_enhancement(&self, auto: bool, upscale: bool) {
        self.vm.apply_picture_enhancement(auto, upscale);
    }
    fn recover_from_background(&self) {
        self.vm.recover_from_background();
    }
    fn promote_for_in_app_selection(&self) {
        self.vm.promote_for_in_app_selection();
    }
    fn sync_network_activity(&self) {
        self.vm.sync_network_activity();
    }
    fn resume_watch_heartbeat_for_menu_return(&self) {}
    fn suspend_watch_heartbeat_for_menu_leave(&self) {}
    fn restore_surface_after_occlusion(&self) {}
    fn set_gpu_render_tier_hidden(&self, _keep_alive: bool) {}
    fn set_max_rate_override(&self, _rate: Option<f64>) {}
    fn set_latency_margin(&self, seconds: f64) {
        self.vm.set_latency_margin(seconds);
    }
    fn configure_time_machine_access(&self, is_cheat_key_user: bool, available: bool) {
        self.vm
            .configure_time_machine_access(is_cheat_key_user, available);
    }
    fn is_time_machine_available(&self) -> bool {
        false
    }
    fn apply_started_settings(&self, settings: &PlayerSettings) {
        self.vm.apply_settings(settings, true);
    }
    fn set_on_playback_state_changed(&self, callback: Option<Arc<dyn Fn() + Send + Sync>>) {
        *lock(&self.on_state_changed) = callback;
    }
    fn set_stream_ended_probe(&self, _probe: Option<StreamEndedProbe>) {}
    async fn start_stream(&self, request: MultiLiveStreamRequest) {
        let Ok(url) = Url::parse(&request.stream_url) else {
            tracing::warn!("멀티라이브: 스트림 URL 해석 실패");
            return;
        };
        let requires_key = lock(&self.auth_key_channels)
            .get(&request.channel_id)
            .copied()
            .unwrap_or(false);
        let cookies = self.cookies.clone();
        let access = PlaybackKeyAccess::new(
            requires_key,
            Arc::new(move || {
                cookies
                    .auth_cookies()
                    .into_iter()
                    .map(|c| PlaybackCookie {
                        name: c.name,
                        value: c.value,
                        domain: c.domain,
                        path: c.path,
                        expires: c.expires,
                    })
                    .collect()
            }),
        );
        let mut start = StartStreamRequest::new(request.channel_id.clone(), url);
        start.channel_name = request.channel_name;
        start.live_title = request.live_title;
        start.playback_key_access = access;
        start.match_web_v2_source_eligible = request.match_web_v2_source_eligible;
        self.vm.start_stream(start).await;
    }
    async fn stop_stream(&self) {
        self.vm.stop_stream().await;
    }
    async fn toggle_play_pause(&self) {
        self.vm.toggle_play_pause().await;
    }
}

// MARK: - 채팅

/// 패인 채팅 — 채팅 채널 ID 만 기억한다.
///
/// 매니저는 다른 메뉴로 나가 2.5초가 지나면 패인 채팅을 끊고(`SuspendChat` → `disconnect`) 돌아오면
/// `reconnect` 로 되살린다. 예전에는 끊을 때 채널 ID 를 지우고 `reconnect` 는 아무것도 하지 않아, 라이브 시청
/// 화면에 잠깐 다녀오면 상태 DTO 의 `chatChannelId` 가 영영 비어 멀티라이브 채팅(선택 패인·멀티채팅)이 끊긴 채
/// 남았다. 이제 채널 ID 는 남기고 연결 여부만 내린다 — 실제 소켓은 프런트엔드 채팅 패널이 가진다.
#[derive(Default)]
pub struct PaneChat {
    chat_channel_id: Mutex<Option<String>>,
    suspended: AtomicBool,
}

impl PaneChat {
    pub fn chat_channel_id(&self) -> Option<String> {
        lock(&self.chat_channel_id).clone()
    }

    fn set_chat_channel_id(&self, chat_channel_id: &str) {
        *lock(&self.chat_channel_id) = Some(chat_channel_id.to_string());
        self.suspended.store(false, Ordering::SeqCst);
    }
}

#[async_trait]
impl MultiLiveChat for PaneChat {
    fn connection_state(&self) -> ChatConnectionKind {
        if lock(&self.chat_channel_id).is_some() && !self.suspended.load(Ordering::SeqCst) {
            ChatConnectionKind::Other
        } else {
            ChatConnectionKind::Disconnected
        }
    }
    fn has_last_connection_config(&self) -> bool {
        lock(&self.chat_channel_id).is_some()
    }
    fn set_background_mode(&self, _background: bool) {}
    fn set_user(&self, _uid: Option<String>, _nickname: Option<String>) {}
    fn set_live_metadata(
        &self,
        _viewer_count: Option<i64>,
        _accumulate_count: Option<i64>,
        _open_date: Option<DateTime<Utc>>,
    ) {
    }
    fn restore_owner_presentation(&self, _background: bool) {}
    async fn disconnect(&self) {
        self.suspended.store(true, Ordering::SeqCst);
    }
    async fn reconnect(&self) {
        if lock(&self.chat_channel_id).is_some() {
            self.suspended.store(false, Ordering::SeqCst);
        }
    }
}

// MARK: - 팩토리·백엔드·설정

/// 만든 플레이어·채팅 등록부 — 페이지 메시지 라우팅과 상태 DTO 에 쓴다.
#[derive(Default)]
pub struct PaneRegistry {
    players: Mutex<Vec<Arc<PanePlayer>>>,
    chats: Mutex<Vec<Arc<PaneChat>>>,
}

impl PaneRegistry {
    pub fn player_for_slot(&self, slot: u32) -> Option<Arc<PanePlayer>> {
        lock(&self.players)
            .iter()
            .find(|p| p.slot() == Some(slot))
            .cloned()
    }

    pub fn player_of(&self, player: &Arc<dyn MultiLivePlayer>) -> Option<Arc<PanePlayer>> {
        lock(&self.players)
            .iter()
            .find(|p| same_object(player, p))
            .cloned()
    }

    pub fn chat_of(&self, chat: &Arc<dyn MultiLiveChat>) -> Option<Arc<PaneChat>> {
        lock(&self.chats)
            .iter()
            .find(|c| same_object(chat, c))
            .cloned()
    }

    /// 매니저에 남은 세션의 것만 남긴다.
    pub fn retain(&self, players: &[Arc<dyn MultiLivePlayer>], chats: &[Arc<dyn MultiLiveChat>]) {
        lock(&self.players).retain(|p| players.iter().any(|x| same_object(x, p)));
        lock(&self.chats).retain(|c| chats.iter().any(|x| same_object(x, c)));
    }
}

/// 세션 팩토리.
pub struct ShellSessionFactory {
    pool: Arc<PanePool>,
    registry: Arc<PaneRegistry>,
    cookies: Arc<CookieManager>,
    settings: SettingsStore,
    auth_key_channels: AuthKeyChannels,
}

impl MultiLiveSessionFactory for ShellSessionFactory {
    fn make_player(&self, engine: Option<SharedHlsEngine>) -> Arc<dyn MultiLivePlayer> {
        let pane = engine.as_ref().and_then(|e| self.pool.find_active(e));
        let player = PanePlayer::new(
            pane,
            self.cookies.clone(),
            self.auth_key_channels.clone(),
            &self.settings.player(),
        );
        lock(&self.registry.players).push(player.clone());
        player
    }
    fn make_chat(&self) -> Arc<dyn MultiLiveChat> {
        let chat = Arc::new(PaneChat::default());
        lock(&self.registry.chats).push(chat.clone());
        chat
    }
}

/// 재생 계획 오류 → 멀티라이브 오류.
pub fn launch_plan_error(e: &PlaybackPlanError) -> LaunchPlanError {
    match e {
        PlaybackPlanError::NoHlsUrl => LaunchPlanError::NoHlsUrl,
        _ => LaunchPlanError::MissingPlaybackJson,
    }
}

/// 세션 백엔드.
pub struct ShellBackend {
    api: ChzzkApiClient,
    settings: SettingsStore,
    registry: Arc<PaneRegistry>,
    auth_key_channels: AuthKeyChannels,
}

#[async_trait]
impl MultiLiveSessionBackend for ShellBackend {
    async fn live_detail(&self, channel_id: &str) -> Result<LiveInfo, MultiLiveBackendError> {
        self.api
            .live_detail(channel_id, true)
            .await
            .map_err(|e| MultiLiveBackendError::Message(e.to_string()))
    }
    fn build_plan(&self, live: &LiveInfo, channel_id: &str) -> Result<LaunchPlan, LaunchPlanError> {
        let plan = playback::resolve(live).map_err(|e| launch_plan_error(&e))?;
        lock(&self.auth_key_channels)
            .insert(channel_id.to_string(), plan.requires_authenticated_key);
        Ok(LaunchPlan {
            stream_url: plan.stream_url.to_string(),
            is_audio_only: plan.is_audio_only,
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
        channel_id: &str,
    ) -> Result<LiveStatusSnapshot, MultiLiveBackendError> {
        let info = self
            .api
            .live_status(channel_id)
            .await
            .map_err(|e| MultiLiveBackendError::Message(e.to_string()))?;
        Ok(LiveStatusSnapshot {
            is_closed: info.status != LiveStatus::Open,
            concurrent_user_count: info.concurrent_user_count,
            accumulate_count: info.accumulate_count,
            open_date: info.open_date,
        })
    }
    async fn connect_chat(
        &self,
        chat: Arc<dyn MultiLiveChat>,
        chat_channel_id: &str,
        _stream_channel_id: &str,
        _channel_name: &str,
    ) -> Result<(), String> {
        if let Some(pane) = self.registry.chat_of(&chat) {
            pane.set_chat_channel_id(chat_channel_id);
        }
        Ok(())
    }
    fn player_settings(&self) -> PlayerSettings {
        self.settings.player()
    }
}

/// 설정 공급자.
pub struct ShellSettings(SettingsStore);

impl MultiLiveSettingsSource for ShellSettings {
    fn multi_live(&self) -> MultiLiveSettings {
        self.0.multi_live()
    }
    fn player(&self) -> PlayerSettings {
        self.0.player()
    }
}

// MARK: - 레이아웃 저장

/// 마지막 레이아웃(모드·비율·포커스 방향)을 데이터 폴더의 JSON 파일로 둔다
/// (Swift UserDefaults `multiLiveLayoutPrefs`).
pub struct FileLayoutPrefsStore {
    dir: std::path::PathBuf,
}

impl FileLayoutPrefsStore {
    pub fn new(dir: impl Into<std::path::PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self, key: &str) -> std::path::PathBuf {
        let safe: String = key
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        self.dir.join(format!("{safe}.json"))
    }
}

impl LayoutPrefsStore for FileLayoutPrefsStore {
    fn get(&self, key: &str) -> Option<Vec<u8>> {
        std::fs::read(self.path(key)).ok()
    }
    fn set(&self, key: &str, data: Vec<u8>) {
        // 원자적으로(임시 파일 + 이름 바꾸기) — 쓰는 중 앱이 죽어도 배치 파일이 잘리지 않게.
        if let Err(e) = cview_persistence::json::write_atomic(&self.path(key), &data) {
            tracing::warn!("멀티라이브 레이아웃 저장 실패: {e}");
        }
    }
}

/// 포커스 레이아웃 변경 요청을 정리한다 — 방향은 알려진 값만, 비율은 허용 범위로 자른다.
pub fn focus_layout_update(
    direction: Option<&str>,
    ratio: Option<f64>,
) -> (Option<MultiLiveFocusDirection>, Option<f64>) {
    (
        direction.and_then(MultiLiveFocusDirection::from_raw),
        ratio
            .filter(|r| r.is_finite())
            .map(MultiLiveLayoutRatios::clamped_focus),
    )
}

// MARK: - 셸

/// 멀티라이브 셸 상태 — Tauri 관리 상태.
pub struct MultiLiveShell {
    pub manager: MultiLiveManager,
    pub pool: Arc<PanePool>,
    pub registry: Arc<PaneRegistry>,
    /// 라이브 메뉴 단독 보기 중(`multilive_watch(Some)`) — 아니면 멀티 허브 규칙(그리드·무대 없음)을 지킨다.
    pub watching: AtomicBool,
    rev: AtomicU64,
    last_state: Mutex<Option<MultiLiveStateDto>>,
}

impl MultiLiveShell {
    pub fn new(
        api: ChzzkApiClient,
        cookies: Arc<CookieManager>,
        settings: SettingsStore,
        sink: SlotCommandSink,
        data_dir: &std::path::Path,
    ) -> Self {
        let pool = Arc::new(PanePool::new(sink));
        let registry = Arc::new(PaneRegistry::default());
        let auth_key_channels: AuthKeyChannels = Arc::default();
        let factory = Arc::new(ShellSessionFactory {
            pool: pool.clone(),
            registry: registry.clone(),
            cookies,
            settings: settings.clone(),
            auth_key_channels: auth_key_channels.clone(),
        });
        let mut deps = MultiLiveDeps::new(pool.clone(), factory);
        deps.layout_prefs = Arc::new(FileLayoutPrefsStore::new(data_dir));
        let manager = MultiLiveManager::new(deps);
        let backend = Arc::new(ShellBackend {
            api,
            settings: settings.clone(),
            registry: registry.clone(),
            auth_key_channels,
        });
        manager.lock().configure(
            Some(backend),
            Some(Arc::new(ShellSettings(settings))),
            None,
            None,
            None,
        );
        // 원본 멀티 허브 모드는 그리드(`isGridLayout = true`)다. 라이브 메뉴 단독 보기(`multilive_watch`)만
        // 그리드를 끄고 무대를 세운다 — 꺼진 채로 두면 새로 추가한 패인이 선택될 때 무대로 올라간다.
        manager.lock().set_grid_layout(true);
        Self {
            manager,
            pool,
            registry,
            watching: AtomicBool::new(false),
            rev: AtomicU64::new(0),
            last_state: Mutex::new(None),
        }
    }

    /// 매니저에서 사라진 세션의 등록부 항목을 정리한다.
    pub fn prune_registry(&self) {
        let st = self.manager.read();
        let players: Vec<_> = st.sessions().iter().map(|s| s.player().clone()).collect();
        let chats: Vec<_> = st.sessions().iter().map(|s| s.chat().clone()).collect();
        drop(st);
        self.registry.retain(&players, &chats);
    }

    /// 프런트엔드 상태 — 내용이 바뀔 때만 오르는 `rev` 를 붙인다. 화면은 상태를 이벤트·명령 응답·1초 주기
    /// 세 경로로 순서 없이 받으므로, 늦게 온 낡은 상태(재사용된 슬롯을 '닫힌 패인'으로 보고 멈추게 하던 것)를
    /// `rev` 로 걸러 낸다.
    ///
    /// 상태를 읽는 일과 `rev` 를 매기는 일은 `last_state` 잠금 안에서 함께 한다. 따로 하면 1초 주기 방출이
    /// 메인 전환 *전* 상태를 읽어 둔 사이 `multilive_select` 가 새 상태에 `rev` 를 매기고, 그 뒤 주기 방출이
    /// 낡은 상태를 '바뀐 것'으로 보고 더 큰 `rev` 를 붙여 보냈다 — 화면이 이전 메인으로 되돌아가던 원인.
    pub fn state_dto(&self) -> MultiLiveStateDto {
        let mut last = self.last_state.lock().unwrap_or_else(|e| e.into_inner());
        let mut dto = self.state_dto_raw();
        let changed = last.as_ref() != Some(&dto);
        if changed {
            *last = Some(dto.clone());
            self.rev.fetch_add(1, Ordering::SeqCst);
        }
        dto.rev = self.rev.load(Ordering::SeqCst);
        dto
    }

    /// 라이브 무대 세우기/해제·접기 직후 패인마다의 상태를 디버그 로그(`multilive.stage`)에 남긴다(리뷰 C-1).
    ///
    /// 무대 왕복 뒤 검은 패인이 '매니저가 로딩을 멈춰 둔 것'인지 '페이지(WebKit)가 멈춘 것'인지 가르려면
    /// 그 순간의 디코드 판정·로딩 정지·재생 의도를 패인 슬롯과 함께 봐야 한다. `ui.player`(화면 수집기)의
    /// `where: paneN` 과 같은 슬롯 번호, `player.net` 과 같은 채널 앞 6자를 쓴다. 꺼져 있으면 비용이 없다.
    pub fn log_stage_transition(&self, action: &str) {
        if !crate::perf_log::enabled() {
            return;
        }
        let st = self.manager.read();
        let panes: Vec<serde_json::Value> = st
            .sessions()
            .iter()
            .map(|s| {
                let engine = self
                    .registry
                    .player_of(s.player())
                    .and_then(|p| p.pane_engine());
                serde_json::json!({
                    "where": engine.as_ref().map(|e| format!("pane{}", e.slot)),
                    "tag": s.channel_id().chars().take(6).collect::<String>(),
                    "decode": st.decode_active(s.id()),
                    "loading_suspended": engine.as_ref().map(|e| e.is_loading_suspended()),
                    "playback_desired": engine.as_ref().map(|e| e.is_playback_desired()),
                    "muted": s.player().is_muted(),
                    "stage": st.stage_session_id() == Some(s.id()),
                    "selected": st.selected_session_id() == Some(s.id()),
                })
            })
            .collect();
        crate::perf_log::emit(
            "multilive.stage",
            serde_json::json!({
                "action": action,
                "grid": st.is_grid_layout(),
                "watching": self.watching.load(Ordering::SeqCst),
                "panes": panes,
            }),
        );
    }

    fn state_dto_raw(&self) -> MultiLiveStateDto {
        let st = self.manager.read();
        let selected = st.selected_session_id();
        let sessions = st
            .sessions()
            .iter()
            .map(|s| {
                let snap = s.snapshot();
                let pane = self.registry.player_of(s.player());
                let engine = pane.as_ref().and_then(|p| p.pane_engine());
                let audible = st.is_audible(s);
                MultiLivePaneDto {
                    id: snap.id.to_string(),
                    slot: engine.as_ref().map(|e| e.slot),
                    channel_id: snap.channel_id,
                    channel_name: snap.channel_name,
                    live_title: snap.live_title,
                    profile_image_url: snap.profile_image_url,
                    viewer_count_text: snap.formatted_viewer_count,
                    accumulate_count: snap.accumulate_count,
                    accumulate_count_text: snap.formatted_accumulate_count,
                    open_date: snap.open_date.map(|d| d.to_rfc3339()),
                    is_muted: snap.is_muted,
                    is_audible: audible,
                    is_audio_only: snap.is_audio_only,
                    state: PaneStateDto::from(&snap.load_state),
                    chat_channel_id: self
                        .registry
                        .chat_of(s.chat())
                        .and_then(|c| c.chat_channel_id()),
                    max_height_cap: engine.as_ref().map(|e| e.max_height_cap()).unwrap_or(0),
                    decode_suspended: st.decode_active(s.id()) == Some(false)
                        || engine.as_ref().is_some_and(|e| e.is_loading_suspended()),
                }
            })
            .collect();
        MultiLiveStateDto {
            sessions,
            selected_id: selected.map(|u| u.to_string()),
            focus_main_id: st
                .focus_main_session_id()
                .or(selected)
                .map(|u| u.to_string()),
            can_add: st.can_add_session(),
            max_sessions: st.effective_max_sessions(),
            can_add_live: st.can_add_session_for(true),
            max_live_sessions: st.max_live_view_sessions(),
            max_multi_sessions: st.max_multi_live_sessions(),
            layout: layout_name(st.grid_layout_mode()).to_string(),
            focus_direction: st.focus_direction().raw_value().to_string(),
            focus_ratio: st.layout_ratios().focus_ratio,
            horizontal_ratio: st.layout_ratios().horizontal_ratio,
            vertical_ratio: st.layout_ratios().vertical_ratio,
            multi_audio: st.is_multi_audio_mode(),
            watch_id: st.stage_session_id().map(|u| u.to_string()),
            rev: 0,
        }
    }
}

/// 레이아웃 이름 ↔ 모드.
pub fn layout_name(mode: MultiLiveGridLayoutMode) -> &'static str {
    match mode {
        MultiLiveGridLayoutMode::Focus => "focus",
        MultiLiveGridLayoutMode::Custom => "custom",
        MultiLiveGridLayoutMode::Preset => "grid",
    }
}

pub fn layout_from_name(name: &str) -> Option<MultiLiveGridLayoutMode> {
    match name {
        "focus" => Some(MultiLiveGridLayoutMode::Focus),
        "grid" => Some(MultiLiveGridLayoutMode::Preset),
        "custom" => Some(MultiLiveGridLayoutMode::Custom),
        _ => None,
    }
}

/// 추가 결과 → 사용자 문구. 성공이면 `None`.
pub fn add_result_message(result: &MultiLiveAddSessionResult) -> Option<String> {
    use MultiLiveAddSessionResult as R;
    match result {
        R::Added { .. } | R::ActivatedExisting | R::Cancelled => None,
        R::MaxSessionsReached => Some("더 이상 추가할 수 없습니다 (최대 세션 수)".into()),
        R::AlreadyInProgress => Some("이미 추가하는 중입니다".into()),
        R::LiveDetailTimeout => Some("방송 정보를 불러오는 데 시간이 너무 오래 걸립니다".into()),
        R::NotLive => Some("현재 방송 중이 아닙니다".into()),
        R::ApiUnavailable => Some("API 클라이언트를 사용할 수 없습니다".into()),
        R::EngineUnavailable => Some("플레이어를 더 만들 수 없습니다".into()),
        R::ApiError { message } => Some(message.clone()),
    }
}

/// 패인 상태.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum PaneStateDto {
    Idle,
    Loading,
    Playing,
    Offline,
    Error { message: String },
}

impl From<&MultiLiveLoadState> for PaneStateDto {
    fn from(s: &MultiLiveLoadState) -> Self {
        match s {
            MultiLiveLoadState::Idle => Self::Idle,
            MultiLiveLoadState::Loading => Self::Loading,
            MultiLiveLoadState::Playing { .. } => Self::Playing,
            MultiLiveLoadState::Offline => Self::Offline,
            MultiLiveLoadState::Error(message) => Self::Error {
                message: message.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiLivePaneDto {
    pub id: String,
    /// 플레이어 iframe 슬롯 — 엔진이 없으면 `None`.
    pub slot: Option<u32>,
    pub channel_id: String,
    pub channel_name: String,
    pub live_title: String,
    pub profile_image_url: Option<String>,
    pub viewer_count_text: String,
    /// 누적 시청자 수 — 아직 모르면 0.
    pub accumulate_count: i64,
    pub accumulate_count_text: String,
    /// 방송 시작 시각(RFC 3339) — 경과 시간 표시용.
    pub open_date: Option<String>,
    pub is_muted: bool,
    pub is_audible: bool,
    pub is_audio_only: bool,
    pub state: PaneStateDto,
    pub chat_channel_id: Option<String>,
    /// 대역폭 코디네이터가 건 높이 상한(0 = 없음).
    pub max_height_cap: i64,
    /// 디코드 예산·가림·시청 모드로 로딩을 멈춘 패인 — 화면의 첫 프레임 감시가 다시 연결하지 않게.
    pub decode_suspended: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiLiveStateDto {
    pub sessions: Vec<MultiLivePaneDto>,
    pub selected_id: Option<String>,
    /// 포커스 레이아웃의 메인 패인(없으면 선택 패인).
    pub focus_main_id: Option<String>,
    /// 지금 보기(라이브 단독 보기면 라이브, 아니면 멀티라이브)에 더 넣을 수 있는가와 그 상한.
    pub can_add: bool,
    pub max_sessions: usize,
    /// 라이브 단독 보기에 담는 추가(카드로 열기·머리줄 채널 추가) — 지금 어느 보기든 이 상한을 따른다.
    pub can_add_live: bool,
    pub max_live_sessions: usize,
    /// 멀티라이브 보기 상한.
    pub max_multi_sessions: usize,
    pub layout: String,
    /// 포커스 메인 위치 — `left` / `right` / `top` / `bottom`.
    pub focus_direction: String,
    /// 포커스 메인 패인 비율(0.45~0.85).
    pub focus_ratio: f64,
    /// 사용자 배치 그리드의 열·행 비율(0.2~0.8).
    pub horizontal_ratio: f64,
    pub vertical_ratio: f64,
    /// 여러 패인 소리 모드.
    pub multi_audio: bool,
    /// 라이브 메뉴에서 단독으로 보는(무대에 올린) 세션 — 원본 `FollowingHubMode.watch` 의 stage.
    pub watch_id: Option<String>,
    /// 상태 순번 — 내용이 바뀔 때만 오른다(같은 내용 비교에서는 0 으로 둔 값끼리 비교).
    pub rev: u64,
}

/// `multilive://command` 페이로드.
#[derive(Debug, Clone, Serialize)]
pub struct SlotCommandPayload {
    pub slot: u32,
    pub script: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    type SentLog = Arc<Mutex<Vec<(u32, String)>>>;

    fn pool() -> (Arc<PanePool>, SentLog) {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let log = sent.clone();
        let pool = Arc::new(PanePool::new(Arc::new(move |slot, script| {
            log.lock().unwrap().push((slot, script));
        })));
        (pool, sent)
    }

    #[tokio::test]
    async fn pool_reuses_released_slots_and_respects_max() {
        let (pool, _) = pool();
        pool.set_max_pool_size(2).await;
        let a = pool.acquire().await.unwrap();
        let b = pool.acquire().await.unwrap();
        assert!(pool.acquire().await.is_none(), "최대 슬롯을 넘지 않는다");
        let slot_a = pool.find_active(&a).unwrap().slot();
        assert_ne!(slot_a, pool.find_active(&b).unwrap().slot());
        pool.release(a).await;
        assert_eq!(pool.total_active_count().await, 1);
        let c = pool.acquire().await.unwrap();
        assert_eq!(
            pool.find_active(&c).unwrap().slot(),
            slot_a,
            "놓은 슬롯을 다시 쓴다"
        );
        pool.discard(c).await;
        pool.drain().await;
        assert_eq!(pool.total_active_count().await, 0);
    }

    #[tokio::test]
    async fn engine_commands_carry_slot_and_caps() {
        let (pool, sent) = pool();
        let engine = pool.acquire().await.unwrap();
        let slot = pool.find_active(&engine).unwrap().slot();
        engine.set_max_resolution_height(480);
        engine.set_max_resolution_height(480);
        engine.set_max_bitrate(1500);
        assert_eq!(engine.max_adaptive_height(), 480);
        assert_eq!(engine.max_adaptive_bitrate(), 1500);
        let log = sent.lock().unwrap().clone();
        assert_eq!(log.len(), 2, "같은 상한은 다시 보내지 않는다");
        assert!(log.iter().all(|(s, _)| *s == slot));
        engine.suspend_loading();
        assert!(engine.is_loading_suspended());
        engine.resume_loading();
        assert!(!engine.is_loading_suspended());
        pool.release(engine.clone()).await;
        assert_eq!(engine.max_adaptive_height(), 0, "풀로 돌아가면 상한을 푼다");
    }

    #[test]
    fn focus_layout_update_validates_and_clamps() {
        assert_eq!(
            focus_layout_update(Some("top"), Some(0.99)),
            (
                Some(MultiLiveFocusDirection::Top),
                Some(MultiLiveLayoutRatios::FOCUS_MAX)
            )
        );
        assert_eq!(
            focus_layout_update(Some("sideways"), Some(0.1)),
            (None, Some(MultiLiveLayoutRatios::FOCUS_MIN))
        );
        assert_eq!(focus_layout_update(None, Some(f64::NAN)), (None, None));
    }

    #[test]
    fn file_layout_store_round_trips() {
        let dir = std::env::temp_dir().join(format!("cview-ml-prefs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = FileLayoutPrefsStore::new(&dir);
        assert!(store.get("multiLiveLayoutPrefs").is_none());
        store.set("multiLiveLayoutPrefs", b"{}".to_vec());
        assert_eq!(
            store.get("multiLiveLayoutPrefs").as_deref(),
            Some(&b"{}"[..])
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn layout_names_round_trip() {
        for mode in [
            MultiLiveGridLayoutMode::Preset,
            MultiLiveGridLayoutMode::Focus,
        ] {
            assert_eq!(layout_from_name(layout_name(mode)), Some(mode));
        }
        assert_eq!(layout_from_name("x"), None);
    }

    #[test]
    fn add_results_map_to_messages() {
        assert!(add_result_message(&MultiLiveAddSessionResult::ActivatedExisting).is_none());
        assert!(add_result_message(&MultiLiveAddSessionResult::MaxSessionsReached).is_some());
        assert_eq!(
            add_result_message(&MultiLiveAddSessionResult::ApiError {
                message: "x".into()
            }),
            Some("x".into())
        );
    }

    #[test]
    fn pane_state_serializes_with_kind() {
        let v = serde_json::to_value(PaneStateDto::from(&MultiLiveLoadState::Error("e".into())))
            .unwrap();
        assert_eq!(v["kind"], "error");
        assert_eq!(v["message"], "e");
        assert_eq!(
            launch_plan_error(&PlaybackPlanError::NoHlsUrl),
            LaunchPlanError::NoHlsUrl
        );
    }

    /// 메뉴를 떠나 채팅이 정지됐다 돌아와도 채널 ID 가 남아 프런트엔드가 같은 채팅을 다시 붙인다.
    #[tokio::test]
    async fn pane_chat_keeps_channel_across_menu_suspend() {
        let chat = PaneChat::default();
        assert_eq!(chat.connection_state(), ChatConnectionKind::Disconnected);
        chat.reconnect().await;
        assert_eq!(
            chat.connection_state(),
            ChatConnectionKind::Disconnected,
            "연결한 적이 없으면 되살릴 채널도 없다"
        );
        chat.set_chat_channel_id("chat-a");
        assert_eq!(chat.connection_state(), ChatConnectionKind::Other);

        chat.disconnect().await; // SuspendChat
        assert_eq!(chat.connection_state(), ChatConnectionKind::Disconnected);
        assert_eq!(
            chat.chat_channel_id().as_deref(),
            Some("chat-a"),
            "정지 중에도 상태 DTO 의 chatChannelId 는 비지 않는다"
        );
        assert!(chat.has_last_connection_config());

        chat.reconnect().await; // Restore
        assert_eq!(chat.connection_state(), ChatConnectionKind::Other);
        assert_eq!(chat.chat_channel_id().as_deref(), Some("chat-a"));

        chat.disconnect().await; // retry: 끊고 새 채널로 다시 연결
        chat.set_chat_channel_id("chat-b");
        assert_eq!(chat.connection_state(), ChatConnectionKind::Other);
        assert_eq!(chat.chat_channel_id().as_deref(), Some("chat-b"));
    }
}
