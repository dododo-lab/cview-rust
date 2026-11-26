//! 설정 읽기/쓰기 — `SettingsStore` 의 도메인을 JSON 그대로 주고받는다.
//!
//! 쓰기는 부분 패치다: 현재 도메인 JSON 위에 받은 키만 덮어쓴 뒤 도메인 타입으로 다시 해석한다.
//! 해석에 실패하면(잘못된 값) 아무것도 바꾸지 않는다.

use cview_core::models::settings::{
    AppTheme, AppearanceSettings, GeneralSettings, NetworkPreset, NetworkSettings, PlayerSettings,
};
use cview_core::models::settings_ui::{
    ChannelNotificationSettings, KeyboardShortcutSettings, MultiChatSettings, MultiLiveSettings,
    SyncSettings,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use tauri::State;

use super::{CmdResult, CommandError};
use crate::multi_live::{MultiLiveShell, MultiLiveStateDto, PaneStateDto};
use crate::state::AppServices;

/// 프런트가 다루는 설정 도메인.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    pub player: Value,
    pub general: Value,
    pub appearance: Value,
    pub chat: Value,
    pub network: Value,
    pub multi_live: Value,
    pub multi_chat: Value,
    pub keyboard: Value,
    pub channel_notifications: Value,
    pub sync: Value,
    /// 지금 값에 맞는 네트워크 프리셋(`auto` 면 자동).
    pub network_preset: NetworkPreset,
    /// 자동 프리셋이 지금 넣어 둔 프리셋(자동이 꺼져 있으면 없음).
    pub network_auto_choice: Option<NetworkPreset>,
}

/// 도메인이 아닌, 현재 값에서 계산해 덧붙이는 DTO 키 — 백업에는 넣지 않는다.
const DERIVED_KEYS: &[&str] = &["networkPreset", "networkAutoChoice"];

/// 설정 도메인 이름(프런트·백업 파일 키).
pub const DOMAINS: &[&str] = &[
    "player",
    "general",
    "appearance",
    "chat",
    "network",
    "multiLive",
    "multiChat",
    "keyboard",
    "channelNotifications",
    "sync",
];

/// 객체 키 단위 얕은 병합. 패치가 객체가 아니면 패치로 통째로 바꾼다.
pub fn merge_patch(current: Value, patch: Value) -> Value {
    match (current, patch) {
        (Value::Object(mut base), Value::Object(patch)) => {
            for (k, v) in patch {
                base.insert(k, v);
            }
            Value::Object(base)
        }
        (_, patch) => patch,
    }
}

/// 현재 값에 패치를 입혀 타입으로 다시 해석한다.
pub fn apply_patch<T: Serialize + DeserializeOwned>(
    current: &T,
    patch: Value,
) -> Result<T, String> {
    let base = serde_json::to_value(current).map_err(|e| e.to_string())?;
    serde_json::from_value(merge_patch(base, patch)).map_err(|e| format!("잘못된 설정 값: {e}"))
}

fn to_value<T: Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

pub(crate) fn snapshot(services: &AppServices) -> SettingsDto {
    let s = &services.settings;
    SettingsDto {
        player: to_value(&s.player()),
        general: to_value(&s.general()),
        appearance: to_value(&s.appearance()),
        chat: s.chat(),
        network: to_value(&s.network()),
        multi_live: to_value(&s.multi_live()),
        multi_chat: to_value(&s.multi_chat()),
        keyboard: to_value(&s.keyboard()),
        channel_notifications: to_value(&s.channel_notifications()),
        sync: to_value(&s.sync()),
        network_preset: s.network().matching_preset(),
        network_auto_choice: auto_network_choice(&s.network()),
    }
}

#[tauri::command]
pub async fn settings_get(services: State<'_, AppServices>) -> CmdResult<SettingsDto> {
    Ok(snapshot(&services))
}

/// 패치를 입힐 바탕 — 화면의 부분 저장은 지금 값, 백업 가져오기는 기본값.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PatchBase {
    Current,
    /// 백업에 없는 키(끔·기본이라 내보내기에서 빠진 이퀄라이저·화면 비율, 채팅의 기본값 키 등)는 기본값으로
    /// 돌아가야 백업 상태가 그대로 복원된다. 예전엔 지금 값 위에 얕게 병합해 그런 값이 그대로 남았다.
    Defaults,
}

