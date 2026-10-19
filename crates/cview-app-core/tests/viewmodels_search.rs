//! SearchViewModel 테스트.
//!
//! 원본: `Tests/CViewAppTests/SearchRequestBudgetTests.swift`, `SearchLandingSnapshotTests.swift`,
//! `SearchAutocompleteTests.swift` 일부. HTTP 트랜스포트 대신 요청을 세는 API 가짜를 쓰고,
//! 응답 캐시(같은 키 재요청 병합)는 가짜 안의 "캐시 세대" 로 흉내 낸다.

use std::collections::HashSet;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;

use cview_app_core::viewmodels::search::{
    MemoryRecentSearchStore, RecentSearchStore, ResultCache, SearchApi, SearchLandingSnapshot,
    SearchScope, SearchViewModel,
};
use cview_core::models::search::{
    AutocompleteSuggestionKind, ClipInfo, LiveSortOption, SearchResult, VodInfo,
};
use cview_core::models::{ChannelInfo, LiveChannelItem, LiveInfo};
use cview_networking::PagedContent;

#[derive(Default)]
struct FakeApi {
    epoch: AtomicUsize,
    channel_keys: Mutex<HashSet<(String, i64, usize)>>,
    lives: AtomicUsize,
    videos: AtomicUsize,
    clip_requests: Mutex<Vec<(String, i64)>>,
}

impl FakeApi {
    fn channel_searches(&self) -> usize {
        self.channel_keys.lock().unwrap().len()
    }
    fn clip_set(&self) -> HashSet<(String, i64)> {
        self.clip_requests.lock().unwrap().iter().cloned().collect()
    }
    fn total(&self) -> usize {
        self.channel_searches()
            + self.lives.load(Ordering::SeqCst)
            + self.videos.load(Ordering::SeqCst)
            + self.clip_requests.lock().unwrap().len()
    }
    /// API 응답 캐시 만료 (Swift `apiClock.advance(120)`).
    fn expire_cache(&self) {
        self.epoch.fetch_add(1, Ordering::SeqCst);
    }
}

fn result<T>(data: Vec<T>) -> SearchResult<T> {
    SearchResult {
        size: data.len() as i64,
        page: None,
        total_count: Some(data.len() as i64),
        data,
    }
}

fn channel(id: &str, name: &str, followers: i64) -> ChannelInfo {
    let mut c = ChannelInfo::new(id, name);
    c.follower_count = followers;
    c
}

#[async_trait]
impl SearchApi for FakeApi {
    async fn search_channels(
        &self,
        keyword: &str,
        offset: i64,
        _size: i64,
    ) -> Result<SearchResult<ChannelInfo>, String> {
        let epoch = self.epoch.load(Ordering::SeqCst);
        self.channel_keys
            .lock()
            .unwrap()
            .insert((keyword.to_string(), offset, epoch));
        Ok(result(vec![
            channel("c3", "전설의 리그 팬", 10),
            channel("c1", "리그", 1_000),
            channel("c2", "리그 방송", 5),
        ]))
    }
    async fn search_lives(
        &self,
        _k: &str,
        _o: i64,
        _s: i64,
    ) -> Result<SearchResult<LiveInfo>, String> {
        self.lives.fetch_add(1, Ordering::SeqCst);
        let mut a = LiveInfo::new(1, "a");
        a.concurrent_user_count = 5;
        let mut b = LiveInfo::new(2, "b");
        b.concurrent_user_count = 50;
        Ok(result(vec![a, b]))
    }
    async fn search_videos(
        &self,
        _k: &str,
        _o: i64,
        _s: i64,
    ) -> Result<SearchResult<VodInfo>, String> {
        self.videos.fetch_add(1, Ordering::SeqCst);
        Ok(result(vec![VodInfo::new(7, "영상")]))
    }
    async fn clip_list(
        &self,
        channel_id: &str,
        page: i64,
        size: i64,
    ) -> Result<PagedContent<ClipInfo>, String> {
        self.clip_requests
            .lock()
            .unwrap()
            .push((channel_id.to_string(), page));
        let count = if channel_id == "c3" { size } else { 5 };
        let data: Vec<ClipInfo> = (0..count)
            .map(|i| {
                ClipInfo::new(
                    format!("{channel_id}-p{page}-{i}"),
                    format!("리그 하이라이트 {i}"),
                )
            })
            .collect();
        Ok(PagedContent::new(data.len() as i64, None, data, None))
    }
}

