//! 시청 준비·표시 — 라이브 단독 보기로 들어가는 채널의 준비 상태와 표시.

use super::{
    FollowingHubMode, FollowingState, FollowingWorkspaceHost, WorkspaceNavigationKind,
    WorkspaceRequestToken,
};
use crate::viewmodels::multi_live::MultiLiveAddSessionResult;

impl FollowingState {
    pub fn begin_watch_preparation(
        &mut self,
        channel_id: &str,
        channel_name: Option<&str>,
        host: Option<&dyn FollowingWorkspaceHost>,
        presents: bool,
    ) -> WorkspaceRequestToken {
        if let Some(previous) = self.preparing_watch_channel_id.clone() {
            if previous != channel_id {
                if let Some(h) = host {
                    h.forget_zap_request(&previous);
                }
            }
        }
        let request = self.begin_workspace_request();
        self.preparing_watch_request = Some(request);
        self.preparing_watch_channel_id = Some(channel_id.to_string());
        if presents {
            self.presenting_watch_channel_id = Some(channel_id.to_string());
            self.presenting_watch_channel_name = channel_name.map(str::to_string);
            self.presenting_watch_request = Some(request);
        } else if self.presenting_watch_channel_id.is_some() {
            self.clear_watch_presentation();
            if let Some(h) = host {
                self.flush_deferred_surface_restore(h);
            }
        }
        request
    }

    pub fn finish_watch_preparation(&mut self, request: WorkspaceRequestToken) {
        if self.preparing_watch_request == Some(request) {
            self.preparing_watch_request = None;
            self.preparing_watch_channel_id = None;
        }
    }

    pub(super) fn clear_watch_presentation(&mut self) {
        self.presenting_watch_channel_id = None;
        self.presenting_watch_channel_name = None;
        self.presenting_watch_request = None;
    }

    pub(super) fn abandon_watch_presentation(
        &mut self,
        request: &WorkspaceRequestToken,
        host: &dyn FollowingWorkspaceHost,
    ) {
        if self.presenting_watch_request.as_ref() != Some(request) {
            return;
        }
        self.clear_watch_presentation();
        self.flush_deferred_surface_restore(host);
    }

    pub fn supersede_watch_preparation(&mut self, host: Option<&dyn FollowingWorkspaceHost>) {
        if let (Some(id), Some(h)) = (&self.preparing_watch_channel_id, host) {
            h.forget_zap_request(id);
        }
        if let Some(presenting) = &self.presenting_watch_channel_id {
            if Some(presenting) != self.preparing_watch_channel_id.as_ref() {
                if let Some(h) = host {
                    h.forget_zap_request(presenting);
                }
            }
        }
        let _ = self.begin_workspace_request();
        if let Some(r) = self.preparing_watch_request {
            self.finish_watch_preparation(r);
        }
        self.clear_watch_presentation();
    }

    pub fn finish_watch_presentation_if_ready(&mut self, host: &dyn FollowingWorkspaceHost) {
        let Some(channel_id) = self.presenting_watch_channel_id.clone() else {
            return;
        };
        let Some(session) = host.session_id_for_channel(&channel_id) else {
            return;
        };
        if host.selected_session_id() != Some(session) || host.presented_frame_tick(session) == 0 {
            return;
        }
        self.clear_watch_presentation();
        host.complete_watch_zap(&channel_id, "ready");
        self.flush_deferred_surface_restore(host);
    }

    pub fn time_out_watch_presentation(&mut self, host: &dyn FollowingWorkspaceHost) {
        let Some(channel_id) = self.presenting_watch_channel_id.clone() else {
            return;
        };
        self.clear_watch_presentation();
        host.complete_watch_zap(&channel_id, "timeout");
        self.flush_deferred_surface_restore(host);
    }

    pub fn flush_deferred_surface_restore(&self, host: &dyn FollowingWorkspaceHost) {
        if self.hub_mode.occupies_full_stage() {
            return;
        }
        host.complete_deferred_peer_restore();
    }

