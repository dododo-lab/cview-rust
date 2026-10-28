//! 세션 시작·재시도·메뉴 수명주기 작업 예약(세대별 단일 작업).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use uuid::Uuid;

use super::{ManagerShared, ManagerState, MenuSessionLifecycleIntent};
use crate::viewmodels::multi_live::deps::ChatConnectionKind;
use crate::viewmodels::multi_live::policy::{self, tuning};
use crate::viewmodels::multi_live::session::MultiLiveSession;
use crate::viewmodels::support::{cancel_slot, slot_is, TaskSlot};

/// 시작 예약이 할 일.
#[derive(Debug, Clone, Copy)]
enum StartupJob {
    Start {
        pane_count: usize,
        delay: Option<Duration>,
    },
    Retry,
}

/// 세션의 앞선 예약을 끊고 세대를 하나 올려 돌려준다.
fn next_generation(
    tasks: &mut HashMap<Uuid, TaskSlot>,
    generations: &mut HashMap<Uuid, u64>,
    sid: Uuid,
) -> u64 {
    if let Some(t) = tasks.remove(&sid) {
        t.cancel();
    }
    let generation = generations.get(&sid).copied().unwrap_or(0).wrapping_add(1);
    generations.insert(sid, generation);
    generation
}

impl ManagerState {
    pub(super) fn clear_per_session_tracking(&mut self) {
        if self.pip_session_id.is_some() {
            self.core.deps.pip.stop_pip();
        }
        self.set_pip_session_id(None);
        self.mru.remove_all();
        self.decode_active.clear();
        self.menu_paused_session_ids.clear();
        self.menu_parked_session_ids.clear();
        self.occlusion_suspended_session_ids.clear();
        cancel_slot(&mut self.menu_leave_full_suspend_task);
        self.last_applied_cap_height.clear();
        self.last_applied_hls_kbps.clear();
        for (_, t) in self.session_startup_tasks.drain() {
            t.cancel();
        }
        self.session_startup_generations.clear();
        for (_, t) in self.session_menu_lifecycle_tasks.drain() {
            t.cancel();
        }
        self.session_menu_lifecycle_generations.clear();
        for (_, t) in self.tm_revert_tasks.drain() {
            t.cancel();
        }
        cancel_slot(&mut self.stage_handoff_hide_task);
        cancel_slot(&mut self.watch_prewarm_task);
        self.zap_requests.clear();
        self.stage_handoff_started_at = None;
        self.stage_handoff_baseline_session_id = None;
        self.stage_handoff_baseline_tick = 0;
        self.stage_session_id = None;
        self.previous_stage_session_id = None;
    }

    pub(super) fn prewarm_next_watch_engine(&mut self) {
        if self.is_grid_layout || self.is_stopping_all || self.is_terminating {
            return;
        }
        if self.sessions.is_empty() || self.watch_prewarm_task.is_some() {
            return;
        }
        let this = self.this.clone();
        self.watch_prewarm_task = Some(TaskSlot::spawn(move |id| async move {
            tokio::time::sleep(tuning::WATCH_PREWARM_DELAY).await;
            let Some(shared) = this.upgrade() else { return };
            let pool = {
                let mut st = shared.obs.lock();
                if !slot_is(&st.watch_prewarm_task, id) {
                    return;
                }
                if st.is_grid_layout || st.is_stopping_all {
                    st.watch_prewarm_task = None;
                    return;
                }
                st.core.deps.engine_pool.clone()
            };
            pool.warmup(1).await;
            let mut st = shared.obs.lock();
            if slot_is(&st.watch_prewarm_task, id) {
                st.watch_prewarm_task = None;
            }
        }));
    }

    /// 세션 시작 예약 (세대별 단일 작업).
    pub fn schedule_session_start(
        &mut self,
        session: &Arc<MultiLiveSession>,
        pane_count: usize,
        delay: Option<Duration>,
    ) {
        self.schedule_startup(session, StartupJob::Start { pane_count, delay });
    }

    fn schedule_session_retry(&mut self, session: &Arc<MultiLiveSession>) {
        self.schedule_startup(session, StartupJob::Retry);
    }

