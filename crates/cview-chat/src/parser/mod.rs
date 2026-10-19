//! CHZZK 채팅 프로토콜 메시지 파서.
//!
//! 원본: `Sources/CViewChat/ChatMessageParser.swift` (+`+Build`, `+ParseDonation`,
//! `+ParseMessage`, `+ParseMeta`). 상태 없는 순수 함수 묶음이다.

mod build;
mod parse_message;
mod parse_meta;

use serde_json::Value;

use crate::error::{ChatError, ChatResult};
use crate::json::{any_str, ns_str};
use crate::protocol::{ChatEvent, ChatProtocolMessage, ChzzkChatCommand};

pub use build::default_os_version;

/// 원시 WebSocket 텍스트를 도메인 이벤트로 바꾸는 파서.
#[derive(Debug, Clone)]
pub struct ChatMessageParser {
    /// cmd 100 의 `osVer` 값 (Swift: `"macOS/<major>.<minor>.<patch>"`).
    os_version: String,
}

impl Default for ChatMessageParser {
    fn default() -> Self {
        Self::new()
    }
}

impl ChatMessageParser {
    /// 클린봇(`msgStatusType == "CBOTBLIND"`)이 삭제한 메시지의 안내 문구.
    pub const CLEAN_BOT_BLIND_TEXT: &'static str = "클린봇이 부적절한 표현을 감지했습니다.";
    /// 관리자 blind / `HIDDEN` 안내 문구.
    pub const ADMIN_BLIND_TEXT: &'static str = "관리자에 의해 삭제된 메시지입니다.";

    pub fn new() -> Self {
        Self { os_version: default_os_version() }
    }

    /// `osVer` 를 직접 지정한다(테스트·플랫폼별 조정용).
    pub fn with_os_version(os_version: impl Into<String>) -> Self {
        Self { os_version: os_version.into() }
    }

    pub fn os_version(&self) -> &str {
        &self.os_version
    }

    // MARK: - 다형 숫자 파서

