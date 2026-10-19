//! `Tests/CViewPlayerTests/PrecisionReviewR4RecordingHLSTests.swift` 포팅 — 루프백 가짜 CDN 으로
//! `#EXT-X-MAP`·BYTERANGE·리다이렉트·순서 보존·토큰 회전을 고정한다.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use url::Url;

use super::segments::{file_key, MAX_BLOCKED_POLLS};
use super::*;
use crate::manifest::parser::ByteRange;
use crate::manifest::HlsByteSpan;

#[derive(Clone)]
enum Route {
    /// Range 가 있으면 206 + 그 구간, 없으면 200 전체.
    Body(Vec<u8>),
    /// Range 를 무시하고 200 전체.
    BodyIgnoringRange(Vec<u8>),
    /// 302 → 절대 URL.
    Redirect(String),
    /// 그 상태 + "nope".
    Status(u16),
    /// 처음 n 번은 404, 그 뒤 200.
    FailFirst(usize, Vec<u8>),
    /// Range 길이만큼 0 부터의 다른 창을 206 으로.
    StaleWindow(Vec<u8>),
}

#[derive(Default)]
struct Table {
    routes: HashMap<String, Route>,
    log: Vec<(String, Option<String>)>,
    hits: HashMap<String, usize>,
}

#[derive(Clone)]
struct Cdn {
    port: u16,
    table: Arc<Mutex<Table>>,
}

impl Cdn {
    async fn spawn() -> Self {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let table = Arc::new(Mutex::new(Table::default()));
        let t = table.clone();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let t = t.clone();
                tokio::spawn(async move {
                    let svc = service_fn(move |req: Request<Incoming>| {
                        let t = t.clone();
                        async move { Ok::<_, Infallible>(respond(&t, &req)) }
                    });
                    let _ = http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), svc)
                        .await;
                });
            }
        });
        Self { port, table }
    }

    fn url(&self, path: &str) -> Url {
        Url::parse(&format!("http://127.0.0.1:{}{path}", self.port)).unwrap()
    }

    fn set(&self, path: &str, route: Route) {
        self.table
            .lock()
            .unwrap()
            .routes
            .insert(path.to_string(), route);
    }

    fn set_all(&self, routes: Vec<(&str, Route)>) {
        for (p, r) in routes {
            self.set(p, r);
        }
    }

    fn requests(&self, path: &str) -> Vec<Option<String>> {
        let t = self.table.lock().unwrap();
        t.log
            .iter()
            .filter(|(p, _)| p == path)
            .map(|(_, r)| r.clone())
            .collect()
    }

    fn paths(&self) -> Vec<String> {
        let t = self.table.lock().unwrap();
        t.log.iter().map(|(p, _)| p.clone()).collect()
    }
}

fn reply(status: u16, body: Vec<u8>, headers: &[(&str, String)]) -> Response<Full<Bytes>> {
    let mut b = Response::builder().status(status);
    for (k, v) in headers {
        b = b.header(*k, v);
    }
    b.body(Full::new(Bytes::from(body))).unwrap()
}

fn parse_range(range: &str) -> Option<(usize, usize)> {
    let rest = range.strip_prefix("bytes=")?;
    let (a, b) = rest.split_once('-')?;
    Some((a.parse().ok()?, b.parse().ok()?))
}

