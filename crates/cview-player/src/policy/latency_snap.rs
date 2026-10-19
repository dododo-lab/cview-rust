//! `Sources/CViewPlayer/LatencySnapPolicy.swift` 포팅 — PID 시작 직후 목표 지연 스냅 판정.

/// JS `LATENCY_CORRECTION_MIN_DRIFT_S` 와 같은 값 — 두 문턱이 갈라지면 선택 시 건너뛰기가 되살아난다.
pub const SNAP_DRIFT_SECONDS: f64 = 1.5;

/// PID 시작 직후 목표 지연으로 한 번에 스냅할지.
/// 측정 전(세그먼트 길이 0)·JS 자기보정 허용 폭 안(≤ 목표 + 1.5s)은 거른다.
pub fn should_snap_on_sync_start(
    latency: f64,
    measured_segment_duration: f64,
    target: f64,
) -> bool {
    if measured_segment_duration <= 0.0 || latency <= 0.0 || target <= 0.0 {
        return false;
    }
    latency > target + SNAP_DRIFT_SECONDS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_gate() {
        assert!(!should_snap_on_sync_start(10.0, 0.0, 3.0));
        assert!(!should_snap_on_sync_start(4.5, 2.0, 3.0));
        assert!(should_snap_on_sync_start(4.51, 2.0, 3.0));
        assert!(!should_snap_on_sync_start(0.0, 2.0, 3.0));
        assert!(!should_snap_on_sync_start(5.0, 2.0, 0.0));
    }
}
