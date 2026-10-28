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
//!
//! 뷰모델 메서드는 주제별 하위 모듈에 있다: 보관함(library) · 검색·채널별(search) · 구독 피드(feed) ·
//! 동기화 패스(sync) · 보임 게이트·이어받기(visibility).

mod feed;
mod library;
mod search;
mod sync;
mod visibility;

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
use cview_core::utils::time::{Date, TimeInterval};
use cview_networking::ChzzkApiClient;
use cview_persistence::models::cached_vod::{CachedVod, CachedVodData};
use cview_persistence::models::playback::{PersistedVodPlaybackData, PersistedVodWatchLaterData};
use cview_persistence::DataStore;

use super::support::{cancel_slot, Observed, ObservedGuard, TaskSlot};
use super::vod_library::{VodChannelApi, VodChannelBrowser, VodLibraryMutationQueue, VodLoadState};
use crate::services::support::{system_clock, Clock};
use crate::services::vod_feed::{
    ChannelPageResult, VodFeedAggregator, VodPageSource, VodSubscriptionFeedCursor,
};
use crate::services::vod_playback_progress::VodPlaybackProgressStore;
use crate::services::vod_sync_state::{FollowSignal, VodSyncMode, VodSyncPlanner};
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
}

impl VodBrowseViewModel {
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
                .map_or_else(|| channel_id.to_string(), |c| c.channel_id.clone()),
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
