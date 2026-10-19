//! `Sources/CViewApp/Caching/WorkspaceValueCache.swift` 포팅.
//!
//! 작은 LRU. 값은 관측 UI 상태로 발행되지 않는다. 항목 상한은 명시적이고 신선도·
//! 큰 값 수용 여부는 소유자가 정한다.

use std::collections::HashMap;

#[derive(Debug, Clone)]
struct Entry<V> {
    value: V,
    access: u64,
}

#[derive(Debug, Clone)]
pub struct WorkspaceValueCache<V> {
    entries: HashMap<String, Entry<V>>,
    sequence: u64,
    capacity: usize,
}

impl<V: Clone> WorkspaceValueCache<V> {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            sequence: 0,
            capacity: capacity.max(1),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn values(&self) -> Vec<V> {
        self.entries.values().map(|e| e.value.clone()).collect()
    }

    /// 조회는 접근 순서를 갱신한다.
    pub fn value(&mut self, key: &str) -> Option<V> {
        let entry = self.entries.get_mut(key)?;
        self.sequence = self.sequence.wrapping_add(1);
        entry.access = self.sequence;
        Some(entry.value.clone())
    }

    pub fn insert(&mut self, value: V, key: &str) {
        if !self.entries.contains_key(key) && self.entries.len() >= self.capacity {
            if let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.access)
                .map(|(k, _)| k.clone())
            {
                self.entries.remove(&oldest);
            }
        }
        self.sequence = self.sequence.wrapping_add(1);
        self.entries.insert(
            key.to_string(),
            Entry {
                value,
                access: self.sequence,
            },
        );
    }

    pub fn remove(&mut self, key: &str) {
        self.entries.remove(key);
    }

    pub fn remove_values_where(&mut self, mut predicate: impl FnMut(&V) -> bool) {
        self.entries.retain(|_, e| !predicate(&e.value));
    }

    pub fn clear(&mut self) {
        self.entries = HashMap::new();
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/WorkspaceCacheTests.swift` 의 LRU 케이스.
    use super::*;

    #[test]
    fn lru_evicts_least_recently_used() {
        let mut cache = WorkspaceValueCache::new(2);
        cache.insert(1, "a");
        cache.insert(2, "b");
        assert_eq!(cache.value("a"), Some(1));
        cache.insert(3, "c");
        assert_eq!(cache.value("b"), None);
        cache.insert(4, "a");
        assert!(cache.value("c") == Some(3) && cache.len() == 2);
        cache.remove_values_where(|v| *v == 4);
        assert_eq!(cache.len(), 1);
        cache.clear();
        assert_eq!(cache.len(), 0);
        assert_eq!(WorkspaceValueCache::<i32>::new(0).capacity(), 1);
    }
}
