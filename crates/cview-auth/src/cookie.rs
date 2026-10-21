//! `Sources/CViewAuth/CookieManager.swift` 포팅 — 네이버 인증 쿠키(`NID_AUT`/`NID_SES`) 관리.
//!
//! - `HTTPCookieStorage` → [`CookieJar`] (스레드 안전 인메모리 저장소, 만료 쿠키는 보이지 않는다).
//! - `WKWebsiteDataStore` → [`WebViewDataStore`] trait. 웹뷰 셸(Tauri 등)이 구현해 넣는다.
//!   넣지 않으면 웹뷰 관련 동작은 no-op 이다.
//! - 웹뷰 로그인 흐름(`UnifiedLoginWebView.syncCookiesToShared`)이 쓰는 쿠키 주입은
//!   [`CookieManager::ingest_webview_cookies`] 로 노출했다.
//!
//! 저장 형식은 원본과 같다 — `[{"name","value","domain","path","expires"(ISO8601)}]` JSON 을
//! 보안 저장소 `naver_auth_cookies` 키에.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, SecondsFormat, Utc};
use cview_core::protocols::HttpCookie;
use cview_core::utils::naver_cookie_policy;

use crate::generation::AuthSessionGeneration;
use crate::secret_store::{KeychainService, StoreError};

/// 네이버 인증에 필요한 쿠키 이름.
pub const REQUIRED_COOKIE_NAMES: [&str; 2] = ["NID_AUT", "NID_SES"];
/// 보안 저장소 키.
pub const COOKIE_KEYCHAIN_KEY: &str = "naver_auth_cookies";

/// 저장소에 든 쿠키 한 개 (Swift `HTTPCookie` 의 필요한 속성).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StoredCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    /// `None` 이면 세션 쿠키.
    pub expires: Option<DateTime<Utc>>,
}

impl StoredCookie {
    pub fn new(
        name: impl Into<String>,
        value: impl Into<String>,
        domain: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            domain: domain.into(),
            path: "/".into(),
            expires: None,
        }
    }

    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }

    pub fn with_expires(mut self, expires: DateTime<Utc>) -> Self {
        self.expires = Some(expires);
        self
    }

    pub fn is_expired(&self) -> bool {
        self.expires.is_some_and(|e| e <= Utc::now())
    }

    /// 비어 있지 않은 `NID_AUT`/`NID_SES` 이고 NAVER 도메인인가.
    pub fn is_naver_auth_cookie(&self) -> bool {
        REQUIRED_COOKIE_NAMES.contains(&self.name.as_str())
            && naver_cookie_policy::matches_domain(&self.domain)
            && !self.value.is_empty()
    }

    /// API 요청에 붙일 형태.
    pub fn to_http_cookie(&self) -> HttpCookie {
        HttpCookie {
            name: self.name.clone(),
            value: self.value.clone(),
            domain: Some(self.domain.clone()),
            path: Some(self.path.clone()),
        }
    }

    fn same_identity(&self, other: &StoredCookie) -> bool {
        self.name == other.name
            && self.domain.eq_ignore_ascii_case(&other.domain)
            && self.path == other.path
    }
}

/// 쿠키 목록이 두 인증 쿠키를 모두(비어 있지 않게, NAVER 도메인으로) 갖는가.
pub fn contains_required_auth_cookies(cookies: &[StoredCookie]) -> bool {
    REQUIRED_COOKIE_NAMES.iter().all(|name| {
        cookies
            .iter()
            .any(|c| c.name == *name && c.is_naver_auth_cookie())
    })
}

// MARK: - CookieJar

/// 스레드 안전 쿠키 저장소 (Swift `HTTPCookieStorage`). 이름·도메인·경로가 같으면 덮어쓴다.
#[derive(Debug, Default)]
pub struct CookieJar {
    cookies: Mutex<Vec<StoredCookie>>,
}

impl CookieJar {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<StoredCookie>> {
        self.cookies.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 쿠키를 넣는다. 이미 만료된 쿠키는 같은 쿠키를 지우는 효과만 낸다.
    pub fn set_cookie(&self, cookie: StoredCookie) {
        let mut list = self.lock();
        list.retain(|c| !c.same_identity(&cookie));
        if !cookie.is_expired() {
            list.push(cookie);
        }
    }

