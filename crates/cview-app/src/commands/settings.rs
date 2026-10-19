//! 설정 읽기/쓰기 — `SettingsStore` 의 도메인을 JSON 그대로 주고받는다.
//!
//! 쓰기는 부분 패치다: 현재 도메인 JSON 위에 받은 키만 덮어쓴 뒤 도메인 타입으로 다시 해석한다.
//! 해석에 실패하면(잘못된 값) 아무것도 바꾸지 않는다.

use cview_core::models::settings::{
    AppTheme, AppearanceSettings, GeneralSettings, NetworkSettings, PlayerSettings,
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
}

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
            s.set_network(apply_patch::<NetworkSettings>(&s.network(), patch)?);
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
    preset: cview_core::models::settings::NetworkPreset,
) -> CmdResult<SettingsDto> {
    let s = &services.settings;
    s.set_network(preset.settings());
    services.api.update_network_settings(&s.network());
    Ok(snapshot(&services))
}

/// 설정 내보내기 — 모든 도메인을 JSON 문자열로(백업, SET-17).
#[tauri::command]
pub async fn settings_export(services: State<'_, AppServices>) -> CmdResult<String> {
    let mut v =
        serde_json::to_value(snapshot(&services)).map_err(|e| CommandError(e.to_string()))?;
    if let Value::Object(map) = &mut v {
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
        };
        let v = serde_json::to_value(dto).unwrap();
        let keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
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
        data_dir: app
            .path()
            .app_data_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
    })
}
