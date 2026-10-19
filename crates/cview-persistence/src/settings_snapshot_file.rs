//! 설정 외부 백업(미러) 파일 — 재설치/스키마 초기화에도 설정 보존.
//!
//! 원본: `Sources/CViewPersistence/SettingsSnapshotFile.swift`.
//!
//! 위치: `<ApplicationSupport>/CView/settings.json`. 형식:
//! `{"schemaVersion": 1, "savedAt": <epoch 초>, "settings": {<도메인>: <JSON 객체/배열>}}`(pretty, 키 정렬).
//! 읽기는 하위 버전을 허용하고 상위 버전만 막는다. 정수가 아닌 `schemaVersion` 은 거부한다.

use serde_json::{Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// `SettingsSnapshotFile` 네임스페이스.
pub struct SettingsSnapshotFile;

/// JSONSerialization 최상위로 허용되는 값(객체·배열)인가.
fn is_container(v: &Value) -> bool {
    matches!(v, Value::Object(_) | Value::Array(_))
}

impl SettingsSnapshotFile {
    /// 스냅샷 스키마 버전.
    pub const SCHEMA_VERSION: i64 = 1;

    /// 백업 파일 경로. 디렉터리는 필요 시 생성. 접근 불가 시 `None`.
    pub fn file_path() -> Option<PathBuf> {
        let dir = crate::store::application_support_dir().ok()?.join("CView");
        let _ = std::fs::create_dir_all(&dir);
        Some(dir.join("settings.json"))
    }

    /// 백업 파일 존재 여부.
    pub fn exists(path: Option<&Path>) -> bool {
        path.is_some_and(Path::exists)
    }

    /// 도메인별 JSON 바이트 묶음을 파일에 원자적으로 기록한다. 실제로 썼으면 true.
    pub fn write(domains: &HashMap<String, Vec<u8>>, saved_at: f64, path: Option<&Path>) -> bool {
        let Some(path) = path else { return false };
        if domains.is_empty() {
            return false;
        }
        let mut settings = Map::new();
        for (key, data) in domains {
            if let Ok(obj) = serde_json::from_slice::<Value>(data) {
                if is_container(&obj) {
                    settings.insert(key.clone(), obj);
                }
            }
        }
        if settings.is_empty() {
            return false;
        }
        let mut root = Map::new();
        root.insert("schemaVersion".into(), Value::from(Self::SCHEMA_VERSION));
        root.insert(
            "savedAt".into(),
            serde_json::Number::from_f64(saved_at)
                .map(Value::Number)
                .unwrap_or(Value::Null),
        );
        root.insert("settings".into(), Value::Object(settings));
        let Ok(out) = crate::json::to_sorted_vec_pretty(&Value::Object(root)) else {
            tracing::warn!("Settings snapshot serialize failed");
            return false;
        };
        match crate::json::write_atomic(path, &out) {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!("Settings snapshot write failed: {e}");
                false
            }
        }
    }

    /// 파일에서 도메인별 JSON 바이트 묶음을 복원. 없음/손상/새 버전이면 `None`.
    pub fn read(path: Option<&Path>) -> Option<HashMap<String, Vec<u8>>> {
        let path = path?;
        if !path.exists() {
            return None;
        }
        match std::fs::read(path) {
            Ok(data) => Self::parse(&data),
            Err(_) => {
                tracing::warn!("Settings snapshot read failed");
                None
            }
        }
    }

    /// `read` 의 순수 부분.
    pub fn parse(data: &[u8]) -> Option<HashMap<String, Vec<u8>>> {
        let Ok(Value::Object(root)) = serde_json::from_slice::<Value>(data) else {
            tracing::warn!("Settings snapshot parse failed (corrupt JSON)");
            return None;
        };
        // JSONSerialization 의 `as? Int` 와 같게 — 정수로 정확히 나타나는 수만.
        let version = match root.get("schemaVersion") {
            Some(Value::Number(n)) => n
                .as_i64()
                .or_else(|| n.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64)),
            _ => None,
        };
        let Some(version) = version else {
            tracing::warn!("Settings snapshot has no schemaVersion — skipping restore");
            return None;
        };
        if version > Self::SCHEMA_VERSION {
            tracing::warn!(
                "Settings snapshot is newer than this app (v{version} > v{}) — skipping restore",
                Self::SCHEMA_VERSION
            );
            return None;
        }
        let Some(Value::Object(settings)) = root.get("settings") else {
            return None;
        };
        let mut domains = HashMap::new();
        for (key, obj) in settings {
            if is_container(obj) {
                if let Ok(d) = serde_json::to_vec(obj) {
                    domains.insert(key.clone(), d);
                }
            }
        }
        (!domains.is_empty()).then_some(domains)
    }
}
