//! 컨테이너 열기 = 사전 점검 → (업그레이드면) 백업 → 열기 → 기록.
//!
//! 원본: `Sources/CViewPersistence/DataStore+StoreOpen.swift` (`createContainer`, `StoreOpenTrace`).
//!
//! 1. [`StorePreflight::inspect`] 는 메타데이터(`user_version`)만 읽는다.
//! 2. `NeedsLightweightUpgrade` 일 때만 백업한다. 아직 연결이 없어 사본이 일관된다.
//! 3. 열기(마이그레이션 포함).
//! 4. 성공·실패를 store-health.json 에 남긴다. 연속 3회부터 error 대신 더 높은 로그로 올린다.
//!
//! ⚠ 실패해도 스토어를 지우거나 옮기지 않는다.
//! signpost 구간과 캡처 줄 `store_open`(PerfCaptureLog) 은 포팅하지 않았다.

use super::DataStore;
use crate::error::{PersistenceError, Result};
use crate::schema::StorePreflight;
use crate::store_backup::{BackupReason, StoreBackup};
use crate::store_open_health::{Attempt, ErrorSummary, StoreOpenHealthRecorder};
use std::path::Path;
use std::time::Instant;

/// 앱 빌드 번호 — `CVIEW_APP_BUILD` 환경 변수, 없으면 "development".
pub fn current_app_build() -> String {
    std::env::var("CVIEW_APP_BUILD").unwrap_or_else(|_| "development".into())
}

impl DataStore {
    /// 실사용 스토어를 연다. 기록은 `<로그 폴더>/store-health.json`, 백업은 스토어 옆 `Backups/`.
    pub fn create_container(health: &StoreOpenHealthRecorder) -> Result<DataStore> {
        let path = match Self::resolve_store_url() {
            Ok(p) => p,
            Err(e) => {
                // 경로를 풀지 못한 실패도 같은 기록에 센다.
                health.record_failure(
                    &e,
                    None,
                    &Attempt {
                        app_build: Some(current_app_build()),
                        ..Attempt::new(0.0, None)
                    },
                );
                return Err(e);
            }
        };
        let backup = StoreBackup::new(&path);
        Self::create_container_at(&path, health, &backup, &current_app_build())
    }

    /// 열기 경로 본체. 테스트는 임시 폴더의 경로·기록기·백업을 넘긴다.
    pub fn create_container_at(
        path: &Path,
        health: &StoreOpenHealthRecorder,
        backup: &StoreBackup,
        app_build: &str,
    ) -> Result<DataStore> {
        let started = Instant::now();
        let (preflight, preflight_error) = StorePreflight::inspect(path);
        let mut backup_folder = None;
        if preflight == StorePreflight::NeedsLightweightUpgrade {
            backup_folder = backup
                .snapshot(
                    BackupReason::BeforeUpgrade,
                    app_build,
                    Some(preflight.raw_value()),
                )
                .map(|e| e.id);
        }
        let store_bytes = StoreBackup::store_bytes(path);
        let opened = DataStore::open(path);
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        let attempt = Attempt {
            ms,
            preflight: Some(preflight),
            backup_folder: backup_folder.clone(),
            store_bytes: Some(store_bytes),
            app_build: Some(app_build.to_string()),
        };
        match opened {
            Ok(store) => {
                health.record_success(&attempt, false);
                if let Some(folder) = &backup_folder {
                    tracing::info!(
                        "store_open upgraded preflight={} backup={} ms={}",
                        preflight.raw_value(),
                        folder,
                        ms as i64
                    );
                } else if preflight == StorePreflight::NeedsLightweightUpgrade {
                    tracing::warn!("store_open upgraded without backup ms={}", ms as i64);
                }
                Ok(store)
            }
            Err(error) => {
                let recorded = health.record_failure(&error, preflight_error.as_ref(), &attempt);
                let summary = ErrorSummary::new(&error, preflight_error.as_ref());
                let message = format!(
                    "store_open failed domain={} code={} underlying={}:{} sqlite={} consecutive={} preflight={}",
                    summary.domain,
                    summary.code,
                    summary.underlying_domain.as_deref().unwrap_or("-"),
                    summary.underlying_code.unwrap_or(0),
                    summary.sqlite_code.unwrap_or(0),
                    recorded.consecutive_failures,
                    preflight.raw_value()
                );
                if recorded.consecutive_failures >= 3 {
                    tracing::error!(fault = true, "{message}");
                } else {
                    tracing::error!("{message}");
                }
                Err(error)
            }
        }
    }
}

/// 이 크레이트 오류를 그대로 쓴다는 표시(공개 경로 문서화용).
pub type StoreOpenError = PersistenceError;
