//! 전수 수집 결과 승격 판정·카탈로그 행 수 라벨, 섹션 신선도 표시.
//!
//! 원본: `Sources/CViewApp/Caching/LiveCatalogPromotion.swift`(판정·라벨 부분) ·
//! `Sources/CViewApp/Views/Components/FreshnessCaption.swift`(`DataOrigin`·`FreshnessDisplay`).
//! 둘 다 HomeViewModel 이 직접 쓰는 순수 값이라 여기 둔다. 카탈로그 저장소 자체는
//! [`super::deps::LiveCatalogPort`] 경계 뒤에 있다.

use chrono::{Local, TimeZone, Timelike};
use serde::Serialize;

use cview_core::models::live_catalog::{LiveCatalogQuality, LiveCatalogSummary};
use cview_core::utils::catalog_sweep::{self, CatalogRetryClass};
use cview_core::utils::data_freshness::{self, FreshnessBand, FreshnessPolicy};
use cview_core::utils::time::{interval_since, Date, TimeInterval};
use cview_networking::response::AllLivesStopReason;

use super::HomeViewModel;

/// 한 번의 순회 결과 중 판정에 쓰는 부분.
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogSweepOutcome {
    pub stop_reason: AllLivesStopReason,
    pub exact_live_count: Option<i64>,
    pub observed_count: usize,
}

impl CatalogSweepOutcome {
    pub fn new(
        stop_reason: AllLivesStopReason,
        exact_live_count: Option<i64>,
        observed_count: usize,
    ) -> Self {
        Self {
            stop_reason,
            exact_live_count,
            observed_count,
        }
    }

    pub fn from_result(result: &cview_networking::response::AllLivesCollectionResult) -> Self {
        Self::new(
            result.stop_reason.clone(),
            result.exact_live_count().map(|c| c as i64),
            result.items.len(),
        )
    }

    pub fn completed_normally(&self) -> bool {
        matches!(
            self.stop_reason,
            AllLivesStopReason::ReachedEnd
                | AllLivesStopReason::ReachedPageLimit
                | AllLivesStopReason::CursorCycle { .. }
                | AllLivesStopReason::UnstableOrdering { .. }
        )
    }

    pub fn event_outcome(&self) -> &'static str {
        match self.stop_reason {
            AllLivesStopReason::ReachedEnd => {
                if self.exact_live_count.is_none() {
                    "unverified"
                } else {
                    "verified"
                }
            }
            AllLivesStopReason::ReachedPageLimit
            | AllLivesStopReason::CursorCycle { .. }
            | AllLivesStopReason::UnstableOrdering { .. } => "unverified",
            AllLivesStopReason::PartialFailure { .. } => "failed",
            AllLivesStopReason::Paused => "paused",
            AllLivesStopReason::Cancelled | AllLivesStopReason::ReachedBoundary { .. } => {
                "cancelled"
            }
        }
    }

    pub fn event_reason(&self) -> Option<&'static str> {
        match self.stop_reason {
            AllLivesStopReason::ReachedEnd => self
                .exact_live_count
                .is_none()
                .then_some("unverified-terminal"),
            AllLivesStopReason::ReachedPageLimit => Some("page-limit"),
            AllLivesStopReason::CursorCycle { .. } => Some("cursor-cycle"),
            AllLivesStopReason::UnstableOrdering { .. } => Some("unstable-ordering"),
            AllLivesStopReason::PartialFailure { .. } => Some("partial-failure"),
            AllLivesStopReason::Cancelled => Some("cancelled"),
            AllLivesStopReason::Paused => Some("paused"),
            AllLivesStopReason::ReachedBoundary { .. } => Some("boundary"),
        }
    }

    pub fn retry_class(&self) -> Option<CatalogRetryClass> {
        match self.stop_reason {
            AllLivesStopReason::ReachedEnd => self
                .exact_live_count
                .is_none()
                .then_some(CatalogRetryClass::Ordering),
            AllLivesStopReason::ReachedPageLimit | AllLivesStopReason::UnstableOrdering { .. } => {
                Some(CatalogRetryClass::Ordering)
            }
            AllLivesStopReason::CursorCycle { .. } | AllLivesStopReason::PartialFailure { .. } => {
                Some(CatalogRetryClass::Transient)
            }
            AllLivesStopReason::Cancelled
            | AllLivesStopReason::Paused
            | AllLivesStopReason::ReachedBoundary { .. } => None,
        }
    }
}

/// 직전 완료본.
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogPreviousState {
    pub quality: LiveCatalogQuality,
    pub completed_at: Date,
}

/// 승격 판정.
#[derive(Debug, Clone, PartialEq)]
pub enum CatalogPromotionDecision {
    Promote {
        exact: i64,
    },
    KeepPrevious {
        banner: String,
        retry_after: TimeInterval,
    },
    Provisional {
        reason: String,
        observed: usize,
    },
    Discard,
}

impl CatalogPromotionDecision {
    pub fn quality(&self) -> Option<LiveCatalogQuality> {
        match self {
            Self::Promote { exact } => Some(LiveCatalogQuality::Verified { exact: *exact }),
            Self::Provisional { reason, observed } => Some(LiveCatalogQuality::Observed {
                count: *observed as i64,
                reason: reason.clone(),
            }),
            Self::KeepPrevious { .. } | Self::Discard => None,
        }
    }

