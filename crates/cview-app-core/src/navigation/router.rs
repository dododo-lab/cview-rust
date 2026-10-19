//! `Sources/CViewApp/Navigation/AppRouter.swift` + `AppRouter+SettingsGroup.swift` 포팅.
//!
//! SwiftUI `NavigationPath` 는 [`AppRoute`] 스택(`Vec`)으로, `UserDefaults.standard` 는 주입된
//! [`KeyValueStore`] 로 바꿨다. 성능 계측(`NavigationTransitionMetrics`·`MenuPerfTrace`)은
//! 옮기지 않았다 — 계측 훅 자리만 주석으로 남긴다. 색상(`accent`)은 SwiftUI 시스템 색 이름
//! 문자열로 돌려준다.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::hub_mode::FollowingHubMode;
use crate::services::menu_transition_gate::MenuTransitionGate;
use crate::services::support::{trim_spaces, KeyValueStore};

/// 마지막 사이드바 항목 저장 키.
pub const LAST_SIDEBAR_ITEM_KEY: &str = "AppRouter.lastSidebarItem";
const SIDEBAR_VISIBILITY_KEY: &str = "router.sidebarVisibility";
const LEGACY_BROWSE_HUB_KEY: &str = "AppRouter.browseHubMode";

// MARK: - App Route

/// 타입 안전 내비게이션 목적지.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AppRoute {
    Home,
    Live { channel_id: String },
    Search { query: Option<String> },
    Following,
    Settings,
    ChannelDetail { channel_id: String },
    ChatOnly { channel_id: String },
    Vod { video_no: i64 },
    Clip { clip_uid: String },
    PopularClips,
    MultiLive,
}

impl AppRoute {
    pub fn id(&self) -> String {
        match self {
            Self::Home => "home".into(),
            Self::Live { channel_id } => format!("live-{channel_id}"),
            Self::Search { query } => format!("search-{}", query.as_deref().unwrap_or("")),
            Self::Following => "following".into(),
            Self::Settings => "settings".into(),
            Self::ChannelDetail { channel_id } => format!("channel-{channel_id}"),
            Self::ChatOnly { channel_id } => format!("chat-{channel_id}"),
            Self::Vod { video_no } => format!("vod-{video_no}"),
            Self::Clip { clip_uid } => format!("clip-{clip_uid}"),
            Self::PopularClips => "popularClips".into(),
            Self::MultiLive => "multiLive".into(),
        }
    }
}

// MARK: - Sidebar Item

/// 사이드바 항목. rawValue(한글)는 영속 키다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SidebarItem {
    #[serde(rename = "홈")]
    Home,
    #[serde(rename = "라이브")]
    Following,
    #[serde(rename = "멀티채팅")]
    MultiChat,
    #[serde(rename = "검색")]
    Search,
    #[serde(rename = "둘러보기")]
    Browse,
    #[serde(rename = "카테고리")]
    Category,
    #[serde(rename = "전체방송")]
    AllBroadcasts,
    #[serde(rename = "라이브러리")]
    Library,
    #[serde(rename = "클립")]
    Clips,
    #[serde(rename = "VOD")]
    Vod,
    #[serde(rename = "다운로드")]
    Downloads,
    #[serde(rename = "인사이트")]
    Insights,
    #[serde(rename = "설정")]
    Settings,
}

