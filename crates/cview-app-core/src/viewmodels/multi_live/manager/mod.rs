//! 멀티라이브 매니저 — 세션 추가·제거·선택·스테이지·디코드 예산·메뉴 수명주기.
//!
//! 원본: `Sources/CViewApp/ViewModels/MultiLiveManager.swift`. 확장은 하위 모듈:
//! `+AudioGrid`(audio_grid) · `+Bandwidth`(bandwidth) · `+CoordinationSnapshot`(coordination) ·
//! `+FocusQuality`(focus_quality) · `+PaneSizing`(pane_sizing) · `+Persistence`(persistence) ·
//! `+RateBudget`(rate_budget).
//!
//! Swift 프로퍼티 `didSet` 의 `reconcileFocusQuality()` 호출은 [`ManagerState`] 의 setter 가
//! 대신한다. 동기 정책은 상태 잠금 안에서, `await` 가 있는 흐름은 잠금을 쥐지 않고 핸들에서 돈다.
//! SwiftUI 트랜잭션(애니메이션 끄기)·OSSignposter·`AppLogger` 세부 문구는 버렸다.

mod audio_grid;
mod bandwidth;
mod coordination;
mod focus_quality;
mod pane_sizing;
mod persistence;
mod rate_budget;

pub use coordination::{CoordinationSnapshot, MetricSnapshot};

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, Mutex, Weak};

use serde::Serialize;
use tokio::time::Instant;
use uuid::Uuid;

use cview_core::models::LiveStatus;
use cview_player::bandwidth_coordinator::MultiLiveBandwidthCoordinator;
use cview_player::coordinator::StreamPhase;
use cview_player::policy::SessionTier;

use super::add_session_result::MultiLiveAddSessionResult;
use super::deps::{
    BandwidthPacing, ChannelIdentitySink, ChatConnectionKind, MultiLiveBackendError,
    MultiLiveChatSessions, MultiLiveEnginePool, MultiLivePip, MultiLiveSessionBackend,
    MultiLiveSessionFactory, MultiLiveSettingsSource, NoPip, PlainPacing, SharedHlsEngine,
};
use super::layout::{
    LayoutPrefsStore, MemoryLayoutPrefsStore, MultiLiveFocusDirection, MultiLiveGridLayoutMode,
    MultiLiveLayoutRatios,
};
use super::policy::{
    self, has_session_capacity, tuning, DecodeSuspensionReason, EvictionCandidate, MruLedger,
    PaneSuspensionCause, MAX_SESSIONS, WATCH_KEEP_ALIVE_WARM_COUNT, ZAP_REQUEST_CAP,
    ZAP_REQUEST_MAX_AGE_SECS,
};
use super::session::{MultiLiveSession, MultiLiveSessionSnapshot};
use crate::viewmodels::chat::deps::ToastPresenter;
use crate::viewmodels::following::FollowingWorkspaceHost;
use crate::viewmodels::support::{cancel_slot, slot_is, Observed, ObservedGuard, TaskSlot};

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
        let setting = self
            .settings
            .as_ref()
            .map(|s| s.multi_live().max_concurrent_sessions.max(0) as usize)
            .unwrap_or(MAX_SESSIONS);
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

// MARK: - 줍(zap) 계측

impl ManagerState {
    pub fn note_zap_requested(&mut self, channel_id: &str) {
        let now = Instant::now();
        if let Some(existing) = self.zap_requests.get(channel_id) {
            if now.duration_since(existing.at).as_secs_f64() <= ZAP_REQUEST_MAX_AGE_SECS {
                return;
            }
        }
        let warm = self.session_for_channel(channel_id).is_some();
        self.zap_requests
            .insert(channel_id.to_string(), ZapRequest { at: now, warm });
        if self.zap_requests.len() > ZAP_REQUEST_CAP {
            if let Some(oldest) = self
                .zap_requests
                .iter()
                .min_by_key(|(_, r)| r.at)
                .map(|(k, _)| k.clone())
            {
                self.zap_requests.remove(&oldest);
            }
        }
    }

    pub fn forget_zap_request(&mut self, channel_id: &str) {
        self.zap_requests.remove(channel_id);
    }

    pub fn has_zap_request(&self, channel_id: &str) -> bool {
        self.zap_requests.contains_key(channel_id)
    }

    pub fn complete_watch_zap(&mut self, channel_id: &str, reason: &str) {
        let now = Instant::now();
        let Some(request) = self.zap_requests.remove(channel_id) else {
            return;
        };
        let elapsed = now.duration_since(request.at).as_secs_f64();
        if elapsed > ZAP_REQUEST_MAX_AGE_SECS {
            return;
        }
        tracing::info!(
            "live_zap phase={} reason={reason} total_ms={} sessions={}",
            if request.warm { "warm" } else { "cold" },
            elapsed * 1_000.0,
            self.sessions.len()
        );
    }
}

// MARK: - 스케줄링

impl ManagerState {
    fn clear_per_session_tracking(&mut self) {
        if self.pip_session_id.is_some() {
            self.core.deps.pip.stop_pip();
        }
        self.set_pip_session_id(None);
        self.mru.remove_all();
        self.decode_active.clear();
        self.menu_paused_session_ids.clear();
        self.menu_parked_session_ids.clear();
        self.occlusion_suspended_session_ids.clear();
        cancel_slot(&mut self.menu_leave_full_suspend_task);
        self.last_applied_cap_height.clear();
        self.last_applied_hls_kbps.clear();
        for (_, t) in self.session_startup_tasks.drain() {
            t.cancel();
        }
        self.session_startup_generations.clear();
        for (_, t) in self.session_menu_lifecycle_tasks.drain() {
            t.cancel();
        }
        self.session_menu_lifecycle_generations.clear();
        for (_, t) in self.tm_revert_tasks.drain() {
            t.cancel();
        }
        cancel_slot(&mut self.stage_handoff_hide_task);
        cancel_slot(&mut self.watch_prewarm_task);
        self.zap_requests.clear();
        self.stage_handoff_started_at = None;
        self.stage_handoff_baseline_session_id = None;
        self.stage_handoff_baseline_tick = 0;
        self.stage_session_id = None;
        self.previous_stage_session_id = None;
    }

    fn prewarm_next_watch_engine(&mut self) {
        if self.is_grid_layout || self.is_stopping_all || self.is_terminating {
            return;
        }
        if self.sessions.is_empty() || self.watch_prewarm_task.is_some() {
            return;
        }
        let this = self.this.clone();
        self.watch_prewarm_task = Some(TaskSlot::spawn(move |id| async move {
            tokio::time::sleep(tuning::WATCH_PREWARM_DELAY).await;
            let Some(shared) = this.upgrade() else { return };
            let pool = {
                let mut st = shared.obs.lock();
                if !slot_is(&st.watch_prewarm_task, id) {
                    return;
                }
                if st.is_grid_layout || st.is_stopping_all {
                    st.watch_prewarm_task = None;
                    return;
                }
                st.core.deps.engine_pool.clone()
            };
            pool.warmup(1).await;
            let mut st = shared.obs.lock();
            if slot_is(&st.watch_prewarm_task, id) {
                st.watch_prewarm_task = None;
            }
        }));
    }

