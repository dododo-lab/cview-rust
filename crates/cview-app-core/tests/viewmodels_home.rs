//! HomeViewModel·카탈로그 승격·추천 테스트.
//!
//! 원본: `Tests/CViewAppTests/HomeViewModelStatsTests.swift` · `HomeStatsCacheQualityTests.swift` ·
//! `LiveCatalogPromotionTests.swift` · `HomeBootHydrateTests.swift`(fake 저장소로) ·
//! `HomeViewModelRevisionTests.swift`·`FreshnessDisplayGuardTests.swift`(일부).

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, FixedOffset, TimeZone, Utc};

use cview_app_core::viewmodels::home::deps::{
    FollowingError, HomeDeps, MemoryMenuSnapshots, MenuChannelListSnapshot, NullCatalog,
    NullFollowingPoller, PopularLiveSource, SnapshotLoadResult,
};
use cview_app_core::viewmodels::home::promotion::{
    verified_live_count_label_in, CatalogPreviousState, CatalogPromotionDecision,
    CatalogSweepOutcome, DataOrigin, FreshnessDisplay, LiveCatalogPromotion,
};
use cview_app_core::viewmodels::home::recommendation::{self, RecommendationInputs};
use cview_app_core::viewmodels::home::{HomeViewModel, StatsCollectionQuality};
use cview_core::models::live_catalog::LiveCatalogQuality;
use cview_core::models::{ChannelInfo, LiveChannelItem, LiveInfo};
use cview_core::utils::catalog_sweep;
use cview_core::utils::data_freshness::FreshnessPolicy;
use cview_core::utils::following_poll::{
    FollowingPollFailure, FollowingPollOutcome, FollowingPollReason, FollowingPollResult,
};
use cview_networking::response::{
    AllLivesCollectionResult, AllLivesStopReason, LivePageCursor, LivePageInfo,
};
use cview_networking::{AgedResponse, PagedContent};

fn date(secs: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(secs, 0).unwrap()
}

fn channel(id: &str, viewers: i64, is_live: bool) -> LiveChannelItem {
    LiveChannelItem {
        id: id.into(),
        channel_name: format!("ch_{id}"),
        channel_image_url: None,
        live_title: format!("title_{id}"),
        viewer_count: viewers,
        category_name: Some("GAME".into()),
        category_type: Some("GAME".into()),
        thumbnail_url: None,
        channel_id: id.into(),
        is_live,
        open_date: None,
        verified_badge: None,
    }
}

fn live_info(id: i64, viewers: i64) -> LiveInfo {
    let mut info = LiveInfo::new(id, format!("live-{id}"));
    info.concurrent_user_count = viewers;
    info.channel = Some(ChannelInfo::new(format!("c{id}"), format!("채널{id}")));
    info
}

struct FakePopular {
    count: i64,
    fetched_at: DateTime<Utc>,
    fail: bool,
}

#[async_trait]
impl PopularLiveSource for FakePopular {
    async fn top_lives_with_age(
        &self,
        size: i64,
    ) -> Result<AgedResponse<PagedContent<LiveInfo>>, String> {
        if self.fail {
            return Err("네트워크 오류".into());
        }
        let data: Vec<LiveInfo> = (1..=self.count.min(size))
            .map(|i| live_info(i, 1_000 - i))
            .collect();
        let page = LivePageInfo::new(
            Some(LivePageCursor {
                concurrent_user_count: Some(1),
                live_id: Some(1),
            }),
            None,
        );
        Ok(AgedResponse {
            value: PagedContent::new(data.len() as i64, None, data, Some(page)),
            fetched_at: self.fetched_at,
            is_stale: false,
        })
    }
}

fn make_vm() -> (
    HomeViewModel,
    Arc<NullCatalog>,
    Arc<MemoryMenuSnapshots>,
    Arc<NullFollowingPoller>,
) {
    let catalog = Arc::new(NullCatalog::default());
    let snapshots = Arc::new(MemoryMenuSnapshots::default());
    let following = Arc::new(NullFollowingPoller::default());
    let mut deps = HomeDeps::with_popular(Arc::new(FakePopular {
        count: 30,
        fetched_at: Utc::now(),
        fail: false,
    }));
    deps.catalog = catalog.clone();
    deps.snapshots = snapshots.clone();
    deps.following = following.clone();
    (HomeViewModel::new(deps), catalog, snapshots, following)
}

// MARK: - 순수 통계 (HomeViewModelStatsTests)

#[test]
fn empty_inputs_have_zero_totals() {
    let r = HomeViewModel::compute_channel_stats(&[], &[]);
    assert_eq!(r.total_live_channel_count, 0);
    assert_eq!(r.total_viewers, 0);
}

