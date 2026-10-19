//! `Sources/CViewNetworking/ResponseCache.swift` 포팅 — 응답 캐시.
//!
//! Swift actor 대신 `std::sync::Mutex` 로 직렬화한다(락은 await 를 넘지 않는다).
//! 값은 타입 소거(`Arc<dyn Any>`)로 보관하고 조회 시 타입이 맞을 때만 돌려준다.

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use chrono::{DateTime, Duration as ChronoDuration, Utc};

use crate::constants::response_cache_defaults;

/// 캐시가 쓰는 시계. 테스트는 가짜 시계를 넣어 기한을 잠 없이 넘긴다.
pub type Clock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;

/// 조회 결과. 값과 함께 그 값을 저장한 시각(`fetched_at`)을 돌려준다.
#[derive(Debug, Clone, PartialEq)]
pub enum Lookup<T> {
    Fresh { value: T, fetched_at: DateTime<Utc> },
    Stale { value: T, fetched_at: DateTime<Utc> },
    Miss,
}

impl<T> Lookup<T> {
    pub fn value(&self) -> Option<&T> {
        match self {
            Lookup::Fresh { value, .. } | Lookup::Stale { value, .. } => Some(value),
            Lookup::Miss => None,
        }
    }

    pub fn fetched_at(&self) -> Option<DateTime<Utc>> {
        match self {
            Lookup::Fresh { fetched_at, .. } | Lookup::Stale { fetched_at, .. } => {
                Some(*fetched_at)
            }
            Lookup::Miss => None,
        }
    }
}

/// 계측용 수치.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheStats {
    pub entries: usize,
    pub stored_bytes: usize,
    pub evictions: usize,
}

struct CacheEntry {
    data: Arc<dyn Any + Send + Sync>,
    /// 최초 저장 시각 — TTL 계산용 (불변)
    timestamp: DateTime<Utc>,
    /// 이 시각부터는 stale 로도 쓰지 않는다.
    expires_at: DateTime<Utc>,
    /// 마지막 접근 순번 — LRU 퇴출용
    last_access_order: u64,
    bytes: usize,
}

#[derive(Default)]
struct CacheState {
    storage: HashMap<String, CacheEntry>,
    /// 단조 접근 순번. 같은 tick 의 연속 접근도 결정적으로 정렬된다.
    access_sequence: u64,
    /// 용량·바이트 상한·메모리 압력으로 지운 항목 수(누적). 기한 정리와 clear 는 세지 않는다.
    eviction_count: usize,
}

impl CacheState {
    fn next_access_order(&mut self) -> u64 {
        self.access_sequence = self.access_sequence.wrapping_add(1);
        self.access_sequence
    }

    fn stored_bytes(&self) -> usize {
        self.storage.values().map(|e| e.bytes).sum()
    }

    fn sorted_keys_by_lru(&self) -> Vec<String> {
        let mut entries: Vec<(&String, u64)> = self
            .storage
            .iter()
            .map(|(k, v)| (k, v.last_access_order))
            .collect();
        entries.sort_by_key(|(_, order)| *order);
        entries.into_iter().map(|(k, _)| k.clone()).collect()
    }

    fn evict(&mut self, keys: &[String]) {
        for key in keys {
            self.storage.remove(key);
        }
        self.eviction_count += keys.len();
    }

    /// 가장 오래 미접근된 25% 항목 제거 (true LRU).
    fn evict_oldest_quarter(&mut self) {
        let sorted = self.sorted_keys_by_lru();
        let to_remove = (sorted.len() / 4).max(1);
        self.evict(&sorted[..to_remove.min(sorted.len())]);
    }

    /// 가장 오래 미접근된 `count` 개 제거.
    fn evict_oldest(&mut self, count: usize) {
        let sorted = self.sorted_keys_by_lru();
        let to_remove = count.min(sorted.len()).max(1).min(sorted.len());
        self.evict(&sorted[..to_remove]);
    }