    /// 세션 시작 예약 (세대별 단일 작업).
    pub fn schedule_session_start(
        &mut self,
        session: &Arc<MultiLiveSession>,
        pane_count: usize,
        delay: Option<std::time::Duration>,
    ) {
        let sid = session.id();
        if let Some(t) = self.session_startup_tasks.remove(&sid) {
            t.cancel();
        }
        let generation = self
            .session_startup_generations
            .get(&sid)
            .copied()
            .unwrap_or(0)
            .wrapping_add(1);
        self.session_startup_generations.insert(sid, generation);
        let this = self.this.clone();
        let weak_session = Arc::downgrade(session);
        let task = TaskSlot::spawn(move |_| async move {
            if let Some(d) = delay {
                tokio::time::sleep(d).await;
            }
            let (Some(shared), Some(session)) = (this.upgrade(), weak_session.upgrade()) else {
                return;
            };
            if !shared.obs.read().contains(sid) {
                return;
            }
            session.start(pane_count).await;
            let mut st = shared.obs.lock();
            if st.session_startup_generations.get(&sid) != Some(&generation) {
                return;
            }
            st.session_startup_tasks.remove(&sid);
            st.session_startup_generations.remove(&sid);
        });
        self.session_startup_tasks.insert(sid, task);
    }

    fn schedule_session_retry(&mut self, session: &Arc<MultiLiveSession>) {
        let sid = session.id();
        if let Some(t) = self.session_startup_tasks.remove(&sid) {
            t.cancel();
        }
        let generation = self
            .session_startup_generations
            .get(&sid)
            .copied()
            .unwrap_or(0)
            .wrapping_add(1);
        self.session_startup_generations.insert(sid, generation);
        let this = self.this.clone();
        let weak_session = Arc::downgrade(session);
        let task = TaskSlot::spawn(move |_| async move {
            let (Some(shared), Some(session)) = (this.upgrade(), weak_session.upgrade()) else {
                return;
            };
            if !shared.obs.read().contains(sid) {
                return;
            }
            session.retry().await;
            let mut st = shared.obs.lock();
            if st.session_startup_generations.get(&sid) != Some(&generation) {
                return;
            }
            st.session_startup_tasks.remove(&sid);
            st.session_startup_generations.remove(&sid);
        });
        self.session_startup_tasks.insert(sid, task);
    }

    fn schedule_menu_session_lifecycle(
        &mut self,
        session: &Arc<MultiLiveSession>,
        intent: MenuSessionLifecycleIntent,
    ) {
        let sid = session.id();
        if let Some(t) = self.session_menu_lifecycle_tasks.remove(&sid) {
            t.cancel();
        }
        let generation = self
            .session_menu_lifecycle_generations
            .get(&sid)
            .copied()
            .unwrap_or(0)
            .wrapping_add(1);
        self.session_menu_lifecycle_generations
            .insert(sid, generation);
        let teardown_generation = self.teardown_generation;
        let this = self.this.clone();
        let weak_session = Arc::downgrade(session);
        let task = TaskSlot::spawn(move |_| async move {
            let current = |shared: &ManagerShared, session: &Arc<MultiLiveSession>| {
                let st = shared.obs.read();
                st.teardown_generation == teardown_generation
                    && st.session_menu_lifecycle_generations.get(&sid) == Some(&generation)
                    && st.sessions.iter().any(|s| Arc::ptr_eq(s, session))
            };
            let finish = |shared: &ManagerShared| {
                let mut st = shared.obs.lock();
                if st.session_menu_lifecycle_generations.get(&sid) == Some(&generation) {
                    st.session_menu_lifecycle_tasks.remove(&sid);
                    st.session_menu_lifecycle_generations.remove(&sid);
                }
            };
            let (Some(shared), Some(session)) = (this.upgrade(), weak_session.upgrade()) else {
                return;
            };
            if !current(&shared, &session) {
                return;
            }
            match intent {
                MenuSessionLifecycleIntent::SuspendChat => {
                    if !session.chat().has_foreground_presentation_lease() {
                        session.chat().disconnect().await;
                    }
                }
                MenuSessionLifecycleIntent::Restore { resume_playback } => {
                    if resume_playback {
                        session.player().toggle_play_pause().await;
                        if !current(&shared, &session) {
                            finish(&shared);
                            return;
                        }
                        session.player().recover_from_background();
                    }
                    if session.chat().connection_state() == ChatConnectionKind::Disconnected {
                        session.chat().reconnect().await;
                        if !current(&shared, &session) {
                            finish(&shared);
                            return;
                        }
                        session.restart_chat_header_polling();
                    }
                    if current(&shared, &session) {
                        let selected = shared.obs.read().selected_session_id;
                        session.set_background_mode(Some(sid) != selected);
                    }
                }
            }
            finish(&shared);
        });
        self.session_menu_lifecycle_tasks.insert(sid, task);
    }

    /// `restartSessionIfStalled(channelId:)`.
    pub fn restart_session_if_stalled(&mut self, channel_id: &str) -> bool {
        if self.backend.is_none() {
            return false;
        }
        let Some(session) = self.session_for_channel(channel_id).cloned() else {
            return false;
        };
        if !policy::needs_restart(&session.load_state(), &session.player().stream_phase()) {
            return false;
        }
        self.schedule_session_retry(&session);
        true
    }

    fn focus_existing_session(&mut self, channel_id: &str, start_immediately: bool) -> bool {
        let Some(existing) = self.session_for_channel(channel_id).map(|s| s.id()) else {
            return false;
        };
        self.select_session(existing, false);
        self.restore_gpu_render_tiers_from_selection(true, None);
        if start_immediately {
            self.restart_session_if_stalled(channel_id);
        }
        true
    }
}

// MARK: - 선택·스테이지

impl ManagerState {
    pub fn select_session(&mut self, id: Uuid, promote_to_focus_main: bool) {
        if !self.contains(id) {
            return;
        }
        let started = Instant::now();
        self.is_updating_session_selection = true;
        let previous_id = self.selected_session_id;
        if promote_to_focus_main {
            self.set_focus_main_session_id(Some(id));
        }
        self.set_selected_session_id(Some(id));
        self.synchronize_bandwidth_selection();
        self.release_bandwidth_caps_immediately(id);
        if self.stage_session_id.is_some() {
            self.set_stage_session(Some(id));
        }
        self.apply_rate_budget();
        if previous_id != Some(id) {
            let prev = previous_id.and_then(|p| self.session(p).cloned());
            if let Some(prev) = &prev {
                prev.player().set_precision_sync_active(false);
            }
            if let Some(cur) = self.session(id) {
                cur.player().set_precision_sync_active(true);
            }
            if let Some(prev) = &prev {
                prev.set_background_mode(true);
            }
            if let Some(current) = self.session(id).cloned() {
                if !self.is_multi_audio_mode {
                    self.route_audio(id);
                }
                current.set_background_mode(false);
                let engine_playing = current.player().engine().is_some_and(|e| e.is_playing());
                if current.player().stream_phase() == StreamPhase::Buffering || !engine_playing {
                    current.player().promote_for_in_app_selection();
                }
                if let Some(prev) = &prev {
                    if let Some(hls) = prev.player().engine() {
                        hls.set_gpu_render_tier(SessionTier::Visible);
                        self.occlusion_suspended_session_ids.remove(&prev.id());
                        prev.player().sync_network_activity();
                    }
                }
                if let Some(hls) = current.player().engine() {
                    hls.set_gpu_render_tier(SessionTier::Active);
                    self.occlusion_suspended_session_ids.remove(&id);
                    current.player().sync_network_activity();
                }
            }
        }
        self.save_state();
        self.mru.touch(id);
        self.reconcile_decode_budget();
        self.is_updating_session_selection = false;
        self.reconcile_focus_quality(None);
        tracing::info!(
            "live_selection session={id} duration_ms={} sessions={}",
            started.elapsed().as_secs_f64() * 1_000.0,
            self.sessions.len()
        );
    }

