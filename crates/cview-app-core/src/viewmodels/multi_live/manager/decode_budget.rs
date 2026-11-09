//! 디코드 예산 — 보호 세션 우선 디코드, 열·가림·메모리 압력에 따른 정지와 복원.

use std::collections::HashSet;
use std::sync::Arc;

use uuid::Uuid;

use super::ManagerState;
use crate::viewmodels::multi_live::layout::MultiLiveGridLayoutMode;
use crate::viewmodels::multi_live::policy::{
    self, tuning, DecodeSuspensionReason, PaneSuspensionCause,
};
use crate::viewmodels::multi_live::session::MultiLiveSession;
use crate::viewmodels::support::{cancel_slot, slot_is, TaskSlot};

impl ManagerState {
    fn current_suspension_cause(&self) -> PaneSuspensionCause {
        if self
            .decode_suspension_reasons
            .contains(&DecodeSuspensionReason::ThermalPressure)
        {
            return PaneSuspensionCause::Thermal;
        }
        if self
            .decode_suspension_reasons
            .contains(&DecodeSuspensionReason::WindowOcclusion)
        {
            return PaneSuspensionCause::Occlusion;
        }
        if self.memory_pressure_k_override.is_some() {
            return PaneSuspensionCause::MemoryPressure;
        }
        PaneSuspensionCause::DecodeBudget
    }

    fn apply_decode_state(&mut self, session: &Arc<MultiLiveSession>, decoding: bool) {
        let sid = session.id();
        if let Some(hls) = session.player().engine() {
            if decoding {
                let sel = Some(sid) == self.selected_session_id;
                hls.set_buffer_caps(
                    if sel { 10.0 } else { 9.0 },
                    if sel { 18.0 } else { 12.0 },
                    (if sel { 25 } else { 8 }) * 1024 * 1024,
                );
            }
            if self.decode_active.get(&sid) != Some(&decoding) {
                self.decode_active.insert(sid, decoding);
                if decoding {
                    hls.resume_loading();
                } else {
                    hls.suspend_loading();
                }
                session.player().sync_network_activity();
            } else if !decoding && !hls.is_suspend_intent_active() {
                hls.suspend_loading();
                session.player().sync_network_activity();
            }
        }
        let cause = if decoding {
            None
        } else {
            Some(self.current_suspension_cause())
        };
        if self.pane_suspension.get(&sid).copied() != cause {
            match cause {
                Some(c) => self.pane_suspension.insert(sid, c),
                None => self.pane_suspension.remove(&sid),
            };
        }
    }

    pub fn reconcile_decode_budget(&mut self) {
        let sessions = self.sessions.clone();
        if !self.decode_suspension_reasons.is_empty() {
            for s in &sessions {
                let protected = self.is_protected_during_suspension(s);
                self.apply_decode_state(s, protected);
            }
            return;
        }
        let reduce_enabled = self
            .settings
            .as_ref()
            .is_none_or(|s| s.multi_live().background_quality_reduction);
        if !reduce_enabled && self.memory_pressure_k_override.is_none() {
            for s in &sessions {
                self.apply_decode_state(s, true);
            }
            return;
        }
        let mut k = policy::effective_decode_budget(
            sessions.len(),
            self.effective_max_sessions(),
            self.memory_pressure_k_override,
        );
        // 라이브 단독 보기에서 멀티라이브 상한보다 많이 담아 온 채 멀티라이브로 돌아오면 넘는 패인은
        // 재생을 멈춘다(동시 재생 한도 — 선택하면 그 패인이 재생된다).
        if self.stage_session_id.is_none() {
            k = k.min(self.max_multi_live_sessions());
        }
        let focus_main = if self.grid_layout_mode == MultiLiveGridLayoutMode::Focus {
            self.focus_main_session_id
        } else {
            None
        };
        let audible: HashSet<Uuid> = sessions
            .iter()
            .filter(|s| self.is_audible_session(s))
            .map(|s| s.id())
            .collect();
        let ids: Vec<Uuid> = sessions.iter().map(|s| s.id()).collect();
        let ranked = policy::rank_for_decode_budget(
            &policy::rank_inputs(&ids, &self.mru),
            self.selected_session_id,
            self.pip_session_id,
            focus_main,
            &audible,
        );
        let keep_alive = self.watch_keep_alive_session_ids();
        for (rank, id) in ranked.into_iter().enumerate() {
            let Some(s) = sessions.iter().find(|s| s.id() == id).cloned() else {
                continue;
            };
            let decoding = rank < k && self.watch_mode_allows_decode_with(id, &keep_alive);
            self.apply_decode_state(&s, decoding);
        }
    }

