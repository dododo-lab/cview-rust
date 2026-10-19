//! 원본: `Tests/CViewPersistenceTests/DataStoreFavoritePersistenceTests.swift`,
//! `DataStoreLiveChannelCacheTests.swift`, `DataStoreLiveChannelPruneTests.swift`.

mod common;

use common::*;
use cview_core::models::ChannelInfo;
use cview_persistence::models::CachedLiveChannelData;
use cview_persistence::time::Date;
use cview_persistence::DataStore;
use std::collections::{BTreeSet, HashSet};

fn info(id: &str, name: &str, image: Option<&str>) -> ChannelInfo {
    let mut i = ChannelInfo::new(id, name);
    i.channel_image_url = image.and_then(|s| url::Url::parse(s).ok());
    i
}

fn def(id: &str) -> ChannelInfo {
    info(id, "채널", Some("https://img.example.com/a.png"))
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

// MARK: - 즐겨찾기 (FAV-1 · CHAN-2)

#[test]
fn clear_all_last_watched_keeps_favorites() {
    let s = store();
    s.start_watch_record("ch1", "채널", None, None, now())
        .unwrap();
    s.set_favorite("ch2", true, Some(&def("ch2"))).unwrap();
    s.start_watch_record("ch2", "채널2", None, None, now())
        .unwrap();
    assert_eq!(s.fetch_recent_items(10).unwrap().len(), 2);
    s.clear_all_last_watched().unwrap();
    assert!(s.fetch_recent_items(10).unwrap().is_empty());
    assert!(s.is_favorite("ch2").unwrap());
}

#[test]
fn delete_all_playback_history() {
    let s = store();
    s.record_vod_playback(7, 45.0, 100.0, "v", "c", None)
        .unwrap();
    s.record_clip_playback("clip-1", 5.0, 30.0, "clip", "c", None)
        .unwrap();
    assert_eq!(s.fetch_recent_vod_playback(5).unwrap().len(), 1);
    assert_eq!(s.fetch_recent_clip_playback(5).unwrap().len(), 1);
    s.delete_all_vod_playback().unwrap();
    s.delete_all_clip_playback().unwrap();
    assert!(s.fetch_recent_vod_playback(5).unwrap().is_empty());
    assert!(s.fetch_recent_clip_playback(5).unwrap().is_empty());
}

#[test]
fn favorite_persists_for_already_watched_channel() {
    let s = store();
    s.start_watch_record("ch1", "채널", None, None, now())
        .unwrap();
    assert!(!s.is_favorite("ch1").unwrap());
    assert!(s.set_favorite("ch1", true, Some(&def("ch1"))).unwrap());
    assert!(s.is_favorite("ch1").unwrap());
    assert_eq!(s.fetch_favorite_items().unwrap().len(), 1);
}

#[test]
fn unfavorite_persists() {
    let s = store();
    s.set_favorite("ch1", true, Some(&def("ch1"))).unwrap();
    s.set_favorite("ch1", false, Some(&def("ch1"))).unwrap();
    assert!(!s.is_favorite("ch1").unwrap());
    assert!(s.fetch_favorite_items().unwrap().is_empty());
}

#[test]
fn set_favorite_inserts_when_row_missing() {
    let s = store();
    assert!(s.set_favorite("new", true, Some(&def("new"))).unwrap());
    assert!(s.is_favorite("new").unwrap());
}

#[test]
fn set_favorite_fails_without_row_or_info() {
    let s = store();
    assert!(!s.set_favorite("ghost", true, None).unwrap());
    assert!(s.fetch_favorite_items().unwrap().is_empty());
    assert_eq!(s.table_row_count("PersistedChannel").unwrap(), 0);
}

#[test]
fn save_channel_never_clobbers_existing_favorite() {
    let s = store();
    s.set_favorite("ch1", true, Some(&def("ch1"))).unwrap();
    s.save_channel(&info("ch1", "새 이름", None), false)
        .unwrap();
    assert!(s.is_favorite("ch1").unwrap());
}

#[test]
fn save_channel_is_favorite_ignored_for_existing_row() {
    let s = store();
    s.save_channel(&def("ch1"), false).unwrap();
    s.save_channel(&def("ch1"), true).unwrap();
    assert!(!s.is_favorite("ch1").unwrap());
}

#[test]
fn save_channel_does_not_erase_image_url() {
    let s = store();
    s.set_favorite(
        "ch1",
        true,
        Some(&info(
            "ch1",
            "채널",
            Some("https://img.example.com/keep.png"),
        )),
    )
    .unwrap();
    s.save_channel(&info("ch1", "채널", None), false).unwrap();
    let row = s.fetch_favorite_items().unwrap().remove(0);
    assert_eq!(
        row.image_url.as_deref(),
        Some("https://img.example.com/keep.png")
    );
}

// MARK: - 라이브 채널 캐시

fn live(id: &str, name: &str, viewers: i64, sources: &[&str], at: Date) -> CachedLiveChannelData {
    let mut d = CachedLiveChannelData::new(id, name);
    d.viewer_count = viewers;
    d.sources = set(sources);
    d.fetched_at = at;
    d
}

fn seed(s: &DataStore) {
    let base = ts(1_700_000_000.0);
    let mut items = Vec::new();
    for i in 0..60 {
        let mut sources = vec!["all-stats"];
        if i % 3 == 0 {
            sources.push("following");
        }
        if i % 10 == 0 {
            sources.push("live-list");
        }
        items.push(live(
            &format!("ch{i}"),
            &format!("채널{i}"),
            i,
            &sources,
            add(&base, i as f64),
        ));
    }
    items.push(live("other", "무관", 0, &["clips"], base));
    s.upsert_cached_live_channels(&items).unwrap();
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn batch_fetch_matches_per_source_fetch() {
    let s = store();
    seed(&s);
    let wanted = strings(&["live-list", "all-stats", "following"]);
    let batch = s.fetch_cached_live_channels_by_sources(&wanted).unwrap();
    for source in &wanted {
        let single = s.fetch_cached_live_channels(source).unwrap();
        assert!(!single.is_empty());
        let a: Vec<_> = batch[source].iter().map(|c| c.channel_id.clone()).collect();
        let b: Vec<_> = single.iter().map(|c| c.channel_id.clone()).collect();
        assert_eq!(a, b, "{source}");
    }
    assert_eq!(batch["all-stats"].len(), 60);
    assert_eq!(batch["following"].len(), 20);
    assert_eq!(batch["live-list"].len(), 6);
    assert!(!batch.values().flatten().any(|c| c.channel_id == "other"));
}

#[test]
fn batch_fetch_keeps_fetched_at_descending() {
    let s = store();
    seed(&s);
    let batch = s
        .fetch_cached_live_channels_by_sources(&strings(&["all-stats"]))
        .unwrap();
    let stamps: Vec<_> = batch["all-stats"].iter().map(|c| c.fetched_at).collect();
    assert_eq!(stamps.len(), 60);
    let mut sorted = stamps.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(stamps, sorted);
}

#[test]
fn batch_fetch_seeds_requested_sources_even_when_empty() {
    let s = store();
    let batch = s
        .fetch_cached_live_channels_by_sources(&strings(&["live-list", "없는소스"]))
        .unwrap();
    assert_eq!(batch.len(), 2);
    assert!(batch["live-list"].is_empty());
    assert!(batch["없는소스"].is_empty());
}

#[test]
fn batch_fetch_with_no_sources_is_empty() {
    let s = store();
    seed(&s);
    assert!(s
        .fetch_cached_live_channels_by_sources(&[])
        .unwrap()
        .is_empty());
}

#[test]
fn small_batch_upsert_updates_existing_rows() {
    let s = store();
    seed(&s);
    let at = ts(1_800_000_000.0);
    s.upsert_cached_live_channels(&[
        live("ch0", "갱신됨", 999, &["live-list"], at),
        live("brand-new", "신규", 1, &["live-list"], at),
    ])
    .unwrap();
    let list = s.fetch_cached_live_channels("live-list").unwrap();
    assert!(list
        .iter()
        .any(|c| c.channel_id == "ch0" && c.channel_name == "갱신됨" && c.viewer_count == 999));
    assert!(list.iter().any(|c| c.channel_id == "brand-new"));
}

#[test]
fn upsert_boundary_200_keeps_rows() {
    let s = store();
    let base = ts(1_700_000_000.0);
    let items: Vec<_> = (0..200)
        .map(|i| {
            live(
                &format!("bound{i}"),
                &format!("B{i}"),
                i,
                &["live-list"],
                add(&base, i as f64),
            )
        })
        .collect();
    s.upsert_cached_live_channels(&items).unwrap();
    let again: Vec<_> = items
        .iter()
        .map(|c| {
            live(
                &c.channel_id,
                &format!("{}x", c.channel_name),
                c.viewer_count,
                &["live-list"],
                add(&base, 1000.0),
            )
        })
        .collect();
    s.upsert_cached_live_channels(&again).unwrap();
    let list = s.fetch_cached_live_channels("live-list").unwrap();
    assert_eq!(
        list.iter()
            .filter(|c| c.channel_id.starts_with("bound"))
            .count(),
        200
    );
    assert!(list
        .iter()
        .any(|c| c.channel_id == "bound0" && c.channel_name == "B0x"));
}

#[test]
fn identical_upsert_does_not_rewrite_fetched_at() {
    let s = store();
    let first = ts(1_700_000_000.0);
    let later = add(&first, 600.0);
    assert_eq!(
        s.upsert_cached_live_channels(&[live("stable", "동일 채널", 100, &["all-stats"], first)])
            .unwrap(),
        1
    );
    let history = s.history_transaction_count();
    assert_eq!(
        s.upsert_cached_live_channels(&[live("stable", "동일 채널", 100, &["all-stats"], later)])
            .unwrap(),
        0
    );
    assert_eq!(
        s.history_transaction_count(),
        history,
        "no-op 인데 커밋했다"
    );
    let rows = s.fetch_cached_live_channels("all-stats").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].fetched_at, first);
}

