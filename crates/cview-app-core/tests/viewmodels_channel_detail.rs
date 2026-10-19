//! ChannelDetailModel 테스트.
//!
//! 원본: `Tests/CViewAppTests/ChannelDetailModelHydrateTests.swift` ·
//! `ChannelDetailPagingRaceTests.swift` · `ChannelDetailModelMergeTests.swift`(일부) · `ChannelDetailTestKit.swift`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration as ChronoDuration, TimeZone, Utc};
use tokio::sync::Notify;

use cview_app_core::viewmodels::channel_detail::{
    ChannelDetailLoader, ChannelDetailModel, ChannelDetailSnapshot, ChannelSeed, MenuContentOrigin,
    PageFetch, SnapshotLoad,
};
use cview_core::models::search::{ClipInfo, VodInfo};
use cview_core::models::{ChannelInfo, LiveInfo, LiveStatus};

// MARK: - Kit

struct Clocks {
    elapsed: Mutex<f64>,
}

impl Clocks {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            elapsed: Mutex::new(0.0),
        })
    }
    fn mono(&self) -> f64 {
        10_000.0 + *self.elapsed.lock().unwrap()
    }
    fn wall(&self) -> DateTime<Utc> {
        Utc.timestamp_opt(1_800_000_000, 0).unwrap()
            + ChronoDuration::milliseconds((*self.elapsed.lock().unwrap() * 1000.0) as i64)
    }
    fn advance(&self, s: f64) {
        *self.elapsed.lock().unwrap() += s;
    }
}

fn channel_info(id: &str, name: &str, followers: i64) -> ChannelInfo {
    let mut c = ChannelInfo::new(id, name);
    c.follower_count = followers;
    c
}

fn vod(no: i64) -> VodInfo {
    VodInfo::new(no, format!("VOD {no}"))
}

fn clip(uid: &str) -> ClipInfo {
    ClipInfo::new(uid, format!("클립 {uid}"))
}

struct Net {
    calls: Mutex<[u32; 4]>,
    fails: AtomicBool,
    profile: Mutex<ChannelInfo>,
    live: Mutex<LiveInfo>,
    vods: Mutex<Vec<VodInfo>>,
    clips: Mutex<Vec<ClipInfo>>,
    profile_gate: Mutex<Option<Arc<Notify>>>,
}

impl Net {
    fn new() -> Arc<Self> {
        let mut live = LiveInfo::new(1, "종료");
        live.status = LiveStatus::Close;
        Arc::new(Self {
            calls: Mutex::new([0; 4]),
            fails: AtomicBool::new(false),
            profile: Mutex::new(channel_info("ch", "네트워크", 99)),
            live: Mutex::new(live),
            vods: Mutex::new((0..12).map(|i| vod(100 - i)).collect()),
            clips: Mutex::new((0..12).map(|i| clip(&format!("n{i}"))).collect()),
            profile_gate: Mutex::new(None),
        })
    }

    fn calls(&self) -> [u32; 4] {
        *self.calls.lock().unwrap()
    }

    fn loader(self: &Arc<Self>) -> ChannelDetailLoader {
        let (a, b, c, d) = (self.clone(), self.clone(), self.clone(), self.clone());
        ChannelDetailLoader {
            profile: Arc::new(move || {
                let n = a.clone();
                Box::pin(async move {
                    n.calls.lock().unwrap()[0] += 1;
                    let gate = n.profile_gate.lock().unwrap().clone();
                    if let Some(g) = gate {
                        g.notified().await;
                    }
                    if n.fails.load(Ordering::SeqCst) {
                        return Err("오프라인".into());
                    }
                    Ok(n.profile.lock().unwrap().clone())
                })
            }),
            live: Arc::new(move || {
                let n = b.clone();
                Box::pin(async move {
                    n.calls.lock().unwrap()[1] += 1;
                    if n.fails.load(Ordering::SeqCst) {
                        return Err("오프라인".into());
                    }
                    Ok(n.live.lock().unwrap().clone())
                })
            }),
            vod: Arc::new(move || {
                let n = c.clone();
                Box::pin(async move {
                    n.calls.lock().unwrap()[2] += 1;
                    if n.fails.load(Ordering::SeqCst) {
                        return Err("오프라인".into());
                    }
                    Ok(n.vods.lock().unwrap().clone())
                })
            }),
            clip: Arc::new(move || {
                let n = d.clone();
                Box::pin(async move {
                    n.calls.lock().unwrap()[3] += 1;
                    if n.fails.load(Ordering::SeqCst) {
                        return Err("오프라인".into());
                    }
                    Ok(n.clips.lock().unwrap().clone())
                })
            }),
        }
    }
}

