//! `Sources/CViewApp/AppState.swift` 와 확장 `AppState+Auth.swift` · `+LiveSections.swift` ·
//! `+LocalDataHygiene.swift` · `+MultiLive.swift` · `+StoreRecovery.swift` 의 UI 비의존 부분 포팅.
//!
//! 원본 `AppState` 는 `@MainActor @Observable` 상태 컨테이너로, ViewModel·매니저·AppKit 관찰자를 직접
//! 들고 있다. 여기서는 **상태와 규칙**만 옮기고, VM·매니저에 닿는 부수 효과는 반환값(효과 묶음)이나
//! 호스트 트레이트로 셸에 넘긴다.
//!
//! - 상태: 로그인·인증 초기화·설정 로드·저장소 실패/백업·사용자 프로필·세션 만료 경고·사이드바 접힘
//!   (`"sidebar.collapsed"`)·분리 창 채널·로컬 데이터 리비전·인증 UI 에포크, 즐겨찾기·최근 시청 미러.
//! - 워크스페이스 스냅샷 캡처/적용([`AppState::capture_workspace_snapshot`], [`workspace_restore`]).
//! - `+LiveSections`: [`open_live_section`] · [`open_sidebar_destination`] · [`pending_live_hub_mode_before_enter`].
//! - `+Auth`: 인증 경계([`AppState::begin_authentication_ui_transition`]), 프로필 반영, 프로필 재동기화
//!   상태기계([`ProfileResync`]), 인증 상태 후속 판정([`auth_follow_up`]).
//! - `+LocalDataHygiene`: 단계별 로컬 행 삭제([`delete_local_user_rows`])와 성공 단계 미러 비우기,
//!   계정 스냅샷 정리 사슬([`AccountSnapshotCleanup`]).
//! - `+MultiLive`: 조합 교체 단일 비행 게이트([`MultiLivePresetApplyGate`]), 프리셋 적용·전체 해제·
//!   세션 종료 알림 문구, 추가 결과 정산 규칙.
//! - `+StoreRecovery`: 열기 실패 요약·복구 정보·복원 진입 조건.
//!
//! 건너뛴 것: `+PiP`·`+Widget`(플랫폼), 액션 레지스트리 바인딩·메뉴 진입 훅(라우터 콜백 배선),
//! 토스트·애니메이션·NotificationCenter 관찰자, `AppDependencies` 의 배선(셸이 조립한다).

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use cview_core::protocols::AuthState;
use cview_core::utils::time::{Date, TimeInterval};
use cview_persistence::models::persisted_channel::ChannelListData;
use cview_persistence::store_backup::BackupEntry;
use cview_persistence::store_open_health::{ErrorSummary, StoreOpenHealth};
use futures_util::future::{BoxFuture, FutureExt, Shared};
use uuid::Uuid;

use crate::caching::favorites_store::FavoritesStore;
use crate::caching::recent_channels_store::RecentChannelsStore;
use crate::caching::workspace_snapshot::WorkspaceSnapshot;
use crate::navigation::hub_mode::FollowingHubMode;
use crate::navigation::router::{AppRouter, SidebarItem};
use crate::navigation::sidebar_destination::SidebarDestination;
use crate::services::channel_preset_store::Preset;
use crate::services::support::{lock, KeyValueStore};
use crate::services::vod_playback_progress::VodPlaybackProgressStore;
use crate::services::vod_sync_state::VodSubscriptionTag;

/// 사이드바 접힘 상태 키.
pub const SIDEBAR_COLLAPSED_KEY: &str = "sidebar.collapsed";
/// 사이드바 전환 애니메이션 창(초) — 그동안 무거운 재평가를 미룬다.
pub const SIDEBAR_TRANSITION_WINDOW: TimeInterval = 0.6;

/// 팔로잉 캐시 범위 UUID 의 Swift `uuidString` 표기(대문자).
pub fn scope_string(scope: Uuid) -> String {
    scope.to_string().to_uppercase()
}

/// 저장된 팔로잉 캐시 범위(`CView.followingCacheScope.v2`).
pub fn persisted_following_cache_scope(defaults: &dyn KeyValueStore) -> Option<Uuid> {
    let raw = defaults.string(VodSubscriptionTag::SCOPE_DEFAULTS_KEY)?;
    Uuid::parse_str(&raw).ok()
}

// MARK: - AppState

/// 앱 전역 상태 컨테이너(`AppState`)의 UI 비의존 부분.
pub struct AppState {
    defaults: Arc<dyn KeyValueStore>,

    pub is_initialized: bool,
    pub is_logged_in: bool,
    /// 로컬 사용자 데이터 삭제마다 올라간다 — 떠 있는 화면이 다시 읽는 신호.
    pub local_user_data_revision: u64,
    /// 인증 경계마다 바뀐다. 늦게 끝난 비동기 단계는 이 값이 달라졌으면 결과를 버린다.
    pub authentication_ui_epoch: Uuid,
    pub is_auth_initialized: bool,

    pub is_settings_loaded: bool,
    pub is_initializing_data_store: bool,
    pub data_store_initialization_failed: bool,
    /// 열린 저장소가 있는가(`dataStore != nil`).
    pub has_data_store: bool,
    pub store_backups: Vec<BackupEntry>,
    pub last_store_open_failure: Option<StoreOpenFailureSummary>,
    pub is_restoring_store_backup: bool,
    pub store_restore_failed: bool,

    pub user_nickname: Option<String>,
    pub user_channel_id: Option<String>,
    pub user_profile_url: Option<String>,
    pub session_expired_alert: bool,
    pub is_app_active: bool,
    pub is_splash_dismissed: bool,
    pub welcome_replay_requests: u64,

    sidebar_collapsed: bool,
    detached_channel_ids: HashSet<String>,

    pub favorites_store: FavoritesStore,
    pub recent_channels_store: RecentChannelsStore,
    pub profile_resync: ProfileResync,
}

impl AppState {
    pub fn new(defaults: Arc<dyn KeyValueStore>) -> Self {
        let sidebar_collapsed = defaults.bool(SIDEBAR_COLLAPSED_KEY);
        Self {
            defaults,
            is_initialized: false,
            is_logged_in: false,
            local_user_data_revision: 0,
            authentication_ui_epoch: Uuid::new_v4(),
            is_auth_initialized: false,
            is_settings_loaded: false,
            is_initializing_data_store: false,
            data_store_initialization_failed: false,
            has_data_store: false,
            store_backups: Vec::new(),
            last_store_open_failure: None,
            is_restoring_store_backup: false,
            store_restore_failed: false,
            user_nickname: None,
            user_channel_id: None,
            user_profile_url: None,
            session_expired_alert: false,
            is_app_active: true,
            is_splash_dismissed: false,
            welcome_replay_requests: 0,
            sidebar_collapsed,
            detached_channel_ids: HashSet::new(),
            favorites_store: FavoritesStore::new(),
            recent_channels_store: RecentChannelsStore::new(),
            profile_resync: ProfileResync::default(),
        }
    }

    pub fn defaults(&self) -> &Arc<dyn KeyValueStore> {
        &self.defaults
    }

    // MARK: Sidebar

    pub fn sidebar_collapsed(&self) -> bool {
        self.sidebar_collapsed
    }

    pub fn toggle_sidebar(&mut self) {
        let next = !self.sidebar_collapsed;
        self.set_sidebar_collapsed(next);
    }

    /// 값이 같으면 아무것도 하지 않는다(쓰기·애니메이션 없음).
    pub fn set_sidebar_collapsed(&mut self, collapsed: bool) -> bool {
        if self.sidebar_collapsed == collapsed {
            return false;
        }
        self.sidebar_collapsed = collapsed;
        self.defaults.set_bool(SIDEBAR_COLLAPSED_KEY, collapsed);
        true
    }

    // MARK: Detached channels

    pub fn detached_channel_ids(&self) -> &HashSet<String> {
        &self.detached_channel_ids
    }

    pub fn register_detached_channel(&mut self, channel_id: &str) {
        self.detached_channel_ids.insert(channel_id.to_string());
    }

    pub fn unregister_detached_channel(&mut self, channel_id: &str) {
        self.detached_channel_ids.remove(channel_id);
    }

