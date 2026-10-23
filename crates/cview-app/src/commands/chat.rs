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
///
/// `op` — 프런트엔드가 세션마다 붙이는 요청 순번(`ChatOpGate`). 더 새 연결·해제 요청이 먼저 돌았으면
/// 아무것도 하지 않고 물러난다(응답은 읽기 전용 성공 — 호출부는 자기 순번으로 이미 무시한다).
#[tauri::command]
pub async fn chat_connect(
    window: tauri::Window,
    services: State<'_, AppServices>,
    chat_channel_id: String,
    channel_id: Option<String>,
    session: Option<String>,
    op: Option<u64>,
) -> CmdResult<ChatConnectedDto> {
    let key = crate::state::chat_session_key(session.as_deref()).to_string();
    let superseded = || ChatConnectedDto {
        chat_channel_id: chat_channel_id.clone(),
        can_send: false,
    };
    if chat_channel_id.trim().is_empty() {
        let detached = services
            .chat_ops
            .run_if_latest(&key, op, || services.chat_vm(session.as_deref()).detach());
        if let Some(d) = detached {
            d.await;
        }
        return Err(CommandError("채팅 채널 정보가 없습니다".into()));
    }
    // 순번 확인·세션 뷰모델 고르기·이전 연결 떼기·연결 순번 잡기를 한 잠금 안에서 — 동시에 도는 다른 연결·해제
    // 요청과 보낸 순서대로 한 줄로 선다(`ChatOpGate`).
    let prologue = services.chat_ops.run_if_latest(&key, op, || {
        if let Some(k) = session
            .as_deref()
            .filter(|k| !k.is_empty() && *k != crate::state::MAIN_CHAT_SESSION)
        {
            services.note_chat_session_window(k, window.label());
        }
        let chat = services.chat_vm(session.as_deref());
        // 다른 채널로 바꾸면 이전 연결은 바로 끊는다 — 토큰을 받는 동안 이전 채널 메시지가 새 채널 채팅창
        // (화면은 이미 비우고 새 채널로 바꿨다)에 섞여 들어오지 않게. 같은 채널 재연결은 기록을 지키려 그대로 둔다.
        let switching =
            chat.lock().active_chat_channel_id().as_deref() != Some(chat_channel_id.as_str());
        let detached = switching.then(|| chat.detach());
        // 토큰·사용자 정보를 받는 동안 다른 채널 요청이 올 수 있다(멀티라이브 패인 전환, 지난 구성 다시 열기).
        // 받기 전에 순번을 잡아 두고 마지막 요청만 연결한다 — 먼저 보낸 요청이 늦게 끝나 새 채널을 덮지 않게.
        let ticket = chat.begin_connect_request();
        (chat, ticket, detached)
    });
    let Some((chat, ticket, detached)) = prologue else {
        tracing::debug!("채팅 연결 요청 {chat_channel_id} ({key}) — 더 새 요청이 먼저 와 건너뜀");
        return Ok(superseded());
    };
    if let Some(d) = detached {
        d.await;
    }
    // 기다리는 동안 더 새 요청이 왔거나, 해제가 이 세션 뷰모델을 표에서 지웠으면 물러난다 — 지워진 뷰모델에
    // 연결하면 다음 해제로도 끊기지 않는 고아 연결이 된다.
    let still_current = |services: &AppServices| {
        chat.is_current_connect_request(ticket)
            && services.chat_ops.is_latest(&key, op)
            && services.is_live_chat_vm(session.as_deref(), &chat)
    };

    let token = match services.api.chat_access_token(&chat_channel_id).await {
        Ok(t) => t,
        Err(e) => {
            if !still_current(&services) {
                return Ok(superseded());
            }
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

    if !still_current(&services) {
        tracing::debug!("채팅 연결 요청 {chat_channel_id} — 더 새 요청이 있어 건너뜀");
        return Ok(superseded());
    }

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
    tauri::async_runtime::spawn(async move { vm.connect_if_current(ticket, request).await });

    Ok(ChatConnectedDto {
        chat_channel_id,
        can_send: logged_in && uid.is_some(),
    })
}

#[tauri::command]
pub async fn chat_disconnect(
    services: State<'_, AppServices>,
    session: Option<String>,
    op: Option<u64>,
) -> CmdResult<()> {
    // 늦게 돈 해제가 그 뒤에 보낸 연결을 끊지 않게 — 순번 확인과 떼어 내기를 한 잠금 안에서(`ChatOpGate`).
    let key = crate::state::chat_session_key(session.as_deref());
    let detached = services.chat_ops.run_if_latest(key, op, || {
        if key == crate::state::MAIN_CHAT_SESSION {
            futures_util::future::Either::Left(services.chat.detach())
        } else {
            futures_util::future::Either::Right(services.detach_chat_session(key))
        }
    });
    if let Some(d) = detached {
        d.await;
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