fn snapshot(
    live: Option<LiveInfo>,
    profile_at: Option<DateTime<Utc>>,
    lists_at: Option<DateTime<Utc>>,
) -> ChannelDetailSnapshot {
    ChannelDetailSnapshot::new(
        "ch",
        Some(channel_info("ch", "스냅샷", 7)),
        live,
        (0..12).map(|i| vod(1_000 - i)).collect(),
        (0..12).map(|i| clip(&format!("s{i}"))).collect(),
        profile_at,
        lists_at,
    )
}

fn model(clocks: &Arc<Clocks>) -> ChannelDetailModel {
    let (c1, c2) = (clocks.clone(), clocks.clone());
    ChannelDetailModel::with_clocks(
        "ch",
        Arc::new(move || c1.mono()),
        Arc::new(move || c2.wall()),
        None,
    )
}

fn load_fixed(s: ChannelDetailSnapshot) -> SnapshotLoad {
    Arc::new(move |_| {
        let s = s.clone();
        Box::pin(async move { Some(s) })
    })
}

// MARK: - 로컬 우선 표시

#[tokio::test]
async fn hydrate_stamps_ages_into_ttl() {
    let clocks = Clocks::new();
    let net = Net::new();
    let m = model(&clocks);
    let snap = snapshot(
        None,
        Some(clocks.wall() - ChronoDuration::seconds(100)),
        Some(clocks.wall() - ChronoDuration::seconds(90)),
    );
    m.hydrate(load_fixed(snap)).await;
    {
        let s = m.read();
        assert_eq!(s.channel_info.as_ref().unwrap().channel_name, "스냅샷");
        assert_eq!(s.channel_info.as_ref().unwrap().follower_count, 7);
        assert_eq!((s.vod_list().len(), s.clip_list().len()), (12, 12));
        assert!(!s.is_loading());
        assert_eq!(s.first_content_origin, Some(MenuContentOrigin::Local));
    }
    m.load(false, net.loader()).await;
    assert_eq!(net.calls(), [0, 1, 1, 1]);
    let s = m.read();
    assert_eq!(s.channel_info.as_ref().unwrap().channel_name, "스냅샷");
    assert_eq!(
        s.vod_list()[0].video_no,
        100,
        "받은 목록이 스냅샷 목록을 대체한다"
    );
}

#[tokio::test]
async fn old_snapshot_profile_is_reloaded() {
    let clocks = Clocks::new();
    let net = Net::new();
    let m = model(&clocks);
    let old = clocks.wall() - ChronoDuration::seconds(400);
    m.hydrate(load_fixed(snapshot(None, Some(old), Some(old))))
        .await;
    m.load(false, net.loader()).await;
    assert_eq!(net.calls(), [1, 1, 1, 1]);
    assert_eq!(m.read().channel_info.as_ref().unwrap().follower_count, 99);
}

#[tokio::test]
async fn unusable_timestamps_are_due_immediately() {
    for future in [true, false] {
        let clocks = Clocks::new();
        let net = Net::new();
        let m = model(&clocks);
        let at = future.then(|| clocks.wall() + ChronoDuration::seconds(500));
        m.hydrate(load_fixed(snapshot(None, at, at))).await;
        assert!(m.read().channel_info.is_some());
        m.load(false, net.loader()).await;
        assert_eq!(net.calls(), [1, 1, 1, 1]);
    }
}

