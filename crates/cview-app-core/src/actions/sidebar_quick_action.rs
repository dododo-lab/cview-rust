//! `Sources/CViewApp/Actions/SidebarQuickAction.swift` + `SidebarQuickActionExecutor.swift` 포팅.
//!
//! 메뉴별 사이드바 빠른 작업 카탈로그와 실행기. 실행기가 읽고 바꾸던 ViewModel
//! (`MultiLiveManager`·`SearchViewModel`·`AllBroadcastsViewModel`·`CategoryBrowseViewModel`·
//! `FollowingViewState`·독립 멀티채팅 매니저)은 [`QuickActionHost`] 로 좁혔다.

use async_trait::async_trait;

use crate::navigation::hub_mode::{FollowingBrowserFilter, FollowingHubMode};
use crate::navigation::router::{
    AppRouter, HomeHubMode, LibraryHubMode, SettingsGroup, SidebarItem,
};
use crate::navigation::sidebar_destination::SidebarDestination;
use crate::services::channel_preset_store::PresetKind;

/// 검색 범위(`ViewModels/SearchViewModel.swift` 의 `SearchScope`). rawValue 는 영속 식별자다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SearchScope {
    All,
    Channel,
    Live,
    Video,
    Clip,
}

impl SearchScope {
    pub fn raw_value(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Channel => "channel",
            Self::Live => "live",
            Self::Video => "video",
            Self::Clip => "clip",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::All => "전체",
            Self::Channel => "채널",
            Self::Live => "라이브",
            Self::Video => "다시보기",
            Self::Clip => "클립",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::All => "square.grid.2x2",
            Self::Channel => "person.2",
            Self::Live => "play.tv",
            Self::Video => "film",
            Self::Clip => "film.stack",
        }
    }
}

/// 인기(전체방송) 화면 모드.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PopularMode {
    All,
    Search,
}

/// 메뉴별 명령. 카탈로그에는 타이머·구독·데이터 로딩이 없다.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SidebarQuickAction {
    ResumeRecent,
    FollowingFilter(FollowingBrowserFilter),
    Insights,
    AddChannel,
    ToggleChat,
    CopyLiveLink,
    FocusAudio,
    AutomaticGrid,
    SaveLivePreset,
    ReconnectChats,
    SaveChatPreset,
    ChatSettings,
    SearchScope(SearchScope),
    ClearSearch,
    PopularList,
    PopularSearch,
    RefreshPopular,
    CategoryAll,
    CategoryType(String),
    Library(LibraryHubMode),
    ReturnToLive,
    NetworkSettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickActionDomain {
    Navigation,
    Playback,
    StandaloneChat,
    Discovery,
}

impl SidebarQuickAction {
    pub fn domain(&self) -> QuickActionDomain {
        use SidebarQuickAction::*;
        match self {
            ResumeRecent | FollowingFilter(_) | Insights | ChatSettings | Library(_)
            | ReturnToLive | NetworkSettings => QuickActionDomain::Navigation,
            AddChannel | ToggleChat | CopyLiveLink | FocusAudio | AutomaticGrid
            | SaveLivePreset => QuickActionDomain::Playback,
            ReconnectChats | SaveChatPreset => QuickActionDomain::StandaloneChat,
            _ => QuickActionDomain::Discovery,
        }
    }

    pub fn id(&self) -> String {
        use SidebarQuickAction::*;
        match self {
            FollowingFilter(f) => format!("following.{}", f.raw_value()),
            SearchScope(s) => format!("search.{}", s.raw_value()),
            CategoryType(t) => format!("category.{t}"),
            Library(m) => format!("library.{}", m.id()),
            ResumeRecent => "resumeRecent".into(),
            Insights => "insights".into(),
            AddChannel => "addChannel".into(),
            ToggleChat => "toggleChat".into(),
            CopyLiveLink => "copyLiveLink".into(),
            FocusAudio => "focusAudio".into(),
            AutomaticGrid => "automaticGrid".into(),
            SaveLivePreset => "saveLivePreset".into(),
            ReconnectChats => "reconnectChats".into(),
            SaveChatPreset => "saveChatPreset".into(),
            ChatSettings => "chatSettings".into(),
            ClearSearch => "clearSearch".into(),
            PopularList => "popularList".into(),
            PopularSearch => "popularSearch".into(),
            RefreshPopular => "refreshPopular".into(),
            CategoryAll => "categoryAll".into(),
            ReturnToLive => "returnToLive".into(),
            NetworkSettings => "networkSettings".into(),
        }
    }