    pub fn delete_cookie(&self, cookie: &StoredCookie) {
        self.lock().retain(|c| !c.same_identity(cookie));
    }

    /// 만료되지 않은 모든 쿠키.
    pub fn cookies(&self) -> Vec<StoredCookie> {
        let mut list = self.lock();
        list.retain(|c| !c.is_expired());
        list.clone()
    }

    pub fn clear(&self) {
        self.lock().clear();
    }
}

// MARK: - WebView store

/// 웹뷰 영구 저장소 (Swift `WKWebsiteDataStore.default()`). 웹뷰 셸이 구현한다.
#[async_trait]
pub trait WebViewDataStore: Send + Sync {
    /// 웹뷰 쿠키 저장소의 모든 쿠키.
    async fn all_cookies(&self) -> Vec<StoredCookie>;
    async fn delete_cookie(&self, cookie: &StoredCookie);
    /// 쿠키·localStorage·캐시 등 웹사이트 데이터 전체 삭제.
    async fn remove_all_website_data(&self);
}

// MARK: - CookieManager

/// 쿠키 관리자.
pub struct CookieManager {
    keychain: Arc<KeychainService>,
    jar: Arc<CookieJar>,
    session_generation: AuthSessionGeneration,
    webview: Option<Arc<dyn WebViewDataStore>>,
}

impl std::fmt::Debug for CookieManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CookieManager")
            .field("jar", &self.jar)
            .finish_non_exhaustive()
    }
}

/// 키체인 쿠키 복원 결과.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CookieRestore {
    /// 인증 쿠키를 되살렸다.
    Restored,
    /// 저장된 쿠키가 없다(또는 해석할 수 없다).
    Empty,
    /// 키체인을 읽지 못했다(잠김·거부) — 저장된 값은 그대로다.
    ReadFailed,
}

impl CookieManager {
    pub fn new(keychain: Arc<KeychainService>, jar: Arc<CookieJar>) -> Self {
        Self {
            keychain,
            jar,
            session_generation: AuthSessionGeneration::new(),
            webview: None,
        }
    }

    /// 웹뷰 저장소를 연결한다.
    pub fn with_webview_store(mut self, store: Arc<dyn WebViewDataStore>) -> Self {
        self.webview = Some(store);
        self
    }

    pub fn jar(&self) -> &Arc<CookieJar> {
        &self.jar
    }

    /// 현재 유효한 인증 쿠키 (NAVER 도메인의 비어 있지 않은 `NID_AUT`/`NID_SES`).
    ///
    /// ⚠ 메모이즈하지 말 것 — 응답 `Set-Cookie` 나 웹뷰 주입이 저장소를 직접 바꾼다.
    pub fn auth_cookies(&self) -> Vec<StoredCookie> {
        self.jar
            .cookies()
            .into_iter()
            .filter(StoredCookie::is_naver_auth_cookie)
            .collect()
    }

    /// 요청용 인증 쿠키.
    pub fn auth_http_cookies(&self) -> Vec<HttpCookie> {
        self.auth_cookies()
            .iter()
            .map(StoredCookie::to_http_cookie)
            .collect()
    }

    /// 두 인증 쿠키를 모두 갖는가.
    pub fn has_cookies(&self) -> bool {
        contains_required_auth_cookies(&self.auth_cookies())
    }

    /// 인증 쿠키를 보안 저장소에 저장한다. 쿠키가 없으면 아무것도 하지 않는다.
    pub fn save_cookies_to_keychain(&self) -> Result<(), StoreError> {
        self.save_cookies_in_generation(self.session_generation.current())
    }

    fn save_cookies_in_generation(&self, generation: u64) -> Result<(), StoreError> {
        self.session_generation.check(generation)?;
        let cookies = self.auth_cookies();
        if cookies.is_empty() {
            return Ok(());
        }
        let data = encode_cookies(&cookies)?;
        self.keychain.save_session_data(
            &data,
            COOKIE_KEYCHAIN_KEY,
            &self.session_generation,
            generation,
        )?;
        tracing::info!("Cookies saved to keychain ({} cookies)", cookies.len());
        Ok(())
    }