    pub fn records_growth_baseline(&self) -> bool {
        matches!(self, Self::Promote { .. })
    }
}

/// `LiveCatalogPromotion` 네임스페이스.
pub mod live_catalog_promotion {
    use super::*;

    pub const KEEP_PREVIOUS_WINDOW: TimeInterval = 30.0 * 60.0;
    pub const UNVERIFIED_TERMINAL_BANNER: &str =
        "전체 페이지는 확인했지만 총계 안정성 검증이 완료되지 않았습니다.";
    pub const PAGE_LIMIT_BANNER: &str = "수집 상한에 도달해 일부만 반영했습니다.";
    pub const CURSOR_CYCLE_BANNER: &str = "페이지 커서가 반복되어 전체 수집을 중단했습니다.";
    pub const UNSTABLE_ORDERING_BANNER: &str =
        "페이지 경계 검증 문제로 정확한 총계를 확정하지 못했습니다.";

    pub fn decide(
        outcome: &CatalogSweepOutcome,
        previous: Option<&CatalogPreviousState>,
        now: &Date,
    ) -> CatalogPromotionDecision {
        let (banner, reason, retry_after): (String, &str, TimeInterval) = match &outcome.stop_reason
        {
            AllLivesStopReason::Cancelled
            | AllLivesStopReason::Paused
            | AllLivesStopReason::ReachedBoundary { .. } => {
                return CatalogPromotionDecision::Discard
            }
            AllLivesStopReason::ReachedEnd => {
                if let Some(exact) = outcome.exact_live_count {
                    return CatalogPromotionDecision::Promote { exact };
                }
                (
                    UNVERIFIED_TERMINAL_BANNER.into(),
                    "unverified-terminal",
                    catalog_sweep::ORDERING_RETRY,
                )
            }
            AllLivesStopReason::ReachedPageLimit => (
                PAGE_LIMIT_BANNER.into(),
                "page-limit",
                catalog_sweep::ORDERING_RETRY,
            ),
            AllLivesStopReason::CursorCycle { .. } => (
                CURSOR_CYCLE_BANNER.into(),
                "cursor-cycle",
                catalog_sweep::TRANSIENT_RETRY,
            ),
            AllLivesStopReason::UnstableOrdering { .. } => (
                UNSTABLE_ORDERING_BANNER.into(),
                "unstable-ordering",
                catalog_sweep::ORDERING_RETRY,
            ),
            AllLivesStopReason::PartialFailure { page, .. } => (
                format!("일부 페이지({page}) 수집에 실패했습니다. 다시 시도해 주세요."),
                "partial-failure",
                catalog_sweep::TRANSIENT_RETRY,
            ),
        };
        if let Some(previous) = previous {
            if previous.quality.is_verified_or_stitched()
                && interval_since(now, &previous.completed_at) < KEEP_PREVIOUS_WINDOW
            {
                return CatalogPromotionDecision::KeepPrevious {
                    banner,
                    retry_after,
                };
            }
        }
        CatalogPromotionDecision::Provisional {
            reason: reason.into(),
            observed: outcome.observed_count,
        }
    }

    /// 로컬 시간대 기준 행 수 라벨.
    pub fn catalog_count_label(
        quality: &LiveCatalogQuality,
        completed_at: Option<&Date>,
    ) -> String {
        catalog_count_label_in(quality, completed_at, &Local)
    }

    /// 시간대를 받는 판(테스트용).
    pub fn catalog_count_label_in<Tz: TimeZone>(
        quality: &LiveCatalogQuality,
        completed_at: Option<&Date>,
        tz: &Tz,
    ) -> String
    where
        Tz::Offset: std::fmt::Display,
    {
        match quality {
            LiveCatalogQuality::Verified { exact } => {
                HomeViewModel::verified_live_count_label_in(*exact, completed_at, tz)
            }
            LiveCatalogQuality::Stitched { observed } => {
                observed_label(*observed, completed_at, tz)
            }
            LiveCatalogQuality::Observed { count, .. } => observed_label(*count, completed_at, tz),
        }
    }

    pub fn summary_count_label(summary: &LiveCatalogSummary) -> String {
        catalog_count_label(&summary.quality, Some(&summary.completed_at))
    }

    pub fn summary_compact_label(summary: &LiveCatalogSummary) -> String {
        match &summary.quality {
            LiveCatalogQuality::Verified { exact } => format!("{exact} 확인"),
            LiveCatalogQuality::Stitched { observed } => format!("{observed} 관측"),
            LiveCatalogQuality::Observed { count, .. } => format!("{count} 관측"),
        }
    }

    fn observed_label<Tz: TimeZone>(count: i64, completed_at: Option<&Date>, tz: &Tz) -> String {
        let Some(at) = completed_at else {
            return format!("{count}개 관측");
        };
        let local = at.with_timezone(tz);
        format!(
            "{count}개 관측 · {:02}:{:02} 집계",
            local.hour(),
            local.minute()
        )
    }
}

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
