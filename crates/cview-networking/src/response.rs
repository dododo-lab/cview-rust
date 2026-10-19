//! `Sources/CViewNetworking/APIResponse.swift` 포팅 — 치지직 API 응답 래퍼와 목록/팔로잉 응답 타입.
//!
//! Swift `JSONDecoder.chzzk` 의 날짜 전략은 `cview_core::models::decode` 가 담당한다.

use chrono::{DateTime, Utc};
use cview_core::models::decode::LenientObject;
use cview_core::models::{LiveChannelItem, LiveInfo};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

/// 치지직 API 표준 응답 래퍼 (`code`, `message`, `content`).
///
/// 방어적 디코딩: `code` 누락/형식 오류 → `-1`, `message` 실패 → `None`,
/// `content` 누락·`null`·디코딩 실패 → `None` (경고 로그).
#[derive(Debug, Clone, PartialEq)]
pub struct ChzzkResponse<T> {
    pub code: i64,
    pub message: Option<String>,
    pub content: Option<T>,
}

impl<T> ChzzkResponse<T> {
    pub fn new(code: i64, message: Option<String>, content: Option<T>) -> Self {
        Self {
            code,
            message,
            content,
        }
    }
}

impl<'de, T: DeserializeOwned> Deserialize<'de> for ChzzkResponse<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let o = LenientObject::from_deserializer(deserializer)?;
        let code = o.get::<i64>("code").unwrap_or_else(|| {
            tracing::warn!("ChzzkResponse: 'code' 필드 누락, 기본값 -1 사용");
            -1
        });
        let message = o.get::<String>("message");
        let content = match o.raw("content") {
            None => {
                tracing::warn!("ChzzkResponse: 'content' 필드 누락");
                None
            }
            Some(Value::Null) => None,
            Some(v) => match T::deserialize(v) {
                Ok(c) => Some(c),
                Err(e) => {
                    tracing::warn!("ChzzkResponse: 'content' 디코딩 실패 ({e}), nil 처리");
                    None
                }
            },
        };
        Ok(Self {
            code,
            message,
            content,
        })
    }
}

/// `page` 메타데이터 상태 — 페이지 디코딩 실패를 자연 종료와 합치지 않기 위한 완전성 정보.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PageMetadataState {
    /// 응답에 `page` 키가 없었다.
    Missing,
    /// 응답이 `page: null` 로 cursor 종료를 명시했다.
    Terminal,
    /// `page` 객체를 스키마에 맞게 디코드했다. `next == None` 은 이 상태에서만 정상 끝이다.
    Decoded,
    /// `page` 키는 있었지만 커서 스키마 디코딩에 실패했다.
    Malformed,
}

/// 페이지네이션 응답.
#[derive(Debug, Clone, PartialEq)]
pub struct PagedContent<T> {
    pub size: i64,
    /// `size` 가 응답에서 실제 정수로 디코드됐는지 여부. 누락/타입 오류를 0과 구분한다.
    pub is_size_metadata_decoded: bool,
    pub total_count: Option<i64>,
    pub data: Vec<T>,
    /// 원소 단위 격리 과정에서 건너뛴 `data` 원소 수. 0이 아니면 목록은 부분 결과다.
    pub data_decoding_failure_count: usize,
    /// 라이브 목록 커서 기반 다음 페이지 정보.
    pub page: Option<LivePageInfo>,
    pub page_metadata_state: PageMetadataState,
}

impl<T> PagedContent<T> {
    /// Swift memberwise init 과 같은 기본값 — `page` 가 있으면 `Decoded`, 없으면 `Missing`.
    pub fn new(
        size: i64,
        total_count: Option<i64>,
        data: Vec<T>,
        page: Option<LivePageInfo>,
    ) -> Self {
        let state = if page.is_some() {
            PageMetadataState::Decoded
        } else {
            PageMetadataState::Missing
        };
        Self {
            size,
            is_size_metadata_decoded: true,
            total_count,
            data,
            data_decoding_failure_count: 0,
            page,
            page_metadata_state: state,
        }
    }
}

