//! 라이브 재생 명령 — 라이브 상세 → playback JSON → app-core `PlayerViewModel` → WebView hls.js.
//!
//! 재생 상태기계는 `cview_app_core::viewmodels::player::PlayerViewModel` 이 맡는다. 뷰모델은
//! `StreamCoordinator` 를 만들어(저지연 PID 제어 포함) 로컬 프록시 URL 을 엔진
//! ([`crate::engine::WebViewHlsEngine`])에 넘기고, 엔진의 `play` 가 `loadSource(...)` 스크립트를
//! `player://command` 로 보낸다. 페이지의 `postToSwift` 메시지는 `player_message` 로 돌아와
//! 뷰모델의 엔진 콜백(첫 프레임·재버퍼·오류)으로 들어간다. 코디네이터 이벤트는 뷰모델 콜백 탭이
//! `player://event` 로 내보낸다(`state.rs`).

use std::sync::Arc;

use cview_app_core::viewmodels::player::{PlayerViewModel, StartStreamRequest};
use cview_core::models::live::LiveStatus;
use cview_core::models::player::{PlayerError, PlayerPhase};
use cview_player::coordinator::StreamPhase;
use cview_player::hlsjs::JsMessage;
use cview_player::policy::{PlaybackCookie, PlaybackKeyAccess};
use serde_json::Value;
use tauri::State;

use super::{CmdResult, CommandError};
use crate::dto::{LiveDetailDto, PlaybackStartedDto, PlayerEventDto};
use crate::engine::EngineSignal;
use crate::playback;
use crate::state::AppServices;

pub use crate::state::PLAYER_PROFILE;

/// 라이브 재생 시작.
#[tauri::command]
pub async fn player_start(
    window: tauri::Window,
    services: State<'_, AppServices>,
    channel_id: String,
) -> CmdResult<PlaybackStartedDto> {
    services.stop_player().await;
    // 재생을 시작한 창이 플레이어 명령을 받는다(보조 플레이어 창, PLY-18).
    crate::windows::set_player_target(window.label());

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

    let mut request = StartStreamRequest::new(channel_id.clone(), plan.stream_url.clone());
    request.channel_name = info
        .channel
        .as_ref()
        .map(|c| c.channel_name.clone())
        .unwrap_or_default();
    request.live_title = info.live_title.clone();
    request.thumbnail_url = info.resolved_live_image_url().cloned();
    request.player_settings = Some(player_settings);
    request.playback_key_access = access;

    let connector_limited_height =
        crate::live_connector::policy_height(plan.p2p_general_max_height);
    services
        .engine
        .set_policy_quality_ceiling_height(connector_limited_height);

    let mut channel = services.player_channel.lock().await;
    services.player.start_stream(request).await;
    let phase = services.player.read().stream_phase.clone();
    if let StreamPhase::Error(message) = phase {
        drop(channel);
        services.player.stop_stream().await;
        return Err(CommandError(format!(
            "재생을 시작하지 못했습니다: {message}"
        )));
    }
    *channel = Some(channel_id.clone());
    drop(channel);

    services.begin_watch_record(&info);
    *services
        .playing_stream
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some((
        plan.stream_url.clone(),
        info.channel
            .as_ref()
            .map(|c| c.channel_name.clone())
            .unwrap_or_else(|| channel_id.clone()),
    ));

    Ok(PlaybackStartedDto {
        live: LiveDetailDto::from_live_info(&info, &channel_id),
        audio_only: plan.is_audio_only,
        requires_authenticated_proxy: plan.requires_authenticated_key,
        connector_limited_height,
    })
}

#[tauri::command]
pub async fn player_stop(services: State<'_, AppServices>) -> CmdResult<()> {
    services.stop_player().await;
    Ok(())
}

/// 볼륨(0~1) — 설정에도 저장한다.
#[tauri::command]
pub async fn player_set_volume(services: State<'_, AppServices>, volume: f32) -> CmdResult<()> {
    let volume = volume.clamp(0.0, 1.0);
    services.player.set_volume(volume);
    services.settings.update_player(|p| p.volume_level = volume);
    Ok(())
}

/// 플레이어 페이지 `postToSwift(type, data)` 메시지 (`{ type, data }`).
#[tauri::command]
pub async fn player_message(services: State<'_, AppServices>, message: Value) -> CmdResult<()> {
    log_player_page_message("viewer", &message);
    let Some((_, parsed)) = JsMessage::parse(&message, now_seconds()) else {
        return Ok(());
    };
    let signal = services.engine.handle_message(&parsed);
    if matches!(parsed, JsMessage::SurfaceFirstFrame { .. }) {
        services.player.engine_surface_presented();
    }
    if let Some(EngineSignal::Fatal(message)) = &signal {
        (services.sinks.player_event)(PlayerEventDto::Error {
            message: message.clone(),
        });
    }
    apply_engine_signal(&services.player, signal).await;
    Ok(())
}

/// 엔진 신호를 뷰모델에 넘긴다(단일 시청 화면·멀티라이브 패인 공용).
pub async fn apply_engine_signal(player: &PlayerViewModel, signal: Option<EngineSignal>) {
    match signal {
        Some(EngineSignal::Playing) => {
            player.engine_rebuffer_changed(false);
            player.engine_state_changed(&PlayerPhase::Playing).await;
        }
        Some(EngineSignal::Buffering) => player.engine_rebuffer_changed(true),
        Some(EngineSignal::Fatal(message)) => {
            tracing::warn!("hls.js 치명 오류: {message}");
            player
                .engine_state_changed(&PlayerPhase::Error(PlayerError::DecodingFailed(message)))
                .await;
        }
        None => {}
    }
}

pub fn now_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// 디버그 로그 — 플레이어 페이지의 오류·버퍼링·화질 신호(지표 메시지는 화면 수집기가 10초 표본으로 남긴다).
pub fn log_player_page_message(source: &str, message: &Value) {
    if !crate::perf_log::enabled() {
        return;
    }
    let kind = message.get("type").and_then(Value::as_str).unwrap_or("");
    if matches!(
        kind,
        "fatalError" | "error" | "playbackState" | "levels" | "forcedHeight" | "playStarted"
    ) {
        crate::perf_log::emit(
            "player.event",
            serde_json::json!({ "source": source, "type": kind, "data": message.get("data").cloned().unwrap_or(Value::Null) }),
        );
    }
}
