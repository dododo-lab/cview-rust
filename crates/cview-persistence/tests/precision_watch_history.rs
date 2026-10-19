//! 원본: `Tests/CViewPersistenceTests/PrecisionReview0926WatchHistorySyncTests.swift`,
//! `PrecisionReview0926WatchRecordMatchingTests.swift`, `PrecisionReview0926WatchStreakScanCapTests.swift`.
//!
//! streak 테스트는 달력을 `Asia/Seoul`(일광 절약 없음)로 고정한다 — Swift 는 `Calendar.current` 를 썼다.

mod common;

use chrono::{TimeZone, Timelike};
use common::*;
use cview_persistence::models::{WatchHistory, WatchHistorySyncRow};
use cview_persistence::time::Date;
use cview_persistence::DataStore;
use std::collections::HashMap;
use std::sync::Arc;

// MARK: - 동기화 큐

#[derive(Clone, Copy)]
enum Kind {
    Synced,
    NeverSynced,
    Stale,
}

fn sync_base() -> Date {
    ts(1_780_000_000.0)
}

fn sync_row(channel_id: &str, minute: i64, kind: Kind) -> WatchHistory {
    let started = add(&sync_base(), (minute * 60) as f64);
    let mut r = WatchHistory::new(
        channel_id,
        format!("채널-{channel_id}"),
        None,
        None,
        started,
    );
    r.ended_at = Some(add(&started, 30.0));
    r.duration = 30.0;
    let updated = add(&started, 30.0);
    r.updated_at = Some(updated);
    r.synced_at = match kind {
        Kind::Synced => Some(updated),
        Kind::NeverSynced => None,
        Kind::Stale => Some(add(&updated, -10.0)),
    };
    r
}

fn seed_sync(synced: i64, tail: &[Kind]) -> DataStore {
    let s = store();
    let mut rows: Vec<_> = (0..synced)
        .map(|i| sync_row(&format!("c{i}"), i, Kind::Synced))
        .collect();
    for (offset, kind) in tail.iter().enumerate() {
        let i = synced + offset as i64;
        rows.push(sync_row(&format!("c{i}"), i, *kind));
    }
    s.insert_watch_history_rows(&rows).unwrap();
    s
}

fn sync_id(channel_id: &str, minute: i64) -> String {
    WatchHistorySyncRow::make_id(channel_id, &add(&sync_base(), (minute * 60) as f64))
}

fn unsynced_ids(s: &DataStore, limit: usize) -> Vec<String> {
    s.fetch_unsynced_watch_history(limit)
        .unwrap()
        .into_iter()
        .map(|r| r.id)
        .collect()
}

#[test]
fn synced_prefix_does_not_starve_boundary_row() {
    let s = seed_sync(10_200, &[Kind::NeverSynced]);
    assert_eq!(s.unsynced_watch_history_count().unwrap(), 1);
    assert_eq!(unsynced_ids(&s, 50), vec![sync_id("c10200", 10_200)]);
}

#[test]
fn stale_row_beyond_legacy_scan_cap_is_fetched() {
    let s = seed_sync(10_200, &[Kind::Stale]);
    assert_eq!(unsynced_ids(&s, 50), vec![sync_id("c10200", 10_200)]);
}

#[test]
fn drain_converges_after_boundary() {
    let s = seed_sync(10_200, &[Kind::NeverSynced, Kind::Stale]);
    let rows = s.fetch_unsynced_watch_history(50).unwrap();
    assert_eq!(rows.len(), 2);
    let versions: HashMap<_, _> = rows.iter().map(|r| (r.id.clone(), r.updated_at)).collect();
    s.mark_watch_history_synced(&versions).unwrap();
    assert!(s.fetch_unsynced_watch_history(50).unwrap().is_empty());
    assert_eq!(s.unsynced_watch_history_count().unwrap(), 0);
}

#[test]
fn merges_never_synced_and_stale_oldest_first() {
    let stale_first = seed_sync(
        0,
        &[
            Kind::Stale,
            Kind::NeverSynced,
            Kind::NeverSynced,
            Kind::NeverSynced,
        ],
    );
    assert_eq!(
        unsynced_ids(&stale_first, 2),
        vec![sync_id("c0", 0), sync_id("c1", 1)]
    );
    let never_first = seed_sync(0, &[Kind::NeverSynced, Kind::Stale, Kind::Stale]);
    assert_eq!(unsynced_ids(&never_first, 1), vec![sync_id("c0", 0)]);
}

