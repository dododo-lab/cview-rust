//! 녹화 (`PlayerViewModel+Recording`).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use cview_core::models::player::{PlayerTool, PlayerToolStatus, PlayerToolStatusKind};

use super::{dirs_movies, format_time_interval, PlayerViewModel};
use crate::viewmodels::support::{cancel_slot, slot_is, TaskSlot};

impl PlayerViewModel {
    /// 녹화 시작. 경로를 주지 않으면 `<영상 폴더>/CView/<채널>_<시각>.ts`.
    pub async fn start_recording(&self, path: Option<PathBuf>) {
        let (engine, channel) = {
            let s = self.state.read();
            if s.is_recording {
                return;
            }
            let Some(e) = s.engine.clone() else {
                return;
            };
            (e, s.channel_name.clone())
        };
        let path = path.unwrap_or_else(|| {
            cview_player::recording::StreamRecordingService::default_recording_path(
                dirs_movies().as_deref(),
                &channel,
                chrono::Local::now(),
            )
        });
        self.state.lock().recording_path = Some(path.clone());
        match engine.start_recording(&path).await {
            Ok(()) => {
                {
                    let mut s = self.state.lock();
                    if let Some(actual) = engine.recording_destination() {
                        s.recording_path = Some(actual);
                    }
                    s.is_recording = true;
                    s.recording_duration = 0.0;
                }
                self.start_recording_timer();
            }
            Err(e) => {
                let reason = e.to_string();
                tracing::error!("녹화 시작 실패: {reason}");
                self.set_recording_status(
                    PlayerToolStatusKind::Failure,
                    format!("녹화 시작 실패 · {reason}"),
                );
            }
        }
    }

    pub async fn stop_recording(&self) {
        let engine = {
            let s = self.state.read();
            if !s.is_recording {
                return;
            }
            let Some(e) = s.engine.clone() else {
                return;
            };
            e
        };
        engine.stop_recording().await;
        let path = {
            let mut s = self.state.lock();
            s.is_recording = false;
            cancel_slot(&mut s.recording_timer);
            s.recording_path.clone()
        };
        if let Some(reason) = engine.recording_failure_reason() {
            self.set_recording_status(
                PlayerToolStatusKind::Failure,
                format!("녹화 저장 실패 · {reason}"),
            );
        } else if let Some(path) = path {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.set_recording_status(
                PlayerToolStatusKind::Success,
                format!("녹화 저장됨 · {name}"),
            );
        }
    }

    /// 엔진의 녹화 오류 이벤트.
    pub fn engine_recording_failed(&self, reason: &str) {
        let was_recording = {
            let mut s = self.state.lock();
            let was = s.is_recording;
            if was {
                s.is_recording = false;
                cancel_slot(&mut s.recording_timer);
            }
            was
        };
        let message = if was_recording {
            format!("녹화 중단됨 · {reason}")
        } else {
            format!("녹화 실패 · {reason}")
        };
        self.set_recording_status(PlayerToolStatusKind::Failure, message);
    }

    pub fn set_recording_status(&self, kind: PlayerToolStatusKind, message: String) {
        let state = self.state.clone();
        let mut s = self.state.lock();
        s.recording_status = Some(PlayerToolStatus::new(kind, message, PlayerTool::Recording));
        cancel_slot(&mut s.recording_status_reset);
        let secs = if kind == PlayerToolStatusKind::Failure {
            6
        } else {
            3
        };
        s.recording_status_reset = Some(TaskSlot::spawn(move |id| async move {
            tokio::time::sleep(Duration::from_secs(secs)).await;
            let mut s = state.lock();
            if slot_is(&s.recording_status_reset, id) {
                s.recording_status = None;
                s.recording_status_reset = None;
            }
        }));
    }

    fn start_recording_timer(&self) {
        let state = self.state.clone();
        let mut s = self.state.lock();
        cancel_slot(&mut s.recording_timer);
        let start = Instant::now();
        s.recording_timer = Some(TaskSlot::spawn(move |id| async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let mut s = state.lock();
                if !slot_is(&s.recording_timer, id) {
                    return;
                }
                s.recording_duration = start.elapsed().as_secs_f64();
            }
        }));
    }

    pub fn formatted_recording_duration(&self) -> String {
        format_time_interval(self.state.read().recording_duration)
    }
}
