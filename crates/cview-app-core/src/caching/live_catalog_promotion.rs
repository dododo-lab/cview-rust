//! `Sources/CViewApp/Caching/LiveCatalogPromotion.swift` 포팅.
//!
//! 전수 수집 결과를 완료본으로 승격할지 정하는 순수 판정.
//!
//! ```text
//! 결과(StopReason·exact) × 이전 상태(검증 통과본인가·몇 분 전인가) → promote / keepPrevious / provisional / discard
//! ```
//!
//! 표시 문구(`catalog_count_label`), 이벤트 표기, 재시도 분류, 이어 붙인 결과 검증
//! (`StitchedCatalogVerifier`)도 여기 있다. `HomeViewModel.verifiedLiveCountLabel` 은
//! [`verified_live_count_label`] 로 옮겼다.

use std::collections::{HashMap, HashSet};

use chrono::{Local, TimeZone, Timelike};
use cview_core::models::live_catalog::{LiveCatalogQuality, LiveCatalogRow, LiveCatalogSummary};
use cview_core::utils::catalog_sweep::{
    CatalogRetryClass, CatalogSweepDecision, ORDERING_RETRY, RESUME_GENERATION_LIMIT,
    TRANSIENT_RETRY,
};
use cview_core::utils::data_freshness::aggregated_caption;
use cview_core::utils::time::{interval_since, Date, TimeInterval};
use cview_networking::response::{AllLivesCollectionResult, AllLivesStopReason};

/// 판정이 보는 수집 결과 — `AllLivesCollectionResult` 에서 필요한 값만 뽑은 순수 값.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogSweepOutcome {
    pub stop_reason: AllLivesStopReason,
    /// 총계 검증을 통과한 관측 총계(`ReachedEnd` 에서만 있다).
    pub exact_live_count: Option<i64>,
    /// 이번에 모은 행 수.
    pub observed_count: i64,
}

impl CatalogSweepOutcome {
    pub fn new(
        stop_reason: AllLivesStopReason,
        exact_live_count: Option<i64>,
        observed_count: i64,
    ) -> Self {
        Self {
            stop_reason,
            exact_live_count,
            observed_count,
        }
    }

    pub fn from_result(result: &AllLivesCollectionResult) -> Self {
        Self::new(
            result.stop_reason.clone(),
            result.exact_live_count().map(|c| c as i64),
            result.items.len() as i64,
        )
    }

    /// 성공 취급(백오프하지 않음) — 검증·미검증 완료. 부분 실패와 중단은 아니다.
    pub fn completed_normally(&self) -> bool {
        matches!(
            self.stop_reason,
            AllLivesStopReason::ReachedEnd
                | AllLivesStopReason::ReachedPageLimit
                | AllLivesStopReason::CursorCycle { .. }
                | AllLivesStopReason::UnstableOrdering { .. }
        )
    }

    /// `catalog_sweep.outcome` — verified / unverified / failed / cancelled / paused.
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

    /// `catalog_sweep.reason` — 완전하지 못한 이유(있을 때만).
    pub fn event_reason(&self) -> Option<&'static str> {
        Some(match self.stop_reason {
            AllLivesStopReason::ReachedEnd => {
                return self
                    .exact_live_count
                    .is_none()
                    .then_some("unverified-terminal")
            }
            AllLivesStopReason::ReachedPageLimit => "page-limit",
            AllLivesStopReason::CursorCycle { .. } => "cursor-cycle",
            AllLivesStopReason::UnstableOrdering { .. } => "unstable-ordering",
            AllLivesStopReason::PartialFailure { .. } => "partial-failure",
            AllLivesStopReason::Cancelled => "cancelled",
            AllLivesStopReason::Paused => "paused",
            AllLivesStopReason::ReachedBoundary { .. } => "boundary",
        })
    }

    /// 불완전 수집 뒤 재시도 분류 — 일시 오류 90초, 정렬 문제·미검증 600초. 정상·중단은 `None`.
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