#[test]
fn falls_back_to_live_and_dedupes_by_id() {
    let live = vec![
        channel("a", 100, true),
        channel("a", 999, true),
        channel("b", 200, true),
    ];
    let r = HomeViewModel::compute_channel_stats(&[], &live);
    assert_eq!(r.total_live_channel_count, 2);
    assert_eq!(r.total_viewers, 300);
}

#[test]
fn catalog_totals_ignore_popular_list() {
    let rows: Vec<_> = (0..8)
        .map(|i| channel(&format!("c{i}"), i * 10, i != 7))
        .collect();
    for popular in [vec![], vec![rows[0].clone()]] {
        let r = HomeViewModel::compute_channel_stats(&rows, &popular);
        assert_eq!(r.total_live_channel_count, 8);
        assert_eq!(r.total_viewers, 280);
    }
}

#[test]
fn popular_top_six_by_viewers() {
    let live: Vec<_> = (0..9).map(|i| channel(&format!("p{i}"), i, true)).collect();
    let ids: Vec<_> = HomeViewModel::popular_top_channels(&live)
        .into_iter()
        .map(|c| c.channel_id)
        .collect();
    assert_eq!(ids, ["p8", "p7", "p6", "p5", "p4", "p3"]);
}

#[test]
fn following_stats() {
    let empty = HomeViewModel::compute_following_stats(&[]);
    assert_eq!(
        (empty.live_count, empty.total_viewers, empty.live_rate),
        (0, 0, 0)
    );
    assert!(empty.recent_live.is_empty());
    let channels = vec![
        channel("a", 100, true),
        channel("b", 200, true),
        channel("c", 0, false),
        channel("d", 0, false),
    ];
    let r = HomeViewModel::compute_following_stats(&channels);
    assert_eq!((r.live_count, r.total_viewers, r.live_rate), (2, 300, 50));
    let ten: Vec<_> = (0..10)
        .map(|i| channel(&format!("ch{i}"), 100, true))
        .collect();
    assert_eq!(
        HomeViewModel::compute_following_stats(&ten)
            .recent_live
            .len(),
        6
    );
}

// MARK: - 캐시 품질 (HomeStatsCacheQualityTests)

#[test]
fn legacy_cache_restores_as_truncated() {
    let q = HomeViewModel::restored_stats_quality(date(1_700_000_000), 2_000, None, None);
    assert!(!q.is_complete());
    assert!(q.needs_partial_banner());
    assert_eq!(
        q,
        StatsCollectionQuality::Truncated {
            at: date(1_700_000_000),
            count: 2_000
        }
    );
}

#[test]
fn complete_requires_matching_exact_count() {
    let at = date(1_700_000_000);
    let complete = HomeViewModel::restored_stats_quality(at, 3_700, Some(true), Some(3_700));
    assert_eq!(
        complete,
        StatsCollectionQuality::Complete { at, count: 3_700 }
    );
    let missing = HomeViewModel::restored_stats_quality(at, 3_700, Some(true), None);
    assert!(!missing.is_complete() && missing.needs_partial_banner());
    let mixed = HomeViewModel::restored_stats_quality(at, 3_700, Some(true), Some(3_786));
    assert!(!mixed.is_complete() && mixed.needs_partial_banner());
    let withdrawn = HomeViewModel::restored_stats_quality(at, 1_015, Some(false), Some(1_116));
    assert_eq!(
        withdrawn,
        StatsCollectionQuality::Truncated { at, count: 1_015 }
    );
}

#[test]
fn incomplete_retry_intervals_follow_policy() {
    assert_eq!(catalog_sweep::TRANSIENT_RETRY, 90.0);
    assert_eq!(catalog_sweep::ORDERING_RETRY, 600.0);
    let now = date(1_700_000_100);
    let previous = CatalogPreviousState {
        quality: LiveCatalogQuality::Verified { exact: 10 },
        completed_at: now - ChronoDuration::seconds(60),
    };
    let cases = [
        (
            AllLivesStopReason::PartialFailure {
                page: 2,
                message: "x".into(),
            },
            90.0,
        ),
        (AllLivesStopReason::CursorCycle { page: 3 }, 90.0),
        (
            AllLivesStopReason::UnstableOrdering {
                attempts: 3,
                duplicate_count: 1,
            },
            600.0,
        ),
        (AllLivesStopReason::ReachedPageLimit, 600.0),
    ];
    for (reason, expected) in cases {
        let d = LiveCatalogPromotion::decide(
            &CatalogSweepOutcome::new(reason, None, 5),
            Some(&previous),
            now,
        );
        let CatalogPromotionDecision::KeepPrevious { retry_after, .. } = d else {
            panic!("keepPrevious 여야 한다");
        };
        assert_eq!(retry_after, expected);
    }
}

