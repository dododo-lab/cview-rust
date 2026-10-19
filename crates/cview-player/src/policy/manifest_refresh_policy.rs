//! `Sources/CViewPlayer/ManifestRefreshPolicy.swift` 포팅 — 매니페스트 주기 갱신 결정 로직.

/// 기본 재연결 임계.
pub const DEFAULT_ESCALATION_THRESHOLD: i64 = 5;

/// 연속 실패 카운터가 재연결 임계에 도달했는지.
pub fn should_escalate_to_reconnect(failure_count: i64, threshold: i64) -> bool {
    failure_count >= threshold
}

/// 기본 임계(5)로 판정 — 프로덕션(`decide_recovery`)이 쓰는 경로.
pub fn should_escalate(failure_count: i64) -> bool {
    should_escalate_to_reconnect(failure_count, DEFAULT_ESCALATION_THRESHOLD)
}

#[cfg(test)]
mod tests {
    //! `Tests/CViewPlayerTests/ManifestRefreshPolicyTests.swift` 포팅.
    use super::*;

    #[test]
    fn thresholds() {
        for c in 0..5 {
            assert!(!should_escalate_to_reconnect(c, 5));
        }
        assert!(should_escalate_to_reconnect(5, 5));
        assert!(should_escalate_to_reconnect(10, 5));
        assert!(!should_escalate(4));
        assert!(should_escalate(5));
        assert!(!should_escalate_to_reconnect(2, 3));
        assert!(should_escalate_to_reconnect(3, 3));
    }
}
