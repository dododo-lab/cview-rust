//! `Sources/CViewApp/Services/VODSyncStore.swift` 포팅 — 팔로우 VOD 증분 동기화의 상태 보관과 진입 예산.
//!
//! - [`VodSyncStore`] — 계정(scope)별 채널 동기화 기록([`VodSyncState`])을 들고 `vod-sync.‹scope›`
//!   스냅숏으로 남긴다. 실패 시각·마지막으로 본 라이브 집합은 메모리에만 둔다.
//! - [`VodEntryWindow`] — 진입 5초 창의 요청 예산(max(24, ⌈팔로우 × 25%⌉)).
//! - [`VodFollowDetails`] — 팔로우 목록에서 동기화가 쓰는 부분.
//!
//! 원본은 `MenuSnapshotStore`(최소 간격 60초·지문 비교·30일 만료)에 저장한다. 그 저장소는 Rust 로
//! 옮겨지지 않아 저장은 [`VodSyncSnapshots`] trait 으로 뺐다. [`FileVodSyncSnapshots`] 는 같은 상태를
//! 같은 바이트로 쓰는(내용이 같으면 쓰지 않는) 단순 파일 구현이다.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::DateTime;
use cview_core::models::channel::ChannelInfo;
use cview_core::models::live_channel_item::LiveChannelItem;
use cview_core::utils::time::{Date, TimeInterval};
use cview_persistence::models::cached_vod::CachedVod;
use futures_util::future::{BoxFuture, FutureExt, Shared};

use super::support::{lock, system_clock, write_atomic, Clock};
use super::vod_feed::{ChannelPageResult, VodSubscriptionFeedCursor};
use super::vod_sync_state::{VodSubscriptionTag, VodSyncPolicy, VodSyncState};

/// 스냅숏 원천 이름.
pub const SNAPSHOT_SOURCE: &str = "vod-sync";

/// 계정 범위 스냅숏 저장소.
#[async_trait]
pub trait VodSyncSnapshots: Send + Sync {
    /// 없거나 만료·거부면 `None`.
    async fn load(&self, scope: &str) -> Option<VodSyncState>;
    async fn save(&self, scope: &str, state: &VodSyncState, source_fetched_at: Date);
}

/// `<root>/vod-sync.<scope>.json` 파일 저장소. 내용이 같으면 다시 쓰지 않는다.
pub struct FileVodSyncSnapshots {
    root: PathBuf,
}

impl FileVodSyncSnapshots {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn path(&self, scope: &str) -> PathBuf {
        self.root.join(format!("{SNAPSHOT_SOURCE}.{scope}.json"))
    }
}

