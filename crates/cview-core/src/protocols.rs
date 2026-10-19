//! `Sources/CViewCore/Protocols` 포팅 — `APIClientProtocol.swift` · `AuthManagerProtocol.swift` ·
//! `PlayerEngineProtocol.swift` 를 trait 으로 옮겼다.
//!
//! - Swift `actor` 프로토콜은 `Send + Sync` trait 과 `async_trait` 메서드로 바꿨다.
//! - `PlayerEngineProtocol` 은 엔진 비의존 부분만 남겼다 — `videoView: NSView` 같은 AppKit
//!   요소는 UI 계층(Tauri) 몫이다.

use std::ops::{BitAnd, BitOr, BitOrAssign, RangeInclusive};
use std::path::Path;

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use url::Url;

use crate::error::ApiError;
use crate::models::player::PlayerError;
use crate::models::settings::ScreenshotFormat;

// MARK: - API client

/// HTTP 메서드.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
}

impl HttpMethod {
    /// Swift `rawValue` (`"GET"` 등).
    pub fn as_str(self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Delete => "DELETE",
        }
    }
}

impl std::fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// URL 쿼리 항목 (Swift `URLQueryItem`). 같은 이름이 여러 번 나올 수 있다.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QueryItem {
    pub name: String,
    pub value: Option<String>,
}

impl QueryItem {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: Some(value.into()),
        }
    }
}

/// 캐시 정책. `ttl` 은 초.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CachePolicy {
    ReturnCacheElseLoad { ttl: f64 },
    ReloadIgnoringCache,
    ReturnCacheOnly,
}

impl CachePolicy {
    /// 기본 60초 캐시 (Swift `CachePolicy.standard`).
    pub const STANDARD: CachePolicy = CachePolicy::ReturnCacheElseLoad { ttl: 60.0 };
    /// 캐시 없음 (Swift `CachePolicy.none`).
    pub const NONE: CachePolicy = CachePolicy::ReloadIgnoringCache;
}

/// 엔드포인트 프로토콜.
pub trait EndpointProtocol: Send + Sync {
    fn path(&self) -> String;
    fn method(&self) -> HttpMethod;
    fn query_items(&self) -> Option<Vec<QueryItem>>;
    fn body(&self) -> Option<Vec<u8>>;
    fn requires_auth(&self) -> bool;
    fn cache_policy(&self) -> CachePolicy;
    /// 이전 값을 쓸 수 있는 전체 기한(초). `cache_policy` 의 ttl 까지는 fresh, 그 뒤 이 기한까지는
    /// 이전 값을 먼저 돌려주고 뒤에서 다시 받는다(stale-while-revalidate). `None` 이면 stale 구간이 없다.
    fn stale_ttl(&self) -> Option<f64> {
        None
    }
    /// 요청 장부·로그에 쓰는 이름. 연관값(채널 ID·검색어)은 넣지 않는다.
    /// (Swift 는 `ChzzkEndpoint` 로 다운캐스트해 얻었다 — Rust 는 trait 기본값 `"other"` 로 둔다.)
    fn diagnostic_name(&self) -> &'static str {
        "other"
    }
}

/// API 클라이언트 프로토콜 — DI 및 테스트 Mock 지원.
#[async_trait]
pub trait ApiClientProtocol: Send + Sync {
    async fn request<T>(&self, endpoint: &dyn EndpointProtocol) -> Result<T, ApiError>
    where
        T: DeserializeOwned + Clone + Send + Sync + 'static;
}

// MARK: - Auth

/// 인증 상태.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthState {
    LoggedOut,
    LoggingIn,
    LoggedIn { user_id: String },
    Expired,
    Error(String),
}

impl AuthState {
    pub fn is_logged_in(&self) -> bool {
        matches!(self, AuthState::LoggedIn { .. })
    }
}

/// 요청에 붙일 쿠키 한 개 (Swift `HTTPCookie` 의 요청 헤더에 필요한 부분).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HttpCookie {
    pub name: String,
    pub value: String,
    pub domain: Option<String>,
    pub path: Option<String>,
}

impl HttpCookie {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            domain: None,
            path: None,
        }
    }

    /// `HTTPCookie.requestHeaderFields(with:)` 의 `Cookie` 값 — `"a=1; b=2"`. 비어 있으면 `None`.
    pub fn header_value(cookies: &[HttpCookie]) -> Option<String> {
        if cookies.is_empty() {
            return None;
        }
        Some(
            cookies
                .iter()
                .map(|c| format!("{}={}", c.name, c.value))
                .collect::<Vec<_>>()
                .join("; "),
        )
    }
}