    // MARK: Workspace snapshot (SES-1)

    /// 현재 워크스페이스를 스냅샷으로 캡처한다. 라우터가 없으면 사이드바·검색어는 비운다.
    pub fn capture_workspace_snapshot(
        &self,
        router: Option<&AppRouter>,
        inputs: &WorkspaceInputs,
        now: Date,
    ) -> WorkspaceSnapshot {
        let mut snap = WorkspaceSnapshot::new(now);
        snap.sidebar = router.map(|r| r.selected_sidebar_item.raw_value().to_string());
        snap.hub_mode = Some(inputs.hub_mode.raw_value().to_string());
        snap.multi_live_channel_ids = inputs.multi_live_channel_ids.clone();
        snap.multi_chat_channel_ids = inputs.multi_chat_channel_ids.clone();
        snap.pending_search_query = router.and_then(|r| r.pending_search_query.clone());
        snap.smart_queue_channel_ids = inputs.smart_queue_channel_ids.clone();
        snap
    }

    // MARK: Auth boundary (+Auth)

    /// 지금 계정의 팔로잉 캐시 범위 — 홈 VM 값이 있으면 그것, 없으면 저장값.
    pub fn current_following_cache_scope(&self, home_scope: Option<Uuid>) -> Option<Uuid> {
        home_scope.or_else(|| persisted_following_cache_scope(self.defaults.as_ref()))
    }

    /// 인증 경계 — 에포크를 바꾸고 계정별 상태를 비운 뒤 새 캐시 범위를 저장한다.
    ///
    /// 셸이 이어서 할 일([`AuthBoundary`] 참고): 채널 상세 비우기, 검색 캐시 무효화, 홈 팔로잉 인증 무효화,
    /// 홈 VM 범위 갱신, 옛 범위 계정 스냅샷 은퇴(+옛 행 삭제), VOD VM 무효화·`VodSyncStore::reset`·구독 VOD
    /// 사본 정리(`vod_subscription_tag` 밖), 멀티라이브·채팅 사용자 정보 비우기, 백그라운드 갱신 중지, 위젯 갱신.
    pub fn begin_authentication_ui_transition(&mut self, home_scope: Option<Uuid>) -> AuthBoundary {
        self.authentication_ui_epoch = Uuid::new_v4();
        self.is_logged_in = false;
        // 즐겨찾기는 계정별 데이터다 — 경계에서 비우지 않으면 이전 계정의 필터·가중치가 남는다.
        self.favorites_store.clear();
        // 부팅 프로필 재동기화는 부팅 세션 것이다 — 로그인 흐름이 직접 로드한다.
        self.profile_resync.disarm();
        let retired_scope = self.current_following_cache_scope(home_scope);
        let scope = Uuid::new_v4();
        self.defaults
            .set_string(VodSubscriptionTag::SCOPE_DEFAULTS_KEY, &scope_string(scope));
        self.clear_user_profile();
        AuthBoundary {
            epoch: self.authentication_ui_epoch,
            retired_scope,
            scope,
            vod_subscription_tag: VodSubscriptionTag::current(Some(scope), true),
        }
    }

    pub fn is_current_epoch(&self, epoch: Uuid) -> bool {
        self.authentication_ui_epoch == epoch
    }

    pub fn clear_user_profile(&mut self) {
        self.user_nickname = None;
        self.user_channel_id = None;
        self.user_profile_url = None;
    }

    /// 로그인 흐름의 인증 결과 반영. 에포크가 바뀌었으면 false(이후 단계를 멈춘다).
    pub fn apply_login_result(&mut self, epoch: Uuid, authenticated: bool) -> bool {
        if !self.is_current_epoch(epoch) {
            return false;
        }
        self.is_logged_in = authenticated;
        true
    }

    /// 쿠키 경로 프로필(`userStatus`) 반영 — 이미 있는 닉네임·이미지는 덮지 않는다.
    pub fn apply_user_status(
        &mut self,
        epoch: Uuid,
        nickname: Option<&str>,
        profile_image_url: Option<&str>,
        user_id_hash: Option<&str>,
    ) -> Option<ChatIdentity> {
        if !self.is_current_epoch(epoch) || !self.is_logged_in {
            return None;
        }
        if self.user_nickname.is_none() {
            self.user_nickname = nickname.map(str::to_string);
        }
        if let Some(url) = profile_image_url {
            if self.user_profile_url.is_none() {
                self.user_profile_url = Some(url.to_string());
            }
        }
        // 채널 ID 는 OAuth 로그인에서만 온다 — 쿠키 세션의 채팅 uid 는 userIdHash.
        Some(ChatIdentity {
            multi_live_uid: self.user_channel_id.clone(),
            chat_uid: user_id_hash
                .map(str::to_string)
                .or_else(|| self.user_channel_id.clone()),
            nickname: self.user_nickname.clone(),
        })
    }

    /// OAuth 프로필 반영 — 채널 ID 는 응답값으로 덮는다.
    pub fn apply_oauth_profile(
        &mut self,
        epoch: Uuid,
        nickname: Option<&str>,
        channel_id: Option<&str>,
        profile_image_url: Option<&str>,
    ) -> Option<ChatIdentity> {
        if !self.is_current_epoch(epoch) || !self.is_logged_in {
            return None;
        }
        if let Some(n) = nickname {
            self.user_nickname = Some(n.to_string());
        }
        self.user_channel_id = channel_id.map(str::to_string);
        if let Some(url) = profile_image_url {
            self.user_profile_url = Some(url.to_string());
        }
        Some(ChatIdentity {
            multi_live_uid: self.user_channel_id.clone(),
            chat_uid: self.user_channel_id.clone(),
            nickname: self.user_nickname.clone(),
        })
    }

    /// 닉네임이 없거나, OAuth 세션인데 채널 ID 가 없으면 미완성이다(쿠키 세션은 채널 ID 를 받지 않는다).
    pub fn is_profile_incomplete(&self, has_oauth_tokens: bool) -> bool {
        if self.user_nickname.is_none() {
            return true;
        }
        self.user_channel_id.is_none() && has_oauth_tokens
    }

    // MARK: Local data (+LocalDataHygiene)

    /// 성공한 단계의 미러만 비운다 — 실패한 단계의 행은 DB 에 남아 있다.
    pub fn apply_local_user_data_clearance(
        &mut self,
        cleared: &LocalUserDataClearance,
        vod_progress: &mut VodPlaybackProgressStore,
    ) {
        if cleared.last_watched {
            self.recent_channels_store.clear();
        }
        if cleared.vod {
            vod_progress.remove_all();
        }
        if cleared.favorites {
            self.favorites_store.clear();
        }
        if let Some(failure) = &cleared.failure {
            tracing::error!("clearLocalUserData failed: {failure}");
        }
    }

    /// 로컬 삭제 마무리 — 리비전을 올리고 라이브 개요에 넘길 최근 시청 ID 를 돌려준다.
    pub fn finish_local_user_data_clear(&mut self) -> Vec<String> {
        self.local_user_data_revision = self.local_user_data_revision.wrapping_add(1);
        self.recent_channels_store.channel_ids()
    }

    // MARK: Favorites / recents (+MultiLive)

    /// 영속이 돌려준 즐겨찾기 상태만 미러에 반영한다(낙관적 갱신 금지).
    pub fn apply_saved_favorite(&mut self, channel_id: &str, saved: bool) {
        self.favorites_store.apply(channel_id, saved);
        self.recent_channels_store.apply_favorite(channel_id, saved);
    }

    /// 즐겨찾기 재읽기 결과. 실패·취소면 미러를 그대로 둔다(빈 목록으로 지우지 않는다).
    pub fn apply_favorites_reload<E>(&mut self, fetched: Result<Vec<String>, E>) {
        if let Ok(ids) = fetched {
            self.favorites_store.hydrate(ids);
        }
    }

    /// 최근 시청 재읽기 결과. 성공하면 라이브 개요에 넘길 ID 목록을, 실패면 `None`(다음 진입이 다시 시도).
    pub fn apply_recent_channels_reload<E>(
        &mut self,
        generation: u64,
        fetched: Result<Vec<ChannelListData>, E>,
    ) -> Option<Vec<String>> {
        let rows = fetched.ok()?;
        self.recent_channels_store.hydrate(generation, &rows);
        Some(self.recent_channels_store.channel_ids())
    }

