//! 쓰기 전용 경로를 지운 뒤 스토어에 남은 행을 한 번 지운다 + 레거시 홈 캐시 삭제.
//!
//! 원본: `Sources/CViewPersistence/DataStore+LegacyCleanup.swift`.

use super::DataStore;
use crate::db;
use crate::error::Result;
use crate::models::CachedClip;

/// 한 번 돈 뒤 다시 돌지 않게 하는 설정 키.
pub const LEGACY_WRITE_ONLY_CLEANUP_KEY: &str = "maintenance.legacyWriteOnlyCleanup.v1";
/// 홈 캐시 blob 이 쓰던 설정 키 접두사.
pub const LEGACY_HOME_CACHE_KEY_PREFIX: &str = "cache.";

/// 정리 결과(`LegacyCleanupResult`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LegacyCleanupResult {
    pub category_stat_rows: i64,
    pub channel_only_clip_rows: i64,
    pub retagged_clip_rows: i64,
}

impl DataStore {
    /// 표식이 없을 때만 정리하고 표식을 남긴다. 이미 돌았으면 `None`.
    pub fn purge_legacy_write_only_rows_if_needed(&self) -> Result<Option<LegacyCleanupResult>> {
        if matches!(
            self.load_setting::<bool>(LEGACY_WRITE_ONLY_CLEANUP_KEY),
            Ok(Some(true))
        ) {
            return Ok(None);
        }
        let result = self.purge_legacy_write_only_rows()?;
        self.save_setting(LEGACY_WRITE_ONLY_CLEANUP_KEY, &true)?;
        Ok(Some(result))
    }

    /// 표식과 무관하게 정리한다. 바뀐 것이 없으면 커밋하지 않는다.
    ///
    /// - CachedCategoryStat 전 행 삭제(파생 테이블).
    /// - CachedClip 의 `channel-*` 태그 제거 — 채널 태그만 가진 행은 삭제, 다른 태그가 남으면 순서 유지로 뗀다.
    pub fn purge_legacy_write_only_rows(&self) -> Result<LegacyCleanupResult> {
        self.write(|tx| {
            let mut result = LegacyCleanupResult {
                category_stat_rows: tx.query_row(
                    "SELECT count(*) FROM CachedCategoryStat",
                    [],
                    |r| r.get(0),
                )?,
                ..Default::default()
            };
            if result.category_stat_rows > 0 {
                tx.execute("DELETE FROM CachedCategoryStat", [])?;
            }
            let tagged = db::fetch::<CachedClip>(tx, "WHERE instr(sources, 'channel-') > 0", &[])?;
            for mut clip in tagged {
                let tags: Vec<&str> = clip.sources.split(',').collect();
                let kept: Vec<&str> = tags
                    .iter()
                    .copied()
                    .filter(|t| !t.starts_with("channel-"))
                    .collect();
                if kept.len() == tags.len() {
                    continue;
                }
                if kept.is_empty() {
                    db::delete(tx, &clip)?;
                    result.channel_only_clip_rows += 1;
                } else {
                    clip.sources = kept.join(",");
                    db::update(tx, &clip)?;
                    result.retagged_clip_rows += 1;
                }
            }
            let changed = result != LegacyCleanupResult::default();
            Ok((result, changed))
        })
    }

    /// 레거시 홈 캐시를 지운다 — `cache.` 로 시작하는 설정 행 전부와 `CachedLiveChannel` 전 행.
    /// 두 번째 실행은 쓰기 없이 `(0, 0)` 이다.
    pub fn purge_legacy_home_cache(&self) -> Result<(i64, i64)> {
        self.write(|tx| {
            let prefix = LEGACY_HOME_CACHE_KEY_PREFIX;
            let cond = "substr(key, 1, length(?1)) = ?1";
            let settings: i64 = tx.query_row(
                &format!("SELECT count(*) FROM PersistedSetting WHERE {cond}"),
                [prefix],
                |r| r.get(0),
            )?;
            let rows: i64 =
                tx.query_row("SELECT count(*) FROM CachedLiveChannel", [], |r| r.get(0))?;
            if settings == 0 && rows == 0 {
                return Ok(((0, 0), false));
            }
            if settings > 0 {
                tx.execute(
                    &format!("DELETE FROM PersistedSetting WHERE {cond}"),
                    [prefix],
                )?;
            }
            if rows > 0 {
                tx.execute("DELETE FROM CachedLiveChannel", [])?;
            }
            Ok(((settings, rows), true))
        })
    }

    /// 테스트·시드용: 카테고리 집계 행을 넣는다.
    #[doc(hidden)]
    pub fn insert_cached_category_stat(
        &self,
        row: &crate::models::CachedCategoryStat,
    ) -> Result<()> {
        self.write(|tx| {
            let mut row = row.clone();
            db::insert(tx, &mut row)?;
            Ok(((), true))
        })
    }
}
