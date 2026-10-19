//! 치지직 라이브 메타 캐시(per-channel upsert · source 태그 · TTL prune).
//!
//! 원본: `Sources/CViewPersistence/DataStore+LiveChannelCache.swift`.

use super::DataStore;
use crate::db::{self, dv, join_sources, split_sources};
use crate::error::Result;
use crate::models::{CachedLiveChannel, CachedLiveChannelData};
use crate::time::Date;
use rusqlite::types::Value;
use rusqlite::Connection;
use std::collections::{BTreeSet, HashMap, HashSet};

/// 소형 배치 IN 조회 청크.
const LIVE_CHANNEL_ID_CHUNK_SIZE: usize = 200;
/// 이 크기 이하 배치는 channelId IN 조회, 넘으면 전 테이블.
const SMALL_BATCH_LIMIT: usize = 200;

/// 같은 channelId 가 중복되면 마지막 값을 채택한다.
fn deduplicated(items: &[CachedLiveChannelData]) -> Vec<CachedLiveChannelData> {
    let mut by_id: HashMap<&str, &CachedLiveChannelData> = HashMap::new();
    for item in items {
        by_id.insert(item.channel_id.as_str(), item);
    }
    by_id.into_values().cloned().collect()
}

fn make_row(item: &CachedLiveChannelData, sources: String) -> CachedLiveChannel {
    CachedLiveChannel {
        id: 0,
        channel_id: item.channel_id.clone(),
        channel_name: item.channel_name.clone(),
        channel_image_url: item.channel_image_url.clone(),
        live_title: item.live_title.clone(),
        viewer_count: item.viewer_count,
        category_name: item.category_name.clone(),
        category_type: item.category_type.clone(),
        thumbnail_url: item.thumbnail_url.clone(),
        is_live: item.is_live,
        open_date: item.open_date,
        sources,
        fetched_at: item.fetched_at,
    }
}

/// 실제 값이 바뀐 속성만 대입한다. Optional 입력이 None 이면 기존 값을 보존한다.
/// 바뀐 행에만 `fetchedAt` 을 함께 기록한다.
fn update_row(row: &mut CachedLiveChannel, item: &CachedLiveChannelData, merged: &str) -> bool {
    let mut changed = false;
    if row.channel_name != item.channel_name {
        row.channel_name = item.channel_name.clone();
        changed = true;
    }
    if let Some(v) = &item.channel_image_url {
        if row.channel_image_url.as_ref() != Some(v) {
            row.channel_image_url = Some(v.clone());
            changed = true;
        }
    }
    if row.live_title != item.live_title {
        row.live_title = item.live_title.clone();
        changed = true;
    }
    if row.viewer_count != item.viewer_count {
        row.viewer_count = item.viewer_count;
        changed = true;
    }
    if let Some(v) = &item.category_name {
        if row.category_name.as_ref() != Some(v) {
            row.category_name = Some(v.clone());
            changed = true;
        }
    }
    if let Some(v) = &item.category_type {
        if row.category_type.as_ref() != Some(v) {
            row.category_type = Some(v.clone());
            changed = true;
        }
    }
    if let Some(v) = &item.thumbnail_url {
        if row.thumbnail_url.as_ref() != Some(v) {
            row.thumbnail_url = Some(v.clone());
            changed = true;
        }
    }
    if row.is_live != item.is_live {
        row.is_live = item.is_live;
        changed = true;
    }
    if let Some(v) = item.open_date {
        if row.open_date != Some(v) {
            row.open_date = Some(v);
            changed = true;
        }
    }
    if row.sources != merged {
        row.sources = merged.to_string();
        changed = true;
    }
    if changed && row.fetched_at != item.fetched_at {
        row.fetched_at = item.fetched_at;
    }
    changed
}

fn fetch_by_ids(conn: &Connection, ids: &[String]) -> Result<Vec<CachedLiveChannel>> {
    let unique: BTreeSet<&String> = ids.iter().collect();
    let keys: Vec<Value> = unique.into_iter().map(|k| Value::Text(k.clone())).collect();
    Ok(db::fetch_in::<CachedLiveChannel>(
        conn,
        "channelId",
        &keys,
        LIVE_CHANNEL_ID_CHUNK_SIZE,
    )?)
}

impl DataStore {
    /// 라이브 채널 batch upsert. 콘텐츠/source 가 실제로 바뀐 행만 수정한다.
    /// - Returns: 삽입 또는 수정한 row 수. 0 이면 커밋하지 않는다.
    pub fn upsert_cached_live_channels(&self, items: &[CachedLiveChannelData]) -> Result<usize> {
        if items.is_empty() {
            return Ok(0);
        }
        self.write(|tx| {
            let existing = if items.len() <= SMALL_BATCH_LIMIT {
                let ids: Vec<String> = items.iter().map(|i| i.channel_id.clone()).collect();
                fetch_by_ids(tx, &ids)?
            } else {
                db::fetch::<CachedLiveChannel>(tx, "", &[])?
            };
            let mut by_id: HashMap<String, CachedLiveChannel> = existing
                .into_iter()
                .map(|r| (r.channel_id.clone(), r))
                .collect();
            let mut changed = 0usize;
            for item in deduplicated(items) {
                let mut sources = item.sources.clone();
                if let Some(row) = by_id.get(&item.channel_id) {
                    sources.extend(split_sources(&row.sources));
                }
                let merged = join_sources(&sources);
                if let Some(row) = by_id.get_mut(&item.channel_id) {
                    if update_row(row, &item, &merged) {
                        db::update(tx, row)?;
                        changed += 1;
                    }
                } else {
                    let mut row = make_row(&item, merged);
                    db::insert(tx, &mut row)?;
                    by_id.insert(item.channel_id.clone(), row);
                    changed += 1;
                }
            }
            Ok((changed, changed > 0))
        })
    }

