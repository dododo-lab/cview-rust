//! `Sources/CViewCore/Utilities/CommonHeaders.swift` 포팅 — 치지직 통신 공통 HTTP 헤더 값.
//!
//! 원본은 CViewCore 유틸에 있지만 Rust 포팅에서는 HTTP 를 다루는 이 크레이트에 둔다.

/// Safari User-Agent (스트림 프록시 등에서 사용).
pub const SAFARI_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";

/// Chrome User-Agent (API 클라이언트, WebSocket 등에서 사용).
pub const CHROME_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36";

/// Referer 헤더 값 (trailing slash 포함).
pub const CHZZK_REFERER: &str = "https://chzzk.naver.com/";

/// Origin 헤더 값 (trailing slash 없음).
pub const CHZZK_ORIGIN: &str = "https://chzzk.naver.com";

/// API 클라이언트가 실제로 보내는 Referer — **슬래시 없음**.
///
/// ⚠ 원본 `ChzzkAPIClient` 는 [`CHZZK_REFERER`] 대신 슬래시 없는 값을 하드코딩한다.
/// 치환하면 전송 헤더가 바뀌므로 그대로 보존한다.
pub const API_REFERER: &str = "https://chzzk.naver.com";
