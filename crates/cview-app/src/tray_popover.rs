//! macOS 메뉴 막대 드롭다운 팝오버 창(`menubar.html`)과 상태·제어 브리지.
//!
//! - macOS 에서 메뉴 막대 아이콘을 **왼쪽 클릭**하면 아이콘 바로 아래에 둥근 카드형 팝오버 창을 띄우고,
//!   바깥을 누르거나(초점 해제) `Escape`·항목 실행 시 닫는다. **오른쪽 클릭(⌃클릭)**은 기존 네이티브 메뉴를 그대로 띄운다.
//! - 창은 첫 클릭 때 한 번 만들고(`visible(false)` 로 준비해 위치 지정 후 표시), 닫을 때는 `hide()` 로 숨겨 다음 클릭이
//!   즉시 열리게 한다. 메모리 압박(`memory_pressure.rs`)이 오면 숨은 팝오버 창은 닫아 웹뷰 메모리를 돌려준다.
//! - 메인 창이 메뉴 막대로 숨겨져 있거나 뒤쪽에 있어도 재생·일시정지·음소거·볼륨·PiP 제어가 동작하도록
//!   메인 창 전용 이벤트([`TRAY_CONTROL_EVENT`])로 전달한다(`menu://action` 은 초점 있는 창만 실행하기 때문).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, Position, Rect, Runtime, Size, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

use crate::commands::{CmdResult, CommandError};
use crate::tray::TrayFrontState;

/// 팝오버 웹뷰 창 라벨(`capabilities/default.json` 에 등록).
pub const TRAY_POPOVER_WINDOW: &str = "menubar";
/// 팝오버 창으로 보내는 상태 갱신 이벤트.
pub const TRAY_STATE_EVENT: &str = "tray://state";
/// 메인 창으로 보내는 직접 제어 이벤트(초점 유무와 무관하게 메인 창이 처리).
pub const TRAY_CONTROL_EVENT: &str = "tray://control";

/// 논리 크기(pt) — 바깥 그림자 여백(좌우·아래 10px, 위 화살표 8px)을 포함한 크기.
pub const POPOVER_WIDTH: f64 = 384.0;
pub const POPOVER_HEIGHT: f64 = 584.0;

/// 아이콘을 다시 눌러 닫을 때 마우스 다운이 먼저 `Focused(false)` 를 일으켜 곧장 다시 열리는 현상을 막는 시간창.
const REOPEN_GUARD: Duration = Duration::from_millis(240);

/// 모니터 영역(물리 픽셀, 좌상단 원점).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}

/// 상태 막대 아이콘 영역(물리 픽셀, 좌상단 원점).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IconBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// 계산된 팝오버 창 위치(물리 픽셀)와 창 왼쪽 끝에서 아이콘 중심까지의 논리 X 좌표(화살표 위치).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PopoverPlacement {
    pub x: i32,
    pub y: i32,
    pub arrow_x: f64,
}

