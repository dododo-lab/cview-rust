//! 포커스·그리드 화질 목표 조정과 화질 개선 적용.
//!
//! 원본: `Sources/CViewApp/ViewModels/MultiLiveManager+FocusQuality.swift`
//! (`MultiLiveFocusQualityPolicy` 자체는 `policy::focus_quality`).

use uuid::Uuid;

use cview_core::models::settings::PlayerSettings;
use cview_player::hlsjs::profile::HlsJsForcedQualityTarget;

use super::ManagerState;
use crate::viewmodels::multi_live::layout::MultiLiveGridLayoutMode;
use crate::viewmodels::multi_live::policy::focus_quality;

impl ManagerState {
    pub(super) fn reconcile_grid_presentation(&mut self, previously_grid: bool) {
        if self.is_grid_layout == previously_grid {
            return;
        }
        if self.is_grid_layout {
            self.set_stage_session(None);
        }
        if !self.is_grid_layout && self.double_tap_zoom_session_id.is_some() {
            self.double_tap_zoom_session_id = None;
        }
        self.reconcile_focus_quality(None);
    }

    /// 1080p 목표를 받는 메인 — 포커스 그리드는 포커스 메인, 그 외는 선택 세션, 없으면 첫 세션.
    pub fn quality_main_session_id(&self) -> Option<Uuid> {
        let first = self.sessions.first().map(|s| s.id());
        if self.is_grid_layout && self.grid_layout_mode == MultiLiveGridLayoutMode::Focus {
            return self
                .focus_main_session_id
                .filter(|id| self.contains(*id))
                .or(first);
        }
        self.selected_session_id
            .filter(|id| self.contains(*id))
            .or(first)
    }

    pub fn thermal_secondary_quality_cap_active(&self) -> bool {
        self.thermal_secondary_quality_cap_active
    }

    pub fn set_thermal_secondary_quality_cap(&mut self, active: bool) {
        if self.thermal_secondary_quality_cap_active == active {
            return;
        }
        self.thermal_secondary_quality_cap_active = active;
        self.reconcile_focus_quality(None);
        tracing::info!(
            "thermal_secondary_quality_cap active={active} sessions={}",
            self.sessions.len()
        );
    }

    pub(super) fn reconcile_rewind_recording(&self) {
        let main = self.rewind_main_session_id();
        for s in &self.sessions {
            let desired = Some(s.id()) == main;
            if s.player().rewind_recording_desired() != desired {
                s.player().set_rewind_recording_desired(desired);
            }
        }
    }

    /// 역할별 화질 목표 — 메인/PiP 1080p, 보조는 패인 크기 기반.
    pub fn reconcile_focus_quality(&mut self, stage_size: Option<(f64, f64)>) {
        if self.is_updating_session_selection || self.sessions.is_empty() {
            return;
        }
        self.reconcile_rewind_recording();
        self.reconcile_picture_enhancement();
        let main = self.quality_main_session_id();
        let size = stage_size.or(self.last_reported_stage_size);
        let pane_size = size.map(|stage| {
            focus_quality::secondary_pane_size(
                stage,
                self.sessions.len(),
                self.is_grid_layout && self.grid_layout_mode == MultiLiveGridLayoutMode::Focus,
                self.focus_direction.is_horizontal(),
                self.layout_ratios.focus_ratio,
            )
        });
        let thermal_cap = self.thermal_secondary_quality_cap_active;
        let warm_candidate = if self.is_grid_layout {
            None
        } else {
            self.watch_warm_candidate_id()
        };
        for s in self.sessions.clone() {
            let id = Some(s.id());
            let primary = id == main || id == self.pip_session_id;
            let secondary_pane_size = if self.is_grid_layout {
                pane_size
            } else if id == warm_candidate {
                Some(size.unwrap_or((0.0, 0.0)))
            } else {
                Some((0.0, 0.0))
            };
            let target = if primary {
                HlsJsForcedQualityTarget::MULTI_LIVE_1080P_8MBPS
            } else {
                focus_quality::secondary_target(
                    secondary_pane_size,
                    s.player().multi_live_quality_target().height,
                    !s.player().is_muted(),
                    thermal_cap,
                )
            };
            if !s.player().set_multi_live_quality_target(target) {
                continue;
            }
            self.release_bandwidth_caps_immediately(s.id());
            tracing::info!(
                "focus_quality session={} role={} target_height={} target_kbps={}",
                s.id(),
                if primary { "main" } else { "secondary" },
                target.height,
                target.target_bitrate_kbps
            );
        }
    }

    /// (자동 개선, 업스케일) — 둘 다 메인 세션만.
    /// 자동 개선은 켜져 있는 동안 영상에 CSS 필터를 걸어 영상 프레임마다 GPU 로 거르고, 2초마다 프레임을
    /// 읽어 간다. 작은 보조 패인에서는 보정이 거의 보이지 않는데 패인 수만큼 그 비용이 늘었다(GPU 최적화
    /// 2026-10-24). 메인이 바뀌면 `reconcile_focus_quality` 가 다시 나눈다.
    pub fn picture_enhancement(&self, session_id: Uuid, ps: &PlayerSettings) -> (bool, bool) {
        let main = Some(session_id) == self.rewind_main_session_id();
        (
            ps.auto_picture_enhancement && main,
            ps.video_upscaling && main,
        )
    }

    pub fn reconcile_picture_enhancement(&self) {
        let Some(settings) = &self.settings else {
            return;
        };
        let ps = settings.player();
        for s in &self.sessions {
            let (auto, upscale) = self.picture_enhancement(s.id(), &ps);
            s.player().apply_picture_enhancement(auto, upscale);
        }
    }
}
