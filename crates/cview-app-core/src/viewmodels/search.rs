//! 통합 검색 — 채널·라이브·다시보기·클립 버킷, 디바운스, 결과 스냅샷 캐시, 최근 검색어.
//!
//! 원본: `Sources/CViewApp/ViewModels/SearchViewModel.swift`,
//! `SearchViewModel+Cache.swift`, `SearchViewModel+FetchHelpers.swift`.
//! `WorkspaceValueCache` 는 같은 의미의 작은 LRU([`ResultCache`])로, UserDefaults 최근 검색어는
//! [`RecentSearchStore`] 로 받는다. 랜딩 스냅샷의 화면 전용 파생 계산은 호출 측 클로저로 둔다.
//! 결과 캐시·정리는 `results`, 버킷 받기는 `fetch` 하위 모듈에 있다.

mod fetch;
mod results;

use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use async_trait::async_trait;
use serde::Serialize;
use tokio::time::Instant;

use cview_core::logic::search_scoring::{self, AutocompleteSources, SuggestionSource};
use cview_core::models::live_catalog::LiveCatalogSummary;
use cview_core::models::search::{
    AutocompleteSuggestion, AutocompleteSuggestionKind, ClipInfo, ClipListPage, LiveSortOption,
    SearchResult, SearchType, VodInfo,
};
use cview_core::models::{ChannelInfo, LiveChannelItem, LiveInfo, LiveStatus};
use cview_networking::ChzzkApiClient;

use super::support::{cancel_slot, slot_is, Observed, ObservedGuard, TaskSlot};

/// 검색 범위 탭.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchScope {
    #[default]
    All,
    Channel,
    Live,
    Video,
    Clip,
}

impl SearchScope {
    pub const ALL: [Self; 5] = [
        Self::All,
        Self::Channel,
        Self::Live,
        Self::Video,
        Self::Clip,
    ];

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

    pub fn search_type(self) -> Option<SearchType> {
        match self {
            Self::All => None,
            Self::Channel => Some(SearchType::Channel),
            Self::Live => Some(SearchType::Live),
            Self::Video => Some(SearchType::Video),
            Self::Clip => Some(SearchType::Clip),
        }
    }

    pub fn from_search_type(t: SearchType) -> Self {
        match t {
            SearchType::Channel => Self::Channel,
            SearchType::Live => Self::Live,
            SearchType::Video => Self::Video,
            SearchType::Clip => Self::Clip,
        }
    }
}

/// 검색 API.
#[async_trait]
pub trait SearchApi: Send + Sync {
    async fn search_channels(
        &self,
        keyword: &str,
        offset: i64,
        size: i64,
    ) -> Result<SearchResult<ChannelInfo>, String>;
    async fn search_lives(
        &self,
        keyword: &str,
        offset: i64,
        size: i64,
    ) -> Result<SearchResult<LiveInfo>, String>;
    async fn search_videos(
        &self,
        keyword: &str,
        offset: i64,
        size: i64,
    ) -> Result<SearchResult<VodInfo>, String>;
    /// 채널 클립 한 쪽 — `cursor` 는 앞 쪽의 `next_clip_uid`, 첫 쪽은 `None`.
    async fn clip_list(
        &self,
        channel_id: &str,
        cursor: Option<&str>,
        size: i64,
    ) -> Result<ClipListPage, String>;
    /// 채널의 지금 방송. 방송 중이 아니면 `None`.
    ///
    /// [SEARCH 2026-11-03] 라이브 검색 API 는 제목·태그로만 찾아 채널 이름이 맞는 방송을 놓친다 —
    /// 채널 검색에서 `openLive` 인 채널은 이것으로 라이브를 받아 라이브 결과에 합친다.
    async fn channel_live(&self, _channel_id: &str) -> Result<Option<LiveInfo>, String> {
        Ok(None)
    }
}

