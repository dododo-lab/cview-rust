
//! 원본: `Tests/CViewCoreTests/LiveDetailRefreshPolicyTests.swift`
use super::*;
use crate::utils::time::from_unix;
use LiveDetailReason as R;

const NOW: f64 = 10_000.0;

fn row(id: &str) -> FollowingRowSnapshot {
    FollowingRowSnapshot {
        channel_id: id.into(),
        channel_name: format!("name-{id}"),
        channel_image_url: Some(format!("https://img/{id}")),
        is_live: true,
        ..Default::default()
    }
}

fn offline(id: &str) -> FollowingRowSnapshot {
    FollowingRowSnapshot {
        is_live: false,
        ..row(id)
    }
}

fn meta_at(fetched_at: f64) -> LiveDetailMeta {
    meta_with(1, fetched_at)
}

fn meta_with(live_id: i64, fetched_at: f64) -> LiveDetailMeta {
    LiveDetailMeta {
        live_id,
        fetched_at_uptime: fetched_at,
        title: "meta-title".into(),
        viewers: 10,
        category: Some("meta-cat".into()),
        category_type: Some("GAME".into()),
        thumbnail_url: Some("https://thumb/meta".into()),
        open_date: Some(from_unix(1_000.0)),
        is_open: true,
    }
}

fn detail() -> LiveDetailSnapshot {
    LiveDetailSnapshot {
        live_id: 1,
        live_title: "detail-title".into(),
        concurrent_user_count: 20,
        live_category_value: Some("detail-cat".into()),
        category_type: Some("SPORTS".into()),
        thumbnail_url: Some("https://thumb/detail".into()),
        open_date: Some(from_unix(2_000.0)),
        is_closed: false,
    }
}

fn bare_detail(live_id: i64) -> LiveDetailSnapshot {
    LiveDetailSnapshot {
        live_id,
        category_type: None,
        thumbnail_url: None,
        open_date: None,
        ..detail()
    }
}

