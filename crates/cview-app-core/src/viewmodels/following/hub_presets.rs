//! 허브 모드 프리셋.

use super::{
    ChatDockFocus, FollowingHubMode, FollowingSortOrder, FollowingState, FollowingWorkspaceHost,
};

impl FollowingState {
    pub fn apply_hub_mode_preset(
        &mut self,
        mode: FollowingHubMode,
        host: &dyn FollowingWorkspaceHost,
        deferring_watch_activation: bool,
        deferring_surface_restore: bool,
    ) {
        if mode != FollowingHubMode::Watch && self.presenting_watch_channel_id.is_some() {
            self.supersede_watch_preparation(Some(host));
        }
        if mode != FollowingHubMode::Multi && self.presenting_multi_channel_id.is_some() {
            self.time_out_multi_presentation();
        }
        if self.hub_mode == mode {
            self.sync_multi_grid_layout(host);
            return;
        }
        let was_full_stage = self.hub_mode.occupies_full_stage();
        self.chat_dock_revealed_in_full_stage = false;
        self.set_hub_mode(mode);
        match mode {
            FollowingHubMode::Explore => {
                if self.chat_dock_focus != ChatDockFocus::Balanced {
                    self.set_chat_dock_focus(ChatDockFocus::Balanced);
                }
                self.filter_live_only = false;
                self.sort_order = FollowingSortOrder::LiveFirst;
                if host.is_grid_layout() {
                    host.set_grid_layout(false);
                }
                self.show_multi_live = false;
            }
            FollowingHubMode::Watch => {
                if self.chat_dock_focus != ChatDockFocus::Single {
                    self.set_chat_dock_focus(ChatDockFocus::Single);
                }
                self.filter_live_only = true;
                self.sort_order = FollowingSortOrder::LiveFirst;
                if !deferring_watch_activation && host.is_grid_layout() {
                    host.set_grid_layout(false);
                }
                self.show_multi_live = true;
            }
            FollowingHubMode::Multi => {
                if self.chat_dock_focus != ChatDockFocus::Multi {
                    self.set_chat_dock_focus(ChatDockFocus::Multi);
                }
                self.filter_live_only = true;
                self.sort_order = FollowingSortOrder::LiveFirst;
                if !host.is_grid_layout() {
                    host.set_grid_layout(true);
                }
                self.show_multi_live = true;
            }
            FollowingHubMode::Search | FollowingHubMode::Popular | FollowingHubMode::Category => {
                if self.chat_dock_focus != ChatDockFocus::Balanced {
                    self.set_chat_dock_focus(ChatDockFocus::Balanced);
                }
            }
        }
        let defers_restore = deferring_watch_activation || deferring_surface_restore;
        if !was_full_stage && mode.occupies_full_stage() {
            host.park_video_surfaces_for_resident_menu_leave();
        } else if was_full_stage && !mode.occupies_full_stage() {
            if defers_restore {
                host.defer_peer_surface_restore();
            } else {
                host.restore_video_surfaces_for_menu_enter();
            }
        } else if !mode.occupies_full_stage() && !defers_restore {
            host.complete_deferred_peer_restore();
        }
    }
}
