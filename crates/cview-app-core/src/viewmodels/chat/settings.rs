//! `ChatSettings` 값 타입.
//!
//! 원본: `Sources/CViewCore/Models/SettingsModels+Chat.swift` 의 `ChatSettings`.
//! Rust `cview-core` 에 아직 없어서(`cview-persistence` 는 불투명 JSON 으로 보관) 뷰모델이 쓰는
//! 형태로 여기 둔다. JSON 키·기본값·클램프 규칙은 Swift 디코더와 같다. `background` 는
//! `ChatBackgroundSettings`(UI 전용) 라 불투명 JSON 으로 왕복한다.

use serde::{Deserialize, Deserializer, Serialize};

use cview_chat::utils::chat_message_capacity_policy;
use cview_core::models::settings::{ChatDesignStyle, ChatDisplayMode, ChatMessageAlignment};

/// 채팅 설정.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSettings {
    pub font_size: f64,
    pub chat_opacity: f64,
    pub line_spacing: f64,
    pub show_timestamp: bool,
    pub show_badge: bool,
    pub highlight_mentions: bool,
    pub highlight_roles: bool,
    pub max_visible_messages: i64,
    pub max_update_rate: i64,
    pub emoticon_enabled: bool,
    pub show_donation: bool,
    pub show_donations_only: bool,
    pub auto_scroll: bool,
    pub chat_filter_enabled: bool,
    pub blocked_words: Vec<String>,
    pub blocked_users: Vec<String>,
    pub tts_enabled: bool,
    pub tts_volume: f32,
    pub tts_rate: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tts_voice_identifier: Option<String>,
    pub display_mode: ChatDisplayMode,
    pub design_style: ChatDesignStyle,
    pub background: serde_json::Value,
    pub message_alignment: ChatMessageAlignment,
    pub overlay_width: f64,
    pub overlay_height: f64,
    pub overlay_background_opacity: f64,
    pub overlay_show_input: bool,
}

impl Default for ChatSettings {
    fn default() -> Self {
        Self {
            font_size: 14.0,
            chat_opacity: 1.0,
            line_spacing: 2.0,
            show_timestamp: true,
            show_badge: true,
            highlight_mentions: true,
            highlight_roles: true,
            max_visible_messages: chat_message_capacity_policy::DEFAULT as i64,
            max_update_rate: 30,
            emoticon_enabled: true,
            show_donation: true,
            show_donations_only: false,
            auto_scroll: true,
            chat_filter_enabled: false,
            blocked_words: vec![],
            blocked_users: vec![],
            tts_enabled: false,
            tts_volume: 0.8,
            tts_rate: 200.0,
            tts_voice_identifier: None,
            display_mode: ChatDisplayMode::Side,
            design_style: ChatDesignStyle::Unified,
            background: serde_json::Value::Object(Default::default()),
            message_alignment: ChatMessageAlignment::Auto,
            overlay_width: 340.0,
            overlay_height: 400.0,
            overlay_background_opacity: 0.5,
            overlay_show_input: false,
        }
    }
}

impl ChatSettings {
    /// Swift init 의 클램프(`maxUpdateRate` 10…60, `maxVisibleMessages` 용량 정책).
    pub fn normalized(mut self) -> Self {
        self.max_update_rate = self.max_update_rate.clamp(10, 60);
        self.max_visible_messages =
            chat_message_capacity_policy::clamped(self.max_visible_messages) as i64;
        self
    }
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Raw {
    font_size: Option<f64>,
    chat_opacity: Option<f64>,
    line_spacing: Option<f64>,
    show_timestamp: Option<bool>,
    show_badge: Option<bool>,
    highlight_mentions: Option<bool>,
    highlight_roles: Option<bool>,
    max_visible_messages: Option<i64>,
    max_update_rate: Option<i64>,
    emoticon_enabled: Option<bool>,
    show_donation: Option<bool>,
    show_donations_only: Option<bool>,
    auto_scroll: Option<bool>,
    chat_filter_enabled: Option<bool>,
    blocked_words: Option<Vec<String>>,
    blocked_users: Option<Vec<String>>,
    tts_enabled: Option<bool>,
    tts_volume: Option<f32>,
    tts_rate: Option<f32>,
    tts_voice_identifier: Option<String>,
    display_mode: Option<ChatDisplayMode>,
    design_style: Option<String>,
    background: Option<serde_json::Value>,
    message_alignment: Option<ChatMessageAlignment>,
    overlay_width: Option<f64>,
    overlay_height: Option<f64>,
    overlay_background_opacity: Option<f64>,
    overlay_show_input: Option<bool>,
}

impl<'de> Deserialize<'de> for ChatSettings {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let r = Raw::deserialize(d)?;
        let f = ChatSettings::default();
        Ok(ChatSettings {
            font_size: r.font_size.unwrap_or(f.font_size),
            chat_opacity: r.chat_opacity.unwrap_or(f.chat_opacity),
            line_spacing: r.line_spacing.unwrap_or(f.line_spacing),
            show_timestamp: r.show_timestamp.unwrap_or(f.show_timestamp),
            show_badge: r.show_badge.unwrap_or(f.show_badge),
            highlight_mentions: r.highlight_mentions.unwrap_or(f.highlight_mentions),
            highlight_roles: r.highlight_roles.unwrap_or(f.highlight_roles),
            max_visible_messages: r.max_visible_messages.unwrap_or(f.max_visible_messages),
            max_update_rate: r.max_update_rate.unwrap_or(f.max_update_rate),
            emoticon_enabled: r.emoticon_enabled.unwrap_or(f.emoticon_enabled),
            show_donation: r.show_donation.unwrap_or(f.show_donation),
            show_donations_only: r.show_donations_only.unwrap_or(f.show_donations_only),
            auto_scroll: r.auto_scroll.unwrap_or(f.auto_scroll),
            chat_filter_enabled: r.chat_filter_enabled.unwrap_or(f.chat_filter_enabled),
            blocked_words: r.blocked_words.unwrap_or_default(),
            blocked_users: r.blocked_users.unwrap_or_default(),
            tts_enabled: r.tts_enabled.unwrap_or(false),
            tts_volume: r.tts_volume.unwrap_or(0.8),
            tts_rate: r.tts_rate.unwrap_or(200.0),
            tts_voice_identifier: r.tts_voice_identifier,
            display_mode: r.display_mode.unwrap_or_default(),
            design_style: ChatDesignStyle::from_saved(r.design_style.as_deref()),
            background: r.background.unwrap_or(f.background),
            message_alignment: r.message_alignment.unwrap_or_default(),
            overlay_width: r.overlay_width.unwrap_or(340.0),
            overlay_height: r.overlay_height.unwrap_or(400.0),
            overlay_background_opacity: r.overlay_background_opacity.unwrap_or(0.5),
            overlay_show_input: r.overlay_show_input.unwrap_or(false),
        }
        .normalized())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_defaults_and_clamps() {
        let s: ChatSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, ChatSettings::default());
        let s: ChatSettings =
            serde_json::from_str(r#"{"maxUpdateRate":500,"designStyle":"calm"}"#).unwrap();
        assert_eq!(s.max_update_rate, 60);
        assert_eq!(s.design_style, ChatDesignStyle::Unified);
        let round: ChatSettings =
            serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
        assert_eq!(round, s);
    }
}
