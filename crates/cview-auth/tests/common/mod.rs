//! 통합 테스트 공용 픽스처 — 실제 네트워크·OS 키체인 없이 동작한다.
#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cview_auth::{
    AuthManager, AuthServiceError, ChzzkOAuthService, CookieJar, CookieManager, KeychainService,
    OAuthConfig, OAuthCredentialStore, OAuthTokens, OAuthUserProfile, ProfileRequestFn,
    SecretStore, StoreError, TokenRequestFn,
};
use futures_util::FutureExt;

/// 인메모리 backend — 실패 주입과 삭제 기록을 지원한다.
#[derive(Default)]
pub struct RecordingStore {
    pub values: Mutex<HashMap<String, Vec<u8>>>,
    pub deny_load: Mutex<bool>,
    pub deny_save: Mutex<bool>,
    /// 이 키의 읽기만 실패시킨다(잠긴 키체인 흉내).
    pub failing_key: Mutex<Option<String>>,
    pub writes: Mutex<usize>,
    pub deleted_keys: Mutex<Vec<String>>,
    pub delete_all_calls: Mutex<usize>,
}

impl RecordingStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
    pub fn raw(&self, key: &str) -> Option<Vec<u8>> {
        self.values.lock().unwrap().get(key).cloned()
    }
}

impl SecretStore for RecordingStore {
    fn save(&self, key: &str, data: &[u8]) -> Result<(), StoreError> {
        if *self.deny_save.lock().unwrap() {
            return Err(StoreError::Backend("denied".into()));
        }
        self.values
            .lock()
            .unwrap()
            .insert(key.to_string(), data.to_vec());
        *self.writes.lock().unwrap() += 1;
        Ok(())
    }
    fn load(&self, key: &str) -> Result<Option<Vec<u8>>, StoreError> {
        if *self.deny_load.lock().unwrap()
            || self.failing_key.lock().unwrap().as_deref() == Some(key)
        {
            return Err(StoreError::Backend("denied".into()));
        }
        Ok(self.raw(key))
    }
    fn delete(&self, key: &str) -> Result<(), StoreError> {
        self.deleted_keys.lock().unwrap().push(key.to_string());
        self.values.lock().unwrap().remove(key);
        Ok(())
    }
    fn delete_all(&self) -> Result<(), StoreError> {
        *self.delete_all_calls.lock().unwrap() += 1;
        self.values.lock().unwrap().clear();
        Ok(())
    }
}

/// 시스템 저장소 모드 KeychainService (양쪽 backend 모두 메모리).
pub fn keychain_with(store: Arc<RecordingStore>) -> Arc<KeychainService> {
    Arc::new(KeychainService::with_backends(
        RecordingStore::new(),
        store,
        true,
    ))
}

pub fn keychain() -> Arc<KeychainService> {
    keychain_with(RecordingStore::new())
}

pub fn fixture_config() -> OAuthConfig {
    OAuthConfig::new("fixture", "fixture", "http://localhost/callback", 1)
}

pub fn token_fn<F, Fut>(f: F) -> TokenRequestFn
where
    F: Fn(BTreeMap<String, String>) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<OAuthTokens, AuthServiceError>> + Send + 'static,
{
    Arc::new(move |body| f(body).boxed())
}

pub fn profile_fn<F, Fut>(f: F) -> ProfileRequestFn
where
    F: Fn(String) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<OAuthUserProfile, AuthServiceError>> + Send + 'static,
{
    Arc::new(move |token| f(token).boxed())
}

pub struct Fixture {
    pub manager: AuthManager,
    pub oauth: Arc<ChzzkOAuthService>,
    pub cookies: Arc<CookieManager>,
    pub jar: Arc<CookieJar>,
}

/// 매니저 픽스처. `credential_env` 가 `Some` 이면 그 환경으로만 자격 증명을 조달한다.
pub fn fixture(
    config: OAuthConfig,
    keychain: Arc<KeychainService>,
    token: TokenRequestFn,
    profile: Option<ProfileRequestFn>,
    credential_env: Option<HashMap<String, String>>,
) -> Fixture {
    let mut oauth = ChzzkOAuthService::new(config, keychain.clone()).with_token_request(token);
    if let Some(p) = profile {
        oauth = oauth.with_profile_request(p);
    }
    if let Some(env) = credential_env {
        oauth = oauth.with_credential_store(OAuthCredentialStore::new(keychain.clone(), "", env));
    }
    let oauth = Arc::new(oauth);
    let jar = Arc::new(CookieJar::new());
    let cookies = Arc::new(CookieManager::new(keychain, jar.clone()));
    let manager = AuthManager::new(
        cookies.clone(),
        oauth.clone(),
        Some(Arc::new(|| async {}.boxed())),
    );
    Fixture {
        manager,
        oauth,
        cookies,
        jar,
    }
}

/// 분류되지 않는 실패 (Swift 테스트의 `Failure.rejected`).
pub fn unclassified() -> AuthServiceError {
    AuthServiceError::Auth(cview_core::error::AuthError::OauthFailed("rejected".into()))
}

/// 조건이 참이 될 때까지 기다린다(최대 5초).
pub async fn wait_until(mut cond: impl FnMut() -> bool) {
    for _ in 0..5_000 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    panic!("condition not met");
}

pub fn logged_in(user: &str) -> cview_core::protocols::AuthState {
    cview_core::protocols::AuthState::LoggedIn {
        user_id: user.into(),
    }
}
