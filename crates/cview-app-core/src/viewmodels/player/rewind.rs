//! 라이브 복귀·되감기(타임머신·타임시프트 기록).

use std::sync::Arc;
use std::time::{Duration, Instant};

use cview_core::models::settings::PlayerSettings;
use cview_player::coordinator::{StreamCoordinator, StreamPhase, TimeshiftRecordingConfig};

use super::{
    spawn_detached, PlayerState, PlayerViewModel, RewindSettings, RewindSource, SharedLiveEngine,
    MAX_TIME_MACHINE_WINDOW, MINIMUM_REWIND_WINDOW, REWIND_RELEASE_GRACE,
};
use crate::viewmodels::support::{cancel_slot, slot_is, TaskSlot};

impl PlayerViewModel {
    fn snap_to_live_edge(engine: &SharedLiveEngine) {
        if !engine.is_hlsjs() {
            return;
        }
        if let Some(h) = engine.hls() {
            h.set_live_latency_correction_enabled(true);
        }
        engine.seek_to_live_edge();
    }

    /// 수동 '실시간으로 이동'.
    pub fn jump_to_live(&self) {
        let (engine, coordinator, paused) = {
            let mut s = self.state.lock();
            let ok = matches!(
                s.stream_phase,
                StreamPhase::Playing | StreamPhase::Buffering
            ) || (s.stream_phase == StreamPhase::Paused && s.is_time_shifting);
            if !ok {
                return;
            }
            let Some(engine) = s.engine.clone() else {
                return;
            };
            s.is_time_shifting = false;
            s.rewind_seek_hint = None;
            s.rewind_request_serial = s.rewind_request_serial.wrapping_add(1);
            (
                engine,
                s.coordinator.clone(),
                s.stream_phase == StreamPhase::Paused,
            )
        };
        let Some(coordinator) = coordinator else {
            Self::snap_to_live_edge(&engine);
            if paused {
                engine.pause();
            }
            return;
        };
        if coordinator.is_local_timeshift_active() {
            spawn_detached(async move { coordinator.exit_timeshift().await });
        } else {
            Self::snap_to_live_edge(&engine);
            if paused {
                engine.pause();
            }
            spawn_detached(async move { coordinator.set_time_shift_active(false).await });
        }
    }

    pub fn configure_time_machine_access(&self, is_cheat_key_user: bool, available: bool) {
        let coordinator = {
            let mut s = self.state.lock();
            let was = s.is_time_machine_available;
            s.is_cheat_key_user = is_cheat_key_user;
            s.is_time_machine_available = is_cheat_key_user && available;
            if was && !s.is_time_machine_available && s.is_time_shifting {
                s.is_time_shifting = false;
                s.coordinator.clone()
            } else {
                None
            }
        };
        if let Some(c) = coordinator {
            spawn_detached(async move { c.set_time_shift_active(false).await });
        }
    }

    pub fn note_presented_frame(&self) {
        let mut s = self.state.lock();
        s.presented_frame_tick = s.presented_frame_tick.wrapping_add(1);
    }

    /// 서버 타임머신 — 최근 1시간 안에서 과거 위치로 seek.
    pub async fn seek_time_machine(&self, seconds_behind: f64) {
        let (engine, coordinator, range) = {
            let s = self.state.read();
            if !s.is_time_machine_available {
                return;
            }
            let Some(engine) = s.engine.clone() else {
                return;
            };
            let Some(range) = engine.seekable_range() else {
                return;
            };
            (engine, s.coordinator.clone(), range)
        };
        let window = MAX_TIME_MACHINE_WINDOW.min(range.end() - range.start());
        if window <= 0.0 {
            return;
        }
        let behind = seconds_behind.max(0.0).min(window);
        if behind <= self.state.read().live_edge_tolerance() {
            self.jump_to_live();
            return;
        }
        self.state.lock().is_time_shifting = true;
        if let Some(c) = &coordinator {
            c.set_time_shift_active(true).await;
        }
        let same = self
            .state
            .read()
            .engine
            .as_ref()
            .is_some_and(|e| Arc::ptr_eq(e, &engine));
        if !same {
            self.state.lock().is_time_shifting = false;
            if let Some(c) = &coordinator {
                c.set_time_shift_active(false).await;
            }
            return;
        }
        engine.set_rate(1.0);
        engine.seek(range.start().max(range.end() - behind));
    }

