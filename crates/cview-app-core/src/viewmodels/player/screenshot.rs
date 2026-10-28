//! 스크린샷 (`PlayerViewModel+Screenshot`).

use std::time::Duration;

use chrono::Utc;
use cview_core::models::player::{PlayerTool, PlayerToolStatus, PlayerToolStatusKind};
use cview_core::protocols::PlayerEngineCapabilities;

use super::{helpers, screenshot_file_base_name, write_screenshot, PlayerViewModel};
use crate::viewmodels::support::{cancel_slot, slot_is, TaskSlot};

impl PlayerViewModel {
    /// 현재 프레임을 캡처해 설정 폴더에 저장한다. 결과는 `screenshot_status` 로 알린다.
    pub async fn take_screenshot(&self) {
        let (engine, format, dir, base) = {
            let s = self.state.read();
            let Some(engine) = s.engine.clone() else {
                drop(s);
                self.set_screenshot_status(
                    PlayerToolStatusKind::Failure,
                    "재생 중인 스트림이 없습니다".into(),
                );
                return;
            };
            if !engine
                .capabilities()
                .contains(PlayerEngineCapabilities::SCREENSHOT)
            {
                let name = s.current_engine_type.display_name();
                drop(s);
                self.set_screenshot_status(
                    PlayerToolStatusKind::Failure,
                    format!("{name} 엔진은 화면 캡처를 지원하지 않습니다"),
                );
                return;
            }
            (
                engine,
                s.screenshot_format,
                helpers::expand_tilde(&s.screenshot_save_path),
                screenshot_file_base_name(&s.channel_name, Utc::now().timestamp()),
            )
        };
        let result = match engine.capture_screenshot(format).await {
            Ok(frame) => {
                let written = tokio::task::spawn_blocking({
                    let frame = frame.clone();
                    move || write_screenshot(&frame, &dir, &base)
                })
                .await;
                match written {
                    Ok(Ok(dest)) => Ok((frame, dest)),
                    Ok(Err(e)) => Err(e.to_string()),
                    Err(e) => Err(e.to_string()),
                }
            }
            Err(e) => Err(e.to_string()),
        };
        match result {
            Ok((frame, dest)) => {
                let name = dest
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.set_screenshot_status(
                    PlayerToolStatusKind::Success,
                    format!(
                        "저장됨 · {}×{} · {name}",
                        frame.pixel_width, frame.pixel_height
                    ),
                );
            }
            Err(reason) => {
                tracing::error!("스크린샷 실패: {reason}");
                self.set_screenshot_status(PlayerToolStatusKind::Failure, reason);
            }
        }
    }

    pub fn set_screenshot_status(&self, kind: PlayerToolStatusKind, message: String) {
        let state = self.state.clone();
        let mut s = self.state.lock();
        s.screenshot_status = Some(PlayerToolStatus::new(kind, message, PlayerTool::Screenshot));
        cancel_slot(&mut s.screenshot_status_reset);
        let secs = if kind == PlayerToolStatusKind::Failure {
            6
        } else {
            3
        };
        s.screenshot_status_reset = Some(TaskSlot::spawn(move |id| async move {
            tokio::time::sleep(Duration::from_secs(secs)).await;
            let mut s = state.lock();
            if slot_is(&s.screenshot_status_reset, id) {
                s.screenshot_status = None;
                s.screenshot_status_reset = None;
            }
        }));
    }
}
