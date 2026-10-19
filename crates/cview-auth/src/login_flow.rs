//! `Sources/CViewAuth/UnifiedLoginWebView.swift` 의 UI 비의존 부분 포팅 — 통합 로그인 상태 기계.
//!
//! 원본은 `WKWebView` 코디네이터 안에 흐름 판단이 섞여 있었다. 웹뷰 자체(SwiftUI/WebKit)는 옮기지 않고,
//! 웹뷰 셸(Tauri 등)이 이벤트를 넘기면 다음 동작을 돌려주는 순수 상태 기계로 분리했다.
//!
//! 흐름:
//! 1. `NaverLogin` — [`NAVER_LOGIN_URL`] 로드 → 쿠키 변화·로드 완료마다 [`UnifiedLoginFlow::on_cookies`].
//!    `NID_AUT`/`NID_SES` 가 모두 생기면 쿠키를 앱 저장소로 주입하고([`crate::cookie::CookieManager::ingest_webview_cookies`])
//!    authURL 이 있으면 2단계, 없으면 쿠키 로그인으로 완료.
//! 2. `OAuthConsent` — 같은 웹뷰에 authURL 로드 → 이동마다 [`UnifiedLoginFlow::on_navigation`].
//!    redirect URI(scheme·host·port·path 정확 일치)를 가로채 code 를 꺼낸다. 오류·code 없음은 쿠키 로그인 폴백.

use url::Url;

use crate::cookie::{contains_required_auth_cookies, StoredCookie};
use crate::csrf::query_value;

/// 네이버 로그인 시작 URL — 로그인 뒤 chzzk 로 리다이렉트한다.
pub const NAVER_LOGIN_URL: &str =
    "https://nid.naver.com/nidlogin.login?url=https://chzzk.naver.com";

/// 1단계 실패 안내 문구.
pub const NAVER_LOGIN_LOAD_FAILED: &str =
    "네이버 로그인 페이지를 불러오지 못했습니다. 네트워크 상태를 확인해주세요.";

/// 진행 단계.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginPhase {
    /// 1단계 — 네이버 로그인.
    NaverLogin,
    /// 2단계 — 치지직 OAuth 동의 (쿠키는 이미 확보).
    OAuthConsent,
    Completed,
}

/// 웹뷰 셸이 할 일.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginAction {
    /// 아무것도 하지 않는다.
    None,
    /// 이 쿠키들을 앱 저장소로 주입한다(그 뒤 다음 동작이 이어진다).
    IngestCookies {
        cookies: Vec<StoredCookie>,
        next: Box<LoginAction>,
    },
    /// 2단계 진입 — 이 URL 을 로드한다.
    LoadOAuthConsent(Url),
    /// 쿠키만으로 완료 (`AuthManager::handle_login_success`).
    CookieOnlySuccess { reason: Option<String> },
    /// OAuth code 수신 (`csrf::validate` 후 `AuthManager::handle_oauth_login_success`).
    CodeReceived { code: String, state: Option<String> },
    /// 복구 불가 실패.
    Error(String),
}

/// 이동 요청에 대한 결정.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavigationDecision {
    Allow,
    /// 이동을 취소하고 동작을 수행한다(redirect URI 가로채기).
    Cancel(LoginAction),
}

/// 웹뷰 로드 오류 종류.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebLoadError {
    /// 사용자가/가로채기로 취소됨 — 정상 흐름.
    Cancelled,
    /// 리다이렉트 경합의 "Frame load interrupted" — 무시.
    FrameLoadInterrupted,
    /// 호스트 연결 실패 — 2단계 localhost redirect 잔여 오류일 때만 정상.
    CannotConnectToHost,
    Other(String),
}

/// 통합 로그인 상태 기계.
#[derive(Debug, Clone)]
pub struct UnifiedLoginFlow {
    phase: LoginPhase,
    auth_url: Option<Url>,
    redirect_uri: String,
}

impl UnifiedLoginFlow {
    /// `auth_url` 이 `None` 이면 쿠키 로그인만 한다.
    pub fn new(auth_url: Option<Url>, redirect_uri: impl Into<String>) -> Self {
        Self {
            phase: LoginPhase::NaverLogin,
            auth_url,
            redirect_uri: redirect_uri.into(),
        }
    }

    pub fn phase(&self) -> LoginPhase {
        self.phase
    }

    /// 실제로 연 authURL (CSRF 검증 기준).
    pub fn auth_url(&self) -> Option<&Url> {
        self.auth_url.as_ref()
    }

    pub fn start_url() -> Url {
        Url::parse(NAVER_LOGIN_URL).expect("static url")
    }

    /// 웹뷰 쿠키 저장소가 바뀌었거나 페이지 로드가 끝났을 때.
    pub fn on_cookies(&mut self, cookies: &[StoredCookie]) -> LoginAction {
        if self.phase != LoginPhase::NaverLogin {
            return LoginAction::None;
        }
        let auth = auth_only(cookies);
        if !contains_required_auth_cookies(&auth) {
            return LoginAction::None;
        }
        let next = self.advance_to_oauth_or_finish();
        LoginAction::IngestCookies {
            cookies: auth,
            next: Box::new(next),
        }
    }

