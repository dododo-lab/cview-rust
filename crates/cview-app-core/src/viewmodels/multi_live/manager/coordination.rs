//! 대역폭 코디네이션 스냅샷 수집.
//!
//! 원본: `Sources/CViewApp/ViewModels/MultiLiveManager+CoordinationSnapshot.swift`.

use std::collections::HashSet;

use uuid::Uuid;

use cview_player::bandwidth_coordinator::StreamBandwidthSample;
use cview_player::hlsjs::network_metrics::StreamTransferSample;

use super::ManagerState;
use crate::viewmodels::multi_live::policy::{
    should_bypass_bandwidth_caps, should_include_in_bandwidth_active_set,
};

#[derive(Debug, Clone, PartialEq)]
pub struct MetricSnapshot {
    pub session_id: Uuid,
    pub bitrate_bps: f64,
    pub buffer_length: f64,
    pub playback_rate: f64,
    pub buffer_confidence: f64,
    pub measured_fetch_duration: f64,
    pub measured_segment_duration: f64,
    pub transfers: Vec<StreamTransferSample>,
    pub protects_quality: bool,
    pub has_manual_quality: bool,
}

impl MetricSnapshot {
    pub fn to_sample(&self) -> StreamBandwidthSample {
        let mut s = StreamBandwidthSample::new(
            self.session_id,
            self.bitrate_bps,
            self.buffer_length,
            self.measured_fetch_duration,
            self.measured_segment_duration,
        );
        s.buffer_confidence = self.buffer_confidence;
        s.playback_rate = self.playback_rate;
        s.transfers = self.transfers.clone();
        s.protects_quality = self.protects_quality;
        s.has_manual_quality = self.has_manual_quality;
        s
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CoordinationSnapshot {
    pub metrics: Vec<MetricSnapshot>,
    pub active_session_ids: HashSet<Uuid>,
}

impl ManagerState {
    pub fn collect_coordination_snapshot(&self) -> CoordinationSnapshot {
        let mut snapshot = CoordinationSnapshot::default();
        let quality_main = self.quality_main_session_id();
        for s in &self.sessions {
            let sid = s.id();
            let Some(hls) = s.player().engine() else {
                continue;
            };
            if should_include_in_bandwidth_active_set(
                &s.player().stream_phase(),
                hls.is_suspend_intent_active(),
                hls.is_in_error_state(),
            ) {
                snapshot.active_session_ids.insert(sid);
            }
            if !hls.is_playing() {
                continue;
            }
            let transfers = hls.transfers();
            let last = transfers.last().cloned();
            snapshot.metrics.push(MetricSnapshot {
                session_id: sid,
                bitrate_bps: hls.current_bitrate(),
                buffer_length: hls.buffer_ahead_seconds().max(0.0),
                playback_rate: hls.rate(),
                buffer_confidence: 1.0,
                measured_fetch_duration: last.as_ref().map_or(0.0, |t| t.end - t.request_start),
                measured_segment_duration: last
                    .as_ref()
                    .map_or_else(|| hls.measured_segment_duration(), |t| t.media_duration),
                transfers,
                protects_quality: Some(sid) == self.selected_session_id
                    || Some(sid) == quality_main
                    || Some(sid) == self.pip_session_id,
                has_manual_quality: should_bypass_bandwidth_caps(
                    hls.force_highest_quality(),
                    hls.forced_height_active(),
                ),
            });
        }
        snapshot
    }
}
