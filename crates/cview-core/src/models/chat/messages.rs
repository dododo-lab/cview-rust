//! `Sources/CViewCore/Models/ChatModels.swift` 포팅 — 채팅 도메인 모델.
//!
//! JSON 키는 Swift 의 합성 `Codable` 키(= 프로퍼티 이름)와 같다. Swift 는 `nil` 옵셔널을
//! 인코딩하지 않으므로(`encodeIfPresent`) `skip_serializing_if` 로 맞췄다.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::ident::new_uuid_string;

/// 이모티콘 id → 이미지 URL 맵 (Swift `[String: String]`).
pub type EmojiMap = BTreeMap<String, String>;

/// 채팅 메시지. (Swift 에서는 `Codable` 이 아니다.)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChatMessage {
    pub id: String,
    pub user_id: Option<String>,
    pub nickname: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub r#type: MessageType,
    pub profile: Option<ChatProfile>,
    pub extras: Option<ChatExtras>,
    /// 수신 시점에 이미 가려진 메시지(클린봇 `CBOTBLIND`, `HIDDEN`).
    /// 파서가 `content` 를 안내 문구로 대체하고 이 플래그를 세운다.
    pub is_blinded: bool,
}

impl ChatMessage {
    /// Swift 기본 인자 `init(nickname:content:)` 에 해당 — id 는 새 UUID, 시각은 지금.
    pub fn new(nickname: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: new_uuid_string(),
            user_id: None,
            nickname: nickname.into(),
            content: content.into(),
            timestamp: Utc::now(),
            r#type: MessageType::Normal,
            profile: None,
            extras: None,
            is_blinded: false,
        }
    }

    /// id 를 지정해 만든다(테스트·파서용 단축).
    pub fn with_id(
        id: impl Into<String>,
        nickname: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            ..Self::new(nickname, content)
        }
    }

    pub fn user_id(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    pub fn message_type(mut self, ty: MessageType) -> Self {
        self.r#type = ty;
        self
    }

    pub fn timestamp(mut self, ts: DateTime<Utc>) -> Self {
        self.timestamp = ts;
        self
    }

    pub fn profile(mut self, profile: ChatProfile) -> Self {
        self.profile = Some(profile);
        self
    }

    pub fn extras(mut self, extras: ChatExtras) -> Self {
        self.extras = Some(extras);
        self
    }

    pub fn blinded(mut self, is_blinded: bool) -> Self {
        self.is_blinded = is_blinded;
        self
    }
}

/// 메시지 유형.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageType {
    #[default]
    Normal,
    Donation,
    Subscription,
    SystemMessage,
    Notice,
}

/// 사용자 역할 (치지직 채팅).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum UserRole {
    #[serde(rename = "streamer")]
    Streamer,
    #[serde(rename = "streaming_chat_manager")]
    Manager,
    #[serde(rename = "streaming_channel_manager")]
    ChannelManager,
    #[default]
    #[serde(rename = "")]
    Viewer,
}

impl UserRole {
    /// Swift `init(from code:)` — `"streamer"` 이외에는 `manager` 를 포함하면 매니저.
    pub fn from_code(code: Option<&str>) -> Self {
        match code {
            Some("streamer") => Self::Streamer,
            Some(c) if c.contains("manager") => Self::Manager,
            _ => Self::Viewer,
        }
    }

    /// Swift raw value.
    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Streamer => "streamer",
            Self::Manager => "streaming_chat_manager",
            Self::ChannelManager => "streaming_channel_manager",
            Self::Viewer => "",
        }
    }

    /// SF Symbol 아이콘 이름 (UI 계층에서 매핑용).
    pub fn icon_name(self) -> Option<&'static str> {
        match self {
            Self::Streamer => Some("mic.circle.fill"),
            Self::Manager | Self::ChannelManager => Some("wrench.and.screwdriver.fill"),
            Self::Viewer => None,
        }
    }

    /// 역할 표시 텍스트.
    pub fn display_label(self) -> Option<&'static str> {
        match self {
            Self::Streamer => Some("스트리머"),
            Self::Manager | Self::ChannelManager => Some("매니저"),
            Self::Viewer => None,
        }
    }

    /// 특수 역할인지 여부.
    pub fn is_special(self) -> bool {
        self != Self::Viewer
    }
}

/// 채팅 프로필.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChatProfile {
    pub nickname: String,
    #[serde(
        rename = "profileImageURL",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub profile_image_url: Option<String>,
    #[serde(
        rename = "userRoleCode",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub user_role_code: Option<String>,
    #[serde(rename = "userRole")]
    pub user_role: UserRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<ChatBadge>,
    pub badges: Vec<ChatBadge>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<ChatTitle>,
    #[serde(rename = "activityBadges")]
    pub activity_badges: Vec<ChatBadge>,
}