impl<'de, T: DeserializeOwned> Deserialize<'de> for PagedContent<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let o = LenientObject::from_deserializer(deserializer)?;
        let (size, is_size_metadata_decoded) = match o.get::<i64>("size") {
            Some(s) => (s, true),
            None => (0, false),
        };
        let total_count = o.get::<i64>("totalCount");
        let report = o.array_report::<T>("data");
        let (page, page_metadata_state) = match o.raw("page") {
            None => (None, PageMetadataState::Missing),
            Some(Value::Null) => (None, PageMetadataState::Terminal),
            Some(v) => match LivePageInfo::deserialize(v) {
                Ok(p) => (Some(p), PageMetadataState::Decoded),
                Err(_) => (None, PageMetadataState::Malformed),
            },
        };
        Ok(Self {
            size,
            is_size_metadata_decoded,
            total_count,
            data: report.values,
            data_decoding_failure_count: report.failure_count,
            page,
            page_metadata_state,
        })
    }
}

/// `next` 커서 메타데이터 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NextMetadataState {
    Missing,
    Terminal,
    Decoded,
    Malformed,
}

/// 라이브 목록 커서 페이지네이션 정보.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LivePageInfo {
    pub next: Option<LivePageCursor>,
    pub prev: Option<LivePageCursor>,
    pub next_metadata_state: NextMetadataState,
}

impl LivePageInfo {
    /// `next` 가 있으면 `Decoded`, 없으면 `Terminal`.
    pub fn new(next: Option<LivePageCursor>, prev: Option<LivePageCursor>) -> Self {
        let state = if next.is_some() {
            NextMetadataState::Decoded
        } else {
            NextMetadataState::Terminal
        };
        Self {
            next,
            prev,
            next_metadata_state: state,
        }
    }
}

impl<'de> Deserialize<'de> for LivePageInfo {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let o = LenientObject::from_deserializer(deserializer)?;
        let prev = o.get::<LivePageCursor>("prev");
        let (next, next_metadata_state) = match o.raw("next") {
            None => (None, NextMetadataState::Missing),
            Some(Value::Null) => (None, NextMetadataState::Terminal),
            Some(v) => match LivePageCursor::deserialize(v) {
                Ok(c) => (Some(c), NextMetadataState::Decoded),
                Err(_) => (None, NextMetadataState::Malformed),
            },
        };
        Ok(Self {
            next,
            prev,
            next_metadata_state,
        })
    }
}

/// 라이브 목록 커서 (concurrentUserCount + liveId).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LivePageCursor {
    #[serde(default)]
    pub concurrent_user_count: Option<i64>,
    #[serde(default)]
    pub live_id: Option<i64>,
}

/// 전체 라이브 수집 진행률.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AllLivesProgress {
    pub current_count: usize,
    pub estimated_total: Option<i64>,
    pub current_page: usize,
    pub deduplicated_count: usize,
}

/// 전수 수집을 이어 받기 위한 재개 지점 (NET-4).
///
/// `paused`·`cancelled`·네트워크 `partialFailure` 로 멈췄을 때만 채워진다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AllLivesResumePoint {
    pub cursor_concurrent_user_count: Option<i64>,
    pub cursor_live_id: Option<i64>,
    /// 지금까지 순서 검증을 통과한 마지막 liveId.
    pub last_ordered_live_id: Option<i64>,
    /// 재개 전 조각까지 누적으로 응답을 받은 페이지 수.
    pub received_pages: usize,
}

impl AllLivesResumePoint {
    /// 다음 요청 커서. 두 값이 모두 없으면 처음부터 받는다.
    pub fn cursor(&self) -> Option<LivePageCursor> {
        if self.cursor_concurrent_user_count.is_none() && self.cursor_live_id.is_none() {
            return None;
        }
        Some(LivePageCursor {
            concurrent_user_count: self.cursor_concurrent_user_count,
            live_id: self.cursor_live_id,
        })
    }
}

