//! `Sources/CViewNetworking/ChzzkAPIClient.swift` · `ChzzkAPIClient+NetworkPolicy.swift` 포팅 —
//! 치지직 API 클라이언트의 공용 요청 경로.
//!
//! - 캐시: GET 만 캐시한다. fresh / stale(stale-while-revalidate) / miss 3단계.
//! - single-flight: 같은 키의 GET 은 공유 작업 하나만 네트워크를 탄다. 기다리는 호출자가 모두
//!   떠나면 공유 작업을 멈춘다.
//! - stale 재검증 게이트: 동시 2개, 대기열 16개, 넘치면 버린다.
//! - 재시도: timeout/5xx 는 백오프 재시도, soft-auth 401 은 쿠키 없이 1회, 429 는 별도 예산.
//! - 로그인 세대(`AuthContext`)가 바뀌면 진행 중 결과를 버린다([`ApiError::Cancelled`]).
//!
//! Swift actor 는 `Arc<Inner>` + `std::sync::Mutex` 로 대체했다(락은 await 를 넘지 않는다).
//! Swift 의 요청 장부(APIRequestTrace/RequestLedger)·픽스처·장애 주입은 포팅하지 않았다.
//!
//! 요청 파이프라인은 `request`, 같은 GET 합치기와 stale 재검증 게이트는 `shared_get` 하위 모듈에 있다.

mod request;
mod shared_get;

use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cview_core::error::ApiError;
use cview_core::models::settings::NetworkSettings;
use cview_core::protocols::{
    ApiClientProtocol, AuthTokenProvider, CachePolicy, EndpointProtocol, HttpCookie, HttpMethod,
    QueryItem,
};
use rand::Rng;
use serde::de::DeserializeOwned;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use url::Url;

use crate::cache::ResponseCache;
use crate::constants::api_defaults;
use crate::response::UserStatusInfo;
use crate::transport::{HttpRequest, HttpResponse, HttpTransport, TransportError};
use crate::wait::GetWait;

/// 응답 본문 타입이 만족해야 하는 조건.
pub trait ResponseBody: DeserializeOwned + Clone + Send + Sync + 'static {}
impl<T: DeserializeOwned + Clone + Send + Sync + 'static> ResponseBody for T {}

/// 응답과 그 값을 받은 시각.
///
/// stale 값(SWR·오류 때 이전 값)도 보통 성공과 똑같이 돌아오므로, 나이가 필요한 호출자는 이 형태로 받는다.
#[derive(Debug, Clone, PartialEq)]
pub struct AgedResponse<V> {
    pub value: V,
    /// 서버에서 받은 시각. 캐시에서 온 값이면 그 항목을 저장한 시각이다.
    pub fetched_at: DateTime<Utc>,
    /// fresh 기한을 지난 이전 값이면 참.
    pub is_stale: bool,
}

/// 요청이 왜 나가는가.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RequestPurpose {
    /// 호출자가 부른 요청.
    Normal,
    /// stale 적중 뒤 뒤에서 다시 받는 요청(읽기 우회).
    Revalidate,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RequestOptions {
    pub bypass_cache: bool,
    pub store_in_cache: bool,
    pub joined: bool,
    pub purpose: RequestPurpose,
}

impl RequestOptions {
    pub(crate) fn new(bypass_cache: bool, store_in_cache: bool) -> Self {
        Self {
            bypass_cache,
            store_in_cache,
            joined: false,
            purpose: RequestPurpose::Normal,
        }
    }
}

/// 엔드포인트의 소유 스냅샷 — 공유 작업(spawn)에 넘기려고 `&dyn EndpointProtocol` 에서 값을 복사한다.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EndpointSnapshot {
    path: String,
    method: HttpMethod,
    query_items: Option<Vec<QueryItem>>,
    body: Option<Vec<u8>>,
    requires_auth: bool,
    cache_policy: CachePolicy,
    stale_ttl: Option<f64>,
    diagnostic_name: &'static str,
}

impl EndpointSnapshot {
    pub(crate) fn of(e: &dyn EndpointProtocol) -> Arc<Self> {
        Arc::new(Self {
            path: e.path(),
            method: e.method(),
            query_items: e.query_items(),
            body: e.body(),
            requires_auth: e.requires_auth(),
            cache_policy: e.cache_policy(),
            stale_ttl: e.stale_ttl(),
            diagnostic_name: e.diagnostic_name(),
        })
    }
}

