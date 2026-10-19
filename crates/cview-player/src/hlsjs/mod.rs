//! hls.js(WebView) 엔진의 엔진 비의존 부분 — 프로파일 값 타입, Rust↔JS 메시지 프로토콜, 진단 파싱.

pub mod profile;

pub use profile::{HlsJsForcedQualityTarget, HlsJsMatchWebV2Config, HlsJsStreamingProfile};
