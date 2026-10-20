//! 프록시 HTTP 서버 — `LocalStreamProxy+Requests.swift`·`+StreamRequest.swift`·`+SegmentStream.swift`·
//! `+Response.swift`·`+Timeshift.swift`(서빙)·`ProxyRedirectDelegate.swift` 의 hyper/reqwest 구현.

use std::convert::Infallible;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use bytes::Bytes;
use cview_core::utils::media_url_policy;
use cview_networking::headers::{CHZZK_ORIGIN, CHZZK_REFERER, SAFARI_USER_AGENT};
use http::{HeaderMap, HeaderValue, Method, Request, Response, StatusCode};
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::{TokioIo, TokioTimer};
use url::Url;

use super::byte_range::{self, ByteRangeWindow};
use super::routing;
use super::timeshift::{
    parse_timeshift_path, TimeshiftResource, TimeshiftTee, TIMESHIFT_PATH_PREFIX,
};
use super::transfer_diagnostics::ProxyTransferTrace;
use super::{lock, Inner, LocalStreamProxy};
use crate::policy::constants::proxy_defaults;
use crate::policy::media_response_policy as mrp;
use crate::policy::playback_key_access::{allow_redirect, apply_key_authentication, is_key_url};

pub(crate) type Body = BoxBody<Bytes, std::io::Error>;

const MAX_REDIRECTS: usize = 10;

/// 연결 수 가드 — drop 시 감소.
struct ConnectionGuard(Arc<std::sync::atomic::AtomicUsize>);

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// 수락 루프. 루프백 피어만 받고 최대 연결 수를 넘기면 거절한다.
/// 이 태스크가 abort 되면 `JoinSet` 이 떨어지며 모든 연결 태스크도 함께 abort 된다.
pub(crate) async fn accept_loop(
    inner: Weak<Inner>,
    listener: tokio::net::TcpListener,
    generation: u64,
) {
    let mut connections = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((stream, peer)) = accepted else { continue };
                let Some(strong) = inner.upgrade() else { return };
                let proxy = LocalStreamProxy { inner: strong };
                if lock(&proxy.inner.state).generation != generation {
                    return;
                }
                if !peer.ip().is_loopback() {
                    tracing::warn!("Proxy: rejecting non-loopback connection from {peer}");
                    continue;
                }
                let counter = proxy.inner.active_connections.clone();
                let count = counter.fetch_add(1, Ordering::SeqCst) + 1;
                let guard = ConnectionGuard(counter);
                if count > proxy_defaults::MAX_ACTIVE_CONNECTIONS {
                    tracing::warn!(
                        "Proxy: max connections ({}) exceeded, rejecting",
                        proxy_defaults::MAX_ACTIVE_CONNECTIONS
                    );
                    continue;
                }
                let weak = Arc::downgrade(&proxy.inner);
                drop(proxy);
                connections.spawn(async move {
                    let _guard = guard;
                    let service = service_fn(move |req: Request<Incoming>| {
                        let weak = weak.clone();
                        async move {
                            let response = match weak.upgrade() {
                                Some(inner) => LocalStreamProxy { inner }.handle(req).await,
                                None => error_response(503, "Service Unavailable", false),
                            };
                            Ok::<_, Infallible>(response)
                        }
                    });
                    let _ = http1::Builder::new()
                        .timer(TokioTimer::new())
                        .header_read_timeout(Duration::from_secs_f64(
                            proxy_defaults::KEEP_ALIVE_TIMEOUT,
                        ))
                        .keep_alive(true)
                        .max_buf_size(proxy_defaults::MAX_RECEIVE_LENGTH.max(8192))
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
            }
            Some(_) = connections.join_next(), if !connections.is_empty() => {}
        }
    }
}

fn full(data: impl Into<Bytes>) -> Body {
    Full::new(data.into())
        .map_err(|never: Infallible| match never {})
        .boxed()
}

fn empty() -> Body {
    Empty::<Bytes>::new()
        .map_err(|never: Infallible| match never {})
        .boxed()
}

fn set_reason(resp: &mut Response<Body>, reason: &str) {
    if let Ok(r) = hyper::ext::ReasonPhrase::try_from(reason.as_bytes().to_vec()) {
        resp.extensions_mut().insert(r);
    }
}

