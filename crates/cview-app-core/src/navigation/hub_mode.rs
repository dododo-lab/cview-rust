//! 라이브 허브 모드와 팔로잉 브라우저 필터·정렬.
//!
//! 원본: `Sources/CViewApp/ViewModels/FollowingViewState.swift` 의 `FollowingHubMode`,
//! `FollowingBrowserSortOrder`, `FollowingBrowserFilter` 와
//! `Views/Search/LiveDiscoveryWorkspace.swift` 의 `LiveDiscoverySection`,
//! `Navigation/SidebarDestination.swift` 의 `FollowingHubMode` 확장.
//!
//! 내비게이션·액션·딥링크가 모두 이 값을 쓰므로 하위 계층인 `navigation` 에 둔다 —
//! ViewModel 포팅은 이 타입을 재사용한다.

use serde::{Deserialize, Serialize};

use super::router::SidebarItem;
use super::sidebar_destination::SidebarDestination;

/// 라이브 메뉴(상주 라이브 화면)의 모드. ⚠ rawValue 는 영속 키라 바꾸지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FollowingHubMode {
    #[serde(rename = "탐색")]
    Explore,
    #[serde(rename = "시청")]
    Watch,
    #[serde(rename = "멀티")]
    Multi,
    #[serde(rename = "검색")]
    Search,
    #[serde(rename = "인기")]
    Popular,
    #[serde(rename = "카테고리")]
    Category,
}

impl FollowingHubMode {
    pub const ALL: [Self; 6] = [
        Self::Explore,
        Self::Watch,
        Self::Multi,
        Self::Search,
        Self::Popular,
        Self::Category,
    ];

    /// 상주 라이브 화면이 전환하는 모드 — 사이드바의 라이브·시청·멀티라이브 행.
    pub const TOP_LEVEL_MODES: [Self; 3] = [Self::Explore, Self::Watch, Self::Multi];

    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Explore => "탐색",
            Self::Watch => "시청",
            Self::Multi => "멀티",
            Self::Search => "검색",
            Self::Popular => "인기",
            Self::Category => "카테고리",
        }
    }

    pub fn from_raw(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.raw_value() == raw)
    }

    pub fn discovery_section(self) -> Option<LiveDiscoverySection> {
        match self {
            Self::Search => Some(LiveDiscoverySection::Search),
            Self::Popular => Some(LiveDiscoverySection::Popular),
            Self::Category => Some(LiveDiscoverySection::Category),
            _ => None,
        }
    }

    /// URL/단축어에서 쓰는 **영문** 토큰.
    pub fn deep_link_token(self) -> &'static str {
        match self {
            Self::Explore => "explore",
            Self::Watch => "watch",
            Self::Multi => "multi",
            Self::Search => "search",
            Self::Popular => "popular",
            Self::Category => "category",
        }
    }

    /// URL 토큰에서 복원. 영문 토큰(대소문자 무시)과 한글 rawValue 를 둘 다 받는다.
    pub fn from_deep_link_token(token: &str) -> Option<Self> {
        let lowered = token.to_lowercase();
        if let Some(m) = Self::ALL
            .into_iter()
            .find(|m| m.deep_link_token() == lowered)
        {
            return Some(m);
        }
        Self::from_raw(token)
    }

    /// 스테이지 전체를 점유하는 모드인가.
    pub fn occupies_full_stage(self) -> bool {
        matches!(self, Self::Search | Self::Popular | Self::Category)
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Explore => "개요",
            Self::Watch => "시청",
            Self::Multi => "멀티",
            Self::Search => "검색",
            Self::Popular => "인기",
            Self::Category => "카테고리",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Explore => "safari",
            Self::Watch => "play.rectangle",
            Self::Multi => "square.grid.2x2",
            Self::Search => "magnifyingglass",
            Self::Popular => "dot.radiowaves.left.and.right",
            Self::Category => "rectangle.3.group.fill",
        }
    }

    /// 상주 라이브 화면(`FollowingView`) 안에서 전환되는 모드인가.
    pub fn is_live_workspace_mode(self) -> bool {
        self.discovery_section().is_none()
    }

    /// 이 모드가 가리키는 사이드바 목적지.
    pub fn sidebar_destination(self) -> SidebarDestination {
        match self {
            Self::Explore | Self::Watch | Self::Multi => SidebarDestination::Live(self),
            Self::Search => SidebarDestination::Item(SidebarItem::Search),
            Self::Popular => SidebarDestination::Item(SidebarItem::AllBroadcasts),
            Self::Category => SidebarDestination::Item(SidebarItem::Category),
        }
    }
}