    // MARK: Store recovery (+StoreRecovery)

    /// 열기 실패 복구 정보를 싣는다. 그사이 열기에 성공했으면 덮지 않는다.
    pub fn note_store_open_failure(&mut self, info: StoreRecoveryInfo) {
        if !self.data_store_initialization_failed {
            return;
        }
        self.last_store_open_failure = Some(info.failure);
        self.store_backups = info.backups;
    }

    pub fn clear_store_recovery_state(&mut self) {
        self.data_store_initialization_failed = false;
        self.last_store_open_failure = None;
        self.store_backups.clear();
        self.store_restore_failed = false;
    }

    /// 백업 복원을 시작해도 되는가 — 열린 컨테이너가 없고, 실패 상태이고, 열기·복원 중이 아닐 때만.
    /// 시작하면 `is_restoring_store_backup` 을 세운다.
    pub fn begin_store_restore(&mut self) -> bool {
        if self.has_data_store
            || !self.data_store_initialization_failed
            || self.is_initializing_data_store
            || self.is_restoring_store_backup
        {
            return false;
        }
        self.is_restoring_store_backup = true;
        self.store_restore_failed = false;
        true
    }

    /// 복원 파일 작업의 결과. 실패면 `store_restore_failed` 를 세우고 false(다시 열지 않는다).
    pub fn finish_store_restore_copy(&mut self, ok: bool) -> bool {
        if !ok {
            self.store_restore_failed = true;
            self.is_restoring_store_backup = false;
        }
        ok
    }

    /// 복원 뒤 다시 열기까지 끝났다. 저장소가 열려 설정까지 로드됐으면 true.
    pub fn finish_store_restore(&mut self) -> bool {
        self.is_restoring_store_backup = false;
        !self.data_store_initialization_failed && self.is_settings_loaded
    }

    /// 부팅 유지보수용 영속 범위 토큰.
    pub fn persisted_vod_subscription_tag(&self) -> Option<String> {
        VodSubscriptionTag::persisted(self.defaults.as_ref())
    }
}

/// 워크스페이스 캡처 입력 — `FollowingViewState`·매니저가 가진 값.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceInputs {
    pub hub_mode: FollowingHubMode,
    pub multi_live_channel_ids: Vec<String>,
    pub multi_chat_channel_ids: Vec<String>,
    pub smart_queue_channel_ids: Vec<String>,
}

/// 스냅샷에서 실제로 복원할 것.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkspaceRestore {
    /// 적용할 라이브 모드 — 명시적 이동이 없고, 예약된 모드가 없고, 라이브 모드일 때만.
    pub hub_mode: Option<FollowingHubMode>,
    pub smart_queue_channel_ids: Option<Vec<String>>,
}

/// 저장된 라이브 모드와 스마트 큐만 복원한다. 시작 경로(홈)·자동 재생·멀티라이브·멀티채팅 세션은 복원하지 않는다.
pub fn workspace_restore(snap: &WorkspaceSnapshot, router: &AppRouter) -> WorkspaceRestore {
    let hub_mode = if !router.has_explicit_navigation() && router.pending_hub_mode.is_none() {
        snap.hub_mode
            .as_deref()
            .and_then(FollowingHubMode::from_raw)
            .filter(|m| m.is_live_workspace_mode())
    } else {
        None
    };
    WorkspaceRestore {
        hub_mode,
        smart_queue_channel_ids: (!snap.smart_queue_channel_ids.is_empty())
            .then(|| snap.smart_queue_channel_ids.clone()),
    }
}

// MARK: - Live sections (+LiveSections)

/// 라이브 진입 결과 — 셸이 `FollowingViewState::apply_hub_mode_preset` 을 불러야 하는가.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveSectionEffect {
    /// 라우트만 바뀌었다(또는 모드를 `pending_hub_mode` 로 예약했다).
    Routed,
    /// 이미 라이브 라우트다 — 루트로 돌아갔고 모드만 바꾼다.
    ApplyHubModePreset(FollowingHubMode),
}

/// 사이드바 행을 연다.
pub fn open_sidebar_destination(
    destination: SidebarDestination,
    router: &mut AppRouter,
) -> LiveSectionEffect {
    match destination {
        SidebarDestination::Item(item) => {
            router.select_sidebar(item);
            LiveSectionEffect::Routed
        }
        SidebarDestination::Live(mode) => open_live_section(mode, router),
    }
}

/// 라이브 모드(개요·시청·멀티) 또는 그 모드 값이 가리키는 독립 화면(검색·인기·카테고리)을 연다.
///
/// 이미 라이브 라우트면 모드만 바꾼다(epoch 을 올리지 않는다). 다른 메뉴에서면 `pending_hub_mode` 로
/// 예약하고 라우트를 바꿔 진입 훅이 커밋 전에 적용한다.
pub fn open_live_section(mode: FollowingHubMode, router: &mut AppRouter) -> LiveSectionEffect {
    if !mode.is_live_workspace_mode() {
        router.select_sidebar(mode.sidebar_destination().route());
        return LiveSectionEffect::Routed;
    }
    if router.selected_sidebar_item == SidebarItem::Following && !router.is_in_settings_mode {
        router.pop_to_root();
        LiveSectionEffect::ApplyHubModePreset(mode)
    } else {
        router.pending_hub_mode = Some(mode);
        router.select_sidebar(SidebarItem::Following);
        LiveSectionEffect::Routed
    }
}

/// 라이브 메뉴 진입 직전(커밋 전) 적용할 모드. 예약을 소비한다. 탐색 모드가 남아 있으면 개요로 정규화한다.
pub fn pending_live_hub_mode_before_enter(
    router: &mut AppRouter,
    current: FollowingHubMode,
) -> Option<FollowingHubMode> {
    let mut target = router
        .pending_hub_mode
        .take()
        .filter(|m| m.is_live_workspace_mode());
    if target.is_none() && !current.is_live_workspace_mode() {
        target = Some(FollowingHubMode::Explore);
    }
    target.filter(|t| *t != current)
}

// MARK: - Auth (+Auth)

/// 인증 경계가 셸에 넘기는 값.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthBoundary {
    pub epoch: Uuid,
    /// 은퇴시킬 옛 팔로잉 캐시 범위.
    pub retired_scope: Option<Uuid>,
    /// 새로 저장한 범위.
    pub scope: Uuid,
    /// 이 토큰 밖의 구독 VOD 사본을 지운다.
    pub vod_subscription_tag: Option<String>,
}

/// 멀티라이브·멀티채팅·단일 채팅에 전파할 사용자 정보.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatIdentity {
    /// 멀티라이브 채팅 전송용 uid(채널 ID).
    pub multi_live_uid: Option<String>,
    /// 멀티채팅 세션 uid(`userIdHash ?? channelId`).
    pub chat_uid: Option<String>,
    pub nickname: Option<String>,
}

/// 인증 상태 스트림 사건의 후속 처리.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthFollowUp {
    Ignore,
    /// OAuth 토큰 갱신 성공 — 걸린 프로필 재동기화 신호를 보낸다.
    FireProfileResyncSignal,
    /// 만료 — 지금 상태를 다시 읽어 [`should_end_expired_session`] 로 판정한다.
    VerifyExpiry,
}

/// 앱이 로그인 상태일 때 두 사건만 받는다 — 최종 만료, OAuth 갱신 성공. `.loggedIn` 으로 UI 를 올리지 않는다.
pub fn auth_follow_up(is_logged_in: bool, state: &AuthState) -> AuthFollowUp {
    if !is_logged_in {
        return AuthFollowUp::Ignore;
    }
    match state {
        AuthState::LoggedIn { user_id } if user_id == "oauth" => {
            AuthFollowUp::FireProfileResyncSignal
        }
        AuthState::Expired => AuthFollowUp::VerifyExpiry,
        _ => AuthFollowUp::Ignore,
    }
}

