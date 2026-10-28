//! 재연결 예약.

use cview_core::models::chat::ChatConnectionState;

use super::{abort, start_reconnection, EngineInner, EngineState};
use crate::error::ChatError;

impl EngineInner {
    /// 소켓 종료 공통 진입점 (호출측이 세대/identity gate 를 먼저 통과시킨다).
    pub(super) fn handle_web_socket_closed(&self, st: &mut EngineState) {
        // guard 보다 먼저 핸드셰이크 대기자를 깨운다.
        self.fail_connect_ack(
            st,
            ChatError::ConnectionFailed("핸드셰이크 중 연결 종료".into()),
        );
        abort(&mut st.recent_messages_task);
        if st.is_manual_disconnect {
            return;
        }
        if st.is_reconnecting() {
            // ack 와 executeReconnection 성공 반환 사이의 close 를 버리지 않는다.
            let nonce = st.connect_ack_state.nonce();
            st.reconnection_owner.request_reconnect(nonce);
            return;
        }
        let generation = st.connection_generation;
        let owner = st.reconnection_owner.begin(generation);
        self.update_connection_state(st, ChatConnectionState::Reconnecting { attempt: 0 });
        st.reconnection_task = Some(tokio::spawn(start_reconnection(
            self.weak_self.clone(),
            generation,
            owner,
        )));
    }
}
