//! PID 제어기 — 정밀 동기화 엔진.
//!
//! 원본: `Sources/CViewCore/Utilities/PIDController.swift`

use std::ops::RangeInclusive;

/// PID 제어기 — 비례(P), 적분(I), 미분(D) 제어. 플레이어-웹 동기화에 사용.
#[derive(Debug, Clone, PartialEq)]
pub struct PidController {
    /// 비례 게인
    pub kp: f64,
    /// 적분 게인
    pub ki: f64,
    /// 미분 게인
    pub kd: f64,
    integral: f64,
    previous_error: f64,
    last_output: f64,
    /// 적분항 윈드업 방지 범위.
    pub integral_clamp: RangeInclusive<f64>,
    /// 출력 제한 범위.
    pub output_clamp: Option<RangeInclusive<f64>>,
}

impl Default for PidController {
    fn default() -> Self {
        Self::new(0.8, 0.1, 0.05)
    }
}

impl PidController {
    /// 정밀 동기화 프리셋 (±0.05초 정밀도).
    pub fn ultra_precise() -> Self {
        Self::new(1.0, 0.15, 0.08)
    }

    /// 표준 동기화 프리셋 (±0.3초 정밀도).
    pub fn standard() -> Self {
        Self::new(0.8, 0.1, 0.05)
    }

    /// 완화된 동기화 프리셋 (±0.5초 정밀도).
    pub fn relaxed() -> Self {
        Self::new(0.5, 0.05, 0.02)
    }

    pub fn new(kp: f64, ki: f64, kd: f64) -> Self {
        Self {
            kp,
            ki,
            kd,
            integral: 0.0,
            previous_error: 0.0,
            last_output: 0.0,
            integral_clamp: -10.0..=10.0,
            output_clamp: None,
        }
    }

    pub fn integral(&self) -> f64 {
        self.integral
    }

    pub fn previous_error(&self) -> f64 {
        self.previous_error
    }

    pub fn last_output(&self) -> f64 {
        self.last_output
    }

    /// 에러 값과 시간 간격으로 제어 출력 계산. `delta_time <= 0` 이면 직전 출력을 그대로 반환.
    pub fn update(&mut self, error: f64, delta_time: f64) -> f64 {
        if delta_time <= 0.0 {
            return self.last_output;
        }
        let proportional = self.kp * error;

        self.integral = clamp(self.integral + error * delta_time, &self.integral_clamp);
        let integral_term = self.ki * self.integral;

        let derivative = (error - self.previous_error) / delta_time;
        let derivative_term = self.kd * derivative;

        self.previous_error = error;

        let mut output = proportional + integral_term + derivative_term;
        if let Some(range) = &self.output_clamp {
            output = clamp(output, range);
        }
        self.last_output = output;
        output
    }

    /// 상태 리셋.
    pub fn reset(&mut self) {
        self.integral = 0.0;
        self.previous_error = 0.0;
        self.last_output = 0.0;
    }
}

/// Swift `Comparable.clamped(to:)` 대응 — `min(max(v, lo), hi)`.
fn clamp(v: f64, range: &RangeInclusive<f64>) -> f64 {
    v.max(*range.start()).min(*range.end())
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewCoreTests/CViewCoreTests.swift` (PID Controller suite)
    use super::*;

    #[test]
    fn zero_error_zero_output() {
        assert_eq!(PidController::default().update(0.0, 0.1), 0.0);
    }

    #[test]
    fn positive_error_positive_output() {
        assert!(PidController::default().update(5.0, 0.1) > 0.0);
    }

    #[test]
    fn negative_error_negative_output() {
        assert!(PidController::default().update(-5.0, 0.1) < 0.0);
    }

    #[test]
    fn proportional_scales_with_kp() {
        let out = PidController::new(2.0, 0.0, 0.0).update(3.0, 0.1);
        assert!((out - 6.0).abs() < 0.001);
    }

    #[test]
    fn integral_accumulates() {
        let mut pid = PidController::new(0.0, 1.0, 0.0);
        pid.update(1.0, 1.0);
        assert!(pid.update(1.0, 1.0) > 1.0);
    }

    #[test]
    fn derivative_responds() {
        let mut pid = PidController::new(0.0, 0.0, 1.0);
        pid.update(0.0, 1.0);
        assert!(pid.update(5.0, 1.0) > 0.0);
    }

    #[test]
    fn reset_clears_state() {
        let mut pid = PidController::default();
        pid.update(5.0, 0.1);
        pid.update(5.0, 0.1);
        pid.reset();
        assert_eq!(pid.update(0.0, 0.1), 0.0);
    }

    #[test]
    fn integral_windup_and_output_clamp() {
        let mut pid = PidController::new(0.0, 1.0, 0.0);
        pid.update(100.0, 1.0);
        assert_eq!(pid.integral(), 10.0);
        pid.output_clamp = Some(-1.0..=1.0);
        assert_eq!(pid.update(100.0, 1.0), 1.0);
        assert_eq!(pid.update(1.0, 0.0), 1.0, "deltaTime 0 은 직전 출력");
    }
}
