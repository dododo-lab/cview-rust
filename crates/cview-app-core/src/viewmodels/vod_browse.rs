//! `Sources/CViewApp/ViewModels/VODBrowseViewModel.swift` 포팅 — VOD 메뉴(팔로우한 채널·보관함·채널별·검색).
//!
//! 팔로우 피드는 로컬 우선이다. 화면은 계정 사본(`CachedVOD` 의 `subscriptions.‹scope›` 토큰이 붙은
//! 채널별 최신 행)으로만 피드를 세우고, 네트워크는 채널 단위로 사본을 갱신한다. 어느 채널을 언제 받을지는
//! [`VodSyncStore`] 와 [`VodSyncPlanner`] 가 정하고, 진입 5초 창의 요청은 [`VodEntryWindow`] 예산 안에 둔다.
//! 마지막 패스가 15분 안이면 요청 0, 오프라인·429 면 백오프.
//!
//! 보일 때만 이어받고([`VodBrowseViewModel::set_visible`]), 계정이 바뀌면 진행 중인 일을 버린다
//! ([`VodBrowseViewModel::invalidate`]).
//!
//! Swift 와 다른 점:
//! - `@MainActor` 직렬화 대신 상태는 [`Observed`] 안에 두고, 동기화 패스는 비동기 뮤텍스로 한 번에
//!   하나만 돈다. 원본은 "도는 진입 패스에 새로고침이 합류" 했지만 여기서는 앞 패스가 끝난 뒤 자기 패스를
//!   돈다(앞 패스가 기록을 갱신했으므로 대부분 due 0 으로 바로 끝난다).
//! - 이어받기 묶음을 숨김으로 끊으면 그 묶음의 결과는 반영하지 않는다(원본은 성공분만 반영).
//! - 계측(`vod_sync_pass` 캡처 줄·signpost)은 `tracing::debug!` 한 줄로 줄였다.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use futures_util::future::{BoxFuture, FutureExt, Shared};
use serde::Serialize;
use tokio::sync::watch;

use cview_core::models::channel::ChannelInfo;
use cview_core::models::search::VodInfo;
use cview_core::utils::time::{adding, Date, TimeInterval};
use cview_networking::ChzzkApiClient;
use cview_persistence::models::cached_vod::{CachedVod, CachedVodData};
use cview_persistence::models::playback::{PersistedVodPlaybackData, PersistedVodWatchLaterData};
use cview_persistence::store::vod_cache::SubscriptionVodPage;
use cview_persistence::DataStore;

use super::support::{cancel_slot, slot_is, Observed, ObservedGuard, TaskSlot};
use super::vod_library::{
    MutationReceipt, VodChannelApi, VodChannelBrowser, VodLibraryMutationQueue, VodLoadState,
};
use crate::services::support::{lock, system_clock, Clock};
use crate::services::vod_feed::{
    ChannelPageResult, VodFailureKind, VodFeedAggregator, VodPageSource, VodSubscriptionFeedCursor,
};
use crate::services::vod_playback_progress::VodPlaybackProgressStore;
use crate::services::vod_sync_state::{
    FollowSignal, VodSubscriptionTag, VodSyncMode, VodSyncPlanner, VodSyncPolicy, VodSyncState,
    VodSyncTier,
};
use crate::services::vod_sync_store::{VodEntryWindow, VodFollowDetails, VodSyncStore};

// MARK: - 탭

/// VOD 메뉴 상단 탭.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum VodTab {
    #[default]
    Subscriptions,
    Library,
    Channel,
    Search,
}

impl VodTab {
    pub const ALL: [VodTab; 4] = [
        Self::Subscriptions,
        Self::Library,
        Self::Channel,
        Self::Search,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Subscriptions => "팔로우한 채널",
            Self::Library => "보관함",
            Self::Channel => "채널별",
            Self::Search => "검색",
        }
    }
}

/// 보관함 세그먼트.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum LibrarySegment {
    #[default]
    ContinueWatching,
    WatchLater,
}

impl LibrarySegment {
    pub fn label(self) -> &'static str {
        match self {
            Self::ContinueWatching => "이어 보기",
            Self::WatchLater => "나중에 볼 VOD",
        }
    }
}

/// 이어받기 진행 — 팔로우 채널 `checked`/`total` 확인 중.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SyncProgress {
    pub checked: usize,
    pub total: usize,
}

/// 동기화 패스 한 번의 결과.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncOutcome {
    pub requested: usize,
    pub successes: usize,
    pub failures: usize,
    pub blocked: bool,
    pub follows: usize,
    pub budget: usize,
    pub planned: usize,
    pub due_left: usize,
    pub fresh_skip: bool,
}

impl SyncOutcome {
    /// 받은 것 없이 실패만 했거나 차단기가 멈춘 묶음.
    pub fn is_failed_batch(&self) -> bool {
        self.failures > 0 && (self.successes == 0 || self.requested < self.planned)
    }
}

/// 첫 로드 단계.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum InitialLoad {
    #[default]
    Idle,
    ReadingLocal,
    Local,
    Syncing,
    Synced,
}

// MARK: - 의존성

/// VOD 검색 API.
#[async_trait]
pub trait VodSearchApi: Send + Sync {
    /// `(항목, 다음 페이지 있음)`.
    async fn search_videos(
        &self,
        keyword: &str,
        offset: i64,
        size: i64,
    ) -> Result<(Vec<VodInfo>, bool), String>;
}

#[async_trait]
impl VodSearchApi for ChzzkApiClient {
    async fn search_videos(
        &self,
        keyword: &str,
        offset: i64,
        size: i64,
    ) -> Result<(Vec<VodInfo>, bool), String> {
        let r = ChzzkApiClient::search_videos(self, keyword, offset, size)
            .await
            .map_err(|e| e.to_string())?;
        let has_next = r.page.and_then(|p| p.next).is_some();
        Ok((r.data, has_next))
    }
}

pub type ChannelIdsProvider = Arc<dyn Fn() -> Vec<String> + Send + Sync>;
pub type TagProvider = Arc<dyn Fn() -> Option<String> + Send + Sync>;
pub type FollowDetailsProvider = Arc<dyn Fn() -> VodFollowDetails + Send + Sync>;
pub type SleepFn = Arc<dyn Fn(Duration) -> BoxFuture<'static, ()> + Send + Sync>;

/// [`VodBrowseViewModel`] 의존성.
#[derive(Clone)]
pub struct VodBrowseDeps {
    pub page_source: Arc<dyn VodPageSource>,
    pub search: Arc<dyn VodSearchApi>,
    pub channel_api: Arc<dyn VodChannelApi>,
    pub data_store: Option<DataStore>,
    /// 계정 태그가 있을 때 쓰는 동기화 기록(앱은 공용 기록을 넘긴다). `None` 이면 메모리 전용.
    pub sync_store: Option<VodSyncStore>,
    pub subscription_tag: Option<TagProvider>,
    pub follow_details: Option<FollowDetailsProvider>,
    pub channel_ids: ChannelIdsProvider,
    pub progress_mirror: Option<Arc<Mutex<VodPlaybackProgressStore>>>,
    pub clock: Clock,
    pub sleep: SleepFn,
}

impl VodBrowseDeps {
    /// 실제 API 클라이언트로 만든 기본 의존성.
    pub fn with_api(api: Arc<ChzzkApiClient>, channel_ids: ChannelIdsProvider) -> Self {
        Self {
            page_source: api.clone(),
            search: api.clone(),
            channel_api: api,
            data_store: None,
            sync_store: None,
            subscription_tag: None,
            follow_details: None,
            channel_ids,
            progress_mirror: None,
            clock: system_clock(),
            sleep: Arc::new(|d| tokio::time::sleep(d).boxed()),
        }
    }
}

// MARK: - 상태

type Joinable = Shared<BoxFuture<'static, ()>>;

fn spawn_joinable<F>(fut: F) -> Joinable
where
    F: Future<Output = ()> + Send + 'static,
{
    let handle = tokio::spawn(fut);
    async move {
        let _ = handle.await;
    }
    .boxed()
    .shared()
}

