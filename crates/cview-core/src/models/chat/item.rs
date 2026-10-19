//! `Sources/CViewCore/Models/ChatMessageItem.swift` 포팅 — 채팅 행 표시용 뷰모델 값.

use std::collections::HashSet;

use chrono::{DateTime, Local, Utc};

use super::emoticon::{ChatContentSegment, EmoticonParser};
use super::ident::new_uuid_string;
use super::messages::{ChatBadge, ChatMessage, EmojiMap, MessageType, UserRole};
use super::notice_kind::ChatSystemNoticeKind;

/// 채팅 메시지를 화면에 표시하기 위한 값.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChatMessageItem {
    pub id: String,
    pub user_id: String,
    pub nickname: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub r#type: MessageType,
    pub badge_image_url: Option<String>,
    pub emojis: EmojiMap,
    pub donation_amount: Option<i64>,
    pub donation_type: Option<String>,
    pub subscription_months: Option<i64>,
    pub profile_image_url: Option<String>,
    pub is_notice: bool,
    pub is_system: bool,
    pub user_role: UserRole,
    pub badges: Vec<ChatBadge>,
    pub title_name: Option<String>,
    pub title_color: Option<String>,
    /// 관리자/클린봇에 의해 가려진 메시지 — 제거하지 않고 placeholder 로 교체한다.
    pub is_blinded: bool,
    /// 시스템 행의 의미. 생산 지점이 선언한다.
    pub system_notice_kind: ChatSystemNoticeKind,
}

impl ChatMessageItem {
    /// 삭제 안내 기본 문구 — 서버가 사유를 주지 않는 관리자 blind(cmd 94008)용.
    pub const ADMIN_BLIND_TEXT: &'static str = "관리자에 의해 삭제된 메시지입니다.";

    /// Swift `init(from:isNotice:)`.
    pub fn from_message(message: &ChatMessage, is_notice: bool) -> Self {
        let profile = message.profile.as_ref();
        let extras = message.extras.as_ref();

        // blinded 메시지는 이모티콘을 강제로 비운다(원문 누출 방지).
        let emojis = if message.is_blinded {
            EmojiMap::new()
        } else {
            extras.and_then(|e| e.emojis.clone()).unwrap_or_default()
        };

        let empty: Vec<ChatBadge> = Vec::new();
        let profile_badges = profile.map(|p| &p.badges).unwrap_or(&empty);
        let subscription_months = extras
            .and_then(|e| e.subscription.as_ref())
            .map(|s| s.months)
            .or_else(|| Self::extract_subscription_months_from_badges(profile_badges));

        // 배지 합산 시 이미지 URL 기준 중복 제거 (URL 없는 배지는 유지).
        let mut seen: HashSet<String> = HashSet::new();
        let badges: Vec<ChatBadge> = profile
            .map(|p| p.badges.iter().chain(p.activity_badges.iter()))
            .into_iter()
            .flatten()
            .filter(|b| match &b.image_url {
                None => true,
                Some(url) => seen.insert(url.clone()),
            })
            .cloned()
            .collect();

        Self {
            id: message.id.clone(),
            user_id: message.user_id.clone().unwrap_or_else(|| "unknown".into()),
            nickname: message.nickname.clone(),
            content: message.content.clone(),
            timestamp: message.timestamp,
            r#type: message.r#type,
            badge_image_url: profile
                .and_then(|p| p.badge.as_ref())
                .and_then(|b| b.image_url.clone()),
            emojis,
            donation_amount: extras.and_then(|e| e.donation.as_ref()).map(|d| d.amount),
            donation_type: extras
                .and_then(|e| e.donation.as_ref())
                .map(|d| d.r#type.clone()),
            subscription_months,
            profile_image_url: profile.and_then(|p| p.profile_image_url.clone()),
            is_notice,
            is_system: false,
            user_role: profile.map(|p| p.user_role).unwrap_or_default(),
            badges,
            title_name: profile
                .and_then(|p| p.title.as_ref())
                .map(|t| t.name.clone()),
            title_color: profile
                .and_then(|p| p.title.as_ref())
                .and_then(|t| t.color.clone()),
            is_blinded: message.is_blinded,
            system_notice_kind: ChatSystemNoticeKind::Info,
        }
    }

    /// 시스템 행. Swift `static func system(_:kind:)`.
    pub fn system(message: impl Into<String>, kind: ChatSystemNoticeKind) -> Self {
        Self {
            id: new_uuid_string(),
            user_id: "system".into(),
            nickname: "시스템".into(),
            content: message.into(),
            timestamp: Utc::now(),
            r#type: MessageType::SystemMessage,
            badge_image_url: None,
            emojis: EmojiMap::new(),
            donation_amount: None,
            donation_type: None,
            subscription_months: None,
            profile_image_url: None,
            is_notice: false,
            is_system: true,
            user_role: UserRole::Viewer,
            badges: Vec::new(),
            title_name: None,
            title_color: None,
            is_blinded: false,
            system_notice_kind: kind,
        }
    }

    /// 최소 필드로 만드는 생성자(Swift memberwise init 의 기본값 버전).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        user_id: impl Into<String>,
        nickname: impl Into<String>,
        content: impl Into<String>,
        timestamp: DateTime<Utc>,
        r#type: MessageType,
        emojis: EmojiMap,
        is_notice: bool,
        is_system: bool,
    ) -> Self {
        Self {
            id: id.into(),
            user_id: user_id.into(),
            nickname: nickname.into(),
            content: content.into(),
            timestamp,
            r#type,
            badge_image_url: None,
            emojis,
            donation_amount: None,
            donation_type: None,
            subscription_months: None,
            profile_image_url: None,
            is_notice,
            is_system,
            user_role: UserRole::Viewer,
            badges: Vec::new(),
            title_name: None,
            title_color: None,
            is_blinded: false,
            system_notice_kind: ChatSystemNoticeKind::Info,
        }
    }