/// 전체 라이브 수집 루프 종료 사유.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AllLivesStopReason {
    /// 커서가 nil 이 되어 정상적으로 마지막 페이지에 도달.
    ReachedEnd,
    /// 비상 상한에 도달하여 cursor 가 남은 채 절단.
    ReachedPageLimit,
    /// 서버가 이미 방문한 커서를 다시 반환해 순환이 감지됨.
    CursorCycle { page: usize },
    /// 안정 정렬 sweep 에서 예상 밖 중복 또는 서버 총계 충돌이 남음.
    UnstableOrdering {
        attempts: usize,
        duplicate_count: usize,
    },
    /// 중간 페이지가 재시도까지 실패해 부분 결과 반환.
    PartialFailure { page: usize, message: String },
    /// 작업이 취소되어 중단.
    Cancelled,
    /// 호출자의 `should_pause` 요청으로 중단 (NET-4).
    Paused,
    /// `boundary_live_id` 이하 원소를 만나 그 앞까지만 받고 종료 (NET-4).
    ReachedBoundary { live_id: i64 },
}

/// 전체 라이브 수집 결과 — 완전성 메타데이터 포함.
#[derive(Debug, Clone, PartialEq)]
pub struct AllLivesCollectionResult {
    pub items: Vec<LiveInfo>,
    /// API 첫 페이지의 `totalCount` (있을 때만).
    pub estimated_total: Option<i64>,
    /// 호출 시도된 페이지 수.
    pub requested_pages: usize,
    /// 응답을 받은 페이지 수 (재시도 실패한 페이지는 미포함).
    pub received_pages: usize,
    /// 페이지 경계에서 제거된 중복 라이브 수.
    pub deduplicated_count: usize,
    pub stop_reason: AllLivesStopReason,
    /// 안정 cursor traversal 검증을 통과한 관측 총계.
    pub verified_observed_count: Option<usize>,
    /// `LATEST` 계약을 실제 응답 전체에서 확인했는지 여부.
    pub stable_cursor_ordering_verified: bool,
    /// 이어 받을 지점 — paused·cancelled·네트워크 partialFailure 일 때만.
    pub resume_point: Option<AllLivesResumePoint>,
    /// 이번 순회에서 채택한 첫 원소의 liveId.
    pub first_live_id: Option<i64>,
}

impl AllLivesCollectionResult {
    /// 전체 수집이 자연 종료된 경우에만 true.
    pub fn is_complete(&self) -> bool {
        matches!(self.stop_reason, AllLivesStopReason::ReachedEnd)
    }

    /// 부분/절단/실패 — paused·reachedBoundary·cancelled 는 호출자 요청 중단이라 false.
    pub fn is_partial(&self) -> bool {
        matches!(
            self.stop_reason,
            AllLivesStopReason::ReachedPageLimit
                | AllLivesStopReason::CursorCycle { .. }
                | AllLivesStopReason::UnstableOrdering { .. }
                | AllLivesStopReason::PartialFailure { .. }
        )
    }

    /// 최신순 불변 cursor 로 terminal 까지 손실 없이 순회했다는 검증을 통과한 값만 노출한다.
    pub fn exact_live_count(&self) -> Option<usize> {
        match self.stop_reason {
            AllLivesStopReason::ReachedEnd => self.verified_observed_count,
            _ => None,
        }
    }
}

/// 팔로잉 API 응답.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FollowingContent {
    /// 원소 단위 격리 — 한 채널 스키마가 어긋나도 목록 전체가 비지 않는다.
    pub following_list: Option<Vec<FollowingItem>>,
    pub total_count: Option<i64>,
    pub size: Option<i64>,
    pub page: Option<FollowingPage>,
}

impl<'de> Deserialize<'de> for FollowingContent {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let o = LenientObject::from_deserializer(deserializer)?;
        Ok(Self {
            // Swift `SafeDecoding.decodeArray` 는 non-optional 배열을 돌려준다 → 항상 Some.
            following_list: Some(o.array::<FollowingItem>("followingList")),
            total_count: o.get("totalCount"),
            size: o.get("size"),
            page: o.get("page"),
        })
    }
}

