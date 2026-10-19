//! `Sources/CViewPlayer/HLSJSPlayerEngine.swift` 의 `HLSJSStreamingProfile`·`HLSJSForcedQualityTarget`과
//! `HLSJSPlayerEngine+MatchWebV2.swift` 의 `HLSJSMatchWebV2Config` 포팅 — 엔진 비의존 값 타입.

use cview_core::models::match_web_v2::MatchWebV2Flag;
use serde::{Deserialize, Serialize};

/// hls.js 시나리오별 프로파일 (`hlsjs-player.html PROFILES`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HlsJsStreamingProfile {
    /// 최저 지연 (hold-back 4s).
    UltraLow,
    /// 저지연 기본값 (hold-back 6s).
    LowLatency,
    /// 웹 기본 동작 근사 (hold-back 6s, 버퍼 30/60s).
    Mirror,
    /// chzzk 웹 재생 위치 미러 (hold-back 3.0s).
    WebParity,
}

impl HlsJsStreamingProfile {
    /// JS `PROFILES` 키.
    pub fn profile_key(self) -> &'static str {
        match self {
            Self::UltraLow => "ultraLow",
            Self::LowLatency => "lowLatency",
            Self::Mirror => "mirror",
            Self::WebParity => "webParity",
        }
    }

    /// `PROFILES.*.liveSyncDuration` 과 동기.
    pub fn target_live_latency_seconds(self) -> f64 {
        match self {
            Self::UltraLow => 4.0,
            Self::LowLatency | Self::Mirror => 6.0,
            Self::WebParity => 3.0,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::UltraLow => "초저지연",
            Self::LowLatency => "저지연 (기본)",
            Self::Mirror => "웹 기본 mirror",
            Self::WebParity => "웹 동등 (chzzk)",
        }
    }
}

/// hls.js 수동 레벨 고정 목표.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HlsJsForcedQualityTarget {
    pub height: i64,
    pub target_bitrate_kbps: i64,
}

impl HlsJsForcedQualityTarget {
    pub fn new(height: i64, target_bitrate_kbps: i64) -> Self {
        let height = height.max(0);
        Self {
            height,
            target_bitrate_kbps: if height > 0 {
                target_bitrate_kbps.max(0)
            } else {
                0
            },
        }
    }

    /// 멀티라이브 메인/PiP 가 유지할 CHZZK 원본급 목표.
    pub const MULTI_LIVE_1080P_8MBPS: Self = Self {
        height: 1080,
        target_bitrate_kbps: 8_000,
    };
}

/// `setMatchWebV2Config(<리터럴>)` 로 JS 에 싣는 v2 설정.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HlsJsMatchWebV2Config {
    pub hls_target_seconds: f64,
    pub ingest_target_seconds: f64,
    pub live_max_latency_seconds: f64,
    pub seek_token_required: bool,
}

impl Default for HlsJsMatchWebV2Config {
    fn default() -> Self {
        Self::new(MatchWebV2Flag::DEFAULT_INGEST_TARGET_SECONDS)
    }
}

impl HlsJsMatchWebV2Config {
    pub fn new(ingest_target_seconds: f64) -> Self {
        Self {
            hls_target_seconds: 2.0,
            ingest_target_seconds,
            live_max_latency_seconds: 16.0,
            seek_token_required: true,
        }
    }

    /// 정렬된 키 6개의 JSON 객체 리터럴. 비유한 값은 기본값으로 바꿔 싣는다.
    pub fn js_object_literal(&self, session_id: u64) -> String {
        fn finite(v: f64, fallback: f64) -> f64 {
            if v.is_finite() {
                v
            } else {
                fallback
            }
        }
        // serde_json::Map 은 기본(BTreeMap)으로 키를 정렬한다.
        let mut map = serde_json::Map::new();
        map.insert("enabled".into(), true.into());
        map.insert(
            "hlsTargetSec".into(),
            json_number(finite(self.hls_target_seconds, 2.0)),
        );
        map.insert(
            "ingestTargetSec".into(),
            json_number(finite(
                self.ingest_target_seconds,
                MatchWebV2Flag::DEFAULT_INGEST_TARGET_SECONDS,
            )),
        );
        map.insert(
            "liveMaxLatencySec".into(),
            json_number(finite(self.live_max_latency_seconds, 16.0)),
        );
        map.insert("seekTokenRequired".into(), self.seek_token_required.into());
        map.insert("sessionId".into(), session_id.into());
        serde_json::Value::Object(map).to_string()
    }
}

/// 정수 값은 정수로 싣는다(`JSONSerialization` 과 같은 출력: `16`, `2`).
fn json_number(v: f64) -> serde_json::Value {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        serde_json::Value::from(v as i64)
    } else {
        serde_json::Value::from(v)
    }
}

#[cfg(test)]
mod tests {
    //! `MatchWebV2PairingTests.configLiteral` 포팅.
    use super::*;

    #[test]
    fn config_literal() {
        let literal = HlsJsMatchWebV2Config::default().js_object_literal(7);
        let v: serde_json::Value = serde_json::from_str(&literal).unwrap();
        let obj = v.as_object().unwrap();
        let keys: Vec<_> = obj.keys().cloned().collect();
        assert_eq!(
            keys,
            [
                "enabled",
                "hlsTargetSec",
                "ingestTargetSec",
                "liveMaxLatencySec",
                "seekTokenRequired",
                "sessionId"
            ]
        );
        assert_eq!(obj["enabled"], true);
        assert_eq!(obj["sessionId"], 7);
        assert_eq!(obj["hlsTargetSec"].as_f64(), Some(2.0));
        assert_eq!(obj["ingestTargetSec"].as_f64(), Some(3.3));
        assert_eq!(obj["liveMaxLatencySec"].as_f64(), Some(16.0));
        assert_eq!(obj["seekTokenRequired"], true);
        assert!(literal.starts_with("{\"enabled\":true"));
        assert!(literal.ends_with("\"sessionId\":7}"));
        let custom = HlsJsMatchWebV2Config::new(3.1).js_object_literal(8);
        let parsed: serde_json::Value = serde_json::from_str(&custom).unwrap();
        assert_eq!(parsed["ingestTargetSec"].as_f64(), Some(3.1));
        let nan = HlsJsMatchWebV2Config::new(f64::NAN).js_object_literal(1);
        assert!(nan.contains("\"ingestTargetSec\":3.3"));
    }

    #[test]
    fn forced_quality_target() {
        assert_eq!(
            HlsJsForcedQualityTarget::new(-5, 100),
            HlsJsForcedQualityTarget {
                height: 0,
                target_bitrate_kbps: 0
            }
        );
        assert_eq!(
            HlsJsForcedQualityTarget::new(720, -3).target_bitrate_kbps,
            0
        );
        assert_eq!(
            HlsJsForcedQualityTarget::MULTI_LIVE_1080P_8MBPS.height,
            1080
        );
    }
}
