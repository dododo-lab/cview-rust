//! CView v2 데스크톱 앱 — Tauri 2 셸 (포팅 계획 단계 7).
//!
//! - 화면은 `app-ui` 의 정적 HTML/CSS/ES 모듈(번들러 없음)이다.
//! - Rust 쪽은 하위 크레이트 서비스를 [`state::AppServices`] 로 묶어 관리 상태로 두고,
//!   얇은 명령 계층([`commands`])으로 노출한다. 결과는 [`dto`] 로 평평하게 만든다.
//! - 라이브 재생은 WebView 안의 기존 hls.js 플레이어(`app-ui/player/player.html`)가 하고,
//!   `cview-player` 의 코디네이터·로컬 프록시가 URL 과 수명을 맡는다([`engine`]).

pub mod commands;
pub mod diagnostics;
pub mod dock_icon;
pub mod dto;
pub mod engine;
pub mod image_service;
pub mod live_connector;
pub mod login;
pub mod memory_pressure;
pub mod menu;
pub mod multi_live;
pub mod perf_log;
pub mod playback;
pub mod state;
pub mod system;
pub mod tray;
pub mod tray_popover;
pub mod ui_watchdog;
pub mod updater;
pub mod usage;
pub mod webview_store;
pub mod window_prefs;
pub mod windows;

use std::sync::Arc;

use cview_core::utils::following_poll::FollowingPollReason;
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
        let mut expired = services.api.subscribe_session_expired();
        services.auth.initialize().await;
        services.sync_authentication().await;
        prefetch_following(&app);
        // broadcast_status 는 쿠키 세션을 서버에 확인한다 — 죽은 세션이면 여기서 만료로 바뀌고 아래 루프가 다시 알린다.
        broadcast_status(&app, &services).await;
        loop {
            tokio::select! {
                state = states.recv() => {
                    if state.is_none() {
                        break;
                    }
                    // 상태 전이마다(로그인·로그아웃·만료) 최신 상태를 다시 계산해 보낸다.
                    while states.try_recv().is_ok() {}
                }
                signal = expired.recv() => {
                    if matches!(signal, Err(tokio::sync::broadcast::error::RecvError::Closed)) {
                        break;
                    }
                    // 쓰는 도중 서버가 401 을 줬다 — 메모된 사용자 상태를 버리고 서버에 다시 확인한다.
                    while expired.try_recv().is_ok() {}
                    services.api.invalidate_authentication_cache().await;
                }
            }
            services.sync_authentication().await;
            broadcast_status(&app, &services).await;
        }
    });
}

/// 멀티라이브 상태 감시 주기 — 세션이 없으면 드물게.
fn multi_live_state_tick(idle: bool) -> std::time::Duration {
    std::time::Duration::from_secs(if idle { 30 } else { 1 })
}

/// 시작 미리 받기 — 웹뷰가 뜨고 홈이 데이터를 부르기 전에 인기 라이브 첫 묶음을 받기 시작한다.
/// 홈·사이드바의 `popular_lives` 는 같은 첫 로드를 기다려 이어 받는다(`load_initial_if_needed`).
fn prefetch_popular<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let services = app.state::<AppServices>();
        services.popular.load_initial_if_needed().await;
        perf_log::emit(
            "startup.prefetch_popular",
            serde_json::json!({ "items": services.popular.read().feed.len() }),
        );
    });
}

/// 로그인 복원 직후 팔로잉 라이브도 미리 받기 시작한다 — 홈·사이드바의 화면 진입 폴은 진행 중인 폴에 붙는다.
fn prefetch_following<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let services = app.state::<AppServices>();
        if !services.auth.is_authenticated() {
            return;
        }
        let ok = services
            .following
            .poll(FollowingPollReason::ScreenEntry)
            .await
            .is_some();
        perf_log::emit(
            "startup.prefetch_following",
            serde_json::json!({ "ok": ok }),
        );
    });
}