/// `base` 에 따라 지금 값 또는 기본값 위에 패치를 입힌다.
fn patched<T: Serialize + DeserializeOwned + Default>(
    base: PatchBase,
    current: impl FnOnce() -> T,
    patch: Value,
) -> Result<T, String> {
    match base {
        PatchBase::Current => apply_patch(&current(), patch),
        PatchBase::Defaults => apply_patch(&T::default(), patch),
    }
}

/// 도메인 하나에 패치를 입힌다(`DOMAINS`). 네트워크 설정은 API 클라이언트에도 반영한다.
fn apply_domain(services: &AppServices, domain: &str, patch: Value) -> CmdResult<()> {
    apply_domain_on(services, domain, patch, PatchBase::Current)
}

fn apply_domain_on(
    services: &AppServices,
    domain: &str,
    patch: Value,
    base: PatchBase,
) -> CmdResult<()> {
    let s = &services.settings;
    match domain {
        "player" => s.set_player(patched::<PlayerSettings>(base, || s.player(), patch)?),
        // 고지 동의 기록은 notice_accept 만 바꾼다 — 설정 저장·가져오기가 지우거나 덮어쓰지 않는다.
        "general" => {
            let current = s.general();
            let next = patched::<GeneralSettings>(base, || current.clone(), patch)?;
            s.set_general(next.with_notice_from(&current));
        }
        "appearance" => s.set_appearance(patched::<AppearanceSettings>(
            base,
            || s.appearance(),
            patch,
        )?),
        "chat" => s.set_chat(match (base, patch) {
            // 가져오기 — 채팅은 화면이 바꾼 키만 저장하므로 백업 객체가 곧 그 상태다(나머지는 화면 기본값).
            (PatchBase::Defaults, patch @ Value::Object(_)) => patch,
            (_, patch) => merge_patch(s.chat(), patch),
        }),
        "network" => {
            // 값 하나를 손으로 바꾸면 자동 프리셋은 꺼진다(백업 복원처럼 `autoPreset` 을 함께 보내면 그 값을 따른다).
            let manual = patch.get("autoPreset").is_none();
            let mut next = patched::<NetworkSettings>(base, || s.network(), patch)?;
            if manual {
                next.auto_preset = false;
            }
            s.set_network(next);
            services.api.update_network_settings(&s.network());
        }
        "multiLive" => s.set_multi_live(patched::<MultiLiveSettings>(
            base,
            || s.multi_live(),
            patch,
        )?),
        "multiChat" => s.set_multi_chat(patched::<MultiChatSettings>(
            base,
            || s.multi_chat(),
            patch,
        )?),
        "keyboard" => s.set_keyboard(patched::<KeyboardShortcutSettings>(
            base,
            || s.keyboard(),
            patch,
        )?),
        "channelNotifications" => {
            s.set_channel_notifications(patched::<ChannelNotificationSettings>(
                base,
                || s.channel_notifications(),
                patch,
            )?)
        }
        "sync" => s.set_sync(patched::<SyncSettings>(base, || s.sync(), patch)?),
        other => return Err(CommandError(format!("알 수 없는 설정 영역: {other}"))),
    }
    Ok(())
}

/// 도메인 값이 바뀐 뒤 앱(창·메뉴·트레이·네트워크)에 반영한다 — 화면 저장·백업 가져오기·초기화가 함께 쓴다.
/// 예전엔 화면 저장만 일반(항상 위·로그인 시 실행·메뉴 막대 아이콘)·단축키(네이티브 메뉴)를 반영해, 초기화나
/// 가져오기 뒤에는 화면만 바뀌고 창은 계속 항상 위·LaunchAgent 는 남는 식으로 어긋났다.
fn after_domain_change(
    app: &tauri::AppHandle,
    services: &AppServices,
    shell: &MultiLiveShell,
    domain: &str,
) {
    match domain {
        // 멀티라이브 상한이 바뀌면 보이는 패인·디코드 예산·패인 크기를 바로 다시 맞춘다(화면과 재생이 어긋나지 않게).
        "multiLive" => shell.manager.lock().multi_live_limits_changed(),
        "appearance" => {
            appearance_changed(app, services);
            // 설정 "디버그 로그"를 켜고 끄면 바로 반영하고 화면 수집기에 알린다.
            if let Ok(dir) = tauri::Manager::path(app).app_data_dir() {
                let on =
                    crate::perf_log::forced_by_env() || services.settings.appearance().debug_mode;
                crate::perf_log::set_enabled(&dir, on);
                let _ = tauri::Emitter::emit(app, "perf://enabled", on);
            }
        }
        "general" => {
            crate::window_prefs::apply_general(app, &services.settings.general());
            // "메뉴 막대에 표시" — 아이콘을 바로 보이거나 숨긴다. 팝오버의 알림 토글도 함께 맞춘다.
            crate::tray::sync(app);
            let _ = tauri::Emitter::emit(
                app,
                "settings://general",
                to_value(&services.settings.general()),
            );
        }
        "keyboard" => {
            crate::menu::rebuild(app);
            // 모든 창(따로 연 플레이어 창 등)의 맨 글자 단축키도 바로 바꾼다 — 예전엔 메뉴 가속기만 바뀌고,
            // 다른 창은 연 때 읽은 단축키를 그대로 써서 메뉴 동작과 맨 글자 동작이 어긋났다.
            let _ = tauri::Emitter::emit(
                app,
                "settings://keyboard",
                to_value(&services.settings.keyboard()),
            );
        }
        "network" => services
            .api
            .update_network_settings(&services.settings.network()),
        _ => {}
    }
}

