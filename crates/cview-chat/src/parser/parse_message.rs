//! 채팅/이모티콘/최근채팅/후원 메시지 파싱.
//! 원본: `Sources/CViewChat/ChatMessageParser+ParseMessage.swift`,
//! `Sources/CViewChat/ChatMessageParser+ParseDonation.swift`.

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

use super::{body_str, ChatMessageParser};
use crate::json::{any_int, fnv1a, ns_bool, ns_int, ns_object, ns_object_array, ns_str, to_foundation_json};
use crate::protocol::ChatProtocolMessage;
use cview_core::models::chat::{
    is_plausible_url, ChatBadge, ChatExtras, ChatMessage, ChatProfile, ChatTitle, DonationInfo, EmojiMap,
    MessageType, SubscriptionInfo,
};

type Dict = Map<String, Value>;

#[derive(Debug)]
struct ParsedProfile {
    nickname: String,
    profile_image_url: Option<String>,
    user_role_code: Option<String>,
    badges: Vec<ChatBadge>,
    activity_badges: Vec<ChatBadge>,
    title: Option<ChatTitle>,
}

impl Default for ParsedProfile {
    fn default() -> Self {
        Self {
            nickname: "Unknown".into(),
            profile_image_url: None,
            user_role_code: None,
            badges: Vec::new(),
            activity_badges: Vec::new(),
            title: None,
        }
    }
}

#[derive(Debug, Default)]
struct ParsedExtras {
    emojis: EmojiMap,
    donation: Option<DonationInfo>,
    subscription: Option<SubscriptionInfo>,
    is_donation: bool,
    is_anonymous: bool,
    is_subscription: bool,
    is_system_message: bool,
}

/// `URL(string:)` 근사 — 받아들일 수 없는 값은 nil.
fn url(s: &str) -> Option<String> {
    is_plausible_url(s).then(|| s.to_string())
}

/// `dict[a] as? String ?? dict[b] as? String`.
fn str_either(dict: &Dict, a: &str, b: &str) -> Option<String> {
    ns_str(dict.get(a)).or_else(|| ns_str(dict.get(b))).map(str::to_owned)
}

impl ChatMessageParser {
    /// cmd 93101 / 93103 / 15101 / 93102 공통. `bdy` 가 배열이거나 `{messageList: [...]}`.
    pub(crate) fn parse_chat_messages(&self, raw: &ChatProtocolMessage) -> Vec<ChatMessage> {
        let Some(body) = raw.bdy.as_ref() else { return Vec::new() };
        let items: &Vec<Value> = if let Some(array) = body.as_array() {
            array
        } else if let Some(list) = body.as_object().and_then(|d| d.get("messageList")).and_then(Value::as_array) {
            list
        } else {
            return Vec::new();
        };
        items
            .iter()
            .filter_map(Value::as_object)
            .map(|dict| self.parse_single_message(dict))
            .collect()
    }

    /// cmd 93102 — 실제 페이로드는 채팅 메시지와 같은 포맷.
    pub(crate) fn parse_donations(&self, raw: &ChatProtocolMessage) -> Vec<ChatMessage> {
        self.parse_chat_messages(raw)
    }

