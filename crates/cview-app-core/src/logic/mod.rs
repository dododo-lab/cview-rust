//! `Sources/CViewApp/Logic` 포팅 (+ `Models/EffectivePlaybackPolicy`, `Playback/PlaybackGuardrail`).
//!
//! | Swift 원본 | Rust |
//! |---|---|
//! | `Logic/CategoryGroupsCompute.swift` | [`category_groups`] |
//! | `Logic/FollowingBrowserQuery.swift`, `FollowingBrowserSnapshotCache.swift` | [`following_browser_query`] |
//! | `Logic/FollowingChannelSidebarCache.swift` | [`following_channel_sidebar`] |
//! | `Logic/FollowingOverviewListQuery.swift` | [`following_overview_list_query`] |
//! | `Logic/MergedChatMergeState.swift` | [`merged_chat_merge_state`] |
//! | `Logic/PinnedCategoryDecoder.swift` | [`pinned_category_decoder`] |
//! | `Models/EffectivePlaybackPolicy.swift` | [`effective_playback_policy`] |
//! | `Playback/PlaybackGuardrail.swift` | [`playback_guardrail`] |

pub mod category_groups;
pub mod effective_playback_policy;
pub mod following_browser_query;
pub mod following_channel_sidebar;
pub mod following_overview_list_query;
pub mod merged_chat_merge_state;
pub mod pinned_category_decoder;
pub mod playback_guardrail;
