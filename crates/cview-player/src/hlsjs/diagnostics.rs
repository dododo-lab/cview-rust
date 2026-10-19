//! `Sources/CViewPlayer/HLSJSDiagnostics.swift` 포팅 — JS 진단 봉투를 원시값 필드로만 받아들인다.
//!
//! 헤더·URL·미디어 데이터·임의 객체는 통과시키지 않는다.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

/// 진단 수준 (Swift `CViewCore.DiagnosticLevel`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticLevel {
    Debug,
    Info,
    Warning,
    Error,
}

const FIELD_NAMES: &[(&str, &str)] = &[
    ("event", "event"),
    ("sourceEpoch", "source_epoch"),
    ("profile", "profile"),
    ("hlsVersion", "hls_version"),
    ("eventWallTimeMs", "event_wall_time_ms"),
    ("eventUptimeMs", "event_uptime_ms"),
    ("type", "error_type"),
    ("details", "details"),
    ("fatal", "fatal"),
    ("recoverable", "recoverable"),
    ("message", "error_message"),
    ("errorName", "error_name"),
    ("stack", "stack"),
    ("reason", "reason"),
    ("httpStatus", "http_status"),
    ("recoveryAction", "recovery_action"),
    ("deferredRecovery", "deferred_recovery"),
    ("hlsResetMediaSource", "hls_reset_media_source"),
    ("errorAction", "hls_error_action"),
    ("errorActionFlags", "hls_error_action_flags"),
    ("fragmentSN", "fragment_sn"),
    ("fragmentLevel", "fragment_level"),
    ("partIndex", "part_index"),
    ("bytes", "bytes"),
    ("currentTime", "current_time"),
    ("readyState", "ready_state"),
    ("networkState", "network_state"),
    ("paused", "paused"),
    ("suspended", "suspended"),
    ("visibility", "visibility"),
    ("mediaErrorCode", "media_error_code"),
    ("requestID", "request_id"),
    ("requestKind", "request_kind"),
    ("outcome", "outcome"),
    ("ttfbMs", "ttfb_ms"),
    ("totalMs", "total_ms"),
    ("firstByteBudgetMs", "first_byte_budget_ms"),
    ("totalBudgetMs", "total_budget_ms"),
    ("requestLevel", "request_level"),
    ("deliveryMSN", "delivery_msn"),
    ("deliveryPart", "delivery_part"),
    ("deliverySkip", "delivery_skip"),
    ("fallbackOfRequestID", "fallback_of_request_id"),
    ("playlistEndSN", "playlist_end_sn"),
    ("playlistPartSN", "playlist_part_sn"),
    ("playlistPartIndex", "playlist_part_index"),
    ("playlistEdge", "playlist_edge"),
    ("playlistAgeMs", "playlist_age_ms"),
    ("seekID", "seek_id"),
    ("seekReason", "seek_reason"),
    ("seekFrom", "seek_from"),
    ("seekTo", "seek_to"),
    ("seekDelta", "seek_delta"),
    ("seekGeneration", "seek_generation"),
    ("seekWaitMs", "seek_wait_ms"),
    ("seeking", "seeking"),
    ("bufferStart", "buffer_start"),
    ("bufferEnd", "buffer_end"),
    ("bufferAhead", "buffer_ahead"),
    ("seekableStart", "seekable_start"),
    ("seekableEnd", "seekable_end"),
    ("latencySeconds", "latency_seconds"),
    ("latencyNumericValid", "latency_numeric_valid"),
    ("targetLatencySeconds", "target_latency_seconds"),
    ("playbackRate", "playback_rate"),
    ("resyncGeneration", "resync_generation"),
    ("script", "script"),
    ("scriptLine", "script_line"),
    ("scriptColumn", "script_column"),
    ("command", "command"),
];

/// `NSNumber.stringValue` 근사 — 정수 값은 소수점 없이.
pub fn number_string(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// 허용된 원시값 필드만 snake_case 키로 옮긴다. 문자열은 4096자에서 자른다.
pub fn fields(data: &Map<String, Value>, surface: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    out.insert("surface".to_string(), surface.to_string());
    for (source, target) in FIELD_NAMES {
        match data.get(*source) {
            Some(Value::String(s)) => {
                out.insert(target.to_string(), s.chars().take(4096).collect());
            }
            Some(Value::Bool(b)) => {
                out.insert(target.to_string(), b.to_string());
            }
            Some(Value::Number(n)) => {
                if let Some(v) = n.as_f64().filter(|v| v.is_finite()) {
                    let text = if n.is_f64() {
                        number_string(v)
                    } else {
                        n.to_string()
                    };
                    out.insert(target.to_string(), text);
                }
            }
            _ => {}
        }
    }
    out
}

/// 진단 수준 판정.
pub fn severity(data: &Map<String, Value>, is_playback_error: bool) -> DiagnosticLevel {
    if is_playback_error {
        return if data.get("fatal").and_then(Value::as_bool) == Some(true) {
            DiagnosticLevel::Error
        } else {
            DiagnosticLevel::Warning
        };
    }
    match data.get("severity").and_then(Value::as_str) {
        Some("error") => DiagnosticLevel::Error,
        Some("warning") => DiagnosticLevel::Warning,
        Some("info") => DiagnosticLevel::Info,
        _ => DiagnosticLevel::Debug,
    }
}

#[cfg(test)]
mod tests {
    //! `HLSJSDiagnosticsTests.nativeFieldsRejectObjectsPayloadsAndNonfiniteNumbers` 포팅.
    use super::*;
    use serde_json::json;

    #[test]
    fn native_fields_reject_objects_and_payloads() {
        let data = json!({
            "fatal": true, "httpStatus": 403, "fragmentSN": "initSegment", "hlsVersion": "1.7.3",
            "requestID": {"token": "secret"}, "headers": {"Cookie": "secret"},
            "base64": "media", "url": "https://example.com/?token=secret", "stack": "bounded stack",
            "currentTime": 12.5, "seekTo": 3.0
        });
        let f = fields(data.as_object().unwrap(), "surface-a");
        assert_eq!(f["surface"], "surface-a");
        assert_eq!(f["fatal"], "true");
        assert_eq!(f["http_status"], "403");
        assert_eq!(f["fragment_sn"], "initSegment");
        assert_eq!(f["hls_version"], "1.7.3");
        assert_eq!(f["current_time"], "12.5");
        assert_eq!(f["seek_to"], "3");
        assert!(!f.contains_key("bytes") && !f.contains_key("request_id"));
        assert!(!f.contains_key("headers") && !f.contains_key("base64") && !f.contains_key("url"));
        let m = |v: Value| v.as_object().unwrap().clone();
        assert_eq!(
            severity(&m(json!({"fatal": true})), true),
            DiagnosticLevel::Error
        );
        assert_eq!(
            severity(&m(json!({"fatal": false})), true),
            DiagnosticLevel::Warning
        );
        assert_eq!(
            severity(&m(json!({"severity": "error"})), false),
            DiagnosticLevel::Error
        );
        assert_eq!(severity(&m(json!({})), false), DiagnosticLevel::Debug);
    }
}
