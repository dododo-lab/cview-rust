//! `Sources/CViewAuth/OAuthClientCredentials.swift` 포팅 — 치지직 OAuth 클라이언트 자격 증명 조달.
//!
//! 비밀(client secret)을 앱에 동봉하지 않는다(RFC 8252 §8.5). 실행 환경이 공급한다:
//! 1. `CVIEW_OAUTH_CHZZK_ID` / `CVIEW_OAUTH_CHZZK_SECRET` 환경변수 — 개발·CI 용 읽기 전용 오버라이드.
//! 2. 보안 저장소(`oauth_client_credentials`) — 사용자가 직접 등록한 애플리케이션 값.
//!
//! 둘 다 없으면 OAuth 단계는 조용히 비활성되고 쿠키 로그인만 남는다.
//! ⚠ 번들/리소스 조회를 다시 추가하지 말 것.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::refresh_failures::AuthServiceError;
use crate::secret_store::{KeychainService, StoreError};

/// 치지직 OAuth 클라이언트 자격 증명 한 쌍. `client_id` 는 공개 값, `client_secret` 만 비밀이다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthClientCredentials {
    pub client_id: String,
    pub client_secret: String,
}

impl OAuthClientCredentials {
    /// 템플릿 placeholder — 실제 값으로 오인하지 않는다.
    pub const PLACEHOLDER: &'static str = "REPLACE_ME";

    pub fn new(client_id: impl Into<String>, client_secret: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: client_secret.into(),
        }
    }

    /// 둘 다 있어야 토큰 교환에 쓸 수 있다.
    pub fn is_usable(&self) -> bool {
        !self.client_id.is_empty() && !self.client_secret.is_empty()
    }

    /// 붙여넣기에 묻어 오는 공백·개행·따옴표를 걷어낸다.
    pub fn sanitize(raw: &str) -> String {
        raw.trim()
            .trim_matches(|c| c == '"' || c == '\'')
            .to_string()
    }
}

/// 자격 증명 조달기 — 환경변수 → 보안 저장소 순.
#[derive(Debug, Clone)]
pub struct OAuthCredentialStore {
    keychain: Arc<KeychainService>,
    environment: HashMap<String, String>,
    /// 환경변수에 secret 만 있을 때 쓸 기본 clientId.
    default_client_id: String,
}

impl OAuthCredentialStore {
    /// 저장소 항목 키. `oauth_tokens`(발급 토큰)과 다르다.
    pub const KEYCHAIN_KEY: &'static str = "oauth_client_credentials";
    pub const ID_ENVIRONMENT_KEY: &'static str = "CVIEW_OAUTH_CHZZK_ID";
    pub const SECRET_ENVIRONMENT_KEY: &'static str = "CVIEW_OAUTH_CHZZK_SECRET";

    pub fn new(
        keychain: Arc<KeychainService>,
        default_client_id: impl Into<String>,
        environment: HashMap<String, String>,
    ) -> Self {
        Self {
            keychain,
            environment,
            default_client_id: default_client_id.into(),
        }
    }

    /// 프로세스 환경변수를 쓴다.
    pub fn from_process_env(
        keychain: Arc<KeychainService>,
        default_client_id: impl Into<String>,
    ) -> Self {
        Self::new(keychain, default_client_id, std::env::vars().collect())
    }

    /// 지금 쓸 수 있는 자격 증명. 없으면 `None` — 호출자는 OAuth 단계를 건너뛴다.
    pub fn resolve(&self) -> Option<OAuthClientCredentials> {
        if let Some(env) = self.environment_credentials() {
            return Some(env);
        }
        self.stored_credentials().filter(|c| c.is_usable())
    }

    /// 토큰 갱신용 조달 — 저장소 **읽기 실패**를 '없음' 과 가른다.
    /// 없으면 `Ok(None)`(최종), 읽지 못하면 [`AuthServiceError::CredentialsUnavailable`](일시).
    /// 항목이 깨져 해석되지 않으면 '없음' 으로 본다.
    pub fn resolve_for_refresh(&self) -> Result<Option<OAuthClientCredentials>, AuthServiceError> {
        if let Some(env) = self.environment_credentials() {
            return Ok(Some(env));
        }
        let stored = match self
            .keychain
            .load_codable::<OAuthClientCredentials>(Self::KEYCHAIN_KEY)
        {
            Ok(v) => v,
            Err(StoreError::Decoding(_)) => None,
            Err(e) => {
                tracing::warn!("OAuth 자격증명 조회 실패 — 토큰은 남기고 다시 시도한다: {e}");
                return Err(AuthServiceError::CredentialsUnavailable {
                    reason: e.to_string(),
                });
            }
        };
        Ok(stored.filter(|c| c.is_usable()))
    }

