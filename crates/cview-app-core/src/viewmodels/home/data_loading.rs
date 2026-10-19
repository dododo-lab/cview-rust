//! 인기 첫 페이지·전수 카탈로그 결과 반영·팔로잉 폴 수신·캐시 복원.
//!
//! 원본: `Sources/CViewApp/ViewModels/HomeViewModel+DataLoading.swift`, 그리고
//! `HomeViewModel.swift` 의 `loadFromCache`·`overlaySnapshotFiles`.

use chrono::Utc;

use cview_core::models::live_catalog::{LiveCatalogQuality, LiveCatalogRow};
use cview_core::models::LiveChannelItem;
use cview_core::utils::catalog_sweep;
use cview_core::utils::following_poll::{
    FollowingPersistThrottle, FollowingPollFailure, FollowingPollReason, FollowingPublishThrottle,
};
use cview_core::utils::time::{interval_since, Date};
use cview_networking::response::{AllLivesCollectionResult, AllLivesStopReason};

use super::deps::{
    CatalogSweepReason, FollowingError, HomeFollowingOutcome, HomeFollowingResult,
    MenuChannelListSnapshot, RestoredCatalogRows, SnapshotLoadResult,
};
use super::promotion::{
    live_catalog_promotion, CatalogPreviousState, CatalogPromotionDecision, CatalogSweepOutcome,
    DataOrigin,
};
use super::{
    live_channel_item_from, HomeState, HomeViewModel, PublishRow, RestoredLiveCache,
    StatsCollectionQuality,
};

/// 재시작 이어 붙이기 검증을 통과한 결과(`StitchedCatalogVerifier.Stitched`).
#[derive(Debug, Clone, PartialEq)]
pub struct StitchedCatalog {
    pub rows: Vec<LiveCatalogRow>,
    pub replaced_by_restart: usize,
    pub pages: usize,
}

fn spawn_detached<F>(fut: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    if let Ok(h) = tokio::runtime::Handle::try_current() {
        h.spawn(fut);
    }
}

impl HomeState {
    /// 화면 수요가 전수 수집을 시작해야 하는가.
    pub fn needs_catalog_sweep_on_demand(&self, now: Date) -> bool {
        if self.catalog_rows_restore_pending {
            return false;
        }
        if self.all_stat_channels().is_empty() {
            return true;
        }
        if let Some(last) = self.deps.catalog.last_attempt_at() {
            if interval_since(&now, &last) < catalog_sweep::TRANSIENT_RETRY {
                return false;
            }
        }
        if self.stats_collection_quality.needs_partial_banner() {
            return true;
        }
        let Some(fresh) = self.all_stat_last_loaded_at.or(self.all_stat_cached_at) else {
            return true;
        };
        interval_since(&now, &fresh) >= catalog_sweep::CATEGORY_INTERVAL
    }

    pub(crate) fn apply_restored_catalog(&mut self, restored: RestoredCatalogRows) {
        let snapshot = restored.snapshot;
        let count = restored.items.len() as i64;
        self.set_all_stat_channels(restored.items);
        self.all_stat_cached_at = Some(snapshot.completed_at);
        if !HomeViewModel::should_apply_restored_stats_metadata(self.is_loading_stats) {
            self.verified_total_live_count = None;
            self.recompute_stats();
            return;
        }
        let exact = snapshot.quality.exact_count();
        let mut quality = HomeViewModel::restored_stats_quality(
            snapshot.completed_at,
            count,
            exact.map(|_| true),
            exact,
        );
        if let LiveCatalogQuality::Stitched { observed } = snapshot.quality {
            if observed == count {
                quality = StatsCollectionQuality::Complete {
                    at: snapshot.completed_at,
                    count: observed,
                };
            }
        }
        let complete = quality.is_complete();
        self.stats_collection_quality = quality;
        self.verified_total_live_count = if complete { exact } else { None };
        self.stats_collected_count = count;
        self.all_stat_last_loaded_at = complete.then_some(snapshot.completed_at);
        self.recompute_stats();
    }

