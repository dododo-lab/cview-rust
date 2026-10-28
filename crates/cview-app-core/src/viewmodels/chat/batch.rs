//! 수신 메시지 대기열·드립 커밋·backlog 보류.
//!
//! 원본: `Sources/CViewApp/ViewModels/ChatViewModel+Batch.swift`.

use std::collections::HashSet;
use std::time::Duration;

use chrono::{DateTime, Utc};
use rand::Rng;

use cview_chat::utils::ChatCommitBackpressure;
use cview_core::models::chat::{ChatMessageItem, ChatSystemNoticeKind, MessageType};

use super::pacing::{drip, flush};
use super::{uptime, ChatState};
use crate::viewmodels::support::{cancel_slot, format_number, slot_is};

impl ChatState {
    pub fn reset_pending_queue(&mut self) {
        self.commit_backpressure = ChatCommitBackpressure::new();
        self.last_live_flush_uptime = None;
        self.pending_messages.clear();
        self.pending_head = 0;
        self.pending_first_enqueued_at = None;
        self.is_drip_catching_up = false;
        self.drip_projected_lag_ms = 0.0;
        self.pending_drop_notice_backlog = 0;
        self.last_drip_interval_ns =
            drip::NOMINAL_DRIP_INTERVAL_NS.max(self.chat_update_interval_floor_ns());
    }

    /// 끊을 때 버리는 미커밋 대기분은 중복 방지 집합에서도 지운다 — 같은 채널 재연결 backlog 로 다시 보이게.
    pub fn forget_uncommitted_pending_ids(&mut self) {
        if self.pending_count() == 0 {
            return;
        }
        let dropped: HashSet<String> = self.pending_messages[self.pending_head..]
            .iter()
            .map(|i| i.id.clone())
            .collect();
        for id in &dropped {
            self.seen_message_ids.remove(id);
        }
        self.seen_message_id_queue
            .retain(|id| !dropped.contains(id));
    }

    pub fn dedupe_for_ingress(&mut self, items: Vec<ChatMessageItem>) -> Vec<ChatMessageItem> {
        let now = Utc::now();
        if !self.recent_sent_echo_keys.is_empty() {
            self.recent_sent_echo_keys.retain(|_, exp| *exp > now);
        }
        let echo_active = !self.recent_sent_echo_keys.is_empty();
        let mut unique = Vec::with_capacity(items.len());
        for item in items {
            if !self.seen_message_ids.insert(item.id.clone()) {
                continue;
            }
            self.seen_message_id_queue.push(item.id.clone());
            if echo_active {
                let key = format!("{}_{}", item.user_id, item.content);
                if let Some(exp) = self.recent_sent_echo_keys.get(&key) {
                    if now < *exp {
                        self.recent_sent_echo_keys.remove(&key);
                        continue;
                    }
                }
            }
            unique.push(item);
        }
        self.trim_seen_ids();
        unique
    }

    pub(crate) fn trim_seen_ids(&mut self) {
        let (cap, keep) = if self.is_background_mode() {
            (300, 150)
        } else {
            (800, 400)
        };
        if self.seen_message_id_queue.len() > cap {
            let remove = self.seen_message_id_queue.len() - keep;
            for id in self.seen_message_id_queue.drain(..remove) {
                self.seen_message_ids.remove(&id);
            }
        }
    }

    pub fn pending_count(&self) -> usize {
        self.pending_messages
            .len()
            .saturating_sub(self.pending_head)
    }

