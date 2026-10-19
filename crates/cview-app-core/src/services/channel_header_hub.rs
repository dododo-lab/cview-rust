//! `Sources/CViewApp/Services/ChannelHeaderHub.swift` 포팅 — 채널 헤더(시청자·누적·방송 시작, 주간 후원
//! 랭킹) 조회를 채널마다 한 벌만 돌린다.
//!
//! - 구독하면 10분 안의 값을 바로 보낸다(복귀 직후 빈칸 없음).
//! - 값이 신선 기한보다 오래됐을 때만 즉시 조회하고, 진행 중 요청이 있으면 그 결과를 함께 받는다.
//! - 채널 하나에 루프 하나. 구독자가 0명이 되면 루프를 끄고 값은 남긴다.
//! - ⚠ 랭킹은 `withMyRank` 라 사용자별 값이다. 계정 경계에서 `invalidate_auth_scoped()` 를 부른다.
//!
//! 원본의 `PowerAwareInterval`(배터리 1.5배)은 플랫폼 전원 감시가 없어 `period` 주입으로 남겼고(기본 그대로),
//! `RequestOrigin.chatPoll` 태그는 Rust 네트워킹에 요청 장부가 없어 생략했다.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use async_trait::async_trait;
use cview_core::error::ApiError;
use cview_core::models::chat::messages::DonationRankResponse;
use cview_core::models::live::LiveInfo;
use cview_core::utils::time::TimeInterval;
use cview_networking::ChzzkApiClient;
use futures_util::future::{BoxFuture, FutureExt, Shared};
use tokio::sync::watch;
use tokio::task::AbortHandle;
use uuid::Uuid;

use super::refresh_coordinator::{system_sleep, Sleep};
use super::support::{lock, system_clock, Clock};
use super::watch_heartbeat::WatchHeartbeatCenter;

/// 구독 즉시 보내는 캐시값의 최대 나이(초).
pub const REPLAY_MAX_AGE: TimeInterval = 600.0;
/// 하트비트 최근값 재사용 상한(초).
pub const HEARTBEAT_REUSE_MAX_AGE: TimeInterval = 15.0;

/// 헤더 조회원.
#[async_trait]
pub trait ChannelHeaderSource: Send + Sync {
    async fn live_status(&self, channel_id: &str) -> Result<LiveInfo, ApiError>;
    async fn weekly_donation_ranking(
        &self,
        channel_id: &str,
        rank_count: i64,
    ) -> Result<DonationRankResponse, ApiError>;
}

#[async_trait]
impl ChannelHeaderSource for ChzzkApiClient {
    async fn live_status(&self, channel_id: &str) -> Result<LiveInfo, ApiError> {
        ChzzkApiClient::live_status(self, channel_id).await
    }
    async fn weekly_donation_ranking(
        &self,
        channel_id: &str,
        rank_count: i64,
    ) -> Result<DonationRankResponse, ApiError> {
        ChzzkApiClient::weekly_donation_ranking(self, channel_id, rank_count, true).await
    }
}

type Fetch<V> = Arc<dyn Fn() -> BoxFuture<'static, Result<V, ApiError>> + Send + Sync>;
type FlightFuture<V> = Shared<BoxFuture<'static, Result<V, ApiError>>>;
/// (channelId, maxAge) → 재생 하트비트가 이미 받은 최근 상태.
pub type HeartbeatStatus = Arc<dyn Fn(&str, TimeInterval) -> Option<LiveInfo> + Send + Sync>;
/// 간격 → 실제 주기.
pub type Period = Arc<dyn Fn(TimeInterval) -> TimeInterval + Send + Sync>;

// MARK: - 구독

/// 헤더 값 구독. 버리면 구독이 풀린다. 최신 값 하나만 버퍼한다.
pub struct HeaderSubscription<V: Clone + Send + Sync + 'static> {
    rx: watch::Receiver<Option<V>>,
    feed: Weak<FeedInner<V>>,
    key: String,
    id: Uuid,
}

impl<V: Clone + Send + Sync + 'static> HeaderSubscription<V> {
    /// 다음 값. 피드가 사라지면 `None`.
    pub async fn next(&mut self) -> Option<V> {
        loop {
            self.rx.changed().await.ok()?;
            if let Some(v) = self.rx.borrow_and_update().clone() {
                return Some(v);
            }
        }
    }

    /// 지금 버퍼에 새 값이 있으면 꺼낸다(기다리지 않는다).
    pub fn try_next(&mut self) -> Option<V> {
        if self.rx.has_changed().ok()? {
            return self.rx.borrow_and_update().clone();
        }
        None
    }
}

