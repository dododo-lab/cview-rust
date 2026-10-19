//! `Sources/CViewApp/Persistence/WorkspaceSnapshot.swift` 포팅.
//!
//! 앱 종료/백그라운드 진입 시점의 워크스페이스 상태 스냅샷. 저장 위치는 키-값 저장소의
//! `"WorkspaceSnapshot.v1"`. 복원은 cold-start 직후 첫 활성화 1회.

use std::sync::Arc;

use cview_core::utils::time::Date;
use serde::{Deserialize, Serialize};

use crate::services::support::KeyValueStore;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    #[serde(default = "WorkspaceSnapshot::default_version")]
    pub version: i64,
    /// `SidebarItem` 원시값.
    #[serde(default)]
    pub sidebar: Option<String>,
    /// `FollowingHubMode` 원시값.
    #[serde(default)]
    pub hub_mode: Option<String>,
    #[serde(default)]
    pub multi_live_channel_ids: Vec<String>,
    #[serde(default)]
    pub multi_chat_channel_ids: Vec<String>,
    #[serde(default)]
    pub pending_search_query: Option<String>,
    #[serde(default)]
    pub smart_queue_channel_ids: Vec<String>,
    pub saved_at: Date,
}

impl WorkspaceSnapshot {
    fn default_version() -> i64 {
        1
    }

    pub fn new(saved_at: Date) -> Self {
        Self {
            version: 1,
            sidebar: None,
            hub_mode: None,
            multi_live_channel_ids: Vec::new(),
            multi_chat_channel_ids: Vec::new(),
            pending_search_query: None,
            smart_queue_channel_ids: Vec::new(),
            saved_at,
        }
    }
}

/// 원자적 저장 — 키 하나에 통째로 쓴다.
#[derive(Clone)]
pub struct WorkspaceStateStore {
    defaults: Arc<dyn KeyValueStore>,
}

impl WorkspaceStateStore {
    pub const DEFAULTS_KEY: &'static str = "WorkspaceSnapshot.v1";

    pub fn new(defaults: Arc<dyn KeyValueStore>) -> Self {
        Self { defaults }
    }

    /// 인코딩 실패는 조용히 무시 — 다음 호출에서 재시도.
    pub fn save(&self, snapshot: &WorkspaceSnapshot) {
        if let Ok(value) = serde_json::to_value(snapshot) {
            self.defaults.set_value(Self::DEFAULTS_KEY, value);
        }
    }

    pub fn load(&self) -> Option<WorkspaceSnapshot> {
        serde_json::from_value(self.defaults.value(Self::DEFAULTS_KEY)?).ok()
    }

    pub fn clear(&self) {
        self.defaults.remove(Self::DEFAULTS_KEY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::support::MemoryKeyValueStore;
    use cview_core::utils::time::from_unix;

    #[test]
    fn round_trips_and_clears() {
        let kv = MemoryKeyValueStore::shared();
        let store = WorkspaceStateStore::new(kv.clone());
        assert!(store.load().is_none());
        let mut snap = WorkspaceSnapshot::new(from_unix(1_700_000_000.0));
        snap.sidebar = Some("라이브".into());
        snap.multi_live_channel_ids = vec!["a".into()];
        store.save(&snap);
        assert_eq!(store.load(), Some(snap));
        kv.set_value(
            WorkspaceStateStore::DEFAULTS_KEY,
            serde_json::json!("garbage"),
        );
        assert!(store.load().is_none(), "손상된 값은 복원하지 않는다");
        store.clear();
        assert!(!kv.contains(WorkspaceStateStore::DEFAULTS_KEY));
    }
}
