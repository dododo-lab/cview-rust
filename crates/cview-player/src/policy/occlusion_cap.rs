//! `Sources/CViewPlayer/OcclusionResolutionCap.swift` 포팅 — keepAlive 가림 중 hls.js 해상도 캡 정책.

/// 가림 지속 후 캡을 걸기까지의 대기(초).
pub const DELAY_SECONDS: f64 = 25.0;
/// 가림 중 적용할 최대 해상도(height).
pub const CAPPED_MAX_HEIGHT: i64 = 480;
/// JS `BG_RESYNC_MIN_DRIFT_S` 와 동일.
pub const BG_RESYNC_MIN_DRIFT_SECONDS: f64 = 2.0;

/// 수동 최고화질 잠금·PiP 활성 중에는 캡을 걸지 않는다. `forced_height_active` 는 API 호환용(무시).
pub fn should_apply_cap(
    force_highest_quality: bool,
    _forced_height_active: bool,
    pip_active: bool,
) -> bool {
    !force_highest_quality && !pip_active
}

/// 백그라운드 리싱크를 생략해도 되는지(드리프트가 이미 작음).
pub fn should_skip_background_resync(
    live_latency_seconds: f64,
    target_latency_seconds: f64,
) -> bool {
    if live_latency_seconds <= 0.0 || target_latency_seconds <= 0.0 {
        return false;
    }
    (live_latency_seconds - target_latency_seconds) < BG_RESYNC_MIN_DRIFT_SECONDS
}

#[cfg(test)]
mod tests {
    //! `Tests/CViewPlayerTests/OcclusionResolutionCapTests.swift` 포팅.
    use super::*;
    use crate::hlsjs::profile::HlsJsStreamingProfile;

    #[test]
    fn constants() {
        assert_eq!(DELAY_SECONDS, 25.0);
        assert_eq!(CAPPED_MAX_HEIGHT, 480);
        assert_eq!(BG_RESYNC_MIN_DRIFT_SECONDS, 2.0);
    }

    #[test]
    fn guards() {
        assert!(should_apply_cap(false, false, false));
        assert!(should_apply_cap(false, true, false));
        assert!(!should_apply_cap(true, false, false));
        assert!(!should_apply_cap(true, true, false));
        assert!(!should_apply_cap(false, false, true));
        assert!(!should_apply_cap(true, false, true));
    }

    #[test]
    fn background_resync_gate() {
        assert!(should_skip_background_resync(3.5, 3.0));
        assert!(should_skip_background_resync(4.9, 3.0));
        assert!(!should_skip_background_resync(5.0, 3.0));
        assert!(!should_skip_background_resync(0.0, 3.0));
        assert!(!should_skip_background_resync(5.0, 0.0));
    }

    #[test]
    fn profile_targets() {
        assert_eq!(
            HlsJsStreamingProfile::UltraLow.target_live_latency_seconds(),
            4.0
        );
        assert_eq!(
            HlsJsStreamingProfile::LowLatency.target_live_latency_seconds(),
            6.0
        );
        assert_eq!(
            HlsJsStreamingProfile::Mirror.target_live_latency_seconds(),
            6.0
        );
        assert_eq!(
            HlsJsStreamingProfile::WebParity.target_live_latency_seconds(),
            3.0
        );
    }
}
