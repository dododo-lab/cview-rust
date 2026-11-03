//! 통계 기간 리포트 — 서머타임 경계와 DataStore 경유 집계.

mod common;

use chrono::TimeZone;
use chrono_tz::Tz;
use common::*;
use cview_persistence::models::WatchHistory;
use cview_persistence::time::{adding, Date};
use cview_persistence::watch_report::{BucketUnit, ReportPeriod};
use std::sync::Arc;

fn local(tz: Tz, y: i32, m: u32, d: u32, h: u32, min: u32) -> Date {
    tz.with_ymd_and_hms(y, m, d, h, min, 0)
        .earliest()
        .unwrap()
        .with_timezone(&chrono::Utc)
}

fn row(ch: &str, cat: &str, start: Date, secs: f64) -> WatchHistory {
    let mut w = WatchHistory::new(ch, format!("{ch} 채널"), None, Some(cat.into()), start);
    w.duration = secs;
    w.ended_at = Some(adding(&start, secs));
    w.updated_at = w.ended_at;
    w
}

#[test]
fn fall_back_day_has_25_hour_buckets_and_keeps_every_second() {
    let tz = chrono_tz::America::New_York;
    let s = store();
    s.set_calendar(Arc::new(tz));
    // 2026-11-01 01:00 이 두 번 온다 — 00:30 부터 3시간.
    let start = local(tz, 2026, 11, 1, 0, 30);
    s.insert_watch_history_rows(&[row("a", "게임", start, 3.0 * 3600.0)])
        .unwrap();
    let now = local(tz, 2026, 11, 1, 22, 0);
    let r = s.watch_report(ReportPeriod::Today, now).unwrap();
    assert_eq!(r.unit, BucketUnit::Hour);
    assert_eq!(r.trend.len(), 25);
    assert_eq!(r.total, 3.0 * 3600.0);
    let summed: f64 = r.trend.iter().map(|b| b.seconds).sum();
    assert_eq!(summed, r.total);
    // 현지 1시 칸은 두 번(1시간씩) 채워져 히트맵에서 합쳐진다. 2026-11-01 은 일요일(6).
    assert_eq!(r.heatmap[6][0], 1800.0);
    assert_eq!(r.heatmap[6][1], 7200.0);
    assert_eq!(r.heatmap[6][2], 1800.0);
}

#[test]
fn store_report_matches_records_and_ignores_zero_rows() {
    let tz = chrono_tz::Asia::Seoul;
    let s = store();
    s.set_calendar(Arc::new(tz));
    let now = local(tz, 2026, 11, 3, 21, 0);
    s.insert_watch_history_rows(&[
        row("a", "게임", local(tz, 2026, 11, 3, 19, 0), 3600.0),
        row("b", "토크", local(tz, 2026, 11, 1, 20, 0), 1800.0),
        row("b", "토크", local(tz, 2026, 10, 30, 20, 0), 0.0),
        row("c", "음악", local(tz, 2026, 10, 1, 20, 0), 900.0),
    ])
    .unwrap();
    let week = s.watch_report(ReportPeriod::Days7, now).unwrap();
    assert_eq!(week.total, 5400.0);
    assert_eq!(week.session_count, 2);
    assert_eq!(week.channel_count, 2);
    assert_eq!(week.channels[0].channel_id, "a");
    assert_eq!(week.active_days, 2);
    assert_eq!(week.current_streak, 1);

    let all = s.watch_report(ReportPeriod::All, now).unwrap();
    assert_eq!(all.total, 6300.0);
    assert_eq!(all.first_watched_at, Some(local(tz, 2026, 10, 1, 20, 0)));
    assert_eq!(all.unit, BucketUnit::Week);
    assert!(approx(
        all.trend.iter().map(|b| b.seconds).sum::<f64>(),
        6300.0,
        1e-6
    ));
}

#[test]
fn chatham_off_hour_offset_change_splits_at_the_transition() {
    // 2026-09-27 02:45(+12:45) → 03:45(+13:45). 02:00 부터 실제 1시간이면 2시 칸 45분, 3시 칸 15분.
    let tz = chrono_tz::Pacific::Chatham;
    let s = store();
    s.set_calendar(Arc::new(tz));
    let start = local(tz, 2026, 9, 27, 2, 0);
    s.insert_watch_history_rows(&[row("a", "게임", start, 3600.0)])
        .unwrap();
    let now = local(tz, 2026, 9, 27, 12, 0);
    let r = s.watch_report(ReportPeriod::Today, now).unwrap();
    assert_eq!(r.total, 3600.0);
    assert_eq!(r.heatmap[6][2], 2700.0);
    assert_eq!(r.heatmap[6][3], 900.0);
    assert_eq!(r.heatmap[6][4], 0.0);
}

#[test]
fn streak_survives_a_day_without_midnight() {
    // America/Havana 2026-03-08 은 0시가 없어 하루가 01:00 에 시작한다. 오늘 아직 안 봤어도 어제까지 이어진다.
    let tz = chrono_tz::America::Havana;
    let s = store();
    s.set_calendar(Arc::new(tz));
    s.insert_watch_history_rows(&[
        row("a", "게임", local(tz, 2026, 3, 6, 20, 0), 600.0),
        row("a", "게임", local(tz, 2026, 3, 7, 20, 0), 600.0),
    ])
    .unwrap();
    let now = local(tz, 2026, 3, 8, 12, 0);
    let r = s.watch_report(ReportPeriod::Days7, now).unwrap();
    assert_eq!(r.current_streak, 2);
    assert_eq!(r.best_streak, 2);
}

#[test]
fn chatham_backward_offset_change_splits_at_the_transition() {
    // 2026-04-05 03:45(+13:45) → 02:45(+12:45). 첫 03:00 부터 실제 1시간이면 3시 칸 45분, 2시 칸 15분.
    let tz = chrono_tz::Pacific::Chatham;
    let s = store();
    s.set_calendar(Arc::new(tz));
    let start = local(tz, 2026, 4, 5, 3, 0);
    s.insert_watch_history_rows(&[row("a", "게임", start, 3600.0)])
        .unwrap();
    let now = local(tz, 2026, 4, 5, 12, 0);
    let r = s.watch_report(ReportPeriod::Today, now).unwrap();
    let hour = |h: usize| r.heatmap.iter().map(|row| row[h]).sum::<f64>();
    assert_eq!(r.total, 3600.0);
    assert_eq!(hour(3), 2700.0);
    assert_eq!(hour(2), 900.0);
}

#[test]
fn previous_period_starts_at_the_shifted_day_start() {
    // Havana 3/8 은 01:00 에 시작한다 — 어제(3/7) 00:30 시청도 "어제 이 시각" 비교에 들어간다.
    let tz = chrono_tz::America::Havana;
    let s = store();
    s.set_calendar(Arc::new(tz));
    s.insert_watch_history_rows(&[row("a", "게임", local(tz, 2026, 3, 7, 0, 30), 1200.0)])
        .unwrap();
    let now = local(tz, 2026, 3, 8, 12, 0);
    let r = s.watch_report(ReportPeriod::Today, now).unwrap();
    assert_eq!(r.previous_total, Some(1200.0));
}
