//! 멀티라이브 매니저 — 세션 추가·제거·선택·스테이지·디코드 예산·메뉴 수명주기.
//!
//! 원본: `Sources/CViewApp/ViewModels/MultiLiveManager.swift`. 확장은 하위 모듈:
//! `+AudioGrid`(audio_grid) · `+Bandwidth`(bandwidth) · `+CoordinationSnapshot`(coordination) ·
//! `+FocusQuality`(focus_quality) · `+PaneSizing`(pane_sizing) · `+Persistence`(persistence) ·
//! `+RateBudget`(rate_budget).
//!
//! 본체도 주제별로 나눴다: 세션 추가·제거·해제(lifecycle) · 시작/재시도 예약(scheduling) ·
//! 선택·스테이지(stage) · 메뉴 이탈/복귀 표면(menu_surfaces) · 디코드 예산(decode_budget) ·
//! 줍 계측(zap) · 팔로잉 작업 공간 창구(workspace_host). 이 파일에는 상태 정의·접근자·setter·
//! 핸들 생성만 남는다.
//!
//! Swift 프로퍼티 `didSet` 의 `reconcileFocusQuality()` 호출은 [`ManagerState`] 의 setter 가
//! 대신한다. 동기 정책은 상태 잠금 안에서, `await` 가 있는 흐름은 잠금을 쥐지 않고 핸들에서 돈다.
//! SwiftUI 트랜잭션(애니메이션 끄기)·OSSignposter·`AppLogger` 세부 문구는 버렸다.

mod audio_grid;
mod bandwidth;
mod coordination;
mod decode_budget;
mod focus_quality;
mod lifecycle;
mod menu_surfaces;
mod pane_sizing;
mod persistence;
mod rate_budget;
mod scheduling;
mod stage;
mod workspace_host;
mod zap;

pub use coordination::{CoordinationSnapshot, MetricSnapshot};

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, Mutex, Weak};

use serde::Serialize;
use tokio::time::Instant;
use uuid::Uuid;

use cview_player::bandwidth_coordinator::MultiLiveBandwidthCoordinator;

use super::add_session_result::MultiLiveAddSessionResult;
use super::deps::{
    BandwidthPacing, ChannelIdentitySink, MultiLiveChatSessions, MultiLiveEnginePool, MultiLivePip,
    MultiLiveSessionBackend, MultiLiveSessionFactory, MultiLiveSettingsSource, NoPip, PlainPacing,
};
use super::layout::{
    LayoutPrefsStore, MemoryLayoutPrefsStore, MultiLiveFocusDirection, MultiLiveGridLayoutMode,
    MultiLiveLayoutRatios,
};
use super::policy::{
    has_session_capacity, DecodeSuspensionReason, MruLedger, PaneSuspensionCause, MAX_SESSIONS,
};
use super::session::{MultiLiveSession, MultiLiveSessionSnapshot};
use crate::viewmodels::chat::deps::ToastPresenter;
use crate::viewmodels::support::{Observed, ObservedGuard, TaskSlot};

/// 런타임이 있으면 분리 실행 (Swift 의 `Task { ... }` 발사 후 망각).
pub(crate) fn spawn_detached<F>(fut: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    if let Ok(h) = tokio::runtime::Handle::try_current() {
        h.spawn(fut);
    }
}

/// 고정 의존성.
pub struct MultiLiveDeps {
    pub engine_pool: Arc<dyn MultiLiveEnginePool>,
    pub session_factory: Arc<dyn MultiLiveSessionFactory>,
    pub layout_prefs: Arc<dyn LayoutPrefsStore>,
    pub pip: Arc<dyn MultiLivePip>,
    pub toast: Option<Arc<dyn ToastPresenter>>,
    pub identity: Option<Arc<dyn ChannelIdentitySink>>,
    pub pacing: Arc<dyn BandwidthPacing>,
    /// 스테이지 크기 미보고 시 쓰는 창/화면 크기 (`NSApp.mainWindow` → `NSScreen.main` → 1920×1080).
    pub fallback_screen_size: (f64, f64),
}

