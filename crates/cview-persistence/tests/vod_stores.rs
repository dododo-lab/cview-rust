//! 원본: `Tests/CViewPersistenceTests/VODPlaybackStoreTests.swift`, `VODWatchLaterStoreTests.swift`,
//! `VODSubscriptionTagPurgeTests.swift`, `SubscriptionVODPagesTests.swift`.
//!
//! Swift 기본값: 구독 per-channel limit 24, 나중에 볼 VOD limit 100, 이어 보기 limit 20.

mod common;

use common::*;
use cview_persistence::models::{CachedVod, CachedVodData};
use cview_persistence::store::vod_cache::SubscriptionVodPage;
use cview_persistence::time::Date;
use cview_persistence::DataStore;
use std::collections::{BTreeMap, HashSet};

const PER_CHANNEL: usize = 24;

// MARK: - VOD 이어 보기

#[test]
fn playback_record_and_fetch_one() {
    let s = store();
    s.record_vod_playback(
        101,
        300.0,
        1200.0,
        "테스트 VOD",
        "채널A",
        Some("https://img.example.com/1.jpg"),
    )
    .unwrap();
    let row = s.fetch_vod_playback(101).unwrap().unwrap();
    assert_eq!(row.video_no, 101);
    assert_eq!(row.last_position_sec, 300.0);
    assert_eq!(row.duration_sec, 1200.0);
    assert_eq!(row.video_title, "테스트 VOD");
    assert!(approx(row.progress(), 0.25, 0.001));
}

#[test]
fn playback_record_updates_existing() {
    let s = store();
    s.record_vod_playback(7, 10.0, 100.0, "A", "ch", None)
        .unwrap();
    s.record_vod_playback(7, 55.0, 100.0, "A2", "ch", None)
        .unwrap();
    let row = s.fetch_vod_playback(7).unwrap().unwrap();
    assert_eq!(row.last_position_sec, 55.0);
    assert_eq!(row.video_title, "A2");
    assert_eq!(
        s.fetch_recent_vod_playback(50)
            .unwrap()
            .iter()
            .filter(|r| r.video_no == 7)
            .count(),
        1
    );
}

#[test]
fn playback_recent_filters_floor_and_completion() {
    let s = store();
    s.record_vod_playback(1, 25.0, 1000.0, "tiny", "c", None)
        .unwrap();
    s.record_vod_playback(2, 600.0, 1000.0, "mid", "c", None)
        .unwrap();
    s.record_vod_playback(3, 990.0, 1000.0, "done", "c", None)
        .unwrap();
    let nos: Vec<_> = s
        .fetch_recent_vod_playback(20)
        .unwrap()
        .iter()
        .map(|r| r.video_no)
        .collect();
    assert_eq!(nos, vec![2]);
}

#[test]
fn playback_recent_sorts_by_last_seen_desc() {
    let s = store();
    s.record_vod_playback(1, 31.0, 100.0, "first", "c", None)
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
    s.record_vod_playback(2, 31.0, 100.0, "second", "c", None)
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
    s.record_vod_playback(1, 40.0, 100.0, "first-again", "c", None)
        .unwrap();
    assert_eq!(s.fetch_recent_vod_playback(20).unwrap()[0].video_no, 1);
}

#[test]
fn playback_remove_deletes_row() {
    let s = store();
    s.record_vod_playback(9, 50.0, 100.0, "x", "c", None)
        .unwrap();
    s.remove_vod_playback(9).unwrap();
    assert!(s.fetch_vod_playback(9).unwrap().is_none());
}

// MARK: - 나중에 볼 VOD

#[test]
fn watch_later_add_and_fetch() {
    let s = store();
    s.add_vod_watch_later(11, "담은 VOD", "채널A", Some("https://img/1.jpg"), 600.0)
        .unwrap();
    let all = s.fetch_vod_watch_later(100).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].video_no, 11);
    assert_eq!(all[0].video_title, "담은 VOD");
    assert_eq!(all[0].duration_sec, 600.0);
}