/// 아이콘 영역과 모니터 목록으로 팝오버가 놓일 물리 좌표와 위쪽 포인터 위치를 계산한다. 순수 함수.
pub fn compute_placement(icon: IconBounds, monitors: &[MonitorBounds]) -> PopoverPlacement {
    let cx = icon.x + icon.width * 0.5;
    let cy = icon.y + icon.height * 0.5;
    let fallback = MonitorBounds {
        x: 0,
        y: 0,
        width: 1440,
        height: 900,
        scale: 1.0,
    };
    let mon = monitors
        .iter()
        .copied()
        .find(|m| {
            let mx = m.x as f64;
            let my = m.y as f64;
            cx >= mx && cx < mx + m.width as f64 && cy >= my && cy < my + m.height as f64
        })
        .or_else(|| {
            monitors.iter().copied().min_by(|a, b| {
                let da = distance_sq_to_monitor(cx, cy, *a);
                let db = distance_sq_to_monitor(cx, cy, *b);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
        })
        .unwrap_or(fallback);

    let scale = if mon.scale.is_finite() && mon.scale > 0.0 {
        mon.scale
    } else {
        1.0
    };
    let pop_w = POPOVER_WIDTH * scale;
    let pop_h = POPOVER_HEIGHT * scale;
    let margin = 8.0 * scale;
    let gap = 2.0 * scale;

    let min_x = mon.x as f64 + margin;
    let max_x = (mon.x as f64 + mon.width as f64 - pop_w - margin).max(min_x);
    let x = (cx - pop_w * 0.5).clamp(min_x, max_x);

    let min_y = mon.y as f64 + 22.0 * scale;
    let max_y = (mon.y as f64 + mon.height as f64 - pop_h - margin).max(min_y);
    let y = (icon.y + icon.height + gap).clamp(min_y, max_y);

    let arrow_x = ((cx - x) / scale).clamp(28.0, POPOVER_WIDTH - 28.0);
    PopoverPlacement {
        x: x.round() as i32,
        y: y.round() as i32,
        arrow_x: (arrow_x * 10.0).round() / 10.0,
    }
}

fn distance_sq_to_monitor(px: f64, py: f64, m: MonitorBounds) -> f64 {
    let mx0 = m.x as f64;
    let my0 = m.y as f64;
    let mx1 = mx0 + m.width as f64;
    let my1 = my0 + m.height as f64;
    let dx = if px < mx0 {
        mx0 - px
    } else if px > mx1 {
        px - mx1
    } else {
        0.0
    };
    let dy = if py < my0 {
        my0 - py
    } else if py > my1 {
        py - my1
    } else {
        0.0
    };
    dx * dx + dy * dy
}

/// 초점 해제 직후 재열림을 막는 순수 상태 기계.
#[derive(Debug)]
pub struct PopoverGate {
    last_blur: Option<Instant>,
    pub arrow_x: f64,
}

impl Default for PopoverGate {
    fn default() -> Self {
        Self {
            last_blur: None,
            arrow_x: POPOVER_WIDTH * 0.5,
        }
    }
}

impl PopoverGate {
    pub fn note_blur(&mut self, now: Instant) {
        self.last_blur = Some(now);
    }

    /// 왼쪽 클릭 시 열어야 하면 `true`, 닫거나 닫힌 채 유지해야 하면 `false`.
    pub fn decide_toggle(&mut self, currently_visible: bool, now: Instant) -> bool {
        let recent_blur = self
            .last_blur
            .take()
            .is_some_and(|t| now.saturating_duration_since(t) < REOPEN_GUARD);
        !currently_visible && !recent_blur
    }
}

static GATE: Mutex<Option<PopoverGate>> = Mutex::new(None);

fn with_gate<T>(f: impl FnOnce(&mut PopoverGate) -> T) -> T {
    let mut g = GATE.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(PopoverGate::default))
}

/// 팝오버 화면에 건네는 상태 스냅샷.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayPopoverSnapshot {
    pub open: bool,
    pub logged_in: bool,
    pub window_hidden: bool,
    pub arrow_x: f64,
    pub front: TrayFrontState,
}

/// 팝오버에서 보낼 수 있는 동작.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum TrayControlAction {
    PlayPause,
    Mute,
    Volume {
        value: u8,
    },
    Pip,
    ToggleWindow,
    ShowMain,
    OpenLive {
        #[serde(rename = "channelId")]
        channel_id: String,
    },
    AddMultilive {
        #[serde(rename = "channelId")]
        channel_id: String,
    },
    OpenMultilive,
    OpenChatWindow {
        #[serde(rename = "channelId")]
        channel_id: Option<String>,
    },
    OpenRoute {
        route: String,
        #[serde(default)]
        params: Value,
    },
    QueueMulti {
        #[serde(rename = "channelIds")]
        channel_ids: Vec<String>,
    },
    CheckUpdate,
    Login,
    Quit,
}