/// 토큰까지 버린 최종 만료만 세션을 끝낸다. 토큰이 남은 만료는 일시 실패(백오프 뒤 갱신 대기)다.
pub fn should_end_expired_session(
    current: &AuthState,
    has_oauth_tokens: bool,
    epoch_unchanged: bool,
    is_logged_in: bool,
) -> bool {
    *current == AuthState::Expired && !has_oauth_tokens && epoch_unchanged && is_logged_in
}

/// 프로필 재동기화 상태기계(정밀 검토 2026-09-26 F07).
///
/// 오프라인 부팅이면 프로필 로드가 실패한 채 끝난다. 복구 신호(홈 팔로잉 성공·백그라운드 폴 성공·OAuth 갱신
/// 성공)에 한 번 재조회를 건다. 시도 상한과 최소 간격으로 빈 계정이 폴마다 두드리지 않게 한다.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProfileResync {
    armed: Option<ResyncArm>,
}

#[derive(Debug, Clone, PartialEq)]
struct ResyncArm {
    epoch: Uuid,
    attempts: u32,
    /// 이 단조 시각(초) 전의 신호는 무시한다.
    not_before: Option<TimeInterval>,
}

/// 신호가 통과시킨 재조회 한 번.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResyncRun {
    pub epoch: Uuid,
    pub attempts: u32,
}

impl ProfileResync {
    pub const DEFAULT_ATTEMPTS: u32 = 3;
    /// 재시도 최소 간격 — 팔로잉 응답 캐시 TTL(60초).
    pub const SPACING: TimeInterval = 60.0;

    pub fn is_armed(&self) -> bool {
        self.armed.is_some()
    }

    pub fn arm(&mut self, epoch: Uuid, attempts: u32, not_before: Option<TimeInterval>) {
        self.armed = Some(ResyncArm {
            epoch,
            attempts,
            not_before,
        });
    }

    pub fn disarm(&mut self) {
        self.armed = None;
    }

    /// 신호 하나. 시각 전이거나 에포크가 바뀌었으면 `None`(걸린 채 둔다). 통과하면 **먼저 해제**해 같은 턴의
    /// 두 신호가 둘 다 조회하지 않게 한다.
    pub fn signal(&mut self, now: TimeInterval, current_epoch: Uuid) -> Option<ResyncRun> {
        let arm = self.armed.as_ref()?;
        if arm.not_before.is_some_and(|t| now < t) || arm.epoch != current_epoch {
            return None;
        }
        let run = ResyncRun {
            epoch: arm.epoch,
            attempts: arm.attempts,
        };
        self.disarm();
        Some(run)
    }

    /// 재조회 뒤에도 미완성이면 시도를 하나 줄여 간격 뒤로 다시 건다.
    pub fn rearm_after(
        &mut self,
        run: ResyncRun,
        still_incomplete: bool,
        current_epoch: Uuid,
        now: TimeInterval,
    ) -> bool {
        if run.epoch != current_epoch || run.attempts <= 1 || !still_incomplete {
            return false;
        }
        self.arm(run.epoch, run.attempts - 1, Some(now + Self::SPACING));
        true
    }
}

// MARK: - Local data hygiene (+LocalDataHygiene)

/// 로컬 사용자 행 삭제의 단계별 결과. 단계마다 저장이 따로라 뒤 단계가 실패해도 앞 단계는 이미 지워졌다.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalUserDataClearance {
    pub history: bool,
    pub last_watched: bool,
    pub vod: bool,
    pub clip: bool,
    pub favorites: bool,
    pub failure: Option<String>,
}

/// 로컬 사용자 행 삭제 단계(`DataStore` 의 해당 메서드).
#[async_trait]
pub trait LocalUserRows: Send + Sync {
    async fn delete_all_watch_history(&self) -> Result<(), String>;
    async fn clear_all_last_watched(&self) -> Result<(), String>;
    async fn delete_all_vod_playback(&self) -> Result<(), String>;
    async fn delete_all_clip_playback(&self) -> Result<(), String>;
    async fn clear_all_favorites(&self) -> Result<(), String>;
}

/// 시청 기록 → 최근 시청 → VOD → 클립 → 즐겨찾기 순으로 지운다. 첫 실패에서 멈춘다.
pub async fn delete_local_user_rows(rows: &dyn LocalUserRows) -> LocalUserDataClearance {
    let mut c = LocalUserDataClearance::default();
    let result: Result<(), String> = async {
        rows.delete_all_watch_history().await?;
        c.history = true;
        rows.clear_all_last_watched().await?;
        c.last_watched = true;
        rows.delete_all_vod_playback().await?;
        c.vod = true;
        rows.delete_all_clip_playback().await?;
        c.clip = true;
        rows.clear_all_favorites().await?;
        c.favorites = true;
        Ok(())
    }
    .await;
    if let Err(e) = result {
        c.failure = Some(e);
    }
    c
}

/// 계정 범위 메뉴 스냅샷 저장소(`MenuSnapshotStore` 의 계정 위생 부분).
#[async_trait]
pub trait AccountSnapshots: Send + Sync {
    /// 범위 하나의 파일·보류 쓰기를 지우고 범위를 은퇴시킨다.
    async fn remove_scope(&self, scope: &str);
    /// `keeping` 밖의 계정 파일을 지우고 은퇴시킨다.
    async fn prune_accounts(&self, keeping: &[String]);
    /// tmp·0바이트·최대 나이 초과·다른 계정 파일과 예산 정리.
    async fn sweep(&self, keep_account_scopes: &[String]);
}

type Flight = Shared<BoxFuture<'static, ()>>;

/// 계정 스냅샷 정리 사슬(`accountSnapshotCleanup`). 경계·부팅 prune·유지보수 sweep 의 순서를 지킨다 —
/// 부팅 prune 이 경계 뒤 새 범위를 고아로 오인해 은퇴시키면 그 세션의 팔로잉 저장이 전부 거절된다.
#[derive(Clone, Default)]
pub struct AccountSnapshotCleanup {
    tail: Arc<Mutex<Option<Flight>>>,
}

/// 지금 계정 범위를 읽는 함수(홈 VM 값, 없으면 저장값).
pub type ScopeReader = Arc<dyn Fn() -> Option<Uuid> + Send + Sync>;

impl AccountSnapshotCleanup {
    pub fn new() -> Self {
        Self::default()
    }

    fn chain(&self, work: impl FnOnce() -> BoxFuture<'static, ()> + Send + 'static) {
        let mut tail = lock(&self.tail);
        let previous = tail.take();
        let handle = tokio::spawn(async move {
            if let Some(p) = previous {
                p.await;
            }
            work().await;
        });
        let flight: Flight = async move {
            let _ = handle.await;
        }
        .boxed()
        .shared();
        *tail = Some(flight);
    }

    /// 계정 범위 하나를 은퇴시킨다. `legacy_row_delete` 는 옛 SwiftData 팔로잉 스냅샷 행 삭제.
    pub fn retire(
        &self,
        snapshots: Arc<dyn AccountSnapshots>,
        scope: Option<Uuid>,
        legacy_row_delete: Option<BoxFuture<'static, ()>>,
    ) {
        self.chain(move || {
            async move {
                if let Some(scope) = scope {
                    snapshots.remove_scope(&scope_string(scope)).await;
                }
                if let Some(delete) = legacy_row_delete {
                    delete.await;
                }
            }
            .boxed()
        });
    }

    /// 부팅 정리 — 그사이 경계가 범위를 바꿨으면 건너뛴다.
    pub fn prune(&self, snapshots: Arc<dyn AccountSnapshots>, keeping: Uuid, current: ScopeReader) {
        self.chain(move || {
            async move {
                if current() != Some(keeping) {
                    return;
                }
                snapshots.prune_accounts(&[scope_string(keeping)]).await;
            }
            .boxed()
        });
    }

    /// 45초 유지보수의 sweep — 앞 사슬을 기다리고, 범위를 모르면 건너뛴다.
    pub async fn sweep(&self, snapshots: &dyn AccountSnapshots, current: ScopeReader) {
        self.wait().await;
        let Some(scope) = current() else { return };
        snapshots.sweep(&[scope_string(scope)]).await;
    }