/// `domain` 은 [`DOMAINS`] 중 하나.
#[tauri::command]
pub async fn settings_update(
    app: tauri::AppHandle,
    services: State<'_, AppServices>,
    shell: State<'_, MultiLiveShell>,
    domain: String,
    patch: Value,
) -> CmdResult<SettingsDto> {
    apply_domain(&services, &domain, patch)?;
    after_domain_change(&app, &services, &shell, &domain);
    Ok(snapshot(&services))
}

/// 처음 실행 고지에 동의했다 — 동의한 문구 버전과 때를 일반 설정에 남긴다(낮은 버전으로는 되돌리지 않는다).
/// 화면은 다음 실행부터 `settings_get` 의 `general.noticeAcceptedVersion` 으로 다시 물을지 정한다.
#[tauri::command]
pub async fn notice_accept(
    services: State<'_, AppServices>,
    version: u32,
) -> CmdResult<SettingsDto> {
    if version == 0 {
        return Err(CommandError("고지 버전이 올바르지 않습니다".into()));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default();
    services
        .settings
        .update_general(|g| record_notice(g, version, now));
    Ok(snapshot(&services))
}

/// 동의 기록 — 이미 더 높은 버전에 동의했으면 그대로 둔다. 순수 함수.
fn record_notice(g: &mut GeneralSettings, version: u32, now_ms: i64) {
    if version >= g.notice_accepted_version {
        g.notice_accepted_version = version;
        g.notice_accepted_at = Some(now_ms);
    }
}

/// 화면 크기를 걸 창인가 — 네이버 로그인 창(외부 nid.naver.com)과 고정 크기 메뉴 막대 팝오버는 제외한다.
pub fn zoom_applies_to(label: &str) -> bool {
    label != crate::login::LOGIN_WINDOW && label != crate::tray_popover::TRAY_POPOVER_WINDOW
}

/// 이 창에 저장된 화면 크기를 건다 — 창을 만든 직후(메인 창 setup·보조 창 window_open)에 불러
/// 100% 로 먼저 그렸다가 튀지 않게 한다. 로그인 창은 건너뛴다.
pub fn apply_saved_zoom<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>, scale: f64) {
    if !zoom_applies_to(window.label()) {
        return;
    }
    if let Err(e) = window.set_zoom(scale) {
        tracing::debug!("화면 크기 적용 실패({}): {e}", window.label());
    }
}

/// 화면 크기 — 열린 모든 창(로그인 창 제외)의 웹뷰 확대 배율을 맞춘다(설정은 앱 전체 하나).
pub fn apply_ui_zoom_all(app: &tauri::AppHandle, scale: f64) {
    for w in tauri::Manager::webview_windows(app).values() {
        apply_saved_zoom(w, scale);
    }
}

/// 마지막으로 창에 건 테마와 다른가 — 화면 크기 슬라이더처럼 테마 밖 외관 저장마다 Dock 아이콘을 다시 그리지 않게.
fn theme_changed_since_last(theme: AppTheme) -> bool {
    static LAST: std::sync::Mutex<Option<AppTheme>> = std::sync::Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if *last == Some(theme) {
        return false;
    }
    *last = Some(theme);
    true
}

