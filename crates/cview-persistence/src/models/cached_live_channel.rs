//! 라이브 채널 메타 캐시.
//!
//! 원본: `Sources/CViewPersistence/Models/CachedLiveChannel.swift` (`CachedLiveChannel`, `CachedLiveChannelData`).

use crate::db::{db_model, split_sources};
use crate::time::{now, Date};
use std::collections::BTreeSet;

db_model! {
    /// 치지직 라이브 채널 메타 캐시 행. `sources` 는 정렬 CSV.
    pub struct CachedLiveChannel in "CachedLiveChannel" {
        pub channel_id: String => "channelId",
        pub channel_name: String => "channelName",
        pub channel_image_url: Option<String> => "channelImageUrl",
        pub live_title: String => "liveTitle",
        pub viewer_count: i64 => "viewerCount",
        pub category_name: Option<String> => "categoryName",
        /// "GAME" | "SPORTS" | "ETC"
        pub category_type: Option<String> => "categoryType",
        pub thumbnail_url: Option<String> => "thumbnailUrl",
        pub is_live: bool => "isLive",
        pub open_date: Option<Date> => "openDate",
        pub sources: String => "sources",
        pub fetched_at: Date => "fetchedAt",
    }
}

/// 라이브 채널 캐시 DTO(`CachedLiveChannelData`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CachedLiveChannelData {
    pub channel_id: String,
    pub channel_name: String,
    pub channel_image_url: Option<String>,
    pub live_title: String,
    pub viewer_count: i64,
    pub category_name: Option<String>,
    pub category_type: Option<String>,
    pub thumbnail_url: Option<String>,
    pub is_live: bool,
    pub open_date: Option<Date>,
    pub sources: BTreeSet<String>,
    pub fetched_at: Date,
}

impl CachedLiveChannelData {
    /// Swift 기본 인자와 같은 기본값(`liveTitle = ""`, `isLive = true`, `fetchedAt = .now`).
    pub fn new(channel_id: impl Into<String>, channel_name: impl Into<String>) -> Self {
        Self {
            channel_id: channel_id.into(),
            channel_name: channel_name.into(),
            channel_image_url: None,
            live_title: String::new(),
            viewer_count: 0,
            category_name: None,
            category_type: None,
            thumbnail_url: None,
            is_live: true,
            open_date: None,
            sources: BTreeSet::new(),
            fetched_at: now(),
        }
    }
}

impl From<&CachedLiveChannel> for CachedLiveChannelData {
    fn from(m: &CachedLiveChannel) -> Self {
        Self {
            channel_id: m.channel_id.clone(),
            channel_name: m.channel_name.clone(),
            channel_image_url: m.channel_image_url.clone(),
            live_title: m.live_title.clone(),
            viewer_count: m.viewer_count,
            category_name: m.category_name.clone(),
            category_type: m.category_type.clone(),
            thumbnail_url: m.thumbnail_url.clone(),
            is_live: m.is_live,
            open_date: m.open_date,
            sources: split_sources(&m.sources),
            fetched_at: m.fetched_at,
        }
    }
}
