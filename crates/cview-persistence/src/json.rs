//! 키 정렬 JSON 인코딩 보조.
//!
//! 원본: Swift `JSONEncoder.outputFormatting = [.sortedKeys]` 를 쓰던 자리
//! (`DataStore.swift` `saveSetting`, `SettingsStore.swift` `encodeAllDomains`,
//! `UserRecordFile.swift` `encode`, `SettingsSnapshotFile.swift` `write`).
//! 같은 값은 항상 같은 바이트가 나와야 같은 값 가드가 걸린다. `serde_json` 의 `preserve_order`
//! 기능이 켜져도 정렬이 유지되도록 객체를 키 순서대로 다시 만든다.

use serde::Serialize;
use serde_json::{Map, Value};

/// 객체 키를 재귀적으로 정렬한 값.
pub fn sorted_value(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let mut out = Map::new();
            for (k, v) in entries {
                out.insert(k, sorted_value(v));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted_value).collect()),
        other => other,
    }
}

/// 키 정렬 압축 JSON 바이트.
pub fn to_sorted_vec<T: Serialize + ?Sized>(value: &T) -> serde_json::Result<Vec<u8>> {
    serde_json::to_vec(&sorted_value(serde_json::to_value(value)?))
}

/// 키 정렬 pretty JSON 바이트.
pub fn to_sorted_vec_pretty<T: Serialize + ?Sized>(value: &T) -> serde_json::Result<Vec<u8>> {
    serde_json::to_vec_pretty(&sorted_value(serde_json::to_value(value)?))
}

/// 임시 파일에 쓴 뒤 rename 하는 원자적 쓰기(Swift `Data.write(to:options:.atomic)`).
pub fn write_atomic(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.tmp-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(data)?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}