/// 외관 설정이 바뀌었다(저장·확대 단계·가져오기·초기화) — 모든 창에 확대를 걸고, 모든 창에 새 외관 값을 알린다.
/// 화면(prefs-effects.js)은 썸네일 배율·모션 등급을, 설정 화면은 '화면 크기' 슬라이더를 이 값으로 맞춘다.
fn appearance_changed(app: &tauri::AppHandle, services: &AppServices) {
    let appearance = services.settings.appearance();
    apply_ui_zoom_all(app, appearance.ui_scale);
    // 창 테마·배경·Dock 아이콘도 — 예전엔 테마 단추(appearance_set_theme)만 걸어, 설정 가져오기·초기화 뒤 웹 화면과
    // 창 제목 막대·Dock 아이콘·`prefers-color-scheme` 이 서로 다른 테마로 남았다.
    if theme_changed_since_last(appearance.theme) {
        apply_window_theme(app, appearance.theme);
    }
    // 사용률 측정 켜기·간격·기간(설정 › 성능) — 같은 값이면 표본 스레드는 그대로다.
    if let Some(monitor) =
        tauri::Manager::try_state::<std::sync::Arc<crate::usage::UsageMonitor>>(app)
    {
        monitor.configure(crate::usage::UsageConfig::from_appearance(&appearance));
    }
    let _ = tauri::Emitter::emit(
        app,
        super::events::APPEARANCE_CHANGED,
        serde_json::to_value(&appearance).unwrap_or(Value::Null),
    );
}

/// 창이 처음 뜰 때 화면(prefs-effects.js)이 부른다 — 이 창에 저장된 화면 크기를 건다.
#[tauri::command]
pub async fn ui_zoom_sync(
    window: tauri::WebviewWindow,
    services: State<'_, AppServices>,
) -> CmdResult<f64> {
    let scale = services.settings.appearance().ui_scale;
    if zoom_applies_to(window.label()) {
        window
            .set_zoom(scale)
            .map_err(|e| CommandError(e.to_string()))?;
    }
    Ok(scale)
}

/// 메뉴 "확대·축소·실제 크기"(⌘= ⌘- ⌘0) — 단계를 옮겨 저장하고 모든 창에 건다.
#[tauri::command]
pub async fn ui_zoom_step(
    app: tauri::AppHandle,
    services: State<'_, AppServices>,
    dir: i32,
) -> CmdResult<SettingsDto> {
    use cview_core::models::settings::ui_scale;
    let mut a = services.settings.appearance();
    a.ui_scale = ui_scale::step(a.ui_scale, dir);
    services.settings.set_appearance(a);
    appearance_changed(&app, &services);
    Ok(snapshot(&services))
}

/// 네트워크 프리셋 적용(원본 NetworkPreset) — `balanced` / `stability` / `lowLatency` / `performance`.
#[tauri::command]
pub async fn network_preset(
    services: State<'_, AppServices>,
    shell: State<'_, MultiLiveShell>,
    preset: NetworkPreset,
) -> CmdResult<SettingsDto> {
    let s = &services.settings;
    s.set_network(preset.settings());
    if preset == NetworkPreset::Auto {
        apply_auto_network(&services, &shell);
    } else {
        services.api.update_network_settings(&s.network());
    }
    Ok(snapshot(&services))
}

/// 재생 오류가 사라진 뒤에도 안정 우선을 유지하는 시간 — 오류·복구가 오가며 프리셋이 흔들리지 않게.
const AUTO_STABILITY_HOLD: std::time::Duration = std::time::Duration::from_secs(90);

/// 마지막으로 재생 오류를 본 시각.
static LAST_UNSTABLE: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// 자동 프리셋이 켜져 있으면 멀티라이브 상황으로 프리셋을 골라 넣는다. 바꿨으면 `true`.
pub fn apply_auto_network(services: &AppServices, shell: &MultiLiveShell) -> bool {
    if !services.settings.network().auto_preset {
        return false;
    }
    apply_auto_network_for(services, &shell.state_dto())
}

/// [`apply_auto_network`] — 이미 만든 멀티라이브 상태로. 상태 감시 루프가 보낼 상태를 한 번만 만들게(예전엔 초마다
/// 두 번 — 패인마다 문자열 수십 개·여러 겹 잠금).
pub fn apply_auto_network_for(services: &AppServices, state: &MultiLiveStateDto) -> bool {
    let s = &services.settings;
    let current = s.network();
    if !current.auto_preset {
        return false;
    }
    let live = |p: &&crate::multi_live::MultiLivePaneDto| {
        !p.decode_suspended && matches!(p.state, PaneStateDto::Playing | PaneStateDto::Loading)
    };
    let active = state.sessions.iter().filter(live).count();
    let erroring = state
        .sessions
        .iter()
        .any(|p| matches!(p.state, PaneStateDto::Error { .. }));
    let unstable = {
        let mut last = LAST_UNSTABLE.lock().unwrap_or_else(|e| e.into_inner());
        if erroring {
            *last = Some(std::time::Instant::now());
        }
        last.is_some_and(|t| t.elapsed() < AUTO_STABILITY_HOLD)
    };
    let choice = NetworkPreset::auto_choice(active, unstable);
    let next = NetworkSettings {
        auto_preset: true,
        ..choice.settings()
    };
    if next == current {
        return false;
    }
    tracing::info!(?choice, active, unstable, "자동 네트워크 프리셋 전환");
    s.set_network(next);
    services.api.update_network_settings(&s.network());
    true
}

