//! 원본: `Tests/CViewPersistenceTests/DataStoreInsightsFingerprintTests.swift`,
//! `DataStoreInsightsRevisionTests.swift`, `DataStoreLegacyCleanupTests.swift`.
//!
//! 소스 텍스트를 검사하던 `testFingerprintDoesNotFetchAllRows` 는 Swift 구현 문자열에 묶인
//! 검사라 옮기지 않았다(Rust 구현은 COUNT/MAX 집계 질의만 쓴다).

mod common;

use common::*;
use cview_persistence::models::{
    CachedCategoryStat, CachedClipData, CachedLiveChannelData, WatchHistory,
};
use cview_persistence::store::legacy_cleanup::{
    LegacyCleanupResult, LEGACY_WRITE_ONLY_CLEANUP_KEY,
};
use cview_persistence::store::watch_history_sync::WatchHistoryFingerprint;
use cview_persistence::WatchInsightsRevision;
use std::collections::{HashMap, HashSet};

fn empty_fp() -> WatchHistoryFingerprint {
    WatchHistoryFingerprint {
        row_count: 0,
        open_count: 0,
        max_updated_at: None,
        max_started_at: None,
    }
}

fn row(
    id: &str,
    started: cview_persistence::time::Date,
    ended: Option<cview_persistence::time::Date>,
    duration: f64,
    updated: Option<cview_persistence::time::Date>,
) -> WatchHistory {
    let mut r = WatchHistory::new(id, id.to_uppercase(), None, None, started);
    r.ended_at = ended;
    r.duration = duration;
    r.updated_at = updated;
    r
}

// MARK: - 지문

#[test]
fn empty_store_fingerprint() {
    assert_eq!(store().watch_history_fingerprint().unwrap(), empty_fp());
}

#[test]
fn fingerprint_reflects_counts_and_latest_times() {
    let s = store();
    let base = ts(1_750_000_000.0);
    s.insert_watch_history_rows(&[
        row(
            "a",
            base,
            Some(add(&base, 600.0)),
            600.0,
            Some(add(&base, 600.0)),
        ),
        row(
            "b",
            add(&base, 1_000.0),
            None,
            0.0,
            Some(add(&base, 1_030.0)),
        ),
        row("c", add(&base, -5_000.0), Some(base), 5_000.0, None),
    ])
    .unwrap();
    let fp = s.watch_history_fingerprint().unwrap();
    assert_eq!(fp.row_count, 3);
    assert_eq!(fp.open_count, 1);
    assert_eq!(fp.max_updated_at, Some(add(&base, 1_030.0)));
    assert_eq!(fp.max_started_at, Some(add(&base, 1_000.0)));
}

#[test]
fn every_aggregating_write_changes_fingerprint() {
    let s = store();
    let n = now();
    let started = s
        .start_watch_record("c1", "채널", None, None, add(&n, -300.0))
        .unwrap();
    let after_start = s.watch_history_fingerprint().unwrap();
    assert_eq!(after_start, s.watch_history_fingerprint().unwrap());
    assert_eq!(after_start.open_count, 1);

    s.checkpoint_watch_record("c1", started, add(&n, 60.0))
        .unwrap();
    let after_checkpoint = s.watch_history_fingerprint().unwrap();
    assert_ne!(after_checkpoint, after_start);

    s.end_watch_record("c1", started, add(&n, 120.0)).unwrap();
    let after_end = s.watch_history_fingerprint().unwrap();
    assert_ne!(after_end, after_checkpoint);
    assert_eq!(after_end.open_count, 0);

    s.start_watch_record("c2", "열림", None, None, add(&n, -30.0))
        .unwrap();
    s.recover_interrupted_watch_records(add(&n, 600.0)).unwrap();
    let after_recover = s.watch_history_fingerprint().unwrap();
    assert_eq!(after_recover.open_count, 0);
    assert_eq!(after_recover.max_updated_at, Some(add(&n, 600.0)));

    s.start_watch_record("old", "옛", None, None, add(&n, -400.0 * 86_400.0))
        .unwrap();
    let before_cleanup = s.watch_history_fingerprint().unwrap();
    s.cleanup_old_watch_history(365).unwrap();
    let after_cleanup = s.watch_history_fingerprint().unwrap();
    assert_eq!(after_cleanup.row_count, before_cleanup.row_count - 1);

    s.delete_all_watch_history().unwrap();
    assert_eq!(s.watch_history_fingerprint().unwrap(), empty_fp());
}

#[test]
fn sync_mark_does_not_change_fingerprint() {
    let s = store();
    let base = ts(1_750_000_000.0);
    let r = row(
        "a",
        base,
        Some(add(&base, 60.0)),
        60.0,
        Some(add(&base, 60.0)),
    );
    s.insert_watch_history_rows(std::slice::from_ref(&r))
        .unwrap();
    let before = s.watch_history_fingerprint().unwrap();
    let id = cview_persistence::models::WatchHistorySyncRow::make_id("a", &base);
    s.mark_watch_history_synced(&HashMap::from([(id, add(&base, 60.0))]))
        .unwrap();
    assert!(s.all_watch_history_rows().unwrap()[0].synced_at.is_some());
    assert_eq!(s.watch_history_fingerprint().unwrap(), before);
}