fn insert(headers: &mut HeaderMap, name: &str, value: &str) {
    if let (Ok(n), Ok(v)) = (
        http::header::HeaderName::from_bytes(name.as_bytes()),
        HeaderValue::from_str(value),
    ) {
        headers.insert(n, v);
    }
}

fn connection_headers(headers: &mut HeaderMap, keep_alive: bool) {
    if keep_alive {
        insert(headers, "Connection", "keep-alive");
        insert(
            headers,
            "Keep-Alive",
            &format!("timeout={}", proxy_defaults::KEEP_ALIVE_TIMEOUT as i64),
        );
    } else {
        insert(headers, "Connection", "close");
    }
}

/// 오류 응답 — 정상 응답과 같은 CORS·캐시 헤더(`errorResponseData`).
pub(crate) fn error_response(status: u16, message: &str, keep_alive: bool) -> Response<Body> {
    let mut resp = Response::new(full(message.to_string()));
    *resp.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
    for (k, v) in byte_range::error_headers(message.len(), keep_alive) {
        insert(resp.headers_mut(), &k, &v);
    }
    set_reason(&mut resp, message);
    resp
}

/// 본문 전체 응답(`sendResponse`).
fn data_response(status: u16, content_type: &str, data: Vec<u8>) -> Response<Body> {
    let len = data.len();
    let mut resp = Response::new(full(data));
    *resp.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
    let h = resp.headers_mut();
    insert(h, "Content-Type", content_type);
    insert(h, "Content-Length", &len.to_string());
    connection_headers(h, true);
    insert(h, "Access-Control-Allow-Origin", "*");
    insert(h, "Cache-Control", "no-cache");
    set_reason(&mut resp, byte_range::reason_phrase(status));
    resp
}

/// 상류 요청 하나 — 리다이렉트를 직접 따라가며 허용 CDN 으로만, 키 URL 이 아니면 쿠키를 지운다.
/// 돌려주는 URL 은 최종 논리 URL(응답 URL).
pub(crate) async fn send_upstream(
    proxy: &LocalStreamProxy,
    client: &reqwest::Client,
    url: &Url,
    byte_range: Option<&str>,
    playlist_path: bool,
) -> Result<(reqwest::Response, Url), reqwest::Error> {
    let mut headers = HeaderMap::new();
    if let Some(host) = url.host_str() {
        insert(&mut headers, "Host", host);
    }
    insert(&mut headers, "User-Agent", SAFARI_USER_AGENT);
    insert(&mut headers, "Referer", CHZZK_REFERER);
    insert(&mut headers, "Origin", CHZZK_ORIGIN);
    if let (false, Some(r)) = (playlist_path, byte_range) {
        insert(&mut headers, "Range", r);
    }
    apply_key_authentication(
        url,
        &Method::GET,
        &mut headers,
        &proxy.current_cookies(),
        chrono::Utc::now(),
    );
    insert(&mut headers, "Accept-Encoding", "identity");

    let mut current = url.clone();
    let mut redirects = 0;
    loop {
        let physical = proxy.physical_url(&current);
        let resp = client
            .get(physical)
            .headers(headers.clone())
            .timeout(Duration::from_secs_f64(proxy_defaults::RESOURCE_TIMEOUT))
            .send()
            .await?;
        let status = resp.status().as_u16();
        if !matches!(status, 301 | 302 | 303 | 307 | 308) || redirects >= MAX_REDIRECTS {
            return Ok((resp, current));
        }
        let Some(next) = resp
            .headers()
            .get(http::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|loc| current.join(loc).ok())
        else {
            return Ok((resp, current));
        };
        let mut redirected = headers.clone();
        if !allow_redirect(&next, &mut redirected) {
            // URLSession 이 리다이렉트를 거절하면 3xx 응답 자체가 전달된다.
            return Ok((resp, current));
        }
        // 재인증: 키 URL 로 갔으면 현재 로그인을 다시 붙인다.
        if is_key_url(&next) {
            apply_key_authentication(
                &next,
                &Method::GET,
                &mut redirected,
                &proxy.current_cookies(),
                chrono::Utc::now(),
            );
        }
        headers = redirected;
        current = next;
        redirects += 1;
    }
}

