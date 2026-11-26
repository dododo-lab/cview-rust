//! 통계·로컬 데이터 관리 — 원본 Statistics/StatisticsView, AppState+StoreRecovery, AppState+LocalDataHygiene.
//!
//! 시청 기록 집계는 persistence `aggregate_insights` 를 그대로 쓰고, 화면에 맞게 DTO 로 편다.
//! 데이터 관리는 스토어 크기·표별 행 수·백업 목록을 보여 주고, 캐시·오래된 기록 정리와 수동 백업을 한다.
//! 백업 복원은 스토어가 열린 채로 파일을 바꿔야 해서 앱에서는 하지 않는다(백업 폴더만 연다).

use cview_persistence::store_backup::{BackupReason, StoreBackup};
use cview_persistence::watch_report::{ReportPeriod, WatchReport};
use serde::Serialize;
use tauri::{Manager, State};

use super::{CmdResult, CommandError};
use crate::state::{store_path, AppServices};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedSeconds {
    pub name: String,
    pub seconds: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaySeconds {
    /// RFC 3339(UTC 자정이 아니라 로컬 하루 시작).
    pub date: String,
    pub seconds: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchRecordDto {
    pub channel_id: String,
    pub channel_name: String,
    pub thumbnail_url: Option<String>,
    pub category_name: Option<String>,
    pub started_at: String,
    pub seconds: f64,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InsightsDto {
    pub total: f64,
    pub last7: f64,
    pub last30: f64,
    pub streak: i64,
    pub average: f64,
    pub channels: Vec<NamedSeconds>,
    pub categories: Vec<NamedSeconds>,
    /// 0~23시, 빈 시간은 0.
    pub hourly: Vec<f64>,
    pub daily: Vec<DaySeconds>,
    pub session_count: usize,
    pub counted_session_count: usize,
    pub unique_channel_count: usize,
    pub favorite_count: usize,
    pub bookmark_count: i64,
    pub recent: Vec<WatchRecordDto>,
}

fn named(v: Vec<(String, f64)>) -> Vec<NamedSeconds> {
    v.into_iter()
        .map(|(name, seconds)| NamedSeconds { name, seconds })
        .collect()
}

/// 통계 개요의 최근 기록 수.
const RECENT_LIMIT: usize = 60;

/// 시청 통계 — KPI·채널/카테고리 순위·시간대·30일 추이·최근 기록.
#[tauri::command]
pub async fn insights_overview(services: State<'_, AppServices>) -> CmdResult<InsightsDto> {
    let Some(store) = &services.data_store else {
        return Err(CommandError(
            "데이터 저장소를 열지 못해 통계를 볼 수 없습니다".into(),
        ));
    };
    // 집계·정렬은 SQLite 잠금과 디스크를 잡는다 — 비동기 작업자가 아니라 블로킹 풀에서 돌린다.
    store
        .off_main_uncancellable(|store| overview(store, chrono::Utc::now()))
        .await
}

fn overview(
    store: &cview_persistence::DataStore,
    now: chrono::DateTime<chrono::Utc>,
) -> CmdResult<InsightsDto> {
    let a = store
        .aggregate_insights(now)
        .map_err(|e| CommandError(e.to_string()))?;
    let mut hourly = vec![0.0; 24];
    for (h, s) in &a.hourly {
        if let Some(slot) = hourly.get_mut(*h as usize) {
            *slot = *s;
        }
    }
    // 최근 60개만 색인으로 읽는다 — 예전엔 기록 전체를 한 번 더 읽어 정렬했다(5천 행에서 약 5ms, 시작·사이드바 위젯마다).
    let recent = store
        .recent_watch_history_rows(RECENT_LIMIT)
        .map_err(|e| CommandError(e.to_string()))?
        .iter()
        .map(|r| WatchRecordDto {
            channel_id: r.channel_id.clone(),
            channel_name: r.channel_name.clone(),
            thumbnail_url: r.thumbnail_url.clone(),
            category_name: r.category_name.clone(),
            started_at: r.started_at.to_rfc3339(),
            seconds: r.effective_duration(now),
            active: r.ended_at.is_none(),
        })
        .collect();
    Ok(InsightsDto {
        total: a.all,
        last7: a.w7,
        last30: a.w30,
        streak: a.streak,
        average: a.avg,
        channels: named(a.channels),
        categories: named(a.categories),
        hourly,
        daily: a
            .daily
            .into_iter()
            .map(|(d, seconds)| DaySeconds {
                date: d.to_rfc3339(),
                seconds,
            })
            .collect(),
        session_count: a.session_count,
        counted_session_count: a.counted_session_count,
        unique_channel_count: a.unique_channel_count,
        favorite_count: store.fetch_favorite_items().map(|v| v.len()).unwrap_or(0),
        bookmark_count: store.table_row_count("StreamBookmark").unwrap_or(0),
        recent,
    })
}

/// 통계 화면 기간 리포트 — `period`: today · 7d · 30d · 90d · all.
/// 기록을 현지 정시 경계로 잘라 담은 추이·요일×시간대·순위·길이 분포를 한 번에 돌려준다.
#[tauri::command]
pub async fn insights_report(
    services: State<'_, AppServices>,
    period: String,
) -> CmdResult<WatchReport> {
    let Some(store) = &services.data_store else {
        return Err(CommandError(
            "데이터 저장소를 열지 못해 통계를 볼 수 없습니다".into(),
        ));
    };
    let period = ReportPeriod::parse(&period)
        .ok_or_else(|| CommandError(format!("알 수 없는 통계 기간: {period}")))?;
    store
        .off_main_uncancellable(move |store| store.watch_report(period, chrono::Utc::now()))
        .await
        .map_err(|e| CommandError(e.to_string()))
}

// ── 로컬 데이터 관리 (DAT-06) ─────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableCount {
    pub table: String,
    pub label: String,
    pub rows: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupDto {
    pub id: String,
    pub created_at: String,
    pub reason: String,
    pub bytes: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataHealthDto {
    pub available: bool,
    pub store_path: String,
    pub store_bytes: i64,
    pub free_bytes: Option<i64>,
    pub tables: Vec<TableCount>,
    pub backups: Vec<BackupDto>,
    pub backup_dir: String,
}

const TABLES: &[(&str, &str)] = &[
    ("WatchHistory", "시청 기록"),
    ("PersistedChannel", "채널(즐겨찾기·최근·메모)"),
    ("StreamBookmark", "북마크"),
    ("PersistedVODPlayback", "VOD 이어 보기"),
    ("PersistedVODWatchLater", "나중에 보기"),
    ("PersistedClipPlayback", "클립 재생 기록"),
    ("CachedLiveChannel", "라이브 목록 캐시"),
    ("CachedImageEntry", "이미지 캐시 색인"),
    ("CachedCategoryStat", "카테고리 통계 캐시"),
    ("CategorySnapshot", "카테고리 스냅숏"),
    ("CachedVOD", "VOD 캐시"),
    ("CachedClip", "클립 캐시"),
];

fn backup_for(app: &tauri::AppHandle) -> CmdResult<(std::path::PathBuf, StoreBackup)> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| CommandError(e.to_string()))?;
    let path = store_path(&dir);
    Ok((path.clone(), StoreBackup::new(path)))
}

fn reason_name(r: BackupReason) -> &'static str {
    match r {
        BackupReason::BeforeUpgrade => "업데이트 전",
        BackupReason::BeforeVersionStamp => "버전 기록 전",
        BackupReason::Manual => "수동",
    }
}

fn health_blocking(
    path: std::path::PathBuf,
    backup: StoreBackup,
    store: Option<cview_persistence::DataStore>,
) -> DataHealthDto {
    let tables = match &store {
        Some(store) => TABLES
            .iter()
            .map(|(t, label)| TableCount {
                table: (*t).into(),
                label: (*label).into(),
                rows: store.table_row_count(t).unwrap_or(0),
            })
            .collect(),
        None => Vec::new(),
    };
    let backup_dir = path
        .parent()
        .map(|p| {
            let dir = p.join("Backups");
            // 백업을 아직 한 번도 만들지 않았으면 폴더가 없어 '백업 폴더 열기'가 "파일이 없습니다"로 실패했다.
            let _ = std::fs::create_dir_all(&dir);
            dir.display().to_string()
        })
        .unwrap_or_default();
    DataHealthDto {
        available: store.is_some(),
        store_path: path.display().to_string(),
        store_bytes: StoreBackup::store_bytes(&path),
        free_bytes: path
            .parent()
            .and_then(cview_persistence::store_backup::volume_available_capacity),
        tables,
        backups: backup
            .list()
            .into_iter()
            .map(|b| BackupDto {
                id: b.id,
                created_at: b.created_at.to_rfc3339(),
                reason: reason_name(b.reason).into(),
                bytes: b.bytes,
            })
            .collect(),
        backup_dir,
    }
}

#[tauri::command]
pub async fn data_health(
    app: tauri::AppHandle,
    services: State<'_, AppServices>,
) -> CmdResult<DataHealthDto> {
    let (path, backup) = backup_for(&app)?;
    let store = services.data_store.clone();
    tauri::async_runtime::spawn_blocking(move || health_blocking(path, backup, store))
        .await
        .map_err(|e| CommandError(e.to_string()))
}

/// 지금 스토어를 백업한다(최근 2개 유지).
/// 열린 스토어가 있으면 SQLite 온라인 백업 API 로 WAL 미체크포인트 행까지 빠짐없이 담는다.
#[tauri::command]
pub async fn data_backup_now(
    app: tauri::AppHandle,
    services: State<'_, AppServices>,
) -> CmdResult<DataHealthDto> {
    let (path, backup) = backup_for(&app)?;
    let build = app.package_info().version.to_string();
    let store = services.data_store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let made = match &store {
            Some(s) => s.backup_online(&backup, BackupReason::Manual, &build, None),
            None => backup.snapshot(BackupReason::Manual, &build, None),
        };
        if made.is_none() {
            return Err(CommandError(
                "백업하지 못했습니다(저장 공간 부족이거나 스토어 파일 없음)".into(),
            ));
        }
        Ok(health_blocking(path, backup, store))
    })
    .await
    .map_err(|e| CommandError(e.to_string()))?
}