#[test]
fn respects_limit_and_ascending_order() {
    let s = seed_sync(5, &[Kind::NeverSynced; 120]);
    let expected: Vec<_> = (5..55).map(|i| sync_id(&format!("c{i}"), i)).collect();
    assert_eq!(unsynced_ids(&s, 50), expected);
}

#[test]
fn tied_stale_rows_behind_tied_synced_rows_are_fetched_in_order() {
    let s = store();
    let mut rows: Vec<_> = (0..3)
        .map(|i| sync_row(&format!("u{i}"), 0, Kind::Stale))
        .collect();
    rows.extend((0..250).map(|i| sync_row(&format!("t{i:03}"), 0, Kind::Synced)));
    s.insert_watch_history_rows(&rows).unwrap();
    let expected: Vec<_> = (0..3).map(|i| sync_id(&format!("u{i}"), 0)).collect();
    assert_eq!(unsynced_ids(&s, 50), expected);
    assert_eq!(unsynced_ids(&s, 2), expected[..2].to_vec());
}

#[test]
fn tied_never_synced_cut_follows_channel_id_order() {
    let s = store();
    let rows: Vec<_> = (0..120)
        .rev()
        .map(|i| sync_row(&format!("n{i:03}"), 0, Kind::NeverSynced))
        .collect();
    s.insert_watch_history_rows(&rows).unwrap();
    let expected: Vec<_> = (0..50).map(|i| sync_id(&format!("n{i:03}"), 0)).collect();
    assert_eq!(unsynced_ids(&s, 50), expected);
}

#[test]
fn tied_merge_at_horizon_keeps_channel_id_order() {
    let s = store();
    let mut rows: Vec<_> = (0..10)
        .map(|i| sync_row(&format!("n{i:02}"), 0, Kind::NeverSynced))
        .collect();
    rows.extend((0..3).map(|i| sync_row(&format!("m{i}"), 0, Kind::Stale)));
    rows.push(sync_row("a-late", 1, Kind::Stale));
    s.insert_watch_history_rows(&rows).unwrap();
    assert_eq!(
        unsynced_ids(&s, 5),
        vec![
            sync_id("m0", 0),
            sync_id("m1", 0),
            sync_id("m2", 0),
            sync_id("n00", 0),
            sync_id("n01", 0),
        ]
    );
}

#[test]
fn tied_non_padded_ids_use_byte_order() {
    let s = store();
    let mut rows: Vec<_> = ["3f9b", "3f11", "3f10"]
        .iter()
        .map(|c| sync_row(c, 0, Kind::NeverSynced))
        .collect();
    rows.push(sync_row("3f2a", 0, Kind::Stale));
    rows.extend(
        ["c9", "c10", "c11"]
            .iter()
            .map(|c| sync_row(c, 1, Kind::NeverSynced)),
    );
    s.insert_watch_history_rows(&rows).unwrap();
    assert_eq!(
        unsynced_ids(&s, 2),
        vec![sync_id("3f10", 0), sync_id("3f11", 0)]
    );
    assert_eq!(
        unsynced_ids(&s, 50),
        vec![
            sync_id("3f10", 0),
            sync_id("3f11", 0),
            sync_id("3f2a", 0),
            sync_id("3f9b", 0),
            sync_id("c10", 1),
            sync_id("c11", 1),
            sync_id("c9", 1),
        ]
    );
}

#[test]
fn tied_non_padded_never_synced_cut_follows_byte_order() {
    let s = store();
    s.insert_watch_history_rows(&[
        sync_row("c9", 0, Kind::NeverSynced),
        sync_row("c10", 0, Kind::NeverSynced),
    ])
    .unwrap();
    assert_eq!(unsynced_ids(&s, 1), vec![sync_id("c10", 0)]);
}

#[test]
fn zero_limit_returns_empty() {
    let s = seed_sync(0, &[Kind::NeverSynced]);
    assert!(unsynced_ids(&s, 0).is_empty());
}

