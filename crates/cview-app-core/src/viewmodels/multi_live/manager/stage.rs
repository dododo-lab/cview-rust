//! 선택·스테이지(라이브 단독 보기) 전환과 웜 핸드오프·타임머신.

use std::collections::HashSet;
use std::sync::Arc;

use cview_player::coordinator::StreamPhase;
use cview_player::policy::SessionTier;
use tokio::time::Instant;
use uuid::Uuid;

use super::{spawn_detached, ManagerState};
use crate::viewmodels::multi_live::layout::MultiLiveGridLayoutMode;
use crate::viewmodels::multi_live::policy::{
    tuning, WATCH_KEEP_ALIVE_WARM_COUNT, ZAP_REQUEST_MAX_AGE_SECS,
};
use crate::viewmodels::multi_live::session::MultiLiveSession;
use crate::viewmodels::support::{cancel_slot, slot_is, TaskSlot};

impl ManagerState {
    pub fn select_session(&mut self, id: Uuid, promote_to_focus_main: bool) {
        if !self.contains(id) {
            return;
        }
        let started = Instant::now();
        self.is_updating_session_selection = true;
        let previous_id = self.selected_session_id;
        if promote_to_focus_main {
            self.set_focus_main_session_id(Some(id));
        }
        self.set_selected_session_id(Some(id));
        self.synchronize_bandwidth_selection();
        self.release_bandwidth_caps_immediately(id);
        if self.stage_session_id.is_some() {
            self.set_stage_session(Some(id));
        }
        self.apply_rate_budget();
        if previous_id != Some(id) {
            let prev = previous_id.and_then(|p| self.session(p).cloned());
            if let Some(prev) = &prev {
                prev.player().set_precision_sync_active(false);
            }
            if let Some(cur) = self.session(id) {
                cur.player().set_precision_sync_active(true);
            }
            if let Some(prev) = &prev {
                prev.set_background_mode(true);
            }
            if let Some(current) = self.session(id).cloned() {
                if !self.is_multi_audio_mode {
                    self.route_audio(id);
                }
                current.set_background_mode(false);
                let engine_playing = current.player().engine().is_some_and(|e| e.is_playing());
                if current.player().stream_phase() == StreamPhase::Buffering || !engine_playing {
                    current.player().promote_for_in_app_selection();
                }
                if let Some(prev) = &prev {
                    if let Some(hls) = prev.player().engine() {
                        hls.set_gpu_render_tier(SessionTier::Visible);
                        self.occlusion_suspended_session_ids.remove(&prev.id());
                        prev.player().sync_network_activity();
                    }
                }
                if let Some(hls) = current.player().engine() {
                    hls.set_gpu_render_tier(SessionTier::Active);
                    self.occlusion_suspended_session_ids.remove(&id);
                    current.player().sync_network_activity();
                }
            }
        }
        self.save_state();
        self.mru.touch(id);
        self.reconcile_decode_budget();
        self.is_updating_session_selection = false;
        self.reconcile_focus_quality(None);
        tracing::info!(
            "live_selection session={id} duration_ms={} sessions={}",
            started.elapsed().as_secs_f64() * 1_000.0,
            self.sessions.len()
        );
    }

    pub fn select_by_channel_id(&mut self, channel_id: &str) {
        let Some(id) = self.session_for_channel(channel_id).map(|s| s.id()) else {
            return;
        };
        let promote = self.grid_layout_mode == MultiLiveGridLayoutMode::Focus;
        self.select_session(id, promote);
    }

