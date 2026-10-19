//! OAuth 서비스 테스트.
//!
//! 원본: `Tests/CViewAuthTests/OAuthClientCredentialsTests.swift` (서비스 부분),
//! `PrecisionReview0926AuthStartupTests.tokenRequestUsesStatusMapping` (소스 검사 대신 가짜 전송으로 실제 경로 검증).
//! 추가: 실제 HTTP 경로(가짜 `HttpTransport`)의 요청 형식·응답 해석·PKCE.

mod common;

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use common::{keychain, token_fn};
use cview_auth::csrf::validate;
use cview_auth::{
    AuthServiceError, ChzzkOAuthService, KeychainService, OAuthConfig, OAuthCredentialStore,
    OAuthTokens, TOKEN_KEYCHAIN_KEY,
};
use cview_networking::{HttpRequest, HttpResponse, HttpTransport, TransportError};
use url::Url;

fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn service_with_env(pairs: &[(&str, &str)]) -> ChzzkOAuthService {
    let kc = Arc::new(KeychainService::in_memory());
    ChzzkOAuthService::new(OAuthConfig::chzzk(), kc.clone()).with_credential_store(
        OAuthCredentialStore::new(kc, OAuthConfig::chzzk().client_id, env(pairs)),
    )
}

fn query(url: &Url, name: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
}

#[test]
fn static_config_carries_no_secret() {
    assert!(OAuthConfig::chzzk().client_secret.is_empty());
    assert!(!OAuthConfig::chzzk().client_id.is_empty());
}

#[test]
fn no_auth_url_without_credentials() {
    let service = service_with_env(&[]);
    assert_eq!(service.generate_auth_url(), None);
    assert!(!service.has_client_credentials());
    assert_eq!(service.effective_client_id(), None);
}

#[test]
fn auth_url_uses_resolved_client_id_and_state_validates() {
    let service = service_with_env(&[
        ("CVIEW_OAUTH_CHZZK_ID", "env-client-id"),
        ("CVIEW_OAUTH_CHZZK_SECRET", "env-secret"),
    ]);
    let url = service.generate_auth_url().unwrap();
    assert_eq!(query(&url, "clientId").as_deref(), Some("env-client-id"));
    assert!(!url.as_str().contains("env-secret"));
    let state = query(&url, "state").unwrap();
    assert_eq!(service.saved_state(), Some(state.clone()));
    assert!(validate(Some(&url), Some(&state)).ok);
    assert_eq!(
        service.effective_client_id().as_deref(),
        Some("env-client-id")
    );
}

#[tokio::test]
async fn token_requests_fail_closed_without_credentials() {
    let kc = Arc::new(KeychainService::in_memory());
    let service = ChzzkOAuthService::new(OAuthConfig::chzzk(), kc.clone())
        .with_token_request(token_fn(|_| async {
            panic!("자격증명이 없는데 토큰 요청이 나갔다");
        }))
        .with_credential_store(OAuthCredentialStore::new(kc, "x", HashMap::new()));
    assert_eq!(
        service.exchange_code_for_tokens("c", "s").await,
        Err(AuthServiceError::MissingClientCredentials)
    );
    assert_eq!(
        service.refresh_tokens("r").await,
        Err(AuthServiceError::MissingClientCredentials)
    );
}

type Captured = Arc<Mutex<Option<BTreeMap<String, String>>>>;

fn capturing() -> (Captured, cview_auth::TokenRequestFn) {
    let captured: Captured = Arc::new(Mutex::new(None));
    let c = captured.clone();
    let f = token_fn(move |body| {
        let c = c.clone();
        async move {
            *c.lock().unwrap() = Some(body);
            Ok(OAuthTokens::bearer("a", Some("r".into()), 3600))
        }
    });
    (captured, f)
}

