//! CViewChat 포팅: CHZZK 채팅 프로토콜·파서·재연결 정책·WebSocket·엔진·모더레이션.
//!
//! | Swift (`Sources/CViewChat/`) | Rust 모듈 |
//! |---|---|
//! | `ChatConstants.swift` | [`constants`] |
//! | `ChatMessageParser.swift` (+`+Build`, `+ParseDonation`, `+ParseMessage`, `+ParseMeta`) | [`protocol`], [`parser`] |
//! | `ReconnectionPolicy.swift` | [`reconnection`] |
//! | `WebSocketService.swift` | [`websocket`] |
//! | `ChatEngine.swift` | [`engine`] |
//! | `ChatModerationService.swift` | [`moderation`] |
//! | `CViewCore/Utilities/ChatMessageBuffer.swift` 등 채팅 전용 유틸 | [`utils`] |
//!
//! Swift actor 는 `Mutex` 로 보호되는 상태 + tokio 태스크로, `AsyncStream` 은 채널/큐로 옮겼다.

pub mod constants;
pub mod engine;
pub mod error;
pub mod json;
pub mod moderation;
pub mod parser;
pub mod protocol;
pub mod reconnection;
pub mod utils;
pub mod websocket;

pub use engine::{ChatEngine, ChatEngineConfiguration, ChatEngineEvent, ChatEventStream};
pub use error::{ChatError, ChatResult};
pub use moderation::{ChatCommand, ChatCommandResult, ChatFilter, ChatModerationService, FilterType};
pub use parser::ChatMessageParser;
pub use protocol::{AnyCodable, ChatEvent, ChatProtocolMessage, ChzzkChatCommand};
pub use reconnection::{ReconnectionConfiguration, ReconnectionPolicy, ReconnectionState};
pub use websocket::{WebSocketConfiguration, WebSocketMessage, WebSocketService, WebSocketState};

#[cfg(test)]
mod tests;
