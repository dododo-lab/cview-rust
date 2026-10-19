//! `Sources/CViewCore/Models/VODModels.swift` · `VODFeed.swift` 포팅 — VOD 재생 도메인 모델과
//! 구독 VOD 피드 병합 헬퍼.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::Url;

use super::channel::ChannelInfo;
use super::decode::chzzk_date_opt;
use super::search::VodInfo;

/// VOD 재생 상세 정보 (API 응답). Swift 합성 Codable — `videoNo`·`videoTitle`·`duration`·`readCount` 필수.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VodDetail {
    pub video_no: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_id: Option<String>,
    pub video_title: String,
    #[serde(rename = "videoImageUrl", default, skip_serializing_if = "Option::is_none")]
    pub video_image_url: Option<Url>,
    pub duration: i64,
    #[serde(default, with = "chzzk_date_opt", skip_serializing_if = "Option::is_none")]
    pub publish_date: Option<DateTime<Utc>>,
    pub read_count: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<ChannelInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vod_status: Option<String>,
    #[serde(rename = "thumbnailImageUrl", default, skip_serializing_if = "Option::is_none")]
    pub thumbnail_image_url: Option<Url>,
    /// 재생 정보 JSON 문자열 (livePlaybackJSON 과 유사한 구조)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_rewind_playback_json: Option<String>,
    /// 직접 VOD URL
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vod_url: Option<String>,
    /// inKey (재생 인증용)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_key: Option<String>,
}

/// VOD 스트림 정보 (해상된 재생 URL).
#[derive(Debug, Clone, PartialEq)]
pub struct VodStreamInfo {
    pub video_no: i64,
    pub title: String,
    pub stream_url: Url,
    pub duration: Duration,
    pub qualities: Vec<VodQuality>,
    pub channel_name: String,
    /// 썸네일 URL string — "이어 보기" 기록 카드 표시에 사용.
    pub thumbnail_url_string: Option<String>,
    pub channel_id: Option<String>,
    pub read_count: i64,
    pub publish_date: Option<DateTime<Utc>>,
    pub thumbnail_url: Option<Url>,
}

/// VOD 화질 정보.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VodQuality {
    pub id: String,
    pub name: String,
    pub resolution: String,
    pub bandwidth: i64,
    pub url: Url,
}

/// VOD 재생 상태.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VodPlaybackState {
    Idle,
    Loading,
    Playing,
    Paused,
    Seeking,
    Buffering,
    Ended,
    Error(String),
}

/// 재생 속도 프리셋.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlaybackSpeed {
    X025,
    X050,
    X075,
    X100,
    X125,
    X150,
    X175,
    X200,
}

impl PlaybackSpeed {
    pub const ALL: [PlaybackSpeed; 8] = [
        Self::X025,
        Self::X050,
        Self::X075,
        Self::X100,
        Self::X125,
        Self::X150,
        Self::X175,
        Self::X200,
    ];

    pub fn raw_value(self) -> f32 {
        match self {
            Self::X025 => 0.25,
            Self::X050 => 0.5,
            Self::X075 => 0.75,
            Self::X100 => 1.0,
            Self::X125 => 1.25,
            Self::X150 => 1.5,
            Self::X175 => 1.75,
            Self::X200 => 2.0,
        }
    }

    /// 메뉴 라벨. [2026-09-19] 실제 배속과 어긋나지 않게 전체 정밀도로 표기한다
    /// (`1.75` → `"1.75x"`). ⚠ 유효숫자 제한을 다시 붙이지 말 것.
    pub fn display_name(self) -> String {
        let v = self.raw_value();
        if v == 1.0 {
            return "1x (기본)".into();
        }
        if v == v.trunc() {
            return format!("{}x", v as i64);
        }
        format!("{v}x")
    }
}

/// 여러 채널의 VOD 목록을 하나의 피드로 병합한다 (Swift `VODFeed.mergeLatest`).
///
/// 규칙: `videoNo` 중복 제거(먼저 등장한 항목 우선) → `publishDate` 내림차순(최신 우선, `None` 은 뒤)
/// → 동률은 `videoNo` 내림차순. `limit` 이 0 이하면 제한 없이 전체 반환.
pub fn merge_latest_vods(lists: &[Vec<VodInfo>], limit: i64) -> Vec<VodInfo> {
    let mut seen = HashSet::new();
    let mut merged: Vec<VodInfo> = Vec::new();
    for list in lists {
        for vod in list {
            if seen.insert(vod.video_no) {
                merged.push(vod.clone());
            }
        }
    }
    merged.sort_by(|l, r| match (&l.publish_date, &r.publish_date) {
        (Some(a), Some(b)) if a != b => b.cmp(a),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        _ => r.video_no.cmp(&l.video_no),
    });
    if limit > 0 {
        merged.truncate(limit as usize);
    }
    merged
}

#[cfg(test)]
mod tests {
    //! `Tests/CViewCoreTests/VODFeedTests.swift` 포팅 + 속도 라벨.
    use super::*;
    use chrono::TimeZone;

    fn vod(no: i64, day: Option<u32>) -> VodInfo {
        let mut v = VodInfo::new(no, format!("v{no}"));
        v.publish_date = day.map(|d| Utc.with_ymd_and_hms(2026, 1, d, 0, 0, 0).unwrap());
        v
    }

    #[test]
    fn merge_dedups_sorts_and_limits() {
        let a = vec![vod(1, Some(1)), vod(2, Some(3)), vod(3, None)];
        let b = vec![vod(2, Some(9)), vod(4, Some(3)), vod(5, None)];
        let merged = merge_latest_vods(&[a, b], 0);
        let order: Vec<i64> = merged.iter().map(|v| v.video_no).collect();
        // 2(3일, 첫 등장 우선) · 4(3일) 동률 → videoNo 내림차순, None 은 뒤 (5, 3)
        assert_eq!(order, vec![4, 2, 1, 5, 3]);
        assert_eq!(merged[1].publish_date, Some(Utc.with_ymd_and_hms(2026, 1, 3, 0, 0, 0).unwrap()));
        assert_eq!(merge_latest_vods(&[vec![vod(1, Some(1)), vod(2, Some(2))]], 1).len(), 1);
        assert!(merge_latest_vods(&[], 5).is_empty());
    }

    #[test]
    fn playback_speed_labels() {
        assert_eq!(PlaybackSpeed::X100.display_name(), "1x (기본)");
        assert_eq!(PlaybackSpeed::X200.display_name(), "2x");
        assert_eq!(PlaybackSpeed::X025.display_name(), "0.25x");
        assert_eq!(PlaybackSpeed::X050.display_name(), "0.5x");
        assert_eq!(PlaybackSpeed::X125.display_name(), "1.25x");
        assert_eq!(PlaybackSpeed::X175.display_name(), "1.75x");
    }

    #[test]
    fn vod_detail_decoding() {
        let d: VodDetail = serde_json::from_str(
            r#"{"videoNo":5,"videoTitle":"t","duration":10,"readCount":3,"publishDate":"2024-06-15 18:30:00","inKey":"k"}"#,
        )
        .unwrap();
        assert_eq!(d.video_no, 5);
        assert_eq!(d.in_key.as_deref(), Some("k"));
        assert!(serde_json::from_str::<VodDetail>(r#"{"videoNo":5,"videoTitle":"t","duration":10}"#).is_err());
    }
}