/// 자동 프리셋이 지금 고른 프리셋(자동이 꺼져 있으면 `None`).
pub fn auto_network_choice(settings: &NetworkSettings) -> Option<NetworkPreset> {
    let mut plain = settings.clone();
    if !std::mem::take(&mut plain.auto_preset) {
        return None;
    }
    Some(plain.matching_preset())
}

/// 설정 내보내기 — 모든 도메인을 JSON 문자열로(백업, SET-17).
#[tauri::command]
pub async fn settings_export(services: State<'_, AppServices>) -> CmdResult<String> {
    let mut v =
        serde_json::to_value(snapshot(&services)).map_err(|e| CommandError(e.to_string()))?;
    if let Value::Object(map) = &mut v {
        for k in DERIVED_KEYS {
            map.remove(*k);
        }
        map.insert("cviewSettingsVersion".into(), Value::from(1));
    }
    serde_json::to_string_pretty(&v).map_err(|e| CommandError(e.to_string()))
}

/// 설정 가져오기 — 내보낸 JSON 을 도메인별로 덮어쓴다. 잘못된 도메인 값은 건너뛰고 이름을 돌려준다.
#[tauri::command]
pub async fn settings_import(
    app: tauri::AppHandle,
    services: State<'_, AppServices>,
    shell: State<'_, MultiLiveShell>,
    json: String,
) -> CmdResult<Vec<String>> {
    let root: Value = serde_json::from_str(&json)
        .map_err(|e| CommandError(format!("백업 파일이 올바르지 않습니다: {e}")))?;
    let Value::Object(map) = root else {
        return Err(CommandError("백업 파일 형식이 아닙니다".into()));
    };
    let mut skipped = Vec::new();
    for domain in DOMAINS {
        if let Some(v) = map.get(*domain) {
            if apply_domain_on(&services, domain, v.clone(), PatchBase::Defaults).is_err() {
                skipped.push((*domain).to_string());
            } else {
                // 가져온 값도 저장·초기화처럼 바로 반영한다(화면 크기·멀티라이브 상한·항상 위·단축키 메뉴 등).
                after_domain_change(&app, &services, &shell, domain);
            }
        }
    }
    Ok(skipped)
}

