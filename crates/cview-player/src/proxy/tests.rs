//! 로컬 모의 CDN(127.0.0.1)을 상대로 한 프록시 루프백 테스트 — 실제 네트워크 없음.
//!
//! 포팅 원본: `ProxySecurityTests`·`StreamCorrectionProxyTests`·`PrecisionReview0926ProxyByteRangeTests`·
//! `PrecisionReview0926ProxyPreflightTests`·`SMPSegmentTokenTests`(루프백)·`Swift6ProxyLifecycleTests`·
//! `ProxyUpstreamSessionRaceTests`·`ProxyClientAbortTests`(이탈 취소)·`PlaybackKeyAccessTests`(프록시 경로).

use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use url::Url;

use super::timeshift::{TeeDestination, TeeTarget, TimeshiftBackend};
use super::*;

const HOST: &str = "video.pstatic.net";

#[derive(Debug, Clone)]
struct Seen {
    host: String,
    path: String,
    query: Option<String>,
    range: Option<String>,
    cookie: Option<String>,
    accept_encoding: Option<String>,
}

#[derive(Default)]
struct MockResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    /// 길이를 알리지 않는다(chunked).
    unknown_length: bool,
    delay: Option<Duration>,
    /// 응답 없이 연결을 끊는다(일시적 실패 모의).
    drop_connection: bool,
    /// 본문을 보낸 뒤 끝내지 않고 멈춘다(상류 정지 모의).
    stall: bool,
}

impl MockResponse {
    fn ok(content_type: &str, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            headers: vec![("Content-Type".into(), content_type.into())],
            body: body.into(),
            ..Default::default()
        }
    }
}

type Handler = Arc<dyn Fn(&Seen) -> MockResponse + Send + Sync>;

struct Mock {
    port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Mock {
    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    fn ranges(&self, path: &str) -> Vec<Option<String>> {
        self.seen()
            .into_iter()
            .filter(|s| s.path == path)
            .map(|s| s.range)
            .collect()
    }
}

#[derive(Debug)]
struct Dropped;
impl std::fmt::Display for Dropped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("dropped")
    }
}
impl std::error::Error for Dropped {}

async fn spawn_mock(handler: Handler) -> Mock {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_task = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let handler = handler.clone();
            let seen = seen_task.clone();
            tokio::spawn(async move {
                let svc = service_fn(move |req: Request<Incoming>| {
                    let handler = handler.clone();
                    let seen = seen.clone();
                    async move {
                        let h = |n: &str| {
                            req.headers()
                                .get(n)
                                .and_then(|v| v.to_str().ok())
                                .map(str::to_string)
                        };
                        let s = Seen {
                            host: h("host").unwrap_or_default(),
                            path: req.uri().path().to_string(),
                            query: req.uri().query().map(str::to_string),
                            range: h("range"),
                            cookie: h("cookie"),
                            accept_encoding: h("accept-encoding"),
                        };
                        seen.lock().unwrap().push(s.clone());
                        let r = handler(&s);
                        if let Some(d) = r.delay {
                            tokio::time::sleep(d).await;
                        }
                        if r.drop_connection {
                            return Err(Dropped);
                        }
                        let mut builder = Response::builder().status(r.status);
                        for (k, v) in &r.headers {
                            builder = builder.header(k, v);
                        }
                        let body: http_body_util::combinators::BoxBody<Bytes, Infallible> =
                            if r.unknown_length || r.stall {
                                let frames =
                                    vec![Ok::<_, Infallible>(Frame::data(Bytes::from(r.body)))];
                                let first = futures_util::stream::iter(frames);
                                if r.stall {
                                    let stalled = futures_util::StreamExt::chain(
                                        first,
                                        futures_util::stream::pending(),
                                    );
                                    StreamBody::new(stalled).boxed()
                                } else {
                                    StreamBody::new(first).boxed()
                                }
                            } else {
                                Full::new(Bytes::from(r.body)).boxed()
                            };
                        Ok::<_, Dropped>(builder.body(body).unwrap())
                    }
                });
                let _ = http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), svc)
                    .await;
            });
        }
    });
    Mock { port, seen }
}

fn override_to(port: u16) -> UpstreamOverride {
    Arc::new(move |logical: &Url| {
        let mut s = format!("http://127.0.0.1:{port}{}", logical.path());
        if let Some(q) = logical.query() {
            s.push('?');
            s.push_str(q);
        }
        Url::parse(&s).unwrap()
    })
}

