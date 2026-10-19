//! 이미지 디스크 캐시 인덱스(파일 바이트는 디스크, 메타만 SQLite).
//!
//! 원본: `Sources/CViewPersistence/DataStore+ImageCache.swift`.

use super::DataStore;
use crate::db::{self, dv};
use crate::error::Result;
use crate::models::{CachedImageEntry, CachedImageEntryData, ImageTouch};
use crate::time::Date;
use rusqlite::types::Value;
use std::collections::{HashMap, HashSet};

/// IN 절 청크 크기(SQLite 파라미터 한계 회피).
pub const IMAGE_INDEX_CHUNK_SIZE: usize = 200;

/// 디스크 → 인덱스 백필 완료 표식 키.
pub const IMAGE_INDEX_BACKFILL_DONE_KEY: &str = "imageCache.indexBackfill.v2";

fn new_row(data: &CachedImageEntryData) -> CachedImageEntry {
    CachedImageEntry {
        id: 0,
        url_hash: data.url_hash.clone(),
        url_string: data.url_string.clone(),
        relative_path: data.relative_path.clone(),
        etag: data.etag.clone(),
        last_modified: data.last_modified.clone(),
        mime_type: data.mime_type.clone(),
        size_bytes: data.size_bytes,
        fetched_at: data.fetched_at,
        accessed_at: data.accessed_at,
        access_count: data.access_count,
        expires_at: data.expires_at,
        source: data.source.clone(),
    }
}

impl DataStore {
    /// urlHash 로 캐시 항목 조회.
    pub fn fetch_image_entry(&self, url_hash: &str) -> Result<Option<CachedImageEntryData>> {
        self.read(|conn| {
            Ok(db::fetch_one::<CachedImageEntry>(
                conn,
                "WHERE urlHash = ?",
                &[Value::Text(url_hash.into())],
            )?
            .as_ref()
            .map(CachedImageEntryData::from))
        })
    }

    /// 캐시 적재/갱신(단건).
    pub fn upsert_image_entry(&self, data: &CachedImageEntryData) -> Result<()> {
        self.upsert_image_entries(std::slice::from_ref(data))
    }

