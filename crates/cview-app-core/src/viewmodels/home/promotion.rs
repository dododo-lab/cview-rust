//! 섹션 신선도 표시.
//!
//! 원본: `Sources/CViewApp/Views/Components/FreshnessCaption.swift`(`DataOrigin`·`FreshnessDisplay`).
//! 승격 판정·카탈로그 행 수 라벨(`LiveCatalogPromotion.swift`)은
//! [`crate::caching::live_catalog_promotion`] 한 곳에 있고 여기서 다시 내보낸다.

use serde::Serialize;

use cview_core::utils::data_freshness::{self, FreshnessBand, FreshnessPolicy};
use cview_core::utils::time::Date;

pub use crate::caching::live_catalog_promotion::{
    verified_live_count_label, verified_live_count_label_in, CatalogPreviousState,
    CatalogPromotionDecision, CatalogSweepOutcome, LiveCatalogPromotion,
};

/// 값이 어디서 왔는가.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DataOrigin {
    Snapshot,
    Network,
}

/// 섹션 머리 캡션과 시청자 수 흐림 여부.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FreshnessDisplay {
    pub caption: Option<String>,
    pub viewers_stale: bool,
}

impl FreshnessDisplay {
    pub const NONE: Self = Self {
        caption: None,
        viewers_stale: false,
    };

    pub fn make(
        fetched_at: Option<&Date>,
        origin: DataOrigin,
        policy: &FreshnessPolicy,
        now: &Date,
    ) -> Self {
        let Some(fetched_at) = fetched_at else {
            return Self::NONE;
        };
        let restored = origin == DataOrigin::Snapshot;
        let band = data_freshness::band(Some(fetched_at), policy, now);
        Self {
            caption: data_freshness::section_caption(Some(fetched_at), restored, policy, now),
            viewers_stale: restored || band != FreshnessBand::Fresh,
        }
    }
}
