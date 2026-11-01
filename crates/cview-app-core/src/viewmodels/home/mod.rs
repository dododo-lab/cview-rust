//! 홈 화면 뷰모델 — 인기·전수 카탈로그·팔로잉 목록과 통계, 캐시·부팅 스냅샷 복원.
//!
//! 원본: `Sources/CViewApp/ViewModels/HomeViewModel.swift` 와 확장들.
//!
//! | Swift | Rust |
//! |---|---|
//! | `HomeViewModel.swift` (상태·통계 정책·캐시 복원·자동 갱신 게이트) | 이 파일 |
//! | `HomeViewModel+StatsSnapshotPolicy.swift` | [`HomeViewModel::CATALOG_PAIR_WRITE_INTERVAL`] |
//! | `HomeViewModel+DataLoading.swift` | [`data_loading`] |
//! | `HomeViewModel+BootSnapshots.swift` | [`boot`] |
//! | `HomeViewModel+LegacyCacheMigration.swift` | 옮기지 않음(UserDefaults 구버전 캐시 이관) |
//! | `HomeRecommendationEngine.swift` | [`recommendation`] |
//!
//! Swift 는 수집 품질·진행률·오류를 `LiveCatalogStore` 프로퍼티로 위임했다. 저장소가 이 크레이트
//! 밖(경계 [`deps::LiveCatalogPort`])이라 여기서는 상태에 직접 두고, 저장소 배선이 setter 로 쓴다.
//! `recomputeStats()` 의 detached 계산은 가벼워 동기로 한다.

pub mod boot;
pub mod data_loading;
pub mod deps;
pub mod promotion;
pub mod recommendation;

use std::collections::HashSet;
use std::sync::{Arc, Weak};
use std::time::Duration;

use chrono::Utc;
use serde::Serialize;
use uuid::Uuid;

use cview_core::models::live_catalog::LiveCatalogSummary;
use cview_core::models::{LiveChannelItem, LiveInfo};
use cview_core::utils::auto_refresh_gate::{AutoRefreshGate, AutoRefreshHolder, AutoRefreshScreen};
use cview_core::utils::data_freshness::{self, FreshnessPolicy};
use cview_core::utils::following_poll::{FollowingPublishItem, FollowingPublishThrottle};
use cview_core::utils::time::{interval_since, Date, TimeInterval};
use cview_networking::response::LivePageCursor;

use self::boot::{BootExtrasSnapshot, BootFollowingSnapshot, BootHero, BootHydrateReport};
use self::deps::HomeDeps;
use self::promotion::{DataOrigin, FreshnessDisplay};
use self::recommendation::ScoredChannel;
use super::support::{cancel_slot, slot_is, Observed, ObservedGuard, TaskSlot};
use crate::services::refresh_coordinator::RefreshDemandSink;

pub use crate::caching::live_catalog_store::StatsCollectionQuality;

/// `computeChannelStats` 결과.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComputedChannelStats {
    pub total_live_channel_count: i64,
    pub total_viewers: i64,
}

/// `computeFollowingStats` 결과.
#[derive(Debug, Clone, PartialEq)]
pub struct FollowingStats {
    pub live_count: i64,
    pub recent_live: Vec<LiveChannelItem>,
    pub total_viewers: i64,
    pub live_rate: i64,
}

/// 캐시 키(구버전 UserDefaults blob — 이관은 옮기지 않았지만 이름은 위생 작업이 쓴다).
pub mod cache_key {
    pub const LIVE_CHANNELS: &str = "cache.liveChannels";
    pub const ALL_STAT_CHANNELS: &str = "cache.allStatChannels";
    pub const FOLLOWING_CHANNELS: &str = "cache.followingChannels";
    pub const FOLLOWING_SNAPSHOT: &str = "cache.following.snapshot.v2";
    pub const LIVE_CHANNELS_CACHED_AT: &str = "cache.liveChannels.ts";
    pub const ALL_STAT_CACHED_AT: &str = "cache.allStatChannels.ts";
    pub const ALL_STAT_COLLECTION_COMPLETE: &str = "cache.allStatChannels.complete.v2";
    pub const ALL_STAT_EXACT_LIVE_COUNT: &str = "cache.allStatChannels.liveCount.v2";
    pub const FOLLOWING_CACHED_AT: &str = "cache.followingChannels.ts";
}

