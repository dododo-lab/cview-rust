//! `Sources/CViewApp/Caching/FollowingStore.swift` 포팅.
//!
//! 팔로잉 폴의 단일 소유자. 팔로잉 네트워크는 여기서만 나간다.
//!
//! ```text
//! poll(사유) → 목록(fetch_following_rows, 캐시 우회) → 상세 선택(live_detail_refresh::plan)
//!           → liveDetail 팬아웃(캐시 우회) → merge → 세대 확인 → 싱크(홈 → 알림기 → 제안기)
//! ```
//!
//! - 단일 비행: 진행 중이면 합류한다. manual 이 manual 아닌 폴에 합류하면 끝난 뒤 한 번 더 돈다.
//!   screenEntry 는 마지막 성공이 재사용 창 안이면 네트워크 없이 그 결과를 돌려준다(싱크는 부르지 않는다).
//! - 실패: 목록이 실패하면 폴 전체가 실패다. 싱크에 실패로 알린다. 취소·계정 경계 뒤에 끝난 폴은
//!   조용히 버린다.
//! - 네트워크는 [`FollowingSource`] trait 뒤에 있다 — `ChzzkApiClient` 구현을 제공하고 테스트는 가짜를 넣는다.
//! - `PollSignpost`/`PerfCaptureLog` 계측은 `tracing::debug!` 한 줄로 대체한다.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use cview_core::error::ApiError;
use cview_core::models::live::{LiveInfo, LiveStatus};
use cview_core::models::LiveChannelItem;
use cview_core::utils::following_poll::{
    FollowingPollFailure, FollowingPollOutcome, FollowingPollReason, FollowingPollResult,
    FollowingVisibleSurface, LiveDetectedTiming,
};
use cview_core::utils::live_detail_refresh::{
    self, FollowingRowSnapshot, LiveDetailMeta, LiveDetailRefreshConfig, LiveDetailRequest,
    LiveDetailSnapshot, MergedFollowingItem, PlanOptions,
};
use cview_core::utils::perf_tuning::background_poll::FOLLOWING_DETAIL_CONCURRENCY;
use cview_core::utils::time::{interval_since, Date, TimeInterval};
use cview_networking::response::{FollowingRow, FollowingRowsPage};
use cview_networking::ChzzkApiClient;
use futures_util::future::{BoxFuture, FutureExt, Shared};
use futures_util::stream::{FuturesUnordered, StreamExt};

use crate::services::support::{lock, system_clock, system_uptime, Clock, Uptime};

/// 폴 결과(채널 항목은 `MergedFollowingItem` — `LiveChannelItem` 과 같은 필드).
pub type PollResult = FollowingPollResult<MergedFollowingItem>;
pub type PollOutcome = FollowingPollOutcome<MergedFollowingItem, ApiError>;
pub type Sink = Arc<dyn Fn(PollOutcome) -> BoxFuture<'static, ()> + Send + Sync>;

/// `MergedFollowingItem` → 화면 모델.
pub fn to_live_channel_item(item: &MergedFollowingItem) -> LiveChannelItem {
    LiveChannelItem {
        id: item.id.clone(),
        channel_name: item.channel_name.clone(),
        channel_image_url: item.channel_image_url.clone(),
        live_title: item.live_title.clone(),
        viewer_count: item.viewer_count,
        category_name: item.category_name.clone(),
        category_type: item.category_type.clone(),
        thumbnail_url: item.thumbnail_url.clone(),
        channel_id: item.channel_id.clone(),
        is_live: item.is_live,
        open_date: item.open_date,
    }
}

/// `LiveInfo` → 병합 입력.
pub fn detail_snapshot(info: &LiveInfo) -> LiveDetailSnapshot {
    LiveDetailSnapshot {
        live_id: info.live_id,
        live_title: info.live_title.clone(),
        concurrent_user_count: info.concurrent_user_count,
        live_category_value: info.live_category_value.clone(),
        category_type: info.category_type.clone(),
        thumbnail_url: info.resolved_live_image_url().map(|u| u.to_string()),
        open_date: info.open_date,
        is_closed: info.status == LiveStatus::Close,
    }
}

