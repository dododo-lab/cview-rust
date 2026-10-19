//! 멀티채팅 도크 폭의 입력/표시/레이아웃 단일 계약.
//!
//! 원본: `Sources/CViewCore/Utilities/MultiChatPanelWidthPolicy.swift`
//!
//! 저장값은 창 너비 대비 비율이다. 설정 컨트롤은 0.1% 단위로 스냅하고, 라이브 화면의 직접
//! 드래그는 현재 창에서 1pt 단위로 스냅한 뒤 비율로 환산한다. 실제 폭에는 채팅 가독성 하한과
//! 영상 스테이지 하한을 함께 적용한다. (`CGFloat` → `f64`)

use std::ops::RangeInclusive;

pub const MINIMUM_RATIO: f64 = 0.05;
pub const MAXIMUM_RATIO: f64 = 0.95;
pub const DEFAULT_RATIO: f64 = 0.20;
pub const SETTINGS_STEP: f64 = 0.001;
pub const DIRECT_MANIPULATION_POINT_STEP: f64 = 1.0;
pub const RATIO_RANGE: RangeInclusive<f64> = MINIMUM_RATIO..=MAXIMUM_RATIO;

/// 유한하지 않거나 음수인 길이를 0으로 접는다.
fn safe(v: f64) -> f64 {
    if v.is_finite() {
        v.max(0.0)
    } else {
        0.0
    }
}

/// 저장/레이아웃에 사용할 수 있는 유한한 비율로 제한한다.
pub fn clamped_ratio(ratio: f64) -> f64 {
    if !ratio.is_finite() {
        return DEFAULT_RATIO;
    }
    MAXIMUM_RATIO.min(MINIMUM_RATIO.max(ratio))
}

/// 설정 슬라이더·숫자 입력용 0.1% 스냅.
pub fn settings_ratio(ratio: f64) -> f64 {
    let clamped = clamped_ratio(ratio);
    clamped_ratio((clamped / SETTINGS_STEP).round() * SETTINGS_STEP)
}

pub fn ratio_from_percent(percent: f64) -> f64 {
    if !percent.is_finite() {
        return DEFAULT_RATIO;
    }
    settings_ratio(percent / 100.0)
}

pub fn percent_from_ratio(ratio: f64) -> f64 {
    settings_ratio(ratio) * 100.0
}

/// 레이아웃 경계 입력(채팅 하한·스테이지 하한·분할선 폭).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PanelBounds {
    pub minimum_chat_width: f64,
    pub reserved_stage_width: f64,
    pub divider_width: f64,
}

impl PanelBounds {
    pub fn new(minimum_chat_width: f64, reserved_stage_width: f64, divider_width: f64) -> Self {
        Self {
            minimum_chat_width,
            reserved_stage_width,
            divider_width,
        }
    }

    /// 스테이지 하한 없이 채팅 하한만.
    pub fn chat_only(minimum_chat_width: f64) -> Self {
        Self::new(minimum_chat_width, 0.0, 0.0)
    }

    /// (채팅 하한, 채팅 최대 폭)
    fn limits(&self, total_width: f64) -> (f64, f64) {
        let min = safe(self.minimum_chat_width);
        let stage_preserving_maximum =
            (total_width - safe(self.reserved_stage_width) - safe(self.divider_width)).max(0.0);
        let max = min.max((total_width * MAXIMUM_RATIO).min(stage_preserving_maximum));
        (min, max)
    }
}

/// 채팅의 절대 가독성 하한과 스테이지 하한을 동시에 보장할 수 있는지.
/// 둘을 함께 담을 수 없는 폭에서는 호출부가 얇은 접힘 핸들로 전환해야 한다.
pub fn can_present_expanded(total_width: f64, bounds: PanelBounds) -> bool {
    if !total_width.is_finite() || total_width <= 0.0 {
        return false;
    }
    total_width
        >= safe(bounds.minimum_chat_width)
            + safe(bounds.reserved_stage_width)
            + safe(bounds.divider_width)
}

/// 라이브 화면의 포인터 위치를 현재 창에서 1pt 단위로 정규화해 저장 비율로 바꾼다.
/// 양 끝은 최소/최대 선호로 저장해 창을 다시 넓혀도 사용자가 고른 경계를 유지한다.
pub fn ratio_for_chat_width(
    requested_chat_width: f64,
    total_width: f64,
    bounds: PanelBounds,
) -> f64 {
    if !total_width.is_finite() || total_width <= 0.0 || !requested_chat_width.is_finite() {
        return DEFAULT_RATIO;
    }
    let (min, max) = bounds.limits(total_width);
    let bounded = requested_chat_width.max(min).min(max);
    let snapped =
        (bounded / DIRECT_MANIPULATION_POINT_STEP).round() * DIRECT_MANIPULATION_POINT_STEP;
    if snapped <= min {
        return MINIMUM_RATIO;
    }
    if snapped >= max {
        return MAXIMUM_RATIO;
    }
    clamped_ratio(snapped / total_width)
}