/// 멀티라이브 상태 감시 → `multilive://state`. 매니저 변화나 주기로 보고, 바뀐 것만 보낸다
/// (세션 재생 상태는 매니저 감시 채널을 거치지 않으므로 주기 확인이 필요하다).
/// 주기는 재생 중인 세션이 있으면 1초, 없으면 30초 — 세션이 없을 때 볼 것은 자동 네트워크 프리셋의
/// 불안정 유지 시간이 지났는지뿐이라, 앱이 놀고 있을 때 초마다 깨지 않게 한다(세션이 생기면 매니저 변화로 깬다).
fn spawn_multi_live_state_task<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        let shell = app.state::<MultiLiveShell>();
        let mut changes = shell.manager.subscribe();
        let mut last = None;
        loop {
            let idle = shell.manager.read().sessions().is_empty();
            tokio::select! {
                changed = changes.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
                _ = tokio::time::sleep(multi_live_state_tick(idle)) => {}
            }
            // 자동 네트워크 프리셋 — 멀티라이브 스트림 수·재생 오류에 맞춰 다시 고른다. 보낼 상태로 고르고,
            // 프리셋을 바꿨을 때만(드물다) 상태를 다시 만든다.
            let mut state = shell.state_dto();
            if commands::settings::apply_auto_network_for(&app.state::<AppServices>(), &state) {
                let _ = app.emit(events::NETWORK_PRESET_CHANGED, ());
                state = shell.state_dto();
            }
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
            match &dto.event {
                crate::dto::ChatEventDto::Messages { messages, .. } => {
                    perf_log::count_chat_batch(messages.len());
                }
                crate::dto::ChatEventDto::State { state, .. } => {
                    perf_log::emit("chat.state", serde_json::json!({ "state": state }));
                }
                crate::dto::ChatEventDto::Reconnecting => {
                    perf_log::emit("chat.state", serde_json::json!({ "state": "reconnecting" }));
                }
                _ => {}
            }
            let _ = chat_app.emit(events::CHAT_EVENT, dto);
        }),
        chat_session_event: Arc::new(move |session, event| {
            if let crate::dto::ChatEventDto::Messages { messages, .. } = &event.event {
                perf_log::count_chat_batch(messages.len());
            }
            // 빌린 구조체로 바로 직렬화한다 — `json!` 은 메시지 묶음 전체를 Value 트리로 한 번 더 복사했다.
            let _ = session_app.emit(
                events::CHAT_SESSION_EVENT,
                crate::dto::SessionChatEvent {
                    session: &session,
                    event: &event,
                },
            );
        }),
    }
}

