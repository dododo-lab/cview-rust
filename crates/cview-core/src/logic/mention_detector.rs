//! 멀티채팅 메시지의 닉네임/키워드/역할 기반 mention 감지.
//!
//! 원본: `Sources/CViewCore/Multi/MentionDetector.swift`
//!
//! 모델 의존 분리: 원본 `detect(message: ChatMessageItem, ...)` 는 메시지의 `content` 와
//! `userRole`(streamer/manager/channelManager 여부)만 읽는다. 채팅 모델과 독립적이도록
//! 두 값을 직접 받는다 — 호출부는 `detect(&msg.content, msg.user_role.is_privileged(), ...)`.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// 감지 규칙. JSON 스키마는 `nickname`·`keywords`·`mutedChannels` 셋뿐이고,
/// 소문자 캐시는 생성/디코드 시 다시 계산한다(원본 `CodingKeys` 제외 필드).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "RuleRepr", into = "RuleRepr")]
pub struct MentionDetectionRule {
    nickname: Option<String>,
    keywords: Vec<String>,
    muted_channels: HashSet<String>,
    /// 빈 닉네임은 `None`.
    lowered_nickname: Option<String>,
    /// `keywords` 와 1:1 정렬(빈 항목은 "").
    lowered_keywords: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuleRepr {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    nickname: Option<String>,
    keywords: Vec<String>,
    muted_channels: HashSet<String>,
}

impl From<RuleRepr> for MentionDetectionRule {
    fn from(r: RuleRepr) -> Self {
        Self::new(r.nickname, r.keywords, r.muted_channels)
    }
}

impl From<MentionDetectionRule> for RuleRepr {
    fn from(r: MentionDetectionRule) -> Self {
        Self {
            nickname: r.nickname,
            keywords: r.keywords,
            muted_channels: r.muted_channels,
        }
    }
}

impl MentionDetectionRule {
    pub fn new(
        nickname: Option<String>,
        keywords: Vec<String>,
        muted_channels: HashSet<String>,
    ) -> Self {
        let lowered_nickname = nickname
            .as_ref()
            .map(|n| n.to_lowercase())
            .filter(|n| !n.is_empty());
        let lowered_keywords = keywords.iter().map(|k| k.to_lowercase()).collect();
        Self {
            nickname,
            keywords,
            muted_channels,
            lowered_nickname,
            lowered_keywords,
        }
    }

    pub fn nickname(&self) -> Option<&str> {
        self.nickname.as_deref()
    }

    pub fn keywords(&self) -> &[String] {
        &self.keywords
    }

    pub fn muted_channels(&self) -> &HashSet<String> {
        &self.muted_channels
    }
}

/// 감지 결과 한 종류. 직렬화 모양은 Swift 합성 `Codable` 과 같다
/// (`{"nickname":{}}`, `{"keyword":{"_0":"..."}}`, `{"streamer":{}}`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MentionMatch {
    Nickname {},
    Keyword {
        #[serde(rename = "_0")]
        keyword: String,
    },
    Streamer {},
}

impl MentionMatch {
    pub fn keyword(k: impl Into<String>) -> Self {
        Self::Keyword { keyword: k.into() }
    }
}

