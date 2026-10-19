//! `Sources/CViewPlayer/PlaybackKeyAccess.swift` 포팅 (+ `ProxyRedirectDelegate.swift` 의 리다이렉트 판정).
//!
//! 로그인 자격 증명은 CHZZK 서버가 발급하는 AES 키 엔드포인트에만 붙는다.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use cview_core::models::stream_proxy::StreamProxyMode;
use cview_core::utils::{media_url_policy, naver_cookie_policy};
use http::{HeaderMap, HeaderValue};
use url::Url;

/// 재생 키 요청에 쓸 쿠키 (Swift `HTTPCookie` 의 필요한 부분).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybackCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub expires: Option<DateTime<Utc>>,
}

impl PlaybackCookie {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            domain: ".naver.com".into(),
            path: "/".into(),
            expires: None,
        }
    }
}

/// 요청 시점마다 현재 로그인을 읽는 쿠키 공급자.
pub type CookieProvider = Arc<dyn Fn() -> Vec<PlaybackCookie> + Send + Sync>;

/// 현재 로그인 상태(로그아웃·쿠키 회전 포함)를 요청 시점에 읽는다.
#[derive(Clone)]
pub struct PlaybackKeyAccess {
    pub requires_proxy: bool,
    pub cookies: CookieProvider,
}

impl std::fmt::Debug for PlaybackKeyAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlaybackKeyAccess")
            .field("requires_proxy", &self.requires_proxy)
            .finish()
    }
}

impl Default for PlaybackKeyAccess {
    fn default() -> Self {
        Self::anonymous()
    }
}

impl PlaybackKeyAccess {
    pub fn anonymous() -> Self {
        Self {
            requires_proxy: false,
            cookies: Arc::new(Vec::new),
        }
    }

    pub fn new(requires_proxy: bool, cookies: CookieProvider) -> Self {
        Self {
            requires_proxy,
            cookies,
        }
    }

    pub fn proxy_mode(&self, requested: StreamProxyMode) -> StreamProxyMode {
        if self.requires_proxy {
            StreamProxyMode::LocalProxy
        } else {
            requested
        }
    }
}

/// `https://api.chzzk.naver.com/service/v1/encryption/lives/<digits>/aes_key` 인지.
pub fn is_key_url(url: &Url) -> bool {
    if !media_url_policy::allows(url)
        || url.host_str().map(str::to_lowercase).as_deref() != Some("api.chzzk.naver.com")
    {
        return false;
    }
    let parts: Vec<&str> = url.path().split('/').collect();
    parts.len() == 7
        && parts[0].is_empty()
        && parts[1] == "service"
        && parts[2] == "v1"
        && parts[3] == "encryption"
        && parts[4] == "lives"
        && parts[6] == "aes_key"
        && !parts[5].is_empty()
        && parts[5].bytes().all(|b| b.is_ascii_digit())
}

/// 키 URL 에 보낼 `Cookie` 헤더 값(적격 쿠키가 없으면 `None`).
pub fn eligible_cookie_header(
    url: &Url,
    cookies: &[PlaybackCookie],
    now: DateTime<Utc>,
) -> Option<String> {
    let host = url.host_str().unwrap_or("").to_lowercase();
    let path = url.path();
    let eligible: Vec<&PlaybackCookie> = cookies
        .iter()
        .filter(|c| {
            if !naver_cookie_policy::is_authentication_cookie(&c.name, &c.domain)
                || c.value.is_empty()
                || c.expires.is_some_and(|e| e <= now)
            {
                return false;
            }
            let domain = c.domain.to_lowercase();
            let domain = domain.trim_matches('.');
            let domain_matches = host == domain
                || (c.domain.starts_with('.') && host.ends_with(&format!(".{domain}")));
            let path_matches = path == c.path
                || (path.starts_with(&c.path)
                    && (c.path.ends_with('/') || path[c.path.len()..].starts_with('/')));
            domain_matches && path_matches
        })
        .collect();
    if eligible.is_empty() {
        return None;
    }
    Some(
        eligible
            .iter()
            .map(|c| format!("{}={}", c.name, c.value))
            .collect::<Vec<_>>()
            .join("; "),
    )
}

/// Swift `ChzzkPlaybackKeyAuthentication.applying` — 키 URL 의 GET 에만 쿠키 헤더를 설정(또는 제거)한다.
/// 그 밖의 요청은 손대지 않는다.
pub fn apply_key_authentication(
    url: &Url,
    method: &http::Method,
    headers: &mut HeaderMap,
    cookies: &[PlaybackCookie],
    now: DateTime<Utc>,
) {
    if !is_key_url(url) || method != http::Method::GET {
        return;
    }
    headers.remove(http::header::COOKIE);
    if let Some(value) = eligible_cookie_header(url, cookies, now) {
        if let Ok(v) = HeaderValue::from_str(&value) {
            headers.insert(http::header::COOKIE, v);
        }
    }
}

/// `ProxyRedirectDelegate.allowedRequest` — 리다이렉트 대상이 허용 호스트일 때만 따라가고,
/// 키 URL 이 아니면 쿠키를 지우며 `Host` 를 새 호스트로 바꾼다. 거절이면 `false`.
pub fn allow_redirect(url: &Url, headers: &mut HeaderMap) -> bool {
    if !media_url_policy::allows(url) {
        return false;
    }
    if !is_key_url(url) {
        headers.remove(http::header::COOKIE);
    }
    if let Some(host) = url.host_str().and_then(|h| HeaderValue::from_str(h).ok()) {
        headers.insert(http::header::HOST, host);
    }
    true
}