    /// `HH:mm` (로컬 시간대).
    pub fn formatted_time(&self) -> String {
        self.timestamp
            .with_timezone(&Local)
            .format("%H:%M")
            .to_string()
    }

    /// 이미지를 그릴 수 없는 곳(OS 알림·멘션 트레이)용 본문 미리보기.
    pub fn plain_text_preview(&self) -> String {
        let content = self.emoticon_placeholder_content();
        let trimmed = content.trim();
        if trimmed.is_empty() {
            "새 메시지가 도착했습니다.".into()
        } else {
            trimmed.to_string()
        }
    }

    /// 이모티콘을 이미지로 그릴 수 없는 **본문 경로**용 문자열.
    /// `plain_text_preview` 와 같은 치환 규칙이지만 트리밍/빈 본문 폴백은 하지 않는다.
    pub fn emoticon_placeholder_content(&self) -> String {
        let rendered: String = EmoticonParser::new()
            .parse(&self.content, Some(&self.emojis))
            .into_iter()
            .map(|seg| match seg {
                ChatContentSegment::Text(t) => t,
                ChatContentSegment::Emoticon { .. } => "[이모티콘]".to_string(),
            })
            .collect();
        if !rendered.contains("{:") {
            return rendered;
        }
        replace_chzzk_codes(&rendered, "[이모티콘]")
    }

    /// emojis 필드만 교체한 복사본. (Swift 와 같이 `system_notice_kind` 는 `Info` 로 돌아간다.)
    pub fn with_emojis(&self, new_emojis: EmojiMap) -> Self {
        Self {
            emojis: new_emojis,
            system_notice_kind: ChatSystemNoticeKind::Info,
            ..self.clone()
        }
    }

    /// 가려진 메시지로 교체 (id/userId/nickname/timestamp/profileImageUrl/userRole 유지).
    pub fn blinded(&self, text: &str) -> Self {
        Self {
            id: self.id.clone(),
            user_id: self.user_id.clone(),
            nickname: self.nickname.clone(),
            content: text.to_string(),
            timestamp: self.timestamp,
            r#type: self.r#type,
            badge_image_url: None,
            emojis: EmojiMap::new(),
            donation_amount: None,
            donation_type: None,
            subscription_months: None,
            profile_image_url: self.profile_image_url.clone(),
            is_notice: false,
            is_system: false,
            user_role: self.user_role,
            badges: Vec::new(),
            title_name: None,
            title_color: None,
            is_blinded: true,
            system_notice_kind: ChatSystemNoticeKind::Info,
        }
    }

