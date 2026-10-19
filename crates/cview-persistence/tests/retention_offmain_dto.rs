//! 원본: `Tests/CViewPersistenceTests/DataStoreRetentionBatchDeleteTests.swift`,
//! `DataStoreOffMainThreadTests.swift`, `DTOTests.swift`, `Audit0919corepersistWatchAvatarTests.swift`.
//!
//! 옮기지 않은 것: 메인 스레드 판정 테스트 3건(Rust 에는 MainActor 가 없다 — `off_main` 은
//! `spawn_blocking` 으로 실행기 스레드를 비우는 것만 계약한다), Audit0919 의 PlaybackSpeed·
//! AutocompleteSuggestion 테스트(CViewCore 소유 — 이 크레이트 범위 밖).

mod common;

use common::*;
use cview_core::models::ChannelInfo;
use cview_persistence::models::{ChannelListData, PersistedVodPlayback, WatchHistoryData};
use cview_persistence::store::clip_playback::CLIP_PLAYBACK_RETENTION;
use cview_persistence::store::vod_playback::VOD_PLAYBACK_RETENTION;
use cview_persistence::time::to_db;
use cview_persistence::PersistenceError;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

// MARK: - 보존 정리

#[test]
fn playback_prune_deletes_only_expired_rows() {
    let s = store();
    let old = add(&now(), -90.0 * 86_400.0);
    let vod = |no: i64, title: &str, seen| PersistedVodPlayback {
        id: 0,
        video_no: no,
        last_position_sec: 60.0,
        duration_sec: 600.0,
        video_title: title.into(),
        channel_name: "c".into(),
        thumbnail_url_string: None,
        last_seen_at: seen,
    };
    s.insert_vod_playback_rows(&[vod(1, "old", old), vod(2, "new", now())])
        .unwrap();
    s.record_clip_playback("old", 5.0, 30.0, "old", "c", None)
        .unwrap();
    s.record_clip_playback("new", 5.0, 30.0, "new", "c", None)
        .unwrap();
    s.with_connection(|c| {
        c.execute(
            "UPDATE PersistedClipPlayback SET lastSeenAt = ? WHERE clipUID = 'old'",
            [to_db(&old)],
        )?;
        Ok(())
    })
    .unwrap();

    assert_eq!(s.prune_vod_playback(VOD_PLAYBACK_RETENTION).unwrap(), 1);
    assert_eq!(s.prune_clip_playback(CLIP_PLAYBACK_RETENTION).unwrap(), 1);
    assert!(s.fetch_vod_playback(2).unwrap().is_some());
    assert!(s.fetch_vod_playback(1).unwrap().is_none());
    let clips: Vec<_> = s
        .fetch_recent_clip_playback(10)
        .unwrap()
        .into_iter()
        .map(|c| c.clip_uid)
        .collect();
    assert_eq!(clips, vec!["new"]);
    assert_eq!(s.prune_vod_playback(VOD_PLAYBACK_RETENTION).unwrap(), 0);
}

#[test]
fn watch_history_cleanup_keeps_recent() {
    let s = store();
    s.start_watch_record("old", "옛", None, None, add(&now(), -400.0 * 86_400.0))
        .unwrap();
    s.start_watch_record("new", "새", None, None, add(&now(), -60.0))
        .unwrap();
    s.cleanup_old_watch_history(365).unwrap();
    let left: Vec<_> = s
        .fetch_watch_history(100)
        .unwrap()
        .into_iter()
        .map(|r| r.channel_id)
        .collect();
    assert_eq!(left, vec!["new"]);
    s.cleanup_old_watch_history(365).unwrap();
}

