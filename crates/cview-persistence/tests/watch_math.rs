//! 원본: `Tests/CViewPersistenceTests/WatchRollupMathTests.swift`, `WatchStreakEquivalenceTests.swift`,
//! `WatchInsightsScaleTests.swift`(기록 전용 — 단정 없음).
//!
//! 시드 난수는 SplitMix64 를 그대로 쓰되 범위 뽑기는 단순 나머지다(Swift `Int.random(in:using:)` 과
//! 같은 수열일 필요는 없다 — 결정적이기만 하면 된다). 달력은 `chrono_tz` 로 고정한다.

mod common;

use chrono::{TimeZone, Timelike};
use chrono_tz::Tz;
use common::*;
use cview_core::models::LiveChannelItem;
use cview_persistence::models::WatchHistory;
use cview_persistence::time::{adding, from_unix, Calendar, Date, TimeInterval};
use cview_persistence::watch_rollup_math::{
    InsightsResult, RollupBucket, WatchRecordSnapshot, WatchRollupMath,
};
use cview_persistence::DataStore;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

// MARK: - 시드 난수·픽스처

struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> i64 {
        (self.next() % n) as i64
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn reference_now() -> Date {
    from_unix(1_789_000_000.0)
}

fn snap(
    id: &str,
    name: &str,
    cat: Option<&str>,
    start: Date,
    end: Option<Date>,
    duration: f64,
    updated: Option<Date>,
) -> WatchRecordSnapshot {
    WatchRecordSnapshot::new(
        id,
        name,
        cat.map(str::to_string),
        start,
        end,
        duration,
        updated,
    )
}

fn records(
    count: usize,
    now: Date,
    cal: &dyn Calendar,
    seed: u64,
    span_days: i64,
) -> Vec<WatchRecordSnapshot> {
    let mut rng = SplitMix64(seed);
    let today = cal.start_of_day(now);
    let mut out = Vec::with_capacity(count + 16);
    for _ in 0..count {
        let offset = rng.below(span_days as u64);
        let day = cal.add_days(-offset, today).unwrap_or(today);
        let roll = rng.below(100);
        let second = match roll {
            0..=4 => 0,
            5..=9 => 86_399,
            _ => rng.below(86_400),
        };
        let start = adding(&day, second as f64);
        let duration = if rng.below(100) < 8 {
            0.0
        } else {
            1.0 + rng.unit() * 7_199.0
        };
        let n = rng.below(40);
        let category = match n % 7 {
            0 => None,
            1 => Some(String::new()),
            k => Some(format!("분류 {k}")),
        };
        let ended = adding(&start, duration);
        let updated = if rng.below(10) == 0 {
            None
        } else {
            Some(ended)
        };
        out.push(WatchRecordSnapshot::new(
            format!("ch{n}"),
            format!("채널 {n}"),
            category,
            start,
            Some(ended),
            duration,
            updated,
        ));
    }
    let cutoff7 = cal.add_days(-7, now).unwrap();
    let cutoff30 = cal.add_days(-30, now).unwrap();
    let closed = |out: &mut Vec<_>, id: &str, start: Date, d: f64, cat: Option<&str>| {
        out.push(snap(
            id,
            &format!("이름 {id}"),
            cat,
            start,
            Some(adding(&start, d)),
            d,
            Some(adding(&start, d)),
        ));
    };
    for (tag, cutoff) in [("w7", cutoff7), ("w30", cutoff30)] {
        closed(&mut out, &format!("{tag}-on"), cutoff, 111.5, Some("경계"));
        closed(
            &mut out,
            &format!("{tag}-before"),
            adding(&cutoff, -1.0),
            222.25,
            Some("경계"),
        );
        closed(
            &mut out,
            &format!("{tag}-after"),
            adding(&cutoff, 1.0),
            333.75,
            Some("경계"),
        );
        closed(
            &mut out,
            &format!("{tag}-dawn"),
            cal.start_of_day(cutoff),
            444.0,
            Some("경계"),
        );
    }
    let open = |out: &mut Vec<_>, id: &str, start: Date, updated: Date, d: f64| {
        out.push(snap(
            id,
            &format!("이름 {id}"),
            Some("열림"),
            start,
            None,
            d,
            Some(updated),
        ));
    };
    open(
        &mut out,
        "open-a",
        adding(&now, -100.0),
        adding(&now, -10.0),
        30.0,
    );
    open(
        &mut out,
        "open-b",
        adding(&now, -40.0),
        adding(&now, -5.0),
        0.0,
    );
    open(
        &mut out,
        "open-c",
        adding(&now, -7_200.0),
        adding(&now, -3_600.0),
        500.0,
    );
    open(
        &mut out,
        "open-d",
        adding(&now, -9_000.0),
        adding(&now, -8_000.0),
        0.0,
    );
    open(
        &mut out,
        "open-e",
        adding(&cutoff7, 60.0),
        adding(&now, -20.0),
        90.0,
    );
    closed(
        &mut out,
        "future",
        adding(&now, 2.0 * 86_400.0),
        900.0,
        None,
    );
    closed(&mut out, "zero", adding(&now, -3_600.0), 0.0, Some(""));
    closed(&mut out, "midnight", today, 60.0, None);
    out
}

fn rollup(records: &[WatchRecordSnapshot], now: Date, cal: &dyn Calendar) -> InsightsResult {
    let buckets = WatchRollupMath::buckets(records, cal);
    let cutoff7 = cal.add_days(-7, now).unwrap();
    let cutoff30 = cal.add_days(-30, now).unwrap();
    let boundary: HashSet<Date> = [cal.start_of_day(cutoff7), cal.start_of_day(cutoff30)].into();
    let open: Vec<_> = records
        .iter()
        .filter(|r| r.ended_at.is_none())
        .cloned()
        .collect();
    let edge: Vec<_> = records
        .iter()
        .filter(|r| boundary.contains(&cal.start_of_day(r.started_at)))
        .cloned()
        .collect();
    WatchRollupMath::insights(&buckets, &open, &edge, now, cal)
}

/// 원시 전량 기준 구현(현행 aggregateInsights 와 같은 규칙).
fn reference(records: &[WatchRecordSnapshot], now: Date, cal: &dyn Calendar) -> InsightsResult {
    let cutoff7 = cal.add_days(-7, now).unwrap();
    let cutoff30 = cal.add_days(-30, now).unwrap();
    let earliest30 = cal.start_of_day(cal.add_days(-29, now).unwrap());
    let (mut total, mut w7, mut w30) = (0.0, 0.0, 0.0);
    let mut channel_map: HashMap<String, (String, f64)> = HashMap::new();
    let mut category_map: HashMap<String, f64> = HashMap::new();
    let mut hourly: BTreeMap<u32, f64> = BTreeMap::new();
    let mut daily_map: HashMap<Date, f64> = HashMap::new();
    let mut streak_days: HashSet<Date> = HashSet::new();
    let mut valid = 0;
    for r in records {
        let d = r.effective_seconds(now);
        total += d;
        if r.started_at >= cutoff7 {
            w7 += d;
        }
        if r.started_at >= cutoff30 {
            w30 += d;
        }
        if d <= 0.0 {
            continue;
        }
        valid += 1;
        let e = channel_map
            .entry(r.channel_id.clone())
            .or_insert((r.channel_name.clone(), 0.0));
        e.1 += d;
        *category_map
            .entry(LiveChannelItem::display_category_name(
                r.category_name.as_deref(),
            ))
            .or_default() += d;
        *hourly.entry(cal.hour(r.started_at)).or_default() += d;
        let day = cal.start_of_day(r.started_at);
        streak_days.insert(day);
        if r.started_at >= earliest30 {
            *daily_map.entry(day).or_default() += d;
        }
    }
    let sort = |rows: &mut Vec<(String, f64)>| {
        rows.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then_with(|| a.0.cmp(&b.0)))
    };
    let mut channels: Vec<_> = channel_map.values().cloned().collect();
    sort(&mut channels);
    channels.truncate(10);
    let mut categories: Vec<_> = category_map.into_iter().collect();
    sort(&mut categories);
    categories.truncate(8);
    let today = cal.start_of_day(now);
    let mut daily: Vec<_> = (0..30)
        .filter_map(|o| cal.add_days(-o, today))
        .map(|day| (day, daily_map.get(&day).copied().unwrap_or(0.0)))
        .collect();
    daily.reverse();
    let mut cursor = today;
    if !streak_days.contains(&cursor) {
        cursor = cal.add_days(-1, cursor).unwrap();
    }
    let mut streak = 0;
    while streak_days.contains(&cursor) {
        streak += 1;
        cursor = cal.add_days(-1, cursor).unwrap();
    }
    InsightsResult {
        all: total,
        w7,
        w30,
        streak,
        avg: if valid == 0 {
            0.0
        } else {
            total / valid as f64
        },
        channels,
        categories,
        hourly,
        daily,
        session_count: records.len() as i64,
        counted_session_count: valid,
        unique_channel_count: channel_map.len(),
        active_session_count: records
            .iter()
            .filter(|r| r.is_actively_updating(now))
            .count() as i64,
        last_updated_at: records
            .iter()
            .map(|r| r.updated_at.or(r.ended_at).unwrap_or(r.started_at))
            .max(),
    }
}