    pub fn enqueue_batched_messages(&mut self, items: Vec<ChatMessageItem>) {
        if items.is_empty() {
            return;
        }
        let unique = self.dedupe_for_ingress(items);
        if unique.is_empty() {
            return;
        }
        if self.pending_head > self.pending_messages.len() {
            tracing::error!(
                "Chat queue invariant violated: pendingHead={} > count={}. Resetting head.",
                self.pending_head,
                self.pending_messages.len()
            );
            self.pending_head = 0;
            self.pending_messages.clear();
        }
        if self.pending_count() == 0 {
            self.pending_first_enqueued_at = Some(Utc::now());
        }
        let n = unique.len();
        self.pending_messages.extend(unique);
        if !self.uses_coalesced_commit_pacing() {
            let now = Utc::now();
            self.recent_message_timestamps
                .extend(std::iter::repeat_n(now, n));
        }
        let effective_cap = if self.prefers_smooth_bottom_drip() {
            let assigned = self.assigned_merged_rows();
            drip::pending_soft_cap(
                self.last_drip_interval_ns,
                self.co_active_foreground_sessions,
                self.pending_hard_cap,
                assigned,
            )
        } else {
            self.pending_hard_cap
        };
        let pending_count = self.pending_count() as i64;
        if pending_count > effective_cap {
            let drop_count = (pending_count - effective_cap) as usize;
            let drop_end = self.pending_head + drop_count;
            let dropped = self.pending_messages[self.pending_head..drop_end].to_vec();
            self.append_to_history(dropped);
            self.pending_head = drop_end;
            self.dropped_message_count += drop_count as i64;
            self.pending_drop_notice_backlog += drop_count as i64;
        }
        if self.is_holding_for_backlog {
            return;
        }
        let can_flush_now = self.permits_immediate_chat_flush(uptime());
        let pending_count = self.pending_count();
        let coalesced = self.uses_coalesced_commit_pacing();
        let smooth = self.prefers_smooth_bottom_drip();
        let idle = self.batch_flush_task.is_none();
        if !coalesced
            && !self.is_replay_mode
            && !smooth
            && idle
            && can_flush_now
            && pending_count > 320
        {
            self.flush_pending_messages();
            return;
        }
        if smooth && idle && can_flush_now && pending_count == 1 {
            self.flush_pending_messages();
            return;
        }
        if !coalesced
            && !self.is_replay_mode
            && !smooth
            && idle
            && can_flush_now
            && pending_count == 1
        {
            self.flush_pending_messages();
            return;
        }
        self.schedule_batch_flush();
    }

    pub fn begin_backlog_hold(&mut self) {
        cancel_slot(&mut self.backlog_hold_task);
        self.is_holding_for_backlog = true;
        self.backlog_hold_task = Some(self.spawn_timer(Self::BACKLOG_HOLD_TIMEOUT, |s, id| {
            if slot_is(&s.backlog_hold_task, id) {
                s.end_backlog_hold();
            }
        }));
    }

    pub fn end_backlog_hold(&mut self) {
        cancel_slot(&mut self.backlog_hold_task);
        if !self.is_holding_for_backlog {
            return;
        }
        self.is_holding_for_backlog = false;
        if self.pending_count() > 0 {
            self.flush_pending_messages();
        }
    }

    /// 접속 직후 backlog — 드립 없이 한 번에 커밋하고 보류를 푼다.
    pub fn commit_backlog_messages(&mut self, items: Vec<ChatMessageItem>) {
        let owns_hold = self.is_holding_for_backlog;
        self.commit_backlog_inner(items, owns_hold);
        self.end_backlog_hold();
    }

    fn commit_backlog_inner(&mut self, items: Vec<ChatMessageItem>, owns_hold: bool) {
        if items.is_empty() {
            return;
        }
        let unique = self.dedupe_for_ingress(items);
        let queued: Vec<ChatMessageItem> = if self.pending_count() > 0 {
            self.pending_messages[self.pending_head..].to_vec()
        } else {
            vec![]
        };
        if unique.is_empty() && queued.is_empty() {
            return;
        }
        cancel_slot(&mut self.batch_flush_task);
        self.reset_pending_queue();
        let unique_for_reposition = unique.clone();
        if Self::queued_precedes_backlog(&queued, &unique) {
            self.commit_messages(queued, false, None);
            self.commit_messages(unique, true, None);
        } else {
            self.commit_messages(unique, true, None);
            self.commit_messages(queued, false, None);
        }
        if !owns_hold && !self.is_replay_mode && !unique_for_reposition.is_empty() {
            self.reposition_late_backlog_rows(&unique_for_reposition);
        }
    }

