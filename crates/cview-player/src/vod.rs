//! `Sources/CViewPlayer/VODStreamResolver.swift` 포팅 — VOD 재생 URL 해석.
//!
//! `liveRewindPlaybackJson`(HLS 매니페스트·화질 트랙)을 먼저 보고, 실패하면 `vodUrl` 로 폴백한다.
//! 두 경로 모두 `inKey` 가 있으면 쿼리에 붙인다.

use std::time::Duration;

use async_trait::async_trait;
use cview_core::error::ApiError;
use cview_core::models::vod::{VodDetail, VodQuality, VodStreamInfo};
use cview_networking::client::ChzzkApiClient;
use serde_json::{Map, Value};
use url::Url;

/// VOD 해석 오류.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VodResolveError {
    #[error("재생 가능한 URL을 찾을 수 없습니다")]
    NoPlayableUrl,
    #[error("재생 정보를 파싱할 수 없습니다")]
    InvalidPlaybackJson,
    #[error("네트워크 오류: {0}")]
    NetworkError(String),
}

/// VOD 상세를 주는 쪽(테스트에서는 모의 구현).
#[async_trait]
pub trait VodDetailSource: Send + Sync {
    async fn vod_detail(&self, video_no: i64) -> Result<VodDetail, ApiError>;
}

#[async_trait]
impl VodDetailSource for ChzzkApiClient {
    async fn vod_detail(&self, video_no: i64) -> Result<VodDetail, ApiError> {
        ChzzkApiClient::vod_detail(self, video_no).await
    }
}

/// VOD/Clip 스트림 URL 해석기.
pub struct VodStreamResolver<S: VodDetailSource> {
    source: S,
}

impl<S: VodDetailSource> VodStreamResolver<S> {
    pub fn new(source: S) -> Self {
        Self { source }
    }

    /// VOD 상세 정보에서 스트림 정보 해석.
    pub async fn resolve_vod(&self, video_no: i64) -> Result<VodStreamInfo, VodResolveError> {
        let detail = self
            .source
            .vod_detail(video_no)
            .await
            .map_err(|e| VodResolveError::NetworkError(e.to_string()))?;
        resolve_from_detail(&detail)
    }
}

/// `inKey` 쿼리를 덧붙인다.
fn with_in_key(url: &Url, in_key: Option<&str>) -> Url {
    let mut out = url.clone();
    if let Some(key) = in_key {
        out.query_pairs_mut().append_pair("inKey", key);
    }
    out
}

fn base_info(detail: &VodDetail, stream_url: Url, qualities: Vec<VodQuality>) -> VodStreamInfo {
    VodStreamInfo {
        video_no: detail.video_no,
        title: detail.video_title.clone(),
        stream_url,
        duration: Duration::from_secs(detail.duration.max(0) as u64),
        qualities,
        channel_name: detail
            .channel
            .as_ref()
            .map(|c| c.channel_name.clone())
            .unwrap_or_default(),
        thumbnail_url_string: detail
            .video_image_url
            .as_ref()
            .or(detail.thumbnail_image_url.as_ref())
            .map(|u| u.to_string()),
        channel_id: detail.channel.as_ref().map(|c| c.channel_id.clone()),
        read_count: detail.read_count,
        publish_date: detail.publish_date,
        thumbnail_url: detail
            .thumbnail_image_url
            .clone()
            .or_else(|| detail.video_image_url.clone()),
    }
}

/// VodDetail 에서 스트림 URL 추출: playbackJson → vodUrl.
pub fn resolve_from_detail(detail: &VodDetail) -> Result<VodStreamInfo, VodResolveError> {
    if let Some(json) = &detail.live_rewind_playback_json {
        match parse_playback_json(json, detail) {
            Ok(info) => return Ok(info),
            Err(e) => tracing::warn!("VOD playbackJson 파싱 실패 (vodUrl 폴백): {e}"),
        }
    }
    if let Some(url) = detail.vod_url.as_deref().and_then(|s| Url::parse(s).ok()) {
        return Ok(base_info(
            detail,
            with_in_key(&url, detail.in_key.as_deref()),
            Vec::new(),
        ));
    }
    Err(VodResolveError::NoPlayableUrl)
}

fn int(track: &Map<String, Value>, key: &str) -> Option<i64> {
    let n = track.get(key)?.as_number()?;
    n.as_i64()
        .or_else(|| n.as_f64().filter(|v| v.fract() == 0.0).map(|v| v as i64))
}

