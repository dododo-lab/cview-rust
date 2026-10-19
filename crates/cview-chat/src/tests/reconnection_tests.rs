//! `ReconnectionPolicyTests.swift` + `ReconnectionPolicyResetThresholdTests.swift` 포팅.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::constants::reconnect_defaults;
use crate::error::ChatError;
use crate::reconnection::{ReconnectionConfiguration, ReconnectionPolicy, ReconnectionState};

/// Swift `Configuration(initialDelay:maxDelay:maxAttempts:backoffMultiplier:jitterFactor:)` —
/// 나머지는 기본값.
fn cfg(initial: f64, max_delay: f64, max_attempts: i64, multiplier: f64, jitter: f64) -> ReconnectionConfiguration {
    ReconnectionConfiguration {
        initial_delay: initial,
        max_delay,
        max_attempts,
        backoff_multiplier: multiplier,
        jitter_factor: jitter,
        ..ReconnectionConfiguration::default()
    }
}

fn cfg_reset(
    initial: f64,
    max_delay: f64,
    max_attempts: i64,
    multiplier: f64,
    jitter: f64,
    reset: f64,
) -> ReconnectionConfiguration {
    ReconnectionConfiguration { reset_threshold: reset, ..cfg(initial, max_delay, max_attempts, multiplier, jitter) }
}

fn approx(a: f64, b: f64, eps: f64) -> bool {
    (a - b).abs() < eps
}

// MARK: - Backoff

#[test]
fn first_delay_equals_initial_delay() {
    let p = ReconnectionPolicy::new(cfg(2.0, 30.0, 10, 2.0, 0.0));
    assert!(approx(p.next_delay().unwrap(), 2.0, 0.01));
}

#[test]
fn second_and_third_delay() {
    let p = ReconnectionPolicy::new(cfg(1.0, 30.0, 10, 2.0, 0.0));
    p.next_delay();
    assert!(approx(p.next_delay().unwrap(), 2.0, 0.01));
    assert!(approx(p.next_delay().unwrap(), 4.0, 0.01));
}

#[test]
fn backoff_sequence() {
    let p = ReconnectionPolicy::new(cfg(1.0, 100.0, 10, 2.0, 0.0));
    for exp in [1.0, 2.0, 4.0, 8.0, 16.0] {
        let d = p.next_delay().unwrap();
        assert!(approx(d, exp, 0.01), "expected {exp}, got {d}");
    }
}

#[test]
fn backoff_with_multiplier_1_5() {
    let p = ReconnectionPolicy::new(cfg(1.0, 100.0, 10, 1.5, 0.0));
    assert!(approx(p.next_delay().unwrap(), 1.0, 0.01));
    assert!(approx(p.next_delay().unwrap(), 1.5, 0.01));
    assert!(approx(p.next_delay().unwrap(), 2.25, 0.01));
}

#[test]
fn delay_is_capped() {
    let p = ReconnectionPolicy::new(cfg(10.0, 15.0, 10, 2.0, 0.0));
    p.next_delay();
    assert!(approx(p.next_delay().unwrap(), 15.0, 0.01));
    assert!(approx(p.next_delay().unwrap(), 15.0, 0.01));
}

#[test]
fn delays_never_exceed_max() {
    let p = ReconnectionPolicy::new(cfg(1.0, 10.0, 20, 2.0, 0.0));
    for _ in 0..20 {
        if let Some(d) = p.next_delay() {
            assert!(d <= 10.0);
        }
    }
}

#[test]
fn backoff_curve_formula() {
    let p = ReconnectionPolicy::new(cfg(2.0, 1000.0, 8, 1.5, 0.0));
    for n in 1..=8 {
        let expected = 2.0 * 1.5f64.powi(n - 1);
        assert!(approx(p.next_delay().unwrap(), expected, 0.001), "attempt {n}");
    }
}

#[test]
fn clamp_around_max_delay() {
    let p = ReconnectionPolicy::new(cfg(1.0, 10.0, 10, 2.0, 0.0));
    let ds: Vec<f64> = (0..6).map(|_| p.next_delay().unwrap()).collect();
    for (d, e) in ds.iter().zip([1.0, 2.0, 4.0, 8.0, 10.0, 10.0]) {
        assert!(approx(*d, e, 0.001));
    }
}

// MARK: - Jitter

#[test]
fn jitter_within_bounds() {
    for _ in 0..100 {
        let p = ReconnectionPolicy::new(cfg(10.0, 30.0, 10, 2.0, 0.25));
        let d = p.next_delay().unwrap();
        assert!(d >= 0.1);
        assert!(d <= 12.5, "{d} exceeds upper jitter bound");
        assert!(d >= 7.5 - 0.0001);
    }
    for _ in 0..100 {
        let p = ReconnectionPolicy::new(cfg(8.0, 100.0, 10, 2.0, 0.25));
        let d = p.next_delay().unwrap();
        assert!((6.0 - 0.0001..=10.0 + 0.0001).contains(&d));
    }
}