#[tokio::test]
async fn snapshot_beats_seed() {
    let clocks = Clocks::new();
    let m = model(&clocks);
    m.adopt_hint(ChannelSeed::new(channel_info("ch", "시드", 0), None));
    assert_eq!(
        m.read().first_content_origin,
        Some(MenuContentOrigin::Local)
    );
    m.hydrate(Arc::new(|_| Box::pin(async { None }))).await;
    assert_eq!(m.read().channel_info.as_ref().unwrap().channel_name, "시드");
    m.hydrate(load_fixed(snapshot(
        None,
        Some(clocks.wall()),
        Some(clocks.wall()),
    )))
    .await;
    assert_eq!(
        m.read().channel_info.as_ref().unwrap().channel_name,
        "스냅샷"
    );
}

#[tokio::test]
async fn hydrate_never_overwrites_network_content() {
    let clocks = Clocks::new();
    let net = Net::new();
    let m = model(&clocks);
    m.load(false, net.loader()).await;
    assert_eq!(
        m.read().first_content_origin,
        Some(MenuContentOrigin::Network)
    );
    m.hydrate(load_fixed(snapshot(
        None,
        Some(clocks.wall()),
        Some(clocks.wall()),
    )))
    .await;
    let s = m.read();
    assert_eq!(s.channel_info.as_ref().unwrap().channel_name, "네트워크");
    assert_eq!(s.vod_list()[0].video_no, 100);
    assert_eq!(s.clip_list()[0].clip_uid, "n0");
}

#[tokio::test]
async fn hydrate_fills_lists_only_when_empty() {
    let clocks = Clocks::new();
    let m = model(&clocks);
    m.lock()
        .set_vod_list((0..30).map(|i| vod(500 + i)).collect());
    m.hydrate(load_fixed(snapshot(
        None,
        Some(clocks.wall()),
        Some(clocks.wall()),
    )))
    .await;
    let s = m.read();
    assert_eq!(s.vod_list().len(), 30);
    assert_eq!(s.vod_list()[0].video_no, 500);
    assert_eq!(s.clip_list().len(), 12);
}

#[tokio::test]
async fn hydrated_live_stays_pending_until_confirmed() {
    let clocks = Clocks::new();
    let net = Net::new();
    let m = model(&clocks);
    let mut live = LiveInfo::new(7, "어제 방송");
    live.concurrent_user_count = 5;
    m.hydrate(load_fixed(snapshot(
        Some(live),
        Some(clocks.wall()),
        Some(clocks.wall()),
    )))
    .await;
    assert_eq!(m.read().live_info.as_ref().unwrap().live_title, "어제 방송");
    assert!(m.read().is_live_status_pending);
    m.load(false, net.loader()).await;
    assert!(m.read().live_info.is_none());
    assert!(!m.read().is_live_status_pending);
}

#[test]
fn hint_live_decides_pending() {
    let clocks = Clocks::new();
    let with_live = model(&clocks);
    with_live.adopt_hint(ChannelSeed::new(
        channel_info("ch", "검색", 0),
        Some(LiveInfo::new(1, "방송")),
    ));
    assert!(!with_live.read().is_live_status_pending);
    assert!(with_live.read().live_info.is_some());
    let profile_only = model(&clocks);
    profile_only.adopt_hint(ChannelSeed::new(channel_info("ch", "검색", 0), None));
    assert!(profile_only.read().is_live_status_pending);
    assert!(profile_only.read().live_info.is_none());
}

