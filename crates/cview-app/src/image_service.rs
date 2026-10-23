//! 썸네일·아바타 서비스 — `cvimg:` 프로토콜 뒤에서 이미지를 받아 디스크·메모리에 두고 내준다.
//!
//! 화면은 `<img src="cvimg://localhost/<종류>%2F<원본 URL>">`(Windows 는 `http://cvimg.localhost/...`)로 부른다.
//! - 메모리(최근 48MB) → 디스크(`image-cache/`, [`ImageCache`]) → 네트워크 순서.
//! - 오래된 것(라이브 45초·그 밖 7일)도 바로 내주고 뒤에서 새로 받는다(stale-while-revalidate).
//!   라이브 썸네일은 CDN 이 `max-age=5`·ETag 없음이라, 예전엔 다시 실행할 때마다 전부 새로 받았다.
//! - 같은 URL 을 동시에 부르면 한 번만 받고, 동시에 받는 수는 16개로 묶는다. 디스크 쓰기는 응답 뒤에 따로 한다.
//! - 쿠키·인증 헤더 없이, 허용한 이미지 호스트(https)만 받는다 — 열린 프록시가 되지 않게.
//!   리다이렉트도 hop 마다 같은 정책을 거친다([`redirect_policy`]).
//! - 오프라인이면 캐시된 것을 내주고, 없으면 실패(화면은 빈칸).

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cview_networking::{HttpRequest, HttpTransport};
use cview_persistence::image_cache::{CacheStats, CachedImage, ImageCache, ImageKind, ImageMeta};
use futures_util::future::{BoxFuture, FutureExt, Shared};
use tokio::sync::Semaphore;
use url::Url;

/// 메모리에 둘 최근 이미지 바이트 상한.
const HOT_CAP_BYTES: usize = 48 * 1024 * 1024;
/// 동시에 받는 수 — 이미지 CDN 은 HTTP/1.1 이라 연결 수가 곧 병렬도다(6 이면 첫 실행이 웹뷰 직접 받기보다 느렸다).
const MAX_CONCURRENT_FETCHES: usize = 16;
const FETCH_TIMEOUT: Duration = Duration::from_secs(15);
/// 따라갈 리다이렉트 수 상한.
const MAX_IMAGE_REDIRECTS: usize = 5;

/// 받을 수 있는 이미지 호스트(접미사). 치지직·네이버 이미지 CDN 뿐이다.
const ALLOWED_HOST_SUFFIXES: [&str; 4] =
    [".pstatic.net", ".akamaized.net", ".naver.net", ".naver.com"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    /// 허용하지 않는 URL(https 가 아니거나 다른 호스트).
    Forbidden,
    /// 받지 못했고 캐시에도 없다.
    Unavailable(String),
}

/// 내줄 이미지.
#[derive(Debug, Clone)]
pub struct Served {
    pub bytes: Arc<Vec<u8>>,
    pub content_type: String,
    pub kind: ImageKind,
}

#[derive(Clone)]
struct HotEntry {
    bytes: Arc<Vec<u8>>,
    content_type: String,
    kind: ImageKind,
    fetched_at: i64,
    etag: Option<String>,
    last_modified: Option<String>,
}

impl HotEntry {
    fn fresh(&self, now: i64) -> bool {
        now - self.fetched_at < self.kind.ttl_ms()
    }
    fn served(&self) -> Served {
        Served {
            bytes: self.bytes.clone(),
            content_type: self.content_type.clone(),
            kind: self.kind,
        }
    }
    fn from_cached(c: CachedImage) -> Self {
        Self {
            bytes: Arc::new(c.bytes),
            content_type: c.content_type,
            kind: c.kind,
            fetched_at: c.fetched_at,
            etag: c.etag,
            last_modified: c.last_modified,
        }
    }
}

#[derive(Default)]
struct Hot {
    map: HashMap<String, HotEntry>,
    order: VecDeque<String>,
    bytes: usize,
}

impl Hot {
    fn get(&mut self, url: &str) -> Option<HotEntry> {
        let entry = self.map.get(url)?.clone();
        if let Some(pos) = self.order.iter().position(|u| u == url) {
            self.order.remove(pos);
        }
        self.order.push_back(url.to_string());
        Some(entry)
    }
    fn put(&mut self, url: &str, entry: HotEntry) {
        if let Some(old) = self.map.remove(url) {
            self.bytes -= old.bytes.len();
            if let Some(pos) = self.order.iter().position(|u| u == url) {
                self.order.remove(pos);
            }
        }
        self.bytes += entry.bytes.len();
        self.map.insert(url.to_string(), entry);
        self.order.push_back(url.to_string());
        while self.bytes > HOT_CAP_BYTES {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(e) = self.map.remove(&oldest) {
                self.bytes -= e.bytes.len();
            }
        }
    }
    fn clear(&mut self) {
        *self = Hot::default();
    }
}