    /// 보안 저장소에서 쿠키를 복원한다. 두 인증 쿠키가 모두 있으면 true.
    pub fn restore_cookies_from_keychain(&self) -> bool {
        self.restore_cookies_from_keychain_detailed() == CookieRestore::Restored
    }

    /// 키체인 쿠키 복원 — 읽기 실패(잠긴 키체인 등)와 "저장된 쿠키 없음"을 구분한다.
    /// 읽기 실패는 아무것도 지우지 않으며, 호출자가 잠시 뒤 다시 시도할 수 있다.
    pub fn restore_cookies_from_keychain_detailed(&self) -> CookieRestore {
        let generation = self.session_generation.current();
        let result = (|| -> Result<usize, StoreError> {
            let Some(data) = self.keychain.load(COOKIE_KEYCHAIN_KEY)? else {
                return Ok(0);
            };
            let entries: Vec<BTreeMap<String, String>> =
                serde_json::from_slice(&data).map_err(|e| StoreError::Decoding(e.to_string()))?;
            self.session_generation.check(generation)?;
            for props in &entries {
                let Some(cookie) = decode_cookie(props) else {
                    continue;
                };
                self.session_generation.with_current(
                    generation,
                    || -> Result<(), StoreError> {
                        self.jar.set_cookie(cookie);
                        Ok(())
                    },
                )?;
            }
            Ok(entries.len())
        })();
        match result {
            Ok(0) => CookieRestore::Empty,
            Ok(n) => {
                tracing::info!("Cookies restored from keychain ({n} cookies)");
                if self.has_cookies() {
                    CookieRestore::Restored
                } else {
                    CookieRestore::Empty
                }
            }
            Err(e @ (StoreError::Backend(_) | StoreError::Io(_))) => {
                tracing::warn!("Failed to read cookies from keychain: {e}");
                CookieRestore::ReadFailed
            }
            Err(e) => {
                tracing::error!("Failed to restore cookies: {e}");
                CookieRestore::Empty
            }
        }
    }

    /// 웹뷰 영구 저장소에서 인증 쿠키를 가져온다(키체인 쿠키가 없을 때의 폴백).
    pub async fn sync_from_webview_store(&self) -> bool {
        let generation = self.session_generation.current();
        let Some(webview) = &self.webview else {
            return false;
        };
        let cookies = webview.all_cookies().await;
        if self.session_generation.check(generation).is_err() {
            return false;
        }
        let auth: Vec<StoredCookie> = cookies
            .into_iter()
            .filter(StoredCookie::is_naver_auth_cookie)
            .collect();
        if auth.is_empty() {
            tracing::info!("WebView store: no auth cookies found");
            return false;
        }
        let applied =
            self.session_generation
                .with_current(generation, || -> Result<(), StoreError> {
                    for c in &auth {
                        self.jar.set_cookie(c.clone());
                    }
                    Ok(())
                });
        if applied.is_err() {
            return false;
        }
        tracing::info!("WebView store: synced {} auth cookies", auth.len());
        if let Err(e) = self.save_cookies_in_generation(generation) {
            tracing::error!("Failed to save cookies to keychain: {e}");
        }
        self.has_cookies()
    }

    /// 웹뷰 로그인 흐름이 얻은 쿠키를 저장소에 넣는다 (Swift `UnifiedLoginWebView.syncCookiesToShared`).
    ///
    /// 인증 쿠키만 넣고, 내부 API 호환을 위해 `.naver.com` 도메인 사본도 함께 넣는다.
    /// 두 인증 쿠키가 모두 들어왔으면 true.
    pub fn ingest_webview_cookies(&self, cookies: &[StoredCookie]) -> bool {
        let auth: Vec<&StoredCookie> = cookies
            .iter()
            .filter(|c| c.is_naver_auth_cookie())
            .collect();
        for cookie in &auth {
            self.jar.set_cookie((*cookie).clone());
            if cookie.domain != ".naver.com" {
                let mut broad = (*cookie).clone();
                broad.domain = ".naver.com".into();
                self.jar.set_cookie(broad);
            }
        }
        self.has_cookies()
    }

