//! `Sources/CViewNetworking/NetworkConstants.swift` 포팅 — 네트워킹 매직 넘버 상수.
//!
//! 이미지 캐시 상수(`ImageCacheDefaults`)는 이미지 캐시를 포팅하지 않으므로 제외했다.

/// API 클라이언트 기본값. 시간 값은 초.
pub mod api_defaults {
    /// API 요청 타임아웃 (초).
    pub const REQUEST_TIMEOUT: f64 = 10.0;
    /// API 리소스 타임아웃 (초) — 실패 지연의 실효 상한.
    pub const RESOURCE_TIMEOUT: f64 = 12.0;
    /// 캐시 정리 주기 (초, 5분).
    pub const CACHE_PURGE_INTERVAL: f64 = 300.0;
    /// 기본 캐시 TTL (초, 5분).
    pub const DEFAULT_CACHE_TTL: f64 = 300.0;
    /// 전체 라이브 수집 비상 안전 상한. 도달 시 결과는 반드시 `truncated` 로 취급한다.
    pub const ALL_LIVES_MAX_PAGES: usize = 200;
    /// 429 Rate Limit 최대 재대기 (초).
    pub const MAX_RATE_LIMIT_RETRY_SECS: f64 = 30.0;
    /// Clip Inkey 요청 타임아웃 (초).
    pub const CLIP_INKEY_TIMEOUT: f64 = 10.0;
    /// VOD 스트림 정보 요청 타임아웃 (초).
    pub const VOD_STREAM_INFO_TIMEOUT: f64 = 10.0;
}

/// 응답 캐시 기본값.
pub mod response_cache_defaults {
    /// 응답 캐시 최대 항목 수.
    pub const MAX_ENTRIES: usize = 100;
    /// 기본 캐시 TTL (초).
    pub const DEFAULT_TTL: f64 = 300.0;
    /// 메모리 응답 캐시 바이트 상한.
    pub const MAX_BYTES: usize = 16 * 1024 * 1024;
}