type Flight = Shared<BoxFuture<'static, Result<HotEntry, ImageError>>>;

pub struct ImageService {
    cache: Option<Arc<ImageCache>>,
    transport: Arc<dyn HttpTransport>,
    hot: Mutex<Hot>,
    inflight: Mutex<HashMap<String, Flight>>,
    revalidating: Mutex<HashSet<String>>,
    permits: Arc<Semaphore>,
    now_ms: Arc<dyn Fn() -> i64 + Send + Sync>,
}

fn system_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 허용한 https 이미지 URL 인지 — https, 기본 포트, 사용자 정보 없음, 허용 호스트.
pub fn is_allowed_image_url(url: &str) -> bool {
    let Ok(u) = Url::parse(url) else {
        return false;
    };
    if u.scheme() != "https" || u.port().is_some() {
        return false;
    }
    if !u.username().is_empty() || u.password().is_some() {
        return false;
    }
    let Some(host) = u.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    ALLOWED_HOST_SUFFIXES
        .iter()
        .any(|s| host.ends_with(s) || host == s.trim_start_matches('.'))
}

/// 리다이렉트 한 hop 의 판단.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RedirectDecision {
    Follow,
    /// 허용하지 않는 목적지(다른 호스트·http·다른 포트 등).
    Forbidden,
    /// 이미 지나온 URL 로 돌아간다.
    Loop,
    TooMany,
}

/// 순수 함수 — `previous` 는 지금까지 요청한 URL(처음 URL 포함).
fn redirect_decision(
    next: &Url,
    previous: &[Url],
    allowed: impl Fn(&Url) -> bool,
) -> RedirectDecision {
    if previous.len() > MAX_IMAGE_REDIRECTS {
        RedirectDecision::TooMany
    } else if !allowed(next) {
        RedirectDecision::Forbidden
    } else if previous.contains(next) {
        RedirectDecision::Loop
    } else {
        RedirectDecision::Follow
    }
}

fn redirect_policy_with(allowed: fn(&Url) -> bool) -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(move |attempt| {
        match redirect_decision(attempt.url(), attempt.previous(), allowed) {
            RedirectDecision::Follow => attempt.follow(),
            RedirectDecision::Forbidden => attempt.error("허용하지 않는 이미지 리다이렉트 목적지"),
            RedirectDecision::Loop => attempt.error("이미지 리다이렉트 순환"),
            RedirectDecision::TooMany => attempt.error("이미지 리다이렉트가 너무 많습니다"),
        }
    })
}

/// 이미지 연결의 리다이렉트 정책 — 처음 URL 과 같은 [`is_allowed_image_url`] 을 hop 마다 적용한다.
/// (reqwest 기본값은 목적지를 가리지 않고 10번까지 따라간다.)
pub fn redirect_policy() -> reqwest::redirect::Policy {
    redirect_policy_with(|u| is_allowed_image_url(u.as_str()))
}

/// `cvimg:` 요청 경로(`/<종류>%2F<원본 URL>` 또는 이미 풀린 `/<종류>/<원본 URL>`)를 나눈다.
pub fn parse_request_path(path: &str) -> Option<(ImageKind, String)> {
    let decoded = percent_encoding::percent_decode_str(path.trim_start_matches('/'))
        .decode_utf8()
        .ok()?
        .into_owned();
    let (kind, url) = decoded.split_once('/')?;
    if url.is_empty() {
        return None;
    }
    Some((ImageKind::parse(kind), url.to_string()))
}

impl ImageService {
    pub fn new(cache: Option<Arc<ImageCache>>, transport: Arc<dyn HttpTransport>) -> Arc<Self> {
        Self::with_clock(cache, transport, Arc::new(system_now_ms))
    }

    pub fn with_clock(
        cache: Option<Arc<ImageCache>>,
        transport: Arc<dyn HttpTransport>,
        now_ms: Arc<dyn Fn() -> i64 + Send + Sync>,
    ) -> Arc<Self> {
        Arc::new(Self {
            cache,
            transport,
            hot: Mutex::new(Hot::default()),
            inflight: Mutex::new(HashMap::new()),
            revalidating: Mutex::new(HashSet::new()),
            permits: Arc::new(Semaphore::new(MAX_CONCURRENT_FETCHES)),
            now_ms,
        })
    }

