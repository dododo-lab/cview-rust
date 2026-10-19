//! 시청 기록 sync push 큐 — 미동기화 개수·조회·동기화 표시.
//!
//! 원본: `Sources/CViewPersistence/DataStore+WatchHistorySyncQueue.swift`.

use super::DataStore;
use crate::db::{self, dv};
use crate::error::Result;
use crate::models::{WatchHistory, WatchHistorySyncRow};
use crate::time::Date;
use rusqlite::types::Value;
use std::collections::{BTreeSet, HashMap};

/// 동기화 뒤 다시 바뀌었나(`syncedAt < effectiveUpdatedAt`).
fn is_stale_after_sync(row: &WatchHistory) -> bool {
    row.synced_at
        .is_some_and(|synced| synced < row.effective_updated_at())
}

/// 큐 순서 — (startedAt, channelId) 바이트 순서.
fn precedes(a: &WatchHistory, b: &WatchHistory) -> std::cmp::Ordering {
    a.started_at
        .cmp(&b.started_at)
        .then_with(|| a.channel_id.as_bytes().cmp(b.channel_id.as_bytes()))
}

impl DataStore {
    /// 미동기화 row 개수(`syncedAt == nil` 은 SQL COUNT, 나머지만 메모리 비교).
    pub fn unsynced_watch_history_count(&self) -> Result<i64> {
        self.read(|conn| {
            let never = db::count::<WatchHistory>(conn, "WHERE syncedAt IS NULL", &[])?;
            let stale = db::fetch::<WatchHistory>(conn, "WHERE syncedAt IS NOT NULL", &[])?
                .iter()
                .filter(|r| is_stale_after_sync(r))
                .count() as i64;
            Ok(never + stale)
        })
    }

    /// 동기화되지 않은(또는 갱신된) 시청 기록 중 가장 오래된 limit 개 — (startedAt, channelId) 오름차순.
    /// 두 갈래(한 번도 안 보냄 / 보낸 뒤 바뀜)는 서로소이고 어느 쪽도 스캔 상한이 없다.
    pub fn fetch_unsynced_watch_history(&self, limit: usize) -> Result<Vec<WatchHistorySyncRow>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        self.read(|conn| {
            // SQLite 기본 BINARY 비교 = 바이트 순서(Swift `.lexical`).
            let never_synced = db::fetch::<WatchHistory>(
                conn,
                "WHERE syncedAt IS NULL ORDER BY startedAt ASC, channelId ASC LIMIT ?",
                &[Value::Integer(limit as i64)],
            )?;
            let horizon = if never_synced.len() == limit {
                never_synced.last().map(|r| r.started_at)
            } else {
                None
            };
            let mut stale: Vec<WatchHistory> = match horizon {
                Some(h) => db::fetch::<WatchHistory>(
                    conn,
                    "WHERE syncedAt IS NOT NULL AND startedAt <= ?",
                    &[dv(&h)],
                )?,
                None => db::fetch::<WatchHistory>(conn, "WHERE syncedAt IS NOT NULL", &[])?,
            }
            .into_iter()
            .filter(is_stale_after_sync)
            .collect();
            stale.sort_by(precedes);
            stale.truncate(limit);

            let mut merged: Vec<WatchHistory> = never_synced.into_iter().chain(stale).collect();
            merged.sort_by(precedes);
            merged.truncate(limit);
            Ok(merged
                .into_iter()
                .map(|row| WatchHistorySyncRow {
                    id: WatchHistorySyncRow::make_id(&row.channel_id, &row.started_at),
                    updated_at: row.effective_updated_at(),
                    channel_id: row.channel_id,
                    channel_name: row.channel_name,
                    thumbnail_url: row.thumbnail_url,
                    category_name: row.category_name,
                    started_at: row.started_at,
                    ended_at: row.ended_at,
                    duration: row.duration,
                })
                .collect())
        })
    }

    /// push 성공한 row 들에 `syncedAt` 을 **보낸 판본**(`updatedAt`) 그대로 표시한다.
    /// 그 뒤 바뀐 행(`effectiveUpdatedAt > pushed`)은 다음 큐에 남긴다.
    pub fn mark_watch_history_synced(&self, versions: &HashMap<String, Date>) -> Result<()> {
        if versions.is_empty() {
            return Ok(());
        }
        let channel_ids: BTreeSet<String> = versions
            .keys()
            .filter_map(|id| id.rfind(':').map(|i| id[..i].to_string()))
            .collect();
        if channel_ids.is_empty() {
            return Ok(());
        }
        self.write(|tx| {
            let keys: Vec<Value> = channel_ids.into_iter().map(Value::Text).collect();
            let candidates = db::fetch_in::<WatchHistory>(tx, "channelId", &keys, 200)?;
            let mut changed = false;
            for mut row in candidates {
                let id = WatchHistorySyncRow::make_id(&row.channel_id, &row.started_at);
                let Some(pushed) = versions.get(&id) else {
                    continue;
                };
                if row.effective_updated_at() > *pushed {
                    continue;
                }
                row.synced_at = Some(*pushed);
                db::update(tx, &row)?;
                changed = true;
            }
            Ok(((), changed))
        })
    }
}
