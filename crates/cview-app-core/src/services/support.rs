//! 앱 계층 공용 기반 — Swift `UserDefaults`·`Date()`·`ProcessInfo.systemUptime` 대응.
//!
//! 원본에는 대응 파일이 없다(Foundation 의 전역 서비스). Swift 쪽이 `UserDefaults.standard`
//! 와 `Date()` 를 직접 부르던 자리를 모두 이 모듈의 주입 가능한 값으로 바꿨다 — 테스트는
//! [`MemoryKeyValueStore`] 와 [`ManualClock`] 을 넣는다.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use chrono::Utc;
use cview_core::utils::time::{adding, Date, TimeInterval};
use serde_json::Value;

/// 독(poison)을 무시하고 잠근다 — 패닉한 쓰기 뒤에도 상태는 값 타입이라 계속 쓸 수 있다.
pub fn lock<T: ?Sized>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// MARK: - 시계

/// 벽시계(`Date()`) 공급자.
pub type Clock = Arc<dyn Fn() -> Date + Send + Sync>;
/// 단조 시계(`ProcessInfo.processInfo.systemUptime`, 초) 공급자.
pub type Uptime = Arc<dyn Fn() -> TimeInterval + Send + Sync>;

/// 시스템 벽시계.
pub fn system_clock() -> Clock {
    Arc::new(Utc::now)
}

/// 프로세스 기준 단조 시계(초).
pub fn system_uptime() -> Uptime {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let start = *START.get_or_init(Instant::now);
    Arc::new(move || start.elapsed().as_secs_f64())
}

/// 테스트용 수동 시계 — 벽시계와 단조 시계를 함께 움직인다.
#[derive(Clone)]
pub struct ManualClock {
    inner: Arc<Mutex<(Date, TimeInterval)>>,
}

impl ManualClock {
    pub fn new(start: Date) -> Self {
        Self {
            inner: Arc::new(Mutex::new((start, 1_000.0))),
        }
    }

    pub fn now(&self) -> Date {
        lock(&self.inner).0
    }

    pub fn uptime(&self) -> TimeInterval {
        lock(&self.inner).1
    }

    pub fn advance(&self, secs: TimeInterval) {
        let mut g = lock(&self.inner);
        g.0 = adding(&g.0, secs);
        g.1 += secs;
    }

    pub fn set(&self, date: Date) {
        lock(&self.inner).0 = date;
    }

    pub fn clock(&self) -> Clock {
        let me = self.clone();
        Arc::new(move || me.now())
    }

    pub fn uptime_fn(&self) -> Uptime {
        let me = self.clone();
        Arc::new(move || me.uptime())
    }
}

// MARK: - 키-값 저장소

/// `UserDefaults` 대응. 값은 JSON 이다(문자열·불리언·수·배열).
pub trait KeyValueStore: Send + Sync {
    fn value(&self, key: &str) -> Option<Value>;
    fn set_value(&self, key: &str, value: Value);
    fn remove(&self, key: &str);

    fn string(&self, key: &str) -> Option<String> {
        match self.value(key)? {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
    fn set_string(&self, key: &str, value: &str) {
        self.set_value(key, Value::String(value.to_string()));
    }
    /// `bool(forKey:)` — 없거나 다른 타입이면 false.
    fn bool(&self, key: &str) -> bool {
        match self.value(key) {
            Some(Value::Bool(b)) => b,
            Some(Value::Number(n)) => n.as_f64().is_some_and(|v| v != 0.0),
            _ => false,
        }
    }
    fn set_bool(&self, key: &str, value: bool) {
        self.set_value(key, Value::Bool(value));
    }
    /// `object(forKey:) != nil`.
    fn contains(&self, key: &str) -> bool {
        self.value(key).is_some()
    }
    fn i64(&self, key: &str) -> Option<i64> {
        self.value(key)?.as_i64()
    }
    fn set_i64(&self, key: &str, value: i64) {
        self.set_value(key, Value::from(value));
    }
    fn f64(&self, key: &str) -> Option<f64> {
        self.value(key)?.as_f64()
    }
    fn set_f64(&self, key: &str, value: f64) {
        self.set_value(key, Value::from(value));
    }
    fn strings(&self, key: &str) -> Option<Vec<String>> {
        match self.value(key)? {
            Value::Array(items) => Some(
                items
                    .into_iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect(),
            ),
            _ => None,
        }
    }
    fn set_strings(&self, key: &str, values: &[String]) {
        self.set_value(
            key,
            Value::Array(values.iter().cloned().map(Value::String).collect()),
        );
    }
}

/// 메모리 저장소(테스트·미리보기).
#[derive(Debug, Default)]
pub struct MemoryKeyValueStore {
    values: Mutex<HashMap<String, Value>>,
}

impl MemoryKeyValueStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn shared() -> Arc<dyn KeyValueStore> {
        Arc::new(Self::new())
    }

    pub fn keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = lock(&self.values).keys().cloned().collect();
        keys.sort();
        keys
    }
}

