//! `Sources/CViewApp/Logic/FollowingBrowserQuery.swift` 포팅.
//!
//! 개요 탭 팔로잉 브라우저의 필터·검색·카테고리·정렬·페이징 계약. ⚠ 페이징은 팔로잉 31위 밖에
//! 도달하는 유일한 경로다 — 지우지 말 것.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use cview_core::models::LiveChannelItem;

use crate::navigation::hub_mode::{FollowingBrowserFilter, FollowingBrowserSortOrder};
use crate::services::support::localized_standard_compare;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryOption {
    pub name: String,
    pub count: usize,
}

/// 브라우저 한 번의 렌더 평가가 공유하는 값 스냅샷.
#[derive(Debug, Clone, PartialEq)]
pub struct ExpandedSnapshot {
    pub results: Vec<LiveChannelItem>,
    pub page: Vec<LiveChannelItem>,
    pub total_pages: usize,
    pub page_index: usize,
    pub filter_counts: HashMap<FollowingBrowserFilter, usize>,
    pub category_options: Vec<CategoryOption>,
}

impl ExpandedSnapshot {
    pub fn result_count(&self) -> usize {
        self.results.len()
    }

    pub fn count(&self, filter: FollowingBrowserFilter) -> usize {
        self.filter_counts.get(&filter).copied().unwrap_or(0)
    }
}

/// 질의 입력.
#[derive(Debug, Clone, Copy)]
pub struct BrowserQuery<'a> {
    pub live_channels: &'a [LiveChannelItem],
    pub offline_channels: &'a [LiveChannelItem],
    pub filter: FollowingBrowserFilter,
    pub favorite_ids: &'a HashSet<String>,
    pub recent_ids: &'a [String],
    pub search_text: &'a str,
    pub category: Option<&'a str>,
    pub sort_order: FollowingBrowserSortOrder,
}

pub struct FollowingBrowserQuery;

impl FollowingBrowserQuery {
    pub fn expanded_snapshot(
        q: BrowserQuery<'_>,
        page_index: i64,
        page_size: i64,
    ) -> ExpandedSnapshot {
        let results = Self::results(q);
        let safe_page_size = page_size.max(1) as usize;
        let total_pages = results.len().div_ceil(safe_page_size).max(1);
        let safe_page_index = (page_index.max(0) as usize).min(total_pages - 1);
        let start = safe_page_index * safe_page_size;
        let end = (start + safe_page_size).min(results.len());
        let page = if start < end {
            results[start..end].to_vec()
        } else {
            Vec::new()
        };
        ExpandedSnapshot {
            filter_counts: Self::counts(
                q.live_channels,
                q.offline_channels,
                q.favorite_ids,
                q.recent_ids,
            ),
            category_options: Self::categories(q),
            results,
            page,
            total_pages,
            page_index: safe_page_index,
        }
    }

    /// 일괄 담기는 현재 페이지 순서를 유지하고 중복·오프라인·활성 세션을 제외한다.
    pub fn queue_candidates(
        channels: &[LiveChannelItem],
        active_ids: &HashSet<String>,
        queued_ids: &HashSet<String>,
    ) -> Vec<LiveChannelItem> {
        let mut seen: HashSet<String> = active_ids.union(queued_ids).cloned().collect();
        channels
            .iter()
            .filter(|c| c.is_live && seen.insert(c.channel_id.clone()))
            .cloned()
            .collect()
    }

    pub fn results(q: BrowserQuery<'_>) -> Vec<LiveChannelItem> {
        let mut channels = Self::base_channels(q);
        if let Some(category) = q.category {
            channels.retain(|c| {
                LiveChannelItem::display_category_name(c.category_name.as_deref()) == category
            });
        }
        let query = q.search_text.trim();
        if !query.is_empty() {
            channels.retain(|c| c.matches_search(query, true));
        }
        // 최근 필터의 기본 추천순은 시청 순서를 보존한다.
        if q.filter == FollowingBrowserFilter::Recent
            && q.sort_order == FollowingBrowserSortOrder::Recommended
        {
            return channels;
        }
        Self::sorted(channels, q.sort_order, q.favorite_ids)
    }

