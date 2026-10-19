//! 특정 시각 이후의 시청 시간 합계(메뉴바 "오늘 시청").
//!
//! 원본: `Sources/CViewPersistence/DataStore+WatchTimeWindow.swift`.

use super::DataStore;
use crate::db::{self, dv};
use crate::error::Result;
use crate::models::WatchHistory;
use crate::time::{now, Date, TimeInterval};

impl DataStore {
    /// `start` 이후에 **시작된** 시청 기록의 시간 합계(진행 중 기록 포함).
    pub fn total_watch_time_since(&self, start: Date) -> Result<TimeInterval> {
        let now = now();
        self.read(|conn| {
            Ok(
                db::fetch::<WatchHistory>(conn, "WHERE startedAt >= ?", &[dv(&start)])?
                    .iter()
                    .map(|r| r.effective_duration(now))
                    .sum(),
            )
        })
    }
}
