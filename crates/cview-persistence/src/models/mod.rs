//! 영속 모델과 DTO.
//!
//! 원본: `Sources/CViewPersistence/Models/` 디렉터리 전체와 `DataStore.swift` 의 DTO.
//! SwiftData @Model 클래스는 행 구조체(`id` + 속성)로, Sendable DTO 는 그대로 구조체로 옮겼다.

pub mod cached_clip;
pub mod cached_image_entry;
pub mod cached_live_channel;
pub mod cached_vod;
pub mod category_snapshot;
pub mod persisted_channel;
pub mod playback;
pub mod stream_bookmark;
pub mod watch_history;

pub use cached_clip::{CachedCategoryStat, CachedClip, CachedClipData};
pub use cached_image_entry::{CachedImageEntry, CachedImageEntryData, ImageTouch};
pub use cached_live_channel::{CachedLiveChannel, CachedLiveChannelData};
pub use cached_vod::{CachedVod, CachedVodData};
pub use category_snapshot::{CategorySnapshot, CategorySnapshotInput};
pub use persisted_channel::{
    ChannelListData, PersistedChannel, PersistedSetting, PersistedStatistic,
};
pub use playback::{
    PersistedClipPlayback, PersistedClipPlaybackData, PersistedVodPlayback,
    PersistedVodPlaybackData, PersistedVodWatchLater, PersistedVodWatchLaterData,
};
pub use stream_bookmark::{StreamBookmark, StreamBookmarkData};
pub use watch_history::{
    WatchHistory, WatchHistoryAggregationPolicy, WatchHistoryData, WatchHistorySyncRow,
};