/// 저장 비율과 절대 하한을 합성해 실제 도크 폭을 계산한다. 모든 비율에서 같은 선형식이라 연속적이다.
pub fn resolved_width(total_width: f64, ratio: f64, bounds: PanelBounds) -> f64 {
    if !total_width.is_finite() || total_width <= 0.0 {
        return 0.0;
    }
    let (min, max) = bounds.limits(total_width);
    (total_width * clamped_ratio(ratio)).max(min).min(max)
}

/// 사용자가 눌러서 고르는 "읽기 편한 폭" 후보(350~420pt 대역).
///
/// ⚠ 저장 계약은 여전히 **비율**이다. 목표 pt 를 현재 창에서 비율로 바꾸는 입력일 뿐이다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReadingWidthPreset {
    Comfortable,
    Roomy,
    Wide,
}

impl ReadingWidthPreset {
    pub const ALL: [Self; 3] = [Self::Comfortable, Self::Roomy, Self::Wide];

    /// 목표 채팅 폭(pt).
    pub fn target_width(self) -> f64 {
        match self {
            Self::Comfortable => 350.0,
            Self::Roomy => 385.0,
            Self::Wide => 420.0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Comfortable => "읽기 편한 폭",
            Self::Roomy => "넉넉한 폭",
            Self::Wide => "가장 넓게",
        }
    }

    /// 원본 `rawValue`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Comfortable => "comfortable",
            Self::Roomy => "roomy",
            Self::Wide => "wide",
        }
    }
}

/// 프리셋을 현재 창에서 저장 가능한 비율로 바꾼다.
pub fn ratio_for_reading_width(
    preset: ReadingWidthPreset,
    total_width: f64,
    bounds: PanelBounds,
) -> f64 {
    ratio_for_chat_width(preset.target_width(), total_width, bounds)
}

