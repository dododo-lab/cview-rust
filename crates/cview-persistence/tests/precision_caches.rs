//! 원본: `Tests/CViewPersistenceTests/PrecisionReview0926ClipCacheTests.swift`,
//! `PrecisionReview0926VODCacheTests.swift`, `PrecisionReview0926VODPlaybackTests.swift`.
//!
//! Swift 의 음수 limit 검사는 Rust 에서 `usize` 라 표현할 수 없어 0 검사만 옮겼다.
//! Swift 기본 limit: 클립 200, VOD 120.

mod common;

use common::*;
use cview_persistence::models::{CachedClipData, CachedVod, CachedVodData, PersistedVodPlayback};
use cview_persistence::time::{interval_since, Date};
use cview_persistence::DataStore;
use std::collections::HashSet;

const WEEK_POPULAR: &str = "trending-within_7_days-popular";
const CLIP_DEFAULT_LIMIT: usize = 200;
const VOD_DEFAULT_LIMIT: usize = 120;

fn base() -> Date {
    ts(1_700_000_000.0)
}

fn tags(items: &[&str]) -> std::collections::BTreeSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

// MARK: - 클립 캐시

fn clip(uid: &str, sources: &[&str], at: i64, read_count: i64) -> CachedClipData {
    let mut c = CachedClipData::new(uid, format!("클립 {uid}"));
    c.duration = 30;
    c.read_count = read_count;
    c.content_hash = if read_count == 0 {
        uid.to_string()
    } else {
        format!("{uid}-{read_count}")
    };
    c.sources = tags(sources);
    c.fetched_at = add(&base(), at as f64);
    c
}

fn seed_clips(s: &DataStore, targets: usize, newer: usize, newer_source: impl Fn(usize) -> String) {
    let t: Vec<_> = (0..targets)
        .map(|i| clip(&format!("target{i}"), &[WEEK_POPULAR], 0, 0))
        .collect();
    s.upsert_cached_clips(&t).unwrap();
    let n: Vec<_> = (0..newer)
        .map(|i| {
            let src = newer_source(i);
            clip(&format!("other{i}"), &[src.as_str()], 1, 0)
        })
        .collect();
    if !n.is_empty() {
        s.upsert_cached_clips(&n).unwrap();
    }
}

fn clip_uids(s: &DataStore, source: &str, limit: usize) -> Vec<String> {
    s.fetch_cached_clips(source, limit)
        .unwrap()
        .into_iter()
        .map(|c| c.clip_uid)
        .collect()
}

#[test]
fn clip_home_limit_one_finds_target_behind_200_newer_rows() {
    let s = store();
    seed_clips(&s, 1, 200, |i| format!("channel-c{i}-popular"));
    assert_eq!(clip_uids(&s, WEEK_POPULAR, 1), vec!["target0"]);
}

#[test]
fn clip_window_boundary_around_200() {
    for newer in [199, 200, 201] {
        let s = store();
        seed_clips(&s, 1, newer, |i| format!("channel-c{i}-popular"));
        assert_eq!(
            clip_uids(&s, WEEK_POPULAR, 1),
            vec!["target0"],
            "newer={newer}"
        );
    }
}

#[test]
fn clip_hydrate_default_limit_finds_rows_behind_800_newer() {
    let s = store();
    seed_clips(&s, 3, 800, |i| {
        if i % 2 == 0 {
            "trending-within_1_day-popular".into()
        } else {
            "trending-within_30_days-recommend".into()
        }
    });
    assert_eq!(
        clip_uids(&s, WEEK_POPULAR, CLIP_DEFAULT_LIMIT),
        vec!["target0", "target1", "target2"]
    );
}

#[test]
fn clip_substring_near_misses_fall_back() {
    let s = store();
    seed_clips(&s, 1, 250, |_| format!("{WEEK_POPULAR}-decoy"));
    assert_eq!(clip_uids(&s, WEEK_POPULAR, 1), vec!["target0"]);
}

#[test]
fn clip_exact_tag_semantics_and_order() {
    let s = store();
    s.upsert_cached_clips(&[
        clip("ab", &["a", "b"], 3, 0),
        clip("near", &["a-x"], 2, 0),
        clip("upper", &["A"], 1, 0),
        clip("a-old", &["a"], 0, 0),
        clip("a-older", &["a"], -1, 0),
    ])
    .unwrap();
    assert_eq!(
        clip_uids(&s, "a", CLIP_DEFAULT_LIMIT),
        vec!["ab", "a-old", "a-older"]
    );
    assert_eq!(clip_uids(&s, "b", CLIP_DEFAULT_LIMIT), vec!["ab"]);
    assert_eq!(clip_uids(&s, "a", 2), vec!["ab", "a-old"]);
}

