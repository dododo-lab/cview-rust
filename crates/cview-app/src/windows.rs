//! 보조 창 — 원본 player-window(⌘N, PLY-18) · IndependentChatWindow(⇧⌘C, CHT-23) ·
//! MultiChatWindowHost(⇧⌘M, MUL-05) · statistics-window(⇧⌘T, DAT-05).
//!
//! 보조 창은 같은 `index.html` 을 `?w=<종류>&…` 로 연다. 화면은 사이드바 없이 해당 화면 하나만 그린다.
//! 플레이어 스크립트(`player://command`)는 **마지막으로 재생을 시작한 창**으로 보낸다 — 보조 플레이어 창이
//! 닫히면 재생을 멈추고 대상을 메인 창으로 되돌린다.

use std::sync::Mutex;

use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

use crate::commands::{CmdResult, CommandError};
use crate::state::AppServices;

static PLAYER_TARGET: Mutex<String> = Mutex::new(String::new());

/// 플레이어 명령을 받을 창 라벨.
pub fn player_target() -> String {
    let t = PLAYER_TARGET
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if t.is_empty() {
        crate::MAIN_WINDOW.to_string()
    } else {
        t
    }
}

pub fn set_player_target(label: &str) {
    *PLAYER_TARGET.lock().unwrap_or_else(|e| e.into_inner()) = label.to_string();
}

/// 종류별 (창 제목, 너비, 높이, 하나만 둘지).
pub fn window_spec(kind: &str) -> Option<(&'static str, f64, f64, bool)> {
    match kind {
        "player" => Some(("CView 플레이어", 960.0, 600.0, false)),
        "chat" => Some(("CView 채팅", 420.0, 720.0, false)),
        "multichat" => Some(("CView 멀티채팅", 1100.0, 760.0, true)),
        "stats" => Some(("CView 통계", 980.0, 760.0, true)),
        _ => None,
    }
}

/// 쿼리 문자열 — 키·값을 인코딩한다. 순수 함수.
pub fn window_query(kind: &str, params: &[(String, String)]) -> String {
    let mut ser = url::form_urlencoded::Serializer::new(String::new());
    ser.append_pair("w", kind);
    for (k, v) in params {
        ser.append_pair(k, v);
    }
    ser.finish()
}

/// 보조 창 열기. 하나만 두는 종류는 이미 있으면 앞으로 가져온다.
#[tauri::command]
pub async fn window_open(
    app: AppHandle,
    kind: String,
    params: Option<std::collections::HashMap<String, String>>,
) -> CmdResult<String> {
    let (title, w, h, single) =
        window_spec(&kind).ok_or_else(|| CommandError(format!("알 수 없는 창: {kind}")))?;
    if single {
        if let Some(existing) = app.get_webview_window(&kind) {
            let _ = existing.unminimize();
            let _ = existing.show();
            let _ = existing.set_focus();
            return Ok(kind);
        }
    }
    let label = if single {
        kind.clone()
    } else {
        format!("{kind}-{}", chrono::Utc::now().timestamp_millis())
    };
    let mut pairs: Vec<(String, String)> = params.unwrap_or_default().into_iter().collect();
    pairs.sort();
    let url = format!("index.html?{}", window_query(&kind, &pairs));
    let builder = WebviewWindowBuilder::new(&app, &label, WebviewUrl::App(url.into()))
        .title(title)
        .inner_size(w, h)
        .min_inner_size(360.0, 320.0)
        .background_color(tauri::window::Color(15, 16, 18, 255));
    // 메인 창과 같은 오버레이 제목 막대 — 제목 글자 없이 신호등만 안쪽으로(원본 TrafficLightsAnimator).
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(16.0, 20.0));
    // Windows — 메인 창처럼 제목 막대 없이 화면 위 띠에 최소화·최대화·닫기를 그린다(window-controls.js).
    #[cfg(windows)]
    let builder = builder.decorations(false);
    let window = builder
        .build()
        .map_err(|e| CommandError(format!("창을 열지 못했습니다: {e}")))?;
    #[cfg(windows)]
    let _ = window.hide_menu();
    let handle = app.clone();
    let l = label.clone();
    let is_player = kind == "player";
    window.on_window_event(move |event| {
        if !matches!(event, tauri::WindowEvent::Destroyed) {
            return;
        }
        // 보조 창이 연 채팅 세션 — OS 닫기 버튼으로 닫혀도 정리한다(예전엔 웹소켓이 계속 돌았다).
        let h = handle.clone();
        let label = l.clone();
        let stop_player = is_player && player_target() == l;
        if stop_player {
            set_player_target(crate::MAIN_WINDOW);
        }
        tauri::async_runtime::spawn(async move {
            let services = h.state::<AppServices>();
            services.drop_chat_sessions_of_window(&label).await;
            if stop_player {
                services.stop_player().await;
            }
        });
    });
    Ok(label)
}

/// 이 창이 보조 창이면 닫는다(보조 창 화면의 "닫기").
#[tauri::command]
pub fn window_close_self<R: Runtime>(window: tauri::WebviewWindow<R>) -> CmdResult<()> {
    if window.label() == crate::MAIN_WINDOW {
        return Err(CommandError("메인 창은 닫지 않습니다".into()));
    }
    window.close().map_err(|e| CommandError(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_and_queries() {
        assert!(window_spec("player").is_some());
        assert!(window_spec("nope").is_none());
        assert_eq!(
            window_query("chat", &[("channelId".into(), "a b&c".into())]),
            "w=chat&channelId=a+b%26c"
        );
        set_player_target("");
        assert_eq!(player_target(), crate::MAIN_WINDOW);
        set_player_target("player-1");
        assert_eq!(player_target(), "player-1");
        set_player_target("");
    }
}