    /// payAmount 다형 파서. 정수 / 실수 / `"1000"` / `"1,000"` / `"₩1,000원"` → 정수.
    /// Bool 은 거절, 실수는 반올림(0.5 는 0 에서 먼 쪽), 범위 밖은 `i64` 상·하한으로 고정.
    pub fn parse_amount(value: Option<&Value>) -> Option<i64> {
        match value? {
            Value::Bool(_) => None,
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    return Some(i);
                }
                let f = n.as_f64()?;
                let rounded = f.round();
                if !rounded.is_finite() {
                    return None;
                }
                if rounded >= i64::MAX as f64 {
                    return Some(i64::MAX);
                }
                if rounded <= i64::MIN as f64 {
                    return Some(i64::MIN);
                }
                Some(rounded as i64)
            }
            Value::String(s) => {
                let digits: String = s.chars().filter(char::is_ascii_digit).collect();
                if digits.is_empty() {
                    return None;
                }
                // 범위 초과는 드롭 대신 상한으로 고정 (후원금액 무음 드롭 방지).
                Some(digits.parse::<i64>().unwrap_or(i64::MAX))
            }
            _ => None,
        }
    }

    /// `f64` 입력용 [`parse_amount`](Self::parse_amount) (비유한 수는 `None`).
    pub fn parse_amount_f64(value: f64) -> Option<i64> {
        if !value.is_finite() {
            return None;
        }
        let rounded = value.round();
        if rounded >= i64::MAX as f64 {
            Some(i64::MAX)
        } else if rounded <= i64::MIN as f64 {
            Some(i64::MIN)
        } else {
            Some(rounded as i64)
        }
    }

    /// tid 다형 파서. 정수 / `"3"` → 정수. cmd 3101 은 문자열 tid 를 쓰고 서버는 받은 형식
    /// 그대로 되돌려주므로 두 형식을 모두 읽는다. Bool·소수·숫자 아닌 문자열은 `None`.
    pub fn parse_tid(value: Option<&Value>) -> Option<i64> {
        match value? {
            Value::Bool(_) => None,
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    return Some(i);
                }
                if n.as_u64().is_some() {
                    return None;
                }
                let f = n.as_f64()?;
                (f.is_finite() && f.fract() == 0.0 && f >= i64::MIN as f64 && f < i64::MAX as f64)
                    .then_some(f as i64)
            }
            Value::String(s) => s.parse::<i64>().ok(),
            _ => None,
        }
    }

    // MARK: - Parsing

    /// 원시 텍스트 → envelope. 객체가 아니거나 `cmd` 가 정수로 읽히지 않으면 실패.
    pub fn parse_raw(&self, text: &str) -> ChatResult<ChatProtocolMessage> {
        let json: Value = serde_json::from_str(text).map_err(|_| ChatError::InvalidMessage)?;
        let obj = json.as_object().ok_or(ChatError::InvalidMessage)?;
        let cmd = Self::parse_tid(obj.get("cmd")).ok_or(ChatError::InvalidMessage)?;

        Ok(ChatProtocolMessage {
            cmd,
            tid: Self::parse_tid(obj.get("tid")),
            svcid: ns_str(obj.get("svcid")).map(str::to_owned),
            cid: ns_str(obj.get("cid")).map(str::to_owned),
            bdy: obj.get("bdy").cloned(),
            ret_code: Self::parse_tid(obj.get("retCode")),
            ret_msg: ns_str(obj.get("retMsg")).map(str::to_owned),
        })
    }

    /// 프레임 하나 → 도메인 이벤트 하나.
    pub fn parse(&self, text: &str) -> ChatResult<ChatEvent> {
        let raw = self.parse_raw(text)?;
        self.parse_to_domain_event(&raw)
    }

    /// 프레임의 본문과 부가 상태(최근 채팅에 포함된 공지)를 함께 전달한다.
    /// 엔진은 이 경로를 쓴다.
    pub fn parse_events(&self, text: &str) -> ChatResult<Vec<ChatEvent>> {
        let raw = self.parse_raw(text)?;
        let mut events = vec![self.parse_to_domain_event(&raw)?];
        if raw.cmd == ChzzkChatCommand::RecentChat.raw_value() {
            if let Some(notice) = raw.bdy.as_ref().and_then(Value::as_object).and_then(|b| b.get("notice")) {
                let notice_raw =
                    ChatProtocolMessage::new(ChzzkChatCommand::Notice.raw_value()).with_bdy(notice.clone());
                // 손상된 부가 공지 하나가 정상 최근 메시지 전체를 버리게 하지 않는다.
                if let Ok(event) = self.parse_to_domain_event(&notice_raw) {
                    events.push(event);
                }
            }
        }
        Ok(events)
    }

    /// envelope → 도메인 이벤트.
    pub fn parse_to_domain_event(&self, raw: &ChatProtocolMessage) -> ChatResult<ChatEvent> {
        let Some(command) = ChzzkChatCommand::from_raw(raw.cmd) else {
            return Ok(ChatEvent::Unknown { cmd: raw.cmd });
        };

        Ok(match command {
            ChzzkChatCommand::Connected => {
                // 명시적 retCode == 0 과 비어 있지 않은 sid 가 함께 있어야 성공이다.
                if raw.ret_code != Some(0) {
                    return Ok(ChatEvent::ConnectionRejected {
                        ret_code: raw.ret_code,
                        ret_msg: raw.ret_msg.clone(),
                    });
                }
                let session_id = self.extract_session_id(raw).trim().to_string();
                if session_id.is_empty() {
                    return Ok(ChatEvent::ConnectionRejected {
                        ret_code: raw.ret_code,
                        ret_msg: raw.ret_msg.clone(),
                    });
                }
                ChatEvent::Connected { session_id }
            }
            ChzzkChatCommand::ChatMessage | ChzzkChatCommand::EmoteMessage => {
                ChatEvent::Messages(self.parse_chat_messages(raw))
            }
            ChzzkChatCommand::Donation => ChatEvent::Donations(self.parse_donations(raw)),
            ChzzkChatCommand::RecentChat => ChatEvent::RecentMessages(self.parse_chat_messages(raw)),
            ChzzkChatCommand::Notice => match &raw.bdy {
                Some(Value::Null) => ChatEvent::NoticeCleared,
                Some(Value::Object(body)) if body.is_empty() => ChatEvent::NoticeCleared,
                _ => ChatEvent::Notice(self.parse_notice(raw)?),
            },
            ChzzkChatCommand::Blind => {
                let (message_id, user_id) = self.parse_blind(raw);
                ChatEvent::Blind { message_id, user_id }
            }
            ChzzkChatCommand::Kick => ChatEvent::Kick { user_id: self.parse_kick_target(raw) },
            ChzzkChatCommand::Penalty => {
                let (user_id, duration) = self.parse_penalty(raw);
                ChatEvent::Penalty { user_id, duration }
            }
            ChzzkChatCommand::Ping => ChatEvent::Ping,
            ChzzkChatCommand::Pong => ChatEvent::Pong,
            ChzzkChatCommand::SendChatResponse => {
                let ret_code = raw.ret_code.unwrap_or(0);
                if ret_code != 0 {
                    tracing::warn!(
                        ret_code,
                        ret_msg = raw.ret_msg.as_deref().unwrap_or("unknown"),
                        "SendChat response error"
                    );
                }
                ChatEvent::SendConfirmed { tid: raw.tid, ret_code, ret_msg: raw.ret_msg.clone() }
            }
            ChzzkChatCommand::SystemMessage => ChatEvent::System(self.extract_system_message(raw)),
            ChzzkChatCommand::Connect
            | ChzzkChatCommand::RequestRecentChat
            | ChzzkChatCommand::SendChat
            | ChzzkChatCommand::SendEmote => ChatEvent::Unknown { cmd: raw.cmd },
        })
    }
}

/// `bdy` 를 객체로 본다.
pub(crate) fn body_object(raw: &ChatProtocolMessage) -> Option<&serde_json::Map<String, Value>> {
    raw.bdy.as_ref().and_then(Value::as_object)
}

/// `dict[key]?.stringValue`.
pub(crate) fn body_str<'a>(dict: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a str> {
    any_str(dict.get(key))
}
