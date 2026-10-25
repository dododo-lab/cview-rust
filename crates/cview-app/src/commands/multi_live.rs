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
use crate::multi_live::{
    add_result_message, focus_layout_update, layout_from_name, MultiLiveShell, MultiLiveStateDto,
};
use cview_app_core::viewmodels::multi_live::MultiLiveLayoutRatios;

fn parse_id(id: &str) -> CmdResult<Uuid> {
    Uuid::parse_str(id).map_err(|_| CommandError("세션 ID 가 올바르지 않습니다".into()))
}

#[tauri::command]
pub async fn multilive_state(shell: State<'_, MultiLiveShell>) -> CmdResult<MultiLiveStateDto> {
    Ok(shell.state_dto())
}

/// 채널을 패인으로 추가한다. 이미 있으면 그 패인을 선택한다. 단독 시청 화면은 멈추지 않는다 —
/// 소리는 화면이 `multilive_set_audio_suspended` 로 한쪽만 내게 한다.
#[tauri::command]
pub async fn multilive_add(
    shell: State<'_, MultiLiveShell>,
    channel_id: String,
) -> CmdResult<MultiLiveStateDto> {
    enforce_hub_mode(&shell);
    let result = shell
        .manager
        .add_session(&channel_id, AddSessionOptions::default(), None)
        .await;
    enforce_hub_mode(&shell);
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
    enforce_hub_mode(&shell);
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

/// 라이브 메뉴에서 이 세션만 보기(`Some`) / 멀티라이브로 돌아가기(`None`) — 원본 시청·멀티 허브 모드.
///
/// 같은 세션(플레이어·채팅 소켓)을 그대로 무대에 올리므로 다시 연결하지 않는다. 무대가 있으면 매니저의
/// 시청 디코드 규칙(`watch_mode_allows_decode`)이 무대·직전 무대·선택·PiP·소리 패인만 디코딩하고
/// 나머지는 로딩을 멈춘다. 돌아가면 모든 패인이 다시 디코딩한다(레이아웃은 그대로).
#[tauri::command]
pub async fn multilive_watch(
    shell: State<'_, MultiLiveShell>,
    session_id: Option<String>,
) -> CmdResult<MultiLiveStateDto> {
    {
        let mut st = shell.manager.lock();
        if let Some(id) = session_id.as_deref() {
            let id = parse_id(id)?;
            st.select_session(id, true);
            st.set_grid_layout(false);
            st.set_stage_session(Some(id));
        }
        shell
            .watching
            .store(session_id.is_some(), std::sync::atomic::Ordering::SeqCst);
    }
    enforce_hub_mode(&shell);
    shell.log_stage_transition(if session_id.is_some() {
        "watch"
    } else {
        "unwatch"
    });
    Ok(shell.state_dto())
}

/// 멀티라이브 패인을 잠시 접는다(`true`, 원본 `parkVideoSurfacesForResidentMenuLeave`) — 다른 채널을 라이브
/// 시청 화면에서 보는 동안. 일시정지 뒤 2.5초가 지나면 로딩·폴링·채팅까지 멈춘다. `false` 면 되살린다
/// (`restoreVideoSurfacesForMenuEnter`).
#[tauri::command]
pub async fn multilive_park(shell: State<'_, MultiLiveShell>, parked: bool) -> CmdResult<()> {
    {
        let mut st = shell.manager.lock();
        if parked {
            st.park_video_surfaces_for_resident_menu_leave();
        } else {
            st.restore_video_surfaces_for_menu_enter();
        }
    }
    shell.log_stage_transition(if parked { "park" } else { "unpark" });
    Ok(())
}

#[tauri::command]
pub async fn multilive_set_layout(
    shell: State<'_, MultiLiveShell>,
    layout: String,
) -> CmdResult<MultiLiveStateDto> {
    let mode = layout_from_name(&layout)
        .ok_or_else(|| CommandError(format!("알 수 없는 레이아웃: {layout}")))?;
    {
        let mut st = shell.manager.lock();
        st.apply_grid_layout_mode(mode);
        st.save_state();
    }
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
    enforce_hub_mode(&shell);
    Ok(shell.state_dto())
}

/// 멀티 허브 모드 규칙 — 앱이 시청 모드(`multilive_watch`)에 들어가 있지 않으면 그리드를 켜고 무대를 비운다.
/// 원본 매니저는 세션이 1개 이하가 되면 그리드를 끄고, 꺼진 상태에서 추가한 패인을 무대로 올린다(원본에선 단독
/// 무대가 자연스럽다). 이 앱은 라이브 메뉴로 들어갈 때만 시청 모드이므로, 그대로 두면 모두 닫고 다시 추가한
/// 첫 패인이 '라이브'로 바뀌고 다른 패인 디코드가 시청 규칙에 묶인다. 무대 세션이 닫혀도 시청 모드를 끝낸다.
fn enforce_hub_mode(shell: &MultiLiveShell) {
    let mut st = shell.manager.lock();
    if shell.watching.load(std::sync::atomic::Ordering::SeqCst) {
        let alive = st
            .stage_session_id()
            .is_some_and(|stage| st.sessions().iter().any(|s| s.id() == stage));
        if alive {
            return;
        }
        shell
            .watching
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }
    if st.stage_session_id().is_some() {
        st.set_stage_session(None);
    }
    if !st.is_grid_layout() {
        st.set_grid_layout(true);
    }
}

/// 패인 플레이어 페이지의 `postToSwift` 메시지.
#[tauri::command]
pub async fn multilive_player_message(
    shell: State<'_, MultiLiveShell>,
    slot: u32,
    message: Value,
) -> CmdResult<()> {
    crate::commands::player::log_player_page_message(&format!("pane{slot}"), &message);
    let Some((_, parsed)) = JsMessage::parse(&message, now_seconds()) else {
        return Ok(());
    };
    if let Some(player) = shell.registry.player_for_slot(slot) {
        player.handle_page_message(&parsed).await;
    }
    Ok(())
}

/// 포커스 레이아웃 — 메인 위치(`left`/`right`/`top`/`bottom`)와 메인 비율. 바꾼 뒤 저장한다.
#[tauri::command]
pub async fn multilive_set_focus_layout(
    shell: State<'_, MultiLiveShell>,
    direction: Option<String>,
    ratio: Option<f64>,
) -> CmdResult<MultiLiveStateDto> {
    let (direction, ratio) = focus_layout_update(direction.as_deref(), ratio);
    {
        let mut st = shell.manager.lock();
        if let Some(d) = direction {
            st.set_focus_direction(d);
        }
        if let Some(r) = ratio {
            let mut ratios: MultiLiveLayoutRatios = st.layout_ratios();
            ratios.focus_ratio = r;
            st.set_layout_ratios(ratios);
        }
        st.save_state();
    }
    Ok(shell.state_dto())
}

/// 단독 시청 화면이 재생하는 동안 멀티라이브 소리를 끈다(`true`). `false` 면 오디오 패인을 되살린다.
#[tauri::command]
pub async fn multilive_set_audio_suspended(
    shell: State<'_, MultiLiveShell>,
    suspended: bool,
) -> CmdResult<()> {
    let mut st = shell.manager.lock();
    if suspended {
        for s in st.sessions().to_vec() {
            s.set_muted(true);
        }
    } else if let Some(id) = st.audio_session_id().or(st.selected_session_id()) {
        st.route_audio(id);
    }
    Ok(())
}

/// 패인 다시 연결 — `session_id` 가 없으면 모든 패인(MUL-08, MUL-12).
#[tauri::command]
pub async fn multilive_retry(
    shell: State<'_, MultiLiveShell>,
    session_id: Option<String>,
) -> CmdResult<MultiLiveStateDto> {
    let targets: Vec<_> = {
        let st = shell.manager.read();
        match session_id.as_deref() {
            Some(id) => {
                let id = parse_id(id)?;
                st.session(id).cloned().into_iter().collect()
            }
            None => st.sessions().to_vec(),
        }
    };
    for s in targets {
        s.retry().await;
    }
    Ok(shell.state_dto())
}

/// 패인 끌어 놓기 — 대상 자리로 옮기거나 서로 바꾼다(MUL-07, 원본 MLTabReorderDropDelegate).
#[tauri::command]
pub async fn multilive_reorder(
    shell: State<'_, MultiLiveShell>,
    dragged_id: String,
    target_id: String,
) -> CmdResult<MultiLiveStateDto> {
    let dragged = parse_id(&dragged_id)?;
    let target = parse_id(&target_id)?;
    {
        let mut st = shell.manager.lock();
        let mut zoom = st.double_tap_zoom_session_id();
        st.drop_tile(dragged, target, &mut zoom);
    }
    Ok(shell.state_dto())
}

/// 다중 오디오 — `toggle_mode` 면 여러 패인 소리 모드를 켜고 끄고, 아니면 그 패인 소리를 토글(MUL-09).
#[tauri::command]
pub async fn multilive_audio(
    shell: State<'_, MultiLiveShell>,
    session_id: Option<String>,
    toggle_mode: bool,
) -> CmdResult<MultiLiveStateDto> {
    {
        let mut st = shell.manager.lock();
        if toggle_mode {
            st.toggle_multi_audio_mode();
        } else if let Some(id) = session_id.as_deref() {
            st.toggle_session_audio(parse_id(id)?);
        }
    }
    Ok(shell.state_dto())
}

/// 그리드 비율(사용자 배치) — 열·행 비율(0.2~0.8). 레이아웃을 사용자 지정으로 바꾸고 저장한다(MUL-06).
#[tauri::command]
pub async fn multilive_set_ratios(
    shell: State<'_, MultiLiveShell>,
    horizontal: Option<f64>,
    vertical: Option<f64>,
) -> CmdResult<MultiLiveStateDto> {
    {
        let mut st = shell.manager.lock();
        let mut r: MultiLiveLayoutRatios = st.layout_ratios();
        if let Some(h) = horizontal.filter(|v| v.is_finite()) {
            r.horizontal_ratio = MultiLiveLayoutRatios::clamped(h);
        }
        if let Some(v) = vertical.filter(|v| v.is_finite()) {
            r.vertical_ratio = MultiLiveLayoutRatios::clamped(v);
        }
        st.set_layout_ratios(r);
        st.apply_grid_layout_mode(
            cview_app_core::viewmodels::multi_live::MultiLiveGridLayoutMode::Custom,
        );
        st.save_state();
    }
    Ok(shell.state_dto())
}

// MARK: - 채널 프리셋 (MUL-14, 원본 ChannelPresetPicker)

fn preset_store(
    app: &tauri::AppHandle,
) -> CmdResult<cview_app_core::services::channel_preset_store::ChannelPresetStore> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| CommandError(e.to_string()))?;
    Ok(
        cview_app_core::services::channel_preset_store::ChannelPresetStore::new(
            cview_app_core::services::channel_preset_store::ChannelPresetStore::default_path(&dir),
        ),
    )
}

#[tauri::command]
pub async fn preset_list(
    app: tauri::AppHandle,
) -> CmdResult<Vec<cview_app_core::services::channel_preset_store::Preset>> {
    preset_store(&app)?
        .load_all()
        .map_err(|e| CommandError(e.to_string()))
}

#[tauri::command]
pub async fn preset_save(
    app: tauri::AppHandle,
    name: String,
    channels: Vec<String>,
) -> CmdResult<cview_app_core::services::channel_preset_store::Preset> {
    use cview_app_core::services::channel_preset_store::{Preset, PresetKind};
    let name = name.trim();
    if name.is_empty() || channels.is_empty() {
        return Err(CommandError("이름과 채널이 필요합니다".into()));
    }
    let preset = Preset::new(name, channels, PresetKind::Live);
    preset_store(&app)?
        .save(&preset)
        .map_err(|e| CommandError(e.to_string()))?;
    Ok(preset)
}

#[tauri::command]
pub async fn preset_delete(app: tauri::AppHandle, id: String) -> CmdResult<()> {
    preset_store(&app)?
        .delete(parse_id(&id)?)
        .map_err(|e| CommandError(e.to_string()))
}