impl EndpointProtocol for EndpointSnapshot {
    fn path(&self) -> String {
        self.path.clone()
    }
    fn method(&self) -> HttpMethod {
        self.method
    }
    fn query_items(&self) -> Option<Vec<QueryItem>> {
        self.query_items.clone()
    }
    fn body(&self) -> Option<Vec<u8>> {
        self.body.clone()
    }
    fn requires_auth(&self) -> bool {
        self.requires_auth
    }
    fn cache_policy(&self) -> CachePolicy {
        self.cache_policy
    }
    fn stale_ttl(&self) -> Option<f64> {
        self.stale_ttl
    }
    fn diagnostic_name(&self) -> &'static str {
        self.diagnostic_name
    }
}

/// 로그인 세대. 로컬 무효화 카운터 + 인증 제공자 세대.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct AuthContext {
    pub local: u64,
    pub provider: u64,
}

/// stale 재검증 게이트 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaleRevalidationSnapshot {
    pub running: usize,
    pub queued: usize,
    pub started: usize,
    pub dropped: usize,
}

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;
type RevalidationRun = Box<dyn FnOnce(ChzzkApiClient) -> BoxFuture<()> + Send>;

struct StaleRevalidation {
    key: String,
    run: RevalidationRun,
}

struct GetWaitEntry {
    wait: GetWait,
    task: JoinHandle<()>,
}

pub(crate) struct UserStatusFlight {
    pub generation: u64,
    pub wait: GetWait,
    pub task: Option<JoinHandle<()>>,
}

pub(crate) struct ClientState {
    /// Swift `cacheGeneration: UUID` — 무효화마다 증가.
    pub cache_generation: u64,
    pub max_retries: i64,
    pub network_policy_generation: u64,
    /// 초 단위.
    pub applied_api_timeout: i64,
    /// 404(영구 미지원)로 확인된 이모티콘 endpoint family. 세션 동안만 유지.
    pub unsupported_emoticon_families: HashSet<String>,
    pub unsupported_emoticon_context: Option<AuthContext>,
    get_waits: HashMap<String, GetWaitEntry>,
    running_leaders: usize,
    revalidation_keys: HashSet<String>,
    revalidation_queue: VecDeque<StaleRevalidation>,
    revalidations_running: usize,
    revalidations_started: usize,
    revalidations_dropped: usize,
    /// 로그인 세대가 같으면 userStatus 를 다시 받지 않는다.
    pub user_status_memo: Option<(u64, UserStatusInfo)>,
    pub user_status_flight: Option<UserStatusFlight>,
    pub user_status_loads_running: usize,
}

pub(crate) struct Inner {
    pub base_url: Url,
    pub game_api_base_url: Url,
    pub comment_api_base_url: Url,
    pub auth: Option<Arc<dyn AuthTokenProvider>>,
    pub cache: Arc<ResponseCache>,
    pub transport: Arc<dyn HttpTransport>,
    pub gate: crate::gate::RequestGate,
    pub state: Mutex<ClientState>,
    session_expired: broadcast::Sender<()>,
    purge_task: Option<JoinHandle<()>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(t) = self.purge_task.take() {
            t.abort();
        }
    }
}

/// 클라이언트 구성.
pub struct ClientConfig {
    pub auth_provider: Option<Arc<dyn AuthTokenProvider>>,
    pub cache: Arc<ResponseCache>,
    pub base_url: Url,
    /// Game API 기본 URL (채팅 토큰·userStatus).
    pub game_api_base_url: Url,
    /// 댓글 API 기본 URL (채널 커뮤니티 글).
    pub comment_api_base_url: Url,
    pub transport: Arc<dyn HttpTransport>,
}

impl ClientConfig {
    pub fn new(transport: Arc<dyn HttpTransport>) -> Self {
        Self {
            auth_provider: None,
            cache: Arc::new(ResponseCache::default()),
            base_url: Url::parse(DEFAULT_BASE_URL).expect("valid base url"),
            game_api_base_url: Url::parse(GAME_API_BASE_URL).expect("valid game url"),
            comment_api_base_url: Url::parse(COMMENT_API_BASE_URL).expect("valid comment url"),
            transport,
        }
    }
}

