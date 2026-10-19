//! 메타 이벤트(공지/블라인드/패널티/추방/세션ID/시스템) 파싱.
//! 원본: `Sources/CViewChat/ChatMessageParser+ParseMeta.swift`.

use chrono::Utc;
use serde_json::{Map, Value};

use super::parse_message::embedded_dictionary;
use super::{body_object, body_str, ChatMessageParser};
use crate::error::{ChatError, ChatResult};
use crate::json::{any_int, fnv1a, ns_object, ns_str};
use crate::protocol::ChatProtocolMessage;
use cview_core::models::chat::{ChatMessage, MessageType};

impl ChatMessageParser {
    /// cmd 94010 — `bdy` 는 객체 또는 단일 원소 배열 두 형태로 온다.
    pub(crate) fn parse_notice(&self, raw: &ChatProtocolMessage) -> ChatResult<ChatMessage> {
        let bdy = raw.bdy.as_ref().ok_or(ChatError::InvalidMessage)?;
        let dict: &Map<String, Value> = if let Some(d) = bdy.as_object() {
            d
        } else if let Some(first) = bdy
            .as_array()
            .and_then(|a| a.first())
            .and_then(Value::as_object)
        {
            first
        } else {
            return Err(ChatError::InvalidMessage);
        };

        let msg = extract_notice_text(dict);
        if msg.is_empty() {
            return Err(ChatError::InvalidMessage);
        }
        let extras = embedded_dictionary(dict.get("extras"));
        let profile = embedded_dictionary(dict.get("profile"));
        let register_profile = extras
            .as_ref()
            .and_then(|e| ns_object(e.get("registerProfile")));
        let nickname = [
            register_profile.and_then(|r| ns_str(r.get("nickname"))),
            extras.as_ref().and_then(|e| ns_str(e.get("nickname"))),
            profile.as_ref().and_then(|p| ns_str(p.get("nickname"))),
        ]
        .into_iter()
        .flatten()
        .find(|s| !s.is_empty())
        .unwrap_or("공지")
        .to_string();

        // 공지 id 는 재수신에도 같아야 dismiss 가 유지된다. 서버 id 우선, 없으면 nickname+content 해시.
        let notice_id = ["messageId", "msgId", "id", "noticeId"]
            .into_iter()
            .find_map(|key| body_str(dict, key).filter(|s| !s.is_empty()))
            .map(|sid| format!("notice_{sid}"))
            .unwrap_or_else(|| format!("notice_{:x}", fnv1a(&format!("{nickname}\u{1}{msg}"))));

        Ok(ChatMessage {
            id: notice_id,
            user_id: None,
            nickname,
            content: msg,
            timestamp: Utc::now(),
            r#type: MessageType::Notice,
            profile: None,
            extras: None,
            is_blinded: false,
        })
    }

    pub(crate) fn parse_blind(&self, raw: &ChatProtocolMessage) -> (String, String) {
        let Some(dict) = body_object(raw) else {
            return (String::new(), String::new());
        };
        (
            body_str(dict, "messageId").unwrap_or("").to_owned(),
            body_str(dict, "userId").unwrap_or("").to_owned(),
        )
    }

    pub(crate) fn parse_penalty(&self, raw: &ChatProtocolMessage) -> (String, i64) {
        let Some(dict) = body_object(raw) else {
            return (String::new(), 0);
        };
        (
            body_str(dict, "targetUserId").unwrap_or("").to_owned(),
            any_int(dict.get("duration")).unwrap_or(0),
        )
    }

    /// cmd 94005 의 대상 userId. 못 찾으면 빈 문자열 — 호출측은 "내 것이 아님"으로 폴백한다.
    pub(crate) fn parse_kick_target(&self, raw: &ChatProtocolMessage) -> String {
        let Some(dict) = body_object(raw) else {
            return String::new();
        };
        ["targetUserId", "userId", "uid", "kickUserId"]
            .into_iter()
            .find_map(|key| body_str(dict, key).filter(|s| !s.is_empty()))
            .unwrap_or("")
            .to_owned()
    }

    pub(crate) fn extract_session_id(&self, raw: &ChatProtocolMessage) -> String {
        body_object(raw)
            .and_then(|d| body_str(d, "sid"))
            .unwrap_or("")
            .to_owned()
    }

    pub(crate) fn extract_system_message(&self, raw: &ChatProtocolMessage) -> String {
        body_object(raw)
            .and_then(|d| body_str(d, "msg"))
            .unwrap_or("")
            .to_owned()
    }
}

/// cmd 94010 본문 — `msg` / `message` / `content` / `text` 및 중첩 `notice` 객체.
fn extract_notice_text(dict: &Map<String, Value>) -> String {
    for key in ["msg", "message", "content", "text"] {
        if let Some(s) = body_str(dict, key).map(str::trim).filter(|s| !s.is_empty()) {
            return s.to_owned();
        }
    }
    if let Some(nested) = dict.get("notice").and_then(Value::as_object) {
        return extract_notice_text(nested);
    }
    String::new()
}