    pub fn select_by_channel_id(&mut self, channel_id: &str) {
        let Some(id) = self.session_for_channel(channel_id).map(|s| s.id()) else {
            return;
        };
        let promote = self.grid_layout_mode == MultiLiveGridLayoutMode::Focus;
        self.select_session(id, promote);
    }

    pub fn set_stage_session(&mut self, id: Option<Uuid>) {
        if self.stage_session_id == id {
            return;
        }
        let previous = self.stage_session_id;
        if let Some(p) = previous {
            if Some(p) != id {
                self.previous_stage_session_id = Some(p);
            }
        }
        if id.is_none() {
            self.previous_stage_session_id = None;
        }
        self.stage_session_id = id;
        self.reconcile_focus_quality(None);
        self.reconcile_decode_budget();
        self.reconcile_time_machine();
        if let Some((w, h)) = self.last_reported_stage_size {
            self.update_pane_sizes(w as i64, h as i64);
        }
        if let Some(id) = id {
            self.release_bandwidth_caps_immediately(id);
        }
        cancel_slot(&mut self.stage_handoff_hide_task);
        self.reconcile_hlsjs_pane_offscreen_surfaces();
        if previous.is_some() && id.is_some() {
            self.stage_handoff_started_at = Some(Instant::now());
            if let Some(incoming) = id.and_then(|i| self.session(i).cloned()) {
                self.stage_handoff_baseline_session_id = Some(incoming.id());
                self.stage_handoff_baseline_tick = incoming.player().presented_frame_tick();
                if let Some(hls) = incoming.player().engine() {
                    hls.arm_surface_presented_probe();
                }
            }
            let this = self.this.clone();
            let handoff_to = id;
            self.stage_handoff_hide_task = Some(TaskSlot::spawn(move |tid| async move {
                tokio::time::sleep(tuning::STAGE_HANDOFF_TIMEOUT).await;
                let Some(shared) = this.upgrade() else { return };
                let mut st = shared.obs.lock();
                if !slot_is(&st.stage_handoff_hide_task, tid) || st.stage_session_id != handoff_to {
                    return;
                }
                st.clear_stage_handoff_warm(handoff_to, "timeout");
            }));
        }
        if id.is_some() {
            self.prewarm_next_watch_engine();
        }
    }

    /// 스테이지 핸드오프 완료 — 이전 스테이지 웜 패인을 거둔다.
    pub fn clear_stage_handoff_warm(&mut self, expected_current: Option<Uuid>, reason: &str) {
        if expected_current.is_some() && self.stage_session_id != expected_current {
            return;
        }
        if self.previous_stage_session_id.is_none() {
            return;
        }
        self.previous_stage_session_id = None;
        cancel_slot(&mut self.stage_handoff_hide_task);
        let now = Instant::now();
        if let Some(started) = self.stage_handoff_started_at.take() {
            tracing::info!(
                "live_stage_handoff reason={reason} duration_ms={} sessions={}",
                now.duration_since(started).as_secs_f64() * 1_000.0,
                self.sessions.len()
            );
        }
        if let Some(channel_id) = self
            .stage_session_id
            .and_then(|s| self.session(s))
            .map(|s| s.channel_id().to_string())
        {
            if let Some(request) = self.zap_requests.remove(&channel_id) {
                let elapsed = now.duration_since(request.at).as_secs_f64();
                if elapsed <= ZAP_REQUEST_MAX_AGE_SECS {
                    tracing::info!(
                        "live_zap phase={} reason={reason} total_ms={} sessions={}",
                        if request.warm { "warm" } else { "cold" },
                        elapsed * 1_000.0,
                        self.sessions.len()
                    );
                }
            }
        }
        self.stage_handoff_baseline_session_id = None;
        self.stage_handoff_baseline_tick = 0;
        self.reconcile_hlsjs_pane_offscreen_surfaces();
        self.reconcile_focus_quality(None);
        self.reconcile_decode_budget();
    }

    pub fn watch_warm_candidate_id(&self) -> Option<Uuid> {
        if self.is_grid_layout {
            return None;
        }
        let stage = self.stage_session_id?;
        if let Some(previous) = self.previous_stage_session_id {
            if previous != stage {
                return Some(previous);
            }
        }
        // Swift `max(by:)` 는 동률이면 마지막 원소를 남긴다.
        let mut best: Option<(Uuid, u64)> = None;
        for s in self
            .sessions
            .iter()
            .filter(|s| s.id() != stage && Some(s.id()) != self.pip_session_id)
        {
            let tick = self.mru.tick(s.id());
            if best.is_none_or(|(_, t)| tick >= t) {
                best = Some((s.id(), tick));
            }
        }
        best.map(|(id, _)| id)
    }

    pub fn watch_mode_allows_decode(&self, id: Uuid) -> bool {
        let keep = self.watch_keep_alive_session_ids();
        self.watch_mode_allows_decode_with(id, &keep)
    }

    fn watch_mode_allows_decode_with(&self, id: Uuid, keep_alive: &HashSet<Uuid>) -> bool {
        if self.is_grid_layout || self.stage_session_id.is_none() {
            return true;
        }
        let some = Some(id);
        if some == self.stage_session_id || some == self.previous_stage_session_id {
            return true;
        }
        if some == self.selected_session_id || some == self.pip_session_id {
            return true;
        }
        if some == self.audio_session_id || keep_alive.contains(&id) {
            return true;
        }
        self.session(id).is_some_and(|s| self.is_audible_session(s))
    }

    pub fn watch_keep_alive_session_ids(&self) -> HashSet<Uuid> {
        let mut keep = HashSet::new();
        if self.is_grid_layout {
            return keep;
        }
        let Some(stage) = self.stage_session_id else {
            return keep;
        };
        if let Some(previous) = self.previous_stage_session_id {
            if previous != stage {
                keep.insert(previous);
            }
        }
        let mut ranked: Vec<&Arc<MultiLiveSession>> = self
            .sessions
            .iter()
            .filter(|s| {
                s.id() != stage && Some(s.id()) != self.pip_session_id && !keep.contains(&s.id())
            })
            .collect();
        ranked.sort_by_key(|s| std::cmp::Reverse(self.mru.tick(s.id())));
        for s in ranked {
            if keep.len() >= WATCH_KEEP_ALIVE_WARM_COUNT {
                break;
            }
            keep.insert(s.id());
        }
        keep
    }

    pub fn reconcile_hlsjs_pane_offscreen_surfaces(&self) {
        let stage = self.stage_session_id;
        let handoff_warm = self.previous_stage_session_id;
        for s in &self.sessions {
            let Some(hls) = s.player().engine() else {
                continue;
            };
            let id = Some(s.id());
            let keep_visible = id == stage || id == handoff_warm || id == self.pip_session_id;
            hls.set_video_surface_visible(stage.is_none() || keep_visible);
        }
    }

    pub fn end_stage_session(&mut self, id: Uuid) {
        if self.stage_session_id == Some(id) {
            self.set_stage_session(None);
        }
    }

    fn reconcile_time_machine(&mut self) {
        let stage = self.stage_session_id;
        let pane_count = self.sessions.len();
        let sessions = self.sessions.clone();
        for s in sessions {
            if Some(s.id()) == stage {
                if let Some(t) = self.tm_revert_tasks.remove(&s.id()) {
                    t.cancel();
                }
                spawn_detached(async move { s.set_time_machine_desired(true, pane_count).await });
            } else {
                self.schedule_time_machine_revert(&s);
            }
        }
    }

