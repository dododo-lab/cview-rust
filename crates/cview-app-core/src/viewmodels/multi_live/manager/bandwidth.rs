//! 대역폭 코디네이션 루프와 hls.js 캡 적용.
//!
//! 원본: `Sources/CViewApp/ViewModels/MultiLiveManager+Bandwidth.swift`.

use std::time::Duration;

use uuid::Uuid;

use cview_player::bandwidth_coordinator::{
    BandwidthAdvice, BandwidthCoordinatorConfig, StreamPaneSizeUpdate,
};
use cview_player::policy::constants::multi_live_bw_defaults as bw;

use super::ManagerState;
use crate::viewmodels::multi_live::policy::{
    should_bypass_bandwidth_caps, smoothed_bitrate_cap_kbps,
};
use crate::viewmodels::support::{cancel_slot, slot_is, TaskSlot};

impl ManagerState {
    pub fn synchronize_bandwidth_selection(&mut self) {
        self.bandwidth_selection_revision = self.bandwidth_selection_revision.wrapping_add(1);
        let revision = self.bandwidth_selection_revision;
        self.core
            .bw()
            .replace_selected_stream(self.selected_session_id, revision);
    }

    pub fn submit_pane_geometry(&mut self, updates: Vec<StreamPaneSizeUpdate>) {
        self.pane_geometry_revision = self.pane_geometry_revision.wrapping_add(1);
        let revision = self.pane_geometry_revision;
        self.core.bw().update_pane_sizes(&updates, revision);
    }

    pub fn update_bandwidth_coordinator_config(&self) {
        let Some(settings) = &self.settings else {
            return;
        };
        let ml = settings.multi_live();
        let config = BandwidthCoordinatorConfig {
            safety_factor: bw::SAFETY_FACTOR,
            min_per_stream_bitrate: bw::MIN_PER_STREAM_BITRATE,
            history_size: bw::HISTORY_SIZE,
            low_buffer_threshold: bw::LOW_BUFFER_THRESHOLD,
            high_buffer_threshold: bw::HIGH_BUFFER_THRESHOLD,
            enable_level_capping: ml.level_capping_enabled,
            selected_session_weight: ml.selected_session_bw_weight,
            emergency_buffer_threshold: bw::EMERGENCY_BUFFER_THRESHOLD,
            ..BandwidthCoordinatorConfig::default()
        };
        self.core.bw().apply_config(config);
    }

    /// 설정이 켜져 있으면 지터 있는 주기 루프를 시작한다.
    pub fn start_bandwidth_coordination(&mut self) {
        let enabled = self
            .settings
            .as_ref()
            .is_some_and(|s| s.multi_live().bandwidth_coordination_enabled);
        if !enabled || self.bw_coordinator_task.is_some() {
            return;
        }
        self.bandwidth_loop_generation = self.bandwidth_loop_generation.wrapping_add(1);
        let loop_generation = self.bandwidth_loop_generation;
        self.update_bandwidth_coordinator_config();
        self.update_estimated_pane_sizes();
        let this = self.this.clone();
        self.bw_coordinator_task = Some(TaskSlot::spawn(move |tid| async move {
            loop {
                let Some(shared) = this.upgrade() else { return };
                let base = shared
                    .core
                    .deps
                    .pacing
                    .base_interval_secs(bw::UPDATE_INTERVAL_SECS);
                let jitter = (rand::random::<f64>() * 0.36 - 0.18) * base;
                let secs = (base + jitter).max(2.0);
                drop(shared);
                tokio::time::sleep(Duration::from_secs_f64(secs)).await;
                let Some(shared) = this.upgrade() else { return };
                let snapshot = {
                    let st = shared.obs.read();
                    if !slot_is(&st.bw_coordinator_task, tid) {
                        return;
                    }
                    st.collect_coordination_snapshot()
                };
                let samples: Vec<_> = snapshot.metrics.iter().map(|m| m.to_sample()).collect();
                let advices = {
                    let mut coordinator = shared.core.bw();
                    coordinator.report_snapshot(&samples, &snapshot.active_session_ids);
                    coordinator.compute_advice()
                };
                let mut st = shared.obs.lock();
                if st.bandwidth_loop_generation != loop_generation
                    || st.sessions.len() < 2
                    || !slot_is(&st.bw_coordinator_task, tid)
                {
                    if st.bandwidth_loop_generation == loop_generation {
                        st.bw_coordinator_task = None;
                    }
                    return;
                }
                st.apply_bandwidth_advices(&advices);
                st.apply_rate_budget();
            }
        }));
        tracing::debug!("MultiLive: 대역폭 코디네이션 시작");
    }