/// 캐시에서 되살린 값.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RestoredLiveCache {
    pub live_channels: Vec<LiveChannelItem>,
    pub following_channels: Vec<LiveChannelItem>,
    pub has_live_channels_snapshot: bool,
    pub has_following_channels_snapshot: bool,
    pub following_scope: Option<Uuid>,
    pub live_channels_cached_at: Option<Date>,
    pub following_cached_at: Option<Date>,
    pub has_popular_file: bool,
    pub has_following_file: bool,
}

pub type Callback = Arc<dyn Fn() + Send + Sync>;

/// `HomeViewModel` 의 관찰 상태.
pub struct HomeState {
    pub(crate) this: Weak<HomeShared>,
    pub deps: HomeDeps,

    pub category_channels_revision: u64,
    pub live_channels_revision: u64,
    pub all_stat_channels_revision: u64,
    pub following_channels_revision: u64,
    live_channels: Vec<LiveChannelItem>,
    all_stat_channels: Vec<LiveChannelItem>,
    following_channels: Vec<LiveChannelItem>,
    pub recommended_channels: Vec<LiveChannelItem>,

    pub is_loading: bool,
    pub is_loading_following: bool,
    pub following_load_depth: i64,
    pub following_publish_throttle: FollowingPublishThrottle,
    pub last_following_persist_at: Option<Date>,
    pub error_message: Option<String>,
    pub search_query: String,
    pub all_stat_last_loaded_at: Option<Date>,

    // `LiveCatalogStore` 위임 프로퍼티.
    pub is_loading_stats: bool,
    pub stats_load_error: Option<String>,
    pub stats_collection_quality: StatsCollectionQuality,
    pub stats_collection_progress: f64,
    pub stats_collected_count: i64,
    pub stats_estimated_total: Option<i64>,

    pub live_channels_network_generation: u64,
    pub all_stats_network_generation: u64,
    pub following_network_generation: u64,
    pub following_auth_generation: u64,

    pub needs_cookie_login: bool,
    pub following_load_error: Option<String>,
    pub has_more_channels: bool,
    pub next_live_cursor: Option<LivePageCursor>,

    pub refresh_coordinator: Option<Arc<dyn RefreshDemandSink>>,
    pub is_auto_refresh_active: bool,
    auto_refresh_gate: AutoRefreshGate,
    pub auto_refresh_settings_enabled_provider: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    pub on_following_loaded: Option<Callback>,
    pub on_following_data_received: Option<Callback>,
    pub last_cache_persist_at: Option<Date>,

    pub total_viewers: i64,
    pub total_live_channel_count: i64,
    pub verified_total_live_count: Option<i64>,
    pub following_live_count: i64,
    pub recent_live_following: Vec<LiveChannelItem>,
    pub top_channels: Vec<LiveChannelItem>,
    pub following_live_rate: i64,
    pub following_total_viewers: i64,

    /// Swift `dataStore != nil`.
    pub has_data_store: bool,
    live_channels_cached_at: Option<Date>,
    pub all_stat_cached_at: Option<Date>,
    following_cached_at: Option<Date>,
    popular_origin: DataOrigin,
    following_origin: DataOrigin,
    pub popular_freshness: FreshnessDisplay,
    pub following_freshness: FreshnessDisplay,
    pub freshness_revision: u64,
    pub staleness_task: Option<TaskSlot>,

    pub following_cache_scope: Uuid,
    pub following_cache_allowed: bool,
    pub boot_following: Option<BootFollowingSnapshot>,
    pub boot_preload: Option<tokio::sync::watch::Receiver<bool>>,
    pub boot_hydrate_report: Option<BootHydrateReport>,
    pub following_is_provisional: bool,
    pub boot_extras: Option<BootExtrasSnapshot>,
    pub boot_hero: Option<BootHero>,
    pub boot_extras_visuals_discarded: bool,
    pub boot_recommendations: Vec<ScoredChannel>,
    pub catalog_rows_restore_pending: bool,
}

pub struct HomeShared {
    pub(crate) obs: Observed<HomeState>,
}

/// 홈 뷰모델 핸들(복제하면 같은 상태를 가리킨다).
#[derive(Clone)]
pub struct HomeViewModel {
    pub(crate) shared: Arc<HomeShared>,
}

