//! `Sources/CViewApp/Actions` 포팅.
//!
//! | Swift | Rust |
//! |---|---|
//! | `CViewAction.swift` | [`cview_action`] |
//! | `CViewActionRegistry.swift` | [`registry`] |
//! | `SidebarQuickAction.swift`, `SidebarQuickActionExecutor.swift` | [`sidebar_quick_action`] |
//! | `ClipboardActions.swift`, `SupportActions.swift`, `SystemSettingsActions.swift` | [`platform`] |

pub mod cview_action;
pub mod platform;
pub mod registry;
pub mod sidebar_quick_action;

pub use cview_action::{CViewAction, LiveOpenMode};
pub use registry::{ActionHost, CViewActionRegistry};
pub use sidebar_quick_action::{
    PopularMode, QuickActionHost, SearchScope, SidebarPresetDraft, SidebarQuickAction,
    SidebarQuickActionExecutor,
};