/// 메시지를 규칙에 대해 평가한다. 결과가 비어 있으면 mention 아님.
///
/// - `is_privileged_role`: 작성자가 streamer/manager/channelManager 인가 — mute 와 무관하게 `Streamer`.
/// - mute 채널이면 닉네임/키워드 매칭을 건너뛴다.
/// - 닉네임·키워드가 하나도 없으면 본문 소문자 변환 자체를 생략한다.
pub fn detect(
    content: &str,
    is_privileged_role: bool,
    chat_channel_id: &str,
    rules: &MentionDetectionRule,
) -> HashSet<MentionMatch> {
    let mut matches = HashSet::new();
    if is_privileged_role {
        matches.insert(MentionMatch::Streamer {});
    }
    if rules.muted_channels.contains(chat_channel_id) {
        return matches;
    }
    let has_keyword = rules.lowered_keywords.iter().any(|k| !k.is_empty());
    if rules.lowered_nickname.is_none() && !has_keyword {
        return matches;
    }
    let lower = content.to_lowercase();
    if rules
        .lowered_nickname
        .as_deref()
        .is_some_and(|n| lower.contains(n))
    {
        matches.insert(MentionMatch::Nickname {});
    }
    for (lowered, original) in rules.lowered_keywords.iter().zip(&rules.keywords) {
        if !lowered.is_empty() && lower.contains(lowered.as_str()) {
            matches.insert(MentionMatch::keyword(original.clone()));
        }
    }
    matches
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewCoreTests/MentionDetectorTests.swift`
    use super::*;

    fn rule(nick: Option<&str>, keywords: &[&str], muted: &[&str]) -> MentionDetectionRule {
        MentionDetectionRule::new(
            nick.map(str::to_string),
            keywords.iter().map(|s| s.to_string()).collect(),
            muted.iter().map(|s| s.to_string()).collect(),
        )
    }

    #[test]
    fn nickname_match() {
        let m = detect("도도 안녕!", false, "ch1", &rule(Some("도도"), &[], &[]));
        assert!(m.contains(&MentionMatch::Nickname {}));
        let m = detect("hello DoDo", false, "ch1", &rule(Some("dodo"), &[], &[]));
        assert!(m.contains(&MentionMatch::Nickname {}), "대소문자 무시");
    }

    #[test]
    fn no_nickname_no_match() {
        let m = detect("그냥 채팅", false, "ch1", &rule(Some("도도"), &[], &[]));
        assert!(!m.contains(&MentionMatch::Nickname {}));
    }

    #[test]
    fn keyword_match() {
        let r = rule(None, &["이벤트", "DONATION"], &[]);
        assert!(detect("오늘 이벤트 있어요", false, "ch1", &r)
            .contains(&MentionMatch::keyword("이벤트")));
        assert!(detect("thanks for the donation", false, "ch1", &r)
            .contains(&MentionMatch::keyword("DONATION")));
    }

    #[test]
    fn muted_channel_skips_non_streamer() {
        let r = rule(Some("도도"), &["이벤트"], &["ch1"]);
        assert!(detect("도도 이벤트", false, "ch1", &r).is_empty());
        assert!(detect("안녕", true, "ch1", &r).contains(&MentionMatch::Streamer {}));
    }

    #[test]
    fn streamer_hi_priority() {
        let r = rule(None, &[], &[]);
        assert!(detect("방송 시작", true, "ch1", &r).contains(&MentionMatch::Streamer {}));
    }

    #[test]
    fn empty_rule_no_match() {
        assert!(detect("안녕하세요", false, "ch1", &rule(None, &[], &[])).is_empty());
        assert!(detect("안녕하세요", false, "ch1", &rule(Some(""), &[""], &[])).is_empty());
    }

    #[test]
    fn codable_schema_matches_swift() {
        let r = rule(Some("Dodo"), &["Event"], &["ch1"]);
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"nickname": "Dodo", "keywords": ["Event"], "mutedChannels": ["ch1"]})
        );
        let decoded: MentionDetectionRule =
            serde_json::from_str(r#"{"keywords":["EVENT"],"mutedChannels":[]}"#).unwrap();
        assert_eq!(decoded.nickname(), None);
        assert!(detect("an event", false, "x", &decoded).contains(&MentionMatch::keyword("EVENT")));

        assert_eq!(
            serde_json::to_string(&MentionMatch::keyword("a")).unwrap(),
            r#"{"keyword":{"_0":"a"}}"#
        );
        assert_eq!(
            serde_json::to_string(&MentionMatch::Streamer {}).unwrap(),
            r#"{"streamer":{}}"#
        );
        let back: MentionMatch = serde_json::from_str(r#"{"nickname":{}}"#).unwrap();
        assert_eq!(back, MentionMatch::Nickname {});
    }
}