impl HomeViewModel {
    pub const PAGE_SIZE: i64 = 30;
    pub const PUBLIC_SNAPSHOT_RESTORE_MAX_AGE: TimeInterval = 86_400.0;
    pub const FOLLOWING_SNAPSHOT_RESTORE_MAX_AGE: TimeInterval = 604_800.0;
    pub const HOME_EXTRAS_RESTORE_MAX_AGE: TimeInterval = 86_400.0;
    /// `LiveCatalogStore.pairWriteInterval`.
    pub const CATALOG_PAIR_WRITE_INTERVAL: TimeInterval = 600.0;
    pub const LIGHT_REFRESH_PERSIST_INTERVAL: TimeInterval = 180.0;

    pub fn new(deps: HomeDeps) -> Self {
        let shared = Arc::new_cyclic(|weak: &Weak<HomeShared>| HomeShared {
            obs: Observed::new(HomeState::new(weak.clone(), deps)),
        });
        Self { shared }
    }

    pub fn lock(&self) -> ObservedGuard<'_, HomeState> {
        self.shared.obs.lock()
    }

    pub fn read(&self) -> ObservedGuard<'_, HomeState> {
        self.shared.obs.read()
    }

    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.shared.obs.subscribe()
    }

    pub fn snapshot(&self) -> HomeSnapshot {
        HomeSnapshot::from_state(&self.read())
    }

    pub(crate) fn deps(&self) -> HomeDeps {
        self.read().deps.clone()
    }

    // MARK: 정적 정책

    pub fn popular_top_channels(live: &[LiveChannelItem]) -> Vec<LiveChannelItem> {
        let mut sorted = live.to_vec();
        sorted.sort_by_key(|c| std::cmp::Reverse(c.viewer_count));
        sorted.truncate(6);
        sorted
    }

    pub fn compute_channel_stats(
        all_stat_channels: &[LiveChannelItem],
        live_channels: &[LiveChannelItem],
    ) -> ComputedChannelStats {
        let raw = if all_stat_channels.is_empty() {
            live_channels
        } else {
            all_stat_channels
        };
        let mut seen = HashSet::new();
        let mut count = 0;
        let mut viewers = 0;
        for item in raw {
            if seen.insert(item.id.as_str()) {
                count += 1;
                viewers += item.viewer_count;
            }
        }
        ComputedChannelStats {
            total_live_channel_count: count,
            total_viewers: viewers,
        }
    }

    pub fn compute_following_stats(channels: &[LiveChannelItem]) -> FollowingStats {
        let live: Vec<&LiveChannelItem> = channels.iter().filter(|c| c.is_live).collect();
        FollowingStats {
            live_count: live.len() as i64,
            recent_live: live.iter().take(6).map(|c| (*c).clone()).collect(),
            total_viewers: live.iter().map(|c| c.viewer_count).sum(),
            live_rate: if channels.is_empty() {
                0
            } else {
                (live.len() * 100 / channels.len()) as i64
            },
        }
    }

    pub fn restored_stats_quality(
        cached_at: Date,
        collected_count: i64,
        was_complete: Option<bool>,
        exact_count: Option<i64>,
    ) -> StatsCollectionQuality {
        if was_complete == Some(true) && exact_count == Some(collected_count) {
            return StatsCollectionQuality::Complete {
                at: cached_at,
                count: collected_count,
            };
        }
        StatsCollectionQuality::Truncated {
            at: cached_at,
            count: collected_count,
        }
    }

    pub fn should_apply_restored_source(
        current_count: usize,
        has_snapshot: bool,
        captured_network_generation: u64,
        current_network_generation: u64,
    ) -> bool {
        current_count == 0
            && captured_network_generation == current_network_generation
            && has_snapshot
    }

    pub fn should_apply_restored_stats_metadata(is_loading_stats: bool) -> bool {
        !is_loading_stats
    }

    pub fn compact_live_count_label(
        quality: &StatsCollectionQuality,
        verified_count: Option<i64>,
        collected_count: i64,
        all_stats_count: i64,
        live_count: i64,
    ) -> String {
        if quality.is_complete() {
            if let Some(v) = verified_count {
                return format!("{v} 확인");
            }
        }
        let observed = if quality.is_collecting() {
            collected_count
        } else if all_stats_count > 0 {
            all_stats_count
        } else {
            live_count
        };
        if observed <= 0 {
            return "집계 중".into();
        }
        if quality.is_collecting() {
            return format!("{observed} 수집");
        }
        format!("{observed} 관측")
    }

    pub fn next_freshness_wake(
        popular_at: Option<&Date>,
        following_at: Option<&Date>,
        now: &Date,
    ) -> Option<Date> {
        [
            data_freshness::next_band_change(popular_at, &FreshnessPolicy::POPULAR, now),
            data_freshness::next_band_change(following_at, &FreshnessPolicy::FOLLOWING, now),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    // MARK: 자동 갱신 게이트

    pub fn retain_auto_refresh(&self, screen: AutoRefreshScreen, instance: u64) {
        let mut s = self.lock();
        s.sync_auto_refresh_settings_axis();
        if let Some(c) = &s.refresh_coordinator {
            c.retain_demand(screen, instance);
        }
        if s.auto_refresh_gate
            .retain(AutoRefreshHolder::new(screen, instance))
        {
            s.apply_auto_refresh_state();
        }
    }

    pub fn release_auto_refresh(&self, screen: AutoRefreshScreen, instance: u64) {
        let mut s = self.lock();
        if let Some(c) = &s.refresh_coordinator {
            c.release_demand(screen, instance);
        }
        if s.auto_refresh_gate
            .release(AutoRefreshHolder::new(screen, instance))
        {
            s.apply_auto_refresh_state();
        }
    }

    pub fn set_auto_refresh_enabled_by_settings(&self, enabled: bool) {
        let mut s = self.lock();
        if s.auto_refresh_gate.set_enabled_by_settings(enabled) {
            s.apply_auto_refresh_state();
        }
    }
}

/// 목록 비교용 래퍼(`FollowingPublishThrottle.sameExceptViewers` 의 LiveChannelItem 판).
pub(crate) struct PublishRow<'a>(pub &'a LiveChannelItem);

