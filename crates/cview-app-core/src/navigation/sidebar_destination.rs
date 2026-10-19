//! `Sources/CViewApp/Navigation/SidebarDestination.swift` 포팅.
//!
//! 좌측 사이드바의 행 = "어느 화면으로 가는가". 라이브·시청·멀티라이브 세 행은 라우트가
//! `.following` 하나이고 사이드바가 그 화면의 모드를 직접 고른다. `ExploreMenuNavigationMotion`
//! 의 SwiftUI `Transaction` 조작은 판정 함수(`is_explore_switch`)만 옮겼다.

use std::sync::OnceLock;

use super::hub_mode::FollowingHubMode;
use super::router::SidebarItem;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SidebarDestination {
    /// 독립 라우트(홈·멀티채팅·검색·인기·카테고리·라이브러리·설정).
    Item(SidebarItem),
    /// 상주 라이브 화면의 모드(개요·시청·멀티).
    Live(FollowingHubMode),
}

impl SidebarDestination {
    pub fn id(&self) -> String {
        match self {
            Self::Item(item) => format!("item:{}", item.raw_value()),
            Self::Live(mode) => format!("live:{}", mode.raw_value()),
        }
    }

    /// 첫 묶음 — 홈과 라이브 계열(영상·채팅).
    pub fn primary() -> &'static [SidebarDestination] {
        static PRIMARY: OnceLock<Vec<SidebarDestination>> = OnceLock::new();
        PRIMARY.get_or_init(|| {
            let mut rows = vec![Self::Item(SidebarItem::Home)];
            rows.extend(
                FollowingHubMode::TOP_LEVEL_MODES
                    .iter()
                    .map(|m| Self::Live(*m)),
            );
            rows.push(Self::Item(SidebarItem::MultiChat));
            rows
        })
    }

    /// "탐색" 묶음 — 라이브 밖에서 열리는 발견 화면과 라이브러리.
    pub const DISCOVER: [SidebarDestination; 4] = [
        Self::Item(SidebarItem::Search),
        Self::Item(SidebarItem::AllBroadcasts),
        Self::Item(SidebarItem::Category),
        Self::Item(SidebarItem::Library),
    ];

    /// 현재 라우트·라이브 모드가 가리키는 사이드바 행.
    pub fn current(route: SidebarItem, hub_mode: FollowingHubMode) -> Self {
        if route != SidebarItem::Following {
            return Self::Item(route);
        }
        Self::Live(if hub_mode.is_live_workspace_mode() {
            hub_mode
        } else {
            FollowingHubMode::Explore
        })
    }

    /// 이 행이 속한 라우트.
    pub fn route(&self) -> SidebarItem {
        match self {
            Self::Item(item) => *item,
            Self::Live(_) => SidebarItem::Following,
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Self::Item(item) => item.title(),
            Self::Live(FollowingHubMode::Explore) => SidebarItem::Following.title(),
            Self::Live(FollowingHubMode::Watch) => "시청",
            Self::Live(FollowingHubMode::Multi) => "멀티라이브",
            Self::Live(mode) => mode.display_name(),
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Item(item) => item.icon(),
            Self::Live(FollowingHubMode::Explore) => SidebarItem::Following.icon(),
            Self::Live(FollowingHubMode::Watch) => "play.rectangle.fill",
            Self::Live(FollowingHubMode::Multi) => "square.grid.2x2.fill",
            Self::Live(mode) => mode.icon(),
        }
    }

    pub fn accent(&self) -> &'static str {
        match self {
            Self::Item(item) => item.accent(),
            Self::Live(FollowingHubMode::Watch) => "indigo",
            Self::Live(FollowingHubMode::Multi) => "cyan",
            Self::Live(_) => SidebarItem::Following.accent(),
        }
    }

    pub fn subtitle(&self) -> &'static str {
        match self {
            Self::Item(item) => item.subtitle(),
            Self::Live(FollowingHubMode::Watch) => "선택한 채널에 집중 · 채널 독 · 채팅",
            Self::Live(FollowingHubMode::Multi) => "여러 라이브를 그리드로 함께 보기 · 멀티채팅",
            Self::Live(_) => SidebarItem::Following.subtitle(),
        }
    }
}

/// 사이드바 '탐색' 네 메뉴 간 이동은 본문과 헤더를 즉시 교체한다(애니메이션 없음).
pub fn is_explore_switch(previous: Option<SidebarItem>, current: SidebarItem) -> bool {
    let Some(previous) = previous else {
        return false;
    };
    if previous == current {
        return false;
    }
    SidebarDestination::DISCOVER.contains(&SidebarDestination::Item(previous))
        && SidebarDestination::DISCOVER.contains(&SidebarDestination::Item(current))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_rows_are_home_live_modes_multichat() {
        let ids: Vec<String> = SidebarDestination::primary()
            .iter()
            .map(|d| d.id())
            .collect();
        assert_eq!(
            ids,
            vec![
                "item:홈",
                "live:탐색",
                "live:시청",
                "live:멀티",
                "item:멀티채팅"
            ]
        );
    }

    #[test]
    fn current_normalizes_discovery_modes_on_live_route() {
        assert_eq!(
            SidebarDestination::current(SidebarItem::Following, FollowingHubMode::Search),
            SidebarDestination::Live(FollowingHubMode::Explore)
        );
        assert_eq!(
            SidebarDestination::current(SidebarItem::Category, FollowingHubMode::Watch),
            SidebarDestination::Item(SidebarItem::Category)
        );
    }

    #[test]
    fn titles() {
        assert_eq!(
            SidebarDestination::Live(FollowingHubMode::Explore).title(),
            "팔로잉"
        );
        assert_eq!(
            SidebarDestination::Live(FollowingHubMode::Multi).title(),
            "멀티라이브"
        );
    }

    #[test]
    fn explore_switch_only_between_discover_items() {
        assert!(is_explore_switch(
            Some(SidebarItem::Search),
            SidebarItem::Category
        ));
        assert!(!is_explore_switch(
            Some(SidebarItem::Home),
            SidebarItem::Category
        ));
        assert!(!is_explore_switch(None, SidebarItem::Category));
        assert!(!is_explore_switch(
            Some(SidebarItem::Search),
            SidebarItem::Search
        ));
    }
}
