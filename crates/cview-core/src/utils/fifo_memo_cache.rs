//! 순수 함수 결과 메모이즈용 공용 FIFO 캐시.
//!
//! 원본: `Sources/CViewCore/Utilities/FIFOMemoCache.swift`
//!
//! 동작 계약(원본과 동일):
//! - 조회는 락 안에서, **생성은 락 밖에서**(파싱/빌드가 비싸다).
//! - 동시 miss 로 다른 스레드가 이미 채웠으면 덮어쓰지 않는다(삽입 순서 꼬임 방지).
//! - 용량 도달 시 가장 오래 전에 삽입된 키부터 축출(LRU 아님 — 접근 시각 갱신 비용이 없다).

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Mutex, MutexGuard};

#[derive(Debug)]
struct State<K, V> {
    entries: HashMap<K, V>,
    /// 삽입 순서 — 진짜 FIFO 축출용(해시맵 순회 순서는 임의다).
    insertion_order: Vec<K>,
    next_eviction_index: usize,
}

/// 순수 함수 결과를 캐싱하는 스레드 안전 FIFO 캐시.
#[derive(Debug)]
pub struct FifoMemoCache<K, V> {
    state: Mutex<State<K, V>>,
    capacity: usize,
}

impl<K: Hash + Eq + Clone, V: Clone> FifoMemoCache<K, V> {
    /// # Panics
    /// `capacity == 0` 이면 패닉(원본의 `precondition`).
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "FIFOMemoCache capacity must be positive");
        Self {
            state: Mutex::new(State {
                entries: HashMap::new(),
                insertion_order: Vec::new(),
                next_eviction_index: 0,
            }),
            capacity,
        }
    }

    fn lock(&self) -> MutexGuard<'_, State<K, V>> {
        // 독(poison)은 make() 패닉에서만 생길 수 있는데 make 는 락 밖에서 돈다 — 상태는 일관적이다.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 캐시에 있으면 그대로, 없으면 `make()` 로 만들어 저장 후 반환한다.
    ///
    /// `make` 는 **락을 잡지 않은 상태**에서 실행되므로 같은 키에 대해 동시에 여러 번
    /// 실행될 수 있다 — 부작용 없는 순수 함수여야 한다.
    pub fn value(&self, key: K, make: impl FnOnce() -> V) -> V {
        if let Some(hit) = self.lock().entries.get(&key) {
            return hit.clone();
        }
        let made = make();
        let mut state = self.lock();
        if state.entries.contains_key(&key) {
            // 원본은 먼저 저장된 값을 유지하고 새로 만든 값을 반환한다. make 가 순수하므로 같은 값이다.
            return made;
        }
        if state.entries.len() >= self.capacity && !state.insertion_order.is_empty() {
            let idx = state.next_eviction_index;
            let evicted = std::mem::replace(&mut state.insertion_order[idx], key.clone());
            state.entries.remove(&evicted);
            state.next_eviction_index = (idx + 1) % self.capacity;
        } else {
            state.insertion_order.push(key.clone());
        }
        state.entries.insert(key, made.clone());
        made
    }

    /// 진단/테스트용 현재 항목 수.
    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 전체 비우기 — 설정 변경 등으로 캐시된 결과가 통째로 무효해질 때.
    pub fn remove_all(&self) {
        let mut state = self.lock();
        state.entries.clear();
        state.insertion_order.clear();
        state.next_eviction_index = 0;
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewCoreTests/FIFOMemoCacheTests.swift`
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn hit_skips_make() {
        let cache = FifoMemoCache::<String, i32>::new(4);
        let calls = AtomicUsize::new(0);
        let load = |k: &str, v: i32| {
            cache.value(k.to_string(), || {
                calls.fetch_add(1, Ordering::SeqCst);
                v
            })
        };
        assert_eq!(load("a", 1), 1);
        assert_eq!(load("a", 999), 1);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn evicts_in_insertion_order() {
        let cache = FifoMemoCache::<&str, usize>::new(3);
        for (i, k) in ["a", "b", "c"].into_iter().enumerate() {
            cache.value(k, || i);
        }
        assert_eq!(cache.len(), 3);
        cache.value("d", || 3);
        assert_eq!(cache.len(), 3);

        let mut remade = false;
        cache.value("a", || {
            remade = true;
            0
        });
        assert!(remade, "가장 먼저 삽입된 'a' 가 축출됐어야 한다");

        let mut d_remade = false;
        cache.value("d", || {
            d_remade = true;
            0
        });
        assert!(!d_remade, "최근 삽입된 'd' 는 남아 있어야 한다");
    }

    #[test]
    fn remove_all_clears() {
        let cache = FifoMemoCache::<String, i32>::new(8);
        for i in 0..5 {
            cache.value(format!("k{i}"), || i);
        }
        assert_eq!(cache.len(), 5);
        cache.remove_all();
        assert!(cache.is_empty());
    }

    #[test]
    fn concurrent_access_stays_consistent() {
        let cache = Arc::new(FifoMemoCache::<usize, usize>::new(32));
        let handles: Vec<_> = (0..8)
            .map(|worker| {
                let cache = Arc::clone(&cache);
                std::thread::spawn(move || {
                    for i in 0..200 {
                        let key = (worker * 7 + i) % 64;
                        cache.value(key, || key);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert!(cache.len() <= 32);
        assert!(!cache.is_empty());
    }

    #[test]
    fn concurrent_miss_keeps_first_stored_value() {
        let cache = Arc::new(FifoMemoCache::<&'static str, i32>::new(4));
        let handles: Vec<_> = (0..16)
            .map(|_| {
                let cache = Arc::clone(&cache);
                std::thread::spawn(move || cache.value("k", || 42))
            })
            .collect();
        for h in handles {
            assert_eq!(h.join().unwrap(), 42);
        }
        assert_eq!(cache.value("k", || 0), 42);
        assert_eq!(cache.len(), 1);
    }
}
