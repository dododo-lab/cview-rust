//! 카테고리 시청자/라이브 수 시계열 스냅샷.
//!
//! 원본: `Sources/CViewPersistence/Models/CategorySnapshot.swift` (`CategorySnapshot`, `CategorySnapshotInput`).

use crate::db::db_model;
use crate::time::Date;

db_model! {
    /// 카테고리 스냅샷 행. `category` 는 unique 가 아니다(시간에 따라 누적).
    pub struct CategorySnapshot in "CategorySnapshot" {
        pub category: String => "category",
        pub viewer_total: i64 => "viewerTotal",
        pub live_count: i64 => "liveCount",
        pub captured_at: Date => "capturedAt",
    }
}

/// 캡처 입력(`CategorySnapshotInput`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CategorySnapshotInput {
    pub category: String,
    pub viewer_total: i64,
    pub live_count: i64,
}

impl CategorySnapshotInput {
    pub fn new(category: impl Into<String>, viewer_total: i64, live_count: i64) -> Self {
        Self {
            category: category.into(),
            viewer_total,
            live_count,
        }
    }
}