/// 재시도해도 안전한(일시적) 상류 오류 — 시간 초과·연결 실패·연결 끊김.
pub(crate) fn is_transient_upstream_error(error: &reqwest::Error) -> bool {
    if error.is_timeout() || error.is_connect() {
        return true;
    }
    let mut source: Option<&(dyn std::error::Error + 'static)> = std::error::Error::source(error);
    while let Some(e) = source {
        if let Some(h) = e.downcast_ref::<hyper::Error>() {
            if h.is_incomplete_message() || h.is_canceled() {
                return true;
            }
        }
        if let Some(io) = e.downcast_ref::<std::io::Error>() {
            if matches!(
                io.kind(),
                std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::UnexpectedEof
                    | std::io::ErrorKind::TimedOut
            ) {
                return true;
            }
        }
        source = e.source();
    }
    false
}

fn mask(url: &Url) -> String {
    let s = format!("{}{}", url.host_str().unwrap_or(""), url.path());
    let n = s.chars().count();
    s.chars().skip(n.saturating_sub(80)).collect()
}

/// 이 앱 웹뷰의 출처 — macOS `tauri://localhost`, Windows `http(s)://tauri.localhost`.
const APP_ORIGINS: &[&str] = &[
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
];

/// 프록시를 써도 되는 출처인지. 출처 머리가 없는 요청(영상 요소의 직접 요청·앱 내부 녹화)은 허용하고,
/// 있으면 앱 출처만 — 사용자가 브라우저로 연 다른 웹페이지가 로컬 포트를 찾아 로그인 쿠키가 붙은 키·재생 목록을
/// 읽지 못하게. `null` 출처는 샌드박스 iframe 으로 흉내 낼 수 있어 받지 않는다. 순수 함수.
pub fn origin_allowed(origin: Option<&str>) -> bool {
    match origin {
        None => true,
        Some(o) => APP_ORIGINS.contains(&o.trim()),
    }
}

impl LocalStreamProxy {
    /// 요청 하나 처리.
    pub(crate) async fn handle(&self, req: Request<Incoming>) -> Response<Body> {
        let origin = req.headers().get("origin").and_then(|v| v.to_str().ok());
        if !origin_allowed(origin) {
            tracing::warn!(origin = ?origin, "프록시: 앱이 아닌 출처의 요청을 거절");
            return error_response(403, "Forbidden", false);
        }
        let uri = req.uri();
        // 절대형 요청 대상(`GET http://x/ HTTP/1.1`)은 경로 검사에서 거절되도록 원문 그대로 둔다.
        let raw = if uri.scheme().is_some() || uri.authority().is_some() {
            uri.to_string()
        } else {
            uri.path_and_query()
                .map(|p| p.as_str().to_string())
                .unwrap_or_else(|| "/".into())
        };
        let path = routing::canonicalize_akamai_token_path(&raw);
        match *req.method() {
            Method::OPTIONS => self.answer_preflight(&path, req.headers()),
            Method::GET => self.handle_get(path, req.headers()).await,
            _ => error_response(400, "Bad Request", false),
        }
    }

    fn answer_preflight(&self, path: &str, headers: &HeaderMap) -> Response<Body> {
        let routable = if path.starts_with(TIMESHIFT_PATH_PREFIX) {
            parse_timeshift_path(path).is_some()
        } else {
            routing::upstream_url(path, &self.target_host()).is_some()
        };
        if !routable {
            return error_response(403, "Forbidden", false);
        }
        let requested = headers
            .get("access-control-request-headers")
            .and_then(|v| v.to_str().ok());
        let mut resp = Response::new(empty());
        *resp.status_mut() = StatusCode::NO_CONTENT;
        for (k, v) in byte_range::preflight_headers(requested, proxy_defaults::KEEP_ALIVE_TIMEOUT) {
            insert(resp.headers_mut(), &k, &v);
        }
        resp
    }

    async fn handle_get(&self, path: String, headers: &HeaderMap) -> Response<Body> {
        if path.starts_with(TIMESHIFT_PATH_PREFIX) {
            return self.serve_timeshift(&path);
        }
        if routing::path_looks_like_segment(&path) {
            self.note_segment_request_arrived(self.monotonic_now());
        }
        let Some(proxied) = routing::upstream_url(&path, &self.target_host()) else {
            return error_response(403, "Forbidden", false);
        };
        let target = self.apply_smp_segment_token(&proxied);
        let range = headers
            .get(http::header::RANGE)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|v| byte_range::is_simple_byte_range(v))
            .map(str::to_string);
        self.proxy_to_upstream(target, range).await
    }

    fn serve_timeshift(&self, path: &str) -> Response<Body> {
        let backend = self.timeshift_backend();
        let (Some(request), Some(backend)) = (parse_timeshift_path(path), backend) else {
            return error_response(404, "Not Found", true);
        };
        if backend.session_id() != request.session_id {
            return error_response(404, "Not Found", true);
        }
        match request.resource {
            TimeshiftResource::Playlist { start_offset } => {
                match backend.local_playlist(start_offset) {
                    Some(text) => {
                        data_response(200, "application/vnd.apple.mpegurl", text.into_bytes())
                    }
                    None => error_response(503, "Service Unavailable", true),
                }
            }
            TimeshiftResource::Segment { seq } => match backend.local_segment(seq) {
                Some(d) => data_response(200, "video/mp4", d),
                None => error_response(404, "Not Found", true),
            },
            TimeshiftResource::InitSegment { id } => match backend.local_init(id) {
                Some(d) => data_response(200, "video/mp4", d),
                None => error_response(404, "Not Found", true),
            },
        }
    }

    /// 공통 응답 진입점 — 헤더 전 일시적 실패는 같은 요청으로 1회 재시도한다.
    async fn proxy_to_upstream(&self, target: Url, byte_range: Option<String>) -> Response<Body> {
        if !media_url_policy::allows(&target) {
            return error_response(502, "Bad Gateway", false);
        }
        let mut retries_left = 1;
        loop {
            match self
                .attempt(&target, byte_range.as_deref(), retries_left > 0)
                .await
            {
                Attempt::Done(resp) => return *resp,
                Attempt::Retry => {
                    retries_left -= 1;
                    tracing::debug!(
                        "Proxy(stream) transient 실패 — 1회 재시도 [{}]",
                        mask(&target)
                    );
                }
            }
        }
    }

    async fn attempt(&self, target: &Url, byte_range: Option<&str>, may_retry: bool) -> Attempt {
        let playlist_path = mrp::playlist_path(target);
        let blocking = mrp::is_blocking_playlist(target);
        let is_playlist_request = playlist_path || blocking;
        let mut kind = if blocking {
            "blockingPlaylist"
        } else if is_playlist_request {
            "playlist"
        } else if is_key_url(target) {
            "key"
        } else {
            "media"
        };
        let trace = self.make_transfer_trace(kind);
        let mut tee = if byte_range.is_none() && !playlist_path {
            self.timeshift_backend().and_then(|backend| {
                backend
                    .tee_destination(target)
                    .map(|d| TimeshiftTee::new(backend, d))
            })
        } else {
            None
        };
        let requested_range = if playlist_path { None } else { byte_range };
        let start = Instant::now();

        let Some(client) = self.upstream_client() else {
            if let Some(t) = tee.as_mut() {
                t.abandon();
            }
            return Attempt::done(error_response(502, "Bad Gateway", false));
        };
        let sent = send_upstream(self, &client, target, requested_range, playlist_path).await;
        let (resp, response_url) = match sent {
            Ok(v) => v,
            Err(error) => {
                let elapsed = start.elapsed().as_secs_f64();
                if is_playlist_request {
                    self.inner.telemetry.note_playlist_upstream_wait(elapsed);
                }
                self.inner.telemetry.record_segment_metrics(elapsed, 0);
                tracing::warn!("Proxy(stream) upstream error: {error} [{}]", mask(target));
                if let Some(t) = tee.as_mut() {
                    t.finish(false);
                }
                if may_retry && is_transient_upstream_error(&error) && self.is_running() {
                    trace.finish("retry");
                    return Attempt::Retry;
                }
                trace.finish("upstreamError");
                return Attempt::done(error_response(502, "Bad Gateway", true));
            }
        };
        let ttfb = start.elapsed().as_secs_f64();
        let status = resp.status().as_u16();
        let header = |name: http::header::HeaderName| {
            resp.headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let original_type = header(http::header::CONTENT_TYPE);

        if mrp::is_playlist(
            status,
            original_type.as_deref(),
            Some(&response_url),
            target,
        ) {
            kind = if blocking {
                "blockingPlaylist"
            } else {
                "playlist"
            };
            if let Some(mut t) = tee.take() {
                t.abandon();
            }
            return self
                .complete_playlist(resp, target, &response_url, &trace, kind, start, may_retry)
                .await;
        }

        let content_type = mrp::content_type(
            status,
            original_type.as_deref(),
            Some(&response_url),
            target,
        );
        trace.response(
            status,
            kind,
            Some(content_type.as_str()) != original_type.as_deref(),
        );
        let encoding = header(http::header::CONTENT_ENCODING).map(|e| e.to_lowercase());
        let identity = encoding.as_deref().is_none_or(|e| e == "identity");
        if status == 206 && !identity {
            trace.finish("encodedRangeRejected");
            if let Some(t) = tee.as_mut() {
                t.abandon();
            }
            return Attempt::done(error_response(502, "Bad Gateway", true));
        }
        let length: i64 = if identity {
            resp.content_length().map(|l| l as i64).unwrap_or(-1)
        } else {
            -1
        };
        let keep_alive = length > 0;
        if let Some(t) = tee.as_mut() {
            t.note_response(status, length);
        }

        // [F05] 상류 206/416 은 그대로, Range 를 무시한 200 은 요청 창의 206(만족 불가면 416)으로.
        let mut head_status = status;
        let mut head_length = if keep_alive { length } else { -1 };
        let mut head_range = header(http::header::CONTENT_RANGE);
        let mut window = None;
        let mut window_done = false;
        if let (200, Some(requested)) = (status, requested_range) {
            if let Some(w) = ByteRangeWindow::new(requested, length) {
                head_status = 206;
                head_length = w.content_length();
                head_range = Some(w.content_range());
                window = Some(w);
            } else if ByteRangeWindow::is_unsatisfiable(requested, length) {
                head_status = 416;
                head_length = 0;
                head_range = Some(format!("bytes */{length}"));
                window_done = true;
            } else {
                tracing::warn!(
                    "Proxy(stream) 상류가 Range 를 무시했는데 길이를 몰라 200 전체를 넘긴다"
                );
            }
        }
        if head_status >= 400 {
            tracing::warn!("Proxy(stream) → CDN: HTTP {head_status} [{}]", mask(target));
            if self.inner.telemetry.record_segment_http_error(head_status) {
                self.notify_auth_failure();
            }
        } else {
            self.inner.telemetry.record_segment_ok();
        }

        let mut builder = Response::builder()
            .status(StatusCode::from_u16(head_status).unwrap_or(StatusCode::BAD_GATEWAY))
            .header("Content-Type", content_type);
        if head_length >= 0 {
            builder = builder.header("Content-Length", head_length.to_string());
        }
        if let Some(range) = byte_range::relayable_content_range(head_range.as_deref()) {
            builder = builder
                .header("Content-Range", range)
                .header("Access-Control-Expose-Headers", "Content-Range");
        }
        if !identity {
            if let Some(e) = &encoding {
                // URLSession 은 본문을 풀어 넘겼다. 여기서는 원문을 넘기므로 인코딩을 알린다.
                builder = builder.header("Content-Encoding", e.as_str());
            }
        }
        let keep_alive_downstream = keep_alive || head_length >= 0;
        let mut resp_out = builder
            .body(empty())
            .unwrap_or_else(|_| error_response(502, "Bad Gateway", true));
        connection_headers(resp_out.headers_mut(), keep_alive_downstream);
        insert(resp_out.headers_mut(), "Access-Control-Allow-Origin", "*");
        insert(resp_out.headers_mut(), "Cache-Control", "no-cache");
        set_reason(&mut resp_out, byte_range::reason_phrase(head_status));

        if window_done {
            // 416 으로 바꿔 답했다 — 보낼 본문이 없다. 상류는 drop 으로 끊는다.
            if let Some(t) = tee.as_mut() {
                t.abandon();
            }
            self.inner
                .telemetry
                .record_segment_metrics(start.elapsed().as_secs_f64(), 0);
            trace.finish("httpError");
            return Attempt::done(resp_out);
        }

        let stream_state = MediaStream {
            proxy: self.clone(),
            upstream: Some(resp),
            trace,
            tee,
            window,
            upstream_offset: 0,
            total_bytes: 0,
            last_enqueued: 0,
            status: head_status,
            kind,
            start,
            ttfb,
            done: false,
            target: target.clone(),
            expected_downstream: head_length,
        };
        let body_stream = futures_util::stream::unfold(stream_state, |mut st| async move {
            st.next_frame().await.map(|frame| (frame, st))
        });
        *resp_out.body_mut() = StreamBody::new(body_stream).boxed();
        Attempt::done(resp_out)
    }

    #[allow(clippy::too_many_arguments)]
    async fn complete_playlist(
        &self,
        mut resp: reqwest::Response,
        target: &Url,
        response_url: &Url,
        trace: &Arc<ProxyTransferTrace>,
        kind: &str,
        start: Instant,
        may_retry: bool,
    ) -> Attempt {
        self.inner.telemetry.record_segment_ok();
        trace.response(resp.status().as_u16(), kind, false);
        let max = crate::policy::media_response_policy::MAXIMUM_PLAYLIST_BYTES;
        let encoded = resp
            .headers()
            .get(http::header::CONTENT_ENCODING)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|e| !e.eq_ignore_ascii_case("identity"));
        let mut failure = resp.content_length().is_some_and(|l| l as usize > max) || encoded;
        let mut data = Vec::new();
        let mut error = None;
        while !failure {
            match resp.chunk().await {
                Ok(Some(chunk)) => {
                    trace.received(chunk.len());
                    if chunk.len() > max - data.len() {
                        failure = true;
                        break;
                    }
                    data.extend_from_slice(&chunk);
                }
                Ok(None) => break,
                Err(e) => {
                    error = Some(e);
                    break;
                }
            }
        }
        drop(resp);
        let elapsed = start.elapsed().as_secs_f64();
        self.inner.telemetry.note_playlist_upstream_wait(elapsed);
        self.inner
            .telemetry
            .record_segment_metrics(elapsed, data.len() as i64);
        if let Some(e) = &error {
            if !failure && may_retry && is_transient_upstream_error(e) && self.is_running() {
                trace.finish("retry");
                return Attempt::Retry;
            }
            tracing::warn!("Proxy(stream) upstream error: {e} [{}]", mask(target));
        }
        let body = if error.is_none() && !failure {
            self.rewritten_playlist(&data, target, Some(response_url))
        } else {
            None
        };
        let Some(body) = body else {
            trace.finish(if failure {
                "playlistTooLarge"
            } else if error.is_none() {
                "invalidPlaylist"
            } else {
                "upstreamError"
            });
            return Attempt::done(error_response(502, "Bad Gateway", true));
        };
        trace.enqueued(body.len());
        trace.processed(body.len(), true);
        self.inner.telemetry.record_bytes_served(body.len() as i64);
        trace.finish("success");
        Attempt::done(data_response(200, "application/vnd.apple.mpegurl", body))
    }
}

