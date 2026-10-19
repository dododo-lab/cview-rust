//! 플레이어 위 후원·구독 알림 큐.
//!
//! 원본: `Sources/CViewApp/ViewModels/ChatViewModel+StreamAlert.swift`.

use std::time::Duration;

use chrono::Duration as ChronoDuration;

use cview_core::models::stream_alert::{StreamAlertItem, StreamAlertType};

use super::ChatState;

impl ChatState {
    pub const MAX_VISIBLE_ALERTS: usize = 3;
    pub const ALERT_DISMISS_DELAY: Duration = Duration::from_secs(5);
    pub const ALERT_MERGE_WINDOW_SECS: i64 = 5;

    /// 같은 닉네임·유형이 5초 안에 다시 오면 합치고, 아니면 최대 3개까지 쌓는다.
    pub fn enqueue_stream_alert(&mut self, alert: StreamAlertItem) {
        if !self.is_stream_alert_enabled {
            return;
        }
        let window = ChronoDuration::seconds(Self::ALERT_MERGE_WINDOW_SECS);
        if let Some(idx) = self.stream_alerts.iter().rposition(|a| {
            a.nickname == alert.nickname
                && a.alert_type == alert.alert_type
                && alert.timestamp - a.timestamp < window
        }) {
            let merged = Self::merge_stream_alert(&self.stream_alerts[idx], &alert);
            let id = merged.id.clone();
            self.stream_alerts[idx] = merged;
            self.arm_stream_alert_dismiss_timer(&id);
            return;
        }
        if self.stream_alerts.len() >= Self::MAX_VISIBLE_ALERTS {
            self.stream_alerts.remove(0);
        }
        let id = alert.id.clone();
        self.stream_alerts.push(alert);
        self.arm_stream_alert_dismiss_timer(&id);
    }

    fn arm_stream_alert_dismiss_timer(&mut self, alert_id: &str) {
        if let Some(t) = self.stream_alert_dismiss_tasks.remove(alert_id) {
            t.cancel();
        }
        let aid = alert_id.to_string();
        let slot = self.spawn_timer(Self::ALERT_DISMISS_DELAY, move |s, id| {
            if s.stream_alert_dismiss_tasks.get(&aid).map(|t| t.id()) != Some(id) {
                return;
            }
            s.stream_alerts.retain(|a| a.id != aid);
            s.stream_alert_dismiss_tasks.remove(&aid);
        });
        self.stream_alert_dismiss_tasks
            .insert(alert_id.to_string(), slot);
    }

    /// 마우스를 올린 동안 자동 닫힘을 멈춘다.
    pub fn freeze_stream_alert_dismissal(&mut self) {
        self.cancel_all_stream_alert_dismiss_tasks();
    }

    pub fn rearm_stream_alert_dismissal(&mut self) {
        let ids: Vec<String> = self.stream_alerts.iter().map(|a| a.id.clone()).collect();
        for id in ids {
            self.arm_stream_alert_dismiss_timer(&id);
        }
    }

    pub fn cancel_all_stream_alert_dismiss_tasks(&mut self) {
        for (_, t) in self.stream_alert_dismiss_tasks.drain() {
            t.cancel();
        }
    }

    fn merge_stream_alert(existing: &StreamAlertItem, newer: &StreamAlertItem) -> StreamAlertItem {
        let is_donation = matches!(
            existing.alert_type,
            StreamAlertType::Donation
                | StreamAlertType::VideoDonation
                | StreamAlertType::MissionDonation
        );
        let amount = if is_donation {
            Some(existing.donation_amount.unwrap_or(0) + newer.donation_amount.unwrap_or(0))
        } else {
            newer.donation_amount
        };
        StreamAlertItem {
            id: existing.id.clone(),
            alert_type: existing.alert_type,
            nickname: existing.nickname.clone(),
            content: if newer.content.is_empty() {
                existing.content.clone()
            } else {
                newer.content.clone()
            },
            timestamp: newer.timestamp,
            donation_amount: amount,
            donation_type: existing
                .donation_type
                .clone()
                .or_else(|| newer.donation_type.clone()),
            subscription_months: newer.subscription_months.or(existing.subscription_months),
        }
    }

    pub fn dismiss_stream_alert(&mut self, id: &str) {
        self.stream_alerts.retain(|a| a.id != id);
        if let Some(t) = self.stream_alert_dismiss_tasks.remove(id) {
            t.cancel();
        }
    }
}
