//! `Sources/CViewCore/Models/EmoticonModels.swift` 포팅 — 치지직 이모티콘 도메인 모델.
//!
//! Swift 의 `EmoticonParser` 는 `FIFOMemoCache` 로 결과를 메모이즈했다. 순수 함수이고
//! 캐시는 성능 최적화일 뿐이라 여기서는 생략했다(필요하면 호출측에서 감싼다).

use serde::{Deserialize, Serialize};

use super::ident::is_plausible_url;
use super::messages::EmojiMap;

/// 이모티콘 팩 정보.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmoticonPack {
    pub emoticon_pack_id: String,
    pub emoticon_pack_name: String,
    #[serde(rename = "emoticonPackImageUrl", default, skip_serializing_if = "Option::is_none")]
    pub emoticon_pack_image_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emoticon_pack_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emoticons: Option<Vec<EmoticonItem>>,
}

impl EmoticonPack {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            emoticon_pack_id: id.into(),
            emoticon_pack_name: name.into(),
            emoticon_pack_image_url: None,
            emoticon_pack_type: None,
            emoticons: None,
        }
    }

    /// `Identifiable.id`.
    pub fn id(&self) -> &str {
        &self.emoticon_pack_id
    }
}

/// 개별 이모티콘.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmoticonItem {
    pub emoticon_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emoticon_name: Option<String>,
    #[serde(rename = "imageUrl", default, skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    #[serde(rename = "darkModeImageUrl", default, skip_serializing_if = "Option::is_none")]
    pub dark_mode_image_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack_id: Option<String>,
}

impl EmoticonItem {
    pub fn new(emoticon_id: impl Into<String>) -> Self {
        Self {
            emoticon_id: emoticon_id.into(),
            emoticon_name: None,
            image_url: None,
            dark_mode_image_url: None,
            width: None,
            height: None,
            pack_id: None,
        }
    }

    /// `Identifiable.id`.
    pub fn id(&self) -> &str {
        &self.emoticon_id
    }

    /// 채팅에서 사용할 때의 패턴 (예: `{:emoticonId:}`).
    pub fn chat_pattern(&self) -> String {
        format!("{{:{}:}}", self.emoticon_id)
    }
}

/// 채널의 이모티콘 배포 정보 (API 응답).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmoticonDeploy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emoticon_packs: Option<Vec<EmoticonPack>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription_emoticon_packs: Option<Vec<EmoticonPack>>,
}

impl EmoticonDeploy {
    /// 모든 팩 (일반 + 구독).
    pub fn all_packs(&self) -> Vec<&EmoticonPack> {
        self.emoticon_packs
            .iter()
            .flatten()
            .chain(self.subscription_emoticon_packs.iter().flatten())
            .collect()
    }

    /// 모든 이모티콘 평탄화.
    pub fn all_emoticons(&self) -> Vec<&EmoticonItem> {
        self.all_packs()
            .into_iter()
            .flat_map(|p| p.emoticons.iter().flatten())
            .collect()
    }
}

/// 채팅 메시지에서 파싱된 세그먼트 (텍스트 또는 이모티콘).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChatContentSegment {
    Text(String),
    Emoticon { id: String, url: String },
}

impl ChatContentSegment {
    /// `Identifiable.id`. Swift 는 텍스트에 `hashValue` 를 썼다(프로세스마다 다름) —
    /// 여기서는 결정적인 FNV-1a 를 쓴다.
    pub fn id(&self) -> String {
        match self {
            Self::Text(t) => format!("text-{:x}", fnv1a(t)),
            Self::Emoticon { id, .. } => format!("emo-{id}"),
        }
    }
}

fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 채팅 메시지 내 이모티콘 패턴(`{:id:}`)을 파싱한다.
#[derive(Debug, Clone, Copy, Default)]
pub struct EmoticonParser;

impl EmoticonParser {
    pub fn new() -> Self {
        Self
    }