    fn schedule_time_machine_revert(&mut self, session: &Arc<MultiLiveSession>) {
        if !session.time_machine_desired() {
            return;
        }
        let sid = session.id();
        if self.tm_revert_tasks.contains_key(&sid) {
            return;
        }
        let this = self.this.clone();
        let weak_session = Arc::downgrade(session);
        let task = TaskSlot::spawn(move |tid| async move {
            tokio::time::sleep(tuning::TIME_MACHINE_REVERT_DELAY).await;
            let (Some(shared), Some(session)) = (this.upgrade(), weak_session.upgrade()) else {
                return;
            };
            let pane_count = {
                let mut st = shared.obs.lock();
                if st.tm_revert_tasks.get(&sid).is_none_or(|t| t.id() != tid) {
                    return;
                }
                st.tm_revert_tasks.remove(&sid);
                if st.stage_session_id == Some(sid) {
                    return;
                }
                st.sessions.len()
            };
            session.set_time_machine_desired(false, pane_count).await;
        });
        self.tm_revert_tasks.insert(sid, task);
    }

    pub fn has_active_streams(&self) -> bool {
        self.sessions
            .iter()
            .any(|s| s.player().stream_phase().requires_playback_resources())
    }
}

// MARK: - 메뉴 이탈/복귀·가림

impl ManagerState {
    pub fn suspend_all_gpu_render_tiers(&mut self, keep_hlsjs_alive: bool) {
        let selected = self.selected_session_id;
        for s in self.sessions.clone() {
            if Some(s.id()) == self.pip_session_id {
                continue;
            }
            if Some(s.id()) == selected && s.player().engine().is_some() {
                s.player().set_gpu_render_tier_hidden(keep_hlsjs_alive);
                if !keep_hlsjs_alive {
                    self.occlusion_suspended_session_ids.insert(s.id());
                }
            }
        }
    }

    pub fn restore_video_surfaces_for_menu_enter(&mut self) {
        cancel_slot(&mut self.menu_leave_full_suspend_task);
        self.has_deferred_peer_restore = false;
        let pip = self.pip_session_id;
        let needs_loader_resume = self.sessions.iter().any(|s| {
            Some(s.id()) != pip
                && s.player()
                    .engine()
                    .is_some_and(|h| h.is_loading_suspended())
        });
        if needs_loader_resume {
            for s in &self.sessions {
                if Some(s.id()) == pip || !self.menu_parked_session_ids.contains(&s.id()) {
                    continue;
                }
                if let Some(hls) = s.player().engine() {
                    hls.resume();
                }
            }
            self.menu_parked_session_ids.clear();
            self.restore_gpu_render_tiers_from_selection(true, None);
            return;
        }
        for s in &self.sessions {
            if Some(s.id()) == pip {
                continue;
            }
            let Some(hls) = s.player().engine() else {
                continue;
            };
            if self.menu_parked_session_ids.remove(&s.id()) {
                hls.resume();
            }
        }
        self.reconcile_hlsjs_pane_offscreen_surfaces();
    }

    pub fn defer_peer_surface_restore(&mut self) {
        cancel_slot(&mut self.menu_leave_full_suspend_task);
        self.has_deferred_peer_restore = true;
    }

    pub fn complete_deferred_peer_restore(&mut self) {
        if self.has_deferred_peer_restore {
            self.restore_video_surfaces_for_menu_enter();
        }
    }

    pub fn restore_video_surface_for_watch(&mut self, channel_id: &str) {
        cancel_slot(&mut self.menu_leave_full_suspend_task);
        let Some(session) = self.session_for_channel(channel_id).cloned() else {
            return;
        };
        let was_parked = self.menu_parked_session_ids.remove(&session.id());
        let hls = session.player().engine();
        if was_parked {
            if let Some(h) = &hls {
                h.resume();
            }
        }
        if was_parked
            || self.menu_paused_session_ids.contains(&session.id())
            || hls.is_some_and(|h| h.is_loading_suspended())
        {
            self.restore_gpu_render_tiers_from_selection(true, Some(session.id()));
        } else {
            self.reconcile_hlsjs_pane_offscreen_surfaces();
        }
    }

    pub fn activate_new_watch_audio(&mut self, channel_id: &str) {
        if !self.is_multi_audio_mode {
            return;
        }
        let Some(session) = self.session_for_channel(channel_id).cloned() else {
            return;
        };
        self.audio_enabled_session_ids.insert(session.id());
        session.set_muted(false);
    }

    pub fn park_video_surfaces_for_resident_menu_leave(&mut self) {
        self.has_deferred_peer_restore = false;
        for s in &self.sessions {
            if Some(s.id()) == self.pip_session_id {
                continue;
            }
            let Some(hls) = s.player().engine() else {
                continue;
            };
            hls.set_video_surface_visible(false);
            if !hls.is_loading_suspended() && hls.is_playback_desired() {
                hls.pause();
                self.menu_parked_session_ids.insert(s.id());
            }
        }
        cancel_slot(&mut self.menu_leave_full_suspend_task);
        let this = self.this.clone();
        self.menu_leave_full_suspend_task = Some(TaskSlot::spawn(move |tid| async move {
            tokio::time::sleep(tuning::RESIDENT_MENU_FULL_SUSPEND_DELAY).await;
            let Some(shared) = this.upgrade() else { return };
            let mut st = shared.obs.lock();
            if slot_is(&st.menu_leave_full_suspend_task, tid) {
                st.release_video_surfaces_for_menu_leave();
            }
        }));
    }

    pub fn restore_gpu_render_tiers_from_selection(
        &mut self,
        resume_menu_suspended_hls: bool,
        only_session_id: Option<Uuid>,
    ) {
        for s in self.sessions.clone() {
            if only_session_id.is_some_and(|o| o != s.id()) {
                continue;
            }
            let protected = self.is_protected_session(s.id());
            let tier = if protected {
                SessionTier::Active
            } else {
                SessionTier::Visible
            };
            let hls = s.player().engine();
            if protected && hls.is_some() {
                s.player().restore_surface_after_occlusion();
            }
            let decode_not_off = self.decode_active.get(&s.id()) != Some(&false);
            if self.occlusion_suspended_session_ids.remove(&s.id()) {
                if let Some(h) = &hls {
                    if h.is_loading_suspended() && decode_not_off {
                        h.set_gpu_render_tier(tier);
                    }
                    s.player().sync_network_activity();
                }
            }
            if resume_menu_suspended_hls {
                if let Some(h) = &hls {
                    if h.is_loading_suspended() && decode_not_off {
                        h.set_gpu_render_tier(tier);
                    }
                    s.player().sync_network_activity();
                }
                s.player().resume_watch_heartbeat_for_menu_return();
                s.resume_polling_for_menu_return();
                let resume_playback = self.menu_paused_session_ids.remove(&s.id());
                self.schedule_menu_session_lifecycle(
                    &s,
                    MenuSessionLifecycleIntent::Restore { resume_playback },
                );
            }
        }
        if resume_menu_suspended_hls && self.sessions.len() >= 2 {
            self.start_bandwidth_coordination();
        }
        self.reconcile_hlsjs_pane_offscreen_surfaces();
    }

