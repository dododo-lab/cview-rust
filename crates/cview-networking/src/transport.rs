//! HTTP 전송 추상화 — Swift `ChzzkAPIClient` 의 `URLSession` / 주입 `transport` 클로저 대응.
//!
//! 테스트는 [`HttpTransport`] 를 구현한 가짜 전송을 넣어 실제 네트워크 없이 클라이언트 동작을 검증한다.
//! 기본 구현은 [`ReqwestTransport`] (rustls).
//!
//! TODO(cert-pinning): `Sources/CViewNetworking/CertificatePinningDelegate.swift` 의 SPKI 핀닝은
//! 아직 포팅하지 않았다. rustls 의 사용자 정의 `ServerCertVerifier` 로 옮겨야 한다.

use std::time::Duration;

use async_trait::async_trait;
use cview_core::protocols::HttpMethod;
use url::Url;

use crate::constants::api_defaults;
use crate::headers;

/// 보낼 요청.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: Url,
    /// 헤더 (이름 대소문자 무시, 같은 이름은 [`HttpRequest::set_header`] 로 덮어쓴다).
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    /// 요청별 타임아웃. `None` 이면 Swift `URLRequest` 기본값(60초)으로 보고 클라이언트 상한을 적용한다.
    pub timeout: Option<Duration>,
}

impl HttpRequest {
    pub fn new(method: HttpMethod, url: Url) -> Self {
        Self {
            method,
            url,
            headers: Vec::new(),
            body: None,
            timeout: None,
        }
    }

    /// 헤더를 설정한다(기존 같은 이름은 지운다). `value` 가 `None` 이면 지우기만 한다.
    pub fn set_header(&mut self, name: &str, value: Option<&str>) {
        self.headers.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
        if let Some(v) = value {
            self.headers.push((name.to_string(), v.to_string()));
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// 받은 응답.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// 전송 오류 — Swift `URLError` 코드 분류 대응.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    /// `.cancelled`
    #[error("cancelled")]
    Cancelled,
    /// `.timedOut` — 재시도 대상.
    #[error("timed out: {0}")]
    TimedOut(String),
    /// `.networkConnectionLost` — 재시도 대상.
    #[error("connection lost: {0}")]
    ConnectionLost(String),
    /// `.notConnectedToInternet`
    #[error("not connected to internet")]
    NotConnected,
    #[error("{0}")]
    Other(String),
}

/// HTTP 전송.
#[async_trait]
pub trait HttpTransport: Send + Sync {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, TransportError>;
}

/// reqwest(rustls) 기반 기본 전송.
///
/// 세션 기본 헤더(User-Agent·Accept·Accept-Language·Referer·Origin)를 붙인다. `Accept-Encoding` 은 reqwest 가
/// 지원하는 압축만 스스로 협상하도록 두었다(원본은 `gzip, deflate, br` 고정).
/// 쿠키 저장소는 쓰지 않는다 — 쿠키는 요청마다 명시적으로 붙이므로, soft-auth 401 재시도에서
/// `Cookie` 헤더를 지우면 그대로 비인증 요청이 된다(Swift 의 `httpShouldHandleCookies = false`).
#[derive(Debug, Clone)]
pub struct ReqwestTransport {
    client: reqwest::Client,
}

impl ReqwestTransport {
    pub fn new() -> Result<Self, reqwest::Error> {
        use reqwest::header::{
            HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, ORIGIN, REFERER, USER_AGENT,
        };
        let mut defaults = HeaderMap::new();
        defaults.insert(
            USER_AGENT,
            HeaderValue::from_static(headers::CHROME_USER_AGENT),
        );
        defaults.insert(ACCEPT, HeaderValue::from_static("application/json"));
        defaults.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("ko-KR,ko;q=0.9"));
        defaults.insert(REFERER, HeaderValue::from_static(headers::API_REFERER));
        defaults.insert(ORIGIN, HeaderValue::from_static(headers::CHZZK_ORIGIN));
        let client = reqwest::Client::builder()
            .default_headers(defaults)
            .connect_timeout(Duration::from_secs_f64(api_defaults::REQUEST_TIMEOUT))
            .timeout(Duration::from_secs_f64(api_defaults::RESOURCE_TIMEOUT))
            .pool_max_idle_per_host(6)
            .build()?;
        Ok(Self { client })
    }

    pub fn with_client(client: reqwest::Client) -> Self {
        Self { client }
    }
}

fn to_reqwest_method(method: HttpMethod) -> reqwest::Method {
    match method {
        HttpMethod::Get => reqwest::Method::GET,
        HttpMethod::Post => reqwest::Method::POST,
        HttpMethod::Put => reqwest::Method::PUT,
        HttpMethod::Delete => reqwest::Method::DELETE,
    }
}

fn map_reqwest_error(e: reqwest::Error) -> TransportError {
    if e.is_timeout() {
        TransportError::TimedOut(e.to_string())
    } else if e.is_connect() || e.is_request() {
        TransportError::ConnectionLost(e.to_string())
    } else {
        TransportError::Other(e.to_string())
    }
}

#[async_trait]
impl HttpTransport for ReqwestTransport {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
        let mut builder = self
            .client
            .request(to_reqwest_method(request.method), request.url);
        for (name, value) in &request.headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
        if let Some(body) = request.body {
            builder = builder.body(body);
        }
        if let Some(timeout) = request.timeout {
            builder = builder.timeout(timeout);
        }
        let response = builder.send().await.map_err(map_reqwest_error)?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(k, v)| {
                v.to_str()
                    .ok()
                    .map(|v| (k.as_str().to_string(), v.to_string()))
            })
            .collect();
        let body = response.bytes().await.map_err(map_reqwest_error)?.to_vec();
        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_set_replaces_case_insensitively() {
        let mut r = HttpRequest::new(HttpMethod::Get, Url::parse("https://x.test/").unwrap());
        r.set_header("Cookie", Some("a=1"));
        r.set_header("cookie", Some("b=2"));
        assert_eq!(r.headers.len(), 1);
        assert_eq!(r.header("COOKIE"), Some("b=2"));
        r.set_header("Cookie", None);
        assert!(r.header("Cookie").is_none());
        let resp = HttpResponse::new(429, "").with_header("Retry-After", "7");
        assert_eq!(resp.header("retry-after"), Some("7"));
    }
}
