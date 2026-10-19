//! VOD 보관함 큐·채널 VOD 브라우저·재생 키 접근 테스트.
//!
//! 원본: `Tests/CViewAppTests/Swift6LibraryMutationTests.swift`,
//! `PlaybackLaunchPlanTests.swift` (`PlaybackLaunchPlanProtectedTests`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::Notify;

use cview_app_core::viewmodels::playback_launch::{requires_authenticated_key, PlaybackKeyAccess};
use cview_app_core::viewmodels::vod_library::{
    VodChannelApi, VodChannelBrowser, VodLibraryMutationQueue, VodLoadState,
};
use cview_core::models::search::VodInfo;
use cview_core::models::{ChannelInfo, LiveInfo, StreamProxyMode};
use cview_networking::PagedContent;

// MARK: - 저장 큐

struct WriteProbe {
    writes: Mutex<Vec<bool>>,
    started: Notify,
    release_first: Notify,
}

async fn repeated_toggle(first_fails: bool, second_fails: bool) {
    let queue = Arc::new(VodLibraryMutationQueue::new());
    let probe = Arc::new(WriteProbe {
        writes: Mutex::new(Vec::new()),
        started: Notify::new(),
        release_first: Notify::new(),
    });
    let first = {
        let (q, p) = (queue.clone(), probe.clone());
        tokio::spawn(async move {
            q.apply(1, false, true, async move {
                p.writes.lock().unwrap().push(true);
                p.started.notify_one();
                p.release_first.notified().await;
                if first_fails {
                    Err("first".to_string())
                } else {
                    Ok(())
                }
            })
            .await
        })
    };
    probe.started.notified().await;
    let second = {
        let (q, p) = (queue.clone(), probe.clone());
        tokio::spawn(async move {
            q.apply(1, true, false, async move {
                p.writes.lock().unwrap().push(false);
                if second_fails {
                    Err("second".to_string())
                } else {
                    Ok(())
                }
            })
            .await
        })
    };
    for _ in 0..200 {
        if queue.is_current(1, 2) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    assert!(queue.is_current(1, 2));
    assert_eq!(*probe.writes.lock().unwrap(), vec![true]);
    probe.release_first.notify_one();
    let old = first.await.unwrap();
    let latest = second.await.unwrap();
    assert!(!queue.is_current(1, old.token));
    assert!(queue.is_current(1, latest.token));
    assert_eq!(latest.outcome.is_member, second_fails && !first_fails);
    assert_eq!(latest.outcome.error.is_some(), second_fails);
    assert_eq!(*probe.writes.lock().unwrap(), vec![true, false]);
    assert!(queue.has_pending());
    queue.finish(1, latest.token);
    assert!(!queue.is_current(1, latest.token));
    assert!(!queue.has_pending());
}

#[tokio::test]
async fn repeated_toggle_recovers_last_committed_membership() {
    for (a, b) in [(true, true), (false, true), (true, false), (false, false)] {
        repeated_toggle(a, b).await;
    }
}

#[tokio::test]
async fn finish_ignores_stale_token() {
    let queue = VodLibraryMutationQueue::new();
    let r1 = queue.apply(7, false, true, async { Ok(()) }).await;
    let r2 = queue.apply(7, true, false, async { Ok(()) }).await;
    queue.finish(7, r1.token);
    assert!(queue.has_pending());
    queue.finish(7, r2.token);
    assert!(!queue.has_pending());
    assert!(!r2.outcome.is_member);
}

// MARK: - 키 접근

#[test]
fn encrypted_playback_requires_authenticated_key() {
    let mut info = LiveInfo::new(21415131, "Protected live");
    info.live_playback_json = Some(
        r#"{"media":[{"mediaId":"HLS","protocol":"HLS","path":"https://video.pstatic.net/protected.m3u8","aes":"HLS_AES"}]}"#
            .into(),
    );
    assert!(requires_authenticated_key(&info, false));
    assert!(!requires_authenticated_key(&info, true));
    let access = PlaybackKeyAccess::for_plan(&info, false);
    assert!(access.requires_proxy);
    assert_eq!(
        access.proxy_mode(StreamProxyMode::None),
        StreamProxyMode::LocalProxy
    );
}

#[test]
fn unencrypted_playback_keeps_selected_mode() {
    let mut info = LiveInfo::new(1, "Plain");
    info.live_playback_json = Some(
        r#"{"media":[{"mediaId":"HLS","path":"https://video.pstatic.net/plain.m3u8"}]}"#.into(),
    );
    assert!(!requires_authenticated_key(&info, false));
    let access = PlaybackKeyAccess::for_plan(&info, false);
    assert_eq!(access, PlaybackKeyAccess::ANONYMOUS);
    assert_eq!(
        access.proxy_mode(StreamProxyMode::None),
        StreamProxyMode::None
    );
    info.live_playback_json = Some("not json".into());
    assert!(!requires_authenticated_key(&info, false));
}

// MARK: - 채널 VOD 브라우저

struct FakeVodApi {
    total: usize,
    pages: Mutex<Vec<(String, i64)>>,
    fail_next: AtomicBool,
    search_hit: Option<(String, String)>,
}

impl FakeVodApi {
    fn new(total: usize) -> Arc<Self> {
        Arc::new(Self {
            total,
            pages: Mutex::new(Vec::new()),
            fail_next: AtomicBool::new(false),
            search_hit: Some(("found".into(), "찾은 채널".into())),
        })
    }
}

#[async_trait]
impl VodChannelApi for FakeVodApi {
    async fn vod_list(
        &self,
        channel_id: &str,
        page: i64,
        size: i64,
    ) -> Result<PagedContent<VodInfo>, String> {
        self.pages
            .lock()
            .unwrap()
            .push((channel_id.to_string(), page));
        if self.fail_next.swap(false, Ordering::SeqCst) {
            return Err("network".into());
        }
        let start = (page * size) as usize;
        let end = (start + size as usize).min(self.total);
        let data: Vec<VodInfo> = (start..end)
            .map(|i| {
                let mut v = VodInfo::new(i as i64, format!("v{i}"));
                v.channel = Some(ChannelInfo::new(channel_id, "목록 채널"));
                v
            })
            .collect();
        Ok(PagedContent::new(data.len() as i64, None, data, None))
    }
    async fn first_channel(&self, _keyword: &str) -> Result<Option<(String, String)>, String> {
        Ok(self.search_hit.clone())
    }
}

#[tokio::test]
async fn select_loads_first_page_and_pages_with_retry() {
    let api = FakeVodApi::new(60);
    let browser = VodChannelBrowser::new(api.clone());
    browser.select("cid").await.unwrap();
    {
        let st = browser.read();
        assert_eq!(st.channel_vods().len(), 24);
        assert_eq!(*st.channel_state(), VodLoadState::Loaded);
        assert!(st.channel_has_more());
        assert_eq!(st.resolved_channel_name(), Some("목록 채널"));
    }
    // 더 보기 실패 — 커서를 되돌리고 hasMore 는 유지.
    api.fail_next.store(true, Ordering::SeqCst);
    browser.load_more_channel().await;
    {
        let st = browser.read();
        assert_eq!(st.channel_page(), 0);
        assert!(st.channel_has_more());
        assert!(!st.is_loading_more_channel());
    }
    browser.load_more_channel().await;
    browser.load_more_channel().await;
    let st = browser.read();
    assert_eq!(st.channel_vods().len(), 60);
    assert!(!st.channel_has_more());
    let pages: Vec<i64> = api.pages.lock().unwrap().iter().map(|p| p.1).collect();
    assert_eq!(pages, vec![0, 1, 1, 2]);
}

#[tokio::test]
async fn first_load_failure_is_error_not_empty() {
    let api = FakeVodApi::new(10);
    api.fail_next.store(true, Ordering::SeqCst);
    let browser = VodChannelBrowser::new(api.clone());
    browser.select("cid").await.unwrap();
    assert_eq!(
        *browser.read().channel_state(),
        VodLoadState::Error("network".into())
    );
    browser.refresh().await;
    assert_eq!(*browser.read().channel_state(), VodLoadState::Loaded);
    // 보여 주던 목록이 있으면 새로고침 실패가 지우지 않는다.
    api.fail_next.store(true, Ordering::SeqCst);
    browser.refresh().await;
    assert_eq!(*browser.read().channel_state(), VodLoadState::Loaded);
    assert_eq!(browser.read().channel_vods().len(), 10);
}

#[tokio::test]
async fn resolve_falls_back_to_name_search() {
    let api = FakeVodApi::new(3);
    let browser = VodChannelBrowser::new(api.clone());
    browser.lock().channel_input = "  긴채널이름TheGreatStreamerGuy ".into();
    browser.resolve_and_load_channel().await;
    {
        let st = browser.read();
        assert_eq!(st.resolved_channel_id(), Some("found"));
        assert_eq!(st.resolved_channel_name(), Some("찾은 채널"));
        assert_eq!(*st.channel_state(), VodLoadState::Loaded);
    }
    let hex = "ab12".repeat(8);
    browser.lock().channel_input = format!("https://chzzk.naver.com/{hex}");
    browser.resolve_and_load_channel().await;
    assert_eq!(browser.read().resolved_channel_id(), Some(hex.as_str()));

    let none = Arc::new(FakeVodApi {
        total: 0,
        pages: Mutex::new(Vec::new()),
        fail_next: AtomicBool::new(false),
        search_hit: None,
    });
    let b2 = VodChannelBrowser::new(none);
    b2.lock().channel_input = "없음".into();
    b2.resolve_and_load_channel().await;
    assert_eq!(
        *b2.read().channel_state(),
        VodLoadState::Error("채널을 찾을 수 없습니다".into())
    );
    b2.invalidate();
    assert_eq!(*b2.read().channel_state(), VodLoadState::Idle);
    assert!(b2.snapshot().channel_input.is_empty());
}

#[tokio::test]
async fn empty_channel_is_empty_state() {
    let api = FakeVodApi::new(0);
    let browser = VodChannelBrowser::new(api);
    browser.select("cid").await.unwrap();
    assert_eq!(*browser.read().channel_state(), VodLoadState::Empty);
}
