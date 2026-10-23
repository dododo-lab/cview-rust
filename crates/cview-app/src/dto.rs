//! 프런트엔드(WebView)로 보내는 DTO 와 도메인 모델 → DTO 변환.
//!
//! 모두 순수 함수다 — 명령 계층(`commands`)이 하위 크레이트 결과를 여기서 평평한 JSON 으로 바꾼다.
//! 필드 이름은 JS 관례(camelCase)를 따른다. 나중에 `cview-app-core` 의 뷰모델로 바꿔 끼울 때도
//! 이 DTO 계약은 그대로 둔다.

use chrono::{DateTime, Utc};
use cview_core::models::chat::{
    ChatConnectionState, ChatMessage, ChatMessageItem, EmojiMap, MessageType, UserRole,
};
use cview_core::models::live::LiveStatus;
use cview_core::models::{format_viewer_count, ChannelInfo, LiveChannelItem, LiveInfo};
use cview_core::protocols::AuthState;
use cview_player::coordinator::{StreamEvent, StreamPhase};
use serde::Serialize;

/// 썸네일·채널 이미지가 없을 때 쓰는 빈 값.
fn url_string(url: Option<&url::Url>) -> Option<String> {
    url.map(|u| u.to_string())
}

fn rfc3339(date: Option<DateTime<Utc>>) -> Option<String> {
    date.map(|d| d.to_rfc3339())
}

/// 카테고리 표시명 — 비었으면 `None`.
fn category(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

// MARK: - 라이브 카드

/// 라이브 격자 카드 한 장.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveCardDto {
    pub channel_id: String,
    pub channel_name: String,
    pub channel_image_url: Option<String>,
    pub live_title: String,
    pub viewer_count: i64,
    /// `1.5만` 같은 한국어 축약 (Swift `formattedViewerCount`).
    pub viewer_count_text: String,
    pub category_name: Option<String>,
    pub thumbnail_url: Option<String>,
    pub is_live: bool,
    pub open_date: Option<String>,
    pub adult: bool,
}

impl LiveCardDto {
    /// 인기·검색 결과의 `LiveInfo`. 채널 정보가 없으면 카드로 만들 수 없다(`None`).
    pub fn from_live_info(info: &LiveInfo) -> Option<Self> {
        let channel = info.channel.as_ref()?;
        Some(Self {
            channel_id: channel.channel_id.clone(),
            channel_name: channel.channel_name.clone(),
            channel_image_url: url_string(channel.channel_image_url.as_ref()),
            live_title: info.live_title.clone(),
            viewer_count: info.concurrent_user_count,
            viewer_count_text: format_viewer_count(info.concurrent_user_count),
            category_name: category(info.live_category_value.as_deref()),
            thumbnail_url: url_string(info.resolved_live_image_url()),
            is_live: info.status == LiveStatus::Open,
            open_date: rfc3339(info.open_date),
            adult: info.adult,
        })
    }

    /// 팔로잉 목록의 `LiveChannelItem`.
    pub fn from_channel_item(item: &LiveChannelItem) -> Self {
        Self {
            channel_id: item.channel_id.clone(),
            channel_name: item.channel_name.clone(),
            channel_image_url: item.channel_image_url.clone(),
            live_title: item.live_title.clone(),
            viewer_count: item.viewer_count,
            viewer_count_text: format_viewer_count(item.viewer_count),
            category_name: category(item.category_name.as_deref()),
            thumbnail_url: item.thumbnail_url.clone(),
            is_live: item.is_live,
            open_date: rfc3339(item.open_date),
            adult: false,
        }
    }
}

/// 라이브 목록 → 카드. 채널 정보 없는 항목은 버리고, 같은 채널은 처음 것만 남긴다.
pub fn live_cards(infos: &[LiveInfo]) -> Vec<LiveCardDto> {
    let mut seen = std::collections::HashSet::new();
    infos
        .iter()
        .filter_map(LiveCardDto::from_live_info)
        .filter(|c| seen.insert(c.channel_id.clone()))
        .collect()
}