    fn catalog_previous_state(
        &self,
        previous_quality: &StatsCollectionQuality,
    ) -> Option<CatalogPreviousState> {
        let StatsCollectionQuality::Complete { at, count } = previous_quality else {
            return None;
        };
        let quality = match self.verified_total_live_count {
            Some(exact) => LiveCatalogQuality::Verified { exact },
            None => LiveCatalogQuality::Stitched { observed: *count },
        };
        Some(CatalogPreviousState {
            quality,
            completed_at: *at,
        })
    }

    fn keep_previous_catalog_rows(
        &mut self,
        banner: String,
        load_error: Option<String>,
        observed: i64,
        now: Date,
    ) {
        self.stats_collected_count = self.all_stat_channels().len() as i64;
        self.verified_total_live_count = None;
        self.stats_collection_quality = StatsCollectionQuality::Partial {
            at: now,
            count: observed,
            message: banner,
        };
        self.stats_load_error = load_error;
        self.recompute_stats();
        tracing::debug!("» 전체 라이브 미검증 결과 — 이전 검증 통과본 유지: 관측={observed}개");
    }

    fn commit_catalog_rows(&mut self, items: Vec<LiveChannelItem>, now: Date) {
        self.all_stats_network_generation = self.all_stats_network_generation.wrapping_add(1);
        self.set_all_stat_channels(items);
        self.all_stat_last_loaded_at = Some(now);
    }

