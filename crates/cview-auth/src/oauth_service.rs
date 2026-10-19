//! `Sources/CViewAuth/ChzzkOAuthService.swift` 포팅 — 치지직 OAuth 토큰 교환·갱신·검증.
//!
//! - Swift actor → `&self` 메서드 + 짧은 `Mutex` 구간(대기 지점을 넘겨 잡지 않는다 — actor 재진입과 같은 모양).
//! - `URLSession` → `cview_networking::HttpTransport` (기본 [`ReqwestTransport`], 테스트는 가짜 전송).
//! - 테스트 주입 클로저 `tokenRequest` / `profileRequest` 는 [`TokenRequestFn`] / [`ProfileRequestFn`].
//!
//! 자격 증명은 요청 직전에 해석한다: 주입된 설정에 이미 있으면 그것 → 환경변수 → 보안 저장소.
//! ⚠ 첫 절(주입 설정 존중)을 지우지 말 것 — 명시적으로 자격 증명을 넘긴 호출자를 무시하게 된다.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

use cview_core::error::AuthError;
use cview_core::protocols::HttpMethod;
use cview_networking::{HttpRequest, HttpTransport, ReqwestTransport, TransportError};
use futures_util::future::BoxFuture;
use url::Url;

use crate::credentials::OAuthCredentialStore;
use crate::generation::AuthSessionGeneration;
use crate::oauth_models::{
    OAuthConfig, OAuthProfileResponse, OAuthTokenResponse, OAuthTokens, OAuthUserProfile,
};
use crate::pkce::PkceChallenge;
use crate::refresh_failures::AuthServiceError;
use crate::secret_store::KeychainService;

/// 토큰 엔드포인트 대체 — 요청 본문(JSON 객체 필드)을 받아 토큰을 돌려준다.
pub type TokenRequestFn = Arc<
    dyn Fn(BTreeMap<String, String>) -> BoxFuture<'static, Result<OAuthTokens, AuthServiceError>>
        + Send
        + Sync,
>;

/// 프로필 엔드포인트 대체 — access token 을 받아 프로필을 돌려준다.
pub type ProfileRequestFn = Arc<
    dyn Fn(String) -> BoxFuture<'static, Result<OAuthUserProfile, AuthServiceError>> + Send + Sync,
>;

/// 발급 토큰 저장 키.
pub const TOKEN_KEYCHAIN_KEY: &str = "oauth_tokens";
const VALIDATE_URL: &str = "https://api.chzzk.naver.com/service/v1/users/me";
const PROFILE_URL: &str = "https://openapi.chzzk.naver.com/open/v1/users/me";

#[derive(Debug, Default)]
struct FlowState {
    current_state: Option<String>,
    /// PKCE 흐름 진행 중인 verifier. 교환 직후 비운다.
    current_pkce: Option<PkceChallenge>,
}

/// 치지직 OAuth 서비스.
pub struct ChzzkOAuthService {
    config: OAuthConfig,
    keychain: Arc<KeychainService>,
    transport: OnceLock<Arc<dyn HttpTransport>>,
    session_generation: Arc<AuthSessionGeneration>,
    token_request: Option<TokenRequestFn>,
    profile_request: Option<ProfileRequestFn>,
    credential_store: OAuthCredentialStore,
    flow: Mutex<FlowState>,
}

impl std::fmt::Debug for ChzzkOAuthService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChzzkOAuthService")
            .field("client_id", &self.config.client_id)
            .field("redirect_uri", &self.config.redirect_uri)
            .finish_non_exhaustive()
    }
}

impl ChzzkOAuthService {
    /// 프로세스 환경변수 + `keychain` 으로 자격 증명을 조달하는 서비스.
    pub fn new(config: OAuthConfig, keychain: Arc<KeychainService>) -> Self {
        let credential_store =
            OAuthCredentialStore::from_process_env(keychain.clone(), config.client_id.clone());
        Self {
            config,
            keychain,
            transport: OnceLock::new(),
            session_generation: Arc::new(AuthSessionGeneration::new()),
            token_request: None,
            profile_request: None,
            credential_store,
            flow: Mutex::new(FlowState::default()),
        }
    }

    pub fn with_transport(self, transport: Arc<dyn HttpTransport>) -> Self {
        let _ = self.transport.set(transport);
        self
    }

    pub fn with_token_request(mut self, f: TokenRequestFn) -> Self {
        self.token_request = Some(f);
        self
    }

