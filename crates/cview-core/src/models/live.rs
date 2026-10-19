//! `Sources/CViewCore/Models/LiveInfo.swift` 포팅 — 라이브 방송 정보 도메인 모델.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use url::Url;

use super::channel::ChannelInfo;
use super::decode::{chzzk_date_opt, LenientObject};
use super::media::{LivePlaybackDetail, MediaInfo};

/// 라이브 방송 상세 정보.
///
/// 디코딩은 매우 관대하다 — live-status 폴링 API 는 `liveId` 가 없을 수 있으므로 모든 필드에
/// 기본값이 있다(`status` 기본 `Open`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveInfo {
    pub live_id: i64,
    pub live_title: String,
    pub status: LiveStatus,
    pub concurrent_user_count: i64,
    /// 누적 시청자 수 (chzzk live-status 의 accumulateCount)
    pub accumulate_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_category_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_channel_id: Option<String>,
    /// `{type}` 플레이스홀더가 `720` 으로 치환된 썸네일 URL.
    #[serde(rename = "liveImageUrl", skip_serializing_if = "Option::is_none")]
    pub live_image_url: Option<Url>,
    #[serde(
        rename = "defaultThumbnailImageUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub default_thumbnail_image_url: Option<Url>,
    #[serde(with = "chzzk_date_opt", skip_serializing_if = "Option::is_none")]
    pub open_date: Option<DateTime<Utc>>,
    pub adult: bool,
    pub tags: Vec<String>,
    /// 스트리머가 현재 라이브에서 타임머신을 허용했는지 여부.
    pub time_machine_active: bool,
    /// 현재 로그인 세션에 타임머신 재생 JSON 이 발급됐는지 여부.
    pub time_machine_playback: bool,
    #[serde(rename = "livePlaybackJson", skip_serializing_if = "Option::is_none")]
    pub live_playback_json: Option<String>,
    /// [2026-06-13] 같이보기+ 오디오 전용 라디오 스트림 JSON (영상 livePlayback 부재 시 폴백).
    #[serde(
        rename = "radioModePlaybackJson",
        skip_serializing_if = "Option::is_none"
    )]
    pub radio_mode_playback_json: Option<String>,
    /// 워치파티(같이보기+) 식별 — `Some` 이면 같이보기 스트림.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watch_party_no: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watch_party_type: Option<String>,
    /// TV 앱 시청 정책. DENIED 여도 제공된 영상 HLS 와 로그인 키로 재생 가능한 방송이 있다.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tv_app_viewing_policy_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<ChannelInfo>,
    /// [WatchCount 2026-07-17] 서버 지정 워치 하트비트 주기(ms) — `livePollingStatusJson` 의
    /// `callPeriodMilliSecond`. 파생 전용이라 인코딩하지 않는다.
    #[serde(skip)]
    pub watch_polling_interval_ms: Option<i64>,
}

impl LiveInfo {
    /// 필수 값만 받는 생성자 — 나머지는 원본 memberwise init 의 기본값.
    pub fn new(live_id: i64, live_title: impl Into<String>) -> Self {
        Self {
            live_id,
            live_title: live_title.into(),
            status: LiveStatus::Open,
            concurrent_user_count: 0,
            accumulate_count: 0,
            category_type: None,
            live_category: None,
            live_category_value: None,
            chat_channel_id: None,
            live_image_url: None,
            default_thumbnail_image_url: None,
            open_date: None,
            adult: false,
            tags: Vec::new(),
            time_machine_active: false,
            time_machine_playback: false,
            live_playback_json: None,
            radio_mode_playback_json: None,
            watch_party_no: None,
            watch_party_type: None,
            tv_app_viewing_policy_type: None,
            channel: None,
            watch_polling_interval_ms: None,
        }
    }

    /// `Identifiable.id`
    pub fn id(&self) -> i64 {
        self.live_id
    }

    /// [WatchCount 2026-07-17] 재생 중 live-status 재폴링(=시청 하트비트) 간격.
    /// 서버 지정값을 정본으로 하되 폭주/오설정 방어를 위해 5~30s 로 클램프.
    pub fn watch_heartbeat_interval(&self) -> Duration {
        let ms = self.watch_polling_interval_ms.unwrap_or(10_000);
        Duration::from_secs_f64((ms as f64 / 1000.0).clamp(5.0, 30.0))
    }