fn assert_equivalent(actual: &InsightsResult, expected: &InsightsResult) {
    let tol = 1.0;
    let close = |a: f64, e: f64, what: &str| assert!((a - e).abs() <= tol, "{what}: {a} vs {e}");
    close(actual.all, expected.all, "all");
    close(actual.w7, expected.w7, "w7");
    close(actual.w30, expected.w30, "w30");
    close(actual.avg, expected.avg, "avg");
    assert_eq!(actual.streak, expected.streak, "streak");
    assert_eq!(actual.session_count, expected.session_count, "sessionCount");
    assert_eq!(
        actual.counted_session_count, expected.counted_session_count,
        "counted"
    );
    assert_eq!(
        actual.unique_channel_count, expected.unique_channel_count,
        "unique"
    );
    assert_eq!(
        actual.active_session_count, expected.active_session_count,
        "active"
    );
    assert_eq!(
        actual.last_updated_at, expected.last_updated_at,
        "lastUpdatedAt"
    );
    assert_eq!(actual.daily.len(), expected.daily.len(), "daily.count");
    for (a, e) in actual.daily.iter().zip(&expected.daily) {
        assert_eq!(a.0, e.0, "daily.day");
        close(a.1, e.1, "daily");
    }
    assert_eq!(
        actual.hourly.keys().collect::<Vec<_>>(),
        expected.hourly.keys().collect::<Vec<_>>(),
        "hourly keys"
    );
    for (h, e) in &expected.hourly {
        close(actual.hourly[h], *e, &format!("hour {h}"));
    }
    assert_eq!(
        actual.categories.len(),
        expected.categories.len(),
        "categories.count"
    );
    for (a, e) in actual.categories.iter().zip(&expected.categories) {
        assert_eq!(a.0, e.0);
        close(a.1, e.1, &format!("category {}", e.0));
    }
    assert_eq!(
        actual.channels.len(),
        expected.channels.len(),
        "channels.count"
    );
    for (a, e) in actual.channels.iter().zip(&expected.channels) {
        assert_eq!(a.0, e.0);
        close(a.1, e.1, &format!("channel {}", e.0));
    }
}

