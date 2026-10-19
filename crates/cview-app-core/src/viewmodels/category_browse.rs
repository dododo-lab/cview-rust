//! 카테고리 화면 상태 — 그룹 계산 캐시와 그 무효화 키, 선택·필터.
//!
//! 원본: `Sources/CViewApp/ViewModels/CategoryBrowseViewModel.swift`.
//! 그룹·랭킹 값(`CategoryGroup`·`CategoryRanking`)은 Logic 계층(`CategoryGroupsCompute`)의 타입이라
//! 타입 매개변수로 받는다. `AccentColorCache` 는 색 대신 팔레트 인덱스를 돌려준다.

use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;

use cview_core::models::LiveChannelItem;
use cview_core::utils::stable_hash;
use cview_core::utils::time::TimeInterval;

use super::home::deps::LiveCatalogPort;

/// 유형 필터 칩.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeFilter {
    pub label: String,
    pub icon: String,
    pub value: Option<String>,
}

impl TypeFilter {
    pub fn all() -> Self {
        Self {
            label: "전체".into(),
            icon: "square.grid.2x2".into(),
            value: None,
        }
    }
}

/// 카테고리 색 팔레트 인덱스 캐시(팔레트 8색).
#[derive(Debug, Default, Clone)]
pub struct AccentColorCache {
    cache: HashMap<String, usize>,
}

impl AccentColorCache {
    pub const PALETTE_SIZE: usize = 8;

    pub fn index(&mut self, category: &str) -> usize {
        if let Some(i) = self.cache.get(category) {
            return *i;
        }
        let computed = stable_hash::index(category, Self::PALETTE_SIZE);
        self.cache.insert(category.to_string(), computed);
        computed
    }
}

pub struct CategoryBrowseViewModel<G, R> {
    pub group_cache: Vec<G>,
    pub pinned_cache: Vec<G>,
    pub unpinned_cache: Vec<G>,
    pub trend_ranking_cache: Vec<R>,
    pub available_type_filters_cache: Vec<TypeFilter>,
    pub by_category_cache: HashMap<String, G>,
    last_computed_group_key: Option<String>,
    groups_channels_revision: Option<u64>,
    pub content_width: f64,
    pub selected_category: Option<String>,
    pub selected_type_filter: Option<String>,
    pub split_selected_category: Option<String>,
    pub is_split_explorer_visible: bool,
    pub global_search_cache: Vec<LiveChannelItem>,
    pub personal_category_watch_time: HashMap<String, TimeInterval>,
    pub personal_categories_loaded: bool,
    pub catalog_store: Option<Arc<dyn LiveCatalogPort>>,
    pub accent_color_cache: AccentColorCache,
}

impl<G, R> Default for CategoryBrowseViewModel<G, R> {
    fn default() -> Self {
        Self {
            group_cache: Vec::new(),
            pinned_cache: Vec::new(),
            unpinned_cache: Vec::new(),
            trend_ranking_cache: Vec::new(),
            available_type_filters_cache: vec![TypeFilter::all()],
            by_category_cache: HashMap::new(),
            last_computed_group_key: None,
            groups_channels_revision: None,
            content_width: 900.0,
            selected_category: None,
            selected_type_filter: None,
            split_selected_category: None,
            is_split_explorer_visible: false,
            global_search_cache: Vec::new(),
            personal_category_watch_time: HashMap::new(),
            personal_categories_loaded: false,
            catalog_store: None,
            accent_color_cache: AccentColorCache::default(),
        }
    }
}

impl<G, R> CategoryBrowseViewModel<G, R> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn guard_key(route_key: &str, channels_revision: u64) -> String {
        format!("{route_key}|r{channels_revision}")
    }

    pub fn should_commit_groups(
        requested_route_key: &str,
        requested_channels_revision: u64,
        current_route_key: &str,
        current_channels_revision: u64,
        is_frontmost: bool,
        is_cancelled: bool,
    ) -> bool {
        !is_cancelled
            && is_frontmost
            && requested_route_key == current_route_key
            && requested_channels_revision == current_channels_revision
    }

    pub fn should_recompute_groups(&self, key: &str) -> bool {
        if self.group_cache.is_empty() {
            return true;
        }
        self.last_computed_group_key.as_deref() != Some(key)
    }

    pub fn last_computed_group_key(&self) -> Option<&str> {
        self.last_computed_group_key.as_deref()
    }

    pub fn groups_channels_revision(&self) -> Option<u64> {
        self.groups_channels_revision
    }

    pub fn mark_groups_computed(&mut self, key: &str, channels_revision: Option<u64>) {
        self.last_computed_group_key = Some(key.to_string());
        if let Some(rev) = channels_revision {
            if Some(rev) != self.groups_channels_revision {
                self.groups_channels_revision = Some(rev);
            }
        }
    }

    pub fn insight_focus_category(&self) -> Option<&str> {
        self.selected_category
            .as_deref()
            .or(if self.is_split_explorer_visible {
                self.split_selected_category.as_deref()
            } else {
                None
            })
    }

    pub fn category_growth_rates(&self) -> HashMap<String, f64> {
        self.catalog_store
            .as_ref()
            .map(|c| c.growth_rates())
            .unwrap_or_default()
    }
}