    /// `content` 와 `emojis`(`id → imageUrl`) 를 받아 세그먼트로 나눈다.
    pub fn parse(&self, content: &str, emojis: Option<&EmojiMap>) -> Vec<ChatContentSegment> {
        let Some(emojis) = emojis.filter(|e| !e.is_empty()) else {
            return vec![ChatContentSegment::Text(content.to_string())];
        };

        let mut segments = Vec::new();
        let mut remaining = content;

        while !remaining.is_empty() {
            // `{` 와 그 뒤의 첫 `}` 를 찾는다.
            let Some(open) = remaining.find('{') else {
                segments.push(ChatContentSegment::Text(remaining.to_string()));
                break;
            };
            let after_open = &remaining[open + 1..];
            let Some(close_rel) = after_open.find('}') else {
                segments.push(ChatContentSegment::Text(remaining.to_string()));
                break;
            };

            let before = &remaining[..open];
            if !before.is_empty() {
                segments.push(ChatContentSegment::Text(before.to_string()));
            }

            let raw_id = &after_open[..close_rel];
            // Chzzk 형식: {:d_54:} → raw ":d_54:" → 콜론 제거.
            let emoticon_id = raw_id.trim_matches(':');

            let url = emojis.get(emoticon_id).or_else(|| emojis.get(raw_id));
            match url {
                Some(u) if is_plausible_url(u) => segments.push(ChatContentSegment::Emoticon {
                    id: emoticon_id.to_string(),
                    url: u.clone(),
                }),
                _ => segments.push(ChatContentSegment::Text(format!("{{{raw_id}}}"))),
            }

            remaining = &after_open[close_rel + 1..];
        }

        segments
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> EmojiMap {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn chat_pattern_and_id() {
        let item = EmoticonItem::new("d_54");
        assert_eq!(item.chat_pattern(), "{:d_54:}");
        assert_eq!(item.id(), "d_54");
        assert_eq!(EmoticonPack::new("p1", "Pack").id(), "p1");
    }

    #[test]
    fn json_decoding_key_mapping() {
        let item: EmoticonItem = serde_json::from_str(
            r#"{"emoticonId":"emo_1","emoticonName":"하하","imageUrl":"https://example.com/emo.png","darkModeImageUrl":"https://example.com/emo_dark.png","width":48,"height":48,"packId":"pack_1"}"#,
        )
        .unwrap();
        assert_eq!(item.emoticon_id, "emo_1");
        assert_eq!(item.emoticon_name.as_deref(), Some("하하"));
        assert_eq!(item.image_url.as_deref(), Some("https://example.com/emo.png"));
        assert_eq!(item.dark_mode_image_url.as_deref(), Some("https://example.com/emo_dark.png"));
        assert_eq!(item.width, Some(48));
        assert_eq!(item.pack_id.as_deref(), Some("pack_1"));

        let minimal: EmoticonItem = serde_json::from_str(r#"{"emoticonId":"e1"}"#).unwrap();
        assert_eq!(minimal.emoticon_name, None);
        assert_eq!(minimal.image_url, None);

        let pack: EmoticonPack = serde_json::from_str(
            r#"{"emoticonPackId":"p1","emoticonPackName":"테스트팩","emoticonPackImageUrl":"https://example.com/pack.png","emoticonPackType":"SUBSCRIPTION","emoticons":[{"emoticonId":"e1"}]}"#,
        )
        .unwrap();
        assert_eq!(pack.emoticon_pack_image_url.as_deref(), Some("https://example.com/pack.png"));
        assert_eq!(pack.emoticons.unwrap().len(), 1);
    }

    #[test]
    fn deploy_all_packs_and_emoticons() {
        let mut p1 = EmoticonPack::new("p1", "A");
        p1.emoticons = Some(vec![EmoticonItem::new("e1"), EmoticonItem::new("e2")]);
        let mut p2 = EmoticonPack::new("p2", "B");
        p2.emoticons = Some(vec![EmoticonItem::new("e3")]);
        let deploy = EmoticonDeploy {
            emoticon_packs: Some(vec![p1]),
            subscription_emoticon_packs: Some(vec![p2]),
        };
        assert_eq!(deploy.all_packs().len(), 2);
        assert_eq!(deploy.all_packs()[0].emoticon_pack_id, "p1");
        assert_eq!(deploy.all_emoticons().len(), 3);
        assert!(EmoticonDeploy::default().all_packs().is_empty());
        let empty = EmoticonDeploy { emoticon_packs: Some(vec![EmoticonPack::new("p", "A")]), ..Default::default() };
        assert!(empty.all_emoticons().is_empty());
    }

    #[test]
    fn segment_ids() {
        assert!(ChatContentSegment::Text("hello".into()).id().starts_with("text-"));
        let seg = ChatContentSegment::Emoticon { id: "d_54".into(), url: "https://x.com/a.png".into() };
        assert_eq!(seg.id(), "emo-d_54");
    }

    #[test]
    fn parser_cases() {
        let p = EmoticonParser::new();
        assert_eq!(p.parse("안녕하세요", None), vec![ChatContentSegment::Text("안녕하세요".into())]);
        assert_eq!(p.parse("hello", Some(&EmojiMap::new())).len(), 1);

        let emojis = map(&[("d_54", "https://example.com/d_54.png")]);
        assert_eq!(
            p.parse("{:d_54:}", Some(&emojis)),
            vec![ChatContentSegment::Emoticon {
                id: "d_54".into(),
                url: "https://example.com/d_54.png".into()
            }]
        );
        let mixed = p.parse("안녕 {:d_54:} 반가워", Some(&emojis));
        assert_eq!(mixed.len(), 3);
        assert_eq!(mixed[0], ChatContentSegment::Text("안녕 ".into()));
        assert_eq!(mixed[2], ChatContentSegment::Text(" 반가워".into()));
        assert_eq!(
            p.parse("{:unknown:}", Some(&emojis)),
            vec![ChatContentSegment::Text("{:unknown:}".into())]
        );

        let ab = map(&[("a", "https://example.com/a.png"), ("b", "https://example.com/b.png")]);
        let both = p.parse("{:a:}{:b:}", Some(&ab));
        assert_eq!(both.len(), 2);
        assert!(matches!(&both[1], ChatContentSegment::Emoticon { id, .. } if id == "b"));

        // 닫는 중괄호가 없으면 나머지는 텍스트.
        assert_eq!(p.parse("x {:a:", Some(&ab)), vec![ChatContentSegment::Text("x {:a:".into())]);
    }
}
