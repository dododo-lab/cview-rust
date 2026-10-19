//! `Sources/CViewApp/Services/MentionInbox.swift` (+ `MultiChatSearchStore.swift`) 포팅.
//!
//! 멀티채팅 마스터 멘션 트레이(최근 N건 링 버퍼)와 멀티채팅 검색 상태.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};

use cview_core::logic::mention_detector::MentionMatch;
use cview_core::models::chat::ChatMessageItem;
use cview_core::utils::time::Date;

use super::multi_chat_preferences::MultiChatScope;
use super::support::lock;

#[derive(Debug, Clone, PartialEq)]
pub struct MentionEntry {
    /// messageId.
    pub id: String,
    /// 트레이 클릭 네비게이션·remove 의 정규 키(stream channelId).
    pub stream_channel_id: String,
    /// 치지직 WS 프로토콜 식별자(참조용).
    pub chat_channel_id: String,
    pub channel_name: String,
    pub message: ChatMessageItem,
    pub matches: HashSet<MentionMatch>,
    pub received_at: Date,
}

#[derive(Debug)]
pub struct MentionInbox {
    cap: usize,
    entries: Mutex<Vec<MentionEntry>>,
}

impl MentionInbox {
    pub fn new(cap: usize) -> Self {
        Self {
            cap,
            entries: Mutex::new(Vec::new()),
        }
    }

    /// 스코프별 인박스 — 라이브 도크와 멀티채팅 작업영역이 트레이를 독립 운영한다.
    pub fn scoped(scope: MultiChatScope) -> Arc<MentionInbox> {
        static LIVE: OnceLock<Arc<MentionInbox>> = OnceLock::new();
        static STANDALONE: OnceLock<Arc<MentionInbox>> = OnceLock::new();
        let slot = match scope {
            MultiChatScope::Live => &LIVE,
            MultiChatScope::Standalone => &STANDALONE,
        };
        slot.get_or_init(|| Arc::new(MentionInbox::new(50))).clone()
    }

    /// 기존 호출 호환 — `Live` 인박스.
    pub fn shared() -> Arc<MentionInbox> {
        Self::scoped(MultiChatScope::Live)
    }

    pub fn entries(&self) -> Vec<MentionEntry> {
        lock(&self.entries).clone()
    }

    pub fn append(&self, entry: MentionEntry) {
        let mut entries = lock(&self.entries);
        entries.push(entry);
        if entries.len() > self.cap {
            let excess = entries.len() - self.cap;
            entries.drain(..excess);
        }
    }

    pub fn clear(&self) {
        lock(&self.entries).clear();
    }

    /// 정규 키(streamChannelId) 기준 제거.
    pub fn remove(&self, stream_channel_id: &str) {
        lock(&self.entries).retain(|e| e.stream_channel_id != stream_channel_id);
    }
}

// MARK: - 검색 상태

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MultiChatSearchScope {
    #[default]
    InChannel,
    CrossChannel,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    /// messageId.
    pub id: String,
    pub chat_channel_id: String,
    pub channel_name: String,
    pub message: ChatMessageItem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ActiveFilters {
    pub donations: bool,
    pub mentions: bool,
    pub links: bool,
}

/// 멀티채팅 검색 상태 — 입력/범위/필터/결과.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MultiChatSearchStore {
    pub is_open: bool,
    pub query: String,
    pub scope: MultiChatSearchScope,
    pub filters: ActiveFilters,
    pub results: Vec<SearchResult>,
    /// **마지막으로 실행된** 질의 — 빈 결과 안내는 이것으로 건다(타이핑만으로 뜨지 않게).
    pub last_committed_query: String,
}

impl MultiChatSearchStore {
    pub fn toggle_open(&mut self) {
        self.is_open = !self.is_open;
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/MentionInboxTests.swift` 핵심.
    use super::*;
    use cview_core::utils::time::from_unix;

    fn entry(id: &str, stream: &str) -> MentionEntry {
        MentionEntry {
            id: id.into(),
            stream_channel_id: stream.into(),
            chat_channel_id: format!("chat-{stream}"),
            channel_name: stream.into(),
            message: ChatMessageItem::new(
                id,
                "u",
                "n",
                "c",
                from_unix(0.0),
                Default::default(),
                Default::default(),
                false,
                false,
            ),
            matches: HashSet::new(),
            received_at: from_unix(0.0),
        }
    }

    #[test]
    fn ring_buffer_caps_and_remove_by_stream_id() {
        let inbox = MentionInbox::new(3);
        for i in 0..5 {
            inbox.append(entry(&format!("m{i}"), if i % 2 == 0 { "a" } else { "b" }));
        }
        let ids: Vec<String> = inbox.entries().into_iter().map(|e| e.id).collect();
        assert_eq!(ids, vec!["m2", "m3", "m4"]);
        inbox.remove("a");
        assert_eq!(inbox.entries().len(), 1);
        inbox.clear();
        assert!(inbox.entries().is_empty());
        assert!(!Arc::ptr_eq(
            &MentionInbox::scoped(MultiChatScope::Live),
            &MentionInbox::scoped(MultiChatScope::Standalone)
        ));
    }

    #[test]
    fn search_store_toggles() {
        let mut s = MultiChatSearchStore::default();
        s.toggle_open();
        assert!(s.is_open);
        assert_eq!(s.scope, MultiChatSearchScope::InChannel);
    }
}
