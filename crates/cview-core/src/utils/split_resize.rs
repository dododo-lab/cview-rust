//! 분할 핸들의 실제 레이아웃 폭과 두 패인의 크기 계산.
//!
//! 원본: `Sources/CViewCore/Utilities/SplitResizeGeometry.swift`
//!
//! 비율은 핸들/간격을 제외한 가용 길이에 적용한다. `CGRect` 대신 최소한의 [`Rect`] 를 둔다.

/// `CGRect` 대응(원점 + 크기, 음수 크기는 다루지 않는다).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
    pub fn min_x(&self) -> f64 {
        self.x
    }
    pub fn min_y(&self) -> f64 {
        self.y
    }
    pub fn max_x(&self) -> f64 {
        self.x + self.width
    }
    pub fn max_y(&self) -> f64 {
        self.y + self.height
    }

    /// `other` 가 이 사각형 안에 완전히 들어가는가(경계 포함).
    pub fn contains(&self, other: &Rect) -> bool {
        other.min_x() >= self.min_x()
            && other.min_y() >= self.min_y()
            && other.max_x() <= self.max_x()
            && other.max_y() <= self.max_y()
    }

    /// 두 사각형이 양의 넓이로 겹치는가(변이 맞닿는 것은 겹침이 아니다).
    pub fn intersects(&self, other: &Rect) -> bool {
        self.min_x().max(other.min_x()) < self.max_x().min(other.max_x())
            && self.min_y().max(other.min_y()) < self.max_y().min(other.max_y())
    }
}

