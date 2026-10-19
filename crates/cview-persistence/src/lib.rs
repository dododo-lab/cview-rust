//! CViewPersistence 포팅: SwiftData 대신 SQLite(rusqlite, bundled) 기반 저장소.
//!
//! 원본: `Sources/CViewPersistence/`. 모듈 대응:
//! - [`schema`] — `UserSchemaV1.swift`, `UserMigrationPlan.swift`, `StorePreflight`(`StoreOpenHealth.swift`)
//! - [`models`] — `Models/*.swift`
//! - [`store`] — `DataStore.swift` 와 `DataStore+*.swift` 확장 전부
//! - [`settings_store`] — `SettingsStore.swift`, `SettingsStore+SnapshotRestore.swift`
//! - [`settings_snapshot_file`] — `SettingsSnapshotFile.swift`
//! - [`user_record_file`] — `UserRecordFile.swift`
//! - [`download_history`] — `DownloadHistoryRecord.swift`
//! - [`watch_rollup_math`] — `WatchRollupMath.swift`
//! - [`store_open_health`] — `StoreOpenHealth.swift`
//! - [`store_backup`] — `StoreBackup.swift`
//! - [`revision`] — `CViewCore/Performance/WatchInsightsRevision.swift`(Rust core 미포팅분)
//!
//! 포팅하지 않은 것: 계측(`DataStore+Trace.swift` 의 `datastore_op` 캡처 줄·signpost,
//! `DataStoreQueueProbe.swift`, `StoreOpenTrace` 의 signpost·캡처 줄), macOS 파일 북마크 해석.

pub mod db;
pub mod download_history;
pub mod error;
pub mod json;
pub mod models;
pub mod revision;
pub mod schema;
pub mod settings_snapshot_file;
pub mod settings_store;
pub mod store;
pub mod store_backup;
pub mod store_open_health;
pub mod time;
pub mod user_record_file;
pub mod watch_rollup_math;

pub use error::{PersistenceError, Result};
pub use revision::WatchInsightsRevision;
pub use schema::StorePreflight;
pub use settings_snapshot_file::SettingsSnapshotFile;
pub use settings_store::{SettingsDomains, SettingsStore};
pub use store::DataStore;
pub use store_backup::StoreBackup;
pub use store_open_health::{StoreOpenHealth, StoreOpenHealthRecorder};
pub use user_record_file::UserRecordFile;
