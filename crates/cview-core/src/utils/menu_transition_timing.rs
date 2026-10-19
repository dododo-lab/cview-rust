//! 메뉴 전환 임계 경로의 **지연 결정** — 뷰 배선에서 분리한 순수 함수.
//!
//! 원본: `Sources/CViewCore/Utilities/MenuTransitionTiming.swift`
//!
//! Swift `Duration` 은 음수를 표현하지만 [`std::time::Duration`] 은 음수가 없다. 원본이 음수
//! 마진을 0으로 접던 처리는 타입이 대신한다.

use std::time::Duration;

use super::time::{adding, interval_since, Date, TimeInterval};

/// 연속 화면 전환은 마지막 목적지의 초기 렌더 구간까지 보호한다.
/// 같은 시각의 중복 알림은 합치고, 짧은 후속 요청은 기존 보호 구간을 줄이지 않는다.
/// 무효 `duration`(0 이하·비유한)은 살아 있는 기존 기한을 그대로 돌려준다.
pub fn extended_gate_deadline(
    now: &Date,
    current_deadline: Option<&Date>,
    duration: TimeInterval,
) -> Option<Date> {
    let active = current_deadline.filter(|d| *d > now).copied();
    if !duration.is_finite() || duration <= 0.0 {
        return active;
    }
    let requested = adding(now, duration);
    Some(active.map_or(requested, |a| a.max(requested)))
}

/// 메뉴 복귀 부수효과(GPU 티어 재승격 · 재생 재개 · 채팅 재연결)를 미룰 시간.
///
/// - 게이트가 없거나 이미 만료됐으면 0 — 기다릴 이유가 없다.
/// - 게이트가 남아 있으면 `잔여 + margin`.
pub fn restore_delay(now: &Date, gate_deadline: Option<&Date>, margin: Duration) -> Duration {
    let Some(deadline) = gate_deadline else {
        return Duration::ZERO;
    };
    let remaining = interval_since(deadline, now);
    if remaining <= 0.0 {
        return Duration::ZERO;
    }
    Duration::from_secs_f64(remaining) + margin
}

/// 라우트 헤더 우측(trailing) 보조 컨텐츠를 반영하기까지 기다릴 시간.
///
/// - `applied_route == None`(최초 마운트) → `debounce`.
/// - 발행 라우트가 마지막 반영 라우트와 다르면 → 0(라우트 전환이 유발한 첫 발행).
/// - 같으면 → 레이아웃 전환 중일 때만 `debounce`(원본 기본값 `is_layout_transitioning = true`).
///
/// ⚠ 판정 입력은 "라우트가 바뀐 시각" 이 아니라 "어느 라우트의 값을 마지막으로 반영했는가" 다.
pub fn header_trailing_delay<Route: PartialEq>(
    published_route: &Route,
    applied_route: Option<&Route>,
    is_layout_transitioning: bool,
    debounce: Duration,
) -> Duration {
    let Some(applied) = applied_route else {
        return debounce;
    };
    if published_route != applied {
        return Duration::ZERO;
    }
    if is_layout_transitioning {
        debounce
    } else {
        Duration::ZERO
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewCoreTests/MenuTransitionTimingTests.swift`
    use super::*;
    use crate::utils::time::from_unix;

    fn ms(v: u64) -> Duration {
        Duration::from_millis(v)
    }

    #[test]
    fn rapid_changes_extend_to_latest_destination() {
        let start = from_unix(1_000.0);
        let first = extended_gate_deadline(&start, None, 0.12);
        let second = extended_gate_deadline(&adding(&start, 0.04), first.as_ref(), 0.12);
        assert_eq!(first, Some(adding(&start, 0.12)));
        assert_eq!(second, Some(adding(&start, 0.16)));
    }

    #[test]
    fn duplicate_and_shorter_requests_coalesce() {
        let start = from_unix(1_000.0);
        let deadline = adding(&start, 0.5);
        assert_eq!(
            extended_gate_deadline(&start, Some(&deadline), 0.5),
            Some(deadline)
        );
        assert_eq!(
            extended_gate_deadline(&adding(&start, 0.04), Some(&deadline), 0.12),
            Some(deadline)
        );
    }

    #[test]
    fn expired_deadline_does_not_accumulate() {
        let now = from_unix(1_000.0);
        assert_eq!(
            extended_gate_deadline(&now, Some(&adding(&now, -1.0)), 0.12),
            Some(adding(&now, 0.12))
        );
    }

    #[test]
    fn invalid_durations_are_harmless() {
        let now = from_unix(1_000.0);
        let deadline = adding(&now, 0.12);
        for d in [0.0, -1.0, f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            assert_eq!(extended_gate_deadline(&now, None, d), None);
            assert_eq!(
                extended_gate_deadline(&now, Some(&deadline), d),
                Some(deadline)
            );
        }
    }

    #[test]
    fn restore_delay_is_zero_without_gate() {
        assert_eq!(restore_delay(&from_unix(0.0), None, ms(40)), Duration::ZERO);
    }

    #[test]
    fn restore_delay_is_zero_for_expired_gate() {
        let now = from_unix(1_000.0);
        assert_eq!(
            restore_delay(&now, Some(&adding(&now, -0.5)), ms(40)),
            Duration::ZERO
        );
    }

    #[test]
    fn restore_delay_covers_remaining_gate_plus_margin() {
        let now = from_unix(1_000.0);
        let d = restore_delay(&now, Some(&adding(&now, 0.1)), ms(40));
        assert!(d > ms(139) && d < ms(141), "{d:?}");
    }

    #[test]
    fn restore_delay_zero_margin_is_remaining_only() {
        let now = from_unix(1_000.0);
        let deadline = adding(&now, 0.1);
        let folded = restore_delay(&now, Some(&deadline), Duration::ZERO);
        assert!(folded > ms(99) && folded < ms(101));
        assert!(restore_delay(&now, Some(&deadline), ms(40)) > folded);
    }

    #[test]
    fn header_trailing_skips_debounce_right_after_route_change() {
        assert_eq!(
            header_trailing_delay(&"vod", Some(&"home"), true, ms(80)),
            Duration::ZERO
        );
    }

    #[test]
    fn header_trailing_keeps_debounce_on_initial_mount() {
        assert_eq!(header_trailing_delay(&"home", None, true, ms(80)), ms(80));
    }

    #[test]
    fn header_trailing_keeps_debounce_for_same_route() {
        assert_eq!(
            header_trailing_delay(&"category", Some(&"category"), true, ms(80)),
            ms(80)
        );
        assert_eq!(
            header_trailing_delay(&"category", Some(&"category"), false, ms(80)),
            Duration::ZERO
        );
    }

    #[test]
    fn header_trailing_decision_is_order_independent() {
        assert_eq!(
            header_trailing_delay(&"clips", Some(&"home"), true, ms(80)),
            Duration::ZERO
        );
        assert_eq!(
            header_trailing_delay(&"clips", Some(&"clips"), true, ms(80)),
            ms(80)
        );
    }
}
