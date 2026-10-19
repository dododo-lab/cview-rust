//! `Sources/CViewCore/Models/MediaInfo.swift` · `EncodingTrack.swift` 포팅 — HLS 미디어/인코딩 트랙.
//!
//! `f64` 필드(`video_frame_rate`) 때문에 `Eq`/`Hash` 는 파생하지 않는다(Swift 는 `Double` 도 Hashable).

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::decode::LenientObject;

/// 미디어 정보 (HLS 스트림).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
    pub media_id: String,
    #[serde(rename = "protocol", default, skip_serializing_if = "Option::is_none")]
    pub media_protocol: Option<String>,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoding_track: Option<Vec<EncodingTrack>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aes: Option<String>,
}

impl MediaInfo {
    pub fn new(media_id: impl Into<String>, media_protocol: Option<String>, path: impl Into<String>) -> Self {
        Self {
            media_id: media_id.into(),
            media_protocol,
            path: path.into(),
            encoding_track: None,
            aes: None,
        }
    }
}

/// 인코딩 트랙 정보.
///
/// API 가 `videoFrameRate` 를 문자열(`"60.0"`)로 반환하기도 하므로 커스텀 디코더가 필요하다.
/// 모든 필드는 관대하게 디코딩된다(`encodingTrackId` 누락 시 빈 문자열).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncodingTrack {
    pub encoding_track_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_codec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_bit_rate: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_bit_rate: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_frame_rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_width: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_height: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_channel: Option<i64>,
}

impl EncodingTrack {
    pub fn new(encoding_track_id: impl Into<String>) -> Self {
        Self { encoding_track_id: encoding_track_id.into(), ..Self::default() }
    }
}

impl<'de> Deserialize<'de> for EncodingTrack {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let o = LenientObject::from_deserializer(d)?;
        // videoFrameRate: Double 또는 String("60.0") 모두 처리
        let video_frame_rate = match o.raw("videoFrameRate") {
            Some(Value::Number(n)) => n.as_f64(),
            Some(Value::String(s)) => match s.trim().parse::<f64>() {
                Ok(v) => Some(v),
                Err(_) => {
                    tracing::debug!("EncodingTrack: invalid videoFrameRate string '{s}'");
                    None
                }
            },
            _ => None,
        };
        Ok(Self {
            encoding_track_id: o.get("encodingTrackId").unwrap_or_default(),
            video_profile: o.get("videoProfile"),
            audio_profile: o.get("audioProfile"),
            video_codec: o.get("videoCodec"),
            video_bit_rate: o.get("videoBitRate"),
            audio_bit_rate: o.get("audioBitRate"),
            video_frame_rate,
            video_width: o.get("videoWidth"),
            video_height: o.get("videoHeight"),
            audio_channel: o.get("audioChannel"),
        })
    }
}

/// 라이브 재생 상세.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LivePlaybackDetail {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_machine: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[cfg(test)]
mod tests {
    //! `Tests/CViewCoreTests/LiveInfoTests.swift` 의 EncodingTrack / MediaInfo / LivePlaybackDetail 부분.
    use super::*;

    #[test]
    fn frame_rate_as_double() {
        let t: EncodingTrack = serde_json::from_str(
            r#"{"encodingTrackId":"track1","videoFrameRate":60.0,"videoWidth":1920,"videoHeight":1080}"#,
        )
        .unwrap();
        assert_eq!(t.encoding_track_id, "track1");
        assert_eq!(t.video_frame_rate, Some(60.0));
        assert_eq!(t.video_width, Some(1920));
        assert_eq!(t.video_height, Some(1080));
    }

    #[test]
    fn frame_rate_as_string() {
        let t: EncodingTrack = serde_json::from_str(r#"{"encodingTrackId":"track2","videoFrameRate":"30.0"}"#).unwrap();
        assert_eq!(t.video_frame_rate, Some(30.0));
    }

    #[test]
    fn frame_rate_missing_or_invalid() {
        let t: EncodingTrack = serde_json::from_str(r#"{"encodingTrackId":"track3"}"#).unwrap();
        assert_eq!(t.video_frame_rate, None);
        let bad: EncodingTrack = serde_json::from_str(r#"{"videoFrameRate":"fast"}"#).unwrap();
        assert_eq!(bad.video_frame_rate, None);
    }

    #[test]
    fn missing_track_id_is_empty() {
        let t: EncodingTrack = serde_json::from_str("{}").unwrap();
        assert_eq!(t.encoding_track_id, "");
    }

    #[test]
    fn full_decoding() {
        let t: EncodingTrack = serde_json::from_str(
            r#"{"encodingTrackId":"t1","videoProfile":"high","audioProfile":"aac","videoCodec":"h264",
                "videoBitRate":5000,"audioBitRate":128,"videoFrameRate":60.0,"videoWidth":1920,
                "videoHeight":1080,"audioChannel":2}"#,
        )
        .unwrap();
        assert_eq!(t.video_profile.as_deref(), Some("high"));
        assert_eq!(t.audio_profile.as_deref(), Some("aac"));
        assert_eq!(t.video_codec.as_deref(), Some("h264"));
        assert_eq!(t.video_bit_rate, Some(5000));
        assert_eq!(t.audio_bit_rate, Some(128));
        assert_eq!(t.audio_channel, Some(2));
    }

    #[test]
    fn media_protocol_key_mapping() {
        let m: MediaInfo = serde_json::from_str(r#"{"mediaId":"m1","protocol":"HLS","path":"/stream.m3u8"}"#).unwrap();
        assert_eq!(m.media_protocol.as_deref(), Some("HLS"));
        assert_eq!(m.path, "/stream.m3u8");
    }

    #[test]
    fn playback_detail_default() {
        let d = LivePlaybackDetail::default();
        assert!(d.start.is_none() && d.open.is_none() && d.time_machine.is_none() && d.status.is_none());
    }
}
