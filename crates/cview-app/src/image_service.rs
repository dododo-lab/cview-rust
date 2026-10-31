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

/// 네이버 이미지 서버의 크기 단계 쿼리(`type=`)를 뺀 원본 주소. 없으면 `None`.
pub fn without_size_param(url: &str) -> Option<String> {
    let mut u = Url::parse(url).ok()?;
    if !u.query_pairs().any(|(k, _)| k == "type") {
        return None;
    }
    let rest: Vec<(String, String)> = u
        .query_pairs()
        .filter(|(k, _)| k != "type")
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if rest.is_empty() {
        u.set_query(None);
    } else {
        u.query_pairs_mut().clear().extend_pairs(rest);
    }
    Some(u.to_string())
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
        let mut response = self.send(url, prev.as_ref()).await?;
        // 화면이 붙인 크기 단계(`?type=w640` 등)를 그 이미지 서버가 모르면 404 다 — 원본 주소로 한 번 더 받는다.
        // (캐시 키는 화면이 부른 주소 그대로라 다음부터는 바로 나온다.)
        if response.status == 404 && prev.is_none() {
            if let Some(original) = without_size_param(url) {
                response = self.send(&original, None).await?;
            }
        }
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

    async fn send(
        &self,
        url: &str,
        prev: Option<&HotEntry>,
    ) -> Result<cview_networking::HttpResponse, ImageError> {
        let parsed = Url::parse(url).map_err(|_| ImageError::Forbidden)?;
        let mut req = HttpRequest::new(cview_core::protocols::HttpMethod::Get, parsed);
        req.timeout = Some(FETCH_TIMEOUT);
        req.set_header(
            "Accept",
            Some("image/avif,image/webp,image/png,image/*;q=0.8"),
        );
        req.set_header("Referer", Some("https://chzzk.naver.com/"));
        if let Some(p) = prev {
            req.set_header("If-None-Match", p.etag.as_deref());
            req.set_header("If-Modified-Since", p.last_modified.as_deref());
        }
        self.transport
            .send(req)
            .await
            .map_err(|e| ImageError::Unavailable(e.to_string()))
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
mod tests;