    pub fn set_stage_session(&mut self, id: Option<Uuid>) {
        if self.stage_session_id == id {
            return;
        }
        let previous = self.stage_session_id;
        if let Some(p) = previous {
            if Some(p) != id {
                self.previous_stage_session_id = Some(p);
            }
        }
        if id.is_none() {
            self.previous_stage_session_id = None;
        }
        self.stage_session_id = id;
        self.reconcile_focus_quality(None);
        self.reconcile_decode_budget();
        self.reconcile_time_machine();
        if let Some((w, h)) = self.last_reported_stage_size {
            self.update_pane_sizes(w as i64, h as i64);
        }
        if let Some(id) = id {
            self.release_bandwidth_caps_immediately(id);
        }
        cancel_slot(&mut self.stage_handoff_hide_task);
        self.reconcile_hlsjs_pane_offscreen_surfaces();
        // 리뷰 C-1 — 무대 왕복 뒤 멈춘 패인이 매니저 판정(디코드 정지)인지 가르는 기록.
        let decode_off = self
            .sessions
            .iter()
            .filter(|s| self.decode_active.get(&s.id()) == Some(&false))
            .count();
        tracing::info!(
            "live_stage stage={id:?} previous={previous:?} warm={:?} grid={} decode_off={decode_off} sessions={}",
            self.previous_stage_session_id,
            self.is_grid_layout,
            self.sessions.len()
        );
        if previous.is_some() && id.is_some() {
            self.stage_handoff_started_at = Some(Instant::now());
            if let Some(incoming) = id.and_then(|i| self.session(i).cloned()) {
                self.stage_handoff_baseline_session_id = Some(incoming.id());
                self.stage_handoff_baseline_tick = incoming.player().presented_frame_tick();
                if let Some(hls) = incoming.player().engine() {
                    hls.arm_surface_presented_probe();
                }
            }
            let this = self.this.clone();
            let handoff_to = id;
            self.stage_handoff_hide_task = Some(TaskSlot::spawn(move |tid| async move {
                tokio::time::sleep(tuning::STAGE_HANDOFF_TIMEOUT).await;
                let Some(shared) = this.upgrade() else { return };
                let mut st = shared.obs.lock();
                if !slot_is(&st.stage_handoff_hide_task, tid) || st.stage_session_id != handoff_to {
                    return;
                }
                st.clear_stage_handoff_warm(handoff_to, "timeout");
            }));
        }
        if id.is_some() {
            self.prewarm_next_watch_engine();
        }
    }

    /// 스테이지 핸드오프 완료 — 이전 스테이지 웜 패인을 거둔다.
    pub fn clear_stage_handoff_warm(&mut self, expected_current: Option<Uuid>, reason: &str) {
        if expected_current.is_some() && self.stage_session_id != expected_current {
            return;
        }
        if self.previous_stage_session_id.is_none() {
            return;
        }
        self.previous_stage_session_id = None;
        cancel_slot(&mut self.stage_handoff_hide_task);
        let now = Instant::now();
        if let Some(started) = self.stage_handoff_started_at.take() {
            tracing::info!(
                "live_stage_handoff reason={reason} duration_ms={} sessions={}",
                now.duration_since(started).as_secs_f64() * 1_000.0,
                self.sessions.len()
            );
        }
        if let Some(channel_id) = self
            .stage_session_id
            .and_then(|s| self.session(s))
            .map(|s| s.channel_id().to_string())
        {
            if let Some(request) = self.zap_requests.remove(&channel_id) {
                let elapsed = now.duration_since(request.at).as_secs_f64();
                if elapsed <= ZAP_REQUEST_MAX_AGE_SECS {
                    tracing::info!(
                        "live_zap phase={} reason={reason} total_ms={} sessions={}",
                        if request.warm { "warm" } else { "cold" },
                        elapsed * 1_000.0,
                        self.sessions.len()
                    );
                }
            }
        }
        self.stage_handoff_baseline_session_id = None;
        self.stage_handoff_baseline_tick = 0;
        self.reconcile_hlsjs_pane_offscreen_surfaces();
        self.reconcile_focus_quality(None);
        self.reconcile_decode_budget();
    }

    pub fn watch_warm_candidate_id(&self) -> Option<Uuid> {
        if self.is_grid_layout {
            return None;
        }
        let stage = self.stage_session_id?;
        if let Some(previous) = self.previous_stage_session_id {
            if previous != stage {
                return Some(previous);
            }
        }
        // Swift `max(by:)` 는 동률이면 마지막 원소를 남긴다.
        let mut best: Option<(Uuid, u64)> = None;
        for s in self
            .sessions
            .iter()
            .filter(|s| s.id() != stage && Some(s.id()) != self.pip_session_id)
        {
            let tick = self.mru.tick(s.id());
            if best.is_none_or(|(_, t)| tick >= t) {
                best = Some((s.id(), tick));
            }
        }
        best.map(|(id, _)| id)
    }

