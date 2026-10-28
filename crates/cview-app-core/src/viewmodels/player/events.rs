//! 코디네이터 이벤트 처리와 hls.js 엔진 콜백 (`PlayerViewModel+EventHandling`).

use std::time::{Duration, Instant};

use chrono::Utc;
use cview_core::models::player::{PlayerEngineType, PlayerPhase};
use cview_player::coordinator::{StreamCoordinator, StreamEvent, StreamPhase};
use cview_player::policy::RecoverySignal;

use super::{upcast, PlayerState, PlayerViewModel, MAX_LATENCY_HISTORY};
use crate::viewmodels::support::{cancel_slot, slot_is, TaskSlot};

impl PlayerViewModel {
    pub(super) fn start_uptime_timer(&self, coordinator: StreamCoordinator) {
        let state = self.state.clone();
        let mut s = self.state.lock();
        let session = s.session_id;
        cancel_slot(&mut s.uptime_task);
        s.uptime_task = Some(TaskSlot::spawn(move |id| async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                let next = coordinator.uptime();
                let mut s = state.lock();
                if !slot_is(&s.uptime_task, id) || s.session_id != session {
                    return;
                }
                if s.uptime as i64 != next as i64 {
                    s.uptime = next;
                }
            }
        }));
    }

    pub(super) fn start_event_listening(&self, coordinator: &StreamCoordinator, session: u64) {
        let mut rx = coordinator.events();
        let me = self.clone();
        let mut s = self.state.lock();
        cancel_slot(&mut s.event_task);
        s.event_task = Some(TaskSlot::spawn(move |id| async move {
            while let Some(event) = rx.recv().await {
                {
                    let s = me.state.read();
                    if !slot_is(&s.event_task, id) || s.session_id != session {
                        return;
                    }
                }
                if let Some(tap) = me.callbacks().on_stream_event {
                    tap(&event);
                }
                me.handle_stream_event(event);
            }
        }));
    }

    fn handle_stream_phase(&self, phase: StreamPhase) {
        {
            let state = self.state.clone();
            let mut s = self.state.lock();
            match &phase {
                StreamPhase::Buffering => {
                    if s.stream_phase == StreamPhase::Playing {
                        let recently_playing = s
                            .last_playing_time
                            .is_some_and(|t| t.elapsed() < Duration::from_millis(1_500));
                        if !recently_playing {
                            cancel_slot(&mut s.buffering_debounce);
                            s.buffering_debounce = Some(TaskSlot::spawn(move |id| async move {
                                tokio::time::sleep(Duration::from_secs(1)).await;
                                let mut s = state.lock();
                                if slot_is(&s.buffering_debounce, id) {
                                    s.buffering_started_at.get_or_insert_with(Instant::now);
                                    s.stream_phase = StreamPhase::Buffering;
                                    s.buffering_debounce = None;
                                }
                            }));
                        }
                    } else if s.stream_phase != StreamPhase::Buffering {
                        s.buffering_started_at.get_or_insert_with(Instant::now);
                        s.stream_phase = StreamPhase::Buffering;
                    }
                }
                StreamPhase::Playing => {
                    cancel_slot(&mut s.buffering_debounce);
                    s.last_playing_time = Some(Instant::now());
                    Self::end_buffering_event(&mut s);
                    if s.stream_phase != StreamPhase::Playing {
                        s.stream_phase = StreamPhase::Playing;
                    }
                    if s.error_message.is_some() {
                        s.error_message = None;
                    }
                }
                other => {
                    if &s.stream_phase != other {
                        s.stream_phase = other.clone();
                    }
                }
            }
            if let StreamPhase::Error(msg) = &phase {
                if s.error_message.as_ref() != Some(msg) {
                    s.error_message = Some(msg.clone());
                }
            }
        }
        self.notify_playback_state_changed();
    }

    fn end_buffering_event(s: &mut PlayerState) {
        if let Some(started) = s.buffering_started_at.take() {
            tracing::debug!(
                "buffering.end ch={} duration_ms={}",
                s.current_channel_id.as_deref().unwrap_or("-"),
                started.elapsed().as_millis()
            );
        }
    }

    /// 코디네이터 이벤트를 관찰 상태로 옮긴다. 같은 값이면 쓰지 않는다.
    pub fn handle_stream_event(&self, event: StreamEvent) {
        match event {
            StreamEvent::PhaseChanged(phase) => self.handle_stream_phase(phase),
            StreamEvent::ProxyRouteChanged(status) => {
                Self::apply_proxy_route_status(&mut self.state.lock(), Some(status));
            }
            StreamEvent::QualitySelected(q) => {
                let mut s = self.state.lock();
                if s.current_quality.as_ref() != Some(&q) {
                    s.current_quality = Some(q);
                }
                if let Some(c) = s.coordinator.clone() {
                    let qualities = c.available_qualities();
                    Self::update_multi_live_quality_options(&mut s, qualities);
                }
            }
            StreamEvent::QualityChanged(q) => {
                let s = self.state.read();
                if s.current_quality.as_ref() != Some(&q) {
                    drop(s);
                    self.state.lock().current_quality = Some(q);
                }
            }
            StreamEvent::LatencyUpdate(info) => {
                let mut s = self.state.lock();
                if s.latency_info != Some(info) {
                    s.latency_info = Some(info);
                }
                s.latency_sample_sequence += 1;
                let n = s.latency_sample_sequence;
                if n == 1 || n.is_multiple_of(10) {
                    s.latency_history.push((Utc::now(), info.current));
                    if s.latency_history.len() > MAX_LATENCY_HISTORY {
                        s.latency_history.remove(0);
                    }
                }
            }
            StreamEvent::BufferUpdate(health) => {
                let s = self.state.read();
                if s.buffer_health != Some(health) {
                    drop(s);
                    self.state.lock().buffer_health = Some(health);
                }
            }
            StreamEvent::Error(msg) => {
                let s = self.state.read();
                if s.error_message.as_ref() != Some(&msg) {
                    drop(s);
                    self.state.lock().error_message = Some(msg);
                }
            }
            StreamEvent::StreamEnded => {
                let mut s = self.state.lock();
                cancel_slot(&mut s.buffering_debounce);
                Self::end_buffering_event(&mut s);
                if s.stream_phase != StreamPhase::StreamEnded {
                    s.stream_phase = StreamPhase::StreamEnded;
                }
            }
            StreamEvent::Stopped => {
                let mut s = self.state.lock();
                Self::apply_proxy_route_status(&mut s, None);
                Self::end_buffering_event(&mut s);
                if s.stream_phase != StreamPhase::Idle {
                    s.stream_phase = StreamPhase::Idle;
                }
            }
            StreamEvent::TimeShiftEnded => {
                let s = self.state.read();
                if s.is_time_shifting {
                    drop(s);
                    self.state.lock().is_time_shifting = false;
                }
            }
        }
    }

    // MARK: 엔진 콜백 (hls.js → 코디네이터)

    fn current_coordinator(&self) -> Option<StreamCoordinator> {
        self.state.read().coordinator.clone()
    }

    /// `onStateChange`.
    pub async fn engine_state_changed(&self, phase: &PlayerPhase) {
        let Some(c) = self.current_coordinator() else {
            return;
        };
        match phase {
            PlayerPhase::Playing => {
                c.handle_playback_engine_state(phase);
                c.report_engine_playing().await;
            }
            PlayerPhase::Error(_) => {
                c.handle_recovery_signal(RecoverySignal::EngineError {
                    engine: PlayerEngineType::Hlsjs,
                    detail: "HLS.js 재생 오류".into(),
                })
                .await;
            }
            _ => {}
        }
    }

    /// `onPlaybackStalled`.
    pub async fn engine_playback_stalled(&self) {
        if let Some(c) = self.current_coordinator() {
            c.handle_recovery_signal(RecoverySignal::PlaybackStall {
                source: "HLS.js playback".into(),
            })
            .await;
        }
    }

    /// `onRebufferStateChanged`.
    pub fn engine_rebuffer_changed(&self, stalled: bool) {
        if let Some(c) = self.current_coordinator() {
            c.handle_playback_engine_state(&if stalled {
                PlayerPhase::Buffering { progress: 0.0 }
            } else {
                PlayerPhase::Playing
            });
        }
    }

    /// `onMatchWebV2Fallback`.
    pub async fn engine_match_web_v2_fallback(&self, reason: &str) {
        let (c, e) = {
            let s = self.state.read();
            (s.coordinator.clone(), s.engine.clone())
        };
        if let (Some(c), Some(e)) = (c, e) {
            c.handle_match_web_v2_fallback(reason, &upcast(&e)).await;
        }
    }

    /// `onSurfacePresented`.
    pub fn engine_surface_presented(&self) {
        self.note_presented_frame();
    }
}
