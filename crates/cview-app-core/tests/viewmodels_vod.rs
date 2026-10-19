//! 원본: `Tests/CViewAppTests/VODProgressPersistenceWriterTests.swift`, `VODProgressMirrorTests.swift`,
//! `VODLocalFirstFeedTests.swift`, `VODSyncEntryBudgetTests.swift`, `VODAccountSwitchTests.swift` 핵심.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use futures_util::FutureExt;
use tokio::sync::Notify;
use url::Url;

use cview_app_core::services::support::Clock;
use cview_app_core::services::vod_feed::{
    ChannelPageResult, VodPageSource, VodSubscriptionFeedCursor,
};
use cview_app_core::services::vod_playback_progress::{
    VodPlaybackProgressStore, VodProgressDisposition,
};
use cview_app_core::services::vod_sync_store::{VodFollowDetails, VodSyncStore};
use cview_app_core::viewmodels::vod_browse::{
    cached_data, VodBrowseDeps, VodBrowseViewModel, VodSearchApi,
};
use cview_app_core::viewmodels::vod_library::{VodChannelApi, VodLoadState};
use cview_app_core::viewmodels::vod_player::{
    format_time, PersistFn, VodPlayback, VodPlayerEngine, VodPlayerViewModel,
    VodProgressPersistenceSnapshot, VodProgressPersistenceWriter, VodStreamSource,
};
use cview_core::error::ApiError;
use cview_core::models::channel::ChannelInfo;
use cview_core::models::player::PlayerPhase;
use cview_core::models::search::VodInfo;
use cview_core::models::vod::{PlaybackSpeed, VodQuality, VodStreamInfo};
use cview_core::utils::time::{adding, from_unix, Date};
use cview_networking::PagedContent;
use cview_persistence::models::cached_vod::CachedVod;
use cview_persistence::store::vod_cache::SubscriptionVodPage;
use cview_persistence::DataStore;

// MARK: - writer

#[derive(Default)]
struct Probe {
    values: Mutex<Vec<VodProgressPersistenceSnapshot>>,
    started: Notify,
    release: Notify,
    released: Mutex<bool>,
}

fn probe_writer(probe: Arc<Probe>) -> VodProgressPersistenceWriter {
    let persist: PersistFn = Arc::new(move |s| {
        let probe = probe.clone();
        async move {
            let first = {
                let mut v = probe.values.lock().unwrap();
                v.push(s);
                v.len() == 1
            };
            if first {
                probe.started.notify_one();
                if !*probe.released.lock().unwrap() {
                    probe.release.notified().await;
                }
            }
        }
        .boxed()
    });
    VodProgressPersistenceWriter::new(None, persist)
}

fn release(probe: &Probe) {
    *probe.released.lock().unwrap() = true;
    probe.release.notify_one();
}

fn snap(
    video_no: i64,
    generation: u64,
    position: f64,
    ended: bool,
) -> VodProgressPersistenceSnapshot {
    VodProgressPersistenceSnapshot {
        video_no,
        generation,
        position,
        duration: 1_000.0,
        video_title: format!("video-{video_no}"),
        channel_name: "channel".into(),
        thumbnail_url_string: None,
        playback_ended: ended,
    }
}

#[tokio::test]
async fn writer_serializes_and_coalesces_without_cross_video_loss() {
    let probe = Arc::new(Probe::default());
    let writer = probe_writer(probe.clone());
    writer.enqueue(snap(1, 1, 40.0, false));
    probe.started.notified().await;
    writer.enqueue(snap(1, 1, 50.0, false));
    writer.enqueue(snap(1, 1, 60.0, false));
    writer.enqueue(snap(2, 1, 35.0, false));
    release(&probe);
    writer.flush().await;
    let v = probe.values.lock().unwrap();
    assert_eq!(v.iter().map(|s| s.video_no).collect::<Vec<_>>(), [1, 1, 2]);
    assert_eq!(
        v.iter().map(|s| s.position).collect::<Vec<_>>(),
        [40.0, 60.0, 35.0]
    );
}

