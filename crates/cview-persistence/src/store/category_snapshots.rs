//! 카테고리 스냅샷(시계열 → 1시간 기준선 대비 증가율).
//!
//! 원본: `Sources/CViewPersistence/DataStore+CategorySnapshots.swift`.

use super::DataStore;
use crate::db::{self, dv};
use crate::error::Result;
use crate::models::{CategorySnapshot, CategorySnapshotInput};
use crate::time::{adding, interval_since, now, Date, TimeInterval};
use rusqlite::Connection;
use std::collections::HashMap;

/// 기준선 기본 창(초).
pub const DEFAULT_BASELINE_WINDOW: TimeInterval = 3600.0;
/// 기준선 기본 허용 오차(초).
pub const DEFAULT_BASELINE_TOLERANCE: TimeInterval = 900.0;
/// 기록 간격 하한 기본값(초).
pub const DEFAULT_MIN_SPACING: TimeInterval = 300.0;
/// 기본 보존 기간(초) — 6시간.
pub const DEFAULT_SNAPSHOT_RETENTION: TimeInterval = 6.0 * 3600.0;

fn insert_snapshots(
    conn: &Connection,
    inputs: &[CategorySnapshotInput],
    captured_at: Date,
) -> Result<()> {
    for input in inputs {
        let mut row = CategorySnapshot {
            id: 0,
            category: input.category.clone(),
            viewer_total: input.viewer_total,
            live_count: input.live_count,
            captured_at,
        };
        db::insert(conn, &mut row)?;
    }
    Ok(())
}

fn baselines(
    conn: &Connection,
    reference: Date,
    window: TimeInterval,
    tolerance: TimeInterval,
) -> Result<HashMap<String, i64>> {
    let target = adding(&reference, -window);
    let lower = adding(&target, -tolerance);
    let upper = adding(&target, tolerance);
    let rows = db::fetch::<CategorySnapshot>(
        conn,
        "WHERE capturedAt >= ? AND capturedAt <= ? ORDER BY capturedAt ASC, id ASC",
        &[dv(&lower), dv(&upper)],
    )?;
    // 카테고리별 target 에 가장 가까운 row(같으면 먼저 본 것).
    let mut best: HashMap<String, (TimeInterval, i64)> = HashMap::new();
    for row in rows {
        let delta = interval_since(&row.captured_at, &target).abs();
        if let Some((current, _)) = best.get(&row.category) {
            if *current <= delta {
                continue;
            }
        }
        best.insert(row.category.clone(), (delta, row.viewer_total));
    }
    Ok(best.into_iter().map(|(k, (_, v))| (k, v)).collect())
}

impl DataStore {
    /// 배치 스냅샷 기록 — 모든 행이 같은 `captured_at`(수집 시각). prune 은 별도 API.
    pub fn record_category_snapshots(
        &self,
        inputs: &[CategorySnapshotInput],
        captured_at: Date,
    ) -> Result<()> {
        if inputs.is_empty() {
            return Ok(());
        }
        self.write(|tx| {
            insert_snapshots(tx, inputs, captured_at)?;
            Ok(((), true))
        })
    }

    /// 기록 + 기준선 읽기를 한 번에. `captured_at - min_spacing` 이후(초과)에 기록이 있으면 쓰지 않는다.
    pub fn record_category_snapshots_and_read_baselines(
        &self,
        inputs: &[CategorySnapshotInput],
        captured_at: Date,
        min_spacing: TimeInterval,
        window: TimeInterval,
        tolerance: TimeInterval,
    ) -> Result<HashMap<String, i64>> {
        if !inputs.is_empty() {
            self.write(|tx| {
                let spacing_start = adding(&captured_at, -min_spacing.max(0.0));
                let recent = db::count::<CategorySnapshot>(
                    tx,
                    "WHERE capturedAt > ?",
                    &[dv(&spacing_start)],
                )?;
                if recent == 0 {
                    insert_snapshots(tx, inputs, captured_at)?;
                    return Ok(((), true));
                }
                Ok(((), false))
            })?;
        }
        self.category_baselines(captured_at, window, tolerance)
    }

    /// 각 카테고리의 `reference - window`(±tolerance) 기준선 viewerTotal. 없으면 결과에서 뺀다.
    pub fn category_baselines(
        &self,
        reference: Date,
        window: TimeInterval,
        tolerance: TimeInterval,
    ) -> Result<HashMap<String, i64>> {
        self.read(|conn| baselines(conn, reference, window, tolerance))
    }

    /// 카테고리별 증가율(%) — 지금 기준. 기준선이 없거나 0 이면 빠진다.
    pub fn category_growth_rates(
        &self,
        current_viewers: &HashMap<String, i64>,
        window: TimeInterval,
        tolerance: TimeInterval,
    ) -> Result<HashMap<String, f64>> {
        let baselines = self.category_baselines(now(), window, tolerance)?;
        let mut result = HashMap::new();
        for (category, baseline) in baselines {
            let Some(current) = current_viewers.get(&category) else {
                continue;
            };
            if baseline <= 0 {
                continue;
            }
            let delta = (current - baseline) as f64;
            result.insert(category, delta / baseline as f64 * 100.0);
        }
        Ok(result)
    }

    /// 보존 기간 초과 row 제거. 지운 수(0 이면 커밋 없음).
    pub fn prune_category_snapshots(&self, retention: TimeInterval, now: Date) -> Result<i64> {
        let cutoff = adding(&now, -retention);
        self.write(|tx| {
            let count = db::count::<CategorySnapshot>(tx, "WHERE capturedAt < ?", &[dv(&cutoff)])?;
            if count == 0 {
                return Ok((0, false));
            }
            tx.execute(
                "DELETE FROM CategorySnapshot WHERE capturedAt < ?",
                [crate::time::to_db(&cutoff)],
            )?;
            Ok((count, true))
        })
    }

    /// 테스트·진단용: 스냅샷 전 행.
    #[doc(hidden)]
    pub fn all_category_snapshots(&self) -> Result<Vec<CategorySnapshot>> {
        self.read(|conn| {
            Ok(db::fetch::<CategorySnapshot>(
                conn,
                "ORDER BY capturedAt ASC, id ASC",
                &[],
            )?)
        })
    }
}