#[tokio::test]
async fn injected_config_wins() {
    let (captured, f) = capturing();
    let kc = Arc::new(KeychainService::in_memory());
    let config = OAuthConfig::new(
        "injected-id",
        "injected-secret",
        "http://localhost/callback",
        1,
    );
    let service = ChzzkOAuthService::new(config, kc.clone())
        .with_token_request(f)
        .with_credential_store(OAuthCredentialStore::new(kc, "x", HashMap::new()));
    assert!(service.has_client_credentials());
    service.refresh_tokens("old").await.unwrap();
    let body = captured.lock().unwrap().clone().unwrap();
    assert_eq!(body["clientId"], "injected-id");
    assert_eq!(body["clientSecret"], "injected-secret");
}

#[tokio::test]
async fn resolved_credentials_reach_token_request() {
    let (captured, f) = capturing();
    let service = service_with_env(&[
        ("CVIEW_OAUTH_CHZZK_ID", "env-client-id"),
        ("CVIEW_OAUTH_CHZZK_SECRET", "env-secret"),
    ])
    .with_token_request(f);
    service.refresh_tokens("old-refresh").await.unwrap();
    let body = captured.lock().unwrap().clone().unwrap();
    assert_eq!(body["clientId"], "env-client-id");
    assert_eq!(body["clientSecret"], "env-secret");
    assert_eq!(body["refreshToken"], "old-refresh");
    assert_eq!(body["grantType"], "refresh_token");
}

#[tokio::test]
async fn refresh_keeps_input_refresh_token_when_omitted() {
    let kc = keychain();
    let service = ChzzkOAuthService::new(common::fixture_config(), kc.clone()).with_token_request(
        token_fn(|_| async { Ok(OAuthTokens::bearer("new", Some(String::new()), 3600)) }),
    );
    let tokens = service.refresh_tokens("keep-me").await.unwrap();
    assert_eq!(tokens.refresh_token.as_deref(), Some("keep-me"));
    assert_eq!(
        service
            .load_stored_tokens()
            .unwrap()
            .refresh_token
            .as_deref(),
        Some("keep-me")
    );
    service.clear_stored_tokens();
    assert_eq!(service.load_stored_tokens(), None);
    assert_eq!(kc.load(TOKEN_KEYCHAIN_KEY).unwrap(), None);
}

#[tokio::test]
async fn pkce_verifier_sent_once_and_state_cleared() {
    let (captured, f) = capturing();
    let kc = Arc::new(KeychainService::in_memory());
    let service =
        ChzzkOAuthService::new(common::fixture_config().with_pkce(true), kc).with_token_request(f);
    let url = service.generate_auth_url().unwrap();
    assert_eq!(query(&url, "codeChallengeMethod").as_deref(), Some("S256"));
    let state = query(&url, "state").unwrap();
    service
        .exchange_code_for_tokens("code", &state)
        .await
        .unwrap();
    let body = captured.lock().unwrap().clone().unwrap();
    let verifier = body["codeVerifier"].clone();
    assert_eq!(
        cview_auth::PkceChallenge::compute_s256_challenge(&verifier),
        query(&url, "codeChallenge").unwrap()
    );
    assert_eq!(body["grantType"], "authorization_code");
    assert_eq!(service.saved_state(), None, "state/PKCE 는 1회용");
    service
        .exchange_code_for_tokens("code2", &state)
        .await
        .unwrap();
    assert!(!captured
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .contains_key("codeVerifier"));
}

// MARK: - 실제 HTTP 경로 (가짜 전송)

struct FakeTransport {
    responses: Mutex<Vec<Result<HttpResponse, TransportError>>>,
    requests: Mutex<Vec<HttpRequest>>,
}

impl FakeTransport {
    fn new(responses: Vec<Result<HttpResponse, TransportError>>) -> Arc<Self> {
        Arc::new(Self {
            responses: Mutex::new(responses),
            requests: Mutex::new(vec![]),
        })
    }
}

#[async_trait]
impl HttpTransport for FakeTransport {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
        self.requests.lock().unwrap().push(request);
        self.responses.lock().unwrap().remove(0)
    }
}

fn http_service(t: Arc<FakeTransport>) -> ChzzkOAuthService {
    ChzzkOAuthService::new(common::fixture_config(), keychain()).with_transport(t)
}

