//! 클립·VOD 재생 진행률과 나중에 볼 VOD.
//!
//! 원본: `Sources/CViewPersistence/Models/PersistedClipPlayback.swift`,
//! `Sources/CViewPersistence/Models/PersistedVODPlayback.swift`,
//! `Sources/CViewPersistence/Models/PersistedVODWatchLater.swift`.

use crate::db::db_model;
use crate::time::Date;

/// 진행률 0…1 (`durationSec <= 0` 이면 0).
pub fn playback_progress(last_position_sec: f64, duration_sec: f64) -> f64 {
    if duration_sec.is_nan() || duration_sec <= 0.0 {
        return 0.0;
    }
    (last_position_sec / duration_sec).clamp(0.0, 1.0)
}

db_model! {
    /// 클립 재생 진행률 행.
    pub struct PersistedClipPlayback in "PersistedClipPlayback" {
        pub clip_uid: String => "clipUID",
        pub last_position_sec: f64 => "lastPositionSec",
        pub duration_sec: f64 => "durationSec",
        pub clip_title: String => "clipTitle",
        pub channel_name: String => "channelName",
        pub thumbnail_url_string: Option<String> => "thumbnailURLString",
        pub last_seen_at: Date => "lastSeenAt",
    }
}

impl PersistedClipPlayback {
    pub fn progress(&self) -> f64 {
        playback_progress(self.last_position_sec, self.duration_sec)
    }
}

/// 클립 재생 진행률 DTO(`PersistedClipPlaybackData`).
#[derive(Debug, Clone, PartialEq)]
pub struct PersistedClipPlaybackData {
    pub clip_uid: String,
    pub last_position_sec: f64,
    pub duration_sec: f64,
    pub clip_title: String,
    pub channel_name: String,
    pub thumbnail_url_string: Option<String>,
    pub last_seen_at: Date,
}

impl PersistedClipPlaybackData {
    pub fn id(&self) -> &str {
        &self.clip_uid
    }
    pub fn progress(&self) -> f64 {
        playback_progress(self.last_position_sec, self.duration_sec)
    }
}

impl From<&PersistedClipPlayback> for PersistedClipPlaybackData {
    fn from(m: &PersistedClipPlayback) -> Self {
        Self {
            clip_uid: m.clip_uid.clone(),
            last_position_sec: m.last_position_sec,
            duration_sec: m.duration_sec,
            clip_title: m.clip_title.clone(),
            channel_name: m.channel_name.clone(),
            thumbnail_url_string: m.thumbnail_url_string.clone(),
            last_seen_at: m.last_seen_at,
        }
    }
}

db_model! {
    /// VOD 재생 진행률 행.
    pub struct PersistedVodPlayback in "PersistedVODPlayback" {
        pub video_no: i64 => "videoNo",
        pub last_position_sec: f64 => "lastPositionSec",
        pub duration_sec: f64 => "durationSec",
        pub video_title: String => "videoTitle",
        pub channel_name: String => "channelName",
        pub thumbnail_url_string: Option<String> => "thumbnailURLString",
        pub last_seen_at: Date => "lastSeenAt",
    }
}

impl PersistedVodPlayback {
    pub fn progress(&self) -> f64 {
        playback_progress(self.last_position_sec, self.duration_sec)
    }
}

/// VOD 재생 진행률 DTO(`PersistedVODPlaybackData`).
#[derive(Debug, Clone, PartialEq)]
pub struct PersistedVodPlaybackData {
    pub video_no: i64,
    pub last_position_sec: f64,
    pub duration_sec: f64,
    pub video_title: String,
    pub channel_name: String,
    pub thumbnail_url_string: Option<String>,
    pub last_seen_at: Date,
}

impl PersistedVodPlaybackData {
    pub fn id(&self) -> i64 {
        self.video_no
    }
    pub fn progress(&self) -> f64 {
        playback_progress(self.last_position_sec, self.duration_sec)
    }
}

impl From<&PersistedVodPlayback> for PersistedVodPlaybackData {
    fn from(m: &PersistedVodPlayback) -> Self {
        Self {
            video_no: m.video_no,
            last_position_sec: m.last_position_sec,
            duration_sec: m.duration_sec,
            video_title: m.video_title.clone(),
            channel_name: m.channel_name.clone(),
            thumbnail_url_string: m.thumbnail_url_string.clone(),
            last_seen_at: m.last_seen_at,
        }
    }
}

db_model! {
    /// 나중에 볼 VOD 행.
    pub struct PersistedVodWatchLater in "PersistedVODWatchLater" {
        pub video_no: i64 => "videoNo",
        pub added_at: Date => "addedAt",
        pub video_title: String => "videoTitle",
        pub channel_name: String => "channelName",
        pub thumbnail_url_string: Option<String> => "thumbnailURLString",
        pub duration_sec: f64 => "durationSec",
    }
}

/// 나중에 볼 VOD DTO(`PersistedVODWatchLaterData`).
#[derive(Debug, Clone, PartialEq)]
pub struct PersistedVodWatchLaterData {
    pub video_no: i64,
    pub added_at: Date,
    pub video_title: String,
    pub channel_name: String,
    pub thumbnail_url_string: Option<String>,
    pub duration_sec: f64,
}

impl PersistedVodWatchLaterData {
    pub fn id(&self) -> i64 {
        self.video_no
    }
}

impl From<&PersistedVodWatchLater> for PersistedVodWatchLaterData {
    fn from(m: &PersistedVodWatchLater) -> Self {
        Self {
            video_no: m.video_no,
            added_at: m.added_at,
            video_title: m.video_title.clone(),
            channel_name: m.channel_name.clone(),
            thumbnail_url_string: m.thumbnail_url_string.clone(),
            duration_sec: m.duration_sec,
        }
    }
}