impl TrayControlAction {
    /// 실행할 때 팝오버를 닫아야 하는 동작인가 — 재생 제어·창 숨김 토글(숨길 때)은 열린 채로 둔다. 순수 함수.
    pub fn closes_popover(&self, main_currently_hidden: bool) -> bool {
        match self {
            Self::PlayPause | Self::Mute | Self::Volume { .. } | Self::Pip => false,
            Self::ToggleWindow => main_currently_hidden,
            _ => true,
        }
    }
}

fn snapshot<R: Runtime>(app: &AppHandle<R>) -> TrayPopoverSnapshot {
    let open = app
        .get_webview_window(TRAY_POPOVER_WINDOW)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    let (logged_in, window_hidden, front) = crate::tray::snapshot_state(app);
    let arrow_x = with_gate(|g| g.arrow_x);
    TrayPopoverSnapshot {
        open,
        logged_in,
        window_hidden,
        arrow_x,
        front,
    }
}

/// 팝오버 창이 만들어져 있으면 최신 상태를 보낸다(없으면 아무것도 하지 않는다).
pub fn emit_state<R: Runtime>(app: &AppHandle<R>) {
    if app.get_webview_window(TRAY_POPOVER_WINDOW).is_none() {
        return;
    }
    let _ = app.emit_to(TRAY_POPOVER_WINDOW, TRAY_STATE_EVENT, snapshot(app));
}

/// 팝오버 숨기기.
pub fn hide<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window(TRAY_POPOVER_WINDOW) {
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
            emit_state(app);
        }
    }
}

/// 메모리 압박 시 숨겨진 팝오버 창을 닫아 웹뷰 프로세스를 정리한다(다음 클릭 때 다시 만든다).
pub fn release_if_hidden<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window(TRAY_POPOVER_WINDOW) {
        if !w.is_visible().unwrap_or(false) {
            let _ = w.close();
        }
    }
}

fn icon_bounds_from_rect(rect: Rect, click: PhysicalPosition<f64>) -> IconBounds {
    let (rx, ry) = match rect.position {
        Position::Physical(p) => (p.x as f64, p.y as f64),
        Position::Logical(p) => (p.x, p.y),
    };
    let (rw, rh) = match rect.size {
        Size::Physical(s) => (s.width as f64, s.height as f64),
        Size::Logical(s) => (s.width, s.height),
    };
    if rw > 0.0 && rh > 0.0 {
        IconBounds {
            x: rx,
            y: ry,
            width: rw,
            height: rh,
        }
    } else {
        IconBounds {
            x: click.x - 14.0,
            y: click.y - 12.0,
            width: 28.0,
            height: 24.0,
        }
    }
}

fn collect_monitors<R: Runtime>(app: &AppHandle<R>) -> Vec<MonitorBounds> {
    app.available_monitors()
        .ok()
        .unwrap_or_default()
        .into_iter()
        .map(|m| MonitorBounds {
            x: m.position().x,
            y: m.position().y,
            width: m.size().width,
            height: m.size().height,
            scale: m.scale_factor(),
        })
        .collect()
}

fn ensure_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<WebviewWindow<R>> {
    if let Some(existing) = app.get_webview_window(TRAY_POPOVER_WINDOW) {
        return Ok(existing);
    }
    let window = WebviewWindowBuilder::new(
        app,
        TRAY_POPOVER_WINDOW,
        WebviewUrl::App("menubar.html".into()),
    )
    .title("CView 메뉴 막대")
    .inner_size(POPOVER_WIDTH, POPOVER_HEIGHT)
    .resizable(false)
    .minimizable(false)
    .maximizable(false)
    .closable(false)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .visible(false)
    .build()?;

    if let Some(services) = app.try_state::<crate::state::AppServices>() {
        let theme = match services.settings.appearance().theme {
            cview_core::models::settings::AppTheme::System => None,
            cview_core::models::settings::AppTheme::Light => Some(tauri::Theme::Light),
            cview_core::models::settings::AppTheme::Dark => Some(tauri::Theme::Dark),
        };
        let _ = window.set_theme(theme);
    }

    #[cfg(target_os = "macos")]
    configure_macos_panel(&window);

    let w = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::Focused(false) = event {
            with_gate(|g| g.note_blur(Instant::now()));
            let _ = w.hide();
            emit_state(w.app_handle());
        }
    });

    Ok(window)
}

