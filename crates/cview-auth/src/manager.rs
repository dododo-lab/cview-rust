//! `Sources/CViewAuth/AuthManager.swift` + `AuthManager+SessionRecovery.swift` 포팅 — 통합 인증 관리자.
//!
//! 쿠키 기반 네이버 로그인과 치지직 OAuth 토큰 인증을 함께 다룬다.
//!
//! 동시성 모델:
//! - Swift actor → `Arc` 공유 내부 + `std::sync::Mutex` 짧은 구간. 대기 지점을 넘겨 락을 잡지 않으므로
//!   actor 재진입과 같은 의미다. 모든 비동기 단계 뒤에는 세션 세대(`AuthSessionGeneration`)를 다시
//!   확인해, 로그아웃·재로그인 뒤 늦게 끝난 작업이 상태를 덮어쓰지 못하게 한다.
//! - Swift 비구조 `Task` → `tokio::spawn`. 진행 중 갱신은 [`Shared`] future 로 동시 호출자를 합친다.
//! - `AsyncStream` 상태 스트림 → 구독자별 `mpsc::UnboundedReceiver` (모든 전이를 빠짐없이 받는다).
//!
//! 갱신 실패 정책(원본 주석 요약):
//! - 실패 뒤 다음 네트워크 시도까지 백오프(60s → ×2 → 30m 상한). 전송 계층 실패는 늘리지 않고 15s.
//! - 최종 실패(거절·자격 증명 없음)는 토큰을 버린다 — 죽은 POST 를 되풀이하지 않게.
//! - 부팅 갱신은 일시 실패만 토큰을 쥔 채 로그인으로 끝낸다. 미분류 실패는 저장 토큰을 남기고 로그아웃.
//! - 갱신 최종 실패 뒤 NAVER 쿠키가 있으면 쿠키 로그인으로 남는다(`Expired` 는 쿠키 접근까지 끈다).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use async_trait::async_trait;
use cview_core::error::AuthError;
use cview_core::protocols::{AuthState, AuthTokenProvider, HttpCookie};
use futures_util::future::{BoxFuture, FutureExt, Shared};
use tokio::sync::mpsc;
use tokio::task::{AbortHandle, JoinHandle};
use tokio::time::Instant;

use crate::cookie::CookieManager;
use crate::oauth_models::{OAuthTokens, OAuthUserProfile};
use crate::oauth_service::ChzzkOAuthService;
use crate::refresh_failures::AuthServiceError;

/// 로그아웃 때 쿠키·웹뷰 데이터를 지우는 동작. 테스트는 no-op 을 넣는다.
pub type CookieCleanupFn = Arc<dyn Fn() -> BoxFuture<'static, ()> + Send + Sync>;

/// 첫 갱신 실패 백오프.
pub const REFRESH_BACKOFF_INITIAL: Duration = Duration::from_secs(60);
/// 백오프 상한.
pub const REFRESH_BACKOFF_CAP: Duration = Duration::from_secs(1_800);
/// 전송 계층 실패 뒤 다음 시도까지의 간격 — 연결이 돌아오면 곧바로 회복하도록 짧게 둔다.
pub const OFFLINE_REFRESH_RETRY: Duration = Duration::from_secs(15);
/// 자동 회전: 만료 몇 초 전에 갱신하는가.
const AUTO_REFRESH_LEAD_SECS: i64 = 15 * 60;
/// 자동 회전 1차 실패 뒤 재시도 간격.
const AUTO_REFRESH_RETRY_DELAY: Duration = Duration::from_secs(30);

type SharedBool = Shared<BoxFuture<'static, bool>>;
type SharedUnit = Shared<BoxFuture<'static, ()>>;

struct InFlight {
    id: u64,
    future: SharedBool,
    abort: AbortHandle,
}

struct State {
    auth_state: AuthState,
    subscribers: Vec<mpsc::UnboundedSender<AuthState>>,
    oauth_tokens: Option<OAuthTokens>,
    refresh_backoff_until: Option<Instant>,
    refresh_backoff: Duration,
    in_flight: Option<InFlight>,
    token_refresh_task: Option<JoinHandle<()>>,
    cleanup: Option<SharedUnit>,
}