    /// `applyCatalogSweepResult` 의 동기 부분. 승격할 판정이면 돌려준다.
    pub fn apply_catalog_sweep_result_sync(
        &mut self,
        result: &AllLivesCollectionResult,
        previous_quality: &StatsCollectionQuality,
        now: Date,
    ) -> Option<CatalogPromotionDecision> {
        let items: Vec<LiveChannelItem> = result.items.iter().map(live_channel_item_from).collect();
        let total_viewers: i64 = items.iter().map(|i| i.viewer_count).sum();
        let n = items.len() as i64;
        let generation_before_commit = self.all_stats_network_generation;
        let decision = live_catalog_promotion::decide(
            &CatalogSweepOutcome::from_result(result),
            self.catalog_previous_state(previous_quality).as_ref(),
            &now,
        );
        match &result.stop_reason {
            AllLivesStopReason::ReachedEnd => match result.exact_live_count() {
                None => {
                    if let CatalogPromotionDecision::KeepPrevious { banner, .. } = &decision {
                        self.keep_previous_catalog_rows(
                            banner.clone(),
                            Some(banner.clone()),
                            n,
                            now,
                        );
                    } else {
                        self.commit_catalog_rows(items, now);
                        self.verified_total_live_count = None;
                        self.stats_collection_progress = 1.0;
                        self.stats_collected_count = n;
                        let message =
                            "전체 페이지는 확인했지만 총계 안정성 검증이 완료되지 않았습니다.";
                        self.stats_collection_quality = StatsCollectionQuality::Partial {
                            at: now,
                            count: n,
                            message: message.into(),
                        };
                        self.stats_load_error = Some(message.into());
                        self.recompute_stats();
                        self.all_stat_cached_at = Some(now);
                        tracing::error!("» 전체 라이브 미검증 terminal 방어: 수집={n}개");
                    }
                }
                Some(verified) => {
                    let verified = verified as i64;
                    self.commit_catalog_rows(items, now);
                    self.verified_total_live_count = Some(verified);
                    self.stats_collection_progress = 1.0;
                    self.stats_collected_count = verified;
                    self.stats_collection_quality = StatsCollectionQuality::Complete {
                        at: now,
                        count: verified,
                    };
                    self.recompute_stats();
                    tracing::debug!(
                        "» 전체 라이브 통계 수집 완료: {verified}개 확인, 총 {total_viewers}명, {}페이지, dedup {}",
                        result.received_pages,
                        result.deduplicated_count
                    );
                    self.all_stat_cached_at = Some(now);
                }
            },
            AllLivesStopReason::ReachedPageLimit => {
                if let CatalogPromotionDecision::KeepPrevious { banner, .. } = &decision {
                    self.keep_previous_catalog_rows(banner.clone(), None, n, now);
                } else {
                    self.commit_catalog_rows(items, now);
                    self.verified_total_live_count = None;
                    self.stats_collection_progress = 1.0;
                    self.stats_collected_count = n;
                    self.stats_collection_quality =
                        StatsCollectionQuality::Truncated { at: now, count: n };
                    self.recompute_stats();
                    tracing::warn!(
                        "» 전체 라이브 수집 상한 도달(maxPages): {n}개, 페이지={}",
                        result.received_pages
                    );
                    self.all_stat_cached_at = Some(now);
                }
            }
            AllLivesStopReason::CursorCycle { page } => {
                if self.all_stat_channels().is_empty() {
                    self.commit_catalog_rows(items, now);
                    self.verified_total_live_count = None;
                    self.stats_collected_count = n;
                    self.all_stat_cached_at = Some(now);
                } else {
                    self.stats_collected_count = self.all_stat_channels().len() as i64;
                }
                self.verified_total_live_count = None;
                let message = "페이지 커서가 반복되어 전체 수집을 중단했습니다.";
                self.stats_collection_quality = StatsCollectionQuality::Partial {
                    at: now,
                    count: n,
                    message: message.into(),
                };
                self.stats_load_error = Some(message.into());
                self.recompute_stats();
                tracing::warn!("» 전체 라이브 커서 순환: page={page}, 수집={n}개");
            }
            AllLivesStopReason::UnstableOrdering {
                attempts,
                duplicate_count,
            } => {
                let load_error =
                    "라이브 페이지 경계를 검증하지 못했습니다. 잠시 후 다시 시도해 주세요.";
                if let CatalogPromotionDecision::KeepPrevious { banner, .. } = &decision {
                    self.keep_previous_catalog_rows(
                        banner.clone(),
                        Some(load_error.into()),
                        n,
                        now,
                    );
                } else {
                    self.commit_catalog_rows(items, now);
                    self.stats_collected_count = n;
                    self.verified_total_live_count = None;
                    self.all_stat_cached_at = Some(now);
                    let message = "페이지 경계 검증 문제로 정확한 총계를 확정하지 못했습니다.";
                    self.stats_collection_quality = StatsCollectionQuality::Partial {
                        at: now,
                        count: n,
                        message: message.into(),
                    };
                    self.stats_load_error = Some(load_error.into());
                    self.recompute_stats();
                    tracing::warn!(
                        "» 전체 라이브 정렬 변동: attempts={attempts}, 수집={n}개, dedup={duplicate_count}"
                    );
                }
            }
            AllLivesStopReason::PartialFailure { page, message } => {
                if self.all_stat_channels().is_empty() {
                    self.commit_catalog_rows(items, now);
                    self.verified_total_live_count = None;
                    self.stats_collected_count = n;
                    self.all_stat_cached_at = Some(now);
                } else {
                    self.stats_collected_count = self.all_stat_channels().len() as i64;
                }
                self.verified_total_live_count = None;
                self.stats_collection_quality = StatsCollectionQuality::Partial {
                    at: now,
                    count: n,
                    message: message.clone(),
                };
                self.stats_load_error = Some(format!(
                    "일부 페이지({page}) 수집에 실패했습니다. 다시 시도해 주세요."
                ));
                self.recompute_stats();
                tracing::warn!("» 전체 라이브 부분 실패: page={page}, 수집={n}개, msg={message}");
            }
            AllLivesStopReason::Cancelled
            | AllLivesStopReason::Paused
            | AllLivesStopReason::ReachedBoundary { .. } => {
                if self.all_stat_channels().is_empty() {
                    self.stats_collection_quality = StatsCollectionQuality::Unknown;
                } else {
                    match previous_quality {
                        StatsCollectionQuality::Unknown | StatsCollectionQuality::Collecting => {
                            self.stats_collection_quality = StatsCollectionQuality::Truncated {
                                at: self.all_stat_cached_at.unwrap_or_else(Utc::now),
                                count: self.all_stat_channels().len() as i64,
                            };
                            self.verified_total_live_count = None;
                        }
                        other => self.stats_collection_quality = other.clone(),
                    }
                }
                tracing::debug!("» 전체 라이브 수집 취소: 페이지={}", result.received_pages);
            }
        }
        (self.all_stats_network_generation != generation_before_commit).then_some(decision)
    }

