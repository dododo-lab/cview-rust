//! `Sources/CViewApp/Services/PinnedNoticeCache.swift` 포팅.
//!
//! 스트리머 채팅 공지(cmd 94010)의 채널별 마지막 값 + 사용자 dismiss 기록 캐시.
//! - 채널별 마지막 공지를 키-값 저장소에 JSON 으로 저장한다(앱 재시작 보존).
//! - 사용자가 배너 X 로 닫은 공지 id 를 별도 목록(최대 500, FIFO)으로 보관한다.
//! - 채널 키는 200개 상한 — 넘으면 저장된 `timestamp` 가 오래된 것부터(해독 불가는 맨 먼저) 버린다.
//! - 같은 공지(같은 id·작성자·내용)가 다시 오면 쓰지 않는다. 단 저장본이 24시간을 넘었으면 다시 쓴다.
//!
//! 원본은 `UserDefaults.standard` 를 쓰는 정적 enum 이다. 여기서는 저장소를 주입받는 값으로 옮겼다.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use cview_core::models::chat::item::ChatMessageItem;
use cview_core::models::chat::messages::MessageType;
use cview_core::models::chat::notice_kind::ChatSystemNoticeKind;
use cview_core::utils::time::TimeInterval;
use serde::{Deserialize, Serialize};

use super::support::{lock, KeyValueStore};

const NOTICE_KEY_PREFIX: &str = "chat.pinnedNotice.v1.";
const DISMISSED_IDS_KEY: &str = "chat.pinnedNotice.dismissedIds.v1";

/// 저장용 최소 공지 — 화면 표시에 필요한 필드만.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersistedNotice {
    pub id: String,
    pub nickname: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

impl PersistedNotice {
    pub fn from_item(item: &ChatMessageItem) -> Self {
        Self {
            id: item.id.clone(),
            nickname: item.nickname.clone(),
            content: item.content.clone(),
            timestamp: item.timestamp,
        }
    }

    /// 같은 공지인가 — 받은 시각은 보지 않는다(재전송마다 달라진다).
    pub fn is_same_notice(&self, other: &Self) -> bool {
        self.id == other.id && self.nickname == other.nickname && self.content == other.content
    }

    pub fn to_chat_message_item(&self) -> ChatMessageItem {
        let mut item = ChatMessageItem::system(self.content.clone(), ChatSystemNoticeKind::Info);
        item.id = self.id.clone();
        item.user_id = "notice".into();
        item.nickname = self.nickname.clone();
        item.timestamp = self.timestamp;
        item.r#type = MessageType::Notice;
        item.is_notice = true;
        item.is_system = false;
        item
    }
}

pub struct PinnedNoticeCache {
    defaults: Arc<dyn KeyValueStore>,
    /// dismissed id 조회용 인메모리 미러(write-through).
    dismissed_ids_cache: Mutex<Option<HashSet<String>>>,
}

impl PinnedNoticeCache {
    /// dismissed id 상한.
    pub const DISMISSED_IDS_MAX_COUNT: usize = 500;
    /// 채널별 공지 키 상한.
    pub const NOTICE_CHANNEL_MAX_COUNT: usize = 200;
    /// 같은 공지의 저장 시각을 갱신하는 최소 간격(24시간).
    pub const RECENCY_REFRESH_INTERVAL: TimeInterval = 24.0 * 60.0 * 60.0;

    pub fn new(defaults: Arc<dyn KeyValueStore>) -> Self {
        Self {
            defaults,
            dismissed_ids_cache: Mutex::new(None),
        }
    }

    fn notice_key(channel_id: &str) -> String {
        format!("{NOTICE_KEY_PREFIX}{channel_id}")
    }

    fn read(&self, key: &str) -> Option<PersistedNotice> {
        serde_json::from_value(self.defaults.value(key)?).ok()
    }