#[test]
fn clip_unchanged_upsert_keeps_fetched_at_and_retain_drops_missing() {
    let s = store();
    let original = clip("keep", &["trending"], 0, 0);
    assert_eq!(
        s.upsert_cached_clips(&[original.clone(), clip("drop", &["trending", "other"], 0, 0)])
            .unwrap(),
        2
    );
    let before = s.history_transaction_count();
    assert_eq!(
        s.upsert_cached_clips(&[clip("keep", &["trending"], 50, 0)])
            .unwrap(),
        0
    );
    assert_eq!(s.history_transaction_count(), before);
    let kept = s
        .fetch_cached_clips("trending", CLIP_DEFAULT_LIMIT)
        .unwrap();
    assert_eq!(
        kept.iter()
            .find(|c| c.clip_uid == "keep")
            .unwrap()
            .fetched_at,
        original.fetched_at
    );
    assert_eq!(
        s.retain_cached_clip_source("trending", &HashSet::from(["keep".to_string()]))
            .unwrap(),
        1
    );
    assert_eq!(clip_uids(&s, "trending", CLIP_DEFAULT_LIMIT), vec!["keep"]);
    assert_eq!(clip_uids(&s, "other", CLIP_DEFAULT_LIMIT), vec!["drop"]);
}

#[test]
fn clip_partially_changed_batch_keeps_order_and_shares_stamp() {
    let s = store();
    let first: Vec<_> = ["A", "B", "C"]
        .iter()
        .map(|u| clip(u, &[WEEK_POPULAR], 0, 0))
        .collect();
    assert_eq!(s.upsert_cached_clips(&first).unwrap(), 3);
    let second: Vec<_> = ["A", "B", "C"]
        .iter()
        .map(|u| clip(u, &[WEEK_POPULAR], 300, if *u == "C" { 99 } else { 0 }))
        .collect();
    assert_eq!(s.upsert_cached_clips(&second).unwrap(), 1);
    let rows = s
        .fetch_cached_clips(WEEK_POPULAR, CLIP_DEFAULT_LIMIT)
        .unwrap();
    let uids: Vec<_> = rows.iter().map(|c| c.clip_uid.as_str()).collect();
    assert_eq!(uids, vec!["A", "B", "C"]);
    let stamp = add(&base(), 300.0);
    assert_eq!(rows[0].fetched_at, stamp);
    assert!(rows
        .iter()
        .all(|c| interval_since(&stamp, &c.fetched_at) < 1.0));
    assert_eq!(rows.last().unwrap().read_count, 99);
    assert_eq!(clip_uids(&s, WEEK_POPULAR, 1), vec!["A"]);
}

#[test]
fn clip_batch_stamp_is_latest_item_time_and_rank_follows_order() {
    let s = store();
    s.upsert_cached_clips(&[
        clip("x", &["s"], 5, 0),
        clip("y", &["s"], 9, 0),
        clip("z", &["s"], 1, 0),
        clip("x", &["s"], 9, 0),
    ])
    .unwrap();
    let rows = s.fetch_cached_clips("s", CLIP_DEFAULT_LIMIT).unwrap();
    let uids: Vec<_> = rows.iter().map(|c| c.clip_uid.as_str()).collect();
    assert_eq!(uids, vec!["x", "y", "z"]);
    let top = add(&base(), 9.0);
    assert_eq!(rows[0].fetched_at, top);
    assert!(rows
        .iter()
        .all(|c| interval_since(&top, &c.fetched_at) < 1.0));
}

#[test]
fn clip_fresh_batch_of_many_rows_keeps_order() {
    let s = store();
    let uids: Vec<String> = (0..100).map(|i| format!("r{i}")).collect();
    let items: Vec<_> = uids
        .iter()
        .map(|u| clip(u, &[WEEK_POPULAR], 0, 0))
        .collect();
    s.upsert_cached_clips(&items).unwrap();
    assert_eq!(clip_uids(&s, WEEK_POPULAR, CLIP_DEFAULT_LIMIT), uids);
    assert_eq!(clip_uids(&s, WEEK_POPULAR, 1), vec!["r0"]);
}

