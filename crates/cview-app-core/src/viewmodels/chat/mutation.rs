//! 네 버퍼(표시·대기·히스토리·리플레이 보류) 일괄 변형.
//!
//! 원본: `Sources/CViewApp/ViewModels/ChatViewModel+Mutation.swift`.

use cview_core::models::chat::ChatMessageItem;

use super::ChatState;

/// 모더레이션 변형.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatMessageMutation {
    /// 가리기. `text` 가 없으면 관리자 삭제 기본 문구.
    Blind {
        message_id: String,
        text: Option<String>,
    },
    PurgeUser {
        user_id: String,
    },
    Remove {
        message_id: String,
    },
}

impl ChatMessageMutation {
    pub fn blind(message_id: impl Into<String>) -> Self {
        Self::Blind {
            message_id: message_id.into(),
            text: None,
        }
    }
}

impl ChatState {
    pub fn apply_mutation(&mut self, mutation: ChatMessageMutation) {
        match mutation {
            ChatMessageMutation::Blind { message_id, text } => {
                let blind_text = text.unwrap_or_else(|| ChatMessageItem::ADMIN_BLIND_TEXT.into());
                let hit = |i: &ChatMessageItem| i.id == message_id && !i.is_blinded;
                self.messages.map_in_place(|item| {
                    if hit(item) {
                        item.blinded(&blind_text)
                    } else {
                        item.clone()
                    }
                });
                let head = self.pending_head.min(self.pending_messages.len());
                for item in &mut self.pending_messages[head..] {
                    if hit(item) {
                        *item = item.blinded(&blind_text);
                    }
                }
                for item in self
                    .chat_history
                    .iter_mut()
                    .chain(self.replay_hold_queue.iter_mut())
                {
                    if hit(item) {
                        *item = item.blinded(&blind_text);
                    }
                }
            }
            ChatMessageMutation::PurgeUser { user_id } => {
                self.messages.remove_where(|i| i.user_id == user_id);
                self.retain_pending(|i| i.user_id != user_id);
                self.chat_history.retain(|i| i.user_id != user_id);
                self.replay_hold_queue.retain(|i| i.user_id != user_id);
            }
            ChatMessageMutation::Remove { message_id } => {
                self.messages.remove_where(|i| i.id == message_id);
                self.retain_pending(|i| i.id != message_id);
                self.chat_history.retain(|i| i.id != message_id);
                self.replay_hold_queue.retain(|i| i.id != message_id);
            }
        }
        if self.chat_history.is_empty() && self.has_chat_history {
            self.has_chat_history = false;
        }
        self.messages_revision = self.messages_revision.wrapping_add(1);
        self.moderation_revision = self.moderation_revision.wrapping_add(1);
    }

    /// 활성 구간(`pendingHead..`) 만 거른다 — 이미 소비한 접두부와 head 는 보존.
    fn retain_pending(&mut self, keep: impl Fn(&ChatMessageItem) -> bool) {
        if self.pending_head < self.pending_messages.len() {
            let kept: Vec<_> = self.pending_messages[self.pending_head..]
                .iter()
                .filter(|i| keep(i))
                .cloned()
                .collect();
            self.pending_messages.truncate(self.pending_head);
            self.pending_messages.extend(kept);
        }
    }
}