    /// 정지 사유가 있는 동안에도 디코드를 이어 갈 세션 — 선택·PiP, 그리고 창 가림만으로 멈추는 동안에는
    /// 들리는 패인(소리를 옮긴 패인, 여러 소리 모드의 켜진 패인)도. 창을 가린 채 소리만 듣는 일이 흔해서
    /// 가림으로 그 패인까지 멈추면 소리가 끊긴다. 발열 정지는 예전처럼 선택·PiP 만 지킨다.
    fn is_protected_during_suspension(&self, session: &MultiLiveSession) -> bool {
        let id = session.id();
        if self.is_protected_session(id) {
            return true;
        }
        if self
            .decode_suspension_reasons
            .contains(&DecodeSuspensionReason::ThermalPressure)
        {
            return false;
        }
        Some(id) == self.audio_session_id
            || Some(id) == self.stage_session_id
            || self.is_audible_session(session)
    }

    pub fn suspend_non_selected_decode(&mut self, reason: DecodeSuspensionReason) {
        let inserted = self.decode_suspension_reasons.insert(reason);
        for s in self.sessions.clone() {
            let protected = self.is_protected_during_suspension(&s);
            self.apply_decode_state(&s, protected);
        }
        if inserted {
            tracing::debug!(
                "decode_suspension reason={reason:?} sessions={}",
                self.sessions.len()
            );
        }
    }

    /// 창 가림 — 가려지면(`true`) 선택·PiP·들리는 패인 밖의 패인 디코드를 멈추고, 다시 보이면 예산대로 되돌린다.
    /// 같은 값이 거듭 와도 한 번만 바뀐다.
    pub fn set_window_occluded(&mut self, occluded: bool) {
        let reason = DecodeSuspensionReason::WindowOcclusion;
        if occluded == self.decode_suspension_reasons.contains(&reason) {
            return;
        }
        if occluded {
            self.suspend_non_selected_decode(reason);
        } else {
            self.restore_decode_from_budget(reason);
        }
    }

    pub fn is_window_occluded(&self) -> bool {
        self.decode_suspension_reasons
            .contains(&DecodeSuspensionReason::WindowOcclusion)
    }

    pub fn restore_decode_from_budget(&mut self, reason: DecodeSuspensionReason) {
        if !self.decode_suspension_reasons.remove(&reason) {
            return;
        }
        self.reconcile_decode_budget();
        tracing::debug!(
            "decode_restoration reason={reason:?} remaining_causes={}",
            self.decode_suspension_reasons.len()
        );
    }

    pub fn handle_memory_pressure(&mut self, critical: bool) {
        self.memory_pressure_k_override = Some(if critical {
            1
        } else {
            (policy::DECODE_BUDGET_K - 1).max(2)
        });
        self.reconcile_decode_budget();
        cancel_slot(&mut self.memory_pressure_recovery_task);
        self.memory_pressure_generation = self.memory_pressure_generation.wrapping_add(1);
        let generation = self.memory_pressure_generation;
        let this = self.this.clone();
        let delay = std::time::Duration::from_secs(if critical { 20 } else { 10 });
        self.memory_pressure_recovery_task = Some(TaskSlot::spawn(move |_| async move {
            tokio::time::sleep(delay).await;
            let Some(shared) = this.upgrade() else { return };
            let mut st = shared.obs.lock();
            if st.memory_pressure_generation != generation {
                return;
            }
            st.memory_pressure_k_override = None;
            st.reconcile_decode_budget();
        }));
    }

    pub fn memory_pressure_override(&self) -> Option<usize> {
        self.memory_pressure_k_override
    }

    /// 스테이지 크기 보고 — 8pt 미만 변화는 무시, 200ms 디바운스 후 패인 크기 갱신.
    pub fn report_stage_size(&mut self, size: (f64, f64)) {
        if let Some((w, h)) = self.last_reported_stage_size {
            if (w - size.0).abs() < 8.0 && (h - size.1).abs() < 8.0 {
                return;
            }
        }
        self.last_reported_stage_size = Some(size);
        cancel_slot(&mut self.pane_resize_debounce_task);
        let this = self.this.clone();
        self.pane_resize_debounce_task = Some(TaskSlot::spawn(move |tid| async move {
            tokio::time::sleep(tuning::PANE_RESIZE_DEBOUNCE).await;
            let Some(shared) = this.upgrade() else { return };
            let mut st = shared.obs.lock();
            if slot_is(&st.pane_resize_debounce_task, tid) {
                st.update_pane_sizes(size.0 as i64, size.1 as i64);
            }
        }));
    }
}