    /// 환경변수 경로만.
    pub fn environment_credentials(&self) -> Option<OAuthClientCredentials> {
        let secret =
            OAuthClientCredentials::sanitize(self.environment.get(Self::SECRET_ENVIRONMENT_KEY)?);
        if secret.is_empty() || secret == OAuthClientCredentials::PLACEHOLDER {
            return None;
        }
        let raw_id = self
            .environment
            .get(Self::ID_ENVIRONMENT_KEY)
            .map(|s| OAuthClientCredentials::sanitize(s))
            .unwrap_or_default();
        let client_id = if raw_id.is_empty() {
            self.default_client_id.clone()
        } else {
            raw_id
        };
        if client_id.is_empty() {
            return None;
        }
        Some(OAuthClientCredentials::new(client_id, secret))
    }

    /// 저장소에 저장된 자격 증명. 읽기 실패는 기록하고 `None`.
    pub fn stored_credentials(&self) -> Option<OAuthClientCredentials> {
        match self.keychain.load_codable(Self::KEYCHAIN_KEY) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("OAuth 자격증명 조회 실패 — OAuth 단계를 건너뛴다: {e}");
                None
            }
        }
    }

    /// 사용자 입력 자격 증명을 저장한다. 비었거나 placeholder 면
    /// [`AuthServiceError::MissingClientCredentials`].
    pub fn save(&self, client_id: &str, client_secret: &str) -> Result<(), AuthServiceError> {
        let credentials = OAuthClientCredentials::new(
            OAuthClientCredentials::sanitize(client_id),
            OAuthClientCredentials::sanitize(client_secret),
        );
        if !credentials.is_usable()
            || credentials.client_secret == OAuthClientCredentials::PLACEHOLDER
        {
            return Err(AuthServiceError::MissingClientCredentials);
        }
        self.keychain
            .save_codable(Self::KEYCHAIN_KEY, &credentials)?;
        Ok(())
    }

    /// 저장된 자격 증명을 지운다.
    pub fn clear(&self) -> Result<(), AuthServiceError> {
        self.keychain.delete(Self::KEYCHAIN_KEY)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAuthTests/OAuthClientCredentialsTests.swift` (조달 규칙 부분).
    //! 번들·`Package.swift`·`build_release.sh` 소스 계약 검사는 Swift 빌드 산출물 전용이라 옮기지 않았다.
    use super::*;

    fn store(env: &[(&str, &str)]) -> OAuthCredentialStore {
        OAuthCredentialStore::new(
            Arc::new(KeychainService::in_memory()),
            "default-id",
            env.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }

    #[test]
    fn environment_resolution() {
        let only_secret = store(&[("CVIEW_OAUTH_CHZZK_SECRET", "s3cr3t")])
            .environment_credentials()
            .unwrap();
        assert_eq!(only_secret.client_id, "default-id");
        assert_eq!(only_secret.client_secret, "s3cr3t");

        let both = store(&[
            ("CVIEW_OAUTH_CHZZK_ID", "my-id"),
            ("CVIEW_OAUTH_CHZZK_SECRET", "s3cr3t"),
        ])
        .environment_credentials();
        assert_eq!(both, Some(OAuthClientCredentials::new("my-id", "s3cr3t")));

        assert_eq!(
            store(&[("CVIEW_OAUTH_CHZZK_SECRET", "REPLACE_ME")]).environment_credentials(),
            None
        );
        assert_eq!(
            store(&[("CVIEW_OAUTH_CHZZK_SECRET", "   ")]).environment_credentials(),
            None
        );
        assert_eq!(store(&[]).environment_credentials(), None);
    }

    #[test]
    fn sanitizes_pasted_input() {
        assert_eq!(OAuthClientCredentials::sanitize("  abc \n"), "abc");
        assert_eq!(OAuthClientCredentials::sanitize("\"abc\""), "abc");
        assert_eq!(OAuthClientCredentials::sanitize("'abc'"), "abc");
    }

    #[test]
    fn both_halves_required() {
        assert!(!OAuthClientCredentials::new("", "s").is_usable());
        assert!(!OAuthClientCredentials::new("i", "").is_usable());
        assert!(OAuthClientCredentials::new("i", "s").is_usable());
    }

    #[test]
    fn save_validates_and_round_trips() {
        let s = store(&[]);
        assert_eq!(
            s.save("id", "REPLACE_ME"),
            Err(AuthServiceError::MissingClientCredentials)
        );
        assert_eq!(
            s.save(" ", "x"),
            Err(AuthServiceError::MissingClientCredentials)
        );
        s.save(" \"id\" ", "'sec'\n").unwrap();
        assert_eq!(s.resolve(), Some(OAuthClientCredentials::new("id", "sec")));
        assert_eq!(
            s.resolve_for_refresh(),
            Ok(Some(OAuthClientCredentials::new("id", "sec")))
        );
        s.clear().unwrap();
        assert_eq!(s.resolve(), None);
        assert_eq!(s.resolve_for_refresh(), Ok(None));
    }

    #[test]
    fn corrupt_stored_value_is_missing_for_refresh() {
        let keychain = Arc::new(KeychainService::in_memory());
        keychain
            .save(OAuthCredentialStore::KEYCHAIN_KEY, b"not json")
            .unwrap();
        let s = OAuthCredentialStore::new(keychain, "", HashMap::new());
        assert_eq!(s.resolve_for_refresh(), Ok(None));
    }
}