/// 팔로잉 채널 → 라이브 중인 카드만, 시청자 많은 순.
pub fn following_live_cards(items: &[LiveChannelItem]) -> Vec<LiveCardDto> {
    let mut cards: Vec<LiveCardDto> = items
        .iter()
        .filter(|i| i.is_live)
        .map(LiveCardDto::from_channel_item)
        .collect();
    cards.sort_by_key(|c| std::cmp::Reverse(c.viewer_count));
    cards
}

// MARK: - 채널 · 라이브 상세

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelDto {
    pub channel_id: String,
    pub channel_name: String,
    pub channel_image_url: Option<String>,
    pub verified: bool,
    pub follower_count: i64,
    pub follower_count_text: String,
    pub description: Option<String>,
    pub open_live: bool,
}

impl From<&ChannelInfo> for ChannelDto {
    fn from(c: &ChannelInfo) -> Self {
        Self {
            channel_id: c.channel_id.clone(),
            channel_name: c.channel_name.clone(),
            channel_image_url: url_string(c.channel_image_url.as_ref()),
            verified: c.verified_mark,
            follower_count: c.follower_count,
            follower_count_text: format_viewer_count(c.follower_count),
            description: c.channel_description.clone().filter(|d| !d.is_empty()),
            open_live: c.open_live,
        }
    }
}

/// 시청 화면 헤더용 라이브 상세.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveDetailDto {
    pub live_id: i64,
    pub channel_id: String,
    pub channel_name: String,
    pub channel_image_url: Option<String>,
    pub verified: bool,
    pub live_title: String,
    /// `OPEN` / `CLOSE`.
    pub status: String,
    pub is_live: bool,
    pub viewer_count: i64,
    pub viewer_count_text: String,
    /// 누적 시청자 수(`accumulateCount`) — 응답에 없으면 0.
    pub accumulate_count: i64,
    pub accumulate_count_text: String,
    pub category_name: Option<String>,
    pub chat_channel_id: Option<String>,
    pub thumbnail_url: Option<String>,
    pub open_date: Option<String>,
    /// 종료된 방송이면 끝난 시각(마지막 방송).
    pub close_date: Option<String>,
    pub adult: bool,
    pub tags: Vec<String>,
}

impl LiveDetailDto {
    /// `channel_id` 는 요청한 채널(응답에 채널이 빠진 경우의 기준).
    pub fn from_live_info(info: &LiveInfo, channel_id: &str) -> Self {
        let channel = info.channel.as_ref();
        Self {
            live_id: info.live_id,
            channel_id: channel
                .map(|c| c.channel_id.clone())
                .filter(|id| !id.is_empty())
                .unwrap_or_else(|| channel_id.to_string()),
            channel_name: channel.map(|c| c.channel_name.clone()).unwrap_or_default(),
            channel_image_url: channel.and_then(|c| url_string(c.channel_image_url.as_ref())),
            verified: channel.is_some_and(|c| c.verified_mark),
            live_title: info.live_title.clone(),
            status: info.status.raw_value().to_string(),
            is_live: info.status == LiveStatus::Open,
            viewer_count: info.concurrent_user_count,
            viewer_count_text: format_viewer_count(info.concurrent_user_count),
            accumulate_count: info.accumulate_count,
            accumulate_count_text: format_viewer_count(info.accumulate_count),
            category_name: category(info.live_category_value.as_deref()),
            chat_channel_id: info.chat_channel_id.clone().filter(|s| !s.is_empty()),
            thumbnail_url: url_string(info.resolved_live_image_url()),
            open_date: rfc3339(info.open_date),
            close_date: rfc3339(info.close_date),
            adult: info.adult,
            tags: info.tags.clone(),
        }
    }
}

/// 통합 검색 결과.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResultsDto {
    pub keyword: String,
    pub channels: Vec<ChannelDto>,
    pub lives: Vec<LiveCardDto>,
}