/// 치지직 API 기본 URL.
pub const DEFAULT_BASE_URL: &str = "https://api.chzzk.naver.com";
/// Game API 기본 URL (chat access token 등).
pub const GAME_API_BASE_URL: &str = "https://comm-api.game.naver.com/nng_main";
/// 댓글 API 기본 URL (채널 커뮤니티 글 — 치지직 웹과 같은 주소).
pub const COMMENT_API_BASE_URL: &str = "https://apis.naver.com/nng_main/nng_comment_api";

/// stale 재검증 동시 수.
pub const STALE_REVALIDATION_CONCURRENCY: usize = 2;
/// stale 재검증 대기열 상한. 넘치면 버린다 — 이전 값은 남아 있고 다음 조회가 다시 예약한다.
pub const STALE_REVALIDATION_QUEUE_LIMIT: usize = 16;

/// 치지직 API 클라이언트. 복제하면 같은 클라이언트(캐시·대기·게이트)를 공유한다.
#[derive(Clone)]
pub struct ChzzkApiClient {
    pub(crate) inner: Arc<Inner>,
}

impl std::fmt::Debug for ChzzkApiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChzzkApiClient")
            .field("base_url", &self.inner.base_url.as_str())
            .finish()
    }
}

/// 백오프 지연: `min(n × 0.5, 3) + jitter(0…0.3 × base)` 초.
fn backoff(n: i64) -> Duration {
    let base = (n as f64 * 0.5).min(3.0);
    let jitter = rand::thread_rng().gen_range(0.0..=0.3) * base;
    Duration::from_secs_f64((base + jitter).max(0.0))
}

/// 쿼리 값 percent-encoding (RFC 3986 unreserved 만 그대로 둔다; 공백은 `%20`).
fn encode_query_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `base` 에 `path` 를 붙이고 쿼리를 단다 (Swift `baseURL.appending(path:)` + `URLComponents.queryItems`).
pub(crate) fn build_url(
    base: &Url,
    path: &str,
    query: Option<&[QueryItem]>,
) -> Result<Url, ApiError> {
    let mut url = base.clone();
    let base_path = url.path().trim_end_matches('/').to_string();
    let joined = if path.starts_with('/') {
        format!("{base_path}{path}")
    } else {
        format!("{base_path}/{path}")
    };
    url.set_path(&joined);
    match query {
        Some(items) if !items.is_empty() => {
            let q = items
                .iter()
                .map(|i| match &i.value {
                    Some(v) => format!(
                        "{}={}",
                        encode_query_component(&i.name),
                        encode_query_component(v)
                    ),
                    None => encode_query_component(&i.name),
                })
                .collect::<Vec<_>>()
                .join("&");
            url.set_query(Some(&q));
        }
        _ => url.set_query(None),
    }
    if url.cannot_be_a_base() {
        return Err(ApiError::NetworkError(format!("Invalid URL: {path}")));
    }
    Ok(url)
}

/// 공유 GET 한 건의 문맥.
#[derive(Clone)]
struct SharedGet {
    key: String,
    context: AuthContext,
    stale_window: f64,
    bypass_cache: bool,
    store_in_cache: bool,
    purpose: RequestPurpose,
}

/// 공유 작업이 끝나면(정상 종료든 abort 든) 기록을 정리한다.
struct LeaderFinish {
    client: ChzzkApiClient,
    key: String,
    wait: GetWait,
}

impl Drop for LeaderFinish {
    fn drop(&mut self) {
        // abort 로 끝났으면 결과가 없다 — 남은 대기자가 있다면 취소로 푼다.
        self.wait.fail(ApiError::Cancelled);
        self.client.finish_get_wait(&self.key, &self.wait);
    }
}

