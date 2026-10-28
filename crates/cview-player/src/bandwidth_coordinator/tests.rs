
//! `MultiLiveBandwidthCoordinatorTests` · `MultiLiveTransferCapacityTests` · `PaneSizeLevelCapTests` 포팅.
use super::*;
use crate::clock::ManualClock;

fn cfg(min_floor: f64, weight: f64, freshness: f64) -> BandwidthCoordinatorConfig {
    BandwidthCoordinatorConfig {
        safety_factor: 1.0,
        min_per_stream_bitrate: min_floor,
        history_size: 30,
        low_buffer_threshold: 3.0,
        high_buffer_threshold: 6.0,
        enable_level_capping: true,
        selected_session_weight: weight,
        emergency_buffer_threshold: 1.0,
        sample_freshness_interval: freshness,
    }
}

fn uuid(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

fn sample(id: Uuid, bitrate: f64, rate: f64) -> StreamBandwidthSample {
    let mut s = StreamBandwidthSample::new(id, bitrate, 5.0, 0.5, 4.0);
    s.playback_rate = rate;
    s.transfers = vec![
        StreamTransferSample::new("a", bitrate * rate / 4.0, 96.0, 96.0, 98.0, 2.0),
        StreamTransferSample::new("b", bitrate * rate / 4.0, 98.0, 98.0, 100.0, 2.0),
    ];
    s
}

fn set(ids: &[Uuid]) -> HashSet<Uuid> {
    ids.iter().copied().collect()
}

fn bw(advices: &[BandwidthAdvice], id: Uuid) -> f64 {
    advices
        .iter()
        .find(|a| a.session_id == id)
        .map(|a| a.max_allowed_bitrate)
        .unwrap_or(0.0)
}

fn manual() -> MultiLiveBandwidthCoordinator {
    MultiLiveBandwidthCoordinator::with_clock(
        BandwidthCoordinatorConfig::default(),
        ManualClock::new(1000.0),
    )
}

#[test]
fn empty_and_unmeasured() {
    let mut c = manual();
    assert!(c.compute_advice().is_empty());
    let id = Uuid::new_v4();
    c.register_stream(id, true);
    let a = c.compute_advice();
    assert_eq!(a.len(), 1);
    assert!(!a[0].has_bandwidth_estimate);
    assert_eq!(a[0].max_allowed_bitrate, 0.0);
}

#[test]
fn unregister_removes_advice() {
    let mut c = manual();
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    c.register_stream(a, true);
    c.register_stream(b, false);
    assert_eq!(c.compute_advice().len(), 2);
    c.unregister_stream(a);
    let after = c.compute_advice();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].session_id, b);
}

#[test]
fn selected_gets_more_bandwidth() {
    let mut c = MultiLiveBandwidthCoordinator::default();
    let (s, u) = (Uuid::new_v4(), Uuid::new_v4());
    c.register_stream_at(s, true, 100.0);
    c.register_stream_at(u, false, 100.0);
    c.report_snapshot_at(
        &[sample(s, 5e6, 1.0), sample(u, 5e6, 1.0)],
        &set(&[s, u]),
        101.0,
    );
    let a = c.compute_advice_at(101.0);
    assert!(bw(&a, s) > bw(&a, u));
}

#[test]
fn emergency_spares_selected_and_hysteresis() {
    let mut c = manual();
    let (s, u) = (Uuid::new_v4(), Uuid::new_v4());
    c.register_stream(s, true);
    c.register_stream(u, false);
    c.report_bandwidth_sample(s, 5e6, 0.5, 0.5, 4.0, 1.0);
    c.report_bandwidth_sample(u, 5e6, 0.5, 0.5, 4.0, 1.0);
    let a = c.compute_advice();
    let find = |id| a.iter().find(|x| x.session_id == id).unwrap();
    assert!(!find(s).emergency_downgrade);
    assert!(find(u).emergency_downgrade);
    assert_eq!(find(u).buffer_phase, BufferHysteresisPhase::Buffering);

    // 히스테리시스: 4s(저수위 3 과 고수위 6 사이)로는 아직 buffering, 20s 면 playing.
    c.report_bandwidth_sample(u, 5e6, 4.0, 0.5, 4.0, 1.0);
    let a = c.compute_advice();
    assert_eq!(
        a.iter().find(|x| x.session_id == u).unwrap().buffer_phase,
        BufferHysteresisPhase::Buffering
    );
    c.report_bandwidth_sample(u, 5e6, 20.0, 0.5, 4.0, 1.0);
    let a = c.compute_advice();
    assert_eq!(
        a.iter().find(|x| x.session_id == u).unwrap().buffer_phase,
        BufferHysteresisPhase::Playing
    );
}