    /// N개 메타를 영향 행만 청크 조회한 뒤 한 번에 반영한다. 같은 해시가 여러 번 오면 마지막 값.
    /// 메타가 그대로면 쓰지 않는다. 누적 LRU 통계(accessCount·accessedAt)는 역행시키지 않는다.
    pub fn upsert_image_entries(&self, entries: &[CachedImageEntryData]) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let mut latest: HashMap<&str, &CachedImageEntryData> = HashMap::new();
        for entry in entries {
            latest.insert(entry.url_hash.as_str(), entry);
        }
        self.write(|tx| {
            let keys: Vec<Value> = latest.keys().map(|k| Value::Text((*k).into())).collect();
            let mut existing: HashMap<String, CachedImageEntry> =
                db::fetch_in::<CachedImageEntry>(tx, "urlHash", &keys, IMAGE_INDEX_CHUNK_SIZE)?
                    .into_iter()
                    .map(|r| (r.url_hash.clone(), r))
                    .collect();
            let mut dirty = false;
            for data in latest.values() {
                if let Some(row) = existing.get_mut(&data.url_hash) {
                    let unchanged = row.url_string == data.url_string
                        && row.relative_path == data.relative_path
                        && row.etag == data.etag
                        && row.last_modified == data.last_modified
                        && row.mime_type == data.mime_type
                        && row.size_bytes == data.size_bytes
                        && row.expires_at == data.expires_at
                        && row.source == data.source;
                    if unchanged {
                        continue;
                    }
                    row.url_string = data.url_string.clone();
                    row.relative_path = data.relative_path.clone();
                    row.etag = data.etag.clone();
                    row.last_modified = data.last_modified.clone();
                    row.mime_type = data.mime_type.clone();
                    row.size_bytes = data.size_bytes;
                    row.fetched_at = data.fetched_at;
                    row.access_count = row.access_count.max(data.access_count);
                    row.accessed_at = row.accessed_at.max(data.accessed_at);
                    row.expires_at = data.expires_at;
                    row.source = data.source.clone();
                    db::update(tx, row)?;
                } else {
                    db::insert(tx, &mut new_row(data))?;
                }
                dirty = true;
            }
            Ok(((), dirty))
        })
    }

    /// 접근 시각 + 카운트 갱신(cache hit 핫패스).
    pub fn touch_image_entry(&self, url_hash: &str, at: Date) -> Result<()> {
        self.write(|tx| {
            let Some(mut row) = db::fetch_one::<CachedImageEntry>(
                tx,
                "WHERE urlHash = ?",
                &[Value::Text(url_hash.into())],
            )?
            else {
                return Ok(((), false));
            };
            row.accessed_at = at;
            row.access_count = row.access_count.wrapping_add(1);
            db::update(tx, &row)?;
            Ok(((), true))
        })
    }

    /// 접근 통계 일괄 갱신 — 영향 행만 IN 조회, 한 번에 커밋.
    pub fn touch_image_entries(&self, batch: &HashMap<String, ImageTouch>) -> Result<()> {
        if batch.is_empty() {
            return Ok(());
        }
        self.write(|tx| {
            let keys: Vec<Value> = batch.keys().map(|k| Value::Text(k.clone())).collect();
            let mut dirty = false;
            for mut row in
                db::fetch_in::<CachedImageEntry>(tx, "urlHash", &keys, IMAGE_INDEX_CHUNK_SIZE)?
            {
                let Some(touch) = batch.get(&row.url_hash) else {
                    continue;
                };
                row.accessed_at = touch.accessed_at;
                row.access_count = row.access_count.wrapping_add(touch.count);
                db::update(tx, &row)?;
                dirty = true;
            }
            Ok(((), dirty))
        })
    }

    /// 캐시 항목 삭제(파일 삭제는 호출자 책임).
    pub fn delete_image_entry(&self, url_hash: &str) -> Result<()> {
        self.write(|tx| {
            tx.execute("DELETE FROM CachedImageEntry WHERE urlHash = ?", [url_hash])?;
            Ok(((), true))
        })
    }

    /// N건 배치 삭제 — 트랜잭션 1회.
    pub fn delete_image_entries(&self, url_hashes: &[String]) -> Result<()> {
        if url_hashes.is_empty() {
            return Ok(());
        }
        self.write(|tx| {
            for chunk in url_hashes.chunks(IMAGE_INDEX_CHUNK_SIZE) {
                tx.execute(
                    &format!(
                        "DELETE FROM CachedImageEntry WHERE urlHash IN ({})",
                        db::placeholders(chunk.len())
                    ),
                    rusqlite::params_from_iter(chunk.iter()),
                )?;
            }
            Ok(((), true))
        })
    }

    /// 전체 인덱스 비우기.
    pub fn delete_all_image_entries(&self) -> Result<()> {
        self.write(|tx| {
            tx.execute("DELETE FROM CachedImageEntry", [])?;
            Ok(((), true))
        })
    }

    /// 만료됐거나 `stale_before` 보다 오래 전에 받은 행.
    pub fn fetch_expired_image_entries(
        &self,
        now: Date,
        stale_before: Date,
    ) -> Result<Vec<CachedImageEntryData>> {
        self.read(|conn| {
            let mut by_hash: HashMap<String, CachedImageEntryData> = HashMap::new();
            for row in
                db::fetch::<CachedImageEntry>(conn, "WHERE fetchedAt < ?", &[dv(&stale_before)])?
            {
                by_hash.insert(row.url_hash.clone(), CachedImageEntryData::from(&row));
            }
            for row in db::fetch::<CachedImageEntry>(
                conn,
                "WHERE expiresAt IS NOT NULL AND expiresAt < ?",
                &[dv(&now)],
            )? {
                by_hash.insert(row.url_hash.clone(), CachedImageEntryData::from(&row));
            }
            Ok(by_hash.into_values().collect())
        })
    }

    /// `fresh_since` 이후이고 만료 시각이 남은 행.
    pub fn fetch_live_image_entries(
        &self,
        now: Date,
        fresh_since: Date,
    ) -> Result<Vec<CachedImageEntryData>> {
        self.read(|conn| {
            Ok(
                db::fetch::<CachedImageEntry>(conn, "WHERE fetchedAt >= ?", &[dv(&fresh_since)])?
                    .iter()
                    .filter(|r| r.expires_at.is_none_or(|e| e >= now))
                    .map(CachedImageEntryData::from)
                    .collect(),
            )
        })
    }

    /// 모든 캐시 항목.
    pub fn all_image_entries(&self) -> Result<Vec<CachedImageEntryData>> {
        self.read(|conn| {
            Ok(db::fetch::<CachedImageEntry>(conn, "", &[])?
                .iter()
                .map(CachedImageEntryData::from)
                .collect())
        })
    }

    /// 백필 완료 표식이 있는가. 읽기 실패는 false.
    pub fn is_image_index_backfill_done(&self) -> bool {
        matches!(
            self.load_setting::<bool>(IMAGE_INDEX_BACKFILL_DONE_KEY),
            Ok(Some(true))
        )
    }

    /// 백필 완료 표식을 남긴다(같은 값이면 쓰지 않는다).
    pub fn mark_image_index_backfill_done(&self) -> Result<()> {
        self.save_setting(IMAGE_INDEX_BACKFILL_DONE_KEY, &true)
    }

    /// 백필 완료 표식을 지운다.
    pub fn clear_image_index_backfill_done(&self) -> Result<()> {
        let keys: HashSet<String> = [IMAGE_INDEX_BACKFILL_DONE_KEY.to_string()].into();
        self.delete_settings(&keys)
    }
}