impl<V: Clone + Send + Sync + 'static> Drop for HeaderSubscription<V> {
    fn drop(&mut self) {
        if let Some(inner) = self.feed.upgrade() {
            Feed { inner }.unsubscribe(&self.key, self.id);
        }
    }
}

// MARK: - 피드

struct Entry<V> {
    value: Option<V>,
    fetched_at: Option<cview_core::utils::time::Date>,
    attempted_at: Option<cview_core::utils::time::Date>,
}

impl<V> Default for Entry<V> {
    fn default() -> Self {
        Self {
            value: None,
            fetched_at: None,
            attempted_at: None,
        }
    }
}

struct Subscriber<V> {
    tx: watch::Sender<Option<V>>,
    interval: TimeInterval,
}

struct FeedState<V> {
    entries: HashMap<String, Entry<V>>,
    fetchers: HashMap<String, Fetch<V>>,
    flights: HashMap<String, (u64, FlightFuture<V>)>,
    subscribers: HashMap<String, HashMap<Uuid, Subscriber<V>>>,
    loops: HashMap<String, AbortHandle>,
    next_flight_id: u64,
    epoch: u64,
}

struct FeedInner<V> {
    state: Mutex<FeedState<V>>,
    now: Clock,
    sleep: Sleep,
    period: Period,
}

/// 한 종류의 헤더 값을 키(채널)마다 한 벌씩 관리한다.
struct Feed<V: Clone + Send + Sync + 'static> {
    inner: Arc<FeedInner<V>>,
}

impl<V: Clone + Send + Sync + 'static> Clone for Feed<V> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

fn secs_between(a: cview_core::utils::time::Date, b: cview_core::utils::time::Date) -> f64 {
    (b - a).num_milliseconds() as f64 / 1000.0
}