impl FollowingPublishItem for PublishRow<'_> {
    fn is_live(&self) -> bool {
        self.0.is_live
    }
    fn same_except_viewers(&self, o: &Self) -> bool {
        let (l, r) = (self.0, o.0);
        l.id == r.id
            && l.channel_id == r.channel_id
            && l.is_live == r.is_live
            && l.live_title == r.live_title
            && l.category_name == r.category_name
            && l.category_type == r.category_type
            && l.channel_name == r.channel_name
            && l.channel_image_url == r.channel_image_url
            && l.thumbnail_url == r.thumbnail_url
            && l.open_date == r.open_date
    }
}

/// `LiveInfo` → 화면 행.
pub fn live_channel_item_from(info: &LiveInfo) -> LiveChannelItem {
    let channel = info.channel.as_ref();
    LiveChannelItem {
        id: channel.map_or_else(|| info.live_id.to_string(), |c| c.channel_id.clone()),
        channel_name: channel.map_or_else(|| "Unknown".into(), |c| c.channel_name.clone()),
        channel_image_url: channel
            .and_then(|c| c.channel_image_url.as_ref())
            .map(|u| u.to_string()),
        live_title: info.live_title.clone(),
        viewer_count: info.concurrent_user_count,
        category_name: info.live_category_value.clone(),
        category_type: info.category_type.clone(),
        thumbnail_url: info.resolved_live_image_url().map(|u| u.to_string()),
        channel_id: channel.map(|c| c.channel_id.clone()).unwrap_or_default(),
        is_live: true,
        open_date: None,
        verified_badge: channel.and_then(|c| c.verified_badge()).map(str::to_string),
    }
}

