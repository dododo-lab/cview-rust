//! 얇은 명령 계층 — 프런트엔드 `invoke` 대상.
//!
//! 각 명령은 `cview-app-core` 의 뷰모델·서비스(팔로잉 스토어·인기·검색·채팅·플레이어)를 부르고,
//! 대응이 없는 것(채널/라이브 상세·인증·설정)은 하위 크레이트를 직접 부른다. 결과는 [`crate::dto`]
//! 로 바꾼다 — 명령 계층에는 화면 로직을 두지 않는다.

pub mod auth;
pub mod chat;
pub mod content;
pub mod download;
pub mod insights;
pub mod live;
pub mod media;
pub mod multi_live;
pub mod player;
pub mod recording;
pub mod settings;

use serde::Serialize;

/// 프런트엔드로 가는 이벤트 이름.
pub mod events {
    /// 인증 상태 변경 — `AuthStatusDto`.
    pub const AUTH_CHANGED: &str = "auth://changed";
    /// 로그인 창 실패/취소 — `{ message }`.
    pub const AUTH_LOGIN_FAILED: &str = "auth://login-failed";
    /// 채팅 이벤트 — `ChatEventDto`.
    pub const CHAT_EVENT: &str = "chat://event";
    /// 추가 채팅 세션 이벤트 — `{ session, event: ChatEventDto }`.
    pub const CHAT_SESSION_EVENT: &str = "chat://session-event";
    /// 플레이어 페이지에서 실행할 스크립트 — `{ script }` (Swift `evaluateJavaScript` 대응).
    pub const PLAYER_COMMAND: &str = "player://command";
    /// 재생 코디네이터 이벤트 — `PlayerEventDto`.
    pub const PLAYER_EVENT: &str = "player://event";
    /// 멀티라이브 패인 플레이어 스크립트 — `{ slot, script }`.
    pub const MULTILIVE_COMMAND: &str = "multilive://command";
    /// 멀티라이브 상태 — `MultiLiveStateDto`.
    pub const MULTILIVE_STATE: &str = "multilive://state";
}

/// 명령 오류 — 프런트엔드에는 사용자용 문구 문자열로 간다.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct CommandError(pub String);

impl Serialize for CommandError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl From<cview_core::error::ApiError> for CommandError {
    fn from(e: cview_core::error::ApiError) -> Self {
        Self(e.to_string())
    }
}

impl From<cview_chat::ChatError> for CommandError {
    fn from(e: cview_chat::ChatError) -> Self {
        Self(e.to_string())
    }
}

impl From<crate::playback::PlaybackPlanError> for CommandError {
    fn from(e: crate::playback::PlaybackPlanError) -> Self {
        Self(e.to_string())
    }
}

impl From<String> for CommandError {
    fn from(e: String) -> Self {
        Self(e)
    }
}

impl From<&str> for CommandError {
    fn from(e: &str) -> Self {
        Self(e.to_string())
    }
}

pub type CmdResult<T> = Result<T, CommandError>;

/// `player://command` 페이로드.
#[derive(Debug, Clone, Serialize)]
pub struct PlayerCommandPayload {
    pub script: String,
}

/// `auth://login-failed` 페이로드.
#[derive(Debug, Clone, Serialize)]
pub struct MessagePayload {
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_error_serializes_as_plain_string() {
        let e = CommandError::from(cview_core::error::ApiError::Unauthorized);
        assert_eq!(
            serde_json::to_string(&e).unwrap(),
            "\"로그인이 필요합니다\""
        );
    }
}