#[tokio::test]
async fn invalidate_discards_late_hydrate() {
    let clocks = Clocks::new();
    let m = model(&clocks);
    let snap = snapshot(None, Some(clocks.wall()), Some(clocks.wall()));
    let m2 = m.clone();
    m.hydrate(Arc::new(move |_| {
        let (m2, snap) = (m2.clone(), snap.clone());
        Box::pin(async move {
            m2.invalidate();
            Some(snap)
        })
    }))
    .await;
    let s = m.read();
    assert!(s.channel_info.is_none() && s.vod_list().is_empty() && s.clip_list().is_empty());
    assert!(s.is_loading());
}

#[tokio::test]
async fn background_hydrate_is_cancelled_by_invalidate() {
    let clocks = Clocks::new();
    let m = model(&clocks);
    let gate = Arc::new(Notify::new());
    let snap = snapshot(None, Some(clocks.wall()), Some(clocks.wall()));
    let g2 = gate.clone();
    m.begin_hydration(Arc::new(move |_| {
        let (g, snap) = (g2.clone(), snap.clone());
        Box::pin(async move {
            g.notified().await;
            Some(snap)
        })
    }));
    assert!(m.read().hydration.is_some());
    m.invalidate();
    gate.notify_waiters();
    tokio::task::yield_now().await;
    let s = m.read();
    assert!(s.channel_info.is_none() && s.vod_list().is_empty());
    assert!(s.hydration.is_none());
}

#[tokio::test]
async fn profile_invalidated_while_hydrating_keeps_lists() {
    let clocks = Clocks::new();
    let net = Net::new();
    let m = model(&clocks);
    let snap = snapshot(None, Some(clocks.wall()), Some(clocks.wall()));
    let m2 = m.clone();
    m.hydrate(Arc::new(move |_| {
        let (m2, snap) = (m2.clone(), snap.clone());
        Box::pin(async move {
            m2.invalidate_profile();
            Some(snap)
        })
    }))
    .await;
    assert!(
        m.read().channel_info.is_none(),
        "낡았다고 표시된 프로필은 되살리지 않는다"
    );
    assert_eq!(m.read().vod_list().len(), 12);
    m.load(false, net.loader()).await;
    assert_eq!(net.calls(), [1, 1, 0, 0]);
}

#[tokio::test]
async fn invalidate_profile_keeps_lists_and_reloads_profile_only() {
    let clocks = Clocks::new();
    let net = Net::new();
    let m = model(&clocks);
    m.load(false, net.loader()).await;
    let vods = m.read().vod_list().to_vec();
    m.invalidate_profile();
    assert_eq!(
        m.read().channel_info.as_ref().unwrap().channel_name,
        "네트워크"
    );
    *net.profile.lock().unwrap() = channel_info("ch", "새 프로필", 100);
    m.load(false, net.loader()).await;
    assert_eq!(net.calls(), [2, 1, 1, 1]);
    assert_eq!(m.read().channel_info.as_ref().unwrap().follower_count, 100);
    assert_eq!(m.read().vod_list(), vods.as_slice());
}