    fn hot(&self) -> std::sync::MutexGuard<'_, Hot> {
        self.hot.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 이미지 한 장. 캐시에 있으면 오래됐어도 바로 내주고(오래됐으면 뒤에서 새로 받는다), 없으면 받는다.
    pub async fn load(self: &Arc<Self>, url: &str, kind: ImageKind) -> Result<Served, ImageError> {
        if !is_allowed_image_url(url) {
            return Err(ImageError::Forbidden);
        }
        let now = (self.now_ms)();
        let hot = self.hot().get(url);
        if let Some(entry) = hot {
            if !entry.fresh(now) {
                self.revalidate(url, kind, &entry);
            }
            return Ok(entry.served());
        }
        if let Some(cache) = &self.cache {
            let cached = {
                let cache = cache.clone();
                let url = url.to_string();
                tokio::task::spawn_blocking(move || cache.get(&url, now))
                    .await
                    .ok()
                    .and_then(|r| r.ok())
                    .flatten()
            };
            if let Some(c) = cached {
                let entry = HotEntry::from_cached(c);
                self.hot().put(url, entry.clone());
                if !entry.fresh(now) {
                    self.revalidate(url, kind, &entry);
                }
                return Ok(entry.served());
            }
        }
        self.fetch_shared(url, kind, None).await.map(|e| e.served())
    }

    /// 오래된 항목을 뒤에서 새로 받는다(같은 URL 은 한 번만). 실패하면 가진 것을 그대로 둔다.
    fn revalidate(self: &Arc<Self>, url: &str, kind: ImageKind, prev: &HotEntry) {
        {
            let mut set = self.revalidating.lock().unwrap_or_else(|e| e.into_inner());
            if !set.insert(url.to_string()) {
                return;
            }
        }
        let this = self.clone();
        let url = url.to_string();
        let prev = prev.clone();
        tokio::spawn(async move {
            let _ = this.fetch_shared(&url, kind, Some(prev)).await;
            this.revalidating
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&url);
        });
    }

    /// 같은 URL 의 받기를 하나로 묶는다.
    async fn fetch_shared(
        self: &Arc<Self>,
        url: &str,
        kind: ImageKind,
        prev: Option<HotEntry>,
    ) -> Result<HotEntry, ImageError> {
        let flight = {
            let mut map = self.inflight.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(f) = map.get(url) {
                f.clone()
            } else {
                let this = self.clone();
                let key = url.to_string();
                let f = async move {
                    let r = this.fetch(&key, kind, prev).await;
                    this.inflight
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(&key);
                    r
                }
                .boxed()
                .shared();
                map.insert(url.to_string(), f.clone());
                f
            }
        };
        flight.await
    }

    async fn fetch(
        self: &Arc<Self>,
        url: &str,
        kind: ImageKind,
        prev: Option<HotEntry>,
    ) -> Result<HotEntry, ImageError> {
        let _permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| ImageError::Unavailable("closed".into()))?;
        let parsed = Url::parse(url).map_err(|_| ImageError::Forbidden)?;
        let mut req = HttpRequest::new(cview_core::protocols::HttpMethod::Get, parsed);
        req.timeout = Some(FETCH_TIMEOUT);
        req.set_header(
            "Accept",
            Some("image/avif,image/webp,image/png,image/*;q=0.8"),
        );
        req.set_header("Referer", Some("https://chzzk.naver.com/"));
        if let Some(p) = &prev {
            req.set_header("If-None-Match", p.etag.as_deref());
            req.set_header("If-Modified-Since", p.last_modified.as_deref());
        }
        let response = self
            .transport
            .send(req)
            .await
            .map_err(|e| ImageError::Unavailable(e.to_string()))?;
        let now = (self.now_ms)();
        if response.status == 304 {
            if let Some(mut p) = prev {
                p.fetched_at = now;
                self.hot().put(url, p.clone());
                if let Some(cache) = &self.cache {
                    let cache = cache.clone();
                    let url = url.to_string();
                    let _ = tokio::task::spawn_blocking(move || cache.mark_fresh(&url, now)).await;
                }
                return Ok(p);
            }
        }
        if !(200..=299).contains(&response.status) {
            return Err(ImageError::Unavailable(format!("http {}", response.status)));
        }
        let content_type = response
            .header("content-type")
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if !content_type.starts_with("image/") || response.body.is_empty() {
            return Err(ImageError::Unavailable(format!(
                "not an image: {content_type}"
            )));
        }
        let etag = response.header("etag").map(str::to_string);
        let last_modified = response.header("last-modified").map(str::to_string);
        let entry = HotEntry {
            bytes: Arc::new(response.body),
            content_type: content_type.clone(),
            kind,
            fetched_at: now,
            etag,
            last_modified,
        };
        self.hot().put(url, entry.clone());
        if let Some(cache) = &self.cache {
            let cache = cache.clone();
            let url = url.to_string();
            let meta = ImageMeta {
                content_type,
                etag: entry.etag.clone(),
                last_modified: entry.last_modified.clone(),
            };
            let bytes = entry.bytes.clone();
            // 화면은 기다리지 않는다 — 디스크·색인 쓰기는 응답 뒤에(메모리에는 이미 있다).
            tokio::task::spawn_blocking(move || {
                if let Err(e) = cache.put(&url, kind, &meta, &bytes, now) {
                    tracing::debug!("이미지 캐시 저장 실패: {e}");
                }
            });
        }
        Ok(entry)
    }

    pub fn stats(&self) -> CacheStats {
        self.cache
            .as_ref()
            .and_then(|c| c.stats().ok())
            .unwrap_or(CacheStats {
                entries: 0,
                bytes: 0,
                cap_bytes: 0,
            })
    }

    pub fn clear(&self) {
        self.hot().clear();
        if let Some(cache) = &self.cache {
            if let Err(e) = cache.clear() {
                tracing::warn!("이미지 캐시 비우기 실패: {e}");
            }
        }
    }
}