#[derive(Default)]
pub struct VodBrowseState {
    pub selected_tab: VodTab,
    pub library_segment: LibrarySegment,
    pub feed: Vec<VodInfo>,
    pub state: VodLoadState,
    pub is_loading_more: bool,
    pub can_load_more: bool,
    pub preview_vod: Option<VodInfo>,
    pub subscriptions_page: usize,
    pub continue_watching: Vec<PersistedVodPlaybackData>,
    pub library_state: VodLoadState,
    pub watch_later: Vec<PersistedVodWatchLaterData>,
    pub watch_later_state: VodLoadState,
    pub watch_later_ids: HashSet<i64>,
    pub library_mutation_error: Option<String>,
    pub search_query: String,
    pub search_results: Vec<VodInfo>,
    pub search_state: VodLoadState,
    pub search_has_more: bool,
    pub is_loading_more_search: bool,
    pub sync_progress: Option<SyncProgress>,
    pub sync_backoff_until: Option<Date>,
    // 내부
    continue_removal_ids: HashSet<i64>,
    library_mutation_generation: u64,
    continue_mutation_generation: u64,
    search_offset: i64,
    search_task: Option<TaskSlot>,
    search_generation: u64,
    cursor: VodSubscriptionFeedCursor,
    subscription_generation: u64,
    local_rows: HashMap<String, Vec<VodInfo>>,
    local_exhausted: HashSet<String>,
    local_rows_tag: Option<String>,
    local_rows_loaded: bool,
    entry_window: VodEntryWindow,
    has_pending_rebuild: bool,
    is_page_active: bool,
    initial_load: InitialLoad,
    local_load_task: Option<Joinable>,
    initial_load_task: Option<Joinable>,
    invalidation_epoch: u64,
    continuation_task: Option<TaskSlot>,
    continuation_running: Option<Joinable>,
    reactivation_task: Option<Joinable>,
    persist_chain: Option<Joinable>,
    sync_running_mode: Option<VodSyncMode>,
}

impl VodBrowseState {
    pub const VOD_PER_PAGE: usize = 20;

    pub fn current_page_items(&self) -> &[VodInfo] {
        let start = self.subscriptions_page * Self::VOD_PER_PAGE;
        if start >= self.feed.len() {
            return &[];
        }
        &self.feed[start..(start + Self::VOD_PER_PAGE).min(self.feed.len())]
    }

    /// 페이지 바가 그릴 총 페이지 수 — 더 받을 수 있으면 한 페이지를 더 센다.
    pub fn subscriptions_page_count(&self) -> usize {
        let loaded = self.feed.len().div_ceil(Self::VOD_PER_PAGE).max(1);
        if self.can_load_more {
            loaded + 1
        } else {
            loaded
        }
    }

    pub fn has_previous_page(&self) -> bool {
        self.subscriptions_page > 0
    }

    pub fn has_next_page(&self) -> bool {
        (self.subscriptions_page + 1) * Self::VOD_PER_PAGE < self.feed.len() || self.can_load_more
    }

    pub fn is_visible(&self) -> bool {
        self.is_page_active && self.selected_tab == VodTab::Subscriptions
    }

    pub fn has_local_feed(&self) -> bool {
        matches!(
            self.initial_load,
            InitialLoad::Local | InitialLoad::Syncing | InitialLoad::Synced
        )
    }

    pub fn entry_window(&self) -> &VodEntryWindow {
        &self.entry_window
    }

    pub fn local_rows(&self) -> &HashMap<String, Vec<VodInfo>> {
        &self.local_rows
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VodBrowseSnapshot {
    pub selected_tab: VodTab,
    pub library_segment: LibrarySegment,
    pub feed: Vec<VodInfo>,
    pub page_items: Vec<VodInfo>,
    pub state: VodLoadState,
    pub is_loading_more: bool,
    pub can_load_more: bool,
    pub subscriptions_page: usize,
    pub page_count: usize,
    pub has_previous_page: bool,
    pub has_next_page: bool,
    pub library_state: VodLoadState,
    pub continue_watching: Vec<LibraryEntryDto>,
    pub watch_later_state: VodLoadState,
    pub watch_later: Vec<LibraryEntryDto>,
    pub watch_later_ids: Vec<i64>,
    pub library_mutation_error: Option<String>,
    pub search_query: String,
    pub search_results: Vec<VodInfo>,
    pub search_state: VodLoadState,
    pub search_has_more: bool,
    pub sync_progress: Option<SyncProgress>,
}

/// 보관함 행.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryEntryDto {
    pub video_no: i64,
    pub video_title: String,
    pub channel_name: String,
    pub thumbnail_url: Option<String>,
    pub duration_sec: f64,
    pub position_sec: Option<f64>,
}

// MARK: - 뷰모델

#[derive(Clone)]
pub struct VodBrowseViewModel {
    state: Arc<Observed<VodBrowseState>>,
    deps: VodBrowseDeps,
    aggregator: Arc<VodFeedAggregator>,
    channel_browser: VodChannelBrowser,
    sync_store: VodSyncStore,
    untagged_sync_store: VodSyncStore,
    mutations: Arc<VodLibraryMutationQueue>,
    pass_lock: Arc<tokio::sync::Mutex<()>>,
}

impl VodBrowseViewModel {
    /// 한 번에 피드에 붙이는 항목 수.
    pub const PAGE_SIZE: usize = 24;
    /// 채널당 사본 상한.
    pub const PER_CHANNEL_LIMIT: usize = 24;
    pub const SYNC_STOP_AFTER_FAILURES: usize = 6;
    pub const MINIMUM_BACKOFF: TimeInterval = 60.0;
    pub const CONTINUATION_DELAY: Duration = Duration::from_secs(8);
    pub const CONTINUATION_INTERVAL: Duration = Duration::from_secs(10);
    pub const CONTINUATION_BATCH_SIZE: usize = 24;
    pub const CONTINUATION_CONCURRENCY: usize = 2;
    pub const CONTINUATION_STOP_AFTER_FAILURES: usize = 3;
    pub const CONTINUATION_MAX_FAILED_BATCHES: usize = 2;
    const SEARCH_PAGE_SIZE: i64 = 24;

    pub fn new(deps: VodBrowseDeps) -> Self {
        let sync_store = deps
            .sync_store
            .clone()
            .unwrap_or_else(|| VodSyncStore::new(None, Some(deps.clock.clone())));
        Self {
            state: Arc::new(Observed::new(VodBrowseState::default())),
            aggregator: Arc::new(VodFeedAggregator::new(deps.page_source.clone())),
            channel_browser: VodChannelBrowser::new(deps.channel_api.clone()),
            untagged_sync_store: VodSyncStore::new(None, Some(deps.clock.clone())),
            sync_store,
            deps,
            mutations: Arc::new(VodLibraryMutationQueue::new()),
            pass_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub fn read(&self) -> ObservedGuard<'_, VodBrowseState> {
        self.state.read()
    }

    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.state.subscribe()
    }

    pub fn channel_browser(&self) -> &VodChannelBrowser {
        &self.channel_browser
    }

    fn now(&self) -> Date {
        (self.deps.clock)()
    }

    fn channel_ids(&self) -> Vec<String> {
        (self.deps.channel_ids)()
    }

    fn current_tag(&self) -> Option<String> {
        self.deps.subscription_tag.as_ref().and_then(|p| p())
    }

    fn follow_details(&self) -> VodFollowDetails {
        self.deps
            .follow_details
            .as_ref()
            .map(|p| p())
            .unwrap_or_default()
    }

    fn epoch(&self) -> u64 {
        self.state.read().invalidation_epoch
    }

    fn is_network_allowed(&self) -> bool {
        let until = self.state.read().sync_backoff_until;
        until.is_none_or(|u| self.now() >= u)
    }

    pub fn snapshot(&self) -> VodBrowseSnapshot {
        let s = self.state.read();
        let mut ids: Vec<i64> = s.watch_later_ids.iter().copied().collect();
        ids.sort_unstable();
        VodBrowseSnapshot {
            selected_tab: s.selected_tab,
            library_segment: s.library_segment,
            feed: s.feed.clone(),
            page_items: s.current_page_items().to_vec(),
            state: s.state.clone(),
            is_loading_more: s.is_loading_more,
            can_load_more: s.can_load_more,
            subscriptions_page: s.subscriptions_page,
            page_count: s.subscriptions_page_count(),
            has_previous_page: s.has_previous_page(),
            has_next_page: s.has_next_page(),
            library_state: s.library_state.clone(),
            continue_watching: s
                .continue_watching
                .iter()
                .map(|r| LibraryEntryDto {
                    video_no: r.video_no,
                    video_title: r.video_title.clone(),
                    channel_name: r.channel_name.clone(),
                    thumbnail_url: r.thumbnail_url_string.clone(),
                    duration_sec: r.duration_sec,
                    position_sec: Some(r.last_position_sec),
                })
                .collect(),
            watch_later_state: s.watch_later_state.clone(),
            watch_later: s
                .watch_later
                .iter()
                .map(|r| LibraryEntryDto {
                    video_no: r.video_no,
                    video_title: r.video_title.clone(),
                    channel_name: r.channel_name.clone(),
                    thumbnail_url: r.thumbnail_url_string.clone(),
                    duration_sec: r.duration_sec,
                    position_sec: None,
                })
                .collect(),
            watch_later_ids: ids,
            library_mutation_error: s.library_mutation_error.clone(),
            search_query: s.search_query.clone(),
            search_results: s.search_results.clone(),
            search_state: s.search_state.clone(),
            search_has_more: s.search_has_more,
            sync_progress: s.sync_progress,
        }
    }