struct Harness {
    vm: SearchViewModel,
    api: Arc<FakeApi>,
    now: Arc<AtomicI64>,
}

const KEYWORD: &str = "리그";

fn harness() -> Harness {
    let api = Arc::new(FakeApi::default());
    let now = Arc::new(AtomicI64::new(10_000));
    let clock = now.clone();
    let vm = SearchViewModel::with_clock(
        api.clone(),
        Arc::new(MemoryRecentSearchStore::default()),
        Box::new(move || clock.load(Ordering::SeqCst) as f64),
    );
    Harness { vm, api, now }
}

async fn search(h: &Harness, text: &str) {
    h.vm.set_query(text);
    h.vm.cancel_pending_search();
    h.vm.perform_search(false).await;
}

fn leave_and_return(h: &Harness) {
    h.vm.set_query("다른 검색어");
    h.vm.cancel_pending_search();
    h.vm.set_query(KEYWORD);
    h.vm.cancel_pending_search();
}

fn req(id: &str, page: i64) -> (String, i64) {
    (id.to_string(), page)
}

#[tokio::test]
async fn cold_search_is_four_requests() {
    let h = harness();
    search(&h, KEYWORD).await;
    assert_eq!(
        h.api.channel_searches(),
        1,
        "클립 소스 조회가 채널 검색과 병합되지 않았다"
    );
    assert_eq!(h.api.clip_set(), [req("c3", 0)].into_iter().collect());
    assert!(h.api.total() <= 4);
    let st = h.vm.read();
    assert_eq!(st.clip_source_channel_ids(), ["c3", "c1", "c2"]);
    assert_eq!(st.channel_results[0].channel_id, "c1");
    assert_eq!(st.clip_results.len(), 20);
    assert!(st.has_more_clips);
    assert!(st.channel_offset == 3 && st.channel_results.len() == 3);
    assert_eq!(st.live_results[0].live_id, 2, "시청자 많은순");
    assert_eq!(st.completed_search_query(), Some(KEYWORD));
    assert_eq!(st.recent_searches, vec![KEYWORD.to_string()]);
}

#[tokio::test]
async fn clip_tab_catches_up_remaining_sources() {
    let h = harness();
    search(&h, KEYWORD).await;
    h.api.expire_cache();
    h.vm.on_scope_changed(SearchScope::Clip).await;
    assert_eq!(
        h.api.channel_searches(),
        1,
        "클립 탭 진입이 채널을 다시 검색했다"
    );
    assert_eq!(
        h.api.clip_set(),
        [req("c3", 0), req("c1", 0), req("c2", 0)]
            .into_iter()
            .collect()
    );
    {
        let st = h.vm.read();
        assert_eq!(st.clip_results.len(), 30);
        let exhausted: HashSet<String> = ["c1".to_string(), "c2".to_string()].into_iter().collect();
        assert_eq!(st.clip_exhausted(), &exhausted);
    }
    h.vm.on_scope_changed(SearchScope::All).await;
    h.vm.on_scope_changed(SearchScope::Clip).await;
    assert_eq!(h.api.total(), 6);
}

#[tokio::test]
async fn load_more_fetches_only_unexhausted_next_page() {
    let h = harness();
    search(&h, KEYWORD).await;
    h.vm.on_scope_changed(SearchScope::Clip).await;
    h.api.expire_cache();
    let before = h.api.clip_set();
    h.vm.load_more().await;
    let added: HashSet<_> = h.api.clip_set().difference(&before).cloned().collect();
    assert_eq!(added, [req("c3", 1)].into_iter().collect());
    assert_eq!(h.api.channel_searches(), 1);
    let st = h.vm.read();
    assert_eq!(st.clip_next_page().get("c3"), Some(&2));
    assert!(st.has_more_clips);
}

