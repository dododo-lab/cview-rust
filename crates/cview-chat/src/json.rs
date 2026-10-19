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

/// `JSONSerialization.data(withJSONObject:options:[.sortedKeys])` 와 같은 문자열.
pub fn to_foundation_json(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

fn write_value(out: &mut String, value: &Value) {
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

fn write_object(out: &mut String, map: &Map<String, Value>) {
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

fn write_number(out: &mut String, n: &serde_json::Number) {
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

fn write_string(out: &mut String, s: &str) {
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

fn numeric_case_insensitive_cmp(a: &str, b: &str) -> Ordering {
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
    fn fnv1a_reference_values() {
        assert_eq!(fnv1a(""), 0xcbf29ce484222325);
        assert_eq!(fnv1a("a"), 0xaf63dc4c8601ec8c);
    }
}
