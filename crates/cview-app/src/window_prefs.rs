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
    /// 최대화한 채였는가 — 위치·크기는 최대화 전 보통 창 값이다(옛 파일엔 없음 = false).
    #[serde(default)]
    pub maximized: bool,
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

/// 지금 창 위치·크기. 최소화·전체 화면이거나 말이 안 되는 값이면 `None`.
pub fn current_frame<R: Runtime>(window: &WebviewWindow<R>) -> Option<WindowFrame> {
    let (Ok(pos), Ok(size)) = (window.outer_position(), window.inner_size()) else {
        return None;
    };
    if window.is_minimized().unwrap_or(false) || window.is_fullscreen().unwrap_or(false) {
        return None;
    }
    let frame = WindowFrame {
        x: pos.x,
        y: pos.y,
        width: size.width,
        height: size.height,
        maximized: window.is_maximized().unwrap_or(false),
    };
    frame.is_sane().then_some(frame)
}

/// 최대화 중 값은 위치·크기를 앞 보통 창 값으로 둔다 — 예전엔 최대화한 채 끄면 화면 크기를 보통 창 크기로 저장해,
/// 다음 실행부터 원래 크기로 돌아갈 수 없었다. 앞 값이 없으면(처음부터 최대화) 지금 값을 쓴다. 순수 함수.
pub fn merge_frame(normal: Option<WindowFrame>, now: WindowFrame) -> WindowFrame {
    match normal {
        Some(n) if now.maximized => WindowFrame {
            maximized: true,
            ..n
        },
        _ => now,
    }
}

pub fn write_frame(dir: &Path, frame: &WindowFrame) {
    if let Ok(json) = serde_json::to_vec(frame) {
        let _ = cview_persistence::json::write_atomic(&dir.join(FRAME_FILE), &json);
    }
}

/// 창을 끄는 동안 위치 저장을 미룬다.
///
/// 이동·크기 조절 이벤트는 끄는 동안 초당 수십 번 메인 스레드로 온다. 매번 원자 쓰기(macOS `F_FULLFSYNC`, 한 번
/// 약 5ms)를 하면 끄는 내내 메인 스레드가 막혔다. 마지막 이벤트 뒤 [`Self::QUIET`] 동안 조용하면 한 번만 백그라운드
/// 스레드에서 쓰고, 같은 값이면 다시 쓰지 않는다.
#[derive(Clone)]
pub struct FrameSaver {
    dir: PathBuf,
    state: std::sync::Arc<std::sync::Mutex<SaverState>>,
}

#[derive(Default)]
struct SaverState {
    pending: Option<WindowFrame>,
    written: Option<WindowFrame>,
    /// 마지막으로 본 보통(최대화 아닌) 창 값 — 시작 때 파일 값에서 출발한다.
    normal: Option<WindowFrame>,
    generation: u64,
}

impl SaverState {
    fn note(&mut self, frame: WindowFrame) -> WindowFrame {
        let merged = merge_frame(self.normal, frame);
        if !frame.maximized {
            self.normal = Some(frame);
        }
        merged
    }
}

impl FrameSaver {
    pub const QUIET: std::time::Duration = std::time::Duration::from_millis(500);