// MARK: - off_main

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_writes_remain_consistent() {
    let s = store();
    let mut handles = Vec::new();
    for i in 0..120 {
        let s = s.clone();
        handles.push(tokio::spawn(async move {
            s.off_main(move |ds| {
                ds.save_channel(&ChannelInfo::new(format!("c{i}"), format!("채널{i}")), true)
            })
            .await
        }));
    }
    for h in handles {
        h.await.unwrap().unwrap();
    }
    let favs = s.fetch_favorite_items().unwrap();
    assert_eq!(favs.len(), 120);
    assert_eq!(
        favs.iter()
            .map(|f| f.channel_id.clone())
            .collect::<HashSet<_>>()
            .len(),
        120
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mixed_direct_and_off_main_callers_do_not_deadlock() {
    let s = store();
    let mut handles = Vec::new();
    for i in 0..40 {
        let a = s.clone();
        handles.push(tokio::spawn(async move {
            a.off_main(move |ds| ds.save_channel(&ChannelInfo::new(format!("m{i}"), "메인"), true))
                .await
                .unwrap();
        }));
        let b = s.clone();
        handles.push(tokio::spawn(async move {
            b.save_channel(&ChannelInfo::new(format!("b{i}"), "백그라운드"), true)
                .unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(s.fetch_favorite_items().unwrap().len(), 80);
}

/// 큐에서 기다리는 동안 취소되면 본문을 실행하지 않는다.
#[test]
fn off_main_skips_body_when_cancelled_before_start() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .max_blocking_threads(1)
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let s = store();
        let (release, wait) = std::sync::mpsc::channel::<()>();
        // 블로킹 풀(1개)을 점유해 off_main 본문을 큐에 묶어 둔다.
        let blocker = tokio::task::spawn_blocking(move || {
            let _ = wait.recv();
        });
        let ran = Arc::new(AtomicBool::new(false));
        let (s2, ran2) = (s.clone(), ran.clone());
        let job = tokio::spawn(async move {
            s2.off_main(move |ds| {
                ran2.store(true, Ordering::SeqCst);
                ds.save_setting("m15.skip", &1)
            })
            .await
        });
        tokio::time::sleep(Duration::from_millis(30)).await;
        job.abort();
        assert!(job.await.unwrap_err().is_cancelled());
        release.send(()).unwrap();
        blocker.await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            !ran.load(Ordering::SeqCst),
            "취소된 호출자의 본문이 실행됐다"
        );
        assert_eq!(s.load_setting::<i64>("m15.skip").unwrap(), None);
    });
}

/// 본문이 시작된 뒤 취소돼도 본문은 끝까지 간다(분할 쓰기 금지).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn off_main_body_is_all_or_nothing_under_mid_body_cancel() {
    let s = store();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
    let s2 = s.clone();
    let job = tokio::spawn(async move {
        s2.off_main(move |ds| {
            ds.save_setting("m15.a", &1)?;
            let _ = started_tx.send(());
            std::thread::sleep(Duration::from_millis(50));
            ds.save_setting("m15.b", &2)
        })
        .await
    });
    started_rx.await.unwrap();
    job.abort();
    let _ = job.await;
    let mut b = None;
    for _ in 0..100 {
        b = s.load_setting::<i64>("m15.b").unwrap();
        if b.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(s.load_setting::<i64>("m15.a").unwrap(), Some(1));
    assert_eq!(b, Some(2), "본문 중간 취소로 분할 쓰기가 발생했다");
}

#[tokio::test]
async fn off_main_without_cancellation_behaves_identically() {
    let s = store();
    let value = s
        .off_main(|ds| {
            ds.save_setting("m15.ok", &7)?;
            Ok(ds.load_setting::<i64>("m15.ok")?.unwrap_or(0))
        })
        .await
        .unwrap();
    assert_eq!(value, 7);
    let err = s
        .off_main(|_| -> cview_persistence::Result<()> {
            Err(PersistenceError::Other("42".into()))
        })
        .await
        .unwrap_err();
    assert!(matches!(err, PersistenceError::Other(ref m) if m == "42"));
    assert_eq!(s.off_main_uncancellable(|_| 5).await, 5);
}

// MARK: - DTO

fn history_data(started: cview_persistence::time::Date, duration: f64) -> WatchHistoryData {
    WatchHistoryData {
        channel_id: "ch1".into(),
        channel_name: "채널".into(),
        thumbnail_url: None,
        category_name: None,
        started_at: started,
        ended_at: None,
        duration,
    }
}

#[test]
fn watch_history_data_id_format() {
    assert_eq!(
        history_data(ts(1_700_000_000.0), 0.0).id(),
        "ch1-1700000000.0"
    );
}

#[test]
fn watch_history_data_formatted_duration() {
    assert_eq!(history_data(now(), 1500.0).formatted_duration(), "25분");
    assert_eq!(
        history_data(now(), 5400.0).formatted_duration(),
        "1시간 30분"
    );
    assert_eq!(history_data(now(), 0.0).formatted_duration(), "0초");
}

#[test]
fn watch_history_data_formatted_date() {
    let f = history_data(ts(1_700_058_600.0), 0.0).formatted_date();
    assert!(f.contains('/'));
    assert!(f.contains(':'));
}

#[test]
fn channel_list_data_id_is_channel_id() {
    let d = ChannelListData {
        channel_id: "abc123".into(),
        channel_name: "채널".into(),
        image_url: None,
        is_favorite: false,
        last_watched: None,
    };
    assert_eq!(d.id(), "abc123");
    assert!(d.image_url.is_none() && d.last_watched.is_none());
}

// MARK: - Audit0919 — 아바타 오염 방지

const LIVE_FRAME: &str = "https://live.example.com/ch1-720.jpg";

#[test]
fn start_watch_record_does_not_overwrite_avatar() {
    let s = store();
    let avatar = "https://img.example.com/avatar.png";
    let mut info = ChannelInfo::new("ch1", "채널");
    info.channel_image_url = url::Url::parse(avatar).ok();
    s.save_channel(&info, false).unwrap();
    s.start_watch_record("ch1", "채널", Some(LIVE_FRAME), None, now())
        .unwrap();
    let row = s.fetch_recent_items(20).unwrap().remove(0);
    assert_eq!(row.channel_id, "ch1");
    assert_eq!(row.image_url.as_deref(), Some(avatar));
}

#[test]
fn start_watch_record_does_not_seed_avatar_with_live_frame() {
    let s = store();
    s.start_watch_record("ch1", "새채널", Some(LIVE_FRAME), None, now())
        .unwrap();
    let row = s.fetch_recent_items(20).unwrap().remove(0);
    assert!(row.image_url.is_none());
}

#[test]
fn watch_history_keeps_live_frame_thumbnail() {
    let s = store();
    s.start_watch_record("ch1", "채널", Some(LIVE_FRAME), Some("게임"), now())
        .unwrap();
    let h = s.fetch_watch_history(100).unwrap().remove(0);
    assert_eq!(h.thumbnail_url.as_deref(), Some(LIVE_FRAME));
    assert_eq!(h.category_name.as_deref(), Some("게임"));
}
