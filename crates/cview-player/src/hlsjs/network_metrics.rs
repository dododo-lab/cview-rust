//! `Sources/CViewPlayer/HLSJSNetworkMetrics.swift` 포팅 — hls.js 메트릭 스냅샷의 네트워크 부분 파싱.
//!
//! JS 가 보낸 JSON(`metrics.network`)을 상한·유한성 검사를 거쳐 받아들인다. 전송 표본은 페이지의
//! "나이(age)" 를 Rust 쪽 monotonic 시각(`sampled_at`)으로 바꿔 절대 시각으로 저장한다.

use serde_json::{Map, Value};

/// 한 번의 body 전송 표본. 시각은 `sampled_at` 과 같은 monotonic 축(초).
#[derive(Debug, Clone, PartialEq)]
pub struct StreamTransferSample {
    pub id: String,
    pub bytes: f64,
    pub request_start: f64,
    pub first_byte: f64,
    pub end: f64,
    pub media_duration: f64,
    pub is_part: bool,
}

impl StreamTransferSample {
    pub fn new(
        id: impl Into<String>,
        bytes: f64,
        request_start: f64,
        first_byte: f64,
        end: f64,
        media_duration: f64,
    ) -> Self {
        Self {
            id: id.into(),
            bytes,
            request_start,
            first_byte,
            end,
            media_duration,
            is_part: false,
        }
    }

    /// 공유 업스트림 용량을 세울 수 있는 표본인가(루프백 즉시 전달 등은 제외).
    pub fn is_usable(&self, now: f64, freshness: f64) -> bool {
        !self.id.is_empty()
            && self.id.chars().count() <= 64
            && self.bytes.is_finite()
            && self.bytes >= 16_384.0
            && self.bytes <= 128.0 * 1024.0 * 1024.0
            && self.request_start.is_finite()
            && self.first_byte.is_finite()
            && self.end.is_finite()
            && self.request_start <= self.first_byte
            && self.end - self.first_byte >= 0.02
            && self.request_start >= now - freshness
            && self.end <= now + 0.25
    }
}

/// 요청 종류별 집계.
#[derive(Debug, Clone, PartialEq)]
pub struct HlsJsRequestMetrics {
    pub kind: String,
    pub success: i64,
    pub error: i64,
    pub timeout: i64,
    pub abort: i64,
    pub ttfb_p90: f64,
    pub total_p90: f64,
    pub first_byte_budget_ms: f64,
    pub total_budget_ms: f64,
}

/// 네트워크 메트릭 스냅샷.
#[derive(Debug, Clone, PartialEq)]
pub struct HlsJsNetworkMetrics {
    pub requests: Vec<HlsJsRequestMetrics>,
    pub transfers: Vec<StreamTransferSample>,
    pub media_source_kind: String,
    pub playlist_media_age_ms: f64,
    pub playlist_segment_stale_ratio: f64,
    pub part_samples: i64,
    pub part_ttfb_p90: f64,
    pub part_total_p90: f64,
}

fn number(row: &Map<String, Value>, key: &str, fallback: f64) -> f64 {
    match row.get(key).and_then(Value::as_f64) {
        Some(v) if v.is_finite() => v,
        _ => fallback,
    }
}

fn rows(data: &Map<String, Value>, key: &str) -> Vec<Map<String, Value>> {
    data.get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_object().cloned()).collect())
        .unwrap_or_default()
}

impl Default for HlsJsNetworkMetrics {
    fn default() -> Self {
        Self::empty()
    }
}

impl HlsJsNetworkMetrics {
    pub fn empty() -> Self {
        Self::parse(&Map::new(), 0.0)
    }

    pub fn from_value(data: &Value, sampled_at: f64) -> Self {
        match data.as_object() {
            Some(map) => Self::parse(map, sampled_at),
            None => Self::empty(),
        }
    }