    /// 채널별 최근 공지 저장.
    pub fn save(&self, notice: &ChatMessageItem, channel_id: &str) {
        if channel_id.is_empty() || notice.id.is_empty() {
            return;
        }
        let persisted = PersistedNotice::from_item(notice);
        let key = Self::notice_key(channel_id);
        if let Some(stored) = self.read(&key) {
            let age = (persisted.timestamp - stored.timestamp).num_milliseconds() as f64 / 1000.0;
            if stored.is_same_notice(&persisted) && age <= Self::RECENCY_REFRESH_INTERVAL {
                return;
            }
        }
        let Ok(value) = serde_json::to_value(&persisted) else {
            return;
        };
        self.defaults.set_value(&key, value);
        self.enforce_channel_key_limit();
    }

    /// 채널의 캐시된 공지. dismissed 검사는 호출자 책임.
    pub fn load(&self, channel_id: &str) -> Option<ChatMessageItem> {
        if channel_id.is_empty() {
            return None;
        }
        self.read(&Self::notice_key(channel_id))
            .map(|n| n.to_chat_message_item())
    }

    /// 사용자가 X 로 닫은 공지 id 기록. 최근 dismiss 가 가장 뒤로 간다.
    pub fn mark_dismissed(&self, notice_id: &str) {
        if notice_id.is_empty() {
            return;
        }
        let mut ids = self.defaults.strings(DISMISSED_IDS_KEY).unwrap_or_default();
        ids.retain(|id| id != notice_id);
        ids.push(notice_id.to_string());
        if ids.len() > Self::DISMISSED_IDS_MAX_COUNT {
            let excess = ids.len() - Self::DISMISSED_IDS_MAX_COUNT;
            ids.drain(..excess);
        }
        self.defaults.set_strings(DISMISSED_IDS_KEY, &ids);
        *lock(&self.dismissed_ids_cache) = Some(ids.into_iter().collect());
    }

    pub fn is_dismissed(&self, notice_id: &str) -> bool {
        if notice_id.is_empty() {
            return false;
        }
        let mut cache = lock(&self.dismissed_ids_cache);
        if let Some(ids) = cache.as_ref() {
            return ids.contains(notice_id);
        }
        let ids: HashSet<String> = self
            .defaults
            .strings(DISMISSED_IDS_KEY)
            .unwrap_or_default()
            .into_iter()
            .collect();
        let hit = ids.contains(notice_id);
        *cache = Some(ids);
        hit
    }

    /// 서버 공지 해제 또는 명시적 초기화.
    pub fn clear(&self, channel_id: &str) {
        if channel_id.is_empty() {
            return;
        }
        self.defaults.remove(&Self::notice_key(channel_id));
    }