    /// `{type}` 치환이 완료된 라이브 썸네일 URL (디코딩 시 이미 해결됨).
    pub fn resolved_live_image_url(&self) -> Option<&Url> {
        self.live_image_url.as_ref()
    }

    /// 같이보기+(워치파티) 스트림 여부.
    pub fn is_watch_party(&self) -> bool {
        self.watch_party_no.is_some()
            || self
                .watch_party_type
                .as_deref()
                .is_some_and(|t| !t.is_empty())
    }

    /// TV 정책이 DENIED 인지. 이 값만으로 모든 영상 재생을 차단하지 않는다.
    pub fn is_external_video_denied(&self) -> bool {
        self.tv_app_viewing_policy_type
            .as_deref()
            .unwrap_or("")
            .to_uppercase()
            == "DENIED"
    }

    /// 영상 livePlayback 은 비었지만 라디오(오디오) 스트림은 있는 경우 — 같이보기+ 오디오 전용.
    pub fn has_only_radio_playback(&self) -> bool {
        self.live_playback_json.as_deref().unwrap_or("").is_empty()
            && !self
                .radio_mode_playback_json
                .as_deref()
                .unwrap_or("")
                .is_empty()
    }

    /// API 가 playback JSON 을 문자열 또는 중첩 JSON 객체로 반환하는 경우 모두 처리.
    fn decode_playback_json(o: &LenientObject, key: &str) -> Option<String> {
        match o.raw(key)? {
            Value::String(s) if !s.is_empty() => Some(s.clone()),
            v @ Value::Object(_) => {
                let playback = LivePlayback::deserialize(v).ok()?;
                serde_json::to_string(&playback).ok()
            }
            _ => None,
        }
    }

    /// `livePollingStatusJson`(문자열) → `callPeriodMilliSecond` 추출.
    fn decode_call_period_ms(o: &LenientObject) -> Option<i64> {
        let raw: String = o.get("livePollingStatusJson")?;
        let obj: serde_json::Map<String, Value> = serde_json::from_str(&raw).ok()?;
        obj.get("callPeriodMilliSecond")?.as_i64()
    }
}

impl<'de> Deserialize<'de> for LiveInfo {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let o = LenientObject::from_deserializer(d)?;
        // {type} 플레이스홀더를 문자열 단계에서 치환 (URL 파싱 시 %7Btype%7D 로 인코딩되는 문제 방지)
        let live_image_url = o
            .get::<String>("liveImageUrl")
            .and_then(|raw| Url::parse(&raw.replace("{type}", "720")).ok());
        Ok(Self {
            live_id: o.get("liveId").unwrap_or(0),
            live_title: o.get("liveTitle").unwrap_or_default(),
            status: o.get("status").unwrap_or(LiveStatus::Open),
            concurrent_user_count: o.get("concurrentUserCount").unwrap_or(0),
            accumulate_count: o.get("accumulateCount").unwrap_or(0),
            category_type: o.get("categoryType"),
            live_category: o.get("liveCategory"),
            live_category_value: o.get("liveCategoryValue"),
            chat_channel_id: o.get("chatChannelId"),
            live_image_url,
            default_thumbnail_image_url: o.url("defaultThumbnailImageUrl"),
            open_date: o.date("openDate"),
            adult: o.get("adult").unwrap_or(false),
            tags: o.get("tags").unwrap_or_default(),
            time_machine_active: o.get("timeMachineActive").unwrap_or(false),
            time_machine_playback: o.get("timeMachinePlayback").unwrap_or(false),
            live_playback_json: Self::decode_playback_json(&o, "livePlaybackJson"),
            radio_mode_playback_json: Self::decode_playback_json(&o, "radioModePlaybackJson"),
            watch_party_no: o.get("watchPartyNo"),
            watch_party_type: o.get("watchPartyType"),
            tv_app_viewing_policy_type: o.get("tvAppViewingPolicyType"),
            channel: o.get("channel"),
            watch_polling_interval_ms: Self::decode_call_period_ms(&o),
        })
    }
}