/// 팔로잉 페이지네이션 정보.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct FollowingPage {
    #[serde(default)]
    pub next: Option<FollowingPageOffset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct FollowingPageOffset {
    #[serde(default)]
    pub offset: Option<i64>,
}

/// 팔로잉 개별 항목. `streamer`·`channel` 형식이 어긋나면 항목이 빠지고,
/// `liveInfo` 는 보조 값이라 어긋나도 항목을 버리지 않는다.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FollowingItem {
    pub streamer: Option<FollowingStreamer>,
    pub channel: Option<FollowingChannelData>,
    pub live_info: Option<FollowingListLiveInfo>,
}

impl<'de> Deserialize<'de> for FollowingItem {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let o = LenientObject::from_deserializer(deserializer)?;
        Ok(Self {
            streamer: o.optional::<_, D::Error>("streamer")?,
            channel: o.optional::<_, D::Error>("channel")?,
            live_info: o.get("liveInfo"),
        })
    }
}

/// 팔로잉 목록 항목에 실린 라이브 요약. 키마다 따로 읽는다(시청자 수가 문자열이면 그 키만 `None`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FollowingListLiveInfo {
    pub live_title: Option<String>,
    pub concurrent_user_count: Option<i64>,
    pub live_category_value: Option<String>,
}

impl<'de> Deserialize<'de> for FollowingListLiveInfo {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let o = LenientObject::from_deserializer(deserializer)?;
        Ok(Self {
            live_title: o.get("liveTitle"),
            concurrent_user_count: o.get("concurrentUserCount"),
            live_category_value: o.get("liveCategoryValue"),
        })
    }
}

impl FollowingListLiveInfo {
    /// 목록만으로 카드의 제목이나 시청자 수를 채울 수 있는가.
    pub fn has_list_fields(&self) -> bool {
        self.live_title.is_some() || self.concurrent_user_count.is_some()
    }
}

/// 팔로잉 목록 한 행 — 라이브 상세를 붙이기 전, 목록 응답만으로 만든 값.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FollowingRow {
    pub channel_id: String,
    pub channel_name: String,
    pub channel_image_url: Option<String>,
    /// 목록 응답의 라이브 여부(`FollowingStreamer::is_actually_live`).
    pub is_live: bool,
    pub list_live_info: Option<FollowingListLiveInfo>,
}

impl FollowingRow {
    /// 목록 값만으로 만든 카드. 라이브 행이면 목록의 제목·시청자·카테고리를 쓴다.
    pub fn list_channel_item(&self) -> LiveChannelItem {
        let info = if self.is_live {
            self.list_live_info.as_ref()
        } else {
            None
        };
        LiveChannelItem {
            id: self.channel_id.clone(),
            channel_name: self.channel_name.clone(),
            channel_image_url: self.channel_image_url.clone(),
            live_title: info.and_then(|i| i.live_title.clone()).unwrap_or_default(),
            viewer_count: info.and_then(|i| i.concurrent_user_count).unwrap_or(0),
            category_name: info.and_then(|i| i.live_category_value.clone()),
            category_type: None,
            thumbnail_url: None,
            channel_id: self.channel_id.clone(),
            is_live: self.is_live,
            open_date: None,
        }
    }
}

/// `ChzzkApiClient::fetch_following_rows` 의 결과.
#[derive(Debug, Clone, PartialEq)]
pub struct FollowingRowsPage {
    /// API 순서, channelId 중복 제거.
    pub rows: Vec<FollowingRow>,
    /// 서버가 알린 팔로잉 수(마지막으로 받은 페이지 기준).
    pub total_count: Option<i64>,
    /// 라이브 행 가운데 하나라도 목록에 제목이나 시청자 수를 실었다.
    pub carries_live_info: bool,
    /// 가장 오래된 페이지를 받은 시각(캐시·stale 값이면 그 저장 시각).
    pub fetched_at: DateTime<Utc>,
}

/// 팔로잉 스트리머 데이터.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowingStreamer {
    /// true = 방송 중, false = 방송 종료, None = 응답에 키 없음 (방송 중으로 간주)
    #[serde(default)]
    pub open_live: Option<bool>,
    /// 일부 API 버전에서 사용하는 대체 라이브 상태 필드.
    #[serde(default)]
    pub live_status: Option<String>,
}