    fn parse_single_message(&self, dict: &Dict) -> ChatMessage {
        // 실시간 93101 은 축약 키, recent 15101 messageList 는 장문 키를 쓴다. 여기서 정규화.
        let msg = body_str(dict, "msg").or_else(|| body_str(dict, "content")).unwrap_or("").to_string();
        let uid = body_str(dict, "uid").or_else(|| body_str(dict, "userId")).unwrap_or("").to_string();
        let msg_time = any_int(dict.get("msgTime")).or_else(|| any_int(dict.get("messageTime"))).unwrap_or(0);
        let msg_type_code = any_int(dict.get("msgTypeCode")).or_else(|| any_int(dict.get("messageTypeCode")));

        let extras_object = embedded_dictionary(dict.get("extras"));
        let msg_id = stable_message_id(dict, &msg, &uid, msg_time, msg_type_code, extras_object.as_ref());

        let parsed_profile = parse_profile(dict.get("profile"));
        let parsed_extras = parse_extras(extras_object.as_ref());

        let status = body_str(dict, "msgStatusType").or_else(|| body_str(dict, "messageStatusType"));
        let is_clean_bot_blinded = status == Some("CBOTBLIND");
        let is_blinded = is_clean_bot_blinded || status == Some("HIDDEN");
        let blind_text = if is_clean_bot_blinded { Self::CLEAN_BOT_BLIND_TEXT } else { Self::ADMIN_BLIND_TEXT };

        // msgTypeCode 우선: 10 = donation, 11 = subscription.
        let message_type = match msg_type_code {
            Some(10) => MessageType::Donation,
            Some(11) => MessageType::Subscription,
            _ if parsed_extras.is_donation => MessageType::Donation,
            _ if parsed_extras.is_subscription => MessageType::Subscription,
            _ if parsed_extras.is_system_message => MessageType::SystemMessage,
            _ => MessageType::Normal,
        };

        // 익명 후원: isAnonymous 플래그 또는 닉네임이 비었거나 "Unknown".
        let mut nickname = parsed_profile.nickname;
        if parsed_extras.is_donation
            && (parsed_extras.is_anonymous || nickname.is_empty() || nickname == "Unknown")
        {
            nickname = "익명".into();
        }

        let profile = ChatProfile::new(
            nickname.clone(),
            parsed_profile.profile_image_url.as_deref().and_then(url),
            parsed_profile.user_role_code,
            parsed_profile.badges.first().cloned(),
            parsed_profile.badges,
            parsed_profile.title,
            parsed_profile.activity_badges,
        );

        // 가려진 메시지는 원문·이모티콘·후원 extras 를 비운다(누출 방지).
        let extras = (!is_blinded).then(|| ChatExtras {
            emojis: (!parsed_extras.emojis.is_empty()).then_some(parsed_extras.emojis),
            os_type: None,
            streaming_channel_id: None,
            donation: parsed_extras.donation,
            subscription: parsed_extras.subscription,
        });

        ChatMessage {
            id: msg_id,
            user_id: Some(uid),
            nickname,
            content: if is_blinded { blind_text.to_string() } else { msg },
            timestamp: DateTime::<Utc>::from_timestamp_millis(msg_time).unwrap_or_default(),
            r#type: if is_blinded { MessageType::Normal } else { message_type },
            profile: Some(profile),
            extras,
            is_blinded,
        }
    }
}

fn parse_profile(value: Option<&Value>) -> ParsedProfile {
    let mut result = ParsedProfile::default();
    // 문자열 JSON 과 객체 payload 가 같은 규칙을 쓴다.
    let Some(parsed) = embedded_dictionary(value) else { return result };

    result.nickname = ns_str(parsed.get("nickname")).unwrap_or("Unknown").to_string();
    result.profile_image_url = ns_str(parsed.get("profileImageUrl")).map(str::to_owned);
    if let Some(role) = ns_str(parsed.get("userRoleCode")) {
        result.user_role_code = Some(role.to_owned());
    }

    // 1) badge (구형 호환)
    if let Some(badge) = ns_object(parsed.get("badge")) {
        if let Some(img) = str_either(badge, "imageUrl", "imageURL") {
            result.badges.push(ChatBadge::new(url(&img), ns_str(badge.get("badgeId")).map(str::to_owned), None));
        }
    }
    // 2) streamingProperty.subscription.badge (구독 뱃지)
    if let Some(subscription) = ns_object(parsed.get("streamingProperty")).and_then(|p| ns_object(p.get("subscription"))) {
        if let Some(sub_url) = ns_object(subscription.get("badge")).and_then(|b| str_either(b, "imageUrl", "imageURL")) {
            let tier = ns_int(subscription.get("tier"));
            let months = ns_int(subscription.get("accumulativeMonth"));
            result.badges.push(ChatBadge::new(
                url(&sub_url),
                Some(format!("subscription_{}", tier.unwrap_or(0))),
                months.map(|m| format!("{m}개월 구독")),
            ));
        }
    }
    // 3) viewerBadges (채널 뱃지)
    if let Some(viewer_badges) = ns_object_array(parsed.get("viewerBadges")) {
        for vb in viewer_badges {
            if let Some(info) = ns_object(vb.get("badge")) {
                if let Some(img) = str_either(info, "imageUrl", "imageURL") {
                    let id = ns_str(info.get("badgeId")).map(str::to_owned);
                    result.badges.push(ChatBadge::new(url(&img), id.clone(), id));
                }
            }
        }
    }
    // 칭호
    if let Some(title) = ns_object(parsed.get("title")) {
        let name = ns_str(title.get("name")).unwrap_or("");
        if !name.is_empty() {
            result.title = Some(ChatTitle {
                name: name.to_owned(),
                color: ns_str(title.get("color")).map(str::to_owned),
            });
        }
    }
    // 활동 뱃지
    if let Some(activity) = ns_object_array(parsed.get("activityBadges")) {
        for badge in activity {
            if let Some(img) = str_either(badge, "imageUrl", "imageURL") {
                result.activity_badges.push(ChatBadge::new(
                    url(&img),
                    ns_str(badge.get("badgeId")).map(str::to_owned),
                    str_either(badge, "title", "badgeNo"),
                ));
            }
        }
    }
    result
}

