//! 통합 로그인 창 — `Sources/CViewAuth/UnifiedLoginWebView.swift` 의 Tauri 셸.
//!
//! 흐름 판단은 `cview_auth::UnifiedLoginFlow` 상태 기계가 한다. 이 모듈은 웹뷰 이벤트를 넘기고
//! 돌려받은 [`LoginAction`] 을 수행한다:
//!
//! 1. 두 번째 창(`login`)에 `nid.naver.com` 로그인 페이지를 연다(Chrome UA — Swift 와 같다).
//! 2. 쿠키 감지 — Swift 의 `cookiesDidChange` 대신 짧은 주기 폴링 + 페이지 로드 완료 시 확인.
//!    `NID_AUT`/`NID_SES` 가 생기면 `CookieManager::ingest_webview_cookies` 로 앱 저장소에 넣는다.
//! 3. OAuth 자격 증명이 있으면 같은 창에서 동의 페이지로 넘어가 redirect URI 를 가로챈다(`on_navigation`).
//!    없으면 쿠키 로그인으로 끝낸다(`AuthManager::handle_login_success`).
//! 4. 창을 닫고 `auth://changed` 를 보낸다. 실패는 `auth://login-failed`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use cview_auth::login_flow::NavigationDecision;
use cview_auth::{LoginAction, UnifiedLoginFlow};
use cview_networking::headers::CHROME_USER_AGENT;
use tauri::webview::PageLoadEvent;
use tauri::{
    AppHandle, Emitter, Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
use tokio::sync::mpsc;

use crate::commands::{auth::broadcast_status, events, MessagePayload};
use crate::state::AppServices;
use crate::webview_store::read_window_cookies;

/// 로그인 창 라벨.
pub const LOGIN_WINDOW: &str = "login";
/// 쿠키 폴링 주기.
const COOKIE_POLL_INTERVAL: Duration = Duration::from_millis(800);

enum LoginSignal {
    /// 페이지 로드 완료 — 쿠키를 다시 본다.
    PageLoaded,
    /// redirect URI 가로채기로 생긴 동작(최신 쿠키 재동기화 후 수행).
    Intercepted(LoginAction),
}

/// 실행 결과.
enum Outcome {
    Continue,
    Finished,
    Failed(String),
}

/// 로그인 창을 연다. 이미 열려 있으면 앞으로 가져온다.
pub fn open_login_window<R: Runtime>(
    app: &AppHandle<R>,
    services: &AppServices,
) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(LOGIN_WINDOW) {
        let _ = window.set_focus();
        return Ok(());
    }

    let auth_url = services
        .oauth
        .has_client_credentials()
        .then(|| services.oauth.generate_auth_url())
        .flatten();
    let flow = Arc::new(Mutex::new(UnifiedLoginFlow::new(
        auth_url,
        services.oauth.redirect_uri().to_string(),
    )));
    let (tx, rx) = mpsc::unbounded_channel::<LoginSignal>();

    let nav_flow = flow.clone();
    let nav_tx = tx.clone();
    let load_tx = tx;
    let window = WebviewWindowBuilder::new(
        app,
        LOGIN_WINDOW,
        WebviewUrl::External(UnifiedLoginFlow::start_url()),
    )
    .title("네이버 로그인")
    .inner_size(480.0, 720.0)
    .min_inner_size(400.0, 560.0)
    .user_agent(CHROME_USER_AGENT)
    .on_navigation(move |url| {
        let decision = nav_flow
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .on_navigation(url, &[]);
        match decision {
            NavigationDecision::Allow => true,
            NavigationDecision::Cancel(action) => {
                let _ = nav_tx.send(LoginSignal::Intercepted(action));
                false
            }
        }
    })
    .on_page_load(move |_, payload| {
        if payload.event() == PageLoadEvent::Finished {
            let _ = load_tx.send(LoginSignal::PageLoaded);
        }
    })
    .build()
    .map_err(|e| format!("로그인 창을 열지 못했습니다: {e}"))?;
    // Windows — 앱 메뉴 막대는 로그인 페이지에 쓸모가 없다(제목 막대는 둔다 — 원격 페이지라 자체 버튼이 없다).
    #[cfg(windows)]
    let _ = window.hide_menu();

    let task = tauri::async_runtime::spawn(run_login(app.clone(), window, flow, rx));
    let mut slot = services
        .login_task
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(previous) = slot.replace(task) {
        previous.abort();
    }
    Ok(())
}