    /// 시작·재시도 공용 — 세션마다 작업 하나만 남기고, 끝난 작업은 제 세대일 때만 기록을 지운다.
    fn schedule_startup(&mut self, session: &Arc<MultiLiveSession>, job: StartupJob) {
        let sid = session.id();
        let generation = next_generation(
            &mut self.session_startup_tasks,
            &mut self.session_startup_generations,
            sid,
        );
        let this = self.this.clone();
        let weak_session = Arc::downgrade(session);
        let task = TaskSlot::spawn(move |_| async move {
            if let StartupJob::Start { delay: Some(d), .. } = job {
                tokio::time::sleep(d).await;
            }
            let (Some(shared), Some(session)) = (this.upgrade(), weak_session.upgrade()) else {
                return;
            };
            if !shared.obs.read().contains(sid) {
                return;
            }
            match job {
                StartupJob::Start { pane_count, .. } => session.start(pane_count).await,
                StartupJob::Retry => session.retry().await,
            }
            let mut st = shared.obs.lock();
            if st.session_startup_generations.get(&sid) != Some(&generation) {
                return;
            }
            st.session_startup_tasks.remove(&sid);
            st.session_startup_generations.remove(&sid);
        });
        self.session_startup_tasks.insert(sid, task);
    }

    pub(super) fn schedule_menu_session_lifecycle(
        &mut self,
        session: &Arc<MultiLiveSession>,
        intent: MenuSessionLifecycleIntent,
    ) {
        let sid = session.id();
        let generation = next_generation(
            &mut self.session_menu_lifecycle_tasks,
            &mut self.session_menu_lifecycle_generations,
            sid,
        );
        let teardown_generation = self.teardown_generation;
        let this = self.this.clone();
        let weak_session = Arc::downgrade(session);
        let task = TaskSlot::spawn(move |_| async move {
            let current = |shared: &ManagerShared, session: &Arc<MultiLiveSession>| {
                let st = shared.obs.read();
                st.teardown_generation == teardown_generation
                    && st.session_menu_lifecycle_generations.get(&sid) == Some(&generation)
                    && st.sessions.iter().any(|s| Arc::ptr_eq(s, session))
            };
            let finish = |shared: &ManagerShared| {
                let mut st = shared.obs.lock();
                if st.session_menu_lifecycle_generations.get(&sid) == Some(&generation) {
                    st.session_menu_lifecycle_tasks.remove(&sid);
                    st.session_menu_lifecycle_generations.remove(&sid);
                }
            };
            let (Some(shared), Some(session)) = (this.upgrade(), weak_session.upgrade()) else {
                return;
            };
            if !current(&shared, &session) {
                return;
            }
            match intent {
                MenuSessionLifecycleIntent::SuspendChat => {
                    if !session.chat().has_foreground_presentation_lease() {
                        session.chat().disconnect().await;
                    }
                }
                MenuSessionLifecycleIntent::Restore { resume_playback } => {
                    if resume_playback {
                        session.player().toggle_play_pause().await;
                        if !current(&shared, &session) {
                            finish(&shared);
                            return;
                        }
                        session.player().recover_from_background();
                    }
                    if session.chat().connection_state() == ChatConnectionKind::Disconnected {
                        session.chat().reconnect().await;
                        if !current(&shared, &session) {
                            finish(&shared);
                            return;
                        }
                        session.restart_chat_header_polling();
                    }
                    if current(&shared, &session) {
                        let selected = shared.obs.read().selected_session_id;
                        session.set_background_mode(Some(sid) != selected);
                    }
                }
            }
            finish(&shared);
        });
        self.session_menu_lifecycle_tasks.insert(sid, task);
    }

    /// `restartSessionIfStalled(channelId:)`.
    pub fn restart_session_if_stalled(&mut self, channel_id: &str) -> bool {
        if self.backend.is_none() {
            return false;
        }
        let Some(session) = self.session_for_channel(channel_id).cloned() else {
            return false;
        };
        if !policy::needs_restart(&session.load_state(), &session.player().stream_phase()) {
            return false;
        }
        self.schedule_session_retry(&session);
        true
    }

    pub(super) fn focus_existing_session(
        &mut self,
        channel_id: &str,
        start_immediately: bool,
    ) -> bool {
        let Some(existing) = self.session_for_channel(channel_id).map(|s| s.id()) else {
            return false;
        };
        self.select_session(existing, false);
        self.restore_gpu_render_tiers_from_selection(true, None);
        if start_immediately {
            self.restart_session_if_stalled(channel_id);
        }
        true
    }
}
