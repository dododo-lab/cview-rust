//! `Sources/CViewApp/Logic/FollowingChannelSidebarCache.swift` 포팅
//! (+ `Views/Following/ChannelSidebarSortOrder.swift`, `FollowingChannelSidebar.filteredChannels`,
//! `FollowingChannelDock.rankedChannels` 의 순수 부분).

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use cview_core::models::LiveChannelItem;

use crate::services::support::localized_standard_compare;

/// 표시 순서만 — 재생 세션이나 시청 큐를 재정렬하지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ChannelSidebarSortOrder {
    #[default]
    Recommended,
    ViewersDescending,
    ViewersAscending,
    Name,
    RecentlyStarted,
}

impl ChannelSidebarSortOrder {
    pub const DEFAULTS_KEY: &'static str = "following.channelSidebar.sortOrder";
    pub const ALL: [Self; 5] = [
        Self::Recommended,
        Self::ViewersDescending,
        Self::ViewersAscending,
        Self::Name,
        Self::RecentlyStarted,
    ];

    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Recommended => "recommended",
            Self::ViewersDescending => "viewersDescending",
            Self::ViewersAscending => "viewersAscending",
            Self::Name => "name",
            Self::RecentlyStarted => "recentlyStarted",
        }
    }

    /// 저장값 복원 — 모르는 값은 추천순.
    pub fn saved(raw: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|s| s.raw_value() == raw)
            .unwrap_or(Self::Recommended)
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Recommended => "추천순",
            Self::ViewersDescending => "시청자 많은 순",
            Self::ViewersAscending => "시청자 적은 순",
            Self::Name => "채널 이름순",
            Self::RecentlyStarted => "최근 방송 시작순",
        }
    }

    pub fn short_title(self) -> &'static str {
        match self {
            Self::ViewersDescending => "시청자 ↓",
            Self::ViewersAscending => "시청자 ↑",
            Self::RecentlyStarted => "최근 시작순",
            other => other.title(),
        }
    }

    pub fn apply(self, ranked: &[LiveChannelItem]) -> Vec<LiveChannelItem> {
        let mut out = ranked.to_vec();
        if self == Self::Recommended {
            return out;
        }
        let started = |c: &LiveChannelItem| c.open_date.unwrap_or(DateTime::<Utc>::MIN_UTC);
        out.sort_by(|l, r| {
            let primary = match self {
                Self::ViewersDescending => r.viewer_count.cmp(&l.viewer_count),
                Self::ViewersAscending => l.viewer_count.cmp(&r.viewer_count),
                Self::RecentlyStarted => started(r).cmp(&started(l)),
                _ => std::cmp::Ordering::Equal,
            };
            primary
                .then_with(|| localized_standard_compare(&l.channel_name, &r.channel_name))
                .then_with(|| l.channel_id.cmp(&r.channel_id))
        });
        out
    }
}

/// 채널 독/사이드바의 데이터 입력(`FollowingChannelDock` 의 값 부분).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FollowingChannelDock {
    pub live_channels: Vec<LiveChannelItem>,
    pub favorite_ids: HashSet<String>,
    pub active_channel_ids: HashSet<String>,
    /// 멀티 모드는 이미 담은 채널을 뒤로 미룬다. 시청 모드는 미루지 않는다.
    pub demotes_active_channels: bool,
}

impl FollowingChannelDock {
    /// 즐겨찾기 라이브 → 일반 라이브 → (멀티 모드면) 이미 열린 채널.
    pub fn ranked_channels(&self) -> Vec<LiveChannelItem> {
        let mut favorites = Vec::new();
        let mut others = Vec::new();
        let mut actives = Vec::new();
        for c in &self.live_channels {
            if self.demotes_active_channels && self.active_channel_ids.contains(&c.channel_id) {
                actives.push(c.clone());
            } else if self.favorite_ids.contains(&c.channel_id) {
                favorites.push(c.clone());
            } else {
                others.push(c.clone());
            }
        }
        favorites.extend(others);
        favorites.extend(actives);
        favorites
    }