/// 목록 행 → 정책 입력.
pub fn row_snapshot(row: &FollowingRow) -> FollowingRowSnapshot {
    let info = row.list_live_info.as_ref();
    FollowingRowSnapshot {
        channel_id: row.channel_id.clone(),
        channel_name: row.channel_name.clone(),
        channel_image_url: row.channel_image_url.clone(),
        is_live: row.is_live,
        list_title: info.and_then(|i| i.live_title.clone()),
        list_viewers: info.and_then(|i| i.concurrent_user_count),
        list_category: info.and_then(|i| i.live_category_value.clone()),
    }
}

/// 팔로잉 네트워크 경계.
#[async_trait]
pub trait FollowingSource: Send + Sync {
    /// 팔로잉 목록 전체(캐시 우회). 페이지 하나라도 실패하면 오류.
    async fn fetch_following_rows(&self) -> Result<FollowingRowsPage, ApiError>;
    /// 채널 상세(캐시 우회). 실패는 `None` — 지난 meta 가 남는다.
    async fn live_detail(&self, channel_id: &str) -> Option<LiveDetailSnapshot>;
}

#[async_trait]
impl FollowingSource for ChzzkApiClient {
    async fn fetch_following_rows(&self) -> Result<FollowingRowsPage, ApiError> {
        ChzzkApiClient::fetch_following_rows(self, true).await
    }

    async fn live_detail(&self, channel_id: &str) -> Option<LiveDetailSnapshot> {
        ChzzkApiClient::live_detail(self, channel_id, true)
            .await
            .ok()
            .map(|i| detail_snapshot(&i))
    }
}

/// 싱크 자리. 선언 순서대로 부른다 — 홈이 먼저 반영하고 알림기가 같은 결과로 이벤트를 낸다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FollowingPollSinkSlot {
    Home,
    Notifier,
    /// 멀티채팅 새 라이브 제안기 — 화면이 떠 있는 동안만 건다.
    Proposer,
}

impl FollowingPollSinkSlot {
    pub const ALL: [Self; 3] = [Self::Home, Self::Notifier, Self::Proposer];
}

/// 상세 팬아웃 동시 수. 정기·부팅 폴은 첨두를 낮추고, 사용자가 기다리는 경로는 8이다.
pub fn detail_concurrency(reason: FollowingPollReason) -> usize {
    match reason {
        FollowingPollReason::Scheduled | FollowingPollReason::Boot => FOLLOWING_DETAIL_CONCURRENCY,
        _ => 8,
    }
}

type SharedPoll = Shared<BoxFuture<'static, Option<PollResult>>>;

#[derive(Clone)]
struct InFlight {
    id: u64,
    reason: FollowingPollReason,
    generation: u64,
    future: SharedPoll,
    abort: tokio::task::AbortHandle,
}

type SetChanged = Arc<dyn Fn(HashSet<String>) + Send + Sync>;
type PollFinished = Arc<dyn Fn(bool) + Send + Sync>;
type NotifyWatchProvider = Arc<dyn Fn() -> Arc<dyn Fn(&str) -> bool + Send + Sync> + Send + Sync>;
type IsSuspended = Arc<dyn Fn() -> bool + Send + Sync>;

struct Inner {
    source: Option<Arc<dyn FollowingSource>>,
    live_count: usize,
    last_success_at: Option<Date>,
    last_failure_at: Option<Date>,
    is_polling: bool,
    last_result: Option<PollResult>,
    meta: HashMap<String, LiveDetailMeta>,
    visible_by_surface: HashMap<FollowingVisibleSurface, HashSet<String>>,
    in_flight: Option<InFlight>,
    generation: u64,
    sinks: HashMap<FollowingPollSinkSlot, Sink>,
    on_following_set_changed: Option<SetChanged>,
    on_poll_finished: Option<PollFinished>,
    notify_watch_provider: NotifyWatchProvider,
    scheduled_cadence: Option<TimeInterval>,
    previous_live: Option<HashSet<String>>,
    followed_ids: Option<HashSet<String>>,
    carries_live_info: bool,
    last_success_uptime: Option<TimeInterval>,
    last_poll_start_uptime: Option<TimeInterval>,
    next_flight_id: u64,
}

/// 팔로잉 폴 저장소 핸들. 복제는 같은 상태를 공유한다.
#[derive(Clone)]
pub struct FollowingStore {
    inner: Arc<Mutex<Inner>>,
    uptime: Uptime,
    clock: Clock,
    is_suspended: IsSuspended,
    config: LiveDetailRefreshConfig,
}