impl HomeState {
    fn new(this: Weak<HomeShared>, deps: HomeDeps) -> Self {
        Self {
            this,
            deps,
            category_channels_revision: 0,
            live_channels_revision: 0,
            all_stat_channels_revision: 0,
            following_channels_revision: 0,
            live_channels: Vec::new(),
            all_stat_channels: Vec::new(),
            following_channels: Vec::new(),
            recommended_channels: Vec::new(),
            is_loading: false,
            is_loading_following: false,
            following_load_depth: 0,
            following_publish_throttle: FollowingPublishThrottle::default(),
            last_following_persist_at: None,
            error_message: None,
            search_query: String::new(),
            all_stat_last_loaded_at: None,
            is_loading_stats: false,
            stats_load_error: None,
            stats_collection_quality: StatsCollectionQuality::Unknown,
            stats_collection_progress: 0.0,
            stats_collected_count: 0,
            stats_estimated_total: None,
            live_channels_network_generation: 0,
            all_stats_network_generation: 0,
            following_network_generation: 0,
            following_auth_generation: 0,
            needs_cookie_login: false,
            following_load_error: None,
            has_more_channels: true,
            next_live_cursor: None,
            refresh_coordinator: None,
            is_auto_refresh_active: false,
            auto_refresh_gate: AutoRefreshGate::default(),
            auto_refresh_settings_enabled_provider: None,
            on_following_loaded: None,
            on_following_data_received: None,
            last_cache_persist_at: None,
            total_viewers: 0,
            total_live_channel_count: 0,
            verified_total_live_count: None,
            following_live_count: 0,
            recent_live_following: Vec::new(),
            top_channels: Vec::new(),
            following_live_rate: 0,
            following_total_viewers: 0,
            has_data_store: false,
            live_channels_cached_at: None,
            all_stat_cached_at: None,
            following_cached_at: None,
            popular_origin: DataOrigin::Network,
            following_origin: DataOrigin::Network,
            popular_freshness: FreshnessDisplay::NONE,
            following_freshness: FreshnessDisplay::NONE,
            freshness_revision: 0,
            staleness_task: None,
            following_cache_scope: Uuid::new_v4(),
            following_cache_allowed: false,
            boot_following: None,
            boot_preload: None,
            boot_hydrate_report: None,
            following_is_provisional: false,
            boot_extras: None,
            boot_hero: None,
            boot_extras_visuals_discarded: false,
            boot_recommendations: Vec::new(),
            catalog_rows_restore_pending: false,
        }
    }

    // MARK: 목록 커밋(리비전)

    pub fn live_channels(&self) -> &[LiveChannelItem] {
        &self.live_channels
    }
    pub fn all_stat_channels(&self) -> &[LiveChannelItem] {
        &self.all_stat_channels
    }
    pub fn following_channels(&self) -> &[LiveChannelItem] {
        &self.following_channels
    }

    pub fn set_live_channels(&mut self, v: Vec<LiveChannelItem>) {
        if v == self.live_channels {
            return;
        }
        self.live_channels = v;
        self.live_channels_revision = self.live_channels_revision.wrapping_add(1);
        self.category_channels_revision = self.category_channels_revision.wrapping_add(1);
    }

    pub fn set_all_stat_channels(&mut self, v: Vec<LiveChannelItem>) {
        if v == self.all_stat_channels {
            return;
        }
        self.all_stat_channels = v;
        self.all_stat_channels_revision = self.all_stat_channels_revision.wrapping_add(1);
        self.category_channels_revision = self.category_channels_revision.wrapping_add(1);
    }

    pub fn set_following_channels(&mut self, v: Vec<LiveChannelItem>) {
        if v == self.following_channels {
            return;
        }
        self.following_channels = v;
        self.following_channels_revision = self.following_channels_revision.wrapping_add(1);
    }

    pub fn category_channels(&self) -> &[LiveChannelItem] {
        if self.all_stat_channels.is_empty() {
            &self.live_channels
        } else {
            &self.all_stat_channels
        }
    }

    pub fn has_verified_total_live_channel_count(&self) -> bool {
        self.stats_collection_quality.is_complete() && self.verified_total_live_count.is_some()
    }

    // MARK: 신선도 didSet

    pub fn live_channels_cached_at(&self) -> Option<Date> {
        self.live_channels_cached_at
    }
    pub fn following_cached_at(&self) -> Option<Date> {
        self.following_cached_at
    }
    pub fn popular_origin(&self) -> DataOrigin {
        self.popular_origin
    }
    pub fn following_origin(&self) -> DataOrigin {
        self.following_origin
    }

    pub fn set_live_channels_cached_at(&mut self, v: Option<Date>) {
        let old = std::mem::replace(&mut self.live_channels_cached_at, v);
        if old != v {
            self.refresh_freshness(Utc::now());
        }
    }

    pub fn set_following_cached_at(&mut self, v: Option<Date>) {
        let old = std::mem::replace(&mut self.following_cached_at, v);
        if v.is_some() {
            if let Some(cb) = &self.on_following_data_received {
                cb();
            }
        }
        if old != v {
            self.refresh_freshness(Utc::now());
        }
    }

    pub fn set_popular_origin(&mut self, v: DataOrigin) {
        let old = std::mem::replace(&mut self.popular_origin, v);
        if old != v {
            self.refresh_freshness(Utc::now());
        }
    }

    pub fn set_following_origin(&mut self, v: DataOrigin) {
        let old = std::mem::replace(&mut self.following_origin, v);
        if old != v {
            self.refresh_freshness(Utc::now());
        }
    }

