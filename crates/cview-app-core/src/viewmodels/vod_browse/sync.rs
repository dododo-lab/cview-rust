//! 로컬 사본과 동기화 패스 — 대상 계획·받기·백오프·반영·저장.

use std::collections::{HashMap, HashSet};

use cview_core::models::search::VodInfo;
use cview_core::utils::time::{adding, Date, TimeInterval};
use cview_persistence::store::vod_cache::SubscriptionVodPage;

use super::{
    authoritative_followed, cached_data, due_count, filling_channel, spawn_joinable, to_vod_info,
    InitialLoad, SyncOutcome, SyncTargets, VodBrowseState, VodBrowseViewModel,
};
use crate::services::vod_feed::{
    ChannelPageResult, VodFailureKind, VodFeedAggregator, VodSubscriptionFeedCursor,
};
use crate::services::vod_sync_state::{
    FollowSignal, VodSubscriptionTag, VodSyncMode, VodSyncPlanner, VodSyncPolicy, VodSyncTier,
};
use crate::services::vod_sync_store::{VodEntryWindow, VodFollowDetails, VodSyncStore};
use crate::viewmodels::vod_library::VodLoadState;

impl VodBrowseViewModel {
    pub(super) async fn prepare_subscriptions(&self) -> Option<String> {
        let tag = self.current_tag();
        self.adopt_tag(tag.clone());
        self.load_local(tag.clone()).await;
        self.open_sync_record(tag.as_deref()).await;
        tag
    }

    pub(super) async fn show_local_feed(&self) {
        self.prepare_subscriptions().await;
        self.rebuild_feed(Self::PAGE_SIZE, false, false, VodLoadState::Loading)
            .await;
    }

    /// 계정 토큰이 바뀌었으면 이전 계정의 사본·피드를 버린다.
    fn adopt_tag(&self, tag: Option<String>) {
        let mut s = self.state.lock();
        if tag == s.local_rows_tag {
            return;
        }
        s.local_rows_tag = tag;
        s.local_rows.clear();
        s.local_exhausted.clear();
        s.local_rows_loaded = false;
        s.has_pending_rebuild = false;
        s.subscription_generation = s.subscription_generation.wrapping_add(1);
        s.cursor = VodSubscriptionFeedCursor::default();
        s.subscriptions_page = 0;
        s.can_load_more = false;
        s.feed.clear();
    }

    /// 계정 사본을 채널별로 읽는다(계정마다 한 번).
    pub async fn load_local(&self, tag: Option<String>) {
        let Some(tag) = tag else { return };
        if self.deps.data_store.is_none() || self.state.read().local_rows_loaded {
            return;
        }
        let epoch = self.epoch();
        let read_tag = tag.clone();
        let rows = self
            .blocking(move |st| {
                st.fetch_subscription_vods(&read_tag, Self::PER_CHANNEL_LIMIT)
                    .map(|m| {
                        m.into_iter()
                            .map(|(k, v)| (k, v.iter().map(to_vod_info).collect::<Vec<_>>()))
                            .collect::<HashMap<_, _>>()
                    })
            })
            .await;
        let Some(Ok(rows)) = rows else { return };
        if self.current_tag().as_deref() != Some(tag.as_str()) {
            return;
        }
        let mut s = self.state.lock();
        if s.local_rows_tag.as_deref() != Some(tag.as_str()) || s.invalidation_epoch != epoch {
            return;
        }
        for (channel_id, stored) in rows {
            s.local_rows.entry(channel_id).or_insert(stored);
        }
        s.local_rows_loaded = true;
    }

    async fn open_sync_record(&self, tag: Option<&str>) -> VodSyncStore {
        match tag.and_then(VodSubscriptionTag::scope_of_tag) {
            Some(scope) => {
                self.sync_store.activate(&scope).await;
                self.sync_store.clone()
            }
            None => self.untagged_sync_store.clone(),
        }
    }