/// 앱 실행.
pub fn run() {
    perf_log::mark_process_start();
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
        // 썸네일·아바타 — `cvimg://localhost/<종류>%2F<URL>`(Windows `http://cvimg.localhost/...`), 디스크·메모리 캐시.
        .register_asynchronous_uri_scheme_protocol("cvimg", |ctx, request, responder| {
            let service = ctx
                .app_handle()
                .try_state::<Arc<image_service::ImageService>>()
                .map(|s| s.inner().clone());
            tauri::async_runtime::spawn(async move {
                responder.respond(image_service::respond(service, request).await);
            });
        })
        .setup(|app| {
            let handle = app.handle().clone();
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            // 디버그 로그 — 환경 변수로 강제하면 설정을 읽기 전부터 시작 단계를 남긴다.
            if perf_log::forced_by_env() {
                perf_log::set_enabled(&data_dir, true);
            }
            perf_log::emit("startup.setup_begin", serde_json::json!({}));
            tracing::info!("데이터 디렉터리: {}", data_dir.display());
            let webview = Arc::new(TauriWebViewStore::new(handle.clone()));
            let keychain_service = state::keychain_service_name(&handle.config().identifier);
            let services = tauri::async_runtime::block_on(AppServices::initialize(
                &data_dir,
                &keychain_service,
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
                services.watch_events.clone(),
                services.data_store.clone(),
            );
            perf_log::set_enabled(
                &data_dir,
                perf_log::forced_by_env() || services.settings.appearance().debug_mode,
            );
            perf_log::emit("startup.services_ready", serde_json::json!({}));
            commands::settings::apply_window_theme(&handle, services.settings.appearance().theme);
            // 저장된 화면 크기 — 첫 화면을 100% 로 그렸다가 튀지 않게 창이 그리기 전에 건다.
            commands::settings::apply_ui_zoom_all(&handle, services.settings.appearance().ui_scale);
            window_prefs::apply_on_launch(&handle, &data_dir, &services.settings.general());
            // 창 위치·크기 저장 — 움직이거나 크기를 바꾸면 기록한다(SYS-16).
            // 끄는 동안의 이벤트는 모았다가 조용해지면 한 번 쓴다(window_prefs::FrameSaver).
            if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
                let saver = window_prefs::FrameSaver::new(data_dir.clone());
                app.manage(saver.clone()); // 종료(⌘Q) 때 밀린 위치를 쓴다 — exit_cleanup
                let w = window.clone();
                window.on_window_event(move |event| {
                    match event {
                        tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => {
                            saver.schedule(window_prefs::current_frame(&w));
                        }
                        tauri::WindowEvent::CloseRequested { .. } => {
                            saver.save_now(window_prefs::current_frame(&w));
                        }
                        _ => {}
                    }
                    // 설정에 따라 닫기·최소화를 메뉴 막대·알림 영역으로 숨기기로 바꾼다(tray.rs).
                    tray::on_main_window_event(&w, event);
                    // 시스템 외형이 바뀌면(앱 테마 "시스템"일 때) Dock 아이콘을 다시 고른다.
                    if let tauri::WindowEvent::ThemeChanged(_) = event {
                        let app = w.app_handle();
                        if let Some(services) = app.try_state::<AppServices>() {
                            dock_icon::apply(app, services.settings.appearance().theme);
                        }
                    }
                });
                // macOS 최소화는 창 이벤트가 없어 AppKit 알림으로 받는다(tray.rs).
                #[cfg(target_os = "macos")]
                tray::observe_minimize(&window);
            }
            if let Some(images) = image_service::open_service(&data_dir) {
                app.manage(images);
            }
            app.manage(commands::download::Downloads::new(&handle, &data_dir));
            app.manage(services);
            app.manage(shell);
            app.manage(system::PendingDeepLinks::default());
            // 앱 사용률(설정 › 성능) — 꺼져 있으면 표본 스레드는 잠들어 있다. 화면이 보고 있을 때만 메인 창에 보낸다.
            {
                let monitor = usage::UsageMonitor::new(usage::UsageConfig::from_appearance(
                    &app.state::<AppServices>().settings.appearance(),
                ));
                let h = handle.clone();
                monitor.start(move |live| {
                    let _ = h.emit_to(MAIN_WINDOW, usage::SAMPLE_EVENT, live);
                });
                app.manage(monitor);
            }
            spawn_multi_live_state_task(handle.clone());
            // OS 메모리 압박 신호 → 이미지·API 캐시 줄이기(심각하면 비선택 패인 디코드 잠시 멈춤).
            memory_pressure::start(handle.clone());
            // 저장소 변경 이력(메모리에만 있는 커밋 시각, 최대 10만 개)을 시작 1분 뒤, 그 뒤 6시간마다 정리한다 —
            // 정리 함수는 있었으나 부르는 곳이 없어 앱을 오래 켜 두면 끝까지 쌓였다. 같은 때 오래된 다시보기(60일)·
            // 클립(30일) 이어 보기 위치도 지운다(역시 부르는 곳이 없어 끝없이 쌓였다). 시청 기록은 지우지 않는다.
            if let Some(store) = app.state::<AppServices>().data_store.clone() {
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    loop {
                        let (clips, vods) = store.prune_playback_positions().await;
                        if clips + vods > 0 {
                            tracing::info!(
                                "오래된 이어 보기 위치 정리: 클립 {clips}, 다시보기 {vods}"
                            );
                        }
                        store.trim_change_history().await;
                        tokio::time::sleep(std::time::Duration::from_secs(6 * 3600)).await;
                    }
                });
            }
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
            // Windows — 제목 막대 없는 창(tauri.windows.conf.json)이라 메뉴 막대도 숨기고 화면 머리줄의 메뉴 버튼으로 연다.
            menu::hide_menu_bars(&handle);
            if let Err(e) = tray::install(&handle) {
                tracing::warn!("메뉴 막대 아이콘을 달지 못했습니다: {e}");
            }
            prefetch_popular(&handle);
            // 웹 화면 멈춤 감시 — GPU 프로세스가 끊긴 뒤 창 전체가 검게 남던 문제(ui_watchdog.rs).
            ui_watchdog::spawn(handle.clone());
            spawn_auth_tasks(handle);
            perf_log::emit("startup.setup_end", serde_json::json!({}));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::auth::auth_status,
            commands::auth::auth_login,
            commands::auth::auth_logout,
            commands::live::following_lives,
            commands::live::popular_lives,
            commands::live::search,
            commands::live::search_more_clips,
            commands::live::channel_info,
            commands::live::live_detail,
            commands::player::player_start,
            commands::player::player_stop,
            commands::player::player_set_volume,
            commands::player::player_message,
            commands::player::player_network_path,
            commands::multi_live::multilive_state,
            commands::multi_live::multilive_add,
            commands::multi_live::multilive_remove,
            commands::multi_live::multilive_select,
            commands::multi_live::multilive_set_view,
            commands::multi_live::multilive_watch,
            commands::multi_live::multilive_park,
            commands::multi_live::multilive_set_window_occluded,
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
            commands::insights::insights_report,
            commands::download::download_list,
            system::deeplink_take_pending,
            tray::tray_set_state,
            tray_popover::tray_get_state,
            tray_popover::tray_popover_close,
            tray_popover::tray_control,
            windows::window_open,
            windows::window_close_self,
            menu::window_menu_popup,
            commands::oauth::oauth_status,
            commands::oauth::oauth_set_credentials,
            commands::oauth::oauth_clear_credentials,
            commands::oauth::oauth_connect,
            system::notify_send,
            system::notify_permission,
            system::update_check,
            system::release_notes,
            diagnostics::diagnostic_report,
            updater::update_install,
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
            commands::usage::usage_status,
            commands::usage::usage_history,
            commands::usage::usage_watch,
            commands::usage::usage_clear,
            commands::usage::usage_export,
            commands::multi_live::multilive_set_audio_suspended,
            commands::media::vod_play,
            commands::preview::preview_source,
            commands::ads::ad_preroll,
            commands::ads::ad_fetch_vast,
            commands::ads::ad_track,
            commands::ads::live_connector_status,
            commands::media::vod_progress_save,
            commands::media::vod_continue_list,
            commands::media::vod_progress_remove,
            commands::media::vod_watch_later_set,
            commands::media::vod_watch_later_list,
            commands::content::channel_detail,
            commands::content::channel_overview,
            commands::content::following_channels_all,
            commands::content::following_vods,
            commands::content::channel_vods,
            commands::content::channel_clips,
            commands::content::channel_posts,
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
            commands::chat::chat_user_card,
            commands::chat_avatar::chat_avatars,
            commands::settings::appearance_set_theme,
            commands::chat::chat_disconnect,
            commands::chat::chat_send,
            commands::settings::settings_get,
            commands::settings::settings_update,
            commands::settings::notice_accept,
            commands::content::app_quit,
            commands::settings::ui_zoom_sync,
            commands::settings::ui_zoom_step,
            commands::settings::perf_log,
            commands::settings::perf_log_status,
            ui_watchdog::ui_heartbeat,
            commands::settings::image_cache_status,
            commands::settings::image_cache_clear,
        ])
        .build(tauri::generate_context!())
        .expect("Tauri 앱을 만들지 못했습니다");

    app.run(|handle, event| {
        // Dock 아이콘을 누르면 메뉴 막대로 숨겨 둔 창을 다시 연다.
        #[cfg(target_os = "macos")]
        if let RunEvent::Reopen {
            has_visible_windows,
            ..
        } = event
        {
            tray::on_reopen(handle, has_visible_windows);
        }
        if let RunEvent::Exit = event {
            // 종료 정리는 tao 의 applicationWillTerminate(extern "C") 안에서 돈다 — 여기서 패닉이 나면 풀 수 없어
            // 앱이 abort 로 죽는다(재부팅·로그아웃·⌘Q 때 크래시 보고서). 그래서 패닉을 잡아 기록만 하고 넘긴다.
            let cleanup = std::panic::AssertUnwindSafe(|| exit_cleanup(handle));
            if std::panic::catch_unwind(cleanup).is_err() {
                tracing::error!("종료 정리 중 패닉 — 정리를 건너뛰고 끝냅니다");
            }
        }
    });
}