/// `liveRewindPlaybackJson` 파싱 — `{ "media": [{ "path", "encodingTrack": [...] }] }`.
pub fn parse_playback_json(
    json: &str,
    detail: &VodDetail,
) -> Result<VodStreamInfo, VodResolveError> {
    let root: Value =
        serde_json::from_str(json).map_err(|_| VodResolveError::InvalidPlaybackJson)?;
    let root = root
        .as_object()
        .ok_or(VodResolveError::InvalidPlaybackJson)?;
    let first = root
        .get("media")
        .and_then(Value::as_array)
        .filter(|a| a.iter().all(Value::is_object))
        .and_then(|a| a.first())
        .and_then(Value::as_object)
        .ok_or(VodResolveError::NoPlayableUrl)?;
    let media_url = first
        .get("path")
        .and_then(Value::as_str)
        .and_then(|p| Url::parse(p).ok())
        .ok_or(VodResolveError::NoPlayableUrl)?;

    let mut qualities: Vec<VodQuality> = first
        .get("encodingTrack")
        .and_then(Value::as_array)
        .filter(|a| a.iter().all(Value::is_object))
        .map(|tracks| {
            tracks
                .iter()
                .filter_map(Value::as_object)
                .map(|track| {
                    let width = int(track, "videoWidth").unwrap_or(0);
                    let height = int(track, "videoHeight").unwrap_or(0);
                    let bitrate = int(track, "videoBitRate")
                        .or_else(|| int(track, "audioBitRate"))
                        .unwrap_or(0);
                    // 화질 라벨: encodingTrackName 우선, 없으면 높이로 "1080p".
                    let raw_name = track
                        .get("encodingTrackName")
                        .and_then(Value::as_str)
                        .map(|s| s.trim_matches(' ').to_string());
                    let name = match raw_name {
                        Some(n) if !n.is_empty() && n.to_lowercase() != "unknown" => n,
                        _ if height > 0 => format!("{height}p"),
                        _ => "트랙".to_string(),
                    };
                    let url = track
                        .get("path")
                        .and_then(Value::as_str)
                        .and_then(|p| Url::parse(p).ok())
                        .unwrap_or_else(|| media_url.clone());
                    VodQuality {
                        id: track
                            .get("encodingTrackId")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string().to_uppercase()),
                        name,
                        resolution: format!("{width}x{height}"),
                        bandwidth: bitrate,
                        url,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    qualities.sort_by_key(|q| std::cmp::Reverse(q.bandwidth));

    Ok(base_info(
        detail,
        with_in_key(&media_url, detail.in_key.as_deref()),
        qualities,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detail(json: Option<&str>, vod_url: Option<&str>, in_key: Option<&str>) -> VodDetail {
        serde_json::from_value(serde_json::json!({
            "videoNo": 42,
            "videoTitle": "다시보기",
            "duration": 3600,
            "readCount": 7,
            "videoImageUrl": "https://img.example.com/v.jpg",
            "liveRewindPlaybackJson": json,
            "vodUrl": vod_url,
            "inKey": in_key,
        }))
        .unwrap()
    }

    const PLAYBACK: &str = r#"{"media":[{"mediaId":"HLS","protocol":"HLS",
        "path":"https://vod.navercdn.com/hls/master.m3u8?a=1",
        "encodingTrack":[
          {"encodingTrackId":"480p","videoWidth":854,"videoHeight":480,"videoBitRate":1200000},
          {"encodingTrackId":"1080p","encodingTrackName":" unknown ","videoWidth":1920,"videoHeight":1080,"videoBitRate":8000000,
           "path":"https://vod.navercdn.com/hls/1080p.m3u8"},
          {"encodingTrackId":"audio","audioBitRate":192000}
        ]}]}"#;

    #[test]
    fn resolves_playback_json_with_qualities_and_in_key() {
        let info = resolve_from_detail(&detail(Some(PLAYBACK), None, Some("k y"))).unwrap();
        assert_eq!(info.video_no, 42);
        assert_eq!(info.duration, Duration::from_secs(3600));
        assert_eq!(
            info.stream_url.as_str(),
            "https://vod.navercdn.com/hls/master.m3u8?a=1&inKey=k+y"
        );
        let names: Vec<_> = info.qualities.iter().map(|q| q.name.as_str()).collect();
        assert_eq!(names, vec!["1080p", "480p", "트랙"], "bandwidth 내림차순");
        assert_eq!(
            info.qualities[0].url.as_str(),
            "https://vod.navercdn.com/hls/1080p.m3u8"
        );
        assert_eq!(
            info.qualities[1].url.as_str(),
            "https://vod.navercdn.com/hls/master.m3u8?a=1"
        );
        assert_eq!(info.qualities[2].bandwidth, 192_000);
        assert_eq!(info.qualities[2].resolution, "0x0");
        assert_eq!(
            info.thumbnail_url_string.as_deref(),
            Some("https://img.example.com/v.jpg")
        );
    }

    #[test]
    fn falls_back_to_vod_url_then_fails() {
        let info = resolve_from_detail(&detail(
            Some("{not json"),
            Some("https://vod.navercdn.com/a.m3u8"),
            None,
        ))
        .unwrap();
        assert_eq!(info.stream_url.as_str(), "https://vod.navercdn.com/a.m3u8");
        assert!(info.qualities.is_empty());
        assert_eq!(
            resolve_from_detail(&detail(Some(r#"{"media":[]}"#), None, None)),
            Err(VodResolveError::NoPlayableUrl)
        );
        assert_eq!(
            parse_playback_json("[]", &detail(None, None, None)),
            Err(VodResolveError::InvalidPlaybackJson)
        );
    }

    struct Source(Result<VodDetail, ApiError>);

    #[async_trait]
    impl VodDetailSource for Source {
        async fn vod_detail(&self, _: i64) -> Result<VodDetail, ApiError> {
            self.0.clone()
        }
    }

    #[tokio::test]
    async fn resolver_maps_api_errors() {
        let ok = VodStreamResolver::new(Source(Ok(detail(Some(PLAYBACK), None, None))));
        assert_eq!(ok.resolve_vod(42).await.unwrap().qualities.len(), 3);
        let err = VodStreamResolver::new(Source(Err(ApiError::NetworkError("down".into()))));
        assert!(matches!(
            err.resolve_vod(42).await,
            Err(VodResolveError::NetworkError(_))
        ));
    }
}