impl SidebarItem {
    pub const ALL: [Self; 13] = [
        Self::Home,
        Self::Following,
        Self::MultiChat,
        Self::Search,
        Self::Browse,
        Self::Category,
        Self::AllBroadcasts,
        Self::Library,
        Self::Clips,
        Self::Vod,
        Self::Downloads,
        Self::Insights,
        Self::Settings,
    ];

    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Home => "홈",
            Self::Following => "라이브",
            Self::MultiChat => "멀티채팅",
            Self::Search => "검색",
            Self::Browse => "둘러보기",
            Self::Category => "카테고리",
            Self::AllBroadcasts => "전체방송",
            Self::Library => "라이브러리",
            Self::Clips => "클립",
            Self::Vod => "VOD",
            Self::Downloads => "다운로드",
            Self::Insights => "인사이트",
            Self::Settings => "설정",
        }
    }

    pub fn from_raw(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|i| i.raw_value() == raw)
    }

    /// 홈·라이브·검색·인기·카테고리는 전환 시 뷰 트리를 상주 유지한다.
    pub fn keeps_resident_detail(self) -> bool {
        matches!(
            self,
            Self::Home | Self::Following | Self::Search | Self::AllBroadcasts | Self::Category
        )
    }

    /// 화면에 쓰는 이름.
    pub fn title(self) -> &'static str {
        match self {
            Self::Following => "팔로잉",
            Self::AllBroadcasts => "인기",
            other => other.raw_value(),
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Home => "sparkles.rectangle.stack.fill",
            Self::Following => "play.tv.fill",
            Self::MultiChat => "bubble.left.and.bubble.right.fill",
            Self::Search => "magnifyingglass",
            Self::Browse => "safari.fill",
            Self::Category => "rectangle.3.group.fill",
            Self::AllBroadcasts => "dot.radiowaves.left.and.right",
            Self::Library => "play.square.stack.fill",
            Self::Clips => "movieclapper.fill",
            Self::Vod => "rectangle.stack.badge.play.fill",
            Self::Downloads => "arrow.down.circle.fill",
            Self::Insights => "chart.bar.xaxis.ascending",
            Self::Settings => "slider.horizontal.3",
        }
    }

    /// 사이드바 항목별 액센트(SwiftUI 시스템 색 이름).
    pub fn accent(self) -> &'static str {
        match self {
            Self::Home => "blue",
            Self::Following => "pink",
            Self::MultiChat => "green",
            Self::Search => "mint",
            Self::Browse | Self::Category => "orange",
            Self::AllBroadcasts => "red",
            Self::Library | Self::Clips => "purple",
            Self::Vod => "cyan",
            Self::Downloads => "teal",
            Self::Insights => "yellow",
            Self::Settings => "gray",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Self::Home => "추천 라이브 · 이어보기 · 멀티라이브",
            Self::Following => "팔로잉 라이브 개요 · 추천 큐 · 활동",
            Self::MultiChat => "영상 없이 여러 채널 채팅만 · 그리드 · 멘션 · 검색",
            Self::Search => "채널 · 라이브 · 다시보기 · 클립 통합 검색",
            Self::Browse => "인기 라이브 · 게임/주제별 카테고리",
            Self::Category => "게임/주제별 라이브 카테고리 탐색",
            Self::AllBroadcasts => "치지직 전체 라이브 방송 · 인기순 · 30개씩",
            Self::Library => "클립 · 다시보기 · 받은 영상",
            Self::Clips => "이번 주 인기 클립과 최근 화제 영상",
            Self::Vod => "팔로우한 채널 다시보기 · 이어보기 · 검색",
            Self::Downloads => "받은 영상 · 진행 중 · 완료 · 다시 받기",
            Self::Insights => "내 시청 시간 · Top 채널/카테고리 · 시간대 분석",
            Self::Settings => "외형·시청·채팅·멀티·고급 환경설정",
        }
    }

    /// 사이드바에 남는 부모 메뉴. 레거시 항목은 허브 탭으로 흡수된다.
    pub fn canonical_sidebar_item(self) -> Self {
        match self {
            Self::Browse => Self::AllBroadcasts,
            Self::Clips | Self::Vod | Self::Downloads => Self::Library,
            Self::Insights => Self::Home,
            other => other,
        }
    }
}

// MARK: - Hub Modes

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum HomeHubMode {
    #[default]
    #[serde(rename = "추천")]
    Feed,
    #[serde(rename = "인사이트")]
    Insights,
}

impl HomeHubMode {
    pub const DEFAULTS_KEY: &'static str = "AppRouter.homeHubMode";

    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Feed => "추천",
            Self::Insights => "인사이트",
        }
    }

    pub fn from_raw(raw: &str) -> Option<Self> {
        [Self::Feed, Self::Insights]
            .into_iter()
            .find(|m| m.raw_value() == raw)
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Feed => "sparkles.rectangle.stack",
            Self::Insights => "chart.bar.xaxis.ascending",
        }
    }

    pub fn load(defaults: &dyn KeyValueStore) -> Self {
        defaults
            .string(Self::DEFAULTS_KEY)
            .and_then(|r| Self::from_raw(&r))
            .unwrap_or(Self::Feed)
    }

    pub fn save(self, defaults: &dyn KeyValueStore) {
        defaults.set_string(Self::DEFAULTS_KEY, self.raw_value());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum LibraryHubMode {
    #[default]
    #[serde(rename = "클립")]
    Clips,
    #[serde(rename = "VOD")]
    Vod,
    #[serde(rename = "다운로드")]
    Downloads,
}

impl LibraryHubMode {
    pub const DEFAULTS_KEY: &'static str = "AppRouter.libraryHubMode";

    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Clips => "클립",
            Self::Vod => "VOD",
            Self::Downloads => "다운로드",
        }
    }

    pub fn id(self) -> &'static str {
        self.raw_value()
    }

    pub fn from_raw(raw: &str) -> Option<Self> {
        [Self::Clips, Self::Vod, Self::Downloads]
            .into_iter()
            .find(|m| m.raw_value() == raw)
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Clips => "movieclapper.fill",
            Self::Vod => "rectangle.stack.badge.play.fill",
            Self::Downloads => "arrow.down.circle.fill",
        }
    }

    pub fn load(defaults: &dyn KeyValueStore) -> Self {
        defaults
            .string(Self::DEFAULTS_KEY)
            .and_then(|r| Self::from_raw(&r))
            .unwrap_or(Self::Clips)
    }

    pub fn save(self, defaults: &dyn KeyValueStore) {
        defaults.set_string(Self::DEFAULTS_KEY, self.raw_value());
    }
}

