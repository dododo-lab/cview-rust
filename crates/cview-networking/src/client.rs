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

use std::any::type_name;
use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
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

use crate::cache::{Lookup, ResponseCache};
use crate::constants::api_defaults;
use crate::headers;
use crate::response::{ChzzkResponse, UserStatusInfo};
use crate::transport::{HttpRequest, HttpResponse, HttpTransport, TransportError};
use crate::validator;
use crate::wait::{GetWait, WaitTicket};

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
    pub transport: Arc<dyn HttpTransport>,
}

impl ClientConfig {
    pub fn new(transport: Arc<dyn HttpTransport>) -> Self {
        Self {
            auth_provider: None,
            cache: Arc::new(ResponseCache::default()),
            base_url: Url::parse(DEFAULT_BASE_URL).expect("valid base url"),
            game_api_base_url: Url::parse(GAME_API_BASE_URL).expect("valid game url"),
            transport,
        }
    }
}

/// 치지직 API 기본 URL.
pub const DEFAULT_BASE_URL: &str = "https://api.chzzk.naver.com";
/// Game API 기본 URL (chat access token 등).
pub const GAME_API_BASE_URL: &str = "https://comm-api.game.naver.com/nng_main";

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
        self.inner.transport.send(request).await
    }

    // MARK: - 공개 요청 API

    /// 표준 응답(`ChzzkResponse<T>`)의 `content` 를 돌려준다.
    pub async fn request<T: ResponseBody>(
        &self,
        endpoint: &dyn EndpointProtocol,
    ) -> Result<T, ApiError> {
        self.request_bypass(endpoint, false).await
    }

    /// `bypass_cache`: 캐시 **읽기**만 건너뛴다(쓰기는 유지).
    pub async fn request_bypass<T: ResponseBody>(
        &self,
        endpoint: &dyn EndpointProtocol,
        bypass_cache: bool,
    ) -> Result<T, ApiError> {
        self.request_with_age(endpoint, bypass_cache, true)
            .await
            .map(|a| a.value)
    }

    /// `request` 와 같고, 값을 받은 시각과 stale 여부를 함께 돌려준다.
    pub async fn request_with_age<T: ResponseBody>(
        &self,
        endpoint: &dyn EndpointProtocol,
        bypass_cache: bool,
        store_in_cache: bool,
    ) -> Result<AgedResponse<T>, ApiError> {
        self.request_inner(
            EndpointSnapshot::of(endpoint),
            RequestOptions::new(bypass_cache, store_in_cache),
        )
        .await
    }

    pub(crate) async fn request_opts<T: ResponseBody>(
        &self,
        endpoint: &dyn EndpointProtocol,
        options: RequestOptions,
    ) -> Result<T, ApiError> {
        self.request_inner(EndpointSnapshot::of(endpoint), options)
            .await
            .map(|a| a.value)
    }

    /// 캐시 키용 쿼리 문자열 — 이름 순 정렬, `name=value` 를 `&` 로 잇는다.
    pub fn query_string(endpoint: &dyn EndpointProtocol) -> String {
        let mut items = endpoint.query_items().unwrap_or_default();
        items.sort_by(|a, b| a.name.cmp(&b.name));
        items
            .iter()
            .map(|i| format!("{}={}", i.name, i.value.as_deref().unwrap_or("")))
            .collect::<Vec<_>>()
            .join("&")
    }

    /// 캐시 키 — 로그인 세대·메서드·응답 타입·경로·쿼리. 타입을 넣어 같은 path 를 다른 T 로 부를 때의 오염을 막는다.
    pub(crate) fn response_cache_key<T>(
        endpoint: &dyn EndpointProtocol,
        context: AuthContext,
    ) -> String {
        format!(
            "{}:{}:{}:{}:{}?{}",
            context.local,
            context.provider,
            endpoint.method().as_str(),
            type_name::<T>(),
            endpoint.path(),
            Self::query_string(endpoint)
        )
    }

    /// stale 기한. 읽기(lookup)와 쓰기(set stale_for)가 같은 값을 쓴다.
    pub fn stale_window(endpoint: &dyn EndpointProtocol, ttl: f64) -> f64 {
        endpoint.stale_ttl().unwrap_or(ttl).max(ttl)
    }

    /// 오류 때 이전 값으로 대신해도 되는 오류인가 — 일시적인 전송·서버 쪽 오류만.
    pub fn allows_stale_on_error(error: &ApiError) -> bool {
        match error {
            ApiError::NetworkError(message) => message != "API fixture miss",
            ApiError::RateLimited { .. } => true,
            ApiError::HttpError { status_code } => (500..=599).contains(status_code),
            _ => false,
        }
    }

    /// 404 negative cache 와 폴백이 있는 선택적 엔드포인트(이모티콘)인가.
    pub(crate) fn is_expected_unsupported_response(
        error: &ApiError,
        endpoint: &dyn EndpointProtocol,
    ) -> bool {
        matches!(error, ApiError::HttpError { status_code: 404 })
            && matches!(
                endpoint.diagnostic_name(),
                "emoticonDeploy" | "userEmoticons" | "basicEmoticonPacks"
            )
    }

    fn boxed_request<T: ResponseBody>(
        client: ChzzkApiClient,
        endpoint: Arc<EndpointSnapshot>,
        options: RequestOptions,
    ) -> BoxFuture<Result<AgedResponse<T>, ApiError>> {
        Box::pin(async move { client.request_inner::<T>(endpoint, options).await })
    }

    /// 요청 경로 본체 (Swift `requestWithAge`).
    pub(crate) async fn request_inner<T: ResponseBody>(
        &self,
        endpoint: Arc<EndpointSnapshot>,
        options: RequestOptions,
    ) -> Result<AgedResponse<T>, ApiError> {
        let context = self.auth_context().await;
        self.check_auth_context(context).await?;
        // 상태를 바꾸는 메서드(POST/DELETE)는 캐시 읽기/쓰기에서 제외한다 — follow/unfollow 는 path 가 같다.
        let is_cacheable_method = endpoint.method() == HttpMethod::Get;
        let cache_key = Self::response_cache_key::<T>(endpoint.as_ref(), context);
        let cache_ttl = match endpoint.cache_policy() {
            CachePolicy::ReturnCacheElseLoad { ttl } if is_cacheable_method => Some(ttl),
            _ => None,
        };

        if let Some(ttl) = cache_ttl {
            let stale_window = Self::stale_window(endpoint.as_ref(), ttl);
            if !options.bypass_cache {
                match self
                    .inner
                    .cache
                    .lookup::<T>(&cache_key, ttl, stale_window, None)
                {
                    Lookup::Fresh { value, fetched_at } => {
                        self.check_auth_context(context).await?;
                        return Ok(AgedResponse {
                            value,
                            fetched_at,
                            is_stale: false,
                        });
                    }
                    Lookup::Stale { value, fetched_at } => {
                        self.check_auth_context(context).await?;
                        let ep = endpoint.clone();
                        self.schedule_stale_revalidation(
                            cache_key.clone(),
                            Box::new(move |client: ChzzkApiClient| {
                                Box::pin(async move {
                                    let options = RequestOptions {
                                        bypass_cache: true,
                                        store_in_cache: true,
                                        joined: false,
                                        purpose: RequestPurpose::Revalidate,
                                    };
                                    let _ = Self::boxed_request::<T>(client, ep, options).await;
                                }) as BoxFuture<()>
                            }),
                        );
                        return Ok(AgedResponse {
                            value,
                            fetched_at,
                            is_stale: true,
                        });
                    }
                    Lookup::Miss => {}
                }
            }
            if !options.joined {
                let shared = SharedGet {
                    key: cache_key,
                    context,
                    stale_window,
                    bypass_cache: options.bypass_cache,
                    store_in_cache: options.store_in_cache,
                    purpose: options.purpose,
                };
                return self.shared_get::<T>(endpoint, shared).await;
            }
        }

        let url = build_url(
            &self.inner.base_url,
            &endpoint.path(),
            endpoint.query_items().as_deref(),
        )?;
        let mut request = HttpRequest::new(endpoint.method(), url);
        request.body = endpoint.body();
        request.set_header("Referer", Some(headers::API_REFERER));
        request.set_header("Origin", Some(headers::CHZZK_ORIGIN));

        // 인증: 쿠키가 있으면 항상 첨부(soft auth). requiresAuth 인데 자격 증명이 없으면 실패.
        let mut used_soft_auth = false;
        if let Some(cookies) = self.auth_cookies().await {
            request.set_header("Cookie", HttpCookie::header_value(&cookies).as_deref());
            if !endpoint.requires_auth() {
                used_soft_auth = true;
            }
        } else if endpoint.requires_auth() {
            let token = match &self.inner.auth {
                Some(a) => a.access_token().await,
                None => None,
            };
            match token {
                Some(token) => {
                    request.set_header("Authorization", Some(&format!("Bearer {token}")))
                }
                None => {
                    tracing::error!("Auth: no credentials for {}", endpoint.path());
                    return Err(ApiError::Unauthorized);
                }
            }
        }

        // 재시도 예산 분리: soft-auth 401 은 1회(예산 미소비), 429 는 전용 카운터, 나머지는 attempt.
        let mut last_error: Option<ApiError> = None;
        let mut attempt: i64 = 0;
        let mut soft_auth_retried = false;
        let mut rate_limit_retries: i64 = 0;
        let attempt_limit = self.lock_state().max_retries;

        'retry: while attempt < attempt_limit {
            self.check_auth_context(context).await?;
            tracing::debug!(
                "Request: {} {} (attempt {})",
                endpoint.method(),
                endpoint.path(),
                attempt + 1
            );
            let error: ApiError = 'attempt: {
                let response = match self.send(request.clone(), Some(context)).await {
                    Ok(r) => r,
                    Err(TransportError::Cancelled) => return Err(ApiError::Cancelled),
                    Err(TransportError::TimedOut(m)) | Err(TransportError::ConnectionLost(m)) => {
                        tracing::warn!("Request transport failure: {} error={m}", endpoint.path());
                        last_error = Some(ApiError::NetworkError(m));
                        if attempt < attempt_limit - 1 {
                            tokio::time::sleep(backoff(attempt + 1)).await;
                            attempt += 1;
                            continue 'retry;
                        }
                        break 'attempt ApiError::NetworkError("Request timed out".into());
                    }
                    Err(TransportError::NotConnected) => {
                        break 'attempt ApiError::NetworkError("No internet connection".into())
                    }
                    Err(TransportError::Other(m)) => break 'attempt ApiError::NetworkError(m),
                };
                self.check_auth_context(context).await?;
                let status = response.status;
                tracing::debug!(
                    "Response: {} status={status} bytes={}",
                    endpoint.path(),
                    response.body.len()
                );
                match status {
                    200..=299 => {}
                    401 => {
                        // soft auth 의 만료 쿠키 401 → 세션 만료 알림 없이 쿠키 제거 후 1회 재시도.
                        if used_soft_auth && !soft_auth_retried {
                            tracing::info!(
                                "Soft auth 401 on {} — 쿠키 제거 후 재시도",
                                endpoint.path()
                            );
                            request.set_header("Cookie", None);
                            used_soft_auth = false;
                            soft_auth_retried = true;
                            continue 'retry;
                        }
                        let _ = self.inner.session_expired.send(());
                        break 'attempt ApiError::Unauthorized;
                    }
                    429 => {
                        let retry_after = response
                            .header("Retry-After")
                            .and_then(|v| v.trim().parse::<f64>().ok())
                            .filter(|v| v.is_finite())
                            .unwrap_or(5.0);
                        if rate_limit_retries < attempt_limit - 1 {
                            rate_limit_retries += 1;
                            tracing::info!("Rate limited, retrying after {retry_after}s");
                            let wait =
                                retry_after.clamp(0.0, api_defaults::MAX_RATE_LIMIT_RETRY_SECS);
                            tokio::time::sleep(Duration::from_secs_f64(wait)).await;
                            continue 'retry;
                        }
                        break 'attempt ApiError::RateLimited { retry_after };
                    }
                    500..=599 => {
                        last_error = Some(ApiError::http(status as i64));
                        if attempt < attempt_limit - 1 {
                            tokio::time::sleep(backoff(attempt + 1)).await;
                            attempt += 1;
                            continue 'retry;
                        }
                        break 'attempt ApiError::http(status as i64);
                    }
                    other => break 'attempt ApiError::http(other as i64),
                }

                let decoded: ChzzkResponse<T> = match validator::validate_and_decode(&response.body)
                {
                    Ok(d) => d,
                    Err(e) => break 'attempt e,
                };
                let Some(content) = decoded.content else {
                    break 'attempt ApiError::EmptyContent;
                };

                self.check_auth_context(context).await?;
                let fetched_at = match cache_ttl {
                    Some(ttl) if options.store_in_cache => self.inner.cache.set(
                        &cache_key,
                        content.clone(),
                        response.body.len(),
                        Self::stale_window(endpoint.as_ref(), ttl),
                    ),
                    _ => self.inner.cache.now(),
                };
                self.check_auth_context(context).await?;
                return Ok(AgedResponse {
                    value: content,
                    fetched_at,
                    is_stale: false,
                });
            };

            // 오류 처리 (Swift `catch let error as APIError`).
            if matches!(&error, ApiError::NetworkError(m) if m == "API fixture miss") {
                return Err(error);
            }
            self.check_auth_context(context).await?;
            if Self::is_expected_unsupported_response(&error, endpoint.as_ref()) {
                tracing::info!(
                    "Optional endpoint unavailable: {} status=404 fallback=emoticons",
                    endpoint.path()
                );
            } else {
                tracing::error!(
                    "Request failed: {} attempt={} error={error:?}",
                    endpoint.path(),
                    attempt + 1
                );
            }
            last_error = Some(error.clone());
            match &error {
                ApiError::Unauthorized
                | ApiError::InvalidResponse
                | ApiError::EmptyContent
                | ApiError::DecodingFailed(_)
                | ApiError::MalformedResponse(_)
                | ApiError::Cancelled => return Err(error),
                ApiError::HttpError { status_code } if (400..=499).contains(status_code) => {
                    return Err(error)
                }
                // 429 는 전용 예산을 다 쓴 뒤라 여기서 끝낸다(normal 예산으로 또 때리지 않는다).
                ApiError::RateLimited { .. } => return Err(error),
                _ => {
                    if attempt >= attempt_limit - 1 {
                        return Err(error);
                    }
                    attempt += 1;
                    tokio::time::sleep(backoff(attempt)).await;
                }
            }
        }
        Err(last_error
            .unwrap_or_else(|| ApiError::NetworkError("Unknown error after retries".into())))
    }

    /// 응답 본문 디코딩 없이 호출한다 (POST/DELETE 등).
    pub async fn request_raw(&self, endpoint: &dyn EndpointProtocol) -> Result<(), ApiError> {
        let url = build_url(
            &self.inner.base_url,
            &endpoint.path(),
            endpoint.query_items().as_deref(),
        )?;
        let mut request = HttpRequest::new(endpoint.method(), url);
        request.body = endpoint.body();
        if endpoint.requires_auth() {
            if let Some(cookies) = self.auth_cookies().await {
                request.set_header("Cookie", HttpCookie::header_value(&cookies).as_deref());
            } else {
                let token = match &self.inner.auth {
                    Some(a) => a.access_token().await,
                    None => None,
                };
                match token {
                    Some(t) => request.set_header("Authorization", Some(&format!("Bearer {t}"))),
                    None => return Err(ApiError::Unauthorized),
                }
            }
        }
        let response = self
            .send(request, None)
            .await
            .map_err(transport_to_api_error)?;
        match response.status {
            200..=299 => Ok(()),
            401 => Err(ApiError::Unauthorized),
            other => Err(ApiError::http(other as i64)),
        }
    }

    // MARK: - 공유 GET

    async fn shared_get<T: ResponseBody>(
        &self,
        endpoint: Arc<EndpointSnapshot>,
        shared: SharedGet,
    ) -> Result<AgedResponse<T>, ApiError> {
        let (ticket, is_leader) = {
            let mut st = self.lock_state();
            if let Some(existing) = st.get_waits.get(&shared.key) {
                (existing.wait.reserve(), false)
            } else {
                let weak: Weak<Inner> = Arc::downgrade(&self.inner);
                let abandon_key = shared.key.clone();
                let wait = GetWait::new(Some(Box::new(move |w: &GetWait| {
                    if let Some(inner) = weak.upgrade() {
                        ChzzkApiClient { inner }.abandon_get_wait(&abandon_key, w);
                    }
                })));
                let ticket = wait.reserve();
                let client = self.clone();
                let task_wait = wait.clone();
                let task_shared = shared.clone();
                // 스폰은 락 안에서 한다 — 작업이 끝나 기록을 지우는 것이 등록보다 앞설 수 없다.
                let task = tokio::spawn(async move {
                    let _finish = LeaderFinish {
                        client: client.clone(),
                        key: task_shared.key.clone(),
                        wait: task_wait.clone(),
                    };
                    let options = RequestOptions {
                        bypass_cache: task_shared.bypass_cache,
                        store_in_cache: task_shared.store_in_cache,
                        joined: true,
                        purpose: task_shared.purpose,
                    };
                    match Self::boxed_request::<T>(client.clone(), endpoint, options).await {
                        Ok(value) => task_wait.succeed(Arc::new(value), None),
                        Err(error) => {
                            // bypass 리더·취소·인증·4xx·디코드 오류는 이전 값으로 덮지 않는다.
                            let fallback = if !task_shared.bypass_cache
                                && Self::allows_stale_on_error(&error)
                            {
                                client.stale_cached::<T>(&task_shared.key, task_shared.stale_window)
                            } else {
                                None
                            };
                            match fallback {
                                Some(cached) => task_wait.succeed(Arc::new(cached), Some(error)),
                                None => task_wait.fail(error),
                            }
                        }
                    }
                });
                st.running_leaders += 1;
                st.get_waits
                    .insert(shared.key.clone(), GetWaitEntry { wait, task });
                (ticket, true)
            }
        };
        let _ = is_leader;
        self.await_shared_get::<T>(ticket, &shared).await
    }

    async fn await_shared_get<T: ResponseBody>(
        &self,
        ticket: WaitTicket,
        shared: &SharedGet,
    ) -> Result<AgedResponse<T>, ApiError> {
        // bypassCache 호출자는 오류 뒤 이전 값을 받지 않는다.
        let any = ticket.value(!shared.bypass_cache).await?;
        let typed = any
            .downcast::<AgedResponse<T>>()
            .map_err(|_| ApiError::DecodingFailed("공유 응답 타입이 다릅니다".into()))?;
        // 기다리는 사이 로그인 세대가 바뀌었으면 이전 계정의 값이다.
        self.check_auth_context(shared.context).await?;
        Ok((*typed).clone())
    }

    /// 공유 작업이 끝나면 부른다. 신원이 같을 때만 지운다.
    fn finish_get_wait(&self, key: &str, wait: &GetWait) {
        let mut st = self.lock_state();
        st.running_leaders = st.running_leaders.saturating_sub(1);
        if st.get_waits.get(key).is_some_and(|e| e.wait.ptr_eq(wait)) {
            st.get_waits.remove(key);
        }
    }

    /// 기다리는 호출자가 모두 떠난 공유 GET 을 멈춘다.
    fn abandon_get_wait(&self, key: &str, wait: &GetWait) {
        let task = {
            let mut st = self.lock_state();
            match st.get_waits.get(key) {
                Some(entry) if entry.wait.ptr_eq(wait) && wait.is_abandoned() => {
                    st.get_waits.remove(key).map(|e| e.task)
                }
                _ => None,
            }
        };
        if let Some(t) = task {
            t.abort();
        }
    }

    fn stale_cached<T: ResponseBody>(&self, key: &str, stale: f64) -> Option<AgedResponse<T>> {
        match self.inner.cache.lookup::<T>(key, 0.0, stale, None) {
            Lookup::Stale { value, fetched_at } => Some(AgedResponse {
                value,
                fetched_at,
                is_stale: true,
            }),
            _ => None,
        }
    }

    // MARK: - stale 재검증 게이트

    fn schedule_stale_revalidation(&self, key: String, run: RevalidationRun) {
        let mut st = self.lock_state();
        // 같은 키를 이미 받는 중이면 그 결과가 캐시를 갱신한다.
        if st.get_waits.contains_key(&key) || st.revalidation_keys.contains(&key) {
            return;
        }
        let job = StaleRevalidation { key, run };
        if st.revalidations_running < STALE_REVALIDATION_CONCURRENCY {
            self.start_stale_revalidation(&mut st, job);
        } else if st.revalidation_queue.len() < STALE_REVALIDATION_QUEUE_LIMIT {
            st.revalidation_keys.insert(job.key.clone());
            st.revalidation_queue.push_back(job);
        } else {
            st.revalidations_dropped += 1;
        }
    }

    fn start_stale_revalidation(&self, st: &mut ClientState, job: StaleRevalidation) {
        st.revalidations_running += 1;
        st.revalidations_started += 1;
        st.revalidation_keys.insert(job.key.clone());
        let client = self.clone();
        tokio::spawn(async move {
            let StaleRevalidation { key, run } = job;
            run(client.clone()).await;
            client.finish_stale_revalidation(&key);
        });
    }

    fn finish_stale_revalidation(&self, key: &str) {
        let mut st = self.lock_state();
        st.revalidations_running = st.revalidations_running.saturating_sub(1);
        st.revalidation_keys.remove(key);
        while st.revalidations_running < STALE_REVALIDATION_CONCURRENCY {
            let Some(next) = st.revalidation_queue.pop_front() else {
                break;
            };
            if st.get_waits.contains_key(&next.key) {
                st.revalidation_keys.remove(&next.key);
                continue;
            }
            self.start_stale_revalidation(&mut st, next);
        }
    }

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