    pub fn title(&self) -> String {
        use SidebarQuickAction::*;
        match self {
            ResumeRecent => "최근 채널 이어보기".into(),
            FollowingFilter(f) => match f {
                FollowingBrowserFilter::All => "전체 팔로잉",
                FollowingBrowserFilter::Live => "방송 중인 채널",
                FollowingBrowserFilter::Favorites => "즐겨찾기 채널",
                FollowingBrowserFilter::Recent => "최근 시청 채널",
            }
            .into(),
            Insights => "시청 인사이트".into(),
            AddChannel => "채널 추가".into(),
            ToggleChat => "채팅 표시 전환".into(),
            CopyLiveLink => "방송 링크 복사".into(),
            FocusAudio => "선택 채널만 듣기".into(),
            AutomaticGrid => "자동 그리드 배치".into(),
            SaveLivePreset | SaveChatPreset => "채널 묶음 저장…".into(),
            ReconnectChats => "전체 채팅 재연결".into(),
            ChatSettings => "채팅 디자인 설정".into(),
            SearchScope(s) => format!("{} 검색", s.title()),
            ClearSearch => "검색어 지우기".into(),
            PopularList => "인기 라이브 목록".into(),
            PopularSearch => "방송 검색".into(),
            RefreshPopular => "목록 새로고침".into(),
            CategoryAll => "전체 카테고리".into(),
            CategoryType(t) => if t == "GAME" {
                "게임만 둘러보기"
            } else {
                "스포츠만 둘러보기"
            }
            .into(),
            Library(m) => match m {
                LibraryHubMode::Clips => "클립 둘러보기",
                LibraryHubMode::Vod => "다시보기 탐색",
                LibraryHubMode::Downloads => "다운로드 관리",
            }
            .into(),
            ReturnToLive => "라이브로 돌아가기".into(),
            NetworkSettings => "네트워크 설정".into(),
        }
    }

    pub fn icon(&self) -> &'static str {
        use SidebarQuickAction::*;
        match self {
            ResumeRecent | ReturnToLive => "play.rectangle",
            FollowingFilter(f) => f.icon(),
            Insights => "chart.bar.xaxis",
            AddChannel => "plus.rectangle.on.rectangle",
            ToggleChat => "bubble.left.and.bubble.right",
            CopyLiveLink => "link",
            FocusAudio => "speaker.wave.2",
            AutomaticGrid => "square.grid.2x2",
            SaveLivePreset | SaveChatPreset => "square.and.arrow.down",
            ReconnectChats | RefreshPopular => "arrow.clockwise",
            ChatSettings => "text.bubble",
            SearchScope(s) => s.icon(),
            ClearSearch => "xmark.circle",
            PopularList => "flame",
            PopularSearch => "magnifyingglass",
            CategoryAll => "square.grid.3x3",
            CategoryType(t) => {
                if t == "GAME" {
                    "gamecontroller"
                } else {
                    "sportscourt"
                }
            }
            Library(m) => m.icon(),
            NetworkSettings => "network",
        }
    }

    pub fn actions_for(destination: &SidebarDestination) -> Vec<Self> {
        use FollowingHubMode as M;
        use SidebarDestination::{Item, Live};
        use SidebarItem as I;
        use SidebarQuickAction::*;
        match destination {
            Item(I::Home) | Item(I::Insights) => vec![
                ResumeRecent,
                FollowingFilter(FollowingBrowserFilter::Favorites),
                Insights,
            ],
            Live(M::Explore) | Item(I::Following) => vec![
                FollowingFilter(FollowingBrowserFilter::All),
                FollowingFilter(FollowingBrowserFilter::Live),
                FollowingFilter(FollowingBrowserFilter::Favorites),
                FollowingFilter(FollowingBrowserFilter::Recent),
            ],
            Live(M::Watch) => vec![AddChannel, ToggleChat, CopyLiveLink],
            Live(M::Multi) => vec![AddChannel, FocusAudio, AutomaticGrid, SaveLivePreset],
            Item(I::MultiChat) => vec![ReconnectChats, SaveChatPreset, ChatSettings],
            Item(I::Search) | Live(M::Search) => vec![
                SearchScope(self::SearchScope::All),
                SearchScope(self::SearchScope::Live),
                SearchScope(self::SearchScope::Channel),
                ClearSearch,
            ],
            Item(I::AllBroadcasts) | Item(I::Browse) | Live(M::Popular) => {
                vec![PopularList, PopularSearch, RefreshPopular]
            }
            Item(I::Category) | Live(M::Category) => vec![
                CategoryAll,
                CategoryType("GAME".into()),
                CategoryType("SPORTS".into()),
            ],
            Item(I::Library) | Item(I::Clips) | Item(I::Vod) | Item(I::Downloads) => vec![
                Library(LibraryHubMode::Clips),
                Library(LibraryHubMode::Vod),
                Library(LibraryHubMode::Downloads),
            ],
            Item(I::Settings) => vec![ReturnToLive, NetworkSettings, ChatSettings],
        }
    }
}