#[test]
fn demand_sweep_decision_respects_spacing() {
    let (vm, catalog, _, _) = make_vm();
    let now = date(1_700_000_100);
    assert!(
        vm.needs_catalog_sweep_on_demand(now),
        "비었으면 무조건 받는다"
    );
    {
        let mut s = vm.lock();
        s.set_all_stat_channels(vec![channel("a", 1, true)]);
        s.all_stat_last_loaded_at = Some(now - ChronoDuration::seconds(30));
        s.stats_collection_quality = StatsCollectionQuality::Complete {
            at: now - ChronoDuration::seconds(30),
            count: 1,
        };
    }
    assert!(!vm.needs_catalog_sweep_on_demand(now));
    vm.lock().all_stat_last_loaded_at = Some(now - ChronoDuration::seconds(600));
    assert!(
        vm.needs_catalog_sweep_on_demand(now),
        "10분이 지난 본문은 받는다"
    );
    vm.lock().stats_collection_quality = StatsCollectionQuality::Partial {
        at: now,
        count: 1,
        message: "x".into(),
    };
    *catalog.last_attempt_at.lock().unwrap() = Some(now - ChronoDuration::seconds(89));
    assert!(!vm.needs_catalog_sweep_on_demand(now));
    *catalog.last_attempt_at.lock().unwrap() = Some(now - ChronoDuration::seconds(90));
    assert!(vm.needs_catalog_sweep_on_demand(now));
}

#[test]
fn restore_commit_rejects_fresh_generation() {
    assert!(!HomeViewModel::should_apply_restored_source(
        1_116, true, 3, 3
    ));
    assert!(HomeViewModel::should_apply_restored_source(0, true, 3, 3));
    assert!(!HomeViewModel::should_apply_restored_source(0, false, 3, 3));
    assert!(!HomeViewModel::should_apply_restored_source(0, true, 3, 4));
    assert!(!HomeViewModel::should_apply_restored_stats_metadata(true));
    assert!(HomeViewModel::should_apply_restored_stats_metadata(false));
}

#[test]
fn compact_count_preserves_quality() {
    let now = date(1_700_000_000);
    assert_eq!(
        HomeViewModel::compact_live_count_label(
            &StatsCollectionQuality::Complete {
                at: now,
                count: 1_102
            },
            Some(1_102),
            1_102,
            1_102,
            20
        ),
        "1102 확인"
    );
    assert_eq!(
        HomeViewModel::compact_live_count_label(
            &StatsCollectionQuality::Partial {
                at: now,
                count: 1_047,
                message: "drift".into()
            },
            None,
            1_047,
            1_047,
            20
        ),
        "1047 관측"
    );
    assert_eq!(
        HomeViewModel::compact_live_count_label(
            &StatsCollectionQuality::Collecting,
            None,
            700,
            1_015,
            20
        ),
        "700 수집"
    );
    assert_eq!(
        HomeViewModel::compact_live_count_label(&StatsCollectionQuality::Unknown, None, 0, 0, 0),
        "집계 중"
    );
}

#[test]
fn catalog_count_labels() {
    let tz = FixedOffset::east_opt(9 * 3600).unwrap();
    let at = tz
        .with_ymd_and_hms(2026, 9, 1, 7, 5, 0)
        .unwrap()
        .with_timezone(&Utc);
    let label = |q: LiveCatalogQuality, at: Option<&DateTime<Utc>>| {
        LiveCatalogPromotion::catalog_count_label_in(&q, at.copied(), &tz)
    };
    assert_eq!(
        label(LiveCatalogQuality::Verified { exact: 1_102 }, Some(&at)),
        "1102개 확인 · 07:05 집계"
    );
    assert_eq!(
        label(LiveCatalogQuality::Stitched { observed: 1_050 }, Some(&at)),
        "1050개 관측 · 07:05 집계"
    );
    assert_eq!(
        label(
            LiveCatalogQuality::Observed {
                count: 900,
                reason: "page-limit".into()
            },
            Some(&at)
        ),
        "900개 관측 · 07:05 집계"
    );
    assert_eq!(
        label(
            LiveCatalogQuality::Observed {
                count: 900,
                reason: "x".into()
            },
            None
        ),
        "900개 관측"
    );
    assert_eq!(
        label(LiveCatalogQuality::Verified { exact: 3 }, None),
        "3개 확인"
    );
    let observed = tz
        .with_ymd_and_hms(2026, 9, 1, 12, 34, 0)
        .unwrap()
        .with_timezone(&Utc);
    assert_eq!(
        verified_live_count_label_in(1_102, Some(observed), &tz),
        "1102개 확인 · 12:34 집계"
    );
}