#[tokio::test]
async fn profile_response_racing_invalidation_stays_due() {
    let clocks = Clocks::new();
    let net = Net::new();
    let gate = Arc::new(Notify::new());
    *net.profile_gate.lock().unwrap() = Some(gate.clone());
    let m = model(&clocks);
    let (m2, loader) = (m.clone(), net.loader());
    let task = tokio::spawn(async move { m2.load(false, loader).await });
    for _ in 0..1000 {
        if net.calls()[0] == 1 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(net.calls()[0], 1);
    m.invalidate_profile();
    *net.profile_gate.lock().unwrap() = None;
    gate.notify_waiters();
    task.await.unwrap();
    assert!(m.read().channel_info.is_some(), "받은 프로필은 보여 준다");
    m.load(false, net.loader()).await;
    assert_eq!(net.calls(), [2, 1, 1, 1]);
}

#[tokio::test]
async fn ttl_and_failure_messages() {
    let clocks = Clocks::new();
    let net = Net::new();
    let m = model(&clocks);
    net.fails.store(true, Ordering::SeqCst);
    m.load(false, net.loader()).await;
    assert_eq!(m.read().error_message.as_deref(), Some("오프라인"));
    net.fails.store(false, Ordering::SeqCst);
    m.load(false, net.loader()).await;
    assert!(m.read().error_message.is_none());
    assert_eq!(net.calls(), [2, 2, 2, 2]);
    clocks.advance(20.0);
    m.load(false, net.loader()).await;
    assert_eq!(net.calls(), [2, 3, 2, 2], "15초 지난 방송만 다시 받는다");
    net.fails.store(true, Ordering::SeqCst);
    m.load(true, net.loader()).await;
    assert_eq!(
        m.read().refresh_error.as_deref(),
        Some(ChannelDetailModel::REFRESH_FAILURE_MESSAGE)
    );
}

#[tokio::test]
async fn persist_snapshot_after_successful_load() {
    let clocks = Clocks::new();
    let net = Net::new();
    let saved: Arc<Mutex<Vec<ChannelDetailSnapshot>>> = Arc::default();
    let s2 = saved.clone();
    let (c1, c2) = (clocks.clone(), clocks.clone());
    let m = ChannelDetailModel::with_clocks(
        "ch",
        Arc::new(move || c1.mono()),
        Arc::new(move || c2.wall()),
        Some(Arc::new(move |snap| {
            let s2 = s2.clone();
            Box::pin(async move { s2.lock().unwrap().push(snap) })
        })),
    );
    m.load(false, net.loader()).await;
    tokio::task::yield_now().await;
    assert_eq!(m.read().saves_scheduled, 1);
    let saved = saved.lock().unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].profile_fetched_at, Some(clocks.wall()));
    assert_eq!(saved[0].lists_fetched_at, Some(clocks.wall()));
    assert!(
        saved[0].live_info().is_none(),
        "종료 방송은 저장하지 않는다"
    );
    assert_eq!(saved[0].vod_infos().len(), 12);
}

// MARK: - 페이지 경합 (ChannelDetailPagingRaceTests)

fn basic_loader() -> ChannelDetailLoader {
    ChannelDetailLoader {
        profile: Arc::new(|| Box::pin(async { Ok(ChannelInfo::new("test", "Test")) })),
        live: Arc::new(|| Box::pin(async { Ok(LiveInfo::new(1, "Live")) })),
        vod: Arc::new(|| Box::pin(async { Ok((0..12).map(|i| VodInfo::new(i, "VOD")).collect()) })),
        clip: Arc::new(|| {
            Box::pin(async {
                Ok((0..12)
                    .map(|i| ClipInfo::new(i.to_string(), "Clip"))
                    .collect())
            })
        }),
    }
}

struct PageGate {
    waiting: Mutex<usize>,
    notify: Notify,
    open: AtomicBool,
}

impl PageGate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            waiting: Mutex::new(0),
            notify: Notify::new(),
            open: AtomicBool::new(false),
        })
    }
    async fn wait(&self) {
        *self.waiting.lock().unwrap() += 1;
        loop {
            let n = self.notify.notified();
            if self.open.load(Ordering::SeqCst) {
                return;
            }
            n.await;
        }
    }
    fn finish(&self) {
        self.open.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }
    async fn wait_for_three(&self) -> bool {
        for _ in 0..1000 {
            if *self.waiting.lock().unwrap() == 3 {
                return true;
            }
            tokio::task::yield_now().await;
        }
        false
    }
}

fn gated_vods(gate: Arc<PageGate>, fails: bool) -> PageFetch<VodInfo> {
    Arc::new(move |_| {
        let g = gate.clone();
        Box::pin(async move {
            g.wait().await;
            if fails {
                Err("timeout".into())
            } else {
                Ok(Vec::new())
            }
        })
    })
}

