//! Foundation `JSONSerialization` 호환 보조 함수.
//!
//! Swift 원본(`ChatMessageParser*.swift`)은 `JSONSerialization` 으로 송신 프레임을 만들고
//! (`options: [.sortedKeys]`), 수신 값을 `as? Int` / `as? Bool` / `as? String` 로 꺼낸다.
//! 와이어 바이트를 같게 하려고 다음 성질을 그대로 옮겼다.
//!
//! - **키 정렬**: `.sortedKeys` 는 `compare(_:options: [.numeric, .caseInsensitive, .forcedOrdering])`
//!   순서다 (대소문자 무시, 숫자 런은 값으로 비교, 동률이면 바이트 순).
//! - **문자열 이스케이프**: `/` 를 `\/` 로 쓴다(`.withoutEscapingSlashes` 미사용).
//!   제어문자는 `\b \f \n \r \t` 또는 `\u00XX`, 그 외 비 ASCII 는 UTF-8 그대로.
//! - **숫자**: 정수는 그대로, 정수값 실수는 소수점 없이.
//!
//! 수신 값 강제 변환(`ns_*`)은 `NSNumber` 브리징 규칙을 따른다 — 예를 들어 `true as? Int == 1`,
//! `2.0 as? Int == 2`, `2.5 as? Int == nil`.

use std::cmp::Ordering;

use serde_json::{Map, Value};

/// JSON 글자를 받는 곳 — 문자열에 모으거나([`String`]) 모으지 않고 바로 해시한다([`Fnv1a`]).
pub trait JsonSink {
    fn push(&mut self, c: char);
    fn push_str(&mut self, s: &str);
}

impl JsonSink for String {
    fn push(&mut self, c: char) {
        String::push(self, c);
    }
    fn push_str(&mut self, s: &str) {
        String::push_str(self, s);
    }
}

/// 스트리밍 FNV-1a 64 — 같은 바이트를 넣으면 [`fnv1a`] 와 같은 값.
#[derive(Debug, Clone, Copy)]
pub struct Fnv1a(u64);