// MARK: - 승격 판정 행렬 (LiveCatalogPromotionTests)

fn previous_states(now: DateTime<Utc>) -> Vec<(Option<CatalogPreviousState>, bool)> {
    let st = |q: LiveCatalogQuality, ago: i64| {
        Some(CatalogPreviousState {
            quality: q,
            completed_at: now - ChronoDuration::seconds(ago),
        })
    };
    vec![
        (None, false),
        (st(LiveCatalogQuality::Verified { exact: 10 }, 600), true),
        (st(LiveCatalogQuality::Stitched { observed: 10 }, 600), true),
        (st(LiveCatalogQuality::Verified { exact: 10 }, 3_600), false),
        (
            st(
                LiveCatalogQuality::Observed {
                    count: 10,
                    reason: "x".into(),
                },
                60,
            ),
            false,
        ),
    ]
}

#[test]
fn promotion_matrix() {
    let now = date(1_700_000_000);
    let decide = |r: AllLivesStopReason, exact: Option<i64>, p: &Option<CatalogPreviousState>| {
        LiveCatalogPromotion::decide(&CatalogSweepOutcome::new(r, exact, 5), p.as_ref(), now)
    };
    for (prev, keepable) in previous_states(now) {
        let d = decide(AllLivesStopReason::ReachedEnd, Some(7), &prev);
        assert_eq!(d, CatalogPromotionDecision::Promote { exact: 7 });
        assert_eq!(d.quality(), Some(LiveCatalogQuality::Verified { exact: 7 }));
        assert!(d.records_growth_baseline());
        for r in [
            AllLivesStopReason::Cancelled,
            AllLivesStopReason::Paused,
            AllLivesStopReason::ReachedBoundary { live_id: 9 },
        ] {
            let d = decide(r, None, &prev);
            assert_eq!(d, CatalogPromotionDecision::Discard);
            assert!(d.quality().is_none() && !d.records_growth_baseline());
        }
        let cases = [
            (AllLivesStopReason::ReachedEnd, 600.0, "unverified-terminal"),
            (AllLivesStopReason::ReachedPageLimit, 600.0, "page-limit"),
            (
                AllLivesStopReason::CursorCycle { page: 4 },
                90.0,
                "cursor-cycle",
            ),
            (
                AllLivesStopReason::UnstableOrdering {
                    attempts: 3,
                    duplicate_count: 2,
                },
                600.0,
                "unstable-ordering",
            ),
            (
                AllLivesStopReason::PartialFailure {
                    page: 3,
                    message: "boom".into(),
                },
                90.0,
                "partial-failure",
            ),
        ];
        for (r, retry, tag) in cases {
            let d = decide(r, None, &prev);
            if keepable {
                let CatalogPromotionDecision::KeepPrevious {
                    banner,
                    retry_after,
                } = &d
                else {
                    panic!("keepPrevious 여야 한다: {d:?}");
                };
                assert!(!banner.is_empty());
                assert_eq!(*retry_after, retry);
                assert!(d.quality().is_none());
            } else {
                assert_eq!(
                    d,
                    CatalogPromotionDecision::Provisional {
                        reason: tag.into(),
                        observed: 5
                    }
                );
            }
            assert!(!d.records_growth_baseline());
        }
    }
}