// MARK: - 시청 기록 정확 일치

fn start_a() -> Date {
    ts(978_307_200.0 + 780_000_000.123_456)
}

fn start_b() -> Date {
    add(&start_a(), 0.5)
}

fn nested_sessions() -> DataStore {
    let s = store();
    assert_eq!(
        s.start_watch_record("same", "같은 채널", None, None, start_a())
            .unwrap(),
        start_a()
    );
    assert_eq!(
        s.start_watch_record("same", "같은 채널", None, None, start_b())
            .unwrap(),
        start_b()
    );
    s
}

fn stored(s: &DataStore, started: Date) -> WatchHistory {
    s.all_watch_history_rows()
        .unwrap()
        .into_iter()
        .find(|r| r.started_at == started)
        .unwrap()
}

#[test]
fn ending_session_a_closes_only_a() {
    let s = nested_sessions();
    assert!(s
        .end_watch_record("same", start_a(), add(&start_a(), 10.0))
        .unwrap());
    let a = stored(&s, start_a());
    let b = stored(&s, start_b());
    assert_eq!(a.ended_at, Some(add(&start_a(), 10.0)));
    assert!(approx(a.duration, 10.0, 0.001));
    assert!(b.ended_at.is_none());
}

#[test]
fn checkpoint_applies_only_to_requested_session() {
    let s = nested_sessions();
    assert!(s
        .checkpoint_watch_record("same", start_a(), add(&start_a(), 30.0))
        .unwrap());
    assert!(approx(stored(&s, start_a()).duration, 30.0, 0.001));
    assert!(approx(stored(&s, start_b()).duration, 0.0, 0.001));
}

#[test]
fn repeated_end_and_checkpoint_of_closed_session_do_not_capture_neighbour() {
    let s = nested_sessions();
    s.end_watch_record("same", start_a(), add(&start_a(), 10.0))
        .unwrap();
    assert!(!s
        .end_watch_record("same", start_a(), add(&start_a(), 20.0))
        .unwrap());
    assert!(!s
        .checkpoint_watch_record("same", start_a(), add(&start_a(), 40.0))
        .unwrap());
    let b = stored(&s, start_b());
    assert!(b.ended_at.is_none());
    assert!(approx(b.duration, 0.0, 0.001));
}

#[test]
fn ending_later_session_b_closes_only_b() {
    let s = nested_sessions();
    assert!(s
        .end_watch_record("same", start_b(), add(&start_b(), 5.0))
        .unwrap());
    assert!(stored(&s, start_a()).ended_at.is_none());
    assert_eq!(stored(&s, start_b()).ended_at, Some(add(&start_b(), 5.0)));
}

#[test]
fn returned_started_at_matches_after_reopening_file_store() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("watch.store");
    let returned = {
        let first = DataStore::open(&path).unwrap();
        first
            .start_watch_record("file", "파일 채널", None, None, start_a())
            .unwrap()
    };
    let reopened = DataStore::open(&path).unwrap();
    assert!(reopened
        .end_watch_record("file", returned, add(&start_a(), 60.0))
        .unwrap());
}

// MARK: - streak 스캔 상한

fn tz() -> chrono_tz::Tz {
    chrono_tz::Asia::Seoul
}

/// 기준일 20:00(서울).
fn streak_now() -> Date {
    let local = sync_base().with_timezone(&tz());
    let d = local.date_naive().and_hms_opt(20, 0, 0).unwrap();
    tz().from_local_datetime(&d)
        .single()
        .unwrap()
        .with_timezone(&chrono::Utc)
}

fn noon(offset: i64) -> Date {
    let n = streak_now();
    let local = n.with_timezone(&tz());
    assert_eq!(local.hour(), 20);
    add(&n, -8.0 * 3600.0 + offset as f64 * 86_400.0)
}

fn start_of_day(d: Date) -> Date {
    let local = d.with_timezone(&tz());
    let midnight = local.date_naive().and_hms_opt(0, 0, 0).unwrap();
    tz().from_local_datetime(&midnight)
        .single()
        .unwrap()
        .with_timezone(&chrono::Utc)
}

struct Row {
    started: Date,
    duration: f64,
    open: bool,
}