impl KeyValueStore for MemoryKeyValueStore {
    fn value(&self, key: &str) -> Option<Value> {
        lock(&self.values).get(key).cloned()
    }
    fn set_value(&self, key: &str, value: Value) {
        lock(&self.values).insert(key.to_string(), value);
    }
    fn remove(&self, key: &str) {
        lock(&self.values).remove(key);
    }
}

/// JSON 파일 하나에 통째로 저장하는 저장소 — 쓰기마다 원자적으로 다시 쓴다.
#[derive(Debug)]
pub struct JsonFileKeyValueStore {
    path: PathBuf,
    values: Mutex<HashMap<String, Value>>,
}

impl JsonFileKeyValueStore {
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let values = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<HashMap<String, Value>>(&bytes).ok())
            .unwrap_or_default();
        Self {
            path,
            values: Mutex::new(values),
        }
    }

    fn flush(&self, values: &HashMap<String, Value>) {
        if let Ok(bytes) = serde_json::to_vec_pretty(values) {
            let _ = write_atomic(&self.path, &bytes);
        }
    }
}

impl KeyValueStore for JsonFileKeyValueStore {
    fn value(&self, key: &str) -> Option<Value> {
        lock(&self.values).get(key).cloned()
    }
    fn set_value(&self, key: &str, value: Value) {
        let mut g = lock(&self.values);
        g.insert(key.to_string(), value);
        self.flush(&g);
    }
    fn remove(&self, key: &str) {
        let mut g = lock(&self.values);
        if g.remove(key).is_some() {
            self.flush(&g);
        }
    }
}

/// `Data.write(to:options: .atomic)` 대응 — 임시 파일에 쓰고 이름을 바꾼다.
pub fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".tmp-{}", uuid::Uuid::new_v4().simple()));
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

// MARK: - 문자열 비교

/// Foundation `localizedStandardCompare` 근사 — 대소문자 무시 + 숫자 구간을 수로 비교한다.
///
/// Finder 정렬 규칙(숫자 인식, 대소문자 무시)을 따른다. 로캘 콜레이션은 코드 포인트 순서로
/// 대신한다 — 한글 음절은 코드 포인트가 가나다순이라 결과가 같다.
pub fn localized_standard_compare(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ca), Some(cb)) => {
                if ca.is_ascii_digit() && cb.is_ascii_digit() {
                    let mut na = String::new();
                    while let Some(c) = ai.peek().copied().filter(char::is_ascii_digit) {
                        na.push(c);
                        ai.next();
                    }
                    let mut nb = String::new();
                    while let Some(c) = bi.peek().copied().filter(char::is_ascii_digit) {
                        nb.push(c);
                        bi.next();
                    }
                    let ta = na.trim_start_matches('0');
                    let tb = nb.trim_start_matches('0');
                    let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                    if ord != Ordering::Equal {
                        return ord;
                    }
                    continue;
                }
                let la: String = ca.to_lowercase().collect();
                let lb: String = cb.to_lowercase().collect();
                let ord = la.cmp(&lb);
                if ord != Ordering::Equal {
                    return ord;
                }
                ai.next();
                bi.next();
            }
        }
    }
}

/// Swift `trimmingCharacters(in: .whitespaces)` — 개행은 남긴다.
pub fn trim_spaces(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() && c != '\n' && c != '\r')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn natural_compare_orders_numbers_and_case() {
        assert_eq!(localized_standard_compare("a2", "a10"), Ordering::Less);
        assert_eq!(localized_standard_compare("Abc", "abd"), Ordering::Less);
        assert_eq!(localized_standard_compare("abc", "ABC"), Ordering::Equal);
        assert_eq!(localized_standard_compare("가나", "가다"), Ordering::Less);
    }

    #[test]
    fn memory_store_round_trip() {
        let s = MemoryKeyValueStore::new();
        s.set_string("k", "v");
        assert_eq!(s.string("k").as_deref(), Some("v"));
        assert!(!s.bool("missing"));
        s.set_bool("b", true);
        assert!(s.bool("b"));
        s.remove("k");
        assert!(s.string("k").is_none());
    }

    #[test]
    fn file_store_persists() {
        let dir = std::env::temp_dir().join(format!("cview-kv-{}", uuid::Uuid::new_v4()));
        let path = dir.join("defaults.json");
        {
            let s = JsonFileKeyValueStore::open(&path);
            s.set_i64("n", 7);
        }
        let s = JsonFileKeyValueStore::open(&path);
        assert_eq!(s.i64("n"), Some(7));
        let _ = std::fs::remove_dir_all(dir);
    }
}