#[test]
fn keep_window_boundary_and_wording() {
    let now = date(1_700_000_000);
    let prev = |ago| CatalogPreviousState {
        quality: LiveCatalogQuality::Verified { exact: 1 },
        completed_at: now - ChronoDuration::seconds(ago),
    };
    let d = |p: CatalogPreviousState| {
        LiveCatalogPromotion::decide(
            &CatalogSweepOutcome::new(AllLivesStopReason::ReachedPageLimit, None, 5),
            Some(&p),
            now,
        )
    };
    assert!(matches!(
        d(prev(1_799)),
        CatalogPromotionDecision::KeepPrevious { .. }
    ));
    assert_eq!(
        d(prev(1_800)),
        CatalogPromotionDecision::Provisional {
            reason: "page-limit".into(),
            observed: 5
        }
    );
    let partial = LiveCatalogPromotion::decide(
        &CatalogSweepOutcome::new(
            AllLivesStopReason::PartialFailure {
                page: 3,
                message: "x".into(),
            },
            None,
            5,
        ),
        Some(&prev(600)),
        now,
    );
    assert_eq!(
        partial,
        CatalogPromotionDecision::KeepPrevious {
            banner: "일부 페이지(3) 수집에 실패했습니다. 다시 시도해 주세요.".into(),
            retry_after: catalog_sweep::TRANSIENT_RETRY
        }
    );
    let o = |r, exact| CatalogSweepOutcome::new(r, exact, 1);
    assert_eq!(
        o(AllLivesStopReason::ReachedEnd, Some(3)).event_outcome(),
        "verified"
    );
    assert_eq!(
        o(AllLivesStopReason::ReachedEnd, Some(3)).event_reason(),
        None
    );
    assert_eq!(
        o(AllLivesStopReason::ReachedEnd, None).event_outcome(),
        "unverified"
    );
    assert_eq!(
        o(AllLivesStopReason::Cancelled, None).event_outcome(),
        "cancelled"
    );
    assert!(!o(
        AllLivesStopReason::PartialFailure {
            page: 1,
            message: "x".into()
        },
        None
    )
    .completed_normally());
}

// MARK: - 수집 결과 반영

fn sweep_result(
    stop: AllLivesStopReason,
    n: i64,
    verified: Option<usize>,
) -> AllLivesCollectionResult {
    AllLivesCollectionResult {
        items: (1..=n).map(|i| live_info(i, 10)).collect(),
        estimated_total: None,
        requested_pages: 1,
        received_pages: 1,
        deduplicated_count: 0,
        stop_reason: stop,
        verified_observed_count: verified,
        stable_cursor_ordering_verified: true,
        resume_point: None,
        first_live_id: None,
    }
}

#[test]
fn sweep_result_verified_completion_commits_rows() {
    let (vm, ..) = make_vm();
    let now = Utc::now();
    let decision = vm.lock().apply_catalog_sweep_result_sync(
        &sweep_result(AllLivesStopReason::ReachedEnd, 5, Some(5)),
        &StatsCollectionQuality::Collecting,
        now,
    );
    assert_eq!(
        decision,
        Some(CatalogPromotionDecision::Promote { exact: 5 })
    );
    let s = vm.read();
    assert_eq!(s.all_stat_channels().len(), 5);
    assert_eq!(s.verified_total_live_count, Some(5));
    assert_eq!(s.total_live_channel_count, 5);
    assert_eq!(s.total_viewers, 50);
    assert!(s.has_verified_total_live_channel_count());
}

#[test]
fn sweep_result_keeps_previous_verified_rows_on_unverified_terminal() {
    let (vm, ..) = make_vm();
    let now = Utc::now();
    let earlier = now - ChronoDuration::seconds(60);
    {
        let mut s = vm.lock();
        s.set_all_stat_channels(vec![channel("a", 1, true), channel("b", 1, true)]);
        s.verified_total_live_count = Some(2);
    }
    let decision = vm.lock().apply_catalog_sweep_result_sync(
        &sweep_result(AllLivesStopReason::ReachedEnd, 9, None),
        &StatsCollectionQuality::Complete {
            at: earlier,
            count: 2,
        },
        now,
    );
    assert_eq!(decision, None, "행이 바뀌지 않았으면 승격하지 않는다");
    let s = vm.read();
    assert_eq!(s.all_stat_channels().len(), 2);
    assert_eq!(s.stats_collected_count, 2);
    assert!(matches!(
        s.stats_collection_quality,
        StatsCollectionQuality::Partial { count: 9, .. }
    ));
    assert_eq!(
        s.stats_load_error.as_deref(),
        Some(LiveCatalogPromotion::UNVERIFIED_TERMINAL_BANNER)
    );
}

#[test]
fn cancelled_sweep_restores_previous_quality() {
    let (vm, ..) = make_vm();
    let now = Utc::now();
    vm.lock().apply_catalog_sweep_result_sync(
        &sweep_result(AllLivesStopReason::Cancelled, 3, None),
        &StatsCollectionQuality::Collecting,
        now,
    );
    assert_eq!(
        vm.read().stats_collection_quality,
        StatsCollectionQuality::Unknown
    );
    vm.lock().set_all_stat_channels(vec![channel("a", 1, true)]);
    vm.lock().apply_catalog_sweep_result_sync(
        &sweep_result(AllLivesStopReason::Paused, 3, None),
        &StatsCollectionQuality::Collecting,
        now,
    );
    assert!(matches!(
        vm.read().stats_collection_quality,
        StatsCollectionQuality::Truncated { count: 1, .. }
    ));
}