/// 도메인 초기화 — 기본값으로 되돌린다(`all` 이면 전부).
#[tauri::command]
pub async fn settings_reset(
    app: tauri::AppHandle,
    services: State<'_, AppServices>,
    shell: State<'_, MultiLiveShell>,
    domain: String,
) -> CmdResult<SettingsDto> {
    let s = &services.settings;
    let reset = |d: &str| match d {
        // 선명도는 명시적 끔 — 기본값(None)이면 화면이 예전 값에서 다시 이전해 초기화가 먹지 않는다.
        "player" => s.set_player(PlayerSettings::reset_defaults()),
        "general" => s.set_general(GeneralSettings::default().with_notice_from(&s.general())),
        "appearance" => s.set_appearance(AppearanceSettings::default()),
        "chat" => s.set_chat(Value::Object(Default::default())),
        "network" => s.set_network(NetworkSettings::default()),
        "multiLive" => s.set_multi_live(MultiLiveSettings::default()),
        "multiChat" => s.set_multi_chat(MultiChatSettings::default()),
        "keyboard" => s.set_keyboard(KeyboardShortcutSettings::default()),
        "channelNotifications" => {
            s.set_channel_notifications(ChannelNotificationSettings::default())
        }
        "sync" => s.set_sync(SyncSettings::default()),
        _ => {}
    };
    if domain == "all" {
        for d in DOMAINS {
            reset(d);
        }
    } else if DOMAINS.contains(&domain.as_str()) {
        reset(&domain);
    } else {
        return Err(CommandError(format!("알 수 없는 설정 영역: {domain}")));
    }
    for d in DOMAINS.iter().filter(|d| domain == "all" || **d == domain) {
        after_domain_change(&app, &services, &shell, d);
    }
    Ok(snapshot(&services))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_restores_keys_missing_from_the_backup_to_defaults() {
        // 이퀄라이저·화면 비율이 꺼져 있던 백업(키 없음) — 지금 켜 둔 값이 남지 않고 기본값(끔)으로 돌아간다.
        let current = PlayerSettings {
            equalizer_preset: Some("Rock".into()),
            aspect_ratio: Some("fill".into()),
            ..PlayerSettings::default()
        };
        let backup = serde_json::to_value(PlayerSettings::default()).unwrap();
        assert!(backup.get("equalizerPreset").is_none());
        let kept: PlayerSettings =
            patched(PatchBase::Current, || current.clone(), backup.clone()).unwrap();
        assert_eq!(kept.equalizer_preset.as_deref(), Some("Rock"));
        let restored: PlayerSettings =
            patched(PatchBase::Defaults, || current.clone(), backup).unwrap();
        assert_eq!(restored.equalizer_preset, None);
        assert_eq!(restored.aspect_ratio, None);
    }

    #[test]
    fn notice_record_keeps_the_highest_version() {
        let mut g = GeneralSettings::default();
        assert_eq!(g.notice_accepted_version, 0);
        assert_eq!(g.notice_accepted_at, None);
        record_notice(&mut g, 1, 1_000);
        assert_eq!(
            (g.notice_accepted_version, g.notice_accepted_at),
            (1, Some(1_000))
        );
        record_notice(&mut g, 2, 2_000);
        assert_eq!(
            (g.notice_accepted_version, g.notice_accepted_at),
            (2, Some(2_000))
        );
        // 옛 화면이 낮은 버전으로 다시 동의해도 새 동의를 지우지 않는다.
        record_notice(&mut g, 1, 3_000);
        assert_eq!(
            (g.notice_accepted_version, g.notice_accepted_at),
            (2, Some(2_000))
        );
    }

    #[test]
    fn settings_patch_and_import_never_touch_notice_consent() {
        let current = GeneralSettings {
            notice_accepted_version: 1,
            notice_accepted_at: Some(5),
            ..GeneralSettings::default()
        };
        // 화면 저장(Current)·백업 가져오기(Defaults) 모두 동의 기록은 지금 값을 둔다.
        for base in [PatchBase::Current, PatchBase::Defaults] {
            let patch = serde_json::json!({ "noticeAcceptedVersion": 0, "noticeAcceptedAt": null, "alwaysOnTop": true });
            let next = patched::<GeneralSettings>(base, || current.clone(), patch)
                .unwrap()
                .with_notice_from(&current);
            assert!(next.always_on_top);
            assert_eq!(next.notice_accepted_version, 1);
            assert_eq!(next.notice_accepted_at, Some(5));
        }
        // 초기화도 마찬가지.
        let reset = GeneralSettings::default().with_notice_from(&current);
        assert_eq!(reset.notice_accepted_version, 1);
        // 화면은 필드가 늘 있다고 보고 판단한다(없으면 옛 백엔드로 보고 묻지 않는다) — 직렬화에 꼭 들어간다.
        let v = serde_json::to_value(GeneralSettings::default()).unwrap();
        assert_eq!(v["noticeAcceptedVersion"], serde_json::json!(0));
        assert!(v.get("noticeAcceptedAt").is_some());
    }

    #[test]
    fn login_and_popover_windows_are_never_zoomed() {
        assert!(!zoom_applies_to(crate::login::LOGIN_WINDOW));
        assert!(!zoom_applies_to(crate::tray_popover::TRAY_POPOVER_WINDOW));
        assert!(zoom_applies_to("main"));
        assert!(zoom_applies_to("chat-1"));
    }

    #[test]
    fn domains_cover_dto_fields() {
        let dto = SettingsDto {
            player: Value::Null,
            general: Value::Null,
            appearance: Value::Null,
            chat: Value::Null,
            network: Value::Null,
            multi_live: Value::Null,
            multi_chat: Value::Null,
            keyboard: Value::Null,
            channel_notifications: Value::Null,
            sync: Value::Null,
            network_preset: NetworkPreset::Balanced,
            network_auto_choice: None,
        };
        let v = serde_json::to_value(dto).unwrap();
        let keys: Vec<_> = v
            .as_object()
            .unwrap()
            .keys()
            .filter(|k| !DERIVED_KEYS.contains(&k.as_str()))
            .cloned()
            .collect();
        for d in DOMAINS {
            assert!(keys.iter().any(|k| k == d), "DTO 에 {d} 없음");
        }
        assert_eq!(keys.len(), DOMAINS.len());
    }

    #[test]
    fn theme_maps_to_window_theme() {
        assert_eq!(window_theme(AppTheme::System), None);
        assert_eq!(window_theme(AppTheme::Light), Some(tauri::Theme::Light));
        assert_eq!(window_theme(AppTheme::Dark), Some(tauri::Theme::Dark));
    }
    use serde_json::json;

    #[test]
    fn merge_is_shallow_and_keeps_other_keys() {
        let merged = merge_patch(
            json!({"a": 1, "b": {"x": 1}}),
            json!({"b": {"y": 2}, "c": 3}),
        );
        assert_eq!(merged, json!({"a": 1, "b": {"y": 2}, "c": 3}));
        assert_eq!(merge_patch(json!({"a": 1}), json!(5)), json!(5));
    }

    #[test]
    fn patch_player_settings() {
        let base = PlayerSettings::default();
        let encoded = serde_json::to_value(&base).unwrap();
        assert!(
            encoded.get("lowLatencyMode").is_some(),
            "PlayerSettings 는 camelCase 로 직렬화된다"
        );
        let patched: PlayerSettings =
            apply_patch(&base, json!({ "lowLatencyMode": !base.low_latency_mode })).unwrap();
        assert_eq!(patched.low_latency_mode, !base.low_latency_mode);
        assert_eq!(patched.volume_level, base.volume_level);
        assert!(apply_patch::<PlayerSettings>(&base, json!({"volumeLevel": "loud"})).is_err());
    }

    #[test]
    fn patch_sharpness_settings() {
        // 화면(sharpness.js)이 보내는 선명도 패치 — 값은 0..=3 으로 맞추고, null 은 '다시 이전하라'(None).
        let base = PlayerSettings::default();
        let set: PlayerSettings = apply_patch(
            &base,
            json!({ "sharpnessLevel": 3, "sharpnessEnlargedOnly": true, "videoUpscaling": true }),
        )
        .unwrap();
        assert_eq!(set.sharpness_level, Some(3));
        assert!(set.sharpness_enlarged_only);
        assert!(set.video_upscaling);
        let clamped: PlayerSettings = apply_patch(&set, json!({ "sharpnessLevel": 7 })).unwrap();
        assert_eq!(clamped.sharpness_level, Some(3));
        let cleared: PlayerSettings = apply_patch(&set, json!({ "sharpnessLevel": null })).unwrap();
        assert_eq!(cleared.sharpness_level, None);
        assert!(serde_json::to_value(&cleared)
            .unwrap()
            .get("sharpnessLevel")
            .is_none());
    }
}

