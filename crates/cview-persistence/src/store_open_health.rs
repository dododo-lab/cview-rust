//! 실행을 넘어 누적하는 스토어 열기 기록(store-health.json).
//!
//! 원본: `Sources/CViewPersistence/StoreOpenHealth.swift` (`StoreOpenHealth`, `ErrorSummary`, `Attempt`,
//! `StoreOpenHealthRecorder`). 사전 점검 `StorePreflight` 는 [`crate::schema::StorePreflight`].
//!
//! ⚠ 메시지·경로는 싣지 않는다(사용자 이름이 섞인다). domain·code 숫자와 폴더 이름만.

use crate::error::PersistenceError;
use crate::schema::StorePreflight;
use crate::time::{now, Date};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// 오류의 식별 가능한 숫자만(`ErrorSummary`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorSummary {
    pub domain: String,
    pub code: i64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub underlying_domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub underlying_code: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub sqlite_code: Option<i64>,
}

fn domain_and_code(error: &PersistenceError) -> (String, i64, Option<i64>) {
    match error {
        PersistenceError::Sqlite(e) => {
            let code = match e {
                rusqlite::Error::SqliteFailure(f, _) => Some(i64::from(f.extended_code)),
                _ => None,
            };
            ("SQLite".into(), code.unwrap_or(1), code)
        }
        PersistenceError::Io(e) => ("IO".into(), i64::from(e.raw_os_error().unwrap_or(0)), None),
        PersistenceError::Json(_) => ("JSON".into(), 1, None),
        PersistenceError::Cancelled => ("Cancelled".into(), 1, None),
        PersistenceError::Join(_) => ("Join".into(), 1, None),
        PersistenceError::NewerSchema { found, .. } => ("Schema".into(), *found, None),
        PersistenceError::Other(_) => ("CViewPersistence".into(), 1, None),
    }
}

impl ErrorSummary {
    /// `context` 는 `error` 에 원인이 없을 때 underlying 자리를 채울 오류(사전 점검 오류).
    pub fn new(error: &PersistenceError, context: Option<&PersistenceError>) -> Self {
        let (domain, code, sqlite) = domain_and_code(error);
        let underlying = context.map(domain_and_code);
        Self {
            domain: domain.chars().take(128).collect(),
            code,
            underlying_domain: underlying
                .as_ref()
                .map(|(d, _, _)| d.chars().take(128).collect()),
            underlying_code: underlying.as_ref().map(|(_, c, _)| *c),
            sqlite_code: sqlite.or(underlying.and_then(|(_, _, s)| s)),
        }
    }
}

/// 열기 한 번의 관측값(`Attempt`). 경로는 없다.
#[derive(Debug, Clone, PartialEq)]
pub struct Attempt {
    pub ms: f64,
    /// 경로를 풀기 전에 실패했으면 `None`.
    pub preflight: Option<StorePreflight>,
    pub backup_folder: Option<String>,
    pub store_bytes: Option<i64>,
    pub app_build: Option<String>,
}

impl Attempt {
    pub fn new(ms: f64, preflight: Option<StorePreflight>) -> Self {
        Self {
            ms,
            preflight,
            backup_folder: None,
            store_bytes: None,
            app_build: None,
        }
    }
}

/// 누적 열기 기록(`StoreOpenHealth`). 날짜는 ISO 8601.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreOpenHealth {
    /// 파일 형식 버전(JSON 키 `v`).
    #[serde(rename = "v")]
    pub version: i64,
    pub total_opens: i64,
    pub failures: i64,
    pub consecutive_failures: i64,
    #[serde(
        with = "crate::time::iso8601::option",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub last_success_at: Option<Date>,
    #[serde(
        with = "crate::time::iso8601::option",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub last_failure_at: Option<Date>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_open_ms: Option<f64>,
    /// 마지막 시도의 `StorePreflight` raw value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_preflight: Option<String>,
    /// 마지막으로 찍은 백업 폴더 **이름**.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_backup_folder: Option<String>,
    pub versioned_fallback_count: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_build: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store_bytes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<ErrorSummary>,
}

impl StoreOpenHealth {
    pub const CURRENT_VERSION: i64 = 1;
}