    async fn blocking<T, F>(&self, f: F) -> Option<T>
    where
        T: Send + 'static,
        F: FnOnce(DataStore) -> T + Send + 'static,
    {
        let store = self.deps.data_store.clone()?;
        tokio::task::spawn_blocking(move || f(store)).await.ok()
    }

    // MARK: 탭

    pub async fn select_tab(&self, tab: VodTab) {
        self.change_tab(tab);
        if tab == VodTab::Library {
            let seg = self.state.read().library_segment;
            self.select_library_segment(seg).await;
        }
    }

    pub async fn select_library_segment(&self, seg: LibrarySegment) {
        self.state.lock().library_segment = seg;
        match seg {
            LibrarySegment::ContinueWatching => self.load_continue_watching().await,
            LibrarySegment::WatchLater => self.load_watch_later().await,
        }
    }

    pub async fn refresh_current_tab(&self) {
        let (tab, seg, query) = {
            let s = self.state.read();
            (s.selected_tab, s.library_segment, s.search_query.clone())
        };
        match tab {
            VodTab::Subscriptions => self.reload().await,
            VodTab::Library => self.select_library_segment(seg).await,
            VodTab::Channel => self.channel_browser.refresh().await,
            VodTab::Search => {
                let q = query.trim().to_string();
                if q.chars().count() >= 2 {
                    self.perform_search(q).await;
                }
            }
        }
    }

    pub fn set_preview(&self, vod: Option<VodInfo>) {
        self.state.lock().preview_vod = vod;
    }

    pub fn clear_library_mutation_error(&self) {
        self.state.lock().library_mutation_error = None;
    }

    // MARK: 보관함 — 이어 보기

    pub async fn load_continue_watching(&self) {
        let generation = self.state.read().continue_mutation_generation;
        if self.deps.data_store.is_none() {
            let mut s = self.state.lock();
            s.continue_watching.clear();
            s.library_state = VodLoadState::Empty;
            return;
        }
        {
            let mut s = self.state.lock();
            if s.continue_watching.is_empty() {
                s.library_state = VodLoadState::Loading;
            }
        }
        let result = self.blocking(|st| st.fetch_recent_vod_playback(30)).await;
        let mut s = self.state.lock();
        if s.continue_mutation_generation != generation {
            return;
        }
        match result {
            None => {}
            Some(Ok(items)) => {
                s.library_state = if items.is_empty() {
                    VodLoadState::Empty
                } else {
                    VodLoadState::Loaded
                };
                s.continue_watching = items;
            }
            Some(Err(e)) => {
                tracing::error!("이어 보기 조회 실패: {e}");
                s.library_state = VodLoadState::Error("보관함을 불러오지 못했습니다".into());
            }
        }
    }

    /// 이어 보기에서 제거(시청 완료 처리).
    pub async fn remove_from_continue_watching(&self, video_no: i64) {
        if self.deps.data_store.is_none()
            || !self.state.lock().continue_removal_ids.insert(video_no)
        {
            return;
        }
        let result = self
            .blocking(move |st| st.remove_vod_playback(video_no))
            .await;
        let mut s = self.state.lock();
        s.continue_removal_ids.remove(&video_no);
        match result {
            Some(Ok(())) => {
                s.continue_mutation_generation = s.continue_mutation_generation.wrapping_add(1);
                if let Some(m) = &self.deps.progress_mirror {
                    lock(m).clear(video_no);
                }
                s.continue_watching.retain(|r| r.video_no != video_no);
                if s.continue_watching.is_empty() {
                    s.library_state = VodLoadState::Empty;
                }
            }
            _ => {
                s.library_mutation_error =
                    Some("이어 보기에서 제거하지 못했습니다. 다시 시도해 주세요.".into());
            }
        }
    }

    // MARK: 보관함 — 나중에 볼 VOD

    pub async fn load_watch_later(&self) {
        if self.mutations.has_pending() {
            return;
        }
        let generation = self.state.read().library_mutation_generation;
        if self.deps.data_store.is_none() {
            let mut s = self.state.lock();
            s.watch_later.clear();
            s.watch_later_state = VodLoadState::Empty;
            return;
        }
        {
            let mut s = self.state.lock();
            if s.watch_later.is_empty() {
                s.watch_later_state = VodLoadState::Loading;
            }
        }
        let result = self.blocking(|st| st.fetch_vod_watch_later(100)).await;
        let mut s = self.state.lock();
        if s.library_mutation_generation != generation || self.mutations.has_pending() {
            return;
        }
        match result {
            Some(Ok(items)) => {
                s.watch_later_ids = items.iter().map(|i| i.video_no).collect();
                s.watch_later_state = if items.is_empty() {
                    VodLoadState::Empty
                } else {
                    VodLoadState::Loaded
                };
                s.watch_later = items;
            }
            Some(Err(e)) => {
                tracing::error!("나중에 볼 조회 실패: {e}");
                s.watch_later_state = VodLoadState::Error("보관함을 불러오지 못했습니다".into());
            }
            None => {}
        }
    }

    /// 카드 토글 상태용 id 집합만 가볍게 로드.
    pub async fn load_watch_later_ids(&self) {
        if self.mutations.has_pending() || self.deps.data_store.is_none() {
            return;
        }
        let generation = self.state.read().library_mutation_generation;
        let ids = self
            .blocking(|st| st.fetch_vod_watch_later_ids())
            .await
            .and_then(Result::ok)
            .unwrap_or_default();
        let mut s = self.state.lock();
        if s.library_mutation_generation != generation || self.mutations.has_pending() {
            return;
        }
        s.watch_later_ids = ids.into_iter().collect();
    }

    pub fn is_in_watch_later(&self, video_no: i64) -> bool {
        self.state.read().watch_later_ids.contains(&video_no)
    }

    /// 나중에 볼 목록에 담기/빼기 토글.
    pub async fn toggle_watch_later(&self, vod: &VodInfo) {
        let Some(store) = self.deps.data_store.clone() else {
            return;
        };
        let no = vod.video_no;
        let (initially_saved, desired) = {
            let mut s = self.state.lock();
            s.library_mutation_generation = s.library_mutation_generation.wrapping_add(1);
            let initially = s.watch_later_ids.contains(&no);
            if initially {
                s.watch_later_ids.remove(&no);
            } else {
                s.watch_later_ids.insert(no);
            }
            (initially, !initially)
        };
        let title = vod.video_title.clone();
        let channel_name = vod
            .channel
            .as_ref()
            .map(|c| c.channel_name.clone())
            .unwrap_or_default();
        let thumbnail = vod.video_image_url.as_ref().map(|u| u.to_string());
        let duration = vod.duration as f64;
        let write = async move {
            tokio::task::spawn_blocking(move || {
                if desired {
                    store.add_vod_watch_later(
                        no,
                        &title,
                        &channel_name,
                        thumbnail.as_deref(),
                        duration,
                    )
                } else {
                    store.remove_vod_watch_later(no)
                }
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())
        };
        let receipt = self
            .mutations
            .apply(no, initially_saved, desired, write)
            .await;
        self.commit_watch_later_mutation(no, receipt).await;
    }

    /// 나중에 볼 목록에서 제거.
    pub async fn remove_from_watch_later(&self, video_no: i64) {
        let Some(store) = self.deps.data_store.clone() else {
            return;
        };
        let initially_saved = {
            let mut s = self.state.lock();
            s.library_mutation_generation = s.library_mutation_generation.wrapping_add(1);
            let initially = s.watch_later_ids.contains(&video_no);
            s.watch_later_ids.remove(&video_no);
            initially
        };
        let write = async move {
            tokio::task::spawn_blocking(move || store.remove_vod_watch_later(video_no))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())
        };
        let receipt = self
            .mutations
            .apply(video_no, initially_saved, false, write)
            .await;
        self.commit_watch_later_mutation(video_no, receipt).await;
    }