    pub fn with_profile_request(mut self, f: ProfileRequestFn) -> Self {
        self.profile_request = Some(f);
        self
    }

    pub fn with_credential_store(mut self, store: OAuthCredentialStore) -> Self {
        self.credential_store = store;
        self
    }

    /// 인증 관리자·저장 계층이 함께 쓰는 세션 세대.
    pub fn session_generation(&self) -> &Arc<AuthSessionGeneration> {
        &self.session_generation
    }

    fn flow(&self) -> std::sync::MutexGuard<'_, FlowState> {
        self.flow.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn transport(&self) -> Result<Arc<dyn HttpTransport>, AuthServiceError> {
        if let Some(t) = self.transport.get() {
            return Ok(t.clone());
        }
        let built = ReqwestTransport::new()
            .map_err(|e| AuthServiceError::Transport(TransportError::Other(e.to_string())))?;
        Ok(self.transport.get_or_init(|| Arc::new(built)).clone())
    }

    // MARK: - Auth URL

    /// OAuth 인가 URL 을 만들고 state(+PKCE)를 기억한다. 자격 증명이 없으면 **만들지 않는다**
    /// (동의를 마친 뒤에야 교환에서 실패하는 것을 막는다). 호출자는 `None` 이면 쿠키 로그인만 한다.
    pub fn generate_auth_url(&self) -> Option<Url> {
        let Some(resolved) = self.resolved_config() else {
            tracing::info!("OAuth 클라이언트 자격증명 미설정 — 인가 URL 생성을 건너뛴다");
            return None;
        };
        let state = uuid::Uuid::new_v4().to_string().to_uppercase();
        let pkce = resolved.use_pkce.then(PkceChallenge::generate);
        {
            let mut flow = self.flow();
            flow.current_state = Some(state.clone());
            flow.current_pkce = pkce.clone();
        }
        resolved.build_auth_url(&state, pkce.as_ref())
    }

    // MARK: - 클라이언트 자격 증명

    /// 쓸 수 있는 자격 증명이 실린 설정. 주입 설정 → 환경변수 → 보안 저장소.
    fn resolved_config(&self) -> Option<OAuthConfig> {
        if self.config.has_credentials() {
            return Some(self.config.clone());
        }
        let credentials = self.credential_store.resolve()?;
        credentials
            .is_usable()
            .then(|| self.config.applying(&credentials))
    }

    /// 갱신용 설정 — 없으면 `MissingClientCredentials`(최종), 읽지 못하면 `CredentialsUnavailable`(일시).
    fn refresh_config(&self) -> Result<OAuthConfig, AuthServiceError> {
        if self.config.has_credentials() {
            return Ok(self.config.clone());
        }
        match self.credential_store.resolve_for_refresh()? {
            Some(c) => Ok(self.config.applying(&c)),
            None => Err(AuthServiceError::MissingClientCredentials),
        }
    }

    /// OAuth 단계를 켤 수 있는가.
    pub fn has_client_credentials(&self) -> bool {
        self.resolved_config().is_some()
    }

    /// 현재 유효한 clientId (표시용).
    pub fn effective_client_id(&self) -> Option<String> {
        self.resolved_config().map(|c| c.client_id)
    }

    /// 사용자가 등록한 애플리케이션의 자격 증명을 저장한다.
    pub fn set_client_credentials(
        &self,
        client_id: &str,
        client_secret: &str,
    ) -> Result<(), AuthServiceError> {
        self.credential_store.save(client_id, client_secret)
    }

    /// 저장된 자격 증명을 지운다.
    pub fn clear_client_credentials(&self) -> Result<(), AuthServiceError> {
        self.credential_store.clear()
    }

    /// 보안 저장소에 쓸 수 있는 자격 증명이 있는가(환경변수 제외).
    pub fn has_stored_client_credentials(&self) -> bool {
        self.credential_store
            .stored_credentials()
            .is_some_and(|c| c.is_usable())
    }

    /// 마지막으로 만든 인가 URL 의 state.
    pub fn saved_state(&self) -> Option<String> {
        self.flow().current_state.clone()
    }

    pub fn redirect_uri(&self) -> &str {
        &self.config.redirect_uri
    }

    pub fn config(&self) -> &OAuthConfig {
        &self.config
    }

    // MARK: - Token Exchange