#[test]
fn replace_snapshot_writes_only_membership_delta() {
    let s = store();
    let first = ts(1_700_000_000.0);
    let next = add(&first, 600.0);
    assert_eq!(
        s.replace_cached_live_channels(
            &[
                live("stable", "유지", 10, &["all-stats"], first),
                live("stale", "종료", 1, &["all-stats"], first),
            ],
            "all-stats"
        )
        .unwrap(),
        2
    );
    assert_eq!(
        s.replace_cached_live_channels(
            &[
                live("stable", "유지", 10, &["all-stats"], next),
                live("added", "신규", 5, &["all-stats"], next),
            ],
            "all-stats"
        )
        .unwrap(),
        2
    );
    let rows = s.fetch_cached_live_channels("all-stats").unwrap();
    let ids: HashSet<_> = rows.iter().map(|c| c.channel_id.as_str()).collect();
    assert_eq!(ids, HashSet::from(["stable", "added"]));
    let get = |id: &str| rows.iter().find(|c| c.channel_id == id).unwrap().fetched_at;
    assert_eq!(get("stable"), first);
    assert_eq!(get("added"), next);
}

#[test]
fn replace_snapshot_updates_changed_content_and_timestamp() {
    let s = store();
    let first = ts(1_700_000_000.0);
    let next = add(&first, 600.0);
    s.replace_cached_live_channels(
        &[live("changing", "채널", 10, &["live-list"], first)],
        "live-list",
    )
    .unwrap();
    assert_eq!(
        s.replace_cached_live_channels(
            &[live("changing", "채널", 11, &["live-list"], next)],
            "live-list"
        )
        .unwrap(),
        1
    );
    let row = s.fetch_cached_live_channels("live-list").unwrap().remove(0);
    assert_eq!(row.viewer_count, 11);
    assert_eq!(row.fetched_at, next);
}

