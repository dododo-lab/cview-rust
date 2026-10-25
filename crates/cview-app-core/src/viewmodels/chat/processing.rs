//! 수신 메시지 전처리(모더레이션·이모티콘 보강·후원 표시 정책)와 재필터.
//!
//! 원본: `Sources/CViewApp/ViewModels/ChatViewModel+Processing.swift`.
//! Swift 는 16건 초과 배치를 분리 태스크로 넘겼다 — Rust 는 잠금 안에서 바로 처리한다.

use std::collections::{HashMap, HashSet};

use cview_core::models::chat::{ChatMessage, ChatMessageItem, EmojiMap, MessageType};
use cview_core::models::stream_alert::StreamAlertItem;

use super::{ChatEventSessionIdentity, ChatState};
use crate::viewmodels::support::cancel_slot;

/// Swift `StreamAlertItem(from: ChatMessageItem)` — 후원·구독만 알림이 된다.
pub fn stream_alert_from_item(item: &ChatMessageItem) -> Option<StreamAlertItem> {
    match item.r#type {
        MessageType::Donation => Some(StreamAlertItem::donation(
            item.nickname.clone(),
            item.content.clone(),
            item.donation_amount,
            item.donation_type.as_deref(),
        )),
        MessageType::Subscription => Some(StreamAlertItem::subscription(
            item.nickname.clone(),
            item.content.clone(),
            item.subscription_months,
        )),
        _ => None,
    }
}

