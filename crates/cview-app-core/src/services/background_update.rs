//! `Sources/CViewApp/Services/BackgroundUpdateService.swift` 포팅.
//!
//! 백그라운드 팔로잉 알림기. 팔로잉 네트워크는 [`FollowingStore`] 가 소유하고, 이 서비스는
//! 폴 결과를 싱크로 받아(`ingest`) 방송 시작·제목·카테고리 변경을 판정한다. 주기 실행은
//! `RefreshCoordinator` 가 한다 — 여기에는 타이머가 없다.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use cview_core::models::{format_viewer_count, LiveChannelItem};
use cview_core::utils::following_poll::{FollowingPollOutcome, FollowingPollReason};
use cview_core::utils::time::{Date, TimeInterval};
use futures_util::future::FutureExt;

use super::support::{lock, system_clock, Clock};
use crate::caching::following_store::{
    to_live_channel_item, FollowingPollSinkSlot, FollowingStore, PollOutcome, PollResult,
};

/// 채널 변경 정보.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelChangeInfo {
    pub channel_id: String,
    pub channel_name: String,
    pub old_value: String,
    pub new_value: String,
    /// 채널 프로필 이미지 — 알림 첨부용.
    pub channel_image_url: Option<String>,
}

impl ChannelChangeInfo {
    fn from_channel(channel: &OnlineChannel, old: &str, new: &str) -> Self {
        Self {
            channel_id: channel.channel_id.clone(),
            channel_name: channel.channel_name.clone(),
            old_value: old.to_string(),
            new_value: new.to_string(),
            channel_image_url: channel.channel_image_url.clone(),
        }
    }
}

/// 방송 중인 팔로잉 채널.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnlineChannel {
    pub channel_id: String,
    pub channel_name: String,
    pub live_title: String,
    pub viewer_count: i64,
    pub thumbnail_url: Option<String>,
    pub category_name: Option<String>,
    pub channel_image_url: Option<String>,
    /// 방송 시작 시각. 라이브 상세 조회에 실패한 행은 `None`.
    pub open_date: Option<Date>,
}

impl OnlineChannel {
    pub fn id(&self) -> &str {
        &self.channel_id
    }

    pub fn formatted_viewer_count(&self) -> String {
        format_viewer_count(self.viewer_count)
    }

    /// 시청자 수 내림차순, 동률은 channelId 오름차순(전순서).
    pub fn display_order(lhs: &Self, rhs: &Self) -> std::cmp::Ordering {
        rhs.viewer_count
            .cmp(&lhs.viewer_count)
            .then_with(|| lhs.channel_id.cmp(&rhs.channel_id))
    }

    /// 팔로잉 목록 → 방송 중인 행(정렬 포함).
    pub fn live_rows(following: &[LiveChannelItem]) -> Vec<Self> {
        let mut rows: Vec<Self> = following
            .iter()
            .filter(|i| i.is_live)
            .map(|item| Self {
                channel_id: item.channel_id.clone(),
                channel_name: item.channel_name.clone(),
                live_title: item.live_title.clone(),
                viewer_count: item.viewer_count,
                thumbnail_url: item.thumbnail_url.clone(),
                category_name: item.category_name.clone(),
                channel_image_url: item.channel_image_url.clone(),
                open_date: item.open_date,
            })
            .collect();
        rows.sort_by(Self::display_order);
        rows
    }
}

/// 감지된 이벤트들.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BackgroundUpdateEvent {
    pub newly_online: Vec<OnlineChannel>,
    pub category_changed: Vec<ChannelChangeInfo>,
    pub title_changed: Vec<ChannelChangeInfo>,
}

impl BackgroundUpdateEvent {
    pub fn has_any_event(&self) -> bool {
        !self.newly_online.is_empty()
            || !self.category_changed.is_empty()
            || !self.title_changed.is_empty()
    }
}

/// 알림 판정 기준선 — 직전 폴의 방송 중 채널과 채널별 카테고리·제목.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BackgroundUpdateBaseline {
    pub online_ids: HashSet<String>,
    pub categories: HashMap<String, String>,
    pub titles: HashMap<String, String>,
}