#[test]
fn replace_snapshot_preserves_other_source_membership() {
    let s = store();
    s.upsert_cached_live_channels(&[live(
        "shared",
        "공유",
        0,
        &["all-stats", "following"],
        now(),
    )])
    .unwrap();
    assert_eq!(s.replace_cached_live_channels(&[], "all-stats").unwrap(), 1);
    assert!(s
        .fetch_cached_live_channels("all-stats")
        .unwrap()
        .is_empty());
    let following = s.fetch_cached_live_channels("following").unwrap();
    assert_eq!(following.len(), 1);
    assert_eq!(following[0].channel_id, "shared");
}

// MARK: - prune

fn seed_one(s: &DataStore, id: &str, sources: &[&str], at: Date) {
    s.upsert_cached_live_channels(&[live(id, id, 0, sources, at)])
        .unwrap();
}

#[test]
fn prune_removes_stale_tag_but_keeps_row() {
    let s = store();
    let past = ts(1_000.0);
    seed_one(&s, "ch1", &["live-list", "following"], past);
    assert_eq!(
        s.prune_cached_live_channels("live-list", add(&past, 1.0))
            .unwrap(),
        1
    );
    let rows = s.fetch_all_cached_live_channels().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].sources, set(&["following"]));
}

#[test]
fn prune_deletes_row_when_last_source_removed() {
    let s = store();
    let past = ts(1_000.0);
    seed_one(&s, "ch1", &["live-list"], past);
    assert_eq!(
        s.prune_cached_live_channels("live-list", add(&past, 1.0))
            .unwrap(),
        1
    );
    assert_eq!(s.cached_live_channel_count().unwrap(), 0);
}

#[test]
fn prune_no_change_when_nothing_stale() {
    let s = store();
    let n = ts(2_000.0);
    seed_one(&s, "ch1", &["live-list"], n);
    let before = s.history_transaction_count();
    assert_eq!(s.prune_cached_live_channels("live-list", n).unwrap(), 0);
    assert_eq!(s.history_transaction_count(), before);
    let rows = s.fetch_all_cached_live_channels().unwrap();
    assert_eq!(rows[0].sources, set(&["live-list"]));
}

#[test]
fn prune_leaves_other_source_rows() {
    let s = store();
    let past = ts(1_000.0);
    seed_one(&s, "ch1", &["all-stats"], past);
    assert_eq!(
        s.prune_cached_live_channels("live-list", add(&past, 1.0))
            .unwrap(),
        0
    );
    assert_eq!(
        s.fetch_all_cached_live_channels().unwrap()[0].sources,
        set(&["all-stats"])
    );
}

#[test]
fn prune_only_stale_rows() {
    let s = store();
    let past = ts(1_000.0);
    let fresh = ts(5_000.0);
    seed_one(&s, "old", &["live-list"], past);
    seed_one(&s, "new", &["live-list"], fresh);
    assert_eq!(s.prune_cached_live_channels("live-list", fresh).unwrap(), 1);
    let rows = s.fetch_all_cached_live_channels().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].channel_id, "new");
}
