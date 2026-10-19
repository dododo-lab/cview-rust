//! `Sources/CViewPlayer/PlayerConstants.swift` 포팅 — CViewPlayer 모듈 매직 넘버 상수.

/// 저지연 컨트롤러 상수.
pub mod latency_defaults {
    /// 속도 변경 최소 유의 변화량 — JS `setRate()` 의 문턱(0.015)과 같아야 한다.
    pub const RATE_SIGNIFICANCE_THRESHOLD: f64 = 0.015;
}

/// 로컬 스트림 프록시 상수(초·개수).
pub mod proxy_defaults {
    pub const KEEP_ALIVE_TIMEOUT: f64 = 15.0;
    pub const REQUEST_TIMEOUT: f64 = 15.0;
    pub const RESOURCE_TIMEOUT: f64 = 30.0;
    pub const MAX_CONNECTIONS_PER_HOST: usize = 24;
    pub const MAX_RECEIVE_LENGTH: usize = 65536;
    pub const UPSTREAM_REQUEST_TIMEOUT: f64 = 15.0;
    pub const MAX_ACTIVE_CONNECTIONS: usize = 80;
    /// 매니페스트 요청용 Accept-Encoding.
    pub const MANIFEST_ACCEPT_ENCODING: &str = "gzip, deflate";
}

/// 멀티라이브 대역폭 코디네이터 상수 (flashls 참조).
pub mod multi_live_bw_defaults {
    pub const SAFETY_FACTOR: f64 = 0.75;
    pub const MIN_PER_STREAM_BITRATE: f64 = 300_000.0;
    pub const HISTORY_SIZE: usize = 30;
    pub const LOW_BUFFER_THRESHOLD: f64 = 3.0;
    pub const HIGH_BUFFER_THRESHOLD: f64 = 6.0;
    pub const SELECTED_SESSION_WEIGHT: f64 = 1.5;
    pub const EMERGENCY_BUFFER_THRESHOLD: f64 = 1.0;
    pub const UPDATE_INTERVAL_SECS: f64 = 8.0;
}

#[cfg(test)]
mod tests {
    //! `Tests/CViewPlayerTests/PlayerConstantsTests.swift` 포팅.
    use super::*;

    #[test]
    fn values() {
        assert_eq!(latency_defaults::RATE_SIGNIFICANCE_THRESHOLD, 0.015);
        assert_eq!(proxy_defaults::KEEP_ALIVE_TIMEOUT, 15.0);
        assert_eq!(proxy_defaults::REQUEST_TIMEOUT, 15.0);
        assert_eq!(proxy_defaults::MAX_CONNECTIONS_PER_HOST, 24);
        assert_eq!(proxy_defaults::MAX_ACTIVE_CONNECTIONS, 80);
    }
}
