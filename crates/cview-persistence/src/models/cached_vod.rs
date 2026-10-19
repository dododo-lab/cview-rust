//! VOD 메타 캐시와 구독 피드 토큰 규칙.
//!
//! 원본: `Sources/CViewPersistence/Models/CachedVOD.swift` (`CachedVOD`, 구독 토큰 확장, `CachedVODData`).

use crate::db::{db_model, split_sources};
use crate::time::{now, Date};
use std::collections::BTreeSet;

db_model! {
    /// 치지직 VOD 메타 캐시 행. `sources` 는 정렬 CSV.
    pub struct CachedVod in "CachedVOD" {
        pub video_no: i64 => "videoNo",
        pub video_id: Option<String> => "videoId",
        pub video_title: String => "videoTitle",
        pub thumbnail_image_url: Option<String> => "thumbnailImageURL",
        pub duration: i64 => "duration",
        pub read_count: i64 => "readCount",
        pub publish_date: Option<Date> => "publishDate",
        pub channel_id: Option<String> => "channelId",
        pub channel_name: Option<String> => "channelName",
        pub channel_image_url: Option<String> => "channelImageURL",
        pub content_hash: String => "contentHash",
        pub sources: String => "sources",
        pub fetched_at: Date => "fetchedAt",
    }
}

impl CachedVod {
    /// 계정 태그 없는 옛 구독 토큰. 읽지도 쓰지도 않고 지우기만 한다.
    pub const LEGACY_SUBSCRIPTION_TAG: &'static str = "subscriptions";
    /// 계정 범위 구독 토큰의 접두사 — 뒤에 scope 가 붙는다.
    pub const SUBSCRIPTION_TAG_PREFIX: &'static str = "subscriptions.";

    /// `CachedVOD.computeContentHash` — 클립과 같은 식.
    pub fn compute_content_hash(
        title: &str,
        thumbnail_url: Option<&str>,
        read_count: i64,
        channel_name: Option<&str>,
    ) -> String {
        super::cached_clip::compute_content_hash(title, thumbnail_url, read_count, channel_name)
    }

    /// scope 의 구독 토큰. scope 가 비었거나 쉼표를 담으면(CSV 를 깨뜨린다) `None`.
    pub fn subscription_tag(scope: &str) -> Option<String> {
        if scope.is_empty() || scope.contains(',') {
            return None;
        }
        Some(format!("{}{}", Self::SUBSCRIPTION_TAG_PREFIX, scope))
    }

    /// 옛 토큰이거나 계정 범위 토큰인가. 다른 원천 토큰(`channel-*` 등)은 false.
    pub fn is_subscription_token(token: &str) -> bool {
        token == Self::LEGACY_SUBSCRIPTION_TAG || token.starts_with(Self::SUBSCRIPTION_TAG_PREFIX)
    }
}

/// VOD 캐시 DTO(`CachedVODData`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CachedVodData {
    pub video_no: i64,
    pub video_id: Option<String>,
    pub video_title: String,
    pub thumbnail_image_url: Option<String>,
    pub duration: i64,
    pub read_count: i64,
    pub publish_date: Option<Date>,
    pub channel_id: Option<String>,
    pub channel_name: Option<String>,
    pub channel_image_url: Option<String>,
    pub content_hash: String,
    pub sources: BTreeSet<String>,
    pub fetched_at: Date,
}

impl CachedVodData {
    /// 필수 값만 받고 나머지는 기본값(`fetchedAt = .now`).
    pub fn new(video_no: i64, video_title: impl Into<String>) -> Self {
        Self {
            video_no,
            video_id: None,
            video_title: video_title.into(),
            thumbnail_image_url: None,
            duration: 0,
            read_count: 0,
            publish_date: None,
            channel_id: None,
            channel_name: None,
            channel_image_url: None,
            content_hash: String::new(),
            sources: BTreeSet::new(),
            fetched_at: now(),
        }
    }
}

impl From<&CachedVod> for CachedVodData {
    fn from(m: &CachedVod) -> Self {
        Self {
            video_no: m.video_no,
            video_id: m.video_id.clone(),
            video_title: m.video_title.clone(),
            thumbnail_image_url: m.thumbnail_image_url.clone(),
            duration: m.duration,
            read_count: m.read_count,
            publish_date: m.publish_date,
            channel_id: m.channel_id.clone(),
            channel_name: m.channel_name.clone(),
            channel_image_url: m.channel_image_url.clone(),
            content_hash: m.content_hash.clone(),
            sources: split_sources(&m.sources),
            fetched_at: m.fetched_at,
        }
    }
}
