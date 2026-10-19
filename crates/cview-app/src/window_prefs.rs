//! 창·시스템 연동 설정 적용 — 항상 위(SYS-17), 로그인 시 자동 실행(SYS-18), 창 위치·크기 기억(SYS-16).
//!
//! 원본: `GeneralSettings.alwaysOnTop/launchAtLogin/restoreWindowOnLaunch`, `WindowFrameAutosave`.
//! 자동 실행은 플러그인 없이 macOS LaunchAgent plist 를 쓴다(다른 OS 는 아직 지원하지 않는다).

use std::path::{Path, PathBuf};

use cview_core::models::settings::GeneralSettings;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, Runtime, WebviewWindow};

use crate::MAIN_WINDOW;

const FRAME_FILE: &str = "window-frame.json";
const LAUNCH_AGENT_LABEL: &str = "app.cview.rust.launcher";

/// 저장한 창 위치·크기(물리 픽셀).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowFrame {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl WindowFrame {
    /// 너무 작거나 말이 안 되는 값은 버린다.
    pub fn is_sane(&self) -> bool {
        self.width >= 600 && self.height >= 400 && self.width <= 20_000 && self.height <= 20_000
    }
}

pub fn load_frame(dir: &Path) -> Option<WindowFrame> {
    let raw = std::fs::read(dir.join(FRAME_FILE)).ok()?;
    serde_json::from_slice::<WindowFrame>(&raw)
        .ok()
        .filter(WindowFrame::is_sane)
}

pub fn save_frame<R: Runtime>(dir: &Path, window: &WebviewWindow<R>) {
    let (Ok(pos), Ok(size)) = (window.outer_position(), window.inner_size()) else {
        return;
    };
    if window.is_minimized().unwrap_or(false) || window.is_fullscreen().unwrap_or(false) {
        return;
    }
    let frame = WindowFrame {
        x: pos.x,
        y: pos.y,
        width: size.width,
        height: size.height,
    };
    if frame.is_sane() {
        if let Ok(json) = serde_json::to_vec(&frame) {
            let _ = std::fs::write(dir.join(FRAME_FILE), json);
        }
    }
}

/// 시작할 때 — 저장한 창 위치를 되살리고(설정이 켜져 있으면) 항상 위를 맞춘다.
pub fn apply_on_launch<R: Runtime>(app: &AppHandle<R>, dir: &Path, general: &GeneralSettings) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    if general.restore_window_on_launch {
        if let Some(f) = load_frame(dir) {
            let _ = window.set_size(PhysicalSize::new(f.width, f.height));
            let _ = window.set_position(PhysicalPosition::new(f.x, f.y));
        }
    }
    let _ = window.set_always_on_top(general.always_on_top);
}

/// 설정을 바꿨을 때 — 항상 위·자동 실행을 즉시 반영한다.
pub fn apply_general<R: Runtime>(app: &AppHandle<R>, general: &GeneralSettings) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.set_always_on_top(general.always_on_top);
    }
    if let Err(e) = set_launch_at_login(general.launch_at_login) {
        tracing::warn!("로그인 시 자동 실행 설정 실패: {e}");
    }
}

fn launch_agent_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| {
        PathBuf::from(h)
            .join("Library/LaunchAgents")
            .join(format!("{LAUNCH_AGENT_LABEL}.plist"))
    })
}

/// LaunchAgent plist 내용 — 실행 파일 경로를 그대로 쓴다.
pub fn launch_agent_plist(exe: &Path) -> String {
    let exe = exe
        .to_string_lossy()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LAUNCH_AGENT_LABEL}</string>
  <key>ProgramArguments</key><array><string>{exe}</string></array>
  <key>RunAtLoad</key><true/>
  <key>ProcessType</key><string>Interactive</string>
</dict>
</plist>
"#
    )
}

/// 로그인 시 자동 실행 켜기·끄기(macOS LaunchAgent).
pub fn set_launch_at_login(on: bool) -> std::io::Result<()> {
    if !cfg!(target_os = "macos") {
        return Ok(());
    }
    let Some(path) = launch_agent_path() else {
        return Ok(());
    };
    if on {
        let exe = std::env::current_exe()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let plist = launch_agent_plist(&exe);
        if std::fs::read_to_string(&path).ok().as_deref() != Some(plist.as_str()) {
            std::fs::write(&path, plist)?;
        }
    } else if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_round_trip_and_sanity() {
        let dir = std::env::temp_dir().join(format!("cview-frame-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(load_frame(&dir).is_none());
        let f = WindowFrame {
            x: 10,
            y: 20,
            width: 1400,
            height: 900,
        };
        std::fs::write(dir.join(FRAME_FILE), serde_json::to_vec(&f).unwrap()).unwrap();
        assert_eq!(load_frame(&dir), Some(f));
        let tiny = WindowFrame { width: 10, ..f };
        std::fs::write(dir.join(FRAME_FILE), serde_json::to_vec(&tiny).unwrap()).unwrap();
        assert!(load_frame(&dir).is_none(), "작은 창은 버린다");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plist_escapes_path() {
        let p = launch_agent_plist(Path::new("/Apps/A&B <x>/CView"));
        assert!(p.contains("/Apps/A&amp;B &lt;x&gt;/CView"));
        assert!(p.contains(LAUNCH_AGENT_LABEL));
    }
}
