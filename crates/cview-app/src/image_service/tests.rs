use super::*;
use async_trait::async_trait;
use cview_networking::{HttpResponse, TransportError};
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

/// 요청 수를 세고, 차례로 정한 응답을 내는 전송.
struct Fake {
    calls: AtomicUsize,
    offline: std::sync::atomic::AtomicBool,
    body: Mutex<Vec<u8>>,
    status: Mutex<u16>,
    delay_ms: u64,
    last_if_none_match: Mutex<Option<String>>,
}

impl Fake {
    fn new(delay_ms: u64) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            offline: false.into(),
            body: Mutex::new(b"img-1".to_vec()),
            status: Mutex::new(200),
            delay_ms,
            last_if_none_match: Mutex::new(None),
        })
    }
}

#[async_trait]
impl HttpTransport for Fake {
    async fn send(&self, req: HttpRequest) -> Result<HttpResponse, TransportError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.last_if_none_match.lock().unwrap() = req.header("If-None-Match").map(str::to_string);
        assert!(req.header("Cookie").is_none(), "쿠키를 붙이지 않는다");
        if self.delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
        }
        if self.offline.load(Ordering::SeqCst) {
            return Err(TransportError::NotConnected);
        }
        let mut r = HttpResponse::new(
            *self.status.lock().unwrap(),
            self.body.lock().unwrap().clone(),
        );
        r.headers.push(("Content-Type".into(), "image/jpeg".into()));
        r.headers.push(("ETag".into(), "\"v1\"".into()));
        Ok(r)
    }
}

const URL: &str = "https://livecloud-thumb.akamaized.net/x/image_405.jpg";

fn service(fake: Arc<Fake>, dir: &std::path::Path, clock: Arc<AtomicI64>) -> Arc<ImageService> {
    let cache = Arc::new(ImageCache::open(dir, 10 * 1024 * 1024).unwrap());
    ImageService::with_clock(
        Some(cache),
        fake,
        Arc::new(move || clock.load(Ordering::SeqCst)),
    )
}

async fn settle() {
    for _ in 0..50 {
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

#[test]
fn only_https_image_cdns_are_allowed() {
    assert!(is_allowed_image_url("https://nng-phinf.pstatic.net/a.png"));
    assert!(is_allowed_image_url(URL));
    assert!(!is_allowed_image_url("http://nng-phinf.pstatic.net/a.png"));
    assert!(!is_allowed_image_url("https://example.com/a.png"));
    assert!(!is_allowed_image_url("https://pstatic.net.evil.com/a.png"));
    assert!(!is_allowed_image_url("file:///etc/passwd"));
    assert!(!is_allowed_image_url("https://a.pstatic.net:8443/a.png"));
    assert!(!is_allowed_image_url("https://u:p@a.pstatic.net/a.png"));
    assert!(is_allowed_image_url("https://a.pstatic.net:443/a.png"));
}

#[test]
fn redirect_hops_follow_the_same_policy() {
    let allowed = |u: &Url| is_allowed_image_url(u.as_str());
    let start = Url::parse("https://nng-phinf.pstatic.net/a.png").unwrap();
    let hop = |s: &str| Url::parse(s).unwrap();
    let prev = std::slice::from_ref(&start);
    assert_eq!(
        redirect_decision(&hop("https://b.akamaized.net/a.png"), prev, allowed),
        RedirectDecision::Follow
    );
    // 허용 → 비허용 호스트, HTTPS → HTTP, 다른 포트
    for bad in [
        "https://example.com/a.png",
        "http://nng-phinf.pstatic.net/a.png",
        "https://nng-phinf.pstatic.net:8443/a.png",
        "http://127.0.0.1/a.png",
    ] {
        assert_eq!(
            redirect_decision(&hop(bad), prev, allowed),
            RedirectDecision::Forbidden,
            "{bad}"
        );
    }
    // 순환
    let second = hop("https://b.akamaized.net/a.png");
    assert_eq!(
        redirect_decision(&start, &[start.clone(), second], allowed),
        RedirectDecision::Loop
    );
    // 상한
    let chain: Vec<Url> = (0..=MAX_IMAGE_REDIRECTS)
        .map(|i| hop(&format!("https://a.pstatic.net/{i}.png")))
        .collect();
    assert_eq!(
        redirect_decision(&hop("https://a.pstatic.net/last.png"), &chain, allowed),
        RedirectDecision::TooMany
    );
}

/// 로컬 통제 서버 — `/img/*` 만 허용 목적지로 보고, 경로마다 정한 응답을 낸다. 받은 경로를 기록한다.
async fn redirect_server() -> (u16, Arc<Mutex<Vec<String>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(Mutex::new(Vec::new()));
    let log = hits.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let log = log.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
                log.lock().unwrap().push(path.clone());
                let base = format!("http://127.0.0.1:{port}");
                let resp = match path.as_str() {
                        "/img/start" => redirect_to(&format!("{base}/img/final")),
                        "/img/out" => redirect_to(&format!("{base}/evil")),
                        "/img/loop" => redirect_to(&format!("{base}/img/loop")),
                        _ => "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 3\r\nConnection: close\r\n\r\npng".to_string(),
                    };
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    });
    (port, hits)
}

