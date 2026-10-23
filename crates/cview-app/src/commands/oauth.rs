//! 치지직 OAuth 연동 — 원본 OAuthCredentialsSection / CViewAuth ChzzkOAuthService (SET-14).
//!
//! 사용자가 치지직 개발자 센터에 등록한 애플리케이션의 Client ID/Secret 을 보안 저장소(키체인)에 둔다.
//! "연결" 은 인가 URL 을 기본 브라우저로 열고, `http://localhost:52735/callback` 로 돌아오는 code·state 를
//! 한 번만 받아 토큰으로 바꾼다(5분 안에 오지 않으면 그만둔다). 비밀값은 프런트로 다시 내보내지 않는다.

use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, State};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{CmdResult, CommandError};
use crate::state::AppServices;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthStatusDto {
    /// 쓸 수 있는 자격 증명이 있다(환경 변수 포함).
    pub has_credentials: bool,
    /// 키체인에 저장한 자격 증명이 있다.
    pub stored: bool,
    /// 표시용 Client ID(앞 8자리만).
    pub client_id_hint: Option<String>,
    /// OAuth 토큰이 있다.
    pub connected: bool,
    pub redirect_uri: String,
}

/// Client ID 표시 — 앞 8자만 보이고 나머지는 가린다. 순수 함수.
pub fn client_id_hint(id: &str) -> String {
    let head: String = id.chars().take(8).collect();
    if id.chars().count() > 8 {
        format!("{head}…")
    } else {
        head
    }
}

fn status(services: &AppServices) -> OAuthStatusDto {
    let oauth = &services.oauth;
    OAuthStatusDto {
        has_credentials: oauth.has_client_credentials(),
        stored: oauth.has_stored_client_credentials(),
        client_id_hint: oauth.effective_client_id().map(|id| client_id_hint(&id)),
        connected: services.auth.oauth_tokens().is_some(),
        redirect_uri: oauth.redirect_uri().to_string(),
    }
}

#[tauri::command]
pub async fn oauth_status(services: State<'_, AppServices>) -> CmdResult<OAuthStatusDto> {
    Ok(status(&services))
}

/// 자격 증명 저장(키체인).
#[tauri::command]
pub async fn oauth_set_credentials(
    services: State<'_, AppServices>,
    client_id: String,
    client_secret: String,
) -> CmdResult<OAuthStatusDto> {
    let id = client_id.trim();
    let secret = client_secret.trim();
    if id.is_empty() || secret.is_empty() {
        return Err(CommandError(
            "Client ID 와 Secret 을 모두 넣어 주세요".into(),
        ));
    }
    services
        .oauth
        .set_client_credentials(id, secret)
        .map_err(|e| CommandError(e.to_string()))?;
    Ok(status(&services))
}

#[tauri::command]
pub async fn oauth_clear_credentials(
    services: State<'_, AppServices>,
) -> CmdResult<OAuthStatusDto> {
    services
        .oauth
        .clear_client_credentials()
        .map_err(|e| CommandError(e.to_string()))?;
    Ok(status(&services))
}

/// 콜백 요청 — 인가 code·state, 또는 사용자 거부·오류(`error=`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OAuthCallback {
    Code { code: String, state: String },
    Denied(String),
}

/// 콜백 요청 줄에서 code·state(또는 `error`)를 꺼낸다. `/callback` 이 아니거나 값이 모자라면 `None`. 순수 함수.
pub fn parse_callback(request_line: &str) -> Option<OAuthCallback> {
    let path = request_line.split_whitespace().nth(1)?;
    let url = url::Url::parse(&format!("http://localhost{path}")).ok()?;
    if url.path() != "/callback" {
        return None;
    }
    let mut code = None;
    let mut state = None;
    let mut error = None;
    let mut description = None;
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "code" => code = Some(v.into_owned()),
            "state" => state = Some(v.into_owned()),
            "error" => error = Some(v.into_owned()),
            "error_description" => description = Some(v.into_owned()),
            _ => {}
        }
    }
    if let Some(error) = error {
        return Some(OAuthCallback::Denied(description.unwrap_or(error)));
    }
    Some(OAuthCallback::Code {
        code: code?,
        state: state?,
    })
}

const DONE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>CView</title><body style=\"font:16px -apple-system,sans-serif;padding:48px;text-align:center\"><h2>치지직 연결을 마쳤습니다</h2><p>이 창을 닫고 CView 로 돌아가세요.</p></body>";
const FAIL_PAGE: &str = "<!doctype html><meta charset=utf-8><title>CView</title><body style=\"font:16px -apple-system,sans-serif;padding:48px;text-align:center\"><h2>연결하지 못했습니다</h2><p>CView 에서 다시 시도해 주세요.</p></body>";

/// 콜백을 기다리는 전체 시간.
const CALLBACK_WAIT: Duration = Duration::from_secs(300);
/// 연결 하나에서 요청 줄을 읽는 시간 상한 — 아무것도 보내지 않는 연결이 전체 대기를 붙잡지 못하게.
const CALLBACK_READ_TIMEOUT: Duration = Duration::from_secs(5);
/// 요청 머리를 읽는 최대 바이트.
const CALLBACK_MAX_HEAD: usize = 8192;

