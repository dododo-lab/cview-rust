//! `Sources/CViewApp/Caching/FavoritesStore.swift` 포팅.
//!
//! 즐겨찾기 채널 ID 집합의 메모리 미러. 진실의 원천은 `DataStore` 이고, 쓰기는 영속이
//! 성공한 뒤에만 반영한다(낙관적 갱신 금지). 쓰기 경로는 `AppState::set_favorite` 하나다.

use std::collections::HashSet;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FavoritesStore {
    channel_ids: HashSet<String>,
    /// 실제로 집합이 바뀐 횟수 — `@Observable` 발행 대응(같은 값 대입은 세지 않는다).
    revision: u64,
}

impl FavoritesStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn channel_ids(&self) -> &HashSet<String> {
        &self.channel_ids
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// 영속에서 통째로 채운다. 같은 집합이면 대입하지 않는다.
    pub fn hydrate<I: IntoIterator<Item = String>>(&mut self, ids: I) {
        let next: HashSet<String> = ids.into_iter().collect();
        if next != self.channel_ids {
            self.channel_ids = next;
            self.revision += 1;
        }
    }

    /// 영속이 성공한 뒤에만 부른다. 실제로 바뀌었는지 돌려준다.
    pub fn apply(&mut self, channel_id: &str, is_favorite: bool) -> bool {
        let had = self.channel_ids.contains(channel_id);
        if had == is_favorite {
            return false;
        }
        if is_favorite {
            self.channel_ids.insert(channel_id.to_string());
        } else {
            self.channel_ids.remove(channel_id);
        }
        self.revision += 1;
        true
    }

    pub fn contains(&self, channel_id: &str) -> bool {
        self.channel_ids.contains(channel_id)
    }

    /// 로그아웃 등 계정 경계에서 비운다.
    pub fn clear(&mut self) {
        if !self.channel_ids.is_empty() {
            self.channel_ids.clear();
            self.revision += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hydrate_skips_same_set_and_apply_reports_change() {
        let mut s = FavoritesStore::new();
        s.hydrate(["a".to_string(), "b".to_string()]);
        let rev = s.revision();
        s.hydrate(["b".to_string(), "a".to_string()]);
        assert_eq!(s.revision(), rev);
        assert!(!s.apply("a", true));
        assert!(s.apply("c", true));
        assert!(s.apply("a", false));
        assert!(s.contains("c") && !s.contains("a"));
        s.clear();
        assert!(s.channel_ids().is_empty());
    }
}