fn proxy_for(mock: &Mock) -> LocalStreamProxy {
    LocalStreamProxy::with_upstream_override(Arc::new(Vec::new), override_to(mock.port))
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

async fn get(port: u16, path: &str) -> reqwest::Response {
    client()
        .get(format!("http://127.0.0.1:{port}{path}"))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .unwrap()
}

async fn get_range(port: u16, path: &str, range: &str) -> reqwest::Response {
    client()
        .get(format!("http://127.0.0.1:{port}{path}"))
        .header("Range", range)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .unwrap()
}

fn header(resp: &reqwest::Response, name: &str) -> Option<String> {
    resp.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

async fn eventually(mut f: impl FnMut() -> bool) -> bool {
    for _ in 0..200 {
        if f() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    false
}

/// 원시 HTTP 응답 하나를 읽는다(헤더 + Content-Length 본문).
async fn read_response(stream: &mut TcpStream) -> (String, Vec<(String, String)>, Vec<u8>) {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        let n = stream.read(&mut byte).await.unwrap();
        assert!(n > 0, "연결이 닫혔다: {:?}", String::from_utf8_lossy(&buf));
        buf.push(byte[0]);
    }
    let text = String::from_utf8_lossy(&buf).to_string();
    let mut lines = text.trim_end().split("\r\n");
    let status = lines.next().unwrap().to_string();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string()))
        .collect();
    let len = headers
        .iter()
        .find(|(k, _)| k == "content-length")
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).await.unwrap();
    (status, headers, body)
}

fn h<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

// MARK: - 수명

#[tokio::test]
async fn refuses_disallowed_host_and_joins_concurrent_starts() {
    let proxy = LocalStreamProxy::default();
    assert!(matches!(
        proxy.start("127.0.0.1").await,
        Err(ProxyError::DisallowedHost)
    ));
    let mut handles = Vec::new();
    for _ in 0..12 {
        let p = proxy.clone();
        handles.push(tokio::spawn(async move {
            p.start("ex-nlive-streaming.navercdn.com").await.unwrap()
        }));
    }
    let mut ports = Vec::new();
    for h in handles {
        ports.push(h.await.unwrap());
    }
    ports.dedup();
    assert_eq!(ports.len(), 1);
    assert_eq!(proxy.port(), ports[0]);
    assert_eq!(proxy.target_host(), "ex-nlive-streaming.navercdn.com");

    // 다른 호스트로 재시작하면 새 리스너.
    let new_port = proxy.start("ssl.pstatic.net").await.unwrap();
    assert_eq!(proxy.port(), new_port);
    assert_eq!(proxy.target_host(), "ssl.pstatic.net");
    assert!(proxy.is_running());
    proxy.stop();
    assert_eq!(proxy.port(), 0);
    assert!(!proxy.is_running());
}

#[tokio::test]
async fn stopped_proxy_refuses_new_sessions() {
    let proxy = LocalStreamProxy::default();
    assert!(proxy.upstream_client().is_some());
    proxy.reset_session();
    assert!(!proxy.has_session());
    assert!(proxy.upstream_client().is_some());
    proxy.stop();
    assert!(proxy.upstream_client().is_none());
    assert!(!proxy.has_session(), "정지 뒤 세션이 되살아났다");
}

#[tokio::test]
async fn proxy_url_rewrites_only_while_running() {
    let proxy = LocalStreamProxy::default();
    let original = Url::parse("https://video.pstatic.net/live/a.m3u8?t=1").unwrap();
    assert_eq!(proxy.proxy_url(&original), original);
    let port = proxy.start(HOST).await.unwrap();
    assert_eq!(
        proxy.proxy_url(&original).as_str(),
        format!("http://127.0.0.1:{port}/live/a.m3u8?t=1")
    );
    let cross = Url::parse("https://b.navercdn.com/x/y.m3u8").unwrap();
    assert_eq!(
        proxy.proxy_url(&cross).as_str(),
        format!("http://127.0.0.1:{port}/_p_/b.navercdn.com/x/y.m3u8")
    );
    assert!(LocalStreamProxy::needs_proxy(&original));
    proxy.stop();
}

// MARK: - 보안

#[tokio::test]
async fn loopback_request_to_untrusted_destination_returns_403() {
    let proxy = LocalStreamProxy::default();
    let port = proxy
        .start("ex-nlive-streaming.navercdn.com")
        .await
        .unwrap();
    let resp = get(port, "/_p_/127.0.0.1/private.m3u8").await;
    assert_eq!(resp.status(), 403);
    assert_eq!(
        header(&resp, "access-control-allow-origin").as_deref(),
        Some("*")
    );
    proxy.stop();
}