fn respond(table: &Mutex<Table>, req: &Request<Incoming>) -> Response<Full<Bytes>> {
    let path = req.uri().path().to_string();
    let with_query = match req.uri().query() {
        Some(q) => format!("{path}?{q}"),
        None => path.clone(),
    };
    let range = req
        .headers()
        .get("range")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let (route, hit) = {
        let mut t = table.lock().unwrap();
        t.log.push((path.clone(), range.clone()));
        let hit = {
            let h = t.hits.entry(with_query.clone()).or_insert(0);
            *h += 1;
            *h
        };
        let route = t
            .routes
            .get(&with_query)
            .or_else(|| t.routes.get(&path))
            .cloned();
        (route, hit)
    };
    let nope = || reply(404, b"nope".to_vec(), &[]);
    match route {
        Some(Route::Body(data)) => match range.as_deref().and_then(parse_range) {
            None => reply(200, data, &[]),
            Some((a, b)) if a <= b && b < data.len() => reply(
                206,
                data[a..=b].to_vec(),
                &[("Content-Range", format!("bytes {a}-{b}/{}", data.len()))],
            ),
            Some(_) => reply(416, Vec::new(), &[]),
        },
        Some(Route::BodyIgnoringRange(data)) => reply(200, data, &[]),
        Some(Route::Redirect(target)) => reply(302, Vec::new(), &[("Location", target)]),
        Some(Route::Status(code)) => reply(code, b"nope".to_vec(), &[]),
        Some(Route::FailFirst(n, data)) => {
            if hit <= n {
                nope()
            } else {
                reply(200, data, &[])
            }
        }
        Some(Route::StaleWindow(data)) => {
            let len = range
                .as_deref()
                .and_then(parse_range)
                .map(|(a, b)| b - a + 1)
                .unwrap_or(data.len());
            reply(
                206,
                data[..len].to_vec(),
                &[(
                    "Content-Range",
                    format!("bytes 0-{}/{}", len - 1, data.len()),
                )],
            )
        }
        None => nope(),
    }
}

struct Harness {
    cdn: Cdn,
    service: StreamRecordingService,
    dir: PathBuf,
}

impl Harness {
    async fn new() -> Self {
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let dir = std::env::temp_dir().join(format!("cview-rec-{}", uuid::Uuid::new_v4()));
        Self {
            cdn: Cdn::spawn().await,
            service: StreamRecordingService::with_options(client, false, Duration::ZERO),
            dir,
        }
    }

    async fn record(&self, playlist: &str, polls: usize) -> Vec<u8> {
        self.record_with(playlist, polls, |_| {}).await
    }

    async fn record_with(&self, playlist: &str, polls: usize, between: impl Fn(usize)) -> Vec<u8> {
        let output = self.dir.join("out.ts");
        let url = self.cdn.url(playlist);
        self.service.start_recording(&url, &output).unwrap();
        for poll in 0..polls {
            between(poll);
            self.service.poll_once(&url).await;
        }
        self.service.stop_recording();
        let data = std::fs::read(&output).unwrap();
        let _ = std::fs::remove_dir_all(&self.dir);
        data
    }
}

fn media_seq(lines: &[&str], sequence: i64) -> Route {
    let mut all = vec![
        "#EXTM3U".to_string(),
        "#EXT-X-VERSION:7".into(),
        "#EXT-X-TARGETDURATION:2".into(),
        format!("#EXT-X-MEDIA-SEQUENCE:{sequence}"),
    ];
    all.extend(lines.iter().map(|s| s.to_string()));
    Route::Body((all.join("\n") + "\n").into_bytes())
}

fn media(lines: &[&str]) -> Route {
    media_seq(lines, 0)
}

const INIT0: &[u8] = b"INIT-A";
const INIT1: &[u8] = b"INIT-B";
const SEG0: &[u8] = b"SEG-0";
const SEG1: &[u8] = b"SEG-1";
const SEG2: &[u8] = b"SEG-2";