#[test]
fn pane_size_capping() {
    let mut c = manual();
    let id = Uuid::new_v4();
    c.register_stream(id, false);
    c.update_pane_size(id, 480, 270);
    let capped = c.compute_advice()[0].capped_max_height;
    assert!(capped > 0 && capped <= 720);
}

#[test]
fn batch_snapshot_is_atomic_and_records_once() {
    let mut c = MultiLiveBandwidthCoordinator::new(cfg(0.0, 2.0, 20.0));
    let (a, b) = (uuid(1), uuid(2));
    c.register_stream_at(a, true, 100.0);
    c.register_stream_at(b, false, 100.0);
    let before = c.aggregate_demand_write_count();
    c.report_snapshot_at(
        &[sample(a, 2e6, 1.5), sample(b, 1e6, 2.0)],
        &set(&[a, b]),
        101.0,
    );
    assert_eq!(c.aggregate_demand_write_count(), before + 1);
    assert_eq!(c.latest_aggregate_demand(), Some(5e6));
    let total: f64 = c
        .compute_advice_at(101.0)
        .iter()
        .map(|x| x.max_allowed_bitrate)
        .sum();
    assert!((total - 5e6).abs() < 0.001);
}

#[test]
fn batch_snapshot_is_order_independent() {
    let (a, b, cc) = (uuid(0x11), uuid(0x12), uuid(0x13));
    let mut first = MultiLiveBandwidthCoordinator::new(cfg(0.0, 1.5, 20.0));
    let mut second = MultiLiveBandwidthCoordinator::new(cfg(0.0, 1.5, 20.0));
    for c in [&mut first, &mut second] {
        c.register_stream_at(a, true, 100.0);
        c.register_stream_at(b, false, 100.0);
        c.register_stream_at(cc, false, 100.0);
    }
    let samples = vec![
        sample(a, 3e6, 1.1),
        sample(b, 1.5e6, 1.0),
        sample(cc, 8e5, 1.25),
    ];
    let rev: Vec<_> = samples.iter().rev().cloned().collect();
    first.report_snapshot_at(&samples, &set(&[a, b, cc]), 101.0);
    second.report_snapshot_at(&rev, &set(&[a, b, cc]), 101.0);
    assert_eq!(
        first.latest_aggregate_demand(),
        second.latest_aggregate_demand()
    );
    assert_eq!(
        first.compute_advice_at(101.0),
        second.compute_advice_at(101.0)
    );
}

#[test]
fn stale_revisions_are_ignored() {
    let mut c = MultiLiveBandwidthCoordinator::new(cfg(0.0, 2.0, 20.0));
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    c.register_stream_at(a, true, 100.0);
    c.register_stream_at(b, false, 100.0);
    c.report_snapshot_at(
        &[sample(a, 1e6, 1.0), sample(b, 1e6, 1.0)],
        &set(&[a, b]),
        101.0,
    );
    c.replace_selected_stream(Some(b), 2);
    c.replace_selected_stream(Some(a), 1);
    let adv = c.compute_advice_at(101.0);
    assert!(bw(&adv, b) > bw(&adv, a));

    let mut c = MultiLiveBandwidthCoordinator::new(cfg(0.0, 1.5, 20.0));
    let id = Uuid::new_v4();
    c.register_stream_at(id, false, 100.0);
    c.update_pane_sizes(
        &[StreamPaneSizeUpdate {
            session_id: id,
            width: 320,
            height: 180,
        }],
        2,
    );
    c.update_pane_sizes(
        &[StreamPaneSizeUpdate {
            session_id: id,
            width: 1920,
            height: 1080,
        }],
        1,
    );
    assert_eq!(c.compute_advice_at(100.0)[0].capped_max_height, 360);
}

#[test]
fn inactive_and_stale_streams_are_excluded() {
    let mut c = MultiLiveBandwidthCoordinator::new(cfg(0.0, 1.5, 5.0));
    let (active, inactive, stale) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    for id in [active, inactive, stale] {
        c.register_stream_at(id, id == active, 0.0);
    }
    c.report_snapshot_at(
        &[sample(active, 2e6, 1.0), sample(inactive, 9e6, 1.0)],
        &set(&[active, stale]),
        10.0,
    );
    let adv = c.compute_advice_at(10.0);
    assert_eq!(
        adv.iter().map(|a| a.session_id).collect::<Vec<_>>(),
        vec![active]
    );
    assert_eq!(c.latest_aggregate_demand(), Some(2e6));
}