    pub fn count(
        filter: FollowingBrowserFilter,
        live: &[LiveChannelItem],
        offline: &[LiveChannelItem],
        favorite_ids: &HashSet<String>,
        recent_ids: &[String],
    ) -> usize {
        Self::counts(live, offline, favorite_ids, recent_ids)
            .get(&filter)
            .copied()
            .unwrap_or(0)
    }

    /// 네 필터 배지를 한 번의 채널 순회로 집계한다.
    pub fn counts(
        live: &[LiveChannelItem],
        offline: &[LiveChannelItem],
        favorite_ids: &HashSet<String>,
        recent_ids: &[String],
    ) -> HashMap<FollowingBrowserFilter, usize> {
        let mut favorite_count = 0;
        let mut available: HashSet<&str> = HashSet::with_capacity(live.len() + offline.len());
        for c in live.iter().chain(offline) {
            available.insert(&c.channel_id);
            if favorite_ids.contains(&c.channel_id) {
                favorite_count += 1;
            }
        }
        let mut seen: HashSet<&str> = HashSet::new();
        let recent_count = recent_ids
            .iter()
            .filter(|id| seen.insert(id.as_str()) && available.contains(id.as_str()))
            .count();
        HashMap::from([
            (FollowingBrowserFilter::All, live.len() + offline.len()),
            (FollowingBrowserFilter::Live, live.len()),
            (FollowingBrowserFilter::Favorites, favorite_count),
            (FollowingBrowserFilter::Recent, recent_count),
        ])
    }

    pub fn categories(q: BrowserQuery<'_>) -> Vec<CategoryOption> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for c in Self::base_channels(q) {
            *counts
                .entry(LiveChannelItem::display_category_name(
                    c.category_name.as_deref(),
                ))
                .or_default() += 1;
        }
        let mut options: Vec<CategoryOption> = counts
            .into_iter()
            .map(|(name, count)| CategoryOption { name, count })
            .collect();
        options.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then_with(|| localized_standard_compare(&a.name, &b.name))
        });
        options
    }

    fn base_channels(q: BrowserQuery<'_>) -> Vec<LiveChannelItem> {
        let all = || q.live_channels.iter().chain(q.offline_channels);
        match q.filter {
            FollowingBrowserFilter::All => all().cloned().collect(),
            FollowingBrowserFilter::Live => q.live_channels.to_vec(),
            FollowingBrowserFilter::Favorites => all()
                .filter(|c| q.favorite_ids.contains(&c.channel_id))
                .cloned()
                .collect(),
            FollowingBrowserFilter::Recent => {
                let mut by_id: HashMap<&str, &LiveChannelItem> = HashMap::new();
                for c in all() {
                    by_id.insert(&c.channel_id, c);
                }
                let mut seen: HashSet<&str> = HashSet::new();
                q.recent_ids
                    .iter()
                    .filter(|id| seen.insert(id.as_str()))
                    .filter_map(|id| by_id.get(id.as_str()).map(|c| (*c).clone()))
                    .collect()
            }
        }
    }

    fn sorted(
        mut channels: Vec<LiveChannelItem>,
        order: FollowingBrowserSortOrder,
        favorite_ids: &HashSet<String>,
    ) -> Vec<LiveChannelItem> {
        channels.sort_by(|l, r| {
            let primary = match order {
                FollowingBrowserSortOrder::Recommended => r
                    .is_live
                    .cmp(&l.is_live)
                    .then_with(|| {
                        favorite_ids
                            .contains(&r.channel_id)
                            .cmp(&favorite_ids.contains(&l.channel_id))
                    })
                    .then_with(|| r.viewer_count.cmp(&l.viewer_count)),
                FollowingBrowserSortOrder::Viewers => r
                    .is_live
                    .cmp(&l.is_live)
                    .then_with(|| r.viewer_count.cmp(&l.viewer_count)),
                FollowingBrowserSortOrder::Name => Ordering::Equal,
            };
            primary
                .then_with(|| localized_standard_compare(&l.channel_name, &r.channel_name))
                .then_with(|| l.channel_id.cmp(&r.channel_id))
        });
        channels
    }
}

