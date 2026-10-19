//! `Sources/CViewPlayer/QualityPolicyMode.swift` 포팅 — 화질 정책 모드.

use serde::{Deserialize, Serialize};

/// 화질 결정 정책.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum QualityPolicyMode {
    /// 절대 하향 금지.
    LockedSource,
    /// 최고 화질 선호, 버퍼 위기 시 임시 하향 허용.
    PreferSourceAllowRescue,
    /// 1080p 선호하되 ABR 자유.
    AdaptiveHigh,
    /// 균형 — ABR 이 가장 적극적.
    AdaptiveBalanced,
}

impl QualityPolicyMode {
    pub fn allows_downgrade(self) -> bool {
        !matches!(self, Self::LockedSource)
    }

    pub fn honors_bandwidth_cap(self) -> bool {
        !matches!(self, Self::LockedSource)
    }

    pub fn prefers_source_quality(self) -> bool {
        matches!(self, Self::LockedSource | Self::PreferSourceAllowRescue)
    }

    /// `forceHighestQuality` boolean 호환 매핑.
    pub fn from_legacy_force_highest_quality(flag: bool) -> Self {
        if flag {
            Self::LockedSource
        } else {
            Self::AdaptiveHigh
        }
    }
}

#[cfg(test)]
mod tests {
    //! `Tests/CViewPlayerTests/QualityPolicyModeTests.swift` 포팅(모드 부분).
    use super::QualityPolicyMode::*;
    use super::*;

    #[test]
    fn flags() {
        assert!(!LockedSource.allows_downgrade() && !LockedSource.honors_bandwidth_cap());
        for m in [PreferSourceAllowRescue, AdaptiveHigh, AdaptiveBalanced] {
            assert!(m.allows_downgrade() && m.honors_bandwidth_cap());
        }
        assert!(LockedSource.prefers_source_quality());
        assert!(PreferSourceAllowRescue.prefers_source_quality());
        assert!(!AdaptiveHigh.prefers_source_quality());
        assert!(!AdaptiveBalanced.prefers_source_quality());
    }

    #[test]
    fn legacy_mapping() {
        assert_eq!(
            QualityPolicyMode::from_legacy_force_highest_quality(true),
            LockedSource
        );
        assert_eq!(
            QualityPolicyMode::from_legacy_force_highest_quality(false),
            AdaptiveHigh
        );
        assert!(QualityPolicyMode::from_legacy_force_highest_quality(true).prefers_source_quality());
        assert!(
            !QualityPolicyMode::from_legacy_force_highest_quality(false).prefers_source_quality()
        );
    }
}