impl MultiLiveDeps {
    pub fn new(
        engine_pool: Arc<dyn MultiLiveEnginePool>,
        session_factory: Arc<dyn MultiLiveSessionFactory>,
    ) -> Self {
        Self {
            engine_pool,
            session_factory,
            layout_prefs: Arc::new(MemoryLayoutPrefsStore::default()),
            pip: Arc::new(NoPip),
            toast: None,
            identity: None,
            pacing: Arc::new(PlainPacing),
            fallback_screen_size: (1920.0, 1080.0),
        }
    }
}

/// 의존성 + 대역폭 코디네이터.
pub struct ManagerCore {
    pub deps: MultiLiveDeps,
    pub bandwidth: Mutex<MultiLiveBandwidthCoordinator>,
}

impl ManagerCore {
    fn bw(&self) -> std::sync::MutexGuard<'_, MultiLiveBandwidthCoordinator> {
        self.bandwidth.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[derive(Debug, Clone, Copy)]
struct ZapRequest {
    at: Instant,
    warm: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuSessionLifecycleIntent {
    SuspendChat,
    Restore { resume_playback: bool },
}

/// `addSession` 선택 인자.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddSessionOptions {
    pub start_immediately: bool,
    pub restoring_from_persistence: bool,
    pub watch_transition: bool,
    pub defer_auto_selection: bool,
}

impl Default for AddSessionOptions {
    fn default() -> Self {
        Self {
            start_immediately: true,
            restoring_from_persistence: false,
            watch_transition: false,
            defer_auto_selection: false,
        }
    }
}

/// 매니저 상태.
pub struct ManagerState {
    this: Weak<ManagerShared>,
    core: Arc<ManagerCore>,
    backend: Option<Arc<dyn MultiLiveSessionBackend>>,
    settings: Option<Arc<dyn MultiLiveSettingsSource>>,
    chat_sessions: Option<Arc<dyn MultiLiveChatSessions>>,
    user_uid: Option<String>,
    user_nickname: Option<String>,

    sessions: Vec<Arc<MultiLiveSession>>,
    selected_session_id: Option<Uuid>,
    double_tap_zoom_session_id: Option<Uuid>,
    is_updating_session_selection: bool,
    pip_session_id: Option<Uuid>,
    is_grid_layout: bool,
    grid_layout_mode: MultiLiveGridLayoutMode,
    layout_ratios: MultiLiveLayoutRatios,
    focus_direction: MultiLiveFocusDirection,
    focus_main_session_id: Option<Uuid>,
    pub dragging_session_index: Option<usize>,
    pub dragging_session_id: Option<Uuid>,
    audio_session_id: Option<Uuid>,
    is_multi_audio_mode: bool,
    audio_enabled_session_ids: HashSet<Uuid>,
    adding_channel_ids: HashSet<String>,
    removing_session_ids: HashSet<Uuid>,
    is_stopping_all: bool,
    session_startup_tasks: HashMap<Uuid, TaskSlot>,
    session_startup_generations: HashMap<Uuid, u64>,
    session_menu_lifecycle_tasks: HashMap<Uuid, TaskSlot>,
    session_menu_lifecycle_generations: HashMap<Uuid, u64>,
    /// 종료 중 — 레이아웃 저장을 막는다 (테스트도 켠다).
    pub is_terminating: bool,
    has_applied_layout_prefs: bool,
    menu_paused_session_ids: HashSet<Uuid>,
    menu_parked_session_ids: HashSet<Uuid>,
    menu_leave_full_suspend_task: Option<TaskSlot>,
    has_deferred_peer_restore: bool,
    occlusion_suspended_session_ids: HashSet<Uuid>,
    bw_coordinator_task: Option<TaskSlot>,
    bandwidth_loop_generation: u64,
    bandwidth_selection_revision: u64,
    pane_geometry_revision: u64,
    last_applied_cap_height: HashMap<Uuid, i64>,
    last_applied_hls_kbps: HashMap<Uuid, f64>,
    mru: MruLedger,
    memory_pressure_k_override: Option<usize>,
    memory_pressure_recovery_task: Option<TaskSlot>,
    memory_pressure_generation: u64,
    decode_active: HashMap<Uuid, bool>,
    pane_suspension: HashMap<Uuid, PaneSuspensionCause>,
    decode_suspension_reasons: HashSet<DecodeSuspensionReason>,
    thermal_secondary_quality_cap_active: bool,
    stage_session_id: Option<Uuid>,
    previous_stage_session_id: Option<Uuid>,
    stage_handoff_hide_task: Option<TaskSlot>,
    watch_prewarm_task: Option<TaskSlot>,
    stage_handoff_started_at: Option<Instant>,
    stage_handoff_baseline_session_id: Option<Uuid>,
    stage_handoff_baseline_tick: u64,
    zap_requests: HashMap<String, ZapRequest>,
    tm_revert_tasks: HashMap<Uuid, TaskSlot>,
    teardown_generation: u64,
    last_reported_stage_size: Option<(f64, f64)>,
    pane_resize_debounce_task: Option<TaskSlot>,
}

// MARK: - 읽기 전용 접근자

impl ManagerState {
    pub fn sessions(&self) -> &[Arc<MultiLiveSession>] {
        &self.sessions
    }