impl Default for StoreOpenHealth {
    fn default() -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            total_opens: 0,
            failures: 0,
            consecutive_failures: 0,
            last_success_at: None,
            last_failure_at: None,
            last_open_ms: None,
            last_preflight: None,
            last_backup_folder: None,
            versioned_fallback_count: 0,
            app_build: None,
            store_bytes: None,
            last_error: None,
        }
    }
}

type Clock = Arc<dyn Fn() -> Date + Send + Sync>;

/// `StoreOpenHealth` 를 파일과 함께 갱신한다(`StoreOpenHealthRecorder`).
pub struct StoreOpenHealthRecorder {
    file_path: PathBuf,
    cache: Mutex<Option<StoreOpenHealth>>,
    now: Clock,
}

impl std::fmt::Debug for StoreOpenHealthRecorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoreOpenHealthRecorder")
            .field("file_path", &self.file_path)
            .finish()
    }
}

impl StoreOpenHealthRecorder {
    pub const FILE_NAME: &'static str = "store-health.json";

    pub fn new(file_path: impl Into<PathBuf>) -> Self {
        Self::with_clock(file_path, Arc::new(now))
    }

    /// 테스트 주입용 시계.
    pub fn with_clock(file_path: impl Into<PathBuf>, now: Clock) -> Self {
        Self {
            file_path: file_path.into(),
            cache: Mutex::new(None),
            now,
        }
    }

    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    /// 파일에서 읽는다(처음 한 번). 없거나 읽을 수 없으면 빈 기록.
    pub fn load(&self) -> StoreOpenHealth {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        self.current(&mut cache)
    }

    /// 성공 — 연속 실패를 0 으로. 누적 실패 수와 마지막 오류 요약은 남긴다.
    pub fn record_success(&self, attempt: &Attempt, versioned_fallback: bool) -> StoreOpenHealth {
        let t = (self.now)();
        self.update(|h| {
            h.consecutive_failures = 0;
            h.last_success_at = Some(t);
            if versioned_fallback {
                h.versioned_fallback_count += 1;
            }
            apply(attempt, h);
        })
    }

    /// 실패.
    pub fn record_failure(
        &self,
        error: &PersistenceError,
        context: Option<&PersistenceError>,
        attempt: &Attempt,
    ) -> StoreOpenHealth {
        let t = (self.now)();
        let summary = ErrorSummary::new(error, context);
        self.update(|h| {
            h.failures += 1;
            h.consecutive_failures += 1;
            h.last_failure_at = Some(t);
            h.last_error = Some(summary);
            apply(attempt, h);
        })
    }

    fn current(&self, cache: &mut Option<StoreOpenHealth>) -> StoreOpenHealth {
        if let Some(c) = cache {
            return c.clone();
        }
        let loaded = std::fs::read(&self.file_path)
            .ok()
            .and_then(|d| serde_json::from_slice::<StoreOpenHealth>(&d).ok())
            .unwrap_or_default();
        *cache = Some(loaded.clone());
        loaded
    }

    fn update(&self, body: impl FnOnce(&mut StoreOpenHealth)) -> StoreOpenHealth {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let mut health = self.current(&mut cache);
        body(&mut health);
        *cache = Some(health.clone());
        self.write(&health);
        health
    }

    fn write(&self, health: &StoreOpenHealth) {
        let result = (|| -> std::io::Result<()> {
            if let Some(dir) = self.file_path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let data = crate::json::to_sorted_vec_pretty(health).map_err(std::io::Error::other)?;
            crate::json::write_atomic(&self.file_path, &data)
        })();
        if let Err(e) = result {
            tracing::warn!(
                "store_health write failed code={}",
                e.raw_os_error().unwrap_or(0)
            );
        }
    }
}

fn apply(attempt: &Attempt, health: &mut StoreOpenHealth) {
    health.version = StoreOpenHealth::CURRENT_VERSION;
    health.total_opens += 1;
    health.last_open_ms = Some(attempt.ms);
    health.last_preflight = attempt.preflight.map(|p| p.raw_value().to_string());
    if let Some(folder) = &attempt.backup_folder {
        health.last_backup_folder = Some(folder.clone());
    }
    if let Some(bytes) = attempt.store_bytes {
        health.store_bytes = Some(bytes);
    }
    if let Some(build) = &attempt.app_build {
        health.app_build = Some(build.clone());
    }
}