    pub fn release_video_surfaces_for_menu_leave(&mut self) {
        cancel_slot(&mut self.menu_leave_full_suspend_task);
        self.has_deferred_peer_restore = false;
        for s in self.sessions.clone() {
            if Some(s.id()) == self.pip_session_id {
                continue;
            }
            if let Some(hls) = s.player().engine() {
                if self.decode_active.get(&s.id()) == Some(&false) {
                    hls.set_video_surface_visible(false);
                } else {
                    hls.set_gpu_render_tier(SessionTier::Hidden);
                }
                self.occlusion_suspended_session_ids.remove(&s.id());
                s.player().sync_network_activity();
            }
            s.player().suspend_watch_heartbeat_for_menu_leave();
            s.suspend_polling_for_menu_leave();
            self.schedule_menu_session_lifecycle(&s, MenuSessionLifecycleIntent::SuspendChat);
        }
        self.stop_bandwidth_coordination();
        let pool = self.core.deps.engine_pool.clone();
        spawn_detached(async move { pool.trim_idle(1).await });
    }
}

// MARK: - 디코드 예산

impl ManagerState {
    fn current_suspension_cause(&self) -> PaneSuspensionCause {
        if self
            .decode_suspension_reasons
            .contains(&DecodeSuspensionReason::ThermalPressure)
        {
            return PaneSuspensionCause::Thermal;
        }
        if self
            .decode_suspension_reasons
            .contains(&DecodeSuspensionReason::WindowOcclusion)
        {
            return PaneSuspensionCause::Occlusion;
        }
        if self.memory_pressure_k_override.is_some() {
            return PaneSuspensionCause::MemoryPressure;
        }
        PaneSuspensionCause::DecodeBudget
    }

    fn apply_decode_state(&mut self, session: &Arc<MultiLiveSession>, decoding: bool) {
        let sid = session.id();
        if let Some(hls) = session.player().engine() {
            if decoding {
                let sel = Some(sid) == self.selected_session_id;
                hls.set_buffer_caps(
                    if sel { 10.0 } else { 9.0 },
                    if sel { 18.0 } else { 12.0 },
                    (if sel { 25 } else { 8 }) * 1024 * 1024,
                );
            }
            if self.decode_active.get(&sid) != Some(&decoding) {
                self.decode_active.insert(sid, decoding);
                if decoding {
                    hls.resume_loading();
                } else {
                    hls.suspend_loading();
                }
                session.player().sync_network_activity();
            } else if !decoding && !hls.is_suspend_intent_active() {
                hls.suspend_loading();
                session.player().sync_network_activity();
            }
        }
        let cause = if decoding {
            None
        } else {
            Some(self.current_suspension_cause())
        };
        if self.pane_suspension.get(&sid).copied() != cause {
            match cause {
                Some(c) => self.pane_suspension.insert(sid, c),
                None => self.pane_suspension.remove(&sid),
            };
        }
    }

    pub fn reconcile_decode_budget(&mut self) {
        let sessions = self.sessions.clone();
        if !self.decode_suspension_reasons.is_empty() {
            for s in &sessions {
                let protected = self.is_protected_session(s.id());
                self.apply_decode_state(s, protected);
            }
            return;
        }
        let reduce_enabled = self
            .settings
            .as_ref()
            .map(|s| s.multi_live().background_quality_reduction)
            .unwrap_or(true);
        if !reduce_enabled && self.memory_pressure_k_override.is_none() {
            for s in &sessions {
                self.apply_decode_state(s, true);
            }
            return;
        }
        let k = policy::effective_decode_budget(
            sessions.len(),
            self.effective_max_sessions(),
            self.memory_pressure_k_override,
        );
        let focus_main = if self.grid_layout_mode == MultiLiveGridLayoutMode::Focus {
            self.focus_main_session_id
        } else {
            None
        };
        let audible: HashSet<Uuid> = sessions
            .iter()
            .filter(|s| self.is_audible_session(s))
            .map(|s| s.id())
            .collect();
        let ids: Vec<Uuid> = sessions.iter().map(|s| s.id()).collect();
        let ranked = policy::rank_for_decode_budget(
            &policy::rank_inputs(&ids, &self.mru),
            self.selected_session_id,
            self.pip_session_id,
            focus_main,
            &audible,
        );
        let keep_alive = self.watch_keep_alive_session_ids();
        for (rank, id) in ranked.into_iter().enumerate() {
            let Some(s) = sessions.iter().find(|s| s.id() == id).cloned() else {
                continue;
            };
            let decoding = rank < k && self.watch_mode_allows_decode_with(id, &keep_alive);
            self.apply_decode_state(&s, decoding);
        }
    }

    pub fn suspend_non_selected_decode(&mut self, reason: DecodeSuspensionReason) {
        let inserted = self.decode_suspension_reasons.insert(reason);
        for s in self.sessions.clone() {
            let protected = self.is_protected_session(s.id());
            self.apply_decode_state(&s, protected);
        }
        if inserted {
            tracing::debug!(
                "decode_suspension reason={reason:?} sessions={}",
                self.sessions.len()
            );
        }
    }

    pub fn restore_decode_from_budget(&mut self, reason: DecodeSuspensionReason) {
        if !self.decode_suspension_reasons.remove(&reason) {
            return;
        }
        self.reconcile_decode_budget();
        tracing::debug!(
            "decode_restoration reason={reason:?} remaining_causes={}",
            self.decode_suspension_reasons.len()
        );
    }

    pub fn handle_memory_pressure(&mut self, critical: bool) {
        self.memory_pressure_k_override = Some(if critical {
            1
        } else {
            (policy::DECODE_BUDGET_K - 1).max(2)
        });
        self.reconcile_decode_budget();
        cancel_slot(&mut self.memory_pressure_recovery_task);
        self.memory_pressure_generation = self.memory_pressure_generation.wrapping_add(1);
        let generation = self.memory_pressure_generation;
        let this = self.this.clone();
        let delay = std::time::Duration::from_secs(if critical { 20 } else { 10 });
        self.memory_pressure_recovery_task = Some(TaskSlot::spawn(move |_| async move {
            tokio::time::sleep(delay).await;
            let Some(shared) = this.upgrade() else { return };
            let mut st = shared.obs.lock();
            if st.memory_pressure_generation != generation {
                return;
            }
            st.memory_pressure_k_override = None;
            st.reconcile_decode_budget();
        }));
    }

    pub fn memory_pressure_override(&self) -> Option<usize> {
        self.memory_pressure_k_override
    }

