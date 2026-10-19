//! 오디오 라우팅·타일 이동·그리드 모드.
//!
//! 원본: `Sources/CViewApp/ViewModels/MultiLiveManager+AudioGrid.swift`.

use uuid::Uuid;

use super::ManagerState;
use crate::viewmodels::multi_live::layout::{
    layout_mode_transition, tile_drop, MultiLiveGridLayoutMode, MultiLiveLayoutRatios,
};
use crate::viewmodels::multi_live::session::MultiLiveSession;

impl ManagerState {
    /// 단일 오디오 — 대상만 소리를 낸다.
    pub fn route_audio(&mut self, session_id: Uuid) {
        if !self.contains(session_id) {
            return;
        }
        self.audio_session_id = Some(session_id);
        for s in &self.sessions {
            s.set_muted(s.id() != session_id);
        }
        self.reconcile_focus_quality(None);
    }

    pub fn toggle_multi_audio_mode(&mut self) {
        self.is_multi_audio_mode = !self.is_multi_audio_mode;
        if self.is_multi_audio_mode {
            self.audio_enabled_session_ids.clear();
            if let Some(current) = self.audio_session_id.or(self.selected_session_id) {
                self.audio_enabled_session_ids.insert(current);
            }
            for s in &self.sessions {
                s.set_muted(!self.audio_enabled_session_ids.contains(&s.id()));
            }
        } else {
            self.audio_enabled_session_ids.clear();
            let active = self.audio_session_id.or(self.selected_session_id);
            for s in &self.sessions {
                s.set_muted(Some(s.id()) != active);
            }
        }
        self.reconcile_focus_quality(None);
    }

    pub fn toggle_session_audio(&mut self, session_id: Uuid) {
        let Some(session) = self.session(session_id).cloned() else {
            self.reconcile_focus_quality(None);
            return;
        };
        if !self.is_multi_audio_mode {
            if self.audio_session_id.or(self.selected_session_id) == Some(session_id) {
                session.set_muted(!session.is_muted());
            } else {
                self.route_audio(session_id);
            }
            self.reconcile_focus_quality(None);
            return;
        }
        if self.is_audible(&session) {
            self.audio_enabled_session_ids.remove(&session_id);
            session.set_muted(true);
        } else {
            self.audio_enabled_session_ids.insert(session_id);
            session.set_muted(false);
        }
        self.reconcile_focus_quality(None);
    }

    pub fn is_audio_enabled(&self, session: &MultiLiveSession) -> bool {
        if self.is_multi_audio_mode {
            self.audio_enabled_session_ids.contains(&session.id())
        } else {
            self.audio_session_id.or(self.selected_session_id) == Some(session.id())
        }
    }

    pub fn is_audible(&self, session: &MultiLiveSession) -> bool {
        self.is_audio_enabled(session) && !session.is_muted()
    }

    /// `moveSession(from:to:)` — 단일 원소 이동.
    pub fn move_session(&mut self, from: usize, to_offset: usize) {
        tile_drop::apply_move(&mut self.sessions, from, to_offset);
        self.sessions_did_change();
        self.save_state();
    }

    /// 타일 끌어 놓기 — 큰 화면이 걸리면 맞바꾸고 메인을 넘긴다.
    pub fn drop_tile(
        &mut self,
        dragged: Uuid,
        target: Uuid,
        double_tap_focused_id: &mut Option<Uuid>,
    ) -> bool {
        let double_tap_main = double_tap_focused_id.filter(|id| self.contains(*id));
        let focus_layout_main = if self.grid_layout_mode == MultiLiveGridLayoutMode::Focus {
            self.focus_main_session_id
                .filter(|id| self.contains(*id))
                .or_else(|| self.sessions.first().map(|s| s.id()))
        } else {
            None
        };
        let order: Vec<Uuid> = self.sessions.iter().map(|s| s.id()).collect();
        let Some(outcome) = tile_drop::resolve(
            &order,
            dragged,
            target,
            double_tap_main.or(focus_layout_main),
        ) else {
            return false;
        };
        match outcome {
            tile_drop::Outcome::Swap(first, second, new_main) => {
                self.sessions.swap(first, second);
                self.sessions_did_change();
                if double_tap_main.is_some() {
                    *double_tap_focused_id = Some(new_main);
                } else {
                    self.set_focus_main_session_id(Some(new_main));
                }
            }
            tile_drop::Outcome::Move { from, to_offset } => {
                tile_drop::apply_move(&mut self.sessions, from, to_offset);
                self.sessions_did_change();
            }
        }
        self.save_state();
        true
    }

    pub fn reset_layout_ratios(&mut self) {
        self.set_layout_ratios(MultiLiveLayoutRatios::default());
    }

    pub fn apply_grid_layout_mode(&mut self, mode: MultiLiveGridLayoutMode) {
        let needs_repair =
            mode == MultiLiveGridLayoutMode::Focus && self.focus_main_session_id.is_none();
        if layout_mode_transition::resolve(self.grid_layout_mode, mode, needs_repair)
            == layout_mode_transition::Outcome::Noop
        {
            return;
        }
        if mode == MultiLiveGridLayoutMode::Custom {
            self.set_layout_ratios(MultiLiveLayoutRatios::with_focus_ratio(
                self.layout_ratios.focus_ratio,
            ));
        }
        if mode == MultiLiveGridLayoutMode::Focus && self.focus_main_session_id.is_none() {
            let first = self.sessions.first().map(|s| s.id());
            self.set_focus_main_session_id(first);
        }
        self.set_grid_layout_mode(mode);
        self.save_state();
    }
}