fn local(tz: Tz, y: i32, m: u32, d: u32, h: u32) -> Date {
    tz.with_ymd_and_hms(y, m, d, h, 0, 0)
        .single()
        .unwrap()
        .with_timezone(&chrono::Utc)
}

// MARK: - 달력별 원시 전량 대비

#[test]
fn seoul_1k_matches_raw_reference() {
    let cal = chrono_tz::Asia::Seoul;
    let now = reference_now();
    let rows = records(1_000, now, &cal, 0xC0FFEE, 120);
    assert_equivalent(&rollup(&rows, now, &cal), &reference(&rows, now, &cal));
}

#[test]
fn seoul_20k_matches_raw_reference() {
    let cal = chrono_tz::Asia::Seoul;
    let now = reference_now();
    let rows = records(20_000, now, &cal, 0xBADC0DE, 120);
    assert_equivalent(&rollup(&rows, now, &cal), &reference(&rows, now, &cal));
}

#[test]
fn new_york_spring_forward_window_matches_raw_reference() {
    let cal = chrono_tz::America::New_York;
    let now = local(cal, 2026, 3, 12, 12);
    let rows = records(2_000, now, &cal, 42, 90);
    assert_equivalent(&rollup(&rows, now, &cal), &reference(&rows, now, &cal));
}

