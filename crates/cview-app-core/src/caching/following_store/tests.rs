//! 원본: `Tests/CViewAppTests/FollowingStoreTests.swift` 핵심 계약.
use super::*;
use crate::services::support::ManualClock;
use cview_core::utils::live_detail_refresh::LiveDetailReason;
use cview_core::utils::time::from_unix;
use cview_networking::response::FollowingListLiveInfo;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Semaphore;

#[derive(Default)]
struct FakeState {
    rows: Vec<FollowingRow>,
    carries: bool,
    fail_next: bool,
    hold: bool,
    holding: usize,
    page_requests: usize,
    detail_requests: Vec<String>,
}

struct Fake {
    state: Mutex<FakeState>,
    gate: Semaphore,
}

impl Fake {
    fn new(rows: Vec<FollowingRow>) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(FakeState {
                rows,
                ..FakeState::default()
            }),
            gate: Semaphore::new(0),
        })
    }
    fn page_requests(&self) -> usize {
        lock(&self.state).page_requests
    }
    fn details(&self) -> Vec<String> {
        lock(&self.state).detail_requests.clone()
    }
    fn set_rows(&self, rows: Vec<FollowingRow>) {
        lock(&self.state).rows = rows;
    }
    async fn wait_until_holding(&self) {
        while lock(&self.state).holding == 0 {
            tokio::task::yield_now().await;
        }
    }
    /// 지금 붙잡힌 요청만 놓아 준다(남는 허가가 다음 보류를 뚫지 않게).
    fn release(&self) {
        let held = {
            let mut s = lock(&self.state);
            s.hold = false;
            s.holding
        };
        self.gate.add_permits(held);
    }
}

#[async_trait]
impl FollowingSource for Fake {
    async fn fetch_following_rows(&self) -> Result<FollowingRowsPage, ApiError> {
        let hold = {
            let mut s = lock(&self.state);
            s.page_requests += 1;
            if s.hold {
                s.holding += 1;
            }
            s.hold
        };
        if hold {
            self.gate.acquire().await.unwrap().forget();
            lock(&self.state).holding -= 1;
        }
        let mut s = lock(&self.state);
        if std::mem::take(&mut s.fail_next) {
            return Err(ApiError::NetworkError("offline".into()));
        }
        Ok(FollowingRowsPage {
            rows: s.rows.clone(),
            total_count: Some(s.rows.len() as i64),
            carries_live_info: s.carries,
            fetched_at: from_unix(1_700_000_000.0),
        })
    }

    async fn live_detail(&self, channel_id: &str) -> Option<LiveDetailSnapshot> {
        lock(&self.state).detail_requests.push(channel_id.into());
        Some(LiveDetailSnapshot {
            live_id: 1,
            live_title: format!("{channel_id}-title"),
            concurrent_user_count: 100,
            ..LiveDetailSnapshot::default()
        })
    }
}

fn row(id: &str, live: bool) -> FollowingRow {
    FollowingRow {
        channel_id: id.into(),
        channel_name: id.into(),
        channel_image_url: None,
        is_live: live,
        list_live_info: None,
        verified: false,
        follow: Default::default(),
    }
}

fn store(fake: &Arc<Fake>, clock: &ManualClock) -> FollowingStore {
    let source: Arc<dyn FollowingSource> = fake.clone();
    FollowingStore::with_clocks(
        Some(source),
        clock.uptime_fn(),
        clock.clock(),
        Arc::new(|| false),
    )
}

fn counting_sink(counter: &Arc<AtomicUsize>, success: bool) -> Sink {
    let counter = counter.clone();
    Arc::new(move |outcome: PollOutcome| {
        if matches!(outcome, FollowingPollOutcome::Success(_)) == success {
            counter.fetch_add(1, Ordering::SeqCst);
        }
        async {}.boxed()
    })
}

const MANUAL: FollowingPollReason = FollowingPollReason::Manual {
    invalidate_thumbnails: false,
};

#[tokio::test]
async fn concurrent_polls_share_one_flight() {
    let fake = Fake::new(vec![row("a", true)]);
    lock(&fake.state).hold = true;
    let clock = ManualClock::new(from_unix(0.0));
    let s = store(&fake, &clock);
    let first = tokio::spawn(s.poll(FollowingPollReason::Scheduled));
    fake.wait_until_holding().await;
    assert!(s.is_polling());
    let second = tokio::spawn(s.poll(FollowingPollReason::Scheduled));
    tokio::task::yield_now().await;
    fake.release();
    let (a, b) = (first.await.unwrap(), second.await.unwrap());
    assert_eq!(fake.page_requests(), 1);
    assert!(a.is_some());
    assert_eq!(a, b, "합류한 호출은 같은 결과를 받는다");
    assert!(!s.is_polling());
    assert!(!s.has_in_flight_poll());
}

