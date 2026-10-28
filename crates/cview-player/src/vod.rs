//! `Sources/CViewPlayer/VODStreamResolver.swift` 포팅 — VOD 재생 URL 해석.
//!
//! `liveRewindPlaybackJson`(HLS 매니페스트·화질 트랙)을 먼저 보고, 실패하면 `vodUrl` 로 폴백한다.
//! 두 경로 모두 `inKey` 가 있으면 쿼리에 붙인다.
//!
//! 둘 다 없는 일반 업로드 VOD(`vodStatus = ABR_HLS`)는 `videoId`·`inKey` 로 네이버 플레이어 재생 정보
//! (`neonplayer/vodplay/v2/playback`, DASH MPD 를 JSON 으로 편 것)를 받아 HLS 마스터와 화질별 HLS,
//! 다운로드용 단일 MP4(PD) 주소를 꺼낸다. Swift 원본에는 없는 경로다(원본은 이런 VOD 를 못 연다).

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

    /// 네이버 플레이어 재생 정보 JSON 원문. 기본은 "없음".
    async fn neon_playback(&self, _video_id: &str, _in_key: &str) -> Result<String, ApiError> {
        Err(ApiError::EmptyContent)
    }
}

/// 네이버 플레이어 재생 정보 주소.
pub fn neon_playback_url(video_id: &str, in_key: &str) -> Option<Url> {
    let mut url = Url::parse("https://apis.naver.com/neonplayer/vodplay/v2/playback/").ok()?;
    url.path_segments_mut().ok()?.pop_if_empty().push(video_id);
    url.query_pairs_mut().append_pair("key", in_key);
    Some(url)
}

#[async_trait]
impl VodDetailSource for ChzzkApiClient {
    async fn vod_detail(&self, video_no: i64) -> Result<VodDetail, ApiError> {
        ChzzkApiClient::vod_detail(self, video_no).await
    }

    async fn neon_playback(&self, video_id: &str, in_key: &str) -> Result<String, ApiError> {
        let url = neon_playback_url(video_id, in_key).ok_or(ApiError::InvalidResponse)?;
        let resp = reqwest::Client::new()
            .get(url)
            .header("Accept", "application/json")
            .header(
                "User-Agent",
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)",
            )
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(|e| ApiError::NetworkError(e.to_string()))?;
        let status = resp.status().as_u16() as i64;
        if !(200..300).contains(&status) {
            return Err(ApiError::HttpError {
                status_code: status,
            });
        }
        resp.text()
            .await
            .map_err(|e| ApiError::NetworkError(e.to_string()))
    }
}

