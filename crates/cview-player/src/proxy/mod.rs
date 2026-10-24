//! `Sources/CViewPlayer/LocalStreamProxy.swift` (+ `+Lifecycle`·`+Authentication`·`+Routing`·
//! `+Requests`·`+StreamRequest`·`+Upstream`·`+Response`·`+SegmentStream`·`+SegmentMetrics`·`+ByteRange`·
//! `+SMPToken`·`+SupplyGapCause`·`+Timeshift`, `ProxyTransferDiagnostics`, `ProxyRedirectDelegate`) 포팅.
//!
//! 로컬 HTTP 리버스 프록시 — hls.js(WebView) → `127.0.0.1:PORT` → CDN(HTTPS).
//! - 잘못 표시된 fMP4 `Content-Type` 교정, 재생목록 URI 재작성(`/_p_/HOST` 크로스CDN 라우팅)
//! - 단일 Range 전달·200 무시 시 창 잘라내기·416, CORS(오류 응답 포함)·OPTIONS 사전 요청
//! - CHZZK AES 키 URL 에만 로그인 쿠키, 리다이렉트는 허용 CDN 으로만
//! - SMP 토큰(`__bgda__`), 로컬 타임시프트(`/_ts/`) 서빙·복사, 공급 공백·상류 지연 계측
//!
//! Swift `NWListener` + 수동 HTTP 파싱 대신 hyper 1.x(HTTP/1.1 keep-alive·요청 프레이밍·클라이언트
//! 이탈 시 응답 본문 drop)와 reqwest 상류를 쓴다. 그래서 `+RequestFraming`·`+ClientWatch` 는 hyper 가
//! 대신한다. 차이점:
//! - 길이를 모르는 응답은 연결 종료 구분 대신 chunked 로 보낸다(잘림이 오류로 드러나는 점은 같다).
//! - reqwest 빌드에 압축 해제 기능이 없어 재생목록도 `Accept-Encoding: identity` 로 요청한다.

pub mod byte_range;
pub mod routing;
mod server;
pub mod smp_token;
pub mod telemetry;
pub mod timeshift;
pub mod transfer_diagnostics;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use cview_core::models::proxy_stats::ProxyNetworkStats;
use cview_core::utils::media_url_policy;
use thiserror::Error;
use tokio::task::JoinHandle;
use url::Url;

use crate::manifest::{ByteRange, HlsByteSpan};
use crate::policy::constants::proxy_defaults;
use crate::policy::playback_key_access::{CookieProvider, PlaybackCookie};
use smp_token::SmpSegmentTokenState;
use telemetry::{ProxyPerfWindow, ProxyTelemetry, SupplyGapCause};
use timeshift::{TimeshiftBackend, TIMESHIFT_PATH_PREFIX};
use transfer_diagnostics::{ProxyTransferDiagnostic, ProxyTransferTrace, TransferRecords};

/// 프록시 오류.
#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("프록시 시작 시간 초과")]
    StartTimeout,
    #[error("허용되지 않은 미디어 호스트")]
    DisallowedHost,
    #[error("프록시 리스너를 열 수 없음: {0}")]
    Bind(#[from] std::io::Error),
}

/// 테스트용 상류 대체 — 논리 URL(`https://cdn/...`)을 실제로 요청할 URL 로 바꾼다.
/// `Host` 헤더와 정책 판정은 늘 논리 URL 기준이다.
pub type UpstreamOverride = Arc<dyn Fn(&Url) -> Url + Send + Sync>;

/// 상류 인증 실패(연속 403) 콜백.
pub type AuthFailureHandler = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct ProxyState {
    is_running: bool,
    port: u16,
    target_host: String,
    generation: u64,
    server: Option<JoinHandle<()>>,
}

struct SessionSlot {
    client: Option<reqwest::Client>,
    /// `stop()` 뒤에는 false — 정지 중 도착한 요청이 세션을 되살리지 않게.
    accepting: bool,
}