impl Default for Fnv1a {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv1a {
    pub fn finish(self) -> u64 {
        self.0
    }
    fn bytes(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= u64::from(*b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

impl JsonSink for Fnv1a {
    fn push(&mut self, c: char) {
        self.bytes(c.encode_utf8(&mut [0; 4]).as_bytes());
    }
    fn push_str(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
}

/// [`to_foundation_json`] 과 같은 글자를 `out` 에 쓴다.
pub fn write_foundation_json(out: &mut impl JsonSink, value: &Value) {
    write_value(out, value);
}

/// [`map_to_foundation_json`] 과 같은 글자를 `out` 에 쓴다.
pub fn write_foundation_object(out: &mut impl JsonSink, map: &Map<String, Value>) {
    write_object(out, map);
}

/// `JSONSerialization.data(withJSONObject:options:[.sortedKeys])` 와 같은 문자열.
pub fn to_foundation_json(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

/// [`to_foundation_json`] 의 객체판 — 맵을 `Value` 로 감싸려고 통째로 복제하지 않는다.
pub fn map_to_foundation_json(map: &Map<String, Value>) -> String {
    let mut out = String::new();
    write_object(&mut out, map);
    out
}

fn write_value(out: &mut impl JsonSink, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => write_number(out, n),
        Value::String(s) => write_string(out, s),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => write_object(out, map),
    }
}

fn write_object(out: &mut impl JsonSink, map: &Map<String, Value>) {
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort_by(|a, b| foundation_key_cmp(a, b));
    out.push('{');
    for (i, key) in keys.into_iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        write_string(out, key);
        out.push(':');
        write_value(out, &map[key]);
    }
    out.push('}');
}

fn write_number(out: &mut impl JsonSink, n: &serde_json::Number) {
    if let Some(i) = n.as_i64() {
        out.push_str(&i.to_string());
    } else if let Some(u) = n.as_u64() {
        out.push_str(&u.to_string());
    } else if let Some(f) = n.as_f64() {
        if f.fract() == 0.0 && f.abs() < 1e15 {
            out.push_str(&format!("{}", f as i64));
        } else {
            out.push_str(&format!("{f}"));
        }
    }
}

fn write_string(out: &mut impl JsonSink, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '/' => out.push_str("\\/"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// `.sortedKeys` 정렬 순서 (`[.numeric, .caseInsensitive, .forcedOrdering]`).
pub fn foundation_key_cmp(a: &str, b: &str) -> Ordering {
    let primary = numeric_case_insensitive_cmp(a, b);
    if primary != Ordering::Equal {
        return primary;
    }
    // forcedOrdering: 동률이면 리터럴 비교로 결정.
    a.cmp(b)
}

/// 대소문자 무시 + 숫자 런은 값으로 비교. 할당하지 않는다(키 비교마다 글자 배열·글자당 문자열을 만들던 것을 없앰).
/// 숫자는 ASCII 숫자만 런으로 보므로 바이트 위치로 잘라도 글자 경계가 맞다.
fn numeric_case_insensitive_cmp(a: &str, b: &str) -> Ordering {
    fn trim_zeros(d: &[u8]) -> &[u8] {
        let n = d.iter().take_while(|&&c| c == b'0').count();
        &d[n..]
    }
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < ab.len() && j < bb.len() {
        if ab[i].is_ascii_digit() && bb[j].is_ascii_digit() {
            let si = i;
            while i < ab.len() && ab[i].is_ascii_digit() {
                i += 1;
            }
            let sj = j;
            while j < bb.len() && bb[j].is_ascii_digit() {
                j += 1;
            }
            let (na, nb) = (trim_zeros(&ab[si..i]), trim_zeros(&bb[sj..j]));
            let ord = na.len().cmp(&nb.len()).then_with(|| na.cmp(nb));
            if ord != Ordering::Equal {
                return ord;
            }
            continue;
        }
        let (Some(ca), Some(cb)) = (a[i..].chars().next(), b[j..].chars().next()) else {
            break;
        };
        // 소문자로 바꾼 글자열끼리 비교 — 글자(코드 포인트) 순서는 UTF-8 바이트 순서와 같다.
        let ord = if ca.is_ascii() && cb.is_ascii() {
            ca.to_ascii_lowercase().cmp(&cb.to_ascii_lowercase())
        } else {
            ca.to_lowercase().cmp(cb.to_lowercase())
        };
        if ord != Ordering::Equal {
            return ord;
        }
        i += ca.len_utf8();
        j += cb.len_utf8();
    }
    // 루프가 끝나면 적어도 한쪽은 다 읽었다 — 남은 쪽이 뒤.
    (ab.len() - i).cmp(&(bb.len() - j))
}

// MARK: - NSNumber 브리징 규칙의 값 꺼내기

/// `value as? String`.
pub fn ns_str(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str)
}

/// `value as? Int` (NSNumber 브리징): 정수, 정수값 실수, Bool(0/1).
pub fn ns_int(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Bool(b) => Some(i64::from(*b)),
        Value::Number(n) => number_exact_i64(n),
        _ => None,
    }
}

/// `value as? Bool` (NSNumber 브리징): Bool, 또는 정확히 0/1 인 숫자.
pub fn ns_bool(value: Option<&Value>) -> Option<bool> {
    match value? {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => match number_exact_i64(n) {
            Some(0) => Some(false),
            Some(1) => Some(true),
            _ => None,
        },
        _ => None,
    }
}

/// `value as? [String: Any]`.
pub fn ns_object(value: Option<&Value>) -> Option<&Map<String, Value>> {
    value.and_then(Value::as_object)
}

/// `value as? [[String: Any]]` — 모든 원소가 객체일 때만 성공한다.
pub fn ns_object_array(value: Option<&Value>) -> Option<Vec<&Map<String, Value>>> {
    value?.as_array()?.iter().map(Value::as_object).collect()
}

/// `AnyCodable.intValue` — **정수로 디코딩된** 숫자만 (`.double` 은 nil).
pub fn any_int(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(n) => n.as_i64(),
        _ => None,
    }
}

/// `AnyCodable.stringValue`.
pub fn any_str(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str)
}

/// `Int(number.stringValue)` — 정수거나 소수부가 0 이고 범위 안인 실수.
fn number_exact_i64(n: &serde_json::Number) -> Option<i64> {
    if let Some(i) = n.as_i64() {
        return Some(i);
    }
    if n.as_u64().is_some() {
        return None; // i64 범위 초과
    }
    let f = n.as_f64()?;
    if f.is_finite() && f.fract() == 0.0 && f >= i64::MIN as f64 && f < i64::MAX as f64 {
        Some(f as i64)
    } else {
        None
    }
}

/// FNV-1a 64bit — `Sources/CViewCore/Utilities/StableHash.swift` (채팅 id 용 로컬 사본).
pub fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn writer_matches_jsonserialization() {
        let v = json!({"b": 1, "a": "x/y", "C": [true, null, 2.0, 1.5], "d": "\"\\\n\u{1}"});
        assert_eq!(
            to_foundation_json(&v),
            r#"{"a":"x\/y","b":1,"C":[true,null,2,1.5],"d":"\"\\\n\u0001"}"#
        );
        assert_eq!(to_foundation_json(&json!("한글 🎉")), "\"한글 🎉\"");
    }