#[test]
fn feasible_floor_does_not_overcommit() {
    let mut c = MultiLiveBandwidthCoordinator::new(cfg(300_000.0, 10.0, 20.0));
    let ids = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
    for (i, id) in ids.iter().enumerate() {
        c.register_stream_at(*id, i == 0, 100.0);
    }
    let samples: Vec<_> = ids.iter().map(|id| sample(*id, 100_000.0, 1.0)).collect();
    c.report_snapshot_at(&samples, &set(&ids), 101.0);
    let adv = c.compute_advice_at(101.0);
    let total: f64 = adv.iter().map(|a| a.max_allowed_bitrate).sum();
    assert_eq!(adv.len(), 3);
    assert!(total <= 300_000.0);
    assert!(adv
        .iter()
        .all(|a| (a.max_allowed_bitrate - 100_000.0).abs() < 0.001));
}

// --- MultiLiveTransferCapacityTests ---

fn transfers(duration: f64) -> Vec<StreamTransferSample> {
    vec![
        StreamTransferSample::new("first", 1e6, 90.0, 91.0, 91.0 + duration, 2.0),
        StreamTransferSample::new("second", 1e6, 96.0, 97.0, 97.0 + duration, 2.0),
    ]
}

fn tsample(
    id: Uuid,
    t: Vec<StreamTransferSample>,
    bitrate: f64,
    protects: bool,
    manual: bool,
    buffer: f64,
) -> StreamBandwidthSample {
    let mut s = StreamBandwidthSample::new(id, bitrate, buffer, 0.0, 0.0);
    s.transfers = t;
    s.protects_quality = protects;
    s.has_manual_quality = manual;
    s
}

fn plain(id: Uuid, t: Vec<StreamTransferSample>) -> StreamBandwidthSample {
    tsample(id, t, 8e6, false, false, 8.0)
}

fn total(adv: &[BandwidthAdvice]) -> f64 {
    adv.iter().map(|a| a.max_allowed_bitrate).sum()
}

#[test]
fn distinguishes_transfer_rate_from_consumption() {
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let mut fast = MultiLiveBandwidthCoordinator::default();
    let mut slow = MultiLiveBandwidthCoordinator::default();
    for c in [&mut fast, &mut slow] {
        c.register_stream_at(a, false, 100.0);
        c.register_stream_at(b, false, 100.0);
    }
    fast.report_snapshot_at(
        &[plain(a, transfers(0.25)), plain(b, transfers(0.25))],
        &set(&[a, b]),
        100.0,
    );
    slow.report_snapshot_at(
        &[plain(a, transfers(2.0)), plain(b, transfers(2.0))],
        &set(&[a, b]),
        100.0,
    );
    assert!((total(&fast.compute_advice_at(100.0)) - 48e6).abs() < 0.01);
    assert!((total(&slow.compute_advice_at(100.0)) - 6e6).abs() < 0.01);
}

#[test]
fn deduplicates_within_session_and_merges_overlap() {
    let mut c = MultiLiveBandwidthCoordinator::default();
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    for id in [a, b] {
        c.register_stream_at(id, false, 100.0);
    }
    let t = transfers(2.0);
    let doubled: Vec<_> = t.iter().chain(t.iter()).cloned().collect();
    c.report_snapshot_at(&[plain(a, doubled), plain(b, t)], &set(&[a, b]), 100.0);
    let adv = c.compute_advice_at(100.0);
    assert!(adv.iter().all(|x| x.has_bandwidth_estimate));
    assert!((total(&adv) - 6e6).abs() < 0.01);
}

#[test]
fn missing_or_expired_samples_do_not_invent_capacity() {
    let mut c = MultiLiveBandwidthCoordinator::default();
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    for id in [a, b] {
        c.register_stream_at(id, false, 100.0);
        c.update_pane_size(id, 320, 180);
    }
    c.report_snapshot_at(
        &[plain(a, transfers(0.25)), plain(b, vec![])],
        &set(&[a, b]),
        100.0,
    );
    assert!(c
        .compute_advice_at(100.0)
        .iter()
        .all(|x| !x.has_bandwidth_estimate
            && x.max_allowed_bitrate == 0.0
            && x.capped_max_height == 360));
    c.report_snapshot_at(
        &[plain(a, transfers(0.25)), plain(b, transfers(0.25))],
        &set(&[a, b]),
        120.0,
    );
    assert!(c
        .compute_advice_at(120.0)
        .iter()
        .all(|x| !x.has_bandwidth_estimate));
}

