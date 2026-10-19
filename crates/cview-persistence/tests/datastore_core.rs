//! 원본: `Tests/CViewPersistenceTests/DataStoreTests.swift`.

mod common;

use common::*;
use cview_core::models::settings::PlayerSettings;
use cview_core::models::ChannelInfo;
use cview_persistence::models::WatchHistoryAggregationPolicy;
use cview_persistence::time::interval_since;
use cview_persistence::{DataStore, PersistenceError};
use std::path::PathBuf;

fn info(id: &str, name: &str) -> ChannelInfo {
    ChannelInfo::new(id, name)
}

// MARK: - 채널

#[test]
fn save_and_fetch_channel() {
    let s = store();
    s.save_channel(&info("ch1", "채널1"), false).unwrap();
    let row = s.fetch_channel("ch1").unwrap().expect("row");
    assert_eq!(row.channel_name, "채널1");
    assert!(!row.is_favorite);
}

#[test]
fn save_channel_updates_name() {
    let s = store();
    s.save_channel(&info("ch1", "이전"), false).unwrap();
    s.save_channel(&info("ch1", "새이름"), false).unwrap();
    assert_eq!(
        s.fetch_channel("ch1").unwrap().unwrap().channel_name,
        "새이름"
    );
    assert_eq!(s.table_row_count("PersistedChannel").unwrap(), 1);
}

#[test]
fn toggle_favorite() {
    let s = store();
    s.save_channel(&info("ch1", "채널1"), false).unwrap();
    assert!(s.toggle_favorite("ch1").unwrap());
    assert!(s.is_favorite("ch1").unwrap());
    assert!(!s.toggle_favorite("ch1").unwrap());
    assert!(!s.is_favorite("ch1").unwrap());
    assert!(!s.toggle_favorite("missing").unwrap());
}

#[test]
fn favorite_items_sorted_by_name() {
    let s = store();
    s.save_channel(&info("b", "B채널"), true).unwrap();
    s.save_channel(&info("a", "A채널"), true).unwrap();
    s.save_channel(&info("c", "C채널"), false).unwrap();
    let favs = s.fetch_favorite_items().unwrap();
    let names: Vec<_> = favs.iter().map(|f| f.channel_name.as_str()).collect();
    assert_eq!(names, vec!["A채널", "B채널"]);
}

#[test]
fn recent_items_respect_limit() {
    let s = store();
    for i in 0..5 {
        let id = format!("ch{i}");
        s.save_channel(&info(&id, &id), false).unwrap();
        s.update_last_watched(&id).unwrap();
    }
    assert_eq!(s.fetch_recent_items(3).unwrap().len(), 3);
}

// MARK: - 메모

#[test]
fn memo_round_trip_and_empty_clears() {
    let s = store();
    s.save_channel(&info("ch1", "채널1"), false).unwrap();
    s.save_memo("ch1", "메모").unwrap();
    assert_eq!(s.fetch_memo("ch1").unwrap(), "메모");
    s.save_memo("ch1", "").unwrap();
    assert_eq!(s.fetch_memo("ch1").unwrap(), "");
    assert!(s.fetch_channel("ch1").unwrap().unwrap().memo.is_none());
}

// MARK: - 설정

#[test]
fn settings_round_trip() {
    let s = store();
    s.save_setting("str", "hello").unwrap();
    s.save_setting("int", &42).unwrap();
    s.save_setting("bool", &true).unwrap();
    assert_eq!(
        s.load_setting::<String>("str").unwrap().as_deref(),
        Some("hello")
    );
    assert_eq!(s.load_setting::<i64>("int").unwrap(), Some(42));
    assert_eq!(s.load_setting::<bool>("bool").unwrap(), Some(true));
    s.save_setting("int", &7).unwrap();
    assert_eq!(s.load_setting::<i64>("int").unwrap(), Some(7));
    assert_eq!(s.load_setting::<i64>("missing").unwrap(), None);
}

#[test]
fn player_settings_round_trip() {
    let s = store();
    let player = PlayerSettings::default();
    s.save_setting("player", &player).unwrap();
    let loaded: PlayerSettings = s.load_setting("player").unwrap().unwrap();
    assert_eq!(loaded, player);
}

#[test]
fn save_statistic_links_channel() {
    let s = store();
    s.save_channel(&info("ch1", "채널1"), false).unwrap();
    s.save_statistic("ch1", 100, Some(1.5)).unwrap();
    assert_eq!(s.table_row_count("PersistedStatistic").unwrap(), 1);
}