    async fn commit_watch_later_mutation(&self, video_no: i64, receipt: MutationReceipt) {
        if !self.mutations.is_current(video_no, receipt.token) {
            return;
        }
        let reload = {
            let mut s = self.state.lock();
            s.library_mutation_generation = s.library_mutation_generation.wrapping_add(1);
            let outcome = &receipt.outcome;
            if outcome.is_member {
                s.watch_later_ids.insert(video_no);
            } else {
                s.watch_later_ids.remove(&video_no);
            }
            if outcome.error.is_some() {
                s.library_mutation_error =
                    Some("나중에 볼 목록을 저장하지 못했습니다. 다시 시도해 주세요.".into());
            }
            if !outcome.is_member {
                s.watch_later.retain(|r| r.video_no != video_no);
                if s.watch_later.is_empty() {
                    s.watch_later_state = VodLoadState::Empty;
                }
            }
            s.library_segment == LibrarySegment::WatchLater
        };
        self.mutations.finish(video_no, receipt.token);
        if reload {
            self.load_watch_later().await;
        }
    }

    /// 미리보기의 "이 채널의 VOD 보기" — 채널 탭으로 전환 후 로드.
    pub fn show_channel_vods(&self, channel_id: &str) {
        self.state.lock().preview_vod = None;
        self.change_tab(VodTab::Channel);
        if tokio::runtime::Handle::try_current().is_ok() {
            drop(self.channel_browser.select(channel_id));
        }
    }

    // MARK: 검색

    /// 검색어 변경 — 300ms 디바운스 후 자동 검색(2자 이상).
    pub fn update_search_query(&self, query: &str) {
        let mut s = self.state.lock();
        s.search_generation = s.search_generation.wrapping_add(1);
        s.search_query = query.to_string();
        cancel_slot(&mut s.search_task);
        let trimmed = query.trim().to_string();
        if trimmed.chars().count() < 2 {
            s.search_results.clear();
            s.search_state = VodLoadState::Idle;
            s.search_has_more = false;
            return;
        }
        let me = self.clone();
        s.search_task = Some(TaskSlot::spawn(move |id| async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            if !slot_is(&me.state.read().search_task, id) {
                return;
            }
            me.perform_search(trimmed).await;
        }));
    }

    async fn perform_search(&self, keyword: String) {
        let generation = {
            let mut s = self.state.lock();
            if s.search_results.is_empty() {
                s.search_state = VodLoadState::Loading;
            }
            s.search_generation
        };
        let result = self
            .deps
            .search
            .search_videos(&keyword, 0, Self::SEARCH_PAGE_SIZE)
            .await;
        let mut s = self.state.lock();
        if s.search_generation != generation {
            return;
        }
        match result {
            Ok((items, has_next)) => {
                s.search_offset = items.len() as i64;
                s.search_has_more = has_next && !items.is_empty();
                s.search_state = if items.is_empty() {
                    VodLoadState::Empty
                } else {
                    VodLoadState::Loaded
                };
                s.search_results = items;
            }
            Err(e) => s.search_state = VodLoadState::Error(e),
        }
    }

    pub async fn load_more_search(&self) {
        let (keyword, offset, generation) = {
            let mut s = self.state.lock();
            if !s.search_has_more
                || s.is_loading_more_search
                || s.search_state != VodLoadState::Loaded
            {
                return;
            }
            let keyword = s.search_query.trim().to_string();
            if keyword.chars().count() < 2 {
                return;
            }
            s.is_loading_more_search = true;
            (keyword, s.search_offset, s.search_generation)
        };
        let result = self
            .deps
            .search
            .search_videos(&keyword, offset, Self::SEARCH_PAGE_SIZE)
            .await;
        let mut s = self.state.lock();
        s.is_loading_more_search = false;
        let Ok((items, has_next)) = result else {
            return;
        };
        if s.search_generation != generation {
            return;
        }
        let existing: HashSet<i64> = s.search_results.iter().map(|v| v.video_no).collect();
        s.search_offset += items.len() as i64;
        s.search_has_more = has_next && !items.is_empty();
        s.search_results.extend(
            items
                .into_iter()
                .filter(|v| !existing.contains(&v.video_no)),
        );
    }

    // MARK: 채널별

    pub async fn resolve_and_load_channel(&self) {
        self.channel_browser.resolve_and_load_channel().await;
    }

    pub async fn load_more_channel(&self) {
        self.channel_browser.load_more_channel().await;
    }

    // MARK: 구독 VOD — 첫 로드

    /// 최초 1회 로드(이미 로드됐으면 무시, 도는 중이면 그 로드에 합류).
    pub async fn load_initial_if_needed(&self) {
        let running = {
            let mut s = self.state.lock();
            if let Some(r) = s.initial_load_task.clone() {
                Some(r)
            } else if matches!(s.initial_load, InitialLoad::Syncing | InitialLoad::Synced) {
                return;
            } else {
                let epoch = s.invalidation_epoch;
                let me = self.clone();
                let task = spawn_joinable(async move { me.perform_initial_load(epoch).await });
                s.initial_load_task = Some(task.clone());
                drop(s);
                task.await;
                let mut s = self.state.lock();
                if s.invalidation_epoch == epoch {
                    s.initial_load_task = None;
                }
                return;
            }
        };
        if let Some(r) = running {
            r.await;
        }
    }

    async fn perform_initial_load(&self, epoch: u64) {
        self.load_local_if_needed().await;
        {
            let mut s = self.state.lock();
            if s.invalidation_epoch != epoch || s.initial_load != InitialLoad::Local {
                return;
            }
            s.initial_load = InitialLoad::Syncing;
        }
        let ids_load = self.load_watch_later_ids();
        let continue_load = self.load_continue_watching();
        let sync = self.sync_and_rebuild(VodSyncMode::Entry);
        futures_util::join!(ids_load, continue_load, sync);
        if self.epoch() != epoch {
            return;
        }
        self.wait_for_follow_list(epoch).await;
        {
            let mut s = self.state.lock();
            if s.invalidation_epoch != epoch {
                return;
            }
            s.initial_load = InitialLoad::Synced;
        }
        self.schedule_continuation_if_needed();
    }

    /// 계정 사본만으로(네트워크 없이) 첫 화면을 세운다.
    pub async fn load_local_if_needed(&self) {
        let task = {
            let mut s = self.state.lock();
            if let Some(r) = s.local_load_task.clone() {
                r
            } else {
                if s.initial_load != InitialLoad::Idle {
                    return;
                }
                s.initial_load = InitialLoad::ReadingLocal;
                s.state = VodLoadState::Loading;
                let epoch = s.invalidation_epoch;
                let me = self.clone();
                let task = spawn_joinable(async move { me.read_local_feed(epoch).await });
                s.local_load_task = Some(task.clone());
                task
            }
        };
        task.await;
        let mut s = self.state.lock();
        if s.initial_load != InitialLoad::ReadingLocal {
            s.local_load_task = None;
        }
    }

    async fn read_local_feed(&self, epoch: u64) {
        let count = self.channel_ids().len() as i64;
        let now = self.now();
        self.state.lock().entry_window.begin(count, now);
        self.show_local_feed().await;
        let mut s = self.state.lock();
        if s.invalidation_epoch == epoch {
            s.initial_load = InitialLoad::Local;
        }
    }

    /// 콜드 레이스 — 팔로우 목록이 아직 비었으면 750ms 간격으로 최대 8번 기다린다.
    async fn wait_for_follow_list(&self, epoch: u64) {
        if !self.channel_ids().is_empty() {
            return;
        }
        for _ in 0..8 {
            (self.deps.sleep)(Duration::from_millis(750)).await;
            if self.epoch() != epoch {
                return;
            }
            let ids = self.channel_ids();
            if !ids.is_empty() {
                let now = self.now();
                self.state.lock().entry_window.begin(ids.len() as i64, now);
                self.prepare_subscriptions().await;
                if self.epoch() != epoch {
                    return;
                }
                if self.state.read().subscriptions_page == 0 {
                    self.rebuild_feed(Self::PAGE_SIZE, false, false, VodLoadState::Loading)
                        .await;
                }
                self.sync_and_rebuild(VodSyncMode::Entry).await;
                return;
            }
        }
    }

    /// 피드 새로고침 — 새 진입 창 안에서 refresh 모드 패스를 돈다.
    pub async fn reload(&self) {
        let epoch = {
            let mut s = self.state.lock();
            s.subscriptions_page = 0;
            s.invalidation_epoch
        };
        if self.state.read().sync_running_mode.is_none() {
            let count = self.channel_ids().len() as i64;
            let now = self.now();
            self.state.lock().entry_window.begin(count, now);
        }
        self.prepare_subscriptions().await;
        if self.epoch() != epoch {
            return;
        }
        if self.state.read().feed.is_empty() {
            self.rebuild_feed(Self::PAGE_SIZE, false, false, VodLoadState::Loading)
                .await;
            if self.epoch() != epoch {
                return;
            }
        }
        self.sync_and_rebuild(VodSyncMode::Refresh).await;
    }