#[async_trait]
impl SearchApi for ChzzkApiClient {
    async fn search_channels(
        &self,
        keyword: &str,
        offset: i64,
        size: i64,
    ) -> Result<SearchResult<ChannelInfo>, String> {
        ChzzkApiClient::search_channels(self, keyword, offset, size)
            .await
            .map_err(|e| e.to_string())
    }
    async fn search_lives(
        &self,
        keyword: &str,
        offset: i64,
        size: i64,
    ) -> Result<SearchResult<LiveInfo>, String> {
        ChzzkApiClient::search_lives(self, keyword, offset, size)
            .await
            .map_err(|e| e.to_string())
    }
    async fn search_videos(
        &self,
        keyword: &str,
        offset: i64,
        size: i64,
    ) -> Result<SearchResult<VodInfo>, String> {
        ChzzkApiClient::search_videos(self, keyword, offset, size)
            .await
            .map_err(|e| e.to_string())
    }
    async fn clip_list(
        &self,
        channel_id: &str,
        cursor: Option<&str>,
        size: i64,
    ) -> Result<ClipListPage, String> {
        ChzzkApiClient::clip_list_page(self, channel_id, cursor, size)
            .await
            .map_err(|e| e.to_string())
    }
    async fn channel_live(&self, channel_id: &str) -> Result<Option<LiveInfo>, String> {
        let live = ChzzkApiClient::live_detail(self, channel_id, false)
            .await
            .map_err(|e| e.to_string())?;
        Ok((live.status == LiveStatus::Open).then_some(live))
    }
}

/// 최근 검색어 저장소 (`UserDefaults` 의 `CView.recentSearches`).
pub trait RecentSearchStore: Send + Sync {
    fn load(&self) -> Option<Vec<String>>;
    fn save(&self, searches: &[String]);
    fn remove(&self);
}

/// 메모리 저장소.
#[derive(Debug, Default)]
pub struct MemoryRecentSearchStore(Mutex<Option<Vec<String>>>);

impl RecentSearchStore for MemoryRecentSearchStore {
    fn load(&self) -> Option<Vec<String>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    fn save(&self, searches: &[String]) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(searches.to_vec());
    }
    fn remove(&self) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// 작은 LRU (`WorkspaceValueCache`).
#[derive(Debug, Clone)]
pub struct ResultCache<V> {
    capacity: usize,
    order: VecDeque<String>,
    values: HashMap<String, V>,
}

impl<V: Clone> ResultCache<V> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            order: VecDeque::new(),
            values: HashMap::new(),
        }
    }

    pub fn value(&mut self, key: &str) -> Option<V> {
        let v = self.values.get(key).cloned()?;
        self.order.retain(|k| k != key);
        self.order.push_back(key.to_string());
        Some(v)
    }

    pub fn insert(&mut self, value: V, key: &str) {
        self.order.retain(|k| k != key);
        self.order.push_back(key.to_string());
        self.values.insert(key.to_string(), value);
        while self.order.len() > self.capacity {
            if let Some(old) = self.order.pop_front() {
                self.values.remove(&old);
            }
        }
    }

    pub fn remove(&mut self, key: &str) {
        self.order.retain(|k| k != key);
        self.values.remove(key);
    }

    pub fn clear(&mut self) {
        self.order.clear();
        self.values.clear();
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// 검색 결과 스냅샷.
#[derive(Debug, Clone)]
pub struct Snapshot {
    channels: Vec<ChannelInfo>,
    lives: Vec<LiveInfo>,
    channel_lives: Vec<LiveInfo>,
    videos: Vec<VodInfo>,
    clips: Vec<ClipInfo>,
    offsets: [i64; 4],
    has_more: [bool; 4],
    loaded: HashSet<SearchType>,
    errors: HashMap<SearchType, String>,
    fetched_at: f64,
    clip_source_channel_ids: Vec<String>,
    clip_source_keyword: Option<String>,
    clip_next_page: HashMap<String, i64>,
    clip_cursor: HashMap<String, String>,
    clip_exhausted: HashSet<String>,
    clip_full_channel_ids: HashSet<String>,
}

/// 랜딩(검색어 없음) 화면 입력.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LandingInputs {
    pub summary: Option<LiveCatalogSummary>,
    pub popular: Vec<LiveChannelItem>,
    pub rows: Vec<LiveChannelItem>,
}