    pub fn apply_stitched_catalog(&mut self, stitched: &StitchedCatalog, now: Date) {
        let items: Vec<LiveChannelItem> = stitched.rows.iter().map(|r| r.item()).collect();
        let n = items.len() as i64;
        self.all_stats_network_generation = self.all_stats_network_generation.wrapping_add(1);
        self.set_all_stat_channels(items);
        self.all_stat_last_loaded_at = Some(now);
        self.all_stat_cached_at = Some(now);
        self.verified_total_live_count = None;
        self.stats_collection_progress = 1.0;
        self.stats_collected_count = n;
        self.stats_collection_quality = StatsCollectionQuality::Complete { at: now, count: n };
        self.stats_load_error = None;
        self.recompute_stats();
        tracing::debug!(
            "» 전체 라이브 이어 붙인 수집 완료: {n}개, 재시작 병합 {}건, {}페이지",
            stitched.replaced_by_restart,
            stitched.pages
        );
    }

    // MARK: 팔로잉

    pub(crate) fn begin_following_load(&mut self) {
        self.following_load_depth += 1;
        if !self.is_loading_following {
            self.is_loading_following = true;
        }
    }

    pub(crate) fn end_following_load(&mut self) {
        self.following_load_depth = (self.following_load_depth - 1).max(0);
        if self.following_load_depth == 0 && self.is_loading_following {
            self.is_loading_following = false;
        }
    }

    /// 폴 결과를 반영한다. `persist` 면 스냅샷 저장을 띄운다.
    pub fn apply_following_poll(&mut self, result: &HomeFollowingResult, persist: bool) {
        let response = result.channels.clone();
        let stats = HomeViewModel::compute_following_stats(&response);
        self.set_following_channels(response.clone());
        let live = stats.live_count;
        self.apply_following_stats(stats);
        tracing::debug!(
            "팔로잉 채널 반영: {}개, 라이브={live}, 사유={}",
            response.len(),
            result.reason.capture_path()
        );
        self.set_following_origin(DataOrigin::Network);
        self.set_following_cached_at(Some(result.fetched_at));
        if !persist {
            return;
        }
        self.last_following_persist_at = Some(result.fetched_at);
        let auth_generation = self.following_auth_generation;
        let scope = self.following_cache_scope.to_string();
        let has_store = self.has_data_store;
        let snapshots = self.deps.snapshots.clone();
        let weak = self.this.clone();
        let ts = result.fetched_at;
        spawn_detached(async move {
            let Some(shared) = weak.upgrade() else { return };
            if shared.obs.read().following_auth_generation != auth_generation || !has_store {
                return;
            }
            drop(shared);
            snapshots
                .save_channel_list(
                    "following",
                    &scope,
                    MenuChannelListSnapshot {
                        channels: response,
                        fetched_at: ts,
                    },
                )
                .await;
        });
    }

    pub fn apply_following_poll_failure(&mut self, failure: &FollowingPollFailure<FollowingError>) {
        if failure.error.is_unauthorized {
            self.needs_cookie_login = true;
            self.following_load_error = None;
            tracing::warn!(
                "팔로잉 조회 실패: 쿠키 인증 필요 — 네이버 로그인으로 NID 쿠키를 획득하세요"
            );
            return;
        }
        if self.following_channels().is_empty() {
            self.following_load_error = Some(failure.error.message.clone());
        }
        tracing::error!(
            "팔로잉 채널 로드 실패({}): {}",
            failure.reason.capture_path(),
            failure.error.message
        );
    }

