//! 레이아웃 선호 저장·복원.
//!
//! 원본: `Sources/CViewApp/ViewModels/MultiLiveManager+Persistence.swift`.

use cview_core::models::settings_ui::{MultiLiveFocusDirectionSetting, MultiLiveLayoutMode};

use super::ManagerState;
use crate::viewmodels::multi_live::layout::{
    MultiLiveFocusDirection, MultiLiveGridLayoutMode, MultiLiveLayoutPrefs, MultiLiveLayoutRatios,
};

fn direction_from_setting(d: MultiLiveFocusDirectionSetting) -> MultiLiveFocusDirection {
    match d {
        MultiLiveFocusDirectionSetting::Left => MultiLiveFocusDirection::Left,
        MultiLiveFocusDirectionSetting::Right => MultiLiveFocusDirection::Right,
        MultiLiveFocusDirectionSetting::Top => MultiLiveFocusDirection::Top,
        MultiLiveFocusDirectionSetting::Bottom => MultiLiveFocusDirection::Bottom,
    }
}

impl ManagerState {
    pub(super) fn current_layout_prefs(&self) -> MultiLiveLayoutPrefs {
        MultiLiveLayoutPrefs::new(
            self.grid_layout_mode,
            self.layout_ratios,
            self.focus_direction,
        )
    }

    pub fn save_state(&self) {
        if self.is_terminating {
            return;
        }
        self.current_layout_prefs()
            .save(self.core.deps.layout_prefs.as_ref());
    }

    pub fn has_applied_layout_prefs(&self) -> bool {
        self.has_applied_layout_prefs
    }

    pub fn apply_initial_layout_prefs_if_needed(&mut self) {
        if self.has_applied_layout_prefs {
            return;
        }
        if let Some(store) = &self.settings {
            if !store.is_loaded() {
                return;
            }
        }
        self.has_applied_layout_prefs = true;
        let ml = self
            .settings
            .as_ref()
            .map(|s| s.multi_live())
            .unwrap_or_default();
        let saved = if ml.remember_last_layout {
            MultiLiveLayoutPrefs::load(self.core.deps.layout_prefs.as_ref())
        } else {
            None
        };
        if let Some(prefs) = saved {
            self.set_grid_layout_mode(prefs.parsed_grid_layout_mode());
            self.set_layout_ratios(MultiLiveLayoutRatios {
                horizontal_ratio: MultiLiveLayoutRatios::clamped(prefs.horizontal_ratio),
                vertical_ratio: MultiLiveLayoutRatios::clamped(prefs.vertical_ratio),
                bottom_row_ratio: MultiLiveLayoutRatios::clamped(prefs.bottom_row_ratio),
                focus_ratio: MultiLiveLayoutRatios::clamped_focus(prefs.focus_ratio),
            });
            self.set_focus_direction(prefs.parsed_focus_direction());
            tracing::info!(
                "MultiLive: 마지막 사용 레이아웃 복원 — mode={} dir={}",
                prefs.grid_layout_mode,
                prefs.focus_direction
            );
        } else {
            self.set_grid_layout_mode(if ml.default_layout_mode == MultiLiveLayoutMode::Focus {
                MultiLiveGridLayoutMode::Focus
            } else {
                MultiLiveGridLayoutMode::Preset
            });
            self.set_focus_direction(direction_from_setting(ml.default_focus_direction));
            tracing::info!("MultiLive: 기본 레이아웃 적용");
        }
    }
}