#[test]
fn watch_later_duplicate_keeps_single_row() {
    let s = store();
    s.add_vod_watch_later(5, "a", "c", None, 0.0).unwrap();
    s.add_vod_watch_later(5, "a-updated", "c", None, 0.0)
        .unwrap();
    let all = s.fetch_vod_watch_later(100).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].video_title, "a-updated");
}

#[test]
fn watch_later_contains_and_remove() {
    let s = store();
    s.add_vod_watch_later(8, "x", "c", None, 0.0).unwrap();
    assert!(s.contains_vod_watch_later(8).unwrap());
    assert!(!s.contains_vod_watch_later(99).unwrap());
    s.remove_vod_watch_later(8).unwrap();
    assert!(!s.contains_vod_watch_later(8).unwrap());
    assert!(s.fetch_vod_watch_later(100).unwrap().is_empty());
}

#[test]
fn watch_later_sorted_by_added_at_desc() {
    let s = store();
    s.add_vod_watch_later(1, "first", "c", None, 0.0).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
    s.add_vod_watch_later(2, "second", "c", None, 0.0).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
    s.add_vod_watch_later(1, "first-again", "c", None, 0.0)
        .unwrap();
    assert_eq!(s.fetch_vod_watch_later(100).unwrap()[0].video_no, 1);
}

#[test]
fn watch_later_fetch_ids() {
    let s = store();
    s.add_vod_watch_later(10, "a", "c", None, 0.0).unwrap();
    s.add_vod_watch_later(20, "b", "c", None, 0.0).unwrap();
    let ids: HashSet<_> = s.fetch_vod_watch_later_ids().unwrap().into_iter().collect();
    assert_eq!(ids, HashSet::from([10, 20]));
}

// MARK: - 구독 태그 정리

fn tag(scope: &str) -> String {
    CachedVod::subscription_tag(scope).unwrap()
}

#[test]
fn token_helpers() {
    assert_eq!(tag("A"), "subscriptions.A");
    assert!(CachedVod::subscription_tag("").is_none());
    assert!(CachedVod::subscription_tag("A,B").is_none());
    let uuid = uuid::Uuid::new_v4().to_string().to_uppercase();
    assert_eq!(tag(&uuid), format!("subscriptions.{uuid}"));
    assert!(CachedVod::is_subscription_token("subscriptions"));
    assert!(CachedVod::is_subscription_token("subscriptions.A"));
    assert!(!CachedVod::is_subscription_token("subscriptions-x"));
    assert!(!CachedVod::is_subscription_token("channel-abc"));
}

fn raw_vod(no: i64, sources: &str) -> CachedVod {
    CachedVod {
        id: 0,
        video_no: no,
        video_id: None,
        video_title: format!("VOD {no}"),
        thumbnail_image_url: None,
        duration: 0,
        read_count: 0,
        publish_date: None,
        channel_id: None,
        channel_name: None,
        channel_image_url: None,
        content_hash: format!("h{no}"),
        sources: sources.into(),
        fetched_at: now(),
    }
}

/// 1 옛 토큰만 · 2 옛+A · 3 B 만 · 4 채널+옛 · 5 채널+B · 6 A 만 · 7 무관 토큰 · 8 A+B.
fn seed_purge(s: &DataStore) {
    s.insert_cached_vod_rows(&[
        raw_vod(1, "subscriptions"),
        raw_vod(2, "subscriptions,subscriptions.A"),
        raw_vod(3, "subscriptions.B"),
        raw_vod(4, "channel-x,subscriptions"),
        raw_vod(5, "channel-y,subscriptions.B"),
        raw_vod(6, "subscriptions.A"),
        raw_vod(7, "channel-z"),
        raw_vod(8, "subscriptions.A,subscriptions.B"),
    ])
    .unwrap();
}

fn sources_by_video(s: &DataStore) -> BTreeMap<i64, String> {
    (1..=20)
        .filter_map(|no| s.cached_vod_row(no).unwrap().map(|r| (no, r.sources)))
        .collect()
}