#[tokio::test]
async fn load_more_catches_up_before_paging() {
    let h = harness();
    search(&h, KEYWORD).await;
    h.vm.lock().selected_scope = SearchScope::Clip;
    h.vm.load_more().await;
    assert_eq!(
        h.api.clip_set(),
        [req("c3", 0), req("c1", 0), req("c2", 0)]
            .into_iter()
            .collect()
    );
}

#[tokio::test]
async fn research_within_fresh_window_sends_nothing() {
    let h = harness();
    search(&h, KEYWORD).await;
    let sent = h.api.total();
    h.now.fetch_add(10, Ordering::SeqCst);
    h.api.expire_cache();
    h.vm.perform_search(false).await;
    leave_and_return(&h);
    h.vm.perform_search(false).await;
    assert_eq!(h.api.total(), sent, "15초 안 재검색이 요청을 보냈다");
    h.now.fetch_add(10, Ordering::SeqCst);
    h.vm.perform_search(false).await;
    assert!(h.api.total() > sent, "신선 구간이 지나면 다시 받는다");
}

#[tokio::test]
async fn snapshot_restore_keeps_clip_sources() {
    let h = harness();
    search(&h, KEYWORD).await;
    h.vm.on_scope_changed(SearchScope::Clip).await;
    h.now.fetch_add(5, Ordering::SeqCst);
    h.api.expire_cache();
    leave_and_return(&h);
    {
        let st = h.vm.read();
        assert_eq!(st.clip_source_channel_ids(), ["c3", "c1", "c2"]);
        assert_eq!(st.clip_results.len(), 30);
    }
    h.vm.lock().selected_scope = SearchScope::Clip;
    let before = h.api.clip_set();
    h.vm.load_more().await;
    assert_eq!(h.api.channel_searches(), 1);
    let added: HashSet<_> = h.api.clip_set().difference(&before).cloned().collect();
    assert_eq!(added, [req("c3", 1)].into_iter().collect());
}

#[tokio::test]
async fn query_change_clears_clip_sources() {
    let h = harness();
    search(&h, KEYWORD).await;
    assert!(!h.vm.read().clip_source_channel_ids().is_empty());
    h.vm.set_query("다른 검색어");
    h.vm.cancel_pending_search();
    let st = h.vm.read();
    assert!(st.clip_source_channel_ids().is_empty() && st.clip_source_keyword().is_none());
    assert!(st.clip_next_page().is_empty() && st.clip_exhausted().is_empty());
    assert!(!st.needs_clip_source_expansion("다른 검색어"));
}

#[tokio::test(start_paused = true)]
async fn debounce_runs_only_last_query() {
    let h = harness();
    h.vm.set_query("리");
    assert!(h.vm.read().is_search_pending());
    h.vm.set_query(KEYWORD);
    tokio::time::sleep(Duration::from_millis(350)).await;
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    let st = h.vm.read();
    assert_eq!(st.completed_search_query(), Some(KEYWORD));
    assert!(!st.is_search_pending());
    assert_eq!(st.recent_searches, vec![KEYWORD.to_string()]);
}

#[tokio::test]
async fn recent_searches_cap_and_persist() {
    let store = Arc::new(MemoryRecentSearchStore::default());
    store.save(&["옛".to_string()]);
    let vm = SearchViewModel::new(Arc::new(FakeApi::default()), store.clone());
    assert_eq!(vm.read().recent_searches, vec!["옛".to_string()]);
    for i in 0..12 {
        vm.set_query(format!("q{i}"));
        vm.cancel_pending_search();
        vm.perform_search(false).await;
    }
    let recent = vm.read().recent_searches.clone();
    assert_eq!(recent.len(), 10);
    assert_eq!(recent[0], "q11");
    assert_eq!(store.load().unwrap(), recent);
    vm.remove_recent_search("q11");
    assert_eq!(store.load().unwrap()[0], "q10");
    vm.clear_recent_searches();
    assert_eq!(store.load(), None);
}