    /// authorization code 를 토큰으로 교환한다(새 세대를 연다). `state` 는 호출자가 연 authURL 의 값.
    pub async fn exchange_code_for_tokens(
        &self,
        code: &str,
        state: &str,
    ) -> Result<OAuthTokens, AuthServiceError> {
        let generation = self.session_generation.advance();
        self.exchange_code_for_tokens_in(code, state, generation)
            .await
    }

    /// 세대 `generation` 소유의 교환. 세대가 바뀌면 [`AuthServiceError::Cancelled`].
    pub async fn exchange_code_for_tokens_in(
        &self,
        code: &str,
        state: &str,
        generation: u64,
    ) -> Result<OAuthTokens, AuthServiceError> {
        self.session_generation.check(generation)?;
        let resolved = self
            .resolved_config()
            .ok_or(AuthServiceError::MissingClientCredentials)?;
        self.session_generation.check(generation)?;
        let mut body = BTreeMap::from([
            ("grantType".to_string(), "authorization_code".to_string()),
            ("clientId".to_string(), resolved.client_id.clone()),
            ("clientSecret".to_string(), resolved.client_secret.clone()),
            ("code".to_string(), code.to_string()),
            ("state".to_string(), state.to_string()),
        ]);
        if let Some(pkce) = &self.flow().current_pkce {
            body.insert("codeVerifier".into(), pkce.code_verifier.clone());
        }
        let result = self.perform_token_request(&resolved, body).await;
        // state/PKCE 는 1회용 — 성공·실패와 무관하게 비운다.
        {
            let mut flow = self.flow();
            if flow.current_state.as_deref() == Some(state) {
                flow.current_state = None;
                flow.current_pkce = None;
            }
        }
        let tokens = result?;
        self.session_generation.check(generation)?;
        self.persist_tokens(&tokens, generation)?;
        tracing::info!("OAuth tokens exchanged successfully");
        Ok(tokens)
    }

    fn persist_tokens(
        &self,
        tokens: &OAuthTokens,
        generation: u64,
    ) -> Result<(), AuthServiceError> {
        match self.keychain.save_codable_in_session(
            tokens,
            TOKEN_KEYCHAIN_KEY,
            &self.session_generation,
            generation,
        ) {
            Ok(()) => {}
            Err(crate::secret_store::StoreError::Cancelled) => {
                return Err(AuthServiceError::Cancelled)
            }
            // 저장 실패해도 로그인은 진행 — 토큰은 메모리에 남는다.
            Err(e) => tracing::warn!("키체인 저장 실패 (토큰은 메모리 유지): {e}"),
        }
        self.session_generation.check(generation)?;
        Ok(())
    }

    // MARK: - Token Refresh

    /// refresh token 으로 액세스 토큰을 갱신한다(현재 세대).
    pub async fn refresh_tokens(
        &self,
        refresh_token: &str,
    ) -> Result<OAuthTokens, AuthServiceError> {
        let generation = self.session_generation.current();
        self.refresh_tokens_in(refresh_token, generation).await
    }

    /// 세대 `generation` 소유의 갱신.
    pub async fn refresh_tokens_in(
        &self,
        refresh_token: &str,
        generation: u64,
    ) -> Result<OAuthTokens, AuthServiceError> {
        self.session_generation.check(generation)?;
        let resolved = self.refresh_config()?;
        self.session_generation.check(generation)?;
        let body = BTreeMap::from([
            ("grantType".to_string(), "refresh_token".to_string()),
            ("clientId".to_string(), resolved.client_id.clone()),
            ("clientSecret".to_string(), resolved.client_secret.clone()),
            ("refreshToken".to_string(), refresh_token.to_string()),
        ]);
        let mut tokens = self.perform_token_request(&resolved, body).await?;
        self.session_generation.check(generation)?;
        // 일부 서버는 refresh 응답에서 새 refreshToken 을 생략한다 — 입력 값을 유지한다.
        if tokens.refresh_token.as_deref().is_none_or(str::is_empty) {
            tokens.refresh_token = Some(refresh_token.to_string());
        }
        self.persist_tokens(&tokens, generation)?;
        tracing::info!("OAuth tokens refreshed successfully");
        Ok(tokens)
    }

    // MARK: - Validation / Profile

