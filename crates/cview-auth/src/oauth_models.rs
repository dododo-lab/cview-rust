//! `Sources/CViewAuth/OAuthModels.swift` 포팅 — 치지직 OAuth 설정·토큰·응답 모델.
//!
//! `OAuthConfig::chzzk()` 의 `client_secret` 은 **비어 있다** — 실제 값은
//! [`crate::credentials::OAuthCredentialStore`] 가 실행 시점에 환경변수·보안 저장소에서 조달한다.
//! ⚠ 여기에 secret 리터럴이나 번들 조회를 다시 넣지 말 것.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::credentials::OAuthClientCredentials;
use crate::pkce::PkceChallenge;

/// 치지직 OAuth 설정.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub loopback_port: u16,
    /// 인가 URL.
    pub auth_base_url: String,
    /// 토큰 엔드포인트.
    pub token_url: String,
    /// PKCE 사용 여부. 치지직이 PKCE 를 문서화하지 않아 기본 false (env `CVIEW_OAUTH_USE_PKCE=1`).
    pub use_pkce: bool,
}

/// 기본 인가 URL.
pub const DEFAULT_AUTH_BASE_URL: &str = "https://chzzk.naver.com/account-interlock";
/// 기본 토큰 엔드포인트.
pub const DEFAULT_TOKEN_URL: &str = "https://openapi.chzzk.naver.com/auth/v1/token";

impl OAuthConfig {
    /// 기본 엔드포인트·PKCE 꺼짐으로 만든다.
    pub fn new(
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        redirect_uri: impl Into<String>,
        loopback_port: u16,
    ) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            redirect_uri: redirect_uri.into(),
            loopback_port,
            auth_base_url: DEFAULT_AUTH_BASE_URL.into(),
            token_url: DEFAULT_TOKEN_URL.into(),
            use_pkce: false,
        }
    }

    pub fn with_auth_base_url(mut self, url: impl Into<String>) -> Self {
        self.auth_base_url = url.into();
        self
    }

    pub fn with_token_url(mut self, url: impl Into<String>) -> Self {
        self.token_url = url.into();
        self
    }

    pub fn with_pkce(mut self, use_pkce: bool) -> Self {
        self.use_pkce = use_pkce;
        self
    }

    /// CView 치지직 OAuth 설정. `client_id` 는 공개 값이라 기본값을 남기고 secret 은 비운다.
    pub fn chzzk() -> Self {
        Self::new(
            "a6e68dba-8cb1-4dd1-9695-a996200765f6",
            "",
            "http://localhost:52735/callback",
            52735,
        )
        .with_pkce(std::env::var("CVIEW_OAUTH_USE_PKCE").as_deref() == Ok("1"))
    }

    /// 조달한 자격 증명을 입힌 사본. 나머지 필드는 보존한다.
    pub fn applying(&self, credentials: &OAuthClientCredentials) -> Self {
        Self {
            client_id: credentials.client_id.clone(),
            client_secret: credentials.client_secret.clone(),
            ..self.clone()
        }
    }

    /// 주입된 설정에 쓸 수 있는 자격 증명이 이미 있는가.
    pub(crate) fn has_credentials(&self) -> bool {
        !self.client_id.is_empty() && !self.client_secret.is_empty()
    }

    /// OAuth 인가 URL (`clientId`·`redirectUri`·`state`, PKCE 면 `codeChallenge`·`codeChallengeMethod`).
    ///
    /// 기존 쿼리는 버린다. 값은 퍼센트 인코딩되며 `+` 는 남기지 않는다(폼 디코딩의 공백 모호성).
    pub fn build_auth_url(&self, state: &str, pkce: Option<&PkceChallenge>) -> Option<Url> {
        let mut url = Url::parse(&self.auth_base_url).ok()?;
        url.set_query(None);
        url.set_fragment(None);
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("clientId", &self.client_id);
            q.append_pair("redirectUri", &self.redirect_uri);
            q.append_pair("state", state);
            if let Some(p) = pkce {
                q.append_pair("codeChallenge", &p.code_challenge);
                q.append_pair("codeChallengeMethod", &p.code_challenge_method);
            }
        }
        // form-urlencoded 는 공백을 `+` 로 쓴다(값의 `+` 는 이미 `%2B`). 남은 `+` 는 공백이다.
        if let Some(q) = url.query().map(|q| q.replace('+', "%20")) {
            url.set_query(Some(&q));
        }
        Some(url)
    }
}

/// OAuth 액세스/리프레시 토큰.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthTokens {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// 초.
    pub expires_in: i64,
    pub token_type: String,
    pub created_at: DateTime<Utc>,
}

