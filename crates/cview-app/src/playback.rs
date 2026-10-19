//! 라이브 재생 계획 — `Sources/CViewApp/Views/LiveStreamView+Logic.swift` 의 `PlaybackLaunchPlan.resolve`
//! 와 `PlaybackLaunchPlan+KeyAccess.swift` 의 순수 부분 포팅.
//!
//! `LiveInfo.livePlaybackJson` → 재생할 HLS media URL. 영상이 없고 라디오만 있으면 오디오 전용.

use cview_core::models::{LiveInfo, LivePlayback, MediaInfo};
use url::Url;

/// 재생 계획 실패.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlaybackPlanError {
    #[error("재생 정보가 없습니다 (방송이 종료되었거나 시청이 제한된 방송입니다)")]
    MissingPlaybackJson,
    #[error("재생 정보를 해석하지 못했습니다: {0}")]
    DecodeFailed(String),
    #[error("재생 가능한 HLS 주소가 없습니다")]
    NoHlsUrl,
}

/// 해석된 재생 계획.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackPlan {
    pub stream_url: Url,
    pub is_audio_only: bool,
    /// 암호화(`hls_aes`) 방송 — 키 요청이 인증 프록시를 거쳐야 한다.
    pub requires_authenticated_key: bool,
}

fn decode(json: &str) -> Result<LivePlayback, PlaybackPlanError> {
    serde_json::from_str(json).map_err(|e| PlaybackPlanError::DecodeFailed(e.to_string()))
}

/// media 선택. `prefer_standard_hls` 면 LL-HLS 대신 표준 HLS 우선(워치파티).
pub fn select_media(media: &[MediaInfo], prefer_standard_hls: bool) -> Option<&MediaInfo> {
    let by_id = |id: &str| media.iter().find(|m| m.media_id.eq_ignore_ascii_case(id));
    let by_protocol = |p: &str| {
        media.iter().find(|m| {
            m.media_protocol
                .as_deref()
                .is_some_and(|x| x.eq_ignore_ascii_case(p))
        })
    };
    if prefer_standard_hls {
        by_id("HLS")
            .or_else(|| by_protocol("HLS"))
            .or_else(|| by_id("LLHLS"))
            .or_else(|| media.first())
    } else {
        // 치지직 웹과 동일 — LLHLS 우선, 없으면 HLS protocol, 마지막으로 첫 media.
        by_id("LLHLS")
            .or_else(|| by_protocol("HLS"))
            .or_else(|| media.first())
    }
}

/// playback JSON 문자열 → media URL.
pub fn media_url(json: &str, prefer_standard_hls: bool) -> Result<Url, PlaybackPlanError> {
    let playback = decode(json)?;
    let media =
        select_media(&playback.media, prefer_standard_hls).ok_or(PlaybackPlanError::NoHlsUrl)?;
    Url::parse(&media.path).map_err(|_| PlaybackPlanError::NoHlsUrl)
}

/// playback JSON 에 `hls_aes` 암호화 media 가 있는가.
pub fn requires_authenticated_key(json: Option<&str>) -> bool {
    json.and_then(|j| decode(j).ok())
        .map(|p| {
            p.media.iter().any(|m| {
                m.aes
                    .as_deref()
                    .is_some_and(|a| a.eq_ignore_ascii_case("hls_aes"))
            })
        })
        .unwrap_or(false)
}

/// 라이브 상세 → 재생 계획.
pub fn resolve(info: &LiveInfo) -> Result<PlaybackPlan, PlaybackPlanError> {
    let prefer_std = info.is_watch_party();
    let non_empty = |s: &Option<String>| s.as_deref().filter(|s| !s.is_empty()).map(str::to_string);
    if let Some(json) = non_empty(&info.live_playback_json) {
        return Ok(PlaybackPlan {
            stream_url: media_url(&json, prefer_std)?,
            is_audio_only: false,
            requires_authenticated_key: requires_authenticated_key(Some(&json)),
        });
    }
    if let Some(json) = non_empty(&info.radio_mode_playback_json) {
        return Ok(PlaybackPlan {
            stream_url: media_url(&json, prefer_std)?,
            is_audio_only: true,
            requires_authenticated_key: requires_authenticated_key(Some(&json)),
        });
    }
    Err(PlaybackPlanError::MissingPlaybackJson)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYBACK: &str = r#"{"media":[
        {"mediaId":"HLS","protocol":"HLS","path":"https://cdn.example.com/hls/playlist.m3u8"},
        {"mediaId":"LLHLS","protocol":"HLS","path":"https://cdn.example.com/llhls/playlist.m3u8"}
    ]}"#;

    fn live(json: serde_json::Value) -> LiveInfo {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn prefers_llhls_by_default() {
        let url = media_url(PLAYBACK, false).unwrap();
        assert_eq!(url.path(), "/llhls/playlist.m3u8");
    }

    #[test]
    fn prefers_standard_hls_for_watch_party() {
        let url = media_url(PLAYBACK, true).unwrap();
        assert_eq!(url.path(), "/hls/playlist.m3u8");
    }

    #[test]
    fn empty_media_is_no_hls_url() {
        assert_eq!(
            media_url(r#"{"media":[]}"#, false),
            Err(PlaybackPlanError::NoHlsUrl)
        );
        assert!(matches!(
            media_url("not json", false),
            Err(PlaybackPlanError::DecodeFailed(_))
        ));
    }

    #[test]
    fn detects_encrypted_media() {
        let enc = r#"{"media":[{"mediaId":"HLS","path":"https://a/b.m3u8","aes":"HLS_AES"}]}"#;
        assert!(requires_authenticated_key(Some(enc)));
        assert!(!requires_authenticated_key(Some(PLAYBACK)));
        assert!(!requires_authenticated_key(None));
    }

    #[test]
    fn resolve_video_then_radio_fallback() {
        let video = live(serde_json::json!({
            "liveId": 1, "liveTitle": "t", "status": "OPEN", "concurrentUserCount": 3,
            "livePlaybackJson": PLAYBACK
        }));
        let plan = resolve(&video).unwrap();
        assert!(!plan.is_audio_only);
        assert_eq!(plan.stream_url.path(), "/llhls/playlist.m3u8");

        let radio = live(serde_json::json!({
            "liveId": 2, "liveTitle": "r", "status": "OPEN", "concurrentUserCount": 0,
            "livePlaybackJson": "",
            "radioModePlaybackJson": r#"{"media":[{"mediaId":"HLS","path":"https://r/a.m3u8"}]}"#
        }));
        let plan = resolve(&radio).unwrap();
        assert!(plan.is_audio_only);

        let none = live(serde_json::json!({"liveId": 3, "liveTitle": "x"}));
        assert_eq!(resolve(&none), Err(PlaybackPlanError::MissingPlaybackJson));
    }
}
