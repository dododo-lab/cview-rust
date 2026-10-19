//! `Sources/CViewApp/Navigation` 포팅 (+ `DeepLinkInbox.swift`, 허브 모드 enum).
//!
//! | Swift | Rust |
//! |---|---|
//! | `AppRouter.swift`, `AppRouter+SettingsGroup.swift` | [`router`] |
//! | `SidebarDestination.swift` | [`sidebar_destination`] |
//! | `DeepLinkRouter.swift` | [`deep_link`] |
//! | `DeepLinkInbox.swift` | [`deep_link_inbox`] |
//! | `PlaybackSurfaceScope.swift` | [`playback_surface`] |
//! | `ViewModels/FollowingViewState.swift` 의 모드·필터 enum | [`hub_mode`] |

pub mod deep_link;
pub mod deep_link_inbox;
pub mod hub_mode;
pub mod playback_surface;
pub mod router;
pub mod sidebar_destination;

pub use deep_link::{DeepLink, DeepLinkHost, DeepLinkRouter};
pub use deep_link_inbox::DeepLinkInbox;
pub use hub_mode::{
    FollowingBrowserFilter, FollowingBrowserSortOrder, FollowingHubMode, LiveDiscoverySection,
};
pub use playback_surface::{FrontmostPlaybackSurface, PlaybackSurfaceScope};
pub use router::{
    AppRoute, AppRouter, HomeHubMode, LibraryHubMode, SettingsGroup, SheetRoute, SidebarItem,
    SidebarVisibility,
};
pub use sidebar_destination::SidebarDestination;