impl OAuthTokens {
    /// 지금 발급된 토큰.
    pub fn new(
        access_token: impl Into<String>,
        refresh_token: Option<String>,
        expires_in: i64,
        token_type: impl Into<String>,
    ) -> Self {
        Self {
            access_token: access_token.into(),
            refresh_token,
            expires_in,
            token_type: token_type.into(),
            created_at: Utc::now(),
        }
    }

    /// `token_type` = `"Bearer"`.
    pub fn bearer(
        access_token: impl Into<String>,
        refresh_token: Option<String>,
        expires_in: i64,
    ) -> Self {
        Self::new(access_token, refresh_token, expires_in, "Bearer")
    }

    /// 만료 시각.
    pub fn expires_at(&self) -> DateTime<Utc> {
        self.created_at + Duration::seconds(self.expires_in)
    }

    /// 만료되었는가 (30초 버퍼).
    pub fn is_expired(&self) -> bool {
        Utc::now() + Duration::seconds(30) > self.expires_at()
    }
}

/// 토큰 교환 응답 envelope.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OAuthTokenResponse {
    #[serde(default)]
    pub code: Option<i64>,
    #[serde(default)]
    pub content: Option<TokenContent>,
    #[serde(default)]
    pub message: Option<String>,
}

/// 토큰 응답 content.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenContent {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    pub expires_in: i64,
    pub token_type: String,
}

/// 프로필 응답 envelope.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OAuthProfileResponse {
    #[serde(default)]
    pub code: Option<i64>,
    #[serde(default)]
    pub content: Option<OAuthUserProfile>,
    #[serde(default)]
    pub message: Option<String>,
}

