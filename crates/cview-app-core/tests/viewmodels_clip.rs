//! 클립 뷰모델 테스트.
//!
//! 원본: `Tests/CViewAppTests/ClipPagingRegressionTests.swift`,
//! `LibrarySettingsShellAuditTests.popularChannelClipsKeepLoadedOrder`,
//! `Audit0919browsedataContractTests` (ChannelResolver). HTTP 트랜스포트 대신 가짜 API 를 쓴다.

use std::collections::{BTreeSet, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use url::Url;

use cview_app_core::viewmodels::clip::{
    channel_resolver, format_time, merging_channel_page, ClipApi, ClipBrowserViewModel,
    ClipCacheStore, ClipHubPageLifecycleEffects, ClipMood, ClipPlayerViewModel, ClipRefreshDemand,
    ClipSortOrder, ClipTab, PlaybackStateSnapshot, QUEUE_DEFAULTS_KEY, WATCH_LATER_DEFAULTS_KEY,
};
use cview_app_core::viewmodels::following::{FollowingDefaults, MemoryDefaults};
use cview_core::models::clip::ClipDetail;
use cview_core::models::search::ClipInfo;
use cview_core::models::vod::VodPlaybackState;
use cview_core::models::ChannelInfo;
use cview_networking::PagedContent;
use cview_persistence::models::cached_clip::CachedClipData;
use cview_persistence::DataStore;

struct FakeApi {
    trending_count: AtomicUsize,
    channel_total: usize,
    channel_pages: Mutex<Vec<i64>>,
    fail_page_once: Mutex<Option<i64>>,
    detail: Mutex<Option<ClipDetail>>,
    stream_url: Mutex<Option<Url>>,
    searches: Mutex<Vec<String>>,
}

impl FakeApi {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            trending_count: AtomicUsize::new(100),
            channel_total: 70,
            channel_pages: Mutex::new(Vec::new()),
            fail_page_once: Mutex::new(None),
            detail: Mutex::new(None),
            stream_url: Mutex::new(None),
            searches: Mutex::new(Vec::new()),
        })
    }
}

#[async_trait]
impl ClipApi for FakeApi {
    async fn home_popular_clips(&self, _f: &str, _o: &str) -> Result<Vec<ClipInfo>, String> {
        let n = self.trending_count.load(Ordering::SeqCst);
        Ok((0..n)
            .map(|i| ClipInfo::new(format!("t{i}"), format!("t{i}")))
            .collect())
    }
    async fn clip_list(
        &self,
        _channel_id: &str,
        page: i64,
        size: i64,
    ) -> Result<PagedContent<ClipInfo>, String> {
        self.channel_pages.lock().unwrap().push(page);
        tokio::task::yield_now().await;
        {
            let mut f = self.fail_page_once.lock().unwrap();
            if *f == Some(page) {
                *f = None;
                return Err("HTTP 400".into());
            }
        }
        let start = (page * size) as usize;
        let end = (start + size as usize).min(self.channel_total);
        let data: Vec<ClipInfo> = (start..end)
            .map(|i| ClipInfo::new(format!("c{i}"), format!("c{i}")))
            .collect();
        Ok(PagedContent::new(
            data.len() as i64,
            Some(self.channel_total as i64),
            data,
            None,
        ))
    }
    async fn search_channels(
        &self,
        keyword: &str,
        _offset: i64,
        _size: i64,
    ) -> Result<Vec<ChannelInfo>, String> {
        self.searches.lock().unwrap().push(keyword.to_string());
        Ok(vec![ChannelInfo::new("cid", keyword)])
    }
    async fn clip_detail(&self, uid: &str) -> Result<ClipDetail, String> {
        self.detail
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| format!("no detail {uid}"))
    }
    async fn clip_stream_url(&self, _uid: &str, _video_id: &str) -> Result<Url, String> {
        self.stream_url
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "inkey".to_string())
    }
}

fn browser(api: &Arc<FakeApi>) -> (ClipBrowserViewModel, Arc<MemoryDefaults>) {
    let defaults = Arc::new(MemoryDefaults::default());
    let vm = ClipBrowserViewModel::new(defaults.clone());
    vm.lock().api = Some(api.clone());
    (vm, defaults)
}