    /// 사슬의 마지막 작업까지 기다린다.
    pub async fn wait(&self) {
        let tail = lock(&self.tail).clone();
        if let Some(t) = tail {
            t.await;
        }
    }
}

// MARK: - Multi-live (+MultiLive)

/// 멀티라이브 추가 결과(`MultiLiveAddSessionResult`)에서 정산에 필요한 부분.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiLiveAddOutcome {
    pub is_success: bool,
    pub is_new_session: bool,
    /// `None` 은 취소 — 사용자 오류가 아니다.
    pub user_message: Option<String>,
}

/// 추가 정산 효과.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MultiLiveSettleEffects {
    pub is_current: bool,
    /// 추가한 채널을 선택하고 채팅 도크로 연다.
    pub activate_channel: bool,
    pub preserves_main_focus: bool,
    pub show_multi_live_panel: bool,
    /// 라이브 영상이 앞에 없다 — 추가가 되살린 재생을 다시 정지한다.
    pub release_video_surfaces: bool,
    pub error_message: Option<String>,
}

/// 활성화·패널·알림은 최신 요청일 때만, 재정지는 요청과 무관하게 지금 화면으로 정한다.
/// `request_is_current` 는 요청 토큰이 있으면 그 판정, 없으면(`activatesChannel == false`) `None`.
pub fn settle_embedded_multi_live_add(
    result: &MultiLiveAddOutcome,
    request_is_current: Option<bool>,
    open_following_panel: bool,
    is_live_video_in_front: bool,
) -> MultiLiveSettleEffects {
    let is_current = request_is_current.unwrap_or(true);
    let mut fx = MultiLiveSettleEffects {
        is_current,
        ..Default::default()
    };
    if result.is_success {
        if is_current && request_is_current.is_some() {
            fx.activate_channel = true;
            fx.preserves_main_focus = result.is_new_session;
        }
        fx.show_multi_live_panel = is_current && open_following_panel;
        fx.release_video_surfaces = !is_live_video_in_front;
    } else if is_current {
        fx.error_message = result.user_message.clone();
    }
    fx
}

/// 추가를 모두 마친 뒤 이동하는 경로의 판정 — (이동할까, 재생을 다시 정지할까).
pub fn settle_deferred_multi_live_navigation(
    menu_unchanged: bool,
    is_live_video_in_front: bool,
) -> (bool, bool) {
    if menu_unchanged {
        (true, false)
    } else {
        (false, !is_live_video_in_front)
    }
}

/// 실행취소 알림 한 건(토스트 문구와 되돌릴 채널).
#[derive(Debug, Clone, PartialEq)]
pub struct MultiLiveNotice {
    pub message: String,
    /// 실행취소로 되살릴 채널(원래 순서). 없으면 실행취소 없음.
    pub undo_channel_ids: Option<Vec<String>>,
    pub duration: Option<TimeInterval>,
}

/// 실행취소 토스트 표시 시간(초).
pub const UNDO_TOAST_DURATION: TimeInterval = 6.0;

/// 프리셋 적용 뒤의 알림. 원래 조합으로 돌아왔으면 `None`(바뀐 것이 없다).
pub fn preset_apply_notice(preset: &Preset, restore_ids: &[String]) -> Option<MultiLiveNotice> {
    if restore_ids == preset.channels.as_slice() {
        return None;
    }
    if restore_ids.is_empty() {
        return Some(MultiLiveNotice {
            message: format!(
                "'{}' 묶음을 열었습니다 (채널 {}개)",
                preset.name,
                preset.channels.len()
            ),
            undo_channel_ids: None,
            duration: None,
        });
    }
    Some(MultiLiveNotice {
        message: format!(
            "'{}' 묶음으로 바꿨습니다 (이전 {}개 해제)",
            preset.name,
            restore_ids.len()
        ),
        undo_channel_ids: Some(restore_ids.to_vec()),
        duration: Some(UNDO_TOAST_DURATION),
    })
}

/// 전체 해제 알림. 세션이 없으면 `None`(해제하지 않는다).
pub fn stop_all_notice(restore_ids: &[String]) -> Option<MultiLiveNotice> {
    if restore_ids.is_empty() {
        return None;
    }
    Some(MultiLiveNotice {
        message: format!("{}개 채널을 모두 해제했습니다", restore_ids.len()),
        undo_channel_ids: Some(restore_ids.to_vec()),
        duration: Some(UNDO_TOAST_DURATION),
    })
}

/// 세션 하나 종료 알림.
pub fn close_session_notice(channel_id: &str, channel_name: &str) -> MultiLiveNotice {
    let name = if channel_name.is_empty() {
        "채널"
    } else {
        channel_name
    };
    MultiLiveNotice {
        message: format!("{name} 세션을 종료했습니다"),
        undo_channel_ids: Some(vec![channel_id.to_string()]),
        duration: Some(UNDO_TOAST_DURATION),
    }
}

/// 조합 교체 티켓.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetApplyTicket {
    pub generation: u64,
    /// 겹친 사슬의 첫 적용 직전 조합 — 실행취소 기준.
    pub origin: Vec<String>,
}

#[derive(Default)]
struct GateState {
    generation: u64,
    chain_origin: Option<Vec<String>>,
    target: Option<Vec<String>>,
    in_flight: Option<Flight>,
}

/// 멀티라이브 '조합 교체'(프리셋 적용·그 실행취소)의 단일 비행 장부(`MultiLivePresetApplyGate`).
///
/// 새 요청은 세대를 올려 앞선 루프를 다음 체크포인트에서 멈추고, 그 루프가 빠질 때까지 기다린다.
/// `Clone` 은 같은 장부를 가리킨다.
#[derive(Clone, Default)]
pub struct MultiLivePresetApplyGate {
    state: Arc<Mutex<GateState>>,
}

impl MultiLivePresetApplyGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// `None` = 할 일 없음(이미 그 조합이거나, 같은 조합으로 진행 중인 교체가 있음).
    fn begin(
        &self,
        target: &[String],
        current: &[String],
    ) -> Option<(PresetApplyTicket, Option<Flight>)> {
        let mut st = lock(&self.state);
        if st.chain_origin.is_none() && current == target {
            return None;
        }
        if st.target.as_deref() == Some(target) {
            return None;
        }
        st.generation = st.generation.wrapping_add(1);
        let origin = st.chain_origin.clone().unwrap_or_else(|| current.to_vec());
        st.chain_origin = Some(origin.clone());
        st.target = Some(target.to_vec());
        Some((
            PresetApplyTicket {
                generation: st.generation,
                origin,
            },
            st.in_flight.clone(),
        ))
    }

    fn attach(&self, flight: Flight, ticket: &PresetApplyTicket) {
        let mut st = lock(&self.state);
        if st.generation == ticket.generation {
            st.in_flight = Some(flight);
        }
    }

    pub fn is_current(&self, ticket: &PresetApplyTicket) -> bool {
        lock(&self.state).generation == ticket.generation
    }

    fn finish(&self, ticket: &PresetApplyTicket) {
        let mut st = lock(&self.state);
        if st.generation != ticket.generation {
            return;
        }
        st.chain_origin = None;
        st.target = None;
        st.in_flight = None;
    }

    /// 조합 교체가 아닌 파괴적 조작(전체 해제)이 진행 중인 교체를 끊는다. `in_flight` 는 남겨 다음 교체가
    /// 끊긴 루프의 마지막 추가가 빠질 때까지 기다리게 한다.
    pub fn supersede(&self) {
        let mut st = lock(&self.state);
        st.generation = st.generation.wrapping_add(1);
        st.chain_origin = None;
        st.target = None;
    }
}

/// 멀티라이브 매니저의 조합 교체 표면.
#[async_trait]
pub trait MultiLiveComposition: Send + Sync {
    fn current_channel_ids(&self) -> Vec<String>;
    async fn stop_all(&self);
    /// 활성화 없이 추가한다(`activatesChannel: false`).
    async fn add_channel(&self, channel_id: &str);
}