#[cfg(test)]
mod tests {
    //! `Tests/CViewPlayerTests/PlaybackKeyAccessTests.swift` 의 순수 부분 포팅.
    use super::*;

    const KEY: &str = "https://api.chzzk.naver.com/service/v1/encryption/lives/21415132/aes_key";

    fn cookie(name: &str, value: &str, domain: &str, path: &str) -> PlaybackCookie {
        PlaybackCookie {
            name: name.into(),
            value: value.into(),
            domain: domain.into(),
            path: path.into(),
            expires: None,
        }
    }

    #[test]
    fn accepts_only_exact_key_endpoint() {
        assert!(is_key_url(&Url::parse(KEY).unwrap()));
        for v in [
            "http://api.chzzk.naver.com/service/v1/encryption/lives/1/aes_key",
            "https://api.chzzk.naver.com.evil.com/service/v1/encryption/lives/1/aes_key",
            "https://api.chzzk.naver.com:8443/service/v1/encryption/lives/1/aes_key",
            "https://api.chzzk.naver.com/service/v1/encryption/lives/x/aes_key",
            "https://api.chzzk.naver.com/service/v1/encryption/lives/1/aes_key/",
            "https://api.chzzk.naver.com/service/v1/encryption/lives//aes_key",
            "https://api.chzzk.naver.com/service/v1/users/me",
            "https://video.pstatic.net/service/v1/encryption/lives/1/aes_key",
        ] {
            assert!(!is_key_url(&Url::parse(v).unwrap()), "{v}");
        }
    }

    #[test]
    fn filters_cookies() {
        let mut expired = cookie("NID_SES", "expired", ".naver.com", "/");
        expired.expires = Some(DateTime::from_timestamp(1, 0).unwrap());
        let cookies = vec![
            cookie("NID_SES", "fixture-session", ".naver.com", "/"),
            cookie("NID_AUT", "fixture-auth", ".naver.com", "/"),
            cookie("OTHER", "x", ".naver.com", "/"),
            cookie("NID_SES", "x", ".evil.com", "/"),
            cookie("NID_SES", "x", "nid.naver.com", "/"),
            cookie("NID_SES", "x", ".naver.com", "/service/v10"),
            expired,
        ];
        let mut headers = HeaderMap::new();
        apply_key_authentication(
            &Url::parse(KEY).unwrap(),
            &http::Method::GET,
            &mut headers,
            &cookies,
            Utc::now(),
        );
        let header = headers.get("cookie").unwrap().to_str().unwrap();
        assert!(header.contains("NID_SES=fixture-session"));
        assert!(header.contains("NID_AUT=fixture-auth"));
        assert_eq!(header.split(';').count(), 2);
    }

    #[test]
    fn stale_cookie_removed_and_post_untouched() {
        let mut headers = HeaderMap::new();
        headers.insert("cookie", HeaderValue::from_static("NID_SES=stale"));
        apply_key_authentication(
            &Url::parse(KEY).unwrap(),
            &http::Method::GET,
            &mut headers,
            &[],
            Utc::now(),
        );
        assert!(headers.get("cookie").is_none());
        let mut post = HeaderMap::new();
        post.insert("cookie", HeaderValue::from_static("NID_SES=keep"));
        apply_key_authentication(
            &Url::parse(KEY).unwrap(),
            &http::Method::POST,
            &mut post,
            &[cookie("NID_SES", "v", ".naver.com", "/")],
            Utc::now(),
        );
        assert_eq!(post.get("cookie").unwrap(), "NID_SES=keep");
    }

    #[test]
    fn redirect_strips_credentials_except_key() {
        for v in [
            "https://video.pstatic.net/seg.ts",
            "https://api.chzzk.naver.com/service/v1/users/me",
        ] {
            let mut h = HeaderMap::new();
            h.insert(
                "cookie",
                HeaderValue::from_static("NID_SES=fixture-session"),
            );
            assert!(allow_redirect(&Url::parse(v).unwrap(), &mut h));
            assert!(h.get("cookie").is_none());
        }
        let mut h = HeaderMap::new();
        h.insert(
            "cookie",
            HeaderValue::from_static("NID_SES=fixture-session"),
        );
        assert!(allow_redirect(&Url::parse(KEY).unwrap(), &mut h));
        assert_eq!(h.get("cookie").unwrap(), "NID_SES=fixture-session");
        assert_eq!(h.get("host").unwrap(), "api.chzzk.naver.com");
        assert!(!allow_redirect(
            &Url::parse("https://evil.com/x").unwrap(),
            &mut HeaderMap::new()
        ));
    }

    #[test]
    fn protected_streams_always_use_proxy() {
        for m in StreamProxyMode::ALL {
            let a = PlaybackKeyAccess::new(true, Arc::new(Vec::new));
            assert_eq!(a.proxy_mode(m), StreamProxyMode::LocalProxy);
            assert_eq!(PlaybackKeyAccess::anonymous().proxy_mode(m), m);
        }
    }
}
