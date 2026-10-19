//! `Sources/CViewCore/Models/LiveChannelItem.swift` · `ViewerCount.swift` · `DashboardModels.swift` 포팅.
//!
//! 라이브 채널 아이템(화면 행), 시청자수 한국어 단위 포맷(단일 출처), 대시보드 통계 값 타입.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

use super::decode::chzzk_date_opt;

/// 라이브 채널 정보를 표시하기 위한 행 모델.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveChannelItem {
    pub id: String,
    pub channel_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_image_url: Option<String>,
    pub live_title: String,
    pub viewer_count: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_name: Option<String>,
    /// `"GAME"` | `"SPORTS"` | `"ETC"`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail_url: Option<String>,
    pub channel_id: String,
    pub is_live: bool,
    #[serde(default, with = "chzzk_date_opt", skip_serializing_if = "Option::is_none")]
    pub open_date: Option<DateTime<Utc>>,
}

impl LiveChannelItem {
    /// [2026-05-16 DF1] `categoryName == nil` / 빈 문자열일 때 사용자에게 노출되는 단일 라벨.
    pub const UNCATEGORIZED_DISPLAY_NAME: &'static str = "분류 없음";

    /// 빈/Null categoryName 을 자동으로 표시용 라벨로 변환. 시각·통계·DB·위젯 모두 이 함수를 거친다.
    pub fn display_category_name(raw: Option<&str>) -> String {
        match raw {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => Self::UNCATEGORIZED_DISPLAY_NAME.to_string(),
        }
    }

    /// [2026-05-16 DF2] 채널 검색 매칭 — 글로벌·카테고리 내부 검색 모두 이 함수로 통일.
    ///
    /// 매칭 대상: `channelName`, `liveTitle`, (`include_category` 면) `categoryName` 중 하나라도 포함.
    /// 비교 방식: 대소문자·다이아크리틱 무시. 쿼리 양 끝 공백은 흡수하고, 비어 있으면 항상 참.
    pub fn matches_search(&self, query: &str, include_category: bool) -> bool {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return true;
        }
        let needle = fold_for_search(trimmed);
        if fold_for_search(&self.channel_name).contains(&needle) {
            return true;
        }
        if fold_for_search(&self.live_title).contains(&needle) {
            return true;
        }
        include_category
            && self
                .category_name
                .as_deref()
                .is_some_and(|c| fold_for_search(c).contains(&needle))
    }

    /// 시청자수 포맷 (예: "1.5만") — [`format_viewer_count`] 위임.
    pub fn formatted_viewer_count(&self) -> String {
        format_viewer_count(self.viewer_count)
    }
}

/// 대소문자·다이아크리틱 무시 비교용 정규화(Swift `[.caseInsensitive, .diacriticInsensitive]` 근사).
///
/// 한글 음절(U+AC00–U+D7A3)은 분해하지 않는다 — 분해하면 `"로"` 가 `"롤"` 안에서 매치되는
/// (자모 접두) 거짓 양성이 생긴다. 그 밖의 문자는 NFD 분해 후 결합 기호를 버린다.
fn fold_for_search(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if ('\u{AC00}'..='\u{D7A3}').contains(&c) {
            out.push(c);
            continue;
        }
        for d in std::iter::once(c).nfd() {
            if !is_combining_mark(d) {
                out.extend(d.to_lowercase());
            }
        }
    }
    out
}

/// `divisor` 단위로 소수 첫째 자리까지 반올림한 값.
fn korean_unit_value(value: i64, divisor: f64) -> f64 {
    ((value as f64 / divisor) * 10.0).round() / 10.0
}

/// 반올림 결과가 `next` 단위에 닿는지 — 닿으면 상위 단위가 받아야 한다.
/// [UIUX 부록D D38 2026-09-21] `9_999` 가 `"10.0천"` 이 되던 사다리 결함을 닫는다.
fn reaches_next_unit(value: i64, divisor: f64, next: f64) -> bool {
    korean_unit_value(value, divisor) >= next / divisor
}

fn korean_unit_text(value: i64, divisor: f64, suffix: &str) -> String {
    format!("{:.1}{suffix}", korean_unit_value(value, divisor))
}

/// 시청자수 한국어 단위 축약 — `1.5만` / `1.2천` / `234`. 앱 전역 단일 출처 (Swift `Int.formattedViewerCount`).
pub fn format_viewer_count(value: i64) -> String {
    if value >= 10_000 {
        return korean_unit_text(value, 10_000.0, "만");
    }
    if value >= 1_000 {
        return if reaches_next_unit(value, 1_000.0, 10_000.0) {
            korean_unit_text(value, 10_000.0, "만")
        } else {
            korean_unit_text(value, 1_000.0, "천")
        };
    }
    value.to_string()
}

/// 억 단위까지 쓰는 한국어 축약 — `1.2억` / `1.5만` / `1.2천` / `234` (Swift `Int.formattedCompactCount`).
pub fn format_compact_count(value: i64) -> String {
    if value >= 100_000_000 {
        return korean_unit_text(value, 100_000_000.0, "억");
    }
    if value >= 10_000 {
        return if reaches_next_unit(value, 10_000.0, 100_000_000.0) {
            korean_unit_text(value, 100_000_000.0, "억")
        } else {
            korean_unit_text(value, 10_000.0, "만")
        };
    }
    format_viewer_count(value)
}

