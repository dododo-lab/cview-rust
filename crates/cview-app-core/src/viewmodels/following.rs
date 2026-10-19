//! 라이브 허브(팔로잉 메뉴) 화면 상태 — 허브 모드 프리셋, 시청·멀티 진입 요청 세대,
//! 메뉴 안 추가 체인의 되돌리기, 채팅 도크·채널 도크, Smart Queue.
//!
//! 원본: `Sources/CViewApp/ViewModels/FollowingViewState.swift`.
//!
//! Swift 는 메서드마다 `MultiLiveManager` 를 받았다. 여기서는 그 면을 [`FollowingWorkspaceHost`]
//! trait 으로 받는다(멀티라이브 매니저가 구현). `MultiChatSessionManager` 는 [`FollowingChatSessions`],
//! `UserDefaults` 는 [`KeyValueStore`](서비스 계층과 같은 저장소), `MenuTransitionGate.menuChangeEpoch` 는 `menu_epoch` 클로저다.
//! 버린 것: `NavigationTransitionMetrics`·`MenuPerfTrace`(계측), `overviewBrowserCache`(뷰 캐시),
//! 아이콘 이름 밖의 색.

use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use uuid::Uuid;

use cview_core::models::LiveChannelItem;

pub use crate::navigation::hub_mode::{
    FollowingBrowserFilter, FollowingBrowserSortOrder, FollowingHubMode, LiveDiscoverySection,
};

use super::chat::deps::ToastPresenter;
use super::chat::ChatViewModel;
use super::multi_live::MultiLiveAddSessionResult;
use super::support::{cancel_slot, Observed, ObservedGuard, TaskSlot};

// MARK: - 열거형

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StageToolPopover {
    None,
    Quality,
    Tools,
    Layout,
    Reconnect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatDockFocus {
    Balanced,
    Single,
    Multi,
}

impl ChatDockFocus {
    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Balanced => "balanced",
            Self::Single => "single",
            Self::Multi => "multi",
        }
    }
    pub fn from_raw(raw: &str) -> Option<Self> {
        [Self::Balanced, Self::Single, Self::Multi]
            .into_iter()
            .find(|v| v.raw_value() == raw)
    }
}

/// 팔로잉 목록 정렬(`Views/FollowingSortOrder.swift`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FollowingSortOrder {
    LiveFirst,
    Viewers,
    NameAsc,
    Original,
}

impl FollowingSortOrder {
    pub fn title(self) -> &'static str {
        match self {
            Self::LiveFirst => "라이브 우선",
            Self::Viewers => "시청자 많은 순",
            Self::NameAsc => "채널명 가나다순",
            Self::Original => "기본 순서",
        }
    }

    pub fn sort(self, channels: &[LiveChannelItem]) -> Vec<LiveChannelItem> {
        let mut v = channels.to_vec();
        match self {
            Self::LiveFirst => v.sort_by(|l, r| {
                r.is_live
                    .cmp(&l.is_live)
                    .then_with(|| r.viewer_count.cmp(&l.viewer_count))
            }),
            Self::Viewers => v.sort_by_key(|c| std::cmp::Reverse(c.viewer_count)),
            Self::NameAsc => v.sort_by(|l, r| l.channel_name.cmp(&r.channel_name)),
            Self::Original => {}
        }
        v
    }
}

// MARK: - 협력자

/// `MultiLiveManager` 중 라이브 허브가 쓰는 면.
pub trait FollowingWorkspaceHost: Send + Sync {
    fn session_ids(&self) -> Vec<Uuid>;
    fn session_channel_ids(&self) -> Vec<String>;
    fn session_id_for_channel(&self, channel_id: &str) -> Option<Uuid>;
    fn selected_session_id(&self) -> Option<Uuid>;
    fn presented_frame_tick(&self, session_id: Uuid) -> u64;
    fn is_adding_channel(&self, channel_id: &str) -> bool;
    fn is_grid_layout(&self) -> bool;
    fn set_grid_layout(&self, grid: bool);
    /// `gridLayoutMode == .focus`.
    fn is_focus_layout(&self) -> bool;
    fn focus_main_session_id(&self) -> Option<Uuid>;
    fn select_session(&self, id: Uuid, promote_to_focus_main: bool);
    fn set_stage_session(&self, id: Option<Uuid>);
    fn is_audible(&self, session_id: Uuid) -> bool;
    fn toggle_session_audio(&self, session_id: Uuid);
    fn note_zap_requested(&self, channel_id: &str);
    fn forget_zap_request(&self, channel_id: &str);
    fn complete_watch_zap(&self, channel_id: &str, reason: &str);
    fn activate_new_watch_audio(&self, channel_id: &str);
    fn restart_session_if_stalled(&self, channel_id: &str);
    fn restore_video_surface_for_watch(&self, channel_id: &str);
    fn park_video_surfaces_for_resident_menu_leave(&self);
    fn defer_peer_surface_restore(&self);
    fn restore_video_surfaces_for_menu_enter(&self);
    fn complete_deferred_peer_restore(&self);
}

/// `MultiChatSessionManager` 중 라이브 허브가 쓰는 면.
pub trait FollowingChatSessions: Send + Sync {
    fn remove_borrowed_sessions(&self, not_in: &HashSet<String>);
    /// 성공이면 `true`(최대 개수 초과면 `false`).
    fn adopt_live_session(&self, session_id: Uuid, channel_id: &str) -> bool;
    fn select_channel(&self, channel_id: &str);
}

#[derive(Debug, Default)]
pub struct NoopChatSessions;
impl FollowingChatSessions for NoopChatSessions {
    fn remove_borrowed_sessions(&self, _not_in: &HashSet<String>) {}
    fn adopt_live_session(&self, _session_id: Uuid, _channel_id: &str) -> bool {
        true
    }
    fn select_channel(&self, _channel_id: &str) {}
}

pub use crate::services::support::{KeyValueStore, MemoryKeyValueStore};

pub mod defaults_key {
    pub const AUTO_SYNC_CHAT_ON_MULTI_LIVE_ADD: &str =
        "following.liveHub.autoSyncChatOnMultiLiveAdd";
    pub const HUB_MODE: &str = "following.liveHub.hubMode";
    pub const CHAT_DOCK_FOCUS: &str = "following.liveHub.chatDockFocus";
    pub const BROWSER_FILTER: &str = "following.liveHub.sheetFilter";
    pub const BROWSER_SORT_ORDER: &str = "following.liveHub.sheetSortOrder";
    pub const CHAT_DOCK_COLLAPSED: &str = "following.liveHub.chatDockCollapsed";
    pub const CHANNEL_DOCK_COMPACT: &str = "following.liveHub.channelDockCompact";
    pub const REDESIGN_MIGRATION: &str = "following.liveHub.redesignMigration";
    pub const REDESIGN_MIGRATION_CURRENT: &str = "2026-04-28-unified";
}