// MARK: - 인증

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatusDto {
    pub logged_in: bool,
    /// `loggedOut` / `loggingIn` / `loggedIn` / `expired` / `error`.
    pub state: String,
    /// 로그인 방식(`cookie` / `oauth`).
    pub method: Option<String>,
    pub message: Option<String>,
    pub nickname: Option<String>,
    pub profile_image_url: Option<String>,
}

impl AuthStatusDto {
    pub fn from_state(state: &AuthState) -> Self {
        let (name, method, message) = match state {
            AuthState::LoggedOut => ("loggedOut", None, None),
            AuthState::LoggingIn => ("loggingIn", None, None),
            AuthState::LoggedIn { user_id } => ("loggedIn", Some(user_id.clone()), None),
            AuthState::Expired => ("expired", None, Some("로그인이 만료되었습니다".to_string())),
            AuthState::Error(e) => ("error", None, Some(e.clone())),
        };
        Self {
            logged_in: state.is_logged_in(),
            state: name.into(),
            method,
            message,
            nickname: None,
            profile_image_url: None,
        }
    }

    pub fn with_profile(mut self, nickname: Option<String>, image: Option<String>) -> Self {
        self.nickname = nickname.filter(|n| !n.is_empty());
        self.profile_image_url = image.filter(|n| !n.is_empty());
        self
    }
}

// MARK: - 채팅

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessageDto {
    pub id: String,
    pub user_id: String,
    pub nickname: String,
    pub content: String,
    /// 유닉스 밀리초.
    pub timestamp: i64,
    /// `normal` / `donation` / `subscription` / `system` / `notice`.
    pub kind: String,
    /// `streamer` / `manager` / `channelManager` / `viewer`.
    pub role: String,
    pub badges: Vec<String>,
    pub emojis: EmojiMap,
    pub donation_amount: Option<i64>,
    /// 후원 종류 — `chat` / `video` / `mission` 등(치지직 `extras.donation.type` 소문자).
    pub donation_type: Option<String>,
    pub subscription_months: Option<i64>,
    /// 구독 티어 이름(있으면).
    pub subscription_tier: Option<String>,
    pub title_name: Option<String>,
    pub title_color: Option<String>,
    pub is_blinded: bool,
    /// 선물 종류(예: `SUBSCRIPTION_GIFT`) — 있으면 선물 구독으로 그린다.
    pub gift_type: Option<String>,
    pub gift_count: Option<i64>,
    pub mission_text: Option<String>,
    pub anonymous: bool,
}

fn message_kind(t: MessageType) -> &'static str {
    match t {
        MessageType::Normal => "normal",
        MessageType::Donation => "donation",
        MessageType::Subscription => "subscription",
        MessageType::SystemMessage => "system",
        MessageType::Notice => "notice",
    }
}

fn role_name(r: UserRole) -> &'static str {
    match r {
        UserRole::Streamer => "streamer",
        UserRole::Manager => "manager",
        UserRole::ChannelManager => "channelManager",
        UserRole::Viewer => "viewer",
    }
}