/// OAuth 사용자 프로필.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthUserProfile {
    #[serde(default)]
    pub channel_id: Option<String>,
    #[serde(default)]
    pub channel_name: Option<String>,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub profile_image_url: Option<String>,
    #[serde(default)]
    pub verified_mark: Option<bool>,
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAuthTests/OAuthModelsTests.swift`
    use super::*;
    use crate::csrf::query_value;

    #[test]
    fn chzzk_preset() {
        let cfg = OAuthConfig::chzzk();
        assert_eq!(cfg.client_id, "a6e68dba-8cb1-4dd1-9695-a996200765f6");
        assert_eq!(cfg.loopback_port, 52735);
        assert_eq!(cfg.redirect_uri, "http://localhost:52735/callback");
        assert_eq!(
            cfg.auth_base_url,
            "https://chzzk.naver.com/account-interlock"
        );
        assert_eq!(
            cfg.token_url,
            "https://openapi.chzzk.naver.com/auth/v1/token"
        );
        assert_ne!(cfg.client_secret, "REPLACE_ME");
    }

    #[test]
    fn build_auth_url() {
        let cfg = OAuthConfig::new("test-client", "secret", "http://localhost:9999/cb", 9999);
        let url = cfg.build_auth_url("abc123", None).unwrap();
        let s = url.as_str();
        assert!(s.contains("clientId=test-client"));
        assert!(s.contains("state=abc123"));
        assert!(s.contains("redirectUri="));
        assert!(s.starts_with("https://chzzk.naver.com/account-interlock"));
    }

    #[test]
    fn build_auth_url_custom_base() {
        let cfg = OAuthConfig::new("c1", "s1", "http://localhost:8080/auth", 8080)
            .with_auth_base_url("https://example.com/oauth");
        let url = cfg.build_auth_url("s", None).unwrap();
        assert!(url.as_str().starts_with("https://example.com/oauth"));
    }

    #[test]
    fn build_auth_url_special_values_round_trip() {
        let redirect = "http://localhost:9999/cb?first=a+b&next=/home#done";
        let state = "a+b=c&state=attacker#한글 x";
        let cfg = OAuthConfig::new("client&state=evil", "unused", redirect, 9999);
        let url = cfg.build_auth_url(state, None).unwrap();
        assert_eq!(url.query_pairs().count(), 3);
        assert_eq!(query_value(&url, "state").as_deref(), Some(state));
        assert_eq!(query_value(&url, "redirectUri").as_deref(), Some(redirect));
        assert_eq!(
            query_value(&url, "clientId").as_deref(),
            Some("client&state=evil")
        );
        assert!(!url.as_str().contains('+'));
        assert!(url.fragment().is_none());
    }

    #[test]
    fn build_auth_url_with_pkce() {
        let cfg = OAuthConfig::new("c", "s", "http://localhost/cb", 1);
        let pkce = PkceChallenge::new("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");
        let url = cfg.build_auth_url("st", Some(&pkce)).unwrap();
        assert_eq!(
            query_value(&url, "codeChallenge").as_deref(),
            Some("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM")
        );
        assert_eq!(
            query_value(&url, "codeChallengeMethod").as_deref(),
            Some("S256")
        );
    }

    #[test]
    fn custom_init_defaults() {
        let cfg = OAuthConfig::new("id", "sec", "http://localhost:1234/cb", 1234);
        assert_eq!(cfg.auth_base_url, DEFAULT_AUTH_BASE_URL);
        assert_eq!(cfg.token_url, DEFAULT_TOKEN_URL);
        assert!(!cfg.use_pkce);
    }

    #[test]
    fn expires_at_computed() {
        let t = OAuthTokens::bearer("at", Some("rt".into()), 3600);
        assert_eq!((t.expires_at() - t.created_at).num_seconds(), 3600);
    }

    #[test]
    fn expiration_rules() {
        assert!(!OAuthTokens::bearer("fresh", None, 3600).is_expired());
        assert!(OAuthTokens::bearer("zero", None, 0).is_expired());
        assert!(
            OAuthTokens::bearer("edge", None, 29).is_expired(),
            "30초 버퍼"
        );
        let t = OAuthTokens::bearer("a", None, 100);
        assert_eq!(t.token_type, "Bearer");
        assert_eq!(t.refresh_token, None);
    }

    #[test]
    fn tokens_serde_round_trip() {
        let original = OAuthTokens::bearer("access-xyz", Some("refresh-abc".into()), 7200);
        let json = serde_json::to_string(&original).unwrap();
        let decoded: OAuthTokens = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, original);

        let none = OAuthTokens::bearer("a", None, 60);
        let decoded: OAuthTokens =
            serde_json::from_str(&serde_json::to_string(&none).unwrap()).unwrap();
        assert_eq!(decoded.refresh_token, None);
        assert_eq!(decoded.access_token, "a");
    }

    #[test]
    fn keychain_round_trip() {
        let svc = crate::secret_store::KeychainService::in_memory();
        let tokens = OAuthTokens::bearer("kc-access", Some("kc-refresh".into()), 1800);
        svc.save_codable("k", &tokens).unwrap();
        let loaded: OAuthTokens = svc.load_codable("k").unwrap().unwrap();
        assert_eq!(loaded, tokens);
    }

    #[test]
    fn token_response_success() {
        let json = r#"{"code":200,"content":{"accessToken":"at-123","refreshToken":"rt-456","expiresIn":3600,"tokenType":"Bearer"},"message":null}"#;
        let r: OAuthTokenResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.code, Some(200));
        let c = r.content.unwrap();
        assert_eq!(c.access_token, "at-123");
        assert_eq!(c.refresh_token.as_deref(), Some("rt-456"));
        assert_eq!(c.expires_in, 3600);
        assert_eq!(c.token_type, "Bearer");
    }

    #[test]
    fn token_response_error() {
        let r: OAuthTokenResponse =
            serde_json::from_str(r#"{"code":401,"content":null,"message":"Unauthorized"}"#)
                .unwrap();
        assert_eq!(r.code, Some(401));
        assert!(r.content.is_none());
        assert_eq!(r.message.as_deref(), Some("Unauthorized"));
    }

    #[test]
    fn token_response_without_refresh_token() {
        let r: OAuthTokenResponse = serde_json::from_str(
            r#"{"code":200,"content":{"accessToken":"at","expiresIn":600,"tokenType":"Bearer"}}"#,
        )
        .unwrap();
        let c = r.content.unwrap();
        assert_eq!(c.refresh_token, None);
        assert_eq!(c.access_token, "at");
    }

    #[test]
    fn profile_response() {
        let r: OAuthProfileResponse = serde_json::from_str(
            r#"{"code":200,"content":{"channelId":"ch-001","channelName":"테스트채널","nickname":"테스터","profileImageUrl":"https://example.com/img.png","verifiedMark":true}}"#,
        )
        .unwrap();
        assert_eq!(r.code, Some(200));
        let c = r.content.unwrap();
        assert_eq!(c.channel_id.as_deref(), Some("ch-001"));
        assert_eq!(c.nickname.as_deref(), Some("테스터"));
        assert_eq!(c.verified_mark, Some(true));
    }

    #[test]
    fn partial_profile_response() {
        let r: OAuthProfileResponse = serde_json::from_str(
            r#"{"code":200,"content":{"channelId":"ch-002","channelName":null,"nickname":null,"profileImageUrl":null,"verifiedMark":null}}"#,
        )
        .unwrap();
        let c = r.content.unwrap();
        assert_eq!(c.channel_id.as_deref(), Some("ch-002"));
        assert_eq!(c.nickname, None);
        assert_eq!(c.verified_mark, None);
    }
}