// MARK: - 리비전·인기 첫 페이지

#[tokio::test]
async fn load_live_channels_commits_revision_and_derived() {
    let (vm, _, snapshots, _) = make_vm();
    vm.lock().has_data_store = true;
    vm.load_live_channels(true).await;
    {
        let s = vm.read();
        assert_eq!(s.live_channels().len(), 30);
        assert_eq!(s.live_channels_revision, 1);
        assert_eq!(s.category_channels_revision, 1);
        assert_eq!(s.top_channels.len(), 6);
        assert_eq!(s.top_channels[0].channel_id, "c1");
        assert!(s.has_more_channels);
        assert_eq!(s.popular_origin(), DataOrigin::Network);
        assert!(s.live_channels_cached_at().is_some());
        assert!(!s.is_loading);
    }
    // 같은 목록이면 리비전이 오르지 않는다.
    vm.load_live_channels(false).await;
    assert_eq!(vm.read().live_channels_revision, 1);
    tokio::task::yield_now().await;
    let saved = snapshots.saved.lock().unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].0, "popular");
}

#[tokio::test]
async fn load_live_channels_failure_sets_error() {
    let deps = HomeDeps::with_popular(Arc::new(FakePopular {
        count: 0,
        fetched_at: Utc::now(),
        fail: true,
    }));
    let vm = HomeViewModel::new(deps);
    vm.load_live_channels(true).await;
    let s = vm.read();
    assert_eq!(s.error_message.as_deref(), Some("네트워크 오류"));
    assert!(!s.is_loading);
}

// MARK: - 팔로잉 폴

fn following_result(
    gen: u64,
    reason: FollowingPollReason,
    n: usize,
    at: DateTime<Utc>,
) -> FollowingPollResult<LiveChannelItem> {
    FollowingPollResult {
        reason,
        channels: (0..n)
            .map(|i| channel(&format!("f{i}"), 10, i % 2 == 0))
            .collect(),
        fetched_at: at,
        generation: gen,
        newly_live: HashSet::new(),
        is_baseline: false,
        list_carries_live_info: true,
        detail_requests: Vec::new(),
    }
}

#[tokio::test]
async fn following_poll_publishes_and_throttles_viewer_only_changes() {
    let (vm, _, _, _) = make_vm();
    let loaded = Arc::new(Mutex::new(0));
    let l2 = loaded.clone();
    vm.lock().on_following_loaded = Some(Arc::new(move || *l2.lock().unwrap() += 1));
    let t0 = Utc::now();
    vm.receive_following_poll(&FollowingPollOutcome::Success(following_result(
        0,
        FollowingPollReason::Scheduled,
        4,
        t0,
    )));
    {
        let s = vm.read();
        assert_eq!(s.following_channels().len(), 4);
        assert_eq!(s.following_live_count, 2);
        assert_eq!(s.following_live_rate, 50);
        assert_eq!(s.following_cached_at(), Some(t0));
    }
    // 시청자 수만 바뀐 결과는 120초 안에서 반영하지 않는다.
    let mut viewers_only = following_result(
        0,
        FollowingPollReason::Scheduled,
        4,
        t0 + ChronoDuration::seconds(60),
    );
    viewers_only.channels[0].viewer_count = 999;
    vm.receive_following_poll(&FollowingPollOutcome::Success(viewers_only.clone()));
    assert_eq!(vm.read().following_channels()[0].viewer_count, 10);
    // 사용자 새로고침은 늘 반영한다.
    viewers_only.reason = FollowingPollReason::Manual {
        invalidate_thumbnails: false,
    };
    vm.receive_following_poll(&FollowingPollOutcome::Success(viewers_only));
    assert_eq!(vm.read().following_channels()[0].viewer_count, 999);
    assert_eq!(*loaded.lock().unwrap(), 3);
    // 다른 세대의 결과는 버린다.
    vm.receive_following_poll(&FollowingPollOutcome::Success(following_result(
        7,
        FollowingPollReason::Manual {
            invalidate_thumbnails: false,
        },
        1,
        t0,
    )));
    assert_eq!(vm.read().following_channels().len(), 4);
}

#[test]
fn following_failure_unauthorized_and_generic() {
    let (vm, ..) = make_vm();
    let fail = |unauth: bool| {
        FollowingPollOutcome::Failure(FollowingPollFailure {
            reason: FollowingPollReason::Scheduled,
            generation: 0,
            error: FollowingError {
                message: "실패".into(),
                is_unauthorized: unauth,
            },
        })
    };
    vm.receive_following_poll(&fail(true));
    assert!(vm.read().needs_cookie_login);
    assert!(vm.read().following_load_error.is_none());
    vm.receive_following_poll(&fail(false));
    assert_eq!(vm.read().following_load_error.as_deref(), Some("실패"));
}

