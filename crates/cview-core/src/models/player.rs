//! `Sources/CViewCore/Models/PlayerModels.swift` 포팅 — 플레이어 도메인 모델.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::live::StreamQuality;

/// 플레이어 상태. 시간 값은 초 단위 `f64` (Swift `TimeInterval`).
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerState {
    pub phase: PlayerPhase,
    pub current_time: f64,
    pub duration: f64,
    pub buffered_duration: f64,
    pub playback_rate: f64,
    pub volume: f32,
    pub latency: Option<f64>,
    pub quality: StreamQuality,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            phase: PlayerPhase::Idle,
            current_time: 0.0,
            duration: 0.0,
            buffered_duration: 0.0,
            playback_rate: 1.0,
            volume: 1.0,
            latency: None,
            quality: StreamQuality::Auto,
        }
    }
}

impl PlayerState {
    pub fn is_active(&self) -> bool {
        matches!(
            self.phase,
            PlayerPhase::Playing | PlayerPhase::Buffering { .. } | PlayerPhase::Paused
        )
    }
}

/// 플레이어 재생 단계 (Swift `PlayerState.Phase`).
#[derive(Debug, Clone, PartialEq)]
pub enum PlayerPhase {
    Idle,
    Loading,
    Buffering { progress: f64 },
    Playing,
    Paused,
    Error(PlayerError),
    Ended,
}

/// 플레이어 에러.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlayerError {
    #[error("스트림을 찾을 수 없습니다")]
    StreamNotFound,
    #[error("네트워크 연결 시간 초과")]
    NetworkTimeout,
    #[error("디코딩 실패: {0}")]
    DecodingFailed(String),
    #[error("플레이어 엔진 초기화 실패")]
    EngineInitFailed,
    #[error("지원하지 않는 포맷: {0}")]
    UnsupportedFormat(String),
    #[error("HLS 파싱 실패: {0}")]
    HlsParsingFailed(String),
    #[error("잘못된 매니페스트")]
    InvalidManifest,
    #[error("연결이 끊어졌습니다")]
    ConnectionLost,
    #[error("인증이 필요합니다")]
    AuthRequired,
    #[error("녹화 실패: {0}")]
    RecordingFailed(String),
    /// [SHOT-1 2026-08-11] 화면 캡처 실패.
    #[error("화면 캡처 실패: {0}")]
    ScreenshotFailed(String),
}

impl PlayerError {
    pub fn error_description(&self) -> String {
        self.to_string()
    }
}

/// [SHOT-1 2026-08-11] 플레이어 도구(캡처 등) 동작의 단발성 결과 — 성공/실패를 **둘 다** 나른다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerToolStatus {
    pub kind: PlayerToolStatusKind,
    pub message: String,
    pub tool: PlayerTool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlayerToolStatusKind {
    Success,
    Failure,
}

/// 어느 도구의 결과인지. 문구는 값이 소유한다 — 뷰가 추측하지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PlayerTool {
    /// 기본값 — 이 타입이 캡처 전용으로 태어났다(`SHOT-1`).
    #[default]
    Screenshot,
    Recording,
}

impl PlayerTool {
    /// VoiceOver 가 문구 앞에 붙여 읽는 도구 이름.
    pub fn display_name(self) -> &'static str {
        match self {
            PlayerTool::Screenshot => "캡처",
            PlayerTool::Recording => "녹화",
        }
    }
}

impl PlayerToolStatus {
    pub fn new(kind: PlayerToolStatusKind, message: impl Into<String>, tool: PlayerTool) -> Self {
        Self {
            kind,
            message: message.into(),
            tool,
        }
    }

    pub fn is_failure(&self) -> bool {
        self.kind == PlayerToolStatusKind::Failure
    }

    /// 접근성 머리말 — `"녹화 실패"` / `"캡처 완료"`.
    pub fn accessibility_headline(&self) -> String {
        format!(
            "{} {}",
            self.tool.display_name(),
            if self.is_failure() {
                "실패"
            } else {
                "완료"
            }
        )
    }
}

/// 라이브 재생 엔진.
///
/// [엔진 단일화 2026-08-11] 라이브 재생은 hls.js 하나로 확정됐다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlayerEngineType {
    #[serde(rename = "HLS.js")]
    Hlsjs,
}

impl PlayerEngineType {
    pub const ALL: [PlayerEngineType; 1] = [PlayerEngineType::Hlsjs];

    pub fn display_name(self) -> &'static str {
        match self {
            PlayerEngineType::Hlsjs => "HLS.js (저지연)",
        }
    }
}

/// 레이턴시 이력 데이터 포인트. `latency` 는 초.
#[derive(Debug, Clone, PartialEq)]
pub struct LatencyDataPoint {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub latency: f64,
}

impl LatencyDataPoint {
    pub fn new(timestamp: DateTime<Utc>, latency: f64) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp,
            latency,
        }
    }

    pub fn latency_duration(&self) -> Duration {
        Duration::from_secs_f64(self.latency.max(0.0))
    }
}

#[cfg(test)]
mod tests {
    //! `Tests/CViewCoreTests/PlayerModelsTests.swift` 포팅(엔진 비의존 부분).
    use super::*;

    #[test]
    fn default_state() {
        let s = PlayerState::default();
        assert_eq!(s.phase, PlayerPhase::Idle);
        assert_eq!(s.playback_rate, 1.0);
        assert_eq!(s.volume, 1.0);
        assert_eq!(s.quality, StreamQuality::Auto);
        assert!(!s.is_active());
    }

    #[test]
    fn is_active_phases() {
        for (phase, active) in [
            (PlayerPhase::Playing, true),
            (PlayerPhase::Paused, true),
            (PlayerPhase::Buffering { progress: 0.5 }, true),
            (PlayerPhase::Loading, false),
            (PlayerPhase::Ended, false),
            (PlayerPhase::Error(PlayerError::ConnectionLost), false),
        ] {
            let s = PlayerState {
                phase,
                ..PlayerState::default()
            };
            assert_eq!(s.is_active(), active);
        }
    }

    #[test]
    fn tool_status_headline() {
        let s = PlayerToolStatus::new(
            PlayerToolStatusKind::Failure,
            "녹화 저장 실패",
            PlayerTool::Recording,
        );
        assert!(s.is_failure());
        assert_eq!(s.accessibility_headline(), "녹화 실패");
        let ok = PlayerToolStatus::new(PlayerToolStatusKind::Success, "ok", PlayerTool::default());
        assert_eq!(ok.accessibility_headline(), "캡처 완료");
    }

    #[test]
    fn engine_type() {
        assert_eq!(
            serde_json::to_string(&PlayerEngineType::Hlsjs).unwrap(),
            "\"HLS.js\""
        );
        assert_eq!(PlayerEngineType::Hlsjs.display_name(), "HLS.js (저지연)");
        assert_eq!(
            PlayerError::ScreenshotFailed("x".into()).to_string(),
            "화면 캡처 실패: x"
        );
    }

    #[test]
    fn latency_points_have_unique_ids() {
        let now = Utc::now();
        assert_ne!(
            LatencyDataPoint::new(now, 1.0).id,
            LatencyDataPoint::new(now, 1.0).id
        );
    }
}
