//! `Sources/CViewPlayer/RecoverySignal.swift` 포팅 — 재생 복구 트리거 통합 모델.
//!
//! 신호 → 결정 매핑(`decide_recovery`)은 코디네이터(`coordinator::recovery`)가 소유한다.

use cview_core::models::player::PlayerEngineType;

/// 재생 복구가 필요할 수 있는 신호.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoverySignal {
    /// 엔진이 명시적 에러로 전환됨.
    EngineError {
        engine: PlayerEngineType,
        detail: String,
    },
    /// 재생 시간 정체·디코더 stall.
    PlaybackStall { source: String },
    /// 매니페스트 주기 갱신이 N회 연속 실패.
    ManifestRefreshFailure { count: i64 },
    /// 업스트림 인증 실패 (proxy 403 등).
    UpstreamAuthFailure { status: u16 },
    /// 방송 종료가 API 로 확정됨 (재시도하지 않아야 함).
    StreamEndedConfirmed,
}

/// supervisor 가 내려주는 결정.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryDecision {
    /// 무시 — cooldown 중이거나 이미 처리 중.
    Ignore { reason: String },
    /// 매니페스트 갱신만 수행.
    RefreshManifest,
    /// 재연결 — 마스터 URL 부터 재해석.
    Reconnect,
    /// 방송 종료로 확정.
    MarkEnded,
    /// 사용자에게 보여줄 명시 실패.
    FailUserVisible(String),
}