fn body(b: &[u8]) -> Route {
    Route::Body(b.to_vec())
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

fn pattern(modulo: usize) -> Vec<u8> {
    (0..1000).map(|i| (i % modulo) as u8).collect()
}

#[tokio::test]
async fn writes_init_once_before_fragments() {
    let h = Harness::new().await;
    h.cdn.set_all(vec![
        (
            "/live/chunklist.m3u8",
            media(&[
                "#EXT-X-MAP:URI=\"init.mp4\"",
                "#EXTINF:2,",
                "seg_0.m4s",
                "#EXTINF:2,",
                "seg_1.m4s",
            ]),
        ),
        ("/live/init.mp4", body(INIT0)),
        ("/live/seg_0.m4s", body(SEG0)),
        ("/live/seg_1.m4s", body(SEG1)),
    ]);
    let file = h.record("/live/chunklist.m3u8", 2).await;
    assert_eq!(file, cat(&[INIT0, SEG0, SEG1]));
    assert_eq!(h.cdn.requests("/live/init.mp4").len(), 1);
    assert_eq!(h.cdn.requests("/live/seg_0.m4s").len(), 1);
    assert_eq!(h.service.bytes_written(), file.len() as i64);
}

#[tokio::test]
async fn rewrites_init_when_map_changes() {
    let h = Harness::new().await;
    h.cdn.set_all(vec![
        (
            "/live/chunklist.m3u8",
            media(&["#EXT-X-MAP:URI=\"init_a.mp4\"", "#EXTINF:2,", "seg_0.m4s"]),
        ),
        ("/live/init_a.mp4", body(INIT0)),
        ("/live/init_b.mp4", body(INIT1)),
        ("/live/seg_0.m4s", body(SEG0)),
        ("/live/seg_1.m4s", body(SEG1)),
    ]);
    let cdn = h.cdn.clone();
    let file = h
        .record_with("/live/chunklist.m3u8", 2, |poll| {
            if poll == 1 {
                cdn.set(
                    "/live/chunklist.m3u8",
                    media(&[
                        "#EXTINF:2,",
                        "seg_0.m4s",
                        "#EXT-X-MAP:URI=\"init_b.mp4\"",
                        "#EXTINF:2,",
                        "seg_1.m4s",
                    ]),
                );
            }
        })
        .await;
    assert_eq!(file, cat(&[INIT0, SEG0, INIT1, SEG1]));
}

#[tokio::test]
async fn records_byte_range_sub_segments() {
    for ignores_range in [false, true] {
        let h = Harness::new().await;
        let data = pattern(251);
        h.cdn.set_all(vec![
            (
                "/live/chunklist.m3u8",
                media(&[
                    "#EXTINF:2,",
                    "#EXT-X-BYTERANGE:300@0",
                    "media.mp4",
                    "#EXTINF:2,",
                    "#EXT-X-BYTERANGE:200",
                    "media.mp4",
                    "#EXTINF:2,",
                    "#EXT-X-BYTERANGE:100@600",
                    "media.mp4",
                ]),
            ),
            (
                "/live/media.mp4",
                if ignores_range {
                    Route::BodyIgnoringRange(data.clone())
                } else {
                    Route::Body(data.clone())
                },
            ),
        ]);
        let file = h.record("/live/chunklist.m3u8", 2).await;
        assert_eq!(
            file,
            cat(&[&data[0..300], &data[300..500], &data[600..700]])
        );
        assert_eq!(
            h.cdn.requests("/live/media.mp4"),
            vec![
                Some("bytes=0-299".to_string()),
                Some("bytes=300-499".into()),
                Some("bytes=600-699".into())
            ],
            "두 번째 폴에서 다시 받았거나 Range 가 빠졌다 (ignores_range={ignores_range})"
        );
    }
}

#[tokio::test]
async fn media_playlist_redirect_uses_final_url() {
    let h = Harness::new().await;
    h.cdn.set_all(vec![
        (
            "/old/live.m3u8",
            Route::Redirect(h.cdn.url("/cdn/v1/chunklist.m3u8").to_string()),
        ),
        (
            "/cdn/v1/chunklist.m3u8",
            media(&["#EXTINF:2,", "seg_0.m4s"]),
        ),
        ("/cdn/v1/seg_0.m4s", body(SEG0)),
        ("/old/seg_0.m4s", Route::Status(404)),
    ]);
    let file = h.record("/old/live.m3u8", 1).await;
    assert_eq!(file, SEG0);
    assert!(
        h.cdn.requests("/old/seg_0.m4s").is_empty(),
        "옛 경로로 조각을 받으러 갔다"
    );
}

#[tokio::test]
async fn master_redirect_uses_final_url() {
    let h = Harness::new().await;
    let master =
        "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=3000000,RESOLUTION=1280x720\n720p/chunklist.m3u8\n";
    h.cdn.set_all(vec![
        (
            "/m/master.m3u8",
            Route::Redirect(h.cdn.url("/cdn/m/master.m3u8").to_string()),
        ),
        ("/cdn/m/master.m3u8", body(master.as_bytes())),
        (
            "/cdn/m/720p/chunklist.m3u8",
            media(&["#EXTINF:2,", "seg_0.m4s"]),
        ),
        ("/cdn/m/720p/seg_0.m4s", body(SEG0)),
    ]);
    let file = h.record("/m/master.m3u8", 1).await;
    assert_eq!(file, SEG0);
    assert!(h.cdn.requests("/m/720p/chunklist.m3u8").is_empty());
}

#[tokio::test]
async fn error_bodies_are_not_recorded() {
    let h = Harness::new().await;
    h.cdn.set_all(vec![
        (
            "/live/chunklist.m3u8",
            media(&["#EXTINF:2,", "gone.m4s", "#EXTINF:2,", "flaky.m4s"]),
        ),
        ("/live/gone.m4s", Route::Status(403)),
        ("/live/flaky.m4s", Route::FailFirst(1, SEG1.to_vec())),
    ]);
    let file = h
        .record("/live/chunklist.m3u8", MAX_BLOCKED_POLLS as usize + 1)
        .await;
    assert_eq!(file, SEG1);
    assert_eq!(
        h.cdn.requests("/live/gone.m4s").len(),
        MAX_BLOCKED_POLLS as usize,
        "4xx 를 한 폴 안에서 재시도했거나 한도 뒤에도 계속 막았다"
    );
    assert_eq!(h.cdn.requests("/live/flaky.m4s").len(), 2);
}

#[tokio::test]
async fn failed_middle_segment_keeps_file_order() {
    let h = Harness::new().await;
    h.cdn.set_all(vec![
        (
            "/live/chunklist.m3u8",
            media(&[
                "#EXTINF:2,",
                "seg_0.m4s",
                "#EXTINF:2,",
                "seg_1.m4s",
                "#EXTINF:2,",
                "seg_2.m4s",
            ]),
        ),
        ("/live/seg_0.m4s", body(SEG0)),
        ("/live/seg_1.m4s", Route::FailFirst(1, SEG1.to_vec())),
        ("/live/seg_2.m4s", body(SEG2)),
    ]);
    let after_first = Mutex::new(None);
    let cdn = h.cdn.clone();
    let file = h
        .record_with("/live/chunklist.m3u8", 2, |poll| {
            if poll == 1 {
                *after_first.lock().unwrap() = Some(cdn.requests("/live/seg_2.m4s"));
            }
        })
        .await;
    assert_eq!(
        file,
        cat(&[SEG0, SEG1, SEG2]),
        "실패한 가운데 조각이 뒤 조각 뒤에 붙었다"
    );
    assert_eq!(after_first.into_inner().unwrap(), Some(vec![]));
    assert_eq!(h.cdn.requests("/live/seg_2.m4s").len(), 1);
}

#[tokio::test]
async fn server_errors_retry_within_poll() {
    let h = Harness::new().await;
    h.cdn.set_all(vec![
        ("/live/chunklist.m3u8", media(&["#EXTINF:2,", "busy.m4s"])),
        ("/live/busy.m4s", Route::Status(503)),
    ]);
    let file = h.record("/live/chunklist.m3u8", 1).await;
    assert!(file.is_empty());
    assert_eq!(h.cdn.requests("/live/busy.m4s").len(), 3);
}

#[tokio::test]
async fn fragment_waits_for_init() {
    let h = Harness::new().await;
    h.cdn.set_all(vec![
        (
            "/live/chunklist.m3u8",
            media(&[
                "#EXT-X-MAP:URI=\"init.mp4\"",
                "#EXTINF:2,",
                "seg_0.m4s",
                "#EXTINF:2,",
                "seg_1.m4s",
                "#EXTINF:2,",
                "seg_2.m4s",
            ]),
        ),
        ("/live/init.mp4", Route::Status(404)),
        ("/live/seg_0.m4s", body(SEG0)),
        ("/live/seg_1.m4s", body(SEG1)),
    ]);
    let file = h.record("/live/chunklist.m3u8", 2).await;
    assert!(file.is_empty());
    assert_eq!(
        h.cdn.requests("/live/init.mp4").len(),
        2,
        "조각마다 init 을 다시 받으러 갔다"
    );
    assert!(h.cdn.requests("/live/seg_0.m4s").is_empty());
}

#[tokio::test]
async fn records_ranged_map_init() {
    for ignores_range in [false, true] {
        let h = Harness::new().await;
        let data = pattern(241);
        h.cdn.set_all(vec![
            (
                "/live/chunklist.m3u8",
                media(&[
                    "#EXT-X-MAP:URI=\"media.mp4\",BYTERANGE=\"100@0\"",
                    "#EXTINF:2,",
                    "#EXT-X-BYTERANGE:200@100",
                    "media.mp4",
                    "#EXTINF:2,",
                    "#EXT-X-BYTERANGE:300",
                    "media.mp4",
                ]),
            ),
            (
                "/live/media.mp4",
                if ignores_range {
                    Route::BodyIgnoringRange(data.clone())
                } else {
                    Route::Body(data.clone())
                },
            ),
        ]);
        let file = h.record("/live/chunklist.m3u8", 2).await;
        assert_eq!(
            file,
            cat(&[&data[0..100], &data[100..300], &data[300..600]])
        );
        assert_eq!(
            h.cdn.requests("/live/media.mp4"),
            vec![
                Some("bytes=0-99".to_string()),
                Some("bytes=100-299".into()),
                Some("bytes=300-599".into())
            ]
        );
    }
}

#[tokio::test]
async fn rotating_hosts_and_tokens_do_not_duplicate() {
    let h = Harness::new().await;
    let edges = [Cdn::spawn().await, Cdn::spawn().await];
    for (index, edge) in edges.iter().enumerate() {
        let token = format!("hdnts=exp={index}~acl=*~hmac=ab{index}");
        let map = format!("#EXT-X-MAP:URI=\"init.mp4?{token}\"");
        let seg0 = format!("seg_0.m4s?{token}");
        let seg1 = format!("hdntl=exp~{index}/seg_1.m4s");
        let seg2 = format!("seg_2.m4s?__token__={index}");
        let mut lines = vec![
            map.as_str(),
            "#EXTINF:2,",
            seg0.as_str(),
            "#EXTINF:2,",
            seg1.as_str(),
        ];
        if index == 1 {
            lines.extend(["#EXTINF:2,", seg2.as_str()]);
        }
        edge.set_all(vec![
            ("/cdn/chunklist.m3u8", media(&lines)),
            ("/cdn/init.mp4", body(INIT0)),
            ("/cdn/seg_0.m4s", body(SEG0)),
            (&format!("/cdn/hdntl=exp~{index}/seg_1.m4s"), body(SEG1)),
            ("/cdn/seg_2.m4s", body(SEG2)),
        ]);
    }
    h.cdn.set(
        "/live.m3u8",
        Route::Redirect(edges[0].url("/cdn/chunklist.m3u8").to_string()),
    );
    let cdn = h.cdn.clone();
    let second = edges[1].url("/cdn/chunklist.m3u8").to_string();
    let file = h
        .record_with("/live.m3u8", 2, |poll| {
            if poll == 1 {
                cdn.set("/live.m3u8", Route::Redirect(second.clone()));
            }
        })
        .await;
    assert_eq!(file, cat(&[INIT0, SEG0, SEG1, SEG2]));
    let requested: Vec<String> = edges.iter().flat_map(Cdn::paths).collect();
    let count = |f: &dyn Fn(&str) -> bool| requested.iter().filter(|p| f(p)).count();
    assert_eq!(
        count(&|p| p == "/cdn/init.mp4"),
        1,
        "토큰만 바뀐 MAP 을 새 init 으로 다시 썼다"
    );
    assert_eq!(count(&|p| p == "/cdn/seg_0.m4s"), 1);
    assert_eq!(
        count(&|p| p.ends_with("/seg_1.m4s")),
        1,
        "hdntl 토큰만 바뀐 조각을 다시 받았다"
    );
}

#[tokio::test]
async fn query_only_differences_are_distinct() {
    let h = Harness::new().await;
    h.cdn.set_all(vec![
        (
            "/live/chunklist.m3u8",
            media(&[
                "#EXT-X-MAP:URI=\"init.mp4?v=1\"",
                "#EXTINF:2,",
                "seg.m4s?n=0",
                "#EXTINF:2,",
                "seg.m4s?n=1",
            ]),
        ),
        ("/live/init.mp4?v=1", body(INIT0)),
        ("/live/init.mp4?v=2", body(INIT1)),
        ("/live/seg.m4s?n=0", body(SEG0)),
        ("/live/seg.m4s?n=1", body(SEG1)),
        ("/live/seg.m4s?n=2", body(SEG2)),
    ]);
    let cdn = h.cdn.clone();
    let file = h
        .record_with("/live/chunklist.m3u8", 2, |poll| {
            if poll == 1 {
                cdn.set(
                    "/live/chunklist.m3u8",
                    media_seq(
                        &[
                            "#EXTINF:2,",
                            "seg.m4s?n=1",
                            "#EXT-X-MAP:URI=\"init.mp4?v=2\"",
                            "#EXTINF:2,",
                            "seg.m4s?n=2",
                        ],
                        1,
                    ),
                );
            }
        })
        .await;
    assert_eq!(file, cat(&[INIT0, SEG0, SEG1, INIT1, SEG2]));
}

#[tokio::test]
async fn discontinuity_rewrites_init() {
    let h = Harness::new().await;
    let restarted = b"INIT-RESTART";
    h.cdn.set_all(vec![
        (
            "/live/chunklist.m3u8",
            media(&[
                "#EXT-X-MAP:URI=\"init.mp4\"",
                "#EXTINF:2,",
                "seg_0.m4s",
                "#EXT-X-DISCONTINUITY",
                "#EXTINF:2,",
                "seg_1.m4s",
            ]),
        ),
        ("/live/init.mp4", body(INIT0)),
        ("/live/seg_0.m4s", body(SEG0)),
        ("/live/seg_1.m4s", Route::FailFirst(1, SEG1.to_vec())),
    ]);
    let cdn = h.cdn.clone();
    let file = h
        .record_with("/live/chunklist.m3u8", 3, |poll| {
            if poll != 0 {
                cdn.set("/live/init.mp4", body(restarted));
            }
        })
        .await;
    assert_eq!(file, cat(&[INIT0, SEG0, INIT0, SEG1]));
    assert_eq!(h.cdn.requests("/live/init.mp4").len(), 2);
}

#[tokio::test]
async fn skipped_discontinuity_still_rewrites_init() {
    let h = Harness::new().await;
    let restarted: &[u8] = b"INIT-RESTART";
    h.cdn.set_all(vec![
        (
            "/live/chunklist.m3u8",
            media(&["#EXT-X-MAP:URI=\"init.mp4\"", "#EXTINF:2,", "seg_0.m4s"]),
        ),
        ("/live/init.mp4", body(INIT0)),
        ("/live/seg_0.m4s", body(SEG0)),
        ("/live/seg_1.m4s", body(SEG1)),
        ("/live/seg_2.m4s", body(SEG2)),
    ]);
    let blocked = MAX_BLOCKED_POLLS as usize;
    let cdn = h.cdn.clone();
    let file = h
        .record_with("/live/chunklist.m3u8", blocked + 2, |poll| {
            if poll == 1 {
                cdn.set(
                    "/live/chunklist.m3u8",
                    media(&[
                        "#EXT-X-MAP:URI=\"init.mp4\"",
                        "#EXTINF:2,",
                        "seg_0.m4s",
                        "#EXT-X-DISCONTINUITY",
                        "#EXTINF:2,",
                        "seg_1.m4s",
                        "#EXTINF:2,",
                        "seg_2.m4s",
                    ]),
                );
                cdn.set("/live/init.mp4", Route::Status(404));
            }
            if poll == blocked + 1 {
                cdn.set("/live/init.mp4", body(restarted));
            }
        })
        .await;
    assert_eq!(
        file,
        cat(&[INIT0, SEG0, restarted, SEG2]),
        "건너뛴 불연속의 init 재기록을 잊고 옛 moov 뒤에 붙였다"
    );
    assert!(h.cdn.requests("/live/seg_1.m4s").is_empty());
}

#[tokio::test]
async fn invalid_discontinuity_restarts_init_once() {
    let h = Harness::new().await;
    let seg3: &[u8] = b"SEG-3";
    let lines = [
        "#EXT-X-MAP:URI=\"init.mp4\"",
        "#EXTINF:2,",
        "seg_0.m4s",
        "#EXT-X-DISCONTINUITY",
        "#EXT-X-BYTERANGE:100",
        "#EXTINF:2,",
        "blob.m4s",
        "#EXTINF:2,",
        "seg_2.m4s",
    ];
    h.cdn.set_all(vec![
        ("/live/chunklist.m3u8", media(&lines)),
        ("/live/init.mp4", body(INIT0)),
        ("/live/seg_0.m4s", body(SEG0)),
        ("/live/seg_2.m4s", body(SEG2)),
        ("/live/seg_3.m4s", body(seg3)),
    ]);
    let cdn = h.cdn.clone();
    let file = h
        .record_with("/live/chunklist.m3u8", 2, |poll| {
            if poll == 1 {
                let mut more = lines.to_vec();
                more.extend(["#EXTINF:2,", "seg_3.m4s"]);
                cdn.set("/live/chunklist.m3u8", media(&more));
            }
        })
        .await;
    assert_eq!(file, cat(&[INIT0, SEG0, INIT0, SEG2, seg3]));
    assert!(h.cdn.requests("/live/blob.m4s").is_empty());
}

#[tokio::test]
async fn stale_content_range_is_rejected() {
    let h = Harness::new().await;
    h.cdn.set_all(vec![
        (
            "/live/chunklist.m3u8",
            media(&["#EXTINF:2,", "#EXT-X-BYTERANGE:200@300", "media.mp4"]),
        ),
        ("/live/media.mp4", Route::StaleWindow(pattern(239))),
    ]);
    let file = h.record("/live/chunklist.m3u8", 1).await;
    assert!(file.is_empty());
}

#[test]
fn recording_key_table() {
    let key = |s: &str| {
        StreamRecordingService::recording_key(&Url::parse(s).unwrap(), &HlsByteSpan::Whole)
    };
    let range = HlsByteSpan::Range(ByteRange {
        length: 10,
        offset: Some(20),
    });
    assert_eq!(
        key("https://a.test/x/hdntl=exp~1/seg.m4s?hdnts=1&__token__=2"),
        key("https://b.test/x/hdntl=exp~2/seg.m4s?HDNTS=3&__bgda__=4&hmac=5&sig=6")
    );
    assert_ne!(
        key("https://a.test/x/seg.m4s?n=5"),
        key("https://a.test/x/seg.m4s?n=6")
    );
    assert_eq!(
        key("https://a.test/x/seg.m4s?n=5&hdnts=1"),
        key("https://b.test/x/seg.m4s?n=5&hdnts=2")
    );
    assert_ne!(
        key("https://a.test/x/init.mp4?v=2"),
        key("https://a.test/x/init.mp4")
    );
    let u = Url::parse("https://a.test/x/m.mp4?v=1").unwrap();
    assert_ne!(
        StreamRecordingService::recording_key(&u, &range),
        key("https://a.test/x/m.mp4?v=1")
    );
    assert_eq!(
        StreamRecordingService::recording_key(&u, &range),
        "x/m.mp4#20-10?v=1"
    );
    assert_eq!(
        key("https://a.test/x/seg.m4s"),
        file_key(&Url::parse("https://a.test/x/seg.m4s").unwrap())
    );
}

#[test]
fn aes128_cbc_round_trip_and_bad_key() {
    use aes::cipher::{BlockEncryptMut, KeyIvInit};
    let key = [7u8; 16];
    let iv = [9u8; 16];
    let plain = b"MPEG-TS payload that is not block aligned".to_vec();
    let cipher = cbc::Encryptor::<aes::Aes128>::new(&key.into(), &iv.into())
        .encrypt_padded_vec_mut::<Pkcs7>(&plain);
    assert_eq!(aes128_cbc_decrypt(&cipher, &key, &iv).unwrap(), plain);
    assert!(aes128_cbc_decrypt(&cipher, &[1u8; 15], &iv).is_err());
    assert!(aes128_cbc_decrypt(&cipher[..cipher.len() - 1], &key, &iv).is_err());
}

#[tokio::test]
async fn decrypts_aes128_segments_with_cached_key() {
    use aes::cipher::{BlockEncryptMut, KeyIvInit};
    let h = Harness::new().await;
    let key = [3u8; 16];
    let iv: [u8; 16] = core::array::from_fn(|i| i as u8);
    let enc = |p: &[u8]| {
        cbc::Encryptor::<aes::Aes128>::new(&key.into(), &iv.into())
            .encrypt_padded_vec_mut::<Pkcs7>(p)
    };
    h.cdn.set_all(vec![
        (
            "/live/chunklist.m3u8",
            media(&[
                "#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\",IV=0x000102030405060708090A0B0C0D0E0F",
                "#EXTINF:2,",
                "seg_0.ts",
                "#EXTINF:2,",
                "seg_1.ts",
            ]),
        ),
        ("/live/key.bin", Route::Body(key.to_vec())),
        ("/live/seg_0.ts", Route::Body(enc(SEG0))),
        ("/live/seg_1.ts", Route::Body(enc(SEG1))),
    ]);
    let file = h.record("/live/chunklist.m3u8", 1).await;
    assert_eq!(file, cat(&[SEG0, SEG1]));
    assert_eq!(
        h.cdn.requests("/live/key.bin").len(),
        1,
        "키를 캐시하지 않았다"
    );
}

#[tokio::test]
async fn start_twice_fails_and_stop_resets_state() {
    let h = Harness::new().await;
    let out = h.dir.join("nested/a.ts");
    let url = h.cdn.url("/x.m3u8");
    h.service.start_recording(&url, &out).unwrap();
    assert!(h.service.is_recording());
    assert!(h.service.recording_start_date().is_some());
    assert!(matches!(
        h.service.start_recording(&url, &out),
        Err(PlayerError::RecordingFailed(_))
    ));
    h.service.stop_recording();
    assert_eq!(h.service.current_state(), RecordingState::Idle);
    assert!(out.exists());
    let _ = std::fs::remove_dir_all(&h.dir);
}

#[tokio::test]
async fn auto_poll_loop_records_until_stopped() {
    let cdn = Cdn::spawn().await;
    cdn.set_all(vec![
        ("/live/chunklist.m3u8", media(&["#EXTINF:2,", "seg_0.m4s"])),
        ("/live/seg_0.m4s", body(SEG0)),
    ]);
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let service = StreamRecordingService::with_options(client, true, Duration::ZERO);
    let dir = std::env::temp_dir().join(format!("cview-rec-{}", uuid::Uuid::new_v4()));
    let out = dir.join("auto.ts");
    service
        .start_recording(&cdn.url("/live/chunklist.m3u8"), &out)
        .unwrap();
    for _ in 0..200 {
        if service.bytes_written() > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    service.stop_recording();
    assert_eq!(std::fs::read(&out).unwrap(), SEG0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn default_recording_path_sanitizes_channel_name() {
    use chrono::TimeZone;
    let now = Local.with_ymd_and_hms(2026, 10, 19, 14, 30, 5).unwrap();
    let p = StreamRecordingService::default_recording_path(
        Some(Path::new("/movies")),
        "한글 Name/ㄱㅏ:x_-!",
        now,
    );
    assert_eq!(
        p,
        PathBuf::from("/movies/CView/한글_Name_ㄱㅏ_x_-__2026-10-19T143005.ts")
    );
}