/// 승격 판정이 비교하는 이전 완료본 — 품질과 집계 시각.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogPreviousState {
    pub quality: LiveCatalogQuality,
    pub completed_at: Date,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CatalogPromotionDecision {
    /// 검증 통과본으로 승격한다(exact 총계).
    Promote { exact: i64 },
    /// 이전 검증 통과본을 지킨다.
    KeepPrevious {
        banner: String,
        retry_after: TimeInterval,
    },
    /// 미검증 관측값으로 채택한다 — 기준선에는 기록하지 않는다.
    Provisional { reason: String, observed: i64 },
    /// 결과를 채택하지 않는다.
    Discard,
}

impl CatalogPromotionDecision {
    /// 이 판정이 만드는 저장 품질. 채택하지 않는 판정은 `None`.
    pub fn quality(&self) -> Option<LiveCatalogQuality> {
        match self {
            Self::Promote { exact } => Some(LiveCatalogQuality::Verified { exact: *exact }),
            Self::Provisional { reason, observed } => Some(LiveCatalogQuality::Observed {
                count: *observed,
                reason: reason.clone(),
            }),
            Self::KeepPrevious { .. } | Self::Discard => None,
        }
    }

    /// 증가율 기준선에 기록해도 되는지 — 승격된 검증 통과본만.
    pub fn records_growth_baseline(&self) -> bool {
        matches!(self, Self::Promote { .. })
    }
}

pub struct LiveCatalogPromotion;

struct UnverifiedNotice {
    banner: String,
    reason: &'static str,
    retry_after: TimeInterval,
}

impl LiveCatalogPromotion {
    /// 이전 검증 통과본을 지키는 시간.
    pub const KEEP_PREVIOUS_WINDOW: TimeInterval = 30.0 * 60.0;

    pub const UNVERIFIED_TERMINAL_BANNER: &'static str =
        "전체 페이지는 확인했지만 총계 안정성 검증이 완료되지 않았습니다.";
    pub const PAGE_LIMIT_BANNER: &'static str = "수집 상한에 도달해 일부만 반영했습니다.";
    pub const CURSOR_CYCLE_BANNER: &'static str =
        "페이지 커서가 반복되어 전체 수집을 중단했습니다.";
    pub const UNSTABLE_ORDERING_BANNER: &'static str =
        "페이지 경계 검증 문제로 정확한 총계를 확정하지 못했습니다.";

