//! CView v2 데스크톱 앱 — Tauri 2 셸 (포팅 계획 단계 7).
//!
//! - 화면은 `rust/app-ui` 의 정적 HTML/CSS/ES 모듈(번들러 없음)이다.
//! - Rust 쪽은 하위 크레이트 서비스를 [`state::AppServices`] 로 묶어 관리 상태로 두고,
//!   얇은 명령 계층([`commands`])으로 노출한다. 결과는 [`dto`] 로 평평하게 만든다.
//! - 라이브 재생은 WebView 안의 기존 hls.js 플레이어(`app-ui/player/player.html`)가 하고,
//!   `cview-player` 의 코디네이터·로컬 프록시가 URL 과 수명을 맡는다([`engine`]).

pub mod commands;
pub mod dto;
pub mod engine;
pub mod login;
pub mod playback;
pub mod state;
pub mod webview_store;

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, RunEvent, Runtime};

use crate::commands::auth::broadcast_status;
use crate::commands::{events, PlayerCommandPayload};
use crate::state::{AppServices, ShellSinks};
use crate::webview_store::TauriWebViewStore;

/// 메인 창 라벨 (`tauri.conf.json`).
pub const MAIN_WINDOW: &str = "main";

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

/// 인증 복원 + 상태 변화 구독 → `auth://changed`.
fn spawn_auth_tasks<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        let services = app.state::<AppServices>();
        let mut states = services.auth.auth_state_stream();
        services.auth.initialize().await;
        services.sync_authentication().await;
        broadcast_status(&app, &services).await;
        while states.recv().await.is_some() {
            // 상태 전이마다(로그인·로그아웃·만료) 최신 상태를 다시 계산해 보낸다.
            while states.try_recv().is_ok() {}
            services.sync_authentication().await;
            broadcast_status(&app, &services).await;
        }
    });
}

/// 프런트엔드로 가는 통로 — 플레이어 스크립트는 메인 창에만, 이벤트는 전체로.
fn shell_sinks<R: Runtime>(app: &AppHandle<R>) -> ShellSinks {
    let command_app = app.clone();
    let player_app = app.clone();
    let chat_app = app.clone();
    ShellSinks {
        player_command: Arc::new(move |script: String| {
            if let Err(e) = command_app.emit_to(
                MAIN_WINDOW,
                events::PLAYER_COMMAND,
                PlayerCommandPayload { script },
            ) {
                tracing::warn!("플레이어 명령 전달 실패: {e}");
            }
        }),
        player_event: Arc::new(move |dto| {
            let _ = player_app.emit(events::PLAYER_EVENT, dto);
        }),
        chat_event: Arc::new(move |dto| {
            let _ = chat_app.emit(events::CHAT_EVENT, dto);
        }),
    }
}

/// 앱 실행.
pub fn run() {
    init_tracing();
    let app = tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            tracing::info!("데이터 디렉터리: {}", data_dir.display());
            let webview = Arc::new(TauriWebViewStore::new(handle.clone()));
            let services = tauri::async_runtime::block_on(AppServices::initialize(
                &data_dir,
                webview,
                shell_sinks(&handle),
            ));
            app.manage(services);
            spawn_auth_tasks(handle);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::auth::auth_status,
            commands::auth::auth_login,
            commands::auth::auth_logout,
            commands::live::following_lives,
            commands::live::popular_lives,
            commands::live::search,
            commands::live::channel_info,
            commands::live::live_detail,
            commands::player::player_start,
            commands::player::player_stop,
            commands::player::player_set_volume,
            commands::player::player_message,
            commands::chat::chat_connect,
            commands::chat::chat_disconnect,
            commands::chat::chat_send,
            commands::settings::settings_get,
            commands::settings::settings_update,
        ])
        .build(tauri::generate_context!())
        .expect("Tauri 앱을 만들지 못했습니다");

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            if let Some(services) = handle.try_state::<AppServices>() {
                tauri::async_runtime::block_on(services.shutdown());
            }
        }
    });
}
