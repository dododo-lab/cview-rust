//! 2채널 분할 방향 판정의 단일 출처.
//!
//! 원본: `Sources/CViewCore/Logic/TwoUpSplitPolicy.swift`
//!
//! 컨테이너 비율 R 에서 2채널 셀의 종횡비는 가로 분할 `R/2`, 세로 분할 `2R`. 16:9 에서 벗어난
//! 정도 `32/(9R)` 대 `9R/8` 이 같아지는 곳은 `81R² = 256` → **R = 16/9** 한 점뿐이다.
//! ⚠ 임계값을 이 파일 바깥에 다시 적지 말 것.

/// 세로 스택 ↔ 가로 분할 교차 비율.
pub const CROSSOVER_ASPECT: f64 = 16.0 / 9.0;

/// 컨테이너가 교차점보다 좁으면(= 세로로 길면) 세로 스택이 letterbox 손해가 작다.
///
/// 높이가 0 이하인 첫 프레임에서는 `false`(가로 분할) — 측정 전에 세로로 쌓았다가
/// 곧바로 뒤집히면 레이아웃이 한 번 튄다.
pub fn prefers_vertical_stack(width: f64, height: f64) -> bool {
    if height <= 0.0 {
        return false;
    }
    width / height < CROSSOVER_ASPECT
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/Audit0919vmmultiliveContractTests.swift` (교차점 수식 검증)
    use super::*;

    fn horizontal_deviation(r: f64) -> f64 {
        32.0 / (9.0 * r)
    }
    fn vertical_deviation(r: f64) -> f64 {
        9.0 * r / 8.0
    }

    #[test]
    fn crossover_math_favors_horizontal_above_sixteen_by_nine() {
        assert!(
            (horizontal_deviation(CROSSOVER_ASPECT) - vertical_deviation(CROSSOVER_ASPECT)).abs()
                < 1e-9
        );
        for r in [1.9, 2.1, 2.35, 2.63, 2.79] {
            assert!(horizontal_deviation(r) < vertical_deviation(r));
            assert!(!prefers_vertical_stack(r * 100.0, 100.0));
        }
        for r in [1.2, 1.5, 1.7] {
            assert!(vertical_deviation(r) < horizontal_deviation(r));
            assert!(prefers_vertical_stack(r * 100.0, 100.0));
        }
    }

    #[test]
    fn zero_height_prefers_horizontal() {
        assert!(!prefers_vertical_stack(100.0, 0.0));
        assert!(!prefers_vertical_stack(0.0, -1.0));
    }
}