    pub fn invalidate_following_authentication(&mut self) {
        self.following_cache_allowed = false;
        self.boot_following = None;
        self.reset_boot_extras();
        self.following_is_provisional = false;
        self.following_auth_generation = self.following_auth_generation.wrapping_add(1);
        self.following_network_generation = self.following_network_generation.wrapping_add(1);
        self.deps.following.reset_for_account_boundary();
        self.following_publish_throttle.reset();
        self.last_following_persist_at = None;
        self.set_following_channels(Vec::new());
        self.following_live_count = 0;
        self.recent_live_following = Vec::new();
        self.following_total_viewers = 0;
        self.following_live_rate = 0;
        self.set_following_cached_at(None);
        self.following_load_error = None;
        self.is_loading_following = false;
    }
}

impl HomeViewModel {
    /// 인기 첫 페이지.
    pub async fn load_live_channels(&self, persist_cache: bool) {
        let popular = {
            let mut s = self.lock();
            if s.is_loading {
                return;
            }
            s.is_loading = true;
            s.error_message = None;
            s.next_live_cursor = None;
            s.deps.popular.clone()
        };
        let fetched = popular.top_lives_with_age(Self::PAGE_SIZE).await;
        let mut s = self.lock();
        match fetched {
            Ok(aged) => {
                let items: Vec<LiveChannelItem> =
                    aged.value.data.iter().map(live_channel_item_from).collect();
                s.live_channels_network_generation =
                    s.live_channels_network_generation.wrapping_add(1);
                s.set_live_channels(items.clone());
                s.next_live_cursor = aged.value.page.as_ref().and_then(|p| p.next);
                s.has_more_channels = s.next_live_cursor.is_some();
                s.refresh_popular_derived();
                tracing::debug!("Loaded {} live channels", items.len());
                let now = aged.fetched_at.min(Utc::now());
                s.set_popular_origin(DataOrigin::Network);
                s.set_live_channels_cached_at(Some(now));
                if persist_cache && s.has_data_store {
                    let snapshots = s.deps.snapshots.clone();
                    spawn_detached(async move {
                        snapshots
                            .save_channel_list(
                                "popular",
                                "public",
                                MenuChannelListSnapshot {
                                    channels: items,
                                    fetched_at: now,
                                },
                            )
                            .await;
                    });
                    s.mark_cache_persisted();
                }
            }
            Err(e) => {
                tracing::error!("Failed to load live channels: {e}");
                s.error_message = Some(e);
            }
        }
        s.is_loading = false;
    }

    pub async fn load_all_stats_channels(&self) {
        let catalog = self.deps().catalog;
        catalog.run_manual().await;
    }

    pub fn needs_catalog_sweep_on_demand(&self, now: Date) -> bool {
        self.read().needs_catalog_sweep_on_demand(now)
    }

    /// 전수 행 파일을 처음 필요할 때 되살린다.
    pub async fn load_catalog_rows_if_needed(&self) {
        let (captured, catalog) = {
            let mut s = self.lock();
            if !s.all_stat_channels().is_empty() {
                s.catalog_rows_restore_pending = false;
                return;
            }
            (s.all_stats_network_generation, s.deps.catalog.clone())
        };
        let restored = catalog.load_persisted_rows().await;
        let mut s = self.lock();
        s.catalog_rows_restore_pending = false;
        let is_none = restored.is_none();
        if let Some(restored) = restored.filter(|_| {
            HomeViewModel::should_apply_restored_source(
                s.all_stat_channels().len(),
                true,
                captured,
                s.all_stats_network_generation,
            )
        }) {
            s.apply_restored_catalog(restored);
        } else if is_none
            && s.all_stat_channels().is_empty()
            && catalog.has_summary()
            && !catalog.is_sweeping()
        {
            catalog.request_sweep(CatalogSweepReason::RowsMissing);
            return;
        }
        if catalog.demand_starts_sweep() && s.needs_catalog_sweep_on_demand(Utc::now()) {
            catalog.request_sweep(CatalogSweepReason::Demand);
        }
    }

    /// 카탈로그 저장소의 `onSweepResult`.
    pub async fn apply_catalog_sweep_result(
        &self,
        result: &AllLivesCollectionResult,
        previous_quality: &StatsCollectionQuality,
        reason: CatalogSweepReason,
    ) {
        let now = Utc::now();
        let (decision, catalog) = {
            let mut s = self.lock();
            let d = s.apply_catalog_sweep_result_sync(result, previous_quality, now);
            (d, s.deps.catalog.clone())
        };
        if let Some(decision) = decision {
            catalog.promote(&decision, &result.items, now, reason).await;
        }
    }