/// 인증 토큰 제공자 (API 클라이언트에서 사용).
#[async_trait]
pub trait AuthTokenProvider: Send + Sync {
    async fn cookies(&self) -> Option<Vec<HttpCookie>>;
    async fn access_token(&self) -> Option<String>;
    async fn is_authenticated(&self) -> bool;
    /// 로그인/로그아웃 때 바뀐다(토큰 회전은 아님). 캐시 키에 자격 증명을 넣지 않기 위한 세대값.
    async fn authentication_generation(&self) -> u64 {
        0
    }
}

// MARK: - Player engine

/// 캡처 기본 구현이 던지는 사유. 계약 테스트가 "기본 구현이 그대로 쓰였는가"를 판별하는 표지.
pub const SCREENSHOT_UNIMPLEMENTED_REASON: &str = "이 엔진은 화면 캡처를 지원하지 않습니다";

/// 녹화 기본 구현이 던지는 사유.
pub const RECORDING_UNSUPPORTED_REASON: &str = "이 엔진은 녹화를 지원하지 않습니다";

/// 캡처된 단일 프레임 — 인코딩까지 끝난 바이트와 원본 픽셀 크기.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedFrame {
    /// `format` 으로 인코딩된 이미지 바이트.
    pub data: Vec<u8>,
    /// 실제 인코딩 포맷. 파일 확장자는 **요청값이 아니라 이 값**을 따라야 한다.
    pub format: ScreenshotFormat,
    pub pixel_width: u32,
    pub pixel_height: u32,
}

/// 플레이어 엔진별 기능 매트릭스 (Swift `OptionSet`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct PlayerEngineCapabilities(pub u32);

