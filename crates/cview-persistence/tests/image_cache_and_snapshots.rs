//! 원본: `Tests/CViewPersistenceTests/DataStoreImageCacheTests.swift`,
//! `DataStoreCategorySnapshotBaselineTests.swift`.

mod common;

use common::*;
use cview_persistence::models::{CachedImageEntryData, CategorySnapshotInput, ImageTouch};
use cview_persistence::store::category_snapshots::{
    DEFAULT_BASELINE_TOLERANCE, DEFAULT_BASELINE_WINDOW, DEFAULT_MIN_SPACING,
};
use cview_persistence::store::image_cache::{
    IMAGE_INDEX_BACKFILL_DONE_KEY, IMAGE_INDEX_CHUNK_SIZE,
};
use cview_persistence::DataStore;
use std::collections::{HashMap, HashSet};

fn entry(hash: &str, access_count: i64, size_bytes: i64) -> CachedImageEntryData {
    let mut e = CachedImageEntryData::new(hash, format!("{hash}.png"));
    e.url_string = Some(format!("https://img.example.com/{hash}.png"));
    e.size_bytes = size_bytes;
    e.fetched_at = ts(1_000_000.0);
    e.accessed_at = ts(1_000_000.0);
    e.access_count = access_count;
    e
}

fn by_hash(s: &DataStore) -> HashMap<String, CachedImageEntryData> {
    s.all_image_entries()
        .unwrap()
        .into_iter()
        .map(|e| (e.url_hash.clone(), e))
        .collect()
}

#[test]
fn upsert_batch_does_not_regress_access_stats() {
    let s = store();
    s.upsert_image_entries(&[
        entry("u0", 2, 100),
        entry("u1", 7, 100),
        entry("u2", 1, 100),
    ])
    .unwrap();
    s.upsert_image_entries(&[entry("u1", 1, 999), entry("u3", 3, 300)])
        .unwrap();
    let rows = by_hash(&s);
    assert_eq!(rows.len(), 4);
    assert_eq!(rows["u1"].size_bytes, 999);
    assert_eq!(rows["u1"].access_count, 7);
    assert_eq!(rows["u3"].size_bytes, 300);
    assert_eq!(rows["u3"].access_count, 3);
}

#[test]
fn touch_updates_only_batched_keys() {
    let s = store();
    for i in 0..5 {
        s.upsert_image_entry(&entry(&format!("h{i}"), 1, 100))
            .unwrap();
    }
    let at = ts(2_000_000.0);
    s.touch_image_entries(&HashMap::from([
        (
            "h1".to_string(),
            ImageTouch {
                accessed_at: at,
                count: 3,
            },
        ),
        (
            "h3".to_string(),
            ImageTouch {
                accessed_at: at,
                count: 2,
            },
        ),
    ]))
    .unwrap();
    let rows = by_hash(&s);
    assert_eq!(rows["h1"].access_count, 4);
    assert_eq!(rows["h1"].accessed_at, at);
    assert_eq!(rows["h3"].access_count, 3);
    assert_eq!(rows["h3"].accessed_at, at);
    for h in ["h0", "h2", "h4"] {
        assert_eq!(rows[h].access_count, 1);
        assert_eq!(rows[h].accessed_at, ts(1_000_000.0));
    }
}

#[test]
fn expired_and_live_queries_are_disjoint() {
    let s = store();
    let n = ts(2_000_000.0);
    let stale = add(&n, -100.0);
    let mut aged = CachedImageEntryData::new("aged", "aged");
    aged.fetched_at = add(&stale, -10.0);
    aged.accessed_at = n;
    let mut ttl = CachedImageEntryData::new("ttl", "ttl");
    ttl.fetched_at = n;
    ttl.accessed_at = n;
    ttl.expires_at = Some(add(&n, -5.0));
    let mut fresh = CachedImageEntryData::new("fresh", "fresh");
    fresh.fetched_at = n;
    fresh.accessed_at = n;
    s.upsert_image_entries(&[aged, ttl, fresh]).unwrap();

    let expired: HashSet<_> = s
        .fetch_expired_image_entries(n, stale)
        .unwrap()
        .into_iter()
        .map(|e| e.url_hash)
        .collect();
    assert_eq!(
        expired,
        HashSet::from(["aged".to_string(), "ttl".to_string()])
    );
    let live: Vec<_> = s
        .fetch_live_image_entries(n, stale)
        .unwrap()
        .into_iter()
        .map(|e| e.url_hash)
        .collect();
    assert_eq!(live, vec!["fresh".to_string()]);
}

