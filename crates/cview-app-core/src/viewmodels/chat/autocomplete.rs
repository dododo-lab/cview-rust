//! `:이모티콘` · `@멘션` 자동완성.
//!
//! 원본: `Sources/CViewApp/ViewModels/ChatViewModel+Autocomplete.swift`,
//! `Sources/CViewCore/Models/AutocompleteModels.swift`.
//! Swift `Range<String.Index>` 는 UTF-8 바이트 범위(`std::ops::Range<usize>`)로 옮겼다.
//! 커서 오프셋은 Swift 와 같이 **문자(char) 단위**다.

use std::collections::HashSet;
use std::ops::Range;
use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;

use cview_core::models::chat::{ChatMessageItem, EmoticonItem};

use super::ChatState;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmoticonSuggestion {
    pub id: String,
    pub emoticon_id: String,
    pub display_name: String,
    pub image_url: Option<String>,
    pub chat_pattern: String,
}

impl EmoticonSuggestion {
    pub fn new(
        emoticon_id: String,
        display_name: String,
        image_url: Option<String>,
        chat_pattern: String,
    ) -> Self {
        Self {
            id: emoticon_id.clone(),
            emoticon_id,
            display_name,
            image_url,
            chat_pattern,
        }
    }

    pub fn from_item(item: &EmoticonItem) -> Self {
        Self::new(
            item.emoticon_id.clone(),
            item.emoticon_name
                .clone()
                .unwrap_or_else(|| item.emoticon_id.clone()),
            item.image_url.clone(),
            item.chat_pattern(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MentionSuggestion {
    pub id: String,
    pub user_id: String,
    pub nickname: String,
    pub profile_image_url: Option<String>,
}

impl MentionSuggestion {
    pub fn new(
        user_id: impl Into<String>,
        nickname: impl Into<String>,
        profile_image_url: Option<String>,
    ) -> Self {
        let user_id = user_id.into();
        Self {
            id: user_id.clone(),
            user_id,
            nickname: nickname.into(),
            profile_image_url,
        }
    }
}

/// 자동완성 트리거. `range` 는 입력 문자열의 바이트 범위.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AutocompleteTrigger {
    #[default]
    None,
    Emoticon {
        query: String,
        range: Range<usize>,
    },
    Mention {
        query: String,
        range: Range<usize>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmoticonAutocompleteSearchEntry {
    pub suggestion: EmoticonSuggestion,
    pub normalized_display_name: String,
    pub normalized_emoticon_id: String,
}

fn emoji_trigger_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r":[a-zA-Z0-9_가-힣]{1,20}$").unwrap())
}

fn mention_trigger_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"@[a-zA-Z0-9_가-힣]{0,20}$").unwrap())
}

impl ChatState {
    pub const AUTOCOMPLETE_SUGGESTION_LIMIT: usize = 8;

    pub fn track_recent_chatters(&mut self, items: &[ChatMessageItem]) {
        let mut seen = HashSet::new();
        const TOP_ZONE: usize = 16;
        let scan_end = TOP_ZONE.min(self.recent_chatters.len());
        let me = self.current_user_uid.clone().unwrap_or_default();
        for item in items {
            if item.is_system || item.user_id == "system" || item.user_id == me {
                continue;
            }
            let uid = &item.user_id;
            if !seen.insert(uid.clone()) {
                continue;
            }
            let limit = scan_end.min(self.recent_chatters.len());
            if self.recent_chatters[..limit]
                .iter()
                .any(|c| &c.user_id == uid)
            {
                continue;
            }
            self.recent_chatters.retain(|c| &c.user_id != uid);
            self.recent_chatters.insert(
                0,
                MentionSuggestion::new(
                    uid.clone(),
                    item.nickname.clone(),
                    item.profile_image_url.clone(),
                ),
            );
        }
        self.recent_chatters
            .truncate(Self::MAX_RECENT_CHATTERS_COUNT);
    }