    pub fn new(dir: PathBuf) -> Self {
        let normal = load_frame(&dir).map(|f| WindowFrame {
            maximized: false,
            ..f
        });
        Self {
            dir,
            state: std::sync::Arc::new(std::sync::Mutex::new(SaverState {
                normal,
                ..SaverState::default()
            })),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, SaverState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 이동·크기 조절 — 값만 적어 두고 조용해지면 쓴다.
    pub fn schedule(&self, frame: Option<WindowFrame>) {
        let Some(frame) = frame else {
            return;
        };
        let generation = self.record(frame);
        let this = self.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Self::QUIET).await;
            // 끄는 동안 새 이동이 왔으면(드래그 중 대부분) 블로킹 풀까지 가지 않는다 — 쓰기는 마지막 이벤트가 한다.
            if this.lock().generation != generation {
                return;
            }
            let _ = tauri::async_runtime::spawn_blocking(move || this.flush_if(generation)).await;
        });
    }

    /// 밀린 값을 바꾸고 새 세대를 돌려준다.
    fn record(&self, frame: WindowFrame) -> u64 {
        let mut st = self.lock();
        st.pending = Some(st.note(frame));
        st.generation += 1;
        st.generation
    }

    /// 닫을 때 — 기다리지 않고 지금 값을 쓴다.
    pub fn save_now(&self, frame: Option<WindowFrame>) {
        let generation = {
            let mut st = self.lock();
            if let Some(frame) = frame {
                st.pending = Some(st.note(frame));
            }
            st.generation
        }; // 잠금을 놓은 뒤 쓴다(flush_if 가 다시 잠근다)
        self.flush_if(generation);
    }

    /// 그사이 새 이벤트가 없었으면(세대가 같으면) 밀린 값을 쓴다. 쓴 값과 같으면 건너뛴다.
    fn flush_if(&self, generation: u64) -> bool {
        let frame = {
            let mut st = self.lock();
            if st.generation != generation {
                return false;
            }
            let Some(frame) = st.pending.take() else {
                return false;
            };
            if st.written == Some(frame) {
                return false;
            }
            st.written = Some(frame);
            frame
        };
        write_frame(&self.dir, &frame);
        true
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
            if f.maximized {
                let _ = window.maximize();
            }
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
            maximized: false,
        };
        std::fs::write(dir.join(FRAME_FILE), serde_json::to_vec(&f).unwrap()).unwrap();
        assert_eq!(load_frame(&dir), Some(f));
        let tiny = WindowFrame { width: 10, ..f };
        std::fs::write(dir.join(FRAME_FILE), serde_json::to_vec(&tiny).unwrap()).unwrap();
        assert!(load_frame(&dir).is_none(), "작은 창은 버린다");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 끄는 동안 쌓인 이벤트는 마지막 값 하나만, 같은 값은 다시 쓰지 않는다. 닫을 때는 바로 쓴다.
    #[test]
    fn frame_saver_writes_only_the_last_quiet_value() {
        let dir = std::env::temp_dir().join(format!("cview-saver-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let saver = FrameSaver::new(dir.clone());
        let at = |x| WindowFrame {
            x,
            y: 0,
            width: 1400,
            height: 900,
            maximized: false,
        };
        let first = saver.record(at(1));
        let last = saver.record(at(2));
        assert!(
            !saver.flush_if(first),
            "뒤에 이벤트가 더 왔으면 쓰지 않는다"
        );
        assert!(load_frame(&dir).is_none());
        assert!(saver.flush_if(last));
        assert_eq!(load_frame(&dir), Some(at(2)));
        let same = saver.record(at(2));
        assert!(!saver.flush_if(same), "같은 값은 다시 쓰지 않는다");
        saver.record(at(3));
        saver.save_now(Some(at(4)));
        assert_eq!(load_frame(&dir), Some(at(4)), "닫을 때는 기다리지 않는다");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 끄는 동안 이벤트가 이어지면 조용해진 뒤 백그라운드에서 마지막 값 하나만 쓴다.
    #[test]
    fn frame_saver_schedule_writes_after_quiet() {
        let dir = std::env::temp_dir().join(format!("cview-saver-q-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let saver = FrameSaver::new(dir.clone());
        for x in 0..20 {
            saver.schedule(Some(WindowFrame {
                x,
                y: 0,
                width: 1400,
                height: 900,
                maximized: false,
            }));
        }
        assert!(
            load_frame(&dir).is_none(),
            "이벤트를 받는 자리에서는 쓰지 않는다"
        );
        std::thread::sleep(FrameSaver::QUIET + std::time::Duration::from_millis(700));
        assert_eq!(load_frame(&dir).map(|f| f.x), Some(19));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 최대화한 채 끄면 보통 창 크기를 지키고 최대화만 표시한다(옛 파일은 maximized 없이도 읽힌다).
    #[test]
    fn maximized_frame_keeps_the_normal_size() {
        let dir = std::env::temp_dir().join(format!("cview-saver-max-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(FRAME_FILE),
            br#"{"x":100,"y":80,"width":1200,"height":800}"#,
        )
        .unwrap();
        assert_eq!(load_frame(&dir).map(|f| f.maximized), Some(false));
        let saver = FrameSaver::new(dir.clone());
        let full = WindowFrame {
            x: 0,
            y: 25,
            width: 3024,
            height: 1890,
            maximized: true,
        };
        saver.save_now(Some(full));
        let saved = load_frame(&dir).unwrap();
        assert_eq!(
            (saved.x, saved.y, saved.width, saved.height, saved.maximized),
            (100, 80, 1200, 800, true)
        );
        // 되돌린 뒤 옮긴 값은 그대로.
        let moved = WindowFrame {
            x: 300,
            maximized: false,
            ..saved
        };
        saver.save_now(Some(moved));
        assert_eq!(load_frame(&dir), Some(moved));
        assert_eq!(merge_frame(None, full), full, "앞 값이 없으면 지금 값");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plist_escapes_path() {
        let p = launch_agent_plist(Path::new("/Apps/A&B <x>/CView"));
        assert!(p.contains("/Apps/A&amp;B &lt;x&gt;/CView"));
        assert!(p.contains(LAUNCH_AGENT_LABEL));
    }
}
