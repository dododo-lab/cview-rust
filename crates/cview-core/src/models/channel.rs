//! `Sources/CViewCore/Models/ChannelInfo.swift` 포팅 — 채널 정보 도메인 모델.

use serde::{Deserialize, Deserializer, Serialize};
use url::Url;

use super::decode::LenientObject;
use super::live::{LiveInfo, LiveStatus};

/// 치지직 채널 기본 정보.
///
/// 디코딩은 원본처럼 관대하다: `channelId`·`channelName` 만 필수이고, 나머지는 없거나
/// 형식이 어긋나면 기본값(`false`/`0`/`None`)이 된다.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelInfo {
    pub channel_id: String,
    pub channel_name: String,
    #[serde(rename = "channelImageUrl", skip_serializing_if = "Option::is_none")]
    pub channel_image_url: Option<Url>,
    pub verified_mark: bool,
    /// 인증 마크 종류(`verifiedMarkType` — `PARTNER` / `CHALLENGER` / `OFFICIAL`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified_mark_type: Option<String>,
    pub follower_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_description: Option<String>,
    /// 현재 라이브 방송 중 여부 (검색/팔로잉 응답에 포함)
    pub open_live: bool,
}

impl ChannelInfo {
    /// 필수 값만 받는 생성자 — 나머지는 기본값.
    pub fn new(channel_id: impl Into<String>, channel_name: impl Into<String>) -> Self {
        Self {
            channel_id: channel_id.into(),
            channel_name: channel_name.into(),
            channel_image_url: None,
            verified_mark: false,
            verified_mark_type: None,
            follower_count: 0,
            channel_description: None,
            open_live: false,
        }
    }

    /// `Identifiable.id`
    pub fn id(&self) -> &str {
        &self.channel_id
    }

    /// 인증 마크 종류(`"partner"` · `"challenger"` · `"official"`) — [`super::verified_badge`].
    pub fn verified_badge(&self) -> Option<&'static str> {
        super::verified_badge(self.verified_mark, self.verified_mark_type.as_deref())
    }
}

impl<'de> Deserialize<'de> for ChannelInfo {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let o = LenientObject::from_deserializer(d)?;
        Ok(Self {
            channel_id: o.required::<_, D::Error>("channelId")?,
            channel_name: o.required::<_, D::Error>("channelName")?,
            channel_image_url: o.url("channelImageUrl"),
            verified_mark: o.get("verifiedMark").unwrap_or(false),
            verified_mark_type: o
                .get::<String>("verifiedMarkType")
                .filter(|t| !t.trim().is_empty()),
            follower_count: o.get("followerCount").unwrap_or(0),
            channel_description: o.get("channelDescription"),
            open_live: o.get("openLive").unwrap_or(false),
        })
    }
}

/// 채팅 아바타용 채널 사진 — 채널 정보 응답에서 `channelImageUrl` 만 읽는다.
/// 치지직 채팅 메시지의 `profile.profileImageUrl` 은 늘 비어 와서(2026-11-12 실측) 채널(= userIdHash)에서 받는다.
/// 채널 이름 같은 다른 값이 비어 있어도 디코딩이 실패하지 않게 따로 둔다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelAvatar {
    pub channel_image_url: Option<Url>,
}

impl<'de> Deserialize<'de> for ChannelAvatar {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let o = LenientObject::from_deserializer(d)?;
        Ok(Self {
            channel_image_url: o
                .url("channelImageUrl")
                .filter(|u| matches!(u.scheme(), "https" | "http")),
        })
    }
}

/// 로그인한 시청자 기준 채널 팔로우 상태 — 채널 정보 응답의 `personalData.following`.
///
/// 비로그인이거나 응답에 `personalData` 가 없으면 `following` 이 `None`(모름)이다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelFollowStatus {
    pub following: Option<bool>,
    pub notification: Option<bool>,
    /// 팔로우한 날짜 원문(`2024-02-19 23:28:11`, KST).
    pub follow_date: Option<String>,
}

impl<'de> Deserialize<'de> for ChannelFollowStatus {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let o = LenientObject::from_deserializer(d)?;
        let f = o
            .raw("personalData")
            .and_then(|p| p.get("following"))
            .filter(|f| f.is_object());
        let Some(f) = f else {
            return Ok(Self::default());
        };
        Ok(Self {
            following: f.get("following").and_then(|v| v.as_bool()),
            notification: f.get("notification").and_then(|v| v.as_bool()),
            follow_date: f
                .get("followDate")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from),
        })
    }
}

/// 팔로잉 채널 (라이브 상태 포함).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowingChannel {
    pub channel: ChannelInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub streamer: Option<StreamerInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_info: Option<LiveInfo>,
}

impl FollowingChannel {
    pub fn new(
        channel: ChannelInfo,
        streamer: Option<StreamerInfo>,
        live_info: Option<LiveInfo>,
    ) -> Self {
        Self {
            channel,
            streamer,
            live_info,
        }
    }

    pub fn id(&self) -> &str {
        &self.channel.channel_id
    }

    /// 실제 라이브 여부: `LiveInfo` 가 존재하고 status 가 `Open` 이어야 한다.
    /// (치지직 API 는 방송 종료 직후에도 status=CLOSE 인 LiveInfo 를 잠시 반환하므로 단순 nil 체크로는 불충분)
    pub fn is_live(&self) -> bool {
        self.live_info
            .as_ref()
            .is_some_and(|l| l.status == LiveStatus::Open)
    }
}