/// `appearance.theme` 문자열 → 창 테마(`None` = 시스템을 따른다).
pub fn window_theme(theme: AppTheme) -> Option<tauri::Theme> {
    match theme {
        AppTheme::System => None,
        AppTheme::Light => Some(tauri::Theme::Light),
        AppTheme::Dark => Some(tauri::Theme::Dark),
    }
}

/// 창(타이틀바·웹뷰 `prefers-color-scheme`) 테마를 설정에 맞춘다.
pub fn apply_window_theme<R: tauri::Runtime>(app: &tauri::AppHandle<R>, theme: AppTheme) {
    use tauri::Manager;
    for window in app.webview_windows().values() {
        if let Err(e) = window.set_theme(window_theme(theme)) {
            tracing::debug!("창 테마 설정 실패: {e}");
        }
        if window.label() == crate::tray_popover::TRAY_POPOVER_WINDOW {
            continue;
        }
        // 웹 화면이 그려지기 전 비치는 창 배경도 테마에 맞춘다(시스템이면 그대로 둔다).
        let background = match theme {
            AppTheme::Light => Some(tauri::window::Color(244, 245, 248, 255)),
            AppTheme::Dark => Some(tauri::window::Color(15, 16, 18, 255)),
            AppTheme::System => None,
        };
        if let Some(color) = background {
            let _ = window.set_background_color(Some(color));
        }
    }
    // 실행 중 Dock 아이콘도 앱 테마에 맞춘다(밝은·어두운 아이콘).
    crate::dock_icon::apply(app, theme);
}

/// 화면 테마 변경 — 설정에 저장하고 창 테마도 바꾼다.
#[tauri::command]
pub async fn appearance_set_theme(
    app: tauri::AppHandle,
    services: State<'_, AppServices>,
    theme: AppTheme,
) -> CmdResult<()> {
    let mut appearance = services.settings.appearance();
    appearance.theme = theme;
    services.settings.set_appearance(appearance);
    appearance_changed(&app, &services); // 창 테마도 여기서 건다
    Ok(())
}