/// 재생 정보 + 다운로드용 단일 MP4 화질(높은 순). PD 가 없는 VOD 는 비어 있다.
#[derive(Debug, Clone)]
pub struct VodSources {
    pub info: VodStreamInfo,
    pub progressive: Vec<VodQuality>,
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
        Ok(self.resolve_sources(video_no).await?.info)
    }

    /// 재생 정보와 다운로드용 MP4 를 함께 해석한다.
    pub async fn resolve_sources(&self, video_no: i64) -> Result<VodSources, VodResolveError> {
        let detail = self
            .source
            .vod_detail(video_no)
            .await
            .map_err(|e| VodResolveError::NetworkError(e.to_string()))?;
        let direct = resolve_from_detail(&detail);
        let (Some(video_id), Some(in_key)) = (detail.video_id.as_deref(), detail.in_key.as_deref())
        else {
            return direct.map(|info| VodSources {
                info,
                progressive: Vec::new(),
            });
        };
        if direct.is_ok() && detail.live_rewind_playback_json.is_some() {
            return direct.map(|info| VodSources {
                info,
                progressive: Vec::new(),
            });
        }
        match self.source.neon_playback(video_id, in_key).await {
            Ok(json) => match parse_neon_playback(&json, &detail) {
                Ok(sources) => Ok(sources),
                Err(e) => direct
                    .map(|info| VodSources {
                        info,
                        progressive: Vec::new(),
                    })
                    .map_err(|_| e),
            },
            Err(e) => direct
                .map(|info| VodSources {
                    info,
                    progressive: Vec::new(),
                })
                .map_err(|_| VodResolveError::NetworkError(e.to_string())),
        }
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
                            .map_or_else(
                                || uuid::Uuid::new_v4().to_string().to_uppercase(),
                                str::to_string,
                            ),
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

fn rep_quality(rep: &Map<String, Value>, url: Url) -> VodQuality {
    let width = int(rep, "width").unwrap_or(0);
    let height = int(rep, "height").unwrap_or(0);
    VodQuality {
        id: rep
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        name: if height > 0 {
            format!("{height}p")
        } else {
            "트랙".into()
        },
        resolution: format!("{width}x{height}"),
        bandwidth: int(rep, "bandwidth").unwrap_or(0),
        url,
    }
}

/// 네이버 플레이어 재생 정보(JSON 으로 편 DASH MPD) 파싱.
/// `video/mp2t` 적응 집합의 `otherAttributes.m3u` 가 HLS 마스터, 각 표현의 `m3u` 가 화질별 HLS 이고,
/// `video/mp4` 집합의 `baseURL` 은 단일 MP4(다운로드용)다.
pub fn parse_neon_streams(json: &str) -> Result<NeonStreams, VodResolveError> {
    let root: Value =
        serde_json::from_str(json).map_err(|_| VodResolveError::InvalidPlaybackJson)?;
    let sets: Vec<&Map<String, Value>> = root
        .pointer("/period/0/adaptationSet")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_object).collect())
        .unwrap_or_default();
    let reps = |set: &Map<String, Value>| -> Vec<Map<String, Value>> {
        set.get("representation")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_object).cloned().collect())
            .unwrap_or_default()
    };
    let mime = |set: &Map<String, Value>| {
        set.get("mimeType")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let m3u = |m: &Map<String, Value>| {
        m.get("otherAttributes")
            .and_then(|o| o.get("m3u"))
            .and_then(Value::as_str)
            .and_then(|s| Url::parse(s).ok())
    };
    let base_url = |m: &Map<String, Value>| {
        m.get("baseURL")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(|b| b.get("value"))
            .and_then(Value::as_str)
            .and_then(|s| Url::parse(s).ok())
    };

    let hls_set = sets.iter().find(|s| mime(s) == "video/mp2t");
    let mut qualities: Vec<VodQuality> = hls_set
        .map(|set| {
            reps(set)
                .iter()
                .filter_map(|r| m3u(r).map(|u| rep_quality(r, u)))
                .collect()
        })
        .unwrap_or_default();
    qualities.sort_by_key(|q| std::cmp::Reverse(q.bandwidth));
    let master = hls_set
        .and_then(|s| m3u(s))
        .or_else(|| qualities.first().map(|q| q.url.clone()))
        .ok_or(VodResolveError::NoPlayableUrl)?;

    let mut progressive: Vec<VodQuality> = sets
        .iter()
        .filter(|s| mime(s) == "video/mp4")
        .flat_map(|s| reps(s))
        .filter_map(|r| base_url(&r).map(|u| rep_quality(&r, u)))
        .collect();
    progressive.sort_by_key(|q| std::cmp::Reverse(q.bandwidth));

    Ok(NeonStreams {
        master,
        hls: qualities,
        progressive,
    })
}

/// [`parse_neon_streams`] 결과를 VOD 상세와 합친다.
/// 재생은 단일 MP4 를 먼저 쓴다 — 이 CDN 의 HLS 는 하위 재생목록·세그먼트에 마스터의 서명 쿼리(`_lsu_sa_`)를
/// 이어 붙여야 해서 WebView 기본 HLS 가 열지 못한다(MEDIA_ERR_SRC_NOT_SUPPORTED). MP4 는 범위 요청으로 탐색된다.
pub fn parse_neon_playback(json: &str, detail: &VodDetail) -> Result<VodSources, VodResolveError> {
    let n = parse_neon_streams(json)?;
    let info = match n.progressive.first() {
        Some(best) => base_info(detail, best.url.clone(), n.progressive.clone()),
        None => base_info(detail, n.master, n.hls),
    };
    Ok(VodSources {
        info,
        progressive: n.progressive,
    })
}

/// 네이버 플레이어 재생 정보에서 꺼낸 주소들.
#[derive(Debug, Clone)]
pub struct NeonStreams {
    /// HLS 마스터(없으면 가장 높은 화질 HLS).
    pub master: Url,
    /// 화질별 HLS(높은 순).
    pub hls: Vec<VodQuality>,
    /// 단일 MP4(높은 순).
    pub progressive: Vec<VodQuality>,
}