impl ChatProfile {
    /// Swift `init(...)` — `userRole` 은 `userRoleCode` 에서 파생된다.
    pub fn new(
        nickname: impl Into<String>,
        profile_image_url: Option<String>,
        user_role_code: Option<String>,
        badge: Option<ChatBadge>,
        badges: Vec<ChatBadge>,
        title: Option<ChatTitle>,
        activity_badges: Vec<ChatBadge>,
    ) -> Self {
        let user_role = UserRole::from_code(user_role_code.as_deref());
        Self {
            nickname: nickname.into(),
            profile_image_url,
            user_role_code,
            user_role,
            badge,
            badges,
            title,
            activity_badges,
        }
    }

    /// 닉네임만 있는 프로필.
    pub fn with_nickname(nickname: impl Into<String>) -> Self {
        Self::new(nickname, None, None, None, Vec::new(), None, Vec::new())
    }
}

/// 채팅 뱃지.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct ChatBadge {
    #[serde(rename = "imageURL", default, skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    #[serde(rename = "badgeId", default, skip_serializing_if = "Option::is_none")]
    pub badge_id: Option<String>,
    #[serde(rename = "altText", default, skip_serializing_if = "Option::is_none")]
    pub alt_text: Option<String>,
}

impl ChatBadge {
    pub fn new(
        image_url: Option<String>,
        badge_id: Option<String>,
        alt_text: Option<String>,
    ) -> Self {
        Self {
            image_url,
            badge_id,
            alt_text,
        }
    }
}

/// 채팅 칭호.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChatTitle {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// 후원 정보.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DonationInfo {
    pub amount: i64,
    pub currency: String,
    pub r#type: String,
}

impl DonationInfo {
    /// Swift 기본값: `currency = "KRW"`, `type = "CHAT"`.
    pub fn new(amount: i64) -> Self {
        Self {
            amount,
            currency: "KRW".into(),
            r#type: "CHAT".into(),
        }
    }
}

/// 주간 후원 랭킹 항목 (1명).
/// API: `GET /commercial/v1/channels/{cid}/donation/rank/weekly?withMyRank=true&rankCount=N`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DonationRankItem {
    pub user_id_hash: String,
    pub nick_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_image_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_mark: Option<bool>,
    pub donation_amount: i64,
    pub ranking: i64,
}

impl DonationRankItem {
    /// `Identifiable` — 동일 유저는 동일 hash.
    pub fn id(&self) -> &str {
        &self.user_id_hash
    }
}

/// 주간 후원 랭킹 응답 envelope content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DonationRankResponse {
    pub rank_list: Vec<DonationRankItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub donation_user_weekly_rank: Option<DonationRankItem>,
}

/// 구독 정보.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionInfo {
    pub months: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier_name: Option<String>,
}

/// 채팅 부가 정보.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emojis: Option<EmojiMap>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub streaming_channel_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub donation: Option<DonationInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription: Option<SubscriptionInfo>,
    /// 선물 종류(`giftType`, 예: 구독 선물) — 있을 때만.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gift_type: Option<String>,
    /// 선물 개수(`quantity`/`giftCount`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gift_count: Option<i64>,
    /// 미션 후원 내용(`missionText`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mission_text: Option<String>,
    /// 익명 후원.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_anonymous: Option<bool>,
}

/// 채팅 접속 토큰.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAccessToken {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub real_name_auth: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temporary_restrict: Option<TemporaryRestrict>,
}

/// 임시 제한 정보.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporaryRestrict {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temporary_restrict: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub times: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<i64>,
}

impl Default for TemporaryRestrict {
    /// Swift 기본 인자: `false`, `0`, `0`.
    fn default() -> Self {
        Self {
            temporary_restrict: Some(false),
            times: Some(0),
            duration: Some(0),
        }
    }
}

/// 채팅 연결 상태.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum ChatConnectionState {
    #[default]
    Disconnected,
    Connecting,
    Connected {
        server_index: i64,
    },
    Reconnecting {
        attempt: i64,
    },
    Failed {
        reason: String,
    },
}

