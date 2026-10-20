//! CView v2 데스크톱 앱 — Tauri 2 셸 (포팅 계획 단계 7).
//!
//! - 화면은 `app-ui` 의 정적 HTML/CSS/ES 모듈(번들러 없음)이다.
//! - Rust 쪽은 하위 크레이트 서비스를 [`state::AppServices`] 로 묶어 관리 상태로 두고,
//!   얇은 명령 계층([`commands`])으로 노출한다. 결과는 [`dto`] 로 평평하게 만든다.
//! - 라이브 재생은 WebView 안의 기존 hls.js 플레이어(`app-ui/player/player.html`)가 하고,
//!   `cview-player` 의 코디네이터·로컬 프록시가 URL 과 수명을 맡는다([`engine`]).

pub mod commands;
pub mod dock_icon;
pub mod dto;
pub mod engine;
pub mod login;
pub mod menu;
pub mod multi_live;
pub mod playback;
pub mod state;
pub mod system;
pub mod tray;
pub mod webview_store;
pub mod window_prefs;
pub mod windows;

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, RunEvent, Runtime};

use crate::commands::auth::broadcast_status;
use crate::commands::{events, PlayerCommandPayload};
use crate::multi_live::{MultiLiveShell, SlotCommandPayload};
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

/// 멀티라이브 상태 감시 → `multilive://state`. 매니저 변화나 1초 주기로 보고, 바뀐 것만 보낸다
/// (세션 재생 상태는 매니저 감시 채널을 거치지 않으므로 주기 확인이 필요하다).
fn spawn_multi_live_state_task<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        let shell = app.state::<MultiLiveShell>();
        let mut changes = shell.manager.subscribe();
        let mut last = None;
        loop {
            tokio::select! {
                changed = changes.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
            }
            let state = shell.state_dto();
            if last.as_ref() != Some(&state) {
                let _ = app.emit(events::MULTILIVE_STATE, &state);
                last = Some(state);
            }
        }
    });
}