/// 랜딩 스냅샷 — 화면이 파생 계산한 결과. 기본 계산은 입력을 그대로 담는다.
pub type SearchLandingSnapshot = LandingInputs;

pub const PAGE_SIZE: i64 = 20;
pub const MAX_RECENT_SEARCHES: usize = 10;
pub const RECENT_SEARCHES_KEY: &str = "CView.recentSearches";
pub const CLIP_SOURCE_CHANNEL_COUNT: usize = 3;
pub const CLIP_FIRST_WAVE_CHANNELS: usize = 1;
/// 채널 검색 첫 쪽에서 라이브를 받아 합칠 방송 중 채널 수 상한.
pub const CHANNEL_LIVE_LIMIT: usize = 6;
pub const LANDING_RECOMPUTE_LIMIT: usize = 3;
pub const ACTIVATION_REUSE_WINDOW: f64 = 300.0;
pub const SNAPSHOT_MAX_AGE: f64 = 300.0;
pub const FRESH_WINDOW: f64 = 15.0;
pub const SNAPSHOT_MAX_ROWS: usize = 200;
pub const RESULT_CACHE_CAPACITY: usize = 12;
pub const SEARCH_DEBOUNCE: Duration = Duration::from_millis(300);

/// 검색 상태.
#[derive(Debug)]
pub struct SearchState {
    this: Weak<SearchShared>,
    query: String,
    pub selected_scope: SearchScope,
    pub selected_channel_id: Option<String>,
    pub channel_results: Vec<ChannelInfo>,
    /// 라이브 검색 결과(키워드가 제목·태그에 맞는 방송).
    pub live_results: Vec<LiveInfo>,
    /// 채널 검색에서 방송 중인 채널의 라이브(채널 관련도 순). 화면은 [`SearchState::merged_live_results`].
    pub channel_lives: Vec<LiveInfo>,
    pub video_results: Vec<VodInfo>,
    pub clip_results: Vec<ClipInfo>,
    pub is_searching_channels: bool,
    pub is_searching_lives: bool,
    pub is_searching_videos: bool,
    pub is_searching_clips: bool,
    pub has_more_channels: bool,
    pub has_more_lives: bool,
    pub has_more_videos: bool,
    pub has_more_clips: bool,
    pub error_message: Option<String>,
    pub bucket_errors: HashMap<SearchType, String>,
    pub recent_searches: Vec<String>,
    live_sort_option: LiveSortOption,
    pub following_channel_names: Vec<String>,
    pub recent_watched_channel_names: Vec<String>,
    is_search_pending: bool,
    completed_search_query: Option<String>,
    search_task: Option<TaskSlot>,
    result_cache: ResultCache<Snapshot>,
    rendered_query: String,
    performing_query: Option<String>,
    result_fetched_at: f64,
    loaded_buckets: HashSet<SearchType>,
    active_search_id: u64,
    pub channel_offset: i64,
    pub live_offset: i64,
    pub video_offset: i64,
    pub clip_page: i64,
    clip_source_channel_ids: Vec<String>,
    clip_source_keyword: Option<String>,
    clip_next_page: HashMap<String, i64>,
    clip_cursor: HashMap<String, String>,
    clip_exhausted: HashSet<String>,
    /// 이름이 검색어와 같은 소스 채널 — 이 채널의 클립은 제목과 관계없이 모두 보인다.
    clip_full_channel_ids: HashSet<String>,
    landing_snapshot: Option<SearchLandingSnapshot>,
    landing_source_revision: Option<i64>,
}

fn trimmed(s: &str) -> String {
    s.trim().to_string()
}

