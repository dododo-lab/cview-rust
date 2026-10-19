//! 클립 메타 캐시(contentHash delta 감지 · 묶음 순위 시각).
//!
//! 원본: `Sources/CViewPersistence/DataStore+ClipCache.swift`.

use super::DataStore;
use crate::db::{self, dv, join_sources, split_sources};
use crate::error::Result;
use crate::models::{CachedClip, CachedClipData};
use crate::time::{adding, Date, TimeInterval};
use rusqlite::types::Value;
use std::collections::{HashMap, HashSet};

/// 묶음 안 순위 하나당 `fetchedAt` 을 당기는 폭(초).
pub const CLIP_BATCH_RANK_STEP: TimeInterval = 0.001;
const CLIP_CACHE_KEY_CHUNK_SIZE: usize = 200;

fn merged_sources(item: &CachedClipData, existing: Option<&CachedClip>) -> String {
    let mut set = item.sources.clone();
    if let Some(row) = existing {
        set.extend(split_sources(&row.sources));
    }
    join_sources(&set)
}

/// 내용이 바뀐 기존 행에 새 값을 옮긴다. 비어 있는 선택 값으로 기존 값을 지우지 않는다.
/// `fetchedAt` 은 여기서 정하지 않는다(묶음 순위로 새긴다).
fn apply(row: &mut CachedClip, item: &CachedClipData, sources: String) {
    row.clip_title = item.clip_title.clone();
    if item.thumbnail_image_url.is_some() {
        row.thumbnail_image_url = item.thumbnail_image_url.clone();
    }
    if item.clip_url.is_some() {
        row.clip_url = item.clip_url.clone();
    }
    row.duration = item.duration;
    row.read_count = item.read_count;
    if item.created_date.is_some() {
        row.created_date = item.created_date;
    }
    if item.channel_id.is_some() {
        row.channel_id = item.channel_id.clone();
    }
    if item.channel_name.is_some() {
        row.channel_name = item.channel_name.clone();
    }
    if item.channel_image_url.is_some() {
        row.channel_image_url = item.channel_image_url.clone();
    }
    row.content_hash = item.content_hash.clone();
    row.sources = sources;
}

/// CSV 정확 태그 일치.
fn has_tag(sources: &str, tag: &str) -> bool {
    sources.split(',').any(|t| t == tag)
}

impl DataStore {
    /// batch upsert. 반환값은 실제로 변경/추가된 row 수.
    ///
    /// 바뀐 행이 0 이면 커밋하지 않는다(직전 묶음 시각과 순서 유지). 바뀐 행이 있으면 이번 묶음의
    /// **모든** 행 `fetchedAt` 을 묶음 시각(항목 시각 최댓값)에서 순위마다 1ms 씩 뺀 값으로 다시 쓴다.
    /// 같은 클립이 두 번 오면 앞 순위만 센다.
    pub fn upsert_cached_clips(&self, items: &[CachedClipData]) -> Result<usize> {
        if items.is_empty() {
            return Ok(0);
        }
        self.write(|tx| {
            let keys: HashSet<&str> = items
                .iter()
                .map(|i| i.clip_uid.as_str())
                .filter(|k| !k.is_empty())
                .collect();
            let key_values: Vec<Value> = keys.iter().map(|k| Value::Text((*k).into())).collect();
            let mut by_uid: HashMap<String, CachedClip> =
                db::fetch_in::<CachedClip>(tx, "clipUID", &key_values, CLIP_CACHE_KEY_CHUNK_SIZE)?
                    .into_iter()
                    .map(|r| (r.clip_uid.clone(), r))
                    .collect();
            // 묶음 순위(항목 순서)대로 모은 clipUID.
            let mut batch_order: Vec<String> = Vec::with_capacity(items.len());
            let mut dirty: HashSet<String> = HashSet::new();
            let mut changed = 0usize;
            for item in items {
                let cid = &item.clip_uid;
                if cid.is_empty() {
                    continue;
                }
                let merged = merged_sources(item, by_uid.get(cid));
                if let Some(row) = by_uid.get_mut(cid) {
                    batch_order.push(cid.clone());
                    if row.content_hash == item.content_hash && row.sources == merged {
                        continue;
                    }
                    apply(row, item, merged);
                    dirty.insert(cid.clone());
                    changed += 1;
                } else {
                    let mut row = CachedClip {
                        id: 0,
                        clip_uid: item.clip_uid.clone(),
                        clip_title: item.clip_title.clone(),
                        thumbnail_image_url: item.thumbnail_image_url.clone(),
                        clip_url: item.clip_url.clone(),
                        duration: item.duration,
                        read_count: item.read_count,
                        created_date: item.created_date,
                        channel_id: item.channel_id.clone(),
                        channel_name: item.channel_name.clone(),
                        channel_image_url: item.channel_image_url.clone(),
                        content_hash: item.content_hash.clone(),
                        sources: join_sources(&item.sources),
                        fetched_at: item.fetched_at,
                    };
                    db::insert(tx, &mut row)?;
                    by_uid.insert(cid.clone(), row);
                    batch_order.push(cid.clone());
                    changed += 1;
                }
            }
            if changed == 0 {
                return Ok((0, false));
            }
            if let Some(batch_stamp) = items.iter().map(|i| i.fetched_at).max() {
                let mut seen: HashSet<&str> = HashSet::new();
                let mut rank = 0i64;
                for cid in &batch_order {
                    if !seen.insert(cid.as_str()) {
                        continue;
                    }
                    let stamp: Date = adding(&batch_stamp, -(rank as f64) * CLIP_BATCH_RANK_STEP);
                    if let Some(row) = by_uid.get_mut(cid) {
                        if row.fetched_at != stamp {
                            row.fetched_at = stamp;
                            dirty.insert(cid.clone());
                        }
                    }
                    rank += 1;
                }
            }
            for cid in &dirty {
                if let Some(row) = by_uid.get(cid) {
                    db::update(tx, row)?;
                }
            }
            Ok((changed, true))
        })
    }

