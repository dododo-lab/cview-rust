//! 클립 메타 캐시와 카테고리 집계 캐시.
//!
//! 원본: `Sources/CViewPersistence/Models/CachedClip.swift`
//! (`CachedClip`, `CachedClipData`, `CachedCategoryStat`).

use crate::db::{db_model, split_sources};
use crate::time::{now, Date};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

db_model! {
    /// 치지직 클립 메타 캐시 행. `sources` 는 정렬 CSV.
    pub struct CachedClip in "CachedClip" {
        pub clip_uid: String => "clipUID",
        pub clip_title: String => "clipTitle",
        pub thumbnail_image_url: Option<String> => "thumbnailImageURL",
        pub clip_url: Option<String> => "clipURL",
        pub duration: i64 => "duration",
        pub read_count: i64 => "readCount",
        pub created_date: Option<Date> => "createdDate",
        pub channel_id: Option<String> => "channelId",
        pub channel_name: Option<String> => "channelName",
        pub channel_image_url: Option<String> => "channelImageURL",
        pub content_hash: String => "contentHash",
        pub sources: String => "sources",
        pub fetched_at: Date => "fetchedAt",
    }
}

/// SHA-256(title|thumb|readCount|channelName) 16진 소문자 — 구분자 없이 이어 붙인다(원본 그대로).
pub fn compute_content_hash(
    title: &str,
    thumbnail_url: Option<&str>,
    read_count: i64,
    channel_name: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(title.as_bytes());
    hasher.update(thumbnail_url.unwrap_or("").as_bytes());
    hasher.update(read_count.to_string().as_bytes());
    hasher.update(channel_name.unwrap_or("").as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

impl CachedClip {
    /// `CachedClip.computeContentHash(title:thumbnailURL:readCount:channelName:)`.
    pub fn compute_content_hash(
        title: &str,
        thumbnail_url: Option<&str>,
        read_count: i64,
        channel_name: Option<&str>,
    ) -> String {
        compute_content_hash(title, thumbnail_url, read_count, channel_name)
    }
}

/// 클립 캐시 DTO(`CachedClipData`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CachedClipData {
    pub clip_uid: String,
    pub clip_title: String,
    pub thumbnail_image_url: Option<String>,
    pub clip_url: Option<String>,
    pub duration: i64,
    pub read_count: i64,
    pub created_date: Option<Date>,
    pub channel_id: Option<String>,
    pub channel_name: Option<String>,
    pub channel_image_url: Option<String>,
    pub content_hash: String,
    pub sources: BTreeSet<String>,
    pub fetched_at: Date,
}

impl CachedClipData {
    /// 필수 값만 받고 나머지는 기본값(`fetchedAt = .now`).
    pub fn new(clip_uid: impl Into<String>, clip_title: impl Into<String>) -> Self {
        Self {
            clip_uid: clip_uid.into(),
            clip_title: clip_title.into(),
            thumbnail_image_url: None,
            clip_url: None,
            duration: 0,
            read_count: 0,
            created_date: None,
            channel_id: None,
            channel_name: None,
            channel_image_url: None,
            content_hash: String::new(),
            sources: BTreeSet::new(),
            fetched_at: now(),
        }
    }
}

impl From<&CachedClip> for CachedClipData {
    fn from(m: &CachedClip) -> Self {
        Self {
            clip_uid: m.clip_uid.clone(),
            clip_title: m.clip_title.clone(),
            thumbnail_image_url: m.thumbnail_image_url.clone(),
            clip_url: m.clip_url.clone(),
            duration: m.duration,
            read_count: m.read_count,
            created_date: m.created_date,
            channel_id: m.channel_id.clone(),
            channel_name: m.channel_name.clone(),
            channel_image_url: m.channel_image_url.clone(),
            content_hash: m.content_hash.clone(),
            sources: split_sources(&m.sources),
            fetched_at: m.fetched_at,
        }
    }
}

db_model! {
    /// 카테고리 집계 캐시 행.
    pub struct CachedCategoryStat in "CachedCategoryStat" {
        pub category_name: String => "categoryName",
        pub live_count: i64 => "liveCount",
        pub total_viewers: i64 => "totalViewers",
        pub top_channel_id: Option<String> => "topChannelId",
        pub top_channel_name: Option<String> => "topChannelName",
        pub fetched_at: Date => "fetchedAt",
    }
}
