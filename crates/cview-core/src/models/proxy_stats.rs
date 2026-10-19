//! `Sources/CViewCore/Models/ProxyNetworkStats.swift` 포팅 — 로컬 프록시 네트워크 통계 스냅샷.

use chrono::{DateTime, Utc};

/// 로컬 스트림 프록시가 수집한 네트워크 통계 스냅샷. 시간 값은 초.
#[derive(Debug, Clone, PartialEq)]
pub struct ProxyNetworkStats {
    /// 총 처리 요청 수 (누적)
    pub total_requests: i64,
    /// M3U8 요청 중 캐시 히트 수
    pub cache_hits: i64,
    /// M3U8 요청 중 캐시 미스 수
    pub cache_misses: i64,
    /// 에러 응답 수 (HTTP 4xx/5xx)
    pub error_count: i64,
    /// 총 CDN 수신 바이트 (누적)
    pub total_bytes_received: i64,
    /// 총 전달 바이트 (누적, 캐시 히트 포함)
    pub total_bytes_served: i64,
    /// 현재 활성 연결 수
    pub active_connections: i64,
    /// 연속 403 에러 수
    pub consecutive_403_count: i64,
    /// 최근 CDN 평균 응답 시간 — 슬라이딩 윈도우
    pub avg_response_time: f64,
    /// 최근 CDN 최대 응답 시간
    pub max_response_time: f64,
    /// 최근 세그먼트 평균 fetch 소요 시간
    pub avg_segment_fetch_duration: f64,
    /// 최근 세그먼트 평균 TTFB
    pub avg_segment_ttfb: f64,
    /// 최근 세그먼트 통계에 포함된 샘플 수
    pub segment_sample_count: i64,
    pub timestamp: DateTime<Utc>,
}

impl Default for ProxyNetworkStats {
    fn default() -> Self {
        Self {
            total_requests: 0,
            cache_hits: 0,
            cache_misses: 0,
            error_count: 0,
            total_bytes_received: 0,
            total_bytes_served: 0,
            active_connections: 0,
            consecutive_403_count: 0,
            avg_response_time: 0.0,
            max_response_time: 0.0,
            avg_segment_fetch_duration: 0.0,
            avg_segment_ttfb: 0.0,
            segment_sample_count: 0,
            timestamp: Utc::now(),
        }
    }
}

impl ProxyNetworkStats {
    /// 캐시 히트율 0.0~1.0.
    pub fn cache_hit_ratio(&self) -> f64 {
        let total = self.cache_hits + self.cache_misses;
        if total <= 0 {
            0.0
        } else {
            self.cache_hits as f64 / total as f64
        }
    }

    /// CDN 수신 속도 (Mbps) — 외부에서 계산하여 주입하므로 항상 0.
    pub fn received_mbps(&self) -> f64 {
        0.0
    }

    /// 에러율 0.0~1.0.
    pub fn error_rate(&self) -> f64 {
        if self.total_requests <= 0 {
            0.0
        } else {
            self.error_count as f64 / self.total_requests as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratios() {
        let s = ProxyNetworkStats::default();
        assert_eq!(s.cache_hit_ratio(), 0.0);
        assert_eq!(s.error_rate(), 0.0);
        let s = ProxyNetworkStats {
            cache_hits: 3,
            cache_misses: 1,
            total_requests: 10,
            error_count: 2,
            ..s
        };
        assert_eq!(s.cache_hit_ratio(), 0.75);
        assert_eq!(s.error_rate(), 0.2);
        assert_eq!(s.received_mbps(), 0.0);
    }
}
