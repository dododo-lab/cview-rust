//! 후원·구독 TTS 경계.
//!
//! 원본: `Sources/CViewApp/ViewModels/ChatTTSService.swift`. `AVSpeechSynthesizer` 재생은
//! 플랫폼 종속이라 옮기지 않고 trait 으로만 둔다. 읽을 문장 규칙([`format_tts_text`])과
//! 대기열 상한([`TtsQueue`], 5건)은 그대로 옮겨 플랫폼 구현이 재사용한다.

use std::collections::VecDeque;

use cview_core::models::chat::{ChatMessageItem, MessageType};

/// TTS 재생기.
pub trait ChatTts: Send + Sync {
    fn enqueue(&self, message: &ChatMessageItem);
    fn stop(&self);
    /// `isEnabled`·`volume`·`rate`·`voiceIdentifier` 반영. 켜져 있다가 꺼지면 `stop()`.
    fn configure(&self, enabled: bool, volume: f32, rate: f32, voice_identifier: Option<String>);
}

/// 아무것도 읽지 않는 기본 구현.
#[derive(Debug, Default)]
pub struct NoopTts;
impl ChatTts for NoopTts {
    fn enqueue(&self, _message: &ChatMessageItem) {}
    fn stop(&self) {}
    fn configure(&self, _e: bool, _v: f32, _r: f32, _id: Option<String>) {}
}

/// 읽을 문장. 후원·구독 외에는 `None`.
pub fn format_tts_text(message: &ChatMessageItem) -> Option<String> {
    match message.r#type {
        MessageType::Donation => {
            let amount = message.donation_amount?;
            let content = if message.content.is_empty() {
                String::new()
            } else {
                format!(". {}", message.content)
            };
            Some(format!(
                "{}님이 {}원 후원{}",
                message.nickname, amount, content
            ))
        }
        MessageType::Subscription => match message.subscription_months {
            Some(months) if months > 0 => {
                Some(format!("{}님이 {}개월 구독", message.nickname, months))
            }
            _ => Some(format!("{}님이 구독", message.nickname)),
        },
        _ => None,
    }
}

/// 재생 대기열 — 활성 상태에서만 받고, 5건이 차면 새 문장을 버린다.
#[derive(Debug, Default)]
pub struct TtsQueue {
    pub queue: VecDeque<String>,
    pub is_enabled: bool,
    pub is_speaking: bool,
}

impl TtsQueue {
    pub const MAX_QUEUE_SIZE: usize = 5;

    /// 받았으면 `true`.
    pub fn enqueue(&mut self, message: &ChatMessageItem) -> bool {
        if !self.is_enabled {
            return false;
        }
        let Some(text) = format_tts_text(message) else {
            return false;
        };
        if self.queue.len() >= Self::MAX_QUEUE_SIZE {
            return false;
        }
        self.queue.push_back(text);
        true
    }

    /// 쉬고 있으면 다음 문장을 꺼내 "재생 중" 으로 바꾼다.
    pub fn next_if_idle(&mut self) -> Option<String> {
        if self.is_speaking {
            return None;
        }
        let t = self.queue.pop_front()?;
        self.is_speaking = true;
        Some(t)
    }

    /// 재생 완료 콜백.
    pub fn did_finish(&mut self) -> Option<String> {
        self.is_speaking = false;
        self.next_if_idle()
    }

    pub fn stop(&mut self) {
        self.queue.clear();
        self.is_speaking = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use cview_core::models::chat::EmojiMap;

    fn msg(t: MessageType) -> ChatMessageItem {
        ChatMessageItem::new(
            "1",
            "u",
            "닉",
            "고마워요",
            Utc::now(),
            t,
            EmojiMap::new(),
            false,
            false,
        )
    }

    #[test]
    fn formats() {
        let mut d = msg(MessageType::Donation);
        assert_eq!(format_tts_text(&d), None);
        d.donation_amount = Some(1000);
        assert_eq!(format_tts_text(&d).unwrap(), "닉님이 1000원 후원. 고마워요");
        let mut s = msg(MessageType::Subscription);
        assert_eq!(format_tts_text(&s).unwrap(), "닉님이 구독");
        s.subscription_months = Some(3);
        assert_eq!(format_tts_text(&s).unwrap(), "닉님이 3개월 구독");
        assert_eq!(format_tts_text(&msg(MessageType::Normal)), None);
    }

    #[test]
    fn queue_caps_at_five() {
        let mut q = TtsQueue {
            is_enabled: true,
            ..Default::default()
        };
        let s = msg(MessageType::Subscription);
        for _ in 0..7 {
            q.enqueue(&s);
        }
        assert_eq!(q.queue.len(), 5);
        assert!(q.next_if_idle().is_some());
        assert!(q.next_if_idle().is_none());
        assert!(q.did_finish().is_some());
    }
}