pub(crate) struct Inner {
    lifecycle: tokio::sync::Mutex<()>,
    state: Mutex<ProxyState>,
    session: Mutex<SessionSlot>,
    pub(crate) telemetry: ProxyTelemetry,
    records: TransferRecords,
    smp: Mutex<SmpSegmentTokenState>,
    timeshift: Mutex<Option<Arc<dyn TimeshiftBackend>>>,
    auth_failure: Mutex<Option<AuthFailureHandler>>,
    cookies: CookieProvider,
    upstream_override: Option<UpstreamOverride>,
    pub(crate) active_connections: Arc<AtomicUsize>,
    epoch: Instant,
    /// 성능 로그에 붙일 이름(채널 ID 앞 6자) — 코디네이터가 단다.
    perf_tag: Mutex<String>,
}

/// 살아 있는 프록시 목록 — 성능 로그 표본 루프가 10초마다 세그먼트 수신 요약을 꺼내 간다(약한 참조).
static PERF_REGISTRY: Mutex<Vec<std::sync::Weak<Inner>>> = Mutex::new(Vec::new());

/// 살아 있는 프록시마다 지난 창의 세그먼트 수신 요약(이름, 요약). 이 창에 일이 없던 프록시는 빠진다.
pub fn take_perf_windows() -> Vec<(String, ProxyPerfWindow)> {
    let mut registry = lock(&PERF_REGISTRY);
    registry.retain(|w| w.strong_count() > 0);
    registry
        .iter()
        .filter_map(|w| w.upgrade())
        .filter_map(|inner| {
            let window = inner.telemetry.take_perf_window()?;
            Some((lock(&inner.perf_tag).clone(), window))
        })
        .collect()
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// 상류 응답 요약(타임시프트 레코더 fetch 용).
#[derive(Debug, Clone, PartialEq)]
pub struct UpstreamFetchResponse {
    pub status: u16,
    pub url: Option<Url>,
    pub content_range: Option<String>,
}

/// 로컬 스트림 프록시. 복제해도 같은 프록시를 가리킨다.
#[derive(Clone)]
pub struct LocalStreamProxy {
    pub(crate) inner: Arc<Inner>,
}

impl Default for LocalStreamProxy {
    fn default() -> Self {
        Self::new(Arc::new(Vec::new))
    }
}

impl LocalStreamProxy {
    /// `cookies` 는 요청 시점마다 현재 로그인 쿠키를 읽는다(키 URL 에만 붙는다).
    pub fn new(cookies: CookieProvider) -> Self {
        Self::build(cookies, None)
    }

    /// 상류를 대체한 프록시(테스트의 로컬 모의 CDN).
    pub fn with_upstream_override(cookies: CookieProvider, upstream: UpstreamOverride) -> Self {
        Self::build(cookies, Some(upstream))
    }

    fn build(cookies: CookieProvider, upstream_override: Option<UpstreamOverride>) -> Self {
        let proxy = Self {
            inner: Arc::new(Inner {
                lifecycle: tokio::sync::Mutex::new(()),
                state: Mutex::new(ProxyState::default()),
                session: Mutex::new(SessionSlot {
                    client: None,
                    accepting: true,
                }),
                telemetry: ProxyTelemetry::default(),
                records: TransferRecords::default(),
                smp: Mutex::new(SmpSegmentTokenState::default()),
                timeshift: Mutex::new(None),
                auth_failure: Mutex::new(None),
                cookies,
                upstream_override,
                active_connections: Arc::new(AtomicUsize::new(0)),
                epoch: Instant::now(),
                perf_tag: Mutex::new(String::new()),
            }),
        };
        lock(&PERF_REGISTRY).push(Arc::downgrade(&proxy.inner));
        proxy
    }

    /// 성능 로그 이름(채널 ID 앞 6자 등).
    pub fn set_perf_tag(&self, tag: &str) {
        *lock(&self.inner.perf_tag) = tag.to_string();
    }

    // MARK: - 상태

    pub fn port(&self) -> u16 {
        lock(&self.inner.state).port
    }

    pub fn target_host(&self) -> String {
        lock(&self.inner.state).target_host.clone()
    }

    pub fn is_running(&self) -> bool {
        lock(&self.inner.state).is_running
    }

    pub fn active_connections(&self) -> usize {
        self.inner.active_connections.load(Ordering::SeqCst)
    }

    /// 단조 시각(초).
    pub(crate) fn monotonic_now(&self) -> f64 {
        self.inner.epoch.elapsed().as_secs_f64()
    }

    pub fn set_on_upstream_auth_failure(&self, handler: Option<AuthFailureHandler>) {
        *lock(&self.inner.auth_failure) = handler;
    }

    pub(crate) fn notify_auth_failure(&self) {
        let handler = lock(&self.inner.auth_failure).clone();
        if let Some(h) = handler {
            h();
        }
    }

    pub(crate) fn current_cookies(&self) -> Vec<PlaybackCookie> {
        (self.inner.cookies)()
    }

    // MARK: - 수명

    /// 프록시를 `host` 대상으로 띄우고 포트를 돌려준다. 같은 호스트로 이미 돌고 있으면 그 포트,
    /// 다른 호스트면 멈추고 새로 띄운다. 동시 호출은 하나의 리스너로 합류한다.
    pub async fn start(&self, host: &str) -> Result<u16, ProxyError> {
        if !media_url_policy::is_allowed_host(host) {
            return Err(ProxyError::DisallowedHost);
        }
        let _guard = self.inner.lifecycle.lock().await;
        {
            let s = lock(&self.inner.state);
            if s.is_running && s.target_host == host && s.port > 0 {
                return Ok(s.port);
            }
        }
        self.stop_locked();
        let listener = tokio::time::timeout(
            Duration::from_secs(3),
            tokio::net::TcpListener::bind(("127.0.0.1", 0)),
        )
        .await
        .map_err(|_| ProxyError::StartTimeout)??;
        let port = listener.local_addr()?.port();
        if port == 0 {
            return Err(ProxyError::StartTimeout);
        }
        lock(&self.inner.session).accepting = true;
        let generation = {
            let mut s = lock(&self.inner.state);
            s.generation = s.generation.wrapping_add(1);
            s.target_host = host.to_string();
            s.port = port;
            s.is_running = true;
            s.generation
        };
        let task = tokio::spawn(server::accept_loop(
            Arc::downgrade(&self.inner),
            listener,
            generation,
        ));
        lock(&self.inner.state).server = Some(task);
        tracing::info!("Proxy started: localhost:{port}");
        Ok(port)
    }

    /// 리스너·연결·상류 세션을 닫고 기록과 계측을 비운다.
    pub fn stop(&self) {
        self.stop_locked();
    }

    fn stop_locked(&self) {
        let server = {
            let mut s = lock(&self.inner.state);
            let next = s.generation.wrapping_add(1);
            let server = s.server.take();
            *s = ProxyState {
                generation: next,
                ..Default::default()
            };
            server
        };
        if let Some(task) = server {
            // 리스너와 연결 태스크(JoinSet)가 함께 내려간다.
            task.abort();
        }
        {
            let mut slot = lock(&self.inner.session);
            slot.client = None;
            slot.accepting = false;
        }
        self.detach_timeshift();
        {
            let mut smp = lock(&self.inner.smp);
            smp.token = None;
            smp.logged_attach = false;
        }
        self.inner.telemetry.reset_consecutive_403();
        self.reset_network_stats();
        *lock(&self.inner.auth_failure) = None;
        tracing::info!("Proxy stopped, session invalidated");
    }

    /// 상류 세션만 리셋 — 오래된 연결 풀 정리(재연결 시).
    pub fn reset_session(&self) {
        lock(&self.inner.session).client = None;
        self.inner.telemetry.reset_consecutive_403();
        tracing::info!("Proxy session reset — stale connections cleared");
    }

    /// 상류 클라이언트 get-or-create. 정지된 프록시면 `None` (세션을 되살리지 않는다).
    pub fn upstream_client(&self) -> Option<reqwest::Client> {
        let mut slot = lock(&self.inner.session);
        if !slot.accepting {
            return None;
        }
        if slot.client.is_none() {
            slot.client = Some(self.make_client());
        }
        slot.client.clone()
    }

    /// 상류 세션이 만들어져 있는가(진단용).
    pub fn has_session(&self) -> bool {
        lock(&self.inner.session).client.is_some()
    }

    fn make_client(&self) -> reqwest::Client {
        let mut builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs_f64(proxy_defaults::REQUEST_TIMEOUT))
            .read_timeout(Duration::from_secs_f64(
                proxy_defaults::UPSTREAM_REQUEST_TIMEOUT,
            ))
            .timeout(Duration::from_secs_f64(proxy_defaults::RESOURCE_TIMEOUT))
            .pool_max_idle_per_host(proxy_defaults::MAX_CONNECTIONS_PER_HOST);
        if self.inner.upstream_override.is_some() {
            builder = builder.no_proxy();
        }
        builder.build().unwrap_or_default()
    }

    /// 논리 URL → 실제 요청 URL.
    pub(crate) fn physical_url(&self, logical: &Url) -> Url {
        match &self.inner.upstream_override {
            Some(f) => f(logical),
            None => logical.clone(),
        }
    }

    // MARK: - URL 변환

    /// 원본 URL 을 프록시 URL 로. 프록시가 꺼져 있거나 허용 외 URL 이면 그대로.
    pub fn proxy_url(&self, original: &Url) -> Url {
        let (running, port, host) = {
            let s = lock(&self.inner.state);
            (s.is_running, s.port, s.target_host.clone())
        };
        if !running {
            return original.clone();
        }
        routing::proxy_url(original, port, &host)
            .and_then(|s| Url::parse(&s).ok())
            .unwrap_or_else(|| original.clone())
    }

    pub fn needs_proxy(url: &Url) -> bool {
        media_url_policy::allows(url)
    }

    /// 재생목록 본문을 재작성한다. `#EXTM3U` 로 시작하지 않으면 `None`.
    pub fn rewritten_playlist(
        &self,
        data: &[u8],
        request_url: &Url,
        response_url: Option<&Url>,
    ) -> Option<Vec<u8>> {
        let text = std::str::from_utf8(data).ok()?;
        let first = text.split(['\n', '\r']).next().unwrap_or("");
        if first != "#EXTM3U" {
            return None;
        }
        if let Some(rec) = self.timeshift_backend() {
            rec.observe_playlist(text, request_url, true, response_url);
        }
        let (port, host) = {
            let s = lock(&self.inner.state);
            (s.port, s.target_host.clone())
        };
        let base = format!("http://127.0.0.1:{port}");
        let absolute = routing::rewrite_m3u8_urls(text, &base, &host);
        let base_path = routing::playlist_base_path(request_url.as_str(), response_url, &host);
        let final_url = response_url.unwrap_or(request_url);
        let response_proxy = self.proxy_url(final_url).to_string();
        Some(
            routing::make_relative_urls_absolute(
                &absolute,
                &base,
                &base_path,
                Some(&response_proxy),
                &host,
            )
            .into_bytes(),
        )
    }

    // MARK: - SMP 토큰

    /// 타임머신 스트림이면 켠다. 켜고 끌 때마다 저장한 토큰을 비운다.
    pub fn set_smp_segment_token_rewrite(&self, enabled: bool) {
        *lock(&self.inner.smp) = SmpSegmentTokenState::new(enabled);
    }

    pub fn apply_smp_segment_token(&self, url: &Url) -> Url {
        lock(&self.inner.smp).apply(url)
    }

    // MARK: - 계측

    pub fn network_stats(&self) -> ProxyNetworkStats {
        self.inner
            .telemetry
            .network_stats(self.active_connections() as i64)
    }

    pub fn reset_network_stats(&self) {
        self.inner.records.clear();
        self.inner.telemetry.reset();
    }

    pub fn transfer_diagnostics(&self) -> Vec<ProxyTransferDiagnostic> {
        self.inner.records.snapshot()
    }

    /// 세그먼트 요청 도착(계측) — `now` 는 단조 초.
    pub fn note_segment_request_arrived(&self, now: f64) -> Option<SupplyGapCause> {
        self.inner.telemetry.note_segment_request_arrived(now)
    }

    pub(crate) fn make_transfer_trace(&self, kind: &str) -> Arc<ProxyTransferTrace> {
        let generation = lock(&self.inner.state).generation;
        let weak = Arc::downgrade(&self.inner);
        Arc::new(ProxyTransferTrace::new(
            generation,
            kind,
            Arc::new(move |record| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                if lock(&inner.state).generation != generation {
                    return;
                }
                if inner.records.insert(record.clone()) {
                    tracing::debug!(
                        "ProxyTransfer id={} kind={} status={} outcome={} header={} body={} drain={} received={} enqueued={} processed={} peakPending={}",
                        record.id, record.kind, record.status, record.outcome, record.header_wait,
                        record.body_time, record.send_drain_time, record.upstream_bytes,
                        record.enqueued_bytes, record.processed_bytes, record.peak_pending_bytes
                    );
                }
            }),
        ))
    }

    // MARK: - 타임시프트

    pub fn timeshift_backend(&self) -> Option<Arc<dyn TimeshiftBackend>> {
        lock(&self.inner.timeshift).clone()
    }

    /// 레코더를 붙인다. 이미 붙어 있으면 그것을 멈추고 교체한다.
    pub fn attach_timeshift(&self, backend: Arc<dyn TimeshiftBackend>) {
        let previous = lock(&self.inner.timeshift).replace(backend);
        if let Some(p) = previous {
            p.stop();
        }
    }

    /// 레코더를 떼고 멈춘다.
    pub fn detach_timeshift(&self) {
        let previous = lock(&self.inner.timeshift).take();
        if let Some(p) = previous {
            p.stop();
        }
    }

    /// hls.js 에 줄 로컬 재생목록 URL. `start_offset` 은 엣지 기준 몇 초 전에서 시작할지.
    pub fn timeshift_playlist_url(&self, start_offset: f64) -> Option<Url> {
        let port = self.port();
        let backend = self.timeshift_backend()?;
        if port == 0 {
            return None;
        }
        Url::parse(&format!(
            "http://127.0.0.1:{port}{TIMESHIFT_PATH_PREFIX}{}/index.m3u8?start={:.3}",
            backend.session_id(),
            start_offset.max(0.0)
        ))
        .ok()
    }

    /// 레코더의 상류 요청 — hls.js 요청과 같은 헤더·같은 세션.
    pub async fn upstream_fetch(
        &self,
        url: &Url,
        byte_range: Option<ByteRange>,
    ) -> (Option<Vec<u8>>, UpstreamFetchResponse) {
        let failed = UpstreamFetchResponse {
            status: 0,
            url: None,
            content_range: None,
        };
        if !media_url_policy::allows(url) {
            return (None, failed);
        }
        let range = match byte_range {
            Some(r) => {
                match HlsByteSpan::validated(r.length, Some(r.offset.unwrap_or(0))).range_header() {
                    Some(h) => Some(h),
                    None => return (None, failed),
                }
            }
            None => None,
        };
        let Some(client) = self.upstream_client() else {
            return (None, failed);
        };
        match server::send_upstream(self, &client, url, range.as_deref(), false).await {
            Ok((resp, final_url)) => {
                let status = resp.status().as_u16();
                let content_range = resp
                    .headers()
                    .get(http::header::CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string);
                let body = resp.bytes().await.ok().map(|b| b.to_vec());
                (
                    body,
                    UpstreamFetchResponse {
                        status,
                        url: Some(final_url),
                        content_range,
                    },
                )
            }
            Err(_) => (None, failed),
        }
    }
}

#[cfg(test)]
mod tests;