    /// 스테이지 크기 보고 — 8pt 미만 변화는 무시, 200ms 디바운스 후 패인 크기 갱신.
    pub fn report_stage_size(&mut self, size: (f64, f64)) {
        if let Some((w, h)) = self.last_reported_stage_size {
            if (w - size.0).abs() < 8.0 && (h - size.1).abs() < 8.0 {
                return;
            }
        }
        self.last_reported_stage_size = Some(size);
        cancel_slot(&mut self.pane_resize_debounce_task);
        let this = self.this.clone();
        self.pane_resize_debounce_task = Some(TaskSlot::spawn(move |tid| async move {
            tokio::time::sleep(tuning::PANE_RESIZE_DEBOUNCE).await;
            let Some(shared) = this.upgrade() else { return };
            let mut st = shared.obs.lock();
            if slot_is(&st.pane_resize_debounce_task, tid) {
                st.update_pane_sizes(size.0 as i64, size.1 as i64);
            }
        }));
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

    fn generation_ok(&self, generation: u64) -> bool {
        let st = self.read();
        st.teardown_generation == generation && !st.is_stopping_all
    }

    async fn return_uncommitted_engine(&self, engine: SharedHlsEngine, add_generation: u64) {
        let pool = self.shared.core.deps.engine_pool.clone();
        if self.generation_ok(add_generation) {
            pool.release(engine).await;
        } else {
            pool.discard(engine).await;
        }
    }

    async fn fetch_live_detail(
        backend: &Arc<dyn MultiLiveSessionBackend>,
        channel_id: &str,
    ) -> Result<cview_core::models::LiveInfo, LiveDetailFailure> {
        match tokio::time::timeout(tuning::LIVE_DETAIL_TIMEOUT, backend.live_detail(channel_id))
            .await
        {
            Err(_) => Err(LiveDetailFailure::Timeout),
            Ok(Err(MultiLiveBackendError::Cancelled)) => Err(LiveDetailFailure::Cancelled),
            Ok(Err(MultiLiveBackendError::Message(m))) => Err(LiveDetailFailure::Message(m)),
            Ok(Ok(info)) => Ok(info),
        }
    }

    /// 세션 추가. `watch_request_is_current` 가 `false` 를 돌려주면 대체된 요청으로 보고 취소한다.
    pub async fn add_session(
        &self,
        channel_id: &str,
        options: AddSessionOptions,
        watch_request_is_current: Option<&(dyn Fn() -> bool + Send + Sync)>,
    ) -> MultiLiveAddSessionResult {
        use MultiLiveAddSessionResult as R;
        let (gen_at_start, settings) = {
            let st = self.read();
            if st.is_stopping_all {
                tracing::info!("MultiLive: 전체 해제 진행 중 addSession 생략");
                return R::Cancelled;
            }
            (st.teardown_generation, st.settings.clone())
        };
        if let Some(store) = settings {
            let mut waited = 0;
            while !store.is_loaded() && waited < tuning::SETTINGS_LOAD_MAX_RETRIES {
                tokio::time::sleep(tuning::SETTINGS_LOAD_POLL).await;
                waited += 1;
            }
        }
        if !self.generation_ok(gen_at_start) {
            return R::Cancelled;
        }
        self.lock().apply_initial_layout_prefs_if_needed();
        self.sync_engine_pool_max().await;
        if !self.generation_ok(gen_at_start) {
            return R::Cancelled;
        }
        {
            let mut st = self.lock();
            if options.watch_transition && st.session_for_channel(channel_id).is_some() {
                return R::ActivatedExisting;
            }
            if !options.watch_transition
                && st.focus_existing_session(channel_id, options.start_immediately)
            {
                tracing::info!("MultiLive: 이미 존재하는 채널 — 재선택/재시작 처리");
                return R::ActivatedExisting;
            }
            if st.adding_channel_ids.contains(channel_id) {
                tracing::warn!("MultiLive: 이미 추가 진행 중");
                return R::AlreadyInProgress;
            }
            st.adding_channel_ids.insert(channel_id.to_string());
        }
        let result = self
            .add_session_inner(channel_id, options, watch_request_is_current, gen_at_start)
            .await;
        self.lock().adding_channel_ids.remove(channel_id);
        result
    }

    async fn add_session_inner(
        &self,
        channel_id: &str,
        options: AddSessionOptions,
        watch_request_is_current: Option<&(dyn Fn() -> bool + Send + Sync)>,
        gen_at_start: u64,
    ) -> MultiLiveAddSessionResult {
        use MultiLiveAddSessionResult as R;
        let is_current = || watch_request_is_current.is_none_or(|f| f());
        let Some(backend) = self.read().backend.clone() else {
            tracing::error!("MultiLive: API 클라이언트 없음");
            return R::ApiUnavailable;
        };
        let pool = self.shared.core.deps.engine_pool.clone();
        let mut validated_watch_info = None;
        if options.watch_transition {
            match Self::fetch_live_detail(&backend, channel_id).await {
                Ok(info) => {
                    if !self.generation_ok(gen_at_start) || !is_current() {
                        return R::Cancelled;
                    }
                    if info.status != LiveStatus::Open {
                        return R::NotLive;
                    }
                    validated_watch_info = Some(info);
                }
                Err(LiveDetailFailure::Timeout) => return R::LiveDetailTimeout,
                Err(LiveDetailFailure::Cancelled) => return R::Cancelled,
                Err(LiveDetailFailure::Message(m)) => return R::ApiError { message: m },
            }
        }
        let has_room_excluding_self = |st: &ManagerState| {
            !st.is_stopping_all
                && has_session_capacity(
                    st.sessions.len() as i64,
                    st.adding_channel_ids.len() as i64 - 1,
                    st.effective_max_sessions() as i64,
                )
        };
        if !is_current() {
            return R::Cancelled;
        }
        let (has_room, can_evict) = {
            let st = self.read();
            (
                has_room_excluding_self(&st),
                !st.is_grid_layout && !options.restoring_from_persistence,
            )
        };
        let allows_provisional_overflow = options.watch_transition && !has_room;
        if !has_room && !options.watch_transition {
            let evicted = if can_evict {
                self.evict_least_recently_watched_session(false, None).await
            } else {
                None
            };
            if !self.generation_ok(gen_at_start) || !is_current() {
                return R::Cancelled;
            }
            if evicted.is_none() {
                tracing::warn!("MultiLive: 최대 세션 수 도달");
                return R::MaxSessionsReached;
            }
        }
        if allows_provisional_overflow {
            let max = self.read().effective_max_sessions();
            pool.set_max_pool_size(max + 1).await;
        }
        let acquired = pool.acquire().await;
        if allows_provisional_overflow {
            self.sync_engine_pool_max().await;
        }
        let Some(engine) = acquired else {
            let active = pool.total_active_count().await;
            tracing::error!("MultiLive: 엔진 풀 할당 실패 — active={active}");
            return R::EngineUnavailable;
        };
        if !self.generation_ok(gen_at_start) {
            pool.discard(engine).await;
            return R::Cancelled;
        }
        if !is_current() {
            pool.release(engine).await;
            return R::Cancelled;
        }
        let live_info = match validated_watch_info {
            Some(info) => info,
            None => match Self::fetch_live_detail(&backend, channel_id).await {
                Ok(info) => info,
                Err(LiveDetailFailure::Timeout) => {
                    self.return_uncommitted_engine(engine, gen_at_start).await;
                    tracing::warn!("MultiLive: liveDetail 타임아웃 — 추가 취소");
                    return R::LiveDetailTimeout;
                }
                Err(LiveDetailFailure::Cancelled) => {
                    self.return_uncommitted_engine(engine, gen_at_start).await;
                    return R::Cancelled;
                }
                Err(LiveDetailFailure::Message(m)) => {
                    self.return_uncommitted_engine(engine, gen_at_start).await;
                    tracing::error!("MultiLive: 세션 추가 실패 — {m}");
                    return R::ApiError { message: m };
                }
            },
        };
        let torn_down = self.read().teardown_generation != gen_at_start;
        if torn_down || !is_current() {
            self.return_uncommitted_engine(engine, gen_at_start).await;
            return R::Cancelled;
        }
        enum Commit {
            Full,
            Existing,
            Added(Arc<MultiLiveSession>, bool),
        }
        let commit = {
            let mut st = self.lock();
            let limit = st.effective_max_sessions() + usize::from(allows_provisional_overflow);
            if st.sessions.len() >= limit {
                Commit::Full
            } else if (options.watch_transition && st.session_for_channel(channel_id).is_some())
                || (!options.watch_transition
                    && st.focus_existing_session(channel_id, options.start_immediately))
            {
                Commit::Existing
            } else {
                let channel_name = live_info
                    .channel
                    .as_ref()
                    .map(|c| c.channel_name.clone())
                    .unwrap_or_else(|| channel_id.to_string());
                let profile = live_info
                    .channel
                    .as_ref()
                    .and_then(|c| c.channel_image_url.clone());
                let factory = &st.core.deps.session_factory;
                let session = MultiLiveSession::with_live_info(
                    channel_id,
                    channel_name,
                    profile,
                    live_info,
                    backend.clone(),
                    factory.make_player(Some(engine.clone())),
                    factory.make_chat(),
                );
                session
                    .chat()
                    .set_user(st.user_uid.clone(), st.user_nickname.clone());
                let weak = self.shared.this_weak();
                session.set_started_hook(Some(Arc::new(move || {
                    if let Some(shared) = weak.upgrade() {
                        shared.obs.lock().reconcile_picture_enhancement();
                    }
                })));
                let is_first = st.sessions.is_empty();
                if !is_first || options.watch_transition || options.defer_auto_selection {
                    session.set_muted(true);
                    session.set_background_mode(true);
                }
                st.push_session(session.clone());
                Commit::Added(session, is_first)
            }
        };
        let (session, is_first) = match commit {
            Commit::Full => {
                tracing::warn!("MultiLive: await 이후 최대 세션 수 도달 — 추가 취소");
                pool.release(engine).await;
                return R::MaxSessionsReached;
            }
            Commit::Existing => {
                pool.release(engine).await;
                return R::ActivatedExisting;
            }
            Commit::Added(s, f) => (s, f),
        };
        let sid = session.id();
        let auto_select = is_first && !options.watch_transition && !options.defer_auto_selection;
        self.shared.core.bw().register_stream(sid, auto_select);
        {
            let st = self.read();
            if st.teardown_generation != gen_at_start || st.is_stopping_all || !st.contains(sid) {
                drop(st);
                self.shared.core.bw().unregister_stream(sid);
                return R::Cancelled;
            }
        }
        if watch_request_is_current.is_some_and(|f| !f()) {
            if self.read().selected_session_id != Some(sid) {
                self.remove_session(sid).await;
            }
            return R::Cancelled;
        }
        let over = {
            let st = self.read();
            st.sessions.len() > st.effective_max_sessions()
        };
        if over {
            if self
                .evict_least_recently_watched_session(true, Some(sid))
                .await
                .is_none()
            {
                self.remove_session(sid).await;
                return R::MaxSessionsReached;
            }
            let st = self.read();
            if st.teardown_generation != gen_at_start || st.is_stopping_all || !st.contains(sid) {
                return R::Cancelled;
            }
        }
        let mut st = self.lock();
        if auto_select {
            st.set_selected_session_id(Some(sid));
            if st.is_multi_audio_mode {
                st.audio_enabled_session_ids.insert(sid);
            }
            session.player().set_precision_sync_active(true);
            if !st.is_grid_layout {
                session.set_time_machine_desired_flag(true);
                st.stage_session_id = Some(sid);
            }
        }
        st.synchronize_bandwidth_selection();
        st.mru.note_added(sid, options.restoring_from_persistence);
        if options.start_immediately {
            let pane_count = st.sessions.len();
            st.schedule_session_start(&session, pane_count, None);
        }
        if st.sessions.len() >= 2 {
            if st.bw_coordinator_task.is_none() {
                st.start_bandwidth_coordination();
            } else {
                st.update_estimated_pane_sizes();
            }
        }
        st.save_state();
        st.reconcile_decode_budget();
        st.reconcile_hlsjs_pane_offscreen_surfaces();
        st.prewarm_next_watch_engine();
        tracing::info!("MultiLive: 세션 추가 — {channel_id}");
        R::Added { session_id: sid }
    }

    /// MRU 최하위 비보호 세션을 닫고 그 이름을 돌려준다.
    async fn evict_least_recently_watched_session(
        &self,
        allow_grid: bool,
        excluding: Option<Uuid>,
    ) -> Option<String> {
        let (victim_id, name) = {
            let st = self.read();
            if st.is_grid_layout && !allow_grid {
                return None;
            }
            let pinned: HashSet<Uuid> = [
                st.selected_session_id,
                st.stage_session_id,
                st.previous_stage_session_id,
                st.pip_session_id,
                st.audio_session_id,
                excluding,
            ]
            .into_iter()
            .flatten()
            .collect();
            let ordered: Vec<EvictionCandidate> = st
                .sessions
                .iter()
                .map(|s| EvictionCandidate {
                    id: s.id(),
                    tick: st.mru.tick(s.id()),
                    is_audible: st.is_audible_session(s),
                })
                .collect();
            let Some(victim) = policy::watch_eviction_victim(&ordered, &pinned)
                .and_then(|v| st.session(v).cloned())
            else {
                tracing::warn!("MultiLive: 시청 전환 회수 대상 없음 — 전부 선택/스테이지/PiP/가청");
                return None;
            };
            let channel_name = victim.channel_name();
            let name = if channel_name.is_empty() {
                victim.channel_id().to_string()
            } else {
                channel_name
            };
            (victim.id(), name)
        };
        tracing::info!("MultiLive: 시청 전환 자리 확보 — {name} 닫음 (MRU 최하위)");
        self.remove_session(victim_id).await;
        if let Some(toast) = &self.shared.core.deps.toast {
            toast.show(
                format!("채널 상한에 도달해 {name} 채널을 닫았습니다"),
                None,
                None,
            );
        }
        Some(name)
    }

    /// 세션 제거.
    pub async fn remove_session(&self, id: Uuid) {
        let (session, teardown_at_removal, chat_sessions) = {
            let mut st = self.lock();
            if st.removing_session_ids.contains(&id) || !st.contains(id) {
                return;
            }
            st.removing_session_ids.insert(id);
            let Some(session) = st.session(id).cloned() else {
                return;
            };
            session.invalidate_for_removal();
            let teardown = st.teardown_generation;
            if let Some(t) = st.session_startup_tasks.remove(&id) {
                t.cancel();
            }
            st.session_startup_generations.remove(&id);
            if let Some(t) = st.session_menu_lifecycle_tasks.remove(&id) {
                t.cancel();
            }
            st.session_menu_lifecycle_generations.remove(&id);
            if st.pip_session_id == Some(id) {
                st.core.deps.pip.stop_pip();
            }
            let Some(index) = st.sessions.iter().position(|s| s.id() == id) else {
                st.removing_session_ids.remove(&id);
                return;
            };
            st.sessions.remove(index);
            st.sessions_did_change();
            if st.selected_session_id == Some(id) {
                let first = st.sessions.first().map(|s| s.id());
                st.set_selected_session_id(first);
                if let Some(new_id) = first {
                    st.mru.touch(new_id);
                    st.synchronize_bandwidth_selection();
                }
                if let Some(new_selected) = st.selected_session().cloned() {
                    new_selected.set_background_mode(false);
                    new_selected.player().recover_from_background();
                    new_selected.player().set_precision_sync_active(true);
                }
            }
            if st.focus_main_session_id == Some(id) {
                let next = st
                    .selected_session_id
                    .or_else(|| st.sessions.first().map(|s| s.id()));
                st.set_focus_main_session_id(next);
            }
            if st.is_multi_audio_mode {
                st.audio_enabled_session_ids.remove(&id);
                if st.audio_session_id == Some(id) {
                    st.audio_session_id = st
                        .audio_enabled_session_ids
                        .iter()
                        .next()
                        .copied()
                        .or_else(|| st.sessions.first().map(|s| s.id()));
                }
            } else {
                let surviving = st.audio_session_id.filter(|a| st.contains(*a));
                if let Some(surviving) = surviving {
                    for s in &st.sessions {
                        if s.id() != surviving {
                            s.set_muted(true);
                        }
                    }
                } else if let Some(target) = st
                    .selected_session()
                    .or_else(|| st.sessions.first())
                    .map(|s| s.id())
                {
                    st.route_audio(target);
                } else {
                    st.audio_session_id = None;
                }
                st.audio_enabled_session_ids.remove(&id);
            }
            if st.sessions.len() <= 1 {
                st.set_grid_layout(false);
            }
            if st.sessions.is_empty() {
                st.has_applied_layout_prefs = false;
                st.is_multi_audio_mode = false;
                st.audio_enabled_session_ids.clear();
                st.audio_session_id = None;
            }
            if st.dragging_session_id == Some(id) {
                st.dragging_session_id = None;
            }
            if st.sessions.len() < 2 {
                st.stop_bandwidth_coordination();
                for s in &st.sessions {
                    s.player().set_max_rate_override(None);
                }
            } else {
                st.update_estimated_pane_sizes();
            }
            st.mru.forget(id);
            st.decode_active.remove(&id);
            if let Some(t) = st.tm_revert_tasks.remove(&id) {
                t.cancel();
            }
            if st.stage_session_id == Some(id) {
                st.stage_session_id = None;
            }
            if st.previous_stage_session_id == Some(id) {
                st.previous_stage_session_id = None;
                cancel_slot(&mut st.stage_handoff_hide_task);
            }
            st.reconcile_decode_budget();
            st.save_state();
            (session, teardown, st.chat_sessions.clone())
        };
        if let Some(cs) = chat_sessions {
            cs.remove_session_if_matching(session.channel_id(), session.chat());
        }
        tracing::info!("MultiLive: 세션 제거 — {}", session.channel_name());
        session.stop().await;
        let pool = self.shared.core.deps.engine_pool.clone();
        if let Some(engine) = session.player().detach_engine() {
            if self.read().teardown_generation == teardown_at_removal {
                pool.release(engine).await;
            } else {
                pool.discard(engine).await;
            }
        } else {
            pool.release_slot().await;
        }
        self.shared.core.bw().unregister_stream(id);
        let empty = {
            let mut st = self.lock();
            st.last_applied_cap_height.remove(&id);
            st.last_applied_hls_kbps.remove(&id);
            st.menu_paused_session_ids.remove(&id);
            st.menu_parked_session_ids.remove(&id);
            st.occlusion_suspended_session_ids.remove(&id);
            st.removing_session_ids.remove(&id);
            st.sessions.is_empty()
        };
        if empty {
            pool.trim_idle(1).await;
        }
    }

    /// 전체 해제.
    pub async fn stop_all(&self) {
        let to_tear_down = {
            let mut st = self.lock();
            if st.is_stopping_all {
                return;
            }
            st.is_stopping_all = true;
            st.teardown_generation = st.teardown_generation.wrapping_add(1);
            let sessions = st.sessions.clone();
            for s in &sessions {
                s.invalidate_for_removal();
            }
            for (_, t) in st.session_startup_tasks.drain() {
                t.cancel();
            }
            st.session_startup_generations.clear();
            for (_, t) in st.session_menu_lifecycle_tasks.drain() {
                t.cancel();
            }
            st.session_menu_lifecycle_generations.clear();
            if !st.sessions.is_empty() {
                st.current_layout_prefs()
                    .save(st.core.deps.layout_prefs.as_ref());
            }
            st.has_applied_layout_prefs = false;
            st.set_sessions(Vec::new());
            if let Some(cs) = &st.chat_sessions {
                cs.remove_borrowed_sessions_all();
            }
            st.set_selected_session_id(None);
            st.audio_session_id = None;
            st.is_multi_audio_mode = false;
            st.audio_enabled_session_ids.clear();
            st.set_grid_layout(false);
            st.set_grid_layout_mode(MultiLiveGridLayoutMode::Preset);
            st.set_layout_ratios(MultiLiveLayoutRatios::default());
            st.set_focus_direction(MultiLiveFocusDirection::Left);
            st.set_focus_main_session_id(None);
            st.dragging_session_index = None;
            st.dragging_session_id = None;
            st.bandwidth_loop_generation = st.bandwidth_loop_generation.wrapping_add(1);
            cancel_slot(&mut st.bw_coordinator_task);
            sessions
        };
        let pool = self.shared.core.deps.engine_pool.clone();
        for s in &to_tear_down {
            s.stop().await;
            match s.player().detach_engine() {
                Some(engine) => pool.release(engine).await,
                None => pool.release_slot().await,
            }
        }
        pool.drain().await;
        self.shared.core.bw().reset();
        let mut st = self.lock();
        st.clear_per_session_tracking();
        st.is_stopping_all = false;
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

impl FollowingWorkspaceHost for MultiLiveManager {
    fn session_ids(&self) -> Vec<Uuid> {
        self.read().sessions.iter().map(|s| s.id()).collect()
    }

    fn session_channel_ids(&self) -> Vec<String> {
        self.read()
            .sessions
            .iter()
            .map(|s| s.channel_id().to_string())
            .collect()
    }

    fn session_id_for_channel(&self, channel_id: &str) -> Option<Uuid> {
        self.read().session_for_channel(channel_id).map(|s| s.id())
    }

    fn selected_session_id(&self) -> Option<Uuid> {
        self.read().selected_session_id
    }

    fn presented_frame_tick(&self, session_id: Uuid) -> u64 {
        self.read()
            .session(session_id)
            .map(|s| s.player().presented_frame_tick())
            .unwrap_or(0)
    }

    fn is_adding_channel(&self, channel_id: &str) -> bool {
        self.read().is_adding_channel(channel_id)
    }

    fn is_grid_layout(&self) -> bool {
        self.read().is_grid_layout
    }

    fn set_grid_layout(&self, grid: bool) {
        self.lock().set_grid_layout(grid);
    }

    fn is_focus_layout(&self) -> bool {
        self.read().grid_layout_mode == MultiLiveGridLayoutMode::Focus
    }

    fn focus_main_session_id(&self) -> Option<Uuid> {
        self.read().focus_main_session_id
    }

    fn select_session(&self, id: Uuid, promote_to_focus_main: bool) {
        self.lock().select_session(id, promote_to_focus_main);
    }

    fn set_stage_session(&self, id: Option<Uuid>) {
        self.lock().set_stage_session(id);
    }

    fn is_audible(&self, session_id: Uuid) -> bool {
        let st = self.read();
        st.session(session_id).is_some_and(|s| st.is_audible(s))
    }

    fn toggle_session_audio(&self, session_id: Uuid) {
        self.lock().toggle_session_audio(session_id);
    }

    fn note_zap_requested(&self, channel_id: &str) {
        self.lock().note_zap_requested(channel_id);
    }

    fn forget_zap_request(&self, channel_id: &str) {
        self.lock().forget_zap_request(channel_id);
    }

    fn complete_watch_zap(&self, channel_id: &str, reason: &str) {
        self.lock().complete_watch_zap(channel_id, reason);
    }

    fn activate_new_watch_audio(&self, channel_id: &str) {
        self.lock().activate_new_watch_audio(channel_id);
    }

    fn restart_session_if_stalled(&self, channel_id: &str) {
        self.lock().restart_session_if_stalled(channel_id);
    }

    fn restore_video_surface_for_watch(&self, channel_id: &str) {
        self.lock().restore_video_surface_for_watch(channel_id);
    }

    fn park_video_surfaces_for_resident_menu_leave(&self) {
        self.lock().park_video_surfaces_for_resident_menu_leave();
    }

    fn defer_peer_surface_restore(&self) {
        self.lock().defer_peer_surface_restore();
    }

    fn restore_video_surfaces_for_menu_enter(&self) {
        self.lock().restore_video_surfaces_for_menu_enter();
    }

    fn complete_deferred_peer_restore(&self) {
        self.lock().complete_deferred_peer_restore();
    }
}