    /// 채널 키 상한 적용 — 오래된 공지부터 버린다. 값 디코드는 상한을 넘겼을 때만 돈다.
    pub fn enforce_channel_key_limit(&self) {
        let keys = self.defaults.keys_with_prefix(NOTICE_KEY_PREFIX);
        if keys.len() <= Self::NOTICE_CHANNEL_MAX_COUNT {
            return;
        }
        let excess = keys.len() - Self::NOTICE_CHANNEL_MAX_COUNT;
        let mut dated: Vec<(String, Option<DateTime<Utc>>)> = keys
            .into_iter()
            .map(|k| {
                let at = self.read(&k).map(|n| n.timestamp);
                (k, at)
            })
            .collect();
        // 해독 불가(None) = 가장 먼저 버릴 후보.
        dated.sort_by_key(|entry| entry.1);
        for (key, _) in dated.into_iter().take(excess) {
            self.defaults.remove(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/PinnedNoticeCacheLimitTests.swift`
    use super::*;
    use crate::services::support::MemoryKeyValueStore;
    use chrono::Duration;
    use cview_core::utils::time::from_unix;

    fn notice(id: &str, content: &str, at: DateTime<Utc>) -> ChatMessageItem {
        PersistedNotice {
            id: id.into(),
            nickname: "스트리머".into(),
            content: content.into(),
            timestamp: at,
        }
        .to_chat_message_item()
    }

    fn seed(store: &MemoryKeyValueStore, channel: &str, at: DateTime<Utc>) {
        let p = PersistedNotice::from_item(&notice(&format!("notice-{channel}"), "공지", at));
        store.set_value(
            &format!("{NOTICE_KEY_PREFIX}{channel}"),
            serde_json::to_value(p).unwrap(),
        );
    }

    fn remaining(store: &MemoryKeyValueStore) -> HashSet<String> {
        store
            .keys_with_prefix(NOTICE_KEY_PREFIX)
            .into_iter()
            .map(|k| k[NOTICE_KEY_PREFIX.len()..].to_string())
            .collect()
    }

    fn setup() -> (Arc<MemoryKeyValueStore>, PinnedNoticeCache) {
        let store = Arc::new(MemoryKeyValueStore::new());
        let cache = PinnedNoticeCache::new(store.clone());
        (store, cache)
    }

    #[test]
    fn under_limit_keeps_everything() {
        let (store, cache) = setup();
        let base = from_unix(1_770_000_000.0);
        for i in 0..10 {
            seed(&store, &format!("ch{i}"), base + Duration::seconds(i));
        }
        cache.enforce_channel_key_limit();
        assert_eq!(remaining(&store).len(), 10);
    }

    #[test]
    fn over_limit_evicts_oldest_first() {
        let (store, cache) = setup();
        let limit = PinnedNoticeCache::NOTICE_CHANNEL_MAX_COUNT;
        let base = from_unix(1_770_000_000.0);
        for i in 0..(limit + 5) {
            seed(
                &store,
                &format!("ch{i}"),
                base + Duration::seconds(i as i64),
            );
        }
        cache.enforce_channel_key_limit();
        let left = remaining(&store);
        assert_eq!(left.len(), limit);
        for i in 0..5 {
            assert!(!left.contains(&format!("ch{i}")));
        }
        assert!(left.contains(&format!("ch{}", limit + 4)));
    }

    #[test]
    fn corrupted_entries_evicted_first() {
        let (store, cache) = setup();
        let limit = PinnedNoticeCache::NOTICE_CHANNEL_MAX_COUNT;
        let base = from_unix(1_770_000_000.0);
        for i in 0..limit {
            seed(
                &store,
                &format!("ok{i}"),
                base + Duration::seconds(i as i64),
            );
        }
        store.set_string(&format!("{NOTICE_KEY_PREFIX}broken"), "not-json");
        cache.enforce_channel_key_limit();
        let left = remaining(&store);
        assert_eq!(left.len(), limit);
        assert!(!left.contains("broken"));
        assert!(left.contains("ok0"));
    }

    #[test]
    fn same_notice_is_not_rewritten_but_refreshed_after_a_day() {
        let (store, cache) = setup();
        let key = format!("{NOTICE_KEY_PREFIX}ch");
        let first = from_unix(1_780_000_000.0);
        cache.save(&notice("n1", "공지", first), "ch");
        let stored = store.value(&key).unwrap();
        cache.save(&notice("n1", "공지", first + Duration::seconds(60)), "ch");
        assert_eq!(
            store.value(&key).unwrap(),
            stored,
            "같은 공지 재전송을 다시 썼다"
        );

        cache.save(
            &notice("n1", "고친 공지", first + Duration::seconds(90)),
            "ch",
        );
        assert_eq!(cache.load("ch").unwrap().content, "고친 공지");

        let later = first + Duration::hours(25);
        cache.save(&notice("n1", "고친 공지", later), "ch");
        assert_eq!(cache.load("ch").unwrap().timestamp, later);
        assert!(cache.load("ch").unwrap().is_notice);
    }

    #[test]
    fn dismissed_ids_are_fifo_capped() {
        let (_store, cache) = setup();
        assert!(!cache.is_dismissed("a"));
        for i in 0..(PinnedNoticeCache::DISMISSED_IDS_MAX_COUNT + 1) {
            cache.mark_dismissed(&format!("id{i}"));
        }
        assert!(!cache.is_dismissed("id0"));
        assert!(cache.is_dismissed("id1"));
        assert!(!cache.is_dismissed(""));
    }
}