#[tokio::test]
async fn other_methods_get_400_and_absolute_form_is_forbidden() {
    let proxy = LocalStreamProxy::default();
    let port = proxy.start(HOST).await.unwrap();
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(b"POST /a.m4s HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\n\r\n")
        .await
        .unwrap();
    let (status, headers, _) = read_response(&mut s).await;
    assert!(status.starts_with("HTTP/1.1 400 "), "{status}");
    assert_eq!(h(&headers, "connection"), Some("close"));

    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(b"GET http://video.pstatic.net/a.m4s HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    let (status, _, _) = read_response(&mut s).await;
    assert!(status.starts_with("HTTP/1.1 403 "), "{status}");
    proxy.stop();
}

// MARK: - 응답 보정

const PLAYLIST: &str = "#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:2,\nseg.m4s\n";
const MEDIA: [u8; 8] = [0x47, 0x40, 0, 0x10, 1, 2, 3, 4];

fn correction_handler() -> Handler {
    let counter = Arc::new(Mutex::new(0));
    Arc::new(move |s: &Seen| {
        let mut r = MockResponse::ok("video/MP2T", MEDIA.to_vec());
        match s.path.as_str() {
            "/manifest" => {
                r = MockResponse::ok(
                    "Application/Vnd.Apple.MpegURL; charset=UTF-8",
                    PLAYLIST.as_bytes(),
                )
            }
            "/increment.m3u8" => {
                let mut c = counter.lock().unwrap();
                *c += 1;
                r = MockResponse::ok(
                    "application/vnd.apple.mpegurl",
                    format!("{PLAYLIST}#EXT-X-MEDIA-SEQUENCE:{c}\n"),
                );
            }
            "/large" | "/large-streamed" => {
                r = MockResponse::ok("application/vnd.apple.mpegurl", vec![65u8; mrp_max() + 1]);
                r.unknown_length = s.path == "/large-streamed";
            }
            "/invalid-header.m3u8" => {
                r = MockResponse::ok("application/vnd.apple.mpegurl", "#EXTM3U-invalid\nseg.m4s")
            }
            "/compressed-range.m4s" => {
                r.status = 206;
                r.headers.push(("Content-Encoding".into(), "gzip".into()));
                r.headers
                    .push(("Content-Range".into(), "bytes 0-7/100".into()));
            }
            "/invalid.m3u8" => r = MockResponse::ok("application/octet-stream", "invalid"),
            "/error.m4s" => r.status = 403,
            "/opaque" => {
                r.headers = vec![("Content-Type".into(), "application/octet-stream".into())]
            }
            _ => {}
        }
        r
    })
}

fn mrp_max() -> usize {
    crate::policy::media_response_policy::MAXIMUM_PLAYLIST_BYTES
}

#[tokio::test]
async fn rewrites_response_identified_playlists() {
    let mock = spawn_mock(correction_handler()).await;
    let proxy = proxy_for(&mock);
    let port = proxy.start(HOST).await.unwrap();
    let resp = get(port, "/manifest").await;
    assert_eq!(resp.status(), 200);
    let len = header(&resp, "content-length");
    assert!(header(&resp, "content-encoding").is_none());
    let text = resp.text().await.unwrap();
    assert_eq!(len, Some(text.len().to_string()));
    assert!(text.contains(&format!("URI=\"http://127.0.0.1:{port}/init.mp4\"")));
    assert!(text.contains(&format!("http://127.0.0.1:{port}/seg.m4s")));
    // 상류 요청 헤더 — Host 는 논리 호스트, Referer/Origin/UA 주입.
    let seen = mock.seen();
    assert_eq!(seen[0].host, HOST);
    assert_eq!(seen[0].accept_encoding.as_deref(), Some("identity"));
    proxy.stop();
}

#[tokio::test]
async fn corrects_only_supported_media_and_preserves_bytes() {
    let mock = spawn_mock(correction_handler()).await;
    let proxy = proxy_for(&mock);
    let port = proxy.start(HOST).await.unwrap();
    for (path, expected) in [
        ("/seg.ts", "video/MP2T"),
        ("/seg.m4s", "video/mp4"),
        ("/audio.cmfa", "audio/mp4"),
        ("/error.m4s", "video/MP2T"),
        ("/opaque?file=.m3u8", "application/octet-stream"),
    ] {
        let before = proxy.transfer_diagnostics().len();
        let resp = get(port, path).await;
        let status = resp.status().as_u16();
        assert_eq!(
            status,
            if path == "/error.m4s" { 403 } else { 200 },
            "{path}"
        );
        assert_eq!(
            header(&resp, "content-type").as_deref(),
            Some(expected),
            "{path}"
        );
        assert_eq!(
            header(&resp, "access-control-allow-origin").as_deref(),
            Some("*")
        );
        let data = resp.bytes().await.unwrap();
        assert_eq!(&data[..], &MEDIA, "{path}");
        assert!(eventually(|| proxy.transfer_diagnostics().len() > before).await);
        let record = proxy.transfer_diagnostics().last().cloned().unwrap();
        assert_eq!(record.upstream_bytes, data.len() as i64);
        assert_eq!(record.enqueued_bytes, data.len() as i64);
        assert_eq!(record.processed_bytes, data.len() as i64);
        assert_eq!(
            record.outcome,
            if status == 403 {
                "httpError"
            } else {
                "success"
            }
        );
    }
    proxy.stop();
}

#[tokio::test]
async fn rejects_unusable_playlists_without_serving_partial_success() {
    let mock = spawn_mock(correction_handler()).await;
    let proxy = proxy_for(&mock);
    let port = proxy.start(HOST).await.unwrap();
    for path in [
        "/large",
        "/large-streamed",
        "/invalid.m3u8",
        "/invalid-header.m3u8",
        "/compressed-range.m4s",
    ] {
        assert_eq!(get(port, path).await.status(), 502, "{path}");
    }
    proxy.stop();
}

#[tokio::test]
async fn repeated_playlist_requests_always_reach_upstream() {
    let mock = spawn_mock(correction_handler()).await;
    let proxy = proxy_for(&mock);
    let port = proxy.start(HOST).await.unwrap();
    let first = get(port, "/increment.m3u8").await.text().await.unwrap();
    let second = get(port, "/increment.m3u8").await.text().await.unwrap();
    assert_ne!(first, second);
    proxy.stop();
}

#[tokio::test]
async fn final_filename_and_only_uri_fields_are_rewritten() {
    let proxy = LocalStreamProxy::default();
    let port = proxy.start("a.navercdn.com").await.unwrap();
    let source = "#EXTM3U\n# comment https://b.pstatic.net/keep\n#EXT-X-UNKNOWN:VALUE=\"https://b.pstatic.net/keep\"\n#EXT-X-PART:DURATION=1,URI=\"?part=3&sig=a%2Bb\"\n#EXTINF:2,\nhttps://b.pstatic.net/hdntl=acl=*%2F*~hmac=x/seg.m4s?sig=a%2Bb";
    let data = proxy
        .rewritten_playlist(
            source.as_bytes(),
            &Url::parse("https://a.navercdn.com/start.m3u8").unwrap(),
            Some(&Url::parse("https://b.pstatic.net/edge/list.m3u8?old=1").unwrap()),
        )
        .unwrap();
    let text = String::from_utf8(data).unwrap();
    assert!(text.contains(&format!(
        "URI=\"http://127.0.0.1:{port}/_p_/b.pstatic.net/edge/list.m3u8?part=3&sig=a%2Bb\""
    )));
    assert!(text.contains("# comment https://b.pstatic.net/keep"));
    assert!(text.contains("#EXT-X-UNKNOWN:VALUE=\"https://b.pstatic.net/keep\""));
    assert!(text.contains("/_p_/b.pstatic.net/hdntl=acl=*%2F*~hmac=x/seg.m4s?sig=a%2Bb"));
    proxy.stop();
}

#[tokio::test]
async fn redirected_playlist_resolves_against_final_url() {
    let handler: Handler = Arc::new(|s: &Seen| match s.path.as_str() {
        "/live/start.m3u8" => MockResponse {
            status: 302,
            headers: vec![(
                "Location".into(),
                "https://b.pstatic.net/edge/7/list.m3u8".into(),
            )],
            ..Default::default()
        },
        "/edge/7/list.m3u8" => {
            assert_eq!(
                s.host, "b.pstatic.net",
                "리다이렉트 뒤 Host 가 갱신되지 않았다"
            );
            MockResponse::ok("application/vnd.apple.mpegurl", "#EXTM3U\nseg.m4s\n")
        }
        _ => MockResponse {
            status: 404,
            ..Default::default()
        },
    });
    let mock = spawn_mock(handler).await;
    let proxy = proxy_for(&mock);
    let port = proxy.start(HOST).await.unwrap();
    let text = get(port, "/live/start.m3u8").await.text().await.unwrap();
    assert!(
        text.contains(&format!(
            "http://127.0.0.1:{port}/_p_/b.pstatic.net/edge/7/seg.m4s"
        )),
        "{text}"
    );
    proxy.stop();
}

// MARK: - Range

fn range_body() -> Vec<u8> {
    (0..1000u32).map(|i| (i % 251) as u8).collect()
}

/// `/r/…` 는 Range 를 지키고, `/ignore/…` 는 무시하고 200 전체, `/unknown/…` 는 길이도 숨긴다.
fn range_handler() -> Handler {
    Arc::new(|s: &Seen| {
        let body = range_body();
        let honors = s.path.starts_with("/r/");
        let mut r = MockResponse::ok("video/mp4", body.clone());
        r.unknown_length = s.path.starts_with("/unknown/");
        if s.path.ends_with(".m3u8") {
            return MockResponse::ok("application/vnd.apple.mpegurl", "#EXTM3U\nseg.m4s\n");
        }
        if let (true, Some(range)) = (honors, s.range.as_deref()) {
            let (start, end) =
                super::byte_range::parse_simple_byte_range(range).expect("단순 구간");
            if start >= body.len() as u64 {
                return MockResponse {
                    status: 416,
                    headers: vec![("Content-Range".into(), "bytes */1000".into())],
                    ..Default::default()
                };
            }
            let end = end.unwrap_or(999).min(999) as usize;
            r.status = 206;
            r.body = body[start as usize..=end].to_vec();
            r.headers
                .push(("Content-Range".into(), format!("bytes {start}-{end}/1000")));
        }
        r
    })
}

#[tokio::test]
async fn forwards_ranges_and_relays_206() {
    let mock = spawn_mock(range_handler()).await;
    let proxy = proxy_for(&mock);
    let port = proxy.start(HOST).await.unwrap();
    let body = range_body();
    let first = get_range(port, "/r/a.m4s", "bytes=0-399").await;
    assert_eq!(first.status(), 206);
    assert_eq!(
        header(&first, "content-range").as_deref(),
        Some("bytes 0-399/1000")
    );
    assert_eq!(header(&first, "content-length").as_deref(), Some("400"));
    assert_eq!(
        header(&first, "access-control-expose-headers").as_deref(),
        Some("Content-Range")
    );
    assert_eq!(&first.bytes().await.unwrap()[..], &body[..400]);
    let second = get_range(port, "/r/a.m4s", "bytes=400-999").await;
    assert_eq!(second.status(), 206);
    assert_eq!(&second.bytes().await.unwrap()[..], &body[400..]);
    assert_eq!(
        mock.ranges("/r/a.m4s"),
        vec![Some("bytes=0-399".into()), Some("bytes=400-999".into())]
    );

    // Range 없는 요청은 200 전체, Content-Range 없음.
    let whole = get(port, "/r/b.m4s").await;
    assert_eq!(whole.status(), 200);
    assert!(header(&whole, "content-range").is_none());
    assert_eq!(&whole.bytes().await.unwrap()[..], &body[..]);

    // 지원하지 않는 구간은 넘기지 않는다.
    for r in [
        "bytes=0-1,5-9",
        "bytes=9-1",
        "bytes=-100",
        "items=0-1",
        "bytes=+5-9",
    ] {
        let resp = get_range(port, "/r/c.m4s", r).await;
        assert_eq!(resp.status(), 200, "{r}");
    }
    assert!(mock.ranges("/r/c.m4s").iter().all(Option::is_none));

    // 상류 416 은 그대로.
    let unsat = get_range(port, "/r/d.m4s", "bytes=5000-").await;
    assert_eq!(unsat.status(), 416);
    assert_eq!(
        header(&unsat, "content-range").as_deref(),
        Some("bytes */1000")
    );

    // 재생목록 Range 는 무시.
    let pl = get_range(port, "/r/list.m3u8", "bytes=0-10").await;
    assert_eq!(pl.status(), 200);
    assert_eq!(mock.ranges("/r/list.m3u8"), vec![None]);
    proxy.stop();
}

#[tokio::test]
async fn ignored_range_is_sliced_and_connection_reused() {
    let mock = spawn_mock(range_handler()).await;
    let proxy = proxy_for(&mock);
    let port = proxy.start(HOST).await.unwrap();
    let body = range_body();
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(b"GET /ignore/a.m4s HTTP/1.1\r\nHost: x\r\nRange: bytes=100-399\r\n\r\n")
        .await
        .unwrap();
    let (status, headers, data) = read_response(&mut s).await;
    assert_eq!(status, "HTTP/1.1 206 Partial Content");
    assert_eq!(h(&headers, "content-range"), Some("bytes 100-399/1000"));
    assert_eq!(h(&headers, "content-length"), Some("300"));
    assert_eq!(data, &body[100..400]);

    // 같은 연결로 다음 요청 — 만족 불가 구간은 416 으로.
    s.write_all(b"GET /ignore/a.m4s HTTP/1.1\r\nHost: x\r\nRange: bytes=1000-\r\n\r\n")
        .await
        .unwrap();
    let (status, headers, data) = read_response(&mut s).await;
    assert!(status.starts_with("HTTP/1.1 416 "), "{status}");
    assert_eq!(h(&headers, "content-range"), Some("bytes */1000"));
    assert_eq!(h(&headers, "content-length"), Some("0"));
    assert!(data.is_empty());

    s.write_all(b"GET /ignore/a.m4s HTTP/1.1\r\nHost: x\r\nRange: bytes=900-\r\n\r\n")
        .await
        .unwrap();
    let (status, _, data) = read_response(&mut s).await;
    assert_eq!(status, "HTTP/1.1 206 Partial Content");
    assert_eq!(data, &body[900..]);

    // 만들어 낸 416 도 오류로 센다.
    let stats = proxy.network_stats();
    assert_eq!(stats.error_count, 1);
    assert_eq!(stats.total_requests, 3);
    proxy.stop();
}

#[tokio::test]
async fn ignored_range_with_unknown_length_passes_through() {
    let mock = spawn_mock(range_handler()).await;
    let proxy = proxy_for(&mock);
    let port = proxy.start(HOST).await.unwrap();
    let resp = get_range(port, "/unknown/a.m4s", "bytes=100-399").await;
    assert_eq!(resp.status(), 200);
    assert!(header(&resp, "content-length").is_none());
    assert!(header(&resp, "content-range").is_none());
    assert_eq!(header(&resp, "connection").as_deref(), Some("close"));
    assert_eq!(&resp.bytes().await.unwrap()[..], &range_body()[..]);
    proxy.stop();
}

// MARK: - 사전 요청

#[tokio::test]
async fn preflight_answers_204_then_ranged_get_on_same_connection() {
    let mock = spawn_mock(range_handler()).await;
    let proxy = proxy_for(&mock);
    let port = proxy.start(HOST).await.unwrap();
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(
        b"OPTIONS /r/a.m4s HTTP/1.1\r\nHost: x\r\nOrigin: tauri://localhost\r\nAccess-Control-Request-Method: GET\r\nAccess-Control-Request-Headers: range, x-evil\r\n\r\n",
    )
    .await
    .unwrap();
    let (status, headers, body) = read_response(&mut s).await;
    assert_eq!(status, "HTTP/1.1 204 No Content");
    assert_eq!(h(&headers, "access-control-allow-origin"), Some("*"));
    assert_eq!(
        h(&headers, "access-control-allow-methods"),
        Some("GET, OPTIONS")
    );
    assert_eq!(h(&headers, "access-control-allow-headers"), Some("range"));
    assert!(h(&headers, "access-control-max-age").is_some());
    assert_eq!(h(&headers, "connection"), Some("keep-alive"));
    assert!(body.is_empty());
    assert!(mock.seen().is_empty(), "사전 요청이 상류로 나갔다");

    s.write_all(b"GET /r/a.m4s HTTP/1.1\r\nHost: x\r\nRange: bytes=0-399\r\n\r\n")
        .await
        .unwrap();
    let (status, headers, body) = read_response(&mut s).await;
    assert_eq!(status, "HTTP/1.1 206 Partial Content");
    assert_eq!(h(&headers, "content-range"), Some("bytes 0-399/1000"));
    assert_eq!(body, &range_body()[..400]);

    // 거절할 경로의 사전 요청은 403 close, 타임시프트 형식은 GET 과 같은 해석.
    for (target, expected) in [
        ("/_p_/evil.com/a.m4s", 403),
        ("/_ts/abc/index.m3u8", 204),
        ("/_ts/abc/seg/x.m4v", 403),
    ] {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(format!("OPTIONS {target} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let (status, headers, _) = read_response(&mut s).await;
        assert!(
            status.starts_with(&format!("HTTP/1.1 {expected} ")),
            "{target}: {status}"
        );
        let conn = if expected == 403 {
            "close"
        } else {
            "keep-alive"
        };
        assert_eq!(h(&headers, "connection"), Some(conn), "{target}");
    }
    proxy.stop();
}

// MARK: - SMP 토큰

#[tokio::test]
async fn loopback_attaches_smp_token_to_segments() {
    const TOKEN: &str = "st=1790000000~exp=1790086400~acl=*/tm/*~hmac=0a1b2c";
    let handler: Handler = Arc::new(|s: &Seen| {
        if s.path.ends_with(".m3u8") {
            MockResponse::ok(
                "application/vnd.apple.mpegurl",
                "#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXTINF:2.0,\nseg_1.m4v\n",
            )
        } else {
            MockResponse::ok("video/mp4", "segment")
        }
    });
    let mock = spawn_mock(handler).await;
    let proxy = proxy_for(&mock);
    let port = proxy
        .start("ex-nlive-streaming.navercdn.com")
        .await
        .unwrap();
    proxy.set_smp_segment_token_rewrite(true);
    get(port, &format!("/tm/1080p/chunklist.m3u8?hdnts={TOKEN}"))
        .await
        .bytes()
        .await
        .unwrap();
    get(port, "/tm/1080p/seg_1.m4v")
        .await
        .bytes()
        .await
        .unwrap();
    let seen = mock.seen();
    let seg = seen.iter().find(|s| s.path.ends_with("seg_1.m4v")).unwrap();
    assert_eq!(
        seg.query.as_deref(),
        Some(format!("__bgda__={TOKEN}").as_str())
    );
    let pl = seen.iter().find(|s| s.path.ends_with(".m3u8")).unwrap();
    assert!(!pl.query.clone().unwrap_or_default().contains("__bgda__"));
    proxy.stop();
}

// MARK: - 인증

#[tokio::test]
async fn key_url_gets_login_cookies_others_do_not() {
    let handler: Handler =
        Arc::new(|_| MockResponse::ok("application/octet-stream", vec![1u8; 16]));
    let mock = spawn_mock(handler).await;
    let cookies: CookieProvider = Arc::new(|| {
        vec![
            PlaybackCookie::new("NID_SES", "fixture-session"),
            PlaybackCookie::new("OTHER", "x"),
        ]
    });
    let proxy = LocalStreamProxy::with_upstream_override(cookies, override_to(mock.port));
    let port = proxy.start(HOST).await.unwrap();
    get(
        port,
        "/_p_/api.chzzk.naver.com/service/v1/encryption/lives/123/aes_key",
    )
    .await
    .bytes()
    .await
    .unwrap();
    get(port, "/seg.m4s").await.bytes().await.unwrap();
    let seen = mock.seen();
    assert_eq!(seen[0].host, "api.chzzk.naver.com");
    assert_eq!(seen[0].cookie.as_deref(), Some("NID_SES=fixture-session"));
    assert!(seen[1].cookie.is_none());
    proxy.stop();
}

#[tokio::test]
async fn consecutive_403_notifies_auth_failure() {
    let handler: Handler = Arc::new(|_| MockResponse {
        status: 403,
        ..Default::default()
    });
    let mock = spawn_mock(handler).await;
    let proxy = proxy_for(&mock);
    let port = proxy.start(HOST).await.unwrap();
    let fired = Arc::new(Mutex::new(0));
    let f = fired.clone();
    proxy.set_on_upstream_auth_failure(Some(Arc::new(move || *f.lock().unwrap() += 1)));
    for i in 0..3 {
        get(port, &format!("/s{i}.m4s"))
            .await
            .bytes()
            .await
            .unwrap();
    }
    assert_eq!(*fired.lock().unwrap(), 1);
    assert_eq!(proxy.network_stats().consecutive_403_count, 0);
    proxy.stop();
}

// MARK: - 일시적 실패 · 클라이언트 이탈

#[tokio::test]
async fn transient_failure_is_retried_once_with_same_range() {
    let attempts = Arc::new(Mutex::new(0));
    let a = attempts.clone();
    let handler: Handler = Arc::new(move |s: &Seen| {
        let mut n = a.lock().unwrap();
        *n += 1;
        if *n == 1 {
            return MockResponse {
                drop_connection: true,
                ..Default::default()
            };
        }
        let body = range_body();
        let mut r = MockResponse::ok("video/mp4", body[400..].to_vec());
        r.status = 206;
        r.headers
            .push(("Content-Range".into(), "bytes 400-999/1000".into()));
        assert_eq!(s.range.as_deref(), Some("bytes=400-999"));
        r
    });
    let mock = spawn_mock(handler).await;
    let proxy = proxy_for(&mock);
    let port = proxy.start(HOST).await.unwrap();
    let resp = get_range(port, "/flaky.m4s", "bytes=400-999").await;
    assert_eq!(resp.status(), 206);
    assert_eq!(resp.bytes().await.unwrap().len(), 600);
    assert_eq!(
        mock.ranges("/flaky.m4s"),
        vec![Some("bytes=400-999".into()), Some("bytes=400-999".into())]
    );
    proxy.stop();
}

#[tokio::test]
async fn client_leaving_mid_body_counts_abort() {
    let handler: Handler = Arc::new(|_| MockResponse {
        stall: true,
        ..MockResponse::ok("video/mp4", vec![7u8; 64])
    });
    let mock = spawn_mock(handler).await;
    let proxy = proxy_for(&mock);
    let port = proxy.start(HOST).await.unwrap();
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(b"GET /stall.m4s HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    let mut buf = [0u8; 256];
    let _ = s.read(&mut buf).await.unwrap();
    drop(s);
    let p = proxy.clone();
    assert!(eventually(|| p.inner.telemetry.client_aborts() >= 1).await);
    proxy.stop();
}

#[tokio::test]
async fn upstream_waiting_client_abort_cancels_upstream() {
    let handler: Handler = Arc::new(|_| MockResponse {
        delay: Some(Duration::from_secs(30)),
        ..MockResponse::ok("video/mp4", vec![1u8; 10])
    });
    let mock = spawn_mock(handler).await;
    let proxy = proxy_for(&mock);
    let port = proxy.start(HOST).await.unwrap();
    let result = client()
        .get(format!("http://127.0.0.1:{port}/slow.m4s"))
        .timeout(Duration::from_millis(200))
        .send()
        .await;
    assert!(result.is_err(), "시한 안에 응답이 왔다");
    tokio::time::sleep(Duration::from_millis(100)).await;
    // 재시도하지 않는다 — 상류 요청은 정확히 한 번.
    assert_eq!(mock.seen().len(), 1);
    proxy.stop();
}

// MARK: - 타임시프트

#[derive(Default)]
struct FakeRecorder {
    commits: Mutex<Vec<(TeeDestination, usize)>>,
    observed: Mutex<Vec<String>>,
}

impl TimeshiftBackend for FakeRecorder {
    fn session_id(&self) -> String {
        "sid1".into()
    }
    fn local_playlist(&self, start_offset: Option<f64>) -> Option<String> {
        start_offset.map(|s| format!("#EXTM3U\n#X-START:{s}\n"))
    }
    fn local_segment(&self, seq: i64) -> Option<Vec<u8>> {
        (seq == 3).then(|| vec![3u8; 5])
    }
    fn local_init(&self, id: i64) -> Option<Vec<u8>> {
        (id == 0).then(|| vec![0u8; 2])
    }
    fn observe_playlist(&self, _: &str, url: &Url, _: bool, _: Option<&Url>) {
        self.observed.lock().unwrap().push(url.path().to_string());
    }
    fn tee_destination(&self, url: &Url) -> Option<TeeDestination> {
        url.path()
            .ends_with("rec.m4v")
            .then_some(TeeDestination::Slot(TeeTarget::Segment { seq: 9 }))
    }
    fn commit(&self, d: &TeeDestination, data: Vec<u8>) {
        self.commits.lock().unwrap().push((d.clone(), data.len()));
    }
    fn abandon(&self, _: &TeeDestination) {}
    fn stop(&self) {}
}

#[tokio::test]
async fn serves_timeshift_and_tees_segments() {
    let handler: Handler = Arc::new(|s: &Seen| {
        if s.path.ends_with(".m3u8") {
            MockResponse::ok("application/vnd.apple.mpegurl", "#EXTM3U\nrec.m4v\n")
        } else {
            MockResponse::ok("video/mp4", vec![9u8; 123])
        }
    });
    let mock = spawn_mock(handler).await;
    let proxy = proxy_for(&mock);
    let port = proxy.start(HOST).await.unwrap();
    assert!(proxy.timeshift_playlist_url(5.0).is_none());
    let recorder = Arc::new(FakeRecorder::default());
    proxy.attach_timeshift(recorder.clone());
    assert_eq!(
        proxy.timeshift_playlist_url(5.0).unwrap().as_str(),
        format!("http://127.0.0.1:{port}/_ts/sid1/index.m3u8?start=5.000")
    );

    let pl = get(port, "/_ts/sid1/index.m3u8?start=12").await;
    assert_eq!(pl.status(), 200);
    assert!(pl.text().await.unwrap().contains("#X-START:12"));
    assert_eq!(get(port, "/_ts/sid1/index.m3u8").await.status(), 503);
    assert_eq!(get(port, "/_ts/other/seg/3.m4v").await.status(), 404);
    assert_eq!(
        get(port, "/_ts/sid1/seg/3.m4v")
            .await
            .bytes()
            .await
            .unwrap()
            .len(),
        5
    );
    assert_eq!(get(port, "/_ts/sid1/init/0.mp4").await.status(), 200);
    assert_eq!(get(port, "/_ts/sid1/seg/4.m4v").await.status(), 404);
    assert!(mock.seen().is_empty(), "타임시프트 요청이 상류로 나갔다");

    get(port, "/live/list.m3u8").await.bytes().await.unwrap();
    assert_eq!(*recorder.observed.lock().unwrap(), vec!["/live/list.m3u8"]);
    get(port, "/live/rec.m4v").await.bytes().await.unwrap();
    // 구간 요청은 복사하지 않는다.
    get_range(port, "/live/rec.m4v", "bytes=0-9")
        .await
        .bytes()
        .await
        .unwrap();
    let r = recorder.clone();
    assert!(eventually(|| !r.commits.lock().unwrap().is_empty()).await);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        *recorder.commits.lock().unwrap(),
        vec![(TeeDestination::Slot(TeeTarget::Segment { seq: 9 }), 123)]
    );
    proxy.stop();
    assert!(proxy.timeshift_backend().is_none());
}

#[tokio::test]
async fn upstream_fetch_uses_same_headers_and_validates_ranges() {
    let mock = spawn_mock(range_handler()).await;
    let proxy = proxy_for(&mock);
    let url = Url::parse("https://video.pstatic.net/r/x.m4s").unwrap();
    let (data, resp) = proxy
        .upstream_fetch(
            &url,
            Some(crate::manifest::ByteRange {
                length: 10,
                offset: Some(5),
            }),
        )
        .await;
    assert_eq!(resp.status, 206);
    assert_eq!(resp.content_range.as_deref(), Some("bytes 5-14/1000"));
    assert_eq!(data.unwrap(), &range_body()[5..15]);
    let (none, bad) = proxy
        .upstream_fetch(
            &url,
            Some(crate::manifest::ByteRange {
                length: -1,
                offset: Some(0),
            }),
        )
        .await;
    assert!(none.is_none());
    assert_eq!(bad.status, 0);
    let (_, denied) = proxy
        .upstream_fetch(&Url::parse("https://evil.com/x").unwrap(), None)
        .await;
    assert_eq!(denied.status, 0);
}

#[tokio::test]
async fn supply_gap_is_noted_only_for_segments() {
    let handler: Handler = Arc::new(|s: &Seen| {
        if s.path.ends_with(".m3u8") {
            MockResponse::ok("application/vnd.apple.mpegurl", "#EXTM3U\n")
        } else {
            MockResponse::ok("video/mp4", vec![1u8; 4])
        }
    });
    let mock = spawn_mock(handler).await;
    let proxy = proxy_for(&mock);
    let port = proxy.start(HOST).await.unwrap();
    get(port, "/a.m3u8").await.bytes().await.unwrap();
    get(port, "/a.m4s").await.bytes().await.unwrap();
    // 직전 세그먼트 시각이 기록됐으면 아주 먼 미래의 도착은 (워밍업) 공백으로 판정된다.
    let far = proxy.monotonic_now() + 100.0;
    assert_eq!(
        proxy.note_segment_request_arrived(far),
        Some(SupplyGapCause::Client)
    );
    proxy.stop();
}

#[tokio::test]
async fn foreign_origin_is_refused() {
    let mock = spawn_mock(range_handler()).await;
    let proxy = proxy_for(&mock);
    let port = proxy.start(HOST).await.unwrap();
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(b"GET /r/a.m4s HTTP/1.1\r\nHost: x\r\nOrigin: https://evil.example\r\n\r\n")
        .await
        .unwrap();
    let (status, _, _) = read_response(&mut s).await;
    assert!(status.starts_with("HTTP/1.1 403"), "{status}");
    proxy.stop();
}
