//! AllBroadcastsViewModel·CategoryBrowseViewModel 테스트.
//!
//! 원본: `Tests/CViewAppTests/AllBroadcastsRefreshRaceTests.swift` (transport fake 대신 API fake).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::Notify;

use cview_app_core::viewmodels::all_broadcasts::{
    AllBroadcastsApi, AllBroadcastsMode, AllBroadcastsViewModel, LoadState,
};
use cview_app_core::viewmodels::category_browse::{AccentColorCache, CategoryBrowseViewModel};
use cview_core::models::search::{SearchPage, SearchPageOffset, SearchResult};
use cview_core::models::LiveInfo;
use cview_networking::response::{LivePageCursor, LivePageInfo};
use cview_networking::{AgedResponse, PagedContent};

fn page(ids: std::ops::RangeInclusive<i64>, next: i64) -> PagedContent<LiveInfo> {
    let data: Vec<LiveInfo> = ids
        .map(|i| {
            let mut l = LiveInfo::new(i, format!("live-{i}"));
            l.concurrent_user_count = 1_000 - i;
            l
        })
        .collect();
    PagedContent::new(
        data.len() as i64,
        None,
        data,
        Some(LivePageInfo::new(
            Some(LivePageCursor {
                concurrent_user_count: Some(1_000 - next),
                live_id: Some(next),
            }),
            None,
        )),
    )
}

#[derive(Default)]
struct FakeApi {
    hold_more: AtomicBool,
    hold_first: AtomicBool,
    holding_more: AtomicBool,
    holding_first: AtomicBool,
    release_more: Notify,
    release_first: Notify,
    first_page_calls: AtomicUsize,
    searches: Mutex<Vec<(String, i64)>>,
}

#[async_trait]
impl AllBroadcastsApi for FakeApi {
    async fn top_lives_with_age(
        &self,
        _size: i64,
    ) -> Result<AgedResponse<PagedContent<LiveInfo>>, String> {
        self.first_page_calls.fetch_add(1, Ordering::SeqCst);
        if self.hold_first.swap(false, Ordering::SeqCst) {
            let n = self.release_first.notified();
            self.holding_first.store(true, Ordering::SeqCst);
            n.await;
        }
        Ok(AgedResponse {
            value: page(1..=30, 30),
            fetched_at: Utc::now(),
            is_stale: false,
        })
    }
    async fn top_lives(
        &self,
        _size: i64,
        _c: Option<i64>,
        _l: Option<i64>,
    ) -> Result<PagedContent<LiveInfo>, String> {
        if self.hold_more.load(Ordering::SeqCst) {
            let n = self.release_more.notified();
            self.holding_more.store(true, Ordering::SeqCst);
            n.await;
        }
        Ok(page(25..=54, 54))
    }
    async fn search_lives(
        &self,
        keyword: &str,
        offset: i64,
        size: i64,
    ) -> Result<SearchResult<LiveInfo>, String> {
        self.searches.lock().unwrap().push((keyword.into(), offset));
        let data: Vec<LiveInfo> = (offset..offset + size)
            .take(if offset >= 60 { 5 } else { size as usize })
            .map(|i| LiveInfo::new(i, "검색"))
            .collect();
        let more = offset < 60;
        Ok(SearchResult {
            size,
            page: Some(SearchPage {
                next: more.then_some(SearchPageOffset {
                    offset: offset + size,
                }),
            }),
            total_count: None,
            data,
        })
    }
}