/// 채널 기준 현재 시청자 정보(`/service/v1.1/channels/{id}/my-info`).
/// 치트키는 전역 상품이지만 치지직 웹도 채널별 my-info 응답의 `cheatKey` 를 권위로 사용한다.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelViewerInfo {
    pub channel_id: String,
    pub user_role: String,
    pub cheat_key: bool,
    pub naver_membership: bool,
}

impl Default for ChannelViewerInfo {
    fn default() -> Self {
        Self {
            channel_id: String::new(),
            user_role: "GUEST".into(),
            cheat_key: false,
            naver_membership: false,
        }
    }
}

impl<'de> Deserialize<'de> for ChannelViewerInfo {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let o = LenientObject::from_deserializer(d)?;
        Ok(Self {
            channel_id: o.get("channelId").unwrap_or_default(),
            user_role: o.get("userRole").unwrap_or_else(|| "GUEST".into()),
            cheat_key: o.get("cheatKey").unwrap_or(false),
            naver_membership: o.get("naverMembership").unwrap_or(false),
        })
    }
}

/// 치트키 타임머신 전용 재생 응답(`/service/v1.1/channels/{id}/live-playback-json`).
/// `tmp == true` 인 응답만 타임머신 스트림으로 신뢰한다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize)]
pub struct TimeMachinePlaybackInfo {
    #[serde(rename = "playbackJson", skip_serializing_if = "Option::is_none")]
    pub playback_json: Option<String>,
    #[serde(rename = "tmp")]
    pub is_time_machine_playback: bool,
}

impl<'de> Deserialize<'de> for TimeMachinePlaybackInfo {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let o = LenientObject::from_deserializer(d)?;
        Ok(Self {
            playback_json: o.get("playbackJson"),
            is_time_machine_playback: o.get("tmp").unwrap_or(false),
        })
    }
}

/// 라이브 방송 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LiveStatus {
    #[serde(rename = "OPEN")]
    Open,
    #[serde(rename = "CLOSE")]
    Close,
}

impl LiveStatus {
    pub fn raw_value(self) -> &'static str {
        match self {
            LiveStatus::Open => "OPEN",
            LiveStatus::Close => "CLOSE",
        }
    }
}

/// 스트림 품질.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StreamQuality {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "1080p")]
    Source,
    #[serde(rename = "720p")]
    High,
    #[serde(rename = "480p")]
    Medium,
    #[serde(rename = "360p")]
    Low,
}

impl StreamQuality {
    pub const ALL: [StreamQuality; 5] = [
        StreamQuality::Auto,
        StreamQuality::Source,
        StreamQuality::High,
        StreamQuality::Medium,
        StreamQuality::Low,
    ];

    pub fn raw_value(self) -> &'static str {
        match self {
            StreamQuality::Auto => "auto",
            StreamQuality::Source => "1080p",
            StreamQuality::High => "720p",
            StreamQuality::Medium => "480p",
            StreamQuality::Low => "360p",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            StreamQuality::Auto => "자동",
            StreamQuality::Source => "원본 (1080p)",
            StreamQuality::High => "고화질 (720p)",
            StreamQuality::Medium => "중화질 (480p)",
            StreamQuality::Low => "저화질 (360p)",
        }
    }

    /// 사용자 선택을 재생 엔진의 **해상도 상한**(픽셀 높이)으로 옮긴다. 0 = 제한 없음.
    /// `Auto` 와 `Source` 는 상한을 두지 않는다.
    pub fn max_height_cap(self) -> i64 {
        match self {
            StreamQuality::Auto | StreamQuality::Source => 0,
            StreamQuality::High => 720,
            StreamQuality::Medium => 480,
            StreamQuality::Low => 360,
        }
    }
}

/// 라이브 재생 정보 (JSON 파싱). Swift 합성 Codable — `media` 는 필수.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LivePlayback {
    pub media: Vec<MediaInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live: Option<LivePlaybackDetail>,
}

#[cfg(test)]
mod tests {
    //! `Tests/CViewCoreTests/LiveInfoTests.swift` 포팅.
    use super::*;