impl ChatMessageDto {
    pub fn from_message(message: &ChatMessage, is_notice: bool) -> Self {
        let item = ChatMessageItem::from_message(message, is_notice);
        let kind = if item.is_notice {
            "notice"
        } else {
            message_kind(item.r#type)
        };
        Self {
            id: item.id,
            user_id: item.user_id,
            nickname: item.nickname,
            content: item.content,
            timestamp: item.timestamp.timestamp_millis(),
            kind: kind.into(),
            role: role_name(item.user_role).into(),
            badges: item
                .badges
                .iter()
                .filter_map(|b| b.image_url.clone())
                .filter(|u| !u.is_empty())
                .collect(),
            emojis: item.emojis,
            donation_amount: item.donation_amount,
            donation_type: donation_type(message),
            subscription_months: item.subscription_months,
            subscription_tier: message
                .extras
                .as_ref()
                .and_then(|e| e.subscription.as_ref())
                .and_then(|s| s.tier_name.clone())
                .filter(|t| !t.trim().is_empty()),
            title_name: item.title_name,
            title_color: item.title_color,
            is_blinded: item.is_blinded,
            gift_type: message.extras.as_ref().and_then(|e| e.gift_type.clone()),
            gift_count: message.extras.as_ref().and_then(|e| e.gift_count),
            mission_text: message.extras.as_ref().and_then(|e| e.mission_text.clone()),
            anonymous: message
                .extras
                .as_ref()
                .and_then(|e| e.is_anonymous)
                .unwrap_or(false),
        }
    }
}

/// 후원 종류(소문자). 후원 메시지가 아니거나 정보가 없으면 `None`.
pub fn donation_type(message: &ChatMessage) -> Option<String> {
    if message.r#type != MessageType::Donation {
        return None;
    }
    let t = message.extras.as_ref()?.donation.as_ref()?.r#type.trim();
    (!t.is_empty()).then(|| t.to_ascii_lowercase())
}

/// 주간 후원 랭킹 한 줄.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DonationRankDto {
    pub rank: i64,
    pub nickname: String,
    pub amount: i64,
    pub profile_image_url: Option<String>,
    pub verified: bool,
}

impl DonationRankDto {
    pub fn from_item(item: &cview_core::models::chat::DonationRankItem) -> Self {
        Self {
            rank: item.ranking,
            nickname: item.nick_name.clone(),
            amount: item.donation_amount,
            profile_image_url: item.profile_image_url.clone().filter(|u| !u.is_empty()),
            verified: item.verified_mark.unwrap_or(false),
        }
    }
}

fn messages(list: &[ChatMessage]) -> Vec<ChatMessageDto> {
    list.iter()
        .map(|m| ChatMessageDto::from_message(m, false))
        .collect()
}

/// `chat://event` 페이로드.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ChatEventDto {
    Connected,
    Disconnected {
        reason: String,
    },
    Reconnecting,
    State {
        /// `disconnected` / `connecting` / `connected` / `reconnecting` / `failed`.
        state: String,
        detail: Option<String>,
    },
    Messages {
        messages: Vec<ChatMessageDto>,
        recent: bool,
    },
    Notice {
        message: Box<ChatMessageDto>,
    },
    NoticeCleared,
    #[serde(rename_all = "camelCase")]
    Blinded {
        message_id: String,
        blind_text: Option<String>,
    },
    Kicked,
    #[serde(rename_all = "camelCase")]
    Penalized {
        user_id: String,
        duration: i64,
    },
    #[serde(rename_all = "camelCase")]
    SendRejected {
        tid: Option<i64>,
        ret_code: i64,
        message: Option<String>,
    },
    System {
        text: String,
    },
    Cleared,
}

impl ChatEventDto {
    pub fn from_state(state: &ChatConnectionState) -> Self {
        let (state, detail) = match state {
            ChatConnectionState::Disconnected => ("disconnected", None),
            ChatConnectionState::Connecting => ("connecting", None),
            ChatConnectionState::Connected { server_index } => {
                ("connected", Some(format!("server {server_index}")))
            }
            ChatConnectionState::Reconnecting { attempt } => {
                ("reconnecting", Some(format!("{attempt}")))
            }
            ChatConnectionState::Failed { reason } => ("failed", Some(reason.clone())),
        };
        Self::State {
            state: state.into(),
            detail,
        }
    }