impl ChzzkApiClient {
    pub fn new(config: ClientConfig) -> Self {
        let (session_expired, _) = broadcast::channel(16);
        let cache = config.cache;
        // 5분 간격으로 만료된 응답 캐시 정리. 런타임 밖에서 만들면 정리 작업 없이 동작한다.
        let purge_task = tokio::runtime::Handle::try_current().ok().map(|handle| {
            let cache = cache.clone();
            handle.spawn(async move {
                let period = Duration::from_secs_f64(api_defaults::CACHE_PURGE_INTERVAL);
                let mut interval =
                    tokio::time::interval_at(tokio::time::Instant::now() + period, period);
                loop {
                    interval.tick().await;
                    cache.purge_expired(None);
                }
            })
        });
        Self {
            inner: Arc::new(Inner {
                base_url: config.base_url,
                game_api_base_url: config.game_api_base_url,
                comment_api_base_url: config.comment_api_base_url,
                auth: config.auth_provider,
                cache,
                transport: config.transport,
                gate: crate::gate::RequestGate::new(),
                state: Mutex::new(ClientState {
                    cache_generation: 0,
                    max_retries: 3,
                    network_policy_generation: 0,
                    applied_api_timeout: NetworkSettings::default().effective_api_timeout(),
                    unsupported_emoticon_families: HashSet::new(),
                    unsupported_emoticon_context: None,
                    get_waits: HashMap::new(),
                    running_leaders: 0,
                    revalidation_keys: HashSet::new(),
                    revalidation_queue: VecDeque::new(),
                    revalidations_running: 0,
                    revalidations_started: 0,
                    revalidations_dropped: 0,
                    user_status_memo: None,
                    user_status_flight: None,
                    user_status_loads_running: 0,
                }),
                session_expired,
                purge_task,
            }),
        }
    }

    /// 기본 구성(reqwest 전송)으로 만든다.
    pub fn with_default_transport() -> Result<Self, reqwest::Error> {
        Ok(Self::new(ClientConfig::new(Arc::new(
            crate::transport::ReqwestTransport::new()?,
        ))))
    }

    /// 서버가 401 로 응답해 로그인 세션이 만료됐을 때 알림을 받는다 (Swift `.chzzkSessionExpired`).
    pub fn subscribe_session_expired(&self) -> broadcast::Receiver<()> {
        self.inner.session_expired.subscribe()
    }

    pub fn cache(&self) -> &Arc<ResponseCache> {
        &self.inner.cache
    }

    pub fn request_gate(&self) -> &crate::gate::RequestGate {
        &self.inner.gate
    }

    pub(crate) fn lock_state(&self) -> MutexGuard<'_, ClientState> {
        self.inner.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) async fn provider_generation(&self) -> u64 {
        match &self.inner.auth {
            Some(a) => a.authentication_generation().await,
            None => 0,
        }
    }

    pub(crate) async fn auth_context(&self) -> AuthContext {
        let local = self.lock_state().cache_generation;
        AuthContext {
            local,
            provider: self.provider_generation().await,
        }
    }

    pub(crate) async fn check_auth_context(&self, context: AuthContext) -> Result<(), ApiError> {
        let provider = self.provider_generation().await;
        let local = self.lock_state().cache_generation;
        if context.local == local && context.provider == provider {
            Ok(())
        } else {
            Err(ApiError::Cancelled)
        }
    }

    pub(crate) async fn auth_cookies(&self) -> Option<Vec<HttpCookie>> {
        match &self.inner.auth {
            Some(a) => a.cookies().await.filter(|c| !c.is_empty()),
            None => None,
        }
    }

    /// 로그인 세대를 올리고 계정에 묶인 캐시·메모·대기열을 비운다.
    pub async fn invalidate_authentication_cache(&self) {
        let flight_task = {
            let mut st = self.lock_state();
            st.cache_generation += 1;
            st.unsupported_emoticon_families.clear();
            st.user_status_memo = None;
            let task = st.user_status_flight.take().and_then(|f| {
                f.wait.fail(ApiError::Cancelled);
                f.task
            });
            let queued: Vec<String> = st.revalidation_queue.drain(..).map(|j| j.key).collect();
            for key in queued {
                st.revalidation_keys.remove(&key);
            }
            task
        };
        if let Some(t) = flight_task {
            t.abort();
        }
        self.inner.cache.clear();
    }

    /// 재시도 횟수 (1–10).
    pub fn update_retry_count(&self, count: i64) {
        self.lock_state().max_retries = count.clamp(1, 10);
    }

    /// 네트워크 설정 반영 (Swift `+NetworkPolicy`). 신규 요청부터 반영한다.
    pub fn update_network_settings(&self, settings: &NetworkSettings) {
        let generation = {
            let mut st = self.lock_state();
            st.network_policy_generation = st.network_policy_generation.wrapping_add(1);
            st.max_retries = settings.effective_api_attempts().clamp(1, 10);
            st.applied_api_timeout = settings.effective_api_timeout();
            st.network_policy_generation
        };
        self.inner.gate.configure(
            settings.effective_api_rate_limit(),
            settings.effective_api_concurrency(),
            Some(generation),
        );
    }

    /// 요청 하나를 보낸다 — 게이트 슬롯을 얻고, 로그인 세대를 다시 확인하고, 타임아웃 상한을 적용한다.
    pub(crate) async fn send(
        &self,
        mut request: HttpRequest,
        context: Option<AuthContext>,
    ) -> Result<HttpResponse, TransportError> {
        let context = match context {
            Some(c) => c,
            None => self.auth_context().await,
        };
        let _permit = self
            .inner
            .gate
            .acquire()
            .await
            .map_err(|_| TransportError::Cancelled)?;
        self.check_auth_context(context)
            .await
            .map_err(|_| TransportError::Cancelled)?;
        let applied = Duration::from_secs(self.lock_state().applied_api_timeout.max(1) as u64);
        request.timeout = Some(
            request
                .timeout
                .unwrap_or(Duration::from_secs(60))
                .min(applied),
        );
        if !crate::observer::is_active() {
            return self.inner.transport.send(request).await;
        }
        // 디버그 로그 — 시도마다(재시도 포함) 경로·상태·시간·크기.
        let method = request.method.as_str();
        let path = crate::observer::redacted_path(&request.url);
        let started = std::time::Instant::now();
        let result = self.inner.transport.send(request).await;
        let (status, error, bytes) = match &result {
            Ok(r) => (Some(r.status), None, r.body.len()),
            Err(e) => (None, Some(e.kind()), 0),
        };
        crate::observer::observe(&crate::observer::RequestRecord {
            method,
            path,
            status,
            error,
            duration_ms: started.elapsed().as_secs_f64() * 1000.0,
            bytes,
            cache_hit: false,
        });
        result
    }
}

