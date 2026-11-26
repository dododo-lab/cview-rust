//! 패인 픽셀 크기 계산·제출.
//!
//! 원본: `Sources/CViewApp/ViewModels/MultiLiveManager+PaneSizing.swift`.

use cview_player::bandwidth_coordinator::StreamPaneSizeUpdate;

use super::ManagerState;
use crate::viewmodels::multi_live::layout::MultiLiveGridLayoutMode;
use crate::viewmodels::multi_live::policy::grid_pane_size;

impl ManagerState {
    /// 스테이지 크기가 없으면 창/화면 크기로 그리드 추정.
    pub fn update_estimated_pane_sizes(&mut self) {
        if let Some((w, h)) = self.last_reported_stage_size {
            self.update_pane_sizes(w as i64, h as i64);
            return;
        }
        let view = self.multi_view_session_ids();
        let count = view.as_ref().map_or(self.sessions.len(), Vec::len);
        if count == 0 {
            return;
        }
        let (sw, sh) = self.core.deps.fallback_screen_size;
        let (pw, ph) = grid_pane_size(count, sw as i64, sh as i64);
        let updates = self
            .sessions
            .iter()
            .map(|s| {
                let hidden = view.as_ref().is_some_and(|v| !v.contains(&s.id()));
                StreamPaneSizeUpdate {
                    session_id: s.id(),
                    width: if hidden { 0 } else { pw },
                    height: if hidden { 0 } else { ph },
                }
            })
            .collect();
        self.submit_pane_geometry(updates);
    }

    /// 역할별 패인 크기 — PiP 패널, 스테이지(전체/가청 854×480/숨김 0), 포커스, 그리드.
    pub fn update_pane_sizes(&mut self, stage_width: i64, stage_height: i64) {
        if self.sessions.is_empty() || stage_width <= 0 || stage_height <= 0 {
            return;
        }
        self.reconcile_focus_quality(Some((stage_width as f64, stage_height as f64)));
        let updates = self.pane_size_updates(stage_width, stage_height);
        if !updates.is_empty() {
            self.submit_pane_geometry(updates);
        }
    }

    /// 역할별 패인 크기 계산(제출하지 않음). 멀티라이브 상한을 넘어 숨긴 패인은 격자에 없다 — 보이는 수로
    /// 나누고, 숨긴 패인은 0×0.
    pub fn pane_size_updates(
        &self,
        stage_width: i64,
        stage_height: i64,
    ) -> Vec<StreamPaneSizeUpdate> {
        let view = self.multi_view_session_ids();
        let count = view.as_ref().map_or(self.sessions.len(), Vec::len);
        if count == 0 || stage_width <= 0 || stage_height <= 0 {
            return Vec::new();
        }
        let stage_id = self.stage_session_id;
        let focus_main = self.focus_main_session_id.or(self.selected_session_id);
        let focus_ratio = self.layout_ratios.focus_ratio;
        let mode = self.grid_layout_mode;
        let pip_id = self.pip_session_id;
        let (pip_w, pip_h) = self.core.deps.pip.panel_pixel_size();
        self.sessions
            .iter()
            .map(|s| {
                let id = s.id();
                let (w, h) = if view.as_ref().is_some_and(|v| !v.contains(&id)) {
                    (0, 0)
                } else if pip_id == Some(id) && pip_w > 0.0 && pip_h > 0.0 {
                    (pip_w.round() as i64, pip_h.round() as i64)
                } else if let Some(stage) = stage_id {
                    if id == stage {
                        (stage_width, stage_height)
                    } else if self.is_audible_session(s) {
                        (854, 480)
                    } else {
                        (0, 0)
                    }
                } else if mode == MultiLiveGridLayoutMode::Focus
                    && focus_main.is_some()
                    && count > 1
                {
                    if Some(id) == focus_main {
                        ((stage_width as f64 * focus_ratio) as i64, stage_height)
                    } else {
                        let side = (count - 1).max(1) as i64;
                        (
                            (stage_width as f64 * (1.0 - focus_ratio)) as i64,
                            stage_height / side,
                        )
                    }
                } else {
                    grid_pane_size(count, stage_width, stage_height)
                };
                StreamPaneSizeUpdate {
                    session_id: id,
                    width: w,
                    height: h,
                }
            })
            .collect()
    }

    pub fn is_pip_target_on_stage(&self) -> bool {
        let Some(pip) = self.core.deps.pip.active_session_id() else {
            return false;
        };
        self.stage_session_id == Some(pip)
    }

    /// 배치(균등·포커스·사용자)·포커스 방향·메인·비율이 바뀌면 같은 무대 크기로 패인 크기를 다시 낸다 — 무대 요소 크기는
    /// 그대로라 화면이 다시 보고하지 않는다. 예전엔 포커스에서 균등으로 바꿔도 보조 패인이 포커스 때의 좁은 크기
    /// (360p 상한)로 남았다.
    pub fn reapply_pane_sizes(&mut self) {
        self.reapply_pane_sizes_for_pip();
    }

    pub fn reapply_pane_sizes_for_pip(&mut self) {
        let Some((w, h)) = self.last_reported_stage_size else {
            return;
        };
        if w > 0.0 && h > 0.0 {
            self.update_pane_sizes(w as i64, h as i64);
        }
    }
}
