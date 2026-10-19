//! 채널·통계·설정 영속 모델.
//!
//! 원본: `Sources/CViewPersistence/Models/PersistedChannel.swift`
//! (`PersistedChannel`, `PersistedStatistic`, `PersistedSetting`) 와
//! `DataStore.swift` 의 `ChannelListData` DTO.

use crate::db::db_model;
use crate::time::{now, Date};
use cview_core::models::ChannelInfo;

db_model! {
    /// 채널 영속 모델. `statistics` 관계는 `PersistedStatistic.channel` 외래 키(cascade)로 표현한다.
    pub struct PersistedChannel in "PersistedChannel" {
        pub channel_id: String => "channelId",
        pub channel_name: String => "channelName",
        pub image_url: Option<String> => "imageURL",
        pub is_favorite: bool => "isFavorite",
        pub last_watched: Option<Date> => "lastWatched",
        pub added_at: Date => "addedAt",
        pub memo: Option<String> => "memo",
    }
}

impl PersistedChannel {
    /// Swift `init(channelId:channelName:imageURL:isFavorite:lastWatched:)` — `addedAt = .now`.
    pub fn new(
        channel_id: impl Into<String>,
        channel_name: impl Into<String>,
        image_url: Option<String>,
        is_favorite: bool,
        last_watched: Option<Date>,
    ) -> Self {
        Self {
            id: 0,
            channel_id: channel_id.into(),
            channel_name: channel_name.into(),
            image_url,
            is_favorite,
            last_watched,
            added_at: now(),
            memo: None,
        }
    }

    /// 도메인 모델로 변환.
    pub fn to_domain(&self) -> ChannelInfo {
        let mut info = ChannelInfo::new(self.channel_id.clone(), self.channel_name.clone());
        info.channel_image_url = self
            .image_url
            .as_deref()
            .and_then(|s| url::Url::parse(s).ok());
        info
    }
}

db_model! {
    /// 시청 통계 영속 모델.
    pub struct PersistedStatistic in "PersistedStatistic" {
        pub channel_id: String => "channelId",
        pub timestamp: Date => "timestamp",
        pub viewer_count: i64 => "viewerCount",
        pub watch_duration: f64 => "watchDuration",
        pub average_latency: Option<f64> => "averageLatency",
        /// 관계 `channel` — `PersistedChannel.id`.
        pub channel: Option<i64> => "channel",
    }
}

db_model! {
    /// 설정 영속 모델.
    pub struct PersistedSetting in "PersistedSetting" {
        pub key: String => "key",
        pub value_data: Vec<u8> => "valueData",
        pub updated_at: Date => "updatedAt",
    }
}

impl PersistedSetting {
    /// Swift `init(key:valueData:)` — `updatedAt = .now`.
    pub fn new(key: impl Into<String>, value_data: Vec<u8>) -> Self {
        Self {
            id: 0,
            key: key.into(),
            value_data,
            updated_at: now(),
        }
    }

    /// 값과 갱신 시각을 바꾼다. 호출자는 **값이 실제로 달라졌을 때만** 부른다
    /// (`DataStore+StoreMaintenance.swift` 의 `replaceValue(with:at:)`).
    pub fn replace_value(&mut self, data: Vec<u8>, at: Date) {
        self.value_data = data;
        self.updated_at = at;
    }
}

/// DataStore 에서 화면으로 전달되는 채널 데이터(`ChannelListData`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChannelListData {
    pub channel_id: String,
    pub channel_name: String,
    pub image_url: Option<String>,
    pub is_favorite: bool,
    pub last_watched: Option<Date>,
}

impl ChannelListData {
    /// `Identifiable.id`.
    pub fn id(&self) -> &str {
        &self.channel_id
    }
}

impl From<&PersistedChannel> for ChannelListData {
    fn from(c: &PersistedChannel) -> Self {
        Self {
            channel_id: c.channel_id.clone(),
            channel_name: c.channel_name.clone(),
            image_url: c.image_url.clone(),
            is_favorite: c.is_favorite,
            last_watched: c.last_watched,
        }
    }
}
