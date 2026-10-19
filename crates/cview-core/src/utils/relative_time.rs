//! 상대 시간 라벨.
//!
//! 원본: `Sources/CViewCore/Utilities/RelativeTimeFormat.swift`
//!
//! 원본의 `short`/`abbreviated`/`shortOrJustNow` 는 시스템 로캘을 따르는 Apple
//! `RelativeDateTimeFormatter` 래퍼라 옮기지 않았다(UI 단계에서 웹 `Intl.RelativeTimeFormat`
//! 으로 대체). 로캘과 무관한 한국어 규칙 [`korean_elapsed`] 만 옮긴다.

use super::time::TimeInterval;

/// "방금"·"12분 전"·"3시간 전"·"어제"·"5일 전"·"2주 전" — 경과 초를 로캘과 무관한 한국어로.
///
/// 1분 미만·음수·비유한 값은 "방금" 이다.
pub fn korean_elapsed(seconds: TimeInterval) -> String {
    if !seconds.is_finite() || seconds < 60.0 {
        return "방금".to_string();
    }
    if seconds < 3_600.0 {
        return format!("{}분 전", (seconds / 60.0) as i64);
    }
    if seconds < 86_400.0 {
        return format!("{}시간 전", (seconds / 3_600.0) as i64);
    }
    let days = (seconds.min(1e12) / 86_400.0) as i64;
    match days {
        1 => "어제".to_string(),
        2..=6 => format!("{days}일 전"),
        _ => format!("{}주 전", days / 7),
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewCoreTests/DataFreshnessTests.swift` (`koreanElapsed` 케이스)
    use super::*;

    #[test]
    fn korean_elapsed_cases() {
        let cases: [(f64, &str); 10] = [
            (f64::NAN, "방금"),
            (-30.0, "방금"),
            (30.0, "방금"),
            (60.0, "1분 전"),
            (7_200.0, "2시간 전"),
            (86_400.0 * 1.5, "어제"),
            (86_400.0 * 6.0, "6일 전"),
            (86_400.0 * 7.0, "1주 전"),
            (86_400.0 * 20.0, "2주 전"),
            (f64::INFINITY, "방금"),
        ];
        for (secs, expected) in cases {
            assert_eq!(korean_elapsed(secs), expected, "{secs}");
        }
    }
}