    /// 인증 쿠키 삭제 + 웹뷰 인증 쿠키·웹사이트 데이터 정리 + 저장소 항목 삭제.
    pub async fn clear_cookies(&self) {
        self.session_generation.advance();
        for cookie in self.jar.cookies() {
            if REQUIRED_COOKIE_NAMES.contains(&cookie.name.as_str())
                && naver_cookie_policy::matches_domain(&cookie.domain)
            {
                self.jar.delete_cookie(&cookie);
            }
        }
        // 웹뷰 영구 저장소 — 로그아웃 뒤 재시작 시 sync 가 쿠키를 되살리지 않게.
        if let Some(webview) = &self.webview {
            for cookie in webview.all_cookies().await {
                if naver_cookie_policy::is_authentication_cookie(&cookie.name, &cookie.domain) {
                    webview.delete_cookie(&cookie).await;
                }
            }
        }
        // 인증 쿠키만 지우면 localStorage/세션이 남아 웹 로그인이 부활할 수 있다.
        self.clear_all_website_data().await;
        if let Err(e) = self.keychain.delete(COOKIE_KEYCHAIN_KEY) {
            tracing::error!("Failed to delete cookies from keychain: {e}");
        }
        tracing::info!("All auth cookies + website data cleared");
    }

    /// 웹뷰 웹사이트 데이터 전체 삭제 (데이터 삭제 UI 용).
    pub async fn clear_all_website_data(&self) {
        if let Some(webview) = &self.webview {
            webview.remove_all_website_data().await;
            tracing::info!("WebView store: all website data removed");
        }
    }
}

fn encode_cookies(cookies: &[StoredCookie]) -> Result<Vec<u8>, StoreError> {
    let entries: Vec<BTreeMap<&str, String>> = cookies
        .iter()
        .map(|c| {
            let mut m = BTreeMap::new();
            m.insert("name", c.name.clone());
            m.insert("value", c.value.clone());
            m.insert("domain", c.domain.clone());
            m.insert("path", c.path.clone());
            if let Some(exp) = c.expires {
                m.insert("expires", exp.to_rfc3339_opts(SecondsFormat::Secs, true));
            }
            m
        })
        .collect();
    serde_json::to_vec(&entries).map_err(|e| StoreError::Encoding(e.to_string()))
}