#[test]
fn invalidate_following_authentication_clears_everything() {
    let (vm, _, _, following) = make_vm();
    vm.receive_following_poll(&FollowingPollOutcome::Success(following_result(
        0,
        FollowingPollReason::Boot,
        4,
        Utc::now(),
    )));
    vm.invalidate_following_authentication();
    let s = vm.read();
    assert!(s.following_channels().is_empty());
    assert_eq!(s.following_live_count, 0);
    assert!(s.following_cached_at().is_none());
    assert_eq!(s.following_auth_generation, 1);
    assert_eq!(*following.generation.lock().unwrap(), 1);
}

// MARK: - 캐시·부팅 스냅샷 (HomeBootHydrateTests)

fn seed_snapshots(snapshots: &MemoryMenuSnapshots, scope: &str, at: DateTime<Utc>) {
    let popular: Vec<_> = (0..30)
        .map(|i| channel(&format!("p{i}"), i, true))
        .collect();
    let following: Vec<_> = (0..320)
        .map(|i| channel(&format!("f{i}"), 1, true))
        .collect();
    let mut lists = snapshots.lists.lock().unwrap();
    lists.insert(
        ("popular".into(), "public".into()),
        SnapshotLoadResult::Fresh(
            MenuChannelListSnapshot {
                channels: popular,
                fetched_at: at,
            },
            at,
        ),
    );
    lists.insert(
        ("following".into(), scope.into()),
        SnapshotLoadResult::Fresh(
            MenuChannelListSnapshot {
                channels: following,
                fetched_at: at,
            },
            at,
        ),
    );
}

#[tokio::test]
async fn boot_preload_fills_popular_and_holds_following_until_allowed() {
    let (vm, _, snapshots, _) = make_vm();
    let at = Utc::now() - ChronoDuration::seconds(120);
    let scope = vm.read().following_cache_scope.to_string();
    seed_snapshots(&snapshots, &scope, at);
    vm.preload_boot_snapshots().await;
    {
        let s = vm.read();
        assert_eq!(s.live_channels().len(), 30);
        assert_eq!(s.live_channels_cached_at(), Some(at));
        assert_eq!(s.popular_origin(), DataOrigin::Snapshot);
        assert!(
            s.following_channels().is_empty(),
            "인증 전에 팔로잉이 적용됐다"
        );
        let report = s.boot_hydrate_report.clone().unwrap();
        assert_eq!((report.popular_rows, report.following_rows), (30, 320));
        assert!(s.popular_freshness.viewers_stale);
        assert!(s.popular_freshness.caption.is_some());
    }
    vm.lock().following_cache_allowed = true;
    vm.apply_boot_following_if_allowed().await;
    let s = vm.read();
    assert_eq!(s.following_channels().len(), 320);
    assert_eq!(s.following_live_count, 320);
    assert_eq!(s.following_live_rate, 100);
    assert!(!s.following_is_provisional);
    assert!(s.boot_following.is_none());
}

#[tokio::test]
async fn provisional_boot_following_then_confirm_or_reject() {
    let (confirmed, _, snapshots, _) = make_vm();
    let at = Utc::now();
    let scope = confirmed.read().following_cache_scope.to_string();
    seed_snapshots(&snapshots, &scope, at);
    confirmed.preload_boot_snapshots().await;
    confirmed.show_provisional_boot_following().await;
    assert_eq!(confirmed.read().following_channels().len(), 320);
    assert!(confirmed.read().following_is_provisional);
    confirmed.lock().following_cache_allowed = true;
    confirmed.apply_boot_following_if_allowed().await;
    assert!(!confirmed.read().following_is_provisional);
    assert_eq!(confirmed.read().following_channels().len(), 320);

    let (rejected, _, snapshots, _) = make_vm();
    let scope = rejected.read().following_cache_scope.to_string();
    seed_snapshots(&snapshots, &scope, at);
    rejected.preload_boot_snapshots().await;
    rejected.show_provisional_boot_following().await;
    assert!(rejected.read().following_is_provisional);
    rejected.invalidate_following_authentication();
    let s = rejected.read();
    assert!(s.following_channels().is_empty());
    assert!(!s.following_is_provisional);
    assert!(s.boot_following.is_none());
}