fn redirect_to(location: &str) -> String {
    format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
}

#[tokio::test]
async fn client_checks_every_redirect_hop_against_a_local_server() {
    let (port, hits) = redirect_server().await;
    let client = reqwest::Client::builder()
        .redirect(redirect_policy_with(|u| u.path().starts_with("/img/")))
        .build()
        .unwrap();
    let base = format!("http://127.0.0.1:{port}");
    // 허용 → 허용: 따라간다
    let ok = client
        .get(format!("{base}/img/start"))
        .send()
        .await
        .unwrap();
    assert_eq!(ok.status(), 200);
    assert_eq!(ok.url().path(), "/img/final");
    // 허용 → 비허용: 목적지에 접속하지 않는다
    let out = client.get(format!("{base}/img/out")).send().await;
    assert!(out.unwrap_err().is_redirect());
    // 순환: 멈춘다
    let looped = client.get(format!("{base}/img/loop")).send().await;
    assert!(looped.unwrap_err().is_redirect());
    let hits = hits.lock().unwrap().clone();
    assert!(
        !hits.iter().any(|p| p == "/evil"),
        "비허용 목적지 요청: {hits:?}"
    );
    assert_eq!(hits.iter().filter(|p| *p == "/img/loop").count(), 1);
}

#[test]
fn request_paths_split_kind_and_url() {
    let (k, u) =
        parse_request_path("/live%2Fhttps%3A%2F%2Fa.pstatic.net%2Fx.jpg%3Ftype%3Df120").unwrap();
    assert_eq!(k, ImageKind::Live);
    assert_eq!(u, "https://a.pstatic.net/x.jpg?type=f120");
    let (k, u) = parse_request_path("/avatar/https://a.pstatic.net/p.png").unwrap();
    assert_eq!(k, ImageKind::Avatar);
    assert_eq!(u, "https://a.pstatic.net/p.png");
    assert!(parse_request_path("/live").is_none());
}

#[tokio::test]
async fn concurrent_loads_share_one_request_and_then_hit_memory() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(30);
    let s = service(fake.clone(), dir.path(), Arc::new(AtomicI64::new(0)));
    let (a, b) = tokio::join!(s.load(URL, ImageKind::Live), s.load(URL, ImageKind::Live));
    assert_eq!(a.unwrap().bytes.as_slice(), b"img-1");
    assert_eq!(b.unwrap().bytes.as_slice(), b"img-1");
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    s.load(URL, ImageKind::Live).await.unwrap();
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1, "신선하면 받지 않는다");
}