fn decode_cookie(props: &BTreeMap<String, String>) -> Option<StoredCookie> {
    let name = props.get("name")?;
    let value = props.get("value")?;
    let domain = props.get("domain")?;
    if !REQUIRED_COOKIE_NAMES.contains(&name.as_str())
        || value.is_empty()
        || !naver_cookie_policy::matches_domain(domain)
    {
        return None;
    }
    let mut cookie = StoredCookie::new(name, value, domain)
        .with_path(props.get("path").cloned().unwrap_or_else(|| "/".into()));
    if let Some(exp) = props
        .get("expires")
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
    {
        cookie.expires = Some(exp.with_timezone(&Utc));
    }
    Some(cookie)
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAuthTests/CViewAuthTests.swift` (CookieManager 스위트) ·
    //! `AuthSessionRaceTests.cookieDomainBoundary`.
    use super::*;
    use chrono::Duration;

    fn deps() -> (CookieManager, Arc<CookieJar>, Arc<KeychainService>) {
        let jar = Arc::new(CookieJar::new());
        let keychain = Arc::new(KeychainService::in_memory());
        (
            CookieManager::new(keychain.clone(), jar.clone()),
            jar,
            keychain,
        )
    }

    fn naver(name: &str, value: &str) -> StoredCookie {
        naver_at(name, value, ".naver.com")
    }

    fn naver_at(name: &str, value: &str, domain: &str) -> StoredCookie {
        StoredCookie::new(name, value, domain).with_expires(Utc::now() + Duration::days(1))
    }

    #[test]
    fn no_cookies() {
        let (m, _, _) = deps();
        assert!(m.auth_cookies().is_empty());
        assert!(!m.has_cookies());
    }

    #[test]
    fn both_cookies_present() {
        let (m, jar, _) = deps();
        jar.set_cookie(naver("NID_AUT", "auth-val"));
        jar.set_cookie(naver("NID_SES", "ses-val"));
        assert!(m.has_cookies());
    }

    #[test]
    fn partial_cookies() {
        let (m, jar, _) = deps();
        jar.set_cookie(naver("NID_AUT", "auth-only"));
        assert!(!m.has_cookies());
    }

    #[test]
    fn empty_value_ignored() {
        let (m, jar, _) = deps();
        jar.set_cookie(naver("NID_AUT", ""));
        jar.set_cookie(naver("NID_SES", "ses-val"));
        assert!(!m.has_cookies());
    }

    #[test]
    fn wrong_domain_ignored() {
        let (m, jar, _) = deps();
        jar.set_cookie(naver_at("NID_AUT", "v1", ".example.com"));
        jar.set_cookie(naver_at("NID_SES", "v2", ".example.com"));
        assert!(m.auth_cookies().is_empty());
    }

    #[test]
    fn auth_cookies_filter() {
        let (m, jar, _) = deps();
        jar.set_cookie(naver("NID_AUT", "auth"));
        jar.set_cookie(naver("NID_SES", "ses"));
        jar.set_cookie(naver("OTHER", "other"));
        let mut names: Vec<_> = m.auth_cookies().into_iter().map(|c| c.name).collect();
        names.sort();
        assert_eq!(names, ["NID_AUT", "NID_SES"]);
    }

    #[test]
    fn expired_cookies_are_invisible() {
        let (m, jar, _) = deps();
        jar.set_cookie(naver("NID_AUT", "a"));
        jar.set_cookie(
            StoredCookie::new("NID_SES", "s", ".naver.com")
                .with_expires(Utc::now() - Duration::seconds(1)),
        );
        assert!(!m.has_cookies());
    }

    #[test]
    fn domain_lookalikes_excluded() {
        let (m, jar, _) = deps();
        for domain in ["notnaver.com", "naver.com.example.invalid"] {
            for name in REQUIRED_COOKIE_NAMES {
                jar.set_cookie(StoredCookie::new(name, "fixture", domain));
            }
        }
        assert!(m.auth_cookies().is_empty() && !m.has_cookies());
        for name in REQUIRED_COOKIE_NAMES {
            jar.set_cookie(StoredCookie::new(name, "fixture", ".naver.com"));
        }
        assert!(m.has_cookies() && m.auth_cookies().len() == 2);
    }

    #[test]
    fn save_writes_keychain_payload() {
        let (m, jar, keychain) = deps();
        jar.set_cookie(naver("NID_AUT", "save-auth"));
        jar.set_cookie(naver("NID_SES", "save-ses"));
        m.save_cookies_to_keychain().unwrap();
        let data = keychain.load(COOKIE_KEYCHAIN_KEY).unwrap().unwrap();
        let decoded: Vec<BTreeMap<String, String>> = serde_json::from_slice(&data).unwrap();
        assert_eq!(decoded.len(), 2);
        let names: Vec<_> = decoded.iter().filter_map(|d| d.get("name")).collect();
        assert!(names.contains(&&"NID_AUT".to_string()));
        assert!(names.contains(&&"NID_SES".to_string()));
        assert!(decoded.iter().all(|d| d.contains_key("expires")));
    }

    #[test]
    fn restore_empty_is_false() {
        let (m, _, _) = deps();
        assert!(!m.restore_cookies_from_keychain());
    }

    #[test]
    fn save_without_cookies_is_noop() {
        let (m, _, keychain) = deps();
        m.save_cookies_to_keychain().unwrap();
        assert_eq!(keychain.load(COOKIE_KEYCHAIN_KEY).unwrap(), None);
    }

    #[test]
    fn save_then_restore_into_fresh_jar() {
        let (m, jar, keychain) = deps();
        jar.set_cookie(naver("NID_AUT", "a"));
        jar.set_cookie(naver("NID_SES", "s"));
        m.save_cookies_to_keychain().unwrap();
        let fresh = Arc::new(CookieJar::new());
        let restored = CookieManager::new(keychain, fresh.clone());
        assert!(restored.restore_cookies_from_keychain());
        assert_eq!(fresh.cookies().len(), 2);
        assert!(fresh.cookies().iter().all(|c| c.expires.is_some()));
    }

    #[test]
    fn restore_skips_foreign_entries() {
        let (m, jar, keychain) = deps();
        let payload = r#"[{"name":"NID_AUT","value":"a","domain":".evil.com"},{"name":"OTHER","value":"x","domain":".naver.com"},{"name":"NID_SES","value":"","domain":".naver.com"}]"#;
        keychain
            .save(COOKIE_KEYCHAIN_KEY, payload.as_bytes())
            .unwrap();
        assert!(!m.restore_cookies_from_keychain());
        assert!(jar.cookies().is_empty());
    }

    #[tokio::test]
    async fn clear_removes_auth_cookies_only() {
        let (m, jar, keychain) = deps();
        jar.set_cookie(naver("NID_AUT", "del"));
        jar.set_cookie(naver("NID_SES", "del"));
        jar.set_cookie(naver("OTHER_COOKIE", "keep"));
        m.save_cookies_to_keychain().unwrap();
        assert!(m.has_cookies());
        m.clear_cookies().await;
        assert!(!m.has_cookies());
        assert!(jar.cookies().iter().any(|c| c.name == "OTHER_COOKIE"));
        assert_eq!(keychain.load(COOKIE_KEYCHAIN_KEY).unwrap(), None);
    }

    #[test]
    fn ingest_broadcasts_naver_domain_copy() {
        let (m, jar, _) = deps();
        let cookies = [
            StoredCookie::new("NID_AUT", "a", "nid.naver.com"),
            StoredCookie::new("NID_SES", "s", ".naver.com"),
            StoredCookie::new("OTHER", "o", ".naver.com"),
            StoredCookie::new("NID_SES", "evil", ".evil.com"),
        ];
        assert!(m.ingest_webview_cookies(&cookies));
        let all = jar.cookies();
        assert_eq!(all.len(), 3, "{all:?}");
        assert!(all
            .iter()
            .any(|c| c.name == "NID_AUT" && c.domain == ".naver.com"));
        assert!(!all.iter().any(|c| c.name == "OTHER"));
    }

    #[derive(Default)]
    struct FakeWebView {
        cookies: Mutex<Vec<StoredCookie>>,
        wiped: Mutex<bool>,
    }

    #[async_trait]
    impl WebViewDataStore for FakeWebView {
        async fn all_cookies(&self) -> Vec<StoredCookie> {
            self.cookies.lock().unwrap().clone()
        }
        async fn delete_cookie(&self, cookie: &StoredCookie) {
            self.cookies
                .lock()
                .unwrap()
                .retain(|c| !c.same_identity(cookie));
        }
        async fn remove_all_website_data(&self) {
            *self.wiped.lock().unwrap() = true;
        }
    }

    #[tokio::test]
    async fn webview_sync_and_clear() {
        let web = Arc::new(FakeWebView::default());
        *web.cookies.lock().unwrap() = vec![
            naver("NID_AUT", "a"),
            naver("NID_SES", "s"),
            naver("NNB", "keep"),
        ];
        let jar = Arc::new(CookieJar::new());
        let keychain = Arc::new(KeychainService::in_memory());
        let m = CookieManager::new(keychain.clone(), jar.clone()).with_webview_store(web.clone());
        assert!(m.sync_from_webview_store().await);
        assert!(keychain.load(COOKIE_KEYCHAIN_KEY).unwrap().is_some());
        m.clear_cookies().await;
        assert!(*web.wiped.lock().unwrap());
        let left: Vec<_> = web
            .cookies
            .lock()
            .unwrap()
            .iter()
            .map(|c| c.name.clone())
            .collect();
        assert_eq!(left, ["NNB"]);
        assert!(!m.sync_from_webview_store().await);
    }
}