#[test]
fn clip_zero_limit_and_empty_source_return_empty() {
    let s = store();
    seed_clips(&s, 2, 0, |_| "unused".into());
    assert!(clip_uids(&s, WEEK_POPULAR, 0).is_empty());
    assert!(clip_uids(&s, "", 5).is_empty());
}

// MARK: - VOD 캐시

const SUBS: &str = "subscriptions";

fn vod(no: i64, sources: &[&str], fetched: i64, published: Option<i64>) -> CachedVodData {
    let mut v = CachedVodData::new(no, format!("VOD {no}"));
    v.duration = 60;
    v.publish_date = published.map(|p| add(&base(), p as f64));
    v.content_hash = format!("h{no}");
    v.sources = tags(sources);
    v.fetched_at = add(&base(), fetched as f64);
    v
}

fn vod_nos(s: &DataStore, source: &str, limit: usize) -> Vec<i64> {
    s.fetch_cached_vods(source, limit)
        .unwrap()
        .into_iter()
        .map(|v| v.video_no)
        .collect()
}

#[test]
fn vod_finds_subscription_rows_behind_newer_rows() {
    for newer in [199_i64, 200, 480, 481] {
        let s = store();
        let mut items = vec![vod(1, &[SUBS], 0, Some(0))];
        items.extend((0..newer).map(|i| {
            let src = format!("channel-c{i}");
            vod(1_000 + i, &[src.as_str()], i + 1, None)
        }));
        s.upsert_cached_vods(&items).unwrap();
        assert_eq!(vod_nos(&s, SUBS, 1), vec![1], "newer={newer}");
        assert_eq!(
            vod_nos(&s, SUBS, VOD_DEFAULT_LIMIT),
            vec![1],
            "newer={newer}"
        );
    }
}

#[test]
fn vod_substring_near_misses_fall_back() {
    let s = store();
    let mut items = vec![vod(1, &[SUBS], 0, None)];
    items.extend((0..200).map(|i| vod(1_000 + i, &["subscriptions-x"], i + 1, None)));
    s.upsert_cached_vods(&items).unwrap();
    assert_eq!(vod_nos(&s, SUBS, 1), vec![1]);
}

#[test]
fn vod_same_batch_keeps_newest_published_within_limit() {
    let s = store();
    let mut items: Vec<_> = (1..=128).map(|i| vod(i, &[SUBS], 0, Some(i))).collect();
    items.push(vod(500, &[SUBS], 0, None));
    items.push(vod(501, &[SUBS], 0, None));
    s.upsert_cached_vods(&items).unwrap();
    assert_eq!(
        vod_nos(&s, SUBS, 120),
        (9..=128).rev().collect::<Vec<i64>>()
    );
}

#[test]
fn vod_unchanged_upsert_keeps_fetched_at_and_retain_drops_missing() {
    let s = store();
    let original = vod(1, &[SUBS], 0, Some(10));
    assert_eq!(
        s.upsert_cached_vods(&[original.clone(), vod(2, &[SUBS, "channel-a"], 1, Some(9))])
            .unwrap(),
        2
    );
    assert_eq!(
        s.upsert_cached_vods(&[vod(1, &[SUBS], 80, Some(10))])
            .unwrap(),
        0
    );
    let kept = s.fetch_cached_vods(SUBS, VOD_DEFAULT_LIMIT).unwrap();
    assert_eq!(
        kept.iter().find(|v| v.video_no == 1).unwrap().fetched_at,
        original.fetched_at
    );
    assert_eq!(
        s.retain_cached_vod_source(SUBS, &HashSet::from([1]))
            .unwrap(),
        1
    );
    assert_eq!(vod_nos(&s, SUBS, VOD_DEFAULT_LIMIT), vec![1]);
    assert_eq!(vod_nos(&s, "channel-a", VOD_DEFAULT_LIMIT), vec![2]);
}

#[test]
fn vod_exact_tag_and_zero_limit() {
    let s = store();
    s.upsert_cached_vods(&[
        vod(1, &[SUBS, "channel-a"], 2, None),
        vod(2, &["subscriptions-x"], 3, None),
        vod(3, &[SUBS], 1, None),
    ])
    .unwrap();
    assert_eq!(vod_nos(&s, SUBS, 10), vec![1, 3]);
    assert!(vod_nos(&s, SUBS, 0).is_empty());
    assert!(vod_nos(&s, "", 10).is_empty());
}