    fn advance_to_oauth_or_finish(&mut self) -> LoginAction {
        match &self.auth_url {
            None => {
                self.phase = LoginPhase::Completed;
                tracing::info!("통합 로그인: OAuth 미구성 — 쿠키 로그인으로 완료");
                LoginAction::CookieOnlySuccess { reason: None }
            }
            Some(url) => {
                self.phase = LoginPhase::OAuthConsent;
                LoginAction::LoadOAuthConsent(url.clone())
            }
        }
    }

    /// 이동 요청. 2단계에서만 redirect URI 를 가로챈다. `latest_cookies` 는 그 시점 웹뷰 쿠키
    /// (2단계 중 회전된 NID 쿠키를 재동기화하기 위함).
    pub fn on_navigation(
        &mut self,
        url: &Url,
        latest_cookies: &[StoredCookie],
    ) -> NavigationDecision {
        if self.phase != LoginPhase::OAuthConsent || !is_redirect_uri(url, &self.redirect_uri) {
            return NavigationDecision::Allow;
        }
        let action = match parse_oauth_callback(url) {
            OAuthCallback::Code { code, state } => {
                self.phase = LoginPhase::Completed;
                LoginAction::CodeReceived { code, state }
            }
            OAuthCallback::Error(e) => self.fallback(format!("OAuth 서버 에러: {e}")),
            OAuthCallback::MissingCode => self.fallback("인증 코드 미수신".into()),
        };
        NavigationDecision::Cancel(with_resync(latest_cookies, action))
    }

    /// 웹뷰 로드 오류. `latest_cookies` 는 그 시점 웹뷰 쿠키.
    pub fn on_web_error(
        &mut self,
        error: &WebLoadError,
        latest_cookies: &[StoredCookie],
    ) -> LoginAction {
        match error {
            WebLoadError::Cancelled | WebLoadError::FrameLoadInterrupted => {
                return LoginAction::None
            }
            WebLoadError::CannotConnectToHost if self.phase == LoginPhase::OAuthConsent => {
                return LoginAction::None
            }
            _ => {}
        }
        match self.phase {
            LoginPhase::NaverLogin => {
                // 하드페일 전 마지막 쿠키 확인 — 이미 얻은 로그인을 버리지 않는다.
                let action = self.on_cookies(latest_cookies);
                if action != LoginAction::None {
                    return action;
                }
                self.phase = LoginPhase::Completed;
                LoginAction::Error(NAVER_LOGIN_LOAD_FAILED.into())
            }
            LoginPhase::OAuthConsent => {
                let reason = match error {
                    WebLoadError::Other(m) => format!("웹 오류: {m}"),
                    other => format!("웹 오류: {other:?}"),
                };
                let action = self.fallback(reason);
                with_resync(latest_cookies, action)
            }
            LoginPhase::Completed => LoginAction::None,
        }
    }

    fn fallback(&mut self, reason: String) -> LoginAction {
        if self.phase != LoginPhase::OAuthConsent {
            return LoginAction::None;
        }
        self.phase = LoginPhase::Completed;
        tracing::warn!("통합 로그인: OAuth 실패 → 쿠키 로그인 폴백 ({reason})");
        LoginAction::CookieOnlySuccess {
            reason: Some(reason),
        }
    }
}

fn auth_only(cookies: &[StoredCookie]) -> Vec<StoredCookie> {
    cookies
        .iter()
        .filter(|c| c.is_naver_auth_cookie())
        .cloned()
        .collect()
}

fn with_resync(latest: &[StoredCookie], action: LoginAction) -> LoginAction {
    let auth = auth_only(latest);
    if auth.is_empty() || action == LoginAction::None {
        return action;
    }
    LoginAction::IngestCookies {
        cookies: auth,
        next: Box::new(action),
    }
}

/// scheme·host·port·path 가 정확히 같은가 (다른 로컬 서비스로의 이동을 redirect 로 오인하지 않는다).
pub fn is_redirect_uri(url: &Url, redirect_uri: &str) -> bool {
    let Ok(target) = Url::parse(redirect_uri) else {
        return false;
    };
    url.scheme() == target.scheme()
        && url.host_str() == target.host_str()
        && url.port_or_known_default() == target.port_or_known_default()
        && url.path() == target.path()
}

/// OAuth redirect 콜백 해석 결과.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OAuthCallback {
    Code {
        code: String,
        state: Option<String>,
    },
    /// `error` 파라미터 (사용자 거부 등).
    Error(String),
    MissingCode,
}

