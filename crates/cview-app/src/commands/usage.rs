//! 앱 사용률 명령 — 상태·기록 읽기, 실시간 표본 받기 켜고 끄기, 기록 지우기·내보내기([`crate::usage`]).

use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use super::content::{create_unique_file, sanitize_file_name};
use super::{CmdResult, CommandError};
use crate::usage::history::{self, Sample};
use crate::usage::{UsageMonitor, UsageStatus};

/// 기록 — 표본과 간격(그래프 가로축·빈칸 판단).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageHistoryDto {
    pub interval_ms: u64,
    pub samples: Vec<Sample>,
}

/// 지금 상태(설정·GPU 지원·마지막 표본).
#[tauri::command]
pub fn usage_status(monitor: State<'_, Arc<UsageMonitor>>) -> UsageStatus {
    monitor.status()
}

/// `since`(유닉스 ms) 뒤 표본. 없으면 기록 전체.
#[tauri::command]
pub fn usage_history(monitor: State<'_, Arc<UsageMonitor>>, since: Option<i64>) -> UsageHistoryDto {
    UsageHistoryDto {
        interval_ms: monitor.status().interval_ms,
        samples: monitor.history(since),
    }
}

/// 화면이 보이는 동안 `on = true` — 그동안만 표본마다 `usage://sample` 을 보낸다. 지금 상태를 돌려준다.
#[tauri::command]
pub fn usage_watch(monitor: State<'_, Arc<UsageMonitor>>, on: bool) -> UsageStatus {
    monitor.set_live(on);
    monitor.status()
}

/// 기록을 비운다(측정은 그대로).
#[tauri::command]
pub fn usage_clear(monitor: State<'_, Arc<UsageMonitor>>) {
    monitor.clear();
}

/// 내보낼 파일 이름 — `CView-사용률-20261121-153000.csv`. 순수 함수.
pub fn export_file_name(format: &str, now: chrono::DateTime<chrono::Local>) -> Option<String> {
    let ext = match format {
        "csv" => "csv",
        "json" => "json",
        _ => return None,
    };
    Some(format!(
        "CView-사용률-{}.{ext}",
        now.format("%Y%m%d-%H%M%S")
    ))
}

/// 기록을 `format`(csv·json)으로 다운로드 폴더의 CView 아래에 저장하고 경로를 돌려준다.
#[tauri::command]
pub async fn usage_export(
    app: tauri::AppHandle,
    monitor: State<'_, Arc<UsageMonitor>>,
    format: String,
) -> CmdResult<String> {
    let name = export_file_name(&format, chrono::Local::now())
        .ok_or_else(|| CommandError("csv 또는 json 으로만 내보낼 수 있습니다".into()))?;
    let status = monitor.status();
    let samples = monitor.history(None);
    if samples.is_empty() {
        return Err(CommandError("내보낼 기록이 없습니다".into()));
    }
    let body = if format == "csv" {
        history::to_csv(&samples, status.cores)
    } else {
        crate::usage::export_json(&status, &samples)
    };
    let dir = tauri::Manager::path(&app)
        .download_dir()
        .map_err(|e| CommandError(format!("다운로드 폴더를 찾지 못했습니다: {e}")))?
        .join("CView");
    tauri::async_runtime::spawn_blocking(move || -> CmdResult<String> {
        std::fs::create_dir_all(&dir)
            .map_err(|e| CommandError(format!("폴더를 만들 수 없습니다: {e}")))?;
        let (path, mut file) = create_unique_file(&dir, &sanitize_file_name(&name))
            .map_err(|e| CommandError(format!("저장하지 못했습니다: {e}")))?;
        if let Err(e) = std::io::Write::write_all(&mut file, body.as_bytes()) {
            drop(file);
            let _ = std::fs::remove_file(&path);
            return Err(CommandError(format!("저장하지 못했습니다: {e}")));
        }
        Ok(path.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| CommandError(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn export_names_only_known_formats() {
        let t = chrono::Local
            .with_ymd_and_hms(2026, 11, 21, 15, 30, 5)
            .unwrap();
        assert_eq!(
            export_file_name("csv", t).as_deref(),
            Some("CView-사용률-20261121-153005.csv")
        );
        assert_eq!(
            export_file_name("json", t).as_deref(),
            Some("CView-사용률-20261121-153005.json")
        );
        assert_eq!(export_file_name("exe", t), None);
        assert_eq!(export_file_name("../csv", t), None);
    }
}