#[async_trait]
impl VodSyncSnapshots for FileVodSyncSnapshots {
    async fn load(&self, scope: &str) -> Option<VodSyncState> {
        let bytes = tokio::fs::read(self.path(scope)).await.ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    async fn save(&self, scope: &str, state: &VodSyncState, _source_fetched_at: Date) {
        let Ok(bytes) = serde_json::to_vec(state) else {
            return;
        };
        let path = self.path(scope);
        if tokio::fs::read(&path).await.ok().as_deref() == Some(bytes.as_slice()) {
            return;
        }
        if let Err(e) = write_atomic(&path, &bytes) {
            tracing::warn!("vod-sync 저장 실패: {e}");
        }
    }
}

type Flight = Shared<BoxFuture<'static, ()>>;

#[derive(Default)]
struct State {
    state: VodSyncState,
    scope: Option<String>,
    generation: u64,
    failed_at: HashMap<String, Date>,
    last_observed_online: Option<HashSet<String>>,
    activation: Option<(String, Flight)>,
    save_task: Option<Flight>,
    is_save_scheduled: bool,
}

/// 계정별 VOD 동기화 기록. `Clone` 은 같은 저장소를 가리킨다.
#[derive(Clone)]
pub struct VodSyncStore {
    inner: Arc<Mutex<State>>,
    snapshots: Option<Arc<dyn VodSyncSnapshots>>,
    now: Clock,
}

impl VodSyncStore {
    /// `snapshots` 가 `None` 이면 메모리 전용이다.
    pub fn new(snapshots: Option<Arc<dyn VodSyncSnapshots>>, now: Option<Clock>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(State::default())),
            snapshots,
            now: now.unwrap_or_else(system_clock),
        }
    }

    pub fn is_persistent(&self) -> bool {
        self.snapshots.is_some()
    }
    pub fn state(&self) -> VodSyncState {
        lock(&self.inner).state.clone()
    }
    pub fn scope(&self) -> Option<String> {
        lock(&self.inner).scope.clone()
    }
    pub fn generation(&self) -> u64 {
        lock(&self.inner).generation
    }
    pub fn failed_at(&self) -> HashMap<String, Date> {
        lock(&self.inner).failed_at.clone()
    }
    pub fn last_observed_online(&self) -> Option<HashSet<String>> {
        lock(&self.inner).last_observed_online.clone()
    }

    // MARK: 계정

    /// 계정 범위를 연다. 같은 범위면 아무것도 하지 않고(읽는 중이면 기다린다), 다르면 기록을 비우고 읽는다.
    pub async fn activate(&self, new_scope: &str) {
        let joined = {
            let st = lock(&self.inner);
            st.activation
                .as_ref()
                .filter(|(scope, _)| scope == new_scope)
                .map(|(_, f)| f.clone())
        };
        if let Some(f) = joined {
            f.await;
            return;
        }
        let flight = {
            let mut st = lock(&self.inner);
            if st.scope.as_deref() == Some(new_scope) {
                return;
            }
            Self::clear(&mut st);
            st.scope = Some(new_scope.to_string());
            let Some(snapshots) = self.snapshots.clone() else {
                return;
            };
            let expected = st.generation;
            let inner = self.inner.clone();
            let scope = new_scope.to_string();
            let flight: Flight = async move {
                let loaded = snapshots.load(&scope).await;
                let mut st = lock(&inner);
                if st.generation == expected {
                    if let Some(loaded) = loaded {
                        st.state = loaded;
                    }
                }
            }
            .boxed()
            .shared();
            st.activation = Some((new_scope.to_string(), flight.clone()));
            (flight, expected)
        };
        let (flight, expected) = flight;
        flight.await;
        let mut st = lock(&self.inner);
        if st.generation == expected {
            st.activation = None;
        }
    }

    /// 인증 경계. 기록·실패·라이브 관찰을 비우고, 아직 쓰지 않은 저장을 버린다.
    pub fn reset(&self) {
        let mut st = lock(&self.inner);
        Self::clear(&mut st);
        st.scope = None;
    }

    fn clear(st: &mut State) {
        st.generation = st.generation.wrapping_add(1);
        st.state = VodSyncState::default();
        st.failed_at.clear();
        st.last_observed_online = None;
        st.activation = None;
        st.is_save_scheduled = false;
    }

    // MARK: 기록

    /// 받은 page 0 결과를 적는다. 실패는 메모리의 실패 시각만 남긴다. page ≥ 1 은 적지 않는다.
    pub fn record(&self, results: &[ChannelPageResult], date: Date) {
        let mut changed = false;
        {
            let mut st = lock(&self.inner);
            for result in results.iter().filter(|r| r.page == 0) {
                if result.failed {
                    st.failed_at.insert(result.channel_id.clone(), date);
                    continue;
                }
                st.failed_at.remove(&result.channel_id);
                let newest = result.items.iter().reduce(|a, b| {
                    if VodSubscriptionFeedCursor::is_newer(a, b) {
                        a
                    } else {
                        b
                    }
                });
                st.state.update(&result.channel_id, |channel| {
                    channel.last_checked_at = Some(date);
                    if let Some(newest) = newest {
                        channel.latest_video_no = Some(newest.video_no);
                        channel.latest_publish_date = newest.publish_date;
                        channel.empty_streak = 0;
                    } else {
                        channel.empty_streak += 1;
                    }
                });
                changed = true;
            }
        }
        if changed {
            self.schedule_save();
        }
    }

    /// 패스가 끝났다. `due_remaining` 이 0 이면 전체 동기화 시각도 적는다.
    pub fn note_pass(&self, date: Date, due_remaining: usize) {
        {
            let mut st = lock(&self.inner);
            st.state.last_pass_at = Some(date);
            if due_remaining == 0 {
                st.state.last_full_sync_at = Some(date);
            }
        }
        self.schedule_save();
    }

    /// 언팔로우한 채널의 기록을 뺀다. 팔로우 목록이 권위 있을 때만 부른다.
    pub fn retain(&self, followed: &HashSet<String>) {
        let removed = {
            let mut st = lock(&self.inner);
            st.failed_at.retain(|k, _| followed.contains(k));
            if let Some(online) = st.last_observed_online.as_mut() {
                online.retain(|k| followed.contains(k));
            }
            st.state.retain(followed)
        };
        if removed > 0 {
            self.schedule_save();
        }
    }

    /// 방송 종료를 적는다. 마지막 확인 뒤의 종료가 이미 있으면 더 이른 쪽을 둔다.
    pub fn note_live_ended(&self, channel_ids: &HashSet<String>, date: Date) {
        if channel_ids.is_empty() {
            return;
        }
        {
            let mut st = lock(&self.inner);
            for channel_id in channel_ids {
                st.state.update(channel_id, |channel| {
                    let checked = channel
                        .last_checked_at
                        .unwrap_or(DateTime::<chrono::Utc>::MIN_UTC);
                    match channel.last_live_ended_at {
                        Some(ended) if ended > checked => {
                            channel.last_live_ended_at = Some(ended.min(date));
                        }
                        _ => channel.last_live_ended_at = Some(date),
                    }
                });
            }
        }
        self.schedule_save();
    }

    /// 지금 라이브인 팔로우 채널 집합. 직전 관찰에 있다가 빠진 채널을 방송 종료로 적는다.
    pub fn observe_live_set(&self, online: &HashSet<String>, date: Date) {
        let previous = lock(&self.inner)
            .last_observed_online
            .replace(online.clone());
        if let Some(previous) = previous {
            let ended: HashSet<String> = previous.difference(online).cloned().collect();
            self.note_live_ended(&ended, date);
        }
    }

    // MARK: 저장

    fn schedule_save(&self) {
        let Some(snapshots) = self.snapshots.clone() else {
            return;
        };
        let mut st = lock(&self.inner);
        if st.scope.is_none() || st.is_save_scheduled {
            return;
        }
        st.is_save_scheduled = true;
        let expected = st.generation;
        let previous = st.save_task.take();
        let inner = self.inner.clone();
        let now = self.now.clone();
        let task = tokio::spawn(async move {
            if let Some(p) = previous {
                p.await;
            }
            let (scope, payload) = {
                let mut st = lock(&inner);
                if st.generation != expected || !st.is_save_scheduled {
                    return;
                }
                let Some(scope) = st.scope.clone() else {
                    return;
                };
                st.is_save_scheduled = false;
                (scope, st.state.clone())
            };
            let clock = now();
            let fetched_at = payload.last_pass_at.map_or(clock, |p| p.min(clock));
            snapshots.save(&scope, &payload, fetched_at).await;
        });
        st.save_task = Some(
            async move {
                let _ = task.await;
            }
            .boxed()
            .shared(),
        );
    }

    /// 예약한 저장이 끝날 때까지 기다린다(테스트·종료 경로).
    pub async fn wait_for_pending_save(&self) {
        let task = lock(&self.inner).save_task.clone();
        if let Some(t) = task {
            t.await;
        }
    }
}

