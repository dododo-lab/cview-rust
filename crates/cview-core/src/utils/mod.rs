//! `Sources/CViewCore/Utilities` 의 순수 로직 포팅 (+ `Networking/ChzzkThumbnailTier.swift`).
//!
//! 플랫폼 종속 유틸(`Logging`·`PowerSourceMonitor`·`CursorHelper`·`DiagnosticResourceSampler` 등)은
//! 옮기지 않는다 — 로깅은 `tracing` 으로 대체한다.

pub mod auto_refresh_gate;
pub mod bounded_concurrent_work;
pub mod broadcast_display;
pub mod catalog_sweep;
pub mod channel_list_merge;
pub mod data_freshness;
pub mod ewma;
pub mod fast_date_parser;
pub mod fifo_memo_cache;
pub mod following_poll;
pub mod levenshtein;
pub mod live_detail_refresh;
pub mod local_data_deletion;
pub mod media_url_policy;
pub mod menu_scope_effect_ledger;
pub mod menu_transition_timing;
pub mod multi_chat_panel_width;
pub mod naver_cookie_policy;
pub mod perf_tuning;
pub mod pid;
pub mod refresh_schedule;
pub mod relative_time;
pub mod split_resize;
pub mod stable_hash;
pub mod system_load;
pub mod thumbnail_tier;
pub mod time;
pub mod watch_duration;

pub use auto_refresh_gate::{AutoRefreshGate, AutoRefreshHolder};
pub use ewma::EwmaCalculator;
pub use fifo_memo_cache::FifoMemoCache;
pub use menu_scope_effect_ledger::MenuScopeEffectLedger;
pub use pid::PidController;
pub use refresh_schedule::{RefreshContext, RefreshDemand, RefreshJobId, RefreshSchedule};
pub use system_load::SystemLoadMode;