    /// 로컬 사본으로 커서를 다시 세우고 첫 화면을 채운다.
    pub(super) async fn rebuild_feed(
        &self,
        target: usize,
        allow_network: bool,
        use_window: bool,
        empty_state: VodLoadState,
    ) {
        let ids = self.channel_ids();
        let follows_nobody = ids.is_empty()
            && self
                .deps
                .follow_details
                .as_ref()
                .is_some_and(|p| p().is_authoritative);
        let generation = {
            let mut s = self.state.lock();
            s.has_pending_rebuild = false;
            s.subscription_generation = s.subscription_generation.wrapping_add(1);
            s.subscriptions_page = 0;
            let order: Vec<String> = if ids.is_empty() && !follows_nobody {
                let mut keys: Vec<String> = s.local_rows.keys().cloned().collect();
                keys.sort();
                keys
            } else {
                ids
            };
            s.cursor = VodSubscriptionFeedCursor::seeded(
                &s.local_rows,
                &order,
                VodSubscriptionFeedCursor::CHANNEL_PAGE_SIZE,
                &s.local_exhausted,
            );
            s.subscription_generation
        };
        let rounds = if allow_network { 2 } else { 0 };
        let Some(first) = self
            .drain_subscriptions(target, rounds, generation, use_window)
            .await
        else {
            return;
        };
        let mut s = self.state.lock();
        s.can_load_more = s.cursor.has_more();
        s.state = if first.is_empty() {
            empty_state
        } else {
            VodLoadState::Loaded
        };
        s.feed = first;
    }

    /// 동기화 패스 한 번과 그 뒤 피드 다시 세우기.
    pub(super) async fn sync_and_rebuild(&self, mode: VodSyncMode) {
        let epoch = self.epoch();
        let outcome = self.run_sync_pass_once(mode).await;
        if self.epoch() != epoch {
            return;
        }
        let (needs_rebuild, defer) = {
            let s = self.state.read();
            (
                outcome.successes > 0 || s.has_pending_rebuild,
                s.subscriptions_page > 0 || s.is_loading_more,
            )
        };
        if needs_rebuild && defer {
            self.state.lock().has_pending_rebuild = true;
        } else {
            if needs_rebuild {
                let allow = self.is_network_allowed();
                self.rebuild_feed(Self::PAGE_SIZE, allow, true, VodLoadState::Empty)
                    .await;
                if self.epoch() != epoch {
                    return;
                }
            }
            self.settle_subscription_state(&outcome);
        }
        if self.state.read().initial_load == InitialLoad::Synced {
            self.schedule_continuation_if_needed();
        }
    }

    fn settle_subscription_state(&self, outcome: &SyncOutcome) {
        let mut s = self.state.lock();
        if !s.feed.is_empty() {
            s.state = VodLoadState::Loaded;
            return;
        }
        s.state = if outcome.blocked {
            VodLoadState::Error("다시보기를 잠시 뒤 다시 불러옵니다".into())
        } else if outcome.successes == 0 && outcome.failures > 0 {
            VodLoadState::Error("팔로우한 채널의 다시보기를 불러오지 못했습니다".into())
        } else {
            VodLoadState::Empty
        };
    }

    /// 패스를 한 번에 하나씩 돈다.
    pub(super) async fn run_sync_pass_once(&self, mode: VodSyncMode) -> SyncOutcome {
        let _guard = self.pass_lock.lock().await;
        self.state.lock().sync_running_mode = Some(mode);
        let started = std::time::Instant::now();
        let outcome = self.perform_sync_pass(mode).await;
        self.state.lock().sync_running_mode = None;
        if outcome.follows > 0 {
            tracing::debug!(
                mode = mode.raw_value(),
                follows = outcome.follows,
                budget = outcome.budget,
                planned = outcome.planned,
                requested = outcome.requested,
                failed = outcome.failures,
                due_left = outcome.due_left,
                fresh_skip = outcome.fresh_skip,
                ms = started.elapsed().as_millis() as u64,
                "vod_sync_pass"
            );
        }
        outcome
    }