impl PlayerEngineCapabilities {
    /// 스트림 녹화.
    pub const RECORDING: Self = Self(1 << 0);
    /// 단일 프레임 캡처 (스크린샷).
    pub const SCREENSHOT: Self = Self(1 << 1);
    /// 자막 트랙 선택/표시.
    pub const SUBTITLES: Self = Self(1 << 2);
    /// 오디오 이퀄라이저 (PreAmp + 밴드).
    pub const EQUALIZER: Self = Self(1 << 3);
    /// 비디오 조정 (밝기/대비/채도/색조/감마).
    pub const VIDEO_ADJUST: Self = Self(1 << 4);
    /// 외부 PID 기반 저지연 속도 보정.
    pub const LOW_LATENCY_RATE_CONTROL: Self = Self(1 << 5);
    /// 외부 ABR 컨트롤러로부터 비트레이트 캡 수용.
    pub const EXTERNAL_ABR: Self = Self(1 << 6);
    /// 로딩 일시 중지/재개 (멀티라이브 비선택 세션 절전).
    pub const SUSPEND_LOADING: Self = Self(1 << 7);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl BitOr for PlayerEngineCapabilities {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for PlayerEngineCapabilities {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl BitAnd for PlayerEngineCapabilities {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

/// 플레이어 엔진 프로토콜 — 엔진 공통 인터페이스(엔진 비의존 부분).
///
/// 시간 값은 초 단위 `f64`. 상태 변경 메서드는 `&self` 를 받는다 — 구현체는 내부 가변성을 쓴다
/// (Swift `AnyObject` 클래스 / actor 와 같은 모양).
///
/// ⚠ 헬스체크(`is_in_error_state`/`reset_retries`)·`capabilities`·`seekable_range` 에는 기본 구현을
/// 두지 않는다 — 원본의 "배선 누락 은폐 장치" 제거 결정을 그대로 따른다. 녹화·캡처 기본 구현은
/// **명시적으로 실패**하는 계약이라 남긴다.
#[async_trait]
pub trait PlayerEngineProtocol: Send + Sync {
    fn is_playing(&self) -> bool;
    fn current_time(&self) -> f64;
    fn duration(&self) -> f64;
    /// 현재 탐색 가능한 미디어 시간 범위. 라이브 DVR 은 0 에서 시작하지 않을 수 있다.
    fn seekable_range(&self) -> Option<RangeInclusive<f64>>;
    fn rate(&self) -> f32;

    async fn play(&self, url: &Url) -> Result<(), PlayerError>;
    fn pause(&self);
    fn resume(&self);
    fn stop(&self);
    fn seek(&self, position: f64);
    fn set_rate(&self, rate: f32);
    fn set_volume(&self, volume: f32);

    // MARK: Recording — 미지원 엔진은 명시적으로 실패한다.
    async fn start_recording(&self, _path: &Path) -> Result<(), PlayerError> {
        Err(PlayerError::RecordingFailed(
            RECORDING_UNSUPPORTED_REASON.into(),
        ))
    }
    async fn stop_recording(&self) {}
    fn is_recording(&self) -> bool {
        false
    }

    // MARK: Health check
    fn is_in_error_state(&self) -> bool;
    fn reset_retries(&self);

    /// 엔진이 **의도적으로** 로딩을 정지한 상태인가 (기본 false).
    /// ⚠ 이 값이 참일 때 재개를 호출하지 말 것 — '개입하지 않을 근거' 로만 쓴다.
    fn is_suspend_intent_active(&self) -> bool {
        false
    }

    /// 현재 표시 중인 프레임 1장을 캡처한다. 기본 구현은 조용한 no-op 대신 명시적 실패.
    /// ⚠ 이 기본 구현을 "성공한 척" 하도록 바꾸지 말 것.
    async fn capture_screenshot(
        &self,
        _format: ScreenshotFormat,
    ) -> Result<CapturedFrame, PlayerError> {
        Err(PlayerError::ScreenshotFailed(
            SCREENSHOT_UNIMPLEMENTED_REASON.into(),
        ))
    }

    /// 본 엔진이 지원하는 기능 집합. UI/설정 패널이 타입 캐스팅 대신 이 flag 로 분기한다.
    fn capabilities(&self) -> PlayerEngineCapabilities;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_policy_constants() {
        assert_eq!(
            CachePolicy::STANDARD,
            CachePolicy::ReturnCacheElseLoad { ttl: 60.0 }
        );
        assert_eq!(CachePolicy::NONE, CachePolicy::ReloadIgnoringCache);
    }

    #[test]
    fn http_method_raw_values() {
        assert_eq!(HttpMethod::Get.as_str(), "GET");
        assert_eq!(HttpMethod::Post.as_str(), "POST");
        assert_eq!(HttpMethod::Put.as_str(), "PUT");
        assert_eq!(HttpMethod::Delete.as_str(), "DELETE");
    }

    #[test]
    fn auth_state() {
        assert!(AuthState::LoggedIn {
            user_id: "u".into()
        }
        .is_logged_in());
        assert!(!AuthState::Expired.is_logged_in());
    }

    #[test]
    fn cookie_header() {
        assert_eq!(HttpCookie::header_value(&[]), None);
        let v = HttpCookie::header_value(&[
            HttpCookie::new("NID_AUT", "a"),
            HttpCookie::new("NID_SES", "b"),
        ]);
        assert_eq!(v.as_deref(), Some("NID_AUT=a; NID_SES=b"));
    }

    #[test]
    fn capabilities_option_set() {
        let caps = PlayerEngineCapabilities::SCREENSHOT | PlayerEngineCapabilities::EQUALIZER;
        assert!(caps.contains(PlayerEngineCapabilities::SCREENSHOT));
        assert!(!caps.contains(PlayerEngineCapabilities::RECORDING));
        assert!(PlayerEngineCapabilities::empty().is_empty());
    }

    struct DummyEngine;

    #[async_trait]
    impl PlayerEngineProtocol for DummyEngine {
        fn is_playing(&self) -> bool {
            false
        }
        fn current_time(&self) -> f64 {
            0.0
        }
        fn duration(&self) -> f64 {
            0.0
        }
        fn seekable_range(&self) -> Option<RangeInclusive<f64>> {
            None
        }
        fn rate(&self) -> f32 {
            1.0
        }
        async fn play(&self, _url: &Url) -> Result<(), PlayerError> {
            Ok(())
        }
        fn pause(&self) {}
        fn resume(&self) {}
        fn stop(&self) {}
        fn seek(&self, _position: f64) {}
        fn set_rate(&self, _rate: f32) {}
        fn set_volume(&self, _volume: f32) {}
        fn is_in_error_state(&self) -> bool {
            false
        }
        fn reset_retries(&self) {}
        fn capabilities(&self) -> PlayerEngineCapabilities {
            PlayerEngineCapabilities::empty()
        }
    }

    /// `PlayerEngineCapabilitiesTests` 의 계약 — 기본 구현은 명시적으로 실패한다.
    #[test]
    fn default_implementations_fail_explicitly() {
        let engine = DummyEngine;
        assert_eq!(
            futures_lite_block_on(engine.capture_screenshot(ScreenshotFormat::Png)),
            Err(PlayerError::ScreenshotFailed(
                SCREENSHOT_UNIMPLEMENTED_REASON.into()
            ))
        );
        assert_eq!(
            futures_lite_block_on(engine.start_recording(Path::new("/tmp/x"))),
            Err(PlayerError::RecordingFailed(
                RECORDING_UNSUPPORTED_REASON.into()
            ))
        );
        assert!(!engine.is_recording());
        assert!(!engine.is_suspend_intent_active());
    }

    /// 런타임 없이 즉시 끝나는 future 를 실행하는 최소 executor (테스트 전용).
    fn futures_lite_block_on<F: std::future::Future>(fut: F) -> F::Output {
        use std::pin::pin;
        use std::task::{Context, Poll, Waker};
        let mut cx = Context::from_waker(Waker::noop());
        let mut fut = pin!(fut);
        loop {
            if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }
}