    pub fn queued_precedes_backlog(
        queued: &[ChatMessageItem],
        backlog: &[ChatMessageItem],
    ) -> bool {
        let (Some(newest_queued), Some(oldest_backlog)) = (
            queued.iter().map(|i| i.timestamp).max(),
            backlog.iter().map(|i| i.timestamp).min(),
        ) else {
            return false;
        };
        newest_queued < oldest_backlog
    }

    fn reposition_late_backlog_rows(&mut self, backlog: &[ChatMessageItem]) {
        let Some(pivot) = backlog.iter().map(|i| i.timestamp).max() else {
            return;
        };
        let current = self.messages.to_vec();
        if current.is_empty() {
            return;
        }
        let backlog_ids: HashSet<&str> = backlog.iter().map(|i| i.id.as_str()).collect();
        let (moved, others): (Vec<_>, Vec<_>) = current
            .iter()
            .cloned()
            .partition(|i| backlog_ids.contains(i.id.as_str()));
        if moved.is_empty() {
            return;
        }
        let Some(insert_at) = others.iter().position(|i| i.timestamp > pivot) else {
            return;
        };
        let mut reordered = Vec::with_capacity(current.len());
        reordered.extend_from_slice(&others[..insert_at]);
        reordered.extend(moved);
        reordered.extend_from_slice(&others[insert_at..]);
        if reordered
            .iter()
            .map(|i| &i.id)
            .eq(current.iter().map(|i| &i.id))
        {
            return;
        }
        self.mutate_messages(|m| m.replace_all(reordered));
        self.last_read_evicted = false;
    }

    /// `(intervalNs, preferLowLatency)`.
    pub fn current_flush_pacing(&mut self) -> (u64, bool) {
        let coalesced = self.uses_coalesced_commit_pacing();
        let base_interval = if self.is_background_mode() {
            Self::BACKGROUND_FLUSH_INTERVAL_NS
        } else if self.is_presentation_suspended() {
            Self::SUSPENDED_FLUSH_INTERVAL_NS
        } else if self.is_replay_mode {
            Self::BACKGROUND_FLUSH_INTERVAL_NS.min(500_000_000)
        } else {
            Self::BATCH_FLUSH_INTERVAL_NS
        };
        let queue_lag_ms = self.queue_lag_ms();
        let pending = self.pending_count() as i64;
        let prefer_low_latency = !coalesced
            && self.is_auto_scroll_enabled
            && !self.is_replay_mode
            && (pending > 40 || queue_lag_ms > 80.0);
        let burst_factor = if coalesced || self.is_replay_mode {
            1.0
        } else if self.prefers_smooth_bottom_drip() {
            drip::dadada_burst_factor(pending)
        } else {
            match pending {
                i64::MIN..=20 => 1.0,
                21..=80 => {
                    if prefer_low_latency {
                        0.68
                    } else {
                        0.75
                    }
                }
                81..=200 => {
                    if prefer_low_latency {
                        0.45
                    } else {
                        0.55
                    }
                }
                _ => {
                    if prefer_low_latency {
                        0.26
                    } else {
                        0.40
                    }
                }
            }
        };
        let adjusted = self
            .deps
            .pacing
            .system_load_mode()
            .adjusted_chat_flush_interval_ns(base_interval);
        let mut interval = (adjusted as f64 * burst_factor) as u64;
        if !coalesced && self.co_active_foreground_sessions > 1 {
            let floor = flush::merged_drip_floor_ns(adjusted, self.co_active_foreground_sessions);
            interval = interval.max(floor);
        }
        interval = flush::render_contention_limited_interval_ns(
            interval,
            Self::ECONOMY_DRIP_FLOOR_NS,
            coalesced,
            self.deps.pacing.prefers_relaxed_commit_pacing(),
            self.deps.pacing.is_window_geometry_transitioning(),
        );
        (
            self.process_limited_chat_interval(interval),
            prefer_low_latency,
        )
    }