    /// 액세스 토큰 유효성 검증 (`/service/v1/users/me` 가 200 인가).
    pub async fn validate_token(&self, access_token: &str) -> bool {
        let Ok(transport) = self.transport() else {
            return false;
        };
        let request = bearer_get(VALIDATE_URL, access_token);
        matches!(transport.send(request).await, Ok(r) if r.status == 200)
    }

    /// OAuth 토큰으로 사용자 프로필 조회 (Open API).
    pub async fn fetch_user_profile(
        &self,
        access_token: &str,
    ) -> Result<OAuthUserProfile, AuthServiceError> {
        if let Some(f) = &self.profile_request {
            return f(access_token.to_string()).await;
        }
        let response = self
            .transport()?
            .send(bearer_get(PROFILE_URL, access_token))
            .await?;
        if response.status != 200 {
            return Err(AuthError::OauthFailed(format!(
                "Profile fetch failed: HTTP {}",
                response.status
            ))
            .into());
        }
        let parsed: OAuthProfileResponse = serde_json::from_slice(&response.body)
            .map_err(|e| AuthServiceError::Decoding(e.to_string()))?;
        let content = parsed
            .content
            .ok_or_else(|| AuthError::OauthFailed("Empty profile response".into()))?;
        tracing::info!(
            "OAuth profile: {} (channel: {})",
            content.nickname.as_deref().unwrap_or("unknown"),
            content.channel_id.as_deref().unwrap_or("none")
        );
        Ok(content)
    }

    // MARK: - Storage

    /// 저장된 토큰. 읽기·해석 실패는 기록하고 `None`.
    pub fn load_stored_tokens(&self) -> Option<OAuthTokens> {
        match self.keychain.load_codable(TOKEN_KEYCHAIN_KEY) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("Failed to load OAuth tokens: {e}");
                None
            }
        }
    }

    /// 저장된 토큰을 지운다(새 세대를 연다).
    pub fn clear_stored_tokens(&self) {
        let generation = self.session_generation.advance();
        self.clear_stored_tokens_in(generation);
    }

    /// 세대 `generation` 소유의 삭제. 진행 중 PKCE 는 지우지 않는다(교환이 정리한다).
    pub fn clear_stored_tokens_in(&self, generation: u64) {
        match self.keychain.delete_in_session(
            TOKEN_KEYCHAIN_KEY,
            &self.session_generation,
            generation,
        ) {
            Ok(()) => tracing::info!("OAuth tokens cleared"),
            Err(crate::secret_store::StoreError::Cancelled) => {}
            Err(e) => tracing::error!("OAuth token deletion failed: {e}"),
        }
    }

    // MARK: - Private

    async fn perform_token_request(
        &self,
        config: &OAuthConfig,
        body: BTreeMap<String, String>,
    ) -> Result<OAuthTokens, AuthServiceError> {
        if let Some(f) = &self.token_request {
            return f(body).await;
        }
        let url = Url::parse(&config.token_url)
            .map_err(|_| AuthError::OauthFailed("Invalid token URL".into()))?;
        let mut request = HttpRequest::new(HttpMethod::Post, url);
        request.set_header("Content-Type", Some("application/json"));
        request.set_header("Accept", Some("application/json"));
        request.body =
            Some(serde_json::to_vec(&body).map_err(|e| AuthServiceError::Decoding(e.to_string()))?);
        request.timeout = Some(std::time::Duration::from_secs(30));
        let response = self.transport()?.send(request).await?;
        if response.status != 200 {
            tracing::error!("Token request failed: HTTP {}", response.status);
            // 거절·일시(5xx·408·429)·미분류.
            return Err(AuthServiceError::token_failure(response.status));
        }
        let parsed: OAuthTokenResponse = serde_json::from_slice(&response.body)
            .map_err(|e| AuthServiceError::Decoding(e.to_string()))?;
        let content = parsed
            .content
            .ok_or_else(|| AuthServiceError::TokenRejected {
                detail: parsed
                    .message
                    .unwrap_or_else(|| "Empty token response".into()),
            })?;
        Ok(OAuthTokens::new(
            content.access_token,
            content.refresh_token,
            content.expires_in,
            content.token_type,
        ))
    }
}

fn bearer_get(url: &str, access_token: &str) -> HttpRequest {
    let mut request = HttpRequest::new(HttpMethod::Get, Url::parse(url).expect("static url"));
    request.set_header("Authorization", Some(&format!("Bearer {access_token}")));
    request.set_header("Accept", Some("application/json"));
    request
}