impl<V: Clone + Send + Sync + 'static> Feed<V> {
    fn new(now: Clock, sleep: Sleep, period: Period) -> Self {
        Self {
            inner: Arc::new(FeedInner {
                state: Mutex::new(FeedState {
                    entries: HashMap::new(),
                    fetchers: HashMap::new(),
                    flights: HashMap::new(),
                    subscribers: HashMap::new(),
                    loops: HashMap::new(),
                    next_flight_id: 0,
                    epoch: 0,
                }),
                now,
                sleep,
                period,
            }),
        }
    }

    fn loop_count(&self) -> usize {
        lock(&self.inner.state).loops.len()
    }
    fn subscriber_count(&self, key: &str) -> usize {
        lock(&self.inner.state)
            .subscribers
            .get(key)
            .map_or(0, HashMap::len)
    }
    fn has_flight(&self, key: &str) -> bool {
        lock(&self.inner.state).flights.contains_key(key)
    }

    fn subscribe(
        &self,
        key: &str,
        interval: TimeInterval,
        fresh: TimeInterval,
        fetch: Fetch<V>,
    ) -> HeaderSubscription<V> {
        let id = Uuid::new_v4();
        let (tx, rx) = watch::channel(None);
        let mut st = lock(&self.inner.state);
        self.prune_expired(&mut st);
        st.fetchers.insert(key.to_string(), fetch.clone());
        let age = self.value_age(&st, key);
        if let (Some(value), Some(age)) = (st.entries.get(key).and_then(|e| e.value.clone()), age) {
            if age <= REPLAY_MAX_AGE {
                tx.send_replace(Some(value));
            }
        }
        st.subscribers
            .entry(key.to_string())
            .or_default()
            .insert(id, Subscriber { tx, interval });
        if age.is_none_or(|a| a >= fresh) {
            drop(self.start_flight(&mut st, key, fetch)); // 떠났다 — 결과는 store 로 온다
        }
        if !st.loops.contains_key(key) {
            let handle = self.make_loop(key);
            st.loops.insert(key.to_string(), handle);
        }
        HeaderSubscription {
            rx,
            feed: Arc::downgrade(&self.inner),
            key: key.to_string(),
            id,
        }
    }

    async fn value(
        &self,
        key: &str,
        max_age: TimeInterval,
        fetch: Fetch<V>,
    ) -> Result<V, ApiError> {
        let flight = {
            let mut st = lock(&self.inner.state);
            self.prune_expired(&mut st);
            if let (Some(value), Some(age)) = (
                st.entries.get(key).and_then(|e| e.value.clone()),
                self.value_age(&st, key),
            ) {
                if age < max_age {
                    return Ok(value);
                }
            }
            self.start_flight(&mut st, key, fetch)
        };
        flight.await
    }

    fn store(&self, value: V, key: &str) {
        let mut st = lock(&self.inner.state);
        Self::store_locked(&mut st, value, key, (self.inner.now)());
    }

    fn store_locked(
        st: &mut FeedState<V>,
        value: V,
        key: &str,
        now: cview_core::utils::time::Date,
    ) {
        let entry = st.entries.entry(key.to_string()).or_default();
        entry.value = Some(value.clone());
        entry.fetched_at = Some(now);
        if let Some(subs) = st.subscribers.get(key) {
            for s in subs.values() {
                s.tx.send_replace(Some(value.clone()));
            }
        }
    }

    fn invalidate(&self) {
        let mut st = lock(&self.inner.state);
        st.epoch = st.epoch.wrapping_add(1);
        st.entries.clear();
        st.flights.clear();
        let keys: Vec<String> = st.loops.keys().cloned().collect();
        for key in keys {
            if let Some(h) = st.loops.remove(&key) {
                h.abort();
            }
            let handle = self.make_loop(&key);
            st.loops.insert(key, handle);
        }
    }

    fn unsubscribe(&self, key: &str, id: Uuid) {
        let mut st = lock(&self.inner.state);
        let Some(subs) = st.subscribers.get_mut(key) else {
            return;
        };
        if subs.remove(&id).is_none() || !subs.is_empty() {
            return;
        }
        st.subscribers.remove(key);
        st.fetchers.remove(key);
        if let Some(h) = st.loops.remove(key) {
            h.abort();
        }
    }

    fn start_flight(&self, st: &mut FeedState<V>, key: &str, fetch: Fetch<V>) -> FlightFuture<V> {
        if let Some((_, f)) = st.flights.get(key) {
            return f.clone();
        }
        st.next_flight_id = st.next_flight_id.wrapping_add(1);
        let id = st.next_flight_id;
        let start_epoch = st.epoch;
        st.entries.entry(key.to_string()).or_default().attempted_at = Some((self.inner.now)());
        let weak = Arc::downgrade(&self.inner);
        let k = key.to_string();
        let handle = tokio::spawn(async move {
            let result = fetch().await;
            if let Some(inner) = weak.upgrade() {
                let mut st = lock(&inner.state);
                if st.flights.get(&k).map(|(i, _)| *i) == Some(id) {
                    st.flights.remove(&k);
                }
                if st.epoch == start_epoch {
                    if let Ok(v) = &result {
                        Self::store_locked(&mut st, v.clone(), &k, (inner.now)());
                    }
                }
            }
            result
        });
        let future: FlightFuture<V> =
            async move { handle.await.unwrap_or_else(|_| Err(ApiError::Cancelled)) }
                .boxed()
                .shared();
        st.flights.insert(key.to_string(), (id, future.clone()));
        future
    }

    fn make_loop(&self, key: &str) -> AbortHandle {
        let me = self.clone();
        let key = key.to_string();
        tokio::spawn(async move { me.run_loop(key).await }).abort_handle()
    }

    /// 채널 하나의 주기 루프. 다른 경로가 값을 넣으면 그만큼 미룬다.
    async fn run_loop(self, key: String) {
        loop {
            let action = {
                let mut st = lock(&self.inner.state);
                let Some(interval) = st
                    .subscribers
                    .get(&key)
                    .and_then(|s| s.values().map(|s| s.interval).reduce(f64::min))
                else {
                    return;
                };
                let Some(fetch) = st.fetchers.get(&key).cloned() else {
                    return;
                };
                let cycle = (self.inner.period)(interval).max(5.0);
                match self.since_last_refresh(&st, &key) {
                    Some(elapsed) if elapsed < cycle * 0.95 => Err(cycle - elapsed),
                    _ => Ok(self.start_flight(&mut st, &key, fetch)),
                }
            };
            match action {
                Err(wait) => (self.inner.sleep)(wait).await,
                Ok(flight) => {
                    let _ = flight.await;
                }
            }
        }
    }

    /// 구독자·진행 중 요청이 없고 재생 기한(10분)을 넘긴 값은 버린다.
    fn prune_expired(&self, st: &mut FeedState<V>) {
        let now = (self.inner.now)();
        let subscribers = &st.subscribers;
        let flights = &st.flights;
        st.entries.retain(|key, entry| {
            if subscribers.contains_key(key) || flights.contains_key(key) {
                return true;
            }
            entry
                .fetched_at
                .is_some_and(|at| secs_between(at, now) <= REPLAY_MAX_AGE)
        });
    }

    /// 값의 나이. 값이 없거나 저장 시각이 미래면 `None`.
    fn value_age(&self, st: &FeedState<V>, key: &str) -> Option<TimeInterval> {
        let entry = st.entries.get(key)?;
        entry.value.as_ref()?;
        let age = secs_between(entry.fetched_at?, (self.inner.now)());
        (age >= 0.0).then_some(age)
    }

    /// 마지막 저장·조회 시작 중 늦은 쪽부터 지난 시간. 미래 시각은 무시한다.
    fn since_last_refresh(&self, st: &FeedState<V>, key: &str) -> Option<TimeInterval> {
        let entry = st.entries.get(key)?;
        let now = (self.inner.now)();
        [entry.fetched_at, entry.attempted_at]
            .into_iter()
            .flatten()
            .filter(|t| *t <= now)
            .max()
            .map(|latest| secs_between(latest, now))
    }
}