/// 종료 정리(서비스·멀티라이브)를 함께 돌리고 길어야 3초만 기다린다 — 네트워크가 막혀도 앱이 끝나지 않는 일이 없게.
/// 타이머(`tokio::time::timeout`)는 반드시 런타임 안(async 블록)에서 만든다: 밖에서 만들면 "no reactor running" 패닉.
fn exit_cleanup<R: Runtime>(handle: &AppHandle<R>) {
    const LIMIT: std::time::Duration = std::time::Duration::from_secs(3);
    // 창을 옮기고 곧바로 끝내면(⌘Q 는 창 닫기 이벤트 없이 끝날 수 있다) 아직 쓰지 않은 위치가 남아 있다.
    if let Some(saver) = handle.try_state::<window_prefs::FrameSaver>() {
        saver.save_now(None);
    }
    let started = std::time::Instant::now();
    let services = handle.try_state::<AppServices>();
    let shell = handle.try_state::<MultiLiveShell>();
    // block_on 은 이 스레드를 막고 기다리므로 State 를 빌린 채로 넘겨도 된다.
    let done = tauri::async_runtime::block_on(async {
        let services = async {
            if let Some(services) = &services {
                services.shutdown().await;
            }
        };
        let multilive = async {
            if let Some(shell) = &shell {
                shell.manager.stop_all().await;
            }
        };
        let downloads = commands::download::cancel_all_for_exit(handle, LIMIT);
        tokio::time::timeout(LIMIT, async {
            tokio::join!(services, multilive, downloads)
        })
        .await
    });
    if done.is_err() {
        tracing::warn!("종료 정리가 3초를 넘어 건너뜁니다");
    }
    // 시청 기록 종료까지 쓴 뒤 WAL 을 비운다 — 앱을 끄고 백업 파일로 바꾸는 손 복원(통계 › 데이터 관리 안내)이 남은 WAL 과 섞이지 않게.
    if let Some(store) = services.as_ref().and_then(|s| s.data_store.as_ref()) {
        if let Err(e) = store.checkpoint_wal() {
            tracing::debug!("종료 WAL 정리 실패: {e}");
        }
    }
    tracing::info!(ms = started.elapsed().as_millis() as u64, "종료 정리 끝");
}
