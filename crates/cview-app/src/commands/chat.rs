//! 채팅 명령 — 연결/해제/전송. 엔진 이벤트는 `chat://event` 로 흘려보낸다.

use std::sync::Arc;

use cview_chat::engine::{ChatCookie, CookieProvider, TokenProvider};
use cview_chat::{ChatEngine, ChatEngineConfiguration, ChatError};
use futures_util::FutureExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use super::{events, CmdResult, CommandError};
use crate::dto::ChatEventDto;
use crate::state::{AppServices, ChatSession};

/// 채팅 한 줄 최대 길이 (치지직 웹 입력 제한).
pub const MAX_CHAT_LENGTH: usize = 100;

/// 전송할 채팅 정리 — 공백만이면 `None`, 길면 자른다.
pub fn sanitize_outgoing(text: &str) -> Option<String> {
    let t = text.trim();
    (!t.is_empty()).then(|| t.chars().take(MAX_CHAT_LENGTH).collect())
}

/// `chat_connect` 응답.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatConnectedDto {
    pub chat_channel_id: String,
    /// 로그인 쿠키로 받은 SEND 토큰인가(아니면 읽기 전용).
    pub can_send: bool,
}

async fn disconnect_session(services: &AppServices) {
    if let Some(session) = services.chat.lock().await.take() {
        session.engine.disconnect().await;
        session.pump.abort();
    }
}

/// 채팅 연결. 기존 연결은 끊는다.
#[tauri::command]
pub async fn chat_connect(
    app: AppHandle,
    services: State<'_, AppServices>,
    chat_channel_id: String,
    channel_id: Option<String>,
) -> CmdResult<ChatConnectedDto> {
    disconnect_session(&services).await;
    if chat_channel_id.trim().is_empty() {
        return Err(CommandError("채팅 채널 정보가 없습니다".into()));
    }

    let token = services.api.chat_access_token(&chat_channel_id).await?;
    let logged_in = services.auth.is_authenticated();
    let uid = if logged_in {
        services
            .api
            .user_status()
            .await
            .ok()
            .and_then(|s| s.user_id_hash)
    } else {
        None
    };

    let cookies = services.cookies.clone();
    let cookie_provider: CookieProvider = Arc::new(move || {
        cookies
            .auth_cookies()
            .into_iter()
            .map(|c| ChatCookie {
                name: c.name,
                value: c.value,
                domain: c.domain,
            })
            .collect()
    });
    let api = services.api.clone();
    let token_channel = chat_channel_id.clone();
    let token_provider: TokenProvider = Arc::new(move || {
        let api = api.clone();
        let id = token_channel.clone();
        async move {
            api.chat_access_token(&id)
                .await
                .map(|t| (t.access_token, t.extra_token))
                .map_err(|e| ChatError::ConnectionFailed(e.to_string()))
        }
        .boxed()
    });

    let config = ChatEngineConfiguration::new(chat_channel_id.clone(), token.access_token)
        .extra_token(token.extra_token)
        .uid(uid.clone())
        .channel_id(channel_id)
        .cookie_provider(Some(cookie_provider))
        .token_provider(Some(token_provider));
    let engine = ChatEngine::new(config);

    let stream = engine.events();
    let event_app = app.clone();
    let pump = tauri::async_runtime::spawn(async move {
        while let Some(event) = stream.recv().await {
            let _ = event_app.emit(events::CHAT_EVENT, ChatEventDto::from_engine(&event));
        }
    });

    *services.chat.lock().await = Some(ChatSession {
        engine: engine.clone(),
        chat_channel_id: chat_channel_id.clone(),
        pump,
    });
    // 연결 실패는 엔진의 재연결 루프가 맡는다 — 상태는 이벤트로 온다.
    tauri::async_runtime::spawn(async move { engine.connect().await });

    Ok(ChatConnectedDto {
        chat_channel_id,
        can_send: logged_in && uid.is_some(),
    })
}

#[tauri::command]
pub async fn chat_disconnect(services: State<'_, AppServices>) -> CmdResult<()> {
    disconnect_session(&services).await;
    Ok(())
}

/// 채팅 전송. 반환값은 전송 `tid`.
#[tauri::command]
pub async fn chat_send(services: State<'_, AppServices>, text: String) -> CmdResult<i64> {
    let Some(text) = sanitize_outgoing(&text) else {
        return Err(CommandError("메시지를 입력하세요".into()));
    };
    if !services.auth.is_authenticated() {
        return Err(CommandError("로그인 후 채팅 참여 가능".into()));
    }
    let engine = services
        .chat
        .lock()
        .await
        .as_ref()
        .map(|s| s.engine.clone())
        .ok_or(ChatError::NotConnected)?;
    Ok(engine.send_message(&text, None).await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outgoing_text_is_trimmed_and_capped() {
        assert_eq!(sanitize_outgoing("   "), None);
        assert_eq!(sanitize_outgoing(" 안녕 "), Some("안녕".into()));
        assert_eq!(
            sanitize_outgoing(&"ㅋ".repeat(150))
                .unwrap()
                .chars()
                .count(),
            MAX_CHAT_LENGTH
        );
    }
}