    pub fn stop_bandwidth_coordination(&mut self) {
        self.bandwidth_loop_generation = self.bandwidth_loop_generation.wrapping_add(1);
        cancel_slot(&mut self.bw_coordinator_task);
        let ids: Vec<Uuid> = self.sessions.iter().map(|s| s.id()).collect();
        for id in ids {
            self.release_bandwidth_caps_immediately(id);
        }
        tracing::info!("MultiLive: 대역폭 코디네이션 중지");
    }

    pub fn release_bandwidth_caps_immediately(&mut self, session_id: Uuid) {
        let Some(hls) = self.session(session_id).and_then(|s| s.player().engine()) else {
            return;
        };
        hls.set_max_resolution_height(0);
        self.last_applied_cap_height.insert(session_id, 0);
        hls.set_max_bitrate(0);
        self.last_applied_hls_kbps.remove(&session_id);
    }

    /// 코디네이터 권고 적용 — 메인·PiP·선택은 캡 해제, 수동 화질은 긴급 강등만 허용.
    pub fn apply_bandwidth_advices(&mut self, advices: &[BandwidthAdvice]) {
        let quality_main = self.quality_main_session_id();
        for advice in advices {
            let sid = advice.session_id;
            let Some(session) = self.session(sid).cloned() else {
                continue;
            };
            if Some(sid) == quality_main || Some(sid) == self.pip_session_id {
                self.release_bandwidth_caps_immediately(sid);
                continue;
            }
            let hls = session.player().engine();
            if let Some(h) = &hls {
                if should_bypass_bandwidth_caps(h.force_highest_quality(), h.forced_height_active())
                    && !advice.emergency_downgrade
                {
                    if h.max_adaptive_height() > 0
                        || h.max_adaptive_bitrate() > 0
                        || self.last_applied_cap_height.get(&sid).copied().unwrap_or(0) > 0
                        || self.last_applied_hls_kbps.contains_key(&sid)
                    {
                        self.release_bandwidth_caps_immediately(sid);
                    }
                    continue;
                }
            }
            if Some(sid) == self.selected_session_id {
                if let Some(h) = &hls {
                    h.set_max_resolution_height(0);
                    h.set_max_bitrate(0);
                }
                self.last_applied_cap_height.insert(sid, 0);
                self.last_applied_hls_kbps.remove(&sid);
                continue;
            }
            let Some(h) = hls else {
                continue;
            };
            if advice.capped_max_height > 0 {
                let last = self.last_applied_cap_height.get(&sid).copied().unwrap_or(0);
                if last != advice.capped_max_height {
                    h.set_max_resolution_height(advice.capped_max_height);
                    self.last_applied_cap_height
                        .insert(sid, advice.capped_max_height);
                    tracing::info!(
                        "hls.js pane cap: session={sid} maxHeight={}p",
                        advice.capped_max_height
                    );
                }
            }
            if !advice.has_bandwidth_estimate {
                if h.max_adaptive_bitrate() > 0 || self.last_applied_hls_kbps.contains_key(&sid) {
                    h.set_max_bitrate(0);
                    self.last_applied_hls_kbps.remove(&sid);
                }
                continue;
            }
            if let Some(kbps) = smoothed_bitrate_cap_kbps(
                advice.max_allowed_bitrate,
                advice.emergency_downgrade,
                self.last_applied_hls_kbps.get(&sid).copied(),
            ) {
                h.set_max_bitrate(kbps as i64);
                self.last_applied_hls_kbps.insert(sid, kbps);
            }
        }
    }
}
