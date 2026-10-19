//! 프런트엔드로 보낼 직렬화 스냅샷 (Swift 원본 없음 — `@Observable` 프로퍼티 대체).

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Serialize;

use cview_core::models::chat::{
    ChatBadge, ChatMessageItem, ChatSystemNoticeKind, DonationRankItem, EmojiMap, MessageType,
    UserRole,
};
use cview_core::models::settings::{ChatDisplayMode, ChatMessageAlignment};
use cview_core::models::stream_alert::{StreamAlertItem, StreamAlertType};

use super::{
    AutocompleteTrigger, ChatRestrictionReason, ChatState, EmoticonSuggestion, MentionSuggestion,
};

/// 채팅 행.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRow {
    pub id: String,
    pub user_id: String,
    pub nickname: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    #[serde(rename = "type")]
    pub kind: MessageType,
    pub badge_image_url: Option<String>,
    pub emojis: EmojiMap,
    pub donation_amount: Option<i64>,
    pub donation_type: Option<String>,
    pub subscription_months: Option<i64>,
    pub profile_image_url: Option<String>,
    pub is_notice: bool,
    pub is_system: bool,
    pub user_role: UserRole,
    pub badges: Vec<ChatBadge>,
    pub title_name: Option<String>,
    pub title_color: Option<String>,
    pub is_blinded: bool,
    pub system_notice_kind: ChatSystemNoticeKind,
}

impl From<&ChatMessageItem> for ChatRow {
    fn from(i: &ChatMessageItem) -> Self {
        Self {
            id: i.id.clone(),
            user_id: i.user_id.clone(),
            nickname: i.nickname.clone(),
            content: i.content.clone(),
            timestamp: i.timestamp,
            kind: i.r#type,
            badge_image_url: i.badge_image_url.clone(),
            emojis: i.emojis.clone(),
            donation_amount: i.donation_amount,
            donation_type: i.donation_type.clone(),
            subscription_months: i.subscription_months,
            profile_image_url: i.profile_image_url.clone(),
            is_notice: i.is_notice,
            is_system: i.is_system,
            user_role: i.user_role,
            badges: i.badges.clone(),
            title_name: i.title_name.clone(),
            title_color: i.title_color.clone(),
            is_blinded: i.is_blinded,
            system_notice_kind: i.system_notice_kind,
        }
    }
}

/// 스트림 알림.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamAlertRow {
    pub id: String,
    pub alert_type: StreamAlertType,
    pub nickname: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub donation_amount: Option<i64>,
    pub donation_type: Option<String>,
    pub subscription_months: Option<i64>,
}

impl From<&StreamAlertItem> for StreamAlertRow {
    fn from(a: &StreamAlertItem) -> Self {
        Self {
            id: a.id.clone(),
            alert_type: a.alert_type,
            nickname: a.nickname.clone(),
            content: a.content.clone(),
            timestamp: a.timestamp,
            donation_amount: a.donation_amount,
            donation_type: a.donation_type.clone(),
            subscription_months: a.subscription_months,
        }
    }
}

/// ChatViewModel 관찰 상태.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSnapshot {
    pub messages: Vec<ChatRow>,
    pub messages_revision: u64,
    pub connection_state: String,
    pub is_connected: bool,
    pub input_text: String,
    pub is_auto_scroll_enabled: bool,
    pub show_donations_only: bool,
    pub font_size: f64,
    pub opacity: f64,
    pub show_timestamp: bool,
    pub show_badge: bool,
    pub line_spacing: f64,
    pub highlight_mentions: bool,
    pub emoticon_enabled: bool,
    pub show_donation: bool,
    pub display_mode: ChatDisplayMode,
    pub message_alignment: ChatMessageAlignment,
    pub overlay_width: f64,
    pub overlay_height: f64,
    pub overlay_background_opacity: f64,
    pub overlay_show_input: bool,
    pub max_visible_messages: usize,
    pub message_count: i64,
    pub messages_per_second: f64,
    pub unread_while_background: i64,
    pub has_chat_history: bool,
    pub is_replay_mode: bool,
    pub unread_count: i64,
    pub pending_scroll_message_id: Option<String>,
    pub pinned_message: Option<ChatRow>,
    pub streamer_notice: Option<ChatRow>,
    pub is_filter_enabled: bool,
    pub mention_message_ids: Vec<String>,
    pub weekly_donation_ranking: Vec<DonationRankItem>,
    pub my_weekly_donation_rank: Option<DonationRankItem>,
    pub live_viewer_count: i64,
    pub live_accumulate_count: i64,
    pub live_open_date: Option<DateTime<Utc>>,
    pub emoticon_suggestions: Vec<EmoticonSuggestion>,
    pub mention_suggestions: Vec<MentionSuggestion>,
    pub autocomplete_selected_index: usize,
    pub autocomplete_trigger: AutocompleteTrigger,
    pub stream_alerts: Vec<StreamAlertRow>,
    pub error_message: Option<String>,
    pub chat_restriction: Option<ChatRestrictionReason>,
    pub can_send_chat: bool,
    pub unique_user_count: i64,
    pub unique_user_count_display: String,
    pub donation_count: i64,
    pub total_donation_amount: i64,
    pub subscription_count: i64,
    pub show_export_sheet: bool,
    pub highlighted_users: Vec<String>,
    pub blocked_words: Vec<String>,
    pub channel_emoticons: BTreeMap<String, String>,
}