    /// 승격 판정. `now` 는 주입한다.
    pub fn decide(
        outcome: &CatalogSweepOutcome,
        previous: Option<&CatalogPreviousState>,
        now: Date,
    ) -> CatalogPromotionDecision {
        let notice =
            |banner: &str, reason: &'static str, retry_after: TimeInterval| UnverifiedNotice {
                banner: banner.to_string(),
                reason,
                retry_after,
            };
        let notice = match &outcome.stop_reason {
            AllLivesStopReason::Cancelled
            | AllLivesStopReason::Paused
            | AllLivesStopReason::ReachedBoundary { .. } => {
                return CatalogPromotionDecision::Discard
            }
            AllLivesStopReason::ReachedEnd => {
                if let Some(exact) = outcome.exact_live_count {
                    return CatalogPromotionDecision::Promote { exact };
                }
                notice(
                    Self::UNVERIFIED_TERMINAL_BANNER,
                    "unverified-terminal",
                    ORDERING_RETRY,
                )
            }
            AllLivesStopReason::ReachedPageLimit => {
                notice(Self::PAGE_LIMIT_BANNER, "page-limit", ORDERING_RETRY)
            }
            AllLivesStopReason::CursorCycle { .. } => {
                notice(Self::CURSOR_CYCLE_BANNER, "cursor-cycle", TRANSIENT_RETRY)
            }
            AllLivesStopReason::UnstableOrdering { .. } => notice(
                Self::UNSTABLE_ORDERING_BANNER,
                "unstable-ordering",
                ORDERING_RETRY,
            ),
            AllLivesStopReason::PartialFailure { page, .. } => UnverifiedNotice {
                banner: format!("일부 페이지({page}) 수집에 실패했습니다. 다시 시도해 주세요."),
                reason: "partial-failure",
                retry_after: TRANSIENT_RETRY,
            },
        };
        if let Some(previous) = previous {
            if previous.quality.is_verified_or_stitched()
                && interval_since(&now, &previous.completed_at) < Self::KEEP_PREVIOUS_WINDOW
            {
                return CatalogPromotionDecision::KeepPrevious {
                    banner: notice.banner,
                    retry_after: notice.retry_after,
                };
            }
        }
        CatalogPromotionDecision::Provisional {
            reason: notice.reason.to_string(),
            observed: outcome.observed_count,
        }
    }

    /// 카탈로그 행 수 라벨. verified 는 "N개 확인 · HH:MM 집계", stitched·observed 는 "N개 관측 · HH:MM 집계".
    pub fn catalog_count_label(quality: &LiveCatalogQuality, completed_at: Option<Date>) -> String {
        Self::catalog_count_label_in(quality, completed_at, &Local)
    }

    /// 시간대 주입판(테스트).
    pub fn catalog_count_label_in<Tz: TimeZone>(
        quality: &LiveCatalogQuality,
        completed_at: Option<Date>,
        tz: &Tz,
    ) -> String
    where
        Tz::Offset: std::fmt::Display,
    {
        match quality {
            LiveCatalogQuality::Verified { exact } => {
                verified_live_count_label_in(*exact, completed_at, tz)
            }
            LiveCatalogQuality::Stitched { observed } => {
                observed_label(*observed, completed_at, tz)
            }
            LiveCatalogQuality::Observed { count, .. } => observed_label(*count, completed_at, tz),
        }
    }

    /// 요약 한 건의 헤더 문구.
    pub fn summary_count_label(summary: &LiveCatalogSummary) -> String {
        Self::catalog_count_label(&summary.quality, Some(summary.completed_at))
    }

    /// 좁은 헤더 칩용 — verified 만 "확인", 나머지는 "관측".
    pub fn summary_compact_label(summary: &LiveCatalogSummary) -> String {
        match &summary.quality {
            LiveCatalogQuality::Verified { exact } => format!("{exact} 확인"),
            LiveCatalogQuality::Stitched { observed } => format!("{observed} 관측"),
            LiveCatalogQuality::Observed { count, .. } => format!("{count} 관측"),
        }
    }
}

/// `HomeViewModel.verifiedLiveCountLabel`.
pub fn verified_live_count_label(count: i64, observed_at: Option<Date>) -> String {
    verified_live_count_label_in(count, observed_at, &Local)
}

/// 시간대 주입판(테스트).
pub fn verified_live_count_label_in<Tz: TimeZone>(
    count: i64,
    observed_at: Option<Date>,
    tz: &Tz,
) -> String
where
    Tz::Offset: std::fmt::Display,
{
    match observed_at {
        None => format!("{count}개 확인"),
        Some(at) => format!("{count}개 확인 · {}", aggregated_caption(&at, tz)),
    }
}

fn observed_label<Tz: TimeZone>(count: i64, completed_at: Option<Date>, tz: &Tz) -> String {
    match completed_at {
        None => format!("{count}개 관측"),
        Some(at) => {
            let local = at.with_timezone(tz);
            format!(
                "{count}개 관측 · {:02}:{:02} 집계",
                local.hour(),
                local.minute()
            )
        }
    }
}

// MARK: - 수집 결과 요약

/// 수집 한 번의 결과 요약 — 이벤트(E5)·성공 여부·재시도 분류의 입력.
#[derive(Debug, Clone, PartialEq)]
pub struct SweepReport {
    pub outcome: String,
    pub detail: Option<String>,
    pub pages: usize,
    pub rows: usize,
    pub discarded_pages: usize,
    pub resumed: bool,
    pub completed_normally: bool,
    pub retry_class: Option<CatalogRetryClass>,
}

impl SweepReport {
    pub fn new(result: &AllLivesCollectionResult, discarded_pages: usize, resumed: bool) -> Self {
        let outcome = CatalogSweepOutcome::from_result(result);
        Self {
            outcome: outcome.event_outcome().to_string(),
            detail: outcome.event_reason().map(str::to_string),
            pages: result.received_pages,
            rows: result.items.len(),
            discarded_pages,
            resumed,
            completed_normally: outcome.completed_normally(),
            retry_class: outcome.retry_class(),
        }
    }