#[cfg(target_os = "macos")]
fn configure_macos_panel<R: Runtime>(window: &WebviewWindow<R>) {
    use objc2_app_kit::{NSStatusWindowLevel, NSWindow, NSWindowCollectionBehavior};

    let Ok(ptr) = window.ns_window() else {
        return;
    };
    if ptr.is_null() {
        return;
    }
    let addr = ptr as usize;
    let _ = window.run_on_main_thread(move || {
        // SAFETY: tao 가 만든 살아 있는 팝오버 NSWindow 이며 메인 스레드에서 설정한다.
        let ns_window = unsafe { &*(addr as *const NSWindow) };
        ns_window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Transient
                | NSWindowCollectionBehavior::IgnoresCycle
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::FullScreenDisallowsTiling,
        );
        ns_window.setLevel(NSStatusWindowLevel);
    });
}

/// (macOS) 상태 막대 아이콘 왼쪽 클릭 — 열려 있으면 닫고, 닫혀 있으면 아이콘 아래에 맞춰 띄운다.
pub fn toggle_from_tray<R: Runtime>(app: &AppHandle<R>, rect: Rect, click: PhysicalPosition<f64>) {
    let currently_visible = app
        .get_webview_window(TRAY_POPOVER_WINDOW)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    let should_open = with_gate(|g| g.decide_toggle(currently_visible, Instant::now()));
    if !should_open {
        hide(app);
        return;
    }

    let placement = compute_placement(icon_bounds_from_rect(rect, click), &collect_monitors(app));
    with_gate(|g| g.arrow_x = placement.arrow_x);

    let window = match ensure_window(app) {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!("메뉴 막대 팝오버 창을 열지 못했습니다: {e}");
            crate::tray::show_main(app);
            return;
        }
    };
    let _ = window.set_position(PhysicalPosition::new(placement.x, placement.y));
    let _ = window.show();
    let _ = window.set_focus();
    crate::perf_log::emit("tray.popover_open", json!({}));
    emit_state(app);
}

/// 팝오버 첫 로드·재열림 시 현재 상태를 읽는다.
#[tauri::command]
pub fn tray_get_state(app: AppHandle) -> TrayPopoverSnapshot {
    snapshot(&app)
}

/// 팝오버 안에서 `Escape`·닫기 동작으로 스스로 숨긴다.
#[tauri::command]
pub fn tray_popover_close(app: AppHandle) {
    hide(&app);
}

/// 메인 창에 직접 제어 신호를 보낸다(초점과 무관하게 메인 창이 처리).
pub(crate) fn send_main_control<R: Runtime>(app: &AppHandle<R>, payload: Value) {
    let _ = app.emit_to(crate::MAIN_WINDOW, TRAY_CONTROL_EVENT, payload);
}

/// `cview://` URL 쿼리에 넣을 채널 ID 를 검증한다(쿼리 구분자·제어 문자 주입 차단).
pub fn validate_channel_id(raw: &str) -> CmdResult<&str> {
    let id = raw.trim();
    if id.is_empty() || id.len() > 64 {
        return Err(CommandError("유효하지 않은 채널 ID 입니다".into()));
    }
    if !id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(CommandError(
            "채널 ID 에 허용되지 않는 문자가 있습니다".into(),
        ));
    }
    Ok(id)
}