/// 스트리머 요약 정보.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamerInfo {
    pub open_live: bool,
}

#[cfg(test)]
mod tests {
    //! `Tests/CViewCoreTests/ChannelInfoTests.swift` 포팅.
    use super::*;

    #[test]
    fn channel_avatar_reads_only_image() {
        let a: ChannelAvatar = serde_json::from_str(
            r#"{"channelId":"a","channelName":null,"channelImageUrl":"https://nng-phinf.pstatic.net/x/y.png"}"#,
        )
        .unwrap();
        assert_eq!(
            a.channel_image_url.map(|u| u.to_string()).as_deref(),
            Some("https://nng-phinf.pstatic.net/x/y.png")
        );
        for raw in [
            r#"{"channelImageUrl":""}"#,
            r#"{"channelImageUrl":null}"#,
            r#"{}"#,
            r#"{"channelImageUrl":"javascript:alert(1)"}"#,
        ] {
            let a: ChannelAvatar = serde_json::from_str(raw).unwrap();
            assert_eq!(a.channel_image_url, None, "{raw}");
        }
    }

    #[test]
    fn follow_status_from_personal_data() {
        let st: ChannelFollowStatus = serde_json::from_str(
            r#"{"channelId":"a","personalData":{"following":{"following":true,"notification":false,"followDate":"2024-02-19 23:28:11"},"privateUserBlock":false}}"#,
        )
        .unwrap();
        assert_eq!(st.following, Some(true));
        assert_eq!(st.notification, Some(false));
        assert_eq!(st.follow_date.as_deref(), Some("2024-02-19 23:28:11"));
        let none: ChannelFollowStatus = serde_json::from_str(r#"{"channelId":"a"}"#).unwrap();
        assert_eq!(none, ChannelFollowStatus::default());
        let odd: ChannelFollowStatus =
            serde_json::from_str(r#"{"personalData":{"following":"x"}}"#).unwrap();
        assert_eq!(odd.following, None);
    }

    #[test]
    fn memberwise_init() {
        let ch = ChannelInfo::new("ch1", "테스트 채널");
        assert_eq!(ch.channel_id, "ch1");
        assert_eq!(ch.channel_name, "테스트 채널");
        assert!(ch.channel_image_url.is_none());
        assert!(!ch.verified_mark);
        assert_eq!(ch.follower_count, 0);
        assert!(ch.channel_description.is_none());
        assert_eq!(ch.id(), "ch1");
    }

    #[test]
    fn json_decoding_full() {
        let json = r#"{
            "channelId": "abc",
            "channelName": "방송채널",
            "channelImageUrl": "https://example.com/img.png",
            "verifiedMark": true,
            "followerCount": 12345,
            "channelDescription": "소개글"
        }"#;
        let ch: ChannelInfo = serde_json::from_str(json).unwrap();
        assert_eq!(ch.channel_id, "abc");
        assert_eq!(ch.channel_name, "방송채널");
        assert_eq!(
            ch.channel_image_url.unwrap().as_str(),
            "https://example.com/img.png"
        );
        assert!(ch.verified_mark);
        assert_eq!(ch.follower_count, 12345);
        assert_eq!(ch.channel_description.as_deref(), Some("소개글"));
    }

    #[test]
    fn json_decoding_defaults() {
        let ch: ChannelInfo =
            serde_json::from_str(r#"{"channelId":"ch2","channelName":"최소"}"#).unwrap();
        assert!(!ch.verified_mark);
        assert_eq!(ch.follower_count, 0);
        assert!(ch.channel_image_url.is_none());
        assert!(ch.channel_description.is_none());
    }

    #[test]
    fn json_decoding_requires_id_and_name() {
        assert!(serde_json::from_str::<ChannelInfo>(r#"{"channelName":"x"}"#).is_err());
    }

    #[test]
    fn lenient_wrong_types_fall_back() {
        let ch: ChannelInfo = serde_json::from_str(
            r#"{"channelId":"c","channelName":"n","followerCount":"many","verifiedMark":null,"channelImageUrl":""}"#,
        )
        .unwrap();
        assert_eq!(ch.follower_count, 0);
        assert!(!ch.verified_mark);
        assert!(ch.channel_image_url.is_none());
    }

    #[test]
    fn round_trip() {
        let mut ch = ChannelInfo::new("c", "n");
        ch.channel_image_url = Some(Url::parse("https://x.example/a.png").unwrap());
        let json = serde_json::to_string(&ch).unwrap();
        assert!(json.contains("channelImageUrl"));
        let back: ChannelInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ch);
    }

    #[test]
    fn hashable_equal() {
        assert_eq!(ChannelInfo::new("ch1", "A"), ChannelInfo::new("ch1", "A"));
    }

    #[test]
    fn following_channel_is_live() {
        let ch = ChannelInfo::new("ch1", "A");
        let live = LiveInfo::new(1, "방송");
        let following = FollowingChannel::new(ch.clone(), None, Some(live));
        assert!(following.is_live());
        assert_eq!(following.id(), "ch1");
        assert!(!FollowingChannel::new(ChannelInfo::new("ch2", "B"), None, None).is_live());

        let mut closed = LiveInfo::new(2, "끝");
        closed.status = LiveStatus::Close;
        assert!(!FollowingChannel::new(ch, None, Some(closed)).is_live());
    }

    #[test]
    fn streamer_info() {
        assert!(!StreamerInfo::default().open_live);
        assert!(StreamerInfo { open_live: true }.open_live);
    }
}