/// 카테고리별 통계.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CategoryStat {
    pub id: String,
    pub name: String,
    pub channel_count: i64,
    pub total_viewers: i64,
}

/// 카테고리 유형(GAME / SPORTS / ETC)별 통계.
#[derive(Debug, Clone, PartialEq)]
pub struct CategoryTypeStat {
    pub id: String,
    pub r#type: String,
    pub display_name: String,
    pub channel_count: i64,
    pub total_viewers: i64,
    pub percentage: f64,
}

impl CategoryTypeStat {
    pub fn display_name_for(r#type: &str) -> &'static str {
        match r#type {
            "GAME" => "게임",
            "SPORTS" => "스포츠",
            _ => "기타",
        }
    }
}

/// 시청자수 분포 구간.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ViewerBucket {
    pub id: String,
    pub label: String,
    pub count: i64,
    pub min_viewers: i64,
    pub max_viewers: i64,
}

#[cfg(test)]
mod tests {
    //! `LiveChannelItemSearchTests.swift` · `DashboardModelsTests.swift` 포팅 + 시청자수 포맷.
    use super::*;

    fn make(name: &str, title: &str, category: Option<&str>) -> LiveChannelItem {
        LiveChannelItem {
            id: "x".into(),
            channel_name: name.into(),
            channel_image_url: None,
            live_title: title.into(),
            viewer_count: 0,
            category_name: category.map(Into::into),
            category_type: None,
            thumbnail_url: None,
            channel_id: "x".into(),
            is_live: true,
            open_date: None,
        }
    }

    #[test]
    fn empty_query_matches_all() {
        assert!(make("임의 채널", "", None).matches_search("", true));
        assert!(make("임의 채널", "", None).matches_search("   ", true));
    }

    #[test]
    fn case_insensitive_and_title() {
        let ch = make("Faker", "", None);
        assert!(ch.matches_search("faker", true));
        assert!(ch.matches_search("FAK", true));
        let t = make("abc", "오늘 발로란트 합방", None);
        assert!(t.matches_search("발로", true));
        assert!(t.matches_search(" 합방 ", true));
    }

    #[test]
    fn category_toggle() {
        let ch = make("abc", "title", Some("League of Legends"));
        assert!(ch.matches_search("League", true));
        assert!(!ch.matches_search("League", false));
    }

    #[test]
    fn diacritic_insensitive() {
        let ch = make("Pokémon", "", None);
        assert!(ch.matches_search("pokemon", true));
        assert!(ch.matches_search("Pokémon", true));
    }

    #[test]
    fn korean_substring() {
        let ch = make("발로란트 강의 채널", "", None);
        assert!(ch.matches_search("발로란트", true));
        assert!(ch.matches_search("강의", true));
        assert!(!ch.matches_search("오버워치", true));
        assert!(!make("롤", "", None).matches_search("로", true));
    }

    #[test]
    fn no_match() {
        assert!(!make("alpha", "beta", Some("gamma")).matches_search("delta", true));
    }

    #[test]
    fn display_category_name() {
        assert_eq!(LiveChannelItem::display_category_name(None), "분류 없음");
        assert_eq!(LiveChannelItem::display_category_name(Some("")), "분류 없음");
        assert_eq!(LiveChannelItem::display_category_name(Some("롤")), "롤");
    }

    #[test]
    fn viewer_count_ladder() {
        assert_eq!(format_viewer_count(234), "234");
        assert_eq!(format_viewer_count(1_234), "1.2천");
        assert_eq!(format_viewer_count(9_949), "9.9천");
        assert_eq!(format_viewer_count(9_999), "1.0만");
        assert_eq!(format_viewer_count(15_000), "1.5만");
        assert_eq!(format_viewer_count(100_000), "10.0만");
        assert_eq!(format_compact_count(99_999_999), "1.0억");
        assert_eq!(format_compact_count(120_000_000), "1.2억");
        assert_eq!(format_compact_count(100_000), "10.0만");
        assert_eq!(format_compact_count(999), "999");
    }

    #[test]
    fn dashboard_models() {
        assert_eq!(CategoryTypeStat::display_name_for("GAME"), "게임");
        assert_eq!(CategoryTypeStat::display_name_for("SPORTS"), "스포츠");
        assert_eq!(CategoryTypeStat::display_name_for("MUSIC"), "기타");
        assert_eq!(CategoryTypeStat::display_name_for(""), "기타");
        let stat = CategoryStat { id: "cat1".into(), name: "게임".into(), channel_count: 10, total_viewers: 50_000 };
        assert_eq!(stat.channel_count, 10);
        let bucket =
            ViewerBucket { id: "b1".into(), label: "100-500".into(), count: 25, min_viewers: 100, max_viewers: 500 };
        assert_eq!(bucket.max_viewers, 500);
    }

    #[test]
    fn item_round_trip() {
        let item = make("a", "b", Some("c"));
        let back: LiveChannelItem = serde_json::from_str(&serde_json::to_string(&item).unwrap()).unwrap();
        assert_eq!(back, item);
    }
}
