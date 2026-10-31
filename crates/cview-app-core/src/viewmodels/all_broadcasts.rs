//! 전체 방송 — 인기순 피드 페이지와 라이브 검색.
//!
//! 원본: `Sources/CViewApp/ViewModels/AllBroadcastsViewModel.swift`.
//! `containerWidth`·검색창 포커스 같은 뷰 상태는 버리지 않고 값으로만 둔다.

use std::collections::HashSet;
use std::sync::{Arc, Weak};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use serde::Serialize;

use cview_core::models::search::SearchResult;
use cview_core::models::LiveInfo;
use cview_core::utils::data_freshness::FreshnessPolicy;
use cview_core::utils::time::Date;
use cview_networking::response::LivePageCursor;
use cview_networking::{AgedResponse, ChzzkApiClient, PagedContent};

use super::home::promotion::{DataOrigin, FreshnessDisplay};
use super::support::{cancel_slot, slot_is, Observed, ObservedGuard, TaskSlot};

/// 이 화면이 부르는 API.
#[async_trait]
pub trait AllBroadcastsApi: Send + Sync {
    async fn top_lives_with_age(
        &self,
        size: i64,
    ) -> Result<AgedResponse<PagedContent<LiveInfo>>, String>;
    async fn top_lives(
        &self,
        size: i64,
        concurrent_user_count: Option<i64>,
        live_id: Option<i64>,
    ) -> Result<PagedContent<LiveInfo>, String>;
    async fn search_lives(
        &self,
        keyword: &str,
        offset: i64,
        size: i64,
    ) -> Result<SearchResult<LiveInfo>, String>;
}

#[async_trait]
impl AllBroadcastsApi for ChzzkApiClient {
    async fn top_lives_with_age(
        &self,
        size: i64,
    ) -> Result<AgedResponse<PagedContent<LiveInfo>>, String> {
        ChzzkApiClient::top_lives_with_age(self, size, None, None, false)
            .await
            .map_err(|e| e.to_string())
    }
    async fn top_lives(
        &self,
        size: i64,
        concurrent_user_count: Option<i64>,
        live_id: Option<i64>,
    ) -> Result<PagedContent<LiveInfo>, String> {
        ChzzkApiClient::top_lives(self, size, concurrent_user_count, live_id, false)
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
pub enum LoadState {
    Idle,
    Loading,
    Loaded,
    Empty,
    Error(String),
}

/// 인기(전체방송) 화면 모드 — 사이드바 빠른 작업도 이 값을 쓴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AllBroadcastsMode {
    All,
    Search,
}

impl AllBroadcastsMode {
    pub fn title(self) -> &'static str {
        match self {
            Self::All => "전체",
            Self::Search => "검색",
        }
    }
}

pub struct AllBroadcastsState {
    this: Weak<AllBroadcastsShared>,
    pub mode: AllBroadcastsMode,
    pub feed: Vec<LiveInfo>,
    pub state: LoadState,
    pub is_loading_more: bool,
    pub can_load_more: bool,
    pub preview_live: Option<LiveInfo>,
    pub container_width: f64,
    pub page: usize,
    next_cursor: Option<LivePageCursor>,
    seen_live_ids: HashSet<i64>,
    pub search_query: String,
    pub search_results: Vec<LiveInfo>,
    pub search_state: LoadState,
    pub search_page: usize,
    pub is_loading_more_search: bool,
    pub search_can_load_more: bool,
    search_next_offset: i64,
    search_seen_ids: HashSet<i64>,
    search_task: Option<TaskSlot>,
    search_generation: u64,
    pub is_refreshing: bool,
    pub is_searching: bool,
    pending_search_focus: bool,
    initial_load: Option<tokio::sync::watch::Receiver<bool>>,
    pub feed_fetched_at: Option<Date>,
    feed_generation: u64,
}

pub struct AllBroadcastsShared {
    obs: Observed<AllBroadcastsState>,
    api: Arc<dyn AllBroadcastsApi>,
}

#[derive(Clone)]
pub struct AllBroadcastsViewModel {
    shared: Arc<AllBroadcastsShared>,
}

fn page_slice(items: &[LiveInfo], page: usize, per_page: usize) -> Vec<LiveInfo> {
    let start = page * per_page;
    if start >= items.len() {
        return Vec::new();
    }
    items[start..(start + per_page).min(items.len())].to_vec()
}

impl AllBroadcastsState {
    pub const PER_PAGE: usize = 30;