async fn settle(vm: &ClipBrowserViewModel, cond: impl Fn(&ClipBrowserViewModel) -> bool) {
    for _ in 0..400 {
        if cond(vm) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

fn clip(uid: &str, reads: i64) -> ClipInfo {
    let mut c = ClipInfo::new(uid, uid);
    c.read_count = reads;
    c
}

// MARK: - ChannelResolver

#[test]
fn channel_id_heuristic_accepts_only_hex() {
    let hex32 = "ab12".repeat(8);
    assert!(channel_resolver::looks_like_channel_id(&hex32));
    assert!(channel_resolver::looks_like_channel_id(&"f".repeat(16)));
    assert!(!channel_resolver::looks_like_channel_id(
        "TheGreatStreamerGuy"
    ));
    assert!(!channel_resolver::looks_like_channel_id(&"가".repeat(16)));
    assert!(!channel_resolver::looks_like_channel_id(&"１".repeat(16)));
    assert!(!channel_resolver::looks_like_channel_id(&"a".repeat(15)));
    assert!(!channel_resolver::looks_like_channel_id(&"a".repeat(65)));
}

#[test]
fn resolve_channel_id_falls_through_for_names() {
    let hex32 = "ab12".repeat(8);
    assert_eq!(
        channel_resolver::resolve_channel_id(&hex32).as_deref(),
        Some(hex32.as_str())
    );
    assert_eq!(
        channel_resolver::resolve_channel_id(&format!("  {hex32} ")).as_deref(),
        Some(hex32.as_str())
    );
    assert_eq!(
        channel_resolver::resolve_channel_id(&format!("https://chzzk.naver.com/{hex32}"))
            .as_deref(),
        Some(hex32.as_str())
    );
    assert_eq!(
        channel_resolver::resolve_channel_id(&format!("chzzk.naver.com/{hex32}/live")).as_deref(),
        Some(hex32.as_str())
    );
    assert_eq!(
        channel_resolver::resolve_channel_id("TheGreatStreamerGuy"),
        None
    );
    assert_eq!(
        channel_resolver::resolve_channel_id("https://chzzk.naver.com/clips/abc"),
        None
    );
    assert_eq!(
        channel_resolver::resolve_channel_id(&format!("https://example.com/{hex32}")),
        None
    );
    assert_eq!(channel_resolver::resolve_channel_id("   "), None);
}

// MARK: - 병합·mood

#[test]
fn popular_channel_clips_keep_loaded_order() {
    let first = merging_channel_page(
        vec![],
        &[clip("a", 10), clip("b", 50), clip("c", 30)],
        ClipSortOrder::Popular,
    );
    let ids: Vec<_> = first.iter().map(|c| c.clip_uid.as_str()).collect();
    assert_eq!(ids, ["b", "c", "a"]);
    let second = merging_channel_page(
        first,
        &[clip("d", 1000), clip("a", 10), clip("e", 5)],
        ClipSortOrder::Popular,
    );
    let ids: Vec<_> = second.iter().map(|c| c.clip_uid.as_str()).collect();
    assert_eq!(ids, ["b", "c", "a", "d", "e"]);
    let recent = merging_channel_page(
        vec![clip("x", 1)],
        &[clip("y", 1), clip("z", 99)],
        ClipSortOrder::Recent,
    );
    let ids: Vec<_> = recent.iter().map(|c| c.clip_uid.as_str()).collect();
    assert_eq!(ids, ["x", "y", "z"]);
}

#[test]
fn mood_matching_uses_title_and_category() {
    assert!(ClipMood::matches("아무거나", None, None));
    assert!(ClipMood::matches("아무거나", None, Some("all")));
    assert!(ClipMood::matches("레전드 장면", None, Some("funny")));
    assert!(ClipMood::matches("x", Some("VALORANT"), Some("valorant")));
    // "방송" 은 "송" 키워드에 걸린다 — 원본 휴리스틱 그대로.
    assert!(ClipMood::matches("조용한 방송", None, Some("music")));
    assert!(!ClipMood::matches("조용한 시간", None, Some("music")));
    assert!(ClipMood::matches("조용한 방송", None, Some("unknown")));
    assert_eq!(ClipMood::default_static_moods(5).len(), 8);
    assert_eq!(ClipMood::default_static_moods(5)[0].count, 5);
}

#[tokio::test]
async fn mood_change_resets_trending_page_and_filters() {
    let api = FakeApi::new();
    let (vm, _) = browser(&api);
    vm.load_trending_clips(true).await;
    vm.go_to_trending_display_page(2);
    assert_eq!(vm.read().trending_display_page, 2);
    vm.lock().set_selected_mood_id(Some("funny".into()));
    let st = vm.read();
    assert_eq!(st.trending_display_page, 0);
    assert!(st.filtered_trending_clips().is_empty());
    assert_eq!(st.trending_total_display_pages(), 1);
}

// MARK: - 페이징 회귀

#[tokio::test]
async fn auto_refresh_keeps_trending_page() {
    let api = FakeApi::new();
    let (vm, _) = browser(&api);
    vm.load_trending_clips(true).await;
    vm.go_to_trending_display_page(3);
    assert_eq!(vm.read().trending_display_page, 3);

    vm.load_trending_clips(false).await;
    assert_eq!(
        vm.read().trending_display_page,
        3,
        "5분 자동 갱신이 1페이지로 튕겼다"
    );

    api.trending_count.store(30, Ordering::SeqCst);
    vm.load_trending_clips(false).await;
    {
        let st = vm.read();
        assert_eq!(
            st.trending_display_page,
            st.trending_total_display_pages() - 1
        );
    }
    vm.load_trending_clips(true).await;
    assert_eq!(vm.read().trending_display_page, 0);
    assert!(!vm.read().trending_is_loading);
}

#[tokio::test]
async fn channel_paging_retries_and_fills() {
    let api = FakeApi::new();
    let (vm, _) = browser(&api);
    vm.lock().resolved_channel_id = Some("channel".into());

    vm.load_channel_clips(true);
    settle(&vm, |v| {
        let st = v.read();
        !st.channel_is_loading && st.channel_clips.len() >= 24
    })
    .await;
    assert_eq!(vm.read().channel_clips.len(), 40);

    *api.fail_page_once.lock().unwrap() = Some(2);
    vm.go_to_channel_display_page(2);
    settle(&vm, |v| {
        let st = v.read();
        !st.channel_is_loading && st.channel_error.is_some()
    })
    .await;
    assert_eq!(vm.read().channel_clips.len(), 40);

    vm.load_more_channel_clips_if_needed();
    settle(&vm, |v| {
        let st = v.read();
        !st.channel_is_loading && !st.channel_has_more
    })
    .await;
    assert_eq!(vm.read().channel_clips.len(), 70, "실패한 page 2 가 빠졌다");
    assert_eq!(*api.channel_pages.lock().unwrap(), vec![0, 1, 2, 2, 3]);
}

fn cached_row(uid: &str, sources: &[&str]) -> CachedClipData {
    let mut d = CachedClipData::new(uid, uid);
    d.duration = 10;
    d.content_hash = format!("h-{uid}");
    d.sources = sources
        .iter()
        .map(|s| s.to_string())
        .collect::<BTreeSet<_>>();
    d.fetched_at = chrono::Utc::now() - chrono::Duration::hours(1);
    d
}

fn cached_uids(store: &DataStore, source: &str) -> HashSet<String> {
    ClipCacheStore::fetch_cached_clips(store, source, 1000)
        .unwrap()
        .into_iter()
        .map(|d| d.clip_uid)
        .collect()
}

#[tokio::test]
async fn channel_pages_do_not_touch_clip_cache() {
    let api = FakeApi::new();
    let (vm, _) = browser(&api);
    let store = Arc::new(DataStore::open_in_memory().unwrap());
    vm.lock().data_store = Some(store.clone());
    vm.lock().resolved_channel_id = Some("channel".into());
    let source = format!("channel-channel-{}", ClipSortOrder::Popular.raw_value());
    store
        .upsert_cached_clips(&[
            cached_row("legacy", &[&source]),
            cached_row("shared", &[&source, "trending-x"]),
        ])
        .unwrap();

    vm.load_channel_clips(true);
    settle(&vm, |v| {
        let st = v.read();
        !st.channel_is_loading && st.channel_clips.len() >= 40
    })
    .await;
    vm.go_to_channel_display_page(1);
    settle(&vm, |v| {
        let st = v.read();
        !st.channel_is_loading && st.channel_clips.len() >= 60
    })
    .await;
    assert_eq!(vm.read().channel_clips.len(), 60);
    assert_eq!(
        cached_uids(&store, &source),
        ["legacy", "shared"].iter().map(|s| s.to_string()).collect()
    );
    assert_eq!(
        cached_uids(&store, "trending-x"),
        ["shared".to_string()].into_iter().collect()
    );
}

#[tokio::test]
async fn trending_refresh_still_prunes_dropped_rows() {
    let api = FakeApi::new();
    let (vm, _) = browser(&api);
    let store = Arc::new(DataStore::open_in_memory().unwrap());
    vm.lock().data_store = Some(store.clone());
    let source = vm.read().trending_source();
    assert_eq!(source, "trending-within_7_days-popular");
    store
        .upsert_cached_clips(&[
            cached_row("gone", &[&source]),
            cached_row("shared", &[&source, "channel-x-recent"]),
        ])
        .unwrap();

    vm.load_trending_clips(true).await;
    assert_eq!(vm.read().trending_clips.len(), 100);
    let expected: HashSet<String> = (0..100).map(|i| format!("t{i}")).collect();
    assert_eq!(cached_uids(&store, &source), expected);
    assert_eq!(
        cached_uids(&store, "channel-x-recent"),
        ["shared".to_string()].into_iter().collect()
    );

    api.trending_count.store(30, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(5)).await;
    vm.load_trending_clips(true).await;
    assert_eq!(vm.read().trending_clips.len(), 30);
    let expected: HashSet<String> = (0..30).map(|i| format!("t{i}")).collect();
    assert_eq!(cached_uids(&store, &source), expected);

    // 콜드 스타트 복원.
    let (fresh, _) = browser(&api);
    fresh.lock().data_store = Some(store.clone());
    fresh.hydrate_from_cache();
    assert_eq!(fresh.read().trending_clips.len(), 30);
}

#[tokio::test]
async fn missing_api_reports_error() {
    let vm = ClipBrowserViewModel::new(Arc::new(MemoryDefaults::default()));
    vm.load_trending_clips(true).await;
    assert_eq!(
        vm.read().trending_error.as_deref(),
        Some("API 클라이언트가 초기화되지 않았습니다")
    );
}

#[tokio::test]
async fn submit_invalid_channel_input_reports_error() {
    let api = FakeApi::new();
    let (vm, _) = browser(&api);
    vm.lock().channel_input = "그냥이름".into();
    vm.submit_channel_input();
    let st = vm.read();
    assert_eq!(
        st.channel_error.as_deref(),
        Some("채널 ID 또는 채널 URL 형식이 올바르지 않습니다")
    );
    assert!(st.resolved_channel_id.is_none());
}

// MARK: - 수명주기

#[derive(Default)]
struct FakeRefresh {
    log: Mutex<Vec<String>>,
}

impl ClipRefreshDemand for FakeRefresh {
    fn retain_clips(&self, _i: u64) {
        self.log.lock().unwrap().push("retain".into());
    }
    fn release_clips(&self, _i: u64) {
        self.log.lock().unwrap().push("release".into());
    }
    fn note_clip_trending_completed(&self, success: bool) {
        self.log.lock().unwrap().push(format!("note:{success}"));
    }
}

#[test]
fn hub_page_lifecycle_effects() {
    let active = ClipHubPageLifecycleEffects::resolve(true);
    assert!(active.maintains_auto_refresh && !active.dismisses_playback);
    let hidden = ClipHubPageLifecycleEffects::resolve(false);
    assert!(!hidden.maintains_auto_refresh && hidden.dismisses_playback);

    let vm = ClipBrowserViewModel::new(Arc::new(MemoryDefaults::default()));
    let refresh = Arc::new(FakeRefresh::default());
    vm.lock().refresh_coordinator = Some(refresh.clone());
    vm.sync_hub_page_activity(true);
    vm.sync_hub_page_activity(true);
    assert!(vm.read().is_trending_auto_refresh_active());
    vm.lock().selected_clip = Some(clip("a", 1));
    vm.sync_hub_page_activity(false);
    vm.sync_hub_page_activity(false);
    assert!(!vm.read().is_trending_auto_refresh_active());
    assert!(vm.read().selected_clip.is_none());
    assert_eq!(
        *refresh.log.lock().unwrap(),
        vec!["retain", "note:true", "release"]
    );
}

// MARK: - 나중에 보기·큐·최근 채널

#[test]
fn watch_later_queue_and_recent_channels_persist() {
    let defaults = Arc::new(MemoryDefaults::default());
    let vm = ClipBrowserViewModel::new(defaults.clone());
    let a = clip("a", 1);
    let b = clip("b", 2);
    vm.toggle_watch_later(&a);
    assert!(vm.read().is_watch_later(&a));
    assert!(vm.read().saved_clip_snapshots.contains_key("a"));
    vm.add_to_queue(&a);
    vm.add_to_queue(&a);
    vm.toggle_queue(&b);
    assert_eq!(vm.read().queue_clips.len(), 2);
    assert!(defaults.string(WATCH_LATER_DEFAULTS_KEY).is_some());
    assert!(defaults.string(QUEUE_DEFAULTS_KEY).is_some());
    for i in 0..10 {
        vm.add_recent_channel(ChannelInfo::new(format!("c{i}"), "n"));
    }
    vm.add_recent_channel(ChannelInfo::new("c5", "n"));
    {
        let st = vm.read();
        assert_eq!(st.recent_channels.len(), 8);
        assert_eq!(st.recent_channels[0].channel_id, "c5");
        assert_eq!(st.recent_channels[1].channel_id, "c9");
    }

    let restored = ClipBrowserViewModel::new(defaults.clone());
    restored.load_watch_later_from_defaults();
    {
        let st = restored.read();
        assert!(st.is_watch_later(&a));
        assert_eq!(st.saved_clip_snapshots["a"].clip_uid, "a");
        let q: Vec<_> = st.queue_clips.iter().map(|c| c.clip_uid.clone()).collect();
        assert_eq!(q, ["a", "b"]);
        assert_eq!(st.recent_channels.len(), 8);
        assert_eq!(st.play_next_in_queue(Some(&a)).unwrap().clip_uid, "b");
        assert_eq!(st.play_next_in_queue(Some(&b)).unwrap().clip_uid, "a");
        assert_eq!(st.play_next_in_queue(None).unwrap().clip_uid, "a");
    }
    vm.toggle_watch_later(&a);
    assert!(!vm.read().is_watch_later(&a));
    assert!(vm.read().saved_clip_snapshots.is_empty());
    vm.clear_queue();
    assert!(vm.read().play_next_in_queue(None).is_none());
}

#[test]
fn clip_tap_routes_to_preview_or_player() {
    let vm = ClipBrowserViewModel::new(Arc::new(MemoryDefaults::default()));
    vm.lock().handle_clip_tap(clip("a", 1), true);
    assert_eq!(vm.read().preview_clip.as_ref().unwrap().clip_uid, "a");
    assert!(vm.read().selected_clip.is_none());
    vm.lock().handle_clip_tap(clip("b", 1), false);
    assert_eq!(vm.read().selected_clip.as_ref().unwrap().clip_uid, "b");
    assert_eq!(vm.snapshot().selected_tab, ClipTab::Trending);
}

// MARK: - 채널 후보

#[tokio::test(start_paused = true)]
async fn channel_suggestions_skip_ids_and_short_ascii_and_debounce() {
    let api = FakeApi::new();
    let (vm, _) = browser(&api);
    vm.update_channel_suggestions("a").await;
    vm.update_channel_suggestions(&"ab12".repeat(8)).await;
    vm.update_channel_suggestions("  ").await;
    assert!(api.searches.lock().unwrap().is_empty());

    // 한글 1글자는 검색한다.
    vm.update_channel_suggestions("가").await;
    assert_eq!(*api.searches.lock().unwrap(), vec!["가"]);
    assert_eq!(vm.read().channel_suggestions.len(), 1);
    assert!(!vm.read().is_searching_channels);

    // 뒤에 온 입력이 앞선 디바운스를 무효화한다.
    let first = {
        let vm = vm.clone();
        tokio::spawn(async move { vm.update_channel_suggestions("abc").await })
    };
    tokio::task::yield_now().await;
    vm.update_channel_suggestions("abcd").await;
    first.await.unwrap();
    assert_eq!(*api.searches.lock().unwrap(), vec!["가", "abcd"]);

    vm.select_channel_suggestion("cid", "이름");
    let st = vm.read();
    assert!(st.channel_suggestions.is_empty());
    assert_eq!(st.resolved_channel_id.as_deref(), Some("cid"));
    assert_eq!(st.channel_input, "이름");
}

// MARK: - ClipPlayerViewModel

#[tokio::test]
async fn clip_player_empty_uid_errors() {
    let vm = ClipPlayerViewModel::new(None);
    assert!(vm.start_clip(&ClipInfo::new("", "t")).is_none());
    let st = vm.read();
    assert_eq!(
        st.playback_state,
        VodPlaybackState::Error("클립 ID가 없습니다".into())
    );
    assert_eq!(st.error_message.as_deref(), Some("클립 ID가 없습니다"));
}

#[tokio::test]
async fn clip_player_sets_embed_and_resolves_download_url() {
    let api = FakeApi::new();
    let mut detail = ClipDetail::new("u1", "상세 제목");
    detail.video_play_url = Some("https://cdn.example/clip.mp4".into());
    *api.detail.lock().unwrap() = Some(detail);
    let vm = ClipPlayerViewModel::new(Some(api.clone()));
    let mut info = ClipInfo::new("u1", "목록 제목");
    info.channel = Some(ChannelInfo::new("c", "채널"));
    let handle = vm.start_clip(&info).unwrap();
    {
        let st = vm.read();
        assert_eq!(
            st.embed_fallback_url.as_ref().unwrap().as_str(),
            "https://chzzk.naver.com/embed/clip/u1"
        );
        assert_eq!(st.playback_state, VodPlaybackState::Paused);
        assert_eq!(st.channel_name, "채널");
        assert!(st.is_playback_active());
    }
    handle.await.unwrap();
    let snap = vm.snapshot();
    assert_eq!(snap.clip_title, "상세 제목");
    assert_eq!(
        snap.download_url.as_deref(),
        Some("https://cdn.example/clip.mp4")
    );
    assert_eq!(snap.playback_state, PlaybackStateSnapshot::Paused);

    // 먼저 해석된 값이 우선, http 아닌 URL 은 무시.
    vm.note_extracted_media_url(&Url::parse("https://other/x.m3u8").unwrap());
    assert_eq!(
        vm.read().download_url.as_ref().unwrap().as_str(),
        "https://cdn.example/clip.mp4"
    );
    vm.stop();
    let st = vm.read();
    assert!(st.embed_fallback_url.is_none());
    assert_eq!(st.playback_state, VodPlaybackState::Idle);
    assert!(!st.is_playback_active());
}

#[tokio::test]
async fn clip_player_falls_back_to_inkey_stream_url() {
    let api = FakeApi::new();
    let mut detail = ClipDetail::new("u2", "");
    detail.video_id = Some("vid".into());
    *api.detail.lock().unwrap() = Some(detail);
    *api.stream_url.lock().unwrap() = Some(Url::parse("https://hls/clip.m3u8").unwrap());
    let vm = ClipPlayerViewModel::new(Some(api.clone()));
    vm.start_clip(&ClipInfo::new("u2", "목록"))
        .unwrap()
        .await
        .unwrap();
    let st = vm.read();
    assert_eq!(st.clip_title, "목록");
    assert_eq!(
        st.download_url.as_ref().unwrap().as_str(),
        "https://hls/clip.m3u8"
    );
}

#[test]
fn clip_player_note_extracted_rejects_blob_and_takes_first() {
    let vm = ClipPlayerViewModel::new(None);
    vm.note_extracted_media_url(&Url::parse("blob:https://chzzk.naver.com/abc").unwrap());
    assert!(vm.read().download_url.is_none());
    vm.note_extracted_media_url(&Url::parse("https://a/1.m3u8").unwrap());
    vm.note_extracted_media_url(&Url::parse("https://a/2.m3u8").unwrap());
    assert_eq!(
        vm.read().download_url.as_ref().unwrap().as_str(),
        "https://a/1.m3u8"
    );
    assert_eq!(vm.read().progress(), 0.0);
}

#[test]
fn format_time_matches_vod_player() {
    assert_eq!(format_time(0.0), "0:00");
    assert_eq!(format_time(-3.0), "0:00");
    assert_eq!(format_time(f64::NAN), "0:00");
    assert_eq!(format_time(65.9), "1:05");
    assert_eq!(format_time(3725.0), "1:02:05");
}