fn map(pairs: &[(i64, &str)]) -> BTreeMap<i64, String> {
    pairs.iter().map(|(k, v)| (*k, v.to_string())).collect()
}

#[test]
fn purge_keeping_scope_deletes_legacy_and_other_accounts() {
    let s = store();
    seed_purge(&s);
    assert_eq!(s.purge_subscription_vod_tags(Some(&tag("A"))).unwrap(), 6);
    assert_eq!(
        sources_by_video(&s),
        map(&[
            (2, "subscriptions.A"),
            (4, "channel-x"),
            (5, "channel-y"),
            (6, "subscriptions.A"),
            (7, "channel-z"),
            (8, "subscriptions.A"),
        ])
    );
}

#[test]
fn purge_without_keep_removes_only_legacy() {
    let s = store();
    seed_purge(&s);
    assert_eq!(s.purge_subscription_vod_tags(None).unwrap(), 3);
    assert_eq!(
        sources_by_video(&s),
        map(&[
            (2, "subscriptions.A"),
            (3, "subscriptions.B"),
            (4, "channel-x"),
            (5, "channel-y,subscriptions.B"),
            (6, "subscriptions.A"),
            (7, "channel-z"),
            (8, "subscriptions.A,subscriptions.B"),
        ])
    );
}

#[test]
fn purge_malformed_keep_falls_back_to_legacy_only() {
    for keep in ["", "subscriptions", "subscriptions.", "channel-x"] {
        let s = store();
        seed_purge(&s);
        assert_eq!(
            s.purge_subscription_vod_tags(Some(keep)).unwrap(),
            3,
            "keep={keep}"
        );
        assert_eq!(sources_by_video(&s)[&3], "subscriptions.B", "keep={keep}");
    }
}

#[test]
fn purge_second_run_returns_zero_and_does_not_save() {
    let dir = tempfile::tempdir().unwrap();
    let s = DataStore::open(dir.path().join("purge.store")).unwrap();
    seed_purge(&s);
    assert_eq!(s.purge_subscription_vod_tags(Some(&tag("A"))).unwrap(), 6);
    let after_first = s.history_transaction_count();
    assert_eq!(s.purge_subscription_vod_tags(Some(&tag("A"))).unwrap(), 0);
    assert_eq!(s.history_transaction_count(), after_first);
}