    /// 되감기 — 출처에 따라 서버 타임머신 seek 또는 로컬 타임시프트 진입.
    pub async fn seek_rewind(&self, seconds_behind: f64) {
        let (source, window, tolerance, coordinator) = {
            let s = self.state.read();
            if !matches!(
                s.stream_phase,
                StreamPhase::Playing | StreamPhase::Buffering | StreamPhase::Paused
            ) {
                return;
            }
            (
                s.rewind_source(),
                s.rewind_window_duration(),
                s.live_edge_tolerance(),
                s.coordinator.clone(),
            )
        };
        let behind = seconds_behind.max(0.0).min(window);
        match source {
            RewindSource::None => {}
            RewindSource::ServerTimeMachine => {
                self.state.lock().rewind_seek_hint =
                    (behind > tolerance).then(|| (behind, Instant::now()));
                self.seek_time_machine(seconds_behind).await;
            }
            RewindSource::LocalTimeshift => {
                let Some(coordinator) = coordinator else {
                    return;
                };
                if behind <= tolerance {
                    self.jump_to_live();
                    return;
                }
                let serial = {
                    let mut s = self.state.lock();
                    s.is_time_shifting = true;
                    s.rewind_seek_hint = Some((behind, Instant::now()));
                    s.rewind_request_serial = s.rewind_request_serial.wrapping_add(1);
                    s.rewind_request_serial
                };
                let entered = coordinator.enter_timeshift(behind).await;
                let mut s = self.state.lock();
                if !entered && serial == s.rewind_request_serial {
                    s.is_time_shifting = false;
                    s.rewind_seek_hint = None;
                }
            }
        }
    }

    /// ←/→ 한 칸.
    pub async fn step_rewind(&self, delta: f64) {
        let behind = {
            let s = self.state.read();
            if s.rewind_source() == RewindSource::None {
                return;
            }
            s.seconds_behind_live()
        };
        self.seek_rewind(behind - delta).await;
    }

    pub fn apply_rewind_settings(&self, settings: &PlayerSettings) {
        let rewind = RewindSettings::from_settings(settings);
        {
            let mut s = self.state.lock();
            if s.rewind_settings == rewind {
                return;
            }
            s.rewind_settings = rewind;
        }
        self.apply_rewind_recording();
    }

    /// 메인 포커스 여부(멀티라이브). true → false 는 유예 뒤에 기록을 끈다.
    pub fn set_rewind_recording_desired(&self, desired: Option<bool>) {
        let (old, jump) = {
            let mut s = self.state.lock();
            let old = s.rewind_recording_desired;
            if old == desired {
                return;
            }
            s.rewind_recording_desired = desired;
            (old, s.is_time_shifting)
        };
        if old == Some(true) && desired == Some(false) {
            if jump {
                self.jump_to_live();
                self.state.lock().is_time_shifting = false;
            }
            self.schedule_rewind_recording_release();
        } else {
            cancel_slot(&mut self.state.lock().rewind_release_task);
            self.apply_rewind_recording();
        }
    }

    fn schedule_rewind_recording_release(&self) {
        let me = self.clone();
        let mut s = self.state.lock();
        cancel_slot(&mut s.rewind_release_task);
        s.rewind_release_task = Some(TaskSlot::spawn(move |id| async move {
            tokio::time::sleep(Duration::from_secs_f64(REWIND_RELEASE_GRACE)).await;
            {
                let mut s = me.state.lock();
                if !slot_is(&s.rewind_release_task, id) {
                    return;
                }
                s.rewind_release_task = None;
            }
            me.apply_rewind_recording();
        }));
    }

    pub fn rewind_recording_config(&self) -> Option<TimeshiftRecordingConfig> {
        self.state.read().rewind_recording_config()
    }

    fn apply_rewind_recording(&self) {
        let (coordinator, config) = {
            let mut s = self.state.lock();
            let Some(c) = s.coordinator.clone() else {
                return;
            };
            let config = s.rewind_recording_config();
            if config.is_none() {
                cancel_slot(&mut s.rewind_window_task);
                Self::apply_timeshift_window(&mut s, 0.0);
                if s.is_time_shifting && !s.is_time_machine_available {
                    s.is_time_shifting = false;
                }
            }
            (c, config)
        };
        let me = self.clone();
        spawn_detached(async move {
            coordinator.set_timeshift_recording(config).await;
            let start = {
                let s = me.state.read();
                config.is_some()
                    && s.rewind_window_task.is_none()
                    && s.stream_phase != StreamPhase::Idle
            };
            if start {
                me.start_rewind_window_updates(coordinator);
            }
        });
    }

    pub(super) fn start_rewind_window_updates(&self, coordinator: StreamCoordinator) {
        let me = self.clone();
        let mut s = self.state.lock();
        let session = s.session_id;
        cancel_slot(&mut s.rewind_window_task);
        s.rewind_window_task = Some(TaskSlot::spawn(move |id| async move {
            loop {
                tokio::time::sleep(Duration::from_secs(2)).await;
                let duration = coordinator.timeshift_recorded_duration();
                let mut s = me.state.lock();
                if !slot_is(&s.rewind_window_task, id) || s.session_id != session {
                    return;
                }
                Self::apply_timeshift_window(&mut s, duration);
            }
        }));
    }

    pub(super) fn apply_timeshift_window(s: &mut PlayerState, recorded: f64) {
        s.timeshift_recorded_duration = recorded;
        let rewindable = recorded >= MINIMUM_REWIND_WINDOW;
        if rewindable != s.has_rewindable_recording {
            s.has_rewindable_recording = rewindable;
        }
    }
}
