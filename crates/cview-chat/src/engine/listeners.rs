//! 소켓 수신·핑 감시 리스너.

use std::time::Instant;

use super::{abort, ChatEngine, EngineInner, EngineState};
use crate::constants::secs;
use crate::websocket::{WebSocketService, WebSocketState};

impl EngineInner {
    pub(super) fn start_message_listening(
        &self,
        st: &mut EngineState,
        ws: &WebSocketService,
        generation: u64,
    ) {
        abort(&mut st.message_listen_task);
        let Some(mut rx) = ws.take_messages() else {
            return;
        };
        let weak = self.weak_self.clone();
        let ws = ws.clone();
        st.message_listen_task = Some(tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                let Some(inner) = weak.upgrade() else { return };
                if let Some(text) = message.text_value() {
                    let result = inner.parser.parse_events(text);
                    inner.handle_parsed_result(result, text, &ws, generation);
                }
            }
            // 스트림 종료 = 연결 종료.
            if let Some(inner) = weak.upgrade() {
                let mut st = inner.lock();
                if Self::is_current_connection(&st, &ws, generation) {
                    inner.handle_web_socket_closed(&mut st);
                }
            }
        }));
    }

    pub(super) fn start_state_listening(
        &self,
        st: &mut EngineState,
        ws: &WebSocketService,
        generation: u64,
    ) {
        abort(&mut st.state_listen_task);
        let Some(mut rx) = ws.take_state_changes() else {
            return;
        };
        let weak = self.weak_self.clone();
        let ws = ws.clone();
        st.state_listen_task = Some(tokio::spawn(async move {
            while let Some(state) = rx.recv().await {
                let Some(inner) = weak.upgrade() else { return };
                let mut st = inner.lock();
                if !Self::is_current_connection(&st, &ws, generation) {
                    continue;
                }
                if let WebSocketState::Failed(reason) = state {
                    tracing::error!(%reason, "WS state failed — triggering reconnection");
                    inner.handle_web_socket_closed(&mut st);
                }
            }
        }));
    }

    pub(super) fn start_watchdog(
        &self,
        st: &mut EngineState,
        ws: &WebSocketService,
        generation: u64,
    ) {
        abort(&mut st.watchdog_task);
        let weak = self.weak_self.clone();
        let ws = ws.clone();
        let period = secs(self.config.watchdog_check_interval);
        st.watchdog_task = Some(tokio::spawn(async move {
            loop {
                tokio::time::sleep(period).await;
                let Some(inner) = weak.upgrade() else { return };
                inner.check_inbound_activity(&ws, generation);
            }
        }));
    }

    /// 마지막 JSON 프레임 또는 성공한 transport pong 이후 동적 silence limit 을 넘으면 재연결.
    fn check_inbound_activity(&self, ws: &WebSocketService, generation: u64) {
        let mut st = self.lock();
        if !Self::is_current_connection(&st, ws, generation)
            || st.is_manual_disconnect
            || st.is_reconnecting()
        {
            return;
        }
        let transport = ws.transport_liveness();
        let latest = st.last_inbound_activity.max(transport.last_activity);
        let silence = latest.elapsed().as_secs_f64();
        let limit = ChatEngine::watchdog_silence_limit(transport.ping_interval);
        if silence <= limit {
            return;
        }
        tracing::warn!(
            silence = silence as i64,
            limit = limit as i64,
            "Chat inactivity watchdog — forcing reconnect"
        );
        st.last_inbound_activity = Instant::now();
        self.handle_web_socket_closed(&mut st);
    }
}