static NEXT_SEARCH_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn new_search_id() -> u64 {
    NEXT_SEARCH_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl SearchState {
    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn live_sort_option(&self) -> LiveSortOption {
        self.live_sort_option
    }

    pub fn set_live_sort_option(&mut self, option: LiveSortOption) {
        self.live_sort_option = option;
        self.sort_live_results();
    }

    pub fn selected_tab(&self) -> SearchType {
        self.selected_scope
            .search_type()
            .unwrap_or(SearchType::Channel)
    }

    pub fn set_selected_tab(&mut self, t: SearchType) {
        self.selected_scope = SearchScope::from_search_type(t);
    }

    pub fn is_search_pending(&self) -> bool {
        self.is_search_pending
    }

    pub fn completed_search_query(&self) -> Option<&str> {
        self.completed_search_query.as_deref()
    }

    /// 화면에 보일 라이브 — [`merge_search_lives`] 규칙(방송 중인 검색 채널 먼저, 중복 제거).
    pub fn merged_live_results(&self) -> Vec<LiveInfo> {
        merge_search_lives(&self.channel_lives, &self.live_results)
    }

    pub fn clip_full_channel_ids(&self) -> &HashSet<String> {
        &self.clip_full_channel_ids
    }

    pub fn clip_source_channel_ids(&self) -> &[String] {
        &self.clip_source_channel_ids
    }

    pub fn clip_source_keyword(&self) -> Option<&str> {
        self.clip_source_keyword.as_deref()
    }

    pub fn clip_next_page(&self) -> &HashMap<String, i64> {
        &self.clip_next_page
    }

    pub fn clip_exhausted(&self) -> &HashSet<String> {
        &self.clip_exhausted
    }

    pub fn loaded_buckets(&self) -> &HashSet<SearchType> {
        &self.loaded_buckets
    }

    pub fn landing_snapshot(&self) -> Option<&SearchLandingSnapshot> {
        self.landing_snapshot.as_ref()
    }

    pub fn landing_source_revision(&self) -> Option<i64> {
        self.landing_source_revision
    }

    pub fn has_pending_search_task(&self) -> bool {
        self.search_task.is_some()
    }

    pub fn sync_following_channel_names(&mut self, channels: &[LiveChannelItem]) {
        let names: Vec<String> = channels.iter().map(|c| c.channel_name.clone()).collect();
        if names != self.following_channel_names {
            self.following_channel_names = names;
        }
    }

    /// 최근 시청 채널 이름 (빈 이름 제외).
    pub fn sync_recent_watched_names(&mut self, names: &[String]) {
        let names: Vec<String> = names.iter().filter(|n| !n.is_empty()).cloned().collect();
        if names != self.recent_watched_channel_names {
            self.recent_watched_channel_names = names;
        }
    }

    pub fn show_autocomplete(&self) -> bool {
        let t = trimmed(&self.query);
        if t.is_empty() || self.completed_search_query.as_deref() == Some(t.as_str()) {
            return false;
        }
        let has_results = !self.channel_results.is_empty()
            || !self.live_results.is_empty()
            || !self.channel_lives.is_empty()
            || !self.video_results.is_empty()
            || !self.clip_results.is_empty();
        !has_results && !self.is_any_bucket_searching()
    }

    pub fn autocomplete_suggestions(&self) -> Vec<AutocompleteSuggestion> {
        let t = trimmed(&self.query);
        if t.is_empty() {
            return Vec::new();
        }
        search_scoring::autocomplete_suggestions(
            &t,
            AutocompleteSources {
                recent: &self.recent_searches,
                following: &self.following_channel_names,
                watched: &self.recent_watched_channel_names,
                popular: &[],
            },
            8,
        )
        .into_iter()
        .map(|s| {
            let kind = match s.source {
                SuggestionSource::Recent | SuggestionSource::Popular => {
                    AutocompleteSuggestionKind::Recent
                }
                SuggestionSource::Following => AutocompleteSuggestionKind::Following,
                SuggestionSource::Watched => AutocompleteSuggestionKind::Watched,
            };
            AutocompleteSuggestion::new(s.text, kind)
        })
        .collect()
    }

    pub fn is_searching(&self) -> bool {
        match self.selected_scope {
            SearchScope::All => self.is_any_bucket_searching(),
            SearchScope::Channel => self.is_searching_channels,
            SearchScope::Live => self.is_searching_lives,
            SearchScope::Video => self.is_searching_videos,
            SearchScope::Clip => self.is_searching_clips,
        }
    }

    pub fn is_any_bucket_searching(&self) -> bool {
        self.is_searching_channels
            || self.is_searching_lives
            || self.is_searching_videos
            || self.is_searching_clips
    }

    fn now(&self) -> f64 {
        self.this.upgrade().map_or(0.0, |s| (s.clock)())
    }

    pub fn current_results_are_fresh(&self) -> bool {
        self.rendered_query == trimmed(&self.query)
            && self.loaded_buckets.len() == 4
            && self.bucket_errors.is_empty()
            && self.now() - self.result_fetched_at < FRESH_WINDOW
    }
}

impl SearchState {
    // MARK: 최근 검색어

    fn save_recent_search(&mut self, keyword: &str, store: &dyn RecentSearchStore) {
        let mut searches: Vec<String> = self
            .recent_searches
            .iter()
            .filter(|s| *s != keyword)
            .cloned()
            .collect();
        searches.insert(0, keyword.to_string());
        searches.truncate(MAX_RECENT_SEARCHES);
        store.save(&searches);
        self.recent_searches = searches;
    }
}

/// 라이브 정렬.
pub fn sort_lives(lives: &mut [LiveInfo], option: LiveSortOption) {
    match option {
        LiveSortOption::ViewerCount => {
            lives.sort_by_key(|l| std::cmp::Reverse(l.concurrent_user_count))
        }
        LiveSortOption::Recent => lives.sort_by_key(|l| std::cmp::Reverse(l.open_date)),
    }
}

/// 검색 라이브 합치기 — 채널 검색에서 방송 중인 채널(채널 관련도 순)이 먼저, 그다음 라이브 검색 결과
/// (선택한 정렬 그대로). 같은 채널·같은 방송은 앞쪽 하나만 남긴다.
pub fn merge_search_lives(channel_lives: &[LiveInfo], keyword_lives: &[LiveInfo]) -> Vec<LiveInfo> {
    let mut channels = HashSet::new();
    let mut lives = HashSet::new();
    channel_lives
        .iter()
        .chain(keyword_lives)
        .filter(|l| {
            let channel_new = l
                .channel
                .as_ref()
                .is_none_or(|c| channels.insert(c.channel_id.clone()));
            lives.insert(l.live_id) && channel_new
        })
        .cloned()
        .collect()
}

/// 채널 이름 비교 키 — 대소문자·공백 무시.
pub fn channel_name_key(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

/// 채널 관련도 정렬.
pub fn sort_channels(channels: &mut [ChannelInfo], query: &str) {
    channels.sort_by(|a, b| {
        let sa = search_scoring::channel_score(&a.channel_name, a.follower_count, query);
        let sb = search_scoring::channel_score(&b.channel_name, b.follower_count, query);
        sb.total_cmp(&sa)
    });
}

/// 공유 셀.
pub struct SearchShared {
    obs: Observed<SearchState>,
    api: Arc<dyn SearchApi>,
    recent_store: Arc<dyn RecentSearchStore>,
    clock: Box<dyn Fn() -> f64 + Send + Sync>,
}

/// 직렬화 스냅샷.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSnapshot {
    pub query: String,
    pub selected_scope: SearchScope,
    pub channel_results: Vec<ChannelInfo>,
    pub live_results: Vec<LiveInfo>,
    pub video_results: Vec<VodInfo>,
    pub clip_results: Vec<ClipInfo>,
    pub is_searching: bool,
    pub is_search_pending: bool,
    pub has_more: [bool; 4],
    pub bucket_errors: HashMap<String, String>,
    pub recent_searches: Vec<String>,
    pub show_autocomplete: bool,
    pub autocomplete: Vec<String>,
}

/// 검색 뷰모델 핸들.
#[derive(Clone)]
pub struct SearchViewModel {
    shared: Arc<SearchShared>,
}

impl SearchViewModel {
    pub fn new(api: Arc<dyn SearchApi>, recent_store: Arc<dyn RecentSearchStore>) -> Self {
        let base = Instant::now();
        Self::with_clock(
            api,
            recent_store,
            Box::new(move || base.elapsed().as_secs_f64()),
        )
    }

    /// `cacheClock` 주입 (테스트).
    pub fn with_clock(
        api: Arc<dyn SearchApi>,
        recent_store: Arc<dyn RecentSearchStore>,
        clock: Box<dyn Fn() -> f64 + Send + Sync>,
    ) -> Self {
        let recent = recent_store.load().unwrap_or_default();
        let shared = Arc::new_cyclic(|this| SearchShared {
            obs: Observed::new(SearchState {
                this: this.clone(),
                query: String::new(),
                selected_scope: SearchScope::All,
                selected_channel_id: None,
                channel_results: Vec::new(),
                live_results: Vec::new(),
                channel_lives: Vec::new(),
                video_results: Vec::new(),
                clip_results: Vec::new(),
                is_searching_channels: false,
                is_searching_lives: false,
                is_searching_videos: false,
                is_searching_clips: false,
                has_more_channels: true,
                has_more_lives: true,
                has_more_videos: true,
                has_more_clips: true,
                error_message: None,
                bucket_errors: HashMap::new(),
                recent_searches: recent,
                live_sort_option: LiveSortOption::ViewerCount,
                following_channel_names: Vec::new(),
                recent_watched_channel_names: Vec::new(),
                is_search_pending: false,
                completed_search_query: None,
                search_task: None,
                result_cache: ResultCache::new(RESULT_CACHE_CAPACITY),
                rendered_query: String::new(),
                performing_query: None,
                result_fetched_at: 0.0,
                loaded_buckets: HashSet::new(),
                active_search_id: new_search_id(),
                channel_offset: 0,
                live_offset: 0,
                video_offset: 0,
                clip_page: 0,
                clip_source_channel_ids: Vec::new(),
                clip_source_keyword: None,
                clip_next_page: HashMap::new(),
                clip_cursor: HashMap::new(),
                clip_exhausted: HashSet::new(),
                clip_full_channel_ids: HashSet::new(),
                landing_snapshot: None,
                landing_source_revision: None,
            }),
            api,
            recent_store,
            clock,
        });
        Self { shared }
    }

    pub fn lock(&self) -> ObservedGuard<'_, SearchState> {
        self.shared.obs.lock()
    }

    pub fn read(&self) -> ObservedGuard<'_, SearchState> {
        self.shared.obs.read()
    }

    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.shared.obs.subscribe()
    }

    pub fn snapshot(&self) -> SearchSnapshot {
        let st = self.read();
        let mut errors: HashMap<String, String> = HashMap::new();
        for (k, v) in &st.bucket_errors {
            errors.insert(
                SearchScope::from_search_type(*k).raw_value().into(),
                v.clone(),
            );
        }
        SearchSnapshot {
            query: st.query.clone(),
            selected_scope: st.selected_scope,
            channel_results: st.channel_results.clone(),
            live_results: st.merged_live_results(),
            video_results: st.video_results.clone(),
            clip_results: st.clip_results.clone(),
            is_searching: st.is_searching(),
            is_search_pending: st.is_search_pending,
            has_more: [
                st.has_more_channels,
                st.has_more_lives,
                st.has_more_videos,
                st.has_more_clips,
            ],
            bucket_errors: errors,
            recent_searches: st.recent_searches.clone(),
            show_autocomplete: st.show_autocomplete(),
            autocomplete: st
                .autocomplete_suggestions()
                .into_iter()
                .map(|s| s.text)
                .collect(),
        }
    }

    /// `query = ...` (didSet → scheduleSearch).
    pub fn set_query(&self, query: impl Into<String>) {
        let mut st = self.lock();
        st.query = query.into();
        self.schedule_search_locked(&mut st);
    }

    /// 대기 중인 디바운스 작업을 취소한다 (테스트의 `searchTask?.cancel(); searchTask = nil`).
    pub fn cancel_pending_search(&self) {
        cancel_slot(&mut self.lock().search_task);
    }

    fn schedule_search_locked(&self, st: &mut SearchState) {
        cancel_slot(&mut st.search_task);
        st.save_snapshot();
        let t = trimmed(&st.query);
        st.clear_results();
        st.loaded_buckets.clear();
        st.performing_query = None;
        st.rendered_query = t.clone();
        if t.is_empty() {
            return;
        }
        if st.restore_snapshot(&t) && st.current_results_are_fresh() {
            st.save_recent_search(&t, self.shared.recent_store.as_ref());
            st.completed_search_query = Some(t);
            return;
        }
        st.is_search_pending = true;
        let weak = Arc::downgrade(&self.shared);
        st.search_task = Some(TaskSlot::spawn(move |id| async move {
            tokio::time::sleep(SEARCH_DEBOUNCE).await;
            let Some(shared) = weak.upgrade() else { return };
            {
                let mut st = shared.obs.lock();
                if !slot_is(&st.search_task, id) {
                    return;
                }
                st.search_task = None;
            }
            SearchViewModel { shared }.perform_search(false).await;
        }));
    }

    /// 네 버킷을 동시에 검색한다. 15초 안의 같은 결과는 다시 받지 않는다.
    pub async fn perform_search(&self, force_refresh: bool) {
        let (t, token) = {
            let mut st = self.lock();
            cancel_slot(&mut st.search_task);
            st.is_search_pending = false;
            let t = trimmed(&st.query);
            if t.is_empty() {
                return;
            }
            st.save_recent_search(&t, self.shared.recent_store.as_ref());
            if !force_refresh && st.current_results_are_fresh() {
                return;
            }
            if st.performing_query.as_deref() == Some(t.as_str()) && !force_refresh {
                return;
            }
            st.error_message = None;
            st.bucket_errors.clear();
            let token = new_search_id();
            st.active_search_id = token;
            st.performing_query = Some(t.clone());
            st.channel_offset = 0;
            st.live_offset = 0;
            st.video_offset = 0;
            st.clip_page = 0;
            st.reset_clip_sources();
            st.has_more_channels = true;
            st.has_more_lives = true;
            st.has_more_videos = true;
            st.has_more_clips = true;
            if st.rendered_query != t {
                st.channel_results.clear();
                st.live_results.clear();
                st.channel_lives.clear();
                st.video_results.clear();
                st.clip_results.clear();
            }
            st.rendered_query = t.clone();
            st.loaded_buckets.clear();
            st.result_fetched_at = st.now();
            st.is_searching_channels = true;
            st.is_searching_lives = true;
            st.is_searching_videos = true;
            st.is_searching_clips = true;
            (t, token)
        };
        tokio::join!(
            self.fetch_channels(&t, false, token),
            self.fetch_lives(&t, false, token),
            self.fetch_videos(&t, false, token),
            self.fetch_clips(&t, false, token, CLIP_FIRST_WAVE_CHANNELS),
        );
        let mut st = self.lock();
        if st.active_search_id == token {
            st.performing_query = None;
            st.result_fetched_at = st.now();
            st.completed_search_query = Some(t);
            st.save_snapshot();
        }
    }

    /// 화면 재진입 — 300초 안의 완결 결과는 재사용.
    pub async fn resume_on_activation(&self) {
        {
            let st = self.read();
            let t = trimmed(&st.query);
            if t.is_empty() {
                return;
            }
            let loaded = st.rendered_query == t
                && st.loaded_buckets.len() == 4
                && st.bucket_errors.is_empty();
            if loaded && st.now() - st.result_fetched_at < ACTIVATION_REUSE_WINDOW {
                return;
            }
        }
        self.perform_search(false).await;
    }

    pub async fn on_scope_changed(&self, scope: SearchScope) {
        enum Next {
            Done,
            Full,
            Expand,
            Fetch(SearchType),
        }
        let (t, token, next) = {
            let mut st = self.lock();
            st.selected_scope = scope;
            let t = trimmed(&st.query);
            if t.is_empty() {
                return;
            }
            let token = st.active_search_id;
            let next = if st.loaded_buckets.is_empty() && !st.is_any_bucket_searching() {
                Next::Full
            } else if scope.search_type().is_some_and(|ty| {
                st.loaded_buckets.contains(&ty) && !st.bucket_errors.contains_key(&ty)
            }) {
                Next::Expand
            } else {
                match scope {
                    SearchScope::Channel
                        if st.channel_results.is_empty() && !st.is_searching_channels =>
                    {
                        Next::Fetch(SearchType::Channel)
                    }
                    SearchScope::Live if st.live_results.is_empty() && !st.is_searching_lives => {
                        Next::Fetch(SearchType::Live)
                    }
                    SearchScope::Video
                        if st.video_results.is_empty() && !st.is_searching_videos =>
                    {
                        Next::Fetch(SearchType::Video)
                    }
                    SearchScope::Clip if st.clip_results.is_empty() && !st.is_searching_clips => {
                        Next::Fetch(SearchType::Clip)
                    }
                    _ => Next::Done,
                }
            };
            (t, token, next)
        };
        match next {
            Next::Done => {}
            Next::Full => self.perform_search(false).await,
            Next::Expand => self.expand_clip_sources_if_needed(scope, &t, token).await,
            Next::Fetch(ty) => self.fetch_bucket(ty, &t, true, token).await,
        }
    }
}

