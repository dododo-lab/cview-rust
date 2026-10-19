//! `Sources/CViewApp/Logic/MergedChatMergeState.swift` 포팅
//! (+ `Views/MergedChatView.swift` 의 `MergedMessageItem`,
//! `Views/MergedChatView+MergeLogic.swift` 의 `applyInPlaceUpdates`).
//!
//! 병합 커서는 렌더 상태가 아니다. 한 틱이 제거·모더레이션·추가·트리밍을 한 값에 모아
//! 뷰는 최종 상한 목록 하나만 발행한다.

use std::collections::HashMap;

use cview_chat::utils::ChatMessageBuffer;
use cview_core::models::chat::ChatMessageItem;

/// 통합 채팅 한 행. 색은 플랫폼 독립 문자열(예: `#FF0000`)로 보관한다.
#[derive(Debug, Clone)]
pub struct MergedMessageItem {
    /// `"{streamChannelId}_{messageId}"` — 세션 재정렬에도 identity 가 유지된다.
    pub id: String,
    pub stream_channel_id: String,
    pub channel_name: String,
    pub channel_color: String,
    pub message: ChatMessageItem,
}

impl PartialEq for MergedMessageItem {
    /// Swift 와 같이 id 와 메시지만 비교한다.
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.message == other.message
    }
}

impl MergedMessageItem {
    /// `MergedChatView.applyInPlaceUpdates` — 같은 채널의 행을 최신 메시지 값으로 교체한다.
    /// 교체한 행 수를 돌려준다.
    pub fn apply_in_place_updates(
        items: &mut [MergedMessageItem],
        stream_channel_id: &str,
        latest: &HashMap<String, ChatMessageItem>,
    ) -> usize {
        let mut replaced = 0;
        for item in items
            .iter_mut()
            .filter(|i| i.stream_channel_id == stream_channel_id)
        {
            let Some(newest) = latest.get(&item.message.id) else {
                continue;
            };
            if *newest == item.message {
                continue;
            }
            item.message = newest.clone();
            replaced += 1;
        }
        replaced
    }
}

#[derive(Debug, Clone)]
pub struct Source<'a> {
    pub id: String,
    pub name: String,
    pub color: String,
    pub messages: &'a ChatMessageBuffer,
    pub moderation_revision: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Batch {
    pub rows: Vec<MergedMessageItem>,
    pub added: Vec<MergedMessageItem>,
    pub suppress_scroll: bool,
}

struct PendingBatch {
    rows: Vec<MergedMessageItem>,
    added: Vec<MergedMessageItem>,
    changed: bool,
    suppress_scroll: bool,
}

#[derive(Debug, Default)]
pub struct MergedChatMergeState {
    pub last_merged_counts: HashMap<String, usize>,
    pub last_merged_message_ids: HashMap<String, String>,
    pub last_merged_moderation_revisions: HashMap<String, u64>,
    pub last_merged_restructure_counts: HashMap<String, u64>,
}

impl MergedChatMergeState {
    pub fn new() -> Self {
        Self::default()
    }

