//! 나중에 볼 VOD.
//!
//! 원본: `Sources/CViewPersistence/DataStore+VODWatchLater.swift`.

use super::DataStore;
use crate::db;
use crate::error::Result;
use crate::models::{PersistedVodWatchLater, PersistedVodWatchLaterData};
use crate::time::now;
use rusqlite::types::Value;

impl DataStore {
    /// 워치레이터에 추가 — 이미 있으면 담은 시각과 표시값을 갱신한다.
    pub fn add_vod_watch_later(
        &self,
        video_no: i64,
        video_title: &str,
        channel_name: &str,
        thumbnail_url_string: Option<&str>,
        duration_sec: f64,
    ) -> Result<()> {
        self.write(|tx| {
            let existing = db::fetch_one::<PersistedVodWatchLater>(
                tx,
                "WHERE videoNo = ?",
                &[Value::Integer(video_no)],
            )?;
            let mut row = existing.unwrap_or(PersistedVodWatchLater {
                id: 0,
                video_no,
                added_at: now(),
                video_title: String::new(),
                channel_name: String::new(),
                thumbnail_url_string: None,
                duration_sec: 0.0,
            });
            row.added_at = now();
            row.video_title = video_title.into();
            row.channel_name = channel_name.into();
            row.thumbnail_url_string = thumbnail_url_string.map(str::to_string);
            row.duration_sec = duration_sec;
            if row.id == 0 {
                db::insert(tx, &mut row)?;
            } else {
                db::update(tx, &row)?;
            }
            Ok(((), true))
        })
    }

    /// 워치레이터에서 제거.
    pub fn remove_vod_watch_later(&self, video_no: i64) -> Result<()> {
        self.write(|tx| {
            let n = tx.execute(
                "DELETE FROM PersistedVODWatchLater WHERE videoNo = ?",
                [video_no],
            )?;
            Ok(((), n > 0))
        })
    }

    /// 워치레이터 포함 여부.
    pub fn contains_vod_watch_later(&self, video_no: i64) -> Result<bool> {
        self.read(|conn| {
            Ok(db::count::<PersistedVodWatchLater>(
                conn,
                "WHERE videoNo = ?",
                &[Value::Integer(video_no)],
            )? > 0)
        })
    }

    /// 워치레이터 전체 — 최근 담은 순서.
    pub fn fetch_vod_watch_later(&self, limit: usize) -> Result<Vec<PersistedVodWatchLaterData>> {
        self.read(|conn| {
            Ok(db::fetch::<PersistedVodWatchLater>(
                conn,
                "ORDER BY addedAt DESC, id DESC LIMIT ?",
                &[Value::Integer(limit as i64)],
            )?
            .iter()
            .map(PersistedVodWatchLaterData::from)
            .collect())
        })
    }

    /// 워치레이터에 담긴 videoNo 목록.
    pub fn fetch_vod_watch_later_ids(&self) -> Result<Vec<i64>> {
        self.read(|conn| {
            Ok(db::fetch::<PersistedVodWatchLater>(conn, "", &[])?
                .into_iter()
                .map(|r| r.video_no)
                .collect())
        })
    }
}
