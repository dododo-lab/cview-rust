//! 설정 반영·내보내기, 표시/키워드 정책 재필터 예약, 갱신 빈도 상한.
//!
//! 원본: `Sources/CViewApp/ViewModels/ChatViewModel+SettingsSync.swift`.

use std::time::Duration;

use super::{uptime, ChatSettings, ChatState};
use crate::viewmodels::support::{cancel_slot, slot_is};

impl ChatState {
    /// 소유 화면이 보이는지 — 안 보이면 1초 묶음 커밋으로 바뀐다.
    pub fn set_owner_presentation_active(&mut self, active: bool) {
        let suspended = !active;
        if self.is_owner_presentation_suspended == suspended {
            return;
        }
        self.is_owner_presentation_suspended = suspended;
        cancel_slot(&mut self.batch_flush_task);
        if self.pending_count() > 0 {
            self.schedule_batch_flush();
        }
    }

    pub fn chat_update_interval_floor_ns(&self) -> u64 {
        (1_000_000_000f64 / self.max_chat_update_rate as f64).ceil() as u64
    }

    pub fn apply_chat_update_rate(&mut self, rate: i64) {
        let rate = rate.clamp(10, 60);
        if self.max_chat_update_rate == rate {
            return;
        }
        self.max_chat_update_rate = rate;
        self.last_drip_interval_ns =
            Self::BATCH_FLUSH_INTERVAL_NS.max(self.chat_update_interval_floor_ns());
        cancel_slot(&mut self.batch_flush_task);
        if self.pending_count() > 0 {
            self.schedule_batch_flush();
        }
    }

    pub fn schedule_display_policy_refilter(&mut self) {
        if self.is_applying_chat_settings {
            return;
        }
        self.message_filter_generation = self.message_filter_generation.wrapping_add(1);
        self.enqueue_display_policy_refilter();
    }

    /// 다음 틱에 한 번 재필터 — 연속 토글은 마지막 것만 실행된다.
    fn enqueue_display_policy_refilter(&mut self) {
        self.display_policy_task_generation = self.display_policy_task_generation.wrapping_add(1);
        let generation = self.display_policy_task_generation;
        cancel_slot(&mut self.display_policy_refilter_task);
        self.display_policy_refilter_task = Some(self.spawn_timer(Duration::ZERO, move |s, id| {
            if !slot_is(&s.display_policy_refilter_task, id)
                || s.display_policy_task_generation != generation
            {
                return;
            }
            s.display_policy_refilter_task = None;
            s.refilter_messages();
        }));
    }

    /// 키워드 정책을 모더레이션 서비스에 반영하는 작업을 예약한다(직전 예약은 대체).
    pub fn schedule_keyword_policy_sync(&mut self) {
        self.settings_sync_task_generation = self.settings_sync_task_generation.wrapping_add(1);
        let task_generation = self.settings_sync_task_generation;
        let keyword_generation = self.keyword_policy_generation;
        let session_generation = self.chat_session_generation;
        cancel_slot(&mut self.settings_moderation_sync_task);
        self.settings_moderation_sync_task =
            Some(self.spawn_timer(Duration::ZERO, move |s, id| {
                if !slot_is(&s.settings_moderation_sync_task, id)
                    || s.settings_sync_task_generation != task_generation
                    || s.keyword_policy_generation != keyword_generation
                    || s.chat_session_generation != session_generation
                {
                    return;
                }
                s.settings_moderation_sync_task = None;
                s.apply_keyword_policy_now();
            }));
    }

    /// 예약 없이 즉시 반영.
    pub fn apply_keyword_policy_now(&mut self) {
        cancel_slot(&mut self.settings_moderation_sync_task);
        let Some(service) = self.moderation_service.clone() else {
            return;
        };
        let words = self.blocked_words.clone();
        let enabled = self.is_filter_enabled;
        service.set_keyword_filter(words.clone(), enabled);
        if enabled && !words.is_empty() {
            self.has_active_moderation = true;
        }
        self.refresh_has_active_moderation();
        self.refilter_messages();
    }