#[test]
fn disposition_keeps_continue_watching_contract() {
    assert_eq!(
        snap(1, 1, 30.0, false).disposition(),
        VodProgressDisposition::Ignore
    );
    assert_eq!(
        snap(1, 1, 31.0, false).disposition(),
        VodProgressDisposition::Record
    );
    assert_eq!(
        snap(1, 1, 950.0, false).disposition(),
        VodProgressDisposition::Remove
    );
    assert_eq!(
        snap(1, 1, 31.0, true).disposition(),
        VodProgressDisposition::Remove
    );
}

#[tokio::test]
async fn late_record_cannot_resurrect_pending_remove() {
    let probe = Arc::new(Probe::default());
    let writer = probe_writer(probe.clone());
    writer.enqueue(snap(1, 1, 40.0, false));
    probe.started.notified().await;
    writer.enqueue(snap(2, 1, 31.0, true));
    writer.enqueue(snap(2, 1, 80.0, false));
    writer.enqueue(snap(2, 1, 10.0, false));
    release(&probe);
    writer.flush().await;
    let v = probe.values.lock().unwrap();
    assert_eq!(v.iter().map(|s| s.video_no).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(
        v.last().unwrap().disposition(),
        VodProgressDisposition::Remove
    );
}

#[tokio::test]
async fn in_flight_remove_rejects_late_record_and_allows_next_generation() {
    let probe = Arc::new(Probe::default());
    let writer = probe_writer(probe.clone());
    writer.enqueue(snap(3, 4, 40.0, true));
    probe.started.notified().await;
    writer.enqueue(snap(3, 4, 80.0, false));
    writer.enqueue(snap(3, 3, 70.0, false));
    writer.enqueue(snap(3, 5, 60.0, false));
    writer.enqueue(snap(3, 4, 50.0, true));
    release(&probe);
    writer.flush().await;
    writer.enqueue(snap(3, 4, 90.0, false));
    writer.flush().await;
    let v = probe.values.lock().unwrap();
    assert_eq!(v.iter().map(|s| s.generation).collect::<Vec<_>>(), [4, 5]);
    assert_eq!(
        v.iter().map(|s| s.position).collect::<Vec<_>>(),
        [40.0, 60.0]
    );
}

#[tokio::test]
async fn completed_remove_keeps_generation_terminal() {
    let probe = Arc::new(Probe::default());
    release(&probe);
    let writer = probe_writer(probe.clone());
    writer.enqueue(snap(8, 7, 40.0, true));
    writer.flush().await;
    writer.enqueue(snap(8, 7, 80.0, false));
    writer.enqueue(snap(8, 6, 70.0, false));
    writer.flush().await;
    let v = probe.values.lock().unwrap();
    assert_eq!(v.iter().map(|s| s.generation).collect::<Vec<_>>(), [7]);
}

#[tokio::test]
async fn mirror_reflects_only_queued_values() {
    let mirror = Arc::new(Mutex::new(VodPlaybackProgressStore::new()));
    let persist: PersistFn = Arc::new(|_| async {}.boxed());
    let writer = VodProgressPersistenceWriter::new(Some(mirror.clone()), persist);
    writer.enqueue(snap(1, 1, 20.0, false)); // 무시 — 미러에도 없음
    assert!(mirror.lock().unwrap().progress(1).is_none());
    writer.enqueue(snap(1, 1, 120.0, false));
    assert_eq!(mirror.lock().unwrap().progress(1).unwrap().position, 120.0);
    writer.enqueue(snap(1, 1, 990.0, false));
    assert!(mirror.lock().unwrap().progress(1).is_none());
    writer.flush().await;
}

// MARK: - VOD 플레이어

#[derive(Default)]
struct FakeEngine {
    log: Mutex<Vec<String>>,
}

#[async_trait]
impl VodPlayerEngine for FakeEngine {
    async fn play_vod(&self, url: &Url) -> Result<(), String> {
        self.log.lock().unwrap().push(format!("play {url}"));
        Ok(())
    }
    fn pause(&self) {
        self.log.lock().unwrap().push("pause".into());
    }
    fn resume(&self) {
        self.log.lock().unwrap().push("resume".into());
    }
    fn seek(&self, to: f64) {
        self.log.lock().unwrap().push(format!("seek {to}"));
    }
    fn set_rate(&self, rate: f32) {
        self.log.lock().unwrap().push(format!("rate {rate}"));
    }
    fn set_volume(&self, v: f32) {
        self.log.lock().unwrap().push(format!("volume {v}"));
    }
    fn stop(&self) {
        self.log.lock().unwrap().push("stop".into());
    }
}

struct FakeResolver;

#[async_trait]
impl VodStreamSource for FakeResolver {
    async fn resolve_vod(&self, video_no: i64) -> Result<VodStreamInfo, String> {
        if video_no < 0 {
            return Err("없음".into());
        }
        Ok(VodStreamInfo {
            video_no,
            title: format!("vod {video_no}"),
            stream_url: Url::parse("https://example.com/master.m3u8").unwrap(),
            duration: Duration::from_secs(1_000),
            qualities: vec![VodQuality {
                id: "720".into(),
                name: "720p".into(),
                resolution: "1280x720".into(),
                bandwidth: 1,
                url: Url::parse("https://example.com/720.m3u8").unwrap(),
            }],
            channel_name: "ch".into(),
            thumbnail_url_string: None,
            channel_id: Some("c1".into()),
            read_count: 3,
            publish_date: None,
            thumbnail_url: None,
        })
    }
}

fn player(store: Option<DataStore>) -> (VodPlayerViewModel, Arc<FakeEngine>) {
    let engine = Arc::new(FakeEngine::default());
    let e = engine.clone();
    let writer = VodProgressPersistenceWriter::with_data_store(store.clone(), None);
    let lookup = store
        .map(|s| Arc::new(s) as Arc<dyn cview_app_core::viewmodels::vod_player::VodPlaybackLookup>);
    let vm = VodPlayerViewModel::new(
        Arc::new(FakeResolver),
        Arc::new(move || e.clone() as Arc<dyn VodPlayerEngine>),
        lookup,
        writer,
        None,
    );
    (vm, engine)
}

#[tokio::test]
async fn vod_player_resumes_saved_position_and_records_on_pause() {
    let store = DataStore::open_in_memory().unwrap();
    store
        .record_vod_playback(7, 300.0, 1_000.0, "t", "c", None)
        .unwrap();
    let (vm, engine) = player(Some(store.clone()));
    vm.start_vod(7).await;
    {
        let s = vm.read();
        assert_eq!(s.playback_state, VodPlayback::Playing);
        assert_eq!(s.resumed_from, Some(300.0));
        assert_eq!(s.current_time, 300.0);
    }
    assert!(engine.log.lock().unwrap().contains(&"seek 300".to_string()));

    let token = vm.read().engine_token();
    vm.on_engine_time(token, 420.0, 1_000.0);
    vm.toggle_play_pause();
    assert_eq!(vm.read().playback_state, VodPlayback::Paused);
    vm.writer().flush().await;
    assert_eq!(
        store
            .fetch_vod_playback(7)
            .unwrap()
            .unwrap()
            .last_position_sec,
        420.0
    );

    // 종료 → 이어 보기 제거.
    vm.on_engine_phase(token, &PlayerPhase::Ended);
    vm.writer().flush().await;
    assert!(store.fetch_vod_playback(7).unwrap().is_none());

    // 낡은 토큰은 무시.
    vm.on_engine_phase(token + 99, &PlayerPhase::Playing);
    assert_eq!(vm.read().playback_state, VodPlayback::Ended);
}

#[tokio::test]
async fn vod_player_quality_speed_mute_and_stop() {
    let (vm, engine) = player(None);
    vm.start_vod(1).await;
    vm.set_speed(PlaybackSpeed::X150);
    assert_eq!(vm.read().playback_speed, PlaybackSpeed::X150);
    vm.select_quality(Some("720")).await;
    assert_eq!(vm.read().current_quality_label(), "720p");
    assert!(engine
        .log
        .lock()
        .unwrap()
        .contains(&"play https://example.com/720.m3u8".to_string()));
    vm.set_volume(0.6);
    vm.toggle_mute();
    assert_eq!(vm.read().volume, 0.0);
    vm.toggle_mute();
    assert!((vm.read().volume - 0.6).abs() < 1e-6);
    vm.stop();
    let s = vm.read();
    assert_eq!(s.playback_state, VodPlayback::Idle);
    assert!(!s.has_engine());
    assert!(engine.log.lock().unwrap().contains(&"stop".to_string()));
}

#[tokio::test]
async fn vod_player_resolve_failure_sets_error() {
    let (vm, _) = player(None);
    vm.start_vod(-1).await;
    assert!(matches!(vm.read().playback_state, VodPlayback::Error(_)));
    assert_eq!(vm.read().error_message.as_deref(), Some("없음"));
}

#[test]
fn format_time_matches_swift() {
    assert_eq!(format_time(0.0), "0:00");
    assert_eq!(format_time(65.0), "1:05");
    assert_eq!(format_time(3_725.0), "1:02:05");
    assert_eq!(format_time(f64::NAN), "0:00");
}

// MARK: - VOD 브라우즈 (로컬 우선 피드)

const SCOPE_A: &str = "0B7F3C1E-7A40-4C55-9A0B-6F0E2D3C4B5A";
const SCOPE_B: &str = "5D2C8E91-1F3A-4B6D-8C7E-9A0B1C2D3E4F";

fn tag(scope: &str) -> String {
    CachedVod::subscription_tag(scope).unwrap()
}

fn now() -> Date {
    from_unix(1_790_000_000.0)
}

fn fixed_clock() -> Clock {
    Arc::new(now)
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Ok,
    Offline,
}

struct Transport {
    requests: Mutex<Vec<(String, i64)>>,
    mode: Mutex<Mode>,
    total: i64,
    hot: HashSet<String>,
    omit_channel: bool,
}

impl Transport {
    fn new(mode: Mode, total: i64, hot: &[&str]) -> Arc<Self> {
        Arc::new(Self {
            requests: Mutex::new(Vec::new()),
            mode: Mutex::new(mode),
            total,
            hot: hot.iter().map(|s| s.to_string()).collect(),
            omit_channel: false,
        })
    }
    fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

fn video_no(channel: &str, order: i64, hot: bool) -> i64 {
    if hot {
        return 5_000_000 - order;
    }
    let index: i64 = channel
        .trim_start_matches(|c: char| !c.is_ascii_digit())
        .parse()
        .unwrap_or(0);
    1_000_000 - order * 1_000 - index
}

fn items(channel: &str, count: i64, hot: bool) -> Vec<VodInfo> {
    (0..count)
        .map(|order| {
            let mut v = VodInfo::new(video_no(channel, order, hot), format!("{channel}-{order}"));
            v.channel = Some(ChannelInfo::new(channel.to_string(), channel.to_string()));
            v
        })
        .collect()
}

#[async_trait]
impl VodPageSource for Transport {
    async fn vod_page(
        &self,
        channel_id: &str,
        page: i64,
        size: i64,
    ) -> Result<Vec<VodInfo>, ApiError> {
        self.requests
            .lock()
            .unwrap()
            .push((channel_id.to_string(), page));
        tokio::task::yield_now().await;
        if *self.mode.lock().unwrap() == Mode::Offline {
            return Err(ApiError::NetworkError("offline".into()));
        }
        let hot = self.hot.contains(channel_id);
        let start = page * size;
        let end = self.total.min(start + size);
        Ok((start..end)
            .map(|order| {
                let mut v = VodInfo::new(
                    video_no(channel_id, order, hot),
                    format!("{channel_id}-{order}"),
                );
                if !self.omit_channel {
                    v.channel = Some(ChannelInfo::new(
                        channel_id.to_string(),
                        channel_id.to_string(),
                    ));
                }
                v
            })
            .collect())
    }
}

struct NoSearch;

#[async_trait]
impl VodSearchApi for NoSearch {
    async fn search_videos(
        &self,
        keyword: &str,
        offset: i64,
        _size: i64,
    ) -> Result<(Vec<VodInfo>, bool), String> {
        if keyword == "fail" {
            return Err("오류".into());
        }
        let base = offset;
        Ok((
            (base..base + 3)
                .map(|n| VodInfo::new(n, format!("{keyword}-{n}")))
                .collect(),
            offset == 0,
        ))
    }
}

#[async_trait]
impl VodChannelApi for NoSearch {
    async fn vod_list(&self, _c: &str, _p: i64, _s: i64) -> Result<PagedContent<VodInfo>, String> {
        Err("unused".into())
    }
    async fn first_channel(&self, _k: &str) -> Result<Option<(String, String)>, String> {
        Ok(None)
    }
}

fn channels(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("c{i}")).collect()
}

struct Kit {
    transport: Arc<Transport>,
    store: Option<DataStore>,
    sync: Option<VodSyncStore>,
    ids: Arc<Mutex<Vec<String>>>,
    tag: Arc<Mutex<Option<String>>>,
    details: Option<VodFollowDetails>,
}

impl Kit {
    fn new(transport: Arc<Transport>, store: Option<DataStore>, ids: Vec<String>) -> Self {
        Self {
            transport,
            store,
            sync: None,
            ids: Arc::new(Mutex::new(ids)),
            tag: Arc::new(Mutex::new(Some(tag(SCOPE_A)))),
            details: None,
        }
    }

    fn vm(&self) -> VodBrowseViewModel {
        let ids = self.ids.clone();
        let t = self.tag.clone();
        let details = self.details.clone();
        VodBrowseViewModel::new(VodBrowseDeps {
            page_source: self.transport.clone(),
            search: Arc::new(NoSearch),
            channel_api: Arc::new(NoSearch),
            data_store: self.store.clone(),
            sync_store: self.sync.clone(),
            subscription_tag: Some(Arc::new(move || t.lock().unwrap().clone())),
            follow_details: details.map(|d| {
                Arc::new(move || d.clone()) as Arc<dyn Fn() -> VodFollowDetails + Send + Sync>
            }),
            channel_ids: Arc::new(move || ids.lock().unwrap().clone()),
            progress_mirror: None,
            clock: fixed_clock(),
            sleep: Arc::new(|_| async {}.boxed()),
        })
    }
}

fn seed(store: &DataStore, tag: &str, channels: &[String]) {
    let pages: Vec<SubscriptionVodPage> = channels
        .iter()
        .map(|c| {
            SubscriptionVodPage::new(
                c.clone(),
                0,
                items(c, 8, false)
                    .iter()
                    .map(|v| cached_data(v, c, tag, now()))
                    .collect(),
            )
        })
        .collect();
    store
        .apply_subscription_vod_pages(tag, &pages, None, 24)
        .unwrap();
}

fn record(store: &VodSyncStore, channels: &[String], at: Date) {
    let results: Vec<ChannelPageResult> = channels
        .iter()
        .map(|c| {
            ChannelPageResult::success(
                c.clone(),
                items(c, 8, false),
                0,
                VodSubscriptionFeedCursor::CHANNEL_PAGE_SIZE,
            )
        })
        .collect();
    store.record(&results, at);
}

#[tokio::test]
async fn fresh_state_makes_no_requests() {
    let follows = channels(3);
    let store = DataStore::open_in_memory().unwrap();
    seed(&store, &tag(SCOPE_A), &follows);
    let sync = VodSyncStore::new(None, Some(fixed_clock()));
    sync.activate(SCOPE_A).await;
    record(&sync, &follows, adding(&now(), -120.0));
    sync.note_pass(adding(&now(), -120.0), 0);
    let mut kit = Kit::new(Transport::new(Mode::Ok, 20, &[]), Some(store), follows);
    kit.sync = Some(sync);
    let vm = kit.vm();

    vm.load_initial_if_needed().await;

    assert_eq!(kit.transport.count(), 0);
    let s = vm.read();
    assert_eq!(s.feed.len(), 24);
    assert_eq!(
        s.feed[..3]
            .iter()
            .map(|v| v.video_title.as_str())
            .collect::<Vec<_>>(),
        ["c0-0", "c1-0", "c2-0"]
    );
    assert_eq!(s.state, VodLoadState::Loaded);
    assert!(s.has_local_feed());
}

#[tokio::test]
async fn failures_keep_the_local_feed() {
    let follows = channels(5);
    let store = DataStore::open_in_memory().unwrap();
    seed(&store, &tag(SCOPE_A), &follows);
    let kit = Kit::new(
        Transport::new(Mode::Offline, 20, &[]),
        Some(store.clone()),
        follows,
    );
    let vm = kit.vm();

    vm.load_initial_if_needed().await;
    assert_eq!(kit.transport.count(), 5);
    assert_eq!(vm.read().feed.len(), 24);
    assert_eq!(vm.read().state, VodLoadState::Loaded);
    assert!(vm.read().sync_backoff_until.is_some(), "오프라인 백오프");

    vm.wait_for_pending_writes().await;
    let stored = store.fetch_subscription_vods(&tag(SCOPE_A), 24).unwrap();
    assert_eq!(stored.len(), 5);
    assert!(stored.values().all(|v| v.len() == 8));

    // 백오프 중 새로고침은 네트워크를 쓰지 않는다.
    vm.reload().await;
    assert_eq!(kit.transport.count(), 5);
    assert_eq!(vm.read().feed.len(), 24);
}

#[tokio::test]
async fn cold_entry_stays_within_budget() {
    let follows = channels(320);
    let store = DataStore::open_in_memory().unwrap();
    let kit = Kit::new(
        Transport::new(Mode::Ok, 20, &["c0"]),
        Some(store.clone()),
        follows.clone(),
    );
    let vm = kit.vm();

    vm.load_initial_if_needed().await;

    let requests = kit.transport.requests.lock().unwrap().clone();
    assert!(requests.len() <= 80, "진입 창 요청 {}건", requests.len());
    let page_zero: HashSet<String> = requests
        .iter()
        .filter(|r| r.1 == 0)
        .map(|r| r.0.clone())
        .collect();
    assert_eq!(page_zero.len(), 77);
    assert_eq!(page_zero, follows[..77].iter().cloned().collect());
    assert!(requests.iter().filter(|r| r.1 > 0).all(|r| r.0 == "c0"));
    assert_eq!(vm.read().feed.len(), 24);
    assert_eq!(vm.read().state, VodLoadState::Loaded);
    vm.wait_for_pending_writes().await;
    assert_eq!(
        store
            .fetch_subscription_vods(&tag(SCOPE_A), 24)
            .unwrap()
            .len(),
        77
    );
}

#[tokio::test]
async fn dominating_channel_fetches_only_its_next_page() {
    let follows: Vec<String> = ["c1", "hot", "c2"].iter().map(|s| s.to_string()).collect();
    let kit = Kit::new(
        Transport::new(Mode::Ok, 40, &["hot"]),
        None,
        follows.clone(),
    );
    let vm = kit.vm();
    vm.load_initial_if_needed().await;
    let requests = kit.transport.requests.lock().unwrap().clone();
    let zero: HashSet<String> = requests
        .iter()
        .filter(|r| r.1 == 0)
        .map(|r| r.0.clone())
        .collect();
    assert_eq!(zero, follows.iter().cloned().collect());
    let later: Vec<_> = requests.iter().filter(|r| r.1 > 0).collect();
    assert!(!later.is_empty());
    assert!(later.iter().all(|r| r.0 == "hot"));
    assert_eq!(
        later.iter().map(|r| r.1).collect::<Vec<_>>(),
        (1..=later.len() as i64).collect::<Vec<_>>()
    );
    assert!(vm.read().feed[..16]
        .iter()
        .all(|v| v.video_title.starts_with("hot-")));
}

#[tokio::test]
async fn load_more_adds_received_pages_only() {
    let follows: Vec<String> = ["hot", "c1"].iter().map(|s| s.to_string()).collect();
    let store = DataStore::open_in_memory().unwrap();
    let kit = Kit::new(
        Transport::new(Mode::Ok, 40, &["hot"]),
        Some(store.clone()),
        follows,
    );
    let vm = kit.vm();
    vm.reload().await;
    vm.load_more().await;
    vm.wait_for_pending_writes().await;
    let stored = store.fetch_subscription_vods(&tag(SCOPE_A), 24).unwrap();
    let hot = &stored["hot"];
    assert_eq!(hot.len(), 24);
    assert_eq!(hot[0].video_title, "hot-0");
    assert_eq!(stored["c1"].len(), 8);
}

#[tokio::test]
async fn unfollowed_channels_pruned_only_with_authoritative_list() {
    let store = DataStore::open_in_memory().unwrap();
    let all: Vec<String> = ["c1", "c2", "c9"].iter().map(|s| s.to_string()).collect();
    seed(&store, &tag(SCOPE_A), &all);
    let sync = VodSyncStore::new(None, Some(fixed_clock()));
    sync.activate(SCOPE_A).await;
    record(&sync, &all, adding(&now(), -86_400.0 * 2.0));
    let ids = vec!["c1".to_string(), "c2".to_string()];

    let mut kit = Kit::new(
        Transport::new(Mode::Ok, 20, &[]),
        Some(store.clone()),
        ids.clone(),
    );
    kit.sync = Some(sync.clone());
    kit.details = Some(VodFollowDetails::with_profiles(
        HashSet::new(),
        HashMap::new(),
        false,
    ));
    let guarded = kit.vm();
    guarded.reload().await;
    guarded.wait_for_pending_writes().await;
    assert!(store
        .fetch_subscription_vods(&tag(SCOPE_A), 24)
        .unwrap()
        .contains_key("c9"));
    assert!(sync.state().get("c9").is_some());

    kit.details = Some(VodFollowDetails::with_profiles(
        HashSet::new(),
        HashMap::new(),
        true,
    ));
    let authoritative = kit.vm();
    authoritative.reload().await;
    authoritative.wait_for_pending_writes().await;
    let stored = store.fetch_subscription_vods(&tag(SCOPE_A), 24).unwrap();
    assert!(!stored.contains_key("c9"));
    assert!(sync.state().get("c9").is_none());
    assert!(!authoritative
        .read()
        .feed
        .iter()
        .any(|v| v.channel.as_ref().is_some_and(|c| c.channel_id == "c9")));
}

#[tokio::test]
async fn missing_channel_is_filled_from_follow_list() {
    let store = DataStore::open_in_memory().unwrap();
    let transport = Arc::new(Transport {
        requests: Mutex::new(Vec::new()),
        mode: Mutex::new(Mode::Ok),
        total: 20,
        hot: HashSet::new(),
        omit_channel: true,
    });
    let mut kit = Kit::new(transport, Some(store.clone()), vec!["c1".into()]);
    let profile = ChannelInfo::new("c1".to_string(), "채널 하나".to_string());
    kit.details = Some(VodFollowDetails::with_profiles(
        HashSet::new(),
        HashMap::from([("c1".to_string(), profile)]),
        false,
    ));
    let vm = kit.vm();
    vm.load_initial_if_needed().await;
    vm.wait_for_pending_writes().await;
    assert!(vm.read().feed.iter().all(|v| v
        .channel
        .as_ref()
        .is_some_and(|c| c.channel_name == "채널 하나")));
    let stored = store.fetch_subscription_vods(&tag(SCOPE_A), 24).unwrap();
    assert_eq!(stored["c1"].len(), 20);
    assert_eq!(stored["c1"][0].channel_name.as_deref(), Some("채널 하나"));
}

#[tokio::test]
async fn account_switch_invalidates_and_reads_new_account_copy() {
    let store = DataStore::open_in_memory().unwrap();
    let a = vec!["c1".to_string()];
    let b = vec!["c2".to_string()];
    seed(&store, &tag(SCOPE_A), &a);
    seed(&store, &tag(SCOPE_B), &b);
    let kit = Kit::new(Transport::new(Mode::Offline, 20, &[]), Some(store), a);
    let vm = kit.vm();
    vm.load_initial_if_needed().await;
    assert!(vm
        .read()
        .feed
        .iter()
        .all(|v| v.video_title.starts_with("c1-")));

    *kit.tag.lock().unwrap() = Some(tag(SCOPE_B));
    *kit.ids.lock().unwrap() = b;
    vm.invalidate();
    assert!(vm.read().feed.is_empty());
    assert_eq!(vm.read().state, VodLoadState::Idle);
    vm.load_initial_if_needed().await;
    let s = vm.read();
    assert!(!s.feed.is_empty());
    assert!(
        s.feed.iter().all(|v| v.video_title.starts_with("c2-")),
        "다른 계정 행이 섞였다"
    );
}

#[tokio::test]
async fn watch_later_toggle_and_library() {
    let store = DataStore::open_in_memory().unwrap();
    let kit = Kit::new(
        Transport::new(Mode::Ok, 20, &[]),
        Some(store.clone()),
        vec![],
    );
    let vm = kit.vm();
    let vod = items("c1", 1, false).remove(0);
    vm.toggle_watch_later(&vod).await;
    assert!(vm.is_in_watch_later(vod.video_no));
    assert_eq!(
        store.fetch_vod_watch_later_ids().unwrap(),
        vec![vod.video_no]
    );
    vm.select_library_segment(cview_app_core::viewmodels::vod_browse::LibrarySegment::WatchLater)
        .await;
    assert_eq!(vm.read().watch_later.len(), 1);
    vm.toggle_watch_later(&vod).await;
    assert!(!vm.is_in_watch_later(vod.video_no));
    assert_eq!(vm.read().watch_later_state, VodLoadState::Empty);

    store
        .record_vod_playback(5, 100.0, 1_000.0, "t", "c", None)
        .unwrap();
    vm.load_continue_watching().await;
    assert_eq!(vm.read().continue_watching.len(), 1);
    vm.remove_from_continue_watching(5).await;
    assert_eq!(vm.read().library_state, VodLoadState::Empty);
}

#[tokio::test]
async fn search_debounces_and_pages() {
    let kit = Kit::new(Transport::new(Mode::Ok, 20, &[]), None, vec![]);
    let vm = kit.vm();
    vm.update_search_query("a");
    assert_eq!(vm.read().search_state, VodLoadState::Idle);
    vm.update_search_query("ab");
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(vm.read().search_results.len(), 3);
    assert!(vm.read().search_has_more);
    vm.load_more_search().await;
    assert_eq!(vm.read().search_results.len(), 6);
    assert!(!vm.read().search_has_more);
    vm.update_search_query("fail");
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(matches!(vm.read().search_state, VodLoadState::Error(_)));
}
