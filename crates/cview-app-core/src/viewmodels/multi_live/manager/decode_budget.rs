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

    /// 디코드 우선순위 — 선택·PiP·포커스 메인·소리 패인, 그다음 최근에 본 순.
    fn decode_rank(&self) -> Vec<Uuid> {
        let focus_main = if self.grid_layout_mode == MultiLiveGridLayoutMode::Focus {
            self.focus_main_session_id
        } else {
            None
        };
        let audible: HashSet<Uuid> = self
            .sessions
            .iter()
            .filter(|s| self.is_audible_session(s))
            .map(|s| s.id())
            .collect();
        let ids: Vec<Uuid> = self.sessions.iter().map(|s| s.id()).collect();
        policy::rank_for_decode_budget(
            &policy::rank_inputs(&ids, &self.mru),
            self.selected_session_id,
            self.pip_session_id,
            focus_main,
            &audible,
        )
    }

    /// 멀티라이브 보기에 올린 세션 — 라이브 단독 보기에서 멀티라이브 상한보다 많이 담아 온 채 멀티라이브로
    /// 돌아왔을 때만 `Some`(그 밖에는 모든 세션이 보인다).
    ///
    /// - 사용자가 고른 목록이 있으면 그 목록만 쓴다 — 상한보다 적게 골랐어도 디코드 순위로 다시 채우지 않는다.
    /// - 아직 고르지 않았으면 자동 보기(잡아 둔 값) → 디코드 우선순위 순으로 상한까지 채운다.
    /// - 어느 쪽이든 선택 세션(단독 보기에서 보던 채널)은 맨 앞에 들어가고, 넘치면 목록의 맨 뒤가 밀려난다.
    pub fn multi_view_session_ids(&self) -> Option<Vec<Uuid>> {
        let max = self.max_multi_live_sessions();
        if self.stage_session_id.is_some() || self.sessions.len() <= max {
            return None;
        }
        let chosen: Vec<Uuid> = self
            .multi_view_ids
            .iter()
            .copied()
            .filter(|id| self.contains(*id))
            .collect();
        let rest: Vec<Uuid> = if chosen.is_empty() {
            self.multi_view_auto_ids
                .iter()
                .copied()
                .chain(self.decode_rank())
                .collect()
        } else {
            chosen
        };
        let mut out: Vec<Uuid> = Vec::with_capacity(max);
        for id in self.selected_session_id.into_iter().chain(rest) {
            if out.len() >= max {
                break;
            }
            if self.contains(id) && !out.contains(&id) {
                out.push(id);
            }
        }
        Some(out)
    }

    /// 지금 보기에 올라간 세션인가 — 상한을 넘지 않았거나 단독 보기면 늘 `true`.
    fn in_multi_view(view: Option<&[Uuid]>, id: Uuid) -> bool {
        view.is_none_or(|v| v.contains(&id))
    }

    /// 멀티라이브에 올릴 세션을 고른다(상한까지, 고른 순서대로). 선택 세션이 빠지면 고른 첫 세션을 선택하고,
    /// 소리도 보이는 패인으로 옮긴다(숨긴 패인이 소리를 쥐면 보이는 패인이 모두 무음이 된다).
    /// 고른 세션이 하나도 없으면(모두 닫힌 세션) 아무것도 바꾸지 않는다.
    pub fn set_multi_view_sessions(&mut self, ids: &[Uuid]) {
        let max = self.max_multi_live_sessions();
        let mut picked: Vec<Uuid> = Vec::with_capacity(max);
        for &id in ids {
            if picked.len() < max && self.contains(id) && !picked.contains(&id) {
                picked.push(id);
            }
        }
        let Some(&first) = picked.first() else {
            return;
        };
        self.multi_view_ids = picked.clone();
        self.multi_view_auto_ids.clear();
        let selected_in_view = self
            .selected_session_id
            .is_some_and(|s| picked.contains(&s));
        if !selected_in_view {
            // 선택이 바뀌며 소리(단일 소리 모드)·디코드 예산도 따라간다.
            self.select_session(first, true);
        } else if !self
            .focus_main_session_id
            .is_some_and(|f| picked.contains(&f))
        {
            self.set_focus_main_session_id(self.selected_session_id);
        }
        self.fit_audio_to_multi_view();
        self.reconcile_decode_budget();
    }

    /// 숨긴 세션이 쥔 소리를 보이는 패인으로 옮긴다 — 단일 소리면 선택 세션으로, 여러 소리면 숨긴 세션을 목록에서
    /// 빼고 끈다(`remove_session` 의 소리 넘기기와 같은 방식).
    fn fit_audio_to_multi_view(&mut self) {
        let Some(view) = self.multi_view_session_ids() else {
            return;
        };
        let hidden: Vec<Uuid> = self
            .sessions
            .iter()
            .map(|s| s.id())
            .filter(|id| !view.contains(id))
            .collect();
        if hidden.is_empty() {
            return;
        }
        if self.is_multi_audio_mode {
            for id in &hidden {
                self.audio_enabled_session_ids.remove(id);
                if let Some(s) = self.session(*id) {
                    s.set_muted(true);
                }
            }
            if self.audio_session_id.is_some_and(|a| hidden.contains(&a)) {
                self.audio_session_id = self
                    .selected_session_id
                    .filter(|s| self.audio_enabled_session_ids.contains(s))
                    .or_else(|| {
                        view.iter()
                            .copied()
                            .find(|v| self.audio_enabled_session_ids.contains(v))
                    })
                    .or(self.selected_session_id);
            }
            self.sync_audio_volumes();
            self.reconcile_focus_quality(None);
        } else if self.audio_session_id.is_some_and(|a| hidden.contains(&a)) {
            if let Some(target) = self.selected_session_id.or_else(|| view.first().copied()) {
                self.route_audio(target);
            }
        }
    }

    /// 멀티라이브 상한 설정이 바뀌었을 때 — 보기·디코드 예산·패인 크기를 다시 맞춘다.
    pub fn multi_live_limits_changed(&mut self) {
        self.fit_audio_to_multi_view();
        self.reconcile_decode_budget();
        self.update_estimated_pane_sizes();
    }

    pub fn reconcile_decode_budget(&mut self) {
        let sessions = self.sessions.clone();
        // 멀티라이브 상한을 넘으면 고른 세션만 보이고, 나머지는 어떤 경우에도 재생을 멈춘다.
        let view = self.multi_view_session_ids();
        if let Some(v) = &view {
            // 고른 목록이 있으면(보던 채널이 앞에 붙었을 수 있다) 그 결과를, 없으면 자동 보기로만 잡아 둔다 —
            // 자동으로 채운 값을 사용자 선택으로 덮어쓰지 않는다.
            if self.multi_view_ids.iter().any(|id| self.contains(*id)) {
                self.multi_view_ids.clone_from(v);
            } else {
                self.multi_view_auto_ids.clone_from(v);
            }
        }
        let in_view = |id: Uuid| Self::in_multi_view(view.as_deref(), id);
        if !self.decode_suspension_reasons.is_empty() {
            for s in &sessions {
                let protected = self.is_protected_during_suspension(s) && in_view(s.id());
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
                self.apply_decode_state(s, in_view(s.id()));
            }
            return;
        }
        let mut k = policy::effective_decode_budget(
            sessions.len(),
            self.effective_max_sessions(),
            self.memory_pressure_k_override,
        );
        // 라이브 단독 보기에서 멀티라이브 상한보다 많이 담아 온 채 멀티라이브로 돌아오면 고른 패인만
        // 재생한다(동시 재생 한도 — 나머지는 라이브 머리줄에만 남는다).
        if self.stage_session_id.is_none() {
            k = k.min(self.max_multi_live_sessions());
        }
        let ranked = self.decode_rank();
        let keep_alive = self.watch_keep_alive_session_ids();
        let mut rank = 0;
        for id in ranked {
            let Some(s) = sessions.iter().find(|s| s.id() == id).cloned() else {
                continue;
            };
            if !in_view(id) {
                self.apply_decode_state(&s, false);
                continue;
            }
            let decoding = rank < k && self.watch_mode_allows_decode_with(id, &keep_alive);
            rank += 1;
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
        // 멀티라이브에서 숨긴 패인은 들리는 패인이어도 멈춘 채 둔다(reconcile_decode_budget 과 같은 규칙).
        let view = self.multi_view_session_ids();
        for s in self.sessions.clone() {
            let protected = self.is_protected_during_suspension(&s)
                && Self::in_multi_view(view.as_deref(), s.id());
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