/// 묶음 저장 시트에 넘길 초안.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarPresetDraft {
    pub id: uuid::Uuid,
    pub kind: PresetKind,
    pub channel_ids: Vec<String>,
}

impl SidebarPresetDraft {
    pub fn new(kind: PresetKind, channel_ids: Vec<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            kind,
            channel_ids,
        }
    }
}

/// 실행기가 앱 상태에 요구하는 읽기·쓰기.
#[async_trait]
pub trait QuickActionHost: Send {
    // 읽기
    fn live_hub_mode(&self) -> FollowingHubMode;
    /// 최근 시청 첫 채널(id, 이름).
    fn first_recent_channel(&self) -> Option<(String, String)>;
    fn has_selected_multi_session(&self) -> bool;
    fn has_multi_sessions(&self) -> bool;
    fn has_standalone_chat_sessions(&self) -> bool;
    fn browser_filter(&self) -> FollowingBrowserFilter;
    /// `showMultiChat && !chatDockCollapsed`.
    fn is_chat_visible(&self) -> bool;
    /// 검색 VM 이 있으면 Some(현재 범위, 검색어 비었는가).
    fn search_state(&self) -> Option<(SearchScope, bool)>;
    /// 인기 VM 이 있으면 Some(모드, 바쁨 여부).
    fn popular_state(&self) -> Option<(PopularMode, bool)>;
    /// 카테고리 VM 이 있으면 Some(유형 필터).
    fn category_type_filter(&self) -> Option<Option<String>>;

    // 쓰기
    fn open_live_section(&mut self, mode: FollowingHubMode, router: &mut AppRouter);
    fn set_browser_filter_and_reset_pages(&mut self, filter: FollowingBrowserFilter);
    fn present_channel_dock(&mut self);
    /// 보이면 숨기고, 아니면 채팅 도크를 연다.
    fn toggle_chat(&mut self);
    /// 선택 세션의 채널 ID.
    fn selected_multi_channel_id(&self) -> Option<String>;
    /// 멀티 오디오 모드를 끄고 선택 세션으로 소리를 보낸다.
    fn focus_audio_on_selected(&mut self);
    /// 그리드 레이아웃을 켜고 프리셋 배치를 적용한다.
    fn apply_automatic_grid(&mut self);
    fn multi_live_channel_ids(&self) -> Vec<String>;
    fn standalone_chat_channel_ids(&self) -> Vec<String>;
    async fn reconnect_standalone_chats(&mut self);
    async fn set_search_scope(&mut self, scope: SearchScope);
    fn clear_search(&mut self);
    fn set_popular_mode(&mut self, mode: PopularMode);
    async fn refresh_popular(&mut self);
    /// 카테고리 그리드로 돌아가 유형 필터를 바꾼다(분할 선택은 보존).
    fn set_category_type(&mut self, type_filter: Option<String>);
    /// 복사 단일 진입점(`ClipboardAction.copy`).
    fn copy_to_clipboard(&mut self, text: &str, subject: &str);
}

/// 명시적 클릭만 일을 한다 — 사이드바 마운트는 아무것도 하지 않는다.
pub struct SidebarQuickActionExecutor;