    /// 이번 묶음에 없는 행에서 `source` 태그만 뗀다. 태그가 없으면 행을 지운다. 빈 묶음은 무시.
    pub fn retain_cached_clip_source(
        &self,
        source: &str,
        clip_uids: &HashSet<String>,
    ) -> Result<usize> {
        if source.is_empty() || clip_uids.is_empty() {
            return Ok(0);
        }
        self.write(|tx| {
            let rows = db::fetch::<CachedClip>(
                tx,
                "WHERE instr(sources, ?) > 0",
                &[Value::Text(source.into())],
            )?;
            let mut changed = 0usize;
            for mut row in rows {
                if clip_uids.contains(&row.clip_uid) {
                    continue;
                }
                let mut sources = split_sources(&row.sources);
                if !sources.remove(source) {
                    continue;
                }
                if sources.is_empty() {
                    db::delete(tx, &row)?;
                } else {
                    row.sources = join_sources(&sources);
                    db::update(tx, &row)?;
                }
                changed += 1;
            }
            Ok((changed, changed > 0))
        })
    }

    /// source 태그(CSV 정확 일치)가 붙은 클립, `fetchedAt` 최신순 limit 개.
    ///
    /// 부분 문자열 조건을 SQL 로 내리고(정확 일치의 필요조건), 창(`max(limit*4, 200)`)이 찼는데
    /// limit 을 못 채우면 제한 없이 한 번 더 읽는다.
    pub fn fetch_cached_clips(&self, source: &str, limit: usize) -> Result<Vec<CachedClipData>> {
        if limit == 0 || source.is_empty() {
            return Ok(Vec::new());
        }
        let window = (limit * 4).max(200);
        self.read(|conn| {
            let base = "WHERE instr(sources, ?) > 0 ORDER BY fetchedAt DESC, id ASC";
            let mut rows = db::fetch::<CachedClip>(
                conn,
                &format!("{base} LIMIT ?"),
                &[Value::Text(source.into()), Value::Integer(window as i64)],
            )?;
            if rows.len() == window
                && rows
                    .iter()
                    .filter(|r| has_tag(&r.sources, source))
                    .take(limit)
                    .count()
                    < limit
            {
                rows = db::fetch::<CachedClip>(conn, base, &[Value::Text(source.into())])?;
            }
            Ok(rows
                .iter()
                .filter(|r| has_tag(&r.sources, source))
                .take(limit)
                .map(CachedClipData::from)
                .collect())
        })
    }

    /// `fetchedAt < cutoff` 인 행에서 `source` 태그를 뗀다. 바꾼 row 수.
    pub fn prune_cached_clips(&self, source: &str, cutoff: Date) -> Result<usize> {
        self.write(|tx| {
            let stale = db::fetch::<CachedClip>(tx, "WHERE fetchedAt < ?", &[dv(&cutoff)])?;
            let mut changed = 0usize;
            for mut row in stale {
                if !row.sources.contains(source) {
                    continue;
                }
                let mut sources = split_sources(&row.sources);
                if !sources.remove(source) {
                    continue;
                }
                if sources.is_empty() {
                    db::delete(tx, &row)?;
                } else {
                    row.sources = join_sources(&sources);
                    db::update(tx, &row)?;
                }
                changed += 1;
            }
            Ok((changed, changed > 0))
        })
    }

    /// 테스트·진단용: 클립 원시 행.
    #[doc(hidden)]
    pub fn cached_clip_row(&self, clip_uid: &str) -> Result<Option<CachedClip>> {
        self.read(|conn| {
            Ok(db::fetch_one::<CachedClip>(
                conn,
                "WHERE clipUID = ?",
                &[Value::Text(clip_uid.into())],
            )?)
        })
    }

    /// 테스트·시드용: 클립 행을 그대로 넣는다.
    #[doc(hidden)]
    pub fn insert_cached_clip_rows(&self, rows: &[CachedClip]) -> Result<()> {
        self.write(|tx| {
            for row in rows {
                let mut row = row.clone();
                db::insert(tx, &mut row)?;
            }
            Ok(((), !rows.is_empty()))
        })
    }
}
