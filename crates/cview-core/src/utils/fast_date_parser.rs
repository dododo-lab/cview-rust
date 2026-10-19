//! Chzzk 날짜 문자열 고속 파서.
//!
//! 원본: `Sources/CViewCore/Utilities/FastDateParser.swift`
//!
//! 원본은 Foundation formatter 체인(최대 487배 느림) 앞에 두는 **고속 경로**다. 문자열이 알려진
//! 형태와 바이트 단위로 정확히 일치할 때만 정수 파싱을 하고, 어긋나면 `None` 을 돌려준다.
//! formatter 가 거절하는 입력(존재하지 않는 날짜, 초 60, 오프셋 없는 ISO8601)은 여기서도 거절한다.
//!
//! ⚠ 타임존 의미:
//! - `yyyy-MM-dd HH:mm:ss` → **시스템 로컬 타임존**으로 해석한다([`parse`]). 테스트·서버 등에서
//!   다른 기준이 필요하면 [`parse_in`] 에 타임존을 넘긴다.
//! - ISO8601(`T` 구분자) → 문자열이 들고 있는 오프셋(`Z` 또는 `±HH:MM`/`±HHMM`)을 적용한다.

use chrono::{DateTime, Local, NaiveDate, TimeZone, Utc};

/// 그레고리력 해당 월의 일수.
fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => 0,
    }
}

/// `s` 가 알려진 형태면 시각, 아니면 `None`. 공백 구분 형식은 시스템 로컬 타임존으로 해석한다.
pub fn parse(s: &str) -> Option<DateTime<Utc>> {
    parse_in(s, &Local)
}