    pub fn session(&self, id: Uuid) -> Option<&Arc<MultiLiveSession>> {
        self.sessions.iter().find(|s| s.id() == id)
    }

    pub fn session_for_channel(&self, channel_id: &str) -> Option<&Arc<MultiLiveSession>> {
        self.sessions.iter().find(|s| s.channel_id() == channel_id)
    }

    fn contains(&self, id: Uuid) -> bool {
        self.sessions.iter().any(|s| s.id() == id)
    }

    pub fn selected_session_id(&self) -> Option<Uuid> {
        self.selected_session_id
    }

    pub fn selected_session(&self) -> Option<&Arc<MultiLiveSession>> {
        self.selected_session_id.and_then(|id| self.session(id))
    }

    pub fn audio_session(&self) -> Option<&Arc<MultiLiveSession>> {
        match self.audio_session_id {
            None => self.selected_session(),
            Some(id) => self.session(id),
        }
    }

    pub fn audio_session_id(&self) -> Option<Uuid> {
        self.audio_session_id
    }

    pub fn is_multi_audio_mode(&self) -> bool {
        self.is_multi_audio_mode
    }

    pub fn audio_enabled_session_ids(&self) -> &HashSet<Uuid> {
        &self.audio_enabled_session_ids
    }

    pub fn pip_session_id(&self) -> Option<Uuid> {
        self.pip_session_id
    }

    pub fn is_grid_layout(&self) -> bool {
        self.is_grid_layout
    }

    pub fn grid_layout_mode(&self) -> MultiLiveGridLayoutMode {
        self.grid_layout_mode
    }

    pub fn layout_ratios(&self) -> MultiLiveLayoutRatios {
        self.layout_ratios
    }

    pub fn focus_direction(&self) -> MultiLiveFocusDirection {
        self.focus_direction
    }

    pub fn focus_main_session_id(&self) -> Option<Uuid> {
        self.focus_main_session_id
    }

    pub fn double_tap_zoom_session_id(&self) -> Option<Uuid> {
        self.double_tap_zoom_session_id
    }

    pub fn stage_session_id(&self) -> Option<Uuid> {
        self.stage_session_id
    }

    pub fn previous_stage_session_id(&self) -> Option<Uuid> {
        self.previous_stage_session_id
    }

    pub fn stage_handoff_baseline_session_id(&self) -> Option<Uuid> {
        self.stage_handoff_baseline_session_id
    }

    pub fn stage_handoff_baseline_tick(&self) -> u64 {
        self.stage_handoff_baseline_tick
    }

    pub fn has_deferred_peer_restore(&self) -> bool {
        self.has_deferred_peer_restore
    }

    pub fn pane_suspension(&self) -> &HashMap<Uuid, PaneSuspensionCause> {
        &self.pane_suspension
    }

    pub fn decode_active(&self, id: Uuid) -> Option<bool> {
        self.decode_active.get(&id).copied()
    }

    pub fn last_reported_stage_size(&self) -> Option<(f64, f64)> {
        self.last_reported_stage_size
    }

    pub fn mru_tick(&self, id: Uuid) -> u64 {
        self.mru.tick(id)
    }

    pub fn scheduled_startup_session_ids(&self) -> HashSet<Uuid> {
        self.session_startup_tasks.keys().copied().collect()
    }

