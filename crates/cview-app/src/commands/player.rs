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
use cview_player::coordinator::{InterfaceType, StreamPhase};
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
    start_player(&services, window.label(), channel_id).await
}

async fn start_player(
    services: &AppServices,
    window_label: &str,
    channel_id: String,
) -> CmdResult<PlaybackStartedDto> {
    let generation = services.begin_player_request();
    if !services.stop_player_for_request(generation).await {
        return Err(CommandError("재생 요청이 취소되었습니다".into()));
    }

    let info = services.api.live_detail(&channel_id, true).await?;
    if !services.is_current_player_request(generation) {
        return Err(CommandError("재생 요청이 취소되었습니다".into()));
    }
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
    let mut channel = services.player_channel.lock().await;
    if !services.is_current_player_request(generation) {
        return Err(CommandError("재생 요청이 취소되었습니다".into()));
    }
    // 최신 요청이 재생을 확정한 창만 명령을 받는다(보조 플레이어 창 포함).
    crate::windows::set_player_target(window_label);
    services
        .engine
        .set_policy_quality_ceiling_height(connector_limited_height);

    // 재생 시작 전에 등록해 둔다 — 첫 `watchActivity` 가 시작 명령보다 먼저 올 수 있다.
    services.watch_events.begin(
        crate::state::VIEWER_WATCH_SOURCE,
        &channel_id,
        Some(info.live_id),
    );
    services.player.start_stream(request).await;
    if !services.is_current_player_request(generation) {
        services.watch_events.end(crate::state::VIEWER_WATCH_SOURCE);
        services.player.stop_stream().await;
        return Err(CommandError("재생 요청이 취소되었습니다".into()));
    }
    let phase = services.player.read().stream_phase.clone();
    if let StreamPhase::Error(message) = phase {
        services.watch_events.end(crate::state::VIEWER_WATCH_SOURCE);
        services.player.stop_stream().await;
        return Err(CommandError(format!(
            "재생을 시작하지 못했습니다: {message}"
        )));
    }
    *channel = Some(channel_id.clone());

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
    drop(channel);

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
    if let JsMessage::WatchActivity { active, ended } = parsed {
        let source = crate::state::VIEWER_WATCH_SOURCE;
        if ended {
            // 방송 종료 화면에 머물러도 시청은 끝났다 — 다음 재생(`player_start`)이 새 세션을 연다.
            services.watch_events.end(source);
        } else {
            services.watch_events.set_active(source, active);
        }
    }
    if let Some(EngineSignal::Fatal(message)) = &signal {
        (services.sinks.player_event)(PlayerEventDto::Error {
            message: message.clone(),
        });
    }
    apply_engine_signal(&services.player, signal).await;
    Ok(())
}

/// 웹뷰의 `online`/`offline` — 단일 시청 화면과 멀티라이브 패인의 코디네이터에 경로 변화를 알린다.
///
/// 이 신호가 없으면 오프라인 → 온라인 복구를 코디네이터가 모른다. 그래서 재연결 중이면 남은 백오프
/// (최대 15초)를 다 기다렸고, 오래 끊겨 재연결이 소진(`Error`)됐으면 네트워크가 돌아와도 오류 카드에
/// 그대로 머물렀다. 경로 종류는 웹뷰가 알려 주지 않으므로 온라인은 `Other` 로 넘긴다 — 판정
/// ([`cview_player::coordinator::network_recovery_decision`])은 오프라인 ↔ 온라인 전이만 본다.
#[tauri::command]
pub async fn player_network_path(
    services: State<'_, AppServices>,
    shell: State<'_, crate::multi_live::MultiLiveShell>,
    online: bool,
) -> CmdResult<()> {
    let interface = if online {
        InterfaceType::Other
    } else {
        InterfaceType::Offline
    };
    tracing::info!(online, "네트워크 경로 변화");
    services.player.network_path_changed(interface);
    for pane in shell.registry.players() {
        pane.view_model().network_path_changed(interface);
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use cview_networking::transport::{HttpRequest, HttpResponse, HttpTransport, TransportError};
    use tokio::sync::Notify;

    struct EmptyWebView;

    #[async_trait]
    impl cview_auth::WebViewDataStore for EmptyWebView {
        async fn all_cookies(&self) -> Vec<cview_auth::StoredCookie> {
            Vec::new()
        }
        async fn delete_cookie(&self, _: &cview_auth::StoredCookie) {}
        async fn remove_all_website_data(&self) {}
    }

    #[derive(Default)]
    struct DelayedLiveApi {
        entered: Notify,
        release: Notify,
    }

    #[async_trait]
    impl HttpTransport for DelayedLiveApi {
        async fn send(&self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
            let slow = request.url.path().contains("/slow/");
            if slow {
                self.entered.notify_one();
                self.release.notified().await;
            }
            let channel = if slow { "slow" } else { "fast" };
            let playback = serde_json::json!({"media":[{
                "mediaId":"HLS", "protocol":"HLS", "path":format!("http://127.0.0.1:9/{channel}.m3u8")
            }]}).to_string();
            let response = serde_json::json!({"code":200,"content":{
                "liveId":1,"liveTitle":"재생 순서 검사","status":"OPEN","livePlaybackJson":playback,
                "channel":{"channelId":channel,"channelName":"테스트"}
            }});
            Ok(HttpResponse::new(
                200,
                serde_json::to_vec(&response).unwrap(),
            ))
        }
    }

    async fn services() -> (tempfile::TempDir, Arc<AppServices>, Arc<DelayedLiveApi>) {
        let dir = tempfile::tempdir().unwrap();
        let mut services = AppServices::initialize(
            dir.path(),
            "app.cview.test.live-order",
            Arc::new(EmptyWebView),
            crate::state::ShellSinks::noop(),
        )
        .await;
        let transport = Arc::new(DelayedLiveApi::default());
        services.api = cview_networking::ChzzkApiClient::new(cview_networking::ClientConfig::new(
            transport.clone(),
        ));
        (dir, Arc::new(services), transport)
    }

    #[tokio::test]
    async fn latest_channel_request_wins_when_live_detail_arrives_out_of_order() {
        let (_dir, services, api) = services().await;
        let slow = {
            let services = services.clone();
            tokio::spawn(async move { start_player(&services, "main", "slow".into()).await })
        };
        api.entered.notified().await;
        start_player(&services, "main", "fast".into())
            .await
            .unwrap();
        api.release.notify_one();
        let old = slow.await.unwrap();
        assert!(old.is_err(), "늦은 라이브 상세가 이전 채널을 다시 재생했다");
        assert_eq!(
            services.player_channel.lock().await.as_deref(),
            Some("fast")
        );
        assert_eq!(
            services.player.read().current_channel_id.as_deref(),
            Some("fast")
        );
        services.stop_player().await;
    }

    #[tokio::test]
    async fn closing_viewer_cancels_pending_live_detail() {
        let (_dir, services, api) = services().await;
        let pending = {
            let services = services.clone();
            tokio::spawn(async move { start_player(&services, "main", "slow".into()).await })
        };
        api.entered.notified().await;
        services.stop_player().await;
        api.release.notify_one();
        let old = pending.await.unwrap();
        assert!(old.is_err(), "시청 화면을 닫은 뒤 뒤늦게 재생을 시작했다");
        assert!(services.player_channel.lock().await.is_none());
        assert_eq!(services.player.read().stream_phase, StreamPhase::Idle);
        assert!(services.playing_stream.lock().unwrap().is_none());
        assert_eq!(services.watch_events.session_count(), 0);
    }
}