/// 콜백 대기가 끝난 이유(성공 제외).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallbackError {
    /// 기한 안에 콜백이 오지 않았다.
    TimedOut,
    /// 사용자가 거부했거나 인가 서버가 오류를 돌려줬다.
    Denied(String),
    /// state 검증·토큰 교환 등 완료 처리가 실패했다.
    Failed(String),
}

impl CallbackError {
    fn message(&self) -> String {
        match self {
            Self::TimedOut => "5분 안에 연결을 마치지 않아 그만두었습니다".into(),
            Self::Denied(reason) => format!("치지직에서 연결을 허용하지 않았습니다: {reason}"),
            Self::Failed(reason) => reason.clone(),
        }
    }
}

/// 첫 줄(요청 줄)이 올 때까지 읽는다. 시간·크기 초과나 연결 오류면 `None`.
async fn read_request_line(
    sock: &mut tokio::net::TcpStream,
    deadline: tokio::time::Instant,
) -> Option<String> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        let n = tokio::time::timeout_at(deadline, sock.read(&mut chunk))
            .await
            .ok()?
            .ok()?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.contains(&b'\n') || buf.len() >= CALLBACK_MAX_HEAD {
            break;
        }
    }
    let head = String::from_utf8_lossy(&buf);
    head.lines().next().map(str::to_string)
}

async fn respond(
    sock: &mut tokio::net::TcpStream,
    status: &str,
    page: &str,
    write_timeout: Duration,
) {
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
        page.len()
    );
    let _ = tokio::time::timeout(write_timeout, sock.write_all(resp.as_bytes())).await;
}

/// 콜백 서버 — `deadline` 까지 `/callback` 을 기다린다. 연결마다 `read_timeout` 안에 요청 줄을 보내지
/// 않으면 닫고 다음 연결을 받는다. code 가 오면 `complete`(state 검증·토큰 교환)를 **끝까지 기다린 뒤**
/// 그 결과에 맞는 페이지를 보낸다 — 실패했는데 브라우저에 "연결을 마쳤습니다" 가 보이지 않게.
/// 완료 처리는 `deadline` 에 잘리지 않는다.
pub(crate) async fn serve_callback<F, Fut>(
    listener: &tokio::net::TcpListener,
    deadline: tokio::time::Instant,
    read_timeout: Duration,
    complete: F,
) -> Result<(), CallbackError>
where
    F: FnOnce(String, String) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    loop {
        let (mut sock, _) = match tokio::time::timeout_at(deadline, listener.accept()).await {
            Err(_) => return Err(CallbackError::TimedOut),
            Ok(Ok(accepted)) => accepted,
            Ok(Err(_)) => {
                // 일시적인 accept 오류(파일 수 한도 등) — 잠깐 쉬고 계속 기다린다.
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
        };
        let read_deadline = deadline.min(tokio::time::Instant::now() + read_timeout);
        let Some(line) = read_request_line(&mut sock, read_deadline).await else {
            continue;
        };
        match parse_callback(&line) {
            None => {
                respond(&mut sock, "404 Not Found", "", read_timeout).await;
            }
            Some(OAuthCallback::Denied(reason)) => {
                respond(&mut sock, "200 OK", FAIL_PAGE, read_timeout).await;
                return Err(CallbackError::Denied(reason));
            }
            Some(OAuthCallback::Code { code, state }) => {
                let result = complete(code, state).await;
                let page = if result.is_ok() { DONE_PAGE } else { FAIL_PAGE };
                respond(&mut sock, "200 OK", page, read_timeout).await;
                return result.map_err(CallbackError::Failed);
            }
        }
    }
}