#[test]
fn new_york_fall_back_window_matches_raw_reference() {
    let cal = chrono_tz::America::New_York;
    let now = local(cal, 2026, 11, 4, 0);
    let rows = records(2_000, now, &cal, 7, 90);
    assert_equivalent(&rollup(&rows, now, &cal), &reference(&rows, now, &cal));
}

// MARK: - DataStore 대비

fn seeded_store(rows: &[WatchRecordSnapshot], cal: Tz) -> DataStore {
    let s = store();
    s.set_calendar(Arc::new(cal));
    let history: Vec<_> = rows
        .iter()
        .map(|r| {
            let mut w = WatchHistory::new(
                r.channel_id.clone(),
                r.channel_name.clone(),
                None,
                r.category_name.clone(),
                r.started_at,
            );
            w.ended_at = r.ended_at;
            w.duration = r.duration;
            w.updated_at = r.updated_at;
            w
        })
        .collect();
    s.insert_watch_history_rows(&history).unwrap();
    s
}

fn compare_with_store(count: usize, seed: u64) {
    let cal = chrono_tz::Asia::Seoul;
    let now = reference_now();
    let rows = records(count, now, &cal, seed, 120);
    let raw = seeded_store(&rows, cal).aggregate_insights(now).unwrap();
    let expected = InsightsResult {
        all: raw.all,
        w7: raw.w7,
        w30: raw.w30,
        streak: raw.streak,
        avg: raw.avg,
        channels: raw.channels,
        categories: raw.categories,
        hourly: raw.hourly,
        daily: raw.daily,
        session_count: raw.session_count as i64,
        counted_session_count: raw.counted_session_count as i64,
        unique_channel_count: raw.unique_channel_count,
        active_session_count: raw.active_session_count as i64,
        last_updated_at: raw.last_updated_at,
    };
    assert_equivalent(&rollup(&rows, now, &cal), &expected);
}

#[test]
fn rollup_matches_data_store_1k() {
    compare_with_store(1_000, 0x1000);
}

#[test]
fn rollup_matches_data_store_20k() {
    compare_with_store(20_000, 0x20000);
}

// MARK: - 개별 규칙

#[test]
fn bucket_holds_persisted_share_and_insights_add_open_increment() {
    let cal = chrono_tz::Asia::Seoul;
    let now = local(cal, 2026, 9, 10, 15);
    let open = snap(
        "a",
        "A",
        Some("게임"),
        adding(&now, -100.0),
        None,
        30.0,
        Some(adding(&now, -10.0)),
    );
    let buckets = WatchRollupMath::buckets(std::slice::from_ref(&open), &cal);
    assert_eq!(buckets.len(), 1);
    assert!(approx(buckets[0].seconds, 30.0, 0.001));
    assert_eq!(buckets[0].sessions, 1);
    assert_eq!(buckets[0].raw_sessions, 1);
    let result = WatchRollupMath::insights(&buckets, &[open], &[], now, &cal);
    assert!(approx(result.all, 100.0, 0.001));
    assert_eq!(result.active_session_count, 1);
}

