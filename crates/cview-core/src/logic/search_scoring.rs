//! 검색 정밀화: 자동완성 우선순위 + 채널 결과 ranking.
//!
//! 원본: `Sources/CViewCore/Logic/SearchScoring.swift`
//!
//! recent > following > watched > popular, Levenshtein ≤ 1 typo tolerance,
//! channel score(prefix relevance + follower weight).
//!
//! 원본 `channelScore(channel: ChannelInfo, ...)` 는 모델의 `channelName`·`followerCount` 만
//! 읽으므로 두 값을 직접 받는 [`channel_score`] 로 옮겼다.

use std::collections::HashSet;

use crate::utils::levenshtein;

/// 자동완성 후보의 출처.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SuggestionSource {
    Recent,
    Following,
    Watched,
    Popular,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub text: String,
    pub source: SuggestionSource,
}

/// 자동완성 후보 입력.
#[derive(Debug, Clone, Copy, Default)]
pub struct AutocompleteSources<'a> {
    pub recent: &'a [String],
    pub following: &'a [String],
    /// 최근 시청 채널 이름(로컬 미러). 팔로잉 다음·인기 앞이다.
    pub watched: &'a [String],
    pub popular: &'a [String],
}

/// 원본 기본 `limit`.
pub const DEFAULT_SUGGESTION_LIMIT: usize = 5;

/// 자동완성 제안 생성. recent → following → watched → popular 순으로 모은 뒤
/// 중복 제거(앞쪽 우선) + `limit` 개까지 자른다.
///
/// 매칭 조건: 소문자 부분 문자열 포함, 또는 Levenshtein ≤ 1.
pub fn autocomplete_suggestions(
    query: &str,
    sources: AutocompleteSources<'_>,
    limit: usize,
) -> Vec<Suggestion> {
    let q = query.to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let matches = |name: &str| {
        let lower = name.to_lowercase();
        lower.contains(&q) || levenshtein::distance(&lower, &q) <= 1
    };
    let groups = [
        (sources.recent, SuggestionSource::Recent),
        (sources.following, SuggestionSource::Following),
        (sources.watched, SuggestionSource::Watched),
        (sources.popular, SuggestionSource::Popular),
    ];
    let mut seen = HashSet::new();
    groups
        .into_iter()
        .flat_map(|(names, source)| names.iter().map(move |n| (n, source)))
        .filter(|(n, _)| matches(n))
        .filter(|(n, _)| seen.insert(n.as_str()))
        .take(limit)
        .map(|(n, source)| Suggestion {
            text: n.clone(),
            source,
        })
        .collect()
}

/// 채널 결과 ranking 점수. 정확 1000 / prefix 500 / contains 200 / typo 80 + log10 팔로워 가중치.
pub fn channel_score(channel_name: &str, follower_count: i64, query: &str) -> f64 {
    let q = query.to_lowercase();
    let q = q.trim_matches(|c: char| c.is_whitespace() && c != '\n' && c != '\r');
    if q.is_empty() {
        return 0.0;
    }
    let name = channel_name.to_lowercase();
    let mut score = if name == q {
        1000.0
    } else if name.starts_with(q) {
        500.0
    } else if name.contains(q) {
        200.0
    } else if levenshtein::distance(&name, q) <= 1 {
        80.0
    } else {
        0.0
    };
    // followerCount 가중치 (log 스케일): 1팔로워≈0, 1k≈3, 1M≈6 (×10)
    let followers = follower_count.max(0);
    if followers > 0 {
        score += (followers as f64 + 1.0).log10() * 10.0;
    }
    score
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/SearchScoringTests.swift`, `SearchAutocompleteTests.swift`(순수 부분)
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn priority() {
        let (r, f, p) = (v(&["abc1"]), v(&["abc2"]), v(&["abc3"]));
        let s = autocomplete_suggestions(
            "abc",
            AutocompleteSources {
                recent: &r,
                following: &f,
                popular: &p,
                ..Default::default()
            },
            DEFAULT_SUGGESTION_LIMIT,
        );
        assert_eq!(s[0].text, "abc1");
        assert!(s.iter().any(|x| x.text == "abc2"));
        assert!(s.iter().any(|x| x.text == "abc3"));
    }

    #[test]
    fn typo() {
        let f = v(&["abd"]);
        let s = autocomplete_suggestions(
            "abc",
            AutocompleteSources {
                following: &f,
                ..Default::default()
            },
            5,
        );
        assert!(s.iter().any(|x| x.text == "abd"));
    }

    #[test]
    fn empty_query() {
        let (r, f, p) = (v(&["a"]), v(&["b"]), v(&["c"]));
        let s = autocomplete_suggestions(
            "",
            AutocompleteSources {
                recent: &r,
                following: &f,
                popular: &p,
                ..Default::default()
            },
            5,
        );
        assert!(s.is_empty());
    }

    #[test]
    fn dedup() {
        let x = v(&["abc"]);
        let s = autocomplete_suggestions(
            "ab",
            AutocompleteSources {
                recent: &x,
                following: &x,
                popular: &x,
                ..Default::default()
            },
            5,
        );
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].source, SuggestionSource::Recent);
    }

    #[test]
    fn watched_order() {
        let (r, f, w, p) = (
            v(&["리그 검색"]),
            v(&["리그 팔로잉"]),
            v(&["리그 시청"]),
            v(&["리그 인기"]),
        );
        let s = autocomplete_suggestions(
            "리그",
            AutocompleteSources {
                recent: &r,
                following: &f,
                watched: &w,
                popular: &p,
            },
            8,
        );
        assert_eq!(
            s.iter().map(|x| x.text.as_str()).collect::<Vec<_>>(),
            ["리그 검색", "리그 팔로잉", "리그 시청", "리그 인기"]
        );
        use SuggestionSource::*;
        assert_eq!(
            s.iter().map(|x| x.source).collect::<Vec<_>>(),
            [Recent, Following, Watched, Popular]
        );
    }

    #[test]
    fn korean_typo_and_limit() {
        let f = v(&["우왁굳", "우왁굳 채널", "다른채널"]);
        let s = autocomplete_suggestions(
            "우왁군",
            AutocompleteSources {
                following: &f,
                ..Default::default()
            },
            5,
        );
        assert_eq!(
            s.iter().map(|x| x.text.as_str()).collect::<Vec<_>>(),
            ["우왁굳"]
        );
        let many: Vec<String> = (0..10).map(|i| format!("abc{i}")).collect();
        assert_eq!(
            autocomplete_suggestions(
                "abc",
                AutocompleteSources {
                    recent: &many,
                    ..Default::default()
                },
                5
            )
            .len(),
            5
        );
    }

    #[test]
    fn channel_score_order() {
        let exact = channel_score("abc", 100, "abc");
        let prefix = channel_score("abc-stream", 100, "abc");
        let contains = channel_score("xx-abc-yy", 100, "abc");
        let typo = channel_score("abd", 100, "abc");
        assert!(exact > prefix);
        assert!(prefix > contains);
        assert!(contains > typo);
        assert_eq!(channel_score("abc", 0, "   "), 0.0);
    }

    #[test]
    fn channel_score_follower() {
        assert!(channel_score("wakgood", 1_000_000, "wak") > channel_score("wakgood", 10, "wak"));
    }
}
