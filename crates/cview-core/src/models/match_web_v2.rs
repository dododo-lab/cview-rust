//! `Sources/CViewCore/Models/MatchWebV2Flag.swift` 포팅 — 웹 동기화 v2 실험 플래그.
//!
//! Swift 는 `UserDefaults` 덮어쓰기를 읽는다. Rust 쪽은 저장소 의존을 없애고 덮어쓰기 값을
//! 호출자가 `Option` 으로 넘긴다(`None` = 키 없음).

use super::live::LiveInfo;

/// 웹 동기화 v2 플래그 판정.
pub struct MatchWebV2Flag;

impl MatchWebV2Flag {
    /// Bool — 키가 있으면(YES/NO 모두) 설정보다 우선한다.
    pub const ENABLED_OVERRIDE_KEY: &'static str = "CViewMatchWebV2";
    /// Double — v2 의 ingest 목표 T_base(초) 덮어쓰기.
    pub const INGEST_TARGET_OVERRIDE_KEY: &'static str = "CViewMatchWebV2IngestTarget";
    pub const DEFAULT_INGEST_TARGET_SECONDS: f64 = 3.3;
    pub const INGEST_TARGET_RANGE: std::ops::RangeInclusive<f64> = 2.0..=6.0;

    /// 덮어쓰기가 **있으면** 그 값, 없으면 설정 값.
    pub fn resolve_enabled(settings_value: bool, override_value: Option<bool>) -> bool {
        override_value.unwrap_or(settings_value)
    }

    /// 덮어쓰기가 있고 유한·범위 안이면 그 값, 아니면 기본값.
    pub fn resolve_ingest_target(override_value: Option<f64>) -> f64 {
        match override_value {
            Some(v) if v.is_finite() && Self::INGEST_TARGET_RANGE.contains(&v) => v,
            _ => Self::DEFAULT_INGEST_TARGET_SECONDS,
        }
    }

    /// 재생 소스가 v2 대상인가 — 워치파티·라디오(오디오 전용)는 제외한다.
    pub fn source_eligible(is_watch_party: bool, is_audio_only: bool) -> bool {
        !is_watch_party && !is_audio_only
    }

    /// `livePlaybackJSON` 이 비어 있으면 라디오 JSON 으로 재생한다.
    pub fn source_eligible_live(live: &LiveInfo) -> bool {
        Self::source_eligible(live.is_watch_party(), live.live_playback_json.as_deref().unwrap_or("").is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve() {
        assert!(MatchWebV2Flag::resolve_enabled(true, None));
        assert!(!MatchWebV2Flag::resolve_enabled(true, Some(false)));
        assert!(MatchWebV2Flag::resolve_enabled(false, Some(true)));
        assert_eq!(MatchWebV2Flag::resolve_ingest_target(None), 3.3);
        assert_eq!(MatchWebV2Flag::resolve_ingest_target(Some(4.0)), 4.0);
        assert_eq!(MatchWebV2Flag::resolve_ingest_target(Some(0.0)), 3.3);
        assert_eq!(MatchWebV2Flag::resolve_ingest_target(Some(f64::NAN)), 3.3);
        assert!(MatchWebV2Flag::source_eligible(false, false));
        assert!(!MatchWebV2Flag::source_eligible(true, false));
        assert!(!MatchWebV2Flag::source_eligible(false, true));
    }
}