impl SidebarQuickActionExecutor {
    pub fn is_enabled(action: &SidebarQuickAction, app: &dyn QuickActionHost) -> bool {
        use SidebarQuickAction::*;
        match action.domain() {
            QuickActionDomain::Navigation => {
                *action != ResumeRecent || app.first_recent_channel().is_some()
            }
            QuickActionDomain::Playback => match action {
                CopyLiveLink | FocusAudio => app.has_selected_multi_session(),
                ToggleChat | AutomaticGrid | SaveLivePreset => app.has_multi_sessions(),
                _ => true,
            },
            QuickActionDomain::StandaloneChat => app.has_standalone_chat_sessions(),
            QuickActionDomain::Discovery => match action {
                SearchScope(_) => app.search_state().is_some(),
                ClearSearch => app.search_state().is_some_and(|(_, empty)| !empty),
                PopularList | PopularSearch => app.popular_state().is_some(),
                RefreshPopular => app.popular_state().is_some_and(|(_, busy)| !busy),
                CategoryAll | CategoryType(_) => app.category_type_filter().is_some(),
                _ => false,
            },
        }
    }

    pub fn is_selected(
        action: &SidebarQuickAction,
        app: &dyn QuickActionHost,
        router: &AppRouter,
    ) -> bool {
        use SidebarQuickAction::*;
        match action.domain() {
            QuickActionDomain::Navigation => match action {
                FollowingFilter(filter) => {
                    router.selected_sidebar_item == SidebarItem::Following
                        && app.live_hub_mode() == FollowingHubMode::Explore
                        && app.browser_filter() == *filter
                }
                Insights => {
                    router.selected_sidebar_item == SidebarItem::Home
                        && router.home_hub_mode == HomeHubMode::Insights
                }
                Library(mode) => router.library_hub_mode == *mode,
                NetworkSettings => router.selected_settings_group == SettingsGroup::Integration,
                ChatSettings => {
                    router.is_in_settings_mode
                        && router.selected_settings_group == SettingsGroup::Chat
                }
                _ => false,
            },
            QuickActionDomain::Playback => *action == ToggleChat && app.is_chat_visible(),
            QuickActionDomain::StandaloneChat => false,
            QuickActionDomain::Discovery => match action {
                SearchScope(scope) => app.search_state().is_some_and(|(s, _)| s == *scope),
                PopularList => app
                    .popular_state()
                    .is_some_and(|(m, _)| m == PopularMode::All),
                PopularSearch => app
                    .popular_state()
                    .is_some_and(|(m, _)| m == PopularMode::Search),
                CategoryAll => app.category_type_filter().is_some_and(|f| f.is_none()),
                CategoryType(t) => app
                    .category_type_filter()
                    .is_some_and(|f| f.as_deref() == Some(t.as_str())),
                _ => false,
            },
        }
    }

    pub fn title(action: &SidebarQuickAction, app: &dyn QuickActionHost) -> String {
        if *action != SidebarQuickAction::ToggleChat {
            return action.title();
        }
        if app.is_chat_visible() {
            "채팅 숨기기".into()
        } else {
            "채팅 표시".into()
        }
    }

