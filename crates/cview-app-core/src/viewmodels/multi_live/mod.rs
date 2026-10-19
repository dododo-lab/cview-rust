//! 멀티라이브 — 여러 방송을 한 화면에서 보는 세션 관리.
//!
//! 원본: `Sources/CViewApp/ViewModels/MultiLive*.swift`
//! (`MultiLiveManager` 와 확장 7개, `MultiLiveSession`, `MultiLiveAddSessionResult`,
//! `MultiLiveLayoutModePolicy`).

pub mod add_session_result;
pub mod deps;
pub mod layout;
pub mod manager;
pub mod policy;
pub mod session;

pub use add_session_result::MultiLiveAddSessionResult;
pub use layout::{
    MultiLiveFocusDirection, MultiLiveGridLayoutMode, MultiLiveLayoutPrefs, MultiLiveLayoutRatios,
};
pub use manager::{
    AddSessionOptions, ManagerState, MultiLiveDeps, MultiLiveManager, MultiLiveManagerSnapshot,
};
pub use session::{MultiLiveLoadState, MultiLiveSession, MultiLiveSessionSnapshot};