    pub fn update_autocomplete_suggestions(&mut self, text: &str, cursor_offset: Option<usize>) {
        let cursor = cursor_offset.unwrap_or_else(|| text.chars().count());
        let trigger = Self::detect_autocomplete_trigger(text, cursor);
        match &trigger {
            AutocompleteTrigger::None => {
                self.publish_autocomplete_state(AutocompleteTrigger::None, vec![], vec![])
            }
            AutocompleteTrigger::Emoticon { query, .. } => {
                let q = query.to_lowercase();
                self.rebuild_emoticon_autocomplete_search_index_if_needed();
                let matches = Self::matching_emoticon_suggestions(
                    &self.emoticon_autocomplete_search_index,
                    &q,
                    Self::AUTOCOMPLETE_SUGGESTION_LIMIT,
                );
                self.publish_autocomplete_state(trigger, matches, vec![]);
            }
            AutocompleteTrigger::Mention { query, .. } => {
                let q = query.to_lowercase();
                let matches = if q.is_empty() {
                    self.recent_chatters
                        .iter()
                        .take(Self::AUTOCOMPLETE_SUGGESTION_LIMIT)
                        .cloned()
                        .collect()
                } else {
                    Self::matching_mention_suggestions(
                        &self.recent_chatters,
                        &q,
                        Self::AUTOCOMPLETE_SUGGESTION_LIMIT,
                    )
                };
                self.publish_autocomplete_state(trigger, vec![], matches);
            }
        }
    }

    pub fn apply_autocompletion(&mut self, text: &str, selected_index: usize) -> Option<String> {
        let (range, replacement) = match &self.autocomplete_trigger {
            AutocompleteTrigger::Emoticon { range, .. } => {
                let s = self.emoticon_suggestions.get(selected_index)?;
                (range.clone(), s.chat_pattern.clone())
            }
            AutocompleteTrigger::Mention { range, .. } => {
                let s = self.mention_suggestions.get(selected_index)?;
                (range.clone(), format!("@{} ", s.nickname))
            }
            AutocompleteTrigger::None => return None,
        };
        if range.end > text.len()
            || !text.is_char_boundary(range.start)
            || !text.is_char_boundary(range.end)
        {
            return None;
        }
        let mut result = text.to_string();
        result.replace_range(range, &replacement);
        self.dismiss_autocomplete();
        Some(result)
    }

    pub fn dismiss_autocomplete(&mut self) {
        self.publish_autocomplete_state(AutocompleteTrigger::None, vec![], vec![]);
    }

    pub fn move_autocomplete_selection(&mut self, delta: i64) {
        let count = if self.is_emoticon_autocomplete() {
            self.emoticon_suggestions.len()
        } else {
            self.mention_suggestions.len()
        } as i64;
        if count == 0 {
            return;
        }
        let next = (self.autocomplete_selected_index as i64 + delta).rem_euclid(count) as usize;
        self.autocomplete_selected_index = next;
    }

    pub fn is_emoticon_autocomplete(&self) -> bool {
        !self.emoticon_suggestions.is_empty()
    }

    fn publish_autocomplete_state(
        &mut self,
        trigger: AutocompleteTrigger,
        emoticons: Vec<EmoticonSuggestion>,
        mentions: Vec<MentionSuggestion>,
    ) {
        self.autocomplete_trigger = trigger;
        self.emoticon_suggestions = emoticons;
        self.mention_suggestions = mentions;
        self.autocomplete_selected_index = 0;
    }

    pub fn invalidate_emoticon_autocomplete_search_index(&mut self) {
        if self.is_emoticon_autocomplete_search_index_dirty
            && self.emoticon_autocomplete_search_index.is_empty()
        {
            return;
        }
        self.is_emoticon_autocomplete_search_index_dirty = true;
        self.emoticon_autocomplete_search_index.clear();
    }

    pub fn rebuild_emoticon_autocomplete_search_index_if_needed(&mut self) {
        if !self.is_emoticon_autocomplete_search_index_dirty {
            return;
        }
        self.emoticon_autocomplete_search_index = self
            .gather_all_emoticons()
            .into_iter()
            .map(|s| EmoticonAutocompleteSearchEntry {
                normalized_display_name: s.display_name.to_lowercase(),
                normalized_emoticon_id: s.emoticon_id.to_lowercase(),
                suggestion: s,
            })
            .collect();
        self.is_emoticon_autocomplete_search_index_dirty = false;
        self.emoticon_autocomplete_search_index_rebuild_count = self
            .emoticon_autocomplete_search_index_rebuild_count
            .wrapping_add(1);
    }