    pub fn has_bandwidth_coordination(&self) -> bool {
        self.bw_coordinator_task.is_some()
    }

    pub fn last_applied_cap_height(&self, id: Uuid) -> Option<i64> {
        self.last_applied_cap_height.get(&id).copied()
    }

    pub fn last_applied_hls_kbps(&self, id: Uuid) -> Option<f64> {
        self.last_applied_hls_kbps.get(&id).copied()
    }

    pub fn is_stopping_all(&self) -> bool {
        self.is_stopping_all
    }

    pub fn active_session_count(&self) -> usize {
        self.sessions
            .iter()
            .filter(|s| s.read().load_state.is_playing())
            .count()
    }

    pub fn is_adding_channel(&self, channel_id: &str) -> bool {
        self.adding_channel_ids.contains(channel_id)
    }

    pub fn effective_max_sessions(&self) -> usize {
        let setting = self.settings.as_ref().map_or(MAX_SESSIONS, |s| {
            s.multi_live().max_concurrent_sessions.max(0) as usize
        });
        setting.max(MAX_SESSIONS)
    }

    pub fn has_open_session_slot(&self) -> bool {
        !self.is_stopping_all && self.sessions.len() < self.effective_max_sessions()
    }

    pub fn can_add_session(&self) -> bool {
        !self.is_stopping_all
            && has_session_capacity(
                self.sessions.len() as i64,
                self.adding_channel_ids.len() as i64,
                self.effective_max_sessions() as i64,
            )
    }

    pub fn is_protected_session(&self, id: Uuid) -> bool {
        Some(id) == self.selected_session_id || Some(id) == self.pip_session_id
    }

    /// 멀티 오디오 모드에서 음소거되지 않은 세션.
    pub fn is_audible_session(&self, session: &MultiLiveSession) -> bool {
        self.is_multi_audio_mode && !session.player().is_muted()
    }

    pub fn rewind_main_session_id(&self) -> Option<Uuid> {
        if self.is_grid_layout {
            if let Some(zoomed) = self.double_tap_zoom_session_id {
                if self.contains(zoomed) {
                    return Some(zoomed);
                }
            }
        }
        self.quality_main_session_id()
    }

    pub fn stage_presented_since_switch(&self, session: &MultiLiveSession) -> bool {
        if self.stage_handoff_baseline_session_id != Some(session.id()) {
            return true;
        }
        session.player().presented_frame_tick() != self.stage_handoff_baseline_tick
    }

    /// `watchEntryFailure(channelId:)`.
    pub fn watch_entry_failure(&self, channel_id: &str) -> Option<MultiLiveAddSessionResult> {
        if self.session_for_channel(channel_id).is_some() {
            return None;
        }
        if self.is_stopping_all {
            return Some(MultiLiveAddSessionResult::Cancelled);
        }
        if self.backend.is_none() {
            return Some(MultiLiveAddSessionResult::ApiUnavailable);
        }
        None
    }
}

// MARK: - didSet 대응 setter

impl ManagerState {
    fn sessions_did_change(&mut self) {
        self.reconcile_focus_quality(None);
        if let Some(identity) = &self.core.deps.identity {
            let ids: Vec<String> = self
                .sessions
                .iter()
                .map(|s| s.channel_id().to_string())
                .collect();
            identity.update(0, &ids);
        }
    }

    /// `sessions = ...`.
    pub fn set_sessions(&mut self, sessions: Vec<Arc<MultiLiveSession>>) {
        self.sessions = sessions;
        self.sessions_did_change();
    }

    pub fn push_session(&mut self, session: Arc<MultiLiveSession>) {
        self.sessions.push(session);
        self.sessions_did_change();
    }

    pub fn set_selected_session_id(&mut self, id: Option<Uuid>) {
        if self.selected_session_id != id {
            self.selected_session_id = id;
            self.reconcile_focus_quality(None);
        }
    }

    pub fn set_double_tap_zoom_session_id(&mut self, id: Option<Uuid>) {
        if self.double_tap_zoom_session_id != id {
            self.double_tap_zoom_session_id = id;
            self.reconcile_focus_quality(None);
        }
    }

    pub fn set_pip_session_id(&mut self, id: Option<Uuid>) {
        if self.pip_session_id != id {
            self.pip_session_id = id;
            self.reconcile_focus_quality(None);
        }
    }

