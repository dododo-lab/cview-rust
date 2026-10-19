//! 모델 디코딩 공용 도구.
//!
//! - 날짜: `Sources/CViewNetworking/APIResponse.swift` 의 `JSONDecoder.chzzk` 날짜 전략과
//!   `Sources/CViewCore/Utilities/FastDateParser.swift` 의 판정 규칙을 옮겼다.
//! - 관대한 디코딩: Swift 커스텀 `init(from:)` 의 `try?` 필드 단위 복원과
//!   `Sources/CViewCore/Utilities/SafeDecoding.swift` 의 원소 단위 격리(`decodeArrayWithReport`)를
//!   `serde_json::Map` 위에서 재현한다.
//!
//! ⚠ 타임존 의미가 형식마다 다르다 (원본과 동일):
//!   - `yyyy-MM-dd HH:mm:ss` → **시스템 로컬 타임존**으로 해석
//!   - ISO8601(`T` 구분자) → 문자열이 들고 있는 오프셋(`Z` 또는 `±HH:MM`)

use chrono::{DateTime, FixedOffset, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use serde::de::{DeserializeOwned, Error as _};
use serde::{Deserialize, Deserializer, Serializer};
use serde_json::{Map, Value};
use url::Url;

/// Swift `Date` 의 기준 시각(2001-01-01T00:00:00Z)과 Unix epoch 의 차이(초).
/// Swift 기본 `JSONEncoder` 는 `Date` 를 이 기준의 Double 초로 인코딩한다.
pub const SWIFT_REFERENCE_DATE_OFFSET: f64 = 978_307_200.0;

/// 치지직 API 날짜 문자열 파싱. 원본 체인(고속 경로 → ISO8601(소수초) → ISO8601 → legacy)과
/// 같은 입력 집합을 받아들이고 같은 시각을 낸다.
///
/// - 존재하지 않는 달력 날짜(2월 30일 등)와 초 60(윤초)은 거절한다 — 원본 formatter 동작.
/// - 오프셋 없는 ISO(`2024-06-15T18:30:00`)는 거절한다.
pub fn parse_chzzk_date(s: &str) -> Option<DateTime<Utc>> {
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let digits = |r: std::ops::Range<usize>| -> Option<u32> {
        let mut v: u32 = 0;
        for &c in &b[r] {
            if !c.is_ascii_digit() {
                return None;
            }
            v = v * 10 + u32::from(c - b'0');
        }
        Some(v)
    };
    if b[4] != b'-' || b[7] != b'-' || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let year = digits(0..4)? as i32;
    let month = digits(5..7)?;
    let day = digits(8..10)?;
    let hour = digits(11..13)?;
    let minute = digits(14..16)?;
    let second = digits(17..19)?;
    let date = NaiveDate::from_ymd_opt(year, month, day)?;
    // from_hms_opt 는 초 60 을 거절한다(윤초는 nanos 로만 표현).
    let time = NaiveTime::from_hms_opt(hour, minute, second)?;
    let naive = NaiveDateTime::new(date, time);

    match b[10] {
        b' ' => {
            // legacy — 정확히 19자, 로컬 타임존.
            if b.len() != 19 {
                return None;
            }
            Local
                .from_local_datetime(&naive)
                .earliest()
                .map(|d| d.with_timezone(&Utc))
        }
        b'T' => {
            let mut idx = 19;
            let mut nanos: u32 = 0;
            if idx < b.len() && b[idx] == b'.' {
                idx += 1;
                let start = idx;
                while idx < b.len() && b[idx].is_ascii_digit() {
                    idx += 1;
                }
                if idx == start {
                    return None;
                }
                // 앞 9자리까지만 나노초로 쓴다.
                let frac = &s[start..idx];
                let mut n: u64 = 0;
                let mut count = 0;
                for c in frac.bytes().take(9) {
                    n = n * 10 + u64::from(c - b'0');
                    count += 1;
                }
                while count < 9 {
                    n *= 10;
                    count += 1;
                }
                nanos = n as u32;
            }
            let rest = &b[idx..];
            let offset_secs: i32 = match rest {
                [b'Z'] | [b'z'] => 0,
                [sign @ (b'+' | b'-'), h1, h2, b':', m1, m2] | [sign @ (b'+' | b'-'), h1, h2, m1, m2] => {
                    let all = [*h1, *h2, *m1, *m2];
                    if !all.iter().all(u8::is_ascii_digit) {
                        return None;
                    }
                    let hh = i32::from(h1 - b'0') * 10 + i32::from(h2 - b'0');
                    let mm = i32::from(m1 - b'0') * 10 + i32::from(m2 - b'0');
                    if hh > 23 || mm > 59 {
                        return None;
                    }
                    let total = hh * 3600 + mm * 60;
                    if *sign == b'-' {
                        -total
                    } else {
                        total
                    }
                }
                _ => return None,
            };
            let offset = FixedOffset::east_opt(offset_secs)?;
            let naive = naive.checked_add_signed(chrono::Duration::nanoseconds(i64::from(nanos)))?;
            offset
                .from_local_datetime(&naive)
                .single()
                .map(|d| d.with_timezone(&Utc))
        }
        _ => None,
    }
}

/// JSON 값 하나를 날짜로 해석한다. 문자열은 [`parse_chzzk_date`], 숫자는 Swift 기본
/// `JSONEncoder` 형식(2001 기준 초)으로 본다.
pub fn date_from_value(v: &Value) -> Option<DateTime<Utc>> {
    match v {
        Value::String(s) => parse_chzzk_date(s),
        Value::Number(n) => {
            let secs = n.as_f64()? + SWIFT_REFERENCE_DATE_OFFSET;
            let whole = secs.floor();
            let nanos = ((secs - whole) * 1e9).round() as u32;
            DateTime::from_timestamp(whole as i64, nanos.min(999_999_999))
        }
        _ => None,
    }
}

/// 날짜를 RFC3339(UTC, `Z`) 문자열로 쓴다. [`parse_chzzk_date`] 로 왕복된다.
pub fn format_date(d: &DateTime<Utc>) -> String {
    d.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)
}