    /// 뱃지 배열에서 구독 개월 수 추출 (`badgeId` 가 `subscription` 으로 시작하는 뱃지의
    /// `altText` 에서 첫 ASCII 숫자 런).
    fn extract_subscription_months_from_badges(badges: &[ChatBadge]) -> Option<i64> {
        badges
            .iter()
            .filter(|b| {
                b.badge_id
                    .as_deref()
                    .is_some_and(|id| id.starts_with("subscription"))
            })
            .find_map(|b| b.alt_text.as_deref().and_then(first_ascii_int_run))
    }
}

/// 문자열에서 첫 번째 연속 ASCII 숫자 런을 정수로 (없거나 범위 초과면 None).
fn first_ascii_int_run(s: &str) -> Option<i64> {
    let digits: String = s
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

/// 정규식 `\{:[^{}\s]+:\}` 매치를 `replacement` 로 치환한다.
fn replace_chzzk_codes(input: &str, replacement: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("{:") {
        out.push_str(&rest[..start]);
        let body = &rest[start + 2..];
        // 본문은 `{`, `}`, 공백이 아닌 문자 1개 이상, 그 뒤 `:}`.
        let end = body
            .char_indices()
            .find(|(_, c)| *c == '{' || *c == '}' || c.is_whitespace())
            .map(|(i, _)| i)
            .unwrap_or(body.len());
        // 탐욕 매치 후 되짚기: `[^{}\s]+` 다음에 `:}` 가 와야 한다. 즉 run 끝이 `}` 이고
        // 그 직전이 `:` 이며 run 길이(콜론 제외)가 1 이상이어야 한다.
        let run = &body[..end];
        let matched = body[end..].starts_with('}') && run.len() >= 2 && run.ends_with(':');
        if matched {
            out.push_str(replacement);
            rest = &body[end + 1..];
        } else {
            out.push_str("{:");
            rest = body;
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::chat::{ChatExtras, ChatProfile, SubscriptionInfo};
    use chrono::TimeZone;

    fn item(content: &str, emojis: &[(&str, &str)]) -> ChatMessageItem {
        ChatMessageItem::new(
            "preview",
            "u",
            "닉",
            content,
            Utc::now(),
            MessageType::Normal,
            emojis
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            false,
            false,
        )
    }

    #[test]
    fn system_message_defaults() {
        let item = ChatMessageItem::system("연결됨", ChatSystemNoticeKind::Info);
        assert_eq!(item.nickname, "시스템");
        assert_eq!(item.content, "연결됨");
        assert!(item.is_system);
        assert!(!item.is_notice);
        assert_eq!(item.user_id, "system");
        assert_eq!(item.r#type, MessageType::SystemMessage);
        assert!(item.emojis.is_empty());
        assert_eq!(item.donation_amount, None);
    }

    #[test]
    fn from_message_maps_fields() {
        let msg = ChatMessage::with_id("test-id", "김치지", "안녕하세요!").user_id("user-42");
        let item = ChatMessageItem::from_message(&msg, false);
        assert_eq!(item.id, "test-id");
        assert_eq!(item.user_id, "user-42");
        assert_eq!(item.nickname, "김치지");
        assert!(!item.is_notice && !item.is_system);

        let guest = ChatMessageItem::from_message(&ChatMessage::new("guest", "hi"), false);
        assert_eq!(guest.user_id, "unknown");

        let notice = ChatMessage::new("공지", "서버 점검").message_type(MessageType::Notice);
        assert!(ChatMessageItem::from_message(&notice, true).is_notice);
    }

    #[test]
    fn badges_dedupe_and_subscription_fallback() {
        let sub = ChatBadge::new(
            Some("https://b/sub".into()),
            Some("subscription_1".into()),
            Some("12개월 구독".into()),
        );
        let dup = ChatBadge::new(Some("https://b/sub".into()), Some("act".into()), None);
        let profile = ChatProfile::new(
            "n",
            None,
            None,
            Some(sub.clone()),
            vec![sub.clone()],
            None,
            vec![dup],
        );
        let msg = ChatMessage::new("n", "c").profile(profile.clone());
        let item = ChatMessageItem::from_message(&msg, false);
        assert_eq!(item.badges.len(), 1);
        assert_eq!(item.subscription_months, Some(12));
        assert_eq!(item.badge_image_url.as_deref(), Some("https://b/sub"));

        let with_extras = ChatMessage::new("n", "c")
            .profile(profile)
            .extras(ChatExtras {
                subscription: Some(SubscriptionInfo {
                    months: 3,
                    tier_name: None,
                }),
                ..Default::default()
            });
        assert_eq!(
            ChatMessageItem::from_message(&with_extras, false).subscription_months,
            Some(3)
        );
    }

    #[test]
    fn blinded_message_drops_emojis() {
        let msg = ChatMessage::new("n", "가려짐")
            .blinded(true)
            .extras(ChatExtras {
                emojis: Some(
                    [("a".to_string(), "https://x".to_string())]
                        .into_iter()
                        .collect(),
                ),
                ..Default::default()
            });
        let item = ChatMessageItem::from_message(&msg, false);
        assert!(item.emojis.is_empty());
        assert!(item.is_blinded);

        let b = item.blinded(ChatMessageItem::ADMIN_BLIND_TEXT);
        assert_eq!(b.content, ChatMessageItem::ADMIN_BLIND_TEXT);
        assert!(b.is_blinded);
    }

    #[test]
    fn formatted_time_is_hh_mm() {
        let local = Local.with_ymd_and_hms(2024, 1, 2, 14, 5, 9).unwrap();
        let mut it = item("msg", &[]);
        it.timestamp = local.with_timezone(&Utc);
        assert_eq!(it.formatted_time(), "14:05");
    }

    #[test]
    fn plain_text_preview_rules() {
        assert_eq!(
            item(
                "안녕 {:d_54:} 반가워",
                &[("d_54", "https://example.com/d_54.png")]
            )
            .plain_text_preview(),
            "안녕 [이모티콘] 반가워"
        );
        assert_eq!(
            item("여기 {:pending_123:} 봐", &[]).plain_text_preview(),
            "여기 [이모티콘] 봐"
        );
        assert_eq!(
            item("값은 {count}개", &[]).plain_text_preview(),
            "값은 {count}개"
        );
    }

    #[test]
    fn placeholder_content_shares_rule_without_fallback() {
        assert_eq!(
            item(
                "안녕 {:d_54:} 반가워",
                &[("d_54", "https://example.com/d_54.png")]
            )
            .emoticon_placeholder_content(),
            "안녕 [이모티콘] 반가워"
        );
        assert_eq!(
            item("여기 {:pending_123:} 봐", &[]).emoticon_placeholder_content(),
            "여기 [이모티콘] 봐"
        );
        assert_eq!(
            item("값은 {count}개", &[]).emoticon_placeholder_content(),
            "값은 {count}개"
        );
        let blank = item("  ", &[]);
        assert_eq!(blank.emoticon_placeholder_content(), "  ");
        assert_eq!(blank.plain_text_preview(), "새 메시지가 도착했습니다.");
    }

    #[test]
    fn regex_equivalent_replacement_edge_cases() {
        assert_eq!(replace_chzzk_codes("{::}", "X"), "{::}");
        assert_eq!(replace_chzzk_codes("{:a:}", "X"), "X");
        assert_eq!(replace_chzzk_codes("{:a b:}", "X"), "{:a b:}");
        assert_eq!(replace_chzzk_codes("{:{:a:}", "X"), "{:X");
        assert_eq!(replace_chzzk_codes("{:a:b:}{:c:}", "X"), "XX");
    }

    #[test]
    fn equality() {
        let date = Utc::now();
        let mk = || {
            ChatMessageItem::new(
                "same",
                "u",
                "n",
                "c",
                date,
                MessageType::Normal,
                EmojiMap::new(),
                false,
                false,
            )
        };
        assert_eq!(mk(), mk());
        assert_ne!(
            ChatMessageItem::system("msg", ChatSystemNoticeKind::Info),
            ChatMessageItem::system("msg", ChatSystemNoticeKind::Info)
        );
    }
}