/// 앱 데이터 폴더 아래 `image-cache/` 로 서비스를 연다. 캐시를 못 열면 저장 없이 받기만 한다.
pub fn open_service(data_dir: &std::path::Path) -> Option<Arc<ImageService>> {
    // 이미지 전용 연결 — API 전송(호스트당 6)과 따로, 16개까지 살려 둔다. 쿠키 저장소 없음.
    let client = reqwest::Client::builder()
        .user_agent(cview_networking::headers::SAFARI_USER_AGENT)
        .connect_timeout(Duration::from_secs(8))
        .timeout(FETCH_TIMEOUT)
        .pool_max_idle_per_host(MAX_CONCURRENT_FETCHES)
        .tcp_nodelay(true)
        .redirect(redirect_policy())
        .build();
    let transport = match client {
        Ok(c) => {
            Arc::new(cview_networking::ReqwestTransport::with_client(c)) as Arc<dyn HttpTransport>
        }
        Err(e) => {
            tracing::warn!("이미지 전송을 만들지 못했습니다: {e}");
            return None;
        }
    };
    let cache = match ImageCache::open(
        &data_dir.join("image-cache"),
        cview_persistence::image_cache::DEFAULT_CAP_BYTES,
    ) {
        Ok(c) => Some(Arc::new(c)),
        Err(e) => {
            tracing::warn!("이미지 캐시를 열지 못해 저장 없이 받습니다: {e}");
            None
        }
    };
    Some(ImageService::new(cache, transport))
}

/// `cvimg:` 요청 하나에 답한다.
pub async fn respond(
    service: Option<Arc<ImageService>>,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    let reply = |status: u16, ct: &str, cache: &str, body: Vec<u8>| {
        tauri::http::Response::builder()
            .status(status)
            .header("Content-Type", ct)
            .header("Cache-Control", cache)
            .header("Access-Control-Allow-Origin", "*")
            .body(body)
            .unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
    };
    let Some(service) = service else {
        return reply(
            503,
            "text/plain",
            "no-store",
            b"image service unavailable".to_vec(),
        );
    };
    let Some((kind, url)) = parse_request_path(request.uri().path()) else {
        return reply(400, "text/plain", "no-store", b"bad image request".to_vec());
    };
    match service.load(&url, kind).await {
        // 라이브 썸네일은 같은 주소의 그림이 바뀐다 — 웹뷰가 붙들지 않게 매번 묻게 하고(우리 메모리 캐시가 답한다),
        // 아바타·포스터는 하루 동안 웹뷰도 들고 있게 한다.
        Ok(img) => reply(
            200,
            &img.content_type,
            if img.kind == ImageKind::Live {
                "no-cache"
            } else {
                "max-age=86400"
            },
            img.bytes.as_ref().clone(),
        ),
        Err(ImageError::Forbidden) => reply(403, "text/plain", "no-store", Vec::new()),
        Err(ImageError::Unavailable(_)) => reply(502, "text/plain", "no-store", Vec::new()),
    }
}

#[cfg(test)]
mod tests {
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
            *self.last_if_none_match.lock().unwrap() =
                req.header("If-None-Match").map(str::to_string);
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
            parse_request_path("/live%2Fhttps%3A%2F%2Fa.pstatic.net%2Fx.jpg%3Ftype%3Df120")
                .unwrap();
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
}