#[tokio::test]
async fn restart_serves_from_disk_without_network() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(AtomicI64::new(0));
    let fake = Fake::new(0);
    service(fake.clone(), dir.path(), clock.clone())
        .load(URL, ImageKind::Avatar)
        .await
        .unwrap();
    settle().await; // 디스크 쓰기는 응답 뒤에 따로 한다
                    // 다시 실행 — 메모리는 비었고 디스크만 있다. 오프라인이어도 보인다.
    let fake2 = Fake::new(0);
    fake2.offline.store(true, Ordering::SeqCst);
    let s2 = service(fake2.clone(), dir.path(), clock.clone());
    let got = s2.load(URL, ImageKind::Avatar).await.unwrap();
    assert_eq!(got.bytes.as_slice(), b"img-1");
    assert_eq!(fake2.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn stale_is_served_immediately_and_refreshed_in_background() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(AtomicI64::new(0));
    let fake = Fake::new(0);
    let s = service(fake.clone(), dir.path(), clock.clone());
    s.load(URL, ImageKind::Live).await.unwrap();
    *fake.body.lock().unwrap() = b"img-2".to_vec();
    clock.store(60_000, Ordering::SeqCst); // 라이브 TTL(45초) 지남
    let got = s.load(URL, ImageKind::Live).await.unwrap();
    assert_eq!(got.bytes.as_slice(), b"img-1", "오래된 것을 바로 내준다");
    settle().await;
    assert_eq!(
        fake.calls.load(Ordering::SeqCst),
        2,
        "뒤에서 한 번 새로 받는다"
    );
    assert_eq!(
        fake.last_if_none_match.lock().unwrap().as_deref(),
        Some("\"v1\""),
        "가진 ETag 로 조건부 요청"
    );
    let got = s.load(URL, ImageKind::Live).await.unwrap();
    assert_eq!(got.bytes.as_slice(), b"img-2", "다음엔 새 그림");
}

#[tokio::test]
async fn offline_refresh_keeps_the_stale_image() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(AtomicI64::new(0));
    let fake = Fake::new(0);
    let s = service(fake.clone(), dir.path(), clock.clone());
    s.load(URL, ImageKind::Live).await.unwrap();
    fake.offline.store(true, Ordering::SeqCst);
    clock.store(60_000, Ordering::SeqCst);
    assert_eq!(
        s.load(URL, ImageKind::Live).await.unwrap().bytes.as_slice(),
        b"img-1"
    );
    settle().await;
    assert_eq!(
        s.load(URL, ImageKind::Live).await.unwrap().bytes.as_slice(),
        b"img-1"
    );
}

#[tokio::test]
async fn not_modified_marks_fresh_without_new_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(AtomicI64::new(0));
    let fake = Fake::new(0);
    let s = service(fake.clone(), dir.path(), clock.clone());
    s.load(URL, ImageKind::Live).await.unwrap();
    *fake.status.lock().unwrap() = 304;
    *fake.body.lock().unwrap() = Vec::new();
    clock.store(60_000, Ordering::SeqCst);
    s.load(URL, ImageKind::Live).await.unwrap();
    settle().await;
    let calls = fake.calls.load(Ordering::SeqCst);
    clock.store(70_000, Ordering::SeqCst);
    assert_eq!(
        s.load(URL, ImageKind::Live).await.unwrap().bytes.as_slice(),
        b"img-1"
    );
    assert_eq!(
        fake.calls.load(Ordering::SeqCst),
        calls,
        "304 뒤엔 다시 신선"
    );
}

#[tokio::test]
async fn misses_fail_offline_and_forbidden_hosts_never_fetch() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(0);
    fake.offline.store(true, Ordering::SeqCst);
    let s = service(fake.clone(), dir.path(), Arc::new(AtomicI64::new(0)));
    assert!(matches!(
        s.load(URL, ImageKind::Live).await,
        Err(ImageError::Unavailable(_))
    ));
    assert_eq!(
        s.load("https://example.com/x.png", ImageKind::Static)
            .await
            .unwrap_err(),
        ImageError::Forbidden
    );
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn clear_empties_memory_and_disk() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(0);
    let s = service(fake.clone(), dir.path(), Arc::new(AtomicI64::new(0)));
    s.load(URL, ImageKind::Avatar).await.unwrap();
    settle().await;
    assert_eq!(s.stats().entries, 1);
    s.clear();
    assert_eq!(s.stats().entries, 0);
    s.load(URL, ImageKind::Avatar).await.unwrap();
    assert_eq!(
        fake.calls.load(Ordering::SeqCst),
        2,
        "비운 뒤엔 다시 받는다"
    );
}