/// 클립 재생 정보 — `play-info/clip/{uid}` 로 `videoId`·`inKey` 를 받아 네이버 플레이어 재생 정보를 푼다.
pub async fn resolve_clip_streams(clip_uid: &str) -> Result<NeonStreams, VodResolveError> {
    let net = |e: reqwest::Error| VodResolveError::NetworkError(e.to_string());
    let mut url = Url::parse("https://api.chzzk.naver.com/service/v1/play-info/clip/")
        .map_err(|_| VodResolveError::NoPlayableUrl)?;
    url.path_segments_mut()
        .map_err(|_| VodResolveError::NoPlayableUrl)?
        .pop_if_empty()
        .push(clip_uid);
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)")
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(net)?;
    let info: Value = client
        .get(url)
        .send()
        .await
        .map_err(net)?
        .json()
        .await
        .map_err(net)?;
    let content = info.get("content").ok_or(VodResolveError::NoPlayableUrl)?;
    let video_id = content.get("videoId").and_then(Value::as_str);
    let in_key = content.get("inKey").and_then(Value::as_str);
    let (Some(video_id), Some(in_key)) = (video_id, in_key) else {
        return Err(VodResolveError::NoPlayableUrl);
    };
    let neon = neon_playback_url(video_id, in_key).ok_or(VodResolveError::NoPlayableUrl)?;
    let text = client
        .get(neon)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(net)?
        .text()
        .await
        .map_err(net)?;
    parse_neon_streams(&text)
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

    const NEON: &str = r#"{"period":[{"adaptationSet":[
      {"mimeType":"video/mp4","representation":[
        {"id":"PD_144","width":256,"height":144,"bandwidth":162000,"baseURL":[{"value":"https://cdn.example.com/144.mp4"}]},
        {"id":"PD_1080","width":1920,"height":1080,"bandwidth":8105000,"baseURL":[{"value":"https://cdn.example.com/1080.mp4"}]}]},
      {"mimeType":"video/mp2t","otherAttributes":{"m3u":"https://cdn.example.com/master.m3u8?s=1"},"representation":[
        {"id":"a","width":1280,"height":720,"bandwidth":3170000,"otherAttributes":{"m3u":"https://cdn.example.com/720.m3u8"}},
        {"id":"b","width":1920,"height":1080,"bandwidth":8105000,"otherAttributes":{"m3u":"https://cdn.example.com/1080.m3u8"}}]},
      {"mimeType":"audio/mp4","representation":[{"id":"aud","bandwidth":128000}]}]}]}"#;

    #[test]
    fn parses_neon_playback_hls_and_progressive() {
        let d = detail(None, None, Some("k"));
        let n = parse_neon_streams(NEON).unwrap();
        assert_eq!(n.master.as_str(), "https://cdn.example.com/master.m3u8?s=1");
        assert_eq!(
            n.hls.iter().map(|q| q.name.as_str()).collect::<Vec<_>>(),
            ["1080p", "720p"]
        );
        let s = parse_neon_playback(NEON, &d).unwrap();
        // 재생은 단일 MP4 우선.
        assert_eq!(
            s.info.stream_url.as_str(),
            "https://cdn.example.com/1080.mp4"
        );
        assert_eq!(
            s.info
                .qualities
                .iter()
                .map(|q| q.name.as_str())
                .collect::<Vec<_>>(),
            ["1080p", "144p"]
        );
        assert_eq!(s.progressive.len(), 2);
        assert_eq!(
            s.progressive[0].url.as_str(),
            "https://cdn.example.com/1080.mp4"
        );
        assert!(parse_neon_playback("{}", &d).is_err());
    }

    #[test]
    fn neon_url_escapes_id_and_key() {
        let u = neon_playback_url("AB/C", "k y").unwrap();
        assert_eq!(
            u.as_str(),
            "https://apis.naver.com/neonplayer/vodplay/v2/playback/AB%2FC?key=k+y"
        );
    }

    struct NeonSource;

    #[async_trait]
    impl VodDetailSource for NeonSource {
        async fn vod_detail(&self, _: i64) -> Result<VodDetail, ApiError> {
            let mut d = detail(None, None, Some("key"));
            d.video_id = Some("VID".into());
            Ok(d)
        }
        async fn neon_playback(&self, id: &str, key: &str) -> Result<String, ApiError> {
            assert_eq!((id, key), ("VID", "key"));
            Ok(NEON.into())
        }
    }

    #[tokio::test]
    async fn resolver_falls_back_to_neon_playback() {
        let r = VodStreamResolver::new(NeonSource);
        let s = r.resolve_sources(1).await.unwrap();
        assert_eq!(s.info.qualities.len(), 2);
        assert_eq!(s.progressive.len(), 2);
    }
}