/// 연결 — 브라우저로 인가 페이지를 열고 콜백을 기다린다.
#[tauri::command]
pub async fn oauth_connect(
    app: AppHandle,
    services: State<'_, AppServices>,
) -> CmdResult<OAuthStatusDto> {
    let Some(auth_url) = services.oauth.generate_auth_url() else {
        return Err(CommandError(
            "먼저 Client ID 와 Secret 을 저장해 주세요".into(),
        ));
    };
    let port = url::Url::parse(services.oauth.redirect_uri())
        .ok()
        .and_then(|u| u.port())
        .unwrap_or(52735);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|e| CommandError(format!("콜백 포트({port})를 열지 못했습니다: {e}")))?;
    super::content::open_external(auth_url.to_string()).await?;

    let auth = &services.auth;
    serve_callback(
        &listener,
        tokio::time::Instant::now() + CALLBACK_WAIT,
        CALLBACK_READ_TIMEOUT,
        |code, state| async move {
            auth.handle_oauth_login_success(&code, &state)
                .await
                .map_err(|e| format!("토큰 교환 실패: {e}"))
        },
    )
    .await
    .map_err(|e| CommandError(e.message()))?;
    super::auth::broadcast_status(&app, &services).await;
    Ok(status(&services))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[test]
    fn callback_parsing_and_hint() {
        assert_eq!(
            parse_callback("GET /callback?code=abc&state=XY%20Z HTTP/1.1"),
            Some(OAuthCallback::Code {
                code: "abc".into(),
                state: "XY Z".into()
            })
        );
        assert_eq!(
            parse_callback("GET /callback?error=access_denied&state=s HTTP/1.1"),
            Some(OAuthCallback::Denied("access_denied".into()))
        );
        assert_eq!(
            parse_callback("GET /callback?error=x&error_description=%EA%B1%B0%EB%B6%80 HTTP/1.1"),
            Some(OAuthCallback::Denied("거부".into()))
        );
        assert_eq!(parse_callback("GET /favicon.ico HTTP/1.1"), None);
        assert_eq!(parse_callback("GET /callback?code=abc HTTP/1.1"), None);
        assert_eq!(client_id_hint("a6e68dba-8cb1"), "a6e68dba…");
        assert_eq!(client_id_hint("short"), "short");
    }

    async fn listener() -> (tokio::net::TcpListener, u16) {
        let l = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = l.local_addr().unwrap().port();
        (l, port)
    }

    /// 요청을 보내고 응답 전체를 읽는다.
    async fn request(port: u16, path: &str) -> String {
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        s.write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        out
    }

    fn deadline(secs: u64) -> tokio::time::Instant {
        tokio::time::Instant::now() + Duration::from_secs(secs)
    }

    #[tokio::test]
    async fn success_page_only_after_completion_succeeds() {
        let (l, port) = listener().await;
        let client = tokio::spawn(async move { request(port, "/callback?code=c&state=s").await });
        let result = serve_callback(
            &l,
            deadline(10),
            Duration::from_secs(2),
            |code, state| async move {
                assert_eq!((code.as_str(), state.as_str()), ("c", "s"));
                Ok(())
            },
        )
        .await;
        assert_eq!(result, Ok(()));
        let resp = client.await.unwrap();
        assert!(resp.starts_with("HTTP/1.1 200 OK"));
        assert!(resp.contains("연결을 마쳤습니다"));
    }

    #[tokio::test]
    async fn failed_state_or_token_exchange_never_shows_success() {
        let (l, port) = listener().await;
        let client =
            tokio::spawn(async move { request(port, "/callback?code=c&state=wrong").await });
        let result = serve_callback(&l, deadline(10), Duration::from_secs(2), |_, _| async {
            Err("토큰 교환 실패: state 불일치".to_string())
        })
        .await;
        assert_eq!(
            result,
            Err(CallbackError::Failed("토큰 교환 실패: state 불일치".into()))
        );
        let resp = client.await.unwrap();
        assert!(resp.contains("연결하지 못했습니다"));
        assert!(!resp.contains("연결을 마쳤습니다"));
    }

    #[tokio::test]
    async fn user_denial_shows_failure_without_token_exchange() {
        let (l, port) = listener().await;
        let client =
            tokio::spawn(
                async move { request(port, "/callback?error=access_denied&state=s").await },
            );
        let called = Arc::new(AtomicBool::new(false));
        let flag = called.clone();
        let result = serve_callback(
            &l,
            deadline(10),
            Duration::from_secs(2),
            |_, _| async move {
                flag.store(true, Ordering::SeqCst);
                Ok(())
            },
        )
        .await;
        assert_eq!(result, Err(CallbackError::Denied("access_denied".into())));
        assert!(!called.load(Ordering::SeqCst));
        let resp = client.await.unwrap();
        assert!(resp.contains("연결하지 못했습니다"));
        assert!(!resp.contains("연결을 마쳤습니다"));
    }

    #[tokio::test]
    async fn silent_and_unrelated_connections_do_not_block_the_callback() {
        let (l, port) = listener().await;
        let client = tokio::spawn(async move {
            // 아무것도 보내지 않는 연결 — 읽기 제한 뒤 닫혀야 한다.
            let _idle = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            let favicon = request(port, "/favicon.ico").await;
            let done = request(port, "/callback?code=c&state=s").await;
            (favicon, done)
        });
        let started = std::time::Instant::now();
        let result = serve_callback(&l, deadline(30), Duration::from_millis(200), |_, _| async {
            Ok(())
        })
        .await;
        assert_eq!(result, Ok(()));
        assert!(started.elapsed() < Duration::from_secs(5));
        let (favicon, done) = client.await.unwrap();
        assert!(favicon.starts_with("HTTP/1.1 404"));
        assert!(done.contains("연결을 마쳤습니다"));
    }

    #[tokio::test]
    async fn no_callback_before_deadline_times_out() {
        let (l, _port) = listener().await;
        let result = serve_callback(
            &l,
            tokio::time::Instant::now() + Duration::from_millis(100),
            Duration::from_millis(50),
            |_, _| async { Ok(()) },
        )
        .await;
        assert_eq!(result, Err(CallbackError::TimedOut));
    }
}