    fn evict_to_budget(&mut self, max_bytes: usize) {
        while self.stored_bytes() > max_bytes && !self.storage.is_empty() {
            self.evict_oldest(1);
        }
    }
}

fn seconds_between(later: DateTime<Utc>, earlier: DateTime<Utc>) -> f64 {
    (later - earlier)
        .num_microseconds()
        .map(|us| us as f64 / 1_000_000.0)
        .unwrap_or(f64::MAX)
}

fn add_seconds(base: DateTime<Utc>, seconds: f64) -> DateTime<Utc> {
    let micros = (seconds.max(0.0) * 1_000_000.0).min(i64::MAX as f64 / 2.0) as i64;
    base.checked_add_signed(ChronoDuration::microseconds(micros))
        .unwrap_or(DateTime::<Utc>::MAX_UTC)
}

/// 응답 캐시 (thread-safe).
pub struct ResponseCache {
    state: Mutex<CacheState>,
    max_entries: usize,
    max_bytes: usize,
    clock: Clock,
}

impl Default for ResponseCache {
    fn default() -> Self {
        Self::new(response_cache_defaults::MAX_ENTRIES)
    }
}

impl std::fmt::Debug for ResponseCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResponseCache")
            .field("stats", &self.stats())
            .finish()
    }
}

impl ResponseCache {
    pub fn new(max_entries: usize) -> Self {
        Self::with_clock(max_entries, Arc::new(Utc::now))
    }

    pub fn with_clock(max_entries: usize, clock: Clock) -> Self {
        Self {
            state: Mutex::new(CacheState::default()),
            max_entries,
            max_bytes: response_cache_defaults::MAX_BYTES,
            clock,
        }
    }

    /// 지금 시각(캐시 시계 기준).
    pub fn now(&self) -> DateTime<Utc> {
        (self.clock)()
    }