// MARK: - Settings Group

/// 작업 중심으로 재구성된 6개 설정 그룹.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum SettingsGroup {
    #[default]
    #[serde(rename = "개요")]
    Overview,
    #[serde(rename = "시청")]
    Viewing,
    #[serde(rename = "채팅")]
    Chat,
    #[serde(rename = "멀티")]
    Multi,
    #[serde(rename = "연동")]
    Integration,
    #[serde(rename = "고급")]
    Advanced,
}

impl SettingsGroup {
    pub const ALL: [Self; 6] = [
        Self::Overview,
        Self::Viewing,
        Self::Chat,
        Self::Multi,
        Self::Integration,
        Self::Advanced,
    ];

    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Overview => "개요",
            Self::Viewing => "시청",
            Self::Chat => "채팅",
            Self::Multi => "멀티",
            Self::Integration => "연동",
            Self::Advanced => "고급",
        }
    }

    /// 저장된 그룹 식별자(연동)는 유지하고 화면 이름만 네트워크로 표시한다.
    pub fn display_name(self) -> &'static str {
        if self == Self::Integration {
            "네트워크"
        } else {
            self.raw_value()
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Overview => "square.grid.2x2",
            Self::Viewing => "play.rectangle.fill",
            Self::Chat => "bubble.left.and.bubble.right.fill",
            Self::Multi => "rectangle.split.2x2.fill",
            Self::Integration => "network",
            Self::Advanced => "wrench.and.screwdriver.fill",
        }
    }

    pub fn color(self) -> &'static str {
        match self {
            Self::Overview => "gray",
            Self::Viewing => "green",
            Self::Chat => "purple",
            Self::Multi => "blue",
            Self::Integration => "cyan",
            Self::Advanced => "orange",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Self::Overview => "검색·자주 쓰는 설정·현재 상태",
            Self::Viewing => "재생·화질·레이턴시·스크린샷",
            Self::Chat => "패널·표시·TTS·필터",
            Self::Multi => "세션·레이아웃·대역폭",
            Self::Integration => "연결·API·작업 절약",
            Self::Advanced => "일반·단축키·업데이트·성능",
        }
    }
}

// MARK: - Sheet / Visibility

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SheetRoute {
    Login,
    ChatSettings,
}

impl SheetRoute {
    pub fn id(self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::ChatSettings => "chatSettings",
        }
    }
}

/// `NavigationSplitViewVisibility` 대응.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SidebarVisibility {
    #[default]
    All,
    DetailOnly,
    DoubleColumn,
    Automatic,
}

impl SidebarVisibility {
    fn raw(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::DetailOnly => "detailOnly",
            Self::DoubleColumn => "doubleColumn",
            Self::Automatic => "automatic",
        }
    }

    fn load(defaults: &dyn KeyValueStore) -> Self {
        match defaults.string(SIDEBAR_VISIBILITY_KEY).as_deref() {
            Some("detailOnly") => Self::DetailOnly,
            Some("doubleColumn") => Self::DoubleColumn,
            Some("automatic") => Self::Automatic,
            _ => Self::All,
        }
    }
}

/// 사이드바 선택이 바뀌기 직전 훅 (from → to).
pub type WillChangeSidebar = Box<dyn FnMut(SidebarItem, SidebarItem) + Send + Sync>;

// MARK: - App Router

/// 앱 내비게이션 상태.
pub struct AppRouter {
    defaults: Arc<dyn KeyValueStore>,
    gate: Arc<MenuTransitionGate>,
    path: Vec<AppRoute>,
    pub selected_sidebar_item: SidebarItem,
    pub is_in_settings_mode: bool,
    pub selected_settings_group: SettingsGroup,
    previous_sidebar_item: SidebarItem,
    /// 사이드바 선택이 바뀌기 *직전* (from → to).
    pub will_change_sidebar: Option<WillChangeSidebar>,
    has_explicit_navigation: bool,
    pub presented_sheet: Option<SheetRoute>,
    pub pending_search_query: Option<String>,
    pub pending_watch_channel_id: Option<String>,
    pub pending_watch_channel_name: Option<String>,
    pending_watch_request_generation: u64,
    /// 시청·멀티 요청을 보낸 탐색 화면(검색·인기·카테고리).
    pub pending_workspace_origin: Option<SidebarItem>,
    pub pending_multi_channel_id: Option<String>,
    pub pending_multi_channel_name: Option<String>,
    pending_multi_request_generation: u64,
    pub pending_category_name: Option<String>,
    pub pending_watch_video_no: Option<i64>,
    pub pending_hub_mode: Option<FollowingHubMode>,
    pub pending_logout_confirmation: bool,
    pub home_hub_mode: HomeHubMode,
    pub library_hub_mode: LibraryHubMode,
    pub sidebar_visibility: SidebarVisibility,
}

