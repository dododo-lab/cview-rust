//! `Sources/CViewCore/Errors/AppError.swift` 포팅 — 통합 에러 타입 시스템.
//!
//! Swift 의 `LocalizedError.errorDescription` 은 [`std::fmt::Display`] 로,
//! `recoverySuggestion` 은 같은 이름의 메서드로 옮겼다. 사용자에게 보이는 문구는
//! 원본과 바이트 단위로 같다(테스트가 고정한다).

use crate::models::player::PlayerError;

/// 앱 최상위 에러 타입.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Network(#[from] NetworkError),
    #[error("{0}")]
    Auth(#[from] AuthError),
    #[error("{0}")]
    Api(#[from] ApiError),
    #[error("{0}")]
    Player(#[from] PlayerError),
    #[error("{0}")]
    Chat(#[from] ChatError),
    #[error("{0}")]
    Persistence(#[from] PersistenceError),
    #[error("알 수 없는 오류: {0}")]
    Unknown(String),
}

impl AppError {
    /// 사용자에게 보이는 문구 (Swift `errorDescription`).
    pub fn error_description(&self) -> String {
        self.to_string()
    }

    /// [UIUX 2026-09-20 C25] `.api` 는 하위 [`ApiError`] 의 값을 **위임**한다.
    /// 그 층이 실제로 던져지는 타입이고, 상태 코드별로 복구 방법이 다르다.
    pub fn recovery_suggestion(&self) -> Option<String> {
        Some(match self {
            AppError::Network(_) => "네트워크 연결을 확인하고 다시 시도해주세요.".into(),
            AppError::Auth(_) => "다시 로그인해주세요.".into(),
            AppError::Api(err) => err
                .recovery_suggestion()
                .unwrap_or_else(|| "잠시 후 다시 시도해주세요.".into()),
            AppError::Player(_) => "다른 품질로 재생하거나 플레이어를 재시작해보세요.".into(),
            AppError::Chat(_) => "채팅 연결을 다시 시도합니다.".into(),
            AppError::Persistence(_) | AppError::Unknown(_) => "앱을 재시작해주세요.".into(),
        })
    }
}

/// 네트워크 에러.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NetworkError {
    #[error("인터넷에 연결되어 있지 않습니다")]
    NoConnection,
    #[error("요청 시간이 초과되었습니다")]
    Timeout,
    #[error("DNS 조회 실패")]
    DnsResolutionFailed,
    #[error("보안 연결 오류")]
    SslError,
    #[error("잘못된 URL: {0}")]
    InvalidUrl(String),
}

impl NetworkError {
    pub fn error_description(&self) -> String {
        self.to_string()
    }
}

/// API 에러 (Swift `APIError`).
///
/// [UIUX 2026-09-20 C25] `Display` 는 **사용자 문구**다. 진단 상세(`DecodingFailed` /
/// `NetworkError` / `MalformedResponse` 의 문자열)는 값으로 보존되지만 화면 문구에는 싣지 않는다.
/// ⚠ 진단 상세를 사용자 문구에 다시 싣지 말 것.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ApiError {
    #[error("로그인이 필요합니다")]
    Unauthorized,
    #[error("서버 응답을 읽지 못했습니다")]
    InvalidResponse,
    #[error("{}", http_error_description(*status_code))]
    HttpError { status_code: i64 },
    #[error("표시할 내용이 없습니다")]
    EmptyContent,
    #[error("서버 응답을 읽지 못했습니다")]
    DecodingFailed(String),
    #[error("네트워크에 연결하지 못했습니다")]
    NetworkError(String),
    /// `retry_after` 는 초 단위.
    #[error("요청이 많아 {}초 후 다시 시도합니다", *retry_after as i64)]
    RateLimited { retry_after: f64 },
    #[error("서버 응답 형식이 예상과 다릅니다")]
    MalformedResponse(String),
}

fn http_error_description(code: i64) -> String {
    match code {
        401 => "로그인이 필요합니다".into(),
        403 => "접근 권한이 없습니다".into(),
        404 => "요청한 정보를 찾을 수 없습니다".into(),
        429 => "요청이 많아 잠시 제한되었습니다".into(),
        500..=599 => "서버에 문제가 생겼습니다".into(),
        _ => format!("요청을 처리하지 못했습니다 ({code})"),
    }
}

/// 복구 액션 종류. 화면은 이 값으로 버튼을 고른다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecoveryKind {
    /// 재시도 버튼.
    Retry,
    /// 로그인 유도.
    Login,
    /// 기다리면 해결 — 버튼 없이 안내만.
    Wait,
    /// 복구할 것이 없음(빈 결과 등).
    None,
}

impl ApiError {
    /// 사용자에게 보이는 문구 (Swift `errorDescription`).
    pub fn error_description(&self) -> String {
        self.to_string()
    }

    /// 이 오류에서 사용자가 **할 수 있는 일**.
    pub fn recovery_suggestion(&self) -> Option<String> {
        match self.recovery_kind() {
            RecoveryKind::Login => Some("다시 로그인하면 이어서 이용할 수 있습니다.".into()),
            RecoveryKind::Wait => Some("잠시 후 자동으로 다시 시도합니다.".into()),
            RecoveryKind::Retry => Some("네트워크 상태를 확인하고 다시 시도해 주세요.".into()),
            RecoveryKind::None => None,
        }
    }

    /// 화면이 어떤 복구 **액션**을 제시해야 하는가.
    pub fn recovery_kind(&self) -> RecoveryKind {
        match self {
            ApiError::Unauthorized => RecoveryKind::Login,
            ApiError::HttpError { status_code: 401 | 403 } => RecoveryKind::Login,
            ApiError::RateLimited { .. } => RecoveryKind::Wait,
            ApiError::HttpError { status_code: 429 } => RecoveryKind::Wait,
            ApiError::EmptyContent => RecoveryKind::None,
            _ => RecoveryKind::Retry,
        }
    }

    /// 편의 생성자 — `ApiError::HttpError { status_code }`.
    pub fn http(status_code: i64) -> Self {
        ApiError::HttpError { status_code }
    }
}

/// 인증 에러.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    #[error("로그인이 필요합니다")]
    NotLoggedIn,
    #[error("인증 쿠키가 만료되었습니다")]
    CookieExpired,
    #[error("OAuth 인증 실패: {0}")]
    OauthFailed(String),
    #[error("키체인 접근이 거부되었습니다")]
    KeychainAccessDenied,
    #[error("토큰 갱신 실패")]
    TokenRefreshFailed,
}