#[tokio::test]
async fn boot_preload_does_not_override_network_or_other_scope() {
    let (vm, _, snapshots, _) = make_vm();
    seed_snapshots(&snapshots, "other-scope", Utc::now());
    let fresh = vec![channel("net", 5, true)];
    vm.lock().set_live_channels(fresh.clone());
    vm.preload_boot_snapshots().await;
    vm.lock().following_cache_allowed = true;
    vm.apply_boot_following_if_allowed().await;
    let s = vm.read();
    assert_eq!(
        s.live_channels(),
        fresh.as_slice(),
        "네트워크가 채운 인기 목록을 덮었다"
    );
    assert!(s.following_channels().is_empty());
}

#[tokio::test]
async fn load_from_cache_respects_scope_and_generation() {
    let (vm, _, snapshots, _) = make_vm();
    let at = Utc::now() - ChronoDuration::seconds(30);
    let scope = vm.read().following_cache_scope.to_string();
    seed_snapshots(&snapshots, &scope, at);
    vm.lock().following_cache_allowed = true;
    vm.attach_data_store().await;
    let s = vm.read();
    assert_eq!(s.live_channels().len(), 30);
    assert_eq!(s.following_channels().len(), 320);
    assert_eq!(s.following_origin(), DataOrigin::Snapshot);
    assert!(s.catalog_rows_restore_pending);
}

// MARK: - 신선도 표시

#[test]
fn freshness_display_rules() {
    let now = date(1_700_000_000);
    assert_eq!(
        FreshnessDisplay::make(None, DataOrigin::Network, &FreshnessPolicy::POPULAR, &now),
        FreshnessDisplay::NONE
    );
    let fresh = FreshnessDisplay::make(
        Some(&now),
        DataOrigin::Network,
        &FreshnessPolicy::POPULAR,
        &now,
    );
    assert_eq!(fresh, FreshnessDisplay::NONE);
    let restored_young = FreshnessDisplay::make(
        Some(&now),
        DataOrigin::Snapshot,
        &FreshnessPolicy::POPULAR,
        &now,
    );
    assert!(restored_young.viewers_stale);
    let old = now - ChronoDuration::seconds(12 * 60);
    let stale = FreshnessDisplay::make(
        Some(&old),
        DataOrigin::Network,
        &FreshnessPolicy::POPULAR,
        &now,
    );
    assert!(stale.viewers_stale);
    assert_eq!(stale.caption.as_deref(), Some("12분 전 기준"));
    assert!(HomeViewModel::next_freshness_wake(None, None, &now).is_none());
    assert_eq!(
        HomeViewModel::next_freshness_wake(Some(&now), None, &now),
        Some(now + ChronoDuration::seconds(60))
    );
}

// MARK: - 자동 갱신 게이트

#[test]
fn auto_refresh_gate_tracks_holders() {
    use cview_core::utils::RefreshDemand;
    let (vm, ..) = make_vm();
    assert!(!vm.read().is_auto_refresh_active);
    vm.retain_auto_refresh(RefreshDemand::Home, 1);
    assert!(vm.read().is_auto_refresh_active);
    vm.retain_auto_refresh(RefreshDemand::Home, 2);
    vm.release_auto_refresh(RefreshDemand::Home, 1);
    assert!(vm.read().is_auto_refresh_active);
    vm.release_auto_refresh(RefreshDemand::Home, 2);
    assert!(!vm.read().is_auto_refresh_active);
}

// MARK: - 추천

#[test]
fn recommendation_scores() {
    let candidates = vec![
        channel("follow", 10, true),
        channel("fav", 100, true),
        channel("watching", 1_000, true),
        channel("plain", 0, true),
    ];
    let inputs = RecommendationInputs {
        candidates,
        following_channel_ids: ["follow".to_string()].into(),
        favorite_channel_ids: ["fav".to_string()].into(),
        recent_categories: ["GAME".to_string()].into(),
        already_watching_channel_ids: ["watching".to_string()].into(),
        ..Default::default()
    };
    let scored = recommendation::score(&inputs, 12);
    let ids: Vec<_> = scored.iter().map(|s| s.id().to_string()).collect();
    assert_eq!(ids, ["follow", "fav", "plain"], "이미 보는 채널은 빠진다");
    assert_eq!(scored[0].reasons, ["팔로잉", "관심 카테고리"]);
    assert!((scored[0].score - (125.0 + 0.15)).abs() < 1e-9);
    assert!((scored[2].score - 25.0).abs() < 1e-9);
    assert!(recommendation::score(&RecommendationInputs::default(), 12).is_empty());
    assert_eq!(recommendation::score(&inputs, 1).len(), 1);
}