    pub fn current_page_items(&self) -> Vec<LiveInfo> {
        page_slice(&self.feed, self.page, Self::PER_PAGE)
    }
    pub fn page_count(&self) -> usize {
        1.max(self.feed.len().div_ceil(Self::PER_PAGE))
    }
    pub fn has_previous_page(&self) -> bool {
        self.page > 0
    }
    pub fn has_next_page(&self) -> bool {
        (self.page + 1) * Self::PER_PAGE < self.feed.len() || self.can_load_more
    }
    fn active_items(&self) -> &[LiveInfo] {
        if self.mode == AllBroadcastsMode::All {
            &self.feed
        } else {
            &self.search_results
        }
    }
    pub fn display_can_load_more(&self) -> bool {
        if self.mode == AllBroadcastsMode::All {
            self.can_load_more
        } else {
            self.search_can_load_more
        }
    }
    pub fn display_state(&self) -> &LoadState {
        if self.mode == AllBroadcastsMode::All {
            &self.state
        } else {
            &self.search_state
        }
    }
    pub fn display_page(&self) -> usize {
        if self.mode == AllBroadcastsMode::All {
            self.page
        } else {
            self.search_page
        }
    }
    pub fn display_items(&self) -> Vec<LiveInfo> {
        page_slice(self.active_items(), self.display_page(), Self::PER_PAGE)
    }
    pub fn display_page_count(&self) -> usize {
        let loaded = 1.max(self.active_items().len().div_ceil(Self::PER_PAGE));
        if self.display_can_load_more() {
            loaded + 1
        } else {
            loaded
        }
    }
    pub fn display_has_previous_page(&self) -> bool {
        self.display_page() > 0
    }
    pub fn display_has_next_page(&self) -> bool {
        (self.display_page() + 1) * Self::PER_PAGE < self.active_items().len()
            || self.display_can_load_more()
    }
    pub fn display_is_loading_more(&self) -> bool {
        if self.mode == AllBroadcastsMode::All {
            self.is_loading_more
        } else {
            self.is_loading_more_search
        }
    }
    pub fn display_is_busy(&self) -> bool {
        if self.mode == AllBroadcastsMode::All {
            self.is_refreshing
        } else {
            self.is_refreshing || self.is_searching
        }
    }
    pub fn feed_freshness(&self) -> FreshnessDisplay {
        FreshnessDisplay::make(
            self.feed_fetched_at.as_ref(),
            DataOrigin::Network,
            &FreshnessPolicy::POPULAR,
            &Utc::now(),
        )
    }

    pub fn set_mode(&mut self, new_mode: AllBroadcastsMode) {
        if self.mode == new_mode {
            return;
        }
        self.mode = new_mode;
        self.preview_live = None;
        if new_mode == AllBroadcastsMode::Search {
            self.pending_search_focus = true;
        }
    }

    pub fn consume_pending_search_focus(&mut self) -> bool {
        std::mem::take(&mut self.pending_search_focus)
    }

    pub fn previous_page(&mut self) {
        if self.has_previous_page() {
            self.page -= 1;
        }
    }

    pub fn previous_search_page(&mut self) {
        if self.search_page > 0 {
            self.search_page -= 1;
        }
    }

    pub fn previous_display_page(&mut self) {
        if self.mode == AllBroadcastsMode::All {
            self.previous_page()
        } else {
            self.previous_search_page()
        }
    }

    fn dedupe(&mut self, items: Vec<LiveInfo>) -> Vec<LiveInfo> {
        items
            .into_iter()
            .filter(|i| self.seen_live_ids.insert(i.live_id))
            .collect()
    }

    fn dedupe_search(&mut self, items: Vec<LiveInfo>) -> Vec<LiveInfo> {
        items
            .into_iter()
            .filter(|i| self.search_seen_ids.insert(i.live_id))
            .collect()
    }
}

fn trimmed_keyword(q: &str) -> Option<String> {
    let t = q.trim();
    (t.chars().count() >= 2).then(|| t.to_string())
}

impl AllBroadcastsViewModel {
    pub const PER_PAGE: usize = AllBroadcastsState::PER_PAGE;
    /// 한 번에 받는 개수 — 치지직 `/service/v1/lives` 가 받아 주는 최대(51 이상은 400).
    /// 화면 쪽 페이지(30개)와 따로 둔다: 30개씩 받으면 홈(48개)·사이드바(50개)가 첫 화면 전에
    /// 요청을 두 번 차례로 기다렸다(실측 한 번 0.88초 ↔ 50개 한 번 0.95초).
    pub const FETCH_SIZE: usize = 50;
    pub const SEARCH_DEBOUNCE: Duration = Duration::from_millis(300);