// MARK: - VodEntryWindow

/// 메뉴 진입 5초 창의 요청 예산.
#[derive(Debug, Clone, Default)]
pub struct VodEntryWindow {
    budget: usize,
    remaining: usize,
    ends_at: Option<Date>,
}

impl VodEntryWindow {
    /// 진입 창 길이(초).
    pub const LENGTH: TimeInterval = 5.0;
    /// 동기화 패스가 첫 화면 drain 에 남겨 두는 요청 수.
    pub const DRAIN_RESERVE: usize = 3;

    pub fn new() -> Self {
        Self::default()
    }
    pub fn budget(&self) -> usize {
        self.budget
    }
    pub fn remaining(&self) -> usize {
        self.remaining
    }
    pub fn ends_at(&self) -> Option<Date> {
        self.ends_at
    }

    /// 창을 새로 연다 — 예산 max(24, ⌈팔로우 × 25%⌉), 길이 5초.
    pub fn begin(&mut self, follow_count: i64, now: Date) {
        self.budget = VodSyncPolicy::entry_budget(follow_count);
        self.remaining = self.budget;
        self.ends_at = Some(now + chrono::Duration::milliseconds((Self::LENGTH * 1000.0) as i64));
    }

    pub fn is_open(&self, now: Date) -> bool {
        self.ends_at.is_some_and(|end| now < end)
    }

    pub fn open_remaining(&self, now: Date) -> Option<usize> {
        self.is_open(now).then_some(self.remaining)
    }

    /// 요청 `count` 건을 꺼낸다. 창 안이면 남은 예산만큼, 창 밖이면 그대로.
    pub fn take(&mut self, count: usize, now: Date) -> usize {
        if count == 0 {
            return 0;
        }
        if !self.is_open(now) {
            return count;
        }
        let granted = count.min(self.remaining);
        self.remaining -= granted;
        granted
    }
}