fn safe(v: f64) -> f64 {
    if v.is_finite() {
        v.max(0.0)
    } else {
        0.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplitResizeGeometry {
    pub content_length: f64,
    pub main_length: f64,
    pub other_length: f64,
    pub handle_length: f64,
    pub gap: f64,
}

impl SplitResizeGeometry {
    pub const HANDLE_EXTENT: f64 = 15.0;

    /// 기본 간격 0, 기본 핸들 15pt.
    pub fn new(length: f64, ratio: f64) -> Self {
        Self::with(length, ratio, 0.0, Self::HANDLE_EXTENT)
    }

    pub fn with(length: f64, ratio: f64, spacing: f64, handle_extent: f64) -> Self {
        let length = safe(length);
        let handle_length = safe(handle_extent).min(length);
        let gap = safe(spacing).min((length - handle_length) / 2.0);
        let content_length = (length - handle_length - 2.0 * gap).max(0.0);
        let ratio = if ratio.is_finite() {
            ratio.clamp(0.0, 1.0)
        } else {
            0.5
        };
        let main_length = content_length * ratio;
        Self {
            content_length,
            main_length,
            other_length: content_length - main_length,
            handle_length,
            gap,
        }
    }

    /// 반환 순서는 메인 패인, 핸들, 보조 패인. 방향 변경에도 순서는 유지한다.
    pub fn frames(
        bounds: Rect,
        is_horizontal: bool,
        ratio: f64,
        main_at_start: bool,
        spacing: f64,
        handle_extent: f64,
    ) -> [Rect; 3] {
        let g = Self::with(
            if is_horizontal {
                bounds.width
            } else {
                bounds.height
            },
            ratio,
            spacing,
            handle_extent,
        );
        let leading_length = if main_at_start {
            g.main_length
        } else {
            g.other_length
        };
        let divider_origin = leading_length + g.gap;
        let trailing_origin = divider_origin + g.handle_length + g.gap;
        let cross = safe(if is_horizontal {
            bounds.height
        } else {
            bounds.width
        });
        let rect = |origin: f64, length: f64| {
            if is_horizontal {
                Rect::new(bounds.x + origin, bounds.y, length, cross)
            } else {
                Rect::new(bounds.x, bounds.y + origin, cross, length)
            }
        };
        [
            rect(
                if main_at_start { 0.0 } else { trailing_origin },
                g.main_length,
            ),
            rect(divider_origin, g.handle_length),
            rect(
                if main_at_start { trailing_origin } else { 0.0 },
                g.other_length,
            ),
        ]
    }
}

/// 드래그 시작 값/가용 길이는 고정하고, 움직이지 않는 좌표계의 누적 이동량만 적용한다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResizeDragAnchor {
    value: f64,
    extent: f64,
    sign: f64,
}

impl ResizeDragAnchor {
    /// 값·길이가 유한하지 않거나 길이가 0 이하면 `None`.
    pub fn new(value: f64, extent: f64, inverted: bool) -> Option<Self> {
        (value.is_finite() && extent.is_finite() && extent > 0.0).then_some(Self {
            value,
            extent,
            sign: if inverted { -1.0 } else { 1.0 },
        })
    }

    pub fn value(&self, translation: f64) -> Option<f64> {
        if !translation.is_finite() {
            return None;
        }
        let result = self.value + self.sign * translation / self.extent;
        result.is_finite().then_some(result)
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewCoreTests/SplitResizeGeometryTests.swift`
    use super::*;

    #[test]
    fn lengths_exactly_fit() {
        for length in [0.0, 5.0, 15.0, 16.0, 300.0, 1000.0, 3840.0] {
            for ratio in [0.0, 0.2, 0.5, 0.85, 1.0] {
                for spacing in [0.0, 6.0, 1000.0] {
                    let s = SplitResizeGeometry::with(
                        length,
                        ratio,
                        spacing,
                        SplitResizeGeometry::HANDLE_EXTENT,
                    );
                    assert!(
                        (s.main_length + s.other_length + s.handle_length + 2.0 * s.gap - length)
                            .abs()
                            < 1e-6
                    );
                    assert!(
                        s.main_length >= 0.0 && s.other_length >= 0.0 && s.content_length >= 0.0
                    );
                }
            }
        }
    }

    #[test]
    fn two_chat_columns_include_handle() {
        let s = SplitResizeGeometry::new(1000.0, 0.5);
        assert_eq!(s.main_length, 492.5);
        assert_eq!(s.other_length, 492.5);
        assert_eq!(s.handle_length, 15.0);
    }

    #[test]
    fn focus_video_edge_has_no_reserved_handle() {
        let s = SplitResizeGeometry::with(1000.0, 0.7, 3.0, 0.0);
        assert_eq!(s.handle_length, 0.0);
        assert_eq!(s.gap, 3.0);
        assert_eq!(s.main_length + s.other_length + 6.0, 1000.0);
        assert!((s.main_length / s.content_length - 0.7).abs() < 1e-6);
        let f = SplitResizeGeometry::frames(
            Rect::new(0.0, 0.0, 1000.0, 500.0),
            true,
            0.7,
            true,
            3.0,
            0.0,
        );
        assert_eq!(f[1].width, 0.0);
        assert!((f[2].min_x() - f[0].max_x() - 6.0).abs() < 1e-6);
    }

    #[test]
    fn frames_fit_and_do_not_overlap() {
        let bounds = Rect::new(23.0, 47.0, 1107.0, 763.0);
        for horizontal in [false, true] {
            for at_start in [false, true] {
                for ratio in [0.2, 0.45, 0.5, 0.85] {
                    let f = SplitResizeGeometry::frames(
                        bounds,
                        horizontal,
                        ratio,
                        at_start,
                        6.0,
                        SplitResizeGeometry::HANDLE_EXTENT,
                    );
                    for frame in &f {
                        assert!(bounds.contains(frame), "{frame:?}");
                    }
                    assert!(!f[0].intersects(&f[1]));
                    assert!(!f[1].intersects(&f[2]));
                    assert!(!f[0].intersects(&f[2]));
                    let o = if at_start { f } else { [f[2], f[1], f[0]] };
                    if horizontal {
                        assert_eq!(o[0].min_x(), bounds.min_x());
                        assert!((o[2].max_x() - bounds.max_x()).abs() < 1e-6);
                        assert!((o[1].min_x() - o[0].max_x() - 6.0).abs() < 1e-6);
                    } else {
                        assert_eq!(o[0].min_y(), bounds.min_y());
                        assert!((o[2].max_y() - bounds.max_y()).abs() < 1e-6);
                        assert!((o[1].min_y() - o[0].max_y() - 6.0).abs() < 1e-6);
                    }
                }
            }
        }
    }

    #[test]
    fn pointer_and_divider_travel_match() {
        for horizontal in [false, true] {
            for inverted in [false, true] {
                let length = 1200.0;
                let split =
                    SplitResizeGeometry::with(length, 0.5, 6.0, SplitResizeGeometry::HANDLE_EXTENT);
                let anchor = ResizeDragAnchor::new(0.5, split.content_length, inverted).unwrap();
                let bounds = Rect::new(0.0, 0.0, length, length);
                let frames = |ratio: f64| {
                    SplitResizeGeometry::frames(
                        bounds,
                        horizontal,
                        ratio,
                        !inverted,
                        6.0,
                        SplitResizeGeometry::HANDLE_EXTENT,
                    )
                };
                let before = frames(0.5)[1];
                for movement in [-180.0, -1.0, 0.0, 1.0, 230.0] {
                    let after = frames(anchor.value(movement).unwrap())[1];
                    let delta = if horizontal {
                        after.min_x() - before.min_x()
                    } else {
                        after.min_y() - before.min_y()
                    };
                    assert!((delta - movement).abs() < 1e-6);
                }
            }
        }
    }

    #[test]
    fn chat_width_uses_stable_anchor() {
        let a = ResizeDragAnchor::new(320.0, 1.0, true).unwrap();
        assert_eq!(a.value(-10.0), Some(330.0));
        assert_eq!(a.value(-50.0), Some(370.0));
        assert_eq!(a.value(-83.0), Some(403.0));
        assert_eq!(a.value(0.0), Some(320.0));
        assert_eq!(a.value(15.0), Some(305.0));
    }

    #[test]
    fn invalid_inputs_are_safe() {
        assert!(ResizeDragAnchor::new(f64::NAN, 1.0, false).is_none());
        assert!(ResizeDragAnchor::new(0.5, 0.0, false).is_none());
        assert!(ResizeDragAnchor::new(0.5, f64::INFINITY, false).is_none());
        assert_eq!(
            ResizeDragAnchor::new(0.5, 1.0, false)
                .unwrap()
                .value(f64::NAN),
            None
        );
        let s = SplitResizeGeometry::with(
            f64::NAN,
            f64::INFINITY,
            f64::NAN,
            SplitResizeGeometry::HANDLE_EXTENT,
        );
        assert_eq!(s.content_length, 0.0);
        assert_eq!(s.main_length, 0.0);
    }
}
