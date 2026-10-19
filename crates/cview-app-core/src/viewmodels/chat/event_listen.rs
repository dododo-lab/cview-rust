//! 엔진 이벤트 수신·처리.
//!
//! 원본: `Sources/CViewApp/ViewModels/ChatViewModel+EventListen.swift`.

use std::sync::Arc;

use cview_chat::ChatEngineEvent;
use cview_core::models::chat::{
    ChatConnectionState, ChatMessageItem, ChatSystemNoticeKind, MessageType,
};

use super::processing::stream_alert_from_item;
use super::{
    ChatEngineHandle, ChatEventSessionIdentity, ChatMessageMutation, ChatRestrictionReason,
    ChatState,
};
use crate::viewmodels::support::{cancel_slot, TaskSlot};

impl ChatState {
    pub fn is_current_event_session(&self, identity: Option<&ChatEventSessionIdentity>) -> bool {
        let Some(identity) = identity else {
            return true;
        };
        self.chat_session_generation == identity.session_generation
            && self
                .chat_engine
                .as_ref()
                .is_some_and(|e| Arc::ptr_eq(e, &identity.engine))
            && self.active_chat_channel_id() == identity.channel_identity
    }

    pub fn start_event_listening(&mut self, engine: Arc<dyn ChatEngineHandle>) {
        cancel_slot(&mut self.event_listen_task);
        let identity = ChatEventSessionIdentity {
            session_generation: self.chat_session_generation,
            engine: engine.clone(),
            channel_identity: self.active_chat_channel_id(),
        };
        let weak = self.this.clone();
        self.event_listen_task = Some(TaskSlot::spawn(move |_| async move {
            while let Some(event) = engine.next_event().await {
                let Some(shared) = weak.upgrade() else { break };
                let mut s = shared.obs.lock();
                if !s.is_current_event_session(Some(&identity)) {
                    break;
                }
                s.handle_event_from(event, Some(&identity));
            }
        }));
    }

    /// 출처 확인 없이 처리 (Swift `handleEvent(_:)`).
    pub fn handle_event(&mut self, event: ChatEngineEvent) {
        self.handle_event_from(event, None);
    }

    pub fn handle_event_from(
        &mut self,
        event: ChatEngineEvent,
        source: Option<&ChatEventSessionIdentity>,
    ) {
        if !self.is_current_event_session(source) {
            return;
        }
        match event {
            ChatEngineEvent::Connected => {
                self.connection_state = ChatConnectionState::Connected { server_index: 0 };
                self.error_message = None;
                self.begin_backlog_hold();
                self.set_chat_restriction(None, None);
                if !self.has_shown_welcome {
                    self.has_shown_welcome = true;
                    self.append_message(Self::welcome_message());
                }
            }
            ChatEngineEvent::Disconnected { reason } => {
                self.connection_state = ChatConnectionState::Disconnected;
                self.error_message = Some(reason);
            }
            ChatEngineEvent::Reconnecting => {
                self.connection_state = ChatConnectionState::Reconnecting { attempt: 0 };
            }
            ChatEngineEvent::StateChanged(state) => self.connection_state = state,
            ChatEngineEvent::NewMessages(msgs) => {
                self.append_filtered_messages(msgs, source, false);
            }
            ChatEngineEvent::RecentMessages(msgs) => {
                self.append_filtered_messages(msgs, source, true);
            }
            ChatEngineEvent::Donations(msgs) => {
                let Some(committed) = self.append_filtered_messages(msgs, source, false) else {
                    return;
                };
                for item in committed
                    .iter()
                    .filter(|i| i.r#type == MessageType::Donation)
                {
                    if let Some(alert) = stream_alert_from_item(item) {
                        self.enqueue_stream_alert(alert);
                    }
                }
            }
            ChatEngineEvent::Notice(msg) => {
                let item = ChatMessageItem::from_message(&msg, true);
                if item.content.trim().is_empty() {
                    return;
                }
                if let Some(cid) = self.active_chat_channel_id().filter(|c| !c.is_empty()) {
                    self.deps.pinned_notices.save(&item, &cid);
                }
                if self.deps.pinned_notices.is_dismissed(&item.id) {
                    return;
                }
                self.streamer_notice = Some(item);
            }
            ChatEngineEvent::NoticeCleared => {
                self.streamer_notice = None;
                if let Some(cid) = self.active_chat_channel_id().filter(|c| !c.is_empty()) {
                    self.deps.pinned_notices.clear(&cid);
                }
            }
            ChatEngineEvent::MessageBlinded {
                message_id,
                blind_text,
            } => self.apply_mutation(ChatMessageMutation::Blind {
                message_id,
                text: blind_text,
            }),
            ChatEngineEvent::Kicked => {
                self.set_chat_restriction(Some(ChatRestrictionReason::Kicked), None);
                self.error_message = Some("채팅에서 추방되었습니다.".into());
                self.append_system(
                    "채팅에서 추방되어 메시지를 보낼 수 없습니다.",
                    ChatSystemNoticeKind::Warning,
                );
            }
            ChatEngineEvent::UserPenalized { user_id, duration } => {
                tracing::info!("User penalized: {user_id} duration={duration}s");
                let mine = self
                    .current_user_uid
                    .as_deref()
                    .is_some_and(|me| !me.is_empty() && me == user_id);
                if mine {
                    let text = if duration <= 0 {
                        "회원님은 채팅이 일시적으로 제한되었습니다.".to_string()
                    } else if duration < 60 {
                        format!("회원님은 {duration}초 동안 채팅이 제한되었습니다.")
                    } else {
                        format!("회원님은 {}분 동안 채팅이 제한되었습니다.", duration / 60)
                    };
                    self.append_system(text, ChatSystemNoticeKind::Warning);
                    if duration > 0 {
                        self.set_chat_restriction(
                            Some(ChatRestrictionReason::Penalized),
                            Some(duration),
                        );
                    }
                }
            }
            ChatEngineEvent::SendRejected {
                tid,
                ret_code,
                message,
            } => {
                let detail = message
                    .map(|m| m.trim().to_string())
                    .filter(|m| !m.is_empty());
                let text = detail
                    .unwrap_or_else(|| format!("메시지를 보내지 못했습니다. (코드 {ret_code})"));
                tracing::warn!("Chat send rejected: retCode={ret_code}");
                self.error_message = Some(text.clone());
                let reclaimed = self.reclaim_rejected_echo(tid);
                if !reclaimed {
                    if let Some(tid) = tid {
                        self.remember_early_send_rejection(tid);
                    }
                }
                self.append_system(text, ChatSystemNoticeKind::Error);
            }
            ChatEngineEvent::SystemMessage(msg) => {
                if msg.is_empty() {
                    return;
                }
                self.append_system(msg, ChatSystemNoticeKind::Info);
            }
            ChatEngineEvent::MessagesCleared => {
                self.messages.clear();
                self.messages_revision = self.messages_revision.wrapping_add(1);
                self.replay_hold_queue.clear();
            }
        }
    }
}
