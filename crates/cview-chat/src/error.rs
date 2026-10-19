//! 채팅 오류.
//!
//! `Sources/CViewCore/Errors/AppError.swift` 의 `ChatError` 를 이 크레이트에 로컬로 옮겼다
//! (cview-core 의 `AppError` 는 다른 작업이 소유한다). Swift 의 `CancellationError` 는
//! [`ChatError::Cancelled`] 로 표현한다 — 취소는 실패가 아니라 제어 흐름이다.

/// 채팅 에러. 메시지는 Swift `errorDescription` 과 같다.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChatError {
    #[error("채팅 연결 실패: {0}")]
    ConnectionFailed(String),
    #[error("채팅 인증 실패")]
    AuthenticationFailed,
    #[error("메시지 전송 실패: {0}")]
    SendFailed(String),
    #[error("최대 재연결 시도 횟수 초과")]
    MaxRetriesExceeded,
    #[error("잘못된 채널 ID")]
    InvalidChannelId,
    #[error("서버 오류: {0}")]
    ServerError(String),
    #[error("채팅에 연결되어 있지 않습니다")]
    NotConnected,
    #[error("잘못된 메시지 형식")]
    InvalidMessage,
    /// Swift `CancellationError` — 수동 종료나 새 연결 세대가 이 시도를 대체했다.
    #[error("취소됨")]
    Cancelled,
}

impl ChatError {
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

pub type ChatResult<T> = Result<T, ChatError>;
