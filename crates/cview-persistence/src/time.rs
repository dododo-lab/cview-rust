//! 날짜·달력 보조 함수.
//!
//! 원본: Swift Foundation `Calendar`(`startOfDay(for:)`, `date(byAdding:.day,…)`,
//! `component(.hour, from:)`)와 `Date` ↔ SQLite 저장 형식. `WatchRollupMath.swift`,
//! `DataStore+WatchHistorySync.swift` 가 달력을 주입받던 자리를 [`Calendar`] trait 로 옮겼다.
//! 날짜는 SQLite 에 Unix epoch 나노초 정수로 저장해 왕복이 정확히 같다.

use chrono::{
    DateTime, Datelike, Duration, LocalResult, NaiveDateTime, Offset, TimeZone, Timelike, Utc,
};
pub use cview_core::utils::time::{adding, from_unix, interval_since, Date, TimeInterval};

/// Foundation `Calendar` 가운데 이 크레이트가 쓰는 부분.
pub trait Calendar: Send + Sync {
    /// 그날 0시(현지). 0시가 없는 날(DST)은 그 뒤 첫 유효 시각.
    fn start_of_day(&self, date: Date) -> Date;
    /// 현지 벽시계를 유지한 채 `days` 일 더한다. 벽시계가 없는 시각이면 앞으로 민다.
    fn add_days(&self, days: i64, date: Date) -> Option<Date>;
    /// 현지 시(0…23).
    fn hour(&self, date: Date) -> u32;
    /// 현지 요일 — 월요일 0 … 일요일 6.
    fn weekday(&self, date: Date) -> u32;
    /// `date` 뒤의 첫 현지 정시(정시 자체면 한 시간 뒤).
    fn next_hour(&self, date: Date) -> Date;
    /// 그달 1일 0시(현지).
    fn start_of_month(&self, date: Date) -> Date;
}

fn resolve<Tz: TimeZone>(tz: &Tz, naive: NaiveDateTime) -> Option<Date> {
    // 모호하면 이른 쪽, 없으면 15분씩 앞으로(최대 3시간) 민다.
    let mut probe = naive;
    for _ in 0..=12 {
        match tz.from_local_datetime(&probe) {
            LocalResult::Single(d) => return Some(d.with_timezone(&Utc)),
            LocalResult::Ambiguous(a, b) => {
                let (a, b) = (a.with_timezone(&Utc), b.with_timezone(&Utc));
                return Some(a.min(b));
            }
            LocalResult::None => probe += Duration::minutes(15),
        }
    }
    None
}