#[test]
fn reserves_protected_consumption() {
    let mut c = MultiLiveBandwidthCoordinator::default();
    let (main, secondary) = (Uuid::new_v4(), Uuid::new_v4());
    for id in [main, secondary] {
        c.register_stream_at(id, id == main, 100.0);
    }
    let t = transfers(0.5);
    c.report_snapshot_at(
        &[
            tsample(main, t.clone(), 8e6, true, false, 8.0),
            plain(secondary, t.clone()),
        ],
        &set(&[main, secondary]),
        100.0,
    );
    let first = c.compute_advice_at(100.0);
    assert_eq!(bw(&first, main), 8e6);
    assert_eq!(bw(&first, secondary), 16e6);
    c.report_snapshot_at(
        &[
            tsample(main, t.clone(), 8e6, true, false, 8.0),
            tsample(secondary, t, 1e6, false, false, 8.0),
        ],
        &set(&[main, secondary]),
        101.0,
    );
    assert_eq!(bw(&c.compute_advice_at(101.0), secondary), 16e6);
}

#[test]
fn exhausted_budget_remains_measured() {
    let mut c = MultiLiveBandwidthCoordinator::default();
    let (main, secondary) = (Uuid::new_v4(), Uuid::new_v4());
    for id in [main, secondary] {
        c.register_stream_at(id, id == main, 100.0);
    }
    let t = transfers(2.0);
    c.report_snapshot_at(
        &[
            tsample(main, t.clone(), 8e6, true, false, 8.0),
            plain(secondary, t),
        ],
        &set(&[main, secondary]),
        100.0,
    );
    let adv = c.compute_advice_at(100.0);
    let s = adv.iter().find(|a| a.session_id == secondary).unwrap();
    assert!(s.has_bandwidth_estimate);
    assert_eq!(s.max_allowed_bitrate, 0.0);
}

#[test]
fn manual_reservation_matches_app_bypass() {
    let mut c = MultiLiveBandwidthCoordinator::default();
    let (man, auto) = (Uuid::new_v4(), Uuid::new_v4());
    for id in [man, auto] {
        c.register_stream_at(id, false, 100.0);
    }
    let t = transfers(0.5);
    c.report_snapshot_at(
        &[
            tsample(man, t.clone(), 8e6, false, true, 8.0),
            plain(auto, t.clone()),
        ],
        &set(&[man, auto]),
        100.0,
    );
    assert_eq!(bw(&c.compute_advice_at(100.0), man), 8e6);
    c.report_snapshot_at(
        &[
            tsample(man, t.clone(), 8e6, false, true, 0.2),
            tsample(auto, t, 8e6, false, false, 0.2),
        ],
        &set(&[man, auto]),
        101.0,
    );
    let adv = c.compute_advice_at(101.0);
    assert!(adv.iter().all(|a| a.emergency_downgrade));
    assert!(adv
        .iter()
        .all(|a| (a.max_allowed_bitrate - 12e6).abs() < 0.01));
}

// --- PaneSizeLevelCapTests ---

#[test]
fn pane_size_level_cap_truth_table() {
    assert_eq!(cap_level_by_pane_size(480, 270, true), 0);
    assert_eq!(cap_level_by_pane_size(3840, 2160, true), 0);
    assert_eq!(cap_level_by_pane_size(100, 100, true), 0);
    assert_eq!(cap_level_by_pane_size(480, 270, false), 360);
    assert_eq!(cap_level_by_pane_size(960, 540, false), 720);
    assert_eq!(cap_level_by_pane_size(1920, 1080, false), 1080);
    assert_eq!(cap_level_by_pane_size(7680, 4320, false), 1080);
    assert_eq!(
        cap_level_by_pane_size(i64::MAX / 2, i64::MAX / 2, false),
        1080
    );
    assert_eq!(cap_level_by_pane_size(0, 0, false), 360);
    assert_eq!(cap_level_by_pane_size(1, 1, false), 360);
    let mut previous = 0;
    for w in (320..=2560).step_by(160) {
        let c = cap_level_by_pane_size(w, w * 9 / 16, false);
        assert!(c >= previous);
        previous = c;
    }
    for w in (1..=4000).step_by(37) {
        let c = cap_level_by_pane_size(w, w * 9 / 16, false);
        assert!([0, 360, 480, 720, 1080].contains(&c));
    }
}