/// [`parse`] 와 같되, 공백 구분 형식(`yyyy-MM-dd HH:mm:ss`)을 `local` 타임존으로 해석한다.
///
/// 로컬 시각이 DST 로 겹치면 이른 쪽을, 존재하지 않으면(DST 공백) `None` 을 돌려준다.
pub fn parse_in<Tz: TimeZone>(s: &str, local: &Tz) -> Option<DateTime<Utc>> {
    let u = s.as_bytes();
    if u.len() > 32 || u.len() < 19 {
        return None;
    }
    if u[4] != b'-' || u[7] != b'-' || u[13] != b':' || u[16] != b':' {
        return None;
    }
    let sep = u[10];
    if sep != b' ' && sep != b'T' {
        return None;
    }

    let num = |a: usize, b: usize| -> Option<i64> {
        u[a..b].iter().try_fold(0i64, |v, &c| {
            c.is_ascii_digit().then(|| v * 10 + i64::from(c - b'0'))
        })
    };
    let year = num(0, 4)?;
    let month = num(5, 7)?;
    let day = num(8, 10)?;
    let hour = num(11, 13)?;
    let minute = num(14, 16)?;
    let second = num(17, 19)?;

    // 범위 검사 — formatter 가 거절하는 입력(그 달에 없는 날, 초 60 등)은 고속 경로도 거절한다.
    if !(1..=12).contains(&month)
        || !(1..=days_in_month(year, month)).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=59).contains(&second)
    {
        return None;
    }

    // ── 형식 A: "yyyy-MM-dd HH:mm:ss" (로컬 타임존) ──
    if sep == b' ' {
        if u.len() != 19 {
            return None;
        }
        let naive = NaiveDate::from_ymd_opt(year as i32, month as u32, day as u32)?.and_hms_opt(
            hour as u32,
            minute as u32,
            second as u32,
        )?;
        return local
            .from_local_datetime(&naive)
            .earliest()
            .map(|d| d.with_timezone(&Utc));
    }

    // ── 형식 B: ISO8601 "yyyy-MM-ddTHH:mm:ss[.fff](Z|±HH:MM|±HHMM)" ──
    let mut i = 19;
    let mut nanos: i64 = 0;
    if i < u.len() && u[i] == b'.' {
        i += 1;
        let start = i;
        let mut scale: i64 = 100_000_000;
        while i < u.len() && u[i].is_ascii_digit() {
            nanos += i64::from(u[i] - b'0') * scale;
            scale /= 10;
            i += 1;
        }
        if i == start {
            return None; // '.' 뒤에 숫자가 없다
        }
    }

    // 오프셋 없음은 거절(ISO8601DateFormatter `.withInternetDateTime` 과 같다).
    let &c = u.get(i)?;
    let offset_seconds = match c {
        b'Z' => {
            if i + 1 != u.len() {
                return None;
            }
            0
        }
        b'+' | b'-' => {
            let sign = if c == b'+' { 1 } else { -1 };
            let rest = u.len() - i - 1;
            let (oh, om) = if rest == 5 && u[i + 3] == b':' {
                (num(i + 1, i + 3)?, num(i + 4, i + 6)?)
            } else if rest == 4 {
                (num(i + 1, i + 3)?, num(i + 3, i + 5)?)
            } else {
                return None;
            };
            if oh > 23 || om > 59 {
                return None;
            }
            sign * (oh * 3600 + om * 60)
        }
        _ => return None,
    };

    // days-from-civil (Howard Hinnant) — 순수 정수 연산.
    let y = year - i64::from(month <= 2);
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + hour * 3600 + minute * 60 + second - offset_seconds;
    DateTime::<Utc>::from_timestamp(secs, nanos as u32)
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewNetworkingTests/ChzzkDateParsingTests.swift`
    //!
    //! 원본은 Foundation formatter 체인(참조 구현)과 대조한다. 여기서는 참조를
    //! `chrono` 의 엄격한 달력 생성(`NaiveDate::from_ymd_opt`)으로 대신하고 기대 시각을 직접 적는다.
    use super::*;
    use chrono::FixedOffset;

    fn kst() -> FixedOffset {
        FixedOffset::east_opt(9 * 3600).unwrap()
    }

    fn ts(s: &str) -> Option<i64> {
        parse_in(s, &kst()).map(|d| d.timestamp_millis())
    }

    fn reference_local(y: i32, mo: u32, d: u32, h: u32, mi: u32, se: u32) -> Option<i64> {
        let naive = NaiveDate::from_ymd_opt(y, mo, d)?.and_hms_opt(h, mi, se)?;
        Some(
            kst()
                .from_local_datetime(&naive)
                .single()?
                .timestamp_millis(),
        )
    }

    fn reference_utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, se: u32) -> Option<i64> {
        let naive = NaiveDate::from_ymd_opt(y, mo, d)?.and_hms_opt(h, mi, se)?;
        Some(naive.and_utc().timestamp_millis())
    }

    #[test]
    fn legacy_format_is_local_time() {
        for (s, parts) in [
            ("2024-06-15 18:30:00", (2024, 6, 15, 18, 30, 0)),
            ("2026-01-01 00:00:00", (2026, 1, 1, 0, 0, 0)),
            ("2026-12-31 23:59:59", (2026, 12, 31, 23, 59, 59)),
            ("2024-02-29 12:00:00", (2024, 2, 29, 12, 0, 0)),
            ("2023-02-28 12:00:00", (2023, 2, 28, 12, 0, 0)),
            ("2026-08-19 11:58:13", (2026, 8, 19, 11, 58, 13)),
        ] {
            let (y, mo, d, h, mi, se) = parts;
            assert_eq!(ts(s), reference_local(y, mo, d, h, mi, se), "{s}");
            assert!(ts(s).is_some(), "{s}");
        }
    }

    #[test]
    fn iso8601_offsets_and_fractions() {
        let base = reference_utc(2026, 8, 19, 11, 58, 13).unwrap();
        assert_eq!(
            ts("2024-01-15T12:00:00Z"),
            reference_utc(2024, 1, 15, 12, 0, 0)
        );
        assert_eq!(
            ts("2024-12-25T09:30:00.123Z"),
            reference_utc(2024, 12, 25, 9, 30, 0).map(|v| v + 123)
        );
        assert_eq!(ts("2026-08-19T11:58:13Z"), Some(base));
        assert_eq!(ts("2026-08-19T11:58:13.5Z"), Some(base + 500));
        assert_eq!(ts("2026-08-19T11:58:13.999Z"), Some(base + 999));
        assert_eq!(ts("2026-08-19T11:58:13+09:00"), Some(base - 9 * 3_600_000));
        assert_eq!(ts("2026-08-19T11:58:13-05:00"), Some(base + 5 * 3_600_000));
        assert_eq!(
            ts("2026-08-19T11:58:13.250+09:00"),
            Some(base - 9 * 3_600_000 + 250)
        );
        assert_eq!(ts("2026-08-19T11:58:13+0900"), Some(base - 9 * 3_600_000));
        assert_eq!(
            ts("2026-08-19T00:00:00+00:00"),
            reference_utc(2026, 8, 19, 0, 0, 0)
        );
    }

    #[test]
    fn malformed_inputs_rejected() {
        for s in [
            "",
            "not-a-date",
            "2024-06-15",
            "2024-06-15T18:30:00",
            "2024-06-15 18:30",
            "2024-06-15 18:30:00.500",
            "2024-13-15 18:30:00",
            "2024-06-32 18:30:00",
            "2024-06-15 25:30:00",
            "2024-06-15 18:61:00",
            "20240615183000",
            "2024/06/15 18:30:00",
            "2024-06-15T18:30:00.Z",
            "2024-06-15T18:30:00+9:00",
            "2024-06-15T18:30:00ZZ",
            "abcd-06-15 18:30:00",
            "2024-06-15T18:30:00.000000000000000000Z",
        ] {
            assert_eq!(ts(s), None, "{s:?}");
        }
    }

    #[test]
    fn impossible_calendar_dates_rejected() {
        for s in [
            "2023-02-29 12:00:00",
            "2024-02-30 12:00:00",
            "2024-04-31 18:30:00",
            "2026-06-31 00:00:00",
            "2026-09-31 23:59:59",
            "1900-02-29 00:00:00",
            "2023-02-29T12:00:00Z",
            "2024-04-31T18:30:00+09:00",
        ] {
            assert_eq!(ts(s), None, "{s}");
        }
        assert!(ts("2000-02-29 12:00:00").is_some());
        assert!(ts("2024-02-29 12:00:00").is_some());
    }

    #[test]
    fn leap_second_rejected() {
        for s in [
            "2024-06-15 18:30:60",
            "2024-06-15T18:30:60Z",
            "2016-12-31T23:59:60Z",
        ] {
            assert_eq!(ts(s), None, "{s}");
        }
    }

    #[test]
    fn valid_inputs_take_fast_path() {
        for s in [
            "2024-02-29 12:00:00",
            "2024-01-31 23:59:59",
            "2026-04-30 00:00:00",
            "2026-08-19T11:58:13.123Z",
        ] {
            assert!(ts(s).is_some(), "{s}");
        }
        assert!(
            parse("2026-08-19T11:58:13.123Z").is_some(),
            "시스템 로컬 경로"
        );
    }

    #[test]
    fn randomized_equivalence() {
        let mut seed: u64 = 0x5EED_1234;
        let mut next = |bound: u64| -> u32 {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((seed >> 33) % bound) as u32
        };
        for _ in 0..500 {
            let y = 2000 + next(50) as i32;
            let mo = 1 + next(12);
            let d = 1 + next(31);
            let (h, mi, se) = (next(24), next(60), next(60));
            let stamp = format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{se:02}");
            assert_eq!(ts(&stamp), reference_local(y, mo, d, h, mi, se), "{stamp}");
            let iso = format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{se:02}Z");
            assert_eq!(ts(&iso), reference_utc(y, mo, d, h, mi, se), "{iso}");
        }
    }

    #[test]
    fn hls_program_date_time() {
        let base = reference_utc(2026, 8, 19, 11, 58, 13).unwrap();
        assert_eq!(ts("2026-08-19T11:58:13.000Z"), Some(base));
        assert_eq!(ts("2026-08-19T11:58:13.033Z"), Some(base + 33));
        assert_eq!(ts("2026-08-19T11:58:13Z"), Some(base));
        assert_eq!(ts("2026-08-19T20:58:13.500+09:00"), Some(base + 500));
    }
}