fn set(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

fn ids(prefix: &str, n: usize) -> Vec<String> {
    (0..n).map(|i| format!("{prefix}{i}")).collect()
}

#[derive(Default)]
struct P {
    previous_live: Option<HashSet<String>>,
    meta: HashMap<String, LiveDetailMeta>,
    visible: HashSet<String>,
    watching: HashSet<String>,
    options: PlanOptions,
    now: Option<f64>,
}

fn run(rows: &[FollowingRowSnapshot], p: P) -> Vec<LiveDetailRequest> {
    plan(
        rows,
        p.previous_live.as_ref(),
        &p.meta,
        &p.visible,
        |id| p.watching.contains(id),
        p.options,
        p.now.unwrap_or(NOW),
        &LiveDetailRefreshConfig::default(),
    )
}

fn req(id: &str, r: R) -> LiveDetailRequest {
    LiveDetailRequest::new(id, r)
}

#[test]
fn no_live_rows_plans_nothing() {
    let rows = [offline("a"), offline("b")];
    let p = P {
        previous_live: Some(set(&[])),
        options: PlanOptions {
            manual: true,
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(run(&rows, p).is_empty());
}

#[test]
fn fresh_meta_plans_nothing() {
    let all = ids("c", 71);
    let rows: Vec<_> = all.iter().map(|i| row(i)).collect();
    let meta = all
        .iter()
        .map(|i| (i.clone(), meta_at(NOW - 30.0)))
        .collect();
    let p = P {
        previous_live: Some(all.iter().cloned().collect()),
        meta,
        visible: all.iter().take(6).cloned().collect(),
        ..Default::default()
    };
    assert!(run(&rows, p).is_empty());
}

#[test]
fn new_live_first_and_uncapped() {
    let old = ids("old", 5);
    let new = ids("new", 30);
    let mut rows: Vec<_> = old.iter().chain(&new).map(|i| row(i)).collect();
    rows.push(offline("off"));
    let meta = old
        .iter()
        .map(|i| (i.clone(), meta_at(NOW - 5_000.0)))
        .collect();
    let result = run(
        &rows,
        P {
            previous_live: Some(old.iter().cloned().collect()),
            meta,
            visible: set(&[&old[0]]),
            ..Default::default()
        },
    );
    let new_live: Vec<_> = result
        .iter()
        .take_while(|r| r.reason == R::NewLive)
        .map(|r| r.channel_id.clone())
        .collect();
    assert_eq!(new_live, new);
    assert_eq!(result.len(), new.len() + old.len());
    assert!(result[new.len()..]
        .iter()
        .all(|r| matches!(r.reason, R::VisibleDue | R::BackgroundDue)));
    assert!(!result.iter().any(|r| r.channel_id == "off"));
}

#[test]
fn previous_live_none_versus_empty() {
    let rows = [row("a"), row("b")];
    assert!(run(&rows, P::default())
        .iter()
        .all(|r| r.reason == R::MissingMeta));
    let after = run(
        &rows,
        P {
            previous_live: Some(set(&[])),
            ..Default::default()
        },
    );
    assert_eq!(
        after.iter().map(|r| r.reason).collect::<Vec<_>>(),
        vec![R::NewLive, R::NewLive]
    );
}

#[test]
fn missing_meta_cap_visible_first() {
    let all = ids("c", 30);
    let rows: Vec<_> = all.iter().map(|i| row(i)).collect();
    let first = run(
        &rows,
        P {
            visible: set(&["c25", "c28"]),
            ..Default::default()
        },
    );
    assert_eq!(first.len(), 24);
    assert!(first.iter().all(|r| r.reason == R::MissingMeta));
    assert_eq!(
        first[..2]
            .iter()
            .map(|r| r.channel_id.as_str())
            .collect::<Vec<_>>(),
        ["c25", "c28"]
    );
    assert_eq!(
        first[2..]
            .iter()
            .map(|r| r.channel_id.clone())
            .collect::<Vec<_>>(),
        all[..22].to_vec()
    );

    let meta = first
        .iter()
        .map(|r| (r.channel_id.clone(), meta_at(NOW)))
        .collect();
    let second = run(
        &rows,
        P {
            previous_live: Some(all.iter().cloned().collect()),
            meta,
            visible: set(&["c25", "c28"]),
            now: Some(NOW + 60.0),
            ..Default::default()
        },
    );
    assert_eq!(
        second
            .iter()
            .map(|r| r.channel_id.as_str())
            .collect::<Vec<_>>(),
        ["c22", "c23", "c24", "c26", "c27", "c29"]
    );
    assert!(second.iter().all(|r| r.reason == R::MissingMeta));
}

#[test]
fn notify_watch_only_without_list_live_info() {
    let rows = [row("a"), row("b"), row("c")];
    let meta: HashMap<_, _> = ["a", "b", "c"]
        .iter()
        .map(|i| (i.to_string(), meta_at(NOW - 1.0)))
        .collect();
    let without = run(
        &rows,
        P {
            previous_live: Some(set(&["a", "b", "c"])),
            meta: meta.clone(),
            watching: set(&["b", "c"]),
            ..Default::default()
        },
    );
    assert_eq!(
        without,
        vec![req("b", R::NotifyWatch), req("c", R::NotifyWatch)]
    );
    let with = run(
        &rows,
        P {
            previous_live: Some(set(&["a", "b", "c"])),
            meta,
            watching: set(&["b", "c"]),
            options: PlanOptions {
                list_carries_live_info: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    assert!(with.is_empty());
}

#[test]
fn notify_watch_beyond_missing_meta_cap() {
    let rows: Vec<_> = ids("c", 30).iter().map(|i| row(i)).collect();
    let result = run(
        &rows,
        P {
            watching: set(&["c29"]),
            ..Default::default()
        },
    );
    assert_eq!(result.len(), 25);
    assert_eq!(result.last(), Some(&req("c29", R::NotifyWatch)));
}

fn single(meta: LiveDetailMeta, visible: bool, options: PlanOptions) -> Vec<LiveDetailRequest> {
    run(
        &[row("a")],
        P {
            previous_live: Some(set(&["a"])),
            meta: HashMap::from([("a".to_string(), meta)]),
            visible: if visible { set(&["a"]) } else { set(&[]) },
            options,
            ..Default::default()
        },
    )
}

#[test]
fn visible_boundary() {
    assert!(single(meta_at(NOW - 119.9), true, PlanOptions::default()).is_empty());
    assert_eq!(
        single(meta_at(NOW - 120.0), true, PlanOptions::default()),
        vec![req("a", R::VisibleDue)]
    );
}

#[test]
fn background_boundary() {
    let d = PlanOptions::default();
    let info = PlanOptions {
        list_carries_live_info: true,
        ..Default::default()
    };
    assert!(single(meta_at(NOW - 599.9), false, d).is_empty());
    assert_eq!(
        single(meta_at(NOW - 600.0), false, d),
        vec![req("a", R::BackgroundDue)]
    );
    assert!(single(meta_at(NOW - 600.0), false, info).is_empty());
    assert!(single(meta_at(NOW - 1799.9), false, info).is_empty());
    assert_eq!(
        single(meta_at(NOW - 1800.0), false, info),
        vec![req("a", R::BackgroundDue)]
    );
}

#[test]
fn suspended_boundary() {
    let sus = PlanOptions {
        suspended: true,
        ..Default::default()
    };
    assert!(single(meta_at(NOW - 600.0), true, sus).is_empty());
    assert!(single(meta_at(NOW - 1799.9), false, sus).is_empty());
    assert_eq!(
        single(meta_at(NOW - 1800.0), false, sus),
        vec![req("a", R::BackgroundDue)]
    );
}

#[test]
fn due_cap_oldest_first() {
    let all = ids("c", 20);
    let rows: Vec<_> = all.iter().map(|i| row(i)).collect();
    let mut meta: HashMap<String, LiveDetailMeta> = all
        .iter()
        .enumerate()
        .map(|(i, id)| (id.clone(), meta_at(NOW - 700.0 - i as f64 * 50.0)))
        .collect();
    let c3 = meta["c3"].clone();
    meta.insert("c4".into(), c3);
    let prev: HashSet<String> = all.iter().cloned().collect();
    let result = run(
        &rows,
        P {
            previous_live: Some(prev.clone()),
            meta: meta.clone(),
            visible: set(&["c1"]),
            ..Default::default()
        },
    );
    assert_eq!(result.len(), 12);
    let expected: Vec<String> = (8..20).rev().map(|i| format!("c{i}")).collect();
    assert_eq!(
        result
            .iter()
            .map(|r| r.channel_id.clone())
            .collect::<Vec<_>>(),
        expected
    );
    assert!(result.iter().all(|r| r.reason == R::BackgroundDue));

    meta.insert("c1".into(), meta_at(NOW - 1_000_000.0));
    let with_visible = run(
        &rows,
        P {
            previous_live: Some(prev),
            meta,
            visible: set(&["c1"]),
            ..Default::default()
        },
    );
    assert_eq!(with_visible.first(), Some(&req("c1", R::VisibleDue)));
    assert_eq!(with_visible.len(), 12);
}

#[test]
fn due_tie_keeps_list_order() {
    let all = ids("c", 15);
    let meta = all
        .iter()
        .map(|i| (i.clone(), meta_at(NOW - 900.0)))
        .collect();
    let rows: Vec<_> = all.iter().map(|i| row(i)).collect();
    let result = run(
        &rows,
        P {
            previous_live: Some(all.iter().cloned().collect()),
            meta,
            ..Default::default()
        },
    );
    assert_eq!(
        result
            .iter()
            .map(|r| r.channel_id.clone())
            .collect::<Vec<_>>(),
        all[..12].to_vec()
    );
}

#[test]
fn future_meta_is_due() {
    assert_eq!(
        single(meta_at(NOW + 50_000.0), false, PlanOptions::default()),
        vec![req("a", R::BackgroundDue)]
    );
}

#[test]
fn manual_takes_all_live() {
    let rows = [row("a"), row("b"), offline("off"), row("c"), row("n")];
    let meta = ["a", "b", "c"]
        .iter()
        .map(|i| (i.to_string(), meta_at(NOW)))
        .collect();
    let result = run(
        &rows,
        P {
            previous_live: Some(set(&["a", "b", "c"])),
            meta,
            visible: set(&["c"]),
            options: PlanOptions {
                manual: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    assert_eq!(
        result,
        vec![
            req("n", R::NewLive),
            req("c", R::Manual),
            req("a", R::Manual),
            req("b", R::Manual)
        ]
    );
}

#[test]
fn duplicate_rows_planned_once() {
    let result = run(
        &[row("a"), row("a"), row("")],
        P {
            previous_live: Some(set(&[])),
            ..Default::default()
        },
    );
    assert_eq!(result, vec![req("a", R::NewLive)]);
}

#[test]
fn reason_ordering() {
    let mut all = LiveDetailReason::ALL;
    all.sort();
    assert_eq!(
        all,
        [
            R::NewLive,
            R::MissingMeta,
            R::NotifyWatch,
            R::VisibleDue,
            R::BackgroundDue,
            R::Manual
        ]
    );
}

// MARK: 병합

#[test]
fn merge_list_wins() {
    let r = FollowingRowSnapshot {
        list_title: Some("list-title".into()),
        list_viewers: Some(99),
        list_category: Some("list-cat".into()),
        ..row("a")
    };
    let listed = merge(&r, Some(&detail()), Some(&meta_at(0.0)), NOW);
    assert_eq!(listed.item.live_title, "list-title");
    assert_eq!(listed.item.viewer_count, 99);
    assert_eq!(listed.item.category_name.as_deref(), Some("list-cat"));
    assert_eq!(
        listed.item.thumbnail_url.as_deref(),
        Some("https://thumb/detail")
    );
    assert_eq!(listed.item.category_type.as_deref(), Some("SPORTS"));
    assert_eq!(listed.item.open_date, Some(from_unix(2_000.0)));
    let m = listed.meta.unwrap();
    assert_eq!(m.title, "detail-title");
    assert_eq!(m.fetched_at_uptime, NOW);
}

#[test]
fn merge_detail_beats_meta() {
    let d = merge(&row("a"), Some(&detail()), Some(&meta_at(0.0)), NOW);
    assert_eq!(d.item.live_title, "detail-title");
    assert_eq!(d.item.viewer_count, 20);
    assert_eq!(d.item.category_name.as_deref(), Some("detail-cat"));
    assert!(d.item.is_live);
}

#[test]
fn merge_falls_back_to_meta() {
    let old = meta_at(0.0);
    let m = merge(&row("a"), None, Some(&old), NOW);
    assert_eq!(m.item.live_title, "meta-title");
    assert_eq!(m.item.viewer_count, 10);
    assert_eq!(m.item.category_name.as_deref(), Some("meta-cat"));
    assert_eq!(m.item.category_type.as_deref(), Some("GAME"));
    assert_eq!(m.item.thumbnail_url.as_deref(), Some("https://thumb/meta"));
    assert!(m.item.is_live);
    assert_eq!(m.meta, Some(old));
}

#[test]
fn merge_partial_list_values() {
    let r = FollowingRowSnapshot {
        list_viewers: Some(7),
        ..row("a")
    };
    let m = merge(&r, None, Some(&meta_at(0.0)), NOW);
    assert_eq!(m.item.viewer_count, 7);
    assert_eq!(m.item.live_title, "meta-title");
    assert_eq!(m.item.channel_name, "name-a");
    assert_eq!(m.item.channel_image_url.as_deref(), Some("https://img/a"));
    assert_eq!(m.item.id, "a");
}

#[test]
fn merge_same_live_carries_missing_fields() {
    let old = meta_with(5, 0.0);
    let r = merge(&row("a"), Some(&bare_detail(5)), Some(&old), NOW);
    assert_eq!(r.item.thumbnail_url.as_deref(), Some("https://thumb/meta"));
    assert_eq!(r.item.open_date, Some(from_unix(1_000.0)));
    assert_eq!(r.item.category_type.as_deref(), Some("GAME"));
    assert_eq!(
        r.meta.unwrap().thumbnail_url.as_deref(),
        Some("https://thumb/meta")
    );
}

#[test]
fn merge_live_id_change_replaces_meta() {
    let old = meta_with(5, 0.0);
    let r = merge(&row("a"), Some(&bare_detail(6)), Some(&old), NOW);
    let m = r.meta.clone().unwrap();
    assert_eq!(m.live_id, 6);
    assert_eq!(m.thumbnail_url, None);
    assert_eq!(m.open_date, None);
    assert_eq!(m.category_type, None);
    assert_eq!(r.item.thumbnail_url, None);
    assert_eq!(r.item.live_title, "detail-title");
}

#[test]
fn offline_drops_meta_then_restart_is_new_live() {
    let old = meta_with(5, NOW - 10.0);
    let off = merge(&offline("a"), None, Some(&old), NOW);
    assert_eq!(off.meta, None);
    assert!(!off.item.is_live);
    assert!(off.item.live_title.is_empty());
    assert_eq!(off.item.thumbnail_url, None);
    assert_eq!(off.item.open_date, None);

    let p = run(
        &[row("a")],
        P {
            previous_live: Some(set(&[])),
            now: Some(NOW + 60.0),
            ..Default::default()
        },
    );
    assert_eq!(p, vec![req("a", R::NewLive)]);

    let restarted = merge(
        &row("a"),
        Some(&LiveDetailSnapshot {
            live_id: 6,
            ..detail()
        }),
        off.meta.as_ref(),
        NOW + 60.0,
    );
    assert_eq!(restarted.meta.unwrap().live_id, 6);
    assert!(restarted.item.is_live);
    assert_eq!(restarted.item.open_date, Some(from_unix(2_000.0)));
}

#[test]
fn merge_close_detail() {
    let closed = merge(
        &row("a"),
        Some(&LiveDetailSnapshot {
            is_closed: true,
            ..detail()
        }),
        None,
        NOW,
    );
    assert!(!closed.item.is_live);
    assert_eq!(closed.item.open_date, None);
    let closed_meta = closed.meta.unwrap();
    assert!(!closed_meta.is_open);

    let next = merge(&row("a"), None, Some(&closed_meta), NOW + 60.0);
    assert!(
        !next.item.is_live,
        "폴마다 라이브↔오프라인이 뒤집히면 배지·알림이 흔들린다"
    );

    let p = run(
        &[row("a")],
        P {
            previous_live: Some(set(&["a"])),
            meta: HashMap::from([("a".to_string(), closed_meta)]),
            now: Some(NOW + 600.0),
            ..Default::default()
        },
    );
    assert_eq!(p, vec![req("a", R::BackgroundDue)]);
}

#[test]
fn merge_without_any_detail() {
    let r = merge(&row("a"), None, None, NOW);
    assert_eq!(r.meta, None);
    assert!(r.item.is_live);
    assert!(r.item.live_title.is_empty());
    assert_eq!(r.item.viewer_count, 0);
    assert_eq!(r.item.category_name, None);
}

// MARK: 알림 감시 판정

/// 채널별 설정 하나("set")만 있는 조회 — 없는 채널은 둘 다 꺼짐.
struct OneChannel {
    category: bool,
    title: bool,
}

impl ChannelNotificationLookup for OneChannel {
    fn is_title_change_notification_enabled(&self, id: &str) -> bool {
        id == "set" && self.title
    }
    fn is_category_change_notification_enabled(&self, id: &str) -> bool {
        id == "set" && self.category
    }
}

fn sink_would_notify(id: &str, g: &NotifyWatchGeneral, c: &OneChannel) -> bool {
    if !g.auto_refresh_enabled || !g.notifications_enabled {
        return false;
    }
    let category = g.notify_on_category_change && c.is_category_change_notification_enabled(id);
    let title = g.notify_on_title_change && c.is_title_change_notification_enabled(id);
    category || title
}

#[test]
fn notify_watch_matches_sink_filter() {
    for mask in 0..64u32 {
        let g = NotifyWatchGeneral {
            notifications_enabled: mask & 1 != 0,
            notify_on_category_change: mask & 2 != 0,
            notify_on_title_change: mask & 4 != 0,
            auto_refresh_enabled: mask & 8 != 0,
        };
        let c = OneChannel {
            category: mask & 16 != 0,
            title: mask & 32 != 0,
        };
        for id in ["set", "absent"] {
            assert_eq!(
                notify_watches(id, &g, &c),
                sink_would_notify(id, &g, &c),
                "mask={mask} id={id}"
            );
        }
    }
}

#[test]
fn default_settings_watch_nothing() {
    assert!(!notify_watches(
        "a",
        &NotifyWatchGeneral::default(),
        &OneChannel {
            category: true,
            title: true
        }
    ));
}
