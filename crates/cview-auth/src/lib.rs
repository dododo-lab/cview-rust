//! CViewAuth 포팅 (`Sources/CViewAuth`): 쿠키, PKCE, OAuth, 자격 증명 저장.
//!
//! | Swift | Rust |
//! |---|---|
//! | `AuthManager.swift`, `AuthManager+SessionRecovery.swift` | [`manager`] |
//! | `AuthSessionGeneration.swift` | [`generation`] |
//! | `ChzzkOAuthService.swift` | [`oauth_service`] |
//! | `CookieManager.swift` | [`cookie`] |
//! | `KeychainService.swift` | [`secret_store`] (`SecretStore` trait + keyring/파일/메모리) |
//! | `OAuthCSRF.swift` | [`csrf`] |
//! | `OAuthClientCredentials.swift` | [`credentials`] |
//! | `OAuthModels.swift` | [`oauth_models`] |
//! | `OAuthRefreshFailures.swift` | [`refresh_failures`] |
//! | `PKCEChallenge.swift` | [`pkce`] |
//! | `UnifiedLoginWebView.swift` (흐름 판단만) | [`login_flow`] |
//!
//! 웹뷰 UI 자체는 옮기지 않았다 — 웹뷰 셸은 [`login_flow::UnifiedLoginFlow`] 에 이벤트를 넘기고,
//! 쿠키는 [`cookie::CookieManager::ingest_webview_cookies`] 로, 영구 저장소 접근은
//! [`cookie::WebViewDataStore`] 구현으로 연결한다.

pub mod cookie;
pub mod credentials;
pub mod csrf;
pub mod generation;
pub mod login_flow;
pub mod manager;
pub mod oauth_models;
pub mod oauth_service;
pub mod pkce;
pub mod refresh_failures;
pub mod secret_store;

pub use cookie::{CookieJar, CookieManager, CookieRestore, StoredCookie, WebViewDataStore};
pub use credentials::{OAuthClientCredentials, OAuthCredentialStore};
pub use generation::{AuthSessionGeneration, Cancelled};
pub use login_flow::{LoginAction, LoginPhase, UnifiedLoginFlow};
pub use manager::{AuthManager, CookieCleanupFn};
pub use oauth_models::{OAuthConfig, OAuthTokens, OAuthUserProfile};
pub use oauth_service::{ChzzkOAuthService, ProfileRequestFn, TokenRequestFn, TOKEN_KEYCHAIN_KEY};
pub use pkce::PkceChallenge;
pub use refresh_failures::AuthServiceError;
pub use secret_store::{
    FileSecretStore, KeychainService, KeyringSecretStore, MemorySecretStore, SecretStore,
    StoreError,
};