#[test]
fn fingerprint_json_round_trip() {
    let fp = WatchHistoryFingerprint {
        row_count: 1_234,
        open_count: 1,
        max_updated_at: Some(ts(1_750_000_123.456)),
        max_started_at: Some(ts(1_749_999_000.5)),
    };
    let json = serde_json::to_vec(&fp).unwrap();
    let decoded: WatchHistoryFingerprint = serde_json::from_slice(&json).unwrap();
    assert_eq!(decoded, fp);
}

// MARK: - 인사이트 리비전

fn assert_bumps(label: &str, op: impl FnOnce()) {
    let before = WatchInsightsRevision::current();
    op();
    assert!(
        WatchInsightsRevision::current() > before,
        "{label} 가 리비전을 올리지 않았다"
    );
}

/// 전역 카운터라 병렬 테스트의 쓰기가 끼면 그 시도를 버리고 다시 잰다.
fn assert_no_bump(label: &str, mut op: impl FnMut()) {
    for _ in 0..20 {
        let before = WatchInsightsRevision::current();
        op();
        if WatchInsightsRevision::current() == before {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("{label} 가 바뀐 것이 없는데 리비전을 올렸다(20회 연속)");
}

#[test]
fn delete_all_bumps_only_when_rows_deleted() {
    let s = store();
    assert_no_bump("빈 deleteAll", || s.delete_all_watch_history().unwrap());
    s.start_watch_record("c1", "채널", None, None, add(&now(), -60.0))
        .unwrap();
    assert_bumps("deleteAll", || s.delete_all_watch_history().unwrap());
    assert!(s.fetch_watch_history(100).unwrap().is_empty());
    assert_no_bump("두 번째 deleteAll", || {
        s.delete_all_watch_history().unwrap()
    });
}

#[test]
fn cleanup_bumps_only_when_rows_deleted() {
    let s = store();
    s.start_watch_record("new", "새", None, None, add(&now(), -60.0))
        .unwrap();
    assert_no_bump("지울 것 없는 cleanup", || {
        s.cleanup_old_watch_history(365).unwrap()
    });
    s.start_watch_record("old", "옛", None, None, add(&now(), -400.0 * 86_400.0))
        .unwrap();
    assert_bumps("cleanup", || s.cleanup_old_watch_history(365).unwrap());
    let left: Vec<_> = s
        .fetch_watch_history(100)
        .unwrap()
        .into_iter()
        .map(|r| r.channel_id)
        .collect();
    assert_eq!(left, vec!["new"]);
}

#[test]
fn recover_bumps_only_when_rows_recovered() {
    let s = store();
    assert_no_bump("열린 행 없는 recover", || {
        s.recover_interrupted_watch_records(now()).unwrap();
    });
    s.start_watch_record("c1", "채널", None, None, add(&now(), -120.0))
        .unwrap();
    assert_bumps("recover", || {
        assert_eq!(s.recover_interrupted_watch_records(now()).unwrap(), 1)
    });
    assert_no_bump("복구 뒤 recover", || {
        s.recover_interrupted_watch_records(now()).unwrap();
    });
}

#[test]
fn bookmark_save_and_delete_bump_but_missing_delete_does_not() {
    let s = store();
    let mut saved = None;
    assert_bumps("saveBookmark", || {
        saved = Some(
            s.save_bookmark("c1", "채널", "제목", None, None, 30.0, None)
                .unwrap(),
        )
    });
    let id = saved.unwrap().id;
    assert_no_bump("없는 deleteBookmark", || {
        s.delete_bookmark("missing|0").unwrap()
    });
    assert_bumps("deleteBookmark", || s.delete_bookmark(&id).unwrap());
    assert_eq!(s.bookmark_count().unwrap(), 0);
}

#[test]
fn clear_all_last_watched_does_not_bump() {
    let s = store();
    s.start_watch_record("c1", "채널", None, None, add(&now(), -60.0))
        .unwrap();
    assert_no_bump("clearAllLastWatched", || {
        s.clear_all_last_watched().unwrap()
    });
    assert!(s.fetch_recent_items(20).unwrap().is_empty());
}

#[test]
fn watch_lifecycle_bumps_and_missing_record_does_not() {
    let s = store();
    let mut started = now();
    assert_bumps("start", || {
        started = s
            .start_watch_record("c1", "채널", None, None, add(&now(), -90.0))
            .unwrap()
    });
    assert_bumps("checkpoint", || {
        s.checkpoint_watch_record("c1", started, now()).unwrap();
    });
    assert_no_bump("못 찾은 checkpoint", || {
        s.checkpoint_watch_record("missing", started, now())
            .unwrap();
    });
    assert_bumps("end", || {
        s.end_watch_record("c1", started, now()).unwrap();
    });
    assert_no_bump("닫힌 기록의 end", || {
        s.end_watch_record("c1", started, now()).unwrap();
    });
}

// MARK: - 레거시 정리

fn clip_row(uid: &str, sources: &[&str]) -> CachedClipData {
    let mut c = CachedClipData::new(uid, uid);
    c.duration = 10;
    c.content_hash = format!("h-{uid}");
    c.sources = sources.iter().map(|s| s.to_string()).collect();
    c
}

fn stat(name: &str, live_count: i64) -> CachedCategoryStat {
    CachedCategoryStat {
        id: 0,
        category_name: name.into(),
        live_count,
        total_viewers: 0,
        top_channel_id: None,
        top_channel_name: None,
        fetched_at: now(),
    }
}

#[test]
fn purge_removes_category_stats_and_channel_tags_keeps_trending() {
    let s = store();
    s.insert_cached_category_stat(&stat("게임", 3)).unwrap();
    s.insert_cached_category_stat(&stat("스포츠", 1)).unwrap();
    s.upsert_cached_clips(&[
        clip_row("channel-only", &["channel-abc-popular"]),
        clip_row("mixed", &["trending-week-popular", "channel-abc-recent"]),
        clip_row("trending-only", &["trending-week-popular"]),
    ])
    .unwrap();

    let result = s.purge_legacy_write_only_rows_if_needed().unwrap();
    assert_eq!(
        result,
        Some(LegacyCleanupResult {
            category_stat_rows: 2,
            channel_only_clip_rows: 1,
            retagged_clip_rows: 1,
        })
    );
    assert_eq!(s.table_row_count("CachedCategoryStat").unwrap(), 0);
    let trending: HashSet<_> = s
        .fetch_cached_clips("trending-week-popular", 100)
        .unwrap()
        .into_iter()
        .map(|c| c.clip_uid)
        .collect();
    assert_eq!(
        trending,
        HashSet::from(["mixed".to_string(), "trending-only".to_string()])
    );
    assert!(s
        .fetch_cached_clips("channel-abc-popular", 100)
        .unwrap()
        .is_empty());
    assert!(s
        .fetch_cached_clips("channel-abc-recent", 100)
        .unwrap()
        .is_empty());
    assert_eq!(s.purge_legacy_write_only_rows_if_needed().unwrap(), None);
}

#[test]
fn purge_with_nothing_writes_only_marker() {
    let s = store();
    assert_eq!(
        s.purge_legacy_write_only_rows_if_needed().unwrap(),
        Some(LegacyCleanupResult::default())
    );
    assert_eq!(
        s.load_setting::<bool>(LEGACY_WRITE_ONLY_CLEANUP_KEY)
            .unwrap(),
        Some(true)
    );
}

#[test]
fn purge_legacy_home_cache_removes_only_cache_keys_and_is_idempotent() {
    let s = store();
    let cache_keys = [
        "cache.liveChannels",
        "cache.liveChannels.ts",
        "cache.allStatChannels",
        "cache.allStatChannels.complete.v2",
        "cache.followingChannels",
        "cache.following.snapshot.v2",
    ];
    let kept_keys = [
        "player",
        "keyboard",
        "multiChat",
        LEGACY_WRITE_ONLY_CLEANUP_KEY,
        "maintenance.menuSnapshotMigration.v1",
        "cachex",
        "my.cache.value",
        "Cache.upper",
    ];
    let pairs: HashMap<String, Vec<u8>> = cache_keys
        .iter()
        .chain(kept_keys.iter())
        .map(|k| (k.to_string(), b"[]".to_vec()))
        .collect();
    s.save_settings(&pairs).unwrap();
    let live: Vec<_> = (0..3)
        .map(|i| {
            let mut d = CachedLiveChannelData::new(format!("c{i}"), format!("채널{i}"));
            d.sources = ["live-list".to_string()].into_iter().collect();
            d
        })
        .collect();
    s.upsert_cached_live_channels(&live).unwrap();

    assert_eq!(
        s.purge_legacy_home_cache().unwrap(),
        (cache_keys.len() as i64, 3)
    );
    let all_keys: HashSet<String> = pairs.keys().cloned().collect();
    let remaining: HashSet<String> = s
        .load_settings_raw(&all_keys)
        .unwrap()
        .into_keys()
        .collect();
    assert_eq!(
        remaining,
        kept_keys
            .iter()
            .map(|k| k.to_string())
            .collect::<HashSet<_>>()
    );
    assert_eq!(s.table_row_count("CachedLiveChannel").unwrap(), 0);

    let before = s.history_transaction_count();
    assert_eq!(s.purge_legacy_home_cache().unwrap(), (0, 0));
    assert_eq!(s.history_transaction_count(), before);
}

#[test]
fn purge_legacy_home_cache_on_empty_store_writes_nothing() {
    let s = store();
    let before = s.history_transaction_count();
    assert_eq!(s.purge_legacy_home_cache().unwrap(), (0, 0));
    assert_eq!(s.history_transaction_count(), before);
}