impl<Tz> Calendar for Tz
where
    Tz: TimeZone + Send + Sync,
    Tz::Offset: Send + Sync,
{
    fn start_of_day(&self, date: Date) -> Date {
        let local = date.with_timezone(self);
        let midnight = local.date_naive().and_hms_opt(0, 0, 0).expect("자정");
        resolve(self, midnight).unwrap_or(date)
    }

    fn add_days(&self, days: i64, date: Date) -> Option<Date> {
        let local = date.with_timezone(self).naive_local();
        let shifted = local.checked_add_signed(Duration::try_days(days)?)?;
        resolve(self, shifted)
    }

    fn hour(&self, date: Date) -> u32 {
        date.with_timezone(self).hour()
    }

    fn weekday(&self, date: Date) -> u32 {
        date.with_timezone(self).weekday().num_days_from_monday()
    }

    fn next_hour(&self, date: Date) -> Date {
        // 현지 분·초를 덜어 한 시간 뒤(절대 시각)를 구한다. 그 사이에 오프셋이 바뀌면(서머타임, 또는
        // Pacific/Chatham 처럼 정시가 아닌 02:45·03:45 전환) 바뀌는 순간에서 끊어 시 칸이 섞이지 않게 한다.
        let local = date.with_timezone(self);
        let into_hour = Duration::seconds(i64::from(local.minute() * 60 + local.second()))
            + Duration::nanoseconds(i64::from(local.nanosecond()));
        let elapsed = date - into_hour + Duration::hours(1);
        let offset = |d: Date| d.with_timezone(self).offset().fix();
        let before = offset(date);
        if offset(elapsed) == before {
            return elapsed;
        }
        // 오프셋이 바뀌는 첫 순간을 초 단위로 좁힌다(한 시간 안에 전환은 한 번뿐).
        let (mut lo, mut hi) = (date, elapsed);
        while hi - lo > Duration::seconds(1) {
            let mid = lo + (hi - lo) / 2;
            if offset(mid) == before {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        hi
    }

    fn start_of_month(&self, date: Date) -> Date {
        let local = date.with_timezone(self);
        let first = local
            .date_naive()
            .with_day(1)
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .expect("1일 자정");
        resolve(self, first).unwrap_or(date)
    }
}

/// 현재 시각(`Date.now`).
pub fn now() -> Date {
    Utc::now()
}

/// `Date` → SQLite 정수(epoch 나노초). 범위를 넘으면 포화.
pub fn to_db(date: &Date) -> i64 {
    date.timestamp_nanos_opt()
        .unwrap_or(if date.timestamp() > 0 {
            i64::MAX
        } else {
            i64::MIN
        })
}

/// SQLite 정수 → `Date`.
pub fn from_db(nanos: i64) -> Date {
    DateTime::from_timestamp_nanos(nanos)
}

/// `timeIntervalSince1970`.
pub fn epoch_seconds(date: &Date) -> f64 {
    date.timestamp() as f64 + f64::from(date.timestamp_subsec_nanos()) / 1e9
}

/// Swift `Double.description` 모양의 문자열 — 정수 값에도 `.0` 이 붙는다(`1700000000.0`).
/// Rust `{:?}` 의 최단 왕복 표기와 같다(지수 표기 경계는 epoch 범위에서 차이 없음).
pub fn swift_double_description(value: f64) -> String {
    format!("{value:?}")
}

/// `Date` → ISO 8601(초 단위, `Z`) — Swift `JSONEncoder.dateEncodingStrategy = .iso8601`.
pub mod iso8601 {
    use super::Date;
    use chrono::{DateTime, SecondsFormat, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn format(date: &Date) -> String {
        date.to_rfc3339_opts(SecondsFormat::Secs, true)
    }

    pub fn parse(text: &str) -> Option<Date> {
        DateTime::parse_from_rfc3339(text)
            .ok()
            .map(|d| d.with_timezone(&Utc))
    }

    pub fn serialize<S: Serializer>(date: &Date, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format(date))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Date, D::Error> {
        let text = String::deserialize(d)?;
        parse(&text).ok_or_else(|| serde::de::Error::custom("ISO 8601 날짜가 아닙니다"))
    }

    /// `Option<Date>` 용.
    pub mod option {
        use super::Date;
        use serde::{Deserialize, Deserializer, Serializer};

        pub fn serialize<S: Serializer>(date: &Option<Date>, s: S) -> Result<S::Ok, S::Error> {
            match date {
                Some(d) => s.serialize_str(&super::format(d)),
                None => s.serialize_none(),
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Date>, D::Error> {
            let text = Option::<String>::deserialize(d)?;
            match text {
                None => Ok(None),
                Some(t) => super::parse(&t)
                    .map(Some)
                    .ok_or_else(|| serde::de::Error::custom("ISO 8601 날짜가 아닙니다")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_round_trip_is_exact() {
        let d = from_unix(1_700_000_000.123_456);
        assert_eq!(from_db(to_db(&d)), d);
    }

    #[test]
    fn swift_double_description_shape() {
        assert_eq!(swift_double_description(1_700_000_000.0), "1700000000.0");
        assert_eq!(swift_double_description(1_700_000_000.5), "1700000000.5");
    }

    #[test]
    fn calendar_start_of_day_and_add_days() {
        let tz = chrono_tz::Asia::Seoul;
        let d = Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap(); // 서울 12:04:05
        let sod = tz.start_of_day(d);
        assert_eq!(sod, Utc.with_ymd_and_hms(2026, 1, 1, 15, 0, 0).unwrap());
        assert_eq!(tz.hour(d), 12);
        assert_eq!(
            tz.add_days(-1, d).unwrap(),
            Utc.with_ymd_and_hms(2026, 1, 1, 3, 4, 5).unwrap()
        );
    }
}
