//! 지수 가중 이동 평균 계산기 — HLS.js 알고리즘 포팅.
//!
//! 원본: `Sources/CViewCore/Utilities/EWMACalculator.swift`

/// EWMA (Exponential Weighted Moving Average) 계산기.
///
/// [P2-4 실측 2026-07-09] 추정값을 **첫 샘플로 시드**하므로 hls.js 의 zero-init 바이어스
/// 보정(`estimate/(1−decay^n)`)을 결합하지 않는다. 결합하면 리셋 직후 2번째 샘플이
/// α=0.15 기준 6.67배 부풀려져 seek 진동을 일으킨다.
#[derive(Debug, Clone, PartialEq)]
pub struct EwmaCalculator {
    alpha: f64,
    estimate: Option<f64>,
    total_weight: f64,
}

impl EwmaCalculator {
    /// `alpha`: 평활화 계수 (0~1, 높을수록 최근 값에 민감).
    ///
    /// # Panics
    /// `alpha` 가 `(0, 1]` 밖이면 패닉한다(원본의 `precondition`).
    pub fn new(alpha: f64) -> Self {
        assert!(alpha > 0.0 && alpha <= 1.0, "Alpha must be in (0, 1]");
        Self {
            alpha,
            estimate: None,
            total_weight: 0.0,
        }
    }

    /// 새 샘플로 EWMA 업데이트 후 현재 추정값 반환.
    pub fn update(&mut self, value: f64) -> f64 {
        match self.estimate {
            Some(current) => {
                let next = value * self.alpha + current * (1.0 - self.alpha);
                self.total_weight += 1.0;
                self.estimate = Some(next);
                next
            }
            None => {
                self.estimate = Some(value);
                self.total_weight = 1.0;
                value
            }
        }
    }

    /// 현재 추정값(샘플이 없으면 0).
    pub fn current(&self) -> f64 {
        self.estimate.unwrap_or(0.0)
    }

    /// 리셋.
    pub fn reset(&mut self) {
        self.estimate = None;
        self.total_weight = 0.0;
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewCoreTests/CViewCoreTests.swift` (EWMA Calculator suite)
    use super::*;

    #[test]
    fn initial_state_is_zero() {
        assert_eq!(EwmaCalculator::new(0.5).current(), 0.0);
    }

    #[test]
    fn single_sample_returns_value() {
        let mut e = EwmaCalculator::new(0.5);
        assert_eq!(e.update(100.0), 100.0);
    }

    #[test]
    fn converges() {
        let mut e = EwmaCalculator::new(0.5);
        let mut v = 0.0;
        for _ in 0..100 {
            v = e.update(50.0);
        }
        assert!((v - 50.0).abs() < 1.0);
    }

    #[test]
    fn higher_alpha_responds_faster() {
        let mut fast = EwmaCalculator::new(0.9);
        let mut slow = EwmaCalculator::new(0.1);
        for _ in 0..10 {
            fast.update(100.0);
            slow.update(100.0);
        }
        assert!(fast.update(200.0) > slow.update(200.0));
    }

    #[test]
    fn reset_clears_state() {
        let mut e = EwmaCalculator::new(0.5);
        e.update(100.0);
        e.reset();
        assert_eq!(e.current(), 0.0);
    }

    #[test]
    fn second_sample_no_inflation() {
        let mut e = EwmaCalculator::new(0.15);
        assert_eq!(e.update(4.4), 4.4);
        let second = e.update(4.4);
        assert!((second - 4.4).abs() < 0.0001, "2번째 샘플 부풀림: {second}");
    }

    #[test]
    fn output_is_convex_blend() {
        let mut e = EwmaCalculator::new(0.15);
        let mut prev = e.update(10.0);
        for raw in [3.0, 12.0, 4.4, 4.4, 30.0, 2.0] {
            let out = e.update(raw);
            assert!(out >= prev.min(raw) - 0.0001 && out <= prev.max(raw) + 0.0001);
            prev = out;
        }
    }

    #[test]
    #[should_panic]
    fn rejects_zero_alpha() {
        let _ = EwmaCalculator::new(0.0);
    }
}