    pub fn refresh_freshness(&mut self, now: Date) {
        let popular = FreshnessDisplay::make(
            self.live_channels_cached_at.as_ref(),
            self.popular_origin,
            &FreshnessPolicy::POPULAR,
            &now,
        );
        let following = FreshnessDisplay::make(
            self.following_cached_at.as_ref(),
            self.following_origin,
            &FreshnessPolicy::FOLLOWING,
            &now,
        );
        let mut changed = false;
        if popular != self.popular_freshness {
            self.popular_freshness = popular;
            changed = true;
        }
        if following != self.following_freshness {
            self.following_freshness = following;
            changed = true;
        }
        if changed {
            self.freshness_revision = self.freshness_revision.wrapping_add(1);
        }
        self.schedule_staleness_wake(now);
    }

    fn schedule_staleness_wake(&mut self, now: Date) {
        cancel_slot(&mut self.staleness_task);
        let Some(wake) = HomeViewModel::next_freshness_wake(
            self.live_channels_cached_at.as_ref(),
            self.following_cached_at.as_ref(),
            &now,
        ) else {
            return;
        };
        let delay = interval_since(&wake, &now).max(0.0);
        let weak = self.this.clone();
        self.staleness_task = Some(TaskSlot::spawn(move |id| async move {
            tokio::time::sleep(Duration::from_secs_f64(delay)).await;
            let Some(shared) = weak.upgrade() else { return };
            let mut s = shared.obs.lock();
            if !slot_is(&s.staleness_task, id) {
                return;
            }
            s.refresh_freshness(Utc::now());
        }));
    }

    // MARK: 통계

    pub fn recompute_stats(&mut self) {
        let result =
            HomeViewModel::compute_channel_stats(&self.all_stat_channels, &self.live_channels);
        self.total_live_channel_count = if self.stats_collection_quality.is_complete() {
            self.verified_total_live_count
                .unwrap_or(result.total_live_channel_count)
        } else {
            result.total_live_channel_count
        };
        let first_paint = if self.all_stat_channels.is_empty() {
            self.deps.catalog.usable_summary()
        } else {
            None
        };
        self.total_viewers = first_paint.map_or(result.total_viewers, |s| s.viewer_total);
        self.apply_popular_derived();
    }

    pub fn refresh_popular_derived(&mut self) {
        if self.all_stat_channels.is_empty() {
            self.recompute_stats();
            return;
        }
        self.apply_popular_derived();
    }

    fn apply_popular_derived(&mut self) {
        let top = HomeViewModel::popular_top_channels(&self.live_channels);
        if self.top_channels != top {
            self.top_channels = top;
        }
    }

    pub(crate) fn apply_following_stats(&mut self, stats: FollowingStats) {
        self.following_live_count = stats.live_count;
        self.recent_live_following = stats.recent_live;
        self.following_total_viewers = stats.total_viewers;
        self.following_live_rate = stats.live_rate;
    }

    // MARK: 첫 그림 요약

    pub fn first_paint_summary(&self) -> Option<LiveCatalogSummary> {
        if self.all_stat_channels.is_empty() {
            self.deps.catalog.usable_summary()
        } else {
            None
        }
    }

    pub fn usable_catalog_summary(&self) -> Option<LiveCatalogSummary> {
        self.deps.catalog.usable_summary()
    }

    pub fn catalog_summary_revision(&self) -> u64 {
        self.deps.catalog.summary_revision()
    }

    pub fn first_paint_top_lives(&self) -> Vec<LiveChannelItem> {
        self.first_paint_summary()
            .map(|s| s.top_lives.iter().map(|r| r.item()).collect())
            .unwrap_or_default()
    }

    // MARK: 자동 갱신

    fn sync_auto_refresh_settings_axis(&mut self) {
        let Some(provider) = self.auto_refresh_settings_enabled_provider.clone() else {
            return;
        };
        if self.auto_refresh_gate.set_enabled_by_settings(provider()) {
            self.apply_auto_refresh_state();
        }
    }

    fn apply_auto_refresh_state(&mut self) {
        let active = self.auto_refresh_gate.is_active();
        if self.is_auto_refresh_active != active {
            self.is_auto_refresh_active = active;
        }
    }

    pub fn auto_refresh_gate(&self) -> &AutoRefreshGate {
        &self.auto_refresh_gate
    }

