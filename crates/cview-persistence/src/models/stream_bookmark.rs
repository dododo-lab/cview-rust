//! 스트림 북마크.
//!
//! 원본: `Sources/CViewPersistence/Models/StreamBookmark.swift` (`StreamBookmark`, `StreamBookmarkData`).

use crate::db::db_model;
use crate::time::{Date, TimeInterval};

db_model! {
    /// 라이브 시청 중 캡처한 순간.
    pub struct StreamBookmark in "StreamBookmark" {
        pub channel_id: String => "channelId",
        pub channel_name: String => "channelName",
        pub live_title: String => "liveTitle",
        pub category_name: Option<String> => "categoryName",
        pub thumbnail_url: Option<String> => "thumbnailURL",
        pub captured_at: Date => "capturedAt",
        /// 방송 시작 후 경과 초.
        pub live_uptime: f64 => "liveUptime",
        pub note: Option<String> => "note",
    }
}

/// 북마크 DTO(`StreamBookmarkData`).
#[derive(Debug, Clone, PartialEq)]
pub struct StreamBookmarkData {
    /// `"channelId|<capturedAt epoch>"` — 안정적인 식별자.
    pub id: String,
    pub channel_id: String,
    pub channel_name: String,
    pub live_title: String,
    pub category_name: Option<String>,
    pub thumbnail_url: Option<String>,
    pub captured_at: Date,
    pub live_uptime: TimeInterval,
    pub note: Option<String>,
}

impl StreamBookmarkData {
    /// `H:MM:SS` 또는 `M:SS`.
    pub fn formatted_uptime(&self) -> String {
        let total = if self.live_uptime.is_finite() {
            self.live_uptime as i64
        } else {
            0
        };
        let hours = total / 3600;
        let minutes = (total % 3600) / 60;
        let seconds = total % 60;
        if hours > 0 {
            format!("{hours}:{minutes:02}:{seconds:02}")
        } else {
            format!("{minutes}:{seconds:02}")
        }
    }
}