/// 폴 결과 한 번과 기준선의 비교(순수).
#[derive(Debug, Clone, PartialEq)]
pub struct BackgroundUpdateDiff {
    pub online: Vec<OnlineChannel>,
    pub newly_online: Vec<OnlineChannel>,
    pub category_changed: Vec<ChannelChangeInfo>,
    pub title_changed: Vec<ChannelChangeInfo>,
    pub next_baseline: BackgroundUpdateBaseline,
}

impl BackgroundUpdateDiff {
    pub fn new(following: &[LiveChannelItem], previous: &BackgroundUpdateBaseline) -> Self {
        let current = OnlineChannel::live_rows(following);
        let mut category_changed = Vec::new();
        let mut title_changed = Vec::new();
        // 직전 폴에도 방송 중이던 채널만 본다. 빈 제목(상세 없음)은 변경이 아니다.
        for channel in current
            .iter()
            .filter(|c| previous.online_ids.contains(&c.channel_id))
        {
            if let (Some(old), Some(new)) = (
                previous.categories.get(&channel.channel_id),
                channel.category_name.as_ref(),
            ) {
                if old != new {
                    category_changed.push(ChannelChangeInfo::from_channel(channel, old, new));
                }
            }
            if let Some(old) = previous.titles.get(&channel.channel_id) {
                if *old != channel.live_title && !channel.live_title.is_empty() {
                    title_changed.push(ChannelChangeInfo::from_channel(
                        channel,
                        old,
                        &channel.live_title,
                    ));
                }
            }
        }
        let newly_online = current
            .iter()
            .filter(|c| !previous.online_ids.contains(&c.channel_id))
            .cloned()
            .collect();
        let mut categories = HashMap::new();
        let mut titles = HashMap::new();
        for ch in &current {
            if let Some(cat) = &ch.category_name {
                categories
                    .entry(ch.channel_id.clone())
                    .or_insert_with(|| cat.clone());
            }
            titles
                .entry(ch.channel_id.clone())
                .or_insert_with(|| ch.live_title.clone());
        }
        let next_baseline = BackgroundUpdateBaseline {
            online_ids: current.iter().map(|c| c.channel_id.clone()).collect(),
            categories,
            titles,
        };
        Self {
            online: current,
            newly_online,
            category_changed,
            title_changed,
            next_baseline,
        }
    }
}

/// 폴 결과를 받는 방식.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestMode {
    Normal,
    /// 기준선만 잡고 이벤트를 내지 않는다(부팅·로그인 결과·시작 시드).
    SeedBaseline,
}

type EventHandler = Arc<dyn Fn(BackgroundUpdateEvent) + Send + Sync>;
type LiveEnded = Arc<dyn Fn(HashSet<String>, Date) + Send + Sync>;

#[derive(Default)]
struct State {
    online_channels: Vec<OnlineChannel>,
    online_count: usize,
    last_updated: Option<Date>,
    last_failure: Option<Date>,
    cycle_epoch: u64,
    last_polled_following: Option<(Vec<LiveChannelItem>, Date)>,
    baseline: BackgroundUpdateBaseline,
    is_first_poll_this_cycle: bool,
    active_event_handler: Option<EventHandler>,
    on_poll_succeeded: Option<Arc<dyn Fn() + Send + Sync>>,
    on_live_ended: Option<LiveEnded>,
    following_store: Option<FollowingStore>,
}

/// 백그라운드 알림기 핸들.
#[derive(Clone)]
pub struct BackgroundUpdateService {
    state: Arc<Mutex<State>>,
    clock: Clock,
}

impl Default for BackgroundUpdateService {
    fn default() -> Self {
        Self::new(system_clock())
    }
}