    /// 변경이 없으면 `None` — 발행하지 않는다.
    pub fn merge(
        &mut self,
        sources: &[Source<'_>],
        existing: &[MergedMessageItem],
        row_cap: usize,
    ) -> Option<Batch> {
        let mut batch = PendingBatch {
            rows: existing.to_vec(),
            added: Vec::new(),
            changed: false,
            suppress_scroll: false,
        };
        for source in sources {
            self.collect(source, &mut batch);
        }
        if !batch.added.is_empty() {
            batch.added.sort_by(|l, r| {
                l.message
                    .timestamp
                    .cmp(&r.message.timestamp)
                    .then_with(|| l.id.cmp(&r.id))
            });
            let rows = std::mem::take(&mut batch.rows);
            batch.rows = Self::inserting(&batch.added, rows);
            batch.changed = true;
            if batch.rows.len() > row_cap {
                let excess = batch.rows.len() - row_cap;
                batch.rows.drain(..excess);
                batch.suppress_scroll = true;
            }
        }
        // 새 메시지 없는 삭제/초기화도 한 번은 발행해야 한다.
        if !batch.changed {
            return None;
        }
        Some(Batch {
            rows: batch.rows,
            added: batch.added,
            suppress_scroll: batch.suppress_scroll,
        })
    }

    fn collect(&mut self, source: &Source<'_>, batch: &mut PendingBatch) {
        let messages = source.messages;
        let current_count = messages.len();
        let restructure = messages.restructure_count();
        let restructured = self
            .last_merged_restructure_counts
            .get(&source.id)
            .is_some_and(|&c| c != restructure);
        self.last_merged_restructure_counts
            .insert(source.id.clone(), restructure);
        if current_count == 0 && !restructured {
            return;
        }
        if self.reconcile_moderated_rows(source, &mut batch.rows) {
            batch.changed = true;
        }
        let last_count = self
            .last_merged_counts
            .get(&source.id)
            .copied()
            .unwrap_or(0);
        let known_last_id = self.last_merged_message_ids.get(&source.id).cloned();
        if !restructured
            && last_count == current_count
            && known_last_id.as_deref() == messages.last().map(|m| m.id.as_str())
        {
            return;
        }

        // 링이 처음 가득 차 오래된 행을 축출해도 ID 로 기준을 잡는다.
        let scan_start = match &known_last_id {
            Some(known) => {
                let found = if !restructured && current_count >= last_count {
                    messages.last_index_of_message_id(known)
                } else {
                    None
                };
                match found {
                    Some(index) => index + 1,
                    None => {
                        let old_count = batch.rows.len();
                        batch.rows.retain(|r| r.stream_channel_id != source.id);
                        batch.changed = batch.changed || batch.rows.len() != old_count;
                        batch.suppress_scroll = true;
                        0
                    }
                }
            }
            None => 0,
        };
        for message in messages.iter().skip(scan_start) {
            batch.added.push(MergedMessageItem {
                id: format!("{}_{}", source.id, message.id),
                stream_channel_id: source.id.clone(),
                channel_name: source.name.clone(),
                channel_color: source.color.clone(),
                message: message.clone(),
            });
        }
        self.last_merged_counts
            .insert(source.id.clone(), current_count);
        match messages.last() {
            Some(last) => {
                self.last_merged_message_ids
                    .insert(source.id.clone(), last.id.clone());
            }
            None => {
                self.last_merged_message_ids.remove(&source.id);
            }
        }
    }

    fn reconcile_moderated_rows(
        &mut self,
        source: &Source<'_>,
        rows: &mut [MergedMessageItem],
    ) -> bool {
        if self.last_merged_moderation_revisions.get(&source.id)
            == Some(&source.moderation_revision)
        {
            return false;
        }
        self.last_merged_moderation_revisions
            .insert(source.id.clone(), source.moderation_revision);
        let latest: HashMap<String, ChatMessageItem> = source
            .messages
            .iter()
            .map(|m| (m.id.clone(), m.clone()))
            .collect();
        MergedMessageItem::apply_in_place_updates(rows, &source.id, &latest) > 0
    }

    fn inserting(
        added: &[MergedMessageItem],
        existing: Vec<MergedMessageItem>,
    ) -> Vec<MergedMessageItem> {
        let needs_merge = match (existing.last(), added.first()) {
            (Some(last), Some(first)) => first.message.timestamp < last.message.timestamp,
            _ => false,
        };
        if !needs_merge {
            let mut out = existing;
            out.extend_from_slice(added);
            return out;
        }
        let mut merged = Vec::with_capacity(existing.len() + added.len());
        let mut e = existing.into_iter().peekable();
        let mut a = added.iter().peekable();
        while let (Some(ex), Some(ad)) = (e.peek(), a.peek()) {
            if ex.message.timestamp <= ad.message.timestamp {
                merged.extend(e.next());
            } else {
                merged.extend(a.next().cloned());
            }
        }
        merged.extend(e);
        merged.extend(a.cloned());
        merged
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/MergedChatBatchTests.swift`
    use std::collections::HashSet;

    use chrono::{TimeZone, Utc};
    use cview_core::models::chat::{EmojiMap, MessageType};

    use super::*;

    fn item(n: i64, blinded: bool) -> ChatMessageItem {
        let mut m = ChatMessageItem::new(
            format!("m{n}"),
            "u",
            "닉",
            if blinded { "삭제됨" } else { "내용" },
            Utc.timestamp_opt(n, 0).unwrap(),
            MessageType::Normal,
            EmojiMap::new(),
            false,
            false,
        );
        m.is_blinded = blinded;
        m
    }

    fn items(range: std::ops::Range<i64>) -> Vec<ChatMessageItem> {
        range.map(|n| item(n, false)).collect()
    }

    fn source<'a>(id: &str, messages: &'a ChatMessageBuffer, revision: u64) -> Source<'a> {
        Source {
            id: id.into(),
            name: id.into(),
            color: "#FF0000".into(),
            messages,
            moderation_revision: revision,
        }
    }

    fn ids(rows: &[MergedMessageItem]) -> Vec<&str> {
        rows.iter().map(|r| r.id.as_str()).collect()
    }

    #[test]
    fn unchanged_ticks_and_first_eviction() {
        let mut state = MergedChatMergeState::new();
        let mut buffer = ChatMessageBuffer::new(10);
        buffer.extend(items(0..8));
        let first = state.merge(&[source("A", &buffer, 0)], &[], 300).unwrap();
        for _ in 0..100 {
            assert!(state
                .merge(&[source("A", &buffer, 0)], &first.rows, 300)
                .is_none());
        }
        buffer.extend(items(8..13));
        let next = state
            .merge(&[source("A", &buffer, 0)], &first.rows, 300)
            .unwrap();
        let added: Vec<String> = next.added.iter().map(|r| r.message.id.clone()).collect();
        let expected: Vec<String> = (8..13).map(|n| format!("m{n}")).collect();
        assert_eq!(added, expected);
        assert_eq!(next.rows.len(), 13);
        let unique: HashSet<&str> = ids(&next.rows).into_iter().collect();
        assert_eq!(unique.len(), next.rows.len());
        assert!(!next.suppress_scroll);
    }

    #[test]
    fn reset_publishes_without_new_messages() {
        let mut state = MergedChatMergeState::new();
        let mut a = ChatMessageBuffer::default();
        let mut b = ChatMessageBuffer::default();
        a.push(item(1, false));
        b.push(item(2, false));
        let first = state
            .merge(&[source("A", &a, 0), source("B", &b, 0)], &[], 300)
            .unwrap();
        a.clear();
        let cleared = state
            .merge(&[source("A", &a, 0), source("B", &b, 0)], &first.rows, 300)
            .unwrap();
        assert_eq!(ids(&cleared.rows), vec!["B_m2"]);
        assert!(cleared.added.is_empty() && cleared.suppress_scroll);
        assert!(state
            .merge(
                &[source("A", &a, 0), source("B", &b, 0)],
                &cleared.rows,
                300
            )
            .is_none());
    }

    #[test]
    fn restructure_and_moderation_in_one_batch() {
        let mut state = MergedChatMergeState::new();
        let mut a = ChatMessageBuffer::default();
        let mut b = ChatMessageBuffer::default();
        a.extend([item(1, false), item(3, false)]);
        b.push(item(4, false));
        let first = state
            .merge(&[source("A", &a, 0), source("B", &b, 0)], &[], 300)
            .unwrap();
        a.replace_all(vec![item(2, false), item(3, false)]);
        b.map_in_place(|_| item(4, true));
        let next = state
            .merge(&[source("A", &a, 0), source("B", &b, 1)], &first.rows, 300)
            .unwrap();
        assert_eq!(ids(&next.rows), vec!["A_m2", "A_m3", "B_m4"]);
        assert!(next.rows.last().unwrap().message.is_blinded);
        assert!(next.suppress_scroll);
        b.map_in_place(|_| item(4, false));
        let moderation_only = state
            .merge(&[source("A", &a, 0), source("B", &b, 2)], &next.rows, 300)
            .unwrap();
        assert!(moderation_only.added.is_empty());
        assert!(!moderation_only.rows.last().unwrap().message.is_blinded);
        assert!(!moderation_only.suppress_scroll);
    }

    #[test]
    fn lost_anchor_and_late_arrival() {
        let mut state = MergedChatMergeState::new();
        let mut a = ChatMessageBuffer::new(10);
        let mut b = ChatMessageBuffer::default();
        a.push(item(0, false));
        let first = state.merge(&[source("A", &a, 0)], &[], 300).unwrap();
        a.extend(items(1..21));
        b.push(item(12, false));
        let next = state
            .merge(&[source("A", &a, 0), source("B", &b, 0)], &first.rows, 300)
            .unwrap();
        assert_eq!(next.rows.len(), 11);
        assert!(!next.rows.iter().any(|r| r.id == "A_m0"));
        b.push(item(15, false));
        let late = state
            .merge(&[source("A", &a, 0), source("B", &b, 0)], &next.rows, 300)
            .unwrap();
        let ts: Vec<_> = late.rows.iter().map(|r| r.message.timestamp).collect();
        let mut sorted = ts.clone();
        sorted.sort();
        assert_eq!(ts, sorted);
        let unique: HashSet<&str> = ids(&late.rows).into_iter().collect();
        assert_eq!(unique.len(), 12);
    }

    #[test]
    fn trim_is_part_of_final_batch() {
        for (cap, expected) in [(300usize, 300usize), (1_500, 305)] {
            let mut state = MergedChatMergeState::new();
            let mut buffer = ChatMessageBuffer::new(400);
            buffer.extend(items(0..300));
            let first = state.merge(&[source("A", &buffer, 0)], &[], cap).unwrap();
            buffer.extend(items(300..305));
            let next = state
                .merge(&[source("A", &buffer, 0)], &first.rows, cap)
                .unwrap();
            assert_eq!(next.rows.len(), expected);
            assert_eq!(next.added.len(), 5);
            assert_eq!(next.suppress_scroll, cap == 300);
            assert_eq!(
                next.rows.first().unwrap().id,
                if cap == 300 { "A_m5" } else { "A_m0" }
            );
        }
    }
}
