//! 채팅 프로필 카드 — 치지직 웹이 채팅 닉네임을 눌렀을 때 띄우는 카드의 원천.
//!
//! `GET comm-api.game.naver.com/nng_main/v1/chats/{chatChannelId}/users/{userIdHash}/profile-card?chatType=STREAMING`
//! 로그인 없이도 받는다. 응답 모양이 자주 바뀌므로 필드마다 관대하게 읽는다 — 하나가 어긋나도 카드는 남는다.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::models::decode::LenientObject;
use crate::models::verified_badge;

/// 카드에 그릴 배지 한 개(채널 활동·치트키 등).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileBadge {
    pub image_url: String,
    pub title: Option<String>,
    pub description: Option<String>,
    /// 사용자가 채팅에 달고 다니도록 켠 배지인가.
    pub active: bool,
}

/// 프로필 카드.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatProfileCard {
    pub user_id_hash: String,
    pub nickname: String,
    pub profile_image_url: Option<String>,
    /// `streamer` · `streaming_channel_manager` · `common_user` …(원문).
    pub user_role_code: Option<String>,
    /// 칭호(`title.name`) — 스트리머·매니저 등.
    pub title_name: Option<String>,
    pub title_color: Option<String>,
    /// 인증 마크 종류(`"partner"` · `"challenger"` · `"official"`).
    pub verified_badge: Option<&'static str>,
    /// 켠 배지가 앞, 나머지는 응답 순서.
    pub badges: Vec<ProfileBadge>,
    /// 이 채널을 팔로우한 날짜 원문(`streamingProperty.following.followDate`).
    pub follow_date: Option<String>,
    /// 누적 구독 개월(`streamingProperty.subscription.accumulativeMonth`).
    pub subscription_months: Option<i64>,
    pub subscription_tier: Option<i64>,
}