    #[test]
    fn memberwise_init() {
        let info = LiveInfo::new(1, "테스트 방송");
        assert_eq!(info.live_id, 1);
        assert_eq!(info.live_title, "테스트 방송");
        assert_eq!(info.status, LiveStatus::Open);
        assert_eq!(info.concurrent_user_count, 0);
        assert!(!info.adult);
        assert!(info.tags.is_empty());
        assert_eq!(info.id(), 1);
    }

    #[test]
    fn json_decoding_basic() {
        let info: LiveInfo = serde_json::from_str(
            r#"{"liveId":42,"liveTitle":"방송 제목","status":"OPEN","concurrentUserCount":1234,"adult":true,"tags":["게임","치지직"]}"#,
        )
        .unwrap();
        assert_eq!(info.live_id, 42);
        assert_eq!(info.live_title, "방송 제목");
        assert_eq!(info.status, LiveStatus::Open);
        assert_eq!(info.concurrent_user_count, 1234);
        assert!(info.adult);
        assert_eq!(info.tags, vec!["게임", "치지직"]);
    }

    #[test]
    fn live_image_url_placeholder() {
        let info: LiveInfo = serde_json::from_str(
            r#"{"liveId":1,"liveTitle":"T","status":"OPEN","concurrentUserCount":0,"liveImageUrl":"https://cdn.example.com/thumb/{type}/image.jpg"}"#,
        )
        .unwrap();
        assert_eq!(
            info.live_image_url.as_ref().unwrap().as_str(),
            "https://cdn.example.com/thumb/720/image.jpg"
        );
        assert_eq!(info.resolved_live_image_url(), info.live_image_url.as_ref());
    }

    #[test]
    fn json_decoding_defaults() {
        let info: LiveInfo = serde_json::from_str("{}").unwrap();
        assert_eq!(info.live_id, 0);
        assert_eq!(info.live_title, "");
        assert_eq!(info.status, LiveStatus::Open);
        assert_eq!(info.concurrent_user_count, 0);
        assert!(!info.adult);
        assert!(info.tags.is_empty());
        assert!(info.live_image_url.is_none());
        assert!(info.chat_channel_id.is_none());
        assert!(!info.time_machine_active);
        assert!(!info.time_machine_playback);
    }

    #[test]
    fn unknown_status_falls_back_to_open() {
        let info: LiveInfo = serde_json::from_str(r#"{"status":"BLOCKED"}"#).unwrap();
        assert_eq!(info.status, LiveStatus::Open);
    }

    #[test]
    fn time_machine_flags() {
        let info: LiveInfo = serde_json::from_str(
            r#"{"liveId":7,"liveTitle":"타임머신 방송","timeMachineActive":true,"timeMachinePlayback":true}"#,
        )
        .unwrap();
        assert!(info.time_machine_active);
        assert!(info.time_machine_playback);
    }

    #[test]
    fn channel_viewer_info_decoding() {
        let info: ChannelViewerInfo = serde_json::from_str(
            r#"{"channelId":"ch","userRole":"COMMON_USER","cheatKey":true,"naverMembership":false}"#,
        )
        .unwrap();
        assert_eq!(info.channel_id, "ch");
        assert!(info.cheat_key);
        assert!(!info.naver_membership);
        let empty: ChannelViewerInfo = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, ChannelViewerInfo::default());
        assert_eq!(empty.user_role, "GUEST");
    }

    #[test]
    fn time_machine_playback_decoding() {
        let info: TimeMachinePlaybackInfo =
            serde_json::from_str(r#"{"playbackJson":"{\"media\":[]}","tmp":true}"#).unwrap();
        assert!(info.is_time_machine_playback);
        assert_eq!(info.playback_json.as_deref(), Some(r#"{"media":[]}"#));
    }

    #[test]
    fn live_playback_json_nested_object() {
        let info: LiveInfo = serde_json::from_str(
            r#"{"liveId":9,"liveTitle":"T","status":"OPEN","livePlaybackJson":{"media":[
                {"mediaId":"LLHLS","protocol":"LL-HLS","path":"https://cdn.example.com/x.m3u8"}]}}"#,
        )
        .unwrap();
        let json = info.live_playback_json.unwrap();
        assert!(json.contains("LLHLS"));
        assert!(json.contains("cdn.example.com"));
    }