#[tokio::test]
async fn manual_after_scheduled_runs_again_but_manual_joins_manual() {
    let fake = Fake::new(vec![row("a", true)]);
    lock(&fake.state).hold = true;
    let clock = ManualClock::new(from_unix(0.0));
    let s = store(&fake, &clock);
    let scheduled = tokio::spawn(s.poll(FollowingPollReason::Scheduled));
    fake.wait_until_holding().await;
    let manual = tokio::spawn(s.poll(MANUAL));
    tokio::task::yield_now().await;
    fake.release();
    assert_eq!(
        scheduled.await.unwrap().unwrap().reason,
        FollowingPollReason::Scheduled
    );
    assert_eq!(manual.await.unwrap().unwrap().reason, MANUAL);
    assert_eq!(
        fake.page_requests(),
        2,
        "수동 새로고침이 진행 중이던 목록을 그대로 받았다"
    );

    lock(&fake.state).hold = true;
    let m1 = tokio::spawn(s.poll(MANUAL));
    fake.wait_until_holding().await;
    let m2 = tokio::spawn(s.poll(MANUAL));
    tokio::task::yield_now().await;
    fake.release();
    m1.await.unwrap();
    m2.await.unwrap();
    assert_eq!(fake.page_requests(), 3);
}

#[tokio::test]
async fn boundary_discards_late_result() {
    let fake = Fake::new(vec![row("a", true)]);
    lock(&fake.state).hold = true;
    let clock = ManualClock::new(from_unix(0.0));
    let s = store(&fake, &clock);
    let delivered = Arc::new(AtomicUsize::new(0));
    s.set_sink(
        FollowingPollSinkSlot::Home,
        Some(counting_sink(&delivered, true)),
    );
    let poll = tokio::spawn(s.poll(FollowingPollReason::Scheduled));
    fake.wait_until_holding().await;
    s.reset_for_account_boundary();
    fake.release();
    assert!(poll.await.unwrap().is_none());
    assert_eq!(
        delivered.load(Ordering::SeqCst),
        0,
        "이전 계정의 결과가 홈에 반영됐다"
    );
    assert!(s.last_result().is_none());
    assert_eq!(s.live_count(), 0);
    assert!(!s.is_polling());
}

#[tokio::test]
async fn screen_entry_reuses_recent_result_and_retries_after_failure() {
    let fake = Fake::new(vec![row("a", true)]);
    let clock = ManualClock::new(from_unix(0.0));
    let s = store(&fake, &clock);
    let delivered = Arc::new(AtomicUsize::new(0));
    s.set_sink(
        FollowingPollSinkSlot::Home,
        Some(counting_sink(&delivered, true)),
    );
    let first = s.poll(FollowingPollReason::Scheduled).await;
    clock.advance(30.0);
    let reused = s.poll(FollowingPollReason::ScreenEntry).await;
    assert_eq!(reused, first);
    assert_eq!(fake.page_requests(), 1);
    assert_eq!(
        delivered.load(Ordering::SeqCst),
        1,
        "재사용한 결과를 싱크에 다시 넘겼다"
    );
    clock.advance(31.0);
    s.poll(FollowingPollReason::ScreenEntry).await;
    assert_eq!(fake.page_requests(), 2);
    assert_eq!(delivered.load(Ordering::SeqCst), 2);

    // 실패 뒤의 진입은 창 안이어도 다시 받는다.
    let failures = Arc::new(AtomicUsize::new(0));
    s.set_sink(
        FollowingPollSinkSlot::Notifier,
        Some(counting_sink(&failures, false)),
    );
    lock(&fake.state).fail_next = true;
    assert!(s.poll(FollowingPollReason::Scheduled).await.is_none());
    assert!(s.last_failure_at().is_some());
    assert_eq!(failures.load(Ordering::SeqCst), 1);
    assert!(s.last_result().is_some(), "실패가 lastResult 를 지웠다");
    assert!(s.poll(FollowingPollReason::ScreenEntry).await.is_some());
    assert_eq!(fake.page_requests(), 4);
    assert!(s.last_failure_at().is_none());
    s.poll(FollowingPollReason::ScreenEntry).await;
    assert_eq!(
        fake.page_requests(),
        4,
        "성공 뒤의 진입은 다시 재사용해야 한다"
    );
}

#[test]
fn reuse_age_follows_cadence_with_cap() {
    assert_eq!(FollowingStore::reuse_age(None), 60.0);
    assert_eq!(FollowingStore::reuse_age(Some(30.0)), 60.0);
    assert!((FollowingStore::reuse_age(Some(120.0)) - 138.0).abs() < 1e-9);
    assert_eq!(FollowingStore::reuse_age(Some(600.0)), 180.0);
}