struct Inner {
    cookie_manager: Arc<CookieManager>,
    oauth: Arc<ChzzkOAuthService>,
    cookie_cleanup: CookieCleanupFn,
    cookie_access: AtomicBool,
    next_flight_id: AtomicU64,
    state: Mutex<State>,
}

/// 통합 인증 관리자. 복제하면 같은 관리자를 가리킨다.
#[derive(Clone)]
pub struct AuthManager {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for AuthManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthManager")
            .field("auth_state", &self.auth_state())
            .field("generation", &self.authentication_generation())
            .finish_non_exhaustive()
    }
}

impl AuthManager {
    /// `cookie_cleanup` 이 `None` 이면 로그아웃이 [`CookieManager::clear_cookies`] 를 부른다.
    pub fn new(
        cookie_manager: Arc<CookieManager>,
        oauth: Arc<ChzzkOAuthService>,
        cookie_cleanup: Option<CookieCleanupFn>,
    ) -> Self {
        let cookie_cleanup = cookie_cleanup.unwrap_or_else(|| {
            let cm = cookie_manager.clone();
            Arc::new(move || {
                let cm = cm.clone();
                async move { cm.clear_cookies().await }.boxed()
            })
        });
        Self {
            inner: Arc::new(Inner {
                cookie_manager,
                oauth,
                cookie_cleanup,
                cookie_access: AtomicBool::new(false),
                next_flight_id: AtomicU64::new(1),
                state: Mutex::new(State {
                    auth_state: AuthState::LoggedOut,
                    subscribers: Vec::new(),
                    oauth_tokens: None,
                    refresh_backoff_until: None,
                    refresh_backoff: REFRESH_BACKOFF_INITIAL,
                    in_flight: None,
                    token_refresh_task: None,
                    cleanup: None,
                }),
            }),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        lock_state(&self.inner)
    }

    pub fn oauth_service(&self) -> &Arc<ChzzkOAuthService> {
        &self.inner.oauth
    }

    pub fn cookie_manager(&self) -> &Arc<CookieManager> {
        &self.inner.cookie_manager
    }

    // MARK: - Generation

    /// 로그인/로그아웃마다 바뀌는 세대.
    pub fn authentication_generation(&self) -> u64 {
        self.inner.oauth.session_generation().current()
    }

    fn is_current(&self, generation: u64) -> bool {
        generation == self.authentication_generation()
    }

    fn begin_session(&self) -> u64 {
        let generation = self.inner.oauth.session_generation().advance();
        let mut st = self.state();
        if let Some(task) = st.token_refresh_task.take() {
            task.abort();
        }
        if let Some(flight) = st.in_flight.take() {
            flight.abort.abort();
        }
        st.oauth_tokens = None;
        // 새 세션은 이전 세션의 실패 백오프를 물려받지 않는다.
        st.refresh_backoff_until = None;
        st.refresh_backoff = REFRESH_BACKOFF_INITIAL;
        self.inner.cookie_access.store(false, Ordering::SeqCst);
        generation
    }

    async fn begin_login(&self) -> Result<u64, AuthServiceError> {
        let previous = self.authentication_generation();
        let cleanup = self.state().cleanup.clone();
        if let Some(cleanup) = cleanup {
            cleanup.await;
        }
        if !self.is_current(previous) {
            return Err(AuthServiceError::Cancelled);
        }
        Ok(self.begin_session())
    }

    // MARK: - Token provider

    /// 요청에 붙일 인증 쿠키. 로그인 상태가 아니면 `None`.
    ///
    /// ⚠ 메모이즈하지 말 것 — 쿠키는 응답·웹뷰 주입으로 바뀌고 통지가 없다.
    pub fn cookies(&self) -> Option<Vec<HttpCookie>> {
        if !self.inner.cookie_access.load(Ordering::SeqCst) {
            return None;
        }
        let cookies = self.inner.cookie_manager.auth_http_cookies();
        (!cookies.is_empty()).then_some(cookies)
    }

    /// 유효한 OAuth 액세스 토큰. 만료됐으면 (백오프를 지키며) 갱신을 시도한다.
    pub async fn access_token(&self) -> Option<String> {
        let tokens = self.state().oauth_tokens.clone()?;
        if !tokens.is_expired() {
            return Some(tokens.access_token);
        }
        if self.refresh_oauth_token_if_needed().await {
            return self
                .state()
                .oauth_tokens
                .as_ref()
                .map(|t| t.access_token.clone());
        }
        None
    }

    pub fn is_authenticated(&self) -> bool {
        self.state().auth_state.is_logged_in()
    }

    /// 현재 OAuth 토큰(읽기 전용).
    pub fn oauth_tokens(&self) -> Option<OAuthTokens> {
        self.state().oauth_tokens.clone()
    }

    // MARK: - State

    pub fn auth_state(&self) -> AuthState {
        self.state().auth_state.clone()
    }

    /// 상태 스트림. 첫 값은 구독 시점의 현재 상태다. 여러 구독자를 지원한다.
    pub fn auth_state_stream(&self) -> mpsc::UnboundedReceiver<AuthState> {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut st = self.state();
        let _ = tx.send(st.auth_state.clone());
        st.subscribers.push(tx);
        rx
    }

    fn update_state(&self, state: AuthState) {
        let mut st = self.state();
        self.inner
            .cookie_access
            .store(state.is_logged_in(), Ordering::SeqCst);
        st.subscribers.retain(|s| s.send(state.clone()).is_ok());
        tracing::info!("Auth state: {state:?}");
        st.auth_state = state;
    }

    // MARK: - Login / Logout

    /// 기존 인증 복원(쿠키 + OAuth 토큰). 이미 사용자가 로그인·로그아웃을 요청했다면 아무것도 하지 않는다.
    pub async fn initialize(&self) {
        if self.authentication_generation() != 0 {
            return;
        }
        let generation = self.begin_session();
        let cm = &self.inner.cookie_manager;
        let mut restored = cm.restore_cookies_from_keychain();
        if !self.is_current(generation) {
            return;
        }
        if !restored {
            restored = cm.sync_from_webview_store().await;
            if !self.is_current(generation) {
                return;
            }
        }
        let stored = self.inner.oauth.load_stored_tokens();
        if !self.is_current(generation) {
            return;
        }
        if let Some(stored) = stored {
            if !stored.is_expired() {
                self.state().oauth_tokens = Some(stored);
                self.update_state(logged_in("oauth"));
                self.schedule_token_auto_refresh();
                return;
            }
            if let Some(refresh_token) = stored.refresh_token.clone() {
                match self
                    .inner
                    .oauth
                    .refresh_tokens_in(&refresh_token, generation)
                    .await
                {
                    Ok(tokens) => {
                        if !self.is_current(generation) {
                            return;
                        }
                        self.state().oauth_tokens = Some(tokens);
                        self.update_state(logged_in("oauth"));
                        self.schedule_token_auto_refresh();
                        return;
                    }
                    Err(e) if e.is_transient_refresh_failure() => {
                        // 일시 실패는 토큰을 지우지 않고 세션 중 갱신에 맡긴다. 상태도 로그인 유지.
                        if !self.is_current(generation) {
                            return;
                        }
                        tracing::warn!("Startup token refresh failed (tokens kept): {e}");
                        self.state().oauth_tokens = Some(stored);
                        self.note_refresh_failure(Some(&e));
                    }
                    Err(e) => {
                        // 일시 실패가 아니면 로그인을 알리지 않는다(유령 세션). 최종만 저장 토큰을 지운다.
                        if !self.is_current(generation) {
                            return;
                        }
                        tracing::warn!("Startup token refresh failed (not transient): {e}");
                        self.clear_stored_tokens_if_terminal(&e, generation);
                        if !self.is_current(generation) {
                            return;
                        }
                    }
                }
            }
        }
        let has_tokens = self.state().oauth_tokens.is_some();
        self.update_state(if restored {
            logged_in("cookie")
        } else if has_tokens {
            logged_in("oauth")
        } else {
            AuthState::LoggedOut
        });
    }

    /// 쿠키 로그인 완료 처리 (웹뷰에서 쿠키를 얻은 뒤 호출).
    pub async fn handle_login_success(&self) {
        let Ok(generation) = self.begin_login().await else {
            return;
        };
        self.inner.oauth.clear_stored_tokens_in(generation);
        if !self.is_current(generation) {
            return;
        }
        if let Err(e) = self.inner.cookie_manager.save_cookies_to_keychain() {
            tracing::warn!("Cookie persistence failed: {e}");
        }
        if !self.is_current(generation) {
            return;
        }
        self.update_state(logged_in("cookie"));
    }

    /// OAuth 로그인 완료 처리. callback state 검증은 호출자 몫(`csrf::validate`).
    pub async fn handle_oauth_login_success(
        &self,
        code: &str,
        state: &str,
    ) -> Result<(), AuthServiceError> {
        let generation = self.begin_login().await?;
        self.update_state(AuthState::LoggingIn);
        self.inner.oauth.clear_stored_tokens_in(generation);
        if !self.is_current(generation) {
            return Err(AuthServiceError::Cancelled);
        }
        let result = self
            .inner
            .oauth
            .exchange_code_for_tokens_in(code, state, generation)
            .await;
        let tokens = match result {
            Ok(t) => t,
            Err(e) => {
                if !self.is_current(generation) {
                    return Err(AuthServiceError::Cancelled);
                }
                if e != AuthServiceError::Cancelled {
                    self.update_state(AuthState::Error(e.to_string()));
                }
                return Err(e);
            }
        };
        if !self.is_current(generation) {
            return Err(AuthServiceError::Cancelled);
        }
        let cm = &self.inner.cookie_manager;
        if cm.has_cookies() {
            if let Err(e) = cm.save_cookies_to_keychain() {
                tracing::warn!("Cookie persistence failed: {e}");
            }
        }
        if !self.is_current(generation) {
            return Err(AuthServiceError::Cancelled);
        }
        self.state().oauth_tokens = Some(tokens);
        self.update_state(logged_in("oauth"));
        self.schedule_token_auto_refresh();
        Ok(())
    }

    /// 만료 토큰 갱신. 토큰이 없으면 false, 유효하면 true.
    /// 직전 실패의 백오프가 남아 있으면 새 요청을 시작하지 않는다(진행 중 갱신에는 합류한다).
    pub async fn refresh_oauth_token_if_needed(&self) -> bool {
        let tokens = {
            let st = self.state();
            let Some(tokens) = st.oauth_tokens.clone() else {
                return false;
            };
            if !tokens.is_expired() {
                return true;
            }
            if st.in_flight.is_none()
                && st
                    .refresh_backoff_until
                    .is_some_and(|until| Instant::now() < until)
            {
                return false;
            }
            tokens
        };
        self.refresh(tokens, false).await
    }

    /// 갱신 실패 기록 — 60초부터 실패마다 2배(30분 상한). 전송 계층 실패는 15초 뒤 다시.
    fn note_refresh_failure(&self, error: Option<&AuthServiceError>) {
        let mut st = self.state();
        if error.is_some_and(AuthServiceError::is_transport_failure) {
            st.refresh_backoff_until = Some(Instant::now() + OFFLINE_REFRESH_RETRY);
            return;
        }
        st.refresh_backoff_until = Some(Instant::now() + st.refresh_backoff);
        st.refresh_backoff = (st.refresh_backoff * 2).min(REFRESH_BACKOFF_CAP);
    }

    /// 백오프를 초기화한다 (진단·테스트용 — 시계를 기다리지 않고 "백오프 경과" 를 만든다).
    pub fn clear_refresh_backoff(&self) {
        let mut st = self.state();
        st.refresh_backoff_until = None;
        st.refresh_backoff = REFRESH_BACKOFF_INITIAL;
    }

    /// 다음 네트워크 갱신을 허용하는 시각.
    pub fn refresh_backoff_until(&self) -> Option<Instant> {
        self.state().refresh_backoff_until
    }

    /// 다음 실패 때 적용할 백오프.
    pub fn refresh_backoff(&self) -> Duration {
        self.state().refresh_backoff
    }

    async fn refresh(&self, tokens: OAuthTokens, retry: bool) -> bool {
        let generation = self.authentication_generation();
        let (future, id) = {
            let mut st = self.state();
            match &st.in_flight {
                Some(flight) => (flight.future.clone(), None),
                None => {
                    let id = self.inner.next_flight_id.fetch_add(1, Ordering::SeqCst);
                    let this = self.clone();
                    let handle = tokio::spawn(async move {
                        let result = this.perform_token_refresh(tokens, generation, retry).await;
                        // 소유자가 취소돼도 슬롯이 남지 않게, 끝난 작업이 자기 슬롯을 비운다.
                        let mut st = this.state();
                        if st.in_flight.as_ref().is_some_and(|f| f.id == id) {
                            st.in_flight = None;
                        }
                        result
                    });
                    let abort = handle.abort_handle();
                    let future = async move { handle.await.unwrap_or(false) }
                        .boxed()
                        .shared();
                    st.in_flight = Some(InFlight {
                        id,
                        future: future.clone(),
                        abort,
                    });
                    (future, Some(id))
                }
            }
        };
        let result = future.await;
        if !self.is_current(generation) {
            return false;
        }
        if let Some(id) = id {
            let mut st = self.state();
            if st.in_flight.as_ref().is_some_and(|f| f.id == id) {
                st.in_flight = None;
            }
        }
        result
    }

    async fn perform_token_refresh(
        &self,
        tokens: OAuthTokens,
        generation: u64,
        retry: bool,
    ) -> bool {
        if !self.is_current(generation) {
            return false;
        }
        let Some(refresh_token) = tokens.refresh_token.clone() else {
            // refresh token 이 없으면 영구 실패 — 토큰을 버린다.
            return self.drop_dead_tokens(generation);
        };
        let mut last_error = None;
        let attempts = if retry { 2 } else { 1 };
        for attempt in 0..attempts {
            if attempt > 0 {
                tokio::time::sleep(AUTO_REFRESH_RETRY_DELAY).await;
            }
            if !self.is_current(generation) {
                return false;
            }
            match self
                .inner
                .oauth
                .refresh_tokens_in(&refresh_token, generation)
                .await
            {
                Ok(updated) => {
                    if !self.is_current(generation) {
                        return false;
                    }
                    self.state().oauth_tokens = Some(updated);
                    self.clear_refresh_backoff();
                    self.update_state(logged_in("oauth"));
                    self.schedule_token_auto_refresh();
                    return true;
                }
                Err(e) if self.is_current(generation) && e.is_terminal_refresh_failure() => {
                    // 거절·자격 증명 없음은 재시도해도 같다.
                    return self.drop_dead_tokens(generation);
                }
                Err(e) => {
                    if !self.is_current(generation) || e == AuthServiceError::Cancelled {
                        return false;
                    }
                    tracing::warn!("Token refresh attempt failed: {e}");
                    last_error = Some(e);
                }
            }
        }
        if !self.is_current(generation) {
            return false;
        }
        // 최종 실패 — 다음 요청이 곧바로 같은 POST 를 쏘지 못하게 백오프를 건다.
        self.note_refresh_failure(last_error.as_ref());
        self.update_state(self.state_after_refresh_failure());
        false
    }

    /// 다시 해도 갱신되지 않는 토큰은 버린다.
    fn drop_dead_tokens(&self, generation: u64) -> bool {
        self.state().oauth_tokens = None;
        self.inner.oauth.clear_stored_tokens_in(generation);
        if self.is_current(generation) {
            self.update_state(self.state_after_refresh_failure());
        }
        false
    }

    /// 갱신 최종 실패 뒤 상태 — NAVER 쿠키가 있으면 쿠키 로그인, 없으면 만료.
    fn state_after_refresh_failure(&self) -> AuthState {
        if self.inner.cookie_manager.has_cookies() {
            logged_in("cookie")
        } else {
            AuthState::Expired
        }
    }

    /// 일시 실패가 아닌 부팅 갱신 뒤 — 최종이면 저장 토큰을 지우고, 미분류는 남긴다.
    fn clear_stored_tokens_if_terminal(&self, error: &AuthServiceError, generation: u64) {
        if error.is_terminal_refresh_failure() {
            self.inner.oauth.clear_stored_tokens_in(generation);
        }
    }

    /// 메모리 상태를 먼저 무효화한 뒤 저장 토큰·쿠키 정리를 기다린다.
    pub async fn logout(&self) {
        let generation = self.begin_session();
        self.update_state(AuthState::LoggedOut);
        let task = {
            let mut st = self.state();
            let previous = st.cleanup.take();
            let oauth = self.inner.oauth.clone();
            let cleanup = self.inner.cookie_cleanup.clone();
            let handle = tokio::spawn(async move {
                if let Some(previous) = previous {
                    previous.await;
                }
                oauth.clear_stored_tokens_in(generation);
                cleanup().await;
            });
            let shared = async move {
                let _ = handle.await;
            }
            .boxed()
            .shared();
            st.cleanup = Some(shared.clone());
            shared
        };
        task.await;
        if self.is_current(generation) {
            self.state().cleanup = None;
        }
        tracing::info!("Logout cleanup completed");
    }

    /// 웹사이트 데이터만 삭제 (쿠키·보안 저장소는 logout 경로).
    pub async fn clear_website_data_only(&self) {
        self.inner.cookie_manager.clear_all_website_data().await;
    }

    /// OAuth 사용자 프로필. 만료 토큰은 (백오프를 지키며) 먼저 갱신하고, 갱신할 수 없으면 보내지 않는다.
    pub async fn fetch_oauth_profile(&self) -> Result<OAuthUserProfile, AuthServiceError> {
        if self.oauth_tokens().is_none() {
            return Err(AuthError::OauthFailed("No OAuth tokens".into()).into());
        }
        let generation = self.authentication_generation();
        let Some(token) = self.access_token().await else {
            return Err(AuthError::OauthFailed("OAuth token unavailable".into()).into());
        };
        if !self.is_current(generation) {
            return Err(AuthServiceError::Cancelled);
        }
        let profile = self.inner.oauth.fetch_user_profile(&token).await?;
        if !self.is_current(generation) {
            return Err(AuthServiceError::Cancelled);
        }
        Ok(profile)
    }

    // MARK: - Token auto-rotation

    /// 만료 15분 전에 갱신을 예약한다. 이미 그 창 안이면 바로 갱신한다.
    fn schedule_token_auto_refresh(&self) {
        let mut st = self.state();
        if let Some(task) = st.token_refresh_task.take() {
            task.abort();
        }
        let Some(tokens) = st.oauth_tokens.clone() else {
            return;
        };
        if tokens.is_expired() {
            return;
        }
        if tokens.refresh_token.is_none() {
            tracing::info!("No refresh token — auto-rotation disabled");
            return;
        }
        let refresh_at = tokens.expires_at() - chrono::Duration::seconds(AUTO_REFRESH_LEAD_SECS);
        let delay = (refresh_at - chrono::Utc::now()).to_std().ok();
        let weak: Weak<Inner> = Arc::downgrade(&self.inner);
        st.token_refresh_task = Some(tokio::spawn(async move {
            match delay {
                Some(d) if !d.is_zero() => tokio::time::sleep(d).await,
                _ => tracing::info!("Token within refresh window — refreshing now"),
            }
            let Some(inner) = weak.upgrade() else {
                return;
            };
            AuthManager { inner }.perform_auto_refresh().await;
        }));
    }

    /// 자동 갱신 (최대 2회 — 1차 실패 뒤 30초).
    async fn perform_auto_refresh(&self) {
        let Some(tokens) = self.oauth_tokens() else {
            return;
        };
        let _ = self.refresh(tokens, true).await;
    }
}

fn lock_state(inner: &Inner) -> std::sync::MutexGuard<'_, State> {
    inner.state.lock().unwrap_or_else(|e| e.into_inner())
}

fn logged_in(user_id: &str) -> AuthState {
    AuthState::LoggedIn {
        user_id: user_id.into(),
    }
}

#[async_trait]
impl AuthTokenProvider for AuthManager {
    async fn cookies(&self) -> Option<Vec<HttpCookie>> {
        AuthManager::cookies(self)
    }
    async fn access_token(&self) -> Option<String> {
        AuthManager::access_token(self).await
    }
    async fn is_authenticated(&self) -> bool {
        AuthManager::is_authenticated(self)
    }
    async fn authentication_generation(&self) -> u64 {
        AuthManager::authentication_generation(self)
    }
}
