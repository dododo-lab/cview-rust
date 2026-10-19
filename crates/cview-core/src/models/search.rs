//! `Sources/CViewCore/Models/SearchModels.swift` 포팅 — 검색 도메인 모델과 VOD/클립 목록 항목.

use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use url::Url;

use super::channel::ChannelInfo;
use super::decode::{format_date, LenientObject};
use super::live::LiveInfo;

/// 검색 결과 타입.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchType {
    Channel,
    Live,
    Video,
    Clip,
}

impl SearchType {
    pub const ALL: [SearchType; 4] = [SearchType::Channel, SearchType::Live, SearchType::Video, SearchType::Clip];
}

/// 검색 결과 컨테이너.
///
/// [CViewCore 리뷰 2026-08-21 P2] `data` 는 원소 단위로 격리 디코딩한다 — 한 원소의 스키마가
/// 어긋나도 나머지는 살아남는다. ⚠ 배열 전체 디코딩으로 되돌리지 말 것.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult<T> {
    pub size: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<SearchPage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_count: Option<i64>,
    pub data: Vec<T>,
}

impl<T> Default for SearchResult<T> {
    fn default() -> Self {
        Self { size: 0, page: None, total_count: None, data: Vec::new() }
    }
}

impl<T> SearchResult<T> {
    /// `data` 만 다른 타입으로 바꾼다 (클라이언트의 래퍼 → 도메인 변환에 쓴다).
    pub fn map<U>(self, f: impl FnMut(T) -> U) -> SearchResult<U> {
        SearchResult { size: self.size, page: self.page, total_count: self.total_count, data: self.data.into_iter().map(f).collect() }
    }
}

impl<'de, T: DeserializeOwned> Deserialize<'de> for SearchResult<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let o = LenientObject::from_deserializer(d)?;
        Ok(Self {
            size: o.get("size").unwrap_or(0),
            total_count: o.get("totalCount"),
            data: o.array("data"),
            // page 는 Object 또는 null — null/부재/형식 오류 모두 None
            page: o.get("page"),
        })
    }
}

/// 검색 페이지 정보 (API 에서 Object 로 반환).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchPage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<SearchPageOffset>,
}

/// 검색 페이지 오프셋.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchPageOffset {
    pub offset: i64,
}

/// 채널 검색 결과 아이템 래퍼 (API 에서 `{"channel": {...}}` 형태로 반환).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelSearchItem {
    pub channel: ChannelInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_live: Option<bool>,
}

/// 라이브 검색 결과 아이템 래퍼 (`{"live": {...}, "channel": {...}}`).
///
/// 디코딩 시 `channel` 을 `live.channel` 에 주입한다. 원본은 `LiveInfo` 를 일부 필드만으로 다시
/// 만들어 워치파티·타임머신 등 일부 필드를 기본값으로 떨어뜨렸는데, Rust 포트는 나머지 필드를
/// 그대로 보존하고 `channel` 만 바꾼다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LiveSearchItem {
    pub live: LiveInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<ChannelInfo>,
}

impl<'de> Deserialize<'de> for LiveSearchItem {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let o = LenientObject::from_deserializer(d)?;
        let mut live: LiveInfo = o.required::<_, D::Error>("live")?;
        let channel: Option<ChannelInfo> = o.get("channel");
        if channel.is_some() {
            live.channel = channel.clone();
        }
        Ok(Self { live, channel })
    }
}

/// 비디오 검색 결과 아이템 래퍼 (`{"video": {...}, "channel": {...}}`). `channel` 을 `video` 에 주입한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VideoSearchItem {
    pub video: VodInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<ChannelInfo>,
}

impl<'de> Deserialize<'de> for VideoSearchItem {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let o = LenientObject::from_deserializer(d)?;
        let mut video: VodInfo = o.required::<_, D::Error>("video")?;
        let channel: Option<ChannelInfo> = o.get("channel");
        if channel.is_some() {
            video.channel = channel.clone();
        }
        Ok(Self { video, channel })
    }
}