impl AuthError {
    pub fn error_description(&self) -> String {
        self.to_string()
    }
}

/// 채팅 에러.
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
}

impl ChatError {
    pub fn error_description(&self) -> String {
        self.to_string()
    }
}

/// 데이터 영속화 에러.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PersistenceError {
    #[error("저장 실패: {0}")]
    SaveFailed(String),
    #[error("조회 실패: {0}")]
    FetchFailed(String),
    #[error("마이그레이션 실패: {0}")]
    MigrationFailed(String),
    #[error("데이터 저장소가 로드되지 않았습니다")]
    ContainerNotLoaded,
}

impl PersistenceError {
    pub fn error_description(&self) -> String {
        self.to_string()
    }
}

#[cfg(test)]
mod tests {
    //! `Tests/CViewCoreTests/AppErrorTests.swift` 포팅.
    use super::*;

    #[test]
    fn app_error_descriptions() {
        assert_eq!(AppError::Network(NetworkError::NoConnection).to_string(), "인터넷에 연결되어 있지 않습니다");
        assert_eq!(AppError::Auth(AuthError::NotLoggedIn).to_string(), "로그인이 필요합니다");
        assert_eq!(AppError::Api(ApiError::Unauthorized).to_string(), "로그인이 필요합니다");
        assert_eq!(AppError::Player(PlayerError::StreamNotFound).to_string(), "스트림을 찾을 수 없습니다");
        assert_eq!(AppError::Chat(ChatError::NotConnected).to_string(), "채팅에 연결되어 있지 않습니다");
        assert_eq!(
            AppError::Persistence(PersistenceError::ContainerNotLoaded).to_string(),
            "데이터 저장소가 로드되지 않았습니다"
        );
        assert_eq!(AppError::Unknown("테스트".into()).error_description(), "알 수 없는 오류: 테스트");
    }

