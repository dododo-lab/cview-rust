//! 전송·로컬 에코 회수·리플레이(읽기) 모드·히스토리.
//!
//! 원본: `Sources/CViewApp/ViewModels/ChatViewModel+Interaction.swift`.

use std::sync::Arc;
use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};

use cview_chat::ChatCommand;
use cview_core::models::chat::{ChatMessageItem, MessageType};

use super::{ChatMessageMutation, ChatState, ChatViewModel, EarlySendRejection, PendingSentEcho};
use crate::viewmodels::support::{cancel_slot, slot_is};

/// 전송 결과.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageSendOutcome {
    Sent,
    HandledLocally,
    Failed,
    SessionChanged,
}

impl ChatViewModel {
    /// 입력창 내용을 보낸다. 성공(또는 로컬 처리)이고 입력이 그대로면 비운다.
    pub async fn send_input(&self) {
        let submitted = self.lock().input_text.clone();
        let outcome = self.send_message(&submitted).await;
        if matches!(
            outcome,
            MessageSendOutcome::Sent | MessageSendOutcome::HandledLocally
        ) {
            let mut s = self.lock();
            if s.input_text == submitted {
                s.input_text.clear();
            }
        }
    }

    pub async fn send_message(&self, submitted: &str) -> MessageSendOutcome {
        let text = submitted.trim().to_string();
        if text.is_empty() {
            return MessageSendOutcome::Failed;
        }
        let (engine, session_generation, channel_identity, local_user_id, local_nickname, emojis) = {
            let mut s = self.lock();
            if let Some(outcome) = s.try_handle_command(&text) {
                return outcome;
            }
            if !s.can_send_chat() {
                s.error_message = Some(if s.is_chat_restricted() {
                    "현재 채팅 참여가 제한되어 있습니다.".into()
                } else {
                    "채팅 연결과 로그인 상태를 확인해 주세요.".into()
                });
                return MessageSendOutcome::Failed;
            }
            let Some(engine) = s.chat_engine.clone() else {
                s.error_message = Some("채팅 연결을 확인해 주세요.".into());
                return MessageSendOutcome::Failed;
            };
            s.error_message = None;
            let emojis = ChatState::resolve_outgoing_emojis(&text, &s.outgoing_emoticon_lookup());
            (
                engine,
                s.chat_session_generation,
                s.active_chat_channel_id(),
                s.current_user_uid.clone().unwrap_or_else(|| "me".into()),
                s.current_user_nickname
                    .clone()
                    .unwrap_or_else(|| "나".into()),
                emojis,
            )
        };
        let result = engine.send_message(&text, &emojis).await;
        let mut s = self.lock();
        let same_session = session_generation == s.chat_session_generation
            && s.chat_engine
                .as_ref()
                .is_some_and(|e| Arc::ptr_eq(e, &engine))
            && s.active_chat_channel_id() == channel_identity;
        if !same_session {
            return MessageSendOutcome::SessionChanged;
        }
        match result {
            Ok(tid) => {
                let now = Utc::now();
                let id = format!("{}_{}", local_user_id, now.timestamp_micros());
                let local = ChatMessageItem::new(
                    id.clone(),
                    local_user_id,
                    local_nickname,
                    text,
                    now,
                    MessageType::Normal,
                    emojis,
                    false,
                    false,
                );
                s.append_to_history(vec![local.clone()]);
                let echo_key = format!("{}_{}", local.user_id, local.content);
                s.append_message(local);
                s.recent_sent_echo_keys
                    .insert(echo_key.clone(), now + ChronoDuration::seconds(5));
                if s.remember_pending_echo(tid, &id, &echo_key) {
                    MessageSendOutcome::Failed
                } else {
                    MessageSendOutcome::Sent
                }
            }
            Err(e) => {
                s.error_message = Some(format!("메시지 전송 실패: {e}"));
                MessageSendOutcome::Failed
            }
        }
    }
}