fn str_at(v: &Value, path: &[&str]) -> Option<String> {
    let mut cur = v;
    for k in path {
        cur = cur.get(k)?;
    }
    cur.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

fn int_at(v: &Value, path: &[&str]) -> Option<i64> {
    let mut cur = v;
    for k in path {
        cur = cur.get(k)?;
    }
    cur.as_i64().or_else(|| cur.as_str()?.trim().parse().ok())
}

fn http_url(s: Option<String>) -> Option<String> {
    s.filter(|u| u.starts_with("http"))
}

/// `viewerBadges`(새 모양: `{ badge: {...}, activatedV2 }`) — 없으면 `activityBadges`(옛 모양: 평평한 `{ imageUrl, activated }`).
fn parse_badges(o: &LenientObject) -> Vec<ProfileBadge> {
    let pick = |key: &str| match o.raw(key) {
        Some(Value::Array(items)) if !items.is_empty() => Some(items.clone()),
        _ => None,
    };
    let items = pick("viewerBadges")
        .or_else(|| pick("activityBadges"))
        .unwrap_or_default();
    let mut out: Vec<ProfileBadge> = Vec::new();
    for item in &items {
        let b = item.get("badge").unwrap_or(item);
        let Some(image_url) = http_url(str_at(b, &["imageUrl"])) else {
            continue;
        };
        if out.iter().any(|x| x.image_url == image_url) {
            continue;
        }
        let active = item
            .get("activatedV2")
            .or_else(|| item.get("activated"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        out.push(ProfileBadge {
            image_url,
            title: str_at(b, &["title"]),
            description: str_at(b, &["description"]),
            active,
        });
    }
    // 안정 정렬 — 켠 배지를 앞으로.
    out.sort_by_key(|b| !b.active);
    out
}

impl<'de> Deserialize<'de> for ChatProfileCard {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let o = LenientObject::from_deserializer(d)?;
        let prop = o.raw("streamingProperty").cloned().unwrap_or(Value::Null);
        let title = o.raw("title").cloned().unwrap_or(Value::Null);
        Ok(Self {
            user_id_hash: o.required::<_, D::Error>("userIdHash")?,
            nickname: o.get("nickname").unwrap_or_default(),
            profile_image_url: http_url(o.get("profileImageUrl")),
            user_role_code: o.get::<String>("userRoleCode").filter(|s| !s.is_empty()),
            title_name: str_at(&title, &["name"]),
            title_color: str_at(&title, &["color"]),
            verified_badge: verified_badge(
                o.get("verifiedMark").unwrap_or(false),
                o.get::<String>("verifiedMarkType").as_deref(),
            ),
            badges: parse_badges(&o),
            follow_date: str_at(&prop, &["following", "followDate"]),
            subscription_months: int_at(&prop, &["subscription", "accumulativeMonth"])
                .filter(|m| *m > 0),
            subscription_tier: int_at(&prop, &["subscription", "tier"]).filter(|t| *t > 0),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_streamer_card() {
        let card: ChatProfileCard = serde_json::from_str(
            r##"{"userIdHash":"abc","nickname":"달콤","profileImageUrl":"https://img/x.png",
            "userRoleCode":"streamer","badge":{"imageUrl":"https://s/streamer.png"},
            "title":{"name":"스트리머","color":"#D9B04F"},"verifiedMark":true,"verifiedMarkType":"PARTNER",
            "streamingProperty":{"nicknameColor":{"colorCode":"CC000"}},
            "viewerBadges":[
              {"type":"STANDARD","badge":{"imageUrl":"https://s/gift_10.png","title":"감동 주는 팬","description":"10개 이상"},"activatedV2":false},
              {"type":"STANDARD","badge":{"imageUrl":"https://s/cheat.png","title":"치트키 보유자","description":"24개월"},"activatedV2":true},
              {"type":"STANDARD","badge":{"imageUrl":"https://s/cheat.png","title":"중복"},"activatedV2":true},
              {"type":"STANDARD","badge":{"title":"그림 없음"}}
            ]}"##,
        )
        .unwrap();
        assert_eq!(card.user_id_hash, "abc");
        assert_eq!(card.nickname, "달콤");
        assert_eq!(card.profile_image_url.as_deref(), Some("https://img/x.png"));
        assert_eq!(card.title_name.as_deref(), Some("스트리머"));
        assert_eq!(card.title_color.as_deref(), Some("#D9B04F"));
        assert_eq!(card.verified_badge, Some("partner"));
        assert_eq!(card.badges.len(), 2);
        assert!(card.badges[0].active);
        assert_eq!(card.badges[0].title.as_deref(), Some("치트키 보유자"));
        assert_eq!(card.follow_date, None);
        assert_eq!(card.subscription_months, None);
    }

    #[test]
    fn parses_viewer_follow_and_subscription() {
        let card: ChatProfileCard = serde_json::from_str(
            r#"{"userIdHash":"u1","nickname":"시청자","profileImageUrl":null,"userRoleCode":"common_user",
            "title":null,"verifiedMark":false,
            "streamingProperty":{"following":{"followDate":"2024-02-19 23:28:11"},
              "subscription":{"accumulativeMonth":"7","tier":1}},
            "activityBadges":[{"imageUrl":"https://s/a.png","title":"활동","activated":true}]}"#,
        )
        .unwrap();
        assert_eq!(card.profile_image_url, None);
        assert_eq!(card.title_name, None);
        assert_eq!(card.verified_badge, None);
        assert_eq!(card.follow_date.as_deref(), Some("2024-02-19 23:28:11"));
        assert_eq!(card.subscription_months, Some(7));
        assert_eq!(card.subscription_tier, Some(1));
        assert_eq!(card.badges.len(), 1);
        assert!(card.badges[0].active);
    }

    #[test]
    fn tolerates_odd_shapes() {
        let card: ChatProfileCard = serde_json::from_str(
            r#"{"userIdHash":"u2","streamingProperty":"x","viewerBadges":"y","title":7,"profileImageUrl":"javascript:1"}"#,
        )
        .unwrap();
        assert_eq!(card.nickname, "");
        assert_eq!(card.profile_image_url, None);
        assert!(card.badges.is_empty());
        assert!(serde_json::from_str::<ChatProfileCard>(r#"{"nickname":"x"}"#).is_err());
    }
}
