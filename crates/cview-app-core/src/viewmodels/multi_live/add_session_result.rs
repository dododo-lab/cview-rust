//! 멀티라이브 세션 추가 결과.
//!
//! 원본: `Sources/CViewApp/ViewModels/MultiLiveAddSessionResult.swift`.

use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MultiLiveAddSessionResult {
    Added { session_id: Uuid },
    ActivatedExisting,
    MaxSessionsReached,
    AlreadyInProgress,
    LiveDetailTimeout,
    NotLive,
    ApiUnavailable,
    EngineUnavailable,
    ApiError { message: String },
    Cancelled,
}

impl MultiLiveAddSessionResult {
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Added { .. } | Self::ActivatedExisting)
    }

    pub fn is_new_session(&self) -> bool {
        matches!(self, Self::Added { .. })
    }

    /// 첫 성공, 없으면 첫 비취소 실패, 모두 취소면 취소.
    pub fn batch_outcome(results: &[Self]) -> Self {
        results
            .iter()
            .find(|r| r.is_success())
            .or_else(|| results.iter().find(|r| **r != Self::Cancelled))
            .cloned()
            .unwrap_or(Self::Cancelled)
    }

    pub fn user_message(&self) -> Option<String> {
        match self {
            Self::Added { .. }
            | Self::ActivatedExisting
            | Self::Cancelled
            | Self::AlreadyInProgress => None,
            Self::MaxSessionsReached => Some("최대 시청 수에 도달했습니다".into()),
            Self::LiveDetailTimeout => {
                Some("채널 정보 확인 지연 — 네트워크 상태를 확인하세요".into())
            }
            Self::NotLive => Some("방송이 종료되었습니다 — 검색 결과를 새로 고침하세요".into()),
            Self::ApiUnavailable => {
                Some("앱이 아직 준비 중입니다 — 잠시 후 다시 시도하세요".into())
            }
            Self::EngineUnavailable => {
                Some("플레이어 엔진 할당 실패 — 잠시 후 다시 시도하세요".into())
            }
            Self::ApiError { message } => Some(if message.is_empty() {
                "채널 추가 실패 — 잠시 후 다시 시도하세요".into()
            } else {
                format!("채널 추가 실패 — {message}")
            }),
        }
    }
}
