//! 신선도 표시(U9)의 순수 부분 — 원천별 기한 표, 구간 판정, 캡션 문구.
//!
//! 원본: `Sources/CViewCore/Utilities/DataFreshness.swift`
//!
//! 로컬 우선 메뉴는 스냅샷·이전 응답을 먼저 그린다. 그 값을 최신으로 오인하지 않게 화면이
//! "n분 전 기준"·"HH:MM 집계" 를 붙인다. 무엇을 언제 붙일지는 여기서만 정한다.

use std::fmt::Display;

use chrono::TimeZone;

use super::relative_time::korean_elapsed;
use super::time::{adding, interval_since, Date, TimeInterval};

/// 원천별 신선도 기한. 나이가 `fresh` 에 닿으면 stale, `stale` 에 닿으면 expired 다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FreshnessPolicy {
    /// 이 나이 전까지는 최신으로 본다(초).
    pub fresh: TimeInterval,
    /// 이 나이 전까지는 오래됐지만 보여 줄 만하다(초). 닿으면 만료.
    pub stale: TimeInterval,
}

impl FreshnessPolicy {
    /// 인기 라이브 — 1분 fresh, 10분 stale.
    pub const POPULAR: Self = Self {
        fresh: 60.0,
        stale: 600.0,
    };
    /// 팔로잉 — 홈 폴 180초 한 번은 fresh, 15분 stale.
    pub const FOLLOWING: Self = Self {
        fresh: 180.0,
        stale: 900.0,
    };
    /// 전수 수집 카탈로그 — 30분 fresh, 하루 stale.
    pub const CATALOG: Self = Self {
        fresh: 1_800.0,
        stale: 86_400.0,
    };
    /// 채널 상세 프로필 — 5분 fresh, 7일 stale(채널 스냅샷 보존 기한).
    pub const CHANNEL_PROFILE: Self = Self {
        fresh: 300.0,
        stale: 604_800.0,
    };

    /// `stale` 이 `fresh` 보다 짧게 들어와도 구간이 뒤집히지 않게 `max(fresh, stale)` 로 올린다.
    pub fn new(fresh: TimeInterval, stale: TimeInterval) -> Self {
        Self {
            fresh,
            stale: fresh.max(stale),
        }
    }
}

/// 신선도 구간.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FreshnessBand {
    Fresh,
    Stale,
    Expired,
}

/// 수집 시각의 신선도 구간.
///
/// - 미래 시각(시계 역행)은 **stale** 이다.
/// - 구간은 반열림이다: [0, fresh) fresh, [fresh, stale) stale, 그 뒤 expired.
/// - `fetched_at` 이 `None` 이면(받은 적 없음) expired 다.
pub fn band(fetched_at: Option<&Date>, policy: &FreshnessPolicy, now: &Date) -> FreshnessBand {
    let Some(fetched_at) = fetched_at else {
        return FreshnessBand::Expired;
    };
    let age = interval_since(now, fetched_at);
    if age < 0.0 {
        FreshnessBand::Stale
    } else if age < policy.fresh {
        FreshnessBand::Fresh
    } else if age < policy.stale {
        FreshnessBand::Stale
    } else {
        FreshnessBand::Expired
    }
}

/// 다음에 구간이 바뀌는 시각. 이미 만료면 `None`. 미래 시각은 그 시각이 되면 fresh 로 돌아온다.
pub fn next_band_change(
    fetched_at: Option<&Date>,
    policy: &FreshnessPolicy,
    now: &Date,
) -> Option<Date> {
    let fetched_at = fetched_at?;
    match band(Some(fetched_at), policy, now) {
        FreshnessBand::Fresh => Some(adding(fetched_at, policy.fresh)),
        FreshnessBand::Stale if fetched_at > now => Some(*fetched_at),
        FreshnessBand::Stale => Some(adding(fetched_at, policy.stale)),
        FreshnessBand::Expired => None,
    }
}

/// "12분 전"·"3시간 전" — 1분 미만이거나 미래 시각이면 `None`.
pub fn age_caption(fetched_at: &Date, now: &Date) -> Option<String> {
    let age = interval_since(now, fetched_at);
    (age.is_finite() && age >= 60.0).then(|| korean_elapsed(age))
}

/// 섹션 머리 캡션 "12분 전 기준".
///
/// 스냅샷에서 되살린 값(`restored`)이거나 구간이 fresh 가 아닐 때만 붙인다.
pub fn section_caption(
    fetched_at: Option<&Date>,
    restored: bool,
    policy: &FreshnessPolicy,
    now: &Date,
) -> Option<String> {
    let fetched_at = fetched_at?;
    if !restored && band(Some(fetched_at), policy, now) == FreshnessBand::Fresh {
        return None;
    }
    age_caption(fetched_at, now).map(|c| format!("{c} 기준"))
}

/// "14:05" — 시각만(24시간 두 자리). 원본의 `calendar` 인자는 타임존 `tz` 로 대신한다.
pub fn clock_text<Tz: TimeZone>(date: &Date, tz: &Tz) -> String
where
    Tz::Offset: Display,
{
    date.with_timezone(tz).format("%H:%M").to_string()
}