#[test]
fn purge_empty_store_is_no_op() {
    assert_eq!(
        store()
            .purge_subscription_vod_tags(Some(&tag("A")))
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn retention_maintenance_purges_legacy_rows() {
    let s = store();
    seed_purge(&s);
    s.run_retention_maintenance(Some(tag("A"))).await;
    let left = sources_by_video(&s);
    assert!(!left.contains_key(&1));
    assert!(!left.contains_key(&3));
    assert_eq!(left[&2], "subscriptions.A");
    assert_eq!(left[&7], "channel-z");
}

#[tokio::test]
async fn retention_maintenance_without_tag_keeps_account_tokens() {
    let s = store();
    seed_purge(&s);
    s.run_retention_maintenance(None).await;
    let left = sources_by_video(&s);
    assert!(!left.contains_key(&1));
    assert_eq!(left[&3], "subscriptions.B");
}

// MARK: - 구독 VOD 페이지

fn pages_base() -> Date {
    ts(1_790_000_000.0)
}

fn vod(no: i64, channel: Option<&str>, published: Option<i64>, sources: &[&str]) -> CachedVodData {
    let mut v = CachedVodData::new(no, format!("VOD {no}"));
    v.duration = 60;
    v.publish_date = published.map(|p| add(&pages_base(), p as f64 * 3600.0));
    v.channel_id = channel.map(str::to_string);
    v.channel_name = channel.map(|c| format!("이름 {c}"));
    v.content_hash = format!("h{no}");
    v.sources = sources.iter().map(|s| s.to_string()).collect();
    v.fetched_at = pages_base();
    v
}

fn page(channel: &str, page: i64, nos: &[i64], exhausted: bool) -> SubscriptionVodPage {
    SubscriptionVodPage::new(
        channel,
        page,
        nos.iter()
            .map(|n| vod(*n, Some(channel), Some(*n), &[]))
            .collect(),
    )
    .exhausted(exhausted)
}

fn apply(
    s: &DataStore,
    t: &str,
    pages: &[SubscriptionVodPage],
    followed: Option<&[&str]>,
) -> usize {
    let followed: Option<HashSet<String>> =
        followed.map(|f| f.iter().map(|s| s.to_string()).collect());
    s.apply_subscription_vod_pages(t, pages, followed.as_ref(), PER_CHANNEL)
        .unwrap()
}

fn channel_nos(s: &DataStore, t: &str, limit: usize) -> BTreeMap<String, Vec<i64>> {
    s.fetch_subscription_vods(t, limit)
        .unwrap()
        .into_iter()
        .map(|(k, v)| (k, v.iter().map(|x| x.video_no).collect()))
        .collect()
}

fn desc(range: std::ops::RangeInclusive<i64>) -> Vec<i64> {
    range.rev().collect()
}

#[test]
fn page_zero_replaces_only_newer_side() {
    let s = store();
    let a = tag("A");
    apply(
        &s,
        &a,
        &[
            page("c1", 0, &desc(93..=100), false),
            page("c1", 1, &desc(85..=92), false),
        ],
        None,
    );
    let changed = apply(
        &s,
        &a,
        &[page("c1", 0, &[101, 100, 99, 98, 96, 95, 94, 93], false)],
        None,
    );
    assert_eq!(changed, 2);
    let mut expected = vec![101, 100, 99, 98, 96, 95, 94, 93];
    expected.extend(desc(85..=92));
    assert_eq!(channel_nos(&s, &a, PER_CHANNEL)["c1"], expected);
}

#[test]
fn later_page_only_appends() {
    let s = store();
    let a = tag("A");
    apply(&s, &a, &[page("c1", 0, &desc(3..=10), false)], None);
    apply(&s, &a, &[page("c1", 1, &[2, 1], true)], None);
    assert_eq!(channel_nos(&s, &a, PER_CHANNEL)["c1"], desc(1..=10));
}

#[test]
fn exhausted_or_empty_page_zero_drops_rest_of_channel() {
    let s = store();
    let a = tag("A");
    apply(
        &s,
        &a,
        &[
            page("c1", 0, &desc(3..=10), false),
            page("c1", 1, &[2, 1], false),
            page("c2", 0, &[50, 49], false),
        ],
        None,
    );
    apply(&s, &a, &[page("c1", 0, &[10, 9], true)], None);
    let rows = channel_nos(&s, &a, PER_CHANNEL);
    assert_eq!(rows["c1"], vec![10, 9]);
    assert_eq!(rows["c2"], vec![50, 49]);
    apply(&s, &a, &[page("c1", 0, &[], false)], None);
    let rows = channel_nos(&s, &a, PER_CHANNEL);
    assert!(!rows.contains_key("c1"));
    assert_eq!(rows["c2"], vec![50, 49]);
}

#[test]
fn followed_removes_unfollowed_channels() {
    let s = store();
    let a = tag("A");
    apply(
        &s,
        &a,
        &[
            page("c1", 0, &[10, 9], false),
            page("c2", 0, &[20, 19], false),
        ],
        None,
    );
    apply(&s, &a, &[page("c3", 0, &[30], false)], Some(&["c1"]));
    let rows = channel_nos(&s, &a, PER_CHANNEL);
    assert_eq!(rows.keys().cloned().collect::<Vec<_>>(), vec!["c1"]);
    assert_eq!(rows["c1"], vec![10, 9]);
}

#[test]
fn per_channel_limit_drops_oldest() {
    let s = store();
    let a = tag("A");
    s.apply_subscription_vod_pages(
        &a,
        &[
            page("c1", 0, &desc(17..=24), false),
            page("c1", 1, &desc(9..=16), false),
            page("c1", 2, &desc(1..=8), false),
        ],
        None,
        20,
    )
    .unwrap();
    assert_eq!(channel_nos(&s, &a, 100)["c1"], desc(5..=24));
    assert_eq!(channel_nos(&s, &a, 3)["c1"], vec![24, 23, 22]);
}

#[test]
fn rows_shared_with_other_tokens_survive_untagging() {
    let s = store();
    let (a, b) = (tag("A"), tag("B"));
    s.upsert_cached_vods(&[
        vod(1, Some("c1"), Some(1), &["channel-x"]),
        vod(2, Some("c1"), Some(2), &[b.as_str()]),
    ])
    .unwrap();
    apply(&s, &a, &[page("c1", 0, &[3, 2, 1], false)], None);
    apply(&s, &a, &[page("c1", 0, &[3], true)], None);
    assert_eq!(
        sources_by_video(&s),
        map(&[(1, "channel-x"), (2, b.as_str()), (3, a.as_str())])
    );
    assert_eq!(channel_nos(&s, &b, PER_CHANNEL)["c1"], vec![2]);
}

#[test]
fn legacy_and_malformed_tags_are_neither_read_nor_written() {
    let s = store();
    s.upsert_cached_vods(&[vod(1, Some("c1"), Some(1), &["subscriptions"])])
        .unwrap();
    for bad in ["subscriptions", "", "subscriptions.", "subscriptions.A,B"] {
        assert_eq!(
            apply(&s, bad, &[page("c1", 0, &[9], false)], None),
            0,
            "tag={bad}"
        );
        assert!(channel_nos(&s, bad, PER_CHANNEL).is_empty(), "tag={bad}");
    }
    assert_eq!(sources_by_video(&s), map(&[(1, "subscriptions")]));
}

#[test]
fn missing_channel_id_is_filled_from_page() {
    let s = store();
    let a = tag("A");
    let items = vec![vod(7, None, Some(7), &[]), vod(6, None, Some(6), &[])];
    apply(&s, &a, &[SubscriptionVodPage::new("c9", 0, items)], None);
    let rows = s.fetch_subscription_vods(&a, PER_CHANNEL).unwrap();
    let nos: Vec<_> = rows["c9"].iter().map(|v| v.video_no).collect();
    assert_eq!(nos, vec![7, 6]);
    assert_eq!(rows["c9"][0].channel_id.as_deref(), Some("c9"));
}

#[test]
fn fetch_orders_newest_first_and_skips_rows_without_channel() {
    let s = store();
    let a = tag("A");
    s.upsert_cached_vods(&[
        vod(1, Some("c1"), None, &[a.as_str()]),
        vod(2, Some("c1"), Some(5), &[a.as_str()]),
        vod(3, Some("c1"), Some(5), &[a.as_str()]),
        vod(4, Some("c1"), Some(9), &[a.as_str()]),
        vod(5, None, Some(10), &[a.as_str()]),
        vod(6, Some("c1"), Some(10), &["subscriptions.AB"]),
    ])
    .unwrap();
    let rows = channel_nos(&s, &a, PER_CHANNEL);
    assert_eq!(rows.keys().cloned().collect::<Vec<_>>(), vec!["c1"]);
    assert_eq!(rows["c1"], vec![4, 3, 2, 1]);
    assert!(channel_nos(&s, &a, 0).is_empty());
}

#[test]
fn unchanged_apply_returns_zero_and_does_not_save() {
    let dir = tempfile::tempdir().unwrap();
    let s = DataStore::open(dir.path().join("pages.store")).unwrap();
    let a = tag("A");
    let pages = [
        page("c1", 0, &[10, 9, 8], false),
        page("c1", 1, &[7], false),
        page("c2", 0, &[20], false),
    ];
    assert_eq!(apply(&s, &a, &pages, Some(&["c1", "c2"])), 5);
    let after_first = s.history_transaction_count();
    assert_eq!(apply(&s, &a, &pages, Some(&["c1", "c2"])), 0);
    assert_eq!(s.history_transaction_count(), after_first);
}