    /// 한 source 의 **완전한 현재 snapshot** 을 원자적으로 교체한다. 입력에 없는 id 의 source 태그만 뗀다.
    /// - Returns: 삽입·실제 수정·태그 제거를 합친 변경 row 수.
    pub fn replace_cached_live_channels(
        &self,
        items: &[CachedLiveChannelData],
        source: &str,
    ) -> Result<usize> {
        if source.is_empty() {
            return Ok(0);
        }
        self.write(|tx| {
            let normalized = deduplicated(items);
            let incoming: HashSet<&str> =
                normalized.iter().map(|i| i.channel_id.as_str()).collect();
            let existing = db::fetch::<CachedLiveChannel>(tx, "", &[])?;
            let existing_ids: Vec<String> = existing.iter().map(|r| r.channel_id.clone()).collect();
            let mut by_id: HashMap<String, CachedLiveChannel> = existing
                .into_iter()
                .map(|r| (r.channel_id.clone(), r))
                .collect();
            let mut changed = 0usize;
            for item in &normalized {
                if let Some(row) = by_id.get_mut(&item.channel_id) {
                    let mut sources = split_sources(&row.sources);
                    sources.extend(item.sources.iter().cloned());
                    sources.insert(source.to_string());
                    if update_row(row, item, &join_sources(&sources)) {
                        db::update(tx, row)?;
                        changed += 1;
                    }
                } else {
                    let mut sources = item.sources.clone();
                    sources.insert(source.to_string());
                    let mut row = make_row(item, join_sources(&sources));
                    db::insert(tx, &mut row)?;
                    by_id.insert(item.channel_id.clone(), row);
                    changed += 1;
                }
            }
            for id in existing_ids
                .iter()
                .filter(|id| !incoming.contains(id.as_str()))
            {
                let Some(row) = by_id.get_mut(id) else {
                    continue;
                };
                let mut sources = split_sources(&row.sources);
                if !sources.remove(source) {
                    continue;
                }
                if sources.is_empty() {
                    db::delete(tx, row)?;
                } else {
                    row.sources = join_sources(&sources);
                    db::update(tx, row)?;
                }
                changed += 1;
            }
            Ok((changed, changed > 0))
        })
    }

    /// 여러 source 를 **테이블 1회 스캔**으로 조회해 source 별로 분배한다.
    /// 요청한 모든 source 키가 결과에 있다(행이 없으면 빈 배열). 각 배열은 `fetchedAt` 내림차순.
    pub fn fetch_cached_live_channels_by_sources(
        &self,
        sources: &[String],
    ) -> Result<HashMap<String, Vec<CachedLiveChannelData>>> {
        if sources.is_empty() {
            return Ok(HashMap::new());
        }
        let wanted: HashSet<&str> = sources.iter().map(String::as_str).collect();
        let mut out: HashMap<String, Vec<CachedLiveChannelData>> =
            wanted.iter().map(|k| (k.to_string(), Vec::new())).collect();
        self.read(|conn| {
            for row in db::fetch::<CachedLiveChannel>(conn, "ORDER BY fetchedAt DESC, id ASC", &[])?
            {
                let tags: Vec<&str> = row.sources.split(',').collect();
                if !tags.iter().any(|t| wanted.contains(t)) {
                    continue;
                }
                let dto = CachedLiveChannelData::from(&row);
                for tag in tags {
                    if let Some(list) = out.get_mut(tag) {
                        list.push(dto.clone());
                    }
                }
            }
            Ok(())
        })?;
        Ok(out)
    }

    /// 특정 source 의 캐시된 라이브 채널(`fetchedAt` 내림차순).
    pub fn fetch_cached_live_channels(&self, source: &str) -> Result<Vec<CachedLiveChannelData>> {
        Ok(self
            .fetch_cached_live_channels_by_sources(&[source.to_string()])?
            .remove(source)
            .unwrap_or_default())
    }

    /// 모든 캐시된 라이브 채널(시청자 수 내림차순).
    pub fn fetch_all_cached_live_channels(&self) -> Result<Vec<CachedLiveChannelData>> {
        self.read(|conn| {
            Ok(
                db::fetch::<CachedLiveChannel>(conn, "ORDER BY viewerCount DESC, id ASC", &[])?
                    .iter()
                    .map(CachedLiveChannelData::from)
                    .collect(),
            )
        })
    }

    /// `fetchedAt < cutoff` 인 행에서 `source` 태그를 뗀다. 태그가 비면 행 삭제.
    /// - Returns: 바꾼 row 수(0 이면 커밋 없음).
    pub fn prune_cached_live_channels(&self, source: &str, cutoff: Date) -> Result<usize> {
        self.write(|tx| {
            let stale = db::fetch::<CachedLiveChannel>(tx, "WHERE fetchedAt < ?", &[dv(&cutoff)])?;
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

    /// 전체 캐시된 라이브 채널 row 수.
    pub fn cached_live_channel_count(&self) -> Result<i64> {
        self.read(|conn| Ok(db::count::<CachedLiveChannel>(conn, "", &[])?))
    }

    /// 테스트·진단용: 원시 행.
    #[doc(hidden)]
    pub fn cached_live_channel_row(&self, channel_id: &str) -> Result<Option<CachedLiveChannel>> {
        self.read(|conn| {
            Ok(db::fetch_one::<CachedLiveChannel>(
                conn,
                "WHERE channelId = ?",
                &[Value::Text(channel_id.into())],
            )?)
        })
    }
}