impl SearchViewModel {
    /// 결과 캐시를 비우고 다시 예약한다.
    pub fn invalidate_result_cache(&self) {
        let mut st = self.lock();
        cancel_slot(&mut st.search_task);
        st.result_cache.clear();
        st.clear_results();
        st.rendered_query.clear();
        st.loaded_buckets.clear();
        st.performing_query = None;
        self.schedule_search_locked(&mut st);
    }

    pub fn remove_recent_search(&self, keyword: &str) {
        let mut st = self.lock();
        st.recent_searches.retain(|s| s != keyword);
        self.shared.recent_store.save(&st.recent_searches);
    }

    pub fn clear_recent_searches(&self) {
        let mut st = self.lock();
        st.recent_searches.clear();
        self.shared.recent_store.remove();
    }

    /// 랜딩 스냅샷 갱신 — 같은 revision 이면 건너뛰고, 계산 중 원천이 바뀌면 최대 3회 다시 계산.
    pub async fn refresh_landing_snapshot<I, S, C, Fut>(&self, source: S, compute: C)
    where
        S: Fn() -> (i64, I),
        C: Fn(I) -> Fut,
        Fut: Future<Output = SearchLandingSnapshot>,
    {
        let mut latest: Option<(i64, SearchLandingSnapshot)> = None;
        for _ in 0..LANDING_RECOMPUTE_LIMIT {
            let (revision, input) = source();
            {
                let st = self.read();
                if st.landing_snapshot.is_some() && st.landing_source_revision == Some(revision) {
                    return;
                }
            }
            let computed = compute(input).await;
            latest = Some((revision, computed.clone()));
            if source().0 == revision {
                let mut st = self.lock();
                st.landing_snapshot = Some(computed);
                st.landing_source_revision = Some(revision);
                return;
            }
        }
        if let Some((revision, snapshot)) = latest {
            let mut st = self.lock();
            st.landing_snapshot = Some(snapshot);
            st.landing_source_revision = Some(revision);
        }
    }
}

fn bucket_fields(st: &mut SearchState, ty: SearchType) -> (&mut i64, &mut bool, &mut bool) {
    match ty {
        SearchType::Channel => (
            &mut st.channel_offset,
            &mut st.has_more_channels,
            &mut st.is_searching_channels,
        ),
        SearchType::Live => (
            &mut st.live_offset,
            &mut st.has_more_lives,
            &mut st.is_searching_lives,
        ),
        SearchType::Video => (
            &mut st.video_offset,
            &mut st.has_more_videos,
            &mut st.is_searching_videos,
        ),
        SearchType::Clip => (
            &mut st.clip_page,
            &mut st.has_more_clips,
            &mut st.is_searching_clips,
        ),
    }
}
