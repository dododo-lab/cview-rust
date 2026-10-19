//! `Sources/CViewCore/Models/ClipModels.swift` 포팅 — 클립 재생을 위한 도메인 모델.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::Url;

use super::channel::ChannelInfo;
use super::decode::chzzk_date_opt;

/// `POST /service/v1/clips/{uid}/inkey` 응답 내용.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipInkeyContent {
    #[serde(rename = "inKey")]
    pub in_key: String,
}

/// Naver rmcnmv VOD play v2.0 응답 (HLS URL 추출용).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NaverVodPlayResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub videos: Option<NaverVodVideos>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NaverVodVideos {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list: Option<Vec<NaverVodVideo>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NaverVodVideo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoded_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub master_playlist_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl NaverVodPlayResponse {
    /// 첫 번째 HLS m3u8 URL 반환 (`masterPlaylistUrl` 또는 `source`). ADHLS/ABR_HLS 타입 우선.
    pub fn best_hls_url(&self) -> Option<String> {
        if self.error_code.is_some() {
            return None;
        }
        let list = self.videos.as_ref().and_then(|v| v.list.as_deref()).unwrap_or(&[]);
        if let Some(adhls) = list
            .iter()
            .find(|v| matches!(v.encoded_type.as_deref(), Some("ADHLS") | Some("ABR_HLS")))
        {
            return adhls.master_playlist_url.clone().or_else(|| adhls.source.clone());
        }
        let first = list.first()?;
        first.master_playlist_url.clone().or_else(|| first.source.clone())
    }
}

/// 클립 상세 정보의 선택적 필드 (`/service/v1/clips/{uid}/detail` → `content.optionalProperty`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipOptionalProperty {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_channel: Option<ChannelInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maker_channel: Option<ChannelInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_delete_permission: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_user_block: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub penalty: Option<bool>,
}

/// 클립 상세 정보 (`/service/v1/clips/{uid}/detail`). Swift 합성 Codable 과 같은 엄격도:
/// `clipUID`·`clipTitle`·`duration` 은 필수.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipDetail {
    /// API 응답 키: `"clipUID"` (대문자 UID)
    #[serde(rename = "clipUID")]
    pub clip_uid: String,
    pub clip_title: String,
    #[serde(rename = "clipUrl", default, skip_serializing_if = "Option::is_none")]
    pub clip_url: Option<Url>,
    #[serde(rename = "thumbnailImageUrl", default, skip_serializing_if = "Option::is_none")]
    pub thumbnail_image_url: Option<Url>,
    pub duration: i64,
    /// 조회수 (clipDetail API 에서는 반환되지 않으므로 optional)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_count: Option<i64>,
    #[serde(default, with = "chzzk_date_opt", skip_serializing_if = "Option::is_none")]
    pub created_date: Option<DateTime<Utc>>,
    /// 선택적 필드 (채널 정보는 여기서 가져와야 함)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optional_property: Option<ClipOptionalProperty>,
    /// 클립 재생 URL — videoPlayUrl 우선, videoUrl 폴백
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_play_url: Option<String>,
    /// 클립 비디오 URL (구버전/폴백)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_url: Option<String>,
    /// VOD 상태 ("ABR_HLS" 등 — WebView fallback 필요 여부 판단)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vod_status: Option<String>,
    /// VOD ID (ABR_HLS 타입에서 embed 재생에 필요)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_id: Option<String>,
}

impl ClipDetail {
    pub fn new(clip_uid: impl Into<String>, clip_title: impl Into<String>) -> Self {
        Self {
            clip_uid: clip_uid.into(),
            clip_title: clip_title.into(),
            clip_url: None,
            thumbnail_image_url: None,
            duration: 0,
            read_count: None,
            created_date: None,
            optional_property: None,
            video_play_url: None,
            video_url: None,
            vod_status: None,
            video_id: None,
        }
    }

    /// 채널 정보 (`optionalProperty.ownerChannel` 우선, `makerChannel` 폴백).
    pub fn channel(&self) -> Option<&ChannelInfo> {
        let p = self.optional_property.as_ref()?;
        p.owner_channel.as_ref().or(p.maker_channel.as_ref())
    }

    /// 최적 재생 URL 반환 (`videoPlayUrl` → `videoUrl` 우선순위).
    pub fn best_playback_url(&self) -> Option<Url> {
        [&self.video_play_url, &self.video_url]
            .into_iter()
            .flatten()
            .filter(|s| !s.is_empty())
            .find_map(|s| Url::parse(s).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn best_hls_url_prefers_adhls() {
        let r: NaverVodPlayResponse = serde_json::from_str(
            r#"{"videos":{"list":[{"encodedType":"MP4","source":"a.mp4"},{"encodedType":"ADHLS","masterPlaylistUrl":"b.m3u8"}]}}"#,
        )
        .unwrap();
        assert_eq!(r.best_hls_url().as_deref(), Some("b.m3u8"));
        let err: NaverVodPlayResponse = serde_json::from_str(r#"{"errorCode":"E1"}"#).unwrap();
        assert_eq!(err.best_hls_url(), None);
        let first: NaverVodPlayResponse =
            serde_json::from_str(r#"{"videos":{"list":[{"source":"s.m3u8"}]}}"#).unwrap();
        assert_eq!(first.best_hls_url().as_deref(), Some("s.m3u8"));
    }

    #[test]
    fn clip_detail_decoding() {
        let d: ClipDetail = serde_json::from_str(
            r#"{"clipUID":"u","clipTitle":"t","duration":30,"createdDate":"2024-06-15 18:30:00",
                "optionalProperty":{"makerChannel":{"channelId":"m","channelName":"M"}},
                "videoPlayUrl":"","videoUrl":"https://v.example/x.m3u8"}"#,
        )
        .unwrap();
        assert_eq!(d.channel().unwrap().channel_id, "m");
        assert_eq!(d.best_playback_url().unwrap().as_str(), "https://v.example/x.m3u8");
        assert!(d.created_date.is_some());
        assert!(serde_json::from_str::<ClipDetail>(r#"{"clipUID":"u","clipTitle":"t"}"#).is_err());
    }
}