#[tokio::test]
async fn token_request_http_shape_and_success() {
    let t = FakeTransport::new(vec![Ok(HttpResponse::new(
        200,
        r#"{"code":200,"content":{"accessToken":"at","refreshToken":"rt","expiresIn":3600,"tokenType":"Bearer"}}"#,
    ))]);
    let service = http_service(t.clone());
    let tokens = service.refresh_tokens("old").await.unwrap();
    assert_eq!(tokens.access_token, "at");
    assert_eq!(tokens.refresh_token.as_deref(), Some("rt"));
    let req = t.requests.lock().unwrap()[0].clone();
    assert_eq!(
        req.url.as_str(),
        "https://openapi.chzzk.naver.com/auth/v1/token"
    );
    assert_eq!(req.method, cview_core::protocols::HttpMethod::Post);
    assert_eq!(req.header("Content-Type"), Some("application/json"));
    let body: serde_json::Value = serde_json::from_slice(req.body.as_ref().unwrap()).unwrap();
    assert_eq!(body["refreshToken"], "old");
    assert_eq!(body["clientSecret"], "fixture");
}

#[tokio::test]
async fn token_request_non_200_goes_through_status_mapping() {
    for (status, transient, terminal) in
        [(503, true, false), (401, false, true), (302, false, false)]
    {
        let t = FakeTransport::new(vec![Ok(HttpResponse::new(status, "maintenance"))]);
        let err = http_service(t).refresh_tokens("r").await.unwrap_err();
        assert_eq!(err, AuthServiceError::token_failure(status));
        assert_eq!(
            err.is_transient_refresh_failure(),
            transient,
            "HTTP {status}"
        );
        assert_eq!(err.is_terminal_refresh_failure(), terminal, "HTTP {status}");
    }
}

#[tokio::test]
async fn token_request_empty_content_is_rejection_and_offline_is_transport() {
    let t = FakeTransport::new(vec![
        Ok(HttpResponse::new(
            200,
            r#"{"code":401,"content":null,"message":"invalid_grant"}"#,
        )),
        Err(TransportError::NotConnected),
        Ok(HttpResponse::new(200, "<html>점검</html>")),
    ]);
    let service = http_service(t);
    assert_eq!(
        service.refresh_tokens("r").await,
        Err(AuthServiceError::TokenRejected {
            detail: "invalid_grant".into()
        })
    );
    let offline = service.refresh_tokens("r").await.unwrap_err();
    assert!(offline.is_transport_failure());
    let garbage = service.refresh_tokens("r").await.unwrap_err();
    assert!(matches!(garbage, AuthServiceError::Decoding(_)));
    assert!(!garbage.is_transient_refresh_failure() && !garbage.is_terminal_refresh_failure());
}

#[tokio::test]
async fn profile_and_validation_over_http() {
    let t = FakeTransport::new(vec![
        Ok(HttpResponse::new(
            200,
            r#"{"code":200,"content":{"channelId":"ch","nickname":"닉"}}"#,
        )),
        Ok(HttpResponse::new(401, "")),
        Ok(HttpResponse::new(200, "{}")),
        Ok(HttpResponse::new(500, "")),
    ]);
    let service = http_service(t.clone());
    let profile = service.fetch_user_profile("tok").await.unwrap();
    assert_eq!(profile.channel_id.as_deref(), Some("ch"));
    assert_eq!(profile.nickname.as_deref(), Some("닉"));
    let err = service.fetch_user_profile("tok").await.unwrap_err();
    assert_eq!(
        err.to_string(),
        "OAuth 인증 실패: Profile fetch failed: HTTP 401"
    );
    assert!(service.validate_token("tok").await);
    assert!(!service.validate_token("tok").await);
    let reqs = t.requests.lock().unwrap();
    assert_eq!(
        reqs[0].url.as_str(),
        "https://openapi.chzzk.naver.com/open/v1/users/me"
    );
    assert_eq!(reqs[0].header("Authorization"), Some("Bearer tok"));
    assert_eq!(
        reqs[2].url.as_str(),
        "https://api.chzzk.naver.com/service/v1/users/me"
    );
}