impl FollowingStreamer {
    /// 라이브 여부 통합 판단: `liveStatus` 가 있으면 `"OPEN"` 여부, 없으면 `openLive ?? true`.
    pub fn is_actually_live(&self) -> bool {
        if let Some(status) = &self.live_status {
            return status == "OPEN";
        }
        self.open_live.unwrap_or(true)
    }
}

/// 팔로잉 채널 데이터.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowingChannelData {
    #[serde(default)]
    pub channel_id: Option<String>,
    #[serde(default)]
    pub channel_name: Option<String>,
    #[serde(default)]
    pub channel_image_url: Option<String>,
    #[serde(default)]
    pub verified_mark: Option<bool>,
}

/// 사용자 상태 정보 (`/service/v1/users/me`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserStatusInfo {
    #[serde(default)]
    pub has_profile: Option<bool>,
    #[serde(default)]
    pub user_id_hash: Option<String>,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(rename = "profileImageUrl", default)]
    pub profile_image_url: Option<String>,
    #[serde(default)]
    pub penalties: Option<Vec<String>>,
    #[serde(default)]
    pub logged_in: Option<bool>,
}

#[cfg(test)]
mod tests {
    //! `FollowingRowsDecodingTests.swift` · `CViewNetworkingTests.swift` 응답 디코딩 부분 포팅.
    use super::*;

