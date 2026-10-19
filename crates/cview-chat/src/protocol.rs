//! CHZZK 채팅 와이어 프로토콜 타입.
//!
//! 원본: `Sources/CViewChat/ChatMessageParser.swift` 의 `ChzzkChatCommand`,
//! `ChatProtocolMessage`, `AnyCodable`, `ChatEvent`.
//!
//! Swift `AnyCodable`(타입 소거 JSON 값)은 [`serde_json::Value`] 로 대체한다. 정수/실수 구분
//! (`.int` / `.double`)은 `Value::Number` 의 `as_i64`/`as_f64` 로 그대로 남는다.

use cview_core::models::chat::ChatMessage;
use serde::{Deserialize, Serialize};

/// Swift `AnyCodable` 대체 — 타입 소거 JSON 값.
pub type AnyCodable = serde_json::Value;

/// CHZZK WebSocket 채팅 프로토콜 명령 코드.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i64)]
pub enum ChzzkChatCommand {
    Ping = 0,
    Pong = 10000,
    Connect = 100,
    Connected = 10100,
    RequestRecentChat = 5101,
    RecentChat = 15101,
    SendChat = 3101,
    ChatMessage = 93101,
    Donation = 93102,
    Kick = 94005,
    Blind = 94008,
    Notice = 94010,
    Penalty = 94015,
    SendChatResponse = 13101,
    SendEmote = 3103,
    EmoteMessage = 93103,
    SystemMessage = 93006,
}

impl ChzzkChatCommand {
    /// Swift `CaseIterable.allCases` (선언 순서).
    pub const ALL: [ChzzkChatCommand; 17] = [
        Self::Ping,
        Self::Pong,
        Self::Connect,
        Self::Connected,
        Self::RequestRecentChat,
        Self::RecentChat,
        Self::SendChat,
        Self::ChatMessage,
        Self::Donation,
        Self::Kick,
        Self::Blind,
        Self::Notice,
        Self::Penalty,
        Self::SendChatResponse,
        Self::SendEmote,
        Self::EmoteMessage,
        Self::SystemMessage,
    ];

    /// 와이어 코드.
    pub fn raw_value(self) -> i64 {
        self as i64
    }

    /// Swift `init(rawValue:)`.
    pub fn from_raw(raw: i64) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.raw_value() == raw)
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Ping => "Ping",
            Self::Pong => "Pong",
            Self::Connect => "Connect",
            Self::Connected => "Connected",
            Self::RequestRecentChat => "RequestRecentChat",
            Self::RecentChat => "RecentChat",
            Self::SendChat => "SendChat",
            Self::ChatMessage => "ChatMessage",
            Self::Donation => "Donation",
            Self::Kick => "Kick",
            Self::Blind => "Blind",
            Self::Notice => "Notice",
            Self::Penalty => "Penalty",
            Self::SendChatResponse => "SendChatResponse",
            Self::SendEmote => "SendEmote",
            Self::EmoteMessage => "EmoteMessage",
            Self::SystemMessage => "SystemMessage",
        }
    }
}

impl std::fmt::Display for ChzzkChatCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.description())
    }
}

/// CHZZK WebSocket 프레임 envelope.
///
/// `bdy` 는 키가 없으면 `None`, 명시적 `null` 이면 `Some(Value::Null)` 이다
/// (cmd 94010 의 `bdy: null` = 공지 해제 판정에 필요).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatProtocolMessage {
    pub cmd: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tid: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub svcid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bdy: Option<AnyCodable>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ret_code: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ret_msg: Option<String>,
}

impl ChatProtocolMessage {
    /// `cmd` 만 채운 envelope.
    pub fn new(cmd: i64) -> Self {
        Self { cmd, ..Default::default() }
    }

    pub fn with_bdy(mut self, bdy: AnyCodable) -> Self {
        self.bdy = Some(bdy);
        self
    }

    pub fn with_ret_code(mut self, ret_code: i64) -> Self {
        self.ret_code = Some(ret_code);
        self
    }
}

/// 파서가 만드는 도메인 이벤트.
#[derive(Debug, Clone, PartialEq)]
// 공지 한 건은 드물고 나머지는 Vec/String 이라 Box 로 API 를 비틀 이유가 없다.
#[allow(clippy::large_enum_variant)]
pub enum ChatEvent {
    Connected { session_id: String },
    /// cmd 10100 이 오류 retCode 를 반환했거나 유효한 sid 를 주지 않은 핸드셰이크 실패.
    ConnectionRejected { ret_code: Option<i64>, ret_msg: Option<String> },
    Messages(Vec<ChatMessage>),
    RecentMessages(Vec<ChatMessage>),
    Donations(Vec<ChatMessage>),
    Notice(ChatMessage),
    NoticeCleared,
    Blind { message_id: String, user_id: String },
    /// cmd 94005 — 추방. 채팅방 전원에게 브로드캐스트되므로 대상 userId 를 동반한다.
    Kick { user_id: String },
    Penalty { user_id: String, duration: i64 },
    Ping,
    Pong,
    /// cmd 13101 — 전송 응답. `tid` 는 대응 요청 번호(서버가 안 주면 `None`).
    SendConfirmed { tid: Option<i64>, ret_code: i64, ret_msg: Option<String> },
    System(String),
    Unknown { cmd: i64 },
}