    /// 떠난 메뉴의 지연 클릭은 아무것도 하지 않는다.
    pub async fn perform(
        action: &SidebarQuickAction,
        destination: &SidebarDestination,
        app: &mut dyn QuickActionHost,
        router: &mut AppRouter,
    ) -> Option<SidebarPresetDraft> {
        let current =
            SidebarDestination::current(router.selected_sidebar_item, app.live_hub_mode());
        if current != *destination
            || !SidebarQuickAction::actions_for(destination).contains(action)
            || !Self::is_enabled(action, app)
        {
            return None;
        }
        use SidebarQuickAction::*;
        match action.domain() {
            QuickActionDomain::Navigation => match action {
                ResumeRecent => {
                    if let Some((id, name)) = app.first_recent_channel() {
                        router.navigate_to_watch(&id, Some(&name));
                    }
                }
                FollowingFilter(filter) => {
                    app.open_live_section(FollowingHubMode::Explore, router);
                    app.set_browser_filter_and_reset_pages(*filter);
                }
                Insights => {
                    router.set_home_hub_mode(HomeHubMode::Insights);
                    router.select_sidebar(SidebarItem::Home);
                }
                ChatSettings => router.select_settings_group(SettingsGroup::Chat),
                Library(mode) => router.set_library_hub_mode(*mode),
                ReturnToLive => {
                    let mode = if app.live_hub_mode() == FollowingHubMode::Multi {
                        FollowingHubMode::Multi
                    } else {
                        FollowingHubMode::Watch
                    };
                    app.open_live_section(mode, router);
                }
                NetworkSettings => router.select_settings_group(SettingsGroup::Integration),
                _ => {}
            },
            QuickActionDomain::Playback => match action {
                AddChannel => app.present_channel_dock(),
                ToggleChat => app.toggle_chat(),
                CopyLiveLink => {
                    if let Some(id) = app.selected_multi_channel_id() {
                        app.copy_to_clipboard(
                            &format!("https://chzzk.naver.com/live/{id}"),
                            "방송 링크",
                        );
                    }
                }
                FocusAudio => app.focus_audio_on_selected(),
                AutomaticGrid => app.apply_automatic_grid(),
                SaveLivePreset => {
                    return Some(SidebarPresetDraft::new(
                        PresetKind::Live,
                        app.multi_live_channel_ids(),
                    ))
                }
                _ => {}
            },
            QuickActionDomain::StandaloneChat => match action {
                ReconnectChats => app.reconnect_standalone_chats().await,
                SaveChatPreset => {
                    return Some(SidebarPresetDraft::new(
                        PresetKind::Chat,
                        app.standalone_chat_channel_ids(),
                    ))
                }
                _ => {}
            },
            QuickActionDomain::Discovery => match action {
                SearchScope(scope) => app.set_search_scope(*scope).await,
                ClearSearch => app.clear_search(),
                PopularList => app.set_popular_mode(PopularMode::All),
                PopularSearch => app.set_popular_mode(PopularMode::Search),
                RefreshPopular => app.refresh_popular().await,
                CategoryAll => app.set_category_type(None),
                CategoryType(t) => app.set_category_type(Some(t.clone())),
                _ => {}
            },
        }
        None
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/SidebarQuickActionsTests.swift` (렌더 테스트 제외).
    use super::*;
    use crate::services::menu_transition_gate::MenuTransitionGate;
    use crate::services::support::MemoryKeyValueStore;
    use std::collections::HashSet;
    use std::sync::Arc;

    #[derive(Default)]
    struct Host {
        mode: Option<FollowingHubMode>,
        recent: Option<(String, String)>,
        sessions: Vec<String>,
        selected: Option<usize>,
        standalone: Vec<String>,
        filter: FollowingBrowserFilter,
        dock_hidden: bool,
        chat_visible: bool,
        search: Option<(SearchScope, bool)>,
        category: Option<Option<String>>,
        audio_focused: bool,
        grid: bool,
        copied: Vec<String>,
    }

    #[async_trait]
    impl QuickActionHost for Host {
        fn live_hub_mode(&self) -> FollowingHubMode {
            self.mode.unwrap_or(FollowingHubMode::Explore)
        }
        fn first_recent_channel(&self) -> Option<(String, String)> {
            self.recent.clone()
        }
        fn has_selected_multi_session(&self) -> bool {
            self.selected.is_some()
        }
        fn has_multi_sessions(&self) -> bool {
            !self.sessions.is_empty()
        }
        fn has_standalone_chat_sessions(&self) -> bool {
            !self.standalone.is_empty()
        }
        fn browser_filter(&self) -> FollowingBrowserFilter {
            self.filter
        }
        fn is_chat_visible(&self) -> bool {
            self.chat_visible
        }
        fn search_state(&self) -> Option<(SearchScope, bool)> {
            self.search
        }
        fn popular_state(&self) -> Option<(PopularMode, bool)> {
            None
        }
        fn category_type_filter(&self) -> Option<Option<String>> {
            self.category.clone()
        }
        fn open_live_section(&mut self, mode: FollowingHubMode, router: &mut AppRouter) {
            self.mode = Some(mode);
            router.select_sidebar(SidebarItem::Following);
        }
        fn set_browser_filter_and_reset_pages(&mut self, filter: FollowingBrowserFilter) {
            self.filter = filter;
        }
        fn present_channel_dock(&mut self) {
            self.dock_hidden = false;
        }
        fn toggle_chat(&mut self) {
            self.chat_visible = !self.chat_visible;
        }
        fn selected_multi_channel_id(&self) -> Option<String> {
            self.selected.map(|i| self.sessions[i].clone())
        }
        fn focus_audio_on_selected(&mut self) {
            self.audio_focused = true;
        }
        fn apply_automatic_grid(&mut self) {
            self.grid = true;
        }
        fn multi_live_channel_ids(&self) -> Vec<String> {
            self.sessions.clone()
        }
        fn standalone_chat_channel_ids(&self) -> Vec<String> {
            self.standalone.clone()
        }
        async fn reconnect_standalone_chats(&mut self) {}
        async fn set_search_scope(&mut self, scope: SearchScope) {
            if let Some(s) = self.search.as_mut() {
                s.0 = scope;
            }
        }
        fn clear_search(&mut self) {}
        fn set_popular_mode(&mut self, _: PopularMode) {}
        async fn refresh_popular(&mut self) {}
        fn set_category_type(&mut self, type_filter: Option<String>) {
            self.category = Some(type_filter);
        }
        fn copy_to_clipboard(&mut self, text: &str, _: &str) {
            self.copied.push(text.into());
        }
    }

    fn fixture(dest: SidebarDestination) -> (Host, AppRouter) {
        let mut router = AppRouter::with_gate(
            MemoryKeyValueStore::shared(),
            Arc::new(MenuTransitionGate::default()),
        );
        router.selected_sidebar_item = dest.route();
        let mut host = Host {
            dock_hidden: true,
            ..Default::default()
        };
        if let SidebarDestination::Live(mode) = dest {
            host.mode = Some(mode);
        }
        (host, router)
    }

    #[test]
    fn catalog_coverage() {
        let mut destinations: Vec<SidebarDestination> = SidebarDestination::primary().to_vec();
        destinations.extend(SidebarDestination::DISCOVER);
        destinations.push(SidebarDestination::Item(SidebarItem::Settings));
        for d in &destinations {
            let actions = SidebarQuickAction::actions_for(d);
            assert!((3..=4).contains(&actions.len()), "{d:?}");
            let ids: HashSet<String> = actions.iter().map(|a| a.id()).collect();
            assert_eq!(ids.len(), actions.len());
            assert!(actions
                .iter()
                .all(|a| !a.title().is_empty() && !a.icon().is_empty()));
        }
        assert_ne!(
            SidebarQuickAction::actions_for(&SidebarDestination::Live(FollowingHubMode::Watch)),
            SidebarQuickAction::actions_for(&SidebarDestination::Live(FollowingHubMode::Multi))
        );
    }

    #[tokio::test]
    async fn availability_and_stale_click() {
        let dest = SidebarDestination::Live(FollowingHubMode::Watch);
        let (mut app, mut router) = fixture(dest);
        assert!(!SidebarQuickActionExecutor::is_enabled(
            &SidebarQuickAction::CopyLiveLink,
            &app
        ));
        assert!(!SidebarQuickActionExecutor::is_enabled(
            &SidebarQuickAction::SaveLivePreset,
            &app
        ));
        assert!(!SidebarQuickActionExecutor::is_enabled(
            &SidebarQuickAction::ReconnectChats,
            &app
        ));
        assert!(!SidebarQuickActionExecutor::is_enabled(
            &SidebarQuickAction::SearchScope(SearchScope::Live),
            &app
        ));
        router.selected_sidebar_item = SidebarItem::Home;
        SidebarQuickActionExecutor::perform(
            &SidebarQuickAction::AddChannel,
            &dest,
            &mut app,
            &mut router,
        )
        .await;
        assert!(app.dock_hidden, "떠난 메뉴의 클릭이 패널을 열었다");
        router.selected_sidebar_item = SidebarItem::Following;
        SidebarQuickActionExecutor::perform(
            &SidebarQuickAction::AddChannel,
            &dest,
            &mut app,
            &mut router,
        )
        .await;
        assert!(!app.dock_hidden);
    }

    #[tokio::test]
    async fn playback_commands_and_preset_draft() {
        let dest = SidebarDestination::Live(FollowingHubMode::Multi);
        let (mut app, mut router) = fixture(dest);
        app.sessions = vec!["c".into(), "a".into(), "b".into()];
        app.selected = Some(1);
        SidebarQuickActionExecutor::perform(
            &SidebarQuickAction::FocusAudio,
            &dest,
            &mut app,
            &mut router,
        )
        .await;
        assert!(app.audio_focused);
        SidebarQuickActionExecutor::perform(
            &SidebarQuickAction::AutomaticGrid,
            &dest,
            &mut app,
            &mut router,
        )
        .await;
        assert!(app.grid);
        let draft = SidebarQuickActionExecutor::perform(
            &SidebarQuickAction::SaveLivePreset,
            &dest,
            &mut app,
            &mut router,
        )
        .await
        .unwrap();
        assert_eq!(draft.kind, PresetKind::Live);
        assert_eq!(draft.channel_ids, vec!["c", "a", "b"]);
    }

    #[tokio::test]
    async fn content_commands() {
        let dest = SidebarDestination::Live(FollowingHubMode::Explore);
        let (mut app, mut router) = fixture(dest);
        SidebarQuickActionExecutor::perform(
            &SidebarQuickAction::FollowingFilter(FollowingBrowserFilter::Favorites),
            &dest,
            &mut app,
            &mut router,
        )
        .await;
        assert_eq!(app.filter, FollowingBrowserFilter::Favorites);
        assert!(SidebarQuickActionExecutor::is_selected(
            &SidebarQuickAction::FollowingFilter(FollowingBrowserFilter::Favorites),
            &app,
            &router
        ));

        router.selected_sidebar_item = SidebarItem::Search;
        app.search = Some((SearchScope::All, true));
        let search = SidebarDestination::Item(SidebarItem::Search);
        SidebarQuickActionExecutor::perform(
            &SidebarQuickAction::SearchScope(SearchScope::Live),
            &search,
            &mut app,
            &mut router,
        )
        .await;
        assert_eq!(app.search.unwrap().0, SearchScope::Live);
        assert!(!SidebarQuickActionExecutor::is_enabled(
            &SidebarQuickAction::ClearSearch,
            &app
        ));

        router.selected_sidebar_item = SidebarItem::Library;
        SidebarQuickActionExecutor::perform(
            &SidebarQuickAction::Library(LibraryHubMode::Downloads),
            &SidebarDestination::Item(SidebarItem::Library),
            &mut app,
            &mut router,
        )
        .await;
        assert_eq!(router.library_hub_mode, LibraryHubMode::Downloads);
    }

    #[tokio::test]
    async fn category_and_settings_commands() {
        let dest = SidebarDestination::Item(SidebarItem::Category);
        let (mut app, mut router) = fixture(dest);
        app.category = Some(None);
        SidebarQuickActionExecutor::perform(
            &SidebarQuickAction::CategoryType("GAME".into()),
            &dest,
            &mut app,
            &mut router,
        )
        .await;
        assert_eq!(app.category, Some(Some("GAME".into())));
        SidebarQuickActionExecutor::perform(
            &SidebarQuickAction::CategoryAll,
            &dest,
            &mut app,
            &mut router,
        )
        .await;
        assert_eq!(app.category, Some(None));

        app.mode = Some(FollowingHubMode::Multi);
        router.enter_settings();
        let settings = SidebarDestination::Item(SidebarItem::Settings);
        SidebarQuickActionExecutor::perform(
            &SidebarQuickAction::NetworkSettings,
            &settings,
            &mut app,
            &mut router,
        )
        .await;
        assert_eq!(router.selected_settings_group, SettingsGroup::Integration);
        assert!(router.is_in_settings_mode);
        SidebarQuickActionExecutor::perform(
            &SidebarQuickAction::ReturnToLive,
            &settings,
            &mut app,
            &mut router,
        )
        .await;
        assert_eq!(router.selected_sidebar_item, SidebarItem::Following);
        assert!(!router.is_in_settings_mode);
        assert_eq!(app.mode, Some(FollowingHubMode::Multi));
    }

    #[test]
    fn toggle_chat_title_reflects_visibility() {
        let (mut app, _) = fixture(SidebarDestination::Live(FollowingHubMode::Watch));
        assert_eq!(
            SidebarQuickActionExecutor::title(&SidebarQuickAction::ToggleChat, &app),
            "채팅 표시"
        );
        app.chat_visible = true;
        assert_eq!(
            SidebarQuickActionExecutor::title(&SidebarQuickAction::ToggleChat, &app),
            "채팅 숨기기"
        );
    }
}