fn closed(started: Date, duration: f64) -> Row {
    Row {
        started,
        duration,
        open: false,
    }
}

fn seed_streak(rows: &[Row]) -> DataStore {
    let s = store();
    s.set_calendar(Arc::new(tz()));
    let records: Vec<_> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let mut w =
                WatchHistory::new(format!("c{i}"), format!("채널-{i}"), None, None, r.started);
            w.ended_at = (!r.open).then(|| add(&r.started, r.duration));
            w.duration = r.duration;
            w.updated_at = Some(if r.open {
                streak_now()
            } else {
                add(&r.started, r.duration)
            });
            w
        })
        .collect();
    s.insert_watch_history_rows(&records).unwrap();
    s
}

fn dense(day: i64, count: usize, duration: f64) -> Vec<Row> {
    (0..count)
        .map(|i| closed(add(&noon(day), i as f64), duration))
        .collect()
}

fn one_each(days: impl IntoIterator<Item = i64>) -> Vec<Row> {
    days.into_iter().map(|d| closed(noon(d), 60.0)).collect()
}

fn streak(s: &DataStore) -> i64 {
    s.current_watch_streak(streak_now()).unwrap()
}

fn insights_streak(s: &DataStore) -> i64 {
    s.aggregate_insights(streak_now()).unwrap().streak
}

#[test]
fn streak_not_truncated_by_dense_today() {
    let mut rows = dense(0, 10_300, 60.0);
    rows.extend(one_each([-1, -2]));
    let s = seed_streak(&rows);
    assert_eq!(streak(&s), 3);
    assert_eq!(streak(&s), insights_streak(&s));
}

#[test]
fn zero_duration_flood_does_not_consume_streak() {
    let mut rows = dense(0, 10_300, 0.0);
    rows.extend(one_each([-1, -2]));
    assert_eq!(streak(&seed_streak(&rows)), 2);
}

#[test]
fn streak_beyond_legacy_row_cap_counts_older_day() {
    let mut rows = dense(0, 3_500, 60.0);
    rows.extend(dense(-1, 3_500, 60.0));
    rows.extend(dense(-2, 3_500, 60.0));
    rows.extend(one_each([-3, -6]));
    assert_eq!(streak(&seed_streak(&rows)), 4);
}

#[test]
fn streak_lengths_around_window_seams() {
    for length in [14_i64, 15, 29] {
        let s = seed_streak(&one_each((0..length).map(|d| -d)));
        assert_eq!(streak(&s), length, "streak {length}일");
        assert_eq!(insights_streak(&s), length);
    }
}

#[test]
fn gap_exactly_at_window_boundary_stops_there() {
    let days: Vec<i64> = (0..=13).map(|d| -d).chain((15..=20).map(|d| -d)).collect();
    assert_eq!(streak(&seed_streak(&one_each(days))), 14);
}

#[test]
fn session_exactly_at_window_lower_bound_counts() {
    let mut rows = one_each((0..=27).map(|d| -d).filter(|d| *d != -13 && *d != -27));
    rows.push(closed(start_of_day(noon(-13)), 60.0));
    rows.push(closed(start_of_day(noon(-27)), 60.0));
    let s = seed_streak(&rows);
    assert_eq!(streak(&s), 28);
    assert_eq!(insights_streak(&s), 28);
}

#[test]
fn only_midnight_session_on_first_window_day() {
    let mut rows = one_each((0..=14).map(|d| -d).filter(|d| *d != -13));
    rows.push(closed(start_of_day(noon(-13)), 60.0));
    assert_eq!(streak(&seed_streak(&rows)), 15);
}

#[test]
fn missing_today_with_streak_crossing_first_window() {
    assert_eq!(streak(&seed_streak(&one_each((1..=15).map(|d| -d)))), 15);
}

#[test]
fn future_row_is_ignored() {
    assert_eq!(streak(&seed_streak(&one_each([1, -2]))), 0);
}

#[test]
fn actively_updating_open_session_counts_today() {
    let mut rows = vec![Row {
        started: add(&streak_now(), -120.0),
        duration: 0.0,
        open: true,
    }];
    rows.extend(one_each([-1]));
    assert_eq!(streak(&seed_streak(&rows)), 2);
}