enum Attempt {
    Done(Box<Response<Body>>),
    Retry,
}

impl Attempt {
    fn done(resp: Response<Body>) -> Self {
        Self::Done(Box::new(resp))
    }
}

/// 미디어 본문 스트리밍 상태. 끝나기 전에 drop 되면 클라이언트가 떠난 것이다.
struct MediaStream {
    proxy: LocalStreamProxy,
    upstream: Option<reqwest::Response>,
    trace: Arc<ProxyTransferTrace>,
    tee: Option<TimeshiftTee>,
    window: Option<ByteRangeWindow>,
    upstream_offset: i64,
    total_bytes: i64,
    /// hyper 가 다음 프레임을 당기면 직전 프레임은 넘겨진 것이다.
    last_enqueued: usize,
    status: u16,
    kind: &'static str,
    start: Instant,
    ttfb: f64,
    done: bool,
    target: Url,
    /// 알린 길이(Content-Length). 모르면 -1.
    expected_downstream: i64,
}

impl MediaStream {
    fn mark_processed(&mut self) {
        if self.last_enqueued > 0 {
            self.trace.processed(self.last_enqueued, true);
            self.last_enqueued = 0;
        }
    }

    async fn next_frame(&mut self) -> Option<Result<Frame<Bytes>, std::io::Error>> {
        self.mark_processed();
        loop {
            let Some(upstream) = self.upstream.as_mut() else {
                self.complete(None);
                return None;
            };
            match upstream.chunk().await {
                Ok(Some(chunk)) => {
                    self.trace.received(chunk.len());
                    if let Some(t) = self.tee.as_mut() {
                        t.append(&chunk);
                    }
                    let offset = self.upstream_offset;
                    self.upstream_offset += chunk.len() as i64;
                    let piece = match self.window {
                        Some(w) => {
                            let reached_end = self.upstream_offset > w.last;
                            let part = w.slice(&chunk, offset).map(Bytes::copy_from_slice);
                            if reached_end {
                                // 창 끝을 넘겼다 — 상류 나머지를 받지 않는다(정상 완료).
                                self.upstream = None;
                            }
                            match part {
                                Some(p) => p,
                                None => continue,
                            }
                        }
                        None => chunk,
                    };
                    self.total_bytes += piece.len() as i64;
                    self.trace.enqueued(piece.len());
                    self.last_enqueued = piece.len();
                    return Some(Ok(Frame::data(piece)));
                }
                Ok(None) => {
                    self.upstream = None;
                    self.complete(None);
                    return None;
                }
                Err(e) => {
                    self.upstream = None;
                    let message = e.to_string();
                    self.complete(Some(e));
                    // 헤더는 이미 갔다 — 오류로 연결을 끊어 잘림을 드러낸다.
                    return Some(Err(std::io::Error::other(message)));
                }
            }
        }
    }

