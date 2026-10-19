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

/// 콜백 요청 줄에서 code·state 를 꺼낸다. 순수 함수.
pub fn parse_callback(request_line: &str) -> Option<(String, String)> {
    let path = request_line.split_whitespace().nth(1)?;
    let url = url::Url::parse(&format!("http://localhost{path}")).ok()?;
    if url.path() != "/callback" {
        return None;
    }
    let mut code = None;
    let mut state = None;
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "code" => code = Some(v.into_owned()),
            "state" => state = Some(v.into_owned()),
            _ => {}
        }
    }
    Some((code?, state?))
}

const DONE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>CView</title><body style=\"font:16px -apple-system,sans-serif;padding:48px;text-align:center\"><h2>치지직 연결을 마쳤습니다</h2><p>이 창을 닫고 CView 로 돌아가세요.</p></body>";
const FAIL_PAGE: &str = "<!doctype html><meta charset=utf-8><title>CView</title><body style=\"font:16px -apple-system,sans-serif;padding:48px;text-align:center\"><h2>연결하지 못했습니다</h2><p>CView 에서 다시 시도해 주세요.</p></body>";

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

    let wait = async {
        loop {
            let (mut sock, _) = listener.accept().await.ok()?;
            let mut buf = vec![0u8; 8192];
            let n = sock.read(&mut buf).await.ok()?;
            let head = String::from_utf8_lossy(&buf[..n]);
            let first = head.lines().next().unwrap_or_default().to_string();
            let parsed = parse_callback(&first);
            let page = if parsed.is_some() {
                DONE_PAGE
            } else {
                FAIL_PAGE
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
                page.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
            if parsed.is_some() {
                return parsed;
            }
        }
    };
    let (code, state) = tokio::time::timeout(Duration::from_secs(300), wait)
        .await
        .map_err(|_| CommandError("5분 안에 연결을 마치지 않아 그만두었습니다".into()))?
        .ok_or_else(|| CommandError("콜백을 받지 못했습니다".into()))?;
    services
        .auth
        .handle_oauth_login_success(&code, &state)
        .await
        .map_err(|e| CommandError(format!("토큰 교환 실패: {e}")))?;
    super::auth::broadcast_status(&app, &services).await;
    Ok(status(&services))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_parsing_and_hint() {
        assert_eq!(
            parse_callback("GET /callback?code=abc&state=XY%20Z HTTP/1.1"),
            Some(("abc".into(), "XY Z".into()))
        );
        assert_eq!(parse_callback("GET /favicon.ico HTTP/1.1"), None);
        assert_eq!(parse_callback("GET /callback?code=abc HTTP/1.1"), None);
        assert_eq!(client_id_hint("a6e68dba-8cb1"), "a6e68dba…");
        assert_eq!(client_id_hint("short"), "short");
    }
}