    #[test]
    fn app_error_recovery_suggestions() {
        assert_eq!(
            AppError::Network(NetworkError::Timeout).recovery_suggestion().as_deref(),
            Some("네트워크 연결을 확인하고 다시 시도해주세요.")
        );
        assert_eq!(AppError::Auth(AuthError::NotLoggedIn).recovery_suggestion().as_deref(), Some("다시 로그인해주세요."));
        assert_eq!(
            AppError::Api(ApiError::Unauthorized).recovery_suggestion().as_deref(),
            Some("다시 로그인하면 이어서 이용할 수 있습니다.")
        );
        assert_eq!(
            AppError::Api(ApiError::NetworkError("x".into())).recovery_suggestion().as_deref(),
            Some("네트워크 상태를 확인하고 다시 시도해 주세요.")
        );
        assert_eq!(
            AppError::Api(ApiError::EmptyContent).recovery_suggestion().as_deref(),
            Some("잠시 후 다시 시도해주세요.")
        );
        assert_eq!(
            AppError::Player(PlayerError::StreamNotFound).recovery_suggestion().as_deref(),
            Some("다른 품질로 재생하거나 플레이어를 재시작해보세요.")
        );
        assert_eq!(AppError::Chat(ChatError::NotConnected).recovery_suggestion().as_deref(), Some("채팅 연결을 다시 시도합니다."));
        assert_eq!(
            AppError::Persistence(PersistenceError::ContainerNotLoaded).recovery_suggestion().as_deref(),
            Some("앱을 재시작해주세요.")
        );
        assert_eq!(AppError::Unknown("x".into()).recovery_suggestion().as_deref(), Some("앱을 재시작해주세요."));
    }

    #[test]
    fn network_error_descriptions() {
        assert_eq!(NetworkError::NoConnection.to_string(), "인터넷에 연결되어 있지 않습니다");
        assert_eq!(NetworkError::Timeout.to_string(), "요청 시간이 초과되었습니다");
        assert_eq!(NetworkError::DnsResolutionFailed.to_string(), "DNS 조회 실패");
        assert_eq!(NetworkError::SslError.to_string(), "보안 연결 오류");
        assert_eq!(NetworkError::InvalidUrl("http://bad".into()).to_string(), "잘못된 URL: http://bad");
    }

    #[test]
    fn api_error_user_copy() {
        assert_eq!(ApiError::Unauthorized.to_string(), "로그인이 필요합니다");
        assert_eq!(ApiError::InvalidResponse.to_string(), "서버 응답을 읽지 못했습니다");
        assert_eq!(ApiError::http(403).to_string(), "접근 권한이 없습니다");
        assert_eq!(ApiError::http(404).to_string(), "요청한 정보를 찾을 수 없습니다");
        assert_eq!(ApiError::http(429).to_string(), "요청이 많아 잠시 제한되었습니다");
        assert_eq!(ApiError::http(503).to_string(), "서버에 문제가 생겼습니다");
        assert_eq!(ApiError::http(999).to_string(), "요청을 처리하지 못했습니다 (999)");
        assert_eq!(ApiError::EmptyContent.to_string(), "표시할 내용이 없습니다");
        assert_eq!(ApiError::NetworkError("timeout".into()).to_string(), "네트워크에 연결하지 못했습니다");
        assert_eq!(ApiError::RateLimited { retry_after: 30.0 }.to_string(), "요청이 많아 30초 후 다시 시도합니다");
    }

    #[test]
    fn diagnostic_detail_stays_out_of_user_copy() {
        let decoding = ApiError::DecodingFailed("keyNotFound(CodingKeys.channelId)".into());
        assert_eq!(decoding.to_string(), "서버 응답을 읽지 못했습니다");
        assert!(!decoding.to_string().contains("CodingKeys"));
        let malformed = ApiError::MalformedResponse("content.live == null".into());
        assert_eq!(malformed.to_string(), "서버 응답 형식이 예상과 다릅니다");
        assert!(!malformed.to_string().contains("null"));
    }

