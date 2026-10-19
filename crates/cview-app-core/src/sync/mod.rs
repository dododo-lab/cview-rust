//! `Sources/CViewApp/Sync` 포팅.
//!
//! | Swift 원본 | Rust |
//! |---|---|
//! | `Sync/InstallIdentity.swift` | [`install_identity`] |
//! | `Sync/WatchHistorySyncService.swift`, `AppState+Sync.swift` | [`watch_history_sync`] |

pub mod install_identity;
pub mod watch_history_sync;

pub use install_identity::InstallIdentity;
pub use watch_history_sync::{
    SyncConfig, WatchHistorySyncService, WatchHistorySyncStatus, WatchHistorySyncStore,
};