impl ChzzkApiClient {
    // MARK: - 메모리 압력 · 계측

    /// 경고면 응답 캐시를 오래 안 쓴 것부터 절반으로 줄이고, critical 이면 전부 비운다.
    pub fn handle_memory_pressure(&self, critical: bool) {
        if critical {
            self.inner.cache.clear();
        } else {
            self.inner.cache.respond_to_memory_pressure();
        }
    }

    pub fn cache_stats(&self) -> crate::cache::CacheStats {
        self.inner.cache.stats()
    }

    // MARK: - 테스트·진단 훅

    /// 진행 중인 공유 GET 에 매달린 호출자 수.
    pub fn get_wait_waiter_count(&self) -> usize {
        self.lock_state()
            .get_waits
            .values()
            .map(|e| e.wait.waiter_count())
            .sum()
    }

    /// 진행 중인 공유 GET 키 수.
    pub fn get_wait_count(&self) -> usize {
        self.lock_state().get_waits.len()
    }

    /// 아직 끝나지 않은 공유 작업 수.
    pub fn running_leader_count(&self) -> usize {
        self.lock_state().running_leaders
    }

    pub fn stale_revalidation_snapshot(&self) -> StaleRevalidationSnapshot {
        let st = self.lock_state();
        StaleRevalidationSnapshot {
            running: st.revalidations_running,
            queued: st.revalidation_queue.len(),
            started: st.revalidations_started,
            dropped: st.revalidations_dropped,
        }
    }

    /// 요청 경로가 쓰는 캐시 키(지금 인증 세대 기준).
    pub async fn response_cache_key_for<T>(&self, endpoint: &dyn EndpointProtocol) -> String {
        Self::response_cache_key::<T>(endpoint, self.auth_context().await)
    }
}

pub(crate) fn transport_to_api_error(e: TransportError) -> ApiError {
    match e {
        TransportError::Cancelled => ApiError::Cancelled,
        TransportError::TimedOut(_) => ApiError::NetworkError("Request timed out".into()),
        TransportError::ConnectionLost(m) | TransportError::Other(m) => ApiError::NetworkError(m),
        TransportError::NotConnected => ApiError::NetworkError("No internet connection".into()),
    }
}

#[async_trait]
impl ApiClientProtocol for ChzzkApiClient {
    async fn request<T>(&self, endpoint: &dyn EndpointProtocol) -> Result<T, ApiError>
    where
        T: DeserializeOwned + Clone + Send + Sync + 'static,
    {
        ChzzkApiClient::request::<T>(self, endpoint).await
    }
}
