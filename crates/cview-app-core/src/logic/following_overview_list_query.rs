//! `Sources/CViewApp/Logic/FollowingOverviewListQuery.swift` 포팅.
//!
//! 개요 탭 인라인 팔로잉 목록의 슬라이스 계약. 입력은 라이브가 앞, 오프라인이 뒤로 이미
//! 전순서 정렬돼 있어야 한다 — 여기서 다시 정렬하지 않는다(정렬 안정성 함정).

use std::collections::HashMap;

use cview_core::models::LiveChannelItem;

use crate::services::support::localized_standard_compare;

#[derive(Debug, Clone, PartialEq)]
pub struct Slice {
    pub rows: Vec<LiveChannelItem>,
    pub match_count: usize,
    pub live_count: usize,
    pub total_count: usize,
}

impl Slice {
    /// 캡 때문에 화면에 나오지 못한 개수.
    pub fn remaining(&self) -> usize {
        self.match_count.saturating_sub(self.rows.len())
    }
}

/// 필터 프리셋은 바뀌어도 원본은 자주 그대로다 — 정렬 결과를 재사용한다.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedSource {
    pub source: Vec<LiveChannelItem>,
    pub ordered: Vec<LiveChannelItem>,
    pub live_count: usize,
    pub categories: Vec<(String, usize)>,
}

pub struct FollowingOverviewListQuery;

impl FollowingOverviewListQuery {
    /// 접힌 상태에서 보여 줄 행 수(`upNextChannels(limit: 8)` 과 맞춘다).
    pub const COLLAPSED_LIMIT: usize = 8;
    /// 개요 팔로잉 페이지당 행 수.
    pub const EXPANDED_LIMIT: usize = 10;

    pub fn slice(all: &[LiveChannelItem], live_count: i64, live_only: bool, limit: i64) -> Slice {
        let clamped_live = (live_count.max(0) as usize).min(all.len());
        let match_count = if live_only { clamped_live } else { all.len() };
        let take = (limit.max(0) as usize).min(match_count);
        Slice {
            rows: all[..take].to_vec(),
            match_count,
            live_count: clamped_live,
            total_count: all.len(),
        }
    }

    /// 라이브 우선 → 시청자 많은 순 → 채널명(로캘) → channelId 전순서 정렬.
    pub fn ordered_following(channels: &[LiveChannelItem]) -> (Vec<LiveChannelItem>, usize) {
        let mut ordered = channels.to_vec();
        ordered.sort_by(|l, r| {
            r.is_live
                .cmp(&l.is_live)
                .then_with(|| r.viewer_count.cmp(&l.viewer_count))
                .then_with(|| localized_standard_compare(&l.channel_name, &r.channel_name))
                .then_with(|| l.channel_id.cmp(&r.channel_id))
        });
        let live = ordered.iter().filter(|c| c.is_live).count();
        (ordered, live)
    }

    pub fn prepare(
        source: &[LiveChannelItem],
        previous: Option<&PreparedSource>,
    ) -> PreparedSource {
        if let Some(prev) = previous {
            if prev.source == source {
                return prev.clone();
            }
        }
        let (ordered, live_count) = Self::ordered_following(source);
        let mut counts: HashMap<String, usize> = HashMap::new();
        for c in source.iter().filter(|c| c.is_live) {
            if let Some(cat) = &c.category_name {
                *counts.entry(cat.clone()).or_default() += 1;
            }
        }
        let mut categories: Vec<(String, usize)> = counts.into_iter().collect();
        categories.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        PreparedSource {
            source: source.to_vec(),
            ordered,
            live_count,
            categories,
        }
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/FollowingOverviewListQueryTests.swift`
    use super::*;

    fn item(id: &str, name: &str, live: bool, viewers: i64) -> LiveChannelItem {
        LiveChannelItem {
            id: id.into(),
            channel_name: name.into(),
            channel_image_url: None,
            live_title: String::new(),
            viewer_count: viewers,
            category_name: live.then(|| "Cat".to_string()),
            category_type: None,
            thumbnail_url: None,
            channel_id: id.into(),
            is_live: live,
            open_date: None,
            verified_badge: None,
        }
    }

    #[test]
    fn slice_caps_and_live_only_lowers_bound() {
        let all = vec![
            item("a", "a", true, 3),
            item("b", "b", true, 2),
            item("c", "c", false, 0),
        ];
        let s = FollowingOverviewListQuery::slice(&all, 2, true, 8);
        assert_eq!(s.rows.len(), 2);
        assert_eq!(s.match_count, 2);
        assert_eq!(s.total_count, 3);
        let s = FollowingOverviewListQuery::slice(&all, 2, false, 1);
        assert_eq!(s.rows.len(), 1);
        assert_eq!(s.remaining(), 2);
        let s = FollowingOverviewListQuery::slice(&all, 99, true, -1);
        assert_eq!(s.live_count, 3);
        assert!(s.rows.is_empty());
    }

    #[test]
    fn ordering_is_total_and_stable_for_offline_ties() {
        let input = vec![
            item("z", "ㄴ채널", false, 0),
            item("y", "ㄱ채널", false, 0),
            item("x", "ㄱ채널", false, 0),
            item("w", "live", true, 5),
        ];
        let (ordered, live) = FollowingOverviewListQuery::ordered_following(&input);
        let ids: Vec<&str> = ordered.iter().map(|c| c.channel_id.as_str()).collect();
        assert_eq!(ids, vec!["w", "x", "y", "z"]);
        assert_eq!(live, 1);
        let mut reversed = input.clone();
        reversed.reverse();
        assert_eq!(
            FollowingOverviewListQuery::ordered_following(&reversed).0,
            ordered
        );
    }

    #[test]
    fn prepare_reuses_previous_for_same_source() {
        let src = vec![item("a", "a", true, 1), item("b", "b", true, 2)];
        let p = FollowingOverviewListQuery::prepare(&src, None);
        assert_eq!(p.categories, vec![("Cat".to_string(), 2)]);
        let again = FollowingOverviewListQuery::prepare(&src, Some(&p));
        assert_eq!(again, p);
    }
}
