//! 수신 메시지 처리 — 명령별 분기와 채팅·후원·공지·시스템 이벤트 변환.

use std::collections::HashSet;
use std::time::Instant;

use cview_core::models::chat::{ChatConnectionState, ChatMessage};

use super::{abort, ChatEngine, ChatEngineEvent, EngineInner, EngineState, BUFFER_TRIM_SLACK};
use crate::error::{ChatError, ChatResult};
use crate::protocol::ChatEvent;
use crate::websocket::WebSocketService;

impl EngineInner {
    pub(super) fn handle_parsed_result(
        &self,
        result: ChatResult<Vec<ChatEvent>>,
        raw: &str,
        ws: &WebSocketService,
        generation: u64,
    ) {
        let mut st = self.lock();
        if !Self::is_current_connection(&st, ws, generation) {
            return;
        }
        st.last_inbound_activity = Instant::now();
        match result {
            Ok(events) => {
                for event in events {
                    self.handle_parsed_event(&mut st, event);
                }
            }
            Err(error) => tracing::warn!(%error, len = raw.len(), "Failed to parse message"),
        }
    }

    pub(super) fn handle_parsed_event(&self, st: &mut EngineState, event: ChatEvent) {
        match event {
            ChatEvent::Connected { session_id } => {
                if !self.fulfill_connect_ack(st) {
                    tracing::debug!("Ignoring late/duplicate Chat CONNECT ack");
                    return;
                }
                st.session_id = Some(session_id);
                self.update_connection_state(
                    st,
                    ChatConnectionState::Connected { server_index: 0 },
                );
                // 핸들을 보관해 취소 가능하게 — 채널 전환 시 이전 요청이 새 소켓으로 나가지 않게.
                let generation = st.connection_generation;
                abort(&mut st.recent_messages_task);
                let weak = self.weak_self.clone();
                st.recent_messages_task = Some(tokio::spawn(async move {
                    if let Some(inner) = weak.upgrade() {
                        inner.fetch_recent_messages(generation).await;
                    }
                }));
                self.emit(ChatEngineEvent::Connected);
            }
            ChatEvent::ConnectionRejected { ret_code, ret_msg } => {
                let code = ret_code.map_or_else(|| "missing".into(), |c| c.to_string());
                let suffix = ret_msg
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| format!(": {s}"))
                    .unwrap_or_default();
                tracing::warn!(%code, "Chat CONNECT rejected");
                self.fail_connect_ack(
                    st,
                    ChatError::ConnectionFailed(format!("연결 응답 거절 (retCode={code}){suffix}")),
                );
            }
            ChatEvent::Messages(msgs) => {
                self.apply_repeated_blind_states(st, &msgs);
                let fresh = self.append_messages(st, msgs);
                if !fresh.is_empty() {
                    self.emit(ChatEngineEvent::NewMessages(fresh));
                }
            }
            ChatEvent::RecentMessages(msgs) => {
                self.apply_repeated_blind_states(st, &msgs);
                let fresh = self.append_messages(st, msgs);
                // 빈 응답도 backlog hold 를 해제하는 완료 신호이므로 반드시 전달한다.
                self.emit(ChatEngineEvent::RecentMessages(fresh));
            }
            ChatEvent::Donations(msgs) => {
                self.apply_repeated_blind_states(st, &msgs);
                let fresh = self.append_messages(st, msgs);
                if !fresh.is_empty() {
                    self.emit(ChatEngineEvent::Donations(fresh));
                }
            }
            // 공지는 상단 배너 전용 — 메시지 버퍼에 넣지 않는다.
            ChatEvent::Notice(msg) => self.emit(ChatEngineEvent::Notice(msg)),
            ChatEvent::NoticeCleared => self.emit(ChatEngineEvent::NoticeCleared),
            ChatEvent::Blind { message_id, .. } => {
                Self::remove_message(st, &message_id);
                self.emit(ChatEngineEvent::MessageBlinded {
                    message_id,
                    blind_text: None,
                });
            }
            ChatEvent::Kick { user_id } => {
                if ChatEngine::is_self_kick(&user_id, self.config.uid.as_deref()) {
                    self.emit(ChatEngineEvent::Kicked);
                } else {
                    tracing::debug!("Kick broadcast for another user — ignored");
                }
            }
            ChatEvent::Penalty { user_id, duration } => {
                self.emit(ChatEngineEvent::UserPenalized { user_id, duration });
            }
            ChatEvent::Ping => {
                // 단일 pong 태스크 — 새 ping 이 오면 이전 미완료 태스크를 취소.
                let pong = self.parser.build_pong();
                let socket = st.web_socket.clone();
                abort(&mut st.pong_response_task);
                st.pong_response_task = Some(tokio::spawn(async move {
                    if let Some(ws) = socket {
                        if let Err(error) = ws.send(&pong).await {
                            tracing::warn!(%error, "Pong send failed");
                        }
                    }
                }));
            }
            ChatEvent::Pong => {}
            ChatEvent::SendConfirmed {
                tid,
                ret_code,
                ret_msg,
            } => {
                if ret_code == 0 {
                    tracing::debug!("Chat send confirmed");
                } else {
                    tracing::warn!(ret_code, "Chat send failed");
                    self.emit(ChatEngineEvent::SendRejected {
                        tid,
                        ret_code,
                        message: ret_msg,
                    });
                }
            }
            ChatEvent::System(msg) => self.emit(ChatEngineEvent::SystemMessage(msg)),
            ChatEvent::Unknown { cmd } => tracing::debug!(cmd, "Unknown chat command"),
        }
    }

    /// 재연결 동안 놓친 삭제가 같은 id 의 HIDDEN/CBOTBLIND 스냅샷으로 올 수 있다.
    /// 원문으로 되돌리는 역방향 갱신은 허용하지 않는다.
    fn apply_repeated_blind_states(&self, st: &mut EngineState, messages: &[ChatMessage]) {
        for message in messages.iter().filter(|m| m.is_blinded && !m.id.is_empty()) {
            if !st.message_ids.contains(&message.id) {
                continue;
            }
            let Some(index) = st.messages.iter().position(|m| m.id == message.id) else {
                continue;
            };
            if st.messages[index].is_blinded {
                continue;
            }
            st.messages[index] = message.clone();
            // 서버가 준 삭제 사유(클린봇/관리자)를 그대로 싣는다.
            self.emit(ChatEngineEvent::MessageBlinded {
                message_id: message.id.clone(),
                blind_text: Some(message.content.clone()),
            });
        }
    }

    /// id 디듑 후 버퍼에 붙이고 새로 들어간 것만 돌려준다. 빈 id 는 디듑하지 않는다.
    pub(super) fn append_messages(
        &self,
        st: &mut EngineState,
        new_messages: Vec<ChatMessage>,
    ) -> Vec<ChatMessage> {
        let mut seen_in_batch: HashSet<String> = HashSet::new();
        let deduped: Vec<ChatMessage> = new_messages
            .into_iter()
            .filter(|m| {
                m.id.is_empty()
                    || (!st.message_ids.contains(&m.id) && seen_in_batch.insert(m.id.clone()))
            })
            .collect();
        if deduped.is_empty() {
            return deduped;
        }
        st.messages.extend(deduped.iter().cloned());
        for m in deduped.iter().filter(|m| !m.id.is_empty()) {
            st.message_ids.insert(m.id.clone());
        }

        let max = self.config.max_message_buffer;
        if st.messages.len() > max + BUFFER_TRIM_SLACK {
            let excess = st.messages.len() - max;
            // 축출될 id 를 제거 **전에** 걷어낸다.
            let EngineState {
                messages,
                message_ids,
                ..
            } = st;
            for m in messages[..excess].iter().filter(|m| !m.id.is_empty()) {
                message_ids.remove(&m.id);
            }
            messages.drain(..excess);
        }
        deduped
    }

    pub(super) fn remove_message(st: &mut EngineState, id: &str) {
        if let Some(index) = st.messages.iter().position(|m| m.id == id) {
            st.messages.remove(index);
            st.message_ids.remove(id);
        }
    }
}