#[test]
fn expired_open_record_keeps_persisted_share() {
    let cal = chrono_tz::Asia::Seoul;
    let now = local(cal, 2026, 9, 10, 15);
    let stale = snap(
        "a",
        "A",
        None,
        adding(&now, -7_200.0),
        None,
        500.0,
        Some(adding(&now, -3_600.0)),
    );
    let result = rollup(&[stale], now, &cal);
    assert!(approx(result.all, 500.0, 0.001));
    assert_eq!(result.active_session_count, 0);
}

#[test]
fn bucket_key_and_category_label() {
    let cal = chrono_tz::Asia::Seoul;
    let start = local(cal, 2026, 9, 10, 9);
    let rows = [
        snap(
            "c",
            "C",
            None,
            start,
            Some(adding(&start, 10.0)),
            10.0,
            None,
        ),
        snap(
            "c",
            "C",
            Some(""),
            adding(&start, 20.0),
            Some(adding(&start, 30.0)),
            10.0,
            None,
        ),
    ];
    let buckets = WatchRollupMath::buckets(&rows, &cal);
    assert_eq!(buckets.len(), 1);
    let day = cal.start_of_day(start);
    assert_eq!(
        buckets[0].key,
        RollupBucket::make_key(&day, "c", LiveChannelItem::UNCATEGORIZED_DISPLAY_NAME)
    );
    assert_eq!(
        buckets[0].key,
        format!(
            "{}|c|{}",
            day.timestamp(),
            LiveChannelItem::UNCATEGORIZED_DISPLAY_NAME
        )
    );
    assert!(approx(buckets[0].seconds, 20.0, 0.001));
    assert_eq!(buckets[0].hour_seconds.len(), 24);
    assert!(approx(buckets[0].hour_seconds[9], 20.0, 0.001));
}

#[test]
fn channel_name_uses_latest_record_name() {
    let cal = chrono_tz::Asia::Seoul;
    let now = local(cal, 2026, 9, 10, 15);
    let older = adding(&now, -5.0 * 86_400.0);
    let newer = adding(&now, -86_400.0);
    let rows = [
        snap(
            "c",
            "옛 이름",
            Some("x"),
            older,
            Some(adding(&older, 100.0)),
            100.0,
            None,
        ),
        snap(
            "c",
            "새 이름",
            Some("y"),
            newer,
            Some(adding(&newer, 100.0)),
            100.0,
            None,
        ),
    ];
    let result = rollup(&rows, now, &cal);
    assert_eq!(
        result
            .channels
            .iter()
            .map(|c| c.0.as_str())
            .collect::<Vec<_>>(),
        vec!["새 이름"]
    );
    assert!(approx(result.channels[0].1, 200.0, 0.001));
}

#[test]
fn empty_input_yields_zeros() {
    let cal = chrono_tz::Asia::Seoul;
    let result = rollup(&[], reference_now(), &cal);
    assert_eq!(result.all, 0.0);
    assert_eq!(result.streak, 0);
    assert_eq!(result.avg, 0.0);
    assert_eq!(result.daily.len(), 30);
    assert!(result.channels.is_empty());
    assert!(result.last_updated_at.is_none());
}

// MARK: - streak 동등성(DataStore)

fn streak_store() -> (DataStore, Date) {
    let s = store();
    let cal = chrono_tz::Asia::Seoul;
    s.set_calendar(Arc::new(cal));
    // 서울 정오 기준의 "지금" — 자정 경계를 피한다.
    let n = now().with_timezone(&cal);
    let noon = cal
        .from_local_datetime(&n.date_naive().and_hms_opt(12, 0, 0).unwrap())
        .single()
        .unwrap()
        .with_timezone(&chrono::Utc);
    assert_eq!(noon.with_timezone(&cal).hour(), 12);
    (s, noon)
}

fn noon(base: Date, offset: i64) -> Date {
    adding(&base, offset as f64 * 86_400.0)
}

fn add_closed(s: &DataStore, id: &str, started: Date, duration: TimeInterval) {
    s.start_watch_record(id, &format!("채널-{id}"), None, None, started)
        .unwrap();
    assert!(s
        .end_watch_record(id, started, adding(&started, duration))
        .unwrap());
}