    pub fn should_persist_light_refresh_cache(&self) -> bool {
        match &self.last_cache_persist_at {
            None => true,
            Some(last) => {
                interval_since(&Utc::now(), last) >= HomeViewModel::LIGHT_REFRESH_PERSIST_INTERVAL
            }
        }
    }

    pub fn mark_cache_persisted(&mut self) {
        self.last_cache_persist_at = Some(Utc::now());
    }
}

/// 홈 관찰 스냅샷.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeSnapshot {
    pub live_channels: Vec<LiveChannelItem>,
    pub all_stat_channels_count: usize,
    pub following_channels: Vec<LiveChannelItem>,
    pub recommended_channels: Vec<LiveChannelItem>,
    pub live_channels_revision: u64,
    pub all_stat_channels_revision: u64,
    pub category_channels_revision: u64,
    pub following_channels_revision: u64,
    pub is_loading: bool,
    pub is_loading_stats: bool,
    pub is_loading_following: bool,
    pub error_message: Option<String>,
    pub stats_load_error: Option<String>,
    pub stats_collection_quality: StatsCollectionQuality,
    pub stats_collection_progress: f64,
    pub stats_collected_count: i64,
    pub stats_estimated_total: Option<i64>,
    pub needs_cookie_login: bool,
    pub following_load_error: Option<String>,
    pub has_more_channels: bool,
    pub total_viewers: i64,
    pub total_live_channel_count: i64,
    pub verified_total_live_count: Option<i64>,
    pub has_verified_total_live_channel_count: bool,
    pub compact_live_count_label: String,
    pub following_live_count: i64,
    pub recent_live_following: Vec<LiveChannelItem>,
    pub top_channels: Vec<LiveChannelItem>,
    pub following_live_rate: i64,
    pub following_total_viewers: i64,
    pub following_is_provisional: bool,
    pub popular_freshness: FreshnessDisplay,
    pub following_freshness: FreshnessDisplay,
    pub freshness_revision: u64,
    pub boot_hero: Option<BootHero>,
    pub boot_recommendations: Vec<ScoredChannel>,
    pub is_auto_refresh_active: bool,
}

impl HomeSnapshot {
    pub fn from_state(s: &HomeState) -> Self {
        Self {
            live_channels: s.live_channels.clone(),
            all_stat_channels_count: s.all_stat_channels.len(),
            following_channels: s.following_channels.clone(),
            recommended_channels: s.recommended_channels.clone(),
            live_channels_revision: s.live_channels_revision,
            all_stat_channels_revision: s.all_stat_channels_revision,
            category_channels_revision: s.category_channels_revision,
            following_channels_revision: s.following_channels_revision,
            is_loading: s.is_loading,
            is_loading_stats: s.is_loading_stats,
            is_loading_following: s.is_loading_following,
            error_message: s.error_message.clone(),
            stats_load_error: s.stats_load_error.clone(),
            stats_collection_quality: s.stats_collection_quality.clone(),
            stats_collection_progress: s.stats_collection_progress,
            stats_collected_count: s.stats_collected_count,
            stats_estimated_total: s.stats_estimated_total,
            needs_cookie_login: s.needs_cookie_login,
            following_load_error: s.following_load_error.clone(),
            has_more_channels: s.has_more_channels,
            total_viewers: s.total_viewers,
            total_live_channel_count: s.total_live_channel_count,
            verified_total_live_count: s.verified_total_live_count,
            has_verified_total_live_channel_count: s.has_verified_total_live_channel_count(),
            compact_live_count_label: HomeViewModel::compact_live_count_label(
                &s.stats_collection_quality,
                s.verified_total_live_count,
                s.stats_collected_count,
                s.all_stat_channels.len() as i64,
                s.live_channels.len() as i64,
            ),
            following_live_count: s.following_live_count,
            recent_live_following: s.recent_live_following.clone(),
            top_channels: s.top_channels.clone(),
            following_live_rate: s.following_live_rate,
            following_total_viewers: s.following_total_viewers,
            following_is_provisional: s.following_is_provisional,
            popular_freshness: s.popular_freshness.clone(),
            following_freshness: s.following_freshness.clone(),
            freshness_revision: s.freshness_revision,
            boot_hero: s.boot_hero.clone(),
            boot_recommendations: s.boot_recommendations.clone(),
            is_auto_refresh_active: s.is_auto_refresh_active,
        }
    }
}