#[test]
fn zero_jitter_is_exact() {
    let p = ReconnectionPolicy::new(cfg(5.0, 30.0, 10, 2.0, 0.0));
    assert!(approx(p.next_delay().unwrap(), 5.0, 0.01));
}

#[test]
fn delay_floor_is_0_1() {
    for _ in 0..100 {
        let p = ReconnectionPolicy::new(cfg(0.01, 30.0, 10, 2.0, 1.0));
        assert!(p.next_delay().unwrap() >= 0.1);
    }
}

// MARK: - Exhaustion

#[test]
fn returns_none_after_max_attempts() {
    let p = ReconnectionPolicy::new(cfg(1.0, 30.0, 3, 2.0, 0.0));
    assert!(p.next_delay().is_some());
    assert!(p.next_delay().is_some());
    assert!(p.next_delay().is_some());
    assert!(p.next_delay().is_none());
    assert_eq!(p.state(), ReconnectionState::Exhausted);
    assert!(!p.should_reconnect());
    assert!(p.is_exhausted());
}

#[test]
fn attempts_remaining_decreases() {
    let p = ReconnectionPolicy::new(cfg(1.0, 30.0, 5, 2.0, 0.0));
    assert_eq!(p.attempts_remaining(), 5);
    p.next_delay();
    assert_eq!(p.attempts_remaining(), 4);
    p.next_delay();
    assert_eq!(p.attempts_remaining(), 3);
}

#[test]
fn zero_max_attempts_returns_none() {
    let p = ReconnectionPolicy::new(cfg(1.0, 30.0, 0, 2.0, 0.0));
    assert!(p.next_delay().is_none());
}

// MARK: - execute_reconnection

#[tokio::test]
async fn cancellation_stops_reconnection_loop() {
    let p = ReconnectionPolicy::new(cfg(0.001, 0.001, 5, 1.0, 0.0));
    let count = Arc::new(AtomicUsize::new(0));
    let c = count.clone();
    let result = p
        .execute_reconnection(move || {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err(ChatError::Cancelled)
            }
        })
        .await;
    assert_eq!(result, Err(ChatError::Cancelled));
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn failing_action_exhausts() {
    // max 4 → delay floor 0.1 × 4 회.
    let p = ReconnectionPolicy::new(cfg(0.001, 0.001, 4, 1.0, 0.0));
    let count = Arc::new(AtomicUsize::new(0));
    let c = count.clone();
    let result = p
        .execute_reconnection(move || {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err(ChatError::ConnectionFailed("x".into()))
            }
        })
        .await;
    assert!(matches!(result, Err(ChatError::ConnectionFailed(_))));
    assert_eq!(count.load(Ordering::SeqCst), 4);
    assert!(p.is_exhausted());
}

#[tokio::test]
async fn success_marks_connected_and_keeps_attempts() {
    let p = ReconnectionPolicy::new(cfg(0.001, 0.001, 4, 1.0, 0.0));
    let count = Arc::new(AtomicUsize::new(0));
    let c = count.clone();
    let result = p
        .execute_reconnection(move || {
            let c = c.clone();
            async move {
                let n = c.fetch_add(1, Ordering::SeqCst) + 1;
                if n < 3 {
                    Err(ChatError::ConnectionFailed("x".into()))
                } else {
                    Ok(())
                }
            }
        })
        .await;
    assert_eq!(result, Ok(()));
    assert_eq!(count.load(Ordering::SeqCst), 3);
    assert_eq!(p.state(), ReconnectionState::Connected);
    assert_eq!(p.attempts_remaining(), 1);
}

// MARK: - Reset & state

#[test]
fn reset_sets_idle_and_restores() {
    let p = ReconnectionPolicy::new(cfg(1.0, 30.0, 5, 2.0, 0.0));
    p.next_delay();
    p.next_delay();
    p.reset();
    assert_eq!(p.state(), ReconnectionState::Idle);
    assert_eq!(p.attempts_remaining(), 5);
}

#[test]
fn reset_allows_reuse_after_exhaustion() {
    let p = ReconnectionPolicy::new(cfg(1.0, 30.0, 2, 2.0, 0.0));
    p.next_delay();
    p.next_delay();
    p.next_delay();
    assert!(p.is_exhausted());
    p.reset();
    assert!(!p.is_exhausted());
    assert!(p.should_reconnect());
    assert!(p.next_delay().is_some());
}

#[test]
fn mark_connected_sets_connected() {
    let p = ReconnectionPolicy::default();
    p.mark_connected();
    assert_eq!(p.state(), ReconnectionState::Connected);
}

#[test]
fn mark_connected_preserves_attempts() {
    let p = ReconnectionPolicy::new(cfg(1.0, 30.0, 10, 2.0, 0.0));
    p.next_delay();
    p.next_delay();
    p.next_delay();
    p.mark_connected();
    assert_eq!(p.attempts_remaining(), 7);
}