    fn queue_lag_ms(&self) -> f64 {
        self.pending_first_enqueued_at.map_or(0.0, |first| {
            (Utc::now() - first).num_microseconds().unwrap_or(0) as f64 / 1000.0
        })
    }

    pub fn schedule_batch_flush(&mut self) {
        if self.batch_flush_task.is_some()
            || self.is_holding_for_backlog
            || self.pending_count() == 0
        {
            return;
        }
        let (interval, prefer_low_latency) = self.current_flush_pacing();
        let coalesced = self.uses_coalesced_commit_pacing();
        self.last_drip_interval_ns = if coalesced {
            drip::NOMINAL_DRIP_INTERVAL_NS.max(self.chat_update_interval_floor_ns())
        } else {
            interval
        };
        let jitter: u64 = if coalesced {
            0
        } else if prefer_low_latency {
            rand::thread_rng().gen_range(0..=2_000_000)
        } else {
            rand::thread_rng().gen_range(0..=8_000_000)
        };
        self.batch_flush_task = Some(self.spawn_timer(
            Duration::from_nanos(interval + jitter),
            |s, id| {
                if slot_is(&s.batch_flush_task, id) {
                    s.flush_pending_messages();
                }
            },
        ));
    }

    pub fn flush_pending_messages(&mut self) {
        self.batch_flush_task = None;
        if self.is_holding_for_backlog || self.pending_count() == 0 {
            return;
        }
        if self.uses_coalesced_commit_pacing() || self.is_replay_mode {
            let to_flush = self.pending_messages.split_off(self.pending_head);
            self.pending_messages.clear();
            self.pending_head = 0;
            self.pending_first_enqueued_at = None;
            let notice = self.take_drop_notice_if_due(Utc::now());
            self.commit_messages(to_flush, false, notice);
            return;
        }
        let count = self.pending_count() as i64;
        let queue_lag_ms = self.queue_lag_ms();
        let flush_count = if self.prefers_smooth_bottom_drip() {
            let assigned = self.assigned_merged_rows();
            let plan = drip::plan(
                count,
                self.last_drip_interval_ns,
                &self.pending_messages,
                self.pending_head,
                self.co_active_foreground_sessions,
                self.is_drip_catching_up,
                assigned,
            );
            self.is_drip_catching_up = plan.is_catching_up;
            self.drip_projected_lag_ms = plan.projected_lag_ms;
            plan.flush_count
        } else {
            self.is_drip_catching_up = false;
            self.drip_projected_lag_ms = 0.0;
            let mut n = flush::base_flush_count(count);
            let prefer_low_latency = self.is_auto_scroll_enabled
                && !self.is_replay_mode
                && (count > 40 || queue_lag_ms > 80.0);
            let raw_budget = flush::adaptive_budget_px(count, prefer_low_latency);
            let budget_px = if let Some(row_share) = self.assigned_merged_rows() {
                n = n.min(row_share);
                row_share * flush::LONG_ROW_ESTIMATE_PX
            } else {
                flush::co_active_scaled_budget_px(raw_budget, self.co_active_foreground_sessions)
            };
            flush::capped_flush_count(n, &self.pending_messages, self.pending_head, budget_px)
        };
        let end =
            (self.pending_head + flush_count.max(0) as usize).min(self.pending_messages.len());
        let to_flush = self.pending_messages[self.pending_head..end].to_vec();
        self.pending_head = end;
        if self.pending_count() == 0 {
            self.pending_first_enqueued_at = None;
        }
        if self.pending_head > 200 || (self.pending_head > 50 && self.pending_count() == 0) {
            self.pending_messages.drain(..self.pending_head);
            self.pending_head = 0;
        }
        self.last_live_flush_uptime = Some(uptime());
        let notice = self.take_drop_notice_if_due(Utc::now());
        self.commit_messages(to_flush, false, notice);
        if self.pending_count() > 0 {
            self.schedule_batch_flush();
        }
    }