/// `#[serde(with = "chzzk_date")]` — 필수 날짜 필드.
pub mod chzzk_date {
    use super::*;

    pub fn serialize<S: Serializer>(d: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format_date(d))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<DateTime<Utc>, D::Error> {
        let v = Value::deserialize(d)?;
        date_from_value(&v).ok_or_else(|| D::Error::custom(format!("Cannot decode date: {v}")))
    }
}

/// `#[serde(with = "chzzk_date_opt", default)]` — 선택 날짜 필드. `null` 은 `None`,
/// 해석할 수 없는 값은 오류(Swift `decodeIfPresent` 와 같은 엄격도).
pub mod chzzk_date_opt {
    use super::*;

    pub fn serialize<S: Serializer>(d: &Option<DateTime<Utc>>, s: S) -> Result<S::Ok, S::Error> {
        match d {
            Some(d) => s.serialize_str(&format_date(d)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<DateTime<Utc>>, D::Error> {
        let v = Value::deserialize(d)?;
        if v.is_null() {
            return Ok(None);
        }
        date_from_value(&v)
            .map(Some)
            .ok_or_else(|| D::Error::custom(format!("Cannot decode date: {v}")))
    }
}

/// 원소 단위 격리 디코딩 결과 (Swift `SafeDecoding.ArrayDecodingResult`).
#[derive(Debug, Clone, PartialEq)]
pub struct ArrayDecodingResult<T> {
    pub values: Vec<T>,
    pub failure_count: usize,
}

/// 관대한 디코딩용 JSON 객체 래퍼. Swift `KeyedDecodingContainer` 의 `try?` 패턴을 재현한다.
#[derive(Debug, Clone, Default)]
pub struct LenientObject(pub Map<String, Value>);

impl LenientObject {
    /// 객체가 아니면 실패한다 — Swift `decoder.container(keyedBy:)` 와 같다.
    pub fn from_deserializer<'de, D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Map::<String, Value>::deserialize(d).map(LenientObject)
    }

    pub fn raw(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    /// 키가 있는가(값이 `null` 이어도 참) — Swift `container.contains`.
    pub fn contains(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    /// 키가 있고 값이 `null` 인가 — Swift `decodeNil(forKey:) == true`.
    pub fn is_null(&self, key: &str) -> bool {
        matches!(self.0.get(key), Some(Value::Null))
    }

    /// `try? container.decode(T.self, forKey:)` — 없거나 `null` 이거나 형식이 어긋나면 `None`.
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let v = self.0.get(key)?;
        if v.is_null() {
            return None;
        }
        T::deserialize(v).ok()
    }

    /// `try container.decode(T.self, forKey:)` — 필수 필드.
    pub fn required<T: DeserializeOwned, E: serde::de::Error>(&self, key: &str) -> Result<T, E> {
        let v = self
            .0
            .get(key)
            .ok_or_else(|| E::custom(format!("키 없음: '{key}'")))?;
        T::deserialize(v).map_err(|e| E::custom(format!("'{key}': {e}")))
    }

    /// `try container.decodeIfPresent(T.self, forKey:)` — 없거나 `null` 이면 `None`,
    /// 있는데 형식이 어긋나면 오류.
    pub fn optional<T: DeserializeOwned, E: serde::de::Error>(&self, key: &str) -> Result<Option<T>, E> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => T::deserialize(v)
                .map(Some)
                .map_err(|e| E::custom(format!("'{key}': {e}"))),
        }
    }

    /// 관대한 날짜 (`try? decode(Date.self)`).
    pub fn date(&self, key: &str) -> Option<DateTime<Utc>> {
        self.0.get(key).and_then(date_from_value)
    }

    /// 엄격한 선택 날짜 (`decodeIfPresent(Date.self)`).
    pub fn optional_date<E: serde::de::Error>(&self, key: &str) -> Result<Option<DateTime<Utc>>, E> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => date_from_value(v)
                .map(Some)
                .ok_or_else(|| E::custom(format!("Cannot decode date: {v}"))),
        }
    }

    /// 관대한 URL (`try? decode(URL.self)`).
    pub fn url(&self, key: &str) -> Option<Url> {
        self.get::<String>(key).and_then(|s| Url::parse(&s).ok())
    }

    /// `SafeDecoding.decodeArrayWithReport` — 유효한 원소는 보존하고 건너뛴 수를 함께 돌려준다.
    /// 키 누락/`null`/비배열처럼 원소 수를 알 수 없는 실패는 최소 1건 실패로 보고한다.
    pub fn array_report<T: DeserializeOwned>(&self, key: &str) -> ArrayDecodingResult<T> {
        let Some(Value::Array(items)) = self.0.get(key) else {
            tracing::warn!("SafeDecoding: '{key}' 배열 컨테이너 없음, 빈 배열 반환");
            return ArrayDecodingResult { values: Vec::new(), failure_count: 1 };
        };
        let mut values = Vec::with_capacity(items.len());
        let mut failure_count = 0;
        for item in items {
            match T::deserialize(item) {
                Ok(v) => values.push(v),
                Err(_) => failure_count += 1,
            }
        }
        if failure_count > 0 {
            tracing::warn!(
                "SafeDecoding: '{key}' 배열 {failure_count}개 요소 디코딩 실패, {}개 성공",
                values.len()
            );
        }
        ArrayDecodingResult { values, failure_count }
    }

    /// `SafeDecoding.decodeArray` — 실패 수 없이 유효 원소만.
    pub fn array<T: DeserializeOwned>(&self, key: &str) -> Vec<T> {
        self.array_report(key).values
    }
}