    async fn perform_sync_pass(&self, mode: VodSyncMode) -> SyncOutcome {
        let ids = self.channel_ids();
        if ids.is_empty() {
            return SyncOutcome::default();
        }
        if !self.is_network_allowed() {
            return SyncOutcome {
                blocked: true,
                ..Default::default()
            };
        }
        let start = self.now();
        let tag = self.current_tag();
        if tag != self.state.read().local_rows_tag {
            return SyncOutcome::default();
        }
        let epoch = self.epoch();
        let store = self.open_sync_record(tag.as_deref()).await;
        if self.epoch() != epoch || self.current_tag() != tag {
            return SyncOutcome::default();
        }
        let details = self.follow_details();
        let followed = authoritative_followed(&ids, &details);
        if let Some(f) = &followed {
            store.retain(f);
            self.state.lock().local_rows.retain(|k, _| f.contains(k));
        }
        store.observe_live_set(&details.live, start);
        let mut outcome = SyncOutcome {
            follows: ids.len(),
            budget: if mode == VodSyncMode::Continuation {
                Self::CONTINUATION_BATCH_SIZE
            } else {
                VodSyncPolicy::entry_budget(ids.len() as i64)
            },
            ..Default::default()
        };
        let has_local = !self.state.read().local_rows.is_empty();
        if mode != VodSyncMode::Refresh
            && mode != VodSyncMode::Continuation
            && has_local
            && VodSyncPolicy::is_feed_fresh(store.state().last_pass_at, start)
        {
            outcome.fresh_skip = true;
            outcome.due_left = due_count(&ids, &store, start);
            return outcome;
        }
        let plan = self.plan_targets(&ids, &details.live, &store, mode, start);
        outcome.planned = plan.channel_ids.len();
        outcome.due_left = plan.due_count;
        if plan.channel_ids.is_empty() {
            return outcome;
        }
        let results = self.fetch_targets(&plan.channel_ids, mode).await;
        {
            let s = self.state.read();
            if self.current_tag() != tag || s.local_rows_tag != tag || s.invalidation_epoch != epoch
            {
                return SyncOutcome::default();
            }
        }
        self.apply_results(
            results, &plan, mode, tag, &store, &details, followed, outcome,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_results(
        &self,
        results: Vec<ChannelPageResult>,
        plan: &SyncTargets,
        mode: VodSyncMode,
        tag: Option<String>,
        store: &VodSyncStore,
        details: &VodFollowDetails,
        followed: Option<HashSet<String>>,
        mut outcome: SyncOutcome,
    ) -> SyncOutcome {
        let filled: Vec<ChannelPageResult> = results
            .iter()
            .map(|r| filling_channel(r, details))
            .collect();
        let finished = self.now();
        store.record(&filled, finished);
        let succeeded: HashSet<String> = filled
            .iter()
            .filter(|r| !r.failed)
            .map(|r| r.channel_id.clone())
            .collect();
        let stop_after = if mode == VodSyncMode::Continuation {
            Self::CONTINUATION_STOP_AFTER_FAILURES
        } else {
            Self::SYNC_STOP_AFTER_FAILURES
        };
        self.apply_backoff(&filled, succeeded.len(), finished, stop_after);
        self.absorb(&filled, tag, followed);
        outcome.requested = filled.len();
        outcome.successes = succeeded.len();
        outcome.failures = filled.len() - succeeded.len();
        outcome.due_left = plan
            .due_count
            .saturating_sub(succeeded.intersection(&plan.due_ids).count());
        if !succeeded.is_empty() {
            store.note_pass(finished, outcome.due_left);
        }
        outcome
    }

    async fn fetch_targets(&self, ids: &[String], mode: VodSyncMode) -> Vec<ChannelPageResult> {
        let size = VodSubscriptionFeedCursor::CHANNEL_PAGE_SIZE;
        let pages = HashMap::new();
        if mode == VodSyncMode::Continuation {
            self.aggregator
                .fetch_pages(
                    ids,
                    &pages,
                    size,
                    Self::CONTINUATION_CONCURRENCY,
                    Self::CONTINUATION_STOP_AFTER_FAILURES,
                )
                .await
        } else {
            self.aggregator
                .fetch_pages(
                    ids,
                    &pages,
                    size,
                    VodFeedAggregator::DEFAULT_MAX_CONCURRENT,
                    Self::SYNC_STOP_AFTER_FAILURES,
                )
                .await
        }
    }

    fn plan_targets(
        &self,
        ids: &[String],
        live: &HashSet<String>,
        store: &VodSyncStore,
        mode: VodSyncMode,
        now: Date,
    ) -> SyncTargets {
        let follows: Vec<FollowSignal> = ids
            .iter()
            .map(|c| FollowSignal {
                channel_id: c.clone(),
                is_live: live.contains(c),
            })
            .collect();
        let state = store.state();
        let failed = store.failed_at();
        if mode == VodSyncMode::Continuation {
            let plan = VodSyncPlanner::plan(
                &follows,
                &state,
                &failed,
                now,
                Self::CONTINUATION_BATCH_SIZE,
                mode,
            );
            return SyncTargets {
                due_ids: plan.channel_ids.iter().cloned().collect(),
                due_count: plan.due_count,
                channel_ids: plan.channel_ids,
            };
        }
        let mut s = self.state.lock();
        let budget = s
            .entry_window
            .open_remaining(now)
            .unwrap_or_else(|| VodSyncPolicy::entry_budget(ids.len() as i64));
        let plan = VodSyncPlanner::plan(
            &follows,
            &state,
            &failed,
            now,
            budget.saturating_sub(VodEntryWindow::DRAIN_RESERVE),
            mode,
        );
        let granted = s.entry_window.take(plan.channel_ids.len(), now);
        let rotation = plan
            .tier_counts
            .get(&VodSyncTier::Rotation)
            .copied()
            .unwrap_or(0);
        let due_len = plan.channel_ids.len().saturating_sub(rotation);
        SyncTargets {
            due_ids: plan.channel_ids[..due_len].iter().cloned().collect(),
            due_count: plan.due_count,
            channel_ids: plan.channel_ids.into_iter().take(granted).collect(),
        }
    }

    /// 429 는 max(60초, Retry-After), 오프라인은 60초.
    fn apply_backoff(
        &self,
        results: &[ChannelPageResult],
        successes: usize,
        at: Date,
        stop_after: usize,
    ) {
        let mut delay: Option<TimeInterval> = None;
        let mut offline = 0;
        for r in results {
            match r.failure_kind {
                Some(VodFailureKind::RateLimited(retry)) => {
                    delay = Some(delay.unwrap_or(0.0).max(Self::MINIMUM_BACKOFF.max(retry)));
                }
                Some(VodFailureKind::Offline) => offline += 1,
                _ => {}
            }
        }
        if delay.is_none() && offline > 0 && (successes == 0 || offline >= stop_after) {
            delay = Some(Self::MINIMUM_BACKOFF);
        }
        let Some(delay) = delay else { return };
        let mut s = self.state.lock();
        let candidate = adding(&at, delay);
        let base = s.sync_backoff_until.unwrap_or(at);
        s.sync_backoff_until = Some(base.max(candidate));
    }

    /// 커서에서 최신순으로 `count` 개를 꺼낸다. 떨어진 활성 채널은 먼저 다음 페이지를 받는다.
    /// 새로고침이 끼어들었으면 `None`.
    pub(super) async fn drain_subscriptions(
        &self,
        count: usize,
        fetch_rounds: usize,
        generation: u64,
        use_window: bool,
    ) -> Option<Vec<VodInfo>> {
        let mut items = Vec::new();
        let mut failed: HashSet<String> = HashSet::new();
        let mut rounds = 0;
        while items.len() < count {
            let pages = {
                let mut s = self.state.lock();
                if s.subscription_generation != generation {
                    return None;
                }
                if let Some(next) = s.cursor.pop_newest(true, &failed) {
                    items.push(next);
                    continue;
                }
                let pages = if rounds < fetch_rounds {
                    let wanted = s.cursor.pages_to_fetch(&failed);
                    self.granted_pages(&mut s, wanted, use_window)
                } else {
                    HashMap::new()
                };
                if pages.is_empty() {
                    match s.cursor.pop_newest(false, &HashSet::new()) {
                        Some(next) => {
                            items.push(next);
                            continue;
                        }
                        None => break,
                    }
                }
                pages
            };
            rounds += 1;
            let tag = self.state.read().local_rows_tag.clone();
            let details = self.follow_details();
            let ids: Vec<String> = pages.keys().cloned().collect();
            let results = self
                .aggregator
                .fetch_pages(
                    &ids,
                    &pages,
                    VodSubscriptionFeedCursor::CHANNEL_PAGE_SIZE,
                    VodFeedAggregator::DEFAULT_MAX_CONCURRENT,
                    usize::MAX,
                )
                .await;
            let filled: Vec<ChannelPageResult> = results
                .iter()
                .map(|r| filling_channel(r, &details))
                .collect();
            {
                let mut s = self.state.lock();
                if s.subscription_generation != generation {
                    return None;
                }
                failed.extend(
                    filled
                        .iter()
                        .filter(|r| r.failed)
                        .map(|r| r.channel_id.clone()),
                );
                s.cursor.apply(&filled);
            }
            let followed = authoritative_followed(&self.channel_ids(), &details);
            self.absorb(&filled, tag, followed);
        }
        Some(items)
    }

    fn granted_pages(
        &self,
        s: &mut VodBrowseState,
        pages: HashMap<String, i64>,
        use_window: bool,
    ) -> HashMap<String, i64> {
        if !use_window || pages.is_empty() {
            return pages;
        }
        let now = self.now();
        let granted = s.entry_window.take(pages.len(), now);
        if granted >= pages.len() {
            return pages;
        }
        let keep: HashSet<&String> = s
            .cursor
            .active()
            .iter()
            .filter(|c| pages.contains_key(*c))
            .take(granted)
            .collect();
        pages
            .iter()
            .filter(|(k, _)| keep.contains(k))
            .map(|(k, v)| (k.clone(), *v))
            .collect()
    }

    /// 받은 페이지를 사본에 합치고 저장소에 쓴다. 실패는 사본을 건드리지 않는다.
    fn absorb(
        &self,
        results: &[ChannelPageResult],
        tag: Option<String>,
        followed: Option<HashSet<String>>,
    ) {
        let succeeded: Vec<&ChannelPageResult> = results.iter().filter(|r| !r.failed).collect();
        if succeeded.is_empty() {
            return;
        }
        {
            let mut s = self.state.lock();
            for r in &succeeded {
                let existing = s.local_rows.get(&r.channel_id).cloned().unwrap_or_default();
                let merged = VodSubscriptionFeedCursor::merged_local_rows(
                    &existing,
                    r.page,
                    &r.items,
                    !r.has_more(),
                    Self::PER_CHANNEL_LIMIT,
                );
                if merged.is_empty() {
                    s.local_rows.remove(&r.channel_id);
                } else {
                    s.local_rows.insert(r.channel_id.clone(), merged);
                }
                if r.has_more() {
                    s.local_exhausted.remove(&r.channel_id);
                } else {
                    s.local_exhausted.insert(r.channel_id.clone());
                }
            }
        }
        self.persist(&succeeded, tag, followed);
    }

    /// 받은 페이지를 계정 사본에 쓴다 — 받은 순서로 잇고, 쓰기 직전에 태그를 다시 본다.
    fn persist(
        &self,
        results: &[&ChannelPageResult],
        tag: Option<String>,
        followed: Option<HashSet<String>>,
    ) {
        let (Some(store), Some(tag)) = (self.deps.data_store.clone(), tag) else {
            return;
        };
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let fetched_at = self.now();
        let pages: Vec<SubscriptionVodPage> = results
            .iter()
            .map(|r| SubscriptionVodPage {
                channel_id: r.channel_id.clone(),
                page: r.page,
                items: r
                    .items
                    .iter()
                    .map(|v| cached_data(v, &r.channel_id, &tag, fetched_at))
                    .collect(),
                exhausted: !r.has_more(),
            })
            .collect();
        let provider = self.deps.subscription_tag.clone();
        let mut s = self.state.lock();
        let previous = s.persist_chain.take();
        let epoch = s.invalidation_epoch;
        let state = self.state.clone();
        let chain = spawn_joinable(async move {
            if let Some(p) = previous {
                p.await;
            }
            let current = provider.as_ref().and_then(|p| p());
            if current.as_deref() != Some(tag.as_str()) || state.read().invalidation_epoch != epoch
            {
                return;
            }
            let _ = tokio::task::spawn_blocking(move || {
                store.apply_subscription_vod_pages(
                    &tag,
                    &pages,
                    followed.as_ref(),
                    Self::PER_CHANNEL_LIMIT,
                )
            })
            .await;
        });
        s.persist_chain = Some(chain);
    }
}