/// redirect 콜백 URL 에서 `code`·`state`·`error` 를 꺼낸다.
pub fn parse_oauth_callback(url: &Url) -> OAuthCallback {
    if let Some(e) = query_value(url, "error") {
        return OAuthCallback::Error(e);
    }
    match query_value(url, "code") {
        Some(code) if !code.is_empty() => OAuthCallback::Code {
            code,
            state: query_value(url, "state"),
        },
        _ => OAuthCallback::MissingCode,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REDIRECT: &str = "http://localhost:52735/callback";

    fn cookies() -> Vec<StoredCookie> {
        vec![
            StoredCookie::new("NID_AUT", "a", ".naver.com"),
            StoredCookie::new("NID_SES", "s", ".naver.com"),
            StoredCookie::new("NNB", "x", ".naver.com"),
        ]
    }

    fn auth_url() -> Url {
        Url::parse("https://chzzk.naver.com/account-interlock?clientId=c&state=S").unwrap()
    }

    #[test]
    fn cookie_only_when_no_auth_url() {
        let mut f = UnifiedLoginFlow::new(None, REDIRECT);
        assert_eq!(f.on_cookies(&cookies()[..1]), LoginAction::None);
        match f.on_cookies(&cookies()) {
            LoginAction::IngestCookies { cookies, next } => {
                assert_eq!(cookies.len(), 2);
                assert_eq!(*next, LoginAction::CookieOnlySuccess { reason: None });
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(f.phase(), LoginPhase::Completed);
    }

    #[test]
    fn oauth_code_flow() {
        let mut f = UnifiedLoginFlow::new(Some(auth_url()), REDIRECT);
        let LoginAction::IngestCookies { next, .. } = f.on_cookies(&cookies()) else {
            panic!()
        };
        assert_eq!(*next, LoginAction::LoadOAuthConsent(auth_url()));
        assert_eq!(f.phase(), LoginPhase::OAuthConsent);
        // 다른 포트의 로컬 서비스는 가로채지 않는다.
        let other = Url::parse("http://localhost:9999/callback?code=x").unwrap();
        assert_eq!(f.on_navigation(&other, &[]), NavigationDecision::Allow);
        let cb = Url::parse("http://localhost:52735/callback?code=CODE&state=S").unwrap();
        assert_eq!(
            f.on_navigation(&cb, &[]),
            NavigationDecision::Cancel(LoginAction::CodeReceived {
                code: "CODE".into(),
                state: Some("S".into())
            })
        );
        assert_eq!(f.phase(), LoginPhase::Completed);
    }

    #[test]
    fn callback_error_falls_back_with_resync() {
        let mut f = UnifiedLoginFlow::new(Some(auth_url()), REDIRECT);
        f.on_cookies(&cookies());
        let cb = Url::parse("http://localhost:52735/callback?error=access_denied").unwrap();
        let NavigationDecision::Cancel(LoginAction::IngestCookies { cookies, next }) =
            f.on_navigation(&cb, &cookies())
        else {
            panic!()
        };
        assert_eq!(cookies.len(), 2);
        assert!(
            matches!(*next, LoginAction::CookieOnlySuccess { reason: Some(ref r) } if r.contains("access_denied"))
        );
    }

    #[test]
    fn web_errors() {
        let mut f = UnifiedLoginFlow::new(None, REDIRECT);
        assert_eq!(
            f.on_web_error(&WebLoadError::Cancelled, &[]),
            LoginAction::None
        );
        assert!(matches!(
            f.on_web_error(&WebLoadError::Other("x".into()), &cookies()),
            LoginAction::IngestCookies { .. }
        ));
        let mut f = UnifiedLoginFlow::new(None, REDIRECT);
        assert_eq!(
            f.on_web_error(&WebLoadError::CannotConnectToHost, &[]),
            LoginAction::Error(NAVER_LOGIN_LOAD_FAILED.into())
        );
        let mut f = UnifiedLoginFlow::new(Some(auth_url()), REDIRECT);
        f.on_cookies(&cookies());
        assert_eq!(
            f.on_web_error(&WebLoadError::CannotConnectToHost, &[]),
            LoginAction::None
        );
        assert!(matches!(
            f.on_web_error(&WebLoadError::Other("boom".into()), &[]),
            LoginAction::CookieOnlySuccess { .. }
        ));
    }

    #[test]
    fn callback_parsing() {
        let u = |s: &str| Url::parse(s).unwrap();
        assert_eq!(
            parse_oauth_callback(&u("http://l/cb?code=")),
            OAuthCallback::MissingCode
        );
        assert_eq!(
            parse_oauth_callback(&u("http://l/cb?code=c")),
            OAuthCallback::Code {
                code: "c".into(),
                state: None
            }
        );
        assert!(is_redirect_uri(
            &u("http://localhost:52735/callback?x=1"),
            REDIRECT
        ));
        assert!(!is_redirect_uri(
            &u("https://localhost:52735/callback"),
            REDIRECT
        ));
        assert!(!is_redirect_uri(
            &u("http://localhost:52735/other"),
            REDIRECT
        ));
    }
}