    #[test]
    fn chzzk_response_defensive() {
        let r: ChzzkResponse<i64> =
            serde_json::from_str(r#"{"code":200,"message":null,"content":5}"#).unwrap();
        assert_eq!(r, ChzzkResponse::new(200, None, Some(5)));
        let missing: ChzzkResponse<i64> = serde_json::from_str(r#"{"content":"x"}"#).unwrap();
        assert_eq!(missing.code, -1);
        assert_eq!(missing.content, None);
        let no_content: ChzzkResponse<i64> =
            serde_json::from_str(r#"{"code":9004,"message":"m"}"#).unwrap();
        assert_eq!(no_content.message.as_deref(), Some("m"));
        assert!(serde_json::from_str::<ChzzkResponse<i64>>("[]").is_err());
    }

    #[test]
    fn paged_content_page_states() {
        let p: PagedContent<i64> = serde_json::from_str(r#"{"size":2,"data":[1,"x",3]}"#).unwrap();
        assert_eq!(p.data, vec![1, 3]);
        assert_eq!(p.data_decoding_failure_count, 1);
        assert_eq!(p.page_metadata_state, PageMetadataState::Missing);
        assert!(p.is_size_metadata_decoded);

        let t: PagedContent<i64> =
            serde_json::from_str(r#"{"size":"2","data":[],"page":null}"#).unwrap();
        assert_eq!(t.page_metadata_state, PageMetadataState::Terminal);
        assert!(!t.is_size_metadata_decoded);
        assert_eq!(t.size, 0);

        let d: PagedContent<i64> = serde_json::from_str(
            r#"{"data":[],"page":{"next":{"concurrentUserCount":5,"liveId":9}}}"#,
        )
        .unwrap();
        assert_eq!(d.page_metadata_state, PageMetadataState::Decoded);
        let page = d.page.unwrap();
        assert_eq!(
            page.next,
            Some(LivePageCursor {
                concurrent_user_count: Some(5),
                live_id: Some(9)
            })
        );
        assert_eq!(page.next_metadata_state, NextMetadataState::Decoded);

        let m: PagedContent<i64> = serde_json::from_str(r#"{"data":[],"page":"bad"}"#).unwrap();
        assert_eq!(m.page_metadata_state, PageMetadataState::Malformed);

        let missing_data: PagedContent<i64> = serde_json::from_str(r#"{"size":1}"#).unwrap();
        assert_eq!(missing_data.data_decoding_failure_count, 1);
    }

    #[test]
    fn live_page_info_next_states() {
        let p: LivePageInfo = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(p.next_metadata_state, NextMetadataState::Missing);
        let p: LivePageInfo = serde_json::from_str(r#"{"next":null}"#).unwrap();
        assert_eq!(p.next_metadata_state, NextMetadataState::Terminal);
        let p: LivePageInfo = serde_json::from_str(r#"{"next":{"liveId":"x"}}"#).unwrap();
        assert_eq!(p.next_metadata_state, NextMetadataState::Malformed);
        assert_eq!(
            LivePageInfo::new(None, None).next_metadata_state,
            NextMetadataState::Terminal
        );
    }

    #[test]
    fn following_rows_decoding() {
        let json = r#"{
            "totalCount": 3, "size": 50,
            "followingList": [
                {"channel":{"channelId":"a","channelName":"A"},"streamer":{"openLive":true},
                 "liveInfo":{"liveTitle":"t","concurrentUserCount":"12","liveCategoryValue":"롤"}},
                {"channel":{"channelId":"b","channelName":"B"},"streamer":{"openLive":false},"liveInfo":"bad"},
                {"channel":{"channelId":5}},
                {"channel":{"channelId":"c"},"streamer":{"liveStatus":"CLOSE","openLive":true}}
            ]
        }"#;
        let c: FollowingContent = serde_json::from_str(json).unwrap();
        let list = c.following_list.unwrap();
        assert_eq!(list.len(), 3, "형식이 어긋난 channel 항목만 빠진다");
        let first = &list[0];
        let info = first.live_info.as_ref().unwrap();
        assert_eq!(info.live_title.as_deref(), Some("t"));
        assert_eq!(
            info.concurrent_user_count, None,
            "문자열 시청자 수는 그 키만 None"
        );
        assert!(info.has_list_fields());
        assert!(first.streamer.as_ref().unwrap().is_actually_live());
        assert!(list[1].live_info.is_none());
        assert!(!list[2].streamer.as_ref().unwrap().is_actually_live());
        assert!(FollowingStreamer::default().is_actually_live());
        assert_eq!(c.total_count, Some(3));
    }

    #[test]
    fn following_row_list_item() {
        let row = FollowingRow {
            channel_id: "a".into(),
            channel_name: "A".into(),
            channel_image_url: None,
            is_live: true,
            list_live_info: Some(FollowingListLiveInfo {
                live_title: Some("t".into()),
                concurrent_user_count: Some(7),
                live_category_value: None,
            }),
        };
        let item = row.list_channel_item();
        assert_eq!(item.live_title, "t");
        assert_eq!(item.viewer_count, 7);
        let offline = FollowingRow {
            is_live: false,
            ..row
        };
        assert_eq!(offline.list_channel_item().live_title, "");
    }

    #[test]
    fn user_status_keys() {
        let u: UserStatusInfo = serde_json::from_str(
            r#"{"loggedIn":true,"nickname":"n","profileImageUrl":"p","userIdHash":"h"}"#,
        )
        .unwrap();
        assert_eq!(u.profile_image_url.as_deref(), Some("p"));
        assert_eq!(u.logged_in, Some(true));
    }

    #[test]
    fn collection_result_flags() {
        let mut r = AllLivesCollectionResult {
            items: vec![],
            estimated_total: None,
            requested_pages: 1,
            received_pages: 1,
            deduplicated_count: 0,
            stop_reason: AllLivesStopReason::ReachedEnd,
            verified_observed_count: Some(3),
            stable_cursor_ordering_verified: true,
            resume_point: None,
            first_live_id: None,
        };
        assert!(r.is_complete() && !r.is_partial());
        assert_eq!(r.exact_live_count(), Some(3));
        r.stop_reason = AllLivesStopReason::Paused;
        assert!(!r.is_partial());
        assert_eq!(r.exact_live_count(), None);
        r.stop_reason = AllLivesStopReason::ReachedPageLimit;
        assert!(r.is_partial());
        let rp = AllLivesResumePoint {
            cursor_concurrent_user_count: None,
            cursor_live_id: None,
            last_ordered_live_id: None,
            received_pages: 0,
        };
        assert_eq!(rp.cursor(), None);
    }
}