impl ChatState {
    /// 반환: 커밋 대상으로 넘긴 항목(세션이 바뀌었으면 `None`).
    pub fn append_filtered_messages(
        &mut self,
        msgs: Vec<ChatMessage>,
        source: Option<&ChatEventSessionIdentity>,
        is_backlog: bool,
    ) -> Option<Vec<ChatMessageItem>> {
        if !self.is_current_event_session(source) {
            return None;
        }
        let t0 = std::time::Instant::now();
        let filtered = match (&self.moderation_service, self.has_active_moderation) {
            (Some(svc), true) => svc.filter_messages(&msgs),
            _ => msgs.clone(),
        };
        let raw: Vec<ChatMessageItem> = filtered
            .iter()
            .map(|m| {
                Self::enrich_with_channel_emoticons_pure(
                    &ChatMessageItem::from_message(m, false),
                    &self.channel_emoticons,
                )
            })
            .collect();
        let items =
            Self::filter_by_donation_prefs_pure(raw, self.show_donations_only, self.show_donation);
        self.collect_emoticons(&msgs);
        if !is_backlog {
            for item in items
                .iter()
                .filter(|i| i.r#type == MessageType::Subscription)
            {
                if let Some(alert) = stream_alert_from_item(item) {
                    self.enqueue_stream_alert(alert);
                }
            }
        }
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        if ms > 5.0 {
            tracing::warn!(
                "Slow chat preparation: wall_ms={ms:.1} msgs={} mod={} backlog={is_backlog}",
                msgs.len(),
                self.has_active_moderation
            );
        }
        if is_backlog {
            self.commit_backlog_messages(items.clone());
        } else {
            self.enqueue_batched_messages(items.clone());
        }
        Some(items)
    }

    pub fn collect_emoticons(&mut self, msgs: &[ChatMessage]) {
        let mut new_urls = Vec::new();
        for msg in msgs {
            let Some(emojis) = msg.extras.as_ref().and_then(|e| e.emojis.as_ref()) else {
                continue;
            };
            for (id, url) in emojis {
                if !self.collected_emoticon_index.contains_key(id) && url::Url::parse(url).is_ok() {
                    self.collected_emoticon_index
                        .insert(id.clone(), self.collected_emoticons.len());
                    self.collected_emoticons.push((id.clone(), url.clone()));
                    new_urls.push(url.clone());
                }
            }
        }
        if !new_urls.is_empty() {
            self.deps.prefetcher.prefetch(new_urls.clone());
        }
        if self.collected_emoticons.len() > 4000 {
            let keep = self
                .collected_emoticons
                .split_off(self.collected_emoticons.len() - 2000);
            self.collected_emoticons = keep;
            self.rebuild_collected_index();
        }
        if !new_urls.is_empty() {
            self.invalidate_emoticon_autocomplete_search_index();
        }
    }

    pub(crate) fn rebuild_collected_index(&mut self) {
        self.collected_emoticon_index = self
            .collected_emoticons
            .iter()
            .enumerate()
            .map(|(i, (id, _))| (id.clone(), i))
            .collect();
    }

    pub fn prefetch_emoticon_images(&self) {
        const PREFETCH_LIMIT: usize = 120;
        let all: Vec<String> = self
            .emoticon_packs
            .iter()
            .flat_map(|p| p.emoticons.iter().flatten())
            .filter_map(|e| e.image_url.clone())
            .collect();
        if all.is_empty() {
            return;
        }
        self.deps
            .prefetcher
            .prefetch(all.into_iter().take(PREFETCH_LIMIT).collect());
    }

    /// 보낼 때 실을 이모티콘 표 — 채널 → 팩 → 수집 순.
    pub fn outgoing_emoticon_lookup(&self) -> HashMap<String, String> {
        let mut map = self.channel_emoticons.clone();
        for pack in &self.emoticon_packs {
            for emo in pack.emoticons.iter().flatten() {
                if let Some(url) = &emo.image_url {
                    map.entry(emo.emoticon_id.clone())
                        .or_insert_with(|| url.clone());
                }
            }
        }
        for (id, url) in &self.collected_emoticons {
            map.entry(id.clone()).or_insert_with(|| url.clone());
        }
        map
    }

    pub fn resolve_outgoing_emojis(text: &str, lookup: &HashMap<String, String>) -> EmojiMap {
        let mut used = EmojiMap::new();
        if !text.contains("{:") || lookup.is_empty() {
            return used;
        }
        for segment in text.split("{:").skip(1) {
            let Some(end) = segment.find(":}") else {
                continue;
            };
            let id = &segment[..end];
            if id.is_empty() {
                continue;
            }
            if let Some(url) = lookup.get(id) {
                used.insert(id.to_string(), url.clone());
            }
        }
        used
    }

    pub fn filter_by_donation_prefs_pure(
        items: Vec<ChatMessageItem>,
        donations_only: bool,
        show_donation: bool,
    ) -> Vec<ChatMessageItem> {
        if donations_only {
            items
                .into_iter()
                .filter(|i| i.r#type == MessageType::Donation)
                .collect()
        } else if !show_donation {
            items
                .into_iter()
                .filter(|i| i.r#type != MessageType::Donation)
                .collect()
        } else {
            items
        }
    }

    pub fn enrich_with_channel_emoticons_pure(
        item: &ChatMessageItem,
        channel_emoticons: &HashMap<String, String>,
    ) -> ChatMessageItem {
        if channel_emoticons.is_empty() || !item.content.contains("{:") {
            return item.clone();
        }
        // 본문에 쓰인 `{:id:}` 만 채운다 — 채널 맵(수백 개)을 메시지마다 통째로 붙이면 화면으로 가는 이벤트가 불어난다.
        let mut merged = item.emojis.clone();
        for segment in item.content.split("{:").skip(1) {
            let Some(end) = segment.find(":}") else {
                continue;
            };
            let id = &segment[..end];
            if merged.contains_key(id) {
                continue;
            }
            if let Some(url) = channel_emoticons.get(id) {
                merged.insert(id.to_string(), url.clone());
            }
        }
        if merged.len() == item.emojis.len() {
            return item.clone();
        }
        item.with_emojis(merged)
    }

    /// 엔진 메시지로 재필터 (Swift `refilterMessages()`).
    pub fn refilter_messages(&mut self) {
        self.message_filter_generation = self.message_filter_generation.wrapping_add(1);
        self.refilter_generation = self.refilter_generation.wrapping_add(1);
        let Some(engine) = self.chat_engine.clone() else {
            return;
        };
        let all = engine.messages();
        self.refilter_messages_inner(all);
    }

    /// 주어진 서버 메시지로 재필터 (Swift `refilterMessages(using:)`).
    pub fn refilter_messages_using(&mut self, all_messages: Vec<ChatMessage>) {
        self.message_filter_generation = self.message_filter_generation.wrapping_add(1);
        self.refilter_generation = self.refilter_generation.wrapping_add(1);
        self.refilter_messages_inner(all_messages);
    }

    fn should_preserve_during_refilter(
        item: &ChatMessageItem,
        current_user_id: Option<&str>,
    ) -> bool {
        item.is_system
            || item.r#type == MessageType::SystemMessage
            || item.is_blinded
            || current_user_id.is_some_and(|u| item.user_id == u)
    }

    fn moderation_surrogate(item: &ChatMessageItem) -> ChatMessage {
        ChatMessage::with_id(item.id.clone(), item.nickname.clone(), item.content.clone())
            .user_id(item.user_id.clone())
            .timestamp(item.timestamp)
            .message_type(item.r#type)
            .blinded(item.is_blinded)
    }

    fn merge_refiltered_items(
        existing: Vec<ChatMessageItem>,
        additions: Vec<ChatMessageItem>,
    ) -> Vec<ChatMessageItem> {
        let mut seen = HashSet::new();
        let mut indexed: Vec<(usize, ChatMessageItem)> = Vec::new();
        for item in existing.into_iter().chain(additions) {
            if seen.insert(item.id.clone()) {
                indexed.push((indexed.len(), item));
            }
        }
        indexed.sort_by(|a, b| a.1.timestamp.cmp(&b.1.timestamp).then(a.0.cmp(&b.0)));
        indexed.into_iter().map(|(_, i)| i).collect()
    }

    fn refilter_messages_inner(&mut self, all_messages: Vec<ChatMessage>) {
        let Some(mod_service) = self.moderation_service.clone() else {
            return;
        };
        let current_user_id = self.current_user_uid.clone();
        let cu = current_user_id.as_deref();
        let donations_only = self.show_donations_only;
        let show_don = self.show_donation;

        let current_pending: Vec<ChatMessageItem> =
            if self.pending_head < self.pending_messages.len() {
                self.pending_messages[self.pending_head..].to_vec()
            } else {
                vec![]
            };
        let current_visible = self.messages.to_vec();
        let current_history = self.chat_history.clone();
        let current_hold = self.replay_hold_queue.clone();

        let mut candidates = all_messages.clone();
        let mut known_ids: HashSet<String> = all_messages.iter().map(|m| m.id.clone()).collect();
        for item in current_visible
            .iter()
            .chain(&current_history)
            .chain(&current_pending)
            .chain(&current_hold)
        {
            if !Self::should_preserve_during_refilter(item, cu) && known_ids.insert(item.id.clone())
            {
                candidates.push(Self::moderation_surrogate(item));
            }
        }
        let allowed: HashSet<String> = mod_service
            .filter_messages(&candidates)
            .into_iter()
            .map(|m| m.id)
            .collect();

        let blinded_ids: HashSet<String> = current_visible
            .iter()
            .chain(&current_history)
            .chain(&current_pending)
            .chain(&current_hold)
            .filter(|i| i.is_blinded)
            .map(|i| i.id.clone())
            .collect();

        let passes_display = |item: &ChatMessageItem| -> bool {
            if Self::should_preserve_during_refilter(item, cu) {
                return true;
            }
            if donations_only {
                return item.r#type == MessageType::Donation;
            }
            if !show_don {
                return item.r#type != MessageType::Donation;
            }
            true
        };
        let keep_existing = |item: &ChatMessageItem| -> bool {
            if Self::should_preserve_during_refilter(item, cu) {
                return true;
            }
            if !known_ids.contains(&item.id) {
                return true;
            }
            allowed.contains(&item.id) && passes_display(item)
        };
        let retained = |v: &[ChatMessageItem]| -> Vec<ChatMessageItem> {
            v.iter().filter(|i| keep_existing(i)).cloned().collect()
        };
        let retained_pending = retained(&current_pending);
        let retained_hold = retained(&current_hold);
        let retained_history = retained(&current_history);
        let retained_visible = retained(&current_visible);

        let server_items: Vec<ChatMessageItem> = all_messages
            .iter()
            .filter_map(|m| {
                let mut item = Self::enrich_with_channel_emoticons_pure(
                    &ChatMessageItem::from_message(m, false),
                    &self.channel_emoticons,
                );
                if blinded_ids.contains(&item.id) && !item.is_blinded {
                    item = item.blinded(ChatMessageItem::ADMIN_BLIND_TEXT);
                }
                let preserved = Self::should_preserve_during_refilter(&item, cu);
                if !(preserved || allowed.contains(&item.id)) {
                    return None;
                }
                passes_display(&item).then_some(item)
            })
            .collect();
        let mut server_by_id: HashMap<String, ChatMessageItem> = HashMap::new();
        for i in &server_items {
            server_by_id
                .entry(i.id.clone())
                .or_insert_with(|| i.clone());
        }
        let reconcile = |items: Vec<ChatMessageItem>| -> Vec<ChatMessageItem> {
            items
                .into_iter()
                .map(|item| {
                    if Self::should_preserve_during_refilter(&item, cu) {
                        return item;
                    }
                    server_by_id.get(&item.id).cloned().unwrap_or(item)
                })
                .collect()
        };
        let filtered_pending = reconcile(retained_pending);
        let filtered_hold = reconcile(retained_hold);
        let filtered_history = reconcile(retained_history);
        let filtered_visible = reconcile(retained_visible);

        let pending_ids: HashSet<&str> = filtered_pending.iter().map(|i| i.id.as_str()).collect();
        let hold_ids: HashSet<&str> = filtered_hold.iter().map(|i| i.id.as_str()).collect();
        let history_additions: Vec<_> = server_items
            .iter()
            .filter(|i| !pending_ids.contains(i.id.as_str()))
            .cloned()
            .collect();
        let visible_additions: Vec<_> = server_items
            .iter()
            .filter(|i| !pending_ids.contains(i.id.as_str()) && !hold_ids.contains(i.id.as_str()))
            .cloned()
            .collect();

        let mut new_history = Self::merge_refiltered_items(filtered_history, history_additions);
        let history_cap = if self.is_background_mode() {
            Self::BACKGROUND_HISTORY_SIZE
        } else {
            Self::MAX_HISTORY_SIZE
        };
        if new_history.len() > history_cap {
            let excess = new_history.len() - history_cap;
            new_history.drain(..excess);
        }
        let new_visible = Self::merge_refiltered_items(filtered_visible, visible_additions);

        cancel_slot(&mut self.batch_flush_task);
        let previous_pending_date = self.pending_first_enqueued_at;
        self.pending_first_enqueued_at = if filtered_pending.is_empty() {
            None
        } else {
            Some(previous_pending_date.unwrap_or_else(chrono::Utc::now))
        };
        self.pending_messages = filtered_pending;
        self.pending_head = 0;
        self.replay_hold_queue = filtered_hold;
        self.has_chat_history = !new_history.is_empty();
        self.chat_history = new_history;
        self.mutate_messages(|m| m.replace_all(new_visible));
        self.last_read_evicted = false;
        for m in &all_messages {
            if !m.id.is_empty() && self.seen_message_ids.insert(m.id.clone()) {
                self.seen_message_id_queue.push(m.id.clone());
            }
        }
        self.trim_seen_ids();
        if self.pending_count() > 0 {
            self.schedule_batch_flush();
        }
    }
}