#[test]
fn streak_counts_consecutive_days_from_today() {
    let (s, base) = streak_store();
    for o in [0, -1, -2] {
        add_closed(&s, &format!("ch{o}"), noon(base, o), 600.0);
    }
    add_closed(&s, "old", noon(base, -4), 600.0);
    assert_eq!(s.current_watch_streak(base).unwrap(), 3);
}

#[test]
fn streak_allows_missing_today_once() {
    let (s, base) = streak_store();
    for o in [-1, -2] {
        add_closed(&s, &format!("ch{o}"), noon(base, o), 600.0);
    }
    assert_eq!(s.current_watch_streak(base).unwrap(), 2);
}

#[test]
fn streak_is_zero_when_gap_before_yesterday() {
    let (s, base) = streak_store();
    add_closed(&s, "ch", noon(base, -2), 600.0);
    assert_eq!(s.current_watch_streak(base).unwrap(), 0);
}

#[test]
fn zero_duration_day_does_not_extend_streak() {
    let (s, base) = streak_store();
    add_closed(&s, "today", noon(base, 0), 600.0);
    add_closed(&s, "zero", noon(base, -1), 0.0);
    add_closed(&s, "before", noon(base, -2), 600.0);
    assert_eq!(s.current_watch_streak(base).unwrap(), 1);
}

#[test]
fn streak_spans_many_rows() {
    let (s, base) = streak_store();
    let days = 105;
    for d in 0..days {
        for i in 0..5 {
            add_closed(
                &s,
                &format!("d{d}-{i}"),
                adding(&noon(base, -d), i as f64 * 600.0),
                300.0,
            );
        }
    }
    assert_eq!(s.current_watch_streak(base).unwrap(), days);
}

#[test]
fn unsynced_count_matches_row_state() {
    let (s, base) = streak_store();
    add_closed(&s, "a", noon(base, 0), 60.0);
    add_closed(&s, "b", noon(base, -1), 60.0);
    assert_eq!(s.unsynced_watch_history_count().unwrap(), 2);
    let rows = s.fetch_unsynced_watch_history(10).unwrap();
    assert_eq!(rows.len(), 2);
    s.mark_watch_history_synced(&rows.iter().map(|r| (r.id.clone(), r.updated_at)).collect())
        .unwrap();
    assert_eq!(s.unsynced_watch_history_count().unwrap(), 0);
    s.start_watch_record("c", "채널c", None, None, noon(base, 0))
        .unwrap();
    assert_eq!(s.unsynced_watch_history_count().unwrap(), 1);
}

#[test]
fn row_changed_during_push_stays_queued() {
    let s = store();
    let started = adding(&now(), -600.0);
    s.start_watch_record("open", "열린 채널", None, None, started)
        .unwrap();
    let pushed = s.fetch_unsynced_watch_history(10).unwrap();
    assert_eq!(pushed.len(), 1);
    std::thread::sleep(std::time::Duration::from_millis(2));
    assert!(s.end_watch_record("open", started, now()).unwrap());
    s.mark_watch_history_synced(
        &pushed
            .iter()
            .map(|r| (r.id.clone(), r.updated_at))
            .collect(),
    )
    .unwrap();
    let queued = s.fetch_unsynced_watch_history(10).unwrap();
    assert_eq!(queued.len(), 1);
    assert!(queued[0].ended_at.is_some());
}

// MARK: - 규모 기준선(기록 전용)

#[test]
fn record_aggregate_insights_duration() {
    let cal = chrono_tz::Asia::Seoul;
    let now = reference_now();
    for rows in [1_000usize, 20_000] {
        let records = records(rows, now, &cal, rows as u64, 120);
        let s = seeded_store(&records, cal);
        let started = std::time::Instant::now();
        let result = s.aggregate_insights(now).unwrap();
        println!(
            "[ROLLUP-MATH] aggregate_insights rows={} elapsed={:?}",
            records.len(),
            started.elapsed()
        );
        assert_eq!(result.session_count, records.len());
    }
}
