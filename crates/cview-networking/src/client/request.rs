//! 공개 요청 API와 요청 파이프라인 — 캐시·게이트·재시도·인증 검사·stale 허용.

use std::any::type_name;
use std::sync::Arc;
use std::time::Duration;

use cview_core::error::ApiError;
use cview_core::protocols::{CachePolicy, EndpointProtocol, HttpCookie, HttpMethod};

use super::{
    backoff, build_url, transport_to_api_error, AgedResponse, AuthContext, BoxFuture,
    ChzzkApiClient, EndpointSnapshot, RequestOptions, RequestPurpose, ResponseBody, SharedGet,
};
use crate::cache::Lookup;
use crate::constants::api_defaults;
use crate::headers;
use crate::response::ChzzkResponse;
use crate::transport::{HttpRequest, TransportError};
use crate::validator;

impl ChzzkApiClient {
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

    pub(super) fn boxed_request<T: ResponseBody>(
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
                        if crate::observer::is_active() {
                            crate::observer::observe(&crate::observer::RequestRecord {
                                method: "GET",
                                path: endpoint.path().to_string(),
                                status: None,
                                error: None,
                                duration_ms: 0.0,
                                bytes: 0,
                                cache_hit: true,
                            });
                        }
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

        // POST(팔로우 등)는 같은 요청을 두 번 보내면 두 번 적용될 수 있어 시간 초과·5xx 에 다시 보내지 않는다.
        let retry_safe = !matches!(endpoint.method(), HttpMethod::Post);
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
                        if retry_safe && attempt < attempt_limit - 1 {
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
                        if retry_safe && attempt < attempt_limit - 1 {
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
}