// MARK: - 허브

/// 채널 헤더 허브. `Clone` 은 같은 허브를 가리킨다.
#[derive(Clone)]
pub struct ChannelHeaderHub {
    heartbeat_status: HeartbeatStatus,
    status_feed: Feed<LiveInfo>,
    ranking_feed: Feed<DonationRankResponse>,
}

impl ChannelHeaderHub {
    pub fn new(
        now: Clock,
        sleep: Sleep,
        period: Period,
        heartbeat_status: HeartbeatStatus,
    ) -> Self {
        Self {
            heartbeat_status,
            status_feed: Feed::new(now.clone(), sleep.clone(), period.clone()),
            ranking_feed: Feed::new(now, sleep, period),
        }
    }

    /// 실제 시계·잠, 하트비트 센터의 최근 상태를 쓰는 허브.
    pub fn with_heartbeat(center: WatchHeartbeatCenter) -> Self {
        let clock = system_clock();
        let now = clock.clone();
        Self::new(
            clock,
            system_sleep(),
            Arc::new(|i| i),
            Arc::new(move |id, max_age| center.recent_live_status(id, max_age, now())),
        )
    }

    fn status_fetch(&self, channel_id: &str, api: Arc<dyn ChannelHeaderSource>) -> Fetch<LiveInfo> {
        let heartbeat = self.heartbeat_status.clone();
        let id = channel_id.to_string();
        Arc::new(move || {
            // 영상 재생이 함께 살아 있으면 하트비트가 같은 endpoint 를 이미 받는다.
            if let Some(recent) = heartbeat(&id, HEARTBEAT_REUSE_MAX_AGE) {
                return async move { Ok(recent) }.boxed();
            }
            let api = api.clone();
            let id = id.clone();
            async move { api.live_status(&id).await }.boxed()
        })
    }

    fn ranking_fetch(
        channel_id: &str,
        rank_count: i64,
        api: Arc<dyn ChannelHeaderSource>,
    ) -> Fetch<DonationRankResponse> {
        let id = channel_id.to_string();
        Arc::new(move || {
            let api = api.clone();
            let id = id.clone();
            async move { api.weekly_donation_ranking(&id, rank_count).await }.boxed()
        })
    }

    fn ranking_key(channel_id: &str, rank_count: i64) -> String {
        format!("{channel_id}|{rank_count}")
    }

    /// 채널 헤더 상태 구독(기본 주기 60초).
    pub fn status_updates(
        &self,
        channel_id: &str,
        api: Arc<dyn ChannelHeaderSource>,
        interval: TimeInterval,
    ) -> HeaderSubscription<LiveInfo> {
        let fetch = self.status_fetch(channel_id, api);
        self.status_feed
            .subscribe(channel_id, interval, interval, fetch)
    }

