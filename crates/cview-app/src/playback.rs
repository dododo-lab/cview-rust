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
    /// P2P 전용(`p2pPath`) 트랙이 있는 방송이면 커넥터 없이 볼 수 있는 최고 높이. 없으면 `None`.
    pub p2p_general_max_height: Option<i64>,
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

/// 커넥터 없이 볼 수 있는 화질 상한 — 치지직 웹 플레이어와 같은 판정이다.
///
/// 웹은 LLHLS media 에 `p2pPath` 트랙이 있으면 LLHLS 를, 아니면 HLS media 를 본다. 그 트랙 목록에서
/// `p2pPath` 가 있는 트랙은 네이버 라이브 스트리밍 커넥터(P2P)로만 재생하고, 나머지 중 가장 큰 트랙이
/// 커넥터 없는 시청자의 최고 화질이다. P2P 트랙이 없으면 제한이 없다(`None`).
pub fn p2p_general_max_height(json: &str) -> Option<i64> {
    let playback = decode(json).ok()?;
    let has_p2p = |m: &MediaInfo| {
        m.encoding_track
            .iter()
            .flatten()
            .any(|t| t.p2p_path.is_some())
    };
    let by_id = |id: &str| {
        playback
            .media
            .iter()
            .find(|m| m.media_id.eq_ignore_ascii_case(id))
    };
    let media = by_id("LLHLS")
        .filter(|m| has_p2p(m))
        .or_else(|| by_id("HLS"))?;
    let tracks = media.encoding_track.as_deref().unwrap_or_default();
    if !tracks.iter().any(|t| t.p2p_path.is_some()) {
        return None;
    }
    let general = tracks
        .iter()
        .filter(|t| t.p2p_path.is_none())
        .filter_map(|t| match (t.video_width, t.video_height) {
            (Some(w), Some(h)) if w > 0 && h > 0 => Some((w.min(h), h)),
            _ => None,
        })
        .max_by_key(|(scanline, _)| *scanline);
    // 일반 트랙이 하나도 없는 드문 응답은 웹의 기본 상한(720p)으로 둔다.
    Some(general.map_or(720, |(_, h)| h))
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
            p2p_general_max_height: p2p_general_max_height(&json),
        });
    }
    if let Some(json) = non_empty(&info.radio_mode_playback_json) {
        return Ok(PlaybackPlan {
            stream_url: media_url(&json, prefer_std)?,
            is_audio_only: true,
            requires_authenticated_key: requires_authenticated_key(Some(&json)),
            p2p_general_max_height: None,
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

    #[test]
    fn p2p_tracks_cap_to_general_max() {
        let json = r#"{"media":[
            {"mediaId":"HLS","path":"https://a/hls.m3u8","encodingTrack":[
                {"encodingTrackId":"360p","videoWidth":640,"videoHeight":360},
                {"encodingTrackId":"720p","videoWidth":1280,"videoHeight":720},
                {"encodingTrackId":"1080p","videoWidth":1920,"videoHeight":1080,"p2pPath":"/p2p/1080p"}]},
            {"mediaId":"LLHLS","path":"https://a/ll.m3u8","encodingTrack":[
                {"encodingTrackId":"720p","videoWidth":1280,"videoHeight":720},
                {"encodingTrackId":"1080p","videoWidth":1920,"videoHeight":1080}]}
        ]}"#;
        // LLHLS 에는 P2P 트랙이 없어 웹처럼 HLS 트랙 목록으로 판정한다.
        assert_eq!(p2p_general_max_height(json), Some(720));
        assert_eq!(p2p_general_max_height(PLAYBACK), None);
        let vertical = r#"{"media":[{"mediaId":"HLS","path":"https://a/h.m3u8","encodingTrack":[
            {"encodingTrackId":"720p","videoWidth":720,"videoHeight":1280},
            {"encodingTrackId":"1080p","videoWidth":1080,"videoHeight":1920,"p2pPath":"/p"}]}]}"#;
        assert_eq!(p2p_general_max_height(vertical), Some(1280));
        let only_p2p = r#"{"media":[{"mediaId":"HLS","path":"https://a/h.m3u8","encodingTrack":[
            {"encodingTrackId":"1080p","videoWidth":1920,"videoHeight":1080,"p2pPath":"/p"}]}]}"#;
        assert_eq!(p2p_general_max_height(only_p2p), Some(720));
        assert_eq!(p2p_general_max_height("nope"), None);
    }
}