impl ChatState {
    /// 모더레이션 명령이면 처리하고 결과를 돌려준다(Swift `sendMessage` 앞부분).
    fn try_handle_command(&mut self, text: &str) -> Option<MessageSendOutcome> {
        let service = self.moderation_service.clone()?;
        let parsed = ChatCommand::parse(text);
        let mutates_policy = parsed.as_ref().is_some_and(|(c, _)| {
            matches!(
                c,
                ChatCommand::Mute
                    | ChatCommand::Unmute
                    | ChatCommand::Ban
                    | ChatCommand::Unban
                    | ChatCommand::Filter
                    | ChatCommand::Block
                    | ChatCommand::Unblock
            )
        });
        if mutates_policy {
            self.message_filter_generation = self.message_filter_generation.wrapping_add(1);
        }
        let result = service.process_command(text)?;
        if mutates_policy {
            if let Some((command, args)) = &parsed {
                if !args.is_empty()
                    && matches!(
                        command,
                        ChatCommand::Mute
                            | ChatCommand::Ban
                            | ChatCommand::Filter
                            | ChatCommand::Block
                    )
                {
                    self.has_active_moderation = true;
                }
                self.refresh_has_active_moderation();
                match command {
                    ChatCommand::Mute | ChatCommand::Ban | ChatCommand::Block => {
                        if let Some(user_id) = args.first() {
                            self.apply_mutation(ChatMessageMutation::PurgeUser {
                                user_id: user_id.clone(),
                            });
                        }
                        self.refilter_messages();
                    }
                    ChatCommand::Unmute
                    | ChatCommand::Unban
                    | ChatCommand::Unblock
                    | ChatCommand::Filter => {
                        self.refilter_messages();
                    }
                    _ => {}
                }
                if matches!(
                    command,
                    ChatCommand::Ban
                        | ChatCommand::Unban
                        | ChatCommand::Block
                        | ChatCommand::Unblock
                ) {
                    let blocked: Vec<String> = service.blocked_users().into_iter().collect();
                    if let Some(cb) = &self.on_blocked_users_changed {
                        cb(blocked);
                    }
                    if let (Some(id), Some(cb)) = (args.first(), &self.on_blocked_user_mutation) {
                        cb(id, matches!(command, ChatCommand::Ban | ChatCommand::Block));
                    }
                }
            }
        }
        self.handle_command_result(result);
        Some(MessageSendOutcome::HandledLocally)
    }

    /// 전송 tid 와 로컬 에코를 묶는다. 이미 거절이 와 있었으면 즉시 회수하고 `true`.
    pub fn remember_pending_echo(&mut self, tid: i64, message_id: &str, echo_key: &str) -> bool {
        let now = Utc::now();
        if !self.pending_sent_echoes.is_empty() {
            self.pending_sent_echoes.retain(|_, e| e.expiry > now);
        }
        self.pending_sent_echoes.insert(
            tid,
            PendingSentEcho {
                message_id: message_id.into(),
                echo_key: echo_key.into(),
                expiry: now + ChronoDuration::seconds(Self::PENDING_SENT_ECHO_TTL_SECS),
                session_generation: self.chat_session_generation,
            },
        );
        if self.pending_sent_echoes.len() > Self::PENDING_SENT_ECHO_CAP {
            let excess = self.pending_sent_echoes.len() - Self::PENDING_SENT_ECHO_CAP;
            let mut by_expiry: Vec<(i64, chrono::DateTime<Utc>)> = self
                .pending_sent_echoes
                .iter()
                .map(|(k, v)| (*k, v.expiry))
                .collect();
            by_expiry.sort_by_key(|(_, e)| *e);
            for (k, _) in by_expiry.into_iter().take(excess) {
                self.pending_sent_echoes.remove(&k);
            }
        }
        if !self.consume_early_send_rejection(tid, now) {
            return false;
        }
        self.reclaim_rejected_echo(Some(tid))
    }

    pub fn remember_early_send_rejection(&mut self, tid: i64) {
        let now = Utc::now();
        let generation = self.chat_session_generation;
        if !self.early_rejected_sends.is_empty() {
            self.early_rejected_sends
                .retain(|_, r| r.expiry > now && r.session_generation == generation);
        }
        self.early_rejected_sends.insert(
            tid,
            EarlySendRejection {
                expiry: now + ChronoDuration::seconds(Self::EARLY_SEND_REJECTION_TTL_SECS),
                session_generation: generation,
            },
        );
        if self.early_rejected_sends.len() > Self::EARLY_SEND_REJECTION_CAP {
            let excess = self.early_rejected_sends.len() - Self::EARLY_SEND_REJECTION_CAP;
            let mut by_expiry: Vec<(i64, chrono::DateTime<Utc>)> = self
                .early_rejected_sends
                .iter()
                .map(|(k, v)| (*k, v.expiry))
                .collect();
            by_expiry.sort_by_key(|(_, e)| *e);
            for (k, _) in by_expiry.into_iter().take(excess) {
                self.early_rejected_sends.remove(&k);
            }
        }
    }