#[test]
fn short_lived_connection_continues_delay() {
    let p = ReconnectionPolicy::new(cfg(1.0, 100.0, 10, 2.0, 0.0));
    p.next_delay();
    p.next_delay();
    p.next_delay();
    p.mark_connected();
    assert!(approx(p.next_delay().unwrap(), 8.0, 0.01));
}

#[test]
fn cancel_and_initial_state_idle() {
    let p = ReconnectionPolicy::default();
    assert_eq!(p.state(), ReconnectionState::Idle);
    p.mark_connected();
    p.cancel();
    assert_eq!(p.state(), ReconnectionState::Idle);
}

#[test]
fn state_is_waiting_after_next_delay() {
    let p = ReconnectionPolicy::new(cfg(1.0, 30.0, 5, 2.0, 0.0));
    p.next_delay();
    match p.state() {
        ReconnectionState::Waiting { attempt, .. } => assert_eq!(attempt, 1),
        other => panic!("expected waiting, got {other:?}"),
    }
}

// MARK: - Reset threshold

#[test]
fn below_threshold_no_reset() {
    let p = ReconnectionPolicy::new(cfg_reset(1.0, 100.0, 10, 2.0, 0.0, 60.0));
    p.next_delay();
    p.next_delay();
    p.mark_connected();
    assert!(approx(p.next_delay().unwrap(), 4.0, 0.01), "짧은 성공 뒤에는 기존 backoff를 이어야 한다");
}

#[test]
fn mark_connected_does_not_immediately_reset() {
    let p = ReconnectionPolicy::new(cfg_reset(1.0, 100.0, 10, 2.0, 0.0, 60.0));
    for _ in 0..4 {
        p.next_delay();
    }
    assert_eq!(p.attempts_remaining(), 6);
    p.mark_connected();
    assert_eq!(p.attempts_remaining(), 6);
    assert_eq!(p.state(), ReconnectionState::Connected);
}

#[test]
fn stable_connection_resets_once() {
    let p = ReconnectionPolicy::new(cfg_reset(1.0, 100.0, 5, 2.0, 0.0, 0.001));
    p.next_delay();
    p.next_delay();
    p.mark_connected();
    std::thread::sleep(Duration::from_millis(20));
    p.next_delay();
    assert_eq!(p.attempts_remaining(), 4);
    std::thread::sleep(Duration::from_millis(20));
    p.next_delay();
    assert_eq!(p.attempts_remaining(), 3);
}

#[test]
fn stable_success_at_max_attempt_can_reset() {
    let p = ReconnectionPolicy::new(cfg_reset(1.0, 10.0, 2, 2.0, 0.0, 0.0));
    p.next_delay();
    p.next_delay();
    p.mark_connected();
    assert!(p.should_reconnect());
    assert_eq!(p.next_delay(), Some(1.0));
    assert_eq!(p.attempts_remaining(), 1);
}

#[test]
fn short_success_at_max_attempt_still_exhausts() {
    let p = ReconnectionPolicy::new(cfg_reset(1.0, 10.0, 2, 2.0, 0.0, 60.0));
    p.next_delay();
    p.next_delay();
    p.mark_connected();
    assert!(p.should_reconnect());
    assert_eq!(p.next_delay(), None);
    assert!(p.is_exhausted());
}

// MARK: - Presets

#[test]
fn default_configuration_values() {
    let c = ReconnectionConfiguration::default();
    assert_eq!(c.initial_delay, 1.0);
    assert_eq!(c.max_delay, reconnect_defaults::DEFAULT_MAX_DELAY);
    assert_eq!(c.max_attempts, reconnect_defaults::DEFAULT_MAX_ATTEMPTS);
    assert_eq!(c.backoff_multiplier, 2.0);
    assert_eq!(c.jitter_factor, reconnect_defaults::DEFAULT_JITTER);
    assert_eq!(c.reset_threshold, reconnect_defaults::RESET_THRESHOLD);
}

#[test]
fn aggressive_configuration_values() {
    let c = ReconnectionConfiguration::aggressive();
    assert_eq!(c.initial_delay, 0.5);
    assert_eq!(c.max_delay, reconnect_defaults::AGGRESSIVE_MAX_DELAY);
    assert_eq!(c.max_attempts, reconnect_defaults::AGGRESSIVE_MAX_ATTEMPTS);
    assert_eq!(c.backoff_multiplier, 1.5);
}

// MARK: - Status description

#[test]
fn status_descriptions() {
    let p = ReconnectionPolicy::default();
    assert_eq!(p.status_description(), "대기 중");
    p.mark_connected();
    assert_eq!(p.status_description(), "연결됨");

    let p = ReconnectionPolicy::new(cfg(1.0, 30.0, 1, 2.0, 0.0));
    p.next_delay();
    p.next_delay();
    assert_eq!(p.status_description(), "재연결 실패");

    let p = ReconnectionPolicy::new(cfg(5.0, 30.0, 10, 2.0, 0.0));
    p.next_delay();
    let desc = p.status_description();
    assert!(desc.contains("1/10"), "{desc}");
    assert!(desc.contains('5'), "{desc}");
}
