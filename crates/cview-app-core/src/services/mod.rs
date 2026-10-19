//! `Sources/CViewApp/Services` 포팅 (+ 앱 루트의 `AppDependencies`·`AppLifecycle`·
//! `SingleInstanceGuard`).

pub mod app_lifecycle;
pub mod background_update;
pub mod boot_progress;
pub mod channel_header_hub;
pub mod channel_preset_store;
pub mod download_history_store;
pub mod mention_inbox;
pub mod menu_transition_gate;
pub mod multi_chat_preferences;
pub mod multi_chat_workspace_store;
pub mod multi_live_proposer;
pub mod network_path_monitor;
pub mod notification_service;
pub mod pinned_notice_cache;
pub mod refresh_coordinator;
pub mod single_instance_guard;
pub mod support;
pub mod update_service;
pub mod vod_feed;
pub mod vod_playback_progress;
pub mod vod_sync_state;
pub mod vod_sync_store;
pub mod watch_heartbeat;