impl FollowingStore {
    /// screenEntry 가 네트워크 없이 마지막 결과를 쓰는 창.
    pub const SCREEN_ENTRY_REUSE_WINDOW: TimeInterval = 60.0;
    /// 알림 주기가 돌 때 창의 상한.
    pub const SCREEN_ENTRY_REUSE_CAP: TimeInterval = 180.0;

    pub fn new(source: Option<Arc<dyn FollowingSource>>) -> Self {
        Self::with_clocks(source, system_uptime(), system_clock(), Arc::new(|| false))
    }

    /// - `uptime`: 단조 시계(초). 상세 기한·screenEntry 창을 잰다.
    /// - `is_suspended`: 창이 가려졌는가 — 가림에는 보이는 카드가 없다.
    pub fn with_clocks(
        source: Option<Arc<dyn FollowingSource>>,
        uptime: Uptime,
        clock: Clock,
        is_suspended: IsSuspended,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                source,
                live_count: 0,
                last_success_at: None,
                last_failure_at: None,
                is_polling: false,
                last_result: None,
                meta: HashMap::new(),
                visible_by_surface: HashMap::new(),
                in_flight: None,
                generation: 0,
                sinks: HashMap::new(),
                on_following_set_changed: None,
                on_poll_finished: None,
                notify_watch_provider: Arc::new(|| Arc::new(|_: &str| false)),
                scheduled_cadence: None,
                previous_live: None,
                followed_ids: None,
                carries_live_info: false,
                last_success_uptime: None,
                last_poll_start_uptime: None,
                next_flight_id: 0,
            })),
            uptime,
            clock,
            is_suspended,
            config: LiveDetailRefreshConfig::default(),
        }
    }

    pub fn configure(&self, source: Arc<dyn FollowingSource>) {
        lock(&self.inner).source = Some(source);
    }

    pub fn set_sink(&self, slot: FollowingPollSinkSlot, sink: Option<Sink>) {
        let mut inner = lock(&self.inner);
        match sink {
            Some(s) => {
                inner.sinks.insert(slot, s);
            }
            None => {
                inner.sinks.remove(&slot);
            }
        }
    }

    pub fn set_on_following_set_changed(&self, f: Option<SetChanged>) {
        lock(&self.inner).on_following_set_changed = f;
    }

    pub fn set_on_poll_finished(&self, f: Option<PollFinished>) {
        lock(&self.inner).on_poll_finished = f;
    }

    pub fn set_notify_watch_provider(&self, f: NotifyWatchProvider) {
        lock(&self.inner).notify_watch_provider = f;
    }

    pub fn set_scheduled_cadence(&self, cadence: Option<TimeInterval>) {
        lock(&self.inner).scheduled_cadence = cadence;
    }

    /// 표면이 지금 보여 주는 카드. 숨거나 비활성이면 빈 집합을 넘긴다.
    pub fn set_visible<I: IntoIterator<Item = String>>(
        &self,
        ids: I,
        surface: FollowingVisibleSurface,
    ) {
        let next: HashSet<String> = ids.into_iter().collect();
        let mut inner = lock(&self.inner);
        if next.is_empty() {
            inner.visible_by_surface.remove(&surface);
        } else {
            inner.visible_by_surface.insert(surface, next);
        }
    }

    pub fn visible_channel_ids(&self) -> HashSet<String> {
        Self::visible_of(&lock(&self.inner))
    }

    fn visible_of(inner: &Inner) -> HashSet<String> {
        inner
            .visible_by_surface
            .values()
            .flat_map(|s| s.iter().cloned())
            .collect()
    }

    pub fn has_in_flight_poll(&self) -> bool {
        lock(&self.inner).in_flight.is_some()
    }

    pub fn is_polling(&self) -> bool {
        lock(&self.inner).is_polling
    }

    pub fn live_count(&self) -> usize {
        lock(&self.inner).live_count
    }

    pub fn last_success_at(&self) -> Option<Date> {
        lock(&self.inner).last_success_at
    }

    pub fn last_failure_at(&self) -> Option<Date> {
        lock(&self.inner).last_failure_at
    }

    pub fn last_result(&self) -> Option<PollResult> {
        lock(&self.inner).last_result.clone()
    }

    pub fn meta(&self) -> HashMap<String, LiveDetailMeta> {
        lock(&self.inner).meta.clone()
    }

    pub fn generation(&self) -> u64 {
        lock(&self.inner).generation
    }

    /// 테스트용 — 취소 없이 세대만 올린다.
    pub fn bump_generation(&self) {
        let mut inner = lock(&self.inner);
        inner.generation = inner.generation.wrapping_add(1);
    }

    /// 화면 진입이 네트워크 없이 마지막 결과를 쓰는 나이 — 알림 주기가 돌면 그 주기의
    /// 115%까지(상한 180초), 아니면 60초.
    pub fn screen_entry_reuse_age(&self) -> TimeInterval {
        Self::reuse_age(lock(&self.inner).scheduled_cadence)
    }

    fn reuse_age(cadence: Option<TimeInterval>) -> TimeInterval {
        match cadence {
            None => Self::SCREEN_ENTRY_REUSE_WINDOW,
            Some(c) => (c * 1.15).clamp(
                Self::SCREEN_ENTRY_REUSE_WINDOW,
                Self::SCREEN_ENTRY_REUSE_CAP,
            ),
        }
    }

    /// 마지막 성공 결과가 `max_age` 초 안이면 돌려준다.
    pub fn fresh_result(&self, max_age: TimeInterval) -> Option<PollResult> {
        let inner = lock(&self.inner);
        let result = inner.last_result.as_ref()?;
        let at = inner.last_success_uptime?;
        let age = (self.uptime)() - at;
        (age >= 0.0 && age <= max_age).then(|| result.clone())
    }

    /// 폴 한 번. 성공하면 싱크에 넘긴 결과를, 실패·취소·경계 뒤면 `None` 을 돌려준다.
    pub fn poll(&self, reason: FollowingPollReason) -> BoxFuture<'static, Option<PollResult>> {
        let this = self.clone();
        async move {
            if reason == FollowingPollReason::ScreenEntry && this.last_failure_at().is_none() {
                if let Some(reused) = this.fresh_result(this.screen_entry_reuse_age()) {
                    return Some(reused);
                }
            }
            let running = lock(&this.inner).in_flight.clone();
            if let Some(running) = running {
                let result = running.future.clone().await;
                if !(reason.is_manual()
                    && !running.reason.is_manual()
                    && running.generation == this.generation())
                {
                    return result;
                }
                return this.poll(reason).await;
            }
            this.start(reason).await
        }
        .boxed()
    }

    /// 계정 경계 — 세대를 올리고 진행 중인 폴을 취소하고 상태를 비운다. 보이는 표면 등록은 남긴다.
    pub fn reset_for_account_boundary(&self) {
        let mut inner = lock(&self.inner);
        inner.generation = inner.generation.wrapping_add(1);
        if let Some(flight) = inner.in_flight.take() {
            flight.abort.abort();
        }
        inner.is_polling = false;
        inner.last_result = None;
        inner.meta.clear();
        inner.previous_live = None;
        inner.followed_ids = None;
        inner.carries_live_info = false;
        inner.last_success_uptime = None;
        inner.last_poll_start_uptime = None;
        inner.live_count = 0;
        inner.last_success_at = None;
        inner.last_failure_at = None;
    }

    fn start(&self, reason: FollowingPollReason) -> SharedPoll {
        let mut inner = lock(&self.inner);
        inner.next_flight_id += 1;
        let id = inner.next_flight_id;
        let generation = inner.generation;
        let this = self.clone();
        // 비구조 작업 — 합류한 호출자 하나가 버려져도 공유 폴은 계속된다. 끊는 것은 계정 경계뿐이다.
        let handle = tokio::spawn(async move {
            let result = this.run(reason, generation).await;
            this.finish_flight(id);
            let finished = {
                let inner = lock(&this.inner);
                (inner.generation == generation)
                    .then(|| inner.on_poll_finished.clone())
                    .flatten()
            };
            if let Some(f) = finished {
                f(result.is_some());
            }
            result
        });
        let abort = handle.abort_handle();
        let future: SharedPoll = async move { handle.await.ok().flatten() }.boxed().shared();
        inner.in_flight = Some(InFlight {
            id,
            reason,
            generation,
            future: future.clone(),
            abort,
        });
        inner.is_polling = true;
        future
    }

    fn finish_flight(&self, id: u64) {
        let mut inner = lock(&self.inner);
        if inner.in_flight.as_ref().map(|f| f.id) != Some(id) {
            return;
        }
        inner.in_flight = None;
        inner.is_polling = false;
    }

    fn is_current(&self, generation: u64) -> bool {
        lock(&self.inner).generation == generation
    }

    async fn run(&self, reason: FollowingPollReason, generation: u64) -> Option<PollResult> {
        let source = lock(&self.inner).source.clone()?;
        let started = (self.uptime)();
        let poll_gap = {
            let mut inner = lock(&self.inner);
            let gap = inner.last_poll_start_uptime.map(|p| started - p);
            inner.last_poll_start_uptime = Some(started);
            gap
        };
        let page = match source.fetch_following_rows().await {
            Ok(page) => page,
            Err(error) => {
                if !self.is_current(generation) || error == ApiError::Cancelled {
                    return None;
                }
                lock(&self.inner).last_failure_at = Some((self.clock)());
                tracing::debug!("팔로잉 폴 실패({}): {error}", reason.capture_path());
                self.deliver(FollowingPollOutcome::Failure(FollowingPollFailure {
                    reason,
                    generation,
                    error,
                }))
                .await;
                return None;
            }
        };
        let listed = (self.uptime)();
        if !self.is_current(generation) {
            return None;
        }
        let rows: Vec<FollowingRowSnapshot> = page.rows.iter().map(row_snapshot).collect();
        let (requests, previous, meta) = {
            let inner = lock(&self.inner);
            // 라이브가 0개인 목록은 liveInfo 여부를 말해 주지 않는다 — 직전 값을 잇는다.
            let carries = if rows.iter().any(|r| r.is_live) {
                page.carries_live_info
            } else {
                inner.carries_live_info
            };
            let notify = (inner.notify_watch_provider)();
            let requests = live_detail_refresh::plan(
                &rows,
                inner.previous_live.as_ref(),
                &inner.meta,
                &Self::visible_of(&inner),
                |id: &str| notify(id),
                PlanOptions {
                    list_carries_live_info: carries,
                    suspended: (self.is_suspended)(),
                    manual: reason.is_manual(),
                },
                listed,
                &self.config,
            );
            (
                (requests, carries),
                inner.previous_live.clone(),
                inner.meta.clone(),
            )
        };
        let (requests, carries) = requests;
        let details =
            Self::fetch_details(&requests, &source, detail_concurrency(reason), &self.uptime).await;
        if !self.is_current(generation) {
            return None;
        }

        let (channels, next_meta) = Self::merge(&rows, &details, &meta, listed);
        let newly_live: HashSet<String> = previous
            .as_ref()
            .map(|prev| {
                channels
                    .iter()
                    .filter(|c| c.is_live && !prev.contains(&c.channel_id))
                    .map(|c| c.channel_id.clone())
                    .collect()
            })
            .unwrap_or_default();
        let mut result = FollowingPollResult::new(reason, channels, page.fetched_at, generation);
        result.newly_live = newly_live;
        result.is_baseline = previous.is_none();
        result.list_carries_live_info = carries;
        result.detail_requests = requests.clone();
        self.commit(&result, &rows, next_meta, carries);
        self.deliver(FollowingPollOutcome::Success(result.clone()))
            .await;
        let delivered = (self.uptime)();
        self.record_live_detected(&result, &details, started, listed, delivered, poll_gap);
        tracing::debug!(
            "FollowingPoll ok pages_ms={} details_n={} details_ms={} carries_live_info={}",
            ((listed - started) * 1_000.0) as i64,
            requests.len(),
            ((delivered - listed) * 1_000.0) as i64,
            u8::from(carries)
        );
        Some(result)
    }

    /// 계획 순서대로 띄운다(새 라이브가 먼저). 동시 수는 슬라이딩 창이다.
    async fn fetch_details(
        requests: &[LiveDetailRequest],
        source: &Arc<dyn FollowingSource>,
        concurrency: usize,
        uptime: &Uptime,
    ) -> HashMap<String, (Option<LiveDetailSnapshot>, TimeInterval)> {
        if requests.is_empty() {
            return HashMap::new();
        }
        type DetailFuture =
            BoxFuture<'static, (String, (Option<LiveDetailSnapshot>, TimeInterval))>;
        let limit = concurrency.max(1);
        let mut pending: FuturesUnordered<DetailFuture> = FuturesUnordered::new();
        let mut collected = HashMap::new();
        for request in requests {
            if pending.len() >= limit {
                if let Some((id, fetched)) = pending.next().await {
                    collected.insert(id, fetched);
                }
            }
            let source = source.clone();
            let uptime = uptime.clone();
            let channel_id = request.channel_id.clone();
            pending.push(
                async move {
                    let info = source.live_detail(&channel_id).await;
                    (channel_id, (info, uptime()))
                }
                .boxed(),
            );
        }
        while let Some((id, fetched)) = pending.next().await {
            collected.insert(id, fetched);
        }
        collected
    }

    /// 목록이 라이브라고 한 행 먼저(API 순서), 오프라인 행 뒤(API 순서).
    fn merge(
        rows: &[FollowingRowSnapshot],
        details: &HashMap<String, (Option<LiveDetailSnapshot>, TimeInterval)>,
        meta: &HashMap<String, LiveDetailMeta>,
        listed: TimeInterval,
    ) -> (Vec<MergedFollowingItem>, HashMap<String, LiveDetailMeta>) {
        let mut live = Vec::new();
        let mut offline = Vec::new();
        let mut next_meta = HashMap::new();
        for row in rows {
            let fetched = details.get(&row.channel_id);
            let merged = live_detail_refresh::merge(
                row,
                fetched.and_then(|f| f.0.as_ref()),
                meta.get(&row.channel_id),
                fetched.map_or(listed, |f| f.1),
            );
            if let Some(entry) = merged.meta {
                next_meta.insert(row.channel_id.clone(), entry);
            }
            if row.is_live {
                live.push(merged.item);
            } else {
                offline.push(merged.item);
            }
        }
        live.extend(offline);
        (live, next_meta)
    }

    fn commit(
        &self,
        result: &PollResult,
        rows: &[FollowingRowSnapshot],
        next_meta: HashMap<String, LiveDetailMeta>,
        carries: bool,
    ) {
        let changed_callback = {
            let mut inner = lock(&self.inner);
            inner.meta = next_meta;
            inner.carries_live_info = carries;
            inner.previous_live = Some(
                rows.iter()
                    .filter(|r| r.is_live)
                    .map(|r| r.channel_id.clone())
                    .collect(),
            );
            let ids: HashSet<String> = rows.iter().map(|r| r.channel_id.clone()).collect();
            let changed = inner
                .followed_ids
                .as_ref()
                .filter(|before| **before != ids)
                .map(|before| {
                    before
                        .symmetric_difference(&ids)
                        .cloned()
                        .collect::<HashSet<_>>()
                });
            inner.followed_ids = Some(ids);
            inner.last_result = Some(result.clone());
            inner.last_success_uptime = Some((self.uptime)());
            inner.live_count = result.live_count();
            inner.last_success_at = Some(result.fetched_at);
            inner.last_failure_at = None;
            inner.on_following_set_changed.clone().zip(changed)
        };
        if let Some((f, changed)) = changed_callback {
            f(changed);
        }
    }

    /// 싱크 사이에 계정 경계가 지나가면 뒤 싱크에는 넘기지 않는다.
    async fn deliver(&self, outcome: PollOutcome) {
        for slot in FollowingPollSinkSlot::ALL {
            let sink = {
                let inner = lock(&self.inner);
                if outcome.generation() != inner.generation {
                    return;
                }
                inner.sinks.get(&slot).cloned()
            };
            if let Some(sink) = sink {
                sink(outcome.clone()).await;
            }
        }
    }

    /// E8 — 새 라이브 한 채널마다 한 줄. 채널 ID 는 싣지 않는다.
    fn record_live_detected(
        &self,
        result: &PollResult,
        details: &HashMap<String, (Option<LiveDetailSnapshot>, TimeInterval)>,
        started: TimeInterval,
        listed: TimeInterval,
        delivered: TimeInterval,
        gap: Option<TimeInterval>,
    ) {
        if result.newly_live.is_empty() || !tracing::enabled!(tracing::Level::DEBUG) {
            return;
        }
        let wall_now = (self.clock)();
        for item in result
            .channels
            .iter()
            .filter(|c| result.newly_live.contains(&c.channel_id))
        {
            let timing = LiveDetectedTiming {
                lag_seconds: item.open_date.map(|o| interval_since(&wall_now, &o)),
                detect_seconds: listed - started,
                detail_seconds: details.get(&item.channel_id).map(|d| d.1 - started),
                notify_seconds: delivered - started,
                poll_seconds: gap,
                path: result.reason.capture_path().to_string(),
                carries_live_info: result.list_carries_live_info,
            };
            tracing::debug!("live_detected {:?}", timing.fields());
        }
    }
}

#[cfg(test)]
mod tests;