    /// 이어 붙여 검증을 통과한 수집 — 검증 통과본으로 센다.
    pub fn stitched(stitched: &Stitched, pages: usize) -> Self {
        Self {
            outcome: "verified".into(),
            detail: None,
            pages,
            rows: stitched.rows.len(),
            discarded_pages: 0,
            resumed: true,
            completed_normally: true,
            retry_class: None,
        }
    }
}

/// `CatalogSweepDecision` 확장.
pub trait CatalogSweepDecisionExt {
    /// 지금 수집을 돌려야 하는가(새로 또는 이어서).
    fn starts_sweep(&self) -> bool;
    /// 코디네이터 작업의 성공 여부 — `Wait` 는 실패로 알려 짧은 백오프 뒤 다시 판정하게 한다.
    fn scheduler_success(&self) -> bool;
}

impl CatalogSweepDecisionExt for CatalogSweepDecision {
    fn starts_sweep(&self) -> bool {
        matches!(self, Self::Run(_) | Self::Resume)
    }
    fn scheduler_success(&self) -> bool {
        !matches!(self, Self::Wait { .. })
    }
}

// MARK: - 이어 붙인 결과 검증 (AC-12)

/// 검증 입력.
#[derive(Debug, Clone)]
pub struct StitchedInputs {
    pub checkpoint_rows: Vec<LiveCatalogRow>,
    pub max_live_id: i64,
    /// 계속분. `None` 이면 이전 실행에서 이미 끝났다(보충분만 필요).
    pub continuation: Option<AllLivesCollectionResult>,
    pub top_up: AllLivesCollectionResult,
    pub started_at: Date,
    pub now: Date,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stitched {
    pub rows: Vec<LiveCatalogRow>,
    pub replaced_by_restart: usize,
    pub pages: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StitchRejectReason {
    GenerationTooOld,
    ContinuationIncomplete,
    ContinuationDuplicates,
    ContinuationOverlap,
    TopUpIncomplete,
    TopUpDuplicates,
    BoundaryUnproven,
}

impl StitchRejectReason {
    pub fn raw_value(self) -> &'static str {
        match self {
            Self::GenerationTooOld => "generation-too-old",
            Self::ContinuationIncomplete => "continuation-incomplete",
            Self::ContinuationDuplicates => "continuation-duplicates",
            Self::ContinuationOverlap => "continuation-overlap",
            Self::TopUpIncomplete => "topup-incomplete",
            Self::TopUpDuplicates => "topup-duplicates",
            Self::BoundaryUnproven => "boundary-unproven",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StitchVerdict {
    Accepted(Stitched),
    Rejected(StitchRejectReason),
}

/// 체크포인트에서 이어 받은 결과(계속분 + 보충분)가 한 번의 순회와 같은 목록이라 말할 수 있는지
/// 판정하는 순수 검증기.
pub struct StitchedCatalogVerifier;

impl StitchedCatalogVerifier {
    pub fn verify(inputs: &StitchedInputs) -> StitchVerdict {
        let elapsed = interval_since(&inputs.now, &inputs.started_at);
        if !(0.0..=RESUME_GENERATION_LIMIT).contains(&elapsed) {
            return StitchVerdict::Rejected(StitchRejectReason::GenerationTooOld);
        }
        if let Some(failure) = Self::continuation_failure(inputs) {
            return StitchVerdict::Rejected(failure);
        }
        if let Some(failure) = Self::top_up_failure(inputs) {
            return StitchVerdict::Rejected(failure);
        }
        let mut combined: Vec<LiveCatalogRow> = inputs
            .top_up
            .items
            .iter()
            .map(LiveCatalogRow::from)
            .collect();
        combined.extend(inputs.checkpoint_rows.iter().cloned());
        if let Some(c) = &inputs.continuation {
            combined.extend(c.items.iter().map(LiveCatalogRow::from));
        }
        let (rows, replaced) = Self::merge_by_channel(combined);
        let pages = inputs.top_up.received_pages
            + inputs
                .continuation
                .as_ref()
                .map(|c| c.received_pages)
                .unwrap_or(0);
        StitchVerdict::Accepted(Stitched {
            rows,
            replaced_by_restart: replaced,
            pages,
        })
    }

    fn continuation_failure(inputs: &StitchedInputs) -> Option<StitchRejectReason> {
        let continuation = inputs.continuation.as_ref()?;
        if continuation.stop_reason != AllLivesStopReason::ReachedEnd
            || !continuation.stable_cursor_ordering_verified
        {
            return Some(StitchRejectReason::ContinuationIncomplete);
        }
        if continuation.deduplicated_count != 0 {
            return Some(StitchRejectReason::ContinuationDuplicates);
        }
        let floor = inputs
            .checkpoint_rows
            .iter()
            .map(|r| r.live_id)
            .min()
            .unwrap_or(0);
        if let Some(first) = continuation.items.iter().map(|i| i.live_id).max() {
            if first >= floor {
                return Some(StitchRejectReason::ContinuationOverlap);
            }
        }
        None
    }

    fn top_up_failure(inputs: &StitchedInputs) -> Option<StitchRejectReason> {
        let top_up = &inputs.top_up;
        match &top_up.stop_reason {
            AllLivesStopReason::ReachedBoundary { live_id } => {
                if *live_id > inputs.max_live_id {
                    return Some(StitchRejectReason::BoundaryUnproven);
                }
            }
            AllLivesStopReason::ReachedEnd => {
                if !top_up.stable_cursor_ordering_verified {
                    return Some(StitchRejectReason::TopUpIncomplete);
                }
            }
            _ => return Some(StitchRejectReason::TopUpIncomplete),
        }
        if top_up.deduplicated_count != 0 {
            return Some(StitchRejectReason::TopUpDuplicates);
        }
        if !top_up.items.iter().all(|i| i.live_id > inputs.max_live_id) {
            return Some(StitchRejectReason::BoundaryUnproven);
        }
        None
    }

    /// 채널 하나에 방송이 둘이면(재시작) 큰 liveId 를 남긴다. 순서는 처음 만난 자리를 지킨다.
    pub fn merge_by_channel(rows: Vec<LiveCatalogRow>) -> (Vec<LiveCatalogRow>, usize) {
        let mut out: Vec<LiveCatalogRow> = Vec::new();
        let mut index_by_channel: HashMap<String, usize> = HashMap::new();
        let mut seen_live_ids: HashSet<i64> = HashSet::new();
        let mut replaced = 0;
        for row in rows {
            if !seen_live_ids.insert(row.live_id) {
                continue;
            }
            if row.channel_id.is_empty() {
                out.push(row);
                continue;
            }
            if let Some(&index) = index_by_channel.get(&row.channel_id) {
                replaced += 1;
                if row.live_id > out[index].live_id {
                    out[index] = row;
                }
            } else {
                index_by_channel.insert(row.channel_id.clone(), out.len());
                out.push(row);
            }
        }
        (out, replaced)
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/LiveCatalogPromotionTests.swift`
    use super::*;
    use chrono::FixedOffset;
    use cview_core::utils::time::{adding, from_unix};

    fn now() -> Date {
        from_unix(1_700_000_000.0)
    }

    fn previous_states() -> Vec<(Option<CatalogPreviousState>, bool)> {
        let st = |quality, ago: f64| {
            Some(CatalogPreviousState {
                quality,
                completed_at: adding(&now(), -ago),
            })
        };
        vec![
            (None, false),
            (st(LiveCatalogQuality::Verified { exact: 10 }, 600.0), true),
            (
                st(LiveCatalogQuality::Stitched { observed: 10 }, 600.0),
                true,
            ),
            (
                st(LiveCatalogQuality::Verified { exact: 10 }, 3_600.0),
                false,
            ),
            (
                st(
                    LiveCatalogQuality::Observed {
                        count: 10,
                        reason: "x".into(),
                    },
                    60.0,
                ),
                false,
            ),
        ]
    }

    fn decide(
        reason: AllLivesStopReason,
        exact: Option<i64>,
        previous: Option<&CatalogPreviousState>,
    ) -> CatalogPromotionDecision {
        LiveCatalogPromotion::decide(&CatalogSweepOutcome::new(reason, exact, 5), previous, now())
    }

    #[test]
    fn exact_completion_always_promotes() {
        for (previous, _) in previous_states() {
            let d = decide(AllLivesStopReason::ReachedEnd, Some(7), previous.as_ref());
            assert_eq!(d, CatalogPromotionDecision::Promote { exact: 7 });
            assert_eq!(d.quality(), Some(LiveCatalogQuality::Verified { exact: 7 }));
            assert!(d.records_growth_baseline());
        }
    }

    #[test]
    fn interrupted_outcomes_are_discarded() {
        for reason in [
            AllLivesStopReason::Cancelled,
            AllLivesStopReason::Paused,
            AllLivesStopReason::ReachedBoundary { live_id: 9 },
        ] {
            for (previous, _) in previous_states() {
                let d = decide(reason.clone(), None, previous.as_ref());
                assert_eq!(d, CatalogPromotionDecision::Discard);
                assert!(d.quality().is_none());
            }
        }
    }

    #[test]
    fn unverified_outcomes_keep_previous_or_become_provisional() {
        let cases = [
            (AllLivesStopReason::ReachedEnd, 600.0, "unverified-terminal"),
            (AllLivesStopReason::ReachedPageLimit, 600.0, "page-limit"),
            (
                AllLivesStopReason::CursorCycle { page: 4 },
                90.0,
                "cursor-cycle",
            ),
            (
                AllLivesStopReason::UnstableOrdering {
                    attempts: 3,
                    duplicate_count: 2,
                },
                600.0,
                "unstable-ordering",
            ),
            (
                AllLivesStopReason::PartialFailure {
                    page: 3,
                    message: "boom".into(),
                },
                90.0,
                "partial-failure",
            ),
        ];
        for (reason, retry, tag) in cases {
            for (previous, keepable) in previous_states() {
                let d = decide(reason.clone(), None, previous.as_ref());
                if keepable {
                    match &d {
                        CatalogPromotionDecision::KeepPrevious {
                            banner,
                            retry_after,
                        } => {
                            assert!(!banner.is_empty());
                            assert_eq!(*retry_after, retry);
                        }
                        other => panic!("keepPrevious 여야 한다: {other:?}"),
                    }
                } else {
                    assert_eq!(
                        d,
                        CatalogPromotionDecision::Provisional {
                            reason: tag.into(),
                            observed: 5
                        }
                    );
                }
                assert!(!d.records_growth_baseline());
            }
        }
    }

    #[test]
    fn keep_window_boundary() {
        let at = |ago: f64| CatalogPreviousState {
            quality: LiveCatalogQuality::Verified { exact: 1 },
            completed_at: adding(&now(), -ago),
        };
        assert!(matches!(
            decide(
                AllLivesStopReason::ReachedPageLimit,
                None,
                Some(&at(1_799.0))
            ),
            CatalogPromotionDecision::KeepPrevious { .. }
        ));
        assert_eq!(
            decide(
                AllLivesStopReason::ReachedPageLimit,
                None,
                Some(&at(1_800.0))
            ),
            CatalogPromotionDecision::Provisional {
                reason: "page-limit".into(),
                observed: 5
            }
        );
    }

    #[test]
    fn banners_keep_legacy_wording() {
        let fresh = previous_states().remove(1).0;
        assert_eq!(
            decide(
                AllLivesStopReason::PartialFailure {
                    page: 3,
                    message: "x".into()
                },
                None,
                fresh.as_ref()
            ),
            CatalogPromotionDecision::KeepPrevious {
                banner: "일부 페이지(3) 수집에 실패했습니다. 다시 시도해 주세요.".into(),
                retry_after: TRANSIENT_RETRY
            }
        );
    }

    #[test]
    fn event_outcome_names() {
        let o = |r, exact| CatalogSweepOutcome::new(r, exact, 1);
        assert_eq!(
            o(AllLivesStopReason::ReachedEnd, Some(3)).event_outcome(),
            "verified"
        );
        assert_eq!(
            o(AllLivesStopReason::ReachedEnd, Some(3)).event_reason(),
            None
        );
        assert_eq!(
            o(AllLivesStopReason::ReachedEnd, None).event_outcome(),
            "unverified"
        );
        assert_eq!(
            o(AllLivesStopReason::ReachedPageLimit, None).event_outcome(),
            "unverified"
        );
        let failed = o(
            AllLivesStopReason::PartialFailure {
                page: 1,
                message: "x".into(),
            },
            None,
        );
        assert_eq!(failed.event_outcome(), "failed");
        assert!(!failed.completed_normally());
        assert_eq!(
            o(AllLivesStopReason::Cancelled, None).event_outcome(),
            "cancelled"
        );
        assert!(o(
            AllLivesStopReason::UnstableOrdering {
                attempts: 1,
                duplicate_count: 0
            },
            None
        )
        .completed_normally());
        assert_eq!(
            o(AllLivesStopReason::CursorCycle { page: 1 }, None).retry_class(),
            Some(CatalogRetryClass::Transient)
        );
    }

    #[test]
    fn count_labels_by_quality() {
        let seoul = FixedOffset::east_opt(9 * 3600).unwrap();
        let at = from_unix(1_700_000_000.0); // 2023-11-15 07:13 KST
        assert_eq!(
            LiveCatalogPromotion::catalog_count_label_in(
                &LiveCatalogQuality::Observed {
                    count: 3,
                    reason: "x".into()
                },
                Some(at),
                &seoul
            ),
            "3개 관측 · 07:13 집계"
        );
        assert_eq!(
            LiveCatalogPromotion::catalog_count_label_in(
                &LiveCatalogQuality::Verified { exact: 9 },
                None,
                &seoul
            ),
            "9개 확인"
        );
    }

    fn row(live_id: i64, channel: &str) -> LiveCatalogRow {
        LiveCatalogRow {
            live_id,
            channel_id: channel.into(),
            channel_name: channel.into(),
            channel_image_url: None,
            live_title: String::new(),
            viewer_count: 0,
            category_name: None,
            category_id: None,
            category_type: None,
            thumbnail_url: None,
            open_date: None,
        }
    }

    #[test]
    fn merge_by_channel_keeps_newest_live_and_first_position() {
        let (rows, replaced) = StitchedCatalogVerifier::merge_by_channel(vec![
            row(5, "a"),
            row(7, "b"),
            row(9, "a"),
            row(9, "z"),
            row(3, ""),
        ]);
        let ids: Vec<(i64, &str)> = rows
            .iter()
            .map(|r| (r.live_id, r.channel_id.as_str()))
            .collect();
        assert_eq!(ids, vec![(9, "a"), (7, "b"), (3, "")]);
        assert_eq!(replaced, 1);
    }

    #[test]
    fn stitched_verifier_rejects_old_generation_and_unproven_boundary() {
        let empty = AllLivesCollectionResult {
            items: Vec::new(),
            estimated_total: None,
            requested_pages: 1,
            received_pages: 1,
            deduplicated_count: 0,
            stop_reason: AllLivesStopReason::ReachedBoundary { live_id: 10 },
            verified_observed_count: None,
            stable_cursor_ordering_verified: false,
            resume_point: None,
            first_live_id: None,
        };
        let inputs = StitchedInputs {
            checkpoint_rows: vec![row(10, "a")],
            max_live_id: 10,
            continuation: None,
            top_up: empty.clone(),
            started_at: now(),
            now: adding(&now(), 30.0),
        };
        match StitchedCatalogVerifier::verify(&inputs) {
            StitchVerdict::Accepted(s) => {
                assert_eq!(s.rows.len(), 1);
                assert_eq!(s.pages, 1);
            }
            other => panic!("{other:?}"),
        }
        let mut old = inputs.clone();
        old.now = adding(&now(), 601.0);
        assert_eq!(
            StitchedCatalogVerifier::verify(&old),
            StitchVerdict::Rejected(StitchRejectReason::GenerationTooOld)
        );
        let mut unproven = inputs;
        unproven.top_up.stop_reason = AllLivesStopReason::ReachedBoundary { live_id: 11 };
        assert_eq!(
            StitchedCatalogVerifier::verify(&unproven),
            StitchVerdict::Rejected(StitchRejectReason::BoundaryUnproven)
        );
    }
}