#[test]
fn autocomplete_uses_recent_following_watched() {
    let vm = SearchViewModel::new(
        Arc::new(FakeApi::default()),
        Arc::new(MemoryRecentSearchStore::default()),
    );
    {
        let mut st = vm.lock();
        st.recent_searches = vec!["리그 결승".into()];
        st.sync_following_channel_names(&[item("x", "리그오브")]);
        st.sync_recent_watched_names(&["".into(), "리그방".into()]);
    }
    vm.set_query("리그");
    vm.cancel_pending_search();
    let st = vm.read();
    let kinds: Vec<_> = st
        .autocomplete_suggestions()
        .iter()
        .map(|s| s.kind)
        .collect();
    assert_eq!(
        kinds,
        vec![
            AutocompleteSuggestionKind::Recent,
            AutocompleteSuggestionKind::Following,
            AutocompleteSuggestionKind::Watched
        ]
    );
    assert!(st.show_autocomplete());
}

#[test]
fn scope_mapping_and_live_sort() {
    use cview_core::models::search::SearchType;
    assert_eq!(SearchScope::All.search_type(), None);
    assert_eq!(
        SearchScope::from_search_type(SearchType::Video),
        SearchScope::Video
    );
    assert_eq!(SearchScope::Clip.title(), "클립");
    let vm = SearchViewModel::new(
        Arc::new(FakeApi::default()),
        Arc::new(MemoryRecentSearchStore::default()),
    );
    let mut st = vm.lock();
    assert_eq!(st.selected_tab(), SearchType::Channel);
    st.set_selected_tab(SearchType::Live);
    assert_eq!(st.selected_scope, SearchScope::Live);
    let mut a = LiveInfo::new(1, "a");
    a.open_date = Some(chrono::Utc::now());
    let b = LiveInfo::new(2, "b");
    st.live_results = vec![b, a];
    st.set_live_sort_option(LiveSortOption::Recent);
    assert_eq!(st.live_results[0].live_id, 1);
}

#[test]
fn result_cache_is_lru() {
    let mut c: ResultCache<i32> = ResultCache::new(2);
    c.insert(1, "a");
    c.insert(2, "b");
    assert_eq!(c.value("a"), Some(1));
    c.insert(3, "c");
    assert_eq!(c.value("b"), None);
    assert_eq!(c.len(), 2);
}

fn item(id: &str, name: &str) -> LiveChannelItem {
    LiveChannelItem {
        id: id.into(),
        channel_name: name.into(),
        channel_image_url: None,
        live_title: String::new(),
        viewer_count: 0,
        category_name: None,
        category_type: None,
        thumbnail_url: None,
        channel_id: id.into(),
        is_live: true,
        open_date: None,
    }
}

fn landing(ids: &[&str]) -> SearchLandingSnapshot {
    SearchLandingSnapshot {
        summary: None,
        popular: ids.iter().map(|i| item(i, i)).collect(),
        rows: Vec::new(),
    }
}

#[tokio::test]
async fn landing_snapshot_skips_same_revision_and_recomputes_on_move() {
    let vm = SearchViewModel::new(
        Arc::new(FakeApi::default()),
        Arc::new(MemoryRecentSearchStore::default()),
    );
    let count = Arc::new(AtomicUsize::new(0));
    let compute = |ids: Vec<&'static str>| {
        let count = count.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            landing(&ids)
        }
    };
    vm.refresh_landing_snapshot(|| (1, vec!["a"]), compute)
        .await;
    vm.refresh_landing_snapshot(|| (1, vec!["a"]), compute)
        .await;
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(vm.read().landing_source_revision(), Some(1));
    vm.refresh_landing_snapshot(|| (2, vec!["a"]), compute)
        .await;
    assert_eq!(count.load(Ordering::SeqCst), 2);

    // 계산 중 revision 이 움직이면 다시 계산해 최신 원천으로 커밋한다.
    let revision = Arc::new(AtomicI64::new(3));
    let moving = {
        let revision = revision.clone();
        let count = count.clone();
        move || {
            let mut r = revision.load(Ordering::SeqCst);
            if count.load(Ordering::SeqCst) == 3 && r == 3 {
                revision.store(4, Ordering::SeqCst);
                r = 4;
            }
            (r, vec![if r == 3 { "old" } else { "new" }])
        }
    };
    vm.refresh_landing_snapshot(moving, compute).await;
    let st = vm.read();
    assert_eq!(st.landing_source_revision(), Some(4));
    assert_eq!(st.landing_snapshot().unwrap().popular[0].channel_id, "new");
}