    #[test]
    fn key_order_is_numeric_and_case_insensitive() {
        let mut keys = vec!["d_10", "d_9", "B", "a", "msgTypeCode", "msgTime", "msg"];
        keys.sort_by(|a, b| foundation_key_cmp(a, b));
        assert_eq!(
            keys,
            vec!["a", "B", "d_9", "d_10", "msg", "msgTime", "msgTypeCode"]
        );
    }

    #[test]
    fn nsnumber_bridging() {
        assert_eq!(ns_int(Some(&json!(2.0))), Some(2));
        assert_eq!(ns_int(Some(&json!(2.5))), None);
        assert_eq!(ns_int(Some(&json!(true))), Some(1));
        assert_eq!(ns_bool(Some(&json!(1))), Some(true));
        assert_eq!(ns_bool(Some(&json!(2))), None);
        assert_eq!(any_int(Some(&json!(2.0))), None);
        assert_eq!(any_int(Some(&json!(7))), Some(7));
        assert!(ns_object_array(Some(&json!([{}, 1]))).is_none());
        assert_eq!(ns_object_array(Some(&json!([{}, {}]))).unwrap().len(), 2);
    }

    #[test]
    fn streaming_fnv_matches_string_fnv() {
        let v = json!({"b": [1, 2.0, "x/y"], "A10": {"z": null, "a2": true}, "a9": "한글\n"});
        let text = to_foundation_json(&v);
        let mut h = Fnv1a::default();
        write_foundation_json(&mut h, &v);
        assert_eq!(h.finish(), fnv1a(&text));
    }

    /// 바꾸기 전 구현(글자 배열·글자당 String) — 새 구현과 순서가 같은지 대조한다.
    fn reference_cmp(a: &str, b: &str) -> Ordering {
        let a: Vec<char> = a.chars().collect();
        let b: Vec<char> = b.chars().collect();
        let (mut i, mut j) = (0, 0);
        while i < a.len() && j < b.len() {
            if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
                let si = i;
                while i < a.len() && a[i].is_ascii_digit() {
                    i += 1;
                }
                let sj = j;
                while j < b.len() && b[j].is_ascii_digit() {
                    j += 1;
                }
                let na: String = a[si..i]
                    .iter()
                    .collect::<String>()
                    .trim_start_matches('0')
                    .to_string();
                let nb: String = b[sj..j]
                    .iter()
                    .collect::<String>()
                    .trim_start_matches('0')
                    .to_string();
                let ord = na.len().cmp(&nb.len()).then_with(|| na.cmp(&nb));
                if ord != Ordering::Equal {
                    return ord;
                }
                continue;
            }
            let ca: String = a[i].to_lowercase().collect();
            let cb: String = b[j].to_lowercase().collect();
            let ord = ca.cmp(&cb);
            if ord != Ordering::Equal {
                return ord;
            }
            i += 1;
            j += 1;
        }
        (a.len() - i).cmp(&(b.len() - j))
    }

    #[test]
    fn zero_alloc_cmp_matches_reference() {
        let words = [
            "",
            "a",
            "A",
            "b",
            "a1",
            "a01",
            "a001b",
            "a10",
            "a2",
            "a2b",
            "A2B",
            "z",
            "Z9",
            "Ǆ",
            "ǆ",
            "İx",
            "ix",
            "é",
            "E",
            "ß",
            "ss",
            "가",
            "가1",
            "가01",
            "1",
            "01",
            "007",
            "7",
            "10a",
            "chatType",
            "chattype",
            "osType",
            "emojis",
            "extraToken",
            "x1y2",
            "x1y10",
            "Ω",
            "ω",
        ];
        for a in words {
            for b in words {
                assert_eq!(
                    numeric_case_insensitive_cmp(a, b),
                    reference_cmp(a, b),
                    "{a:?} vs {b:?}"
                );
            }
        }
    }

    #[test]
    fn fnv1a_reference_values() {
        assert_eq!(fnv1a(""), 0xcbf29ce484222325);
        assert_eq!(fnv1a("a"), 0xaf63dc4c8601ec8c);
    }
}
