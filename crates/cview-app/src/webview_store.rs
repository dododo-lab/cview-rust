//! Tauri 웹뷰 쿠키 저장소 → `cview_auth::WebViewDataStore` (Swift `WKWebsiteDataStore.default()`).
//!
//! Tauri 2 의 `Webview::cookies()`/`delete_cookie()`/`clear_all_browsing_data()` 를 쓴다. 같은 앱의
//! 창들은 기본 웹뷰 데이터 저장소를 공유하므로, 로그인 창이 받은 NID 쿠키를 메인 창에서도 읽을 수 있다.
//! 쿠키 API 는 이벤트 루프와 왕복하므로(Windows 는 동기 호출 시 교착) 블로킹 스레드에서 부른다.

use async_trait::async_trait;
use cview_auth::{StoredCookie, WebViewDataStore};
use tauri::webview::Cookie;
use tauri::{AppHandle, Manager, Runtime, WebviewWindow};

use crate::MAIN_WINDOW;

/// 웹뷰 쿠키 → 저장 쿠키. 도메인이 없으면(host-only) `fallback_host` 를 쓴다. 만료는 세션으로 둔다.
pub fn stored_cookie_from_parts(
    name: &str,
    value: &str,
    domain: Option<&str>,
    path: Option<&str>,
    fallback_host: &str,
) -> StoredCookie {
    let domain = domain
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .unwrap_or(fallback_host);
    StoredCookie::new(name, value, domain).with_path(path.filter(|p| !p.is_empty()).unwrap_or("/"))
}

/// 인증 쿠키가 실릴 수 있는 NAVER 호스트 — host-only 쿠키 도메인 보정용.
pub const NAVER_COOKIE_FALLBACK_HOST: &str = ".naver.com";

pub fn to_stored(cookie: &Cookie<'_>) -> StoredCookie {
    stored_cookie_from_parts(
        cookie.name(),
        cookie.value(),
        cookie.domain(),
        cookie.path(),
        NAVER_COOKIE_FALLBACK_HOST,
    )
}

/// 창 하나의 모든 쿠키(블로킹 스레드에서 읽는다).
pub async fn read_window_cookies<R: Runtime>(window: &WebviewWindow<R>) -> Vec<StoredCookie> {
    let window = window.clone();
    tokio::task::spawn_blocking(move || match window.cookies() {
        Ok(list) => list.iter().map(to_stored).collect(),
        Err(e) => {
            tracing::warn!("웹뷰 쿠키를 읽지 못했습니다: {e}");
            Vec::new()
        }
    })
    .await
    .unwrap_or_default()
}

/// 메인 창의 웹뷰 저장소.
pub struct TauriWebViewStore<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> TauriWebViewStore<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }

    fn window(&self) -> Option<WebviewWindow<R>> {
        self.app.get_webview_window(MAIN_WINDOW)
    }
}

#[async_trait]
impl<R: Runtime> WebViewDataStore for TauriWebViewStore<R> {
    async fn all_cookies(&self) -> Vec<StoredCookie> {
        match self.window() {
            Some(w) => read_window_cookies(&w).await,
            None => Vec::new(),
        }
    }

    async fn delete_cookie(&self, target: &StoredCookie) {
        let Some(window) = self.window() else {
            return;
        };
        let target = target.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let Ok(list) = window.cookies() else {
                return;
            };
            for cookie in list {
                let stored = to_stored(&cookie);
                if stored.name == target.name && stored.domain.eq_ignore_ascii_case(&target.domain)
                {
                    if let Err(e) = window.delete_cookie(cookie) {
                        tracing::warn!("웹뷰 쿠키 삭제 실패: {e}");
                    }
                }
            }
        })
        .await;
    }

    async fn remove_all_website_data(&self) {
        let Some(window) = self.window() else {
            return;
        };
        let _ = tokio::task::spawn_blocking(move || {
            if let Err(e) = window.clear_all_browsing_data() {
                tracing::warn!("웹사이트 데이터 삭제 실패: {e}");
            }
        })
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_only_cookie_gets_fallback_domain() {
        let c = stored_cookie_from_parts("NID_AUT", "v", None, None, ".naver.com");
        assert_eq!(c.domain, ".naver.com");
        assert_eq!(c.path, "/");
        assert!(c.is_naver_auth_cookie());
    }

    #[test]
    fn explicit_domain_and_path_are_kept() {
        let c = stored_cookie_from_parts("NID_SES", "v", Some(".naver.com"), Some("/x"), "h");
        assert_eq!(c.domain, ".naver.com");
        assert_eq!(c.path, "/x");
        let other = stored_cookie_from_parts("NID_SES", "v", Some("evil.com"), None, "h");
        assert!(!other.is_naver_auth_cookie());
    }

    #[test]
    fn tauri_cookie_conversion() {
        let cookie = Cookie::build(("NID_AUT", "abc"))
            .domain(".naver.com")
            .path("/")
            .build();
        let stored = to_stored(&cookie);
        assert_eq!(stored.name, "NID_AUT");
        assert_eq!(stored.value, "abc");
        // cookie 크레이트는 Domain 속성의 앞 점을 떼어 돌려준다 — NAVER 도메인 판정은 그대로다.
        assert_eq!(stored.domain, "naver.com");
        assert!(stored.is_naver_auth_cookie());
    }
}