/// VOD 정보 (Swift `VODInfo`).
///
/// 썸네일은 `thumbnailImageUrl`(검색/목록 표준) 우선, 없으면 `videoImageUrl`(상세) 을 받는다.
/// 인코딩은 `thumbnailImageUrl` 로 쓴다.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VodInfo {
    pub video_no: i64,
    pub video_id: Option<String>,
    pub video_title: String,
    pub video_image_url: Option<Url>,
    pub duration: i64,
    pub publish_date: Option<DateTime<Utc>>,
    pub read_count: i64,
    pub channel: Option<ChannelInfo>,
}

impl VodInfo {
    pub fn new(video_no: i64, video_title: impl Into<String>) -> Self {
        Self {
            video_no,
            video_id: None,
            video_title: video_title.into(),
            video_image_url: None,
            duration: 0,
            publish_date: None,
            read_count: 0,
            channel: None,
        }
    }

    pub fn id(&self) -> i64 {
        self.video_no
    }

    /// `H:MM:SS` 또는 `M:SS`.
    pub fn formatted_duration(&self) -> String {
        let hours = self.duration / 3600;
        let minutes = (self.duration % 3600) / 60;
        let seconds = self.duration % 60;
        if hours > 0 {
            format!("{hours}:{minutes:02}:{seconds:02}")
        } else {
            format!("{minutes}:{seconds:02}")
        }
    }
}

impl<'de> Deserialize<'de> for VodInfo {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let o = LenientObject::from_deserializer(d)?;
        Ok(Self {
            video_no: o.required::<_, D::Error>("videoNo")?,
            video_id: o.optional::<_, D::Error>("videoId")?,
            video_title: o.optional::<_, D::Error>("videoTitle")?.unwrap_or_default(),
            video_image_url: o.url("thumbnailImageUrl").or_else(|| o.url("videoImageUrl")),
            duration: o.optional::<_, D::Error>("duration")?.unwrap_or(0),
            publish_date: o.optional_date::<D::Error>("publishDate")?,
            read_count: o.optional::<_, D::Error>("readCount")?.unwrap_or(0),
            channel: o.optional::<_, D::Error>("channel")?,
        })
    }
}

impl Serialize for VodInfo {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(None)?;
        m.serialize_entry("videoNo", &self.video_no)?;
        if let Some(v) = &self.video_id {
            m.serialize_entry("videoId", v)?;
        }
        m.serialize_entry("videoTitle", &self.video_title)?;
        if let Some(v) = &self.video_image_url {
            m.serialize_entry("thumbnailImageUrl", v)?;
        }
        m.serialize_entry("duration", &self.duration)?;
        if let Some(v) = &self.publish_date {
            m.serialize_entry("publishDate", &format_date(v))?;
        }
        m.serialize_entry("readCount", &self.read_count)?;
        if let Some(v) = &self.channel {
            m.serialize_entry("channel", v)?;
        }
        m.end()
    }
}

/// 클립 정보.
///
/// API 응답 키 매핑:
///   - clip list: `clipUID`(대문자), `ownerChannel`
///   - clip detail: `clipUid`(소문자), `channel`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClipInfo {
    pub clip_uid: String,
    pub clip_title: String,
    pub thumbnail_image_url: Option<Url>,
    pub clip_url: Option<Url>,
    pub duration: i64,
    pub read_count: i64,
    pub created_date: Option<DateTime<Utc>>,
    pub channel: Option<ChannelInfo>,
}

impl ClipInfo {
    pub fn new(clip_uid: impl Into<String>, clip_title: impl Into<String>) -> Self {
        Self {
            clip_uid: clip_uid.into(),
            clip_title: clip_title.into(),
            thumbnail_image_url: None,
            clip_url: None,
            duration: 0,
            read_count: 0,
            created_date: None,
            channel: None,
        }
    }

    pub fn id(&self) -> &str {
        &self.clip_uid
    }
}

impl<'de> Deserialize<'de> for ClipInfo {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let o = LenientObject::from_deserializer(d)?;
        let clip_uid = o
            .get::<String>("clipUID")
            .filter(|s| !s.is_empty())
            .or_else(|| o.get::<String>("clipUid").filter(|s| !s.is_empty()))
            .unwrap_or_default();
        Ok(Self {
            clip_uid,
            clip_title: o.get("clipTitle").unwrap_or_default(),
            thumbnail_image_url: o.url("thumbnailImageUrl"),
            clip_url: o.url("clipUrl"),
            duration: o.get("duration").unwrap_or(0),
            read_count: o.get("readCount").unwrap_or(0),
            created_date: o.date("createdDate"),
            channel: o.get("ownerChannel").or_else(|| o.get("channel")),
        })
    }
}

