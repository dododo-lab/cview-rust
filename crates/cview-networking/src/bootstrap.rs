//! `Sources/CViewNetworking/ChatSessionBootstrap.swift` 포팅 — 라이브 채팅방 접속 자격 증명 부트스트랩.
//!
//! liveDetail → chatAccessToken → userStatus 를 한 곳에서 준비한다. 채팅 토큰 타입
//! (`ChatAccessToken`)은 채팅 모델 포팅이 소유하므로 [`ChatTokenFields`] 트레이트로 필요한 필드만 읽는다.
//!
//! TODO(chat-models): 이모티콘 병합(`loadMergedEmoticons`, `LiveChatEmoticonBundle`)은
//! `EmoticonPack` 타입이 필요해 아직 포팅하지 않았다. [`prepare_connection`] 은 토큰·uid 만 준비한다.

use cview_core::error::ApiError;

use crate::client::{ChzzkApiClient, ResponseBody};

/// 채팅 토큰 응답에서 접속에 필요한 필드.
pub trait ChatTokenFields {
    fn access_token(&self) -> &str;
    fn extra_token(&self) -> Option<&str>;
}

/// stream channelId → chat WebSocket 접속에 필요한 자격 증명.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveChatSessionPlan {
    pub stream_channel_id: String,
    pub channel_name: String,
    pub chat_channel_id: String,
    pub access_token: String,
    pub extra_token: Option<String>,
    pub uid: Option<String>,
    pub channel_image_url: Option<String>,
}

/// liveDetail 조회 결과 — 오프라인 채널과 준비 완료 세션을 구분.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveChatPrefetchOutcome {
    Ready(LiveChatSessionPlan),
    Offline { saved_channel_name: String },
    Error(String),
}

/// 토큰 + uid + 닉네임까지 포함한 채팅 연결 준비 결과 (이모티콘 제외).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveChatConnectionPrep {
    pub plan: LiveChatSessionPlan,
    pub user_nickname: Option<String>,
}

/// 로그인 사용자의 채팅 uid — userIdHash 우선, 없으면 `fallback_uid`.
pub async fn resolve_chat_uid(
    client: &ChzzkApiClient,
    is_logged_in: bool,
    fallback_uid: Option<&str>,
) -> Option<String> {
    if !is_logged_in {
        return fallback_uid.map(Into::into);
    }
    match client.user_status().await {
        Ok(info) => info.user_id_hash.or_else(|| fallback_uid.map(Into::into)),
        Err(_) => fallback_uid.map(Into::into),
    }
}

async fn chat_uid(
    resolved_uid: Option<&str>,
    client: &ChzzkApiClient,
    is_logged_in: bool,
    fallback_uid: Option<&str>,
) -> Option<String> {
    match resolved_uid {
        Some(uid) => Some(uid.to_string()),
        None => resolve_chat_uid(client, is_logged_in, fallback_uid).await,
    }
}

/// 연결 준비에 필요한 인자.
#[derive(Debug, Clone, Default)]
pub struct ConnectionRequest {
    pub stream_channel_id: String,
    pub chat_channel_id: String,
    pub channel_name: String,
    pub channel_image_url: Option<String>,
    pub is_logged_in: bool,
    pub fallback_uid: Option<String>,
}

/// 단일 라이브 / 멀티라이브 공통 — token ∥ uid 병렬 준비.
pub async fn prepare_connection<T: ResponseBody + ChatTokenFields>(
    client: &ChzzkApiClient,
    req: &ConnectionRequest,
) -> Result<LiveChatConnectionPrep, ApiError> {
    let token_fut = client.chat_access_token::<T>(&req.chat_channel_id);
    let user_fut = async {
        if req.is_logged_in {
            client.user_status().await.ok()
        } else {
            None
        }
    };
    let (token, user) = tokio::join!(token_fut, user_fut);
    let token = token?;
    let uid = user
        .as_ref()
        .and_then(|u| u.user_id_hash.clone())
        .or_else(|| req.fallback_uid.clone());
    Ok(LiveChatConnectionPrep {
        plan: LiveChatSessionPlan {
            stream_channel_id: req.stream_channel_id.clone(),
            channel_name: req.channel_name.clone(),
            chat_channel_id: req.chat_channel_id.clone(),
            access_token: token.access_token().to_string(),
            extra_token: token.extra_token().map(Into::into),
            uid,
            channel_image_url: req.channel_image_url.clone(),
        },
        user_nickname: user.and_then(|u| u.nickname),
    })
}

/// 라이브 stream channelId → chatChannelId + accessToken. `None` 이면 방송 미진행(오프라인).
///
/// `resolved_uid` 가 있으면 userStatus 를 부르지 않는다(복원처럼 uid 를 한 번 받아 나눠 주는 경로).
pub async fn resolve_live_session<T: ResponseBody + ChatTokenFields>(
    client: &ChzzkApiClient,
    stream_channel_id: &str,
    is_logged_in: bool,
    fallback_uid: Option<&str>,
    resolved_uid: Option<&str>,
) -> Result<Option<LiveChatSessionPlan>, ApiError> {
    let (detail, uid) = tokio::join!(
        client.live_detail(stream_channel_id, false),
        chat_uid(resolved_uid, client, is_logged_in, fallback_uid)
    );
    let detail = detail?;
    let Some(chat_channel_id) = detail.chat_channel_id.clone() else {
        return Ok(None);
    };
    let token = client.chat_access_token::<T>(&chat_channel_id).await?;
    let channel_name = detail
        .channel
        .as_ref()
        .map(|c| c.channel_name.clone())
        .unwrap_or_else(|| stream_channel_id.into());
    Ok(Some(LiveChatSessionPlan {
        stream_channel_id: stream_channel_id.into(),
        channel_name,
        chat_channel_id,
        access_token: token.access_token().to_string(),
        extra_token: token.extra_token().map(Into::into),
        uid,
        channel_image_url: detail
            .channel
            .and_then(|c| c.channel_image_url)
            .map(|u| u.to_string()),
    }))
}

/// 저장 세션 복원용 prefetch. 부팅 직후 인증 복원이 세대를 올려 [`ApiError::Cancelled`] 로 버려지면
/// 새 인증 상태로 한 번 더 묻는다(호출자가 future 를 drop 했다면 여기까지 오지 않는다).
pub async fn prefetch_restore_session<T: ResponseBody + ChatTokenFields>(
    client: &ChzzkApiClient,
    saved_channel_id: &str,
    saved_channel_name: &str,
    is_logged_in: bool,
    fallback_uid: Option<&str>,
    resolved_uid: Option<&str>,
) -> LiveChatPrefetchOutcome {
    let first = resolve_live_session::<T>(
        client,
        saved_channel_id,
        is_logged_in,
        fallback_uid,
        resolved_uid,
    )
    .await;
    let result = match first {
        Err(ApiError::Cancelled) => {
            resolve_live_session::<T>(
                client,
                saved_channel_id,
                is_logged_in,
                fallback_uid,
                resolved_uid,
            )
            .await
        }
        other => other,
    };
    match result {
        Ok(Some(plan)) => LiveChatPrefetchOutcome::Ready(plan),
        Ok(None) => LiveChatPrefetchOutcome::Offline {
            saved_channel_name: saved_channel_name.into(),
        },
        Err(e) => LiveChatPrefetchOutcome::Error(e.to_string()),
    }
}
