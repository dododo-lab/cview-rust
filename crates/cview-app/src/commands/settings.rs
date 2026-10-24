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
use crate::multi_live::{MultiLiveShell, PaneStateDto};
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

fn snapshot(services: &AppServices) -> SettingsDto {
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

/// 도메인 하나에 패치를 입힌다(`DOMAINS`). 네트워크 설정은 API 클라이언트에도 반영한다.
fn apply_domain(services: &AppServices, domain: &str, patch: Value) -> CmdResult<()> {
    let s = &services.settings;
    match domain {
        "player" => s.set_player(apply_patch::<PlayerSettings>(&s.player(), patch)?),
        "general" => s.set_general(apply_patch::<GeneralSettings>(&s.general(), patch)?),
        "appearance" => {
            s.set_appearance(apply_patch::<AppearanceSettings>(&s.appearance(), patch)?)
        }
        "chat" => s.set_chat(merge_patch(s.chat(), patch)),
        "network" => {
            // 값 하나를 손으로 바꾸면 자동 프리셋은 꺼진다(백업 복원처럼 `autoPreset` 을 함께 보내면 그 값을 따른다).
            let manual = patch.get("autoPreset").is_none();
            let mut next = apply_patch::<NetworkSettings>(&s.network(), patch)?;
            if manual {
                next.auto_preset = false;
            }
            s.set_network(next);
            services.api.update_network_settings(&s.network());
        }
        "multiLive" => s.set_multi_live(apply_patch::<MultiLiveSettings>(&s.multi_live(), patch)?),
        "multiChat" => s.set_multi_chat(apply_patch::<MultiChatSettings>(&s.multi_chat(), patch)?),
        "keyboard" => s.set_keyboard(apply_patch::<KeyboardShortcutSettings>(
            &s.keyboard(),
            patch,
        )?),
        "channelNotifications" => s.set_channel_notifications(apply_patch::<
            ChannelNotificationSettings,
        >(
            &s.channel_notifications(), patch
        )?),
        "sync" => s.set_sync(apply_patch::<SyncSettings>(&s.sync(), patch)?),
        other => return Err(CommandError(format!("알 수 없는 설정 영역: {other}"))),
    }
    Ok(())
}

/// `domain` 은 [`DOMAINS`] 중 하나.
#[tauri::command]
pub async fn settings_update(
    app: tauri::AppHandle,
    services: State<'_, AppServices>,
    domain: String,
    patch: Value,
) -> CmdResult<SettingsDto> {
    apply_domain(&services, &domain, patch)?;
    if domain == "appearance" {
        // 설정 "디버그 로그"를 켜고 끄면 바로 반영하고 화면 수집기에 알린다.
        if let Ok(dir) = tauri::Manager::path(&app).app_data_dir() {
            let on = crate::perf_log::forced_by_env() || services.settings.appearance().debug_mode;
            crate::perf_log::set_enabled(&dir, on);
            let _ = tauri::Emitter::emit(&app, "perf://enabled", on);
        }
    }
    if domain == "general" {
        crate::window_prefs::apply_general(&app, &services.settings.general());
    }
    if domain == "keyboard" {
        crate::menu::rebuild(&app);
    }
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
    let s = &services.settings;
    let current = s.network();
    if !current.auto_preset {
        return false;
    }
    let state = shell.state_dto();
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
    services: State<'_, AppServices>,
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
            if apply_domain(&services, domain, v.clone()).is_err() {
                skipped.push((*domain).to_string());
            }
        }
    }
    Ok(skipped)
}

/// 도메인 초기화 — 기본값으로 되돌린다(`all` 이면 전부).
#[tauri::command]
pub async fn settings_reset(
    services: State<'_, AppServices>,
    domain: String,
) -> CmdResult<SettingsDto> {
    let s = &services.settings;
    let reset = |d: &str| match d {
        "player" => s.set_player(PlayerSettings::default()),
        "general" => s.set_general(GeneralSettings::default()),
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
    services.api.update_network_settings(&s.network());
    Ok(snapshot(&services))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    apply_window_theme(&app, theme);
    Ok(())
}

/// 앱 정보 — 버전·데이터 폴더(정보 창, 데이터 폴더 열기).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfoDto {
    pub name: String,
    pub version: String,
    /// 빌드 번호(CFBundleVersion).
    pub build: String,
    pub data_dir: String,
    pub os: String,
    pub arch: String,
}

#[tauri::command]
pub async fn app_info(app: tauri::AppHandle) -> CmdResult<AppInfoDto> {
    use tauri::Manager;
    let info = app.package_info();
    Ok(AppInfoDto {
        name: info.name.clone(),
        version: info.version.to_string(),
        // 빌드 번호(CFBundleVersion) — tauri.conf.json bundle.macOS.bundleVersion, 배포 빌드마다 +1.
        build: app
            .config()
            .bundle
            .macos
            .bundle_version
            .clone()
            .unwrap_or_default(),
        data_dir: app
            .path()
            .app_data_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
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

#[tauri::command]
pub fn image_cache_status(app: tauri::AppHandle) -> ImageCacheStatusDto {
    image_cache_dto(image_service(&app).as_deref())
}

/// "썸네일 캐시 비우기".
#[tauri::command]
pub fn image_cache_clear(app: tauri::AppHandle) -> ImageCacheStatusDto {
    let images = image_service(&app);
    if let Some(i) = &images {
        i.clear();
    }
    image_cache_dto(images.as_deref())
}