    fn lock(&self) -> MutexGuard<'_, CacheState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// TTL 안이면 값을 돌려준다. 접근 시 LRU 순번을 갱신한다.
    pub fn get<T: Clone + 'static>(&self, key: &str, ttl: f64) -> Option<T> {
        let now = self.now();
        let mut st = self.lock();
        let order = st.next_access_order();
        let entry = st.storage.get_mut(key)?;
        if now >= entry.expires_at || seconds_between(now, entry.timestamp) >= ttl {
            return None;
        }
        let value = entry.data.downcast_ref::<T>()?.clone();
        entry.last_access_order = order;
        Some(value)
    }

    /// fresh 안이면 `Fresh`, fresh 를 지났지만 stale 안이면 `Stale`.
    /// 저장 때 정한 기한(`expires_at`)을 지난 항목은 stale 창이 더 넓어도 `Miss` 다.
    pub fn lookup<T: Clone + 'static>(
        &self,
        key: &str,
        fresh: f64,
        stale: f64,
        now: Option<DateTime<Utc>>,
    ) -> Lookup<T> {
        let now = now.unwrap_or_else(|| self.now());
        let mut st = self.lock();
        let order = st.next_access_order();
        let Some(entry) = st.storage.get_mut(key) else {
            return Lookup::Miss;
        };
        let Some(value) = entry.data.downcast_ref::<T>() else {
            return Lookup::Miss;
        };
        let age = seconds_between(now, entry.timestamp);
        if age >= stale || now >= entry.expires_at {
            return Lookup::Miss;
        }
        let value = value.clone();
        entry.last_access_order = order;
        let fetched_at = entry.timestamp;
        if age < fresh {
            Lookup::Fresh { value, fetched_at }
        } else {
            Lookup::Stale { value, fetched_at }
        }
    }

    /// 캐시에 저장하고 저장 시각을 돌려준다.
    ///
    /// `stale_for`: 저장 시각부터 이 항목을 쓸 수 있는 전체 기한(초). 예산보다 큰 응답은 보관하지 않고
    /// 같은 키의 이전 응답도 지운다.
    pub fn set<T: Send + Sync + 'static>(
        &self,
        key: &str,
        value: T,
        bytes: usize,
        stale_for: f64,
    ) -> DateTime<Utc> {
        self.set_arc(key, Arc::new(value), bytes, stale_for)
    }

    pub(crate) fn set_arc(
        &self,
        key: &str,
        value: Arc<dyn Any + Send + Sync>,
        bytes: usize,
        stale_for: f64,
    ) -> DateTime<Utc> {
        let now = self.now();
        let mut st = self.lock();
        if bytes > self.max_bytes {
            st.storage.remove(key);
            return now;
        }
        // 기존 키 갱신은 엔트리 수를 늘리지 않으므로 다른 키를 퇴출하면 안 된다.
        if !st.storage.contains_key(key) && st.storage.len() >= self.max_entries {
            st.evict_oldest_quarter();
        }
        let order = st.next_access_order();
        st.storage.insert(
            key.to_string(),
            CacheEntry {
                data: value,
                timestamp: now,
                expires_at: add_seconds(now, stale_for),
                last_access_order: order,
                bytes: bytes.max(1),
            },
        );
        st.evict_to_budget(self.max_bytes);
        now
    }

    /// 기본 인자(`bytes = 1024`, `stale_for = 300`)로 저장.
    pub fn set_default<T: Send + Sync + 'static>(&self, key: &str, value: T) -> DateTime<Utc> {
        self.set(key, value, 1024, response_cache_defaults::DEFAULT_TTL)
    }

    pub fn remove(&self, key: &str) {
        self.lock().storage.remove(key);
    }

    pub fn clear(&self) {
        self.lock().storage.clear();
    }

    /// 기한이 지난 항목 정리. 항목마다 저장 때 정한 기한으로 지운다.
    pub fn purge_expired(&self, now: Option<DateTime<Utc>>) {
        let now = now.unwrap_or_else(|| self.now());
        self.lock().storage.retain(|_, e| now < e.expires_at);
    }

    /// 메모리 경고. 바이트의 절반이 되도록 오래 안 쓴 항목부터 지운다.
    pub fn respond_to_memory_pressure(&self) {
        let mut st = self.lock();
        let limit = st.stored_bytes() / 2;
        while st.stored_bytes() > limit && !st.storage.is_empty() {
            st.evict_oldest(1);
        }
    }

    pub fn stats(&self) -> CacheStats {
        let st = self.lock();
        CacheStats {
            entries: st.storage.len(),
            stored_bytes: st.stored_bytes(),
            evictions: st.eviction_count,
        }
    }
}

#[cfg(test)]
mod tests {
    //! `ResponseCacheTests.swift` · `ResponseCacheDeadlineTests.swift` 포팅.
    use super::*;
    use std::sync::atomic::{AtomicI64, Ordering};

    fn fake_clock() -> (Clock, Arc<AtomicI64>) {
        let offset = Arc::new(AtomicI64::new(0));
        let base = Utc::now();
        let o = offset.clone();
        (
            Arc::new(move || base + ChronoDuration::milliseconds(o.load(Ordering::SeqCst))),
            offset,
        )
    }

    fn advance(offset: &AtomicI64, seconds: f64) {
        offset.fetch_add((seconds * 1000.0) as i64, Ordering::SeqCst);
    }

    #[test]
    fn set_and_get() {
        let cache = ResponseCache::default();
        cache.set_default("k", "v".to_string());
        assert_eq!(cache.get::<String>("k", 60.0), Some("v".into()));
        assert_eq!(cache.get::<i64>("k", 60.0), None, "타입이 다르면 miss");
        assert_eq!(cache.get::<String>("missing", 60.0), None);
        cache.remove("k");
        assert_eq!(cache.get::<String>("k", 60.0), None);
    }