impl std::fmt::Debug for AppRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppRouter")
            .field("path", &self.path)
            .field("selected_sidebar_item", &self.selected_sidebar_item)
            .field("is_in_settings_mode", &self.is_in_settings_mode)
            .finish_non_exhaustive()
    }
}

impl AppRouter {
    pub fn new(defaults: Arc<dyn KeyValueStore>) -> Self {
        Self::with_gate(defaults, MenuTransitionGate::global())
    }

    pub fn with_gate(defaults: Arc<dyn KeyValueStore>, gate: Arc<MenuTransitionGate>) -> Self {
        let library_hub_mode = LibraryHubMode::load(defaults.as_ref());
        let sidebar_visibility = SidebarVisibility::load(defaults.as_ref());
        let me = Self {
            defaults,
            gate,
            path: Vec::new(),
            selected_sidebar_item: SidebarItem::Home,
            is_in_settings_mode: false,
            selected_settings_group: SettingsGroup::Overview,
            previous_sidebar_item: SidebarItem::Home,
            will_change_sidebar: None,
            has_explicit_navigation: false,
            presented_sheet: None,
            pending_search_query: None,
            pending_watch_channel_id: None,
            pending_watch_channel_name: None,
            pending_watch_request_generation: 0,
            pending_workspace_origin: None,
            pending_multi_channel_id: None,
            pending_multi_channel_name: None,
            pending_multi_request_generation: 0,
            pending_category_name: None,
            pending_watch_video_no: None,
            pending_hub_mode: None,
            pending_logout_confirmation: false,
            // 일반 실행은 항상 홈 피드로 시작한다.
            home_hub_mode: HomeHubMode::Feed,
            library_hub_mode,
            sidebar_visibility,
        };
        if let Some(raw) = me.defaults.string(LAST_SIDEBAR_ITEM_KEY) {
            me.migrate_browse_hub_if_needed(&raw);
        }
        me
    }

    pub fn gate(&self) -> &Arc<MenuTransitionGate> {
        &self.gate
    }

    pub fn defaults(&self) -> &Arc<dyn KeyValueStore> {
        &self.defaults
    }

    // MARK: Path

    pub fn path(&self) -> &[AppRoute] {
        &self.path
    }

    /// 경로 대입 — 바뀔 때만 콘텐츠 전환 게이트를 연다.
    pub fn set_path(&mut self, path: Vec<AppRoute>) {
        if self.path == path {
            return;
        }
        self.gate.notify_content_change();
        self.path = path;
    }

    fn push(&mut self, route: AppRoute) {
        let mut next = self.path.clone();
        next.push(route);
        self.set_path(next);
    }

    pub fn has_explicit_navigation(&self) -> bool {
        self.has_explicit_navigation
    }

    pub fn pending_watch_request_generation(&self) -> u64 {
        self.pending_watch_request_generation
    }

    pub fn pending_multi_request_generation(&self) -> u64 {
        self.pending_multi_request_generation
    }

    // MARK: Hub modes

    pub fn set_home_hub_mode(&mut self, mode: HomeHubMode) {
        if self.home_hub_mode == mode {
            return;
        }
        self.gate.notify_content_change();
        self.home_hub_mode = mode;
        mode.save(self.defaults.as_ref());
    }

    pub fn set_library_hub_mode(&mut self, mode: LibraryHubMode) {
        if self.library_hub_mode == mode {
            return;
        }
        self.gate.notify_content_change();
        self.library_hub_mode = mode;
        mode.save(self.defaults.as_ref());
    }

    // MARK: Sidebar visibility

    pub fn set_sidebar_visibility(&mut self, value: SidebarVisibility) {
        if self.sidebar_visibility == value {
            return;
        }
        self.sidebar_visibility = value;
        self.defaults
            .set_string(SIDEBAR_VISIBILITY_KEY, value.raw());
    }

    pub fn toggle_sidebar(&mut self) {
        let next = if self.sidebar_visibility == SidebarVisibility::All {
            SidebarVisibility::DetailOnly
        } else {
            SidebarVisibility::All
        };
        self.set_sidebar_visibility(next);
    }

