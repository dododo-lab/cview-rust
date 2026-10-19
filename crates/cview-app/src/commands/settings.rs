//! 설정 읽기/쓰기 — `SettingsStore` 의 도메인을 JSON 그대로 주고받는다.
//!
//! 쓰기는 부분 패치다: 현재 도메인 JSON 위에 받은 키만 덮어쓴 뒤 도메인 타입으로 다시 해석한다.
//! 해석에 실패하면(잘못된 값) 아무것도 바꾸지 않는다.

use cview_core::models::settings::{AppearanceSettings, GeneralSettings, PlayerSettings};
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
}

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
    }
}

#[tauri::command]
pub async fn settings_get(services: State<'_, AppServices>) -> CmdResult<SettingsDto> {
    Ok(snapshot(&services))
}

/// `domain` = `player` / `general` / `appearance` / `chat`.
#[tauri::command]
pub async fn settings_update(
    services: State<'_, AppServices>,
    domain: String,
    patch: Value,
) -> CmdResult<SettingsDto> {
    let s = &services.settings;
    match domain.as_str() {
        "player" => s.set_player(apply_patch::<PlayerSettings>(&s.player(), patch)?),
        "general" => s.set_general(apply_patch::<GeneralSettings>(&s.general(), patch)?),
        "appearance" => {
            s.set_appearance(apply_patch::<AppearanceSettings>(&s.appearance(), patch)?)
        }
        "chat" => s.set_chat(merge_patch(s.chat(), patch)),
        other => return Err(CommandError(format!("알 수 없는 설정 영역: {other}"))),
    }
    Ok(snapshot(&services))
}

#[cfg(test)]
mod tests {
    use super::*;
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