    pub fn set_grid_layout(&mut self, grid: bool) {
        let previous = self.is_grid_layout;
        self.is_grid_layout = grid;
        self.reconcile_grid_presentation(previous);
    }

    pub fn set_grid_layout_mode(&mut self, mode: MultiLiveGridLayoutMode) {
        if self.grid_layout_mode != mode {
            self.grid_layout_mode = mode;
            self.reconcile_focus_quality(None);
        }
    }

    pub fn set_layout_ratios(&mut self, ratios: MultiLiveLayoutRatios) {
        self.layout_ratios = ratios;
        self.reconcile_focus_quality(None);
    }

    pub fn set_focus_direction(&mut self, direction: MultiLiveFocusDirection) {
        if self.focus_direction != direction {
            self.focus_direction = direction;
            self.reconcile_focus_quality(None);
        }
    }

    pub fn set_focus_main_session_id(&mut self, id: Option<Uuid>) {
        if self.focus_main_session_id != id {
            self.focus_main_session_id = id;
            self.reconcile_focus_quality(None);
        }
    }

    pub fn set_multi_audio_mode_flag(&mut self, on: bool) {
        self.is_multi_audio_mode = on;
    }

    pub fn configure(
        &mut self,
        backend: Option<Arc<dyn MultiLiveSessionBackend>>,
        settings: Option<Arc<dyn MultiLiveSettingsSource>>,
        user_uid: Option<String>,
        user_nickname: Option<String>,
        chat_sessions: Option<Arc<dyn MultiLiveChatSessions>>,
    ) {
        self.backend = backend;
        self.settings = settings;
        self.user_uid = user_uid;
        self.user_nickname = user_nickname;
        self.chat_sessions = chat_sessions;
    }

    pub fn update_user_info(&mut self, uid: Option<String>, nickname: Option<String>) {
        self.user_uid = uid.clone();
        self.user_nickname = nickname.clone();
        for s in &self.sessions {
            s.chat().set_user(uid.clone(), nickname.clone());
        }
    }
}

/// 공유 셀.
pub struct ManagerShared {
    obs: Observed<ManagerState>,
    core: Arc<ManagerCore>,
}

/// 스냅샷.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiLiveManagerSnapshot {
    pub sessions: Vec<MultiLiveSessionSnapshot>,
    pub selected_session_id: Option<Uuid>,
    pub audio_session_id: Option<Uuid>,
    pub is_multi_audio_mode: bool,
    pub audio_enabled_session_ids: Vec<Uuid>,
    pub is_grid_layout: bool,
    pub grid_layout_mode: MultiLiveGridLayoutMode,
    pub layout_ratios: MultiLiveLayoutRatios,
    pub focus_direction: MultiLiveFocusDirection,
    pub focus_main_session_id: Option<Uuid>,
    pub stage_session_id: Option<Uuid>,
    pub previous_stage_session_id: Option<Uuid>,
    pub pip_session_id: Option<Uuid>,
    pub pane_suspension: HashMap<Uuid, PaneSuspensionCause>,
    pub can_add_session: bool,
    pub has_open_session_slot: bool,
    pub active_session_count: usize,
}

/// 멀티라이브 매니저 핸들.
#[derive(Clone)]
pub struct MultiLiveManager {
    shared: Arc<ManagerShared>,
}