#[test]
fn touch_ignores_unknown_keys() {
    let s = store();
    s.upsert_image_entry(&entry("known", 5, 100)).unwrap();
    let at = ts(3_000_000.0);
    s.touch_image_entries(&HashMap::from([
        (
            "known".to_string(),
            ImageTouch {
                accessed_at: at,
                count: 1,
            },
        ),
        (
            "missing".to_string(),
            ImageTouch {
                accessed_at: at,
                count: 9,
            },
        ),
    ]))
    .unwrap();
    let rows = s.all_image_entries().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].access_count, 6);
}

#[test]
fn touch_handles_more_than_one_chunk() {
    let s = store();
    let count = IMAGE_INDEX_CHUNK_SIZE + 25;
    let entries: Vec<_> = (0..count)
        .map(|i| entry(&format!("bulk{i}"), 0, 100))
        .collect();
    s.upsert_image_entries(&entries).unwrap();
    let batch: HashMap<_, _> = (0..count)
        .map(|i| {
            (
                format!("bulk{i}"),
                ImageTouch {
                    accessed_at: ts(4_000_000.0),
                    count: 1,
                },
            )
        })
        .collect();
    s.touch_image_entries(&batch).unwrap();
    let rows = s.all_image_entries().unwrap();
    assert_eq!(rows.len(), count);
    assert!(rows.iter().all(|r| r.access_count == 1));
}

#[test]
fn delete_entries_removes_only_requested() {
    let s = store();
    for i in 0..6 {
        s.upsert_image_entry(&entry(&format!("d{i}"), 0, 100))
            .unwrap();
    }
    s.delete_image_entries(&["d0".into(), "d2".into(), "d5".into()])
        .unwrap();
    let mut remaining: Vec<_> = s
        .all_image_entries()
        .unwrap()
        .into_iter()
        .map(|e| e.url_hash)
        .collect();
    remaining.sort();
    assert_eq!(remaining, vec!["d1", "d3", "d4"]);
}

#[test]
fn delete_entries_handles_empty_and_unknown() {
    let s = store();
    s.upsert_image_entry(&entry("keep", 0, 100)).unwrap();
    s.delete_image_entries(&[]).unwrap();
    s.delete_image_entries(&["nonexistent".into()]).unwrap();
    let remaining: Vec<_> = s
        .all_image_entries()
        .unwrap()
        .into_iter()
        .map(|e| e.url_hash)
        .collect();
    assert_eq!(remaining, vec!["keep"]);
}

#[test]
fn backfill_marker_round_trips() {
    let s = store();
    assert_eq!(IMAGE_INDEX_BACKFILL_DONE_KEY, "imageCache.indexBackfill.v2");
    assert!(!s.is_image_index_backfill_done());
    s.mark_image_index_backfill_done().unwrap();
    s.mark_image_index_backfill_done().unwrap();
    assert!(s.is_image_index_backfill_done());
    assert_eq!(
        s.load_setting::<bool>(IMAGE_INDEX_BACKFILL_DONE_KEY)
            .unwrap(),
        Some(true)
    );
    assert_eq!(s.table_row_count("PersistedSetting").unwrap(), 1);
    s.clear_image_index_backfill_done().unwrap();
    assert!(!s.is_image_index_backfill_done());
}

#[test]
fn backfill_marker_does_not_survive_store_recreation() {
    let original = store();
    original.upsert_image_entry(&entry("kept", 0, 100)).unwrap();
    original.mark_image_index_backfill_done().unwrap();
    let recreated = store();
    assert!(!recreated.is_image_index_backfill_done());
}

#[test]
fn delete_all_entries_leaves_marker() {
    let s = store();
    s.upsert_image_entry(&entry("a0", 0, 100)).unwrap();
    s.mark_image_index_backfill_done().unwrap();
    s.delete_all_image_entries().unwrap();
    assert!(s.is_image_index_backfill_done());
    assert!(s.all_image_entries().unwrap().is_empty());
}

#[test]
fn delete_all_entries_clears_index() {
    let s = store();
    for i in 0..4 {
        s.upsert_image_entry(&entry(&format!("a{i}"), 0, 100))
            .unwrap();
    }
    s.delete_all_image_entries().unwrap();
    assert!(s.all_image_entries().unwrap().is_empty());
}

// MARK: - 카테고리 스냅샷 기준선

fn base() -> cview_persistence::time::Date {
    ts(1_700_000_000.0)
}

fn inputs(pairs: &[(&str, i64)]) -> Vec<CategorySnapshotInput> {
    pairs
        .iter()
        .map(|(c, v)| CategorySnapshotInput::new(*c, *v, 1))
        .collect()
}