// MARK: - VodFollowDetails

/// 팔로우 목록에서 동기화가 쓰는 부분.
#[derive(Debug, Clone, Default)]
pub struct VodFollowDetails {
    /// 지금 라이브 중인 채널.
    pub live: HashSet<String>,
    /// 목록이 서버에서 받은 완전한 목록인가. 그럴 때만 언팔로우를 정리한다.
    pub is_authoritative: bool,
    profiles: HashMap<String, ChannelInfo>,
    followed: HashMap<String, LiveChannelItem>,
}

impl VodFollowDetails {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn with_profiles(
        live: HashSet<String>,
        profiles: HashMap<String, ChannelInfo>,
        is_authoritative: bool,
    ) -> Self {
        Self {
            live,
            is_authoritative,
            profiles,
            followed: HashMap::new(),
        }
    }

    pub fn from_channels(channels: &[LiveChannelItem], is_authoritative: bool) -> Self {
        let mut followed = HashMap::new();
        for c in channels {
            followed
                .entry(c.channel_id.clone())
                .or_insert_with(|| c.clone());
        }
        Self {
            live: channels
                .iter()
                .filter(|c| c.is_live)
                .map(|c| c.channel_id.clone())
                .collect(),
            is_authoritative,
            profiles: HashMap::new(),
            followed,
        }
    }

    /// vodList 응답에 채널 정보가 없을 때 채울 이름·아바타.
    pub fn profile(&self, channel_id: &str) -> Option<ChannelInfo> {
        if let Some(p) = self.profiles.get(channel_id) {
            return Some(p.clone());
        }
        let item = self.followed.get(channel_id)?;
        let mut info = ChannelInfo::new(item.channel_id.clone(), item.channel_name.clone());
        info.channel_image_url = item
            .channel_image_url
            .as_deref()
            .and_then(|s| url::Url::parse(s).ok());
        Some(info)
    }
}