    pub fn matching_emoticon_suggestions(
        entries: &[EmoticonAutocompleteSearchEntry],
        normalized_query: &str,
        limit: usize,
    ) -> Vec<EmoticonSuggestion> {
        entries
            .iter()
            .filter(|e| {
                e.normalized_display_name.contains(normalized_query)
                    || e.normalized_emoticon_id.contains(normalized_query)
            })
            .take(limit)
            .map(|e| e.suggestion.clone())
            .collect()
    }

    pub fn matching_mention_suggestions(
        entries: &[MentionSuggestion],
        normalized_query: &str,
        limit: usize,
    ) -> Vec<MentionSuggestion> {
        entries
            .iter()
            .filter(|e| e.nickname.to_lowercase().contains(normalized_query))
            .take(limit)
            .cloned()
            .collect()
    }

    /// `cursor_offset` 은 문자 단위.
    pub fn detect_autocomplete_trigger(text: &str, cursor_offset: usize) -> AutocompleteTrigger {
        if text.is_empty() || cursor_offset == 0 {
            return AutocompleteTrigger::None;
        }
        let char_count = text.chars().count();
        let safe = cursor_offset.min(char_count);
        let cursor_byte = text
            .char_indices()
            .nth(safe)
            .map(|(b, _)| b)
            .unwrap_or(text.len());
        let before = &text[..cursor_byte];
        if let Some(m) = emoji_trigger_regex().find(before) {
            let query = before[m.start() + 1..m.end()].to_string();
            return AutocompleteTrigger::Emoticon {
                query,
                range: m.start()..m.end(),
            };
        }
        if let Some(m) = mention_trigger_regex().find(before) {
            let query = before[m.start() + 1..m.end()].to_string();
            return AutocompleteTrigger::Mention {
                query,
                range: m.start()..m.end(),
            };
        }
        AutocompleteTrigger::None
    }

    pub fn gather_all_emoticons(&self) -> Vec<EmoticonSuggestion> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for pack in &self.emoticon_packs {
            for item in pack.emoticons.iter().flatten() {
                if seen.insert(item.emoticon_id.clone()) {
                    out.push(EmoticonSuggestion::from_item(item));
                }
            }
        }
        let mut channel: Vec<(&String, &String)> = self.channel_emoticons.iter().collect();
        channel.sort();
        for (id, url) in channel {
            if seen.insert(id.clone()) {
                out.push(EmoticonSuggestion::new(
                    id.clone(),
                    id.clone(),
                    url::Url::parse(url).ok().map(|_| url.clone()),
                    format!("{{:{id}:}}"),
                ));
            }
        }
        for (id, url) in &self.collected_emoticons {
            if seen.insert(id.clone()) {
                out.push(EmoticonSuggestion::new(
                    id.clone(),
                    id.clone(),
                    Some(url.clone()),
                    format!("{{:{id}:}}"),
                ));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_triggers() {
        assert_eq!(
            ChatState::detect_autocomplete_trigger("", 0),
            AutocompleteTrigger::None
        );
        match ChatState::detect_autocomplete_trigger("hi :sm", 6) {
            AutocompleteTrigger::Emoticon { query, range } => {
                assert_eq!(query, "sm");
                assert_eq!(range, 3..6);
            }
            other => panic!("{other:?}"),
        }
        match ChatState::detect_autocomplete_trigger("안녕 @도도", 6) {
            AutocompleteTrigger::Mention { query, .. } => assert_eq!(query, "도도"),
            other => panic!("{other:?}"),
        }
        match ChatState::detect_autocomplete_trigger("a @", 3) {
            AutocompleteTrigger::Mention { query, .. } => assert_eq!(query, ""),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            ChatState::detect_autocomplete_trigger("a :", 3),
            AutocompleteTrigger::None
        );
    }
}