impl MultiLiveManager {
    pub fn new(deps: MultiLiveDeps) -> Self {
        let core = Arc::new(ManagerCore {
            deps,
            bandwidth: Mutex::new(MultiLiveBandwidthCoordinator::default()),
        });
        let shared = Arc::new_cyclic(|this: &Weak<ManagerShared>| ManagerShared {
            obs: Observed::new(ManagerState {
                this: this.clone(),
                core: core.clone(),
                backend: None,
                settings: None,
                chat_sessions: None,
                user_uid: None,
                user_nickname: None,
                sessions: Vec::new(),
                selected_session_id: None,
                double_tap_zoom_session_id: None,
                is_updating_session_selection: false,
                pip_session_id: None,
                is_grid_layout: false,
                grid_layout_mode: MultiLiveGridLayoutMode::Preset,
                layout_ratios: MultiLiveLayoutRatios::default(),
                focus_direction: MultiLiveFocusDirection::Left,
                focus_main_session_id: None,
                dragging_session_index: None,
                dragging_session_id: None,
                audio_session_id: None,
                is_multi_audio_mode: false,
                audio_enabled_session_ids: HashSet::new(),
                adding_channel_ids: HashSet::new(),
                removing_session_ids: HashSet::new(),
                is_stopping_all: false,
                session_startup_tasks: HashMap::new(),
                session_startup_generations: HashMap::new(),
                session_menu_lifecycle_tasks: HashMap::new(),
                session_menu_lifecycle_generations: HashMap::new(),
                is_terminating: false,
                has_applied_layout_prefs: false,
                menu_paused_session_ids: HashSet::new(),
                menu_parked_session_ids: HashSet::new(),
                menu_leave_full_suspend_task: None,
                has_deferred_peer_restore: false,
                occlusion_suspended_session_ids: HashSet::new(),
                bw_coordinator_task: None,
                bandwidth_loop_generation: 0,
                bandwidth_selection_revision: 0,
                pane_geometry_revision: 0,
                last_applied_cap_height: HashMap::new(),
                last_applied_hls_kbps: HashMap::new(),
                mru: MruLedger::default(),
                memory_pressure_k_override: None,
                memory_pressure_recovery_task: None,
                memory_pressure_generation: 0,
                decode_active: HashMap::new(),
                pane_suspension: HashMap::new(),
                decode_suspension_reasons: HashSet::new(),
                thermal_secondary_quality_cap_active: false,
                stage_session_id: None,
                previous_stage_session_id: None,
                stage_handoff_hide_task: None,
                watch_prewarm_task: None,
                stage_handoff_started_at: None,
                stage_handoff_baseline_session_id: None,
                stage_handoff_baseline_tick: 0,
                zap_requests: HashMap::new(),
                tm_revert_tasks: HashMap::new(),
                teardown_generation: 0,
                last_reported_stage_size: None,
                pane_resize_debounce_task: None,
            }),
            core: core.clone(),
        });
        Self { shared }
    }

    pub fn lock(&self) -> ObservedGuard<'_, ManagerState> {
        self.shared.obs.lock()
    }

    pub fn read(&self) -> ObservedGuard<'_, ManagerState> {
        self.shared.obs.read()
    }

    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.shared.obs.subscribe()
    }

    pub fn core(&self) -> &Arc<ManagerCore> {
        &self.shared.core
    }

    pub fn snapshot(&self) -> MultiLiveManagerSnapshot {
        let st = self.read();
        let mut audio: Vec<Uuid> = st.audio_enabled_session_ids.iter().copied().collect();
        audio.sort();
        MultiLiveManagerSnapshot {
            sessions: st.sessions.iter().map(|s| s.snapshot()).collect(),
            selected_session_id: st.selected_session_id,
            audio_session_id: st.audio_session_id,
            is_multi_audio_mode: st.is_multi_audio_mode,
            audio_enabled_session_ids: audio,
            is_grid_layout: st.is_grid_layout,
            grid_layout_mode: st.grid_layout_mode,
            layout_ratios: st.layout_ratios,
            focus_direction: st.focus_direction,
            focus_main_session_id: st.focus_main_session_id,
            stage_session_id: st.stage_session_id,
            previous_stage_session_id: st.previous_stage_session_id,
            pip_session_id: st.pip_session_id,
            pane_suspension: st.pane_suspension.clone(),
            can_add_session: st.can_add_session(),
            has_open_session_slot: st.has_open_session_slot(),
            active_session_count: st.active_session_count(),
        }
    }

    /// 엔진 풀 상한을 설정과 맞춘다.
    pub async fn sync_engine_pool_max(&self) {
        let target = self.read().effective_max_sessions();
        self.shared
            .core
            .deps
            .engine_pool
            .set_max_pool_size(target)
            .await;
    }
}

impl ManagerShared {
    fn this_weak(self: &Arc<Self>) -> Weak<ManagerShared> {
        Arc::downgrade(self)
    }
}

enum LiveDetailFailure {
    Timeout,
    Cancelled,
    Message(String),
}