    /// 읽기 경유 1회 조회 — 하트비트 최근값 → `max_age` 안의 캐시 → 진행 중 요청 합류 → 조회 순.
    pub async fn live_status(
        &self,
        channel_id: &str,
        api: Arc<dyn ChannelHeaderSource>,
        max_age: TimeInterval,
    ) -> Result<LiveInfo, ApiError> {
        if let Some(recent) =
            (self.heartbeat_status)(channel_id, max_age.min(HEARTBEAT_REUSE_MAX_AGE))
        {
            self.record(recent.clone(), channel_id);
            return Ok(recent);
        }
        let fetch = self.status_fetch(channel_id, api);
        self.status_feed.value(channel_id, max_age, fetch).await
    }

    /// 다른 경로가 받은 최신 상태를 넣는다.
    pub fn record(&self, info: LiveInfo, channel_id: &str) {
        self.status_feed.store(info, channel_id);
    }

    /// 채널 헤더 후원 랭킹 구독(기본 10위·300초·신선 30초).
    pub fn ranking_updates(
        &self,
        channel_id: &str,
        api: Arc<dyn ChannelHeaderSource>,
        rank_count: i64,
        interval: TimeInterval,
        fresh: TimeInterval,
    ) -> HeaderSubscription<DonationRankResponse> {
        let fetch = Self::ranking_fetch(channel_id, rank_count, api);
        self.ranking_feed.subscribe(
            &Self::ranking_key(channel_id, rank_count),
            interval,
            fresh,
            fetch,
        )
    }

    pub async fn ranking(
        &self,
        channel_id: &str,
        api: Arc<dyn ChannelHeaderSource>,
        rank_count: i64,
        max_age: TimeInterval,
    ) -> Result<DonationRankResponse, ApiError> {
        let fetch = Self::ranking_fetch(channel_id, rank_count, api);
        self.ranking_feed
            .value(&Self::ranking_key(channel_id, rank_count), max_age, fetch)
            .await
    }

    /// 사용자별 값(랭킹의 내 순위)을 비운다. 응답 캐시를 먼저 비운 뒤에 부른다.
    pub fn invalidate_auth_scoped(&self) {
        self.ranking_feed.invalidate();
    }