impl Serialize for ClipInfo {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(None)?;
        m.serialize_entry("clipUID", &self.clip_uid)?;
        m.serialize_entry("clipTitle", &self.clip_title)?;
        if let Some(v) = &self.thumbnail_image_url {
            m.serialize_entry("thumbnailImageUrl", v)?;
        }
        if let Some(v) = &self.clip_url {
            m.serialize_entry("clipUrl", v)?;
        }
        m.serialize_entry("duration", &self.duration)?;
        m.serialize_entry("readCount", &self.read_count)?;
        if let Some(v) = &self.created_date {
            m.serialize_entry("createdDate", &format_date(v))?;
        }
        if let Some(v) = &self.channel {
            m.serialize_entry("channel", v)?;
        }
        m.end()
    }
}

/// 검색 자동완성 제안 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AutocompleteSuggestionKind {
    Recent,
    Following,
    /// [SEARCH-1 2026-09-29] 최근 시청 채널 — 네트워크 요청 없이 로컬에서 나온다.
    Watched,
}

impl AutocompleteSuggestionKind {
    /// Swift 케이스 이름 (안정 id 에 쓴다).
    pub fn name(self) -> &'static str {
        match self {
            Self::Recent => "recent",
            Self::Following => "following",
            Self::Watched => "watched",
        }
    }

    /// 드롭다운 행 앞 아이콘(SF Symbol 이름 — UI 계층이 매핑한다).
    pub fn system_image(self) -> &'static str {
        match self {
            Self::Recent => "clock",
            Self::Following => "person.circle",
            Self::Watched => "play.circle",
        }
    }

    /// 행 끝 출처 라벨. 최근 검색어는 아이콘만으로 충분해 라벨이 없다.
    pub fn source_label(self) -> Option<&'static str> {
        match self {
            Self::Recent => None,
            Self::Following => Some("팔로잉"),
            Self::Watched => Some("최근 시청"),
        }
    }
}

/// 검색 자동완성 제안 항목.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AutocompleteSuggestion {
    pub text: String,
    pub kind: AutocompleteSuggestionKind,
}

impl AutocompleteSuggestion {
    pub fn new(text: impl Into<String>, kind: AutocompleteSuggestionKind) -> Self {
        Self { text: text.into(), kind }
    }

    /// [2026-09-19] 값에서 파생된 안정 식별자 (`"{kind}|{text}"`).
    /// ⚠ 무작위 UUID 로 되돌리지 말 것 — 행 정체성이 매 프레임 바뀌는 회귀가 된다.
    pub fn id(&self) -> String {
        format!("{}|{}", self.kind.name(), self.text)
    }
}

/// 라이브 검색 결과 정렬 옵션.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LiveSortOption {
    ViewerCount,
    Recent,
}

impl LiveSortOption {
    pub const ALL: [LiveSortOption; 2] = [LiveSortOption::ViewerCount, LiveSortOption::Recent];

    pub fn raw_value(self) -> &'static str {
        match self {
            Self::ViewerCount => "시청자 많은순",
            Self::Recent => "최신순",
        }
    }
}

#[cfg(test)]
mod tests {
    //! `Tests/CViewCoreTests/SearchModelsTests.swift` 의 디코딩 계약을 옮겼다.
    use super::*;