    /// 카탈로그 저장소의 `onStitchedResult`.
    pub fn apply_stitched_catalog(&self, stitched: &StitchedCatalog) {
        self.lock().apply_stitched_catalog(stitched, Utc::now());
    }

    pub async fn load_following_channels(&self, invalidate_thumbnails: bool) {
        let (deps, profile_urls) = {
            let mut s = self.lock();
            s.begin_following_load();
            let urls: Vec<String> = s
                .following_channels()
                .iter()
                .filter_map(|c| c.channel_image_url.clone())
                .filter(|u| !u.is_empty())
                .collect();
            (s.deps.clone(), urls)
        };
        if invalidate_thumbnails {
            deps.images.invalidate_all_thumbnails().await;
            deps.images.invalidate(&profile_urls).await;
        }
        let reason = if invalidate_thumbnails {
            FollowingPollReason::Manual {
                invalidate_thumbnails: true,
            }
        } else {
            FollowingPollReason::ScreenEntry
        };
        deps.following.poll(reason).await;
        self.lock().end_following_load();
    }

    pub async fn poll_following(&self, reason: FollowingPollReason) -> Option<HomeFollowingResult> {
        let following = {
            let mut s = self.lock();
            s.begin_following_load();
            s.deps.following.clone()
        };
        let r = following.poll(reason).await;
        self.lock().end_following_load();
        r
    }

    /// `FollowingStore` 싱크.
    pub fn receive_following_poll(&self, outcome: &HomeFollowingOutcome) {
        let mut s = self.lock();
        if outcome.generation() != s.deps.following.generation() {
            return;
        }
        match outcome {
            cview_core::utils::following_poll::FollowingPollOutcome::Success(result) => {
                if s.needs_cookie_login {
                    s.needs_cookie_login = false;
                }
                if s.following_load_error.is_some() {
                    s.following_load_error = None;
                }
                if let Some(cb) = s.on_following_loaded.clone() {
                    cb();
                }
                s.following_network_generation = s.following_network_generation.wrapping_add(1);
                s.following_is_provisional = false;
                let current: Vec<PublishRow> =
                    s.following_channels().iter().map(PublishRow).collect();
                let next: Vec<PublishRow> = result.channels.iter().map(PublishRow).collect();
                let mut throttle: FollowingPublishThrottle = s.following_publish_throttle.clone();
                let publish =
                    throttle.should_publish(&current, &next, result.reason, result.fetched_at);
                drop(current);
                drop(next);
                s.following_publish_throttle = throttle;
                if !publish {
                    return;
                }
                let persist = FollowingPersistThrottle::should_persist(
                    result.reason,
                    s.last_following_persist_at.as_ref(),
                    &result.fetched_at,
                );
                s.apply_following_poll(result, persist);
            }
            cview_core::utils::following_poll::FollowingPollOutcome::Failure(f) => {
                s.apply_following_poll_failure(f)
            }
        }
    }

    pub fn invalidate_following_authentication(&self) {
        self.lock().invalidate_following_authentication();
    }

    /// 사용자 새로고침 — 전수 수집 요청 + 인기·팔로잉 동시.
    pub async fn refresh(&self) {
        self.deps()
            .catalog
            .request_sweep(CatalogSweepReason::Manual);
        futures_util::join!(
            self.load_live_channels(true),
            self.load_following_channels(true)
        );
    }

    pub async fn hydrate_catalog_first_paint(&self) {
        let catalog = self.deps().catalog;
        catalog.hydrate_summary().await;
        catalog.hydrate_checkpoint().await;
        if catalog.usable_summary().is_some() {
            self.lock().recompute_stats();
        }
    }

    /// `dataStore` 를 붙인다(Swift `didSet`) — 행 복원 대기 표시 후 캐시 복원.
    pub async fn attach_data_store(&self) {
        {
            let mut s = self.lock();
            s.has_data_store = true;
            s.catalog_rows_restore_pending = true;
        }
        self.load_from_cache().await;
    }