#[tokio::test]
async fn result_order_is_live_first_then_api_order() {
    let fake = Fake::new(vec![
        row("a", false),
        row("b", true),
        row("c", false),
        row("d", true),
    ]);
    let clock = ManualClock::new(from_unix(0.0));
    let s = store(&fake, &clock);
    let result = s.poll(FollowingPollReason::Boot).await.unwrap();
    let ids: Vec<&str> = result
        .channels
        .iter()
        .map(|c| c.channel_id.as_str())
        .collect();
    assert_eq!(ids, vec!["b", "d", "a", "c"]);
    assert!(result.is_baseline);
    assert!(result.newly_live.is_empty());
    assert_eq!(s.live_count(), 2);
    assert!(result.channels[..2]
        .iter()
        .all(|c| c.is_live && c.viewer_count == 100));
}

#[tokio::test]
async fn new_live_is_detected_and_fetched_first() {
    let fake = Fake::new(vec![row("a", true), row("x", false)]);
    let clock = ManualClock::new(from_unix(0.0));
    let s = store(&fake, &clock);
    s.poll(FollowingPollReason::Boot).await.unwrap();
    let before = fake.details().len();
    fake.set_rows(vec![row("a", true), row("x", true)]);
    clock.advance(10.0);
    let result = s.poll(FollowingPollReason::Scheduled).await.unwrap();
    assert_eq!(result.newly_live, HashSet::from(["x".to_string()]));
    assert!(!result.is_baseline);
    assert_eq!(
        result.detail_requests.first(),
        Some(&LiveDetailRequest::new("x", LiveDetailReason::NewLive))
    );
    assert_eq!(
        fake.details()[before..],
        ["x".to_string()],
        "기한이 남은 채널의 상세를 다시 받았다"
    );
    let x = result
        .channels
        .iter()
        .find(|c| c.channel_id == "x")
        .unwrap();
    assert_eq!(x.live_title, "x-title");
}

#[tokio::test]
async fn list_live_info_wins_over_stale_detail() {
    let fake = Fake::new(vec![row("a", true)]);
    let clock = ManualClock::new(from_unix(0.0));
    let s = store(&fake, &clock);
    s.poll(FollowingPollReason::Boot).await.unwrap();
    let before = fake.details().len();
    let mut r = row("a", true);
    r.list_live_info = Some(FollowingListLiveInfo {
        live_title: Some("renamed".into()),
        concurrent_user_count: Some(999),
        live_category_value: None,
    });
    fake.set_rows(vec![r]);
    lock(&fake.state).carries = true;
    clock.advance(10.0);
    let result = s.poll(FollowingPollReason::Scheduled).await.unwrap();
    assert_eq!(fake.details().len(), before);
    assert!(result.list_carries_live_info);
    let c = &result.channels[0];
    assert!(c.viewer_count == 999 && c.live_title == "renamed");
}

#[tokio::test]
async fn notify_watch_fetches_every_poll() {
    let fake = Fake::new(vec![row("a", true), row("b", true)]);
    let clock = ManualClock::new(from_unix(0.0));
    let s = store(&fake, &clock);
    s.set_notify_watch_provider(Arc::new(|| Arc::new(|id: &str| id == "b")));
    s.poll(FollowingPollReason::Boot).await.unwrap();
    let before = fake.details().len();
    clock.advance(5.0);
    s.poll(FollowingPollReason::Scheduled).await.unwrap();
    assert_eq!(fake.details()[before..], ["b".to_string()]);
}

#[tokio::test]
async fn following_set_change_is_reported() {
    let fake = Fake::new(vec![row("a", true), row("b", false)]);
    let clock = ManualClock::new(from_unix(0.0));
    let s = store(&fake, &clock);
    let changes: Arc<Mutex<Vec<HashSet<String>>>> = Arc::default();
    let sink = changes.clone();
    s.set_on_following_set_changed(Some(Arc::new(move |c| lock(&sink).push(c))));
    s.poll(FollowingPollReason::Boot).await.unwrap();
    assert!(lock(&changes).is_empty());
    fake.set_rows(vec![row("a", true), row("c", false)]);
    s.poll(MANUAL).await.unwrap();
    assert_eq!(
        *lock(&changes),
        vec![HashSet::from(["b".to_string(), "c".to_string()])]
    );
}

#[tokio::test]
async fn poll_finished_reports_success_flag() {
    let fake = Fake::new(vec![row("a", true)]);
    let clock = ManualClock::new(from_unix(0.0));
    let s = store(&fake, &clock);
    let seen: Arc<Mutex<Vec<bool>>> = Arc::default();
    let sink = seen.clone();
    s.set_on_poll_finished(Some(Arc::new(move |ok| lock(&sink).push(ok))));
    s.poll(FollowingPollReason::Scheduled).await;
    lock(&fake.state).fail_next = true;
    s.poll(FollowingPollReason::Scheduled).await;
    assert_eq!(*lock(&seen), vec![true, false]);
}