    #[test]
    fn search_result_isolates_bad_elements() {
        let json = r#"{"size":3,"totalCount":10,"page":{"next":{"offset":20}},
            "data":[{"channel":{"channelId":"a","channelName":"A"}},{"channel":{"channelName":"no id"}},
                    {"channel":{"channelId":"c","channelName":"C"}}]}"#;
        let r: SearchResult<ChannelSearchItem> = serde_json::from_str(json).unwrap();
        assert_eq!(r.size, 3);
        assert_eq!(r.total_count, Some(10));
        assert_eq!(r.page.unwrap().next.unwrap().offset, 20);
        assert_eq!(r.data.len(), 2);
        assert_eq!(r.data[1].channel.channel_id, "c");
    }

    #[test]
    fn search_result_defaults() {
        let r: SearchResult<ChannelSearchItem> = serde_json::from_str(r#"{"page":null}"#).unwrap();
        assert_eq!(r.size, 0);
        assert!(r.page.is_none());
        assert!(r.data.is_empty());
    }

    #[test]
    fn live_search_item_injects_channel() {
        let item: LiveSearchItem = serde_json::from_str(
            r#"{"live":{"liveId":1,"liveTitle":"t","watchPartyNo":3},"channel":{"channelId":"c","channelName":"n"}}"#,
        )
        .unwrap();
        assert_eq!(item.live.channel.as_ref().unwrap().channel_id, "c");
        assert_eq!(item.live.watch_party_no, Some(3));
        assert!(serde_json::from_str::<LiveSearchItem>(r#"{"channel":null}"#).is_err());
    }

    #[test]
    fn video_search_item_injects_channel() {
        let item: VideoSearchItem = serde_json::from_str(
            r#"{"video":{"videoNo":7,"videoTitle":"v","thumbnailImageUrl":"https://i.example/t.jpg"},"channel":{"channelId":"c","channelName":"n"}}"#,
        )
        .unwrap();
        assert_eq!(item.video.video_no, 7);
        assert_eq!(item.video.channel.unwrap().channel_name, "n");
    }

    #[test]
    fn vod_info_image_fallback_and_round_trip() {
        let v: VodInfo =
            serde_json::from_str(r#"{"videoNo":1,"videoImageUrl":"https://i.example/detail.jpg","publishDate":"2024-01-15T12:00:00Z"}"#)
                .unwrap();
        assert_eq!(v.video_image_url.as_ref().unwrap().as_str(), "https://i.example/detail.jpg");
        assert_eq!(v.video_title, "");
        let json = serde_json::to_value(&v).unwrap();
        assert!(json.get("thumbnailImageUrl").is_some());
        let back: VodInfo = serde_json::from_value(json).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn vod_info_strictness() {
        assert!(serde_json::from_str::<VodInfo>(r#"{"videoTitle":"x"}"#).is_err());
        assert!(serde_json::from_str::<VodInfo>(r#"{"videoNo":1,"duration":"long"}"#).is_err());
    }

    #[test]
    fn vod_formatted_duration() {
        let mut v = VodInfo::new(1, "t");
        v.duration = 3725;
        assert_eq!(v.formatted_duration(), "1:02:05");
        v.duration = 65;
        assert_eq!(v.formatted_duration(), "1:05");
    }

    #[test]
    fn clip_info_both_key_styles() {
        let list: ClipInfo = serde_json::from_str(
            r#"{"clipUID":"U1","clipTitle":"t","ownerChannel":{"channelId":"o","channelName":"O"}}"#,
        )
        .unwrap();
        assert_eq!(list.clip_uid, "U1");
        assert_eq!(list.channel.as_ref().unwrap().channel_id, "o");
        let detail: ClipInfo =
            serde_json::from_str(r#"{"clipUid":"u2","channel":{"channelId":"d","channelName":"D"}}"#).unwrap();
        assert_eq!(detail.clip_uid, "u2");
        assert_eq!(detail.channel.as_ref().unwrap().channel_id, "d");
        let empty: ClipInfo = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.clip_uid, "");
        let back: ClipInfo = serde_json::from_value(serde_json::to_value(&detail).unwrap()).unwrap();
        assert_eq!(back, detail);
    }

    #[test]
    fn autocomplete_stable_id() {
        let a = AutocompleteSuggestion::new("롤", AutocompleteSuggestionKind::Following);
        assert_eq!(a.id(), "following|롤");
        assert_eq!(a.id(), AutocompleteSuggestion::new("롤", AutocompleteSuggestionKind::Following).id());
        assert_eq!(AutocompleteSuggestionKind::Recent.source_label(), None);
        assert_eq!(AutocompleteSuggestionKind::Watched.source_label(), Some("최근 시청"));
    }

    #[test]
    fn search_type_raw() {
        assert_eq!(serde_json::to_string(&SearchType::Channel).unwrap(), "\"channel\"");
        assert_eq!(LiveSortOption::ViewerCount.raw_value(), "시청자 많은순");
    }
}