    /// 건너뛴 메시지 안내 — 5초에 한 번만.
    pub fn take_drop_notice_if_due(&mut self, now: DateTime<Utc>) -> Option<ChatMessageItem> {
        if self.pending_drop_notice_backlog <= 0 {
            return None;
        }
        if let Some(last) = self.last_drop_notice_at {
            let elapsed = (now - last).num_milliseconds() as f64 / 1000.0;
            if elapsed < Self::DROP_NOTICE_MIN_INTERVAL_SEC {
                return None;
            }
        }
        let skipped = self.pending_drop_notice_backlog;
        self.pending_drop_notice_backlog = 0;
        self.last_drop_notice_at = Some(now);
        Some(ChatMessageItem::system(
            format!(
                "채팅 속도가 표시 한계를 넘어 메시지 {}개를 건너뛰었습니다.",
                format_number(skipped)
            ),
            ChatSystemNoticeKind::Warning,
        ))
    }

    pub(crate) fn commit_messages(
        &mut self,
        msgs: Vec<ChatMessageItem>,
        is_backlog: bool,
        drop_notice: Option<ChatMessageItem>,
    ) {
        if msgs.is_empty() {
            return;
        }
        let t0 = std::time::Instant::now();
        if !self.uses_batch_flush_mode() && !is_backlog {
            for m in &msgs {
                if m.r#type == MessageType::Donation || m.r#type == MessageType::Subscription {
                    self.deps.tts.enqueue(m);
                }
            }
        }
        let mut visible = Vec::with_capacity(msgs.len() + 1);
        if let Some(n) = drop_notice {
            visible.push(n);
        }
        visible.extend(msgs.iter().cloned());
        self.track_recent_chatters(&msgs);
        self.append_to_history(visible.clone());
        if self.is_replay_mode && !is_backlog {
            self.unread_count += msgs.len() as i64;
        }
        self.update_incremental_stats(&msgs);
        self.message_count += msgs.len() as i64;
        if self.uses_batch_flush_mode() && !is_backlog {
            self.unread_while_background += msgs.len() as i64;
        }
        if self.is_replay_mode {
            self.replay_hold_queue.extend(visible);
            if self.replay_hold_queue.len() > self.max_visible_messages {
                let excess = self.replay_hold_queue.len() - self.max_visible_messages;
                self.replay_hold_queue.drain(..excess);
            }
        } else {
            self.mutate_messages(|m| m.extend(visible));
        }
        if !is_backlog {
            self.detect_mentions(&msgs);
        }
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        if !self.uses_coalesced_commit_pacing() && !is_backlog && !self.is_replay_mode {
            self.commit_backpressure.record(ms, uptime());
        }
        if ms > 5.0 {
            let now = Utc::now();
            let cooldown = Self::slow_commit_diagnostic_cooldown(ms);
            let recent = self
                .last_slow_commit_diagnostic_at
                .is_some_and(|l| ((now - l).num_milliseconds() as f64 / 1000.0) < cooldown);
            if recent {
                self.suppressed_slow_commit_diagnostics += 1;
            } else {
                self.last_slow_commit_diagnostic_at = Some(now);
                let suppressed = std::mem::take(&mut self.suppressed_slow_commit_diagnostics);
                tracing::warn!(
                    "Slow commitMessages: {ms:.1}ms for {} msgs (msgs={} history={} suppressed={suppressed})",
                    msgs.len(),
                    self.messages.len(),
                    self.chat_history.len()
                );
            }
        }
    }
}
