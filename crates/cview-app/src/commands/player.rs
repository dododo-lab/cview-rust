//! 라이브 재생 명령 — 라이브 상세 → playback JSON → (로컬 프록시) URL → WebView hls.js.
//!
//! 재생은 `cview_player::coordinator::StreamCoordinator` 가 맡는다. 코디네이터는 자기
//! `LocalStreamProxy` 를 띄워 CDN 주소를 `http://127.0.0.1:<port>/…` 로 바꾸고, 엔진
//! ([`WebViewHlsEngine`])의 `play` 가 `loadSource(...)` 스크립트를 `player://command` 로 보낸다.
//! 페이지의 `postToSwift` 메시지는 `player_message` 로 돌아와 코디네이터에 첫 프레임·재버퍼를 알린다.

use std::sync::Arc;

use cview_core::models::live::LiveStatus;
use cview_core::models::player::PlayerPhase;
use cview_player::coordinator::{Configuration, StreamCoordinator};
use cview_player::hlsjs::{HlsJsStreamingProfile, JsCommand, JsMessage};
use cview_player::policy::{PlaybackCookie, PlaybackKeyAccess};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Runtime, State};

use super::{events, CmdResult, CommandError, PlayerCommandPayload};
use crate::dto::{LiveDetailDto, PlaybackStartedDto, PlayerEventDto};
use crate::engine::{CommandSink, EngineSignal, WebViewHlsEngine};
use crate::playback;
use crate::state::{AppServices, PlayerSession};
use crate::MAIN_WINDOW;

/// 플레이어 페이지 hls.js 프로필.
pub const PLAYER_PROFILE: HlsJsStreamingProfile = HlsJsStreamingProfile::LowLatency;

fn command_sink<R: Runtime>(app: AppHandle<R>) -> CommandSink {
    Arc::new(move |script: String| {
        if let Err(e) = app.emit_to(
            MAIN_WINDOW,
            events::PLAYER_COMMAND,
            PlayerCommandPayload { script },
        ) {
            tracing::warn!("플레이어 명령 전달 실패: {e}");
        }
    })
}

async fn stop_session(services: &AppServices) {
    if let Some(session) = services.player.lock().await.take() {
        session.coordinator.stop_stream().await;
        session.pump.abort();
    }
}

/// 라이브 재생 시작.
#[tauri::command]
pub async fn player_start(
    app: AppHandle,
    services: State<'_, AppServices>,
    channel_id: String,
) -> CmdResult<PlaybackStartedDto> {
    stop_session(&services).await;

    let info = services.api.live_detail(&channel_id, true).await?;
    if info.status != LiveStatus::Open {
        return Err(CommandError("현재 방송 중이 아닙니다".into()));
    }
    let plan = playback::resolve(&info)?;
    let player_settings = services.settings.player();

    let cookies = services.cookies.clone();
    let access = PlaybackKeyAccess::new(
        plan.requires_authenticated_key,
        Arc::new(move || {
            cookies
                .auth_cookies()
                .into_iter()
                .map(|c| PlaybackCookie {
                    name: c.name,
                    value: c.value,
                    domain: c.domain,
                    path: c.path,
                    expires: c.expires,
                })
                .collect()
        }),
    );
    let mut config = Configuration::new(channel_id.clone());
    // PID 저지연 제어는 아직 연결하지 않았다 — hls.js 페이지 자체 보정에 맡긴다(engine.rs 참고).
    config.enable_low_latency = false;
    config.stream_proxy_mode = player_settings.stream_proxy_mode;
    config.force_highest_quality = player_settings.force_highest_quality;
    config.preferred_quality = Some(player_settings.quality);

    let coordinator = StreamCoordinator::new(config, access);
    let engine = Arc::new(WebViewHlsEngine::new(
        command_sink(app.clone()),
        PLAYER_PROFILE,
        player_settings.force_highest_quality,
    ));
    coordinator.set_player_engine(engine.clone()).await;

    let mut rx = coordinator.events();
    let event_app = app.clone();
    let pump = tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let Some(dto) = PlayerEventDto::from_stream_event(&event) {
                let _ = event_app.emit(events::PLAYER_EVENT, dto);
            }
        }
    });

    engine.set_volume_silently(player_settings.volume_level);
    *services.player.lock().await = Some(PlayerSession {
        channel_id: channel_id.clone(),
        coordinator: coordinator.clone(),
        engine,
        pump,
    });

    if let Err(e) = coordinator.start_stream(plan.stream_url.clone()).await {
        stop_session(&services).await;
        return Err(CommandError(format!("재생을 시작하지 못했습니다: {e}")));
    }

    if let (Some(store), Some(channel)) = (&services.data_store, &info.channel) {
        if let Err(e) = store.update_last_watched(&channel.channel_id) {
            tracing::debug!("최근 시청 기록 실패: {e}");
        }
    }

    Ok(PlaybackStartedDto {
        live: LiveDetailDto::from_live_info(&info, &channel_id),
        audio_only: plan.is_audio_only,
        requires_authenticated_proxy: plan.requires_authenticated_key,
    })
}

#[tauri::command]
pub async fn player_stop(services: State<'_, AppServices>) -> CmdResult<()> {
    stop_session(&services).await;
    Ok(())
}

/// 볼륨(0~1) — 설정에도 저장한다.
#[tauri::command]
pub async fn player_set_volume(services: State<'_, AppServices>, volume: f32) -> CmdResult<()> {
    let volume = volume.clamp(0.0, 1.0);
    if let Some(session) = services.player.lock().await.as_ref() {
        session.engine.send(JsCommand::SetVolume(volume));
    }
    services.settings.update_player(|p| p.volume_level = volume);
    Ok(())
}

/// 플레이어 페이지 `postToSwift(type, data)` 메시지 (`{ type, data }`).
#[tauri::command]
pub async fn player_message(
    app: AppHandle,
    services: State<'_, AppServices>,
    message: Value,
) -> CmdResult<()> {
    let Some((_, parsed)) = JsMessage::parse(&message, now_seconds()) else {
        return Ok(());
    };
    let session = services.player.lock().await;
    let Some(session) = session.as_ref() else {
        return Ok(());
    };
    match session.engine.handle_message(&parsed) {
        Some(EngineSignal::Playing) => {
            session
                .coordinator
                .handle_playback_engine_state(&PlayerPhase::Playing);
            session.coordinator.report_engine_playing().await;
        }
        Some(EngineSignal::Buffering) => {
            session
                .coordinator
                .handle_playback_engine_state(&PlayerPhase::Buffering { progress: 0.0 });
        }
        Some(EngineSignal::Fatal(message)) => {
            tracing::warn!("hls.js 치명 오류 ({}): {message}", session.channel_id);
            let _ = app.emit(events::PLAYER_EVENT, PlayerEventDto::Error { message });
        }
        None => {}
    }
    Ok(())
}

fn now_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}
