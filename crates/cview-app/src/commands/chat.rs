//! 채팅 명령 — 연결/해제/전송. 상태기계는 app-core `ChatViewModel` 이 맡고, 엔진 이벤트는
//! 뷰모델이 꺼내는 순간 `chat://event` 로도 흘려보낸다(`state.rs` 의 엔진 어댑터).

use cview_app_core::viewmodels::chat::connection::ConnectRequest;
use cview_app_core::viewmodels::chat::interaction::MessageSendOutcome;
use cview_app_core::viewmodels::chat::ChatViewModel;
use serde::Serialize;
use tauri::State;

use super::{CmdResult, CommandError};
use crate::state::AppServices;

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

/// 채팅 연결. 기존 연결은 뷰모델이 끊는다.
#[tauri::command]
pub async fn chat_connect(
    services: State<'_, AppServices>,
    chat_channel_id: String,
    channel_id: Option<String>,
    session: Option<String>,
) -> CmdResult<ChatConnectedDto> {
    let chat = services.chat_vm(session.as_deref());
    if chat_channel_id.trim().is_empty() {
        chat.disconnect().await;
        return Err(CommandError("채팅 채널 정보가 없습니다".into()));
    }

    let token = match services.api.chat_access_token(&chat_channel_id).await {
        Ok(t) => t,
        Err(e) => {
            chat.disconnect().await;
            return Err(e.into());
        }
    };
    let logged_in = services.auth.is_authenticated();
    let (uid, nickname) = if logged_in {
        match services.api.user_status().await {
            Ok(s) => (s.user_id_hash, s.nickname),
            Err(_) => (None, None),
        }
    } else {
        (None, None)
    };

    {
        let mut s = chat.lock();
        s.current_user_uid = uid.clone();
        s.current_user_nickname = nickname;
    }
    let request = ConnectRequest {
        chat_channel_id: chat_channel_id.clone(),
        access_token: token.access_token,
        extra_token: token.extra_token,
        uid: uid.clone(),
        channel_id,
        channel_name: None,
        token_provider: Some(ChatViewModel::make_token_provider(
            services.api.clone(),
            chat_channel_id.clone(),
        )),
    };
    // 연결 실패는 엔진의 재연결 루프가 맡는다 — 상태는 이벤트로 온다.
    let vm = chat.clone();
    tauri::async_runtime::spawn(async move { vm.connect(request).await });

    Ok(ChatConnectedDto {
        chat_channel_id,
        can_send: logged_in && uid.is_some(),
    })
}

#[tauri::command]
pub async fn chat_disconnect(
    services: State<'_, AppServices>,
    session: Option<String>,
) -> CmdResult<()> {
    match session
        .as_deref()
        .filter(|k| *k != crate::state::MAIN_CHAT_SESSION)
    {
        Some(key) => services.drop_chat_session(key).await,
        None => services.chat.disconnect().await,
    }
    Ok(())
}

/// 채팅 전송. `/` 명령은 뷰모델이 로컬에서 처리한다.
#[tauri::command]
pub async fn chat_send(
    services: State<'_, AppServices>,
    text: String,
    session: Option<String>,
) -> CmdResult<()> {
    let chat = services.chat_vm(session.as_deref());
    let Some(text) = sanitize_outgoing(&text) else {
        return Err(CommandError("메시지를 입력하세요".into()));
    };
    if !services.auth.is_authenticated() {
        return Err(CommandError("로그인 후 채팅 참여 가능".into()));
    }
    match chat.send_message(&text).await {
        MessageSendOutcome::Sent | MessageSendOutcome::HandledLocally => Ok(()),
        MessageSendOutcome::SessionChanged => Err(CommandError(
            "채팅 연결이 바뀌었습니다. 다시 보내 주세요".into(),
        )),
        MessageSendOutcome::Failed => Err(CommandError(
            chat.lock()
                .error_message
                .take()
                .unwrap_or_else(|| "메시지를 보내지 못했습니다".into()),
        )),
    }
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

/// 채널의 주간 후원 랭킹(상위 `count`명).
#[tauri::command]
pub async fn chat_donation_ranking(
    services: State<'_, AppServices>,
    channel_id: String,
    count: Option<i64>,
) -> CmdResult<Vec<crate::dto::DonationRankDto>> {
    let count = count.unwrap_or(10).clamp(1, 50);
    let response = services
        .api
        .weekly_donation_ranking(&channel_id, count, false)
        .await?;
    let mut list: Vec<_> = response
        .rank_list
        .iter()
        .map(crate::dto::DonationRankDto::from_item)
        .collect();
    list.sort_by_key(|r| r.rank);
    Ok(list)
}

/// 채널에서 쓸 수 있는 이모티콘 팩(기본 + 채널·구독) — 이모티콘 선택기·자동완성용.
#[tauri::command]
pub async fn chat_emoticons(
    services: State<'_, AppServices>,
    channel_id: String,
) -> CmdResult<Vec<cview_core::models::chat::EmoticonPack>> {
    let bundle = cview_networking::bootstrap::load_merged_emoticons(
        &services.api,
        &channel_id,
        &std::collections::HashMap::new(),
        &[],
    )
    .await;
    Ok(bundle
        .packs
        .into_iter()
        .filter(|p| p.emoticons.as_ref().is_some_and(|e| !e.is_empty()))
        .collect())
}