    pub fn new(api: Arc<dyn AllBroadcastsApi>) -> Self {
        let shared = Arc::new_cyclic(|weak| AllBroadcastsShared {
            obs: Observed::new(AllBroadcastsState {
                this: weak.clone(),
                mode: AllBroadcastsMode::All,
                feed: Vec::new(),
                state: LoadState::Idle,
                is_loading_more: false,
                can_load_more: false,
                preview_live: None,
                container_width: 900.0,
                page: 0,
                next_cursor: None,
                seen_live_ids: HashSet::new(),
                search_query: String::new(),
                search_results: Vec::new(),
                search_state: LoadState::Idle,
                search_page: 0,
                is_loading_more_search: false,
                search_can_load_more: false,
                search_next_offset: 0,
                search_seen_ids: HashSet::new(),
                search_task: None,
                search_generation: 0,
                is_refreshing: false,
                is_searching: false,
                pending_search_focus: false,
                initial_load: None,
                feed_fetched_at: None,
                feed_generation: 0,
            }),
            api,
        });
        Self { shared }
    }

    pub fn lock(&self) -> ObservedGuard<'_, AllBroadcastsState> {
        self.shared.obs.lock()
    }
    pub fn read(&self) -> ObservedGuard<'_, AllBroadcastsState> {
        self.shared.obs.read()
    }
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.shared.obs.subscribe()
    }
    pub fn snapshot(&self) -> AllBroadcastsSnapshot {
        AllBroadcastsSnapshot::from_state(&self.read())
    }

    pub async fn go_to_display_page(&self, target: i64) {
        if self.read().mode == AllBroadcastsMode::All {
            self.go_to_page(target).await
        } else {
            self.go_to_search_page(target).await
        }
    }

    pub async fn next_display_page(&self) {
        let p = self.read().display_page() as i64;
        self.go_to_display_page(p + 1).await;
    }

    pub async fn refresh_current(&self) {
        let (mode, searching, query) = {
            let s = self.read();
            if s.is_refreshing {
                return;
            }
            (s.mode, s.is_searching, s.search_query.clone())
        };
        if mode == AllBroadcastsMode::All {
            self.reload().await;
        } else {
            if searching {
                return;
            }
            if let Some(q) = trimmed_keyword(&query) {
                self.perform_search(q).await;
            }
        }
    }

    /// 첫 페이지 그대로인 피드만 새로 받는다.
    pub async fn refresh_first_page_if_pristine(&self) -> bool {
        {
            let s = self.read();
            if s.mode != AllBroadcastsMode::All
                || s.page != 0
                || s.is_loading_more
                || s.is_refreshing
                || !(s.state == LoadState::Loaded || s.state == LoadState::Empty)
                || s.feed.len() > Self::FETCH_SIZE
            {
                return false;
            }
        }
        self.reload().await;
        true
    }

    pub async fn load_initial_if_needed(&self) {
        let existing = self.read().initial_load.clone();
        if let Some(mut rx) = existing {
            let _ = rx.wait_for(|d| *d).await;
            return;
        }
        let tx = {
            let mut s = self.lock();
            if s.state != LoadState::Idle {
                return;
            }
            let (tx, rx) = tokio::sync::watch::channel(false);
            s.initial_load = Some(rx);
            tx
        };
        self.reload().await;
        let _ = tx.send(true);
        self.lock().initial_load = None;
    }

    pub async fn reload(&self) {
        let generation = {
            let mut s = self.lock();
            s.feed_generation = s.feed_generation.wrapping_add(1);
            s.page = 0;
            if s.feed.is_empty() {
                s.state = LoadState::Loading;
            }
            s.is_refreshing = true;
            s.feed_generation
        };
        let result = self
            .shared
            .api
            .top_lives_with_age(Self::FETCH_SIZE as i64)
            .await;
        let mut s = self.lock();
        if generation != s.feed_generation {
            return;
        }
        match result {
            Ok(aged) => {
                s.feed_fetched_at = Some(aged.fetched_at.min(Utc::now()));
                let mut seen = HashSet::new();
                let fresh: Vec<LiveInfo> = aged
                    .value
                    .data
                    .into_iter()
                    .filter(|i| seen.insert(i.live_id))
                    .collect();
                s.seen_live_ids = seen;
                s.page = 0;
                let n = fresh.len();
                s.feed = fresh;
                s.next_cursor = aged.value.page.and_then(|p| p.next);
                s.can_load_more = s.next_cursor.is_some();
                s.state = if s.feed.is_empty() {
                    LoadState::Empty
                } else {
                    LoadState::Loaded
                };
                tracing::debug!("전체방송 첫 페이지 로드: {n}개, more={}", s.can_load_more);
            }
            Err(e) => {
                if s.feed.is_empty() {
                    s.state = LoadState::Error(e.clone());
                }
                tracing::error!("전체방송 로드 실패: {e}");
            }
        }
        s.is_refreshing = false;
    }

    pub async fn load_more(&self) {
        let (generation, cursor) = {
            let mut s = self.lock();
            if !s.can_load_more || s.is_loading_more || s.state != LoadState::Loaded {
                return;
            }
            s.is_loading_more = true;
            (s.feed_generation, s.next_cursor)
        };
        let result = self
            .shared
            .api
            .top_lives(
                Self::FETCH_SIZE as i64,
                cursor.as_ref().and_then(|c| c.concurrent_user_count),
                cursor.as_ref().and_then(|c| c.live_id),
            )
            .await;
        let mut s = self.lock();
        s.is_loading_more = false;
        match result {
            Ok(response) => {
                if generation != s.feed_generation {
                    return;
                }
                let fresh = s.dedupe(response.data);
                s.feed.extend(fresh);
                s.next_cursor = response.page.and_then(|p| p.next);
                s.can_load_more = s.next_cursor.is_some();
            }
            Err(e) => tracing::error!("전체방송 더보기 실패: {e}"),
        }
    }

    pub async fn go_to_page(&self, target: i64) {
        if target < 0 {
            return;
        }
        let target = target as usize;
        let generation = self.read().feed_generation;
        let mut guard_count = 0;
        loop {
            let before = {
                let s = self.read();
                if !(target * Self::PER_PAGE >= s.feed.len()
                    && s.can_load_more
                    && !s.is_loading_more
                    && guard_count < 100)
                {
                    break;
                }
                s.feed.len()
            };
            self.load_more().await;
            if generation != self.read().feed_generation {
                return;
            }
            guard_count += 1;
            if self.read().feed.len() == before {
                break;
            }
        }
        let mut s = self.lock();
        if generation != s.feed_generation || target * Self::PER_PAGE >= s.feed.len() {
            return;
        }
        s.page = target;
    }

    pub fn previous_page(&self) {
        self.lock().previous_page();
    }

    pub fn previous_display_page(&self) {
        self.lock().previous_display_page();
    }

    pub fn set_mode(&self, mode: AllBroadcastsMode) {
        self.lock().set_mode(mode);
    }

    /// 검색어 입력 — 2자 미만이면 결과를 비우고, 아니면 300ms 뒤 검색한다.
    pub fn update_search_query(&self, query: &str) {
        let mut s = self.lock();
        s.search_query = query.to_string();
        cancel_slot(&mut s.search_task);
        s.search_generation = s.search_generation.wrapping_add(1);
        let Some(trimmed) = trimmed_keyword(query) else {
            s.search_results = Vec::new();
            s.search_state = LoadState::Idle;
            s.search_can_load_more = false;
            s.search_page = 0;
            s.is_searching = false;
            return;
        };
        let weak = s.this.clone();
        s.search_task = Some(TaskSlot::spawn(move |id| async move {
            tokio::time::sleep(Self::SEARCH_DEBOUNCE).await;
            let Some(shared) = weak.upgrade() else { return };
            if !slot_is(&shared.obs.read().search_task, id) {
                return;
            }
            Self { shared }.perform_search(trimmed).await;
        }));
    }

    pub fn submit_search(&self) {
        let trimmed = {
            let mut s = self.lock();
            cancel_slot(&mut s.search_task);
            s.search_generation = s.search_generation.wrapping_add(1);
            trimmed_keyword(&s.search_query)
        };
        let Some(trimmed) = trimmed else { return };
        let vm = self.clone();
        if let Ok(h) = tokio::runtime::Handle::try_current() {
            h.spawn(async move { vm.perform_search(trimmed).await });
        }
    }

    pub async fn perform_search(&self, keyword: String) {
        let generation = {
            let mut s = self.lock();
            s.search_generation = s.search_generation.wrapping_add(1);
            s.search_page = 0;
            s.search_next_offset = 0;
            s.search_seen_ids.clear();
            if s.search_results.is_empty() {
                s.search_state = LoadState::Loading;
            }
            s.is_searching = true;
            s.search_generation
        };
        let result = self
            .shared
            .api
            .search_lives(&keyword, 0, Self::PER_PAGE as i64)
            .await;
        let mut s = self.lock();
        s.is_searching = false;
        if generation != s.search_generation {
            return;
        }
        match result {
            Ok(r) => {
                let received = r.data.len() as i64;
                let has_next =
                    r.page.as_ref().and_then(|p| p.next.as_ref()).is_some() && received > 0;
                let fresh = s.dedupe_search(r.data);
                let n = fresh.len();
                s.search_results = fresh;
                s.search_next_offset = received;
                s.search_can_load_more = has_next;
                s.search_state = if n == 0 {
                    LoadState::Empty
                } else {
                    LoadState::Loaded
                };
                tracing::debug!("전체방송 검색 '{keyword}': {n}개, more={has_next}");
            }
            Err(e) => {
                if s.search_results.is_empty() {
                    s.search_state = LoadState::Error(e.clone());
                }
                tracing::error!("전체방송 검색 실패: {e}");
            }
        }
    }

    async fn load_more_search(&self) {
        let (keyword, offset, generation) = {
            let mut s = self.lock();
            if !s.search_can_load_more
                || s.is_loading_more_search
                || s.search_state != LoadState::Loaded
            {
                return;
            }
            let Some(keyword) = trimmed_keyword(&s.search_query) else {
                return;
            };
            s.is_loading_more_search = true;
            (keyword, s.search_next_offset, s.search_generation)
        };
        let result = self
            .shared
            .api
            .search_lives(&keyword, offset, Self::PER_PAGE as i64)
            .await;
        let mut s = self.lock();
        s.is_loading_more_search = false;
        match result {
            Ok(r) => {
                if generation != s.search_generation {
                    return;
                }
                let received = r.data.len() as i64;
                let has_next =
                    r.page.as_ref().and_then(|p| p.next.as_ref()).is_some() && received > 0;
                let fresh = s.dedupe_search(r.data);
                s.search_results.extend(fresh);
                s.search_next_offset += received;
                s.search_can_load_more = has_next;
            }
            Err(e) => tracing::error!("전체방송 검색 더보기 실패: {e}"),
        }
    }

    pub async fn go_to_search_page(&self, target: i64) {
        if target < 0 {
            return;
        }
        let target = target as usize;
        let mut guard_count = 0;
        loop {
            let before = {
                let s = self.read();
                if !(target * Self::PER_PAGE >= s.search_results.len()
                    && s.search_can_load_more
                    && !s.is_loading_more_search
                    && guard_count < 100)
                {
                    break;
                }
                s.search_results.len()
            };
            self.load_more_search().await;
            guard_count += 1;
            if self.read().search_results.len() == before {
                break;
            }
        }
        let mut s = self.lock();
        if target * Self::PER_PAGE >= s.search_results.len() {
            return;
        }
        s.search_page = target;
    }
}

/// 직렬화 스냅샷.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AllBroadcastsSnapshot {
    pub mode: AllBroadcastsMode,
    pub display_items: Vec<LiveInfo>,
    pub display_state: LoadState,
    pub display_page: usize,
    pub display_page_count: usize,
    pub display_has_previous_page: bool,
    pub display_has_next_page: bool,
    pub display_is_loading_more: bool,
    pub display_is_busy: bool,
    pub search_query: String,
    pub preview_live: Option<LiveInfo>,
    pub feed_freshness: FreshnessDisplay,
}

impl AllBroadcastsSnapshot {
    pub fn from_state(s: &AllBroadcastsState) -> Self {
        Self {
            mode: s.mode,
            display_items: s.display_items(),
            display_state: s.display_state().clone(),
            display_page: s.display_page(),
            display_page_count: s.display_page_count(),
            display_has_previous_page: s.display_has_previous_page(),
            display_has_next_page: s.display_has_next_page(),
            display_is_loading_more: s.display_is_loading_more(),
            display_is_busy: s.display_is_busy(),
            search_query: s.search_query.clone(),
            preview_live: s.preview_live.clone(),
            feed_freshness: s.feed_freshness(),
        }
    }
}