/// 프런트엔드로 가는 통로 — 플레이어 스크립트는 메인 창에만, 이벤트는 전체로.
fn shell_sinks<R: Runtime>(app: &AppHandle<R>) -> ShellSinks {
    let command_app = app.clone();
    let player_app = app.clone();
    let chat_app = app.clone();
    let session_app = app.clone();
    ShellSinks {
        player_command: Arc::new(move |script: String| {
            if let Err(e) = command_app.emit_to(
                windows::player_target().as_str(),
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
        chat_session_event: Arc::new(move |session, event| {
            let _ = session_app.emit(
                events::CHAT_SESSION_EVENT,
                serde_json::json!({ "session": session, "event": event }),
            );
        }),
    }
}

/// 앱 실행.
pub fn run() {
    init_tracing();
    let app = tauri::Builder::default()
        // 단일 인스턴스는 다른 플러그인보다 먼저 등록해야 한다(SYS-07).
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            system::on_second_instance(app, argv);
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_notification::init())
        .menu(menu::build)
        .on_menu_event(|handle, event| menu::forward(handle, event.id().as_ref()))
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
            let command_app = handle.clone();
            let shell = MultiLiveShell::new(
                services.api.clone(),
                services.cookies.clone(),
                services.settings.clone(),
                Arc::new(move |slot, script| {
                    if let Err(e) = command_app.emit_to(
                        MAIN_WINDOW,
                        events::MULTILIVE_COMMAND,
                        SlotCommandPayload { slot, script },
                    ) {
                        tracing::warn!("멀티라이브 명령 전달 실패: {e}");
                    }
                }),
                &data_dir,
            );
            commands::settings::apply_window_theme(&handle, services.settings.appearance().theme);
            window_prefs::apply_on_launch(&handle, &data_dir, &services.settings.general());
            // 창 위치·크기 저장 — 움직이거나 크기를 바꾸면 기록한다(SYS-16).
            if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
                let dir = data_dir.clone();
                let w = window.clone();
                window.on_window_event(move |event| {
                    if matches!(
                        event,
                        tauri::WindowEvent::Moved(_)
                            | tauri::WindowEvent::Resized(_)
                            | tauri::WindowEvent::CloseRequested { .. }
                    ) {
                        window_prefs::save_frame(&dir, &w);
                    }
                    // 시스템 외형이 바뀌면(앱 테마 "시스템"일 때) Dock 아이콘을 다시 고른다.
                    if let tauri::WindowEvent::ThemeChanged(_) = event {
                        let app = w.app_handle();
                        if let Some(services) = app.try_state::<AppServices>() {
                            dock_icon::apply(app, services.settings.appearance().theme);
                        }
                    }
                });
            }
            app.manage(commands::download::Downloads::new(&data_dir));
            app.manage(services);
            app.manage(shell);
            app.manage(system::PendingDeepLinks::default());
            spawn_multi_live_state_task(handle.clone());
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                let h = handle.clone();
                app.deep_link().on_open_url(move |event| {
                    system::handle_urls(&h, event.urls().into_iter().map(|u| u.to_string()));
                });
                if let Ok(Some(urls)) = app.deep_link().get_current() {
                    system::handle_urls(&handle, urls.into_iter().map(|u| u.to_string()));
                }
            }
            system::spawn_live_notifications(handle.clone());
            if let Err(e) = tray::install(&handle) {
                tracing::warn!("메뉴 막대 아이콘을 달지 못했습니다: {e}");
            }
            if let Ok(path) = std::env::var("CVIEW_DEBUG_EVAL") {
                let h = handle.clone();
                std::thread::spawn(move || loop {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    if let Ok(js) = std::fs::read_to_string(&path) {
                        let _ = std::fs::remove_file(&path);
                        for w in h.webview_windows().values() {
                            let _ = w.eval(&js);
                        }
                    }
                });
            }
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
            commands::multi_live::multilive_state,
            commands::multi_live::multilive_add,
            commands::multi_live::multilive_remove,
            commands::multi_live::multilive_select,
            commands::multi_live::multilive_watch,
            commands::multi_live::multilive_park,
            commands::multi_live::multilive_set_layout,
            commands::multi_live::multilive_stage_size,
            commands::multi_live::multilive_stop_all,
            commands::multi_live::multilive_player_message,
            commands::multi_live::multilive_set_focus_layout,
            commands::multi_live::multilive_retry,
            commands::multi_live::multilive_reorder,
            commands::multi_live::multilive_audio,
            commands::multi_live::multilive_set_ratios,
            commands::multi_live::preset_list,
            commands::multi_live::preset_save,
            commands::multi_live::preset_delete,
            commands::insights::insights_overview,
            commands::download::download_list,
            system::deeplink_take_pending,
            windows::window_open,
            windows::window_close_self,
            commands::oauth::oauth_status,
            commands::oauth::oauth_set_credentials,
            commands::oauth::oauth_clear_credentials,
            commands::oauth::oauth_connect,
            system::notify_send,
            system::notify_permission,
            system::update_check,
            commands::content::open_path,
            commands::content::open_notification_settings,
            commands::download::download_start,
            commands::download::download_cancel,
            commands::download::download_clear,
            commands::insights::data_health,
            commands::insights::data_backup_now,
            commands::insights::data_cleanup,
            commands::settings::app_info,
            commands::settings::settings_export,
            commands::settings::settings_import,
            commands::settings::settings_reset,
            commands::settings::network_preset,
            commands::multi_live::multilive_set_audio_suspended,
            commands::media::vod_play,
            commands::media::vod_progress_save,
            commands::media::vod_continue_list,
            commands::media::vod_progress_remove,
            commands::media::vod_watch_later_set,
            commands::media::vod_watch_later_list,
            commands::content::channel_detail,
            commands::content::following_channels_all,
            commands::content::following_vods,
            commands::content::channel_vods,
            commands::content::channel_clips,
            commands::content::popular_clips,
            commands::content::search_videos,
            commands::content::clip_play,
            commands::content::vod_detail,
            commands::content::channel_follow,
            commands::content::favorite_set,
            commands::content::favorites_list,
            commands::content::recent_channels,
            commands::content::recent_channels_clear,
            commands::content::memo_set,
            commands::content::bookmark_add,
            commands::content::bookmarks_list,
            commands::content::bookmark_delete,
            commands::content::save_file,
            commands::recording::player_record_start,
            commands::recording::player_record_stop,
            commands::recording::player_record_status,
            commands::recording::equalizer_presets,
            commands::content::reveal_path,
            commands::content::open_external,
            commands::chat::chat_connect,
            commands::chat::chat_donation_ranking,
            commands::chat::chat_emoticons,
            commands::settings::appearance_set_theme,
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
            if let Some(shell) = handle.try_state::<MultiLiveShell>() {
                tauri::async_runtime::block_on(shell.manager.stop_all());
            }
        }
    });
}
