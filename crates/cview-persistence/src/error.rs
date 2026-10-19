//! 저장소 오류 타입.
//!
//! 원본: Swift 쪽은 SwiftData/CoreData 가 던지는 `NSError` 를 그대로 전파했다
//! (`DataStore.swift` 의 `throws` 메서드 전반, `DataStore+SaveOrRollback.swift`).
//! Rust 포팅은 SQLite·JSON·IO 오류를 한 열거형으로 감싼다.

use cview_core::error::{AppError, PersistenceError as CorePersistenceError};

/// 저장소 계층 오류.
#[derive(Debug, thiserror::Error)]
pub enum PersistenceError {
    /// SQLite 오류(`rusqlite`).
    #[error("SQLite 오류: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// JSON 인코딩/디코딩 오류.
    #[error("JSON 오류: {0}")]
    Json(#[from] serde_json::Error),
    /// 파일 입출력 오류.
    #[error("IO 오류: {0}")]
    Io(#[from] std::io::Error),
    /// 호출자가 취소해 본문을 실행하지 않았다(Swift `CancellationError`).
    #[error("작업이 취소되었습니다")]
    Cancelled,
    /// 백그라운드 스레드 작업이 패닉 등으로 끝나지 못했다.
    #[error("백그라운드 작업 실패: {0}")]
    Join(String),
    /// 스토어의 스키마 버전이 이 빌드가 아는 것보다 새롭다(다운그레이드).
    #[error("스키마 버전 {found} 은(는) 지원 버전 {supported} 보다 새롭습니다")]
    NewerSchema { found: i64, supported: i64 },
    /// 그 밖의 오류.
    #[error("{0}")]
    Other(String),
}

/// 저장소 결과 타입.
pub type Result<T> = std::result::Result<T, PersistenceError>;

impl PersistenceError {
    /// SQLite 확장 오류 코드(있으면). `StoreOpenHealth.ErrorSummary.sqliteCode` 대응.
    pub fn sqlite_code(&self) -> Option<i32> {
        match self {
            PersistenceError::Sqlite(rusqlite::Error::SqliteFailure(e, _)) => Some(e.extended_code),
            _ => None,
        }
    }
}

impl From<PersistenceError> for AppError {
    fn from(value: PersistenceError) -> Self {
        let core = match &value {
            PersistenceError::NewerSchema { .. } => {
                CorePersistenceError::MigrationFailed(value.to_string())
            }
            _ => CorePersistenceError::SaveFailed(value.to_string()),
        };
        AppError::Persistence(core)
    }
}
