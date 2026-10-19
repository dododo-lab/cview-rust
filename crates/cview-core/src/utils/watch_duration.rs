//! 시청 시간 표기 단일 출처.
//!
//! 원본: `Sources/CViewCore/Utilities/WatchDurationFormat.swift` (`TimeInterval.formattedWatchDuration`)
//!
//! ⚠ 0값 표기를 강제 통일하지 않는다 — `"0초"`(실제 0)와 `"-"`(평균 미정의)은 다른 빈값이며
//!   그 판정은 호출부가 한다. ⚠ 라틴 약어(`h`/`m`)를 되살리지 말 것.

use super::time::TimeInterval;

/// `3시간 20분` / `20분` / `45초`. 음수·NaN 은 `0초` 로 접는다.
pub fn formatted_watch_duration(seconds: TimeInterval) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "0초".to_string();
    }
    let total = seconds as i64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    if hours > 0 {
        format!("{hours}시간 {minutes}분")
    } else if minutes > 0 {
        format!("{minutes}분")
    } else {
        format!("{}초", total % 60)
    }
}

#[cfg(test)]
mod tests {
    //! 원본 근거: `Tests/CViewPersistenceTests/DTOTests.swift` (0 → "0초")
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(formatted_watch_duration(0.0), "0초");
        assert_eq!(formatted_watch_duration(-5.0), "0초");
        assert_eq!(formatted_watch_duration(f64::NAN), "0초");
        assert_eq!(formatted_watch_duration(45.9), "45초");
        assert_eq!(formatted_watch_duration(20.0 * 60.0), "20분");
        assert_eq!(
            formatted_watch_duration(3.0 * 3600.0 + 20.0 * 60.0 + 5.0),
            "3시간 20분"
        );
        assert_eq!(formatted_watch_duration(3600.0), "1시간 0분");
    }
}