    pub fn status_loop_count(&self) -> usize {
        self.status_feed.loop_count()
    }
    pub fn ranking_loop_count(&self) -> usize {
        self.ranking_feed.loop_count()
    }
    pub fn status_subscriber_count(&self, channel_id: &str) -> usize {
        self.status_feed.subscriber_count(channel_id)
    }
    pub fn has_status_flight(&self, channel_id: &str) -> bool {
        self.status_feed.has_flight(channel_id)
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/ChannelHeaderHubTests.swift`
    use super::*;
    use crate::services::support::ManualClock;
    use cview_core::models::chat::messages::DonationRankItem;
    use cview_core::utils::time::from_unix;
    use std::sync::atomic::{AtomicI64, Ordering};
    use tokio::sync::oneshot;

    type Sleeper = (f64, oneshot::Sender<()>);

    /// 가짜 시계 + 잠 — `advance` 가 기한이 지난 잠을 깨운다.
    #[derive(Clone)]
    struct FakeTime {
        clock: ManualClock,
        sleepers: Arc<Mutex<Vec<Sleeper>>>,
    }

    impl FakeTime {
        fn new() -> Self {
            Self {
                clock: ManualClock::new(from_unix(1_000.0)),
                sleepers: Arc::new(Mutex::new(Vec::new())),
            }
        }
        fn pending(&self) -> usize {
            lock(&self.sleepers).retain(|(_, tx)| !tx.is_closed());
            lock(&self.sleepers).len()
        }
        fn advance(&self, secs: f64) {
            self.clock.advance(secs);
            let now = self.clock.uptime();
            let mut list = lock(&self.sleepers);
            let (due, rest): (Vec<_>, Vec<_>) = list.drain(..).partition(|(d, _)| *d <= now);
            *list = rest;
            for (_, tx) in due {
                let _ = tx.send(());
            }
        }
        fn sleep(&self) -> Sleep {
            let me = self.clone();
            Arc::new(move |secs| {
                let (tx, rx) = oneshot::channel();
                lock(&me.sleepers).push((me.clock.uptime() + secs, tx));
                async move {
                    let _ = rx.await;
                }
                .boxed()
            })
        }
    }

    struct FakeApi {
        status_calls: AtomicI64,
        ranking_calls: AtomicI64,
    }

    #[async_trait]
    impl ChannelHeaderSource for FakeApi {
        async fn live_status(&self, _: &str) -> Result<LiveInfo, ApiError> {
            let n = self.status_calls.fetch_add(1, Ordering::SeqCst) + 1;
            let mut info = LiveInfo::new(1, "라이브");
            info.concurrent_user_count = n * 100;
            Ok(info)
        }
        async fn weekly_donation_ranking(
            &self,
            _: &str,
            _: i64,
        ) -> Result<DonationRankResponse, ApiError> {
            let n = self.ranking_calls.fetch_add(1, Ordering::SeqCst) + 1;
            let item: DonationRankItem = serde_json::from_value(serde_json::json!({
                "userIdHash": format!("user-{n}"), "nickName": "n", "donationAmount": 1, "ranking": 1
            }))
            .unwrap();
            Ok(DonationRankResponse {
                rank_list: vec![item],
                donation_user_weekly_rank: None,
            })
        }
    }

    struct Fx {
        time: FakeTime,
        hub: ChannelHeaderHub,
        api: Arc<FakeApi>,
        heartbeat: Arc<Mutex<Option<LiveInfo>>>,
    }

    fn fixture() -> Fx {
        let time = FakeTime::new();
        let heartbeat: Arc<Mutex<Option<LiveInfo>>> = Arc::new(Mutex::new(None));
        let hb = heartbeat.clone();
        let hub = ChannelHeaderHub::new(
            time.clock.clock(),
            time.sleep(),
            Arc::new(|i| i),
            Arc::new(move |_, _| lock(&hb).clone()),
        );
        Fx {
            time,
            hub,
            api: Arc::new(FakeApi {
                status_calls: AtomicI64::new(0),
                ranking_calls: AtomicI64::new(0),
            }),
            heartbeat,
        }
    }

    async fn eventually(mut f: impl FnMut() -> bool) -> bool {
        for _ in 0..2_000 {
            if f() {
                return true;
            }
            tokio::task::yield_now().await;
        }
        f()
    }

    impl Fx {
        fn api(&self) -> Arc<dyn ChannelHeaderSource> {
            self.api.clone()
        }
        fn status_calls(&self) -> i64 {
            self.api.status_calls.load(Ordering::SeqCst)
        }
        fn ranking_calls(&self) -> i64 {
            self.api.ranking_calls.load(Ordering::SeqCst)
        }
    }

    async fn collect<V: Clone + Send + Sync + 'static>(
        sub: &mut HeaderSubscription<V>,
        out: &mut Vec<V>,
        want: usize,
    ) {
        for _ in 0..2_000 {
            while let Some(v) = sub.try_next() {
                out.push(v);
            }
            if out.len() >= want {
                return;
            }
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test]
    async fn two_subscribers_share_one_loop() {
        let fx = fixture();
        let mut a = fx.hub.status_updates("ch", fx.api(), 60.0);
        let mut b = fx.hub.status_updates("ch", fx.api(), 60.0);
        let (mut va, mut vb) = (Vec::new(), Vec::new());
        collect(&mut a, &mut va, 1).await;
        collect(&mut b, &mut vb, 1).await;
        assert_eq!(fx.status_calls(), 1, "구독자마다 즉시 조회가 따로 나갔다");
        assert!(eventually(|| fx.time.pending() == 1).await);
        assert_eq!(fx.hub.status_loop_count(), 1);
        for tick in 2..=3usize {
            fx.time.advance(60.0);
            collect(&mut a, &mut va, tick).await;
            collect(&mut b, &mut vb, tick).await;
            assert_eq!(fx.status_calls(), tick as i64);
            assert!(eventually(|| fx.time.pending() == 1).await);
        }
        let counts: Vec<i64> = va.iter().map(|v| v.concurrent_user_count).collect();
        assert_eq!(counts, [100, 200, 300]);
    }

    #[tokio::test]
    async fn resubscribe_within_interval_reuses_cache() {
        let fx = fixture();
        let mut first = fx.hub.status_updates("ch", fx.api(), 60.0);
        let mut v = Vec::new();
        collect(&mut first, &mut v, 1).await;
        drop(first);
        assert!(eventually(|| fx.hub.status_loop_count() == 0 && fx.time.pending() == 0).await);

        fx.time.advance(30.0);
        let mut again = fx.hub.status_updates("ch", fx.api(), 60.0);
        assert!(!fx.hub.has_status_flight("ch"));
        let mut v = Vec::new();
        collect(&mut again, &mut v, 1).await;
        assert_eq!(v[0].concurrent_user_count, 100);
        assert_eq!(fx.status_calls(), 1);
        assert!(eventually(|| fx.time.pending() == 1).await);
        fx.time.advance(30.0);
        collect(&mut again, &mut v, 2).await;
        assert_eq!(fx.status_calls(), 2);
    }

    #[tokio::test]
    async fn resubscribe_after_interval_replays_then_fetches_once() {
        let fx = fixture();
        let mut first = fx.hub.status_updates("ch", fx.api(), 60.0);
        let mut v = Vec::new();
        collect(&mut first, &mut v, 1).await;
        drop(first);
        assert!(eventually(|| fx.hub.status_loop_count() == 0).await);
        fx.time.advance(70.0);
        let mut again = fx.hub.status_updates("ch", fx.api(), 60.0);
        let mut v = Vec::new();
        // 최신 1개만 버퍼하므로 캐시값을 먼저 꺼낸다.
        collect(&mut again, &mut v, 1).await;
        collect(&mut again, &mut v, 2).await;
        let counts: Vec<i64> = v.iter().map(|x| x.concurrent_user_count).collect();
        assert_eq!(counts.last(), Some(&200));
        assert_eq!(fx.status_calls(), 2);
    }

    #[tokio::test]
    async fn invalidate_auth_scoped_refetches_ranking() {
        let fx = fixture();
        let mut probe = fx.hub.ranking_updates("ch", fx.api(), 10, 300.0, 30.0);
        let mut v = Vec::new();
        collect(&mut probe, &mut v, 1).await;
        assert_eq!(fx.ranking_calls(), 1);
        fx.hub.invalidate_auth_scoped();
        collect(&mut probe, &mut v, 2).await;
        assert_eq!(fx.ranking_calls(), 2);
        assert_eq!(v.last().unwrap().rank_list[0].user_id_hash, "user-2");
        drop(probe);
        assert!(eventually(|| fx.hub.ranking_loop_count() == 0).await);

        fx.time.advance(10.0);
        let mut warm = fx.hub.ranking_updates("ch", fx.api(), 10, 300.0, 30.0);
        let mut w = Vec::new();
        collect(&mut warm, &mut w, 1).await;
        assert_eq!(fx.ranking_calls(), 2);
        drop(warm);

        fx.hub.invalidate_auth_scoped();
        let mut fresh = fx.hub.ranking_updates("ch", fx.api(), 10, 300.0, 30.0);
        let mut f = Vec::new();
        collect(&mut fresh, &mut f, 1).await;
        assert_eq!(fx.ranking_calls(), 3);
        assert_eq!(f[0].rank_list[0].user_id_hash, "user-3");
    }

    #[tokio::test]
    async fn last_unsubscribe_cancels_loop() {
        let fx = fixture();
        let mut first = fx.hub.status_updates("ch", fx.api(), 60.0);
        let second = fx.hub.status_updates("ch", fx.api(), 60.0);
        let mut v = Vec::new();
        collect(&mut first, &mut v, 1).await;
        assert!(eventually(|| fx.time.pending() == 1).await);
        drop(first);
        assert_eq!(fx.hub.status_subscriber_count("ch"), 1);
        assert_eq!(fx.hub.status_loop_count(), 1);
        drop(second);
        assert!(eventually(|| fx.hub.status_loop_count() == 0 && fx.time.pending() == 0).await);
        fx.time.advance(600.0);
        assert_eq!(fx.status_calls(), 1);
    }

    #[tokio::test]
    async fn live_status_read_through_order() {
        let fx = fixture();
        let mut hb = LiveInfo::new(1, "하트비트");
        hb.concurrent_user_count = 7;
        *lock(&fx.heartbeat) = Some(hb);
        let got = fx.hub.live_status("ch", fx.api(), 60.0).await.unwrap();
        assert_eq!(got.concurrent_user_count, 7);
        assert_eq!(fx.status_calls(), 0);
        *lock(&fx.heartbeat) = None;
        let cached = fx.hub.live_status("ch", fx.api(), 60.0).await.unwrap();
        assert_eq!(cached.concurrent_user_count, 7, "캐시값");
        fx.time.advance(61.0);
        let fetched = fx.hub.live_status("ch", fx.api(), 60.0).await.unwrap();
        assert_eq!(fetched.concurrent_user_count, 100);
        assert_eq!(fx.status_calls(), 1);
    }
}