    pub fn apply_settings(&mut self, settings: &ChatSettings) {
        let display_changed = self.show_donation != settings.show_donation
            || self.show_donations_only != settings.show_donations_only;
        let moderation_changed = self.is_filter_enabled != settings.chat_filter_enabled
            || self.blocked_words != settings.blocked_words;
        if display_changed || moderation_changed {
            self.message_filter_generation = self.message_filter_generation.wrapping_add(1);
        }
        if moderation_changed {
            self.keyword_policy_generation = self.keyword_policy_generation.wrapping_add(1);
            self.moderation_state_generation = self.moderation_state_generation.wrapping_add(1);
        }
        self.is_applying_chat_settings = true;
        self.apply_chat_update_rate(settings.max_update_rate);
        self.font_size = settings.font_size;
        self.opacity = settings.chat_opacity;
        self.line_spacing = settings.line_spacing;
        self.show_timestamp = settings.show_timestamp;
        self.show_badge = settings.show_badge;
        self.highlight_mentions = settings.highlight_mentions;
        self.set_max_visible_messages(settings.max_visible_messages.max(0) as usize);
        self.emoticon_enabled = settings.emoticon_enabled;
        self.set_show_donation(settings.show_donation);
        self.set_show_donations_only(settings.show_donations_only);
        self.is_auto_scroll_enabled = settings.auto_scroll;
        self.is_filter_enabled = settings.chat_filter_enabled;
        self.blocked_words = settings.blocked_words.clone();
        self.initial_blocked_users = settings.blocked_users.clone();
        self.is_applying_chat_settings = false;
        if moderation_changed {
            self.schedule_keyword_policy_sync();
        } else if display_changed {
            self.enqueue_display_policy_refilter();
        }
        self.deps.tts.configure(
            settings.tts_enabled,
            settings.tts_volume,
            settings.tts_rate,
            settings.tts_voice_identifier.clone(),
        );
        self.display_mode = settings.display_mode;
        self.message_alignment = settings.message_alignment;
        self.overlay_width = settings.overlay_width;
        self.overlay_height = settings.overlay_height;
        self.overlay_background_opacity = settings.overlay_background_opacity;
        self.overlay_show_input = settings.overlay_show_input;
    }

    pub fn export_settings(&self, base: &ChatSettings) -> ChatSettings {
        let mut s = base.clone();
        s.font_size = self.font_size;
        s.max_update_rate = self.max_chat_update_rate;
        s.chat_opacity = self.opacity;
        s.line_spacing = self.line_spacing;
        s.show_timestamp = self.show_timestamp;
        s.show_badge = self.show_badge;
        s.highlight_mentions = self.highlight_mentions;
        s.max_visible_messages = self.max_visible_messages as i64;
        s.emoticon_enabled = self.emoticon_enabled;
        s.show_donation = self.show_donation;
        s.show_donations_only = self.show_donations_only;
        s.auto_scroll = self.is_auto_scroll_enabled;
        s.chat_filter_enabled = self.is_filter_enabled;
        s.blocked_words = self.blocked_words.clone();
        s.display_mode = self.display_mode;
        s.message_alignment = self.message_alignment;
        s.overlay_width = self.overlay_width;
        s.overlay_height = self.overlay_height;
        s.overlay_background_opacity = self.overlay_background_opacity;
        s.overlay_show_input = self.overlay_show_input;
        s
    }

    pub fn slow_commit_diagnostic_cooldown(milliseconds: f64) -> f64 {
        if milliseconds >= 33.0 {
            0.25
        } else {
            2.0
        }
    }

    /// 직전 실시간 커밋 후 갱신 빈도 상한만큼 지났는가.
    pub fn permits_immediate_chat_flush(&self, now: f64) -> bool {
        match self.last_live_flush_uptime {
            None => true,
            Some(last) => {
                now - last >= self.chat_update_interval_floor_ns() as f64 / 1_000_000_000.0
            }
        }
    }

    pub fn process_limited_chat_interval(&self, interval: u64) -> u64 {
        if self.uses_coalesced_commit_pacing() || self.is_replay_mode {
            return interval;
        }
        interval
            .max(self.chat_update_interval_floor_ns())
            .max(self.commit_backpressure.minimum_interval_ns(uptime()))
    }
}