/// 이 프리셋이 현재 창에서 실제로 그 폭이 되는지(1pt 스냅·왕복 오차 2pt 허용).
pub fn can_represent_reading_width(
    preset: ReadingWidthPreset,
    total_width: f64,
    bounds: PanelBounds,
) -> bool {
    if !total_width.is_finite() || total_width <= 0.0 {
        return false;
    }
    let resolved = resolved_width(
        total_width,
        ratio_for_reading_width(preset, total_width, bounds),
        bounds,
    );
    (resolved - preset.target_width()).abs() <= 2.0
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewCoreTests/MultiChatPanelWidthPolicyTests.swift`
    //!
    //! `MultiChatSettings` 모델(세션 정리·프리셋 복원 이력·디코드 교정) 테스트는 모델 포팅 쪽 몫이라 옮기지 않았다.
    use super::*;

    fn full(min: f64) -> PanelBounds {
        PanelBounds::new(min, 400.0, 12.0)
    }

    #[test]
    fn clamps_persisted_ratio() {
        assert_eq!(clamped_ratio(0.01), 0.05);
        assert_eq!(clamped_ratio(0.10), 0.10);
        assert_eq!(clamped_ratio(0.75), 0.75);
        assert_eq!(clamped_ratio(1.0), 0.95);
        assert_eq!(clamped_ratio(f64::NAN), 0.20);
        assert!(RATIO_RANGE.contains(&clamped_ratio(3.0)));
    }

    #[test]
    fn settings_use_tenth_percent_steps() {
        assert!((settings_ratio(0.2036) - 0.204).abs() < 1e-6);
        assert!((ratio_from_percent(20.36) - 0.204).abs() < 1e-6);
        assert!((percent_from_ratio(0.204) - 20.4).abs() < 1e-6);
    }

    #[test]
    fn detects_insufficient_workspace_width() {
        assert!(!can_present_expanded(
            700.0,
            PanelBounds::new(320.0, 400.0, 12.0)
        ));
        assert!(can_present_expanded(
            732.0,
            PanelBounds::new(320.0, 400.0, 12.0)
        ));
        assert!(!can_present_expanded(
            f64::NAN,
            PanelBounds::new(320.0, 400.0, 12.0)
        ));
    }

    #[test]
    fn direct_manipulation_uses_point_precision() {
        let r = ratio_for_chat_width(317.4, 1440.0, PanelBounds::chat_only(280.0));
        assert!((r - 317.0 / 1440.0).abs() < 1e-6);
        assert_eq!(
            ratio_for_chat_width(318.0, 1000.0, PanelBounds::chat_only(320.0)),
            MINIMUM_RATIO
        );
        assert_eq!(
            ratio_for_chat_width(510.0, 1000.0, PanelBounds::chat_only(280.0)),
            0.51
        );
        assert_eq!(
            ratio_for_chat_width(900.0, 1000.0, full(280.0)),
            MAXIMUM_RATIO
        );
        assert!((ratio_for_chat_width(2200.0, 2800.0, full(280.0)) - 2200.0 / 2800.0).abs() < 1e-6);
    }

    #[test]
    fn resolved_width_respects_layout_bounds() {
        assert_eq!(
            resolved_width(1000.0, 0.05, PanelBounds::chat_only(320.0)),
            320.0
        );
        assert_eq!(
            resolved_width(1000.0, 0.80, PanelBounds::chat_only(280.0)),
            800.0
        );
        assert_eq!(
            resolved_width(0.0, 0.20, PanelBounds::chat_only(280.0)),
            0.0
        );
        assert_eq!(resolved_width(796.0, 0.95, full(280.0)), 384.0);
    }

    #[test]
    fn old_minimum_boundary_is_continuous() {
        let total = 2800.0;
        for width in [419.0, 420.0, 421.0] {
            let r = ratio_for_chat_width(width, total, full(280.0));
            assert!((resolved_width(total, r, full(280.0)) - width).abs() < 1e-6);
        }
    }

    #[test]
    fn narrow_workspace_drag_respects_both_pane_minimums() {
        let total = 796.0;
        for (requested, expected) in [
            (100.0, 280.0),
            (319.4, 319.0),
            (383.6, 384.0),
            (500.0, 384.0),
        ] {
            let r = ratio_for_chat_width(requested, total, full(280.0));
            let chat = resolved_width(total, r, full(280.0));
            assert!((chat - expected).abs() < 1e-6, "{requested}: {chat}");
            assert!(total - chat - 12.0 >= 400.0);
        }
    }

    #[test]
    fn edge_preferences_survive_window_resize() {
        let minimum = ratio_for_chat_width(100.0, 1000.0, full(320.0));
        assert_eq!(minimum, MINIMUM_RATIO);
        assert_eq!(resolved_width(1000.0, minimum, full(320.0)), 320.0);
        assert_eq!(resolved_width(1600.0, minimum, full(320.0)), 320.0);
        let maximum = ratio_for_chat_width(900.0, 1000.0, full(280.0));
        assert_eq!(maximum, MAXIMUM_RATIO);
        assert_eq!(resolved_width(1000.0, maximum, full(280.0)), 588.0);
        assert_eq!(resolved_width(1600.0, maximum, full(280.0)), 1188.0);
    }

    #[test]
    fn reversing_at_edges_does_not_require_hidden_travel() {
        let total = 1000.0;
        let min_plus_one = ratio_for_chat_width(321.0, total, full(320.0));
        let max_minus_one = ratio_for_chat_width(587.0, total, full(280.0));
        assert_eq!(resolved_width(total, min_plus_one, full(320.0)), 321.0);
        assert_eq!(resolved_width(total, max_minus_one, full(280.0)), 587.0);
    }

    #[test]
    fn direct_drag_is_monotonic_across_feasible_widths() {
        for (total, minimum) in [(796.0, 280.0), (1440.0, 320.0), (2800.0, 320.0)] {
            let maximum: f64 = (total * MAXIMUM_RATIO).min(total - 400.0 - 12.0);
            let mut previous = minimum;
            for point in (minimum as i64)..=(maximum.floor() as i64) {
                let requested = point as f64;
                let r = ratio_for_chat_width(requested, total, full(minimum));
                let actual = resolved_width(total, r, full(minimum));
                assert!((actual - requested).abs() <= 1.0);
                assert!(actual >= previous - 1e-6);
                assert!(actual - previous <= 1.000_001);
                previous = actual;
            }
        }
    }

    #[test]
    fn reading_width_preset_converts_to_ratio() {
        let total = 1600.0;
        for preset in ReadingWidthPreset::ALL {
            let r = ratio_for_reading_width(preset, total, PanelBounds::chat_only(280.0));
            let resolved = resolved_width(total, r, PanelBounds::chat_only(280.0));
            assert!((resolved - preset.target_width()).abs() <= 2.0);
            assert!(can_represent_reading_width(
                preset,
                total,
                PanelBounds::chat_only(280.0)
            ));
        }
    }

    #[test]
    fn reading_width_presets_respect_wide_and_narrow_geometry() {
        let total = 2800.0;
        for preset in ReadingWidthPreset::ALL {
            let r = ratio_for_reading_width(preset, total, full(280.0));
            let width = resolved_width(total, r, full(280.0));
            assert!(can_represent_reading_width(preset, total, full(280.0)));
            assert!((width - preset.target_width()).abs() <= 2.0);
        }
        assert!(!can_represent_reading_width(
            ReadingWidthPreset::Wide,
            732.0,
            full(320.0)
        ));
    }

    #[test]
    fn reading_width_presets_stay_in_planned_band() {
        for preset in ReadingWidthPreset::ALL {
            assert!((350.0..=420.0).contains(&preset.target_width()));
            assert!(!preset.label().is_empty());
        }
    }
}
