//! 멘션 감지와 부수효과(인박스·미읽음·알림).
//!
//! 원본: `Sources/CViewApp/ViewModels/ChatViewModel+Mention.swift`.

use std::collections::HashSet;

use chrono::Utc;

use cview_core::logic::mention_detector::{detect, MentionMatch};
use cview_core::models::chat::{ChatMessageItem, UserRole};

use super::{ChatState, MentionInboxEntry};

impl ChatState {
    pub const MENTION_MATCHES_CAP: usize = 4096;

    pub fn detect_mention(&mut self, item: &ChatMessageItem) {
        self.detect_mentions(std::slice::from_ref(item));
    }

    pub fn detect_mentions(&mut self, items: &[ChatMessageItem]) {
        if items.is_empty() {
            return;
        }
        let Some(key) = self.channel_preference_key().filter(|k| !k.is_empty()) else {
            return;
        };
        let prefs = self.deps.preferences.clone();
        let rule = prefs.current_mention_rule(self.local_user_nickname.as_deref());
        let has_text_matcher = !rule.muted_channels().contains(&key)
            && (rule.nickname().is_some_and(|n| !n.is_empty())
                || rule.keywords().iter().any(|k| !k.is_empty()));
        let mut detected: Vec<(&ChatMessageItem, HashSet<MentionMatch>)> = Vec::new();
        for item in items {
            let privileged = matches!(
                item.user_role,
                UserRole::Streamer | UserRole::Manager | UserRole::ChannelManager
            );
            let matches = if !has_text_matcher {
                if !privileged {
                    continue;
                }
                HashSet::from([MentionMatch::Streamer {}])
            } else {
                let m = detect(&item.content, privileged, &key, &rule);
                if m.is_empty() {
                    continue;
                }
                m
            };
            detected.push((item, matches));
        }
        if detected.is_empty() {
            return;
        }
        let mut new_ids = HashSet::new();
        for (item, _) in &detected {
            if !self.mention_matches.contains_key(&item.id) && new_ids.insert(item.id.clone()) {
                self.mention_matches_queue.push(item.id.clone());
            }
        }
        let overflow = self
            .mention_matches_queue
            .len()
            .saturating_sub(Self::MENTION_MATCHES_CAP);
        let evicted: Vec<String> = self.mention_matches_queue.drain(..overflow).collect();
        for (item, matches) in &detected {
            self.mention_matches
                .insert(item.id.clone(), matches.clone());
        }
        for id in evicted {
            self.mention_matches.remove(&id);
        }
        let display_channel_name = self.mention_channel_display_name();
        for (item, matches) in detected {
            let streamer_only = matches.len() == 1 && matches.contains(&MentionMatch::Streamer {});
            if streamer_only {
                continue;
            }
            let entry = MentionInboxEntry {
                id: item.id.clone(),
                stream_channel_id: key.clone(),
                chat_channel_id: self.active_chat_channel_id().unwrap_or_else(|| key.clone()),
                channel_name: display_channel_name.clone(),
                message: item.clone(),
                matches,
                received_at: Utc::now(),
            };
            self.deps.mention_sink.deliver(entry, self.multi_chat_scope);
            prefs.increment_mention_unread(&key, self.multi_chat_scope);
        }
    }
}