    fn consume_early_send_rejection(&mut self, tid: i64, now: chrono::DateTime<Utc>) -> bool {
        let Some(r) = self.early_rejected_sends.remove(&tid) else {
            return false;
        };
        r.expiry > now && r.session_generation == self.chat_session_generation
    }

    /// 거절된 tid 의 로컬 에코를 네 버퍼에서 지운다.
    pub fn reclaim_rejected_echo(&mut self, tid: Option<i64>) -> bool {
        let Some(tid) = tid else { return false };
        let Some(pending) = self.pending_sent_echoes.remove(&tid) else {
            return false;
        };
        if pending.expiry <= Utc::now()
            || pending.session_generation != self.chat_session_generation
        {
            return false;
        }
        self.apply_mutation(ChatMessageMutation::Remove {
            message_id: pending.message_id,
        });
        self.recent_sent_echo_keys.remove(&pending.echo_key);
        true
    }

    pub fn clear_messages(&mut self) {
        self.messages.clear();
        self.last_read_evicted = false;
        self.messages_revision = self.messages_revision.wrapping_add(1);
        self.replay_hold_queue.clear();
        self.reset_pending_queue();
        self.seen_message_ids.clear();
        self.seen_message_id_queue.clear();
        cancel_slot(&mut self.batch_flush_task);
        self.message_count = 0;
        self.chat_history.clear();
        self.has_chat_history = false;
        self.reset_incremental_stats();
        self.exit_replay_mode();
        if let Some(e) = &self.chat_engine {
            e.clear_messages();
        }
    }

    pub fn toggle_auto_scroll(&mut self) {
        self.is_auto_scroll_enabled = !self.is_auto_scroll_enabled;
        if !self.is_auto_scroll_enabled {
            self.enter_replay_mode();
        } else {
            self.exit_replay_mode();
        }
    }

    pub fn cancel_replay_debounce(&mut self) {
        cancel_slot(&mut self.replay_debounce_task);
    }

    /// 스크롤 위치 변화 — 바닥이면 즉시 복귀, 위로 올라가면 150ms 디바운스 후 읽기 모드.
    pub fn on_scroll_position_changed(&mut self, is_near_bottom: bool) {
        if is_near_bottom {
            cancel_slot(&mut self.replay_debounce_task);
            if self.is_replay_mode {
                self.exit_replay_mode();
            }
        } else {
            if self.is_replay_mode
                || self.messages.len() <= 3
                || self.replay_debounce_task.is_some()
            {
                return;
            }
            self.replay_debounce_task =
                Some(self.spawn_timer(Duration::from_millis(150), |s, id| {
                    if !slot_is(&s.replay_debounce_task, id) || s.is_replay_mode {
                        return;
                    }
                    s.enter_replay_mode();
                    s.replay_debounce_task = None;
                }));
        }
    }

    pub fn enter_replay_mode(&mut self) {
        if self.is_replay_mode {
            return;
        }
        self.is_replay_mode = true;
        self.unread_count = 0;
        self.is_auto_scroll_enabled = false;
    }

    pub fn exit_replay_mode(&mut self) {
        cancel_slot(&mut self.replay_debounce_task);
        self.is_replay_mode = false;
        self.unread_count = 0;
        self.is_auto_scroll_enabled = true;
        if !self.replay_hold_queue.is_empty() {
            let held = std::mem::take(&mut self.replay_hold_queue);
            self.mutate_messages(|m| m.extend(held));
        }
        if self.pending_count() > 0 {
            cancel_slot(&mut self.batch_flush_task);
            self.schedule_batch_flush();
        }
    }

    pub fn append_to_history(&mut self, items: Vec<ChatMessageItem>) {
        let cap = if self.is_background_mode() {
            Self::BACKGROUND_HISTORY_SIZE
        } else {
            Self::MAX_HISTORY_SIZE
        };
        if self.chat_history.is_empty() {
            let initial =
                (cap + Self::HISTORY_TRIM_SLACK).min(Self::HISTORY_TRIM_SLACK.max(items.len()));
            self.chat_history.reserve(initial);
        }
        self.chat_history.extend(items);
        if self.chat_history.len() > cap + Self::HISTORY_TRIM_SLACK {
            let excess = self.chat_history.len() - cap;
            self.chat_history.drain(..excess);
        }
        if !self.has_chat_history {
            self.has_chat_history = true;
        }
    }
}