#[cfg(test)]
mod tests {
    //! `Tests/CViewNetworkingTests/ChzzkDateParsingTests.swift` 의 동등성 검사를 옮겼다
    //! (참조 formatter 체인 대신 기대값을 직접 고정).
    use super::*;

    fn local(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Local
            .with_ymd_and_hms(y, mo, d, h, mi, s)
            .earliest()
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn legacy_format_is_local_time() {
        assert_eq!(parse_chzzk_date("2024-06-15 18:30:00"), Some(local(2024, 6, 15, 18, 30, 0)));
        assert_eq!(parse_chzzk_date("2024-02-29 12:00:00"), Some(local(2024, 2, 29, 12, 0, 0)));
        assert_eq!(parse_chzzk_date("2026-12-31 23:59:59"), Some(local(2026, 12, 31, 23, 59, 59)));
    }

    #[test]
    fn iso8601_offsets_and_fractions() {
        let base = Utc.with_ymd_and_hms(2026, 8, 19, 11, 58, 13).unwrap();
        assert_eq!(parse_chzzk_date("2026-08-19T11:58:13Z"), Some(base));
        assert_eq!(
            parse_chzzk_date("2026-08-19T11:58:13.5Z"),
            Some(base + chrono::Duration::milliseconds(500))
        );
        assert_eq!(
            parse_chzzk_date("2026-08-19T11:58:13.999Z"),
            Some(base + chrono::Duration::milliseconds(999))
        );
        assert_eq!(
            parse_chzzk_date("2026-08-19T20:58:13+09:00"),
            Some(base)
        );
        assert_eq!(
            parse_chzzk_date("2026-08-19T06:58:13-05:00"),
            Some(base)
        );
        assert_eq!(
            parse_chzzk_date("2026-08-19T20:58:13.250+09:00"),
            Some(base + chrono::Duration::milliseconds(250))
        );
    }

    #[test]
    fn malformed_inputs_are_rejected() {
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
        ] {
            assert_eq!(parse_chzzk_date(s), None, "{s}");
        }
    }