impl ChatConnectionState {
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected { .. })
    }

    pub fn display_text(&self) -> String {
        match self {
            Self::Disconnected => "연결 안됨".into(),
            Self::Connecting => "연결 중…".into(),
            Self::Connected { server_index } => format!("연결됨 (서버 {server_index})"),
            Self::Reconnecting { attempt } => format!("재연결 중 ({attempt}회)"),
            Self::Failed { reason } => format!("연결 실패: {reason}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_role_from_code() {
        assert_eq!(UserRole::from_code(Some("streamer")), UserRole::Streamer);
        assert_eq!(
            UserRole::from_code(Some("streaming_chat_manager")),
            UserRole::Manager
        );
        assert_eq!(
            UserRole::from_code(Some("streaming_channel_manager")),
            UserRole::Manager
        );
        assert_eq!(UserRole::from_code(Some("common_user")), UserRole::Viewer);
        assert_eq!(UserRole::from_code(None), UserRole::Viewer);
        assert!(UserRole::Streamer.is_special());
        assert!(!UserRole::Viewer.is_special());
        assert_eq!(UserRole::Manager.display_label(), Some("매니저"));
    }

    #[test]
    fn message_stores_properties() {
        let now = Utc::now();
        let m = ChatMessage::with_id("msg-1", "TestUser", "Hello, world!")
            .user_id("user-1")
            .timestamp(now);
        assert_eq!(m.id, "msg-1");
        assert_eq!(m.user_id.as_deref(), Some("user-1"));
        assert_eq!(m.nickname, "TestUser");
        assert_eq!(m.content, "Hello, world!");
        assert_eq!(m.timestamp, now);
        assert_eq!(m.r#type, MessageType::Normal);
    }

    #[test]
    fn message_with_donation_extras() {
        let m = ChatMessage::with_id("msg-2", "Donor", "donation message")
            .message_type(MessageType::Donation)
            .extras(ChatExtras {
                donation: Some(DonationInfo::new(1000)),
                ..Default::default()
            });
        assert_eq!(m.extras.unwrap().donation.unwrap().amount, 1000);
    }

    #[test]
    fn connection_state_display_text() {
        assert_eq!(
            ChatConnectionState::Disconnected.display_text(),
            "연결 안됨"
        );
        assert_eq!(ChatConnectionState::Connecting.display_text(), "연결 중…");
        assert_eq!(
            ChatConnectionState::Connected { server_index: 3 }.display_text(),
            "연결됨 (서버 3)"
        );
        assert_eq!(
            ChatConnectionState::Reconnecting { attempt: 2 }.display_text(),
            "재연결 중 (2회)"
        );
        assert_eq!(
            ChatConnectionState::Failed {
                reason: "timeout".into()
            }
            .display_text(),
            "연결 실패: timeout"
        );
    }

    #[test]
    fn connection_state_is_connected() {
        assert!(!ChatConnectionState::Disconnected.is_connected());
        assert!(!ChatConnectionState::Connecting.is_connected());
        assert!(ChatConnectionState::Connected { server_index: 0 }.is_connected());
        assert!(!ChatConnectionState::Reconnecting { attempt: 1 }.is_connected());
        assert!(!ChatConnectionState::Failed { reason: "x".into() }.is_connected());
    }

    #[test]
    fn serde_keys_match_swift_codable() {
        let profile = ChatProfile::new(
            "nick",
            Some("https://img/p.png".into()),
            Some("streamer".into()),
            None,
            vec![ChatBadge::new(
                Some("https://b".into()),
                Some("sub".into()),
                None,
            )],
            Some(ChatTitle {
                name: "t".into(),
                color: None,
            }),
            vec![],
        );
        let v = serde_json::to_value(&profile).unwrap();
        assert_eq!(v["profileImageURL"], "https://img/p.png");
        assert_eq!(v["userRoleCode"], "streamer");
        assert_eq!(v["userRole"], "streamer");
        assert_eq!(v["badges"][0]["imageURL"], "https://b");
        assert_eq!(v["badges"][0]["badgeId"], "sub");
        assert!(v.get("badge").is_none());
        let back: ChatProfile = serde_json::from_value(v).unwrap();
        assert_eq!(back, profile);

        let token: ChatAccessToken = serde_json::from_str(
            r#"{"accessToken":"a","extraToken":"e","realNameAuth":false,"temporaryRestrict":{"temporaryRestrict":false,"times":0,"duration":0}}"#,
        )
        .unwrap();
        assert_eq!(token.access_token, "a");
        assert_eq!(token.extra_token.as_deref(), Some("e"));
        assert_eq!(token.temporary_restrict, Some(TemporaryRestrict::default()));

        let rank: DonationRankResponse = serde_json::from_str(
            r#"{"rankList":[{"userIdHash":"h","nickName":"n","donationAmount":1000,"ranking":1}]}"#,
        )
        .unwrap();
        assert_eq!(rank.rank_list[0].id(), "h");
        assert_eq!(rank.rank_list[0].verified_mark, None);

        assert_eq!(
            serde_json::to_string(&MessageType::SystemMessage).unwrap(),
            "\"systemMessage\""
        );
        assert_eq!(serde_json::to_string(&UserRole::Viewer).unwrap(), "\"\"");
    }
}