/// 조합 교체(전체 해제 → 순차 추가)를 게이트 아래에서 수행한다. 끝까지 최신이었을 때만 티켓을 돌려준다.
/// 순차 추가여야 한다 — 동시 추가는 슬롯 예약이 경합한다.
pub async fn replace_multi_live_channels(
    gate: &MultiLivePresetApplyGate,
    host: Arc<dyn MultiLiveComposition>,
    ids: Vec<String>,
) -> Option<PresetApplyTicket> {
    let current = host.current_channel_ids();
    let (ticket, previous) = gate.begin(&ids, &current)?;
    let g = gate.clone();
    let t = ticket.clone();
    let handle = tokio::spawn(async move {
        // 앞선 루프가 다음 체크포인트에서 빠질 때까지 기다린다.
        if let Some(p) = previous {
            p.await;
        }
        if !g.is_current(&t) {
            return;
        }
        host.stop_all().await;
        for id in &ids {
            if !g.is_current(&t) {
                return;
            }
            host.add_channel(id).await;
        }
    });
    let flight: Flight = async move {
        let _ = handle.await;
    }
    .boxed()
    .shared();
    gate.attach(flight.clone(), &ticket);
    flight.await;
    if !gate.is_current(&ticket) {
        return None;
    }
    gate.finish(&ticket);
    Some(ticket)
}

/// 프리셋 적용 — 끝까지 최신이었고 조합이 실제로 바뀌었으면 알림을 돌려준다.
pub async fn apply_multi_live_preset(
    gate: &MultiLivePresetApplyGate,
    host: Arc<dyn MultiLiveComposition>,
    preset: &Preset,
) -> Option<MultiLiveNotice> {
    let ticket = replace_multi_live_channels(gate, host, preset.channels.clone()).await?;
    preset_apply_notice(preset, &ticket.origin)
}

// MARK: - Store recovery (+StoreRecovery)

/// 실패 화면에 보이는 열기 실패 요약 — 오류 domain·code 와 연속 횟수만(경로·메시지 없음).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreOpenFailureSummary {
    pub domain: String,
    pub code: i64,
    pub underlying_domain: Option<String>,
    pub underlying_code: Option<i64>,
    pub consecutive_failures: i64,
}

impl StoreOpenFailureSummary {
    /// '오류 코드 SQLite 1 (IO 13) · 연속 2회' 꼴의 한 줄.
    pub fn display_line(&self) -> String {
        let mut line = format!("오류 코드 {} {}", self.domain, self.code);
        if let (Some(d), Some(c)) = (&self.underlying_domain, self.underlying_code) {
            line.push_str(&format!(" ({d} {c})"));
        }
        format!("{line} · 연속 {}회", self.consecutive_failures)
    }

    /// 지원 보고 본문용 한 줄.
    pub fn support_line(&self) -> String {
        format!(
            "저장소 열기 실패 {}회 ({} {})",
            self.consecutive_failures, self.domain, self.code
        )
    }
}

/// `window_visibility`(E9) 한 표본 — 0/1 두 값만.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowVisibilitySample {
    pub visible: bool,
    pub active: bool,
}

impl WindowVisibilitySample {
    pub fn fields(&self) -> BTreeMap<&'static str, &'static str> {
        let bit = |b: bool| if b { "1" } else { "0" };
        BTreeMap::from([("visible", bit(self.visible)), ("active", bit(self.active))])
    }
}

/// 실패 뒤 읽는 복구 정보 묶음.
#[derive(Debug, Clone, PartialEq)]
pub struct StoreRecoveryInfo {
    pub failure: StoreOpenFailureSummary,
    pub backups: Vec<BackupEntry>,
}