// MARK: - 요청 토큰·되돌리기 기록

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRequestToken {
    pub generation: u64,
    pub menu_epoch: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceNavigationKind {
    Watch,
    Multi,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchNavigationOutcome {
    Committed,
    Failed(MultiLiveAddSessionResult),
    Superseded,
}

/// 진입 전 레이아웃.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MultiNavigationOrigin {
    pub mode: FollowingHubMode,
    pub grid_layout: bool,
    pub show_multi_live: bool,
    pub filter_live_only: bool,
    pub sort_order: FollowingSortOrder,
    pub chat_dock_focus: ChatDockFocus,
    pub chat_dock_reveal: bool,
}

impl MultiNavigationOrigin {
    /// 그리드 여부를 뺀 사용자 레이아웃이 같은가.
    pub fn matches_user_layout(&self, other: &Self) -> bool {
        self.mode == other.mode
            && self.show_multi_live == other.show_multi_live
            && self.filter_live_only == other.filter_live_only
            && self.sort_order == other.sort_order
            && self.chat_dock_focus == other.chat_dock_focus
            && self.chat_dock_reveal == other.chat_dock_reveal
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavigationOriginStart {
    pub origin: MultiNavigationOrigin,
    pub chain_root: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InMenuAddEntry {
    request: WorkspaceRequestToken,
    chain_root: u64,
    kind: WorkspaceNavigationKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiNavigationOriginRecord {
    pub request: WorkspaceRequestToken,
    pub origin: MultiNavigationOrigin,
    pub applied: MultiNavigationOrigin,
    pub applied_session_ids: Vec<Uuid>,
    pub chain_root: u64,
}

struct WorkspaceNavigationSlot {
    kind: WorkspaceNavigationKind,
    channel_id: String,
    serial: u64,
    task: TaskSlot,
}

/// 개요 브라우저 상호작용 상태.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowingBrowserInteractionState {
    pub is_expanded: bool,
    pub search_text: String,
    pub page_index: usize,
    pub selected_category: Option<String>,
}

impl Default for FollowingBrowserInteractionState {
    fn default() -> Self {
        Self {
            is_expanded: true,
            search_text: String::new(),
            page_index: 0,
            selected_category: None,
        }
    }
}

// MARK: - 상태

pub type MenuEpoch = Arc<dyn Fn() -> u64 + Send + Sync>;

pub struct FollowingState {
    defaults: Arc<dyn KeyValueStore>,
    menu_epoch: MenuEpoch,
    pub chat_sessions: Arc<dyn FollowingChatSessions>,
    pub toast: Option<Arc<dyn ToastPresenter>>,
    this: std::sync::Weak<FollowingShared>,

    pub sort_order: FollowingSortOrder,
    pub filter_live_only: bool,
    pub selected_category: Option<String>,
    pub overview_scroll_y: f64,
    pub overview_browser_interaction: FollowingBrowserInteractionState,
    pub chat_notice: Option<String>,
    hub_mode: FollowingHubMode,
    live_page_prefetch_task: Option<TaskSlot>,
    offline_page_prefetch_task: Option<TaskSlot>,
    workspace_request_generation: u64,
    preparing_watch_request: Option<WorkspaceRequestToken>,
    preparing_watch_channel_id: Option<String>,
    presenting_watch_channel_id: Option<String>,
    presenting_watch_channel_name: Option<String>,
    presenting_watch_request: Option<WorkspaceRequestToken>,
    presenting_multi_channel_id: Option<String>,
    presenting_multi_channel_name: Option<String>,
    presenting_multi_request: Option<WorkspaceRequestToken>,
    is_awaiting_multi_entry_motion: bool,
    multi_navigation_origin: Option<MultiNavigationOriginRecord>,
    in_menu_adds: Vec<InMenuAddEntry>,
    pending_multi_rollback: Option<MultiNavigationOriginRecord>,
    workspace_navigation: Option<WorkspaceNavigationSlot>,
    workspace_navigation_serial: u64,

    chat_dock_focus: ChatDockFocus,
    browser_filter: FollowingBrowserFilter,
    browser_sort_order: FollowingBrowserSortOrder,
    pub live_page_index: usize,
    pub offline_page_index: usize,
    pub show_multi_live: bool,
    pub show_multi_chat: bool,
    pub ml_add_error: Option<String>,
    chat_dock_collapsed: bool,
    pub stage_tool_popover: StageToolPopover,
    auto_sync_chat_on_multi_live_add: bool,
    pub smart_queue_channel_ids: Vec<String>,
    pub recent_channel_ids: Vec<String>,
    pub is_multi_live_pip_mode: bool,
    is_channel_dock_compact: bool,
    pub is_channel_dock_hidden: bool,
    pub show_chat_settings: bool,
    pub chat_settings_target_vm: Option<ChatViewModel>,
    pub chat_dock_revealed_in_full_stage: bool,
}

const GRID_HEADER_CHAT_CAPACITY_MESSAGE: &str =
    "채팅 최대 개수에 도달해 해당 채널의 채팅을 열지 못했습니다.";
const CHAT_CAPACITY_AFTER_ADD_MESSAGE: &str =
    "영상은 추가됐지만 채팅 최대 개수에 도달해 채팅을 함께 열지 못했습니다.";

impl FollowingState {
    fn new(
        this: std::sync::Weak<FollowingShared>,
        defaults: Arc<dyn KeyValueStore>,
        menu_epoch: MenuEpoch,
        chat_sessions: Arc<dyn FollowingChatSessions>,
    ) -> Self {
        use defaults_key as k;
        let mut s = Self {
            defaults: defaults.clone(),
            menu_epoch,
            chat_sessions,
            toast: None,
            this,
            sort_order: FollowingSortOrder::LiveFirst,
            filter_live_only: false,
            selected_category: None,
            overview_scroll_y: 0.0,
            overview_browser_interaction: FollowingBrowserInteractionState::default(),
            chat_notice: None,
            hub_mode: FollowingHubMode::Explore,
            live_page_prefetch_task: None,
            offline_page_prefetch_task: None,
            workspace_request_generation: 0,
            preparing_watch_request: None,
            preparing_watch_channel_id: None,
            presenting_watch_channel_id: None,
            presenting_watch_channel_name: None,
            presenting_watch_request: None,
            presenting_multi_channel_id: None,
            presenting_multi_channel_name: None,
            presenting_multi_request: None,
            is_awaiting_multi_entry_motion: false,
            multi_navigation_origin: None,
            in_menu_adds: Vec::new(),
            pending_multi_rollback: None,
            workspace_navigation: None,
            workspace_navigation_serial: 0,
            chat_dock_focus: ChatDockFocus::Balanced,
            browser_filter: FollowingBrowserFilter::All,
            browser_sort_order: FollowingBrowserSortOrder::Recommended,
            live_page_index: 0,
            offline_page_index: 0,
            show_multi_live: true,
            show_multi_chat: false,
            ml_add_error: None,
            chat_dock_collapsed: false,
            stage_tool_popover: StageToolPopover::None,
            auto_sync_chat_on_multi_live_add: true,
            smart_queue_channel_ids: Vec::new(),
            recent_channel_ids: Vec::new(),
            is_multi_live_pip_mode: false,
            is_channel_dock_compact: false,
            is_channel_dock_hidden: true,
            show_chat_settings: false,
            chat_settings_target_vm: None,
            chat_dock_revealed_in_full_stage: false,
        };
        if defaults.string(k::REDESIGN_MIGRATION).as_deref() != Some(k::REDESIGN_MIGRATION_CURRENT)
        {
            defaults.remove(k::HUB_MODE);
            defaults.remove(k::CHAT_DOCK_FOCUS);
            defaults.remove(k::BROWSER_FILTER);
            defaults.remove(k::BROWSER_SORT_ORDER);
            defaults.set_string(k::REDESIGN_MIGRATION, k::REDESIGN_MIGRATION_CURRENT);
        }
        if let Some(v) = defaults
            .value(k::AUTO_SYNC_CHAT_ON_MULTI_LIVE_ADD)
            .and_then(|v| v.as_bool())
        {
            s.set_auto_sync_chat_on_multi_live_add(v);
        }
        if let Some(v) = defaults
            .value(k::CHANNEL_DOCK_COMPACT)
            .and_then(|v| v.as_bool())
        {
            s.set_channel_dock_compact(v);
        }
        if let Some(v) = defaults
            .string(k::CHAT_DOCK_FOCUS)
            .and_then(|r| ChatDockFocus::from_raw(&r))
        {
            s.set_chat_dock_focus(v);
        }
        if let Some(v) = defaults
            .string(k::BROWSER_FILTER)
            .and_then(|r| FollowingBrowserFilter::from_raw(&r))
        {
            s.set_browser_filter(v);
        }
        if let Some(v) = defaults
            .string(k::BROWSER_SORT_ORDER)
            .and_then(|r| FollowingBrowserSortOrder::from_raw(&r))
        {
            s.set_browser_sort_order(v);
        }
        if let Some(v) = defaults
            .value(k::CHAT_DOCK_COLLAPSED)
            .and_then(|v| v.as_bool())
        {
            s.set_chat_dock_collapsed(v);
        }
        s
    }

    // MARK: 저장되는 프로퍼티

    pub fn hub_mode(&self) -> FollowingHubMode {
        self.hub_mode
    }

    pub fn set_hub_mode(&mut self, mode: FollowingHubMode) {
        if mode != self.hub_mode {
            self.hub_mode = mode;
            self.workspace_request_generation = self.workspace_request_generation.wrapping_add(1);
            self.preparing_watch_request = None;
            self.cancel_page_prefetches();
        }
        self.defaults
            .set_string(defaults_key::HUB_MODE, mode.raw_value());
    }

    pub fn chat_dock_focus(&self) -> ChatDockFocus {
        self.chat_dock_focus
    }
    pub fn set_chat_dock_focus(&mut self, v: ChatDockFocus) {
        self.chat_dock_focus = v;
        self.defaults
            .set_string(defaults_key::CHAT_DOCK_FOCUS, v.raw_value());
    }
    pub fn browser_filter(&self) -> FollowingBrowserFilter {
        self.browser_filter
    }
    pub fn set_browser_filter(&mut self, v: FollowingBrowserFilter) {
        self.browser_filter = v;
        self.defaults
            .set_string(defaults_key::BROWSER_FILTER, v.raw_value());
    }
    pub fn browser_sort_order(&self) -> FollowingBrowserSortOrder {
        self.browser_sort_order
    }
    pub fn set_browser_sort_order(&mut self, v: FollowingBrowserSortOrder) {
        self.browser_sort_order = v;
        self.defaults
            .set_string(defaults_key::BROWSER_SORT_ORDER, v.raw_value());
    }
    pub fn chat_dock_collapsed(&self) -> bool {
        self.chat_dock_collapsed
    }
    pub fn set_chat_dock_collapsed(&mut self, v: bool) {
        self.chat_dock_collapsed = v;
        self.defaults.set_bool(defaults_key::CHAT_DOCK_COLLAPSED, v);
    }
    pub fn auto_sync_chat_on_multi_live_add(&self) -> bool {
        self.auto_sync_chat_on_multi_live_add
    }
    pub fn set_auto_sync_chat_on_multi_live_add(&mut self, v: bool) {
        self.auto_sync_chat_on_multi_live_add = v;
        self.defaults
            .set_bool(defaults_key::AUTO_SYNC_CHAT_ON_MULTI_LIVE_ADD, v);
    }
    pub fn is_channel_dock_compact(&self) -> bool {
        self.is_channel_dock_compact
    }
    pub fn set_channel_dock_compact(&mut self, v: bool) {
        self.is_channel_dock_compact = v;
        self.defaults
            .set_bool(defaults_key::CHANNEL_DOCK_COMPACT, v);
    }

    // MARK: 페이지 선읽기

    /// 같은 종류의 직전 선읽기를 취소하고 새로 띄운다.
    pub fn replace_page_prefetch<F>(&mut self, is_live: bool, operation: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.cancel_page_prefetch(is_live);
        let task = TaskSlot::spawn(move |_| operation);
        if is_live {
            self.live_page_prefetch_task = Some(task);
        } else {
            self.offline_page_prefetch_task = Some(task);
        }
    }

    pub fn cancel_page_prefetch(&mut self, is_live: bool) {
        if is_live {
            cancel_slot(&mut self.live_page_prefetch_task);
        } else {
            cancel_slot(&mut self.offline_page_prefetch_task);
        }
    }

    pub fn cancel_page_prefetches(&mut self) {
        self.cancel_page_prefetch(true);
        self.cancel_page_prefetch(false);
    }

    // MARK: 요청 세대

    pub fn begin_workspace_request(&mut self) -> WorkspaceRequestToken {
        self.workspace_request_generation = self.workspace_request_generation.wrapping_add(1);
        WorkspaceRequestToken {
            generation: self.workspace_request_generation,
            menu_epoch: (self.menu_epoch)(),
        }
    }

    pub fn is_current_workspace_request(&self, token: &WorkspaceRequestToken) -> bool {
        token.generation == self.workspace_request_generation
            && token.menu_epoch == (self.menu_epoch)()
    }

    pub fn current_menu_epoch(&self) -> u64 {
        (self.menu_epoch)()
    }

    pub fn workspace_request_generation(&self) -> u64 {
        self.workspace_request_generation
    }

    pub fn is_preparing_watch(&self) -> bool {
        self.preparing_watch_request
            .is_some_and(|r| self.is_current_workspace_request(&r))
    }

    pub fn active_watch_preparation_channel_id(&self) -> Option<&str> {
        if self.is_preparing_watch() {
            self.preparing_watch_channel_id.as_deref()
        } else {
            None
        }
    }

    pub fn preparing_watch_request(&self) -> Option<WorkspaceRequestToken> {
        self.preparing_watch_request
    }
    pub fn preparing_watch_channel_id(&self) -> Option<&str> {
        self.preparing_watch_channel_id.as_deref()
    }
    pub fn presenting_watch_channel_id(&self) -> Option<&str> {
        self.presenting_watch_channel_id.as_deref()
    }
    pub fn presenting_watch_channel_name(&self) -> Option<&str> {
        self.presenting_watch_channel_name.as_deref()
    }
    pub fn presenting_multi_channel_id(&self) -> Option<&str> {
        self.presenting_multi_channel_id.as_deref()
    }
    pub fn presenting_multi_channel_name(&self) -> Option<&str> {
        self.presenting_multi_channel_name.as_deref()
    }
    pub fn is_awaiting_multi_entry_motion(&self) -> bool {
        self.is_awaiting_multi_entry_motion
    }

    // MARK: 시청 준비·표시

    pub fn begin_watch_preparation(
        &mut self,
        channel_id: &str,
        channel_name: Option<&str>,
        host: Option<&dyn FollowingWorkspaceHost>,
        presents: bool,
    ) -> WorkspaceRequestToken {
        if let Some(previous) = self.preparing_watch_channel_id.clone() {
            if previous != channel_id {
                if let Some(h) = host {
                    h.forget_zap_request(&previous);
                }
            }
        }
        let request = self.begin_workspace_request();
        self.preparing_watch_request = Some(request);
        self.preparing_watch_channel_id = Some(channel_id.to_string());
        if presents {
            self.presenting_watch_channel_id = Some(channel_id.to_string());
            self.presenting_watch_channel_name = channel_name.map(str::to_string);
            self.presenting_watch_request = Some(request);
        } else if self.presenting_watch_channel_id.is_some() {
            self.clear_watch_presentation();
            if let Some(h) = host {
                self.flush_deferred_surface_restore(h);
            }
        }
        request
    }

    pub fn finish_watch_preparation(&mut self, request: WorkspaceRequestToken) {
        if self.preparing_watch_request == Some(request) {
            self.preparing_watch_request = None;
            self.preparing_watch_channel_id = None;
        }
    }

    fn clear_watch_presentation(&mut self) {
        self.presenting_watch_channel_id = None;
        self.presenting_watch_channel_name = None;
        self.presenting_watch_request = None;
    }

    fn abandon_watch_presentation(
        &mut self,
        request: &WorkspaceRequestToken,
        host: &dyn FollowingWorkspaceHost,
    ) {
        if self.presenting_watch_request.as_ref() != Some(request) {
            return;
        }
        self.clear_watch_presentation();
        self.flush_deferred_surface_restore(host);
    }

    pub fn supersede_watch_preparation(&mut self, host: Option<&dyn FollowingWorkspaceHost>) {
        if let (Some(id), Some(h)) = (&self.preparing_watch_channel_id, host) {
            h.forget_zap_request(id);
        }
        if let Some(presenting) = &self.presenting_watch_channel_id {
            if Some(presenting) != self.preparing_watch_channel_id.as_ref() {
                if let Some(h) = host {
                    h.forget_zap_request(presenting);
                }
            }
        }
        let _ = self.begin_workspace_request();
        if let Some(r) = self.preparing_watch_request {
            self.finish_watch_preparation(r);
        }
        self.clear_watch_presentation();
    }

    pub fn finish_watch_presentation_if_ready(&mut self, host: &dyn FollowingWorkspaceHost) {
        let Some(channel_id) = self.presenting_watch_channel_id.clone() else {
            return;
        };
        let Some(session) = host.session_id_for_channel(&channel_id) else {
            return;
        };
        if host.selected_session_id() != Some(session) || host.presented_frame_tick(session) == 0 {
            return;
        }
        self.clear_watch_presentation();
        host.complete_watch_zap(&channel_id, "ready");
        self.flush_deferred_surface_restore(host);
    }

    pub fn time_out_watch_presentation(&mut self, host: &dyn FollowingWorkspaceHost) {
        let Some(channel_id) = self.presenting_watch_channel_id.clone() else {
            return;
        };
        self.clear_watch_presentation();
        host.complete_watch_zap(&channel_id, "timeout");
        self.flush_deferred_surface_restore(host);
    }

    pub fn flush_deferred_surface_restore(&self, host: &dyn FollowingWorkspaceHost) {
        if self.hub_mode.occupies_full_stage() {
            return;
        }
        host.complete_deferred_peer_restore();
    }

    pub fn apply_menu_enter_surface_policy(
        &self,
        has_pending_watch_request: bool,
        host: &dyn FollowingWorkspaceHost,
    ) {
        if self.hub_mode.occupies_full_stage() || self.is_awaiting_multi_entry_motion {
            host.park_video_surfaces_for_resident_menu_leave();
        } else if has_pending_watch_request || self.presenting_watch_channel_id.is_some() {
            host.defer_peer_surface_restore();
        } else {
            host.restore_video_surfaces_for_menu_enter();
        }
    }

    fn is_navigation_in_flight(&self, kind: WorkspaceNavigationKind, channel_id: &str) -> bool {
        match kind {
            WorkspaceNavigationKind::Watch => {
                self.is_preparing_watch()
                    && self.preparing_watch_channel_id.as_deref() == Some(channel_id)
            }
            WorkspaceNavigationKind::Multi => {
                self.presenting_multi_channel_id.as_deref() == Some(channel_id)
                    && self
                        .presenting_multi_request
                        .is_some_and(|r| self.is_current_workspace_request(&r))
            }
        }
    }

    fn commit_added_watch(
        &mut self,
        channel_id: &str,
        result: &MultiLiveAddSessionResult,
        request: WorkspaceRequestToken,
        host: &dyn FollowingWorkspaceHost,
    ) {
        if !matches!(result, MultiLiveAddSessionResult::Added { .. }) {
            self.commit_existing_watch(channel_id, request, host);
            return;
        }
        self.commit_prepared_watch(channel_id, request, host);
        host.activate_new_watch_audio(channel_id);
    }

    fn commit_existing_watch(
        &mut self,
        channel_id: &str,
        request: WorkspaceRequestToken,
        host: &dyn FollowingWorkspaceHost,
    ) {
        if !self.commit_prepared_watch(channel_id, request, host) {
            return;
        }
        host.restart_session_if_stalled(channel_id);
    }

    pub fn finish_multi_presentation_if_ready(&mut self, host: &dyn FollowingWorkspaceHost) {
        let Some(channel_id) = self.presenting_multi_channel_id.clone() else {
            return;
        };
        let Some(session) = host.session_id_for_channel(&channel_id) else {
            return;
        };
        if host.presented_frame_tick(session) == 0 {
            return;
        }
        self.presenting_multi_channel_id = None;
        self.presenting_multi_channel_name = None;
        self.presenting_multi_request = None;
    }

    pub fn time_out_multi_presentation(&mut self) {
        self.presenting_multi_channel_id = None;
        self.presenting_multi_channel_name = None;
        self.presenting_multi_request = None;
        self.is_awaiting_multi_entry_motion = false;
    }

    pub fn commit_prepared_watch(
        &mut self,
        channel_id: &str,
        request: WorkspaceRequestToken,
        host: &dyn FollowingWorkspaceHost,
    ) -> bool {
        if !self.is_current_workspace_request(&request)
            || host.session_id_for_channel(channel_id).is_none()
        {
            return false;
        }
        self.settle_multi_navigation_origin(&request);
        host.set_grid_layout(false);
        host.set_stage_session(None);
        self.activate_added_live_channel(channel_id, host, false);
        if let Some(session) = host.session_id_for_channel(channel_id) {
            host.set_stage_session(Some(session));
        }
        host.restore_video_surface_for_watch(channel_id);
        self.apply_hub_mode_preset(FollowingHubMode::Watch, host, false, false);
        self.finish_watch_preparation(request);
        self.finish_watch_presentation_if_ready(host);
        true
    }

    pub fn sync_multi_grid_layout(&self, host: &dyn FollowingWorkspaceHost) {
        if self.hub_mode != FollowingHubMode::Multi
            || host.session_ids().len() < 2
            || host.is_grid_layout()
        {
            return;
        }
        host.set_grid_layout(true);
    }

    // MARK: Smart Queue

    /// 개요의 라이브 채널 중 활성·대기열에 없는 것을 붙인다.
    pub fn enqueue_overview_channels(
        &mut self,
        channels: &[LiveChannelItem],
        active_ids: &HashSet<String>,
    ) -> usize {
        let mut seen: HashSet<String> = active_ids.clone();
        seen.extend(self.smart_queue_channel_ids.iter().cloned());
        let additions: Vec<String> = channels
            .iter()
            .filter(|c| c.is_live && seen.insert(c.channel_id.clone()))
            .map(|c| c.channel_id.clone())
            .collect();
        let n = additions.len();
        self.smart_queue_channel_ids.extend(additions);
        n
    }

    pub fn clear_smart_queue_with_undo(&mut self) {
        let previous = std::mem::take(&mut self.smart_queue_channel_ids);
        if previous.is_empty() {
            return;
        }
        let Some(toast) = self.toast.clone() else {
            return;
        };
        let weak = self.this.clone();
        let count = previous.len();
        toast.show(
            format!("Smart Queue 를 비웠습니다 ({count}개)"),
            Some("실행 취소".into()),
            Some(Box::new(move || {
                if let Some(shared) = weak.upgrade() {
                    shared.obs.lock().smart_queue_channel_ids = previous;
                }
            })),
        );
    }

    // MARK: 도크

    pub fn present_channel_dock(&mut self) {
        self.is_channel_dock_hidden = false;
    }
    pub fn hide_channel_dock(&mut self) {
        self.is_channel_dock_hidden = true;
    }
    pub fn toggle_channel_dock(&mut self) {
        if self.hub_mode != FollowingHubMode::Watch && self.hub_mode != FollowingHubMode::Multi {
            return;
        }
        self.is_channel_dock_hidden = !self.is_channel_dock_hidden;
    }

    pub fn present_chat_dock(&mut self) {
        self.show_multi_chat = true;
        self.set_chat_dock_collapsed(false);
        self.chat_dock_revealed_in_full_stage = true;
    }

    pub fn toggle_chat_dock(&mut self) {
        if self.show_multi_chat {
            self.show_multi_chat = false;
        } else {
            self.present_chat_dock();
        }
    }

    pub fn activate_added_live_channel(
        &mut self,
        channel_id: &str,
        host: &dyn FollowingWorkspaceHost,
        preserves_main_focus: bool,
    ) {
        let Some(session) = host.session_id_for_channel(channel_id) else {
            return;
        };
        if preserves_main_focus && self.adopt_chat_keeping_main_focus(session, channel_id, host) {
            return;
        }
        let promote = host.is_grid_layout() && host.is_focus_layout();
        if host.selected_session_id() != Some(session)
            || (promote && host.focus_main_session_id() != Some(session))
        {
            host.select_session(session, promote);
        }
        if !self.auto_sync_chat_on_multi_live_add {
            return;
        }
        let channels: HashSet<String> = host.session_channel_ids().into_iter().collect();
        self.chat_sessions.remove_borrowed_sessions(&channels);
        if !self.chat_sessions.adopt_live_session(session, channel_id) {
            self.ml_add_error = Some(CHAT_CAPACITY_AFTER_ADD_MESSAGE.into());
            return;
        }
        self.chat_sessions.select_channel(channel_id);
        self.present_chat_dock();
    }

    pub fn activate_grid_header_channel(
        &mut self,
        channel_id: &str,
        host: &dyn FollowingWorkspaceHost,
        reveals_chat_dock: bool,
    ) {
        let Some(session) = host.session_id_for_channel(channel_id) else {
            return;
        };
        if host.selected_session_id() != Some(session) {
            host.select_session(session, false);
        }
        if !host.is_audible(session) {
            host.toggle_session_audio(session);
        }
        if !self.chat_sessions.adopt_live_session(session, channel_id) {
            self.ml_add_error = Some(GRID_HEADER_CHAT_CAPACITY_MESSAGE.into());
            return;
        }
        if self.ml_add_error.as_deref() == Some(GRID_HEADER_CHAT_CAPACITY_MESSAGE) {
            self.ml_add_error = None;
        }
        self.chat_sessions.select_channel(channel_id);
        if reveals_chat_dock {
            self.present_chat_dock();
        }
    }

    pub fn adopt_chat_keeping_main_focus(
        &mut self,
        session: Uuid,
        channel_id: &str,
        host: &dyn FollowingWorkspaceHost,
    ) -> bool {
        let Some(current) = host.selected_session_id() else {
            return false;
        };
        if current == session || !host.session_ids().contains(&current) {
            return false;
        }
        if !self.auto_sync_chat_on_multi_live_add {
            return true;
        }
        let channels: HashSet<String> = host.session_channel_ids().into_iter().collect();
        self.chat_sessions.remove_borrowed_sessions(&channels);
        if self.chat_sessions.adopt_live_session(session, channel_id) {
            return true;
        }
        self.ml_add_error = Some(CHAT_CAPACITY_AFTER_ADD_MESSAGE.into());
        true
    }

    // MARK: 허브 모드 프리셋

    pub fn apply_hub_mode_preset(
        &mut self,
        mode: FollowingHubMode,
        host: &dyn FollowingWorkspaceHost,
        deferring_watch_activation: bool,
        deferring_surface_restore: bool,
    ) {
        if mode != FollowingHubMode::Watch && self.presenting_watch_channel_id.is_some() {
            self.supersede_watch_preparation(Some(host));
        }
        if mode != FollowingHubMode::Multi && self.presenting_multi_channel_id.is_some() {
            self.time_out_multi_presentation();
        }
        if self.hub_mode == mode {
            self.sync_multi_grid_layout(host);
            return;
        }
        let was_full_stage = self.hub_mode.occupies_full_stage();
        self.chat_dock_revealed_in_full_stage = false;
        self.set_hub_mode(mode);
        match mode {
            FollowingHubMode::Explore => {
                if self.chat_dock_focus != ChatDockFocus::Balanced {
                    self.set_chat_dock_focus(ChatDockFocus::Balanced);
                }
                self.filter_live_only = false;
                self.sort_order = FollowingSortOrder::LiveFirst;
                if host.is_grid_layout() {
                    host.set_grid_layout(false);
                }
                self.show_multi_live = false;
            }
            FollowingHubMode::Watch => {
                if self.chat_dock_focus != ChatDockFocus::Single {
                    self.set_chat_dock_focus(ChatDockFocus::Single);
                }
                self.filter_live_only = true;
                self.sort_order = FollowingSortOrder::LiveFirst;
                if !deferring_watch_activation && host.is_grid_layout() {
                    host.set_grid_layout(false);
                }
                self.show_multi_live = true;
            }
            FollowingHubMode::Multi => {
                if self.chat_dock_focus != ChatDockFocus::Multi {
                    self.set_chat_dock_focus(ChatDockFocus::Multi);
                }
                self.filter_live_only = true;
                self.sort_order = FollowingSortOrder::LiveFirst;
                if !host.is_grid_layout() {
                    host.set_grid_layout(true);
                }
                self.show_multi_live = true;
            }
            FollowingHubMode::Search | FollowingHubMode::Popular | FollowingHubMode::Category => {
                if self.chat_dock_focus != ChatDockFocus::Balanced {
                    self.set_chat_dock_focus(ChatDockFocus::Balanced);
                }
            }
        }
        let defers_restore = deferring_watch_activation || deferring_surface_restore;
        if !was_full_stage && mode.occupies_full_stage() {
            host.park_video_surfaces_for_resident_menu_leave();
        } else if was_full_stage && !mode.occupies_full_stage() {
            if defers_restore {
                host.defer_peer_surface_restore();
            } else {
                host.restore_video_surfaces_for_menu_enter();
            }
        } else if !mode.occupies_full_stage() && !defers_restore {
            host.complete_deferred_peer_restore();
        }
    }

    // MARK: 진입 전 레이아웃 기록·되돌리기

    pub fn begin_multi_navigation_request(
        &mut self,
        host: &dyn FollowingWorkspaceHost,
        apply_preset: impl FnOnce(&mut Self),
    ) -> MultiNavigationOriginRecord {
        let start = self.navigation_origin_start(host, true);
        apply_preset(self);
        let request = self.begin_workspace_request();
        self.record_navigation_origin(start, request, host)
    }

    pub fn navigation_origin_start(
        &self,
        host: &dyn FollowingWorkspaceHost,
        joins_in_menu_chain: bool,
    ) -> NavigationOriginStart {
        let inherited = self.inherited_multi_navigation_origin(host).filter(|r| {
            joins_in_menu_chain
                || !self.has_in_menu_add(r.chain_root, WorkspaceNavigationKind::Multi)
        });
        NavigationOriginStart {
            origin: inherited
                .as_ref()
                .map(|r| r.origin)
                .unwrap_or_else(|| self.capture_multi_navigation_origin(host)),
            chain_root: inherited.map(|r| r.chain_root),
        }
    }

    fn record_navigation_origin(
        &mut self,
        start: NavigationOriginStart,
        request: WorkspaceRequestToken,
        host: &dyn FollowingWorkspaceHost,
    ) -> MultiNavigationOriginRecord {
        let record = MultiNavigationOriginRecord {
            request,
            origin: start.origin,
            applied: self.capture_multi_navigation_origin(host),
            applied_session_ids: host.session_ids(),
            chain_root: start.chain_root.unwrap_or(request.generation),
        };
        self.multi_navigation_origin = Some(record.clone());
        self.pending_multi_rollback = None;
        record
    }

    pub fn begin_in_menu_watch_add(
        &mut self,
        start: NavigationOriginStart,
        request: WorkspaceRequestToken,
        host: &dyn FollowingWorkspaceHost,
    ) -> MultiNavigationOriginRecord {
        let record = self.record_navigation_origin(start, request, host);
        self.in_menu_adds.push(InMenuAddEntry {
            request,
            chain_root: record.chain_root,
            kind: WorkspaceNavigationKind::Watch,
        });
        record
    }

    pub fn roll_back_watch_navigation(
        &mut self,
        started: &MultiNavigationOriginRecord,
        host: &dyn FollowingWorkspaceHost,
    ) {
        self.settle_multi_navigation_origin(&started.request);
        if Self::has_session_added(started, host) {
            return;
        }
        if !self
            .in_menu_adds
            .iter()
            .any(|e| e.chain_root == started.chain_root)
        {
            self.restore_multi_navigation_origin(started.origin, host, false);
            return;
        }
        self.pending_multi_rollback = Some(MultiNavigationOriginRecord {
            request: WorkspaceRequestToken {
                generation: self.workspace_request_generation,
                menu_epoch: (self.menu_epoch)(),
            },
            origin: started.origin,
            applied: self.capture_multi_navigation_origin(host),
            applied_session_ids: host.session_ids(),
            chain_root: started.chain_root,
        });
    }

    fn has_in_menu_add(&self, chain_root: u64, kind: WorkspaceNavigationKind) -> bool {
        self.in_menu_adds
            .iter()
            .any(|e| e.chain_root == chain_root && e.kind == kind)
    }

    fn inherited_multi_navigation_origin(
        &self,
        host: &dyn FollowingWorkspaceHost,
    ) -> Option<MultiNavigationOriginRecord> {
        let session_ids = host.session_ids();
        let layout = self.capture_multi_navigation_origin(host);
        [&self.multi_navigation_origin, &self.pending_multi_rollback]
            .into_iter()
            .flatten()
            .find(|r| {
                r.request.generation == self.workspace_request_generation
                    && r.applied_session_ids == session_ids
                    && layout.matches_user_layout(&r.applied)
            })
            .cloned()
    }

    pub fn settle_multi_navigation_origin(&mut self, request: &WorkspaceRequestToken) {
        if self.multi_navigation_origin.as_ref().map(|r| &r.request) == Some(request) {
            self.multi_navigation_origin = None;
        }
    }

    pub fn abandon_multi_navigation_origin(&mut self, request: &WorkspaceRequestToken) {
        if self.multi_navigation_origin.as_ref().map(|r| &r.request) != Some(request)
            || request.generation == self.workspace_request_generation
        {
            return;
        }
        self.multi_navigation_origin = None;
    }

    pub fn begin_in_menu_multi_add(
        &mut self,
        host: &dyn FollowingWorkspaceHost,
        apply_preset: impl FnOnce(&mut Self),
    ) -> MultiNavigationOriginRecord {
        let record = self.begin_multi_navigation_request(host, apply_preset);
        self.in_menu_adds.push(InMenuAddEntry {
            request: record.request,
            chain_root: record.chain_root,
            kind: WorkspaceNavigationKind::Multi,
        });
        record
    }

    pub fn finish_in_menu_multi_add(
        &mut self,
        result: &MultiLiveAddSessionResult,
        started: &MultiNavigationOriginRecord,
        host: &dyn FollowingWorkspaceHost,
    ) -> bool {
        if let Some(i) = self
            .in_menu_adds
            .iter()
            .position(|e| e.request == started.request)
        {
            self.in_menu_adds.remove(i);
        }
        self.resolve_pending_multi_rollback(host);
        if !self.is_current_workspace_request(&started.request) {
            self.abandon_multi_navigation_origin(&started.request);
            return false;
        }
        self.settle_multi_navigation_origin(&started.request);
        if result.is_success() {
            return true;
        }
        if *result == MultiLiveAddSessionResult::Cancelled {
            return false;
        }
        if host.session_ids().is_empty() {
            self.roll_back_multi_navigation(started, host);
        }
        false
    }

    /// Smart Queue 일괄 추가 마무리. 화면이 활성화할 채널을 돌려준다.
    pub fn finish_smart_queue_batch(
        &mut self,
        outcomes: &[(String, MultiLiveAddSessionResult)],
        started: Option<&MultiNavigationOriginRecord>,
        host: &dyn FollowingWorkspaceHost,
        live_menu_visible: bool,
    ) -> Option<String> {
        let started = started?;
        let results: Vec<MultiLiveAddSessionResult> =
            outcomes.iter().map(|(_, r)| r.clone()).collect();
        let result = MultiLiveAddSessionResult::batch_outcome(&results);
        let was_current = self.is_current_workspace_request(&started.request);
        if self.finish_in_menu_multi_add(&result, started, host) {
            return outcomes
                .iter()
                .rev()
                .find(|(_, r)| r.is_success())
                .map(|(c, _)| c.clone());
        }
        if was_current
            || started.request.generation != self.workspace_request_generation
            || result.is_success()
            || result == MultiLiveAddSessionResult::Cancelled
            || !host.session_ids().is_empty()
            || self
                .in_menu_adds
                .iter()
                .any(|e| e.chain_root == started.chain_root)
            || !self
                .capture_multi_navigation_origin(host)
                .matches_user_layout(&started.applied)
        {
            return None;
        }
        self.settle_multi_navigation_origin(&started.request);
        self.restore_multi_navigation_origin(started.origin, host, !live_menu_visible);
        None
    }

    fn roll_back_multi_navigation(
        &mut self,
        started: &MultiNavigationOriginRecord,
        host: &dyn FollowingWorkspaceHost,
    ) {
        if Self::has_session_added(started, host) {
            return;
        }
        if self
            .in_menu_adds
            .iter()
            .any(|e| e.chain_root == started.chain_root)
        {
            self.pending_multi_rollback = Some(started.clone());
        } else {
            self.restore_multi_navigation_origin(started.origin, host, false);
        }
    }

    fn has_session_added(
        started: &MultiNavigationOriginRecord,
        host: &dyn FollowingWorkspaceHost,
    ) -> bool {
        host.session_ids()
            .iter()
            .any(|id| !started.applied_session_ids.contains(id))
    }

    fn resolve_pending_multi_rollback(&mut self, host: &dyn FollowingWorkspaceHost) {
        let Some(pending) = self.pending_multi_rollback.clone() else {
            return;
        };
        if !self.is_current_workspace_request(&pending.request)
            || Self::has_session_added(&pending, host)
            || !self
                .capture_multi_navigation_origin(host)
                .matches_user_layout(&pending.applied)
        {
            self.pending_multi_rollback = None;
            return;
        }
        if self
            .in_menu_adds
            .iter()
            .any(|e| e.chain_root == pending.chain_root)
        {
            return;
        }
        self.pending_multi_rollback = None;
        self.restore_multi_navigation_origin(pending.origin, host, false);
    }

    fn capture_multi_navigation_origin(
        &self,
        host: &dyn FollowingWorkspaceHost,
    ) -> MultiNavigationOrigin {
        MultiNavigationOrigin {
            mode: self.hub_mode,
            grid_layout: host.is_grid_layout(),
            show_multi_live: self.show_multi_live,
            filter_live_only: self.filter_live_only,
            sort_order: self.sort_order,
            chat_dock_focus: self.chat_dock_focus,
            chat_dock_reveal: self.chat_dock_revealed_in_full_stage,
        }
    }

    fn restore_multi_navigation_origin(
        &mut self,
        origin: MultiNavigationOrigin,
        host: &dyn FollowingWorkspaceHost,
        off_menu: bool,
    ) {
        if off_menu {
            if origin.mode != FollowingHubMode::Watch && self.presenting_watch_channel_id.is_some()
            {
                self.clear_watch_presentation();
            }
            if origin.mode != FollowingHubMode::Multi && self.presenting_multi_channel_id.is_some()
            {
                self.time_out_multi_presentation();
            }
            if self.hub_mode != origin.mode {
                self.set_hub_mode(origin.mode);
            }
        } else {
            self.apply_hub_mode_preset(origin.mode, host, false, false);
        }
        host.set_grid_layout(origin.grid_layout);
        self.show_multi_live = origin.show_multi_live;
        self.filter_live_only = origin.filter_live_only;
        self.sort_order = origin.sort_order;
        self.set_chat_dock_focus(origin.chat_dock_focus);
        self.chat_dock_revealed_in_full_stage = origin.chat_dock_reveal;
    }
}

// MARK: - 핸들

pub struct FollowingShared {
    obs: Observed<FollowingState>,
}

#[derive(Clone)]
pub struct FollowingViewState {
    shared: Arc<FollowingShared>,
}

impl FollowingViewState {
    /// 진입 모션 대기 — 줄인 동작이면 35ms, 아니면 180ms.
    pub async fn wait_for_live_entry_motion(reduced_motion: bool) {
        tokio::time::sleep(Duration::from_millis(if reduced_motion { 35 } else { 180 })).await;
    }

    pub const ADDING_POLL_INTERVAL: Duration = Duration::from_millis(40);

    pub fn new(
        defaults: Arc<dyn KeyValueStore>,
        menu_epoch: MenuEpoch,
        chat_sessions: Arc<dyn FollowingChatSessions>,
    ) -> Self {
        let shared = Arc::new_cyclic(|weak| FollowingShared {
            obs: Observed::new(FollowingState::new(
                weak.clone(),
                defaults,
                menu_epoch,
                chat_sessions,
            )),
        });
        Self { shared }
    }

    pub fn lock(&self) -> ObservedGuard<'_, FollowingState> {
        self.shared.obs.lock()
    }
    pub fn read(&self) -> ObservedGuard<'_, FollowingState> {
        self.shared.obs.read()
    }
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.shared.obs.subscribe()
    }
    pub fn snapshot(&self) -> FollowingSnapshot {
        FollowingSnapshot::from_state(&self.read())
    }

    /// 같은 종류·채널의 진입이 이미 진행 중이면 무시하고, 아니면 직전 진입을 취소하고 띄운다.
    pub fn run_workspace_navigation<F>(
        &self,
        kind: WorkspaceNavigationKind,
        channel_id: &str,
        operation: F,
    ) where
        F: Future<Output = ()> + Send + 'static,
    {
        let mut s = self.lock();
        if let Some(running) = &s.workspace_navigation {
            if running.kind == kind
                && running.channel_id == channel_id
                && s.is_navigation_in_flight(kind, channel_id)
            {
                return;
            }
        }
        if let Some(prev) = s.workspace_navigation.take() {
            prev.task.cancel();
        }
        s.workspace_navigation_serial = s.workspace_navigation_serial.wrapping_add(1);
        let serial = s.workspace_navigation_serial;
        let weak = Arc::downgrade(&self.shared);
        let task = TaskSlot::spawn(move |_| async move {
            operation.await;
            let Some(shared) = weak.upgrade() else { return };
            let mut s = shared.obs.lock();
            if s.workspace_navigation.as_ref().map(|w| w.serial) == Some(serial) {
                s.workspace_navigation = None;
            }
        });
        s.workspace_navigation = Some(WorkspaceNavigationSlot {
            kind,
            channel_id: channel_id.to_string(),
            serial,
            task,
        });
    }

    /// 시청 모드로 채널을 연다.
    pub async fn navigate_to_watch<W, A, AF>(
        &self,
        channel_id: &str,
        channel_name: Option<&str>,
        host: Arc<dyn FollowingWorkspaceHost>,
        wait_for_entry_motion: W,
        add_session: A,
    ) -> WatchNavigationOutcome
    where
        W: Future<Output = ()>,
        A: FnOnce(WorkspaceRequestToken) -> AF,
        AF: Future<Output = MultiLiveAddSessionResult>,
    {
        let h = host.as_ref();
        let (request, started) = {
            let mut s = self.lock();
            if s.is_preparing_watch() && s.preparing_watch_channel_id.as_deref() == Some(channel_id)
            {
                return WatchNavigationOutcome::Superseded;
            }
            let start = s.navigation_origin_start(h, false);
            h.note_zap_requested(channel_id);
            s.apply_hub_mode_preset(FollowingHubMode::Watch, h, true, false);
            let request = s.begin_watch_preparation(channel_id, channel_name, Some(h), true);
            let started = s.record_navigation_origin(start, request, h);
            if h.session_id_for_channel(channel_id).is_some() {
                s.commit_existing_watch(channel_id, request, h);
                return WatchNavigationOutcome::Committed;
            }
            (request, started)
        };
        let outcome = self
            .navigate_to_watch_body(
                channel_id,
                h,
                request,
                &started,
                wait_for_entry_motion,
                add_session,
            )
            .await;
        self.lock().finish_watch_preparation(request);
        outcome
    }

    async fn navigate_to_watch_body<W, A, AF>(
        &self,
        channel_id: &str,
        h: &dyn FollowingWorkspaceHost,
        request: WorkspaceRequestToken,
        started: &MultiNavigationOriginRecord,
        wait_for_entry_motion: W,
        add_session: A,
    ) -> WatchNavigationOutcome
    where
        W: Future<Output = ()>,
        A: FnOnce(WorkspaceRequestToken) -> AF,
        AF: Future<Output = MultiLiveAddSessionResult>,
    {
        let superseded = |s: &mut FollowingState| {
            h.forget_zap_request(channel_id);
            if !s.is_current_workspace_request(&request) {
                s.abandon_watch_presentation(&request, h);
            }
            s.abandon_multi_navigation_origin(&request);
            WatchNavigationOutcome::Superseded
        };
        wait_for_entry_motion.await;
        if !self.read().is_current_workspace_request(&request) {
            return superseded(&mut self.lock());
        }
        while h.is_adding_channel(channel_id) {
            tokio::time::sleep(Self::ADDING_POLL_INTERVAL).await;
            if !self.read().is_current_workspace_request(&request) {
                return superseded(&mut self.lock());
            }
        }
        if h.session_id_for_channel(channel_id).is_some() {
            self.lock().commit_existing_watch(channel_id, request, h);
            return WatchNavigationOutcome::Committed;
        }
        let result = add_session(request).await;
        let mut s = self.lock();
        if !s.is_current_workspace_request(&request) {
            return superseded(&mut s);
        }
        if !result.is_success() {
            h.forget_zap_request(channel_id);
            s.supersede_watch_preparation(Some(h));
            s.roll_back_watch_navigation(started, h);
            s.flush_deferred_surface_restore(h);
            return WatchNavigationOutcome::Failed(result);
        }
        s.commit_added_watch(channel_id, &result, request, h);
        WatchNavigationOutcome::Committed
    }

    /// 멀티 모드에 채널을 더한다.
    pub async fn navigate_to_multi<W, A, AF>(
        &self,
        channel_id: &str,
        channel_name: Option<&str>,
        host: Arc<dyn FollowingWorkspaceHost>,
        wait_for_entry_motion: W,
        add_session: A,
    ) -> WatchNavigationOutcome
    where
        W: Future<Output = ()>,
        A: FnOnce(WorkspaceRequestToken) -> AF,
        AF: Future<Output = MultiLiveAddSessionResult>,
    {
        let h = host.as_ref();
        let started = {
            let mut s = self.lock();
            s.presenting_multi_channel_id = Some(channel_id.to_string());
            s.presenting_multi_channel_name = channel_name.map(str::to_string);
            s.is_awaiting_multi_entry_motion = true;
            let started = s.begin_multi_navigation_request(h, |s| {
                s.apply_hub_mode_preset(FollowingHubMode::Multi, h, false, true)
            });
            s.presenting_multi_request = Some(started.request);
            started
        };
        let request = started.request;
        let superseded = |s: &mut FollowingState| {
            if !s.is_current_workspace_request(&request)
                && s.presenting_multi_request == Some(request)
            {
                s.time_out_multi_presentation();
            }
            s.abandon_multi_navigation_origin(&request);
            WatchNavigationOutcome::Superseded
        };
        wait_for_entry_motion.await;
        {
            let mut s = self.lock();
            if !s.is_current_workspace_request(&request) {
                return superseded(&mut s);
            }
            s.is_awaiting_multi_entry_motion = false;
        }
        h.restore_video_surfaces_for_menu_enter();
        let result = add_session(request).await;
        let mut s = self.lock();
        if !s.is_current_workspace_request(&request) {
            return superseded(&mut s);
        }
        s.settle_multi_navigation_origin(&request);
        if !result.is_success() {
            s.time_out_multi_presentation();
            s.roll_back_multi_navigation(&started, h);
            return WatchNavigationOutcome::Failed(result);
        }
        s.activate_added_live_channel(channel_id, h, result.is_new_session());
        if result.is_new_session() {
            h.activate_new_watch_audio(channel_id);
        }
        s.finish_multi_presentation_if_ready(h);
        WatchNavigationOutcome::Committed
    }
}

/// 직렬화 스냅샷.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowingSnapshot {
    pub hub_mode: FollowingHubMode,
    pub sort_order: FollowingSortOrder,
    pub filter_live_only: bool,
    pub selected_category: Option<String>,
    pub chat_notice: Option<String>,
    pub is_preparing_watch: bool,
    pub active_watch_preparation_channel_id: Option<String>,
    pub presenting_watch_channel_id: Option<String>,
    pub presenting_watch_channel_name: Option<String>,
    pub presenting_multi_channel_id: Option<String>,
    pub presenting_multi_channel_name: Option<String>,
    pub is_awaiting_multi_entry_motion: bool,
    pub chat_dock_focus: ChatDockFocus,
    pub browser_filter: FollowingBrowserFilter,
    pub browser_sort_order: FollowingBrowserSortOrder,
    pub live_page_index: usize,
    pub offline_page_index: usize,
    pub show_multi_live: bool,
    pub show_multi_chat: bool,
    pub ml_add_error: Option<String>,
    pub chat_dock_collapsed: bool,
    pub stage_tool_popover: StageToolPopover,
    pub auto_sync_chat_on_multi_live_add: bool,
    pub smart_queue_channel_ids: Vec<String>,
    pub recent_channel_ids: Vec<String>,
    pub is_multi_live_pip_mode: bool,
    pub is_channel_dock_compact: bool,
    pub is_channel_dock_hidden: bool,
    pub show_chat_settings: bool,
    pub chat_dock_revealed_in_full_stage: bool,
    pub overview_browser_interaction: FollowingBrowserInteractionState,
}

