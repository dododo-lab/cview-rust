//! "여기까지 읽음" 구분선.
//!
//! 원본: `Sources/CViewApp/ViewModels/ChatViewModel+ReadDivider.swift`.

use super::ChatState;

impl ChatState {
    /// 구분선을 그 앞에 그릴 메시지 id. 마지막 읽음이 버퍼 밖으로 밀려났으면 한 번 판정 후 `None` 고정.
    pub fn read_divider_before_message_id(&mut self) -> Option<String> {
        let cid = self.channel_preference_key()?;
        if self.last_read_evicted {
            return None;
        }
        let last_read = match &self.cached_last_read_state {
            Some((key, id)) if *key == cid => id.clone(),
            _ => {
                let id = self
                    .deps
                    .preferences
                    .last_read_message_id(&cid, self.multi_chat_scope);
                self.cached_last_read_state = Some((cid.clone(), id.clone()));
                id
            }
        };
        let last_read = last_read?;
        if self.messages.last().is_some_and(|m| m.id == last_read) {
            return None;
        }
        let len = self.messages.len();
        for i in 0..len {
            if self.messages.get(i).is_some_and(|m| m.id == last_read) {
                return if i + 1 < len {
                    self.messages.get(i + 1).map(|m| m.id.clone())
                } else {
                    None
                };
            }
        }
        if self.messages.is_empty() || self.is_holding_for_backlog {
            return None;
        }
        self.last_read_evicted = true;
        None
    }

    pub fn note_last_read(&mut self, message_id: &str) {
        let Some(cid) = self.channel_preference_key() else {
            return;
        };
        self.deps
            .preferences
            .set_last_read_message_id(&cid, message_id, self.multi_chat_scope);
        self.cached_last_read_state = Some((cid, Some(message_id.to_string())));
        self.last_read_evicted = false;
    }
}
