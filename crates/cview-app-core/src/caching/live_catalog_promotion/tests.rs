
//! 원본: `Tests/CViewAppTests/LiveCatalogPromotionTests.swift`
use super::*;
use chrono::FixedOffset;
use cview_core::utils::time::{adding, from_unix};

fn now() -> Date {
    from_unix(1_700_000_000.0)
}

fn previous_states() -> Vec<(Option<CatalogPreviousState>, bool)> {
    let st = |quality, ago: f64| {
        Some(CatalogPreviousState {
            quality,
            completed_at: adding(&now(), -ago),
        })
    };
    vec![
        (None, false),
        (st(LiveCatalogQuality::Verified { exact: 10 }, 600.0), true),
        (
            st(LiveCatalogQuality::Stitched { observed: 10 }, 600.0),
            true,
        ),
        (
            st(LiveCatalogQuality::Verified { exact: 10 }, 3_600.0),
            false,
        ),
        (
            st(
                LiveCatalogQuality::Observed {
                    count: 10,
                    reason: "x".into(),
                },
                60.0,
            ),
            false,
        ),
    ]
}

fn decide(
    reason: AllLivesStopReason,
    exact: Option<i64>,
    previous: Option<&CatalogPreviousState>,
) -> CatalogPromotionDecision {
    LiveCatalogPromotion::decide(&CatalogSweepOutcome::new(reason, exact, 5), previous, now())
}

#[test]
fn exact_completion_always_promotes() {
    for (previous, _) in previous_states() {
        let d = decide(AllLivesStopReason::ReachedEnd, Some(7), previous.as_ref());
        assert_eq!(d, CatalogPromotionDecision::Promote { exact: 7 });
        assert_eq!(d.quality(), Some(LiveCatalogQuality::Verified { exact: 7 }));
        assert!(d.records_growth_baseline());
    }
}

#[test]
fn interrupted_outcomes_are_discarded() {
    for reason in [
        AllLivesStopReason::Cancelled,
        AllLivesStopReason::Paused,
        AllLivesStopReason::ReachedBoundary { live_id: 9 },
    ] {
        for (previous, _) in previous_states() {
            let d = decide(reason.clone(), None, previous.as_ref());
            assert_eq!(d, CatalogPromotionDecision::Discard);
            assert!(d.quality().is_none());
        }
    }
}