    /// 더 보기 — 받아 둔 항목과 떨어진 채널의 다음 페이지로 신규 항목을 붙인다.
    pub async fn load_more(&self) {
        let generation = {
            let mut s = self.state.lock();
            if !s.can_load_more
                || s.is_loading_more
                || s.state != VodLoadState::Loaded
                || !s.cursor.has_more()
            {
                return;
            }
            s.is_loading_more = true;
            s.subscription_generation
        };
        let rounds = if self.is_network_allowed() { 3 } else { 0 };
        let next = self
            .drain_subscriptions(Self::PAGE_SIZE, rounds, generation, true)
            .await;
        let mut s = self.state.lock();
        s.is_loading_more = false;
        let Some(next) = next else {
            return;
        };
        let mut existing: HashSet<i64> = s.feed.iter().map(|v| v.video_no).collect();
        s.feed
            .extend(next.into_iter().filter(|v| existing.insert(v.video_no)));
        s.can_load_more = s.cursor.has_more();
    }

    /// 지정 페이지로 이동. 데이터가 부족하면 먼저 더 받는다.
    pub async fn go_to_page(&self, page: usize) {
        let (pending, loading) = {
            let s = self.state.read();
            (s.has_pending_rebuild, s.is_loading_more)
        };
        if page == 0 && pending && !loading {
            let allow = self.is_network_allowed();
            self.rebuild_feed(Self::PAGE_SIZE, allow, true, VodLoadState::Empty)
                .await;
            return;
        }
        let target_start = page * VodBrowseState::VOD_PER_PAGE;
        let need_more = {
            let s = self.state.read();
            target_start >= s.feed.len() && s.can_load_more && !s.is_loading_more
        };
        if need_more {
            self.load_more().await;
        }
        let mut s = self.state.lock();
        if target_start < s.feed.len() {
            s.subscriptions_page = page;
        }
    }

    pub async fn next_subscriptions_page(&self) {
        let page = self.state.read().subscriptions_page + 1;
        self.go_to_page(page).await;
    }

    pub fn previous_subscriptions_page(&self) {
        let rebuild = {
            let mut s = self.state.lock();
            if !s.has_previous_page() {
                return;
            }
            s.subscriptions_page -= 1;
            s.subscriptions_page == 0 && s.has_pending_rebuild
        };
        if rebuild && tokio::runtime::Handle::try_current().is_ok() {
            let me = self.clone();
            tokio::spawn(async move { me.go_to_page(0).await });
        }
    }

    /// 받은 쓰기가 모두 끝날 때까지 기다린다(테스트).
    pub async fn wait_for_pending_writes(&self) {
        loop {
            let chain = self.state.read().persist_chain.clone();
            let Some(chain) = chain else { return };
            chain.clone().await;
            let s = self.state.read();
            let same = s.persist_chain.as_ref().map(|c| c.ptr_eq(&chain));
            if same != Some(false) {
                return;
            }
        }
    }

    // MARK: 로컬 사본·동기화 패스

    async fn prepare_subscriptions(&self) -> Option<String> {
        let tag = self.current_tag();
        self.adopt_tag(tag.clone());
        self.load_local(tag.clone()).await;
        self.open_sync_record(tag.as_deref()).await;
        tag
    }

    async fn show_local_feed(&self) {
        self.prepare_subscriptions().await;
        self.rebuild_feed(Self::PAGE_SIZE, false, false, VodLoadState::Loading)
            .await;
    }

    /// 계정 토큰이 바뀌었으면 이전 계정의 사본·피드를 버린다.
    fn adopt_tag(&self, tag: Option<String>) {
        let mut s = self.state.lock();
        if tag == s.local_rows_tag {
            return;
        }
        s.local_rows_tag = tag;
        s.local_rows.clear();
        s.local_exhausted.clear();
        s.local_rows_loaded = false;
        s.has_pending_rebuild = false;
        s.subscription_generation = s.subscription_generation.wrapping_add(1);
        s.cursor = VodSubscriptionFeedCursor::default();
        s.subscriptions_page = 0;
        s.can_load_more = false;
        s.feed.clear();
    }

    /// 계정 사본을 채널별로 읽는다(계정마다 한 번).
    pub async fn load_local(&self, tag: Option<String>) {
        let Some(tag) = tag else { return };
        if self.deps.data_store.is_none() || self.state.read().local_rows_loaded {
            return;
        }
        let epoch = self.epoch();
        let read_tag = tag.clone();
        let rows = self
            .blocking(move |st| {
                st.fetch_subscription_vods(&read_tag, Self::PER_CHANNEL_LIMIT)
                    .map(|m| {
                        m.into_iter()
                            .map(|(k, v)| (k, v.iter().map(to_vod_info).collect::<Vec<_>>()))
                            .collect::<HashMap<_, _>>()
                    })
            })
            .await;
        let Some(Ok(rows)) = rows else { return };
        if self.current_tag().as_deref() != Some(tag.as_str()) {
            return;
        }
        let mut s = self.state.lock();
        if s.local_rows_tag.as_deref() != Some(tag.as_str()) || s.invalidation_epoch != epoch {
            return;
        }
        for (channel_id, stored) in rows {
            s.local_rows.entry(channel_id).or_insert(stored);
        }
        s.local_rows_loaded = true;
    }

    async fn open_sync_record(&self, tag: Option<&str>) -> VodSyncStore {
        match tag.and_then(VodSubscriptionTag::scope_of_tag) {
            Some(scope) => {
                self.sync_store.activate(&scope).await;
                self.sync_store.clone()
            }
            None => self.untagged_sync_store.clone(),
        }
    }

    /// 로컬 사본으로 커서를 다시 세우고 첫 화면을 채운다.
    async fn rebuild_feed(
        &self,
        target: usize,
        allow_network: bool,
        use_window: bool,
        empty_state: VodLoadState,
    ) {
        let ids = self.channel_ids();
        let follows_nobody = ids.is_empty()
            && self
                .deps
                .follow_details
                .as_ref()
                .is_some_and(|p| p().is_authoritative);
        let generation = {
            let mut s = self.state.lock();
            s.has_pending_rebuild = false;
            s.subscription_generation = s.subscription_generation.wrapping_add(1);
            s.subscriptions_page = 0;
            let order: Vec<String> = if ids.is_empty() && !follows_nobody {
                let mut keys: Vec<String> = s.local_rows.keys().cloned().collect();
                keys.sort();
                keys
            } else {
                ids
            };
            s.cursor = VodSubscriptionFeedCursor::seeded(
                &s.local_rows,
                &order,
                VodSubscriptionFeedCursor::CHANNEL_PAGE_SIZE,
                &s.local_exhausted,
            );
            s.subscription_generation
        };
        let rounds = if allow_network { 2 } else { 0 };
        let Some(first) = self
            .drain_subscriptions(target, rounds, generation, use_window)
            .await
        else {
            return;
        };
        let mut s = self.state.lock();
        s.can_load_more = s.cursor.has_more();
        s.state = if first.is_empty() {
            empty_state
        } else {
            VodLoadState::Loaded
        };
        s.feed = first;
    }

    /// 동기화 패스 한 번과 그 뒤 피드 다시 세우기.
    async fn sync_and_rebuild(&self, mode: VodSyncMode) {
        let epoch = self.epoch();
        let outcome = self.run_sync_pass_once(mode).await;
        if self.epoch() != epoch {
            return;
        }
        let (needs_rebuild, defer) = {
            let s = self.state.read();
            (
                outcome.successes > 0 || s.has_pending_rebuild,
                s.subscriptions_page > 0 || s.is_loading_more,
            )
        };
        if needs_rebuild && defer {
            self.state.lock().has_pending_rebuild = true;
        } else {
            if needs_rebuild {
                let allow = self.is_network_allowed();
                self.rebuild_feed(Self::PAGE_SIZE, allow, true, VodLoadState::Empty)
                    .await;
                if self.epoch() != epoch {
                    return;
                }
            }
            self.settle_subscription_state(&outcome);
        }
        if self.state.read().initial_load == InitialLoad::Synced {
            self.schedule_continuation_if_needed();
        }
    }

    fn settle_subscription_state(&self, outcome: &SyncOutcome) {
        let mut s = self.state.lock();
        if !s.feed.is_empty() {
            s.state = VodLoadState::Loaded;
            return;
        }
        s.state = if outcome.blocked {
            VodLoadState::Error("다시보기를 잠시 뒤 다시 불러옵니다".into())
        } else if outcome.successes == 0 && outcome.failures > 0 {
            VodLoadState::Error("팔로우한 채널의 다시보기를 불러오지 못했습니다".into())
        } else {
            VodLoadState::Empty
        };
    }

