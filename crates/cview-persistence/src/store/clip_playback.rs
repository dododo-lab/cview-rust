//! 클립 재생 진행률(이어 보기).
//!
//! 원본: `Sources/CViewPersistence/DataStore+ClipPlayback.swift`. 현재 소비자는 없지만 스키마 보존을 위해 남긴다.

use super::DataStore;
use crate::db::{self, dv};
use crate::error::Result;
use crate::models::{PersistedClipPlayback, PersistedClipPlaybackData};
use crate::time::{adding, now, TimeInterval};
use rusqlite::types::Value;

/// 클립 진행률 기본 보존 기간 — 30일.
pub const CLIP_PLAYBACK_RETENTION: TimeInterval = 30.0 * 24.0 * 3600.0;

impl DataStore {
    /// 클립 재생 진행률 기록 — 같은 clipUID 가 있으면 update, 없으면 insert. `lastSeenAt = now`.
    pub fn record_clip_playback(
        &self,
        clip_uid: &str,
        last_position_sec: f64,
        duration_sec: f64,
        clip_title: &str,
        channel_name: &str,
        thumbnail_url_string: Option<&str>,
    ) -> Result<()> {
        self.write(|tx| {
            let existing = db::fetch_one::<PersistedClipPlayback>(
                tx,
                "WHERE clipUID = ?",
                &[Value::Text(clip_uid.into())],
            )?;
            let mut row = existing.unwrap_or(PersistedClipPlayback {
                id: 0,
                clip_uid: clip_uid.into(),
                last_position_sec,
                duration_sec,
                clip_title: clip_title.into(),
                channel_name: channel_name.into(),
                thumbnail_url_string: None,
                last_seen_at: now(),
            });
            row.last_position_sec = last_position_sec;
            row.duration_sec = duration_sec;
            row.clip_title = clip_title.into();
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

    /// "이어 보기" 후보 — 5% < progress < 95%, 최근 본 순서. 창은 `limit * 3`.
    pub fn fetch_recent_clip_playback(
        &self,
        limit: usize,
    ) -> Result<Vec<PersistedClipPlaybackData>> {
        self.read(|conn| {
            let rows = db::fetch::<PersistedClipPlayback>(
                conn,
                "ORDER BY lastSeenAt DESC, id DESC LIMIT ?",
                &[Value::Integer((limit * 3) as i64)],
            )?;
            Ok(rows
                .iter()
                .filter(|r| {
                    let p = r.progress();
                    p > 0.05 && p < 0.95
                })
                .take(limit)
                .map(PersistedClipPlaybackData::from)
                .collect())
        })
    }

    /// 보존 기간 초과 row 제거(기본 30일). 지운 수.
    pub fn prune_clip_playback(&self, retention: TimeInterval) -> Result<i64> {
        let cutoff = adding(&now(), -retention);
        self.write(|tx| {
            let count =
                db::count::<PersistedClipPlayback>(tx, "WHERE lastSeenAt < ?", &[dv(&cutoff)])?;
            if count == 0 {
                return Ok((0, false));
            }
            tx.execute(
                "DELETE FROM PersistedClipPlayback WHERE lastSeenAt < ?",
                [crate::time::to_db(&cutoff)],
            )?;
            Ok((count, true))
        })
    }

    /// 클립 재생 기록 전부 삭제.
    pub fn delete_all_clip_playback(&self) -> Result<()> {
        self.write(|tx| {
            let n = tx.execute("DELETE FROM PersistedClipPlayback", [])?;
            Ok(((), n > 0))
        })
    }
}