fn gated_clips(gate: Arc<PageGate>, fails: bool) -> PageFetch<ClipInfo> {
    Arc::new(move |_| {
        let g = gate.clone();
        Box::pin(async move {
            g.wait().await;
            if fails {
                Err("timeout".into())
            } else {
                Ok(Vec::new())
            }
        })
    })
}

#[tokio::test]
async fn old_page_completion_preserves_newer_loading_flag() {
    for vod in [true, false] {
        for old_fails in [true, false] {
            let m = ChannelDetailModel::new("test");
            let (old, new) = (PageGate::new(), PageGate::new());
            m.load(false, basic_loader()).await;
            let m1 = m.clone();
            let o = old.clone();
            let first = tokio::spawn(async move {
                if vod {
                    m1.load_more_vods(Some(gated_vods(o, old_fails))).await
                } else {
                    m1.load_more_clips(Some(gated_clips(o, old_fails))).await
                }
            });
            assert!(old.wait_for_three().await);
            m.invalidate();
            {
                let s = m.read();
                assert!(s.vod_page == 0 && s.clip_page == 0 && s.has_more_vods && s.has_more_clips);
            }
            m.load(false, basic_loader()).await;
            let m2 = m.clone();
            let n2 = new.clone();
            let second = tokio::spawn(async move {
                if vod {
                    m2.load_more_vods(Some(gated_vods(n2, false))).await
                } else {
                    m2.load_more_clips(Some(gated_clips(n2, false))).await
                }
            });
            assert!(new.wait_for_three().await);
            old.finish();
            first.await.unwrap();
            {
                let s = m.read();
                assert!(if vod {
                    s.is_loading_more_vods
                } else {
                    s.is_loading_more_clips
                });
                assert!(s.vod_list().len() == 12 && s.clip_list().len() == 12);
            }
            new.finish();
            second.await.unwrap();
            let s = m.read();
            assert!(!s.is_loading_more_vods && !s.is_loading_more_clips);
            assert!(if vod {
                !s.has_more_vods
            } else {
                !s.has_more_clips
            });
        }
    }
}

#[tokio::test]
async fn paging_needs_first_page() {
    let m = ChannelDetailModel::new("test");
    m.load_more_vods(Some(Arc::new(|_| {
        Box::pin(async { panic!("Unexpected page request") })
    })))
    .await;
    m.load_more_clips(Some(Arc::new(|_| {
        Box::pin(async { panic!("Unexpected page request") })
    })))
    .await;
    assert!(m.read().vod_page == 0 && m.read().clip_page == 0);
}

#[tokio::test]
async fn partial_middle_page_stops_ordered_assembly() {
    let m = ChannelDetailModel::new("test");
    m.load(false, basic_loader()).await;
    m.load_more_vods(Some(Arc::new(|page| {
        Box::pin(async move {
            let n = if page == 2 { 1 } else { 12 };
            Ok((0..n).map(|i| VodInfo::new(page * 12 + i, "VOD")).collect())
        })
    })))
    .await;
    let ids: Vec<i64> = m.read().vod_list().iter().map(|v| v.video_no).collect();
    assert_eq!(ids, (0..25).collect::<Vec<_>>());
    assert!(m.read().vod_page == 2 && !m.read().has_more_vods);

    let m = ChannelDetailModel::new("test");
    m.load(false, basic_loader()).await;
    m.load_more_clips(Some(Arc::new(|page| {
        Box::pin(async move {
            let n = if page == 2 { 1 } else { 12 };
            Ok((0..n)
                .map(|i| ClipInfo::new((page * 12 + i).to_string(), "Clip"))
                .collect())
        })
    })))
    .await;
    let ids: Vec<String> = m
        .read()
        .clip_list()
        .iter()
        .map(|c| c.clip_uid.clone())
        .collect();
    assert_eq!(ids, (0..25).map(|i| i.to_string()).collect::<Vec<_>>());
    assert!(m.read().clip_page == 2 && !m.read().has_more_clips);
}
