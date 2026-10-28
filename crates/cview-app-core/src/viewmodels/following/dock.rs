//! 스마트 큐와 채팅 도크.

use std::collections::HashSet;

use cview_core::models::LiveChannelItem;
use uuid::Uuid;

use super::{
    FollowingHubMode, FollowingState, FollowingWorkspaceHost, CHAT_CAPACITY_AFTER_ADD_MESSAGE,
    GRID_HEADER_CHAT_CAPACITY_MESSAGE,
};

impl FollowingState {
    /// 개요의 라이브 채널 중 활성·대기열에 없는 것을 붙인다.
    pub fn enqueue_overview_channels(
        &mut self,
        channels: &[LiveChannelItem],
        active_ids: &HashSet<String>,
    ) -> usize {
        let mut seen: HashSet<String> = active_ids.clone();
        seen.extend(self.smart_queue_channel_ids.iter().cloned());
        let additions: Vec<String> = channels
            .iter()
            .filter(|c| c.is_live && seen.insert(c.channel_id.clone()))
            .map(|c| c.channel_id.clone())
            .collect();
        let n = additions.len();
        self.smart_queue_channel_ids.extend(additions);
        n
    }

    pub fn clear_smart_queue_with_undo(&mut self) {
        let previous = std::mem::take(&mut self.smart_queue_channel_ids);
        if previous.is_empty() {
            return;
        }
        let Some(toast) = self.toast.clone() else {
            return;
        };
        let weak = self.this.clone();
        let count = previous.len();
        toast.show(
            format!("Smart Queue 를 비웠습니다 ({count}개)"),
            Some("실행 취소".into()),
            Some(Box::new(move || {
                if let Some(shared) = weak.upgrade() {
                    shared.obs.lock().smart_queue_channel_ids = previous;
                }
            })),
        );
    }

    // MARK: 도크

    pub fn present_channel_dock(&mut self) {
        self.is_channel_dock_hidden = false;
    }
    pub fn hide_channel_dock(&mut self) {
        self.is_channel_dock_hidden = true;
    }
    pub fn toggle_channel_dock(&mut self) {
        if self.hub_mode != FollowingHubMode::Watch && self.hub_mode != FollowingHubMode::Multi {
            return;
        }
        self.is_channel_dock_hidden = !self.is_channel_dock_hidden;
    }

    pub fn present_chat_dock(&mut self) {
        self.show_multi_chat = true;
        self.set_chat_dock_collapsed(false);
        self.chat_dock_revealed_in_full_stage = true;
    }

    pub fn toggle_chat_dock(&mut self) {
        if self.show_multi_chat {
            self.show_multi_chat = false;
        } else {
            self.present_chat_dock();
        }
    }

    pub fn activate_added_live_channel(
        &mut self,
        channel_id: &str,
        host: &dyn FollowingWorkspaceHost,
        preserves_main_focus: bool,
    ) {
        let Some(session) = host.session_id_for_channel(channel_id) else {
            return;
        };
        if preserves_main_focus && self.adopt_chat_keeping_main_focus(session, channel_id, host) {
            return;
        }
        let promote = host.is_grid_layout() && host.is_focus_layout();
        if host.selected_session_id() != Some(session)
            || (promote && host.focus_main_session_id() != Some(session))
        {
            host.select_session(session, promote);
        }
        if !self.auto_sync_chat_on_multi_live_add {
            return;
        }
        let channels: HashSet<String> = host.session_channel_ids().into_iter().collect();
        self.chat_sessions.remove_borrowed_sessions(&channels);
        if !self.chat_sessions.adopt_live_session(session, channel_id) {
            self.ml_add_error = Some(CHAT_CAPACITY_AFTER_ADD_MESSAGE.into());
            return;
        }
        self.chat_sessions.select_channel(channel_id);
        self.present_chat_dock();
    }

    pub fn activate_grid_header_channel(
        &mut self,
        channel_id: &str,
        host: &dyn FollowingWorkspaceHost,
        reveals_chat_dock: bool,
    ) {
        let Some(session) = host.session_id_for_channel(channel_id) else {
            return;
        };
        if host.selected_session_id() != Some(session) {
            host.select_session(session, false);
        }
        if !host.is_audible(session) {
            host.toggle_session_audio(session);
        }
        if !self.chat_sessions.adopt_live_session(session, channel_id) {
            self.ml_add_error = Some(GRID_HEADER_CHAT_CAPACITY_MESSAGE.into());
            return;
        }
        if self.ml_add_error.as_deref() == Some(GRID_HEADER_CHAT_CAPACITY_MESSAGE) {
            self.ml_add_error = None;
        }
        self.chat_sessions.select_channel(channel_id);
        if reveals_chat_dock {
            self.present_chat_dock();
        }
    }

    pub fn adopt_chat_keeping_main_focus(
        &mut self,
        session: Uuid,
        channel_id: &str,
        host: &dyn FollowingWorkspaceHost,
    ) -> bool {
        let Some(current) = host.selected_session_id() else {
            return false;
        };
        if current == session || !host.session_ids().contains(&current) {
            return false;
        }
        if !self.auto_sync_chat_on_multi_live_add {
            return true;
        }
        let channels: HashSet<String> = host.session_channel_ids().into_iter().collect();
        self.chat_sessions.remove_borrowed_sessions(&channels);
        if self.chat_sessions.adopt_live_session(session, channel_id) {
            return true;
        }
        self.ml_add_error = Some(CHAT_CAPACITY_AFTER_ADD_MESSAGE.into());
        true
    }
}