/// 앱 정보 — 버전·데이터 폴더·기본 저장 폴더(정보 창, 데이터 폴더 열기, 화면 안내 경로 일치).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfoDto {
    pub name: String,
    pub version: String,
    /// 빌드 번호(CFBundleVersion).
    pub build: String,
    pub data_dir: String,
    pub recording_dir: String,
    pub screenshot_dir: String,
    pub download_dir: String,
    pub os: String,
    pub arch: String,
}

/// 빌드 번호(CFBundleVersion) — tauri.conf.json bundle.macOS.bundleVersion, 배포 빌드마다 +1. 없으면 빈 글자.
pub(crate) fn build_number<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> String {
    app.config()
        .bundle
        .macos
        .bundle_version
        .clone()
        .unwrap_or_default()
}

#[tauri::command]
pub async fn app_info(app: tauri::AppHandle) -> CmdResult<AppInfoDto> {
    use tauri::Manager;
    let info = app.package_info();
    Ok(AppInfoDto {
        name: info.name.clone(),
        version: info.version.to_string(),
        build: build_number(&app),
        data_dir: app
            .path()
            .app_data_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        recording_dir: super::content::app_save_dir(&app, "recording")
            .to_string_lossy()
            .into_owned(),
        screenshot_dir: super::content::app_save_dir(&app, "screenshot")
            .to_string_lossy()
            .into_owned(),
        download_dir: super::content::app_save_dir(&app, "download")
            .to_string_lossy()
            .into_owned(),
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
    })
}

/// 화면 수집기의 기록 한 줄(`ui.<kind>`). 디버그 로그가 꺼져 있으면 버린다.
#[tauri::command]
pub fn perf_log(kind: String, data: Value) {
    if crate::perf_log::enabled() {
        crate::perf_log::count_ui_event();
        let kind: String = kind
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
            .take(40)
            .collect();
        crate::perf_log::emit(&format!("ui.{kind}"), data);
    }
}

/// 디버그 로그 상태 — 켜짐 여부·파일·폴더(설정의 "로그 폴더 열기").
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerfLogStatusDto {
    pub enabled: bool,
    pub forced_by_env: bool,
    pub file: Option<String>,
    pub dir: Option<String>,
}

#[tauri::command]
pub fn perf_log_status() -> PerfLogStatusDto {
    PerfLogStatusDto {
        enabled: crate::perf_log::enabled(),
        forced_by_env: crate::perf_log::forced_by_env(),
        file: crate::perf_log::log_file().map(|p| p.to_string_lossy().into_owned()),
        dir: crate::perf_log::log_dir().map(|p| p.to_string_lossy().into_owned()),
    }
}

/// 썸네일 캐시 상태 — 설정 "썸네일 캐시" 줄.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageCacheStatusDto {
    pub entries: u64,
    pub bytes: u64,
    pub cap_bytes: u64,
}

fn image_service(
    app: &tauri::AppHandle,
) -> Option<std::sync::Arc<crate::image_service::ImageService>> {
    tauri::Manager::try_state::<std::sync::Arc<crate::image_service::ImageService>>(app)
        .map(|s| s.inner().clone())
}

fn image_cache_dto(images: Option<&crate::image_service::ImageService>) -> ImageCacheStatusDto {
    let s = images.map(|i| i.stats());
    ImageCacheStatusDto {
        entries: s.map_or(0, |s| s.entries),
        bytes: s.map_or(0, |s| s.bytes),
        cap_bytes: s.map_or(0, |s| s.cap_bytes),
    }
}

/// 동기 `#[tauri::command]` 는 메인(UI) 스레드에서 돈다 — 색인 합산·수천 개 파일 삭제는 블로킹 풀로 보낸다.
#[tauri::command]
pub async fn image_cache_status(app: tauri::AppHandle) -> ImageCacheStatusDto {
    let images = image_service(&app);
    tauri::async_runtime::spawn_blocking(move || image_cache_dto(images.as_deref()))
        .await
        .unwrap_or_else(|_| image_cache_dto(None))
}

/// "썸네일 캐시 비우기".
#[tauri::command]
pub async fn image_cache_clear(app: tauri::AppHandle) -> ImageCacheStatusDto {
    let images = image_service(&app);
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(i) = &images {
            i.clear();
        }
        image_cache_dto(images.as_deref())
    })
    .await
    .unwrap_or_else(|_| image_cache_dto(None))
}
