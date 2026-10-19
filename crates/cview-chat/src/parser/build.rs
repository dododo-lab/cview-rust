//! 송신 프레임 빌더. 원본: `Sources/CViewChat/ChatMessageParser+Build.swift`.
//!
//! 출력은 Swift `JSONSerialization(.sortedKeys)` 와 바이트 단위로 같다
//! ([`crate::json::to_foundation_json`]).

use serde_json::{json, Map, Value};

use super::ChatMessageParser;
use crate::constants::chat_defaults;
use crate::json::to_foundation_json;
use crate::protocol::ChzzkChatCommand;
use cview_core::models::chat::EmojiMap;

/// 현재 OS 의 `osVer` 문자열. macOS 에서는 Swift 와 같은 `macOS/<major>.<minor>.<patch>`.
pub fn default_os_version() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Some(v) = macos_product_version() {
            return format!("macOS/{v}");
        }
        "macOS/0.0.0".to_string()
    }
    #[cfg(not(target_os = "macos"))]
    {
        format!("{}/0.0.0", std::env::consts::OS)
    }
}

#[cfg(target_os = "macos")]
fn macos_product_version() -> Option<String> {
    let plist = std::fs::read_to_string("/System/Library/CoreServices/SystemVersion.plist").ok()?;
    let key = plist.find("<key>ProductVersion</key>")?;
    let rest = &plist[key..];
    let start = rest.find("<string>")? + "<string>".len();
    let end = rest[start..].find("</string>")? + start;
    let v = &rest[start..end];
    // `operatingSystemVersion` 은 항상 major.minor.patch 세 자리다.
    let mut parts: Vec<&str> = v.split('.').collect();
    while parts.len() < 3 {
        parts.push("0");
    }
    Some(parts[..3].join("."))
}

impl ChatMessageParser {
    /// cmd 100 (connect) 프레임.
    pub fn build_connect_message(&self, chat_channel_id: &str, access_token: &str, uid: Option<&str>) -> String {
        let body = json!({
            "ver": "3",
            "cmd": ChzzkChatCommand::Connect.raw_value(),
            "svcid": "game",
            "cid": chat_channel_id,
            "bdy": {
                "uid": uid.unwrap_or(""),
                "devType": chat_defaults::DEVICE_TYPE,
                "accTkn": access_token,
                "auth": if uid.is_some() { "SEND" } else { "READ" },
                "libVer": "4.9.3",
                "osVer": self.os_version(),
                "devName": "CView_v2/1.0.0",
                "locale": "ko",
                "timezone": "Asia/Seoul",
            },
            "tid": 1,
        });
        to_foundation_json(&body)
    }

    /// cmd 3101 (send chat) 프레임 — 치지직 웹 HAR 기반 포맷. `tid` 는 **문자열**로 보낸다.
    #[allow(clippy::too_many_arguments)]
    pub fn build_send_message(
        &self,
        chat_channel_id: &str,
        channel_id: Option<&str>,
        session_id: Option<&str>,
        extra_token: Option<&str>,
        message: &str,
        tid: i64,
        emojis: Option<&EmojiMap>,
    ) -> String {
        let msg_time = chrono::Utc::now().timestamp_millis();
        self.build_send_message_at(chat_channel_id, channel_id, session_id, extra_token, message, tid, emojis, msg_time)
    }

    /// 시각을 지정하는 [`build_send_message`](Self::build_send_message) (결정적 테스트용).
    #[allow(clippy::too_many_arguments)]
    pub fn build_send_message_at(
        &self,
        chat_channel_id: &str,
        channel_id: Option<&str>,
        session_id: Option<&str>,
        extra_token: Option<&str>,
        message: &str,
        tid: i64,
        emojis: Option<&EmojiMap>,
        msg_time: i64,
    ) -> String {
        let emoji_obj: Map<String, Value> = emojis
            .map(|m| m.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect())
            .unwrap_or_default();
        let mut msg_extras = Map::new();
        msg_extras.insert("chatType".into(), "STREAMING".into());
        msg_extras.insert("osType".into(), "PC".into());
        msg_extras.insert("streamingChannelId".into(), channel_id.unwrap_or(chat_channel_id).into());
        msg_extras.insert("emojis".into(), Value::Object(emoji_obj));
        if let Some(token) = extra_token.filter(|t| !t.is_empty()) {
            msg_extras.insert("extraToken".into(), token.into());
        }

        let body = json!({
            "ver": "3",
            "cmd": ChzzkChatCommand::SendChat.raw_value(),
            "svcid": "game",
            "cid": chat_channel_id,
            "sid": session_id.unwrap_or(""),
            "retry": false,
            "tid": tid.to_string(),
            "bdy": {
                "msg": message,
                "msgTypeCode": 1,
                "extras": to_foundation_json(&Value::Object(msg_extras)),
                "msgTime": msg_time,
                "ctime": msg_time,
            },
        });
        to_foundation_json(&body)
    }

    /// cmd 5101 (최근 채팅 요청). cmd 10100 에서 받은 비어 있지 않은 sid 가 필요하다.
    pub fn build_recent_chat_request(&self, chat_channel_id: &str, session_id: &str, count: i64) -> String {
        let body = json!({
            "ver": "3",
            "cmd": ChzzkChatCommand::RequestRecentChat.raw_value(),
            "svcid": "game",
            "cid": chat_channel_id,
            "sid": session_id,
            "tid": 2,
            "bdy": { "recentMessageCount": count },
        });
        to_foundation_json(&body)
    }

    /// pong 응답 (cmd 10000). 키 순서까지 Swift 리터럴 그대로.
    pub fn build_pong(&self) -> String {
        format!("{{\"ver\":\"3\",\"cmd\":{}}}", ChzzkChatCommand::Pong.raw_value())
    }
}