/// 정리 — `kind`: liveCache · imageIndex · oldHistory · legacy · recent.
#[tauri::command]
pub async fn data_cleanup(
    app: tauri::AppHandle,
    services: State<'_, AppServices>,
    kind: String,
) -> CmdResult<DataHealthDto> {
    let Some(store) = services.data_store.clone() else {
        return Err(CommandError("데이터 저장소를 열지 못했습니다".into()));
    };
    if !matches!(
        kind.as_str(),
        "liveCache" | "imageIndex" | "oldHistory" | "legacy" | "recent"
    ) {
        return Err(CommandError(format!("알 수 없는 정리 항목: {kind}")));
    }
    let (path, backup) = backup_for(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let err = |e: cview_persistence::PersistenceError| CommandError(e.to_string());
        match kind.as_str() {
            "liveCache" => {
                store
                    .with_connection(|c| {
                        c.execute("DELETE FROM CachedLiveChannel", [])?;
                        Ok(())
                    })
                    .map_err(err)?;
            }
            "imageIndex" => store.delete_all_image_entries().map_err(err)?,
            "oldHistory" => store.cleanup_old_watch_history(90).map_err(err)?,
            "legacy" => {
                store.purge_legacy_write_only_rows().map_err(err)?;
                store.purge_legacy_home_cache().map_err(err)?;
            }
            "recent" => store.clear_all_last_watched().map_err(err)?,
            _ => unreachable!(),
        }
        Ok(health_blocking(path, backup, Some(store)))
    })
    .await
    .map_err(|e| CommandError(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 백업을 한 번도 만들지 않은 새 설치에서도 '백업 폴더 열기'가 열 폴더가 있다.
    #[test]
    fn data_health_makes_the_backup_folder_it_points_to() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cview.sqlite");
        let health = health_blocking(path.clone(), StoreBackup::new(path), None);
        assert!(!health.backup_dir.is_empty());
        assert!(std::path::Path::new(&health.backup_dir).is_dir());
    }
}