    fn complete(&mut self, error: Option<reqwest::Error>) {
        if self.done {
            return;
        }
        self.done = true;
        let succeeded = error.is_none();
        if let Some(t) = self.tee.as_mut() {
            t.finish(succeeded);
        }
        let elapsed = self.start.elapsed().as_secs_f64();
        let telemetry = &self.proxy.inner.telemetry;
        telemetry.record_segment_metrics(elapsed, self.upstream_offset);
        if succeeded && self.status < 400 && self.total_bytes > 0 && self.kind == "media" {
            telemetry.record_segment_fetch_sample(elapsed, self.ttfb);
        }
        if let Some(e) = error {
            tracing::warn!("Proxy(stream) upstream error: {e} [{}]", mask(&self.target));
            self.trace.finish("upstreamError");
            return;
        }
        self.trace.finish(if self.status >= 400 {
            "httpError"
        } else {
            "success"
        });
        telemetry.record_bytes_served(self.total_bytes);
    }
}

impl Drop for MediaStream {
    fn drop(&mut self) {
        if !self.done
            && self.expected_downstream >= 0
            && self.total_bytes >= self.expected_downstream
        {
            // 알린 길이를 다 넘겼다 — hyper 는 끝(None)을 당기지 않고 본문을 놓는다. 정상 완료다.
            self.mark_processed();
            self.complete(None);
            return;
        }
        if !self.done {
            // 응답 도중 클라이언트가 떠났다 — 상류(reqwest 응답)는 함께 drop 되어 취소된다.
            self.done = true;
            self.proxy.inner.telemetry.record_client_abort();
            if let Some(t) = self.tee.as_mut() {
                t.finish(false);
            }
            self.trace.finish("clientAbort");
        }
    }
}

#[cfg(test)]
mod origin_tests {
    use super::origin_allowed;

    #[test]
    fn only_app_origins_or_none() {
        assert!(origin_allowed(None));
        assert!(origin_allowed(Some("tauri://localhost")));
        assert!(origin_allowed(Some("http://tauri.localhost")));
        assert!(!origin_allowed(Some("https://evil.example")));
        assert!(!origin_allowed(Some("null")));
        assert!(!origin_allowed(Some("http://localhost:3000")));
    }
}