impl BackgroundUpdateService {
    pub fn new(clock: Clock) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                is_first_poll_this_cycle: true,
                ..State::default()
            })),
            clock,
        }
    }

    // MARK: 읽기

    pub fn online_channels(&self) -> Vec<OnlineChannel> {
        lock(&self.state).online_channels.clone()
    }
    pub fn online_count(&self) -> usize {
        lock(&self.state).online_count
    }
    pub fn last_updated(&self) -> Option<Date> {
        lock(&self.state).last_updated
    }
    pub fn last_failure(&self) -> Option<Date> {
        lock(&self.state).last_failure
    }
    pub fn current_epoch(&self) -> u64 {
        lock(&self.state).cycle_epoch
    }
    pub fn last_polled_following(&self) -> Option<(Vec<LiveChannelItem>, Date)> {
        lock(&self.state).last_polled_following.clone()
    }
    /// 폴 진행 중(메뉴바 헤더 스피너).
    pub fn is_updating(&self) -> bool {
        lock(&self.state)
            .following_store
            .as_ref()
            .is_some_and(|s| s.is_polling())
    }

    pub fn set_on_poll_succeeded(&self, f: Option<Arc<dyn Fn() + Send + Sync>>) {
        lock(&self.state).on_poll_succeeded = f;
    }

    /// 방송 종료 알림 — 첫 폴·기준선 시드는 부르지 않는다.
    pub fn set_on_live_ended(&self, f: Option<LiveEnded>) {
        lock(&self.state).on_live_ended = f;
    }

    // MARK: 수명

    /// 폴 결과 싱크를 건다(앱 초기화에서 한 번).
    pub fn attach(&self, store: &FollowingStore) {
        lock(&self.state).following_store = Some(store.clone());
        let weak = Arc::downgrade(&self.state);
        let clock = self.clock.clone();
        store.set_sink(
            FollowingPollSinkSlot::Notifier,
            Some(Arc::new(move |outcome: PollOutcome| {
                if let Some(state) = weak.upgrade() {
                    let service = BackgroundUpdateService {
                        state,
                        clock: clock.clone(),
                    };
                    service.receive(outcome);
                }
                async {}.boxed()
            })),
        );
    }

    /// 알림 주기 — 최소 30초.
    pub fn poll_interval(interval: TimeInterval) -> TimeInterval {
        interval.max(30.0)
    }

    /// 알림 주기 시작 — 이벤트 처리기를 걸고 기준선을 시드한다.
    pub fn start(&self, interval: TimeInterval, on_event: EventHandler) {
        self.stop(false);
        let store = {
            let mut state = lock(&self.state);
            state.active_event_handler = Some(on_event);
            state.following_store.clone()
        };
        let timer_interval = Self::poll_interval(interval);
        if let Some(store) = store {
            if let Some(seed) = store.fresh_result(timer_interval * 2.0) {
                self.seed_baseline(&seed);
            }
            store.set_scheduled_cadence(Some(timer_interval));
        }
        tracing::info!("BackgroundUpdateService started (interval: {interval}s)");
    }

    /// 업데이트 중지. `clearing_snapshot` 이면 표시 중인 결과까지 버린다.
    pub fn stop(&self, clearing_snapshot: bool) {
        let store = {
            let mut state = lock(&self.state);
            state.active_event_handler = None;
            state.baseline = BackgroundUpdateBaseline::default();
            state.is_first_poll_this_cycle = true;
            state.cycle_epoch = state.cycle_epoch.wrapping_add(1);
            state.following_store.clone()
        };
        if let Some(store) = store {
            store.set_scheduled_cadence(None);
        }
        if clearing_snapshot {
            self.clear_snapshot();
        }
    }

    /// 표시 중인 결과를 비운다 — 계정 경계와 자동 새로고침 끔에서만.
    pub fn clear_snapshot(&self) {
        let mut state = lock(&self.state);
        state.online_channels.clear();
        state.online_count = 0;
        state.last_updated = None;
        state.last_failure = None;
        state.last_polled_following = None;
        state.cycle_epoch = state.cycle_epoch.wrapping_add(1);
    }

    /// 수동 새로고침 — 스토어의 수동 폴에 맡긴다. 결과는 싱크로 돌아온다.
    pub async fn refresh(&self) {
        let store = lock(&self.state).following_store.clone();
        if let Some(store) = store {
            store
                .poll(FollowingPollReason::Manual {
                    invalidate_thumbnails: false,
                })
                .await;
        }
    }

    fn accepts(&self, reason: FollowingPollReason) -> bool {
        lock(&self.state).active_event_handler.is_some() || reason.is_manual()
    }

    fn receive(&self, outcome: PollOutcome) {
        match outcome {
            FollowingPollOutcome::Success(result) => {
                if !self.accepts(result.reason) {
                    return;
                }
                let mode = if result.reason.seeds_baseline() {
                    IngestMode::SeedBaseline
                } else {
                    IngestMode::Normal
                };
                self.ingest(&result, mode);
            }
            FollowingPollOutcome::Failure(failure) => {
                if !self.accepts(failure.reason) {
                    return;
                }
                let epoch = self.current_epoch();
                self.record_failure(&failure.error.to_string(), epoch);
            }
        }
    }

    /// 성공한 폴 결과를 받아 비교·게시·이벤트를 처리한다.
    pub fn ingest(&self, result: &PollResult, mode: IngestMode) {
        let following: Vec<LiveChannelItem> =
            result.channels.iter().map(to_live_channel_item).collect();
        let previous = lock(&self.state).baseline.clone();
        let diff = BackgroundUpdateDiff::new(&following, &previous);
        self.publish(&diff.online, following, result.fetched_at, true);
        let (handler, live_ended, event) = {
            let mut state = lock(&self.state);
            let is_first = state.is_first_poll_this_cycle || mode == IngestMode::SeedBaseline;
            state.is_first_poll_this_cycle = false;
            state.baseline = diff.next_baseline.clone();
            let ended: HashSet<String> = previous
                .online_ids
                .difference(&diff.next_baseline.online_ids)
                .cloned()
                .collect();
            let live_ended = (!is_first && !ended.is_empty())
                .then(|| state.on_live_ended.clone().map(|f| (f, ended)))
                .flatten();
            let event = if is_first {
                BackgroundUpdateEvent::default()
            } else {
                BackgroundUpdateEvent {
                    newly_online: diff.newly_online.clone(),
                    category_changed: diff.category_changed.clone(),
                    title_changed: diff.title_changed.clone(),
                }
            };
            (state.active_event_handler.clone(), live_ended, event)
        };
        if let Some((f, ended)) = live_ended {
            f(ended, result.fetched_at);
        }
        if event.has_any_event() {
            if let Some(handler) = handler {
                handler(event.clone());
            }
        }
        tracing::debug!(
            "Background update: {} online, {} new, {} changed",
            diff.online.len(),
            event.newly_online.len(),
            event.category_changed.len() + event.title_changed.len()
        );
    }

    fn seed_baseline(&self, result: &PollResult) {
        let following: Vec<LiveChannelItem> =
            result.channels.iter().map(to_live_channel_item).collect();
        let diff = BackgroundUpdateDiff::new(&following, &BackgroundUpdateBaseline::default());
        self.publish(&diff.online, following, result.fetched_at, false);
        let mut state = lock(&self.state);
        state.baseline = diff.next_baseline;
        state.is_first_poll_this_cycle = false;
    }

    fn publish(
        &self,
        online: &[OnlineChannel],
        following: Vec<LiveChannelItem>,
        fetched_at: Date,
        signals_success: bool,
    ) {
        let callback = {
            let mut state = lock(&self.state);
            state.last_polled_following = Some((following, fetched_at));
            if state.online_channels != online {
                state.online_channels = online.to_vec();
            }
            state.online_count = online.len();
            state.last_updated = Some(fetched_at);
            state.last_failure = None;
            signals_success
                .then(|| state.on_poll_succeeded.clone())
                .flatten()
        };
        if let Some(f) = callback {
            f();
        }
    }

    /// 실패한 폴 기록. 이미 지나간 주기의 실패는 화면 상태로 올리지 않는다.
    pub fn record_failure(&self, message: &str, epoch: u64) {
        tracing::error!("Background update failed: {message}");
        let now = (self.clock)();
        let mut state = lock(&self.state);
        if epoch == state.cycle_epoch {
            state.last_failure = Some(now);
        }
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/FollowingNotifierIngestTests.swift` 핵심.
    use super::*;
    use crate::services::support::ManualClock;
    use cview_core::utils::following_poll::FollowingPollResult;
    use cview_core::utils::live_detail_refresh::MergedFollowingItem;
    use cview_core::utils::time::from_unix;

    fn item(id: &str, viewers: i64, title: &str, cat: Option<&str>) -> MergedFollowingItem {
        MergedFollowingItem {
            id: id.into(),
            channel_name: id.into(),
            channel_id: id.into(),
            live_title: title.into(),
            viewer_count: viewers,
            category_name: cat.map(str::to_string),
            is_live: true,
            ..MergedFollowingItem::default()
        }
    }

    fn result(reason: FollowingPollReason, channels: Vec<MergedFollowingItem>) -> PollResult {
        FollowingPollResult::new(reason, channels, from_unix(1_700_000_000.0), 0)
    }

    #[test]
    fn diff_detects_new_title_category_and_orders() {
        let prev_items: Vec<LiveChannelItem> = [item("a", 1, "t", Some("g"))]
            .iter()
            .map(to_live_channel_item)
            .collect();
        let base = BackgroundUpdateDiff::new(&prev_items, &BackgroundUpdateBaseline::default());
        let next: Vec<LiveChannelItem> = [
            item("a", 1, "t2", Some("h")),
            item("b", 5, "x", None),
            item("c", 5, "", None),
        ]
        .iter()
        .map(to_live_channel_item)
        .collect();
        let diff = BackgroundUpdateDiff::new(&next, &base.next_baseline);
        let ids: Vec<&str> = diff.online.iter().map(|c| c.channel_id.as_str()).collect();
        assert_eq!(ids, vec!["b", "c", "a"]);
        assert_eq!(diff.newly_online.len(), 2);
        assert_eq!(diff.category_changed[0].new_value, "h");
        assert_eq!(diff.title_changed[0].old_value, "t");
    }

    #[test]
    fn first_ingest_is_baseline_then_events_flow() {
        let clock = ManualClock::new(from_unix(0.0));
        let s = BackgroundUpdateService::new(clock.clock());
        let events: Arc<Mutex<Vec<BackgroundUpdateEvent>>> = Arc::default();
        let sink = events.clone();
        s.start(60.0, Arc::new(move |e| lock(&sink).push(e)));
        let ended: Arc<Mutex<Vec<HashSet<String>>>> = Arc::default();
        let e2 = ended.clone();
        s.set_on_live_ended(Some(Arc::new(move |ids, _| lock(&e2).push(ids))));

        s.ingest(
            &result(
                FollowingPollReason::Scheduled,
                vec![item("a", 1, "t", None)],
            ),
            IngestMode::Normal,
        );
        assert!(lock(&events).is_empty(), "첫 폴은 기준선만 잡는다");
        assert_eq!(s.online_count(), 1);
        s.ingest(
            &result(
                FollowingPollReason::Scheduled,
                vec![item("b", 1, "t", None)],
            ),
            IngestMode::Normal,
        );
        assert_eq!(lock(&events).len(), 1);
        assert_eq!(lock(&events)[0].newly_online[0].channel_id, "b");
        assert_eq!(*lock(&ended), vec![HashSet::from(["a".to_string()])]);

        // 시드 모드는 이벤트를 내지 않는다.
        s.ingest(
            &result(FollowingPollReason::Boot, vec![item("z", 1, "t", None)]),
            IngestMode::SeedBaseline,
        );
        assert_eq!(lock(&events).len(), 1);
    }

    #[test]
    fn stale_epoch_failure_is_ignored_and_clear_snapshot_resets() {
        let clock = ManualClock::new(from_unix(0.0));
        let s = BackgroundUpdateService::new(clock.clock());
        let old = s.current_epoch();
        s.stop(false);
        s.record_failure("x", old);
        assert!(s.last_failure().is_none());
        s.record_failure("x", s.current_epoch());
        assert!(s.last_failure().is_some());
        s.clear_snapshot();
        assert!(s.last_failure().is_none());
        assert_eq!(BackgroundUpdateService::poll_interval(10.0), 30.0);
    }
}