/// 라이브 밖 발견 화면(검색·인기·카테고리).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LiveDiscoverySection {
    Search,
    Popular,
    Category,
}

impl LiveDiscoverySection {
    pub fn mode(self) -> FollowingHubMode {
        match self {
            Self::Search => FollowingHubMode::Search,
            Self::Popular => FollowingHubMode::Popular,
            Self::Category => FollowingHubMode::Category,
        }
    }

    /// 이 화면의 사이드바 라우트.
    pub fn route(self) -> SidebarItem {
        match self {
            Self::Search => SidebarItem::Search,
            Self::Popular => SidebarItem::AllBroadcasts,
            Self::Category => SidebarItem::Category,
        }
    }
}

/// 팔로잉 브라우저 정렬 기준. rawValue 는 저장 키다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FollowingBrowserSortOrder {
    #[default]
    Recommended,
    Viewers,
    Name,
}

impl FollowingBrowserSortOrder {
    pub const ALL: [Self; 3] = [Self::Recommended, Self::Viewers, Self::Name];

    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Recommended => "recommended",
            Self::Viewers => "viewers",
            Self::Name => "name",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Recommended => "추천순",
            Self::Viewers => "시청자순",
            Self::Name => "이름순",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Recommended => "sparkles",
            Self::Viewers => "person.2.fill",
            Self::Name => "textformat.abc",
        }
    }
}

/// 팔로잉 브라우저 필터(전체/라이브/즐겨찾기/최근). rawValue 는 저장 키다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FollowingBrowserFilter {
    #[default]
    All,
    Live,
    Favorites,
    Recent,
}

impl FollowingBrowserFilter {
    pub const ALL: [Self; 4] = [Self::All, Self::Live, Self::Favorites, Self::Recent];

    pub fn raw_value(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Live => "live",
            Self::Favorites => "favorites",
            Self::Recent => "recent",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::All => "전체",
            Self::Live => "라이브",
            Self::Favorites => "즐겨찾기",
            Self::Recent => "최근",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::All => "square.grid.2x2",
            Self::Live => "antenna.radiowaves.left.and.right",
            Self::Favorites => "star.fill",
            Self::Recent => "clock",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_link_tokens_round_trip_and_accept_korean_raw() {
        for mode in FollowingHubMode::ALL {
            assert_eq!(
                FollowingHubMode::from_deep_link_token(mode.deep_link_token()),
                Some(mode)
            );
            assert_eq!(
                FollowingHubMode::from_deep_link_token(mode.raw_value()),
                Some(mode)
            );
        }
        assert_eq!(
            FollowingHubMode::from_deep_link_token("MULTI"),
            Some(FollowingHubMode::Multi)
        );
        assert_eq!(FollowingHubMode::from_deep_link_token("nope"), None);
    }

    #[test]
    fn serde_uses_korean_raw_values() {
        let json = serde_json::to_string(&FollowingHubMode::Popular).unwrap();
        assert_eq!(json, "\"인기\"");
    }

    #[test]
    fn discovery_modes_are_not_live_workspace() {
        assert!(FollowingHubMode::Explore.is_live_workspace_mode());
        assert!(!FollowingHubMode::Category.is_live_workspace_mode());
        assert!(FollowingHubMode::Search.occupies_full_stage());
    }
}