impl FollowingSnapshot {
    pub fn from_state(s: &FollowingState) -> Self {
        Self {
            hub_mode: s.hub_mode,
            sort_order: s.sort_order,
            filter_live_only: s.filter_live_only,
            selected_category: s.selected_category.clone(),
            chat_notice: s.chat_notice.clone(),
            is_preparing_watch: s.is_preparing_watch(),
            active_watch_preparation_channel_id: s
                .active_watch_preparation_channel_id()
                .map(str::to_string),
            presenting_watch_channel_id: s.presenting_watch_channel_id.clone(),
            presenting_watch_channel_name: s.presenting_watch_channel_name.clone(),
            presenting_multi_channel_id: s.presenting_multi_channel_id.clone(),
            presenting_multi_channel_name: s.presenting_multi_channel_name.clone(),
            is_awaiting_multi_entry_motion: s.is_awaiting_multi_entry_motion,
            chat_dock_focus: s.chat_dock_focus,
            browser_filter: s.browser_filter,
            browser_sort_order: s.browser_sort_order,
            live_page_index: s.live_page_index,
            offline_page_index: s.offline_page_index,
            show_multi_live: s.show_multi_live,
            show_multi_chat: s.show_multi_chat,
            ml_add_error: s.ml_add_error.clone(),
            chat_dock_collapsed: s.chat_dock_collapsed,
            stage_tool_popover: s.stage_tool_popover,
            auto_sync_chat_on_multi_live_add: s.auto_sync_chat_on_multi_live_add,
            smart_queue_channel_ids: s.smart_queue_channel_ids.clone(),
            recent_channel_ids: s.recent_channel_ids.clone(),
            is_multi_live_pip_mode: s.is_multi_live_pip_mode,
            is_channel_dock_compact: s.is_channel_dock_compact,
            is_channel_dock_hidden: s.is_channel_dock_hidden,
            show_chat_settings: s.show_chat_settings,
            chat_dock_revealed_in_full_stage: s.chat_dock_revealed_in_full_stage,
            overview_browser_interaction: s.overview_browser_interaction.clone(),
        }
    }
}
