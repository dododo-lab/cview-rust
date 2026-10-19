//! `Tests/CViewChatTests` 포팅.
//!
//! 소스 텍스트를 grep 하던 Swift 배선 테스트(`*WiringTests`)는 Swift 소스 전용이라 옮기지 않았다.
//! 대신 같은 계약(ack 대기·소켓 정리·세대 gate)을 로컬 tokio-tungstenite 서버로 **동작** 검증한다
//! ([`websocket_tests`]).

mod engine_tests;
mod moderation_tests;
mod parser_tests;
mod reconnection_tests;
mod state_tests;
mod utils_tests;
mod websocket_tests;

/// Swift 테스트의 `jsonEscape` — JSON 문자열 값 안에 넣을 수 있게 이스케이프.
pub(crate) fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
