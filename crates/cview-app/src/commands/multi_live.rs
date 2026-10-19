//! 멀티라이브 명령 — 세션 추가/제거/선택, 레이아웃, 스테이지 크기, 패인 플레이어 메시지.
//!
//! 상태는 `multilive://state`(`MultiLiveStateDto`)로 흘려보낸다(`lib.rs` 의 감시 작업). 명령은
//! 바뀐 상태를 바로 돌려줘 프런트엔드가 이벤트를 기다리지 않아도 된다.

use cview_app_core::viewmodels::multi_live::AddSessionOptions;
use cview_player::hlsjs::JsMessage;
use serde_json::Value;
use tauri::State;
use uuid::Uuid;

use super::player::now_seconds;
use super::{CmdResult, CommandError};
use crate::multi_live::{add_result_message, layout_from_name, MultiLiveShell, MultiLiveStateDto};
use crate::state::AppServices;

fn parse_id(id: &str) -> CmdResult<Uuid> {
    Uuid::parse_str(id).map_err(|_| CommandError("세션 ID 가 올바르지 않습니다".into()))
}

#[tauri::command]
pub async fn multilive_state(shell: State<'_, MultiLiveShell>) -> CmdResult<MultiLiveStateDto> {
    Ok(shell.state_dto())
}

/// 채널을 패인으로 추가한다. 이미 있으면 그 패인을 선택한다.
#[tauri::command]
pub async fn multilive_add(
    shell: State<'_, MultiLiveShell>,
    services: State<'_, AppServices>,
    channel_id: String,
) -> CmdResult<MultiLiveStateDto> {
    // 단일 시청 화면과 함께 소리가 겹치지 않게 멈춘다.
    services.stop_player().await;
    let result = shell
        .manager
        .add_session(&channel_id, AddSessionOptions::default(), None)
        .await;
    tracing::info!(channel_id = %channel_id, ?result, "멀티라이브 세션 추가");
    if let Some(message) = add_result_message(&result) {
        return Err(CommandError(message));
    }
    Ok(shell.state_dto())
}

#[tauri::command]
pub async fn multilive_remove(
    shell: State<'_, MultiLiveShell>,
    session_id: String,
) -> CmdResult<MultiLiveStateDto> {
    let id = parse_id(&session_id)?;
    shell.manager.remove_session(id).await;
    shell.prune_registry();
    Ok(shell.state_dto())
}

/// 패인 선택 — 오디오·채팅·고화질이 이 패인을 따라간다.
#[tauri::command]
pub async fn multilive_select(
    shell: State<'_, MultiLiveShell>,
    session_id: String,
) -> CmdResult<MultiLiveStateDto> {
    let id = parse_id(&session_id)?;
    shell.manager.lock().select_session(id, true);
    Ok(shell.state_dto())
}

#[tauri::command]
pub async fn multilive_set_layout(
    shell: State<'_, MultiLiveShell>,
    layout: String,
) -> CmdResult<MultiLiveStateDto> {
    let mode = layout_from_name(&layout)
        .ok_or_else(|| CommandError(format!("알 수 없는 레이아웃: {layout}")))?;
    shell.manager.lock().apply_grid_layout_mode(mode);
    Ok(shell.state_dto())
}

/// 멀티라이브 무대(패인 영역) 크기 — 패인 크기 기반 화질 상한에 쓴다.
#[tauri::command]
pub async fn multilive_stage_size(
    shell: State<'_, MultiLiveShell>,
    width: f64,
    height: f64,
) -> CmdResult<()> {
    if width > 0.0 && height > 0.0 {
        let mut st = shell.manager.lock();
        st.report_stage_size((width, height));
        st.update_pane_sizes(width.round() as i64, height.round() as i64);
    }
    Ok(())
}

/// 전부 닫는다.
#[tauri::command]
pub async fn multilive_stop_all(shell: State<'_, MultiLiveShell>) -> CmdResult<MultiLiveStateDto> {
    shell.manager.stop_all().await;
    shell.prune_registry();
    Ok(shell.state_dto())
}

/// 패인 플레이어 페이지의 `postToSwift` 메시지.
#[tauri::command]
pub async fn multilive_player_message(
    shell: State<'_, MultiLiveShell>,
    slot: u32,
    message: Value,
) -> CmdResult<()> {
    let Some((_, parsed)) = JsMessage::parse(&message, now_seconds()) else {
        return Ok(());
    };
    if let Some(player) = shell.registry.player_for_slot(slot) {
        player.handle_page_message(&parsed).await;
    }
    Ok(())
}