// MARK: - 시청 기록

#[test]
fn start_watch_record_updates_recent() {
    let s = store();
    let started = s
        .start_watch_record("ch1", "채널1", None, Some("게임"), ts(1_750_000_000.0))
        .unwrap();
    assert_eq!(started, ts(1_750_000_000.0));
    let recent = s.fetch_recent_items(10).unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].last_watched, Some(started));
}

#[test]
fn end_watch_record_sets_duration() {
    let s = store();
    let started = s
        .start_watch_record("ch1", "채널1", None, None, now())
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(s.end_watch_record("ch1", started, now()).unwrap());
    let rows = s.all_watch_history_rows().unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].duration > 0.0);
    assert!(rows[0].ended_at.is_some());
}

#[test]
fn cleanup_zero_days_empties() {
    let s = store();
    let started = s
        .start_watch_record("ch1", "채널1", None, None, add(&now(), -10.0))
        .unwrap();
    s.end_watch_record("ch1", started, now()).unwrap();
    s.cleanup_old_watch_history(0).unwrap();
    assert_eq!(s.table_row_count("WatchHistory").unwrap(), 0);
}

#[test]
fn aggregate_counts_active_open_session() {
    let s = store();
    let n = now();
    let started = add(&n, -120.0);
    s.start_watch_record("ch1", "채널1", None, Some("게임"), started)
        .unwrap();
    let insights = s.aggregate_insights(n).unwrap();
    assert!(approx(insights.all, 120.0, 0.1), "all={}", insights.all);
    assert_eq!(insights.active_session_count, 1);
    assert_eq!(insights.channels[0].0, "채널1");
    assert_eq!(insights.categories[0].0, "게임");
}

#[test]
fn checkpoint_then_recover_keeps_checkpoint() {
    let s = store();
    let base = add(&now(), -1000.0);
    let started = s
        .start_watch_record("ch1", "채널1", None, None, base)
        .unwrap();
    let checkpoint = add(&base, 75.0);
    assert!(s
        .checkpoint_watch_record("ch1", started, checkpoint)
        .unwrap());
    assert_eq!(
        s.recover_interrupted_watch_records(add(&base, 180.0))
            .unwrap(),
        1
    );
    let row = &s.all_watch_history_rows().unwrap()[0];
    assert!(approx(row.duration, 75.0, 0.001));
    assert_eq!(row.ended_at, Some(checkpoint));
}

#[test]
fn stale_open_session_uses_persisted_duration() {
    let start = ts(1_000_000.0);
    let updated = add(&start, 60.0);
    let n = add(&start, 600.0);
    let d = WatchHistoryAggregationPolicy::effective_duration(start, None, 60.0, Some(updated), n);
    assert_eq!(d, 60.0);
    assert!(interval_since(&n, &start) > 60.0);
}

// MARK: - 경로

#[test]
fn app_support_failure_skips_create() {
    let mut created = false;
    let r = DataStore::resolve_store_url_with(
        || Err(PersistenceError::Other("no app support".into())),
        |_| {
            created = true;
            Ok(())
        },
    );
    assert!(r.is_err());
    assert!(!created);
}

#[test]
fn create_directory_failure_propagates() {
    let r = DataStore::resolve_store_url_with(
        || Ok(PathBuf::from("/nonexistent")),
        |_| Err(PersistenceError::Other("denied".into())),
    );
    assert!(r.is_err());
}

#[test]
fn resolved_path_is_stable() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().to_path_buf();
    let resolve = || {
        DataStore::resolve_store_url_with(
            || Ok(base.clone()),
            |d| std::fs::create_dir_all(d).map_err(Into::into),
        )
    };
    let a = resolve().unwrap();
    let b = resolve().unwrap();
    assert_eq!(a, b);
    assert_eq!(a.file_name().unwrap(), "CView_v2.store");
    assert!(base.join("CView").is_dir());
}

#[test]
fn file_in_place_of_directory_errors_then_recovers() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().to_path_buf();
    let blocker = base.join("CView");
    std::fs::write(&blocker, b"keep").unwrap();
    let resolve = || {
        DataStore::resolve_store_url_with(
            || Ok(base.clone()),
            |d| std::fs::create_dir_all(d).map_err(Into::into),
        )
    };
    assert!(resolve().is_err());
    assert_eq!(std::fs::read(&blocker).unwrap(), b"keep");
    std::fs::remove_file(&blocker).unwrap();
    assert!(resolve().is_ok());
}
