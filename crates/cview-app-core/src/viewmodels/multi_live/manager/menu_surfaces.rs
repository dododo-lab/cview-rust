//! 메뉴 이탈/복귀·가림 — 비디오 표면과 GPU 렌더 단계 보관·복원.

use cview_player::policy::SessionTier;
use uuid::Uuid;

use super::{spawn_detached, ManagerState, MenuSessionLifecycleIntent};
use crate::viewmodels::multi_live::policy::tuning;
use crate::viewmodels::support::{cancel_slot, slot_is, TaskSlot};

impl ManagerState {
    pub fn suspend_all_gpu_render_tiers(&mut self, keep_hlsjs_alive: bool) {
        let selected = self.selected_session_id;
        for s in self.sessions.clone() {
            if Some(s.id()) == self.pip_session_id {
                continue;
            }
            if Some(s.id()) == selected && s.player().engine().is_some() {
                s.player().set_gpu_render_tier_hidden(keep_hlsjs_alive);
                if !keep_hlsjs_alive {
                    self.occlusion_suspended_session_ids.insert(s.id());
                }
            }
        }
    }

    pub fn restore_video_surfaces_for_menu_enter(&mut self) {
        cancel_slot(&mut self.menu_leave_full_suspend_task);
        self.has_deferred_peer_restore = false;
        let pip = self.pip_session_id;
        let needs_loader_resume = self.sessions.iter().any(|s| {
            Some(s.id()) != pip
                && s.player()
                    .engine()
                    .is_some_and(|h| h.is_loading_suspended())
        });
        if needs_loader_resume {
            for s in &self.sessions {
                if Some(s.id()) == pip || !self.menu_parked_session_ids.contains(&s.id()) {
                    continue;
                }
                if let Some(hls) = s.player().engine() {
                    hls.resume();
                }
            }
            self.menu_parked_session_ids.clear();
            self.restore_gpu_render_tiers_from_selection(true, None);
            return;
        }
        for s in &self.sessions {
            if Some(s.id()) == pip {
                continue;
            }
            let Some(hls) = s.player().engine() else {
                continue;
            };
            if self.menu_parked_session_ids.remove(&s.id()) {
                hls.resume();
            }
        }
        self.reconcile_hlsjs_pane_offscreen_surfaces();
    }

    pub fn defer_peer_surface_restore(&mut self) {
        cancel_slot(&mut self.menu_leave_full_suspend_task);
        self.has_deferred_peer_restore = true;
    }

    pub fn complete_deferred_peer_restore(&mut self) {
        if self.has_deferred_peer_restore {
            self.restore_video_surfaces_for_menu_enter();
        }
    }

    pub fn restore_video_surface_for_watch(&mut self, channel_id: &str) {
        cancel_slot(&mut self.menu_leave_full_suspend_task);
        let Some(session) = self.session_for_channel(channel_id).cloned() else {
            return;
        };
        let was_parked = self.menu_parked_session_ids.remove(&session.id());
        let hls = session.player().engine();
        if was_parked {
            if let Some(h) = &hls {
                h.resume();
            }
        }
        if was_parked
            || self.menu_paused_session_ids.contains(&session.id())
            || hls.is_some_and(|h| h.is_loading_suspended())
        {
            self.restore_gpu_render_tiers_from_selection(true, Some(session.id()));
        } else {
            self.reconcile_hlsjs_pane_offscreen_surfaces();
        }
    }

    pub fn activate_new_watch_audio(&mut self, channel_id: &str) {
        if !self.is_multi_audio_mode {
            return;
        }
        let Some(session) = self.session_for_channel(channel_id).cloned() else {
            return;
        };
        self.audio_enabled_session_ids.insert(session.id());
        session.set_muted(false);
    }

    pub fn park_video_surfaces_for_resident_menu_leave(&mut self) {
        self.has_deferred_peer_restore = false;
        for s in &self.sessions {
            if Some(s.id()) == self.pip_session_id {
                continue;
            }
            let Some(hls) = s.player().engine() else {
                continue;
            };
            hls.set_video_surface_visible(false);
            if !hls.is_loading_suspended() && hls.is_playback_desired() {
                hls.pause();
                self.menu_parked_session_ids.insert(s.id());
            }
        }
        cancel_slot(&mut self.menu_leave_full_suspend_task);
        let this = self.this.clone();
        self.menu_leave_full_suspend_task = Some(TaskSlot::spawn(move |tid| async move {
            tokio::time::sleep(tuning::RESIDENT_MENU_FULL_SUSPEND_DELAY).await;
            let Some(shared) = this.upgrade() else { return };
            let mut st = shared.obs.lock();
            if slot_is(&st.menu_leave_full_suspend_task, tid) {
                st.release_video_surfaces_for_menu_leave();
            }
        }));
    }

    pub fn restore_gpu_render_tiers_from_selection(
        &mut self,
        resume_menu_suspended_hls: bool,
        only_session_id: Option<Uuid>,
    ) {
        for s in self.sessions.clone() {
            if only_session_id.is_some_and(|o| o != s.id()) {
                continue;
            }
            let protected = self.is_protected_session(s.id());
            let tier = if protected {
                SessionTier::Active
            } else {
                SessionTier::Visible
            };
            let hls = s.player().engine();
            if protected && hls.is_some() {
                s.player().restore_surface_after_occlusion();
            }
            let decode_not_off = self.decode_active.get(&s.id()) != Some(&false);
            if self.occlusion_suspended_session_ids.remove(&s.id()) {
                if let Some(h) = &hls {
                    if h.is_loading_suspended() && decode_not_off {
                        h.set_gpu_render_tier(tier);
                    }
                    s.player().sync_network_activity();
                }
            }
            if resume_menu_suspended_hls {
                if let Some(h) = &hls {
                    if h.is_loading_suspended() && decode_not_off {
                        h.set_gpu_render_tier(tier);
                    }
                    s.player().sync_network_activity();
                }
                s.player().resume_watch_heartbeat_for_menu_return();
                s.resume_polling_for_menu_return();
                let resume_playback = self.menu_paused_session_ids.remove(&s.id());
                self.schedule_menu_session_lifecycle(
                    &s,
                    MenuSessionLifecycleIntent::Restore { resume_playback },
                );
            }
        }
        if resume_menu_suspended_hls && self.sessions.len() >= 2 {
            self.start_bandwidth_coordination();
        }
        self.reconcile_hlsjs_pane_offscreen_surfaces();
    }

    pub fn release_video_surfaces_for_menu_leave(&mut self) {
        cancel_slot(&mut self.menu_leave_full_suspend_task);
        self.has_deferred_peer_restore = false;
        for s in self.sessions.clone() {
            if Some(s.id()) == self.pip_session_id {
                continue;
            }
            if let Some(hls) = s.player().engine() {
                if self.decode_active.get(&s.id()) == Some(&false) {
                    hls.set_video_surface_visible(false);
                } else {
                    hls.set_gpu_render_tier(SessionTier::Hidden);
                }
                self.occlusion_suspended_session_ids.remove(&s.id());
                s.player().sync_network_activity();
            }
            s.player().suspend_watch_heartbeat_for_menu_leave();
            s.suspend_polling_for_menu_leave();
            self.schedule_menu_session_lifecycle(&s, MenuSessionLifecycleIntent::SuspendChat);
        }
        self.stop_bandwidth_coordination();
        let pool = self.core.deps.engine_pool.clone();
        spawn_detached(async move { pool.trim_idle(1).await });
    }
}