#[test]
fn unverified_outcomes_keep_previous_or_become_provisional() {
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
    for (reason, retry, tag) in cases {
        for (previous, keepable) in previous_states() {
            let d = decide(reason.clone(), None, previous.as_ref());
            if keepable {
                match &d {
                    CatalogPromotionDecision::KeepPrevious {
                        banner,
                        retry_after,
                    } => {
                        assert!(!banner.is_empty());
                        assert_eq!(*retry_after, retry);
                    }
                    other => panic!("keepPrevious 여야 한다: {other:?}"),
                }
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
fn keep_window_boundary() {
    let at = |ago: f64| CatalogPreviousState {
        quality: LiveCatalogQuality::Verified { exact: 1 },
        completed_at: adding(&now(), -ago),
    };
    assert!(matches!(
        decide(
            AllLivesStopReason::ReachedPageLimit,
            None,
            Some(&at(1_799.0))
        ),
        CatalogPromotionDecision::KeepPrevious { .. }
    ));
    assert_eq!(
        decide(
            AllLivesStopReason::ReachedPageLimit,
            None,
            Some(&at(1_800.0))
        ),
        CatalogPromotionDecision::Provisional {
            reason: "page-limit".into(),
            observed: 5
        }
    );
}

#[test]
fn banners_keep_legacy_wording() {
    let fresh = previous_states().remove(1).0;
    assert_eq!(
        decide(
            AllLivesStopReason::PartialFailure {
                page: 3,
                message: "x".into()
            },
            None,
            fresh.as_ref()
        ),
        CatalogPromotionDecision::KeepPrevious {
            banner: "일부 페이지(3) 수집에 실패했습니다. 다시 시도해 주세요.".into(),
            retry_after: TRANSIENT_RETRY
        }
    );
}

#[test]
fn event_outcome_names() {
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
        o(AllLivesStopReason::ReachedPageLimit, None).event_outcome(),
        "unverified"
    );
    let failed = o(
        AllLivesStopReason::PartialFailure {
            page: 1,
            message: "x".into(),
        },
        None,
    );
    assert_eq!(failed.event_outcome(), "failed");
    assert!(!failed.completed_normally());
    assert_eq!(
        o(AllLivesStopReason::Cancelled, None).event_outcome(),
        "cancelled"
    );
    assert!(o(
        AllLivesStopReason::UnstableOrdering {
            attempts: 1,
            duplicate_count: 0
        },
        None
    )
    .completed_normally());
    assert_eq!(
        o(AllLivesStopReason::CursorCycle { page: 1 }, None).retry_class(),
        Some(CatalogRetryClass::Transient)
    );
}

#[test]
fn count_labels_by_quality() {
    let seoul = FixedOffset::east_opt(9 * 3600).unwrap();
    let at = from_unix(1_700_000_000.0); // 2023-11-15 07:13 KST
    assert_eq!(
        LiveCatalogPromotion::catalog_count_label_in(
            &LiveCatalogQuality::Observed {
                count: 3,
                reason: "x".into()
            },
            Some(at),
            &seoul
        ),
        "3개 관측 · 07:13 집계"
    );
    assert_eq!(
        LiveCatalogPromotion::catalog_count_label_in(
            &LiveCatalogQuality::Verified { exact: 9 },
            None,
            &seoul
        ),
        "9개 확인"
    );
}

fn row(live_id: i64, channel: &str) -> LiveCatalogRow {
    LiveCatalogRow {
        live_id,
        channel_id: channel.into(),
        channel_name: channel.into(),
        channel_image_url: None,
        live_title: String::new(),
        viewer_count: 0,
        category_name: None,
        category_id: None,
        category_type: None,
        thumbnail_url: None,
        open_date: None,
    }
}

#[test]
fn merge_by_channel_keeps_newest_live_and_first_position() {
    let (rows, replaced) = StitchedCatalogVerifier::merge_by_channel(vec![
        row(5, "a"),
        row(7, "b"),
        row(9, "a"),
        row(9, "z"),
        row(3, ""),
    ]);
    let ids: Vec<(i64, &str)> = rows
        .iter()
        .map(|r| (r.live_id, r.channel_id.as_str()))
        .collect();
    assert_eq!(ids, vec![(9, "a"), (7, "b"), (3, "")]);
    assert_eq!(replaced, 1);
}

#[test]
fn stitched_verifier_rejects_old_generation_and_unproven_boundary() {
    let empty = AllLivesCollectionResult {
        items: Vec::new(),
        estimated_total: None,
        requested_pages: 1,
        received_pages: 1,
        deduplicated_count: 0,
        stop_reason: AllLivesStopReason::ReachedBoundary { live_id: 10 },
        verified_observed_count: None,
        stable_cursor_ordering_verified: false,
        resume_point: None,
        first_live_id: None,
    };
    let inputs = StitchedInputs {
        checkpoint_rows: vec![row(10, "a")],
        max_live_id: 10,
        continuation: None,
        top_up: empty.clone(),
        started_at: now(),
        now: adding(&now(), 30.0),
    };
    match StitchedCatalogVerifier::verify(&inputs) {
        StitchVerdict::Accepted(s) => {
            assert_eq!(s.rows.len(), 1);
            assert_eq!(s.pages, 1);
        }
        other => panic!("{other:?}"),
    }
    let mut old = inputs.clone();
    old.now = adding(&now(), 601.0);
    assert_eq!(
        StitchedCatalogVerifier::verify(&old),
        StitchVerdict::Rejected(StitchRejectReason::GenerationTooOld)
    );
    let mut unproven = inputs;
    unproven.top_up.stop_reason = AllLivesStopReason::ReachedBoundary { live_id: 11 };
    assert_eq!(
        StitchedCatalogVerifier::verify(&unproven),
        StitchVerdict::Rejected(StitchRejectReason::BoundaryUnproven)
    );
}
