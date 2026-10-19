//! 보존 정리와 변경 이력 정리.
//!
//! 원본: `Sources/CViewPersistence/DataStore+StoreMaintenance.swift` (`runRetentionMaintenance`,
//! `purgeChangeHistory`, `historyTransactionCount`, `StoreMaintenancePolicy`).
//!
//! SwiftData 는 save 마다 persistent history 를 쌓았다. 이 포팅은 커밋 시각만 프로세스 메모리에
//! 기록하고([`DataStore::write`]), 정리는 그 기록을 창 단위로 지운 뒤 `PRAGMA incremental_vacuum` 으로
//! 빈 페이지를 돌려준다(스토어는 `auto_vacuum = INCREMENTAL`).

use super::DataStore;
use crate::time::{adding, now, Date, TimeInterval};

/// 스토어 유지보수 상수(`StoreMaintenancePolicy`).
pub struct StoreMaintenancePolicy;

impl StoreMaintenancePolicy {
    /// 이력 삭제 창 — 하루치씩.
    pub const HISTORY_DELETE_WINDOW: TimeInterval = 24.0 * 3600.0;
}

/// 시청 기록 보존 일수.
pub const WATCH_HISTORY_RETENTION_DAYS: i64 = 365;

impl DataStore {
    /// 보존 정리(시청 기록 365일 · 클립 30일 · VOD 60일) + 레거시 정리 + 변경 이력 삭제. 실패는 삼킨다.
    ///
    /// `vod_subscription_tag` 는 지금 계정의 구독 VOD 토큰(`CachedVod::subscription_tag`). `None` 이면 옛 토큰만 지운다.
    pub async fn run_retention_maintenance(&self, vod_subscription_tag: Option<String>) {
        let store = self.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let _ = store.cleanup_old_watch_history(WATCH_HISTORY_RETENTION_DAYS);
            let _ = store.prune_clip_playback(super::clip_playback::CLIP_PLAYBACK_RETENTION);
            let _ = store.prune_vod_playback(super::vod_playback::VOD_PLAYBACK_RETENTION);
            let _ = store.purge_legacy_write_only_rows_if_needed();
            let _ = store.purge_subscription_vod_tags(vod_subscription_tag.as_deref());
        })
        .await;
        self.purge_change_history(now(), StoreMaintenancePolicy::HISTORY_DELETE_WINDOW)
            .await;
    }

    /// 변경 이력을 `cutoff` 이전까지 창 단위로 지운다. 지운 창 수(실패 시 `None`).
    /// 창이 0 이하·NaN 이면 기본 창, 하한 60초.
    pub async fn purge_change_history(&self, cutoff: Date, window: TimeInterval) -> Option<usize> {
        let step = if window.is_finite() {
            window.max(60.0)
        } else {
            StoreMaintenancePolicy::HISTORY_DELETE_WINDOW
        };
        let oldest = self.history().first().copied();
        let mut edge = match oldest {
            Some(e) if e < cutoff => e,
            _ => return Some(0),
        };
        let mut windows = 0usize;
        while edge < cutoff {
            edge = cutoff.min(adding(&edge, step));
            let bound = edge;
            self.history().retain(|t| *t >= bound);
            windows += 1;
            tokio::task::yield_now().await;
        }
        let store = self.clone();
        let vacuum = tokio::task::spawn_blocking(move || {
            store.read(|conn| {
                conn.execute_batch("PRAGMA incremental_vacuum;")?;
                Ok(())
            })
        })
        .await;
        match vacuum {
            Ok(Ok(())) => Some(windows),
            Ok(Err(e)) => {
                tracing::error!("변경 이력 정리 실패: {e}");
                None
            }
            Err(e) => {
                tracing::error!("변경 이력 정리 실패: {e}");
                None
            }
        }
    }

    /// 남아 있는 이력 트랜잭션 수(테스트·진단 전용).
    pub fn history_transaction_count(&self) -> usize {
        self.history().len()
    }
}