impl VodSubscriptionTag {
    /// 계정 토큰에서 scope 를 꺼낸다. 계정 토큰이 아니면 `None`.
    pub fn scope_of_tag(tag: &str) -> Option<String> {
        let prefix = CachedVod::SUBSCRIPTION_TAG_PREFIX;
        let rest = tag.strip_prefix(prefix)?;
        if rest.is_empty() || tag.contains(',') {
            return None;
        }
        Some(rest.to_string())
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/VODSyncStoreTests.swift`, `VODSyncEntryBudgetTests.swift` 일부.
    use super::*;
    use chrono::Duration;
    use cview_core::models::search::VodInfo;
    use cview_core::utils::time::from_unix;

    const SCOPE_A: &str = "AAAAAAAA-0000-0000-0000-000000000001";
    const SCOPE_B: &str = "BBBBBBBB-0000-0000-0000-000000000002";

    fn now() -> Date {
        from_unix(1_800_000_000.0)
    }

    fn items(channel: &str, published: Date) -> Vec<VodInfo> {
        (0..3)
            .map(|i| {
                let mut v = VodInfo::new(
                    channel.len() as i64 * 100 + 10 - i,
                    format!("{channel}-{i}"),
                );
                v.publish_date = Some(published - Duration::hours(i));
                v
            })
            .collect()
    }

    fn record(store: &VodSyncStore, channels: &[&str], at: Date, published: Date) {
        let results: Vec<_> = channels
            .iter()
            .map(|c| ChannelPageResult::success(*c, items(c, published), 0, 8))
            .collect();
        store.record(&results, at);
    }

    fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!("vod-sync-{}", uuid::Uuid::new_v4()))
    }

    fn file_store(root: &std::path::Path) -> VodSyncStore {
        let clock: Clock = Arc::new(now);
        VodSyncStore::new(Some(Arc::new(FileVodSyncSnapshots::new(root))), Some(clock))
    }

    #[tokio::test]
    async fn round_trip() {
        let root = temp_root();
        let store = file_store(&root);
        store.activate(SCOPE_A).await;
        let published = now() - Duration::hours(1);
        record(
            &store,
            &["c1", "c0"],
            now() - Duration::seconds(60),
            published,
        );
        store.note_pass(now() - Duration::seconds(30), 0);
        store.wait_for_pending_save().await;
        assert!(FileVodSyncSnapshots::new(root.clone())
            .path(SCOPE_A)
            .exists());

        let reopened = file_store(&root);
        reopened.activate(SCOPE_A).await;
        assert_eq!(reopened.state(), store.state());
        let ids: Vec<_> = reopened
            .state()
            .channels()
            .iter()
            .map(|c| c.channel_id.clone())
            .collect();
        assert_eq!(ids, ["c0", "c1"]);
        let c0 = reopened.state().get("c0").cloned().unwrap();
        assert_eq!(c0.latest_video_no, Some(items("c0", published)[0].video_no));
        assert_eq!(c0.latest_publish_date, Some(published));
        assert_eq!(
            reopened.state().last_full_sync_at,
            Some(now() - Duration::seconds(30))
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn scopes_are_isolated_and_reset_drops_pending_save() {
        let root = temp_root();
        let store = file_store(&root);
        store.activate(SCOPE_A).await;
        record(&store, &["a1"], now(), now());
        store.wait_for_pending_save().await;
        store.activate(SCOPE_B).await;
        assert!(store.state().channels().is_empty());
        store.activate(SCOPE_A).await;
        assert_eq!(store.state().channels()[0].channel_id, "a1");

        let root2 = temp_root();
        let store = file_store(&root2);
        store.activate(SCOPE_A).await;
        record(&store, &["c0"], now(), now());
        store.reset();
        store.wait_for_pending_save().await;
        assert!(!FileVodSyncSnapshots::new(root2.clone())
            .path(SCOPE_A)
            .exists());
        assert!(store.scope().is_none() && store.state().channels().is_empty());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&root2);
    }

    #[tokio::test]
    async fn failure_and_retain() {
        let store = VodSyncStore::new(None, None);
        store.activate(SCOPE_A).await;
        record(
            &store,
            &["a", "b", "c0"],
            now() - Duration::seconds(600),
            now(),
        );
        store.record(&[ChannelPageResult::failure("c0", 0, 8, None)], now());
        let c0 = store.state().get("c0").cloned().unwrap();
        assert_eq!(c0.last_checked_at, Some(now() - Duration::seconds(600)));
        assert_eq!(store.failed_at().get("c0"), Some(&now()));
        store.record(&[ChannelPageResult::success("c0", vec![], 0, 8)], now());
        assert_eq!(store.state().get("c0").unwrap().empty_streak, 1);
        assert!(!store.failed_at().contains_key("c0"));

        store.record(&[ChannelPageResult::failure("d", 0, 8, None)], now());
        store.retain(&HashSet::from(["a".to_string(), "b".to_string()]));
        assert_eq!(store.state().channels().len(), 2);
        assert!(!store.failed_at().contains_key("d"));
    }

    #[tokio::test]
    async fn live_end_signals() {
        let store = VodSyncStore::new(None, None);
        store.activate(SCOPE_A).await;
        let t1 = now() - Duration::seconds(300);
        store.observe_live_set(&HashSet::from(["x".to_string()]), t1);
        assert!(
            store.state().get("x").is_none(),
            "첫 관찰은 종료를 만들지 않는다"
        );
        store.observe_live_set(&HashSet::new(), t1);
        assert_eq!(store.state().get("x").unwrap().last_live_ended_at, Some(t1));
        store.note_live_ended(&HashSet::from(["x".to_string()]), now());
        assert_eq!(
            store.state().get("x").unwrap().last_live_ended_at,
            Some(t1),
            "늦은 신호가 더 이른 시각을 덮었다"
        );
    }

    #[test]
    fn entry_window_budget() {
        let mut w = VodEntryWindow::new();
        assert_eq!(w.take(5, now()), 5, "창 밖은 제한하지 않는다");
        w.begin(320, now());
        assert_eq!(w.budget(), 80);
        assert_eq!(w.take(70, now()), 70);
        assert_eq!(w.take(70, now()), 10);
        assert_eq!(w.open_remaining(now()), Some(0));
        assert_eq!(w.take(3, now() + Duration::seconds(5)), 3);
        w.begin(10, now());
        assert_eq!(w.budget(), 24);
    }

    #[test]
    fn scope_of_tag_and_profiles() {
        assert_eq!(
            VodSubscriptionTag::scope_of_tag("subscriptions.ABC").as_deref(),
            Some("ABC")
        );
        assert_eq!(VodSubscriptionTag::scope_of_tag("subscriptions."), None);
        assert_eq!(VodSubscriptionTag::scope_of_tag("subscriptions.A,B"), None);
        assert_eq!(VodSubscriptionTag::scope_of_tag("other"), None);
        let details = VodFollowDetails::empty();
        assert!(details.profile("x").is_none());
    }
}