fn parse_extras(object: Option<&Dict>) -> ParsedExtras {
    let mut result = ParsedExtras::default();
    let Some(dict) = object else { return result };

    // Emojis — 문자열 값만 받는다. null·숫자 등은 유효한 URL 이 될 수 없으므로 조용히 버린다.
    if let Some(emojis) = ns_object(dict.get("emojis")) {
        result.emojis = emojis
            .iter()
            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
            .collect();
        if !result.emojis.is_empty() {
            tracing::debug!(count = result.emojis.len(), "parseExtras: 메시지당 이모티콘 수신");
        }
    }

    // Donation — payAmount 는 정수 / 실수 / "1000" / "1,000" 모두 가능.
    if let Some(pay) = ChatMessageParser::parse_amount(dict.get("payAmount")).filter(|p| *p > 0) {
        result.is_donation = true;
        result.donation = Some(DonationInfo {
            amount: pay,
            currency: "KRW".into(),
            r#type: ns_str(dict.get("donationType")).unwrap_or("CHAT").to_owned(),
        });
    }

    if ns_bool(dict.get("isAnonymous")) == Some(true) || ns_bool(dict.get("anonymous")) == Some(true) {
        result.is_anonymous = true;
    }

    if let Some(chat_type) = ns_str(dict.get("chatType")) {
        result.is_subscription = chat_type == "SUBSCRIPTION";
        result.is_system_message = chat_type == "SYSTEM";
    }

    if result.is_subscription {
        let month = ChatMessageParser::parse_amount(dict.get("month"))
            .or_else(|| ChatMessageParser::parse_amount(dict.get("accumulativeMonth")))
            .or_else(|| ChatMessageParser::parse_amount(dict.get("months")))
            .unwrap_or(1);
        result.subscription = Some(SubscriptionInfo { months: month, tier_name: str_either(dict, "tierName", "tierInfo") });
    }
    result
}

/// `extras`/`profile` 은 JSON 문자열 또는 객체 — 둘 다 객체로 정규화한다.
pub(crate) fn embedded_dictionary(value: Option<&Value>) -> Option<Dict> {
    match value? {
        Value::String(s) => match serde_json::from_str::<Value>(s) {
            Ok(Value::Object(map)) => Some(map),
            _ => None,
        },
        Value::Object(map) => Some(map.clone()),
        _ => None,
    }
}

/// 서버 제공 ID 또는 페이로드 해시 기반 결정론적 메시지 ID.
pub(crate) fn stable_message_id(
    dict: &Dict,
    msg: &str,
    uid: &str,
    msg_time: i64,
    msg_type_code: Option<i64>,
    normalized_extras: Option<&Dict>,
) -> String {
    for key in ["messageId", "msgId", "id", "chatLogId"] {
        if let Some(server_id) = body_str(dict, key).filter(|s| !s.is_empty()) {
            return server_id.to_owned();
        }
    }

    let type_part = msg_type_code.map(|c| c.to_string()).unwrap_or_else(|| "0".into());
    let fingerprint = extras_fingerprint(dict.get("extras"), normalized_extras);
    let payload = format!("{uid}|{msg_time}|{type_part}|{msg}|{fingerprint}");
    let hex = format!("{:x}", fnv1a(&payload));
    if uid.is_empty() {
        format!("anon_{msg_time}_{hex}")
    } else {
        format!("{uid}_{msg_time}_{hex}")
    }
}

/// 중첩 사전 순서 / JSON 공백 / 문자열 대 객체 표현 차이를 지운 extras 정체성.
fn extras_fingerprint(value: Option<&Value>, normalized: Option<&Dict>) -> String {
    let Some(value) = value else { return String::new() };
    if let Some(obj) = normalized {
        return to_foundation_json(&Value::Object(obj.clone()));
    }
    match value {
        Value::String(text) => match serde_json::from_str::<Value>(text) {
            Ok(v @ Value::Object(_)) => to_foundation_json(&v),
            _ => text.clone(),
        },
        Value::Object(_) => to_foundation_json(value),
        _ => String::new(),
    }
}