/// 라이브 작업영역당 결과 하나를 유지한다(`FollowingBrowserSnapshotCache`).
#[derive(Debug, Clone, PartialEq)]
pub struct BrowserSnapshotInput {
    pub all_following: Vec<LiveChannelItem>,
    pub live_count: i64,
    pub filter: FollowingBrowserFilter,
    pub favorite_ids: HashSet<String>,
    pub recent_ids: Vec<String>,
    pub search_text: String,
    pub category: Option<String>,
    pub sort_order: FollowingBrowserSortOrder,
    pub page_index: i64,
    pub page_size: i64,
}

#[derive(Debug, Default)]
pub struct FollowingBrowserSnapshotCache {
    entry: Option<(BrowserSnapshotInput, ExpandedSnapshot)>,
}

impl FollowingBrowserSnapshotCache {
    pub fn snapshot(&mut self, input: &BrowserSnapshotInput) -> ExpandedSnapshot {
        if let Some((cached_input, snap)) = &self.entry {
            if cached_input == input {
                return snap.clone();
            }
        }
        let count = (input.live_count.max(0) as usize).min(input.all_following.len());
        let (live, offline) = input.all_following.split_at(count);
        let snapshot = FollowingBrowserQuery::expanded_snapshot(
            BrowserQuery {
                live_channels: live,
                offline_channels: offline,
                filter: input.filter,
                favorite_ids: &input.favorite_ids,
                recent_ids: &input.recent_ids,
                search_text: &input.search_text,
                category: input.category.as_deref(),
                sort_order: input.sort_order,
            },
            input.page_index,
            input.page_size,
        );
        self.entry = Some((input.clone(), snapshot.clone()));
        snapshot
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/FollowingBrowserQueryTests.swift` 핵심 계약.
    use super::*;

    pub(crate) fn item(
        id: &str,
        name: &str,
        live: bool,
        viewers: i64,
        cat: Option<&str>,
    ) -> LiveChannelItem {
        LiveChannelItem {
            id: id.into(),
            channel_name: name.into(),
            channel_image_url: None,
            live_title: format!("{name} 방송"),
            viewer_count: viewers,
            category_name: cat.map(str::to_string),
            category_type: None,
            thumbnail_url: None,
            channel_id: id.into(),
            is_live: live,
            open_date: None,
            verified_badge: None,
        }
    }

    fn q<'a>(
        live: &'a [LiveChannelItem],
        off: &'a [LiveChannelItem],
        filter: FollowingBrowserFilter,
        fav: &'a HashSet<String>,
        recent: &'a [String],
        sort: FollowingBrowserSortOrder,
    ) -> BrowserQuery<'a> {
        BrowserQuery {
            live_channels: live,
            offline_channels: off,
            filter,
            favorite_ids: fav,
            recent_ids: recent,
            search_text: "",
            category: None,
            sort_order: sort,
        }
    }

    #[test]
    fn recommended_sort_live_then_favorite_then_viewers_then_name() {
        let live = vec![
            item("a", "Alpha", true, 10, None),
            item("b", "Beta", true, 50, None),
            item("c", "Ccc", true, 10, None),
        ];
        let off = vec![item("z", "Zed", false, 0, None)];
        let fav: HashSet<String> = ["c".to_string()].into();
        let r = FollowingBrowserQuery::results(q(
            &live,
            &off,
            FollowingBrowserFilter::All,
            &fav,
            &[],
            FollowingBrowserSortOrder::Recommended,
        ));
        let ids: Vec<&str> = r.iter().map(|c| c.channel_id.as_str()).collect();
        assert_eq!(ids, vec!["c", "b", "a", "z"]);
    }

    #[test]
    fn recent_filter_keeps_watch_order_and_dedupes() {
        let live = vec![item("a", "A", true, 1, None), item("b", "B", true, 9, None)];
        let off = vec![item("c", "C", false, 0, None)];
        let recent: Vec<String> = ["c", "a", "c", "missing"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let fav = HashSet::new();
        let r = FollowingBrowserQuery::results(q(
            &live,
            &off,
            FollowingBrowserFilter::Recent,
            &fav,
            &recent,
            FollowingBrowserSortOrder::Recommended,
        ));
        let ids: Vec<&str> = r.iter().map(|c| c.channel_id.as_str()).collect();
        assert_eq!(ids, vec!["c", "a"]);
        let counts = FollowingBrowserQuery::counts(&live, &off, &fav, &recent);
        assert_eq!(counts[&FollowingBrowserFilter::Recent], 2);
        assert_eq!(counts[&FollowingBrowserFilter::All], 3);
        assert_eq!(counts[&FollowingBrowserFilter::Live], 2);
    }

    #[test]
    fn paging_clamps_index_and_size() {
        let live: Vec<LiveChannelItem> = (0..25)
            .map(|i| {
                item(
                    &format!("c{i:02}"),
                    &format!("n{i:02}"),
                    true,
                    100 - i,
                    None,
                )
            })
            .collect();
        let fav = HashSet::new();
        let snap = FollowingBrowserQuery::expanded_snapshot(
            q(
                &live,
                &[],
                FollowingBrowserFilter::All,
                &fav,
                &[],
                FollowingBrowserSortOrder::Viewers,
            ),
            99,
            10,
        );
        assert_eq!(snap.total_pages, 3);
        assert_eq!(snap.page_index, 2);
        assert_eq!(snap.page.len(), 5);
        let snap = FollowingBrowserQuery::expanded_snapshot(
            q(
                &[],
                &[],
                FollowingBrowserFilter::All,
                &fav,
                &[],
                FollowingBrowserSortOrder::Viewers,
            ),
            -3,
            0,
        );
        assert_eq!(snap.total_pages, 1);
        assert_eq!(snap.page_index, 0);
        assert!(snap.page.is_empty());
    }

    #[test]
    fn categories_and_search() {
        let live = vec![
            item("a", "A", true, 1, Some("LoL")),
            item("b", "B", true, 1, Some("LoL")),
            item("c", "C", true, 1, None),
        ];
        let fav = HashSet::new();
        let mut query = q(
            &live,
            &[],
            FollowingBrowserFilter::All,
            &fav,
            &[],
            FollowingBrowserSortOrder::Name,
        );
        let cats = FollowingBrowserQuery::categories(query);
        assert_eq!(
            cats[0],
            CategoryOption {
                name: "LoL".into(),
                count: 2
            }
        );
        assert_eq!(cats[1].name, "분류 없음");
        query.category = Some("분류 없음");
        assert_eq!(FollowingBrowserQuery::results(query).len(), 1);
        query.category = None;
        query.search_text = "  b 방송 ";
        assert_eq!(FollowingBrowserQuery::results(query)[0].channel_id, "b");
    }

    #[test]
    fn queue_candidates_skip_offline_active_and_duplicates() {
        let chans = vec![
            item("a", "A", true, 1, None),
            item("b", "B", false, 0, None),
            item("a", "A", true, 1, None),
            item("c", "C", true, 1, None),
            item("d", "D", true, 1, None),
        ];
        let active: HashSet<String> = ["c".to_string()].into();
        let queued: HashSet<String> = HashSet::new();
        let r = FollowingBrowserQuery::queue_candidates(&chans, &active, &queued);
        let ids: Vec<&str> = r.iter().map(|c| c.channel_id.as_str()).collect();
        assert_eq!(ids, vec!["a", "d"]);
    }

    #[test]
    fn snapshot_cache_reuses_equal_input() {
        let mut cache = FollowingBrowserSnapshotCache::default();
        let input = BrowserSnapshotInput {
            all_following: vec![
                item("a", "A", true, 1, None),
                item("b", "B", false, 0, None),
            ],
            live_count: 1,
            filter: FollowingBrowserFilter::Live,
            favorite_ids: HashSet::new(),
            recent_ids: vec![],
            search_text: String::new(),
            category: None,
            sort_order: FollowingBrowserSortOrder::Recommended,
            page_index: 0,
            page_size: 10,
        };
        let first = cache.snapshot(&input);
        assert_eq!(first.results.len(), 1);
        assert_eq!(cache.snapshot(&input), first);
    }
}
