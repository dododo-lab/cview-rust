//! [`FollowingWorkspaceHost`] 구현 — 팔로잉 작업 공간이 매니저를 다루는 창구.

use uuid::Uuid;

use super::MultiLiveManager;
use crate::viewmodels::following::FollowingWorkspaceHost;
use crate::viewmodels::multi_live::layout::MultiLiveGridLayoutMode;

impl FollowingWorkspaceHost for MultiLiveManager {
    fn session_ids(&self) -> Vec<Uuid> {
        self.read().sessions.iter().map(|s| s.id()).collect()
    }

    fn session_channel_ids(&self) -> Vec<String> {
        self.read()
            .sessions
            .iter()
            .map(|s| s.channel_id().to_string())
            .collect()
    }

    fn session_id_for_channel(&self, channel_id: &str) -> Option<Uuid> {
        self.read().session_for_channel(channel_id).map(|s| s.id())
    }

    fn selected_session_id(&self) -> Option<Uuid> {
        self.read().selected_session_id
    }

    fn presented_frame_tick(&self, session_id: Uuid) -> u64 {
        self.read()
            .session(session_id)
            .map_or(0, |s| s.player().presented_frame_tick())
    }

    fn is_adding_channel(&self, channel_id: &str) -> bool {
        self.read().is_adding_channel(channel_id)
    }

    fn is_grid_layout(&self) -> bool {
        self.read().is_grid_layout
    }

    fn set_grid_layout(&self, grid: bool) {
        self.lock().set_grid_layout(grid);
    }

    fn is_focus_layout(&self) -> bool {
        self.read().grid_layout_mode == MultiLiveGridLayoutMode::Focus
    }

    fn focus_main_session_id(&self) -> Option<Uuid> {
        self.read().focus_main_session_id
    }

    fn select_session(&self, id: Uuid, promote_to_focus_main: bool) {
        self.lock().select_session(id, promote_to_focus_main);
    }

    fn set_stage_session(&self, id: Option<Uuid>) {
        self.lock().set_stage_session(id);
    }

    fn is_audible(&self, session_id: Uuid) -> bool {
        let st = self.read();
        st.session(session_id).is_some_and(|s| st.is_audible(s))
    }

    fn toggle_session_audio(&self, session_id: Uuid) {
        self.lock().toggle_session_audio(session_id);
    }

    fn note_zap_requested(&self, channel_id: &str) {
        self.lock().note_zap_requested(channel_id);
    }

    fn forget_zap_request(&self, channel_id: &str) {
        self.lock().forget_zap_request(channel_id);
    }

    fn complete_watch_zap(&self, channel_id: &str, reason: &str) {
        self.lock().complete_watch_zap(channel_id, reason);
    }

    fn activate_new_watch_audio(&self, channel_id: &str) {
        self.lock().activate_new_watch_audio(channel_id);
    }

    fn restart_session_if_stalled(&self, channel_id: &str) {
        self.lock().restart_session_if_stalled(channel_id);
    }

    fn restore_video_surface_for_watch(&self, channel_id: &str) {
        self.lock().restore_video_surface_for_watch(channel_id);
    }

    fn park_video_surfaces_for_resident_menu_leave(&self) {
        self.lock().park_video_surfaces_for_resident_menu_leave();
    }

    fn defer_peer_surface_restore(&self) {
        self.lock().defer_peer_surface_restore();
    }

    fn restore_video_surfaces_for_menu_enter(&self) {
        self.lock().restore_video_surfaces_for_menu_enter();
    }

    fn complete_deferred_peer_restore(&self) {
        self.lock().complete_deferred_peer_restore();
    }
}