    pub fn apply_menu_enter_surface_policy(
        &self,
        has_pending_watch_request: bool,
        host: &dyn FollowingWorkspaceHost,
    ) {
        if self.hub_mode.occupies_full_stage() || self.is_awaiting_multi_entry_motion {
            host.park_video_surfaces_for_resident_menu_leave();
        } else if has_pending_watch_request || self.presenting_watch_channel_id.is_some() {
            host.defer_peer_surface_restore();
        } else {
            host.restore_video_surfaces_for_menu_enter();
        }
    }

    pub(super) fn is_navigation_in_flight(
        &self,
        kind: WorkspaceNavigationKind,
        channel_id: &str,
    ) -> bool {
        match kind {
            WorkspaceNavigationKind::Watch => {
                self.is_preparing_watch()
                    && self.preparing_watch_channel_id.as_deref() == Some(channel_id)
            }
            WorkspaceNavigationKind::Multi => {
                self.presenting_multi_channel_id.as_deref() == Some(channel_id)
                    && self
                        .presenting_multi_request
                        .is_some_and(|r| self.is_current_workspace_request(&r))
            }
        }
    }

    pub(super) fn commit_added_watch(
        &mut self,
        channel_id: &str,
        result: &MultiLiveAddSessionResult,
        request: WorkspaceRequestToken,
        host: &dyn FollowingWorkspaceHost,
    ) {
        if !matches!(result, MultiLiveAddSessionResult::Added { .. }) {
            self.commit_existing_watch(channel_id, request, host);
            return;
        }
        self.commit_prepared_watch(channel_id, request, host);
        host.activate_new_watch_audio(channel_id);
    }

    pub(super) fn commit_existing_watch(
        &mut self,
        channel_id: &str,
        request: WorkspaceRequestToken,
        host: &dyn FollowingWorkspaceHost,
    ) {
        if !self.commit_prepared_watch(channel_id, request, host) {
            return;
        }
        host.restart_session_if_stalled(channel_id);
    }

    pub fn finish_multi_presentation_if_ready(&mut self, host: &dyn FollowingWorkspaceHost) {
        let Some(channel_id) = self.presenting_multi_channel_id.clone() else {
            return;
        };
        let Some(session) = host.session_id_for_channel(&channel_id) else {
            return;
        };
        if host.presented_frame_tick(session) == 0 {
            return;
        }
        self.presenting_multi_channel_id = None;
        self.presenting_multi_channel_name = None;
        self.presenting_multi_request = None;
    }

    pub fn time_out_multi_presentation(&mut self) {
        self.presenting_multi_channel_id = None;
        self.presenting_multi_channel_name = None;
        self.presenting_multi_request = None;
        self.is_awaiting_multi_entry_motion = false;
    }

    pub fn commit_prepared_watch(
        &mut self,
        channel_id: &str,
        request: WorkspaceRequestToken,
        host: &dyn FollowingWorkspaceHost,
    ) -> bool {
        if !self.is_current_workspace_request(&request)
            || host.session_id_for_channel(channel_id).is_none()
        {
            return false;
        }
        self.settle_multi_navigation_origin(&request);
        host.set_grid_layout(false);
        host.set_stage_session(None);
        self.activate_added_live_channel(channel_id, host, false);
        if let Some(session) = host.session_id_for_channel(channel_id) {
            host.set_stage_session(Some(session));
        }
        host.restore_video_surface_for_watch(channel_id);
        self.apply_hub_mode_preset(FollowingHubMode::Watch, host, false, false);
        self.finish_watch_preparation(request);
        self.finish_watch_presentation_if_ready(host);
        true
    }

    pub fn sync_multi_grid_layout(&self, host: &dyn FollowingWorkspaceHost) {
        if self.hub_mode != FollowingHubMode::Multi
            || host.session_ids().len() < 2
            || host.is_grid_layout()
        {
            return;
        }
        host.set_grid_layout(true);
    }
}
