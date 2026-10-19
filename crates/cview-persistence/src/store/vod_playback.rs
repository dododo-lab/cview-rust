//! VOD 재생 진행률(이어 보기 보관함).
//!
//! 원본: `Sources/CViewPersistence/DataStore+VODPlayback.swift`.

use super::DataStore;
use crate::db::{self, dv};
use crate::error::Result;
use crate::models::{PersistedVodPlayback, PersistedVodPlaybackData};
use crate::time::{adding, now, TimeInterval};
use rusqlite::types::Value;

/// VOD 진행률 기본 보존 기간 — 60일.
pub const VOD_PLAYBACK_RETENTION: TimeInterval = 60.0 * 24.0 * 3600.0;

impl DataStore {
    /// VOD 재생 진행률 기록 — 같은 videoNo 가 있으면 update, 없으면 insert. `lastSeenAt = now`.
    pub fn record_vod_playback(
        &self,
        video_no: i64,
        last_position_sec: f64,
        duration_sec: f64,
        video_title: &str,
        channel_name: &str,
        thumbnail_url_string: Option<&str>,
    ) -> Result<()> {
        self.write(|tx| {
            let existing = db::fetch_one::<PersistedVodPlayback>(
                tx,
                "WHERE videoNo = ?",
                &[Value::Integer(video_no)],
            )?;
            let mut row = existing.unwrap_or(PersistedVodPlayback {
                id: 0,
                video_no,
                last_position_sec,
                duration_sec,
                video_title: video_title.into(),
                channel_name: channel_name.into(),
                thumbnail_url_string: None,
                last_seen_at: now(),
            });
            row.last_position_sec = last_position_sec;
            row.duration_sec = duration_sec;
            row.video_title = video_title.into();
            row.channel_name = channel_name.into();
            row.thumbnail_url_string = thumbnail_url_string.map(str::to_string);
            row.last_seen_at = now();
            if row.id == 0 {
                db::insert(tx, &mut row)?;
            } else {
                db::update(tx, &row)?;
            }
            Ok(((), true))
        })
    }

    /// 특정 VOD 의 저장된 재생 위치.
    pub fn fetch_vod_playback(&self, video_no: i64) -> Result<Option<PersistedVodPlaybackData>> {
        self.read(|conn| {
            Ok(db::fetch_one::<PersistedVodPlayback>(
                conn,
                "WHERE videoNo = ?",
                &[Value::Integer(video_no)],
            )?
            .as_ref()
            .map(PersistedVodPlaybackData::from))
        })
    }

    /// "이어 보기" 후보 — 30초 초과 시청 & progress < 95%, 최근 본 순서.
    /// 페이지(`max(limit*3, 60)`)를 limit 개를 채울 때까지 읽는다(동률은 videoNo 내림차순).
    pub fn fetch_recent_vod_playback(&self, limit: usize) -> Result<Vec<PersistedVodPlaybackData>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let page_size = (limit * 3).max(60);
        self.read(|conn| {
            let mut collected = Vec::new();
            let mut offset = 0usize;
            'outer: while collected.len() < limit {
                let page = db::fetch::<PersistedVodPlayback>(
                    conn,
                    "ORDER BY lastSeenAt DESC, videoNo DESC LIMIT ? OFFSET ?",
                    &[
                        Value::Integer(page_size as i64),
                        Value::Integer(offset as i64),
                    ],
                )?;
                for row in &page {
                    if row.last_position_sec > 30.0 && row.progress() < 0.95 {
                        collected.push(PersistedVodPlaybackData::from(row));
                        if collected.len() >= limit {
                            break 'outer;
                        }
                    }
                }
                if page.len() < page_size {
                    break;
                }
                offset += page_size;
            }
            Ok(collected)
        })
    }

    /// 이어 보기에서 제거.
    pub fn remove_vod_playback(&self, video_no: i64) -> Result<()> {
        self.write(|tx| {
            let n = tx.execute(
                "DELETE FROM PersistedVODPlayback WHERE videoNo = ?",
                [video_no],
            )?;
            Ok(((), n > 0))
        })
    }

    /// 보존 기간 초과 row 제거(기본 60일). 지운 수.
    pub fn prune_vod_playback(&self, retention: TimeInterval) -> Result<i64> {
        let cutoff = adding(&now(), -retention);
        self.write(|tx| {
            let count =
                db::count::<PersistedVodPlayback>(tx, "WHERE lastSeenAt < ?", &[dv(&cutoff)])?;
            if count == 0 {
                return Ok((0, false));
            }
            tx.execute(
                "DELETE FROM PersistedVODPlayback WHERE lastSeenAt < ?",
                [crate::time::to_db(&cutoff)],
            )?;
            Ok((count, true))
        })
    }

    /// VOD 이어 보기 기록 전부 삭제.
    pub fn delete_all_vod_playback(&self) -> Result<()> {
        self.write(|tx| {
            let n = tx.execute("DELETE FROM PersistedVODPlayback", [])?;
            Ok(((), n > 0))
        })
    }

    /// 테스트·시드용: 진행률 행을 그대로 넣는다.
    #[doc(hidden)]
    pub fn insert_vod_playback_rows(&self, rows: &[PersistedVodPlayback]) -> Result<()> {
        self.write(|tx| {
            for row in rows {
                let mut row = row.clone();
                db::insert(tx, &mut row)?;
            }
            Ok(((), !rows.is_empty()))
        })
    }
}