/// "14:05 집계" — 한 시점에 모아 계산한 총계의 기준 시각.
pub fn aggregated_caption<Tz: TimeZone>(date: &Date, tz: &Tz) -> String
where
    Tz::Offset: Display,
{
    format!("{} 집계", clock_text(date, tz))
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewCoreTests/DataFreshnessTests.swift`
    use super::*;
    use crate::utils::time::from_unix;
    use chrono::FixedOffset;

    fn base() -> Date {
        from_unix(1_790_000_000.0)
    }

    #[test]
    fn policy_table() {
        assert_eq!(FreshnessPolicy::POPULAR, FreshnessPolicy::new(60.0, 600.0));
        assert_eq!(
            FreshnessPolicy::FOLLOWING,
            FreshnessPolicy::new(180.0, 900.0)
        );
        assert_eq!(
            FreshnessPolicy::CATALOG,
            FreshnessPolicy::new(1_800.0, 86_400.0)
        );
        assert_eq!(
            FreshnessPolicy::CHANNEL_PROFILE,
            FreshnessPolicy::new(300.0, 604_800.0)
        );
        assert_eq!(FreshnessPolicy::new(100.0, 10.0).stale, 100.0);
    }

    #[test]
    fn band_boundaries_half_open() {
        use FreshnessBand::*;
        for (age, expected) in [
            (0.0, Fresh),
            (59.9, Fresh),
            (60.0, Stale),
            (599.9, Stale),
            (600.0, Expired),
            (86_400.0, Expired),
        ] {
            let fetched = base();
            assert_eq!(
                band(
                    Some(&fetched),
                    &FreshnessPolicy::POPULAR,
                    &adding(&fetched, age)
                ),
                expected,
                "{age}"
            );
        }
    }

    #[test]
    fn future_and_missing() {
        let b = base();
        assert_eq!(
            band(Some(&adding(&b, 5.0)), &FreshnessPolicy::FOLLOWING, &b),
            FreshnessBand::Stale
        );
        assert_eq!(
            band(
                Some(&adding(&b, 86_400.0 * 30.0)),
                &FreshnessPolicy::CATALOG,
                &b
            ),
            FreshnessBand::Stale
        );
        assert_eq!(
            band(None, &FreshnessPolicy::POPULAR, &b),
            FreshnessBand::Expired
        );
    }

    #[test]
    fn next_band_change_lands_on_new_band() {
        let policy = FreshnessPolicy::FOLLOWING;
        let fetched = base();
        let first = next_band_change(Some(&fetched), &policy, &adding(&fetched, 10.0)).unwrap();
        assert_eq!(first, adding(&fetched, 180.0));
        assert_eq!(band(Some(&fetched), &policy, &first), FreshnessBand::Stale);

        let second = next_band_change(Some(&fetched), &policy, &first).unwrap();
        assert_eq!(second, adding(&fetched, 900.0));
        assert_eq!(
            band(Some(&fetched), &policy, &second),
            FreshnessBand::Expired
        );
        assert_eq!(next_band_change(Some(&fetched), &policy, &second), None);

        let future = adding(&fetched, 30.0);
        assert_eq!(
            next_band_change(Some(&future), &policy, &fetched),
            Some(future)
        );
        assert_eq!(band(Some(&future), &policy, &future), FreshnessBand::Fresh);
        assert_eq!(next_band_change(None, &policy, &fetched), None);
    }

    #[test]
    fn age_caption_cases() {
        let cases: [(f64, Option<&str>); 8] = [
            (0.0, None),
            (59.9, None),
            (60.0, Some("1분 전")),
            (3_599.0, Some("59분 전")),
            (3_600.0, Some("1시간 전")),
            (86_399.0, Some("23시간 전")),
            (86_400.0 * 2.5, Some("2일 전")),
            (-120.0, None),
        ];
        let now = base();
        for (age, expected) in cases {
            assert_eq!(
                age_caption(&adding(&now, -age), &now).as_deref(),
                expected,
                "{age}"
            );
        }
    }

    #[test]
    fn section_caption_rules() {
        let now = base();
        let p_follow = FreshnessPolicy::FOLLOWING;
        let p_pop = FreshnessPolicy::POPULAR;
        assert_eq!(
            section_caption(Some(&adding(&now, -90.0)), false, &p_follow, &now),
            None
        );
        assert_eq!(
            section_caption(Some(&adding(&now, -90.0)), true, &p_follow, &now).as_deref(),
            Some("1분 전 기준")
        );
        assert_eq!(
            section_caption(Some(&adding(&now, -300.0)), false, &p_pop, &now).as_deref(),
            Some("5분 전 기준")
        );
        assert_eq!(
            section_caption(Some(&adding(&now, -10.0)), true, &p_pop, &now),
            None
        );
        assert_eq!(section_caption(None, true, &p_pop, &now), None);
    }

    #[test]
    fn aggregated_caption_two_digit_24h() {
        let seoul = FixedOffset::east_opt(9 * 3600).unwrap();
        for (h, m, expected) in [
            (14, 5, "14:05 집계"),
            (9, 3, "09:03 집계"),
            (0, 0, "00:00 집계"),
            (23, 59, "23:59 집계"),
        ] {
            let date = seoul
                .with_ymd_and_hms(2026, 9, 29, h, m, 42)
                .unwrap()
                .to_utc();
            assert_eq!(aggregated_caption(&date, &seoul), expected);
        }
    }
}
