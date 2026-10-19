//! 차단·뮤트·키워드 필터·하이라이트·고정.
//!
//! 원본: `Sources/CViewApp/ViewModels/ChatViewModel+Moderation.swift`.
//! 모더레이션 서비스가 동기 API 라 Swift 의 `async` 메서드는 동기 메서드가 됐다.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use cview_chat::ChatModerationService;
use cview_core::models::chat::{ChatMessageItem, ChatSystemNoticeKind};

use super::{ChatMessageMutation, ChatState, ChatViewModel};

impl ChatState {
    /// 활성 규칙 여부를 다시 읽는다.
    pub fn refresh_has_active_moderation(&mut self) {
        self.moderation_state_generation = self.moderation_state_generation.wrapping_add(1);
        if let Some(svc) = &self.moderation_service {
            self.has_active_moderation = svc.has_active_rules();
        }
    }

    /// 뮤트 만료 뒤 활성 규칙 재확인.
    fn schedule_moderation_refresh(
        &mut self,
        after_secs: f64,
        service: &Arc<ChatModerationService>,
    ) {
        let generation = self.chat_session_generation;
        let svc = Arc::downgrade(service);
        let delay = Duration::from_secs_f64(after_secs.max(0.0) + 1.0);
        // 이 작업은 저장 슬롯이 없다(Swift 도 fire-and-forget).
        let _ = self.spawn_timer(delay, move |s, _| {
            let Some(svc) = svc.upgrade() else { return };
            if s.chat_session_generation == generation
                && s.moderation_service
                    .as_ref()
                    .is_some_and(|m| Arc::ptr_eq(m, &svc))
            {
                s.refresh_has_active_moderation();
            }
        });
    }

    fn notify_blocked(&self, service: &ChatModerationService, user_id: &str, blocked: bool) {
        let list: Vec<String> = service.blocked_users().into_iter().collect();
        if let Some(cb) = &self.on_blocked_users_changed {
            cb(list);
        }
        if let Some(cb) = &self.on_blocked_user_mutation {
            cb(user_id, blocked);
        }
    }

    /// 차단 — 버퍼에서 즉시 지우고 재필터. 되돌리기 토스트를 띄운다.
    pub fn block_user(&mut self, user_id: &str, nickname: Option<&str>) {
        self.message_filter_generation = self.message_filter_generation.wrapping_add(1);
        let Some(service) = self.moderation_service.clone() else {
            return;
        };
        service.block_user(user_id);
        self.has_active_moderation = true;
        self.apply_mutation(ChatMessageMutation::PurgeUser {
            user_id: user_id.into(),
        });
        self.notify_blocked(&service, user_id, true);
        self.refresh_has_active_moderation();
        self.refilter_messages();
        let who = nickname.unwrap_or(user_id);
        let weak = self.this.clone();
        let uid = user_id.to_string();
        self.deps.toast.show(
            format!("{who} 님을 차단했습니다 — 이후 메시지가 보이지 않습니다"),
            Some("차단 해제".into()),
            Some(Box::new(move || {
                if let Some(shared) = weak.upgrade() {
                    ChatViewModel::from_shared(shared).unblock_user(&uid, true);
                }
            })),
        );
    }

    pub fn unblock_user(&mut self, user_id: &str, notify_persistence: bool) {
        self.message_filter_generation = self.message_filter_generation.wrapping_add(1);
        let Some(service) = self.moderation_service.clone() else {
            return;
        };
        service.unblock_user(user_id);
        if notify_persistence {
            self.notify_blocked(&service, user_id, false);
        }
        self.refresh_has_active_moderation();
        self.refilter_messages();
    }

    pub fn blocked_users(&self) -> HashSet<String> {
        self.moderation_service
            .as_ref()
            .map(|s| s.blocked_users())
            .unwrap_or_default()
    }

    pub fn prefill_mention(&mut self, nickname: &str) {
        let token = format!("@{nickname} ");
        if self.input_text.is_empty() {
            self.input_text = token;
        } else if !self.input_text.starts_with(&token) {
            self.input_text = format!("{token}{}", self.input_text);
        }
    }

    /// 뮤트(기본 1시간).
    pub fn mute_user(&mut self, user_id: &str, duration_secs: f64) {
        self.message_filter_generation = self.message_filter_generation.wrapping_add(1);
        let Some(service) = self.moderation_service.clone() else {
            return;
        };
        service.mute_user(user_id, duration_secs);
        self.apply_mutation(ChatMessageMutation::PurgeUser {
            user_id: user_id.into(),
        });
        self.has_active_moderation = true;
        self.refresh_has_active_moderation();
        self.schedule_moderation_refresh(duration_secs, &service);
        self.refilter_messages();
    }

    /// 키워드 필터 교체 후 즉시 반영(Swift 는 동기화 작업을 기다렸다).
    pub fn add_keyword_filter(&mut self, keywords: Vec<String>) {
        self.message_filter_generation = self.message_filter_generation.wrapping_add(1);
        self.keyword_policy_generation = self.keyword_policy_generation.wrapping_add(1);
        self.blocked_words = keywords;
        self.apply_keyword_policy_now();
    }

    pub fn toggle_highlight(&mut self, user_id: &str) {
        if !self.highlighted_users.remove(user_id) {
            self.highlighted_users.insert(user_id.to_string());
        }
    }

    pub fn set_filter_enabled(&mut self, enabled: bool) {
        self.message_filter_generation = self.message_filter_generation.wrapping_add(1);
        self.keyword_policy_generation = self.keyword_policy_generation.wrapping_add(1);
        self.is_filter_enabled = enabled;
        self.schedule_keyword_policy_sync();
    }

    pub fn is_highlighted(&self, user_id: &str) -> bool {
        self.highlighted_users.contains(user_id)
    }

    pub fn pin_message(&mut self, message: ChatMessageItem) {
        self.pinned_message = Some(message);
    }

    pub fn unpin_message(&mut self) {
        self.pinned_message = None;
    }

    pub fn dismiss_streamer_notice(&mut self) {
        if let Some(n) = &self.streamer_notice {
            self.deps.pinned_notices.mark_dismissed(&n.id);
        }
        self.streamer_notice = None;
    }

    pub fn append_mention(&mut self, nickname: &str) {
        self.prefill_mention(nickname);
    }

    /// 신고는 웹으로 넘긴다는 안내만.
    pub fn report_user(&mut self, user_id: &str) {
        tracing::info!("reportUser(web-handoff): {user_id}");
        self.append_system(
            "ℹ️ 채팅 신고는 현재 클라이언트에서 미지원이며, chzzk 웹사이트의 채팅창에서 실행할 수 있습니다.",
            ChatSystemNoticeKind::Info,
        );
    }
}

impl ChatViewModel {
    /// 잠금 없이 부를 수 있는 차단 진입점(컨텍스트 메뉴·되돌리기용).
    pub fn block_user(&self, user_id: &str, nickname: Option<&str>) {
        self.lock().block_user(user_id, nickname);
    }

    pub fn unblock_user(&self, user_id: &str, notify_persistence: bool) {
        self.lock().unblock_user(user_id, notify_persistence);
    }
}