    #[test]
    fn chat_channel_id_and_close() {
        let info: LiveInfo = serde_json::from_str(
            r#"{"liveId":5,"liveTitle":"T","status":"CLOSE","concurrentUserCount":0,"chatChannelId":"chat_abc123"}"#,
        )
        .unwrap();
        assert_eq!(info.status, LiveStatus::Close);
        assert_eq!(info.chat_channel_id.as_deref(), Some("chat_abc123"));
    }

    #[test]
    fn watch_polling_interval_from_status_json() {
        let info: LiveInfo = serde_json::from_str(
            r#"{"livePollingStatusJson":"{\"status\":\"STARTED\",\"callPeriodMilliSecond\":20000}"}"#,
        )
        .unwrap();
        assert_eq!(info.watch_polling_interval_ms, Some(20_000));
        assert_eq!(info.watch_heartbeat_interval(), Duration::from_secs(20));
        let mut fast = info.clone();
        fast.watch_polling_interval_ms = Some(100);
        assert_eq!(fast.watch_heartbeat_interval(), Duration::from_secs(5));
        assert_eq!(
            LiveInfo::new(1, "t").watch_heartbeat_interval(),
            Duration::from_secs(10)
        );
    }

    #[test]
    fn watch_party_and_radio_flags() {
        let info: LiveInfo = serde_json::from_str(
            r#"{"livePlaybackJson":"","radioModePlaybackJson":"{\"media\":[]}","watchPartyType":"WATCH","tvAppViewingPolicyType":"denied"}"#,
        )
        .unwrap();
        assert!(info.is_watch_party());
        assert!(info.has_only_radio_playback());
        assert!(info.is_external_video_denied());
    }

    #[test]
    fn live_status_raw_values() {
        assert_eq!(LiveStatus::Open.raw_value(), "OPEN");
        assert_eq!(LiveStatus::Close.raw_value(), "CLOSE");
        assert_eq!(
            serde_json::from_str::<LiveStatus>("\"OPEN\"").unwrap(),
            LiveStatus::Open
        );
    }

    #[test]
    fn stream_quality() {
        assert_eq!(StreamQuality::Auto.display_name(), "자동");
        assert_eq!(StreamQuality::Source.display_name(), "원본 (1080p)");
        assert_eq!(StreamQuality::High.display_name(), "고화질 (720p)");
        assert_eq!(StreamQuality::Medium.display_name(), "중화질 (480p)");
        assert_eq!(StreamQuality::Low.display_name(), "저화질 (360p)");
        assert_eq!(StreamQuality::ALL.len(), 5);
        assert_eq!(StreamQuality::Auto.raw_value(), "auto");
        assert_eq!(StreamQuality::Source.raw_value(), "1080p");
        assert_eq!(
            serde_json::to_string(&StreamQuality::Source).unwrap(),
            "\"1080p\""
        );
        assert_eq!(StreamQuality::High.max_height_cap(), 720);
        assert_eq!(StreamQuality::Source.max_height_cap(), 0);
    }

    #[test]
    fn live_playback_round_trip() {
        let original = LivePlayback {
            media: vec![MediaInfo::new(
                "m1",
                Some("HLS".into()),
                "/live/stream.m3u8",
            )],
            live: Some(LivePlaybackDetail {
                start: Some("2024-01-01".into()),
                status: Some("STARTED".into()),
                ..Default::default()
            }),
        };
        let data = serde_json::to_string(&original).unwrap();
        let decoded: LivePlayback = serde_json::from_str(&data).unwrap();
        assert_eq!(decoded.media.len(), 1);
        assert_eq!(decoded.media[0].media_id, "m1");
        assert_eq!(decoded.live.unwrap().status.as_deref(), Some("STARTED"));
        assert!(LivePlayback::default().media.is_empty());
    }

    #[test]
    fn live_info_round_trip_keeps_fields() {
        let mut info = LiveInfo::new(3, "x");
        info.open_date =
            Some(chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 1, 2, 3, 4, 5).unwrap());
        info.channel = Some(ChannelInfo::new("c", "n"));
        let json = serde_json::to_string(&info).unwrap();
        let back: LiveInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back, info);
    }
}