async fn run_login<R: Runtime>(
    app: AppHandle<R>,
    window: WebviewWindow<R>,
    flow: Arc<Mutex<UnifiedLoginFlow>>,
    mut rx: mpsc::UnboundedReceiver<LoginSignal>,
) {
    let outcome = loop {
        let signal = tokio::select! {
            s = rx.recv() => s,
            _ = tokio::time::sleep(COOKIE_POLL_INTERVAL) => Some(LoginSignal::PageLoaded),
        };
        if app.get_webview_window(LOGIN_WINDOW).is_none() {
            break Outcome::Failed("로그인 창이 닫혔습니다".into());
        }
        let action = match signal {
            None => break Outcome::Failed("로그인 창이 닫혔습니다".into()),
            Some(LoginSignal::PageLoaded) => {
                let cookies = read_window_cookies(&window).await;
                flow.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .on_cookies(&cookies)
            }
            Some(LoginSignal::Intercepted(action)) => {
                // 2단계 중 회전된 NID 쿠키를 다시 넣고 동작을 이어간다(Swift `resyncCookiesThen`).
                let cookies = read_window_cookies(&window).await;
                let services = app.state::<AppServices>();
                services.cookies.ingest_webview_cookies(&cookies);
                action
            }
        };
        match perform(&app, &window, &flow, action).await {
            Outcome::Continue => continue,
            other => break other,
        }
    };

    let _ = window.close();
    let services = app.state::<AppServices>();
    match outcome {
        Outcome::Finished => {
            services.api.invalidate_authentication_cache().await;
            broadcast_status(&app, &services).await;
        }
        Outcome::Failed(message) => {
            let _ = app.emit(events::AUTH_LOGIN_FAILED, MessagePayload { message });
            broadcast_status(&app, &services).await;
        }
        Outcome::Continue => {}
    }
}

async fn perform<R: Runtime>(
    app: &AppHandle<R>,
    window: &WebviewWindow<R>,
    flow: &Arc<Mutex<UnifiedLoginFlow>>,
    action: LoginAction,
) -> Outcome {
    let services = app.state::<AppServices>();
    let mut action = action;
    loop {
        match action {
            LoginAction::None => return Outcome::Continue,
            LoginAction::IngestCookies { cookies, next } => {
                if !services.cookies.ingest_webview_cookies(&cookies) {
                    tracing::warn!("통합 로그인: 인증 쿠키 주입이 불완전합니다");
                }
                action = *next;
            }
            LoginAction::LoadOAuthConsent(url) => {
                if let Err(e) = window.navigate(url) {
                    tracing::warn!("OAuth 동의 페이지 이동 실패: {e} — 쿠키 로그인으로 완료");
                    services.auth.handle_login_success().await;
                    return Outcome::Finished;
                }
                return Outcome::Continue;
            }
            LoginAction::CookieOnlySuccess { reason } => {
                if let Some(reason) = reason {
                    tracing::info!("통합 로그인: 쿠키 로그인 ({reason})");
                }
                services.auth.handle_login_success().await;
                return Outcome::Finished;
            }
            LoginAction::CodeReceived { code, state } => {
                let opened = flow
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .auth_url()
                    .cloned();
                let csrf = cview_auth::csrf::validate(opened.as_ref(), state.as_deref());
                let sent_state = csrf.sent_state.unwrap_or_default();
                if csrf.ok {
                    match services
                        .auth
                        .handle_oauth_login_success(&code, &sent_state)
                        .await
                    {
                        Ok(()) => return Outcome::Finished,
                        Err(e) => tracing::warn!("OAuth 토큰 교환 실패: {e} — 쿠키 로그인 폴백"),
                    }
                } else {
                    tracing::warn!("OAuth state 불일치 — 쿠키 로그인 폴백");
                }
                services.auth.handle_login_success().await;
                return Outcome::Finished;
            }
            LoginAction::Error(message) => return Outcome::Failed(message),
        }
    }
}