    /// 1회성 마이그레이션 — 폐기된 둘러보기 허브의 저장값을 정리한다(탐색을 일으키지 않는다).
    fn migrate_browse_hub_if_needed(&self, last_sidebar_raw: &str) {
        if last_sidebar_raw != SidebarItem::Browse.raw_value() {
            return;
        }
        let legacy_raw = self
            .defaults
            .string(LEGACY_BROWSE_HUB_KEY)
            .unwrap_or_else(|| FollowingHubMode::Popular.raw_value().to_string());
        let destination =
            if FollowingHubMode::from_raw(&legacy_raw) == Some(FollowingHubMode::Category) {
                SidebarItem::Category
            } else {
                SidebarItem::AllBroadcasts
            };
        self.defaults.remove(LEGACY_BROWSE_HUB_KEY);
        self.defaults
            .set_string(LAST_SIDEBAR_ITEM_KEY, destination.raw_value());
    }

    // MARK: Navigation actions

    pub fn navigate(&mut self, route: AppRoute) {
        match route {
            AppRoute::Following => self.select_sidebar(SidebarItem::Following),
            AppRoute::Home => self.select_sidebar(SidebarItem::Home),
            AppRoute::Search { query } => {
                let trimmed = query
                    .as_deref()
                    .map(|q| trim_spaces(q).to_string())
                    .unwrap_or_default();
                self.pending_search_query = Some(trimmed);
                self.select_sidebar(SidebarItem::Search);
            }
            AppRoute::Live { channel_id } => self.navigate_to_watch(&channel_id, None),
            AppRoute::PopularClips => self.select_sidebar(SidebarItem::Clips),
            AppRoute::MultiLive => self.navigate_to_multi_live(),
            AppRoute::Settings => self.select_sidebar(SidebarItem::Settings),
            AppRoute::Vod { video_no } => self.navigate_to_vod(video_no),
            other => {
                self.has_explicit_navigation = true;
                self.push(other);
            }
        }
    }

    pub fn navigate_back(&mut self) {
        if self.path.is_empty() {
            return;
        }
        let mut next = self.path.clone();
        next.pop();
        self.set_path(next);
    }

    /// 검색·인기·카테고리의 채널을 시청 탭에 먼저 표시한 뒤 연결한다.
    pub fn navigate_to_watch(&mut self, channel_id: &str, channel_name: Option<&str>) {
        let trimmed = channel_id.trim();
        if trimmed.is_empty() {
            return;
        }
        self.pending_hub_mode = None;
        self.pending_multi_channel_id = None;
        self.pending_multi_channel_name = None;
        self.pending_multi_request_generation =
            self.pending_multi_request_generation.wrapping_add(1);
        self.pending_watch_channel_id = Some(trimmed.to_string());
        self.pending_watch_channel_name = channel_name.map(str::to_string);
        self.pending_watch_request_generation =
            self.pending_watch_request_generation.wrapping_add(1);
        self.pending_workspace_origin =
            Self::workspace_return_route(self.selected_sidebar_item, self.is_in_settings_mode);
        self.select_sidebar(SidebarItem::Following);
    }