/// 복구 정보 — store-health 의 마지막 오류(이번 실패가 방금 기록했다)가 있으면 그것, 없으면 이번 오류.
pub fn store_recovery_info(
    health: &StoreOpenHealth,
    error: ErrorSummary,
    backups: Vec<BackupEntry>,
) -> StoreRecoveryInfo {
    let recorded = if health.consecutive_failures > 0 {
        health.last_error.clone()
    } else {
        None
    };
    let summary = recorded.unwrap_or(error);
    StoreRecoveryInfo {
        failure: StoreOpenFailureSummary {
            domain: summary.domain,
            code: summary.code,
            underlying_domain: summary.underlying_domain,
            underlying_code: summary.underlying_code,
            consecutive_failures: health.consecutive_failures.max(1),
        },
        backups,
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests` 의 AppState 인증 경계·프로필 재동기화·로컬 삭제·프리셋 게이트·
    //! 저장소 복구 테스트 핵심.
    use super::*;
    use crate::services::channel_preset_store::PresetKind;
    use crate::services::support::MemoryKeyValueStore;
    use cview_core::utils::time::from_unix;

    fn state() -> AppState {
        AppState::new(MemoryKeyValueStore::shared())
    }

    #[test]
    fn sidebar_collapsed_persists() {
        let kv = MemoryKeyValueStore::shared();
        let mut s = AppState::new(kv.clone());
        assert!(!s.sidebar_collapsed());
        s.toggle_sidebar();
        assert!(kv.bool(SIDEBAR_COLLAPSED_KEY));
        assert!(!s.set_sidebar_collapsed(true), "같은 값은 쓰지 않는다");
        assert!(AppState::new(kv).sidebar_collapsed());
    }

    #[test]
    fn workspace_restore_only_applies_live_modes_without_explicit_navigation() {
        let s = state();
        let mut router = AppRouter::new(MemoryKeyValueStore::shared());
        let inputs = WorkspaceInputs {
            hub_mode: FollowingHubMode::Multi,
            multi_live_channel_ids: vec!["a".into()],
            multi_chat_channel_ids: vec![],
            smart_queue_channel_ids: vec!["q".into()],
        };
        let snap = s.capture_workspace_snapshot(Some(&router), &inputs, from_unix(1.0));
        assert_eq!(snap.sidebar.as_deref(), Some("홈"));
        let restore = workspace_restore(&snap, &router);
        assert_eq!(restore.hub_mode, Some(FollowingHubMode::Multi));
        assert_eq!(restore.smart_queue_channel_ids, Some(vec!["q".to_string()]));

        let mut old = snap.clone();
        old.hub_mode = Some("검색".into());
        assert_eq!(
            workspace_restore(&old, &router).hub_mode,
            None,
            "탐색 모드는 복원하지 않는다"
        );

        router.select_sidebar(SidebarItem::Search);
        assert_eq!(workspace_restore(&snap, &router).hub_mode, None);
    }

    #[test]
    fn live_section_routes_or_applies_mode() {
        let mut router = AppRouter::new(MemoryKeyValueStore::shared());
        assert_eq!(
            open_live_section(FollowingHubMode::Popular, &mut router),
            LiveSectionEffect::Routed
        );
        assert_eq!(router.selected_sidebar_item, SidebarItem::AllBroadcasts);

        assert_eq!(
            open_live_section(FollowingHubMode::Multi, &mut router),
            LiveSectionEffect::Routed
        );
        assert_eq!(router.selected_sidebar_item, SidebarItem::Following);
        assert_eq!(router.pending_hub_mode, Some(FollowingHubMode::Multi));
        assert_eq!(
            pending_live_hub_mode_before_enter(&mut router, FollowingHubMode::Explore),
            Some(FollowingHubMode::Multi)
        );
        assert_eq!(router.pending_hub_mode, None, "예약은 소비된다");

        assert_eq!(
            open_live_section(FollowingHubMode::Watch, &mut router),
            LiveSectionEffect::ApplyHubModePreset(FollowingHubMode::Watch)
        );
        assert_eq!(
            pending_live_hub_mode_before_enter(&mut router, FollowingHubMode::Search),
            Some(FollowingHubMode::Explore),
            "탐색 모드 잔재는 개요로 정규화"
        );
        assert_eq!(
            pending_live_hub_mode_before_enter(&mut router, FollowingHubMode::Watch),
            None
        );
    }

    #[test]
    fn auth_boundary_rotates_scope_and_clears_account_state() {
        let kv = MemoryKeyValueStore::shared();
        let mut s = AppState::new(kv.clone());
        let old = Uuid::new_v4();
        kv.set_string(VodSubscriptionTag::SCOPE_DEFAULTS_KEY, &scope_string(old));
        s.is_logged_in = true;
        s.user_nickname = Some("n".into());
        s.favorites_store.hydrate(vec!["c".to_string()]);
        s.profile_resync.arm(s.authentication_ui_epoch, 3, None);
        let before = s.authentication_ui_epoch;

        let b = s.begin_authentication_ui_transition(None);
        assert_ne!(b.epoch, before);
        assert!(!s.is_current_epoch(before));
        assert_eq!(b.retired_scope, Some(old));
        assert_eq!(
            kv.string(VodSubscriptionTag::SCOPE_DEFAULTS_KEY),
            Some(scope_string(b.scope))
        );
        assert_eq!(b.vod_subscription_tag, s.persisted_vod_subscription_tag());
        assert!(!s.is_logged_in && s.user_nickname.is_none());
        assert!(s.favorites_store.channel_ids().is_empty());
        assert!(!s.profile_resync.is_armed());
        assert!(!s.apply_login_result(before, true), "옛 에포크는 버린다");
        assert!(s.apply_login_result(b.epoch, true));
    }

    #[test]
    fn profile_application_rules() {
        let mut s = state();
        let epoch = s.authentication_ui_epoch;
        assert!(
            s.apply_user_status(epoch, Some("a"), None, None).is_none(),
            "비로그인"
        );
        s.is_logged_in = true;
        let id = s
            .apply_user_status(epoch, Some("a"), Some("http://img"), Some("hash"))
            .unwrap();
        assert_eq!(id.chat_uid.as_deref(), Some("hash"));
        assert_eq!(id.multi_live_uid, None);
        s.apply_user_status(epoch, Some("b"), Some("http://other"), None);
        assert_eq!(
            s.user_nickname.as_deref(),
            Some("a"),
            "쿠키 경로는 덮지 않는다"
        );
        assert_eq!(s.user_profile_url.as_deref(), Some("http://img"));
        assert!(!s.is_profile_incomplete(true) || s.user_channel_id.is_none());
        assert!(s.is_profile_incomplete(true), "OAuth 세션인데 채널 ID 없음");
        assert!(!s.is_profile_incomplete(false));
        let id = s
            .apply_oauth_profile(epoch, None, Some("ch"), None)
            .unwrap();
        assert_eq!(id.chat_uid.as_deref(), Some("ch"));
        assert_eq!(s.user_nickname.as_deref(), Some("a"));
        assert!(!s.is_profile_incomplete(true));
    }

    #[test]
    fn profile_resync_signals_once_and_rearms_with_spacing() {
        let epoch = Uuid::new_v4();
        let mut r = ProfileResync::default();
        assert_eq!(r.signal(0.0, epoch), None);
        r.arm(epoch, ProfileResync::DEFAULT_ATTEMPTS, None);
        assert_eq!(r.signal(0.0, Uuid::new_v4()), None, "에포크가 다르면 무시");
        let run = r.signal(0.0, epoch).unwrap();
        assert_eq!(r.signal(0.0, epoch), None, "한 번에 하나");
        assert!(r.rearm_after(run, true, epoch, 10.0));
        assert_eq!(r.signal(69.0, epoch), None, "간격 전");
        let run = r.signal(70.0, epoch).unwrap();
        assert_eq!(run.attempts, 2);
        assert!(!r.rearm_after(run, false, epoch, 70.0), "완성되면 멈춘다");
        assert!(!r.rearm_after(ResyncRun { epoch, attempts: 1 }, true, epoch, 0.0));
    }

    #[test]
    fn auth_follow_up_only_takes_oauth_refresh_and_final_expiry() {
        let oauth = AuthState::LoggedIn {
            user_id: "oauth".into(),
        };
        assert_eq!(auth_follow_up(false, &oauth), AuthFollowUp::Ignore);
        assert_eq!(
            auth_follow_up(true, &oauth),
            AuthFollowUp::FireProfileResyncSignal
        );
        assert_eq!(
            auth_follow_up(
                true,
                &AuthState::LoggedIn {
                    user_id: "cookie".into()
                }
            ),
            AuthFollowUp::Ignore
        );
        assert_eq!(
            auth_follow_up(true, &AuthState::Expired),
            AuthFollowUp::VerifyExpiry
        );
        assert!(should_end_expired_session(
            &AuthState::Expired,
            false,
            true,
            true
        ));
        assert!(!should_end_expired_session(
            &AuthState::Expired,
            true,
            true,
            true
        ));
        assert!(!should_end_expired_session(
            &AuthState::LoggedOut,
            false,
            true,
            true
        ));
    }

    struct FailingRows {
        fail_at: usize,
    }

    #[async_trait]
    impl LocalUserRows for FailingRows {
        async fn delete_all_watch_history(&self) -> Result<(), String> {
            (self.fail_at != 0).then_some(()).ok_or("h".into())
        }
        async fn clear_all_last_watched(&self) -> Result<(), String> {
            (self.fail_at != 1).then_some(()).ok_or("l".into())
        }
        async fn delete_all_vod_playback(&self) -> Result<(), String> {
            (self.fail_at != 2).then_some(()).ok_or("v".into())
        }
        async fn delete_all_clip_playback(&self) -> Result<(), String> {
            (self.fail_at != 3).then_some(()).ok_or("c".into())
        }
        async fn clear_all_favorites(&self) -> Result<(), String> {
            (self.fail_at != 4).then_some(()).ok_or("f".into())
        }
    }

    #[tokio::test]
    async fn local_clear_only_empties_mirrors_of_succeeded_steps() {
        let cleared = delete_local_user_rows(&FailingRows { fail_at: 2 }).await;
        assert_eq!(
            cleared,
            LocalUserDataClearance {
                history: true,
                last_watched: true,
                failure: Some("v".into()),
                ..Default::default()
            }
        );
        let mut s = state();
        s.favorites_store.hydrate(vec!["fav".to_string()]);
        let mut vod = VodPlaybackProgressStore::new();
        s.apply_local_user_data_clearance(&cleared, &mut vod);
        assert!(
            s.favorites_store.contains("fav"),
            "실패한 단계는 미러를 남긴다"
        );
        assert!(s.recent_channels_store.channel_ids().is_empty());
        assert_eq!(s.local_user_data_revision, 0);
        s.finish_local_user_data_clear();
        assert_eq!(s.local_user_data_revision, 1);

        let all = delete_local_user_rows(&FailingRows { fail_at: 9 }).await;
        assert!(all.favorites && all.failure.is_none());
    }

    #[test]
    fn reload_failures_keep_mirrors() {
        let mut s = state();
        s.apply_favorites_reload::<()>(Ok(vec!["a".into()]));
        s.apply_favorites_reload(Err("cancelled"));
        assert!(s.favorites_store.contains("a"));
        let generation = s.recent_channels_store.generation();
        assert_eq!(s.apply_recent_channels_reload(generation, Err(())), None);
        assert!(!s.recent_channels_store.did_hydrate());
        s.apply_saved_favorite("a", false);
        assert!(!s.favorites_store.contains("a"));
    }

    #[derive(Default)]
    struct RecordingSnapshots {
        log: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl AccountSnapshots for RecordingSnapshots {
        async fn remove_scope(&self, scope: &str) {
            tokio::task::yield_now().await;
            lock(&self.log).push(format!("remove:{scope}"));
        }
        async fn prune_accounts(&self, keeping: &[String]) {
            lock(&self.log).push(format!("prune:{}", keeping.join(",")));
        }
        async fn sweep(&self, keep: &[String]) {
            lock(&self.log).push(format!("sweep:{}", keep.join(",")));
        }
    }

    #[tokio::test]
    async fn account_cleanup_chain_keeps_order_and_skips_stale_prune() {
        let snaps = Arc::new(RecordingSnapshots::default());
        let chain = AccountSnapshotCleanup::new();
        let old = Uuid::new_v4();
        let new = Uuid::new_v4();
        let current: ScopeReader = Arc::new(move || Some(new));
        chain.retire(snaps.clone(), Some(old), Some(async {}.boxed()));
        chain.prune(snaps.clone(), old, current.clone());
        chain.prune(snaps.clone(), new, current.clone());
        chain.sweep(snaps.as_ref(), current).await;
        assert_eq!(
            *lock(&snaps.log),
            vec![
                format!("remove:{}", scope_string(old)),
                format!("prune:{}", scope_string(new)),
                format!("sweep:{}", scope_string(new)),
            ]
        );
        chain.sweep(snaps.as_ref(), Arc::new(|| None)).await;
        assert_eq!(lock(&snaps.log).len(), 3, "범위를 모르면 건너뛴다");
    }

    #[test]
    fn settle_rules() {
        let ok = MultiLiveAddOutcome {
            is_success: true,
            is_new_session: true,
            user_message: None,
        };
        let fx = settle_embedded_multi_live_add(&ok, Some(true), true, false);
        assert!(fx.activate_channel && fx.preserves_main_focus && fx.show_multi_live_panel);
        assert!(fx.release_video_surfaces);
        let fx = settle_embedded_multi_live_add(&ok, Some(false), true, true);
        assert!(!fx.activate_channel && !fx.show_multi_live_panel && !fx.is_current);
        let fx = settle_embedded_multi_live_add(&ok, None, false, true);
        assert!(!fx.activate_channel && fx.is_current);
        let err = MultiLiveAddOutcome {
            is_success: false,
            is_new_session: false,
            user_message: Some("실패".into()),
        };
        assert_eq!(
            settle_embedded_multi_live_add(&err, Some(true), false, true).error_message,
            Some("실패".into())
        );
        assert_eq!(
            settle_embedded_multi_live_add(&err, Some(false), false, true).error_message,
            None
        );
        assert_eq!(
            settle_deferred_multi_live_navigation(true, false),
            (true, false)
        );
        assert_eq!(
            settle_deferred_multi_live_navigation(false, false),
            (false, true)
        );
    }

    #[test]
    fn notices() {
        let p = Preset::new("묶음", vec!["a".into(), "b".into()], PresetKind::Live);
        assert_eq!(preset_apply_notice(&p, &p.channels), None);
        let n = preset_apply_notice(&p, &[]).unwrap();
        assert_eq!(n.message, "'묶음' 묶음을 열었습니다 (채널 2개)");
        assert!(n.undo_channel_ids.is_none());
        let n = preset_apply_notice(&p, &["x".into()]).unwrap();
        assert_eq!(n.message, "'묶음' 묶음으로 바꿨습니다 (이전 1개 해제)");
        assert_eq!(n.undo_channel_ids, Some(vec!["x".to_string()]));
        assert_eq!(stop_all_notice(&[]), None);
        assert_eq!(
            stop_all_notice(&["a".into(), "b".into()]).unwrap().message,
            "2개 채널을 모두 해제했습니다"
        );
        assert_eq!(
            close_session_notice("c", "").message,
            "채널 세션을 종료했습니다"
        );
    }

    /// 멀티라이브 매니저 가짜 — `slow` 채널의 추가는 `release` 가 열릴 때까지 멈춘다.
    struct FakeManager {
        sessions: Mutex<Vec<String>>,
        slow: String,
        release: tokio::sync::Notify,
        started_slow: tokio::sync::Notify,
    }

    impl FakeManager {
        fn new(initial: &[&str], slow: &str) -> Arc<Self> {
            Arc::new(Self {
                sessions: Mutex::new(initial.iter().map(|s| s.to_string()).collect()),
                slow: slow.into(),
                release: tokio::sync::Notify::new(),
                started_slow: tokio::sync::Notify::new(),
            })
        }
    }

    #[async_trait]
    impl MultiLiveComposition for FakeManager {
        fn current_channel_ids(&self) -> Vec<String> {
            lock(&self.sessions).clone()
        }
        async fn stop_all(&self) {
            lock(&self.sessions).clear();
        }
        async fn add_channel(&self, id: &str) {
            if id == self.slow {
                self.started_slow.notify_one();
                self.release.notified().await;
            }
            lock(&self.sessions).push(id.to_string());
        }
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[tokio::test]
    async fn gate_noop_when_already_that_composition() {
        let gate = MultiLivePresetApplyGate::new();
        let m = FakeManager::new(&["a"], "");
        assert_eq!(
            replace_multi_live_channels(&gate, m.clone(), ids(&["a"])).await,
            None
        );
        let t = replace_multi_live_channels(&gate, m.clone(), ids(&["b", "c"]))
            .await
            .unwrap();
        assert_eq!(t.origin, ids(&["a"]));
        assert_eq!(m.current_channel_ids(), ids(&["b", "c"]));
    }

    #[tokio::test]
    async fn overlapping_applies_do_not_mix_and_keep_chain_origin() {
        let gate = MultiLivePresetApplyGate::new();
        let m = FakeManager::new(&["orig"], "slow");
        let first = {
            let gate = gate.clone();
            let m = m.clone();
            tokio::spawn(
                async move { replace_multi_live_channels(&gate, m, ids(&["slow", "x"])).await },
            )
        };
        m.started_slow.notified().await;
        let second = {
            let gate = gate.clone();
            let m = m.clone();
            tokio::spawn(async move { replace_multi_live_channels(&gate, m, ids(&["y"])).await })
        };
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
        m.release.notify_one();
        assert_eq!(first.await.unwrap(), None, "대체된 적용은 티켓이 없다");
        let t = second.await.unwrap().unwrap();
        assert_eq!(
            t.origin,
            ids(&["orig"]),
            "실행취소 기준은 사슬의 첫 적용 직전"
        );
        assert_eq!(
            m.current_channel_ids(),
            ids(&["y"]),
            "앞 루프의 x 가 섞이지 않는다"
        );
    }

    #[tokio::test]
    async fn supersede_stops_running_apply() {
        let gate = MultiLivePresetApplyGate::new();
        let m = FakeManager::new(&[], "slow");
        let run = {
            let gate = gate.clone();
            let m = m.clone();
            tokio::spawn(
                async move { replace_multi_live_channels(&gate, m, ids(&["slow", "x"])).await },
            )
        };
        m.started_slow.notified().await;
        gate.supersede();
        m.release.notify_one();
        assert_eq!(run.await.unwrap(), None);
        assert_eq!(m.current_channel_ids(), ids(&["slow"]));
    }

    #[test]
    fn store_recovery_summary_and_restore_gate() {
        let mut health = StoreOpenHealth::default();
        let err = ErrorSummary {
            domain: "SQLite".into(),
            code: 1,
            underlying_domain: Some("IO".into()),
            underlying_code: Some(13),
            sqlite_code: None,
        };
        let info = store_recovery_info(&health, err.clone(), vec![]);
        assert_eq!(info.failure.consecutive_failures, 1);
        assert_eq!(
            info.failure.display_line(),
            "오류 코드 SQLite 1 (IO 13) · 연속 1회"
        );
        assert_eq!(
            info.failure.support_line(),
            "저장소 열기 실패 1회 (SQLite 1)"
        );
        health.consecutive_failures = 2;
        health.last_error = Some(ErrorSummary {
            domain: "Schema".into(),
            code: 9,
            underlying_domain: None,
            underlying_code: None,
            sqlite_code: None,
        });
        let info = store_recovery_info(&health, err, vec![]);
        assert_eq!(info.failure.display_line(), "오류 코드 Schema 9 · 연속 2회");

        let mut s = state();
        s.note_store_open_failure(info.clone());
        assert!(
            s.last_store_open_failure.is_none(),
            "실패 상태가 아니면 덮지 않는다"
        );
        assert!(!s.begin_store_restore());
        s.data_store_initialization_failed = true;
        s.note_store_open_failure(info);
        assert!(s.last_store_open_failure.is_some());
        assert!(s.begin_store_restore());
        assert!(!s.begin_store_restore(), "복원 중");
        assert!(!s.finish_store_restore_copy(false));
        assert!(s.store_restore_failed && !s.is_restoring_store_backup);
        assert!(s.begin_store_restore());
        assert!(s.finish_store_restore_copy(true));
        s.data_store_initialization_failed = false;
        s.is_settings_loaded = true;
        assert!(s.finish_store_restore());
        s.clear_store_recovery_state();
        assert!(s.last_store_open_failure.is_none());
        assert_eq!(
            WindowVisibilitySample {
                visible: true,
                active: false
            }
            .fields()["active"],
            "0"
        );
    }
}
