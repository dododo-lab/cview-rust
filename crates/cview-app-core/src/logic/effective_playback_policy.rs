//! `Sources/CViewApp/Models/EffectivePlaybackPolicy.swift` 포팅.
//!
//! 사용자 설정을 재생 계층이 소비할 하나의 불변 계약으로 정규화한다.

use cview_core::models::live::StreamQuality;
use cview_core::models::settings::PlayerSettings;
use cview_player::policy::QualityPolicyMode;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectivePlaybackPolicy {
    pub auto_play_on_channel_entry: bool,
    pub low_latency_enabled: bool,
    pub preferred_quality: Option<StreamQuality>,
    pub quality_policy: QualityPolicyMode,
    /// 0.5~10초로 정규화한 버퍼.
    pub buffer_duration_seconds: f64,
}

impl EffectivePlaybackPolicy {
    pub fn new(settings: &PlayerSettings) -> Self {
        let (preferred_quality, quality_policy) = if settings.force_highest_quality {
            (None, QualityPolicyMode::LockedSource)
        } else {
            match settings.quality {
                StreamQuality::Auto => (None, QualityPolicyMode::AdaptiveBalanced),
                StreamQuality::Source => (None, QualityPolicyMode::PreferSourceAllowRescue),
                q @ (StreamQuality::High | StreamQuality::Medium | StreamQuality::Low) => {
                    (Some(q), QualityPolicyMode::AdaptiveHigh)
                }
            }
        };
        Self {
            auto_play_on_channel_entry: settings.auto_play,
            low_latency_enabled: settings.low_latency_mode,
            preferred_quality,
            quality_policy,
            buffer_duration_seconds: settings.buffer_duration.clamp(0.5, 10.0),
        }
    }

    pub fn force_highest_quality(&self) -> bool {
        self.quality_policy == QualityPolicyMode::LockedSource
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/EffectivePlaybackPolicyTests.swift`
    use super::*;

    #[test]
    fn default_prefers_source_with_rescue() {
        let settings = PlayerSettings::default();
        let policy = EffectivePlaybackPolicy::new(&settings);
        assert_eq!(settings.quality, StreamQuality::Source);
        assert!(!settings.force_highest_quality);
        assert_eq!(
            policy.quality_policy,
            QualityPolicyMode::PreferSourceAllowRescue
        );
    }

    #[test]
    fn force_highest_locks_source() {
        let settings = PlayerSettings {
            quality: StreamQuality::Low,
            force_highest_quality: true,
            ..Default::default()
        };
        let policy = EffectivePlaybackPolicy::new(&settings);
        assert_eq!(policy.quality_policy, QualityPolicyMode::LockedSource);
        assert_eq!(policy.preferred_quality, None);
        assert!(policy.force_highest_quality());
    }

    #[test]
    fn explicit_quality_is_preferred() {
        let settings = PlayerSettings {
            quality: StreamQuality::Medium,
            ..Default::default()
        };
        let policy = EffectivePlaybackPolicy::new(&settings);
        assert_eq!(policy.quality_policy, QualityPolicyMode::AdaptiveHigh);
        assert_eq!(policy.preferred_quality, Some(StreamQuality::Medium));
        let auto = EffectivePlaybackPolicy::new(&PlayerSettings {
            quality: StreamQuality::Auto,
            ..Default::default()
        });
        assert_eq!(auto.quality_policy, QualityPolicyMode::AdaptiveBalanced);
    }

    #[test]
    fn buffer_is_clamped() {
        let mut settings = PlayerSettings {
            buffer_duration: 99.0,
            ..Default::default()
        };
        assert_eq!(
            EffectivePlaybackPolicy::new(&settings).buffer_duration_seconds,
            10.0
        );
        settings.buffer_duration = 0.01;
        assert_eq!(
            EffectivePlaybackPolicy::new(&settings).buffer_duration_seconds,
            0.5
        );
        settings.buffer_duration = 3.0;
        assert_eq!(
            EffectivePlaybackPolicy::new(&settings).buffer_duration_seconds,
            3.0
        );
    }

    #[test]
    fn toggles_are_preserved() {
        let settings = PlayerSettings {
            auto_play: false,
            low_latency_mode: false,
            ..Default::default()
        };
        let policy = EffectivePlaybackPolicy::new(&settings);
        assert!(!policy.auto_play_on_channel_entry);
        assert!(!policy.low_latency_enabled);
    }
}
