//! `Sources/CViewAuth/OAuthRefreshFailures.swift` 포팅 — OAuth 오류와 토큰 갱신 실패 분류.
//!
//! Swift 는 오류 타입 여러 개(`OAuthTokenRejected`, `OAuthTokenEndpointUnavailable`,
//! `OAuthCredentialsUnavailable`, `OAuthError`, `URLError`, `CancellationError`, `AuthError`)를 `is` 로
//! 분류했다. Rust 는 [`AuthServiceError`] 한 enum 의 변형으로 모았다. 분류 규칙
//! (`AuthManager+SessionRecovery.swift`)은 같다:
//!
//! - **전송 계층**(HTTP 응답을 못 받음) — 백오프를 늘리지 않고 15초 뒤 다시.
//! - **일시**(전송 계층 · 5xx/408/429 · 자격 증명 읽기 실패) — 부팅은 토큰을 쥔 채 로그인으로 끝낸다.
//! - **최종**(4xx 거절 · 자격 증명 없음) — 토큰을 버린다.
//! - 그 밖(3xx · 본문 해석 실패 등)은 미분류 — 부팅은 저장 토큰을 남긴 채 로그아웃으로 끝낸다.

use cview_core::error::AuthError;
use cview_networking::TransportError;

use crate::generation::Cancelled;
use crate::secret_store::StoreError;

/// 인증 서비스 오류.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthServiceError {
    /// 세션 세대가 바뀌었다 (Swift `CancellationError`).
    #[error("작업이 취소되었습니다")]
    Cancelled,
    /// 클라이언트 자격 증명이 **없다** (Swift `OAuthError.missingClientCredentials`) — 최종.
    #[error("치지직 OAuth 클라이언트 ID 와 secret 이 설정되지 않았습니다.")]
    MissingClientCredentials,
    /// 자격 증명을 **읽지 못했다**(잠긴 키체인 등, Swift `OAuthCredentialsUnavailable`) — 일시.
    #[error("치지직 OAuth 클라이언트 자격증명을 읽지 못했습니다: {reason}")]
    CredentialsUnavailable { reason: String },
    /// 토큰 엔드포인트가 거절했다 — 4xx(408·429 제외) 또는 토큰 없는 응답 (Swift `OAuthTokenRejected`).
    #[error("{}", AuthError::OauthFailed(detail.clone()))]
    TokenRejected { detail: String },
    /// 토큰 엔드포인트가 지금은 답하지 못했다 — 5xx·408·429 (Swift `OAuthTokenEndpointUnavailable`).
    #[error("{}", AuthError::OauthFailed(format!("HTTP {status}")))]
    EndpointUnavailable { status: u16 },
    /// HTTP 응답을 받지 못했다 (Swift `URLError`).
    #[error("네트워크 오류: {0}")]
    Transport(TransportError),
    /// 그 밖의 인증 오류 (Swift `AuthError`).
    #[error("{0}")]
    Auth(AuthError),
    /// 응답 본문을 해석하지 못했다 (Swift `DecodingError`).
    #[error("응답을 해석하지 못했습니다: {0}")]
    Decoding(String),
    /// 저장소 오류.
    #[error("{0}")]
    Store(StoreError),
}

impl From<Cancelled> for AuthServiceError {
    fn from(_: Cancelled) -> Self {
        AuthServiceError::Cancelled
    }
}

impl From<StoreError> for AuthServiceError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::Cancelled => AuthServiceError::Cancelled,
            other => AuthServiceError::Store(other),
        }
    }
}

impl From<AuthError> for AuthServiceError {
    fn from(e: AuthError) -> Self {
        AuthServiceError::Auth(e)
    }
}

impl From<TransportError> for AuthServiceError {
    fn from(e: TransportError) -> Self {
        AuthServiceError::Transport(e)
    }
}

impl AuthServiceError {
    /// 화면 계층용 [`AuthError`] 로 접는다.
    pub fn to_auth_error(&self) -> AuthError {
        match self {
            AuthServiceError::Auth(e) => e.clone(),
            AuthServiceError::Store(_) => AuthError::KeychainAccessDenied,
            other => AuthError::OauthFailed(other.to_string()),
        }
    }

    /// 4xx(408·429 제외)는 거절.
    pub fn is_rejection(status: u16) -> bool {
        (400..500).contains(&status) && status != 408 && status != 429
    }

    /// 5xx·408·429 는 일시 불가.
    pub fn is_unavailable(status: u16) -> bool {
        (500..600).contains(&status) || status == 408 || status == 429
    }

    /// 토큰 엔드포인트의 200 아닌 응답 → 던질 오류 (Swift `ChzzkOAuthService.tokenFailure(status:)`).
    /// ⚠ 토큰 요청은 반드시 이것을 거친다 — 5xx 가 미분류로 떨어지면 점검 중 부팅이 로그아웃으로 끝난다.
    pub fn token_failure(status: u16) -> Self {
        if Self::is_rejection(status) {
            AuthServiceError::TokenRejected {
                detail: format!("HTTP {status}"),
            }
        } else if Self::is_unavailable(status) {
            AuthServiceError::EndpointUnavailable { status }
        } else {
            AuthServiceError::Auth(AuthError::OauthFailed(format!("HTTP {status}")))
        }
    }