    #[test]
    fn recovery_kinds() {
        assert_eq!(ApiError::Unauthorized.recovery_kind(), RecoveryKind::Login);
        assert_eq!(ApiError::http(401).recovery_kind(), RecoveryKind::Login);
        assert_eq!(ApiError::http(403).recovery_kind(), RecoveryKind::Login);
        assert_eq!(ApiError::RateLimited { retry_after: 5.0 }.recovery_kind(), RecoveryKind::Wait);
        assert_eq!(ApiError::http(429).recovery_kind(), RecoveryKind::Wait);
        assert_eq!(ApiError::EmptyContent.recovery_kind(), RecoveryKind::None);
        assert_eq!(ApiError::NetworkError("x".into()).recovery_kind(), RecoveryKind::Retry);
        assert_eq!(ApiError::http(500).recovery_kind(), RecoveryKind::Retry);
        assert!(ApiError::Unauthorized.recovery_suggestion().unwrap().contains("로그인"));
        assert_eq!(ApiError::EmptyContent.recovery_suggestion(), None);
    }

    #[test]
    fn api_error_equality() {
        assert_eq!(ApiError::Unauthorized, ApiError::Unauthorized);
        assert_eq!(ApiError::http(200), ApiError::http(200));
        assert_ne!(ApiError::http(200), ApiError::http(404));
        assert_eq!(ApiError::RateLimited { retry_after: 10.0 }, ApiError::RateLimited { retry_after: 10.0 });
        assert_ne!(ApiError::RateLimited { retry_after: 10.0 }, ApiError::RateLimited { retry_after: 30.0 });
    }

    #[test]
    fn auth_chat_persistence_descriptions() {
        assert_eq!(AuthError::NotLoggedIn.to_string(), "로그인이 필요합니다");
        assert_eq!(AuthError::CookieExpired.to_string(), "인증 쿠키가 만료되었습니다");
        assert_eq!(AuthError::OauthFailed("reason".into()).to_string(), "OAuth 인증 실패: reason");
        assert_eq!(AuthError::KeychainAccessDenied.to_string(), "키체인 접근이 거부되었습니다");
        assert_eq!(AuthError::TokenRefreshFailed.to_string(), "토큰 갱신 실패");

        assert_eq!(ChatError::ConnectionFailed("timeout".into()).to_string(), "채팅 연결 실패: timeout");
        assert_eq!(ChatError::AuthenticationFailed.to_string(), "채팅 인증 실패");
        assert_eq!(ChatError::SendFailed("오류".into()).to_string(), "메시지 전송 실패: 오류");
        assert_eq!(ChatError::MaxRetriesExceeded.to_string(), "최대 재연결 시도 횟수 초과");
        assert_eq!(ChatError::InvalidChannelId.to_string(), "잘못된 채널 ID");
        assert_eq!(ChatError::ServerError("500".into()).to_string(), "서버 오류: 500");
        assert_eq!(ChatError::NotConnected.to_string(), "채팅에 연결되어 있지 않습니다");
        assert_eq!(ChatError::InvalidMessage.to_string(), "잘못된 메시지 형식");

        assert_eq!(PersistenceError::SaveFailed("disk full".into()).to_string(), "저장 실패: disk full");
        assert_eq!(PersistenceError::FetchFailed("not found".into()).to_string(), "조회 실패: not found");
        assert_eq!(PersistenceError::MigrationFailed("v2".into()).to_string(), "마이그레이션 실패: v2");
        assert_eq!(PersistenceError::ContainerNotLoaded.to_string(), "데이터 저장소가 로드되지 않았습니다");
    }

    #[test]
    fn player_error_descriptions() {
        assert_eq!(PlayerError::StreamNotFound.to_string(), "스트림을 찾을 수 없습니다");
        assert_eq!(PlayerError::NetworkTimeout.to_string(), "네트워크 연결 시간 초과");
        assert_eq!(PlayerError::DecodingFailed("corrupt".into()).to_string(), "디코딩 실패: corrupt");
        assert_eq!(PlayerError::EngineInitFailed.to_string(), "플레이어 엔진 초기화 실패");
        assert_eq!(PlayerError::UnsupportedFormat("mkv".into()).to_string(), "지원하지 않는 포맷: mkv");
        assert_eq!(PlayerError::HlsParsingFailed("x".into()).to_string(), "HLS 파싱 실패: x");
        assert_eq!(PlayerError::InvalidManifest.to_string(), "잘못된 매니페스트");
        assert_eq!(PlayerError::ConnectionLost.to_string(), "연결이 끊어졌습니다");
        assert_eq!(PlayerError::AuthRequired.to_string(), "인증이 필요합니다");
        assert_eq!(PlayerError::RecordingFailed("perm".into()).to_string(), "녹화 실패: perm");
    }
}
