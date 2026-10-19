//! `Sources/CViewApp/Caching` 포팅 (+ `Persistence/WorkspaceSnapshot.swift`).
//!
//! | Swift 원본 | Rust |
//! |---|---|
//! | `Caching/FavoritesStore.swift` | [`favorites_store`] |
//! | `Caching/FollowingStore.swift` | [`following_store`] |
//! | `Caching/RecentChannelsStore.swift` | [`recent_channels_store`] |
//! | `Caching/WorkspaceValueCache.swift` | [`workspace_value_cache`] |
//! | `Persistence/WorkspaceSnapshot.swift` | [`workspace_snapshot`] |

pub mod favorites_store;
pub mod following_store;
pub mod recent_channels_store;
pub mod workspace_snapshot;
pub mod workspace_value_cache;