impl ChatSnapshot {
    pub fn from_state(s: &ChatState) -> Self {
        let mut mention_ids: Vec<String> = s.mention_matches.keys().cloned().collect();
        mention_ids.sort();
        let mut highlighted: Vec<String> = s.highlighted_users.iter().cloned().collect();
        highlighted.sort();
        Self {
            messages: s.messages.iter().map(ChatRow::from).collect(),
            messages_revision: s.messages_revision,
            connection_state: s.connection_state.display_text(),
            is_connected: s.connection_state.is_connected(),
            input_text: s.input_text.clone(),
            is_auto_scroll_enabled: s.is_auto_scroll_enabled,
            show_donations_only: s.show_donations_only,
            font_size: s.font_size,
            opacity: s.opacity,
            show_timestamp: s.show_timestamp,
            show_badge: s.show_badge,
            line_spacing: s.line_spacing,
            highlight_mentions: s.highlight_mentions,
            emoticon_enabled: s.emoticon_enabled,
            show_donation: s.show_donation,
            display_mode: s.display_mode,
            message_alignment: s.message_alignment,
            overlay_width: s.overlay_width,
            overlay_height: s.overlay_height,
            overlay_background_opacity: s.overlay_background_opacity,
            overlay_show_input: s.overlay_show_input,
            max_visible_messages: s.max_visible_messages,
            message_count: s.message_count,
            messages_per_second: s.messages_per_second,
            unread_while_background: s.unread_while_background,
            has_chat_history: s.has_chat_history,
            is_replay_mode: s.is_replay_mode,
            unread_count: s.unread_count,
            pending_scroll_message_id: s.pending_scroll_message_id.clone(),
            pinned_message: s.pinned_message.as_ref().map(ChatRow::from),
            streamer_notice: s.streamer_notice.as_ref().map(ChatRow::from),
            is_filter_enabled: s.is_filter_enabled,
            mention_message_ids: mention_ids,
            weekly_donation_ranking: s.weekly_donation_ranking.clone(),
            my_weekly_donation_rank: s.my_weekly_donation_rank.clone(),
            live_viewer_count: s.live_viewer_count,
            live_accumulate_count: s.live_accumulate_count,
            live_open_date: s.live_open_date,
            emoticon_suggestions: s.emoticon_suggestions.clone(),
            mention_suggestions: s.mention_suggestions.clone(),
            autocomplete_selected_index: s.autocomplete_selected_index,
            autocomplete_trigger: s.autocomplete_trigger.clone(),
            stream_alerts: s.stream_alerts.iter().map(StreamAlertRow::from).collect(),
            error_message: s.error_message.clone(),
            chat_restriction: s.chat_restriction,
            can_send_chat: s.can_send_chat(),
            unique_user_count: s.unique_user_count,
            unique_user_count_display: s.unique_user_count_display(),
            donation_count: s.donation_count,
            total_donation_amount: s.total_donation_amount,
            subscription_count: s.subscription_count,
            show_export_sheet: s.show_export_sheet,
            highlighted_users: highlighted,
            blocked_words: s.blocked_words.clone(),
            channel_emoticons: s
                .channel_emoticons
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        }
    }
}