    /// 패스를 한 번에 하나씩 돈다.
    async fn run_sync_pass_once(&self, mode: VodSyncMode) -> SyncOutcome {
        let _guard = self.pass_lock.lock().await;
        self.state.lock().sync_running_mode = Some(mode);
        let started = std::time::Instant::now();
        let outcome = self.perform_sync_pass(mode).await;
        self.state.lock().sync_running_mode = None;
        if outcome.follows > 0 {
            tracing::debug!(
                mode = mode.raw_value(),
                follows = outcome.follows,
                budget = outcome.budget,
                planned = outcome.planned,
                requested = outcome.requested,
                failed = outcome.failures,
                due_left = outcome.due_left,
                fresh_skip = outcome.fresh_skip,
                ms = started.elapsed().as_millis() as u64,
                "vod_sync_pass"
            );
        }
        outcome
    }

    async fn perform_sync_pass(&self, mode: VodSyncMode) -> SyncOutcome {
        let ids = self.channel_ids();
        if ids.is_empty() {
            return SyncOutcome::default();
        }
        if !self.is_network_allowed() {
            return SyncOutcome {
                blocked: true,
                ..Default::default()
            };
        }
        let start = self.now();
        let tag = self.current_tag();
        if tag != self.state.read().local_rows_tag {
            return SyncOutcome::default();
        }
        let epoch = self.epoch();
        let store = self.open_sync_record(tag.as_deref()).await;
        if self.epoch() != epoch || self.current_tag() != tag {
            return SyncOutcome::default();
        }
        let details = self.follow_details();
        let followed = authoritative_followed(&ids, &details);
        if let Some(f) = &followed {
            store.retain(f);
            self.state.lock().local_rows.retain(|k, _| f.contains(k));
        }
        store.observe_live_set(&details.live, start);
        let mut outcome = SyncOutcome {
            follows: ids.len(),
            budget: if mode == VodSyncMode::Continuation {
                Self::CONTINUATION_BATCH_SIZE
            } else {
                VodSyncPolicy::entry_budget(ids.len() as i64)
            },
            ..Default::default()
        };
        let has_local = !self.state.read().local_rows.is_empty();
        if mode != VodSyncMode::Refresh
            && mode != VodSyncMode::Continuation
            && has_local
            && VodSyncPolicy::is_feed_fresh(store.state().last_pass_at, start)
        {
            outcome.fresh_skip = true;
            outcome.due_left = due_count(&ids, &store, start);
            return outcome;
        }
        let plan = self.plan_targets(&ids, &details.live, &store, mode, start);
        outcome.planned = plan.channel_ids.len();
        outcome.due_left = plan.due_count;
        if plan.channel_ids.is_empty() {
            return outcome;
        }
        let results = self.fetch_targets(&plan.channel_ids, mode).await;
        {
            let s = self.state.read();
            if self.current_tag() != tag || s.local_rows_tag != tag || s.invalidation_epoch != epoch
            {
                return SyncOutcome::default();
            }
        }
        self.apply_results(
            results, &plan, mode, tag, &store, &details, followed, outcome,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_results(
        &self,
        results: Vec<ChannelPageResult>,
        plan: &SyncTargets,
        mode: VodSyncMode,
        tag: Option<String>,
        store: &VodSyncStore,
        details: &VodFollowDetails,
        followed: Option<HashSet<String>>,
        mut outcome: SyncOutcome,
    ) -> SyncOutcome {
        let filled: Vec<ChannelPageResult> = results
            .iter()
            .map(|r| filling_channel(r, details))
            .collect();
        let finished = self.now();
        store.record(&filled, finished);
        let succeeded: HashSet<String> = filled
            .iter()
            .filter(|r| !r.failed)
            .map(|r| r.channel_id.clone())
            .collect();
        let stop_after = if mode == VodSyncMode::Continuation {
            Self::CONTINUATION_STOP_AFTER_FAILURES
        } else {
            Self::SYNC_STOP_AFTER_FAILURES
        };
        self.apply_backoff(&filled, succeeded.len(), finished, stop_after);
        self.absorb(&filled, tag, followed);
        outcome.requested = filled.len();
        outcome.successes = succeeded.len();
        outcome.failures = filled.len() - succeeded.len();
        outcome.due_left = plan
            .due_count
            .saturating_sub(succeeded.intersection(&plan.due_ids).count());
        if !succeeded.is_empty() {
            store.note_pass(finished, outcome.due_left);
        }
        outcome
    }

    async fn fetch_targets(&self, ids: &[String], mode: VodSyncMode) -> Vec<ChannelPageResult> {
        let size = VodSubscriptionFeedCursor::CHANNEL_PAGE_SIZE;
        let pages = HashMap::new();
        if mode == VodSyncMode::Continuation {
            self.aggregator
                .fetch_pages(
                    ids,
                    &pages,
                    size,
                    Self::CONTINUATION_CONCURRENCY,
                    Self::CONTINUATION_STOP_AFTER_FAILURES,
                )
                .await
        } else {
            self.aggregator
                .fetch_pages(
                    ids,
                    &pages,
                    size,
                    VodFeedAggregator::DEFAULT_MAX_CONCURRENT,
                    Self::SYNC_STOP_AFTER_FAILURES,
                )
                .await
        }
    }

    fn plan_targets(
        &self,
        ids: &[String],
        live: &HashSet<String>,
        store: &VodSyncStore,
        mode: VodSyncMode,
        now: Date,
    ) -> SyncTargets {
        let follows: Vec<FollowSignal> = ids
            .iter()
            .map(|c| FollowSignal {
                channel_id: c.clone(),
                is_live: live.contains(c),
            })
            .collect();
        let state = store.state();
        let failed = store.failed_at();
        if mode == VodSyncMode::Continuation {
            let plan = VodSyncPlanner::plan(
                &follows,
                &state,
                &failed,
                now,
                Self::CONTINUATION_BATCH_SIZE,
                mode,
            );
            return SyncTargets {
                due_ids: plan.channel_ids.iter().cloned().collect(),
                due_count: plan.due_count,
                channel_ids: plan.channel_ids,
            };
        }
        let mut s = self.state.lock();
        let budget = s
            .entry_window
            .open_remaining(now)
            .unwrap_or_else(|| VodSyncPolicy::entry_budget(ids.len() as i64));
        let plan = VodSyncPlanner::plan(
            &follows,
            &state,
            &failed,
            now,
            budget.saturating_sub(VodEntryWindow::DRAIN_RESERVE),
            mode,
        );
        let granted = s.entry_window.take(plan.channel_ids.len(), now);
        let rotation = plan
            .tier_counts
            .get(&VodSyncTier::Rotation)
            .copied()
            .unwrap_or(0);
        let due_len = plan.channel_ids.len().saturating_sub(rotation);
        SyncTargets {
            due_ids: plan.channel_ids[..due_len].iter().cloned().collect(),
            due_count: plan.due_count,
            channel_ids: plan.channel_ids.into_iter().take(granted).collect(),
        }
    }

    /// 429 는 max(60초, Retry-After), 오프라인은 60초.
    fn apply_backoff(
        &self,
        results: &[ChannelPageResult],
        successes: usize,
        at: Date,
        stop_after: usize,
    ) {
        let mut delay: Option<TimeInterval> = None;
        let mut offline = 0;
        for r in results {
            match r.failure_kind {
                Some(VodFailureKind::RateLimited(retry)) => {
                    delay = Some(delay.unwrap_or(0.0).max(Self::MINIMUM_BACKOFF.max(retry)));
                }
                Some(VodFailureKind::Offline) => offline += 1,
                _ => {}
            }
        }
        if delay.is_none() && offline > 0 && (successes == 0 || offline >= stop_after) {
            delay = Some(Self::MINIMUM_BACKOFF);
        }
        let Some(delay) = delay else { return };
        let mut s = self.state.lock();
        let candidate = adding(&at, delay);
        let base = s.sync_backoff_until.unwrap_or(at);
        s.sync_backoff_until = Some(base.max(candidate));
    }

    /// 커서에서 최신순으로 `count` 개를 꺼낸다. 떨어진 활성 채널은 먼저 다음 페이지를 받는다.
    /// 새로고침이 끼어들었으면 `None`.
    async fn drain_subscriptions(
        &self,
        count: usize,
        fetch_rounds: usize,
        generation: u64,
        use_window: bool,
    ) -> Option<Vec<VodInfo>> {
        let mut items = Vec::new();
        let mut failed: HashSet<String> = HashSet::new();
        let mut rounds = 0;
        while items.len() < count {
            let pages = {
                let mut s = self.state.lock();
                if s.subscription_generation != generation {
                    return None;
                }
                if let Some(next) = s.cursor.pop_newest(true, &failed) {
                    items.push(next);
                    continue;
                }
                let pages = if rounds < fetch_rounds {
                    let wanted = s.cursor.pages_to_fetch(&failed);
                    self.granted_pages(&mut s, wanted, use_window)
                } else {
                    HashMap::new()
                };
                if pages.is_empty() {
                    match s.cursor.pop_newest(false, &HashSet::new()) {
                        Some(next) => {
                            items.push(next);
                            continue;
                        }
                        None => break,
                    }
                }
                pages
            };
            rounds += 1;
            let tag = self.state.read().local_rows_tag.clone();
            let details = self.follow_details();
            let ids: Vec<String> = pages.keys().cloned().collect();
            let results = self
                .aggregator
                .fetch_pages(
                    &ids,
                    &pages,
                    VodSubscriptionFeedCursor::CHANNEL_PAGE_SIZE,
                    VodFeedAggregator::DEFAULT_MAX_CONCURRENT,
                    usize::MAX,
                )
                .await;
            let filled: Vec<ChannelPageResult> = results
                .iter()
                .map(|r| filling_channel(r, &details))
                .collect();
            {
                let mut s = self.state.lock();
                if s.subscription_generation != generation {
                    return None;
                }
                failed.extend(
                    filled
                        .iter()
                        .filter(|r| r.failed)
                        .map(|r| r.channel_id.clone()),
                );
                s.cursor.apply(&filled);
            }
            let followed = authoritative_followed(&self.channel_ids(), &details);
            self.absorb(&filled, tag, followed);
        }
        Some(items)
    }

    fn granted_pages(
        &self,
        s: &mut VodBrowseState,
        pages: HashMap<String, i64>,
        use_window: bool,
    ) -> HashMap<String, i64> {
        if !use_window || pages.is_empty() {
            return pages;
        }
        let now = self.now();
        let granted = s.entry_window.take(pages.len(), now);
        if granted >= pages.len() {
            return pages;
        }
        let keep: HashSet<&String> = s
            .cursor
            .active()
            .iter()
            .filter(|c| pages.contains_key(*c))
            .take(granted)
            .collect();
        pages
            .iter()
            .filter(|(k, _)| keep.contains(k))
            .map(|(k, v)| (k.clone(), *v))
            .collect()
    }

    /// 받은 페이지를 사본에 합치고 저장소에 쓴다. 실패는 사본을 건드리지 않는다.
    fn absorb(
        &self,
        results: &[ChannelPageResult],
        tag: Option<String>,
        followed: Option<HashSet<String>>,
    ) {
        let succeeded: Vec<&ChannelPageResult> = results.iter().filter(|r| !r.failed).collect();
        if succeeded.is_empty() {
            return;
        }
        {
            let mut s = self.state.lock();
            for r in &succeeded {
                let existing = s.local_rows.get(&r.channel_id).cloned().unwrap_or_default();
                let merged = VodSubscriptionFeedCursor::merged_local_rows(
                    &existing,
                    r.page,
                    &r.items,
                    !r.has_more(),
                    Self::PER_CHANNEL_LIMIT,
                );
                if merged.is_empty() {
                    s.local_rows.remove(&r.channel_id);
                } else {
                    s.local_rows.insert(r.channel_id.clone(), merged);
                }
                if r.has_more() {
                    s.local_exhausted.remove(&r.channel_id);
                } else {
                    s.local_exhausted.insert(r.channel_id.clone());
                }
            }
        }
        self.persist(&succeeded, tag, followed);
    }

    /// 받은 페이지를 계정 사본에 쓴다 — 받은 순서로 잇고, 쓰기 직전에 태그를 다시 본다.
    fn persist(
        &self,
        results: &[&ChannelPageResult],
        tag: Option<String>,
        followed: Option<HashSet<String>>,
    ) {
        let (Some(store), Some(tag)) = (self.deps.data_store.clone(), tag) else {
            return;
        };
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let fetched_at = self.now();
        let pages: Vec<SubscriptionVodPage> = results
            .iter()
            .map(|r| SubscriptionVodPage {
                channel_id: r.channel_id.clone(),
                page: r.page,
                items: r
                    .items
                    .iter()
                    .map(|v| cached_data(v, &r.channel_id, &tag, fetched_at))
                    .collect(),
                exhausted: !r.has_more(),
            })
            .collect();
        let provider = self.deps.subscription_tag.clone();
        let mut s = self.state.lock();
        let previous = s.persist_chain.take();
        let epoch = s.invalidation_epoch;
        let state = self.state.clone();
        let chain = spawn_joinable(async move {
            if let Some(p) = previous {
                p.await;
            }
            let current = provider.as_ref().and_then(|p| p());
            if current.as_deref() != Some(tag.as_str()) || state.read().invalidation_epoch != epoch
            {
                return;
            }
            let _ = tokio::task::spawn_blocking(move || {
                store.apply_subscription_vod_pages(
                    &tag,
                    &pages,
                    followed.as_ref(),
                    Self::PER_CHANNEL_LIMIT,
                )
            })
            .await;
        });
        s.persist_chain = Some(chain);
    }

    // MARK: 보임 게이트 · 이어받기

    /// 허브 페이지가 앞에 있는지 알린다.
    pub fn set_visible(&self, page_active: bool) {
        let was = {
            let mut s = self.state.lock();
            let was = s.is_visible();
            s.is_page_active = page_active;
            was
        };
        self.apply_visibility(was);
    }

    fn change_tab(&self, tab: VodTab) {
        let was = {
            let mut s = self.state.lock();
            let was = s.is_visible();
            s.selected_tab = tab;
            was
        };
        self.apply_visibility(was);
    }

    fn apply_visibility(&self, was_visible: bool) {
        let visible = self.state.read().is_visible();
        if visible == was_visible {
            return;
        }
        if !visible {
            self.cancel_continuation();
            return;
        }
        let mut s = self.state.lock();
        if s.initial_load != InitialLoad::Synced || s.reactivation_task.is_some() {
            return;
        }
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let epoch = s.invalidation_epoch;
        let me = self.clone();
        s.reactivation_task = Some(spawn_joinable(async move {
            me.reactivate(epoch).await;
            let mut s = me.state.lock();
            if s.invalidation_epoch == epoch {
                s.reactivation_task = None;
            }
        }));
    }

    async fn reactivate(&self, epoch: u64) {
        if !self.state.read().is_visible() || !self.is_network_allowed() {
            return;
        }
        let ids = self.channel_ids();
        if ids.is_empty() {
            return;
        }
        self.prepare_subscriptions().await;
        if self.epoch() != epoch || !self.state.read().is_visible() {
            return;
        }
        let Some(store) = self.active_sync_store() else {
            return;
        };
        if VodSyncPolicy::is_feed_fresh(store.state().last_pass_at, self.now()) {
            self.schedule_continuation_if_needed();
            return;
        }
        let now = self.now();
        self.state.lock().entry_window.begin(ids.len() as i64, now);
        self.sync_and_rebuild(VodSyncMode::Stale).await;
    }

    fn active_sync_store(&self) -> Option<VodSyncStore> {
        match self
            .current_tag()
            .as_deref()
            .and_then(VodSubscriptionTag::scope_of_tag)
        {
            None => Some(self.untagged_sync_store.clone()),
            Some(scope) => (self.sync_store.scope().as_deref() == Some(scope.as_str()))
                .then(|| self.sync_store.clone()),
        }
    }

    fn has_backlog(state: &VodSyncState) -> bool {
        let Some(last_pass) = state.last_pass_at else {
            return false;
        };
        match state.last_full_sync_at {
            None => true,
            Some(full) => full < last_pass,
        }
    }

    fn schedule_continuation_if_needed(&self) {
        {
            let s = self.state.read();
            if s.continuation_task.is_some()
                || !s.is_visible()
                || s.initial_load != InitialLoad::Synced
            {
                return;
            }
        }
        if !self.is_network_allowed() || tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let Some(store) = self.active_sync_store() else {
            return;
        };
        if !Self::has_backlog(&store.state()) {
            return;
        }
        let ids = self.channel_ids();
        let due = due_count(&ids, &store, self.now());
        if due == 0 {
            return;
        }
        let mut s = self.state.lock();
        s.sync_progress = Some(SyncProgress {
            checked: ids.len() - due,
            total: ids.len(),
        });
        let epoch = s.invalidation_epoch;
        let me = self.clone();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let slot = TaskSlot::spawn(move |id| async move {
            me.run_continuation(epoch, id).await;
            let _ = tx.send(());
        });
        s.continuation_task = Some(slot);
        s.continuation_running = Some(
            async move {
                let _ = rx.await;
            }
            .boxed()
            .shared(),
        );
    }

    async fn run_continuation(&self, epoch: u64, id: u64) {
        let mut failed_batches = 0;
        (self.deps.sleep)(Self::CONTINUATION_DELAY).await;
        loop {
            {
                let s = self.state.read();
                if !slot_is(&s.continuation_task, id)
                    || !s.is_visible()
                    || s.invalidation_epoch != epoch
                {
                    break;
                }
            }
            if !self.is_network_allowed() {
                break;
            }
            let outcome = self.run_sync_pass_once(VodSyncMode::Continuation).await;
            if self.epoch() != epoch {
                break;
            }
            self.apply_background_batch(&outcome).await;
            if self.epoch() != epoch
                || outcome.planned == 0
                || outcome.due_left == 0
                || !self.is_network_allowed()
            {
                break;
            }
            self.state.lock().sync_progress = Some(SyncProgress {
                checked: outcome.follows.saturating_sub(outcome.due_left),
                total: outcome.follows,
            });
            if outcome.is_failed_batch() {
                failed_batches += 1;
            }
            if failed_batches >= Self::CONTINUATION_MAX_FAILED_BATCHES {
                break;
            }
            (self.deps.sleep)(Self::CONTINUATION_INTERVAL).await;
        }
        let mut s = self.state.lock();
        if s.invalidation_epoch == epoch && slot_is(&s.continuation_task, id) {
            s.continuation_task = None;
            s.sync_progress = None;
        }
    }

    async fn apply_background_batch(&self, outcome: &SyncOutcome) {
        let (pending, defer, empty_state) = {
            let s = self.state.read();
            (
                s.has_pending_rebuild,
                s.subscriptions_page > 0 || s.is_loading_more,
                if s.feed.is_empty() {
                    s.state.clone()
                } else {
                    VodLoadState::Empty
                },
            )
        };
        if outcome.successes == 0 && !pending {
            return;
        }
        if defer {
            self.state.lock().has_pending_rebuild = true;
            return;
        }
        self.rebuild_feed(Self::PAGE_SIZE, false, false, empty_state)
            .await;
    }

    fn cancel_continuation(&self) {
        let mut s = self.state.lock();
        cancel_slot(&mut s.continuation_task);
        s.continuation_running = None;
        s.sync_progress = None;
    }

    /// 이어받기가 끝날 때까지 기다린다(테스트).
    pub async fn wait_for_continuation(&self) {
        let running = self.state.read().continuation_running.clone();
        if let Some(r) = running {
            r.await;
        }
    }

    /// 다시 보임에 따른 패스가 끝날 때까지 기다린다(테스트).
    pub async fn wait_for_reactivation(&self) {
        let running = self.state.read().reactivation_task.clone();
        if let Some(r) = running {
            r.await;
        }
    }

    // MARK: 계정 전환

    /// 인증 경계(로그인·로그아웃·계정 전환) — 도는 일을 끊고 이전 계정의 피드·사본을 비운다.
    pub fn invalidate(&self) {
        self.channel_browser.invalidate();
        let mut s = self.state.lock();
        s.library_mutation_generation = s.library_mutation_generation.wrapping_add(1);
        s.continue_mutation_generation = s.continue_mutation_generation.wrapping_add(1);
        s.invalidation_epoch = s.invalidation_epoch.wrapping_add(1);
        s.subscription_generation = s.subscription_generation.wrapping_add(1);
        s.local_load_task = None;
        s.initial_load_task = None;
        cancel_slot(&mut s.continuation_task);
        s.continuation_running = None;
        s.reactivation_task = None;
        s.sync_progress = None;
        s.has_pending_rebuild = false;
        s.local_rows.clear();
        s.local_exhausted.clear();
        s.local_rows_tag = None;
        s.local_rows_loaded = false;
        s.cursor = VodSubscriptionFeedCursor::default();
        s.subscriptions_page = 0;
        s.can_load_more = false;
        s.feed.clear();
        s.preview_vod = None;
        s.state = VodLoadState::Idle;
        s.initial_load = InitialLoad::Idle;
    }

    /// 로그인이 확정돼 계정 토큰이 생겼다(또는 바뀌었다) — 다른 계정으로 읽었으면 처음부터 다시 읽는다.
    pub async fn auth_availability_changed(&self) {
        let running = self.state.read().initial_load_task.clone();
        if let Some(r) = running {
            r.await;
        }
        let Some(tag) = self.current_tag() else {
            return;
        };
        {
            let s = self.state.read();
            if s.initial_load != InitialLoad::Synced || s.local_rows_tag.as_deref() == Some(&tag) {
                return;
            }
        }
        self.invalidate();
        self.load_initial_if_needed().await;
    }

    pub fn has_local_feed(&self) -> bool {
        self.state.read().has_local_feed()
    }

    pub fn is_visible(&self) -> bool {
        self.state.read().is_visible()
    }
}

/// 이번 패스에 받을 채널.
struct SyncTargets {
    channel_ids: Vec<String>,
    due_count: usize,
    due_ids: HashSet<String>,
}

fn due_count(ids: &[String], store: &VodSyncStore, now: Date) -> usize {
    let follows: Vec<FollowSignal> = ids
        .iter()
        .map(|c| FollowSignal {
            channel_id: c.clone(),
            is_live: false,
        })
        .collect();
    VodSyncPlanner::plan(
        &follows,
        &store.state(),
        &store.failed_at(),
        now,
        0,
        VodSyncMode::Continuation,
    )
    .due_count
}

fn authoritative_followed(ids: &[String], details: &VodFollowDetails) -> Option<HashSet<String>> {
    (details.is_authoritative && !ids.is_empty()).then(|| ids.iter().cloned().collect())
}

/// vodList 항목의 채널 정보가 없으면 결과의 채널과 팔로우 목록의 이름·아바타로 채운다.
fn filling_channel(result: &ChannelPageResult, details: &VodFollowDetails) -> ChannelPageResult {
    if result.items.iter().all(|i| i.channel.is_some()) {
        return result.clone();
    }
    let channel = details
        .profile(&result.channel_id)
        .unwrap_or_else(|| ChannelInfo::new(result.channel_id.clone(), String::new()));
    result.replacing_items(
        result
            .items
            .iter()
            .map(|item| {
                let mut item = item.clone();
                if item.channel.is_none() {
                    item.channel = Some(channel.clone());
                }
                item
            })
            .collect(),
    )
}

/// CachedVodData → VodInfo.
pub fn to_vod_info(d: &CachedVodData) -> VodInfo {
    let channel = d.channel_id.as_ref().filter(|c| !c.is_empty()).map(|cid| {
        let mut c = ChannelInfo::new(cid.clone(), d.channel_name.clone().unwrap_or_default());
        c.channel_image_url = d
            .channel_image_url
            .as_deref()
            .and_then(|s| url::Url::parse(s).ok());
        c
    });
    let mut v = VodInfo::new(d.video_no, d.video_title.clone());
    v.video_id = d.video_id.clone();
    v.video_image_url = d
        .thumbnail_image_url
        .as_deref()
        .and_then(|s| url::Url::parse(s).ok());
    v.duration = d.duration;
    v.publish_date = d.publish_date;
    v.read_count = d.read_count;
    v.channel = channel;
    v
}

/// VodInfo → 사본 행. 채널 이름을 모르면 `None` — 저장소 병합이 아는 이름을 지우지 않게.
pub fn cached_data(vod: &VodInfo, channel_id: &str, tag: &str, fetched_at: Date) -> CachedVodData {
    let thumbnail = vod.video_image_url.as_ref().map(|u| u.to_string());
    let channel_name = vod
        .channel
        .as_ref()
        .map(|c| c.channel_name.clone())
        .filter(|n| !n.is_empty());
    CachedVodData {
        video_no: vod.video_no,
        video_id: vod.video_id.clone(),
        video_title: vod.video_title.clone(),
        thumbnail_image_url: thumbnail.clone(),
        duration: vod.duration,
        read_count: vod.read_count,
        publish_date: vod.publish_date,
        channel_id: Some(
            vod.channel
                .as_ref()
                .map(|c| c.channel_id.clone())
                .unwrap_or_else(|| channel_id.to_string()),
        ),
        content_hash: CachedVod::compute_content_hash(
            &vod.video_title,
            thumbnail.as_deref(),
            vod.read_count,
            channel_name.as_deref(),
        ),
        channel_name,
        channel_image_url: vod
            .channel
            .as_ref()
            .and_then(|c| c.channel_image_url.as_ref().map(|u| u.to_string())),
        sources: BTreeSet::from([tag.to_string()]),
        fetched_at,
    }
}
