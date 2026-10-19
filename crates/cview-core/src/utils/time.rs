//! Foundation `Date` / `TimeInterval` 대응 헬퍼.
//!
//! 포팅 규약: Swift `Date` → [`Date`] (`chrono::DateTime<Utc>`), `TimeInterval` → `f64` 초.
//! 단조 시계(uptime) 입력은 원본처럼 `f64` 초로 받는다.

use chrono::{DateTime, Duration, Utc};

/// Swift `Date` 대응.
pub type Date = DateTime<Utc>;

/// Swift `TimeInterval`(초) 대응.
pub type TimeInterval = f64;

/// `a.timeIntervalSince(b)` — `a - b` 를 초(소수 포함)로.
pub fn interval_since(a: &Date, b: &Date) -> TimeInterval {
    let d = *a - *b;
    match d.num_microseconds() {
        Some(us) => us as f64 / 1_000_000.0,
        None => d.num_milliseconds() as f64 / 1_000.0,
    }
}

/// `date.addingTimeInterval(secs)`. 표현 범위를 넘으면 포화한다.
pub fn adding(date: &Date, secs: TimeInterval) -> Date {
    if !secs.is_finite() {
        return if secs > 0.0 { DateTime::<Utc>::MAX_UTC } else { DateTime::<Utc>::MIN_UTC };
    }
    let micros = (secs * 1_000_000.0).round();
    let delta = if micros.abs() < i64::MAX as f64 {
        Duration::microseconds(micros as i64)
    } else {
        Duration::MAX
    };
    date.checked_add_signed(delta).unwrap_or(if secs > 0.0 {
        DateTime::<Utc>::MAX_UTC
    } else {
        DateTime::<Utc>::MIN_UTC
    })
}

/// `Date(timeIntervalSince1970:)`.
pub fn from_unix(secs: TimeInterval) -> Date {
    adding(&DateTime::<Utc>::UNIX_EPOCH, secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let base = from_unix(1_790_000_000.0);
        let later = adding(&base, 90.5);
        assert_eq!(interval_since(&later, &base), 90.5);
        assert_eq!(interval_since(&base, &later), -90.5);
    }
}