    pub fn from_engine(event: &cview_chat::ChatEngineEvent) -> Self {
        use cview_chat::ChatEngineEvent as E;
        match event {
            E::Connected => Self::Connected,
            E::Disconnected { reason } => Self::Disconnected {
                reason: reason.clone(),
            },
            E::Reconnecting => Self::Reconnecting,
            E::StateChanged(s) => Self::from_state(s),
            E::NewMessages(list) | E::Donations(list) => Self::Messages {
                messages: messages(list),
                recent: false,
            },
            E::RecentMessages(list) => Self::Messages {
                messages: messages(list),
                recent: true,
            },
            E::Notice(m) => Self::Notice {
                message: Box::new(ChatMessageDto::from_message(m, true)),
            },
            E::NoticeCleared => Self::NoticeCleared,
            E::MessageBlinded {
                message_id,
                blind_text,
            } => Self::Blinded {
                message_id: message_id.clone(),
                blind_text: blind_text.clone(),
            },
            E::Kicked => Self::Kicked,
            E::UserPenalized { user_id, duration } => Self::Penalized {
                user_id: user_id.clone(),
                duration: *duration,
            },
            E::SendRejected {
                tid,
                ret_code,
                message,
            } => Self::SendRejected {
                tid: *tid,
                ret_code: *ret_code,
                message: message.clone(),
            },
            E::SystemMessage(text) => Self::System { text: text.clone() },
            E::MessagesCleared => Self::Cleared,
        }
    }
}

// MARK: - 플레이어

/// `player://event` 페이로드 (코디네이터 이벤트).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PlayerEventDto {
    /// `idle` / `loadingInfo` / … / `error` 와 오류 문구.
    Phase {
        phase: String,
        message: Option<String>,
    },
    Quality {
        name: String,
        resolution: String,
        bandwidth: i64,
    },
    Latency {
        current: f64,
        target: f64,
    },
    Buffer {
        level: f64,
        healthy: bool,
    },
    Error {
        message: String,
    },
    Ended,
    Stopped,
}

/// 재생 단계 이름 (JS 쪽 상태 표시용).
pub fn phase_name(phase: &StreamPhase) -> (&'static str, Option<String>) {
    match phase {
        StreamPhase::Idle => ("idle", None),
        StreamPhase::LoadingInfo => ("loadingInfo", None),
        StreamPhase::LoadingManifest => ("loadingManifest", None),
        StreamPhase::Connecting => ("connecting", None),
        StreamPhase::Playing => ("playing", None),
        StreamPhase::Paused => ("paused", None),
        StreamPhase::Buffering => ("buffering", None),
        StreamPhase::Reconnecting => ("reconnecting", None),
        StreamPhase::StreamEnded => ("streamEnded", None),
        StreamPhase::Error(m) => ("error", Some(m.clone())),
    }
}

impl PlayerEventDto {
    /// 프런트에 보낼 가치가 없는 이벤트(프록시 경로 등)는 `None`.
    pub fn from_stream_event(event: &StreamEvent) -> Option<Self> {
        Some(match event {
            StreamEvent::PhaseChanged(p) => {
                let (phase, message) = phase_name(p);
                Self::Phase {
                    phase: phase.into(),
                    message,
                }
            }
            StreamEvent::QualitySelected(q) | StreamEvent::QualityChanged(q) => Self::Quality {
                name: q.name.clone(),
                resolution: q.resolution.clone(),
                bandwidth: q.bandwidth,
            },
            StreamEvent::LatencyUpdate(l) => Self::Latency {
                current: l.current,
                target: l.target,
            },
            StreamEvent::BufferUpdate(b) => Self::Buffer {
                level: b.current_level,
                healthy: b.is_healthy,
            },
            StreamEvent::Error(m) => Self::Error { message: m.clone() },
            StreamEvent::StreamEnded => Self::Ended,
            StreamEvent::Stopped => Self::Stopped,
            StreamEvent::ProxyRouteChanged(_) | StreamEvent::TimeShiftEnded => return None,
        })
    }
}

/// `player_start` 응답.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackStartedDto {
    pub live: LiveDetailDto,
    pub audio_only: bool,
    /// 암호화(hls_aes) 방송이라 인증 프록시가 필요한가.
    pub requires_authenticated_proxy: bool,
}

#[cfg(test)]
mod tests;