    pub fn watch_mode_allows_decode(&self, id: Uuid) -> bool {
        let keep = self.watch_keep_alive_session_ids();
        self.watch_mode_allows_decode_with(id, &keep)
    }

    pub(super) fn watch_mode_allows_decode_with(
        &self,
        id: Uuid,
        keep_alive: &HashSet<Uuid>,
    ) -> bool {
        if self.is_grid_layout || self.stage_session_id.is_none() {
            return true;
        }
        let some = Some(id);
        if some == self.stage_session_id || some == self.previous_stage_session_id {
            return true;
        }
        if some == self.selected_session_id || some == self.pip_session_id {
            return true;
        }
        if some == self.audio_session_id || keep_alive.contains(&id) {
            return true;
        }
        self.session(id).is_some_and(|s| self.is_audible_session(s))
    }

    pub fn watch_keep_alive_session_ids(&self) -> HashSet<Uuid> {
        let mut keep = HashSet::new();
        if self.is_grid_layout {
            return keep;
        }
        let Some(stage) = self.stage_session_id else {
            return keep;
        };
        if let Some(previous) = self.previous_stage_session_id {
            if previous != stage {
                keep.insert(previous);
            }
        }
        let mut ranked: Vec<&Arc<MultiLiveSession>> = self
            .sessions
            .iter()
            .filter(|s| {
                s.id() != stage && Some(s.id()) != self.pip_session_id && !keep.contains(&s.id())
            })
            .collect();
        ranked.sort_by_key(|s| std::cmp::Reverse(self.mru.tick(s.id())));
        for s in ranked {
            if keep.len() >= WATCH_KEEP_ALIVE_WARM_COUNT {
                break;
            }
            keep.insert(s.id());
        }
        keep
    }

    pub fn reconcile_hlsjs_pane_offscreen_surfaces(&self) {
        let stage = self.stage_session_id;
        let handoff_warm = self.previous_stage_session_id;
        for s in &self.sessions {
            let Some(hls) = s.player().engine() else {
                continue;
            };
            let id = Some(s.id());
            let keep_visible = id == stage || id == handoff_warm || id == self.pip_session_id;
            hls.set_video_surface_visible(!(stage.is_some() && !keep_visible));
        }
    }

    pub fn end_stage_session(&mut self, id: Uuid) {
        if self.stage_session_id == Some(id) {
            self.set_stage_session(None);
        }
    }

    fn reconcile_time_machine(&mut self) {
        let stage = self.stage_session_id;
        let pane_count = self.sessions.len();
        let sessions = self.sessions.clone();
        for s in sessions {
            if Some(s.id()) == stage {
                if let Some(t) = self.tm_revert_tasks.remove(&s.id()) {
                    t.cancel();
                }
                spawn_detached(async move { s.set_time_machine_desired(true, pane_count).await });
            } else {
                self.schedule_time_machine_revert(&s);
            }
        }
    }

    fn schedule_time_machine_revert(&mut self, session: &Arc<MultiLiveSession>) {
        if !session.time_machine_desired() {
            return;
        }
        let sid = session.id();
        if self.tm_revert_tasks.contains_key(&sid) {
            return;
        }
        let this = self.this.clone();
        let weak_session = Arc::downgrade(session);
        let task = TaskSlot::spawn(move |tid| async move {
            tokio::time::sleep(tuning::TIME_MACHINE_REVERT_DELAY).await;
            let (Some(shared), Some(session)) = (this.upgrade(), weak_session.upgrade()) else {
                return;
            };
            let pane_count = {
                let mut st = shared.obs.lock();
                if !st.tm_revert_tasks.get(&sid).is_some_and(|t| t.id() == tid) {
                    return;
                }
                st.tm_revert_tasks.remove(&sid);
                if st.stage_session_id == Some(sid) {
                    return;
                }
                st.sessions.len()
            };
            session.set_time_machine_desired(false, pane_count).await;
        });
        self.tm_revert_tasks.insert(sid, task);
    }

    pub fn has_active_streams(&self) -> bool {
        self.sessions
            .iter()
            .any(|s| s.player().stream_phase().requires_playback_resources())
    }
}
