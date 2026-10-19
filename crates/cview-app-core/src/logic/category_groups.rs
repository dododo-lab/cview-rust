//! `Sources/CViewApp/Logic/CategoryGroupsCompute.swift` 포팅
//! (+ `Views/Category/CategoryBrowseModels.swift` 의 `CategorySortMode`).
//!
//! 카테고리 그리드의 group/sort/filter/typeFilters 계산을 한 번에 묶은 순수 함수.

use std::collections::{BTreeSet, HashMap, HashSet};

use cview_core::models::live_catalog::{CategorySummary, LiveCatalogSummary};
use cview_core::models::LiveChannelItem;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CategorySortMode {
    /// 라이브 수 많은 순(기본).
    #[default]
    LiveCountDesc,
    /// 가나다.
    NameAsc,
}

impl CategorySortMode {
    pub fn raw_value(self) -> &'static str {
        match self {
            Self::LiveCountDesc => "liveCountDesc",
            Self::NameAsc => "nameAsc",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::LiveCountDesc => "라이브 수",
            Self::NameAsc => "이름순",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CategoryGroup {
    pub category: String,
    pub channels: Vec<LiveChannelItem>,
    pub viewer_total: i64,
    /// 라이브 수 — 행이 있으면 `channels.len()`, 미리보기는 요약의 값.
    pub live_count: i64,
    /// 요약으로 그린 미리보기.
    pub is_preview: bool,
}

impl CategoryGroup {
    pub fn new(category: String, channels: Vec<LiveChannelItem>, viewer_total: i64) -> Self {
        let live_count = channels.len() as i64;
        Self {
            category,
            channels,
            viewer_total,
            live_count,
            is_preview: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CategoryRanking {
    pub category: String,
    pub channels: Vec<LiveChannelItem>,
    pub viewer_total: i64,
    pub live_count: i64,
}

/// 유형 필터 칩.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeFilter {
    pub label: String,
    pub icon: String,
    pub value: Option<String>,
}

impl TypeFilter {
    /// "전체" 칩.
    pub fn all() -> Self {
        Self::new("전체", "square.grid.2x2", None)
    }

    fn new(label: &str, icon: &str, value: Option<&str>) -> Self {
        Self {
            label: label.into(),
            icon: icon.into(),
            value: value.map(str::to_string),
        }
    }
}

const KNOWN_TYPES: [(&str, &str, &str); 3] = [
    ("GAME", "게임", "gamecontroller.fill"),
    ("SPORTS", "스포츠", "sportscourt.fill"),
    ("ETC", "기타", "ellipsis.circle.fill"),
];

fn type_filters(present: &HashSet<String>) -> Vec<TypeFilter> {
    let mut filters = vec![TypeFilter::all()];
    for (raw, label, icon) in KNOWN_TYPES {
        if present.contains(raw) {
            filters.push(TypeFilter::new(label, icon, Some(raw)));
        }
    }
    let unknown: BTreeSet<&String> = present
        .iter()
        .filter(|t| !KNOWN_TYPES.iter().any(|(raw, _, _)| raw == t))
        .collect();
    for t in unknown {
        filters.push(TypeFilter::new(t, "tag.fill", Some(t)));
    }
    filters
}

fn sort_groups(mapped: &mut [CategoryGroup], sort: CategorySortMode) {
    match sort {
        CategorySortMode::LiveCountDesc => mapped.sort_by(|l, r| {
            r.live_count
                .cmp(&l.live_count)
                .then_with(|| l.category.cmp(&r.category))
        }),
        CategorySortMode::NameAsc => mapped.sort_by(|l, r| l.category.cmp(&r.category)),
    }
}

fn partition(
    mapped: &[CategoryGroup],
    pins: &HashSet<String>,
) -> (Vec<CategoryGroup>, Vec<CategoryGroup>) {
    if pins.is_empty() {
        return (Vec::new(), mapped.to_vec());
    }
    mapped
        .iter()
        .cloned()
        .partition(|g| pins.contains(&g.category))
}

fn ranking(mapped: &[CategoryGroup]) -> Vec<CategoryRanking> {
    let mut ranking: Vec<CategoryRanking> = mapped
        .iter()
        .map(|g| CategoryRanking {
            category: g.category.clone(),
            channels: g.channels.clone(),
            viewer_total: g.viewer_total,
            live_count: g.live_count,
        })
        .collect();
    ranking.sort_by(|l, r| {
        r.live_count
            .cmp(&l.live_count)
            .then_with(|| r.viewer_total.cmp(&l.viewer_total))
            .then_with(|| l.category.cmp(&r.category))
    });
    ranking
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComputedCategoryGroups {
    pub all: Vec<CategoryGroup>,
    pub pinned: Vec<CategoryGroup>,
    pub unpinned: Vec<CategoryGroup>,
    pub ranking: Vec<CategoryRanking>,
    pub type_filters: Vec<TypeFilter>,
    /// 선택 카테고리 조회용 인덱스(유형 필터 미적용 base).
    pub by_category: HashMap<String, CategoryGroup>,
}

impl ComputedCategoryGroups {
    pub fn compute(
        channels: &[LiveChannelItem],
        type_filter: Option<&str>,
        sort: CategorySortMode,
        pins: &HashSet<String>,
        uncategorized_label: &str,
    ) -> Self {
        let present: HashSet<String> = channels
            .iter()
            .filter_map(|c| c.category_type.clone())
            .collect();
        let filters = type_filters(&present);

        // 1-pass 통합 grouping — 키 순서를 첫 등장 순으로 기억해 결정적으로 만든다.
        let mut by_cat: HashMap<String, (Vec<LiveChannelItem>, i64)> = HashMap::new();
        let mut grouped: HashMap<String, (Vec<LiveChannelItem>, i64)> = HashMap::new();
        for ch in channels {
            let key = match ch.category_name.as_deref() {
                Some(raw) if !raw.is_empty() => raw.to_string(),
                _ => uncategorized_label.to_string(),
            };
            let entry = by_cat.entry(key.clone()).or_default();
            entry.0.push(ch.clone());
            entry.1 += ch.viewer_count;
            if let Some(tf) = type_filter {
                if ch.category_type.as_deref() == Some(tf) {
                    let g = grouped.entry(key).or_default();
                    g.0.push(ch.clone());
                    g.1 += ch.viewer_count;
                }
            }
        }
        if type_filter.is_none() {
            grouped = by_cat.clone();
        }

        let mut mapped: Vec<CategoryGroup> = grouped
            .into_iter()
            .map(|(k, (list, viewers))| CategoryGroup::new(k, list, viewers))
            .collect();
        sort_groups(&mut mapped, sort);
        let (pinned, unpinned) = partition(&mapped, pins);
        let ranking = ranking(&mapped);
        let by_category = by_cat
            .into_iter()
            .map(|(k, (list, viewers))| (k.clone(), CategoryGroup::new(k, list, viewers)))
            .collect();
        Self {
            all: mapped,
            pinned,
            unpinned,
            ranking,
            type_filters: filters,
            by_category,
        }
    }

    /// catalog-summary 만으로 카테고리 그리드를 즉시 그린다(행이 오기 전 첫 그림).
    pub fn previews(
        summary: &LiveCatalogSummary,
        type_filter: Option<&str>,
        sort: CategorySortMode,
        pins: &HashSet<String>,
    ) -> Self {
        let present: HashSet<String> = summary
            .categories
            .iter()
            .filter_map(|c| c.r#type.clone())
            .collect();
        let filters = type_filters(&present);
        let group = |c: &CategorySummary| CategoryGroup {
            category: c.key.clone(),
            channels: vec![c.leader.item()],
            viewer_total: c.viewer_total,
            live_count: c.live_count,
            is_preview: true,
        };
        let mut by_category = HashMap::with_capacity(summary.categories.len());
        let mut mapped = Vec::new();
        for c in &summary.categories {
            let made = group(c);
            by_category.insert(c.key.clone(), made.clone());
            if type_filter.is_none() || c.r#type.as_deref() == type_filter {
                mapped.push(made);
            }
        }
        sort_groups(&mut mapped, sort);
        let (pinned, unpinned) = partition(&mapped, pins);
        let ranking = ranking(&mapped);
        Self {
            all: mapped,
            pinned,
            unpinned,
            ranking,
            type_filters: filters,
            by_category,
        }
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/ComputedCategoryGroupsTests.swift` 핵심 계약.
    use super::*;

    fn ch(id: &str, cat: Option<&str>, ty: Option<&str>, viewers: i64) -> LiveChannelItem {
        LiveChannelItem {
            id: id.into(),
            channel_name: id.into(),
            channel_image_url: None,
            live_title: "t".into(),
            viewer_count: viewers,
            category_name: cat.map(str::to_string),
            category_type: ty.map(str::to_string),
            thumbnail_url: None,
            channel_id: id.into(),
            is_live: true,
            open_date: None,
        }
    }

    fn sample() -> Vec<LiveChannelItem> {
        vec![
            ch("a", Some("LoL"), Some("GAME"), 100),
            ch("b", Some("LoL"), Some("GAME"), 50),
            ch("c", Some("축구"), Some("SPORTS"), 300),
            ch("d", None, Some("ETC"), 5),
            ch("e", Some(""), Some("ZZZ"), 7),
            ch("f", Some("talk"), Some("ETC"), 1),
        ]
    }

    #[test]
    fn groups_sorts_and_type_filters() {
        let r = ComputedCategoryGroups::compute(
            &sample(),
            None,
            CategorySortMode::LiveCountDesc,
            &HashSet::new(),
            "분류 없음",
        );
        let names: Vec<&str> = r.all.iter().map(|g| g.category.as_str()).collect();
        assert_eq!(names, vec!["LoL", "분류 없음", "talk", "축구"]);
        assert_eq!(r.by_category["분류 없음"].channels.len(), 2);
        assert_eq!(r.by_category["LoL"].viewer_total, 150);
        let labels: Vec<&str> = r.type_filters.iter().map(|t| t.label.as_str()).collect();
        assert_eq!(labels, vec!["전체", "게임", "스포츠", "기타", "ZZZ"]);
        // ranking tie-break: live count → viewers → name
        let ranking: Vec<&str> = r.ranking.iter().map(|g| g.category.as_str()).collect();
        assert_eq!(ranking, vec!["LoL", "분류 없음", "축구", "talk"]);
    }

    #[test]
    fn type_filter_keeps_base_index_and_pins_partition() {
        let pins: HashSet<String> = ["축구".to_string()].into();
        let r = ComputedCategoryGroups::compute(
            &sample(),
            Some("SPORTS"),
            CategorySortMode::NameAsc,
            &pins,
            "분류 없음",
        );
        assert_eq!(r.all.len(), 1);
        assert_eq!(r.pinned.len(), 1);
        assert!(r.unpinned.is_empty());
        assert_eq!(r.by_category.len(), 4, "base 인덱스는 유형 필터를 무시한다");
    }
}