fn baselines(s: &DataStore, reference: cview_persistence::time::Date) -> HashMap<String, i64> {
    s.category_baselines(
        reference,
        DEFAULT_BASELINE_WINDOW,
        DEFAULT_BASELINE_TOLERANCE,
    )
    .unwrap()
}

fn record_and_read(
    s: &DataStore,
    items: &[CategorySnapshotInput],
    at: cview_persistence::time::Date,
) -> HashMap<String, i64> {
    s.record_category_snapshots_and_read_baselines(
        items,
        at,
        DEFAULT_MIN_SPACING,
        DEFAULT_BASELINE_WINDOW,
        DEFAULT_BASELINE_TOLERANCE,
    )
    .unwrap()
}

fn viewer_totals(s: &DataStore) -> Vec<i64> {
    s.all_category_snapshots()
        .unwrap()
        .iter()
        .map(|r| r.viewer_total)
        .collect()
}

#[test]
fn record_stamps_capture_time_and_reads_relative_to_reference() {
    let s = store();
    s.record_category_snapshots(&inputs(&[("게임", 1_000), ("스포츠", 200)]), base())
        .unwrap();
    let stamps: HashSet<_> = s
        .all_category_snapshots()
        .unwrap()
        .iter()
        .map(|r| r.captured_at)
        .collect();
    assert_eq!(stamps, HashSet::from([base()]));
    assert_eq!(
        baselines(&s, add(&base(), 3_600.0)),
        HashMap::from([("게임".to_string(), 1_000), ("스포츠".to_string(), 200)])
    );
    assert!(baselines(&s, add(&base(), 3_600.0 + 901.0)).is_empty());
    assert!(baselines(&s, now()).is_empty());
}

#[test]
fn baseline_picks_row_closest_to_one_hour_before() {
    let s = store();
    s.record_category_snapshots(&inputs(&[("게임", 900)]), add(&base(), -600.0))
        .unwrap();
    s.record_category_snapshots(&inputs(&[("게임", 1_000)]), add(&base(), 120.0))
        .unwrap();
    s.record_category_snapshots(&inputs(&[("게임", 1_100)]), add(&base(), 800.0))
        .unwrap();
    assert_eq!(baselines(&s, add(&base(), 3_600.0))["게임"], 1_000);
}

#[test]
fn record_and_read_respects_min_spacing() {
    let s = store();
    assert!(record_and_read(&s, &inputs(&[("게임", 1_000)]), base()).is_empty());
    assert_eq!(viewer_totals(&s).len(), 1);
    record_and_read(&s, &inputs(&[("게임", 5_000)]), add(&base(), 299.0));
    assert_eq!(viewer_totals(&s).len(), 1, "minSpacing 안쪽인데 기록했다");
    record_and_read(&s, &inputs(&[("게임", 1_200)]), add(&base(), 301.0));
    assert_eq!(viewer_totals(&s).len(), 2);
    let later = record_and_read(&s, &inputs(&[("게임", 2_000)]), add(&base(), 3_600.0));
    assert_eq!(later, HashMap::from([("게임".to_string(), 1_000)]));
    assert_eq!(viewer_totals(&s), vec![1_000, 1_200, 2_000]);
}

#[test]
fn record_and_read_treats_later_rows_as_inside_spacing() {
    let s = store();
    s.record_category_snapshots(&inputs(&[("게임", 1_000)]), add(&base(), 1_000.0))
        .unwrap();
    record_and_read(&s, &inputs(&[("게임", 1)]), base());
    assert_eq!(viewer_totals(&s).len(), 1);
}

#[test]
fn record_and_read_with_empty_inputs_only_reads() {
    let s = store();
    s.record_category_snapshots(&inputs(&[("게임", 700)]), base())
        .unwrap();
    assert_eq!(
        record_and_read(&s, &[], add(&base(), 3_600.0)),
        HashMap::from([("게임".to_string(), 700)])
    );
    assert_eq!(viewer_totals(&s).len(), 1);
}

#[test]
fn prune_deletes_expired_and_skips_save_when_nothing() {
    let s = store();
    s.record_category_snapshots(&inputs(&[("게임", 1), ("스포츠", 2)]), base())
        .unwrap();
    s.record_category_snapshots(&inputs(&[("게임", 3)]), add(&base(), 7.0 * 3_600.0))
        .unwrap();
    let n = add(&base(), 8.0 * 3_600.0);
    assert_eq!(s.prune_category_snapshots(6.0 * 3_600.0, n).unwrap(), 2);
    assert_eq!(viewer_totals(&s), vec![3]);
    let before = s.history_transaction_count();
    assert_eq!(s.prune_category_snapshots(6.0 * 3_600.0, n).unwrap(), 0);
    assert_eq!(s.history_transaction_count(), before);
}