    #[test]
    fn impossible_calendar_dates_and_leap_seconds_are_rejected() {
        for s in [
            "2023-02-29 12:00:00",
            "2024-02-30 12:00:00",
            "2024-04-31 18:30:00",
            "2026-06-31 00:00:00",
            "2026-09-31 23:59:59",
            "1900-02-29 00:00:00",
            "2023-02-29T12:00:00Z",
            "2024-04-31T18:30:00+09:00",
            "2024-06-15 18:30:60",
            "2024-06-15T18:30:60Z",
            "2016-12-31T23:59:60Z",
        ] {
            assert_eq!(parse_chzzk_date(s), None, "{s}");
        }
        assert!(parse_chzzk_date("2000-02-29 12:00:00").is_some());
        assert!(parse_chzzk_date("2024-02-29 12:00:00").is_some());
    }

    #[test]
    fn format_round_trips() {
        let d = Utc.with_ymd_and_hms(2026, 8, 19, 11, 58, 13).unwrap() + chrono::Duration::milliseconds(123);
        assert_eq!(parse_chzzk_date(&format_date(&d)), Some(d));
    }

    #[test]
    fn swift_reference_seconds() {
        // 0 = 2001-01-01T00:00:00Z
        let d = date_from_value(&serde_json::json!(0)).unwrap();
        assert_eq!(d, Utc.with_ymd_and_hms(2001, 1, 1, 0, 0, 0).unwrap());
    }

    #[test]
    fn array_report_isolates_bad_elements() {
        let obj: LenientObject =
            serde_json::from_str::<Map<String, Value>>(r#"{"a":[1,"x",3],"b":null}"#).map(LenientObject).unwrap();
        let r = obj.array_report::<i64>("a");
        assert_eq!(r.values, vec![1, 3]);
        assert_eq!(r.failure_count, 1);
        let missing = obj.array_report::<i64>("b");
        assert!(missing.values.is_empty());
        assert_eq!(missing.failure_count, 1);
    }
}
