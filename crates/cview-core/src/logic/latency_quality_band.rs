//! 지연 시간 품질 구간 — 값 → 등급의 단일 출처.
//!
//! 원본: `Sources/CViewCore/Logic/LatencyQualityBand.swift`
//!
//! 상태는 색만으로 전달하지 않는다 — 라벨·기호와 함께 온다. 원본의 `color`(SwiftUI
//! `DesignTokens.Colors`)는 UI 계층 값이라 의미 토큰 [`StatusTone`] 으로 바꿨다.

/// 디자인 시스템 상태색의 의미 토큰(`Colors.success` / `.warning` / `.error`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StatusTone {
    Success,
    Warning,
    Error,
}

/// 스트림 지연 시간의 품질 구간.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LatencyQualityBand {
    Good,
    Fair,
    Poor,
}

impl LatencyQualityBand {
    pub const ALL: [Self; 3] = [Self::Good, Self::Fair, Self::Poor];

    /// 구간 경계(초). 임계선을 그릴 때도 이 값을 쓴다.
    pub const FAIR_THRESHOLD: f64 = 3.0;
    pub const POOR_THRESHOLD: f64 = 5.0;

    pub fn from_seconds(seconds: f64) -> Self {
        if seconds < Self::FAIR_THRESHOLD {
            Self::Good
        } else if seconds < Self::POOR_THRESHOLD {
            Self::Fair
        } else {
            Self::Poor
        }
    }

    /// 범례·툴팁에 쓰는 구간 표기.
    pub fn label(self) -> String {
        let fair = Self::FAIR_THRESHOLD as i64;
        let poor = Self::POOR_THRESHOLD as i64;
        match self {
            Self::Good => format!("< {fair}초"),
            Self::Fair => format!("{fair}-{poor}초"),
            Self::Poor => format!("> {poor}초"),
        }
    }

    /// 색 **외의** 부호(SF Symbols 이름). 색약 사용자와 흑백 인쇄에서 등급을 전달한다.
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Good => "checkmark.circle.fill",
            Self::Fair => "exclamationmark.circle.fill",
            Self::Poor => "xmark.octagon.fill",
        }
    }

    /// 상태색 의미 토큰.
    pub fn tone(self) -> StatusTone {
        match self {
            Self::Good => StatusTone::Success,
            Self::Fair => StatusTone::Warning,
            Self::Poor => StatusTone::Error,
        }
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/UIUXAudit0920GuardTests.swift` (C84 순수 부분)
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn bands_carry_shape_and_label() {
        for band in LatencyQualityBand::ALL {
            assert!(!band.symbol().is_empty());
            assert!(!band.label().is_empty());
        }
        let symbols: HashSet<_> = LatencyQualityBand::ALL.iter().map(|b| b.symbol()).collect();
        assert_eq!(symbols.len(), 3);
    }

    #[test]
    fn thresholds_and_labels() {
        assert_eq!(
            LatencyQualityBand::from_seconds(2.99),
            LatencyQualityBand::Good
        );
        assert_eq!(
            LatencyQualityBand::from_seconds(3.0),
            LatencyQualityBand::Fair
        );
        assert_eq!(
            LatencyQualityBand::from_seconds(4.99),
            LatencyQualityBand::Fair
        );
        assert_eq!(
            LatencyQualityBand::from_seconds(5.0),
            LatencyQualityBand::Poor
        );
        assert_eq!(LatencyQualityBand::Good.label(), "< 3초");
        assert_eq!(LatencyQualityBand::Fair.label(), "3-5초");
        assert_eq!(LatencyQualityBand::Poor.label(), "> 5초");
        assert_eq!(LatencyQualityBand::Poor.tone(), StatusTone::Error);
    }
}