    pub fn parse(data: &Map<String, Value>, sampled_at: f64) -> Self {
        // Swift 의 `[[String: Any]]` 캐스트는 원소 하나라도 사전이 아니면 배열 전체를 버린다.
        let all_objects = |key: &str| {
            data.get(key)
                .and_then(Value::as_array)
                .is_none_or(|a| a.iter().all(Value::is_object))
        };
        let requests = if all_objects("requests") {
            rows(data, "requests")
                .into_iter()
                .take(8)
                .filter_map(|row| {
                    let kind = row.get("kind")?.as_str()?;
                    if kind.chars().count() > 32 {
                        return None;
                    }
                    let count = |k: &str| number(&row, k, 0.0).clamp(0.0, i32::MAX as f64) as i64;
                    Some(HlsJsRequestMetrics {
                        kind: kind.to_string(),
                        success: count("success"),
                        error: count("error"),
                        timeout: count("timeout"),
                        abort: count("abort"),
                        ttfb_p90: number(&row, "ttfbP90", 0.0).max(0.0),
                        total_p90: number(&row, "totalP90", 0.0).max(0.0),
                        first_byte_budget_ms: number(&row, "firstByteBudgetMs", 0.0).max(0.0),
                        total_budget_ms: number(&row, "totalBudgetMs", 0.0).max(0.0),
                    })
                })
                .collect()
        } else {
            Vec::new()
        };
        let transfers = if all_objects("transfers") {
            rows(data, "transfers")
                .into_iter()
                .take(128)
                .filter_map(|row| {
                    let id = row.get("id")?.as_str()?.to_string();
                    let start_age = number(&row, "requestAge", -1.0);
                    let first_age = number(&row, "firstByteAge", -1.0);
                    let end_age = number(&row, "endAge", -1.0);
                    if !(start_age >= first_age
                        && first_age >= end_age
                        && end_age >= 0.0
                        && start_age <= 30.0
                        && sampled_at.is_finite())
                    {
                        return None;
                    }
                    let sample = StreamTransferSample {
                        id,
                        bytes: number(&row, "bytes", 0.0),
                        request_start: sampled_at - start_age,
                        first_byte: sampled_at - first_age,
                        end: sampled_at - end_age,
                        media_duration: number(&row, "mediaDuration", 0.0).max(0.0),
                        is_part: row.get("isPart").and_then(Value::as_bool).unwrap_or(false),
                    };
                    sample.is_usable(sampled_at, 30.0).then_some(sample)
                })
                .collect()
        } else {
            Vec::new()
        };
        Self {
            requests,
            transfers,
            media_source_kind: data
                .get("mediaSourceKind")
                .and_then(Value::as_str)
                .map_or_else(|| "unknown".into(), |s| s.chars().take(32).collect()),
            playlist_media_age_ms: number(data, "playlistMediaAgeMs", -1.0),
            playlist_segment_stale_ratio: number(data, "playlistSegmentStaleRatio", 0.0)
                .clamp(0.0, 1.0),
            part_samples: number(data, "partSamples", 0.0).clamp(0.0, 64.0) as i64,
            part_ttfb_p90: number(data, "partTtfbP90", 0.0).max(0.0),
            part_total_p90: number(data, "partTotalP90", 0.0).max(0.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_and_bounds() {
        let data = json!({
            "requests": [
                {"kind": "fragment", "success": 10, "error": -2, "timeout": 1e12, "abort": 1,
                 "ttfbP90": 0.2, "totalP90": -1, "firstByteBudgetMs": 900, "totalBudgetMs": 3000},
                {"kind": "x".repeat(40)},
                {"success": 1}
            ],
            "transfers": [
                {"id": "a", "bytes": 100000, "requestAge": 3.0, "firstByteAge": 2.5, "endAge": 1.0, "mediaDuration": 2, "isPart": true},
                {"id": "small", "bytes": 10, "requestAge": 3.0, "firstByteAge": 2.5, "endAge": 1.0},
                {"id": "order", "bytes": 100000, "requestAge": 1.0, "firstByteAge": 2.5, "endAge": 1.0},
                {"id": "old", "bytes": 100000, "requestAge": 31.0, "firstByteAge": 2.5, "endAge": 1.0}
            ],
            "mediaSourceKind": "ManagedMediaSource",
            "playlistSegmentStaleRatio": 4,
            "partSamples": 100,
        });
        let m = HlsJsNetworkMetrics::from_value(&data, 100.0);
        assert_eq!(m.requests.len(), 1);
        let r = &m.requests[0];
        assert_eq!((r.success, r.error, r.timeout), (10, 0, i32::MAX as i64));
        assert_eq!(r.total_p90, 0.0);
        assert_eq!(m.transfers.len(), 1);
        let t = &m.transfers[0];
        assert_eq!((t.request_start, t.first_byte, t.end), (97.0, 97.5, 99.0));
        assert!(t.is_part);
        assert_eq!(m.media_source_kind, "ManagedMediaSource");
        assert_eq!(m.playlist_media_age_ms, -1.0);
        assert_eq!(m.playlist_segment_stale_ratio, 1.0);
        assert_eq!(m.part_samples, 64);
        let e = HlsJsNetworkMetrics::empty();
        assert!(e.requests.is_empty() && e.transfers.is_empty());
        assert_eq!(e.media_source_kind, "unknown");
    }

    #[test]
    fn usable_rules() {
        let ok = StreamTransferSample::new("a", 20_000.0, 1.0, 1.0, 1.5, 2.0);
        assert!(ok.is_usable(2.0, 30.0));
        assert!(!StreamTransferSample::new("", 20_000.0, 1.0, 1.0, 1.5, 2.0).is_usable(2.0, 30.0));
        assert!(
            !StreamTransferSample::new("a", 20_000.0, 1.0, 1.0, 1.01, 2.0).is_usable(2.0, 30.0)
        );
        assert!(!StreamTransferSample::new("a", 20_000.0, 1.0, 1.0, 3.0, 2.0).is_usable(2.0, 30.0));
        assert!(!ok.is_usable(40.0, 30.0));
    }
}