#[test]
fn vod_scoped_tags_isolate_accounts() {
    let s = store();
    let a = CachedVod::subscription_tag("0A1B-A").unwrap();
    let b = CachedVod::subscription_tag("0A1B-AB").unwrap();
    s.upsert_cached_vods(&[
        vod(1, &[a.as_str()], 1, Some(1)),
        vod(2, &[b.as_str()], 2, Some(2)),
        vod(3, &[a.as_str(), b.as_str()], 3, Some(3)),
        vod(4, &[SUBS], 4, Some(4)),
    ])
    .unwrap();
    assert_eq!(vod_nos(&s, &a, VOD_DEFAULT_LIMIT), vec![3, 1]);
    assert_eq!(vod_nos(&s, &b, VOD_DEFAULT_LIMIT), vec![3, 2]);
    assert_eq!(vod_nos(&s, SUBS, VOD_DEFAULT_LIMIT), vec![4]);
}

// MARK: - VOD 이어 보기

fn playback_base() -> Date {
    ts(1_800_000_000.0)
}

fn seed_playback(rows: &[(i64, f64, f64, f64)]) -> DataStore {
    let s = store();
    let rows: Vec<_> = rows
        .iter()
        .map(|&(no, pos, dur, seen)| PersistedVodPlayback {
            id: 0,
            video_no: no,
            last_position_sec: pos,
            duration_sec: dur,
            video_title: format!("VOD {no}"),
            channel_name: "채널".into(),
            thumbnail_url_string: None,
            last_seen_at: add(&playback_base(), seen),
        })
        .collect();
    s.insert_vod_playback_rows(&rows).unwrap();
    s
}

fn recent(s: &DataStore, limit: usize) -> Vec<i64> {
    s.fetch_recent_vod_playback(limit)
        .unwrap()
        .into_iter()
        .map(|p| p.video_no)
        .collect()
}

#[test]
fn playback_home_limit_one_skips_newer_ineligible_rows() {
    let mut rows = vec![(100, 600.0, 1000.0, 0.0)];
    rows.extend((1..=5).map(|i| (i, 20.0, 600.0, i as f64)));
    assert_eq!(recent(&seed_playback(&rows), 1), vec![100]);
}

#[test]
fn playback_browse_limit_finds_eligible_behind_completed_rows() {
    let mut rows = vec![(100, 600.0, 1000.0, 1.0), (101, 600.0, 1000.0, 0.0)];
    rows.extend((0..200).map(|i| (1_000 + i, 980.0, 1000.0, 10.0 + i as f64)));
    assert_eq!(recent(&seed_playback(&rows), 30), vec![100, 101]);
}

#[test]
fn playback_collects_across_pages_and_stops_at_limit() {
    let mut rows = Vec::new();
    let mut seen = 1_000.0;
    let mut next = || {
        seen -= 1.0;
        seen
    };
    for i in 0..150 {
        rows.push((2_000 + i, 10.0, 600.0, next()));
    }
    rows.push((1, 100.0, 1000.0, next()));
    for i in 0..150 {
        rows.push((3_000 + i, 999.0, 1000.0, next()));
    }
    rows.push((2, 100.0, 1000.0, next()));
    rows.push((3, 100.0, 1000.0, next()));
    assert_eq!(recent(&seed_playback(&rows), 2), vec![1, 2]);
}

#[test]
fn playback_tie_on_last_seen_is_deterministic_across_pages() {
    let rows: Vec<_> = (1..=130)
        .map(|no| {
            let eligible = no == 70 || no == 71;
            (
                no,
                if eligible { 100.0 } else { 10.0 },
                if eligible { 1000.0 } else { 600.0 },
                0.0,
            )
        })
        .collect();
    let s = seed_playback(&rows);
    assert_eq!(recent(&s, 1), vec![71]);
    assert_eq!(recent(&s, 5), vec![71, 70]);
}

#[test]
fn playback_eligibility_boundaries() {
    let s = seed_playback(&[
        (1, 30.0, 100.0, 6.0),
        (2, 31.0, 1000.0, 5.0),
        (3, 950.0, 1000.0, 4.0),
        (4, 949.0, 1000.0, 3.0),
        (5, 45.0, 0.0, 2.0),
        (6, 200.0, 100.0, 1.0),
    ]);
    assert_eq!(recent(&s, 10), vec![2, 4, 5]);
}

#[test]
fn playback_zero_limit_returns_empty() {
    let s = seed_playback(&[(1, 100.0, 1000.0, 0.0)]);
    assert!(recent(&s, 0).is_empty());
}