async fn wait_flag(flag: &AtomicBool) {
    for _ in 0..1000 {
        if flag.load(Ordering::SeqCst) {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("flag never set");
}

#[tokio::test]
async fn stale_load_more_is_discarded() {
    let api = Arc::new(FakeApi::default());
    let vm = AllBroadcastsViewModel::new(api.clone());
    vm.reload().await;
    assert_eq!(vm.read().feed.len(), 30);

    api.hold_more.store(true, Ordering::SeqCst);
    let v1 = vm.clone();
    let more = tokio::spawn(async move { v1.load_more().await });
    wait_flag(&api.holding_more).await;

    api.hold_first.store(true, Ordering::SeqCst);
    let v2 = vm.clone();
    let refresh = tokio::spawn(async move { v2.reload().await });
    wait_flag(&api.holding_first).await;

    api.release_more.notify_waiters();
    more.await.unwrap();
    api.release_first.notify_waiters();
    refresh.await.unwrap();

    let ids: Vec<i64> = vm.read().feed.iter().map(|l| l.live_id).collect();
    assert_eq!(
        ids,
        (1..=30).collect::<Vec<_>>(),
        "새 첫 페이지가 옛 페이지와 섞였다"
    );
    assert_eq!(vm.read().page, 0);
    assert!(!vm.read().is_refreshing);
}

#[tokio::test]
async fn paging_loads_more_and_dedupes() {
    let api = Arc::new(FakeApi::default());
    let vm = AllBroadcastsViewModel::new(api.clone());
    vm.load_initial_if_needed().await;
    vm.load_initial_if_needed().await;
    assert_eq!(api.first_page_calls.load(Ordering::SeqCst), 1);
    {
        let s = vm.read();
        assert_eq!(s.state, LoadState::Loaded);
        assert_eq!(s.page_count(), 1);
        assert!(s.has_next_page());
        assert!(s.feed_freshness().caption.is_none());
    }
    vm.next_display_page().await;
    let s = vm.read();
    assert_eq!(s.feed.len(), 54, "겹친 25~30 은 한 번만 남는다");
    assert_eq!(s.page, 1);
    assert_eq!(s.current_page_items().len(), 24);
    assert_eq!(s.display_page_count(), 3);
}

#[tokio::test]
async fn refresh_first_page_only_when_pristine() {
    let api = Arc::new(FakeApi::default());
    let vm = AllBroadcastsViewModel::new(api.clone());
    assert!(
        !vm.refresh_first_page_if_pristine().await,
        "idle 이면 받지 않는다"
    );
    vm.reload().await;
    assert!(vm.refresh_first_page_if_pristine().await);
    vm.go_to_page(1).await;
    assert!(!vm.refresh_first_page_if_pristine().await);
}

#[tokio::test(start_paused = true)]
async fn search_debounces_and_pages() {
    let api = Arc::new(FakeApi::default());
    let vm = AllBroadcastsViewModel::new(api.clone());
    vm.set_mode(AllBroadcastsMode::Search);
    assert!(vm.lock().consume_pending_search_focus());
    assert!(!vm.lock().consume_pending_search_focus());
    vm.update_search_query("a");
    assert_eq!(vm.read().search_state, LoadState::Idle);
    vm.update_search_query("롤 ");
    vm.update_search_query("롤토");
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(
        api.searches.lock().unwrap().as_slice(),
        &[("롤토".to_string(), 0)],
        "디바운스 안의 이전 입력은 검색하지 않는다"
    );
    {
        let s = vm.read();
        assert_eq!(s.search_state, LoadState::Loaded);
        assert_eq!(s.display_items().len(), 30);
        assert!(s.search_can_load_more);
    }
    vm.go_to_display_page(2).await;
    {
        let s = vm.read();
        assert_eq!(s.search_results.len(), 65);
        assert_eq!(s.search_page, 2);
        assert!(!s.search_can_load_more);
    }
    vm.update_search_query("");
    let s = vm.read();
    assert!(s.search_results.is_empty() && s.search_state == LoadState::Idle);
}

#[test]
fn category_browse_cache_keys() {
    type Vm = CategoryBrowseViewModel<String, String>;
    let key = Vm::guard_key("category/GAME", 7);
    assert_eq!(key, "category/GAME|r7");
    assert!(Vm::should_commit_groups("a", 1, "a", 1, true, false));
    assert!(!Vm::should_commit_groups("a", 1, "a", 2, true, false));
    assert!(!Vm::should_commit_groups("a", 1, "b", 1, true, false));
    assert!(!Vm::should_commit_groups("a", 1, "a", 1, false, false));
    assert!(!Vm::should_commit_groups("a", 1, "a", 1, true, true));
    let mut vm = Vm::new();
    assert!(vm.should_recompute_groups(&key), "비었으면 계산한다");
    vm.group_cache.push("g".into());
    assert!(vm.should_recompute_groups(&key));
    vm.mark_groups_computed(&key, Some(7));
    assert!(!vm.should_recompute_groups(&key));
    assert_eq!(vm.groups_channels_revision(), Some(7));
    assert_eq!(vm.available_type_filters_cache.len(), 1);
    assert_eq!(vm.insight_focus_category(), None);
    vm.split_selected_category = Some("A".into());
    assert_eq!(vm.insight_focus_category(), None);
    vm.is_split_explorer_visible = true;
    assert_eq!(vm.insight_focus_category(), Some("A"));
    vm.selected_category = Some("B".into());
    assert_eq!(vm.insight_focus_category(), Some("B"));
    let mut colors = AccentColorCache::default();
    let i = colors.index("게임");
    assert!(i < AccentColorCache::PALETTE_SIZE);
    assert_eq!(colors.index("게임"), i);
}