/// 팝오버 UI 의 모든 제어 명령 진입점.
#[tauri::command]
pub async fn tray_control(app: AppHandle, action: TrayControlAction) -> CmdResult<()> {
    let main_hidden = crate::tray::is_main_hidden(&app);
    if action.closes_popover(main_hidden) {
        hide(&app);
    }
    match action {
        TrayControlAction::PlayPause => {
            send_main_control(&app, json!({ "kind": "play-pause" }));
        }
        TrayControlAction::Mute => {
            send_main_control(&app, json!({ "kind": "mute" }));
        }
        TrayControlAction::Volume { value } => {
            send_main_control(&app, json!({ "kind": "volume", "value": value.min(100) }));
        }
        TrayControlAction::Pip => {
            send_main_control(&app, json!({ "kind": "pip" }));
        }
        TrayControlAction::ToggleWindow => {
            if main_hidden {
                crate::tray::show_main(&app);
            } else {
                crate::tray::hide_main(&app);
            }
        }
        TrayControlAction::ShowMain => {
            crate::tray::show_main(&app);
        }
        TrayControlAction::OpenLive { channel_id } => {
            let id = validate_channel_id(&channel_id)?;
            crate::system::handle_urls(&app, [format!("cview://live?channelId={id}")]);
        }
        TrayControlAction::AddMultilive { channel_id } => {
            let id = validate_channel_id(&channel_id)?;
            crate::system::handle_urls(&app, [format!("cview://live?channelId={id}&mode=multi")]);
        }
        TrayControlAction::OpenMultilive => {
            crate::system::handle_urls(&app, ["cview://multilive".to_string()]);
        }
        TrayControlAction::OpenChatWindow { channel_id } => {
            let mut params = std::collections::HashMap::new();
            if let Some(raw) = channel_id.filter(|s| !s.trim().is_empty()) {
                let id = validate_channel_id(&raw)?;
                params.insert("channelId".into(), id.to_string());
            }
            let arg = if params.is_empty() {
                None
            } else {
                Some(params)
            };
            crate::windows::window_open(app, "chat".into(), arg).await?;
        }
        TrayControlAction::OpenRoute { route, params } => {
            crate::tray::show_main(&app);
            send_main_control(
                &app,
                json!({ "kind": "navigate", "route": route, "params": params }),
            );
        }
        TrayControlAction::QueueMulti { channel_ids } => {
            let mut validated = Vec::with_capacity(channel_ids.len());
            for raw in &channel_ids {
                validated.push(validate_channel_id(raw)?.to_string());
            }
            crate::tray::show_main(&app);
            send_main_control(
                &app,
                json!({ "kind": "queue-multi", "channelIds": validated }),
            );
        }
        TrayControlAction::CheckUpdate => {
            crate::tray::show_main(&app);
            send_main_control(&app, json!({ "kind": "check-update" }));
        }
        TrayControlAction::Login => {
            crate::system::handle_urls(&app, ["cview://login".to_string()]);
        }
        TrayControlAction::Quit => {
            crate::tray::request_quit(&app);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centers_below_icon_and_clamps_to_screen_edges() {
        let retina = [MonitorBounds {
            x: 0,
            y: 0,
            width: 3024,
            height: 1964,
            scale: 2.0,
        }];
        // 화면 가운데쯤 있는 아이콘 — 정확히 아래 중앙에 놓이고 화살표도 가운데다.
        let mid = compute_placement(
            IconBounds {
                x: 2000.0,
                y: 0.0,
                width: 60.0,
                height: 48.0,
            },
            &retina,
        );
        // pop_w = 384 * 2 = 768, 중심 2030 → x = 2030 - 384 = 1646
        assert_eq!(mid.x, 1646);
        assert_eq!(mid.y, 52);
        assert!((mid.arrow_x - POPOVER_WIDTH * 0.5).abs() < 0.2);

        // 오른쪽 끝(시계·제어 센터 옆) 아이콘 — 창은 화면 오른쪽 여백 안으로 밀리고 화살표는 오른쪽 아이콘을 가리킨다.
        let right = compute_placement(
            IconBounds {
                x: 2960.0,
                y: 0.0,
                width: 56.0,
                height: 48.0,
            },
            &retina,
        );
        let max_x = 3024 - (POPOVER_WIDTH as i32 * 2) - 16;
        assert_eq!(right.x, max_x);
        assert!(right.arrow_x > POPOVER_WIDTH - 40.0);
        assert!(right.arrow_x <= POPOVER_WIDTH - 28.0);
    }

    #[test]
    fn picks_secondary_monitor_with_negative_coordinates() {
        let monitors = [
            MonitorBounds {
                x: 0,
                y: 0,
                width: 2560,
                height: 1440,
                scale: 2.0,
            },
            MonitorBounds {
                x: -1920,
                y: 120,
                width: 1920,
                height: 1080,
                scale: 1.0,
            },
        ];
        let p = compute_placement(
            IconBounds {
                x: -200.0,
                y: 120.0,
                width: 32.0,
                height: 24.0,
            },
            &monitors,
        );
        // 왼쪽 보조 모니터(-1920..0) 안에 머문다.
        assert!(p.x >= -1920 + 8);
        assert!(p.x + POPOVER_WIDTH as i32 <= -8);
        assert_eq!(p.y, 146);
    }

    #[test]
    fn toggle_gate_prevents_blur_reopen_race() {
        let mut gate = PopoverGate::default();
        let t0 = Instant::now();
        assert!(gate.decide_toggle(false, t0), "닫혀 있으면 연다");
        assert!(!gate.decide_toggle(true, t0), "열려 있으면 닫는다");

        // 아이콘을 다시 누르는 순간 마우스 다운으로 Focused(false)가 먼저 온 경우
        gate.note_blur(t0);
        assert!(
            !gate.decide_toggle(false, t0 + Duration::from_millis(60)),
            "방금 초점 해제로 닫혔으면 같은 클릭에서 다시 열지 않는다"
        );
        // 그 다음 클릭은 정상적으로 연다.
        assert!(gate.decide_toggle(false, t0 + Duration::from_millis(80)));

        // 바깥을 눌러 닫은 뒤 시간이 충분히 지난 뒤의 클릭은 연다.
        gate.note_blur(t0);
        assert!(gate.decide_toggle(false, t0 + Duration::from_millis(400)));
    }

    #[test]
    fn playback_controls_keep_popover_open() {
        assert!(!TrayControlAction::PlayPause.closes_popover(true));
        assert!(!TrayControlAction::Mute.closes_popover(false));
        assert!(!TrayControlAction::Volume { value: 50 }.closes_popover(true));
        assert!(!TrayControlAction::Pip.closes_popover(false));
        // 메인 창 숨기기는 팝오버를 남기고, 숨겨진 메인 창 열기는 팝오버를 닫고 창을 보여 준다.
        assert!(!TrayControlAction::ToggleWindow.closes_popover(false));
        assert!(TrayControlAction::ToggleWindow.closes_popover(true));
        assert!(TrayControlAction::OpenMultilive.closes_popover(false));
    }

    #[test]
    fn channel_id_validation_blocks_query_injection_and_accepts_valid_ids() {
        assert_eq!(
            validate_channel_id("  0dad8baf12a4b9c0_ch-1 ").unwrap(),
            "0dad8baf12a4b9c0_ch-1"
        );
        for bad in [
            "",
            "   ",
            "ch1&mode=multi",
            "ch1?foo=bar",
            "ch1#frag",
            "../etc",
            "ch 1",
            &"a".repeat(65),
        ] {
            assert!(
                validate_channel_id(bad).is_err(),
                "주입·비정상 채널 ID 는 거부해야 한다: {bad:?}"
            );
        }
    }

    #[test]
    fn zero_size_icon_rect_falls_back_around_click_point() {
        let zero = Rect {
            position: Position::Physical(PhysicalPosition::new(0, 0)),
            size: Size::Physical(tauri::PhysicalSize::new(0, 0)),
        };
        let bounds = icon_bounds_from_rect(zero, PhysicalPosition::new(1200.0, 18.0));
        assert_eq!(bounds.width, 28.0);
        assert_eq!(bounds.height, 24.0);
        assert_eq!(bounds.x + bounds.width * 0.5, 1200.0);
    }
}