    #[test]
    fn ttl_expiry_and_lookup_states() {
        let (clock, offset) = fake_clock();
        let cache = ResponseCache::with_clock(10, clock);
        let at = cache.set("k", 1_i64, 10, 600.0);
        assert_eq!(
            cache.lookup::<i64>("k", 60.0, 600.0, None),
            Lookup::Fresh {
                value: 1,
                fetched_at: at
            }
        );
        advance(&offset, 61.0);
        assert_eq!(cache.get::<i64>("k", 60.0), None);
        assert_eq!(
            cache.lookup::<i64>("k", 60.0, 600.0, None),
            Lookup::Stale {
                value: 1,
                fetched_at: at
            }
        );
        advance(&offset, 600.0);
        assert_eq!(
            cache.lookup::<i64>("k", 60.0, 600.0, None),
            Lookup::<i64>::Miss
        );
    }

    #[test]
    fn deadline_caps_wider_stale_window() {
        let (clock, offset) = fake_clock();
        let cache = ResponseCache::with_clock(10, clock);
        cache.set("k", 1_i64, 10, 100.0);
        advance(&offset, 150.0);
        assert_eq!(
            cache.lookup::<i64>("k", 60.0, 3600.0, None),
            Lookup::<i64>::Miss
        );
    }

    #[test]
    fn purge_uses_per_entry_deadline() {
        let (clock, offset) = fake_clock();
        let cache = ResponseCache::with_clock(10, clock);
        cache.set("short", 1_i64, 10, 60.0);
        cache.set("long", 2_i64, 10, 3600.0);
        advance(&offset, 301.0);
        cache.purge_expired(None);
        assert_eq!(cache.stats().entries, 1);
        assert!(matches!(
            cache.lookup::<i64>("long", 60.0, 3600.0, None),
            Lookup::Stale { value: 2, .. }
        ));
        assert_eq!(cache.stats().evictions, 0, "기한 정리는 퇴출로 세지 않는다");
    }

    #[test]
    fn lru_eviction_keeps_recently_used() {
        let cache = ResponseCache::new(4);
        for k in ["a", "b", "c", "d"] {
            cache.set_default(k, 0_i64);
        }
        assert!(cache.get::<i64>("a", 60.0).is_some());
        cache.set_default("e", 0_i64);
        assert_eq!(cache.stats().entries, 4);
        assert!(
            cache.get::<i64>("a", 60.0).is_some(),
            "최근 접근한 a 는 남는다"
        );
        assert!(
            cache.get::<i64>("b", 60.0).is_none(),
            "가장 오래 미접근된 b 가 퇴출"
        );
        assert_eq!(cache.stats().evictions, 1);
    }

    #[test]
    fn updating_existing_key_does_not_evict() {
        let cache = ResponseCache::new(2);
        cache.set_default("a", 1_i64);
        cache.set_default("b", 2_i64);
        cache.set_default("a", 3_i64);
        assert_eq!(cache.stats().entries, 2);
        assert_eq!(cache.get::<i64>("b", 60.0), Some(2));
        assert_eq!(cache.get::<i64>("a", 60.0), Some(3));
    }

    #[test]
    fn byte_budget() {
        let cache = ResponseCache::new(100);
        let max = response_cache_defaults::MAX_BYTES;
        cache.set("old", 1_i64, max / 2, 300.0);
        cache.set("mid", 2_i64, max / 2, 300.0);
        cache.set("new", 3_i64, max / 2, 300.0);
        assert!(cache.stats().stored_bytes <= max);
        assert!(cache.get::<i64>("old", 60.0).is_none());
        assert!(cache.get::<i64>("new", 60.0).is_some());
        // 예산보다 큰 응답은 저장하지 않고 같은 키의 이전 값도 지운다.
        cache.set("new", 4_i64, max + 1, 300.0);
        assert!(cache.get::<i64>("new", 60.0).is_none());
    }

    #[test]
    fn memory_pressure_halves_bytes() {
        let cache = ResponseCache::new(100);
        for i in 0..4 {
            cache.set(&format!("k{i}"), i, 100, 300.0);
        }
        cache.respond_to_memory_pressure();
        let stats = cache.stats();
        assert_eq!(stats.stored_bytes, 200);
        assert_eq!(stats.evictions, 2);
        assert!(cache.get::<i32>("k3", 60.0).is_some());
        cache.clear();
        assert_eq!(cache.stats().entries, 0);
    }
}