    /// 스냅샷 파일에서 인기·팔로잉을 되살린다. 네트워크가 먼저 채웠으면 덮지 않는다.
    pub async fn load_from_cache(&self) {
        self.hydrate_catalog_first_paint().await;
        let (captured_live, captured_following) = {
            let s = self.read();
            (
                s.live_channels_network_generation,
                s.following_network_generation,
            )
        };
        let restored = self
            .overlay_snapshot_files(RestoredLiveCache::default())
            .await;
        let mut s = self.lock();
        let should_restore_live = Self::should_apply_restored_source(
            s.live_channels().len(),
            restored.has_live_channels_snapshot,
            captured_live,
            s.live_channels_network_generation,
        );
        let should_restore_following = s.following_cache_allowed
            && restored.following_scope == Some(s.following_cache_scope)
            && Self::should_apply_restored_source(
                s.following_channels().len(),
                restored.has_following_channels_snapshot,
                captured_following,
                s.following_network_generation,
            );
        if should_restore_live {
            s.set_live_channels(restored.live_channels);
            s.set_popular_origin(DataOrigin::Snapshot);
            s.set_live_channels_cached_at(restored.live_channels_cached_at);
        }
        if should_restore_following {
            s.set_following_channels(restored.following_channels);
            s.set_following_origin(DataOrigin::Snapshot);
            s.set_following_cached_at(restored.following_cached_at);
        }
        tracing::info!(
            "캐시 복원: 라이브 {}개, 팔로잉 {}개",
            s.live_channels().len(),
            s.following_channels().len()
        );
        s.recompute_stats();
        let stats = Self::compute_following_stats(s.following_channels());
        s.apply_following_stats(stats);
    }

    pub async fn overlay_snapshot_files(&self, blobs: RestoredLiveCache) -> RestoredLiveCache {
        let snapshots = self.deps().snapshots;
        let mut restored = blobs;
        match snapshots
            .load_channel_list("popular", "public", Self::PUBLIC_SNAPSHOT_RESTORE_MAX_AGE)
            .await
        {
            SnapshotLoadResult::Fresh(popular, _) => {
                restored.live_channels = popular.channels;
                restored.has_live_channels_snapshot = true;
                restored.has_popular_file = true;
                restored.live_channels_cached_at = Some(popular.fetched_at);
            }
            SnapshotLoadResult::Expired(fetched_at) => {
                restored.live_channels = Vec::new();
                restored.has_live_channels_snapshot = true;
                restored.has_popular_file = true;
                restored.live_channels_cached_at = Some(fetched_at);
            }
            SnapshotLoadResult::Missing | SnapshotLoadResult::Rejected => {}
        }
        self.overlay_following_snapshot(restored).await
    }

    async fn overlay_following_snapshot(&self, blobs: RestoredLiveCache) -> RestoredLiveCache {
        let (allowed, scope, snapshots) = {
            let s = self.read();
            (
                s.following_cache_allowed,
                s.following_cache_scope,
                s.deps.snapshots.clone(),
            )
        };
        if !allowed {
            return blobs;
        }
        let mut restored = blobs;
        match snapshots
            .load_channel_list(
                "following",
                &scope.to_string(),
                Self::FOLLOWING_SNAPSHOT_RESTORE_MAX_AGE,
            )
            .await
        {
            SnapshotLoadResult::Fresh(following, _) => {
                restored.following_channels = following.channels;
                restored.has_following_channels_snapshot = true;
                restored.following_scope = Some(scope);
                restored.following_cached_at = Some(following.fetched_at);
                restored.has_following_file = true;
            }
            SnapshotLoadResult::Expired(_) => {
                restored.following_channels = Vec::new();
                restored.has_following_file = true;
                restored.has_following_channels_snapshot = false;
                restored.following_scope = None;
                restored.following_cached_at = None;
            }
            SnapshotLoadResult::Missing | SnapshotLoadResult::Rejected => {}
        }
        restored
    }
}
