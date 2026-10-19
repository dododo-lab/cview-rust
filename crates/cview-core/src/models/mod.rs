//! `Sources/CViewCore/Models` 포팅.
//!
//! 채팅 모델(`chat`)은 별도 포팅 작업이 소유한다.

pub mod chat;

pub mod channel;
pub mod clip;
pub mod decode;
pub mod equalizer;
pub mod live;
pub mod live_catalog;
pub mod live_channel_item;
pub mod live_event_bus;
pub mod match_web_v2;
pub mod media;
pub mod player;
pub mod proxy_stats;
pub mod search;
pub mod settings;
pub mod settings_ui;
pub mod stream_alert;
pub mod stream_proxy;
pub mod vod;

pub use channel::{ChannelInfo, FollowingChannel, StreamerInfo};
pub use clip::{ClipDetail, ClipInkeyContent, ClipOptionalProperty, NaverVodPlayResponse};
pub use live::{
    ChannelViewerInfo, LiveInfo, LivePlayback, LiveStatus, StreamQuality, TimeMachinePlaybackInfo,
};
pub use live_channel_item::{format_compact_count, format_viewer_count, LiveChannelItem};
pub use media::{EncodingTrack, LivePlaybackDetail, MediaInfo};
pub use player::{PlayerError, PlayerPhase, PlayerState};
pub use search::{
    ChannelSearchItem, ClipInfo, LiveSearchItem, SearchResult, VideoSearchItem, VodInfo,
};
pub use stream_proxy::StreamProxyMode;
pub use vod::{VodDetail, VodStreamInfo};