    /// `FollowingChannelSidebar.filteredChannels`.
    pub fn filtered_channels(
        &self,
        query: &str,
        favorites_only: bool,
        sort_order: ChannelSidebarSortOrder,
    ) -> Vec<LiveChannelItem> {
        let source = if sort_order == ChannelSidebarSortOrder::Recommended {
            self.ranked_channels()
        } else {
            self.live_channels.clone()
        };
        let filtered: Vec<LiveChannelItem> = source
            .into_iter()
            .filter(|c| {
                (!favorites_only || self.favorite_ids.contains(&c.channel_id))
                    && c.matches_search(query, false)
            })
            .collect();
        sort_order.apply(&filtered)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Input {
    channels: Vec<LiveChannelItem>,
    favorites: HashSet<String>,
    active: HashSet<String>,
    demotes_active: bool,
    query: String,
    favorites_only: bool,
    sort_order: ChannelSidebarSortOrder,
}

/// 마운트된 피커당 엔트리 하나 — 호버·포커스는 검색·정렬을 다시 돌리지 않는다.
#[derive(Debug, Default)]
pub struct FollowingChannelSidebarCache {
    entry: Option<(Input, Vec<LiveChannelItem>)>,
    computation_count: usize,
}

impl FollowingChannelSidebarCache {
    pub fn computation_count(&self) -> usize {
        self.computation_count
    }

    pub fn channels(
        &mut self,
        dock: &FollowingChannelDock,
        query: &str,
        favorites_only: bool,
        sort_order: ChannelSidebarSortOrder,
    ) -> Vec<LiveChannelItem> {
        let query = query.trim().to_string();
        let input = Input {
            channels: dock.live_channels.clone(),
            favorites: dock.favorite_ids.clone(),
            active: if dock.demotes_active_channels {
                dock.active_channel_ids.clone()
            } else {
                HashSet::new()
            },
            demotes_active: dock.demotes_active_channels,
            query: query.clone(),
            favorites_only,
            sort_order,
        };
        if let Some((cached, channels)) = &self.entry {
            if *cached == input {
                return channels.clone();
            }
        }
        let channels = dock.filtered_channels(&query, favorites_only, sort_order);
        self.entry = Some((input, channels.clone()));
        self.computation_count += 1;
        channels
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/FollowingChannelSidebarTests.swift`·`ChannelSidebarRefreshTests.swift` 핵심.
    use super::*;

    fn item(id: &str, viewers: i64) -> LiveChannelItem {
        LiveChannelItem {
            id: id.into(),
            channel_name: id.into(),
            channel_image_url: None,
            live_title: String::new(),
            viewer_count: viewers,
            category_name: None,
            category_type: None,
            thumbnail_url: None,
            channel_id: id.into(),
            is_live: true,
            open_date: None,
            verified_badge: None,
        }
    }

    fn dock(demote: bool) -> FollowingChannelDock {
        FollowingChannelDock {
            live_channels: vec![item("a", 1), item("b", 3), item("c", 2)],
            favorite_ids: ["c".to_string()].into(),
            active_channel_ids: ["a".to_string()].into(),
            demotes_active_channels: demote,
        }
    }

    fn ids(v: &[LiveChannelItem]) -> Vec<&str> {
        v.iter().map(|c| c.channel_id.as_str()).collect()
    }

    #[test]
    fn ranking_demotes_active_only_in_multi() {
        assert_eq!(ids(&dock(true).ranked_channels()), vec!["c", "b", "a"]);
        assert_eq!(ids(&dock(false).ranked_channels()), vec!["c", "a", "b"]);
    }

    #[test]
    fn sort_orders() {
        let d = dock(true);
        assert_eq!(
            ids(&d.filtered_channels("", false, ChannelSidebarSortOrder::ViewersDescending)),
            vec!["b", "c", "a"]
        );
        assert_eq!(
            ids(&d.filtered_channels("", true, ChannelSidebarSortOrder::Name)),
            vec!["c"]
        );
        assert_eq!(
            ChannelSidebarSortOrder::saved("bogus"),
            ChannelSidebarSortOrder::Recommended
        );
    }

    #[test]
    fn cache_skips_recompute_for_same_input() {
        let mut cache = FollowingChannelSidebarCache::default();
        let d = dock(true);
        cache.channels(&d, " ", false, ChannelSidebarSortOrder::Recommended);
        cache.channels(&d, "", false, ChannelSidebarSortOrder::Recommended);
        assert_eq!(cache.computation_count(), 1);
        let mut changed = d.clone();
        changed.live_channels[0].viewer_count = 99;
        cache.channels(&changed, "", false, ChannelSidebarSortOrder::Recommended);
        assert_eq!(cache.computation_count(), 2, "메타데이터 변경은 무효화한다");
    }
}