    /// HTTP 응답을 받지 못한 실패 (Swift `isTransportFailure`).
    pub fn is_transport_failure(&self) -> bool {
        matches!(self, AuthServiceError::Transport(_))
    }

    /// 시간이 지나면 풀릴 수 있는 갱신 실패 (Swift `isTransientRefreshFailure`).
    pub fn is_transient_refresh_failure(&self) -> bool {
        self.is_transport_failure()
            || matches!(
                self,
                AuthServiceError::EndpointUnavailable { .. }
                    | AuthServiceError::CredentialsUnavailable { .. }
            )
    }

    /// 같은 refresh token 으로 다시 해도 성공하지 않는 실패 (Swift `isTerminalRefreshFailure`).
    pub fn is_terminal_refresh_failure(&self) -> bool {
        matches!(
            self,
            AuthServiceError::TokenRejected { .. } | AuthServiceError::MissingClientCredentials
        )
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `PrecisionReview0926AuthStartupTests.swift` 의 분류 테스트.
    use super::*;

    #[test]
    fn refresh_failure_classification() {
        for e in [
            TransportError::NotConnected,
            TransportError::TimedOut("t".into()),
            TransportError::ConnectionLost("l".into()),
            TransportError::Other("tls".into()),
        ] {
            let e = AuthServiceError::Transport(e);
            assert!(e.is_transport_failure(), "{e:?}");
            assert!(e.is_transient_refresh_failure(), "{e:?}");
            assert!(!e.is_terminal_refresh_failure(), "{e:?}");
        }
        assert!(
            !AuthServiceError::EndpointUnavailable { status: 503 }.is_transport_failure(),
            "서버가 답한 실패는 백오프를 늘린다"
        );
        for status in [500, 503, 408, 429] {
            assert!(AuthServiceError::EndpointUnavailable { status }.is_transient_refresh_failure());
            assert!(AuthServiceError::is_unavailable(status));
        }
        assert!(!AuthServiceError::is_unavailable(401));
        let rejected = AuthServiceError::TokenRejected {
            detail: "HTTP 401".into(),
        };
        assert!(!rejected.is_transient_refresh_failure());
        assert!(rejected.is_terminal_refresh_failure());
        assert!(!AuthServiceError::MissingClientCredentials.is_transient_refresh_failure());
        assert!(AuthServiceError::MissingClientCredentials.is_terminal_refresh_failure());
        let generic = AuthServiceError::Auth(AuthError::OauthFailed("Invalid response".into()));
        assert!(!generic.is_transient_refresh_failure());
        assert!(!generic.is_terminal_refresh_failure());
        assert!(
            !AuthServiceError::EndpointUnavailable { status: 503 }.is_terminal_refresh_failure()
        );
        let unreadable = AuthServiceError::CredentialsUnavailable {
            reason: "locked".into(),
        };
        assert!(
            unreadable.is_transient_refresh_failure(),
            "잠긴 키체인은 풀리면 읽힌다"
        );
        assert!(!unreadable.is_terminal_refresh_failure());
        assert!(!unreadable.is_transport_failure());
    }

    #[test]
    fn token_endpoint_status_mapping() {
        for status in [500, 502, 503, 408, 429] {
            let e = AuthServiceError::token_failure(status);
            assert!(e.is_transient_refresh_failure(), "HTTP {status}");
            assert!(!e.is_terminal_refresh_failure(), "HTTP {status}");
        }
        for status in [400, 401, 403] {
            let e = AuthServiceError::token_failure(status);
            assert!(e.is_terminal_refresh_failure(), "HTTP {status}");
            assert!(!e.is_transient_refresh_failure(), "HTTP {status}");
        }
        for status in [302, 304] {
            let e = AuthServiceError::token_failure(status);
            assert!(
                !e.is_transient_refresh_failure() && !e.is_terminal_refresh_failure(),
                "HTTP {status}"
            );
        }
    }

    #[test]
    fn messages() {
        assert_eq!(
            AuthServiceError::TokenRejected {
                detail: "HTTP 401".into()
            }
            .to_string(),
            "OAuth 인증 실패: HTTP 401"
        );
        assert_eq!(
            AuthServiceError::EndpointUnavailable { status: 503 }.to_string(),
            "OAuth 인증 실패: HTTP 503"
        );
        assert_eq!(
            AuthServiceError::MissingClientCredentials.to_string(),
            "치지직 OAuth 클라이언트 ID 와 secret 이 설정되지 않았습니다."
        );
        assert_eq!(
            AuthServiceError::from(StoreError::Cancelled),
            AuthServiceError::Cancelled
        );
    }
}