    /// 특정 카테고리를 들고 카테고리 메뉴로 진입.
    pub fn navigate_to_category(&mut self, name: &str) {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return;
        }
        self.pending_category_name = Some(trimmed.to_string());
        self.select_sidebar(SidebarItem::Category);
    }

    /// VOD 사이드바로 이동한 뒤 해당 영상을 바로 재생한다.
    pub fn navigate_to_vod(&mut self, video_no: i64) {
        self.pending_watch_video_no = Some(video_no);
        self.select_sidebar(SidebarItem::Vod);
    }

    /// 멀티라이브 모드로 진입.
    pub fn navigate_to_multi_live(&mut self) {
        self.pending_watch_channel_id = None;
        self.pending_watch_channel_name = None;
        self.pending_watch_request_generation =
            self.pending_watch_request_generation.wrapping_add(1);
        self.pending_multi_channel_id = None;
        self.pending_multi_channel_name = None;
        self.pending_multi_request_generation =
            self.pending_multi_request_generation.wrapping_add(1);
        self.pending_workspace_origin = None;
        self.pending_hub_mode = Some(FollowingHubMode::Multi);
        self.select_sidebar(SidebarItem::Following);
    }

    /// 검색 워크스페이스의 멀티 추가 — 목적지 탭 표시가 먼저다.
    pub fn navigate_to_multi_live_channel(&mut self, channel_id: &str, channel_name: Option<&str>) {
        let trimmed = channel_id.trim();
        if trimmed.is_empty() {
            return;
        }
        self.pending_hub_mode = None;
        self.pending_watch_channel_id = None;
        self.pending_watch_channel_name = None;
        self.pending_watch_request_generation =
            self.pending_watch_request_generation.wrapping_add(1);
        self.pending_multi_channel_id = Some(trimmed.to_string());
        self.pending_multi_channel_name = channel_name.map(str::to_string);
        self.pending_multi_request_generation =
            self.pending_multi_request_generation.wrapping_add(1);
        self.pending_workspace_origin =
            Self::workspace_return_route(self.selected_sidebar_item, self.is_in_settings_mode);
        self.select_sidebar(SidebarItem::Following);
    }

    fn fire_will_change(&mut self, from: SidebarItem, to: SidebarItem) {
        if let Some(hook) = self.will_change_sidebar.as_mut() {
            hook(from, to);
        }
    }

    pub fn select_sidebar(&mut self, item: SidebarItem) {
        self.has_explicit_navigation = true;
        self.apply_destination_semantics(item);
        let canonical = item.canonical_sidebar_item();
        if canonical == SidebarItem::Settings {
            self.enter_settings();
            return;
        }
        let from = self.selected_sidebar_item;
        let will_change = from != canonical || self.is_in_settings_mode;
        if will_change {
            tracing::info!(
                "Navigation: {} → {}",
                from.raw_value(),
                canonical.raw_value()
            );
            // 게이트를 상태 변경 *전에* 켠다.
            self.gate.notify_menu_change();
            self.fire_will_change(from, canonical);
        }
        self.is_in_settings_mode = false;
        self.selected_sidebar_item = canonical;
        if will_change
            || self.defaults.string(LAST_SIDEBAR_ITEM_KEY).as_deref() != Some(canonical.raw_value())
        {
            self.defaults
                .set_string(LAST_SIDEBAR_ITEM_KEY, canonical.raw_value());
        }
        self.pop_to_root();
    }

    /// 쌓인 상세를 닫고 현재 메뉴의 루트로 돌아간다.
    pub fn pop_to_root(&mut self) {
        if !self.path.is_empty() {
            self.set_path(Vec::new());
        }
    }

    /// 레거시 사이드바 별칭이 가리키는 허브 탭을 맞춘다.
    pub fn apply_destination_semantics(&mut self, item: SidebarItem) {
        match item {
            SidebarItem::Clips => self.set_library_hub_mode(LibraryHubMode::Clips),
            SidebarItem::Vod => self.set_library_hub_mode(LibraryHubMode::Vod),
            SidebarItem::Downloads => self.set_library_hub_mode(LibraryHubMode::Downloads),
            SidebarItem::Insights => self.set_home_hub_mode(HomeHubMode::Insights),
            _ => {}
        }
    }

    pub fn enter_settings(&mut self) {
        self.has_explicit_navigation = true;
        let from = self.selected_sidebar_item;
        let will_change = from != SidebarItem::Settings || !self.is_in_settings_mode;
        if will_change {
            self.gate.notify_menu_change();
            self.fire_will_change(from, SidebarItem::Settings);
        }
        if self.selected_sidebar_item != SidebarItem::Settings {
            self.previous_sidebar_item = self.selected_sidebar_item;
            self.selected_sidebar_item = SidebarItem::Settings;
        }
        if will_change {
            self.defaults
                .set_string(LAST_SIDEBAR_ITEM_KEY, SidebarItem::Settings.raw_value());
        }
        self.is_in_settings_mode = true;
        if !self.path.is_empty() {
            self.set_path(Vec::new());
        }
    }

    pub fn exit_settings(&mut self) {
        if !self.is_in_settings_mode && self.selected_sidebar_item != SidebarItem::Settings {
            return;
        }
        let from = self.selected_sidebar_item;
        let to = self.previous_sidebar_item.canonical_sidebar_item();
        if self.is_in_settings_mode || from != to {
            self.gate.notify_menu_change();
            self.fire_will_change(from, to);
        }
        self.is_in_settings_mode = false;
        self.selected_sidebar_item = to;
        self.defaults
            .set_string(LAST_SIDEBAR_ITEM_KEY, to.raw_value());
        if !self.path.is_empty() {
            self.set_path(Vec::new());
        }
    }

    pub fn select_settings_group(&mut self, group: SettingsGroup) {
        if self.selected_settings_group != group {
            self.gate.notify_content_change();
            self.selected_settings_group = group;
        }
        if !self.is_in_settings_mode {
            self.enter_settings();
        }
    }

    pub fn present_sheet(&mut self, sheet: SheetRoute) {
        self.presented_sheet = Some(sheet);
    }

    // MARK: 시청·멀티 실패 복귀

    /// 실패 시 되돌아갈 수 있는 출발 화면인가 — 상주 탐색 화면(검색·인기·카테고리)만.
    pub fn workspace_return_route(
        item: SidebarItem,
        is_in_settings_mode: bool,
    ) -> Option<SidebarItem> {
        if is_in_settings_mode {
            return None;
        }
        match item {
            SidebarItem::Search | SidebarItem::AllBroadcasts | SidebarItem::Category => Some(item),
            _ => None,
        }
    }

    /// 실패한 시청·멀티 요청을 출발 화면으로 되돌린다. 되돌렸으면 true.
    pub fn return_to_workspace_origin(&mut self, origin: Option<SidebarItem>) -> bool {
        let Some(origin) = origin else { return false };
        if self.selected_sidebar_item != SidebarItem::Following || self.is_in_settings_mode {
            return false;
        }
        self.select_sidebar(origin);
        true
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `AppRouterMenuTransitionTests.swift`, `WorkspaceOriginReturnTests.swift`,
    //! `SidebarMenuConsolidationTests.swift`, `BrowseIntoLiveMergeTests.swift` 중 라우터 계약.
    use super::*;
    use crate::services::support::MemoryKeyValueStore;
    use std::sync::Mutex;

    fn router() -> (AppRouter, Arc<MemoryKeyValueStore>) {
        let defaults = Arc::new(MemoryKeyValueStore::new());
        let gate = Arc::new(MenuTransitionGate::default());
        (AppRouter::with_gate(defaults.clone(), gate), defaults)
    }

    #[test]
    fn launch_starts_home_even_with_saved_sidebar() {
        let defaults = Arc::new(MemoryKeyValueStore::new());
        defaults.set_string(LAST_SIDEBAR_ITEM_KEY, "라이브");
        let r = AppRouter::with_gate(defaults, Arc::new(MenuTransitionGate::default()));
        assert_eq!(r.selected_sidebar_item, SidebarItem::Home);
        assert!(!r.has_explicit_navigation());
    }

    #[test]
    fn browse_migration_rewrites_last_sidebar() {
        let defaults = Arc::new(MemoryKeyValueStore::new());
        defaults.set_string(LAST_SIDEBAR_ITEM_KEY, "둘러보기");
        defaults.set_string(LEGACY_BROWSE_HUB_KEY, "카테고리");
        let _r = AppRouter::with_gate(defaults.clone(), Arc::new(MenuTransitionGate::default()));
        assert_eq!(
            defaults.string(LAST_SIDEBAR_ITEM_KEY).as_deref(),
            Some("카테고리")
        );
        assert!(defaults.string(LEGACY_BROWSE_HUB_KEY).is_none());

        let defaults = Arc::new(MemoryKeyValueStore::new());
        defaults.set_string(LAST_SIDEBAR_ITEM_KEY, "둘러보기");
        let _r = AppRouter::with_gate(defaults.clone(), Arc::new(MenuTransitionGate::default()));
        assert_eq!(
            defaults.string(LAST_SIDEBAR_ITEM_KEY).as_deref(),
            Some("전체방송")
        );
    }

    #[test]
    fn legacy_aliases_select_canonical_and_hub_tab() {
        let (mut r, _) = router();
        r.select_sidebar(SidebarItem::Vod);
        assert_eq!(r.selected_sidebar_item, SidebarItem::Library);
        assert_eq!(r.library_hub_mode, LibraryHubMode::Vod);
        r.select_sidebar(SidebarItem::Insights);
        assert_eq!(r.selected_sidebar_item, SidebarItem::Home);
        assert_eq!(r.home_hub_mode, HomeHubMode::Insights);
        r.select_sidebar(SidebarItem::Browse);
        assert_eq!(r.selected_sidebar_item, SidebarItem::AllBroadcasts);
    }

    #[test]
    fn search_route_sets_trimmed_pending_query() {
        let (mut r, _) = router();
        r.navigate(AppRoute::Search {
            query: Some("  abc ".into()),
        });
        assert_eq!(r.selected_sidebar_item, SidebarItem::Search);
        assert_eq!(r.pending_search_query.as_deref(), Some("abc"));
        r.navigate(AppRoute::Search { query: None });
        assert_eq!(r.pending_search_query.as_deref(), Some(""));
    }

    #[test]
    fn select_sidebar_fires_will_change_once_and_pops() {
        let (mut r, defaults) = router();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        r.will_change_sidebar = Some(Box::new(move |f, t| s.lock().unwrap().push((f, t))));
        r.navigate(AppRoute::ChannelDetail {
            channel_id: "c".into(),
        });
        assert_eq!(r.path().len(), 1);
        r.select_sidebar(SidebarItem::Following);
        r.select_sidebar(SidebarItem::Following);
        assert!(r.path().is_empty());
        assert_eq!(
            *seen.lock().unwrap(),
            vec![(SidebarItem::Home, SidebarItem::Following)]
        );
        assert_eq!(
            defaults.string(LAST_SIDEBAR_ITEM_KEY).as_deref(),
            Some("라이브")
        );
    }

    #[test]
    fn menu_change_bumps_gate_epoch_only_on_change() {
        let (mut r, _) = router();
        let epoch = r.gate().menu_change_epoch();
        r.select_sidebar(SidebarItem::Home);
        assert_eq!(r.gate().menu_change_epoch(), epoch);
        r.select_sidebar(SidebarItem::Following);
        assert_eq!(r.gate().menu_change_epoch(), epoch + 1);
        assert!(r.gate().is_transitioning());
    }

    #[test]
    fn settings_enter_and_exit_restore_previous() {
        let (mut r, _) = router();
        r.select_sidebar(SidebarItem::Category);
        r.select_settings_group(SettingsGroup::Chat);
        assert!(r.is_in_settings_mode);
        assert_eq!(r.selected_sidebar_item, SidebarItem::Settings);
        assert_eq!(r.selected_settings_group, SettingsGroup::Chat);
        r.exit_settings();
        assert!(!r.is_in_settings_mode);
        assert_eq!(r.selected_sidebar_item, SidebarItem::Category);
    }

    #[test]
    fn watch_request_records_workspace_origin() {
        let (mut r, _) = router();
        r.select_sidebar(SidebarItem::Search);
        let gen = r.pending_watch_request_generation();
        r.navigate_to_watch(" abc ", Some("이름"));
        assert_eq!(r.pending_watch_channel_id.as_deref(), Some("abc"));
        assert_eq!(r.pending_workspace_origin, Some(SidebarItem::Search));
        assert_eq!(r.pending_watch_request_generation(), gen + 1);
        assert_eq!(r.selected_sidebar_item, SidebarItem::Following);
        assert!(r.return_to_workspace_origin(r.pending_workspace_origin));
        assert_eq!(r.selected_sidebar_item, SidebarItem::Search);
    }

    #[test]
    fn watch_from_home_has_no_origin_and_blank_is_ignored() {
        let (mut r, _) = router();
        r.navigate_to_watch("   ", None);
        assert_eq!(r.selected_sidebar_item, SidebarItem::Home);
        r.navigate_to_watch("x", None);
        assert_eq!(r.pending_workspace_origin, None);
        assert!(!r.return_to_workspace_origin(None));
    }

    #[test]
    fn origin_not_restored_when_user_left_live() {
        let (mut r, _) = router();
        r.select_sidebar(SidebarItem::Category);
        r.navigate_to_watch("x", None);
        r.select_sidebar(SidebarItem::Home);
        assert!(!r.return_to_workspace_origin(Some(SidebarItem::Category)));
        assert_eq!(r.selected_sidebar_item, SidebarItem::Home);
    }

    #[test]
    fn multi_live_routes_set_pending_hub_mode_and_clear_watch() {
        let (mut r, _) = router();
        r.navigate_to_watch("w", None);
        r.navigate(AppRoute::MultiLive);
        assert_eq!(r.pending_hub_mode, Some(FollowingHubMode::Multi));
        assert!(r.pending_watch_channel_id.is_none());
        r.navigate_to_multi_live_channel("m", None);
        assert_eq!(r.pending_hub_mode, None);
        assert_eq!(r.pending_multi_channel_id.as_deref(), Some("m"));
    }

    #[test]
    fn vod_and_category_routes() {
        let (mut r, _) = router();
        r.navigate(AppRoute::Vod { video_no: 42 });
        assert_eq!(r.pending_watch_video_no, Some(42));
        assert_eq!(r.selected_sidebar_item, SidebarItem::Library);
        assert_eq!(r.library_hub_mode, LibraryHubMode::Vod);
        r.navigate_to_category(" 게임 ");
        assert_eq!(r.pending_category_name.as_deref(), Some("게임"));
        assert_eq!(r.selected_sidebar_item, SidebarItem::Category);
    }

    #[test]
    fn sidebar_visibility_persists() {
        let (mut r, defaults) = router();
        r.toggle_sidebar();
        assert_eq!(r.sidebar_visibility, SidebarVisibility::DetailOnly);
        let r2 = AppRouter::with_gate(defaults, Arc::new(MenuTransitionGate::default()));
        assert_eq!(r2.sidebar_visibility, SidebarVisibility::DetailOnly);
    }

    #[test]
    fn titles_and_resident_flags() {
        assert_eq!(SidebarItem::AllBroadcasts.title(), "인기");
        assert_eq!(SidebarItem::AllBroadcasts.raw_value(), "전체방송");
        assert!(SidebarItem::Category.keeps_resident_detail());
        assert!(!SidebarItem::Library.keeps_resident_detail());
        assert_eq!(SettingsGroup::Integration.display_name(), "네트워크");
    }
}
