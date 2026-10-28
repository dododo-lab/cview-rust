//! 증분 통계·명령 결과 표시·초당 메시지 수·진단 하트비트.
//!
//! 원본: `Sources/CViewApp/ViewModels/ChatViewModel+Stats.swift`.

use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};

use cview_chat::ChatCommandResult;
use cview_core::models::chat::{
    ChatConnectionState, ChatMessageItem, ChatSystemNoticeKind, MessageType,
};

use super::pacing::drip;
use super::ChatState;
use crate::viewmodels::support::TaskSlot;

impl ChatState {
    pub const UNIQUE_USER_TRACKING_CAP: usize = 30_000;
    pub const STATS_INTERVAL: Duration = Duration::from_secs(3);

    pub fn update_incremental_stats(&mut self, batch: &[ChatMessageItem]) {
        let mut donation_delta = 0;
        let mut amount_delta = 0;
        let mut subscription_delta = 0;
        for msg in batch {
            if self.unique_users.len() < Self::UNIQUE_USER_TRACKING_CAP {
                self.unique_users.insert(msg.user_id.clone());
            }
            match msg.r#type {
                MessageType::Donation => {
                    donation_delta += 1;
                    if let Some(a) = msg.donation_amount {
                        amount_delta += a;
                    }
                }
                MessageType::Subscription => subscription_delta += 1,
                _ => {}
            }
        }
        self.donation_count += donation_delta;
        self.total_donation_amount += amount_delta;
        self.subscription_count += subscription_delta;
        self.unique_user_count = self.unique_users.len() as i64;
    }

    pub fn reset_incremental_stats(&mut self) {
        self.unique_users.clear();
        self.unique_user_count = 0;
        self.donation_count = 0;
        self.total_donation_amount = 0;
        self.subscription_count = 0;
    }

    pub fn handle_command_result(&mut self, result: ChatCommandResult) {
        match result {
            ChatCommandResult::LocalAction(msg) => {
                self.append_system(msg, ChatSystemNoticeKind::Info)
            }
            ChatCommandResult::ServerCommand { command, .. } => {
                tracing::info!("Server command not wired to API: {}", command.raw_value());
                let hint = format!(
                    "'{}' {} 명령은 관리자 권한이 필요합니다. 현재 클라이언트에서는 미지원이며, chzzk 웹사이트의 채팅창에서 실행할 수 있습니다.",
                    command.raw_value(),
                    command.description()
                );
                self.append_system(format!("ℹ️ {hint}"), ChatSystemNoticeKind::Info);
            }
            ChatCommandResult::ClearChat => self.clear_messages(),
            ChatCommandResult::ExportChat => self.show_export_sheet = true,
            ChatCommandResult::Error(msg) => {
                self.append_system(format!("⚠️ {msg}"), ChatSystemNoticeKind::Error)
            }
        }
    }

    pub fn start_stats_tracking(&mut self) {
        let weak = self.this.clone();
        self.stats_task = Some(TaskSlot::spawn(move |id| async move {
            let mut ticker = tokio::time::interval(Self::STATS_INTERVAL);
            ticker.tick().await;
            loop {
                ticker.tick().await;
                let Some(shared) = weak.upgrade() else { break };
                let mut s = shared.obs.lock();
                if !crate::viewmodels::support::slot_is(&s.stats_task, id) {
                    break;
                }
                if s.uses_coalesced_commit_pacing() {
                    continue;
                }
                s.tick_messages_per_second();
                s.log_chat_diagnostics_if_needed();
            }
        }));
    }

    /// 최근 5초 창의 초당 메시지 수.
    pub fn tick_messages_per_second(&mut self) {
        let cutoff = Utc::now() - ChronoDuration::seconds(5);
        let lo = self
            .recent_message_timestamps
            .partition_point(|t| *t < cutoff);
        if lo > 0 {
            self.recent_message_timestamps.drain(..lo);
        }
        let next = self.recent_message_timestamps.len() as f64 / 5.0;
        if self.messages_per_second != next {
            self.messages_per_second = next;
        }
    }

    pub fn diagnostic_log_cooldown(is_idle: bool, is_anomalous: bool) -> f64 {
        if is_anomalous {
            3.0
        } else if is_idle {
            60.0
        } else {
            30.0
        }
    }

    fn log_chat_diagnostics_if_needed(&mut self) {
        let now = Utc::now();
        let is_idle = self.recent_message_timestamps.is_empty();
        let elapsed = self.last_diagnostic_log_at.map_or(f64::INFINITY, |l| {
            (now - l).num_milliseconds() as f64 / 1000.0
        });
        let is_connected = self.connection_state.is_connected();
        let has_new_drop = self.dropped_message_count > self.last_diagnostic_dropped_count;
        let anomalous = !is_connected
            || self.is_drip_catching_up
            || self.drip_projected_lag_ms > drip::LAG_BUDGET_MS
            || has_new_drop;
        if elapsed < Self::diagnostic_log_cooldown(is_idle, anomalous) {
            return;
        }
        self.last_diagnostic_log_at = Some(now);
        self.last_diagnostic_dropped_count = self.dropped_message_count;
        let conn = match &self.connection_state {
            ChatConnectionState::Connected { server_index } => format!("connected({server_index})"),
            ChatConnectionState::Connecting => "connecting".into(),
            ChatConnectionState::Disconnected => "disconnected".into(),
            ChatConnectionState::Reconnecting { attempt } => format!("reconnecting({attempt})"),
            ChatConnectionState::Failed { reason } => {
                format!("failed({})", reason.chars().take(40).collect::<String>())
            }
        };
        tracing::debug!(
            "Chat heartbeat: conn={conn} msgs={} pendingCount={} pendingHead={} pendingArr={} flush={} mps={} replay={} lagMs={} catchUp={} dropped={}",
            self.messages.len(),
            self.pending_count(),
            self.pending_head,
            self.pending_messages.len(),
            if self.batch_flush_task.is_none() { "idle" } else { "scheduled" },
            self.messages_per_second,
            self.is_replay_mode,
            self.drip_projected_lag_ms as i64,
            self.is_drip_catching_up,
            self.dropped_message_count
        );
    }
}
