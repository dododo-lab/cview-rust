//! `Sources/CViewApp/Caching/LiveCatalogStore.swift` (+ `LiveCatalogStore+Activity.swift`) 포팅.
//!
//! 카탈로그(전체 라이브 전수 수집)의 **단일 소유자**. 수집은 이 스토어가 소유한 작업 한 개로만
//! 돈다 — 화면을 옮겨도 진행 중이던 수집이 버려지지 않는다.
//!
//! ```text
//! 화면·코디네이터 ──request_sweep──▶ LiveCatalogStore(작업 소유) ──결과──▶ 결과 처리기(행 commit·영속화)
//! ```
//!
//! - 수요(`CatalogDemandHolder`)는 카테고리·홈·검색 랜딩만 센다. 수요를 놓아도 진행 중인 수집은 취소하지 않는다.
//! - 진행 중에 요청이 오면 합류한다(같은 `sweep_id`, 순회 1회).
//! - 수동 규칙: 마지막 정상 수집이 `manual_skip_window` 안이면 새로 돌리지 않는다.
//!
//! 이식 범위 밖(목록은 크레이트 보고서에도 있다):
//! - `+Checkpoint`(이어받기·체크포인트 파일)·`+Persistence`(행·요약 쌍 파일)는 Rust 코어에 없는
//!   `MenuSnapshotStore` 에 묶여 있어 옮기지 않았다. 이어 붙인 결과 검증은
//!   [`super::live_catalog_promotion::StitchedCatalogVerifier`] 로 옮겼다.
//! - `promote`(요약 빌드·기준선 기록)는 Rust 코어에 없는 `LiveCatalogSummaryBuilder` 몫이라, 여기서는
//!   만든 요약을 받는 [`LiveCatalogStore::commit_summary`] 만 제공한다.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use cview_core::models::live_catalog::LiveCatalogSummary;
use cview_core::utils::catalog_sweep::{
    self, CatalogRetryClass, CatalogSweepAttempt, CatalogSweepDecision, CatalogSweepIdleReason,
    CatalogSweepInputs, SUMMARY_MAX_AGE,
};
use cview_core::utils::refresh_schedule::{RefreshContext, RefreshDemand};
use cview_core::utils::time::{interval_since, Date, TimeInterval};
use cview_networking::response::{AllLivesCollectionResult, AllLivesProgress};
use cview_networking::ChzzkApiClient;
use cview_networking::TraversalOptions;
use futures_util::future::{BoxFuture, FutureExt, Shared};
use uuid::Uuid;

use super::live_catalog_promotion::SweepReport;
use crate::services::support::{lock, system_clock, Clock};

pub type PauseCheck = Arc<dyn Fn() -> bool + Send + Sync>;
pub type ProgressFn = Arc<dyn Fn(AllLivesProgress) + Send + Sync>;

/// 전수 순회기. 운영은 API 클라이언트, 테스트는 가짜.
#[async_trait]
pub trait CatalogSweeping: Send + Sync {
    /// 처음부터 받는 순회 — `should_pause` 가 참이면 페이지 사이에서 `Paused` 로 끝난다.
    async fn sweep(
        &self,
        should_pause: PauseCheck,
        on_progress: ProgressFn,
    ) -> AllLivesCollectionResult;
}

/// 운영 순회기 — 재개 가능 순회(`live_catalog_traversal`)를 처음부터 부른다.
pub struct ApiCatalogSweeper {
    pub api_client: ChzzkApiClient,
    pub batch_size: i64,
}

impl ApiCatalogSweeper {
    pub fn new(api_client: ChzzkApiClient) -> Self {
        Self {
            api_client,
            batch_size: 50,
        }
    }
}

#[async_trait]
impl CatalogSweeping for ApiCatalogSweeper {
    async fn sweep(
        &self,
        should_pause: PauseCheck,
        on_progress: ProgressFn,
    ) -> AllLivesCollectionResult {
        let options = TraversalOptions {
            batch_size: self.batch_size,
            from: None,
            seed_live_ids: HashSet::new(),
            boundary_live_id: None,
        };
        self.api_client
            .live_catalog_traversal(
                options,
                move || {
                    let pause = should_pause();
                    async move { pause }
                },
                move |p| on_progress(p),
            )
            .await
    }
}

/// 카탈로그 수집을 요구하는 화면 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CatalogDemandKind {
    Category,
    Home,
    SearchLanding,
}

impl CatalogDemandKind {
    pub const ALL: [Self; 3] = [Self::Category, Self::Home, Self::SearchLanding];

    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Category => "category",
            Self::Home => "home",
            Self::SearchLanding => "searchLanding",
        }
    }

    /// 화면 수요 → 카탈로그 수요. `.live` 등은 수요가 아니다.
    pub fn from_demand(demand: RefreshDemand) -> Option<Self> {
        match demand {
            RefreshDemand::Category => Some(Self::Category),
            RefreshDemand::Home => Some(Self::Home),
            RefreshDemand::SearchLanding => Some(Self::SearchLanding),
            _ => None,
        }
    }

    /// 수요가 시작될 때 수집을 일으키는가 — 검색 랜딩은 인기 목록만 쓴다.
    pub fn starts_sweep(self) -> bool {
        self != Self::SearchLanding
    }
}

/// 수요 요구자 — 종류 + 마운트 인스턴스.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CatalogDemandHolder {
    pub kind: CatalogDemandKind,
    pub instance: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CatalogSweepReason {
    /// 코디네이터의 `catalogSweep` 작업.
    Scheduled,
    /// 화면이 나와 수요가 시작됐다.
    Demand,
    /// 사용자가 누른 새로고침·오류 재시도.
    Manual,
    /// 요약 파일은 있는데 행 파일이 없어 행을 다시 받는다.
    RowsMissing,
}

impl CatalogSweepReason {
    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Scheduled => "scheduled",
            Self::Demand => "demand",
            Self::Manual => "manual",
            Self::RowsMissing => "rowsMissing",
        }
    }
}

/// 수집 품질(원본은 `HomeViewModel.swift` 의 `StatsCollectionQuality`). 홈 뷰모델도 이 값을 쓴다.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StatsCollectionQuality {
    /// 아직 한 번도 시도하지 않음.
    #[default]
    Unknown,
    /// 수집 진행 중.
    Collecting,
    /// 정상 완료 — 커서 끝까지 도달.
    Complete { at: Date, count: i64 },
    /// 상한에 도달해 절단됨.
    Truncated { at: Date, count: i64 },
    /// 중간 페이지 실패로 부분 결과.
    Partial {
        at: Date,
        count: i64,
        message: String,
    },
    /// 완전 실패.
    Failed {
        message: String,
        has_previous_data: bool,
    },
}

impl StatsCollectionQuality {
    /// "일부만 표시 중" 배너가 필요한가.
    pub fn needs_partial_banner(&self) -> bool {
        matches!(self, Self::Truncated { .. } | Self::Partial { .. })
    }
    pub fn is_collecting(&self) -> bool {
        matches!(self, Self::Collecting)
    }
    pub fn is_complete(&self) -> bool {
        matches!(self, Self::Complete { .. })
    }
}

/// 수집 결과 처리 — 행 commit·영속화·품질 갱신은 호출자(VM)가 한다.
pub type ResultHandler = Arc<
    dyn Fn(
            AllLivesCollectionResult,
            StatsCollectionQuality,
            CatalogSweepReason,
        ) -> BoxFuture<'static, ()>
        + Send
        + Sync,
>;

/// 재생·네트워크 게이트(`noteGateChanged` 가 밀어 넣는 값).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActivityGate {
    pub is_playing: bool,
    pub is_offline: bool,
    pub reduce_resource_work: bool,
}

/// 관찰 상태 스냅샷(화면이 읽는 값).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CatalogLoadState {
    pub is_loading: bool,
    pub progress: f64,
    pub collected_count: usize,
    pub estimated_total: Option<i64>,
    pub quality: StatsCollectionQuality,
    pub load_error: Option<String>,
}

type SharedSweep = Shared<BoxFuture<'static, ()>>;

struct State {
    load: CatalogLoadState,
    current_sweep_id: Option<u64>,
    last_sweep_id: u64,
    last_attempt_at: Option<Date>,
    started_sweep_count: usize,
    joined_request_count: usize,
    skipped_manual_count: usize,
    last_sweep_succeeded: bool,
    summary: Option<LiveCatalogSummary>,
    growth_rates: HashMap<String, f64>,
    summary_revision: u64,
    is_terminating: bool,
    activity_gate: ActivityGate,
    boot_settled_at: Option<Date>,
    last_retry_class: Option<CatalogRetryClass>,
    manual_skip_window: TimeInterval,
    app_build: String,
    on_sweep_result: Option<ResultHandler>,
    on_unscheduled_sweep_finished: Option<Arc<dyn Fn(bool) + Send + Sync>>,
    should_sweep_on_demand: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    sweep: Option<SharedSweep>,
    holders: HashSet<CatalogDemandHolder>,
    last_report: Option<SweepReport>,
}

/// 카탈로그 스토어 핸들. 복제는 같은 상태를 공유한다.
#[derive(Clone)]
pub struct LiveCatalogStore {
    state: Arc<Mutex<State>>,
    sweeper: Arc<dyn CatalogSweeping>,
    clock: Clock,
}

impl LiveCatalogStore {
    /// 요약을 첫 그림에 쓸 수 있는 최대 나이(1일).
    pub const SUMMARY_FIRST_PAINT_MAX_AGE: TimeInterval = SUMMARY_MAX_AGE;

    pub fn new(sweeper: Arc<dyn CatalogSweeping>) -> Self {
        Self::with_clock(sweeper, system_clock())
    }

    pub fn with_clock(sweeper: Arc<dyn CatalogSweeping>, clock: Clock) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                load: CatalogLoadState::default(),
                current_sweep_id: None,
                last_sweep_id: 0,
                last_attempt_at: None,
                started_sweep_count: 0,
                joined_request_count: 0,
                skipped_manual_count: 0,
                last_sweep_succeeded: true,
                summary: None,
                growth_rates: HashMap::new(),
                summary_revision: 0,
                is_terminating: false,
                activity_gate: ActivityGate::default(),
                boot_settled_at: None,
                last_retry_class: None,
                manual_skip_window: 0.0,
                app_build: "0".into(),
                on_sweep_result: None,
                on_unscheduled_sweep_finished: None,
                should_sweep_on_demand: None,
                sweep: None,
                holders: HashSet::new(),
                last_report: None,
            })),
            sweeper,
            clock,
        }
    }

    // MARK: 설정

    pub fn set_on_sweep_result(&self, handler: Option<ResultHandler>) {
        lock(&self.state).on_sweep_result = handler;
    }

    /// 예약이 아닌 수집이 끝났을 때 — 코디네이터가 "방금 돌았다" 로 친다.
    pub fn set_on_unscheduled_sweep_finished(&self, f: Option<Arc<dyn Fn(bool) + Send + Sync>>) {
        lock(&self.state).on_unscheduled_sweep_finished = f;
    }

    /// 수요가 시작될 때 지금 새로 받아야 하는지. 미주입이면 항상 받는다.
    pub fn set_should_sweep_on_demand(&self, f: Option<Arc<dyn Fn() -> bool + Send + Sync>>) {
        lock(&self.state).should_sweep_on_demand = f;
    }

    /// 수동 요청을 건너뛰는 창(초). 0 이면 끈다(앱이 60 을 넣는다).
    pub fn set_manual_skip_window(&self, window: TimeInterval) {
        lock(&self.state).manual_skip_window = window;
    }

    pub fn manual_skip_window(&self) -> TimeInterval {
        lock(&self.state).manual_skip_window
    }

    pub fn set_app_build(&self, build: impl Into<String>) {
        lock(&self.state).app_build = build.into();
    }

    pub fn set_terminating(&self, terminating: bool) {
        lock(&self.state).is_terminating = terminating;
    }

    // MARK: 읽기

    pub fn load_state(&self) -> CatalogLoadState {
        lock(&self.state).load.clone()
    }

    /// 미리보기·테스트용 품질 setter.
    pub fn set_quality(&self, quality: StatsCollectionQuality) {
        lock(&self.state).load.quality = quality;
    }

    pub fn is_sweeping(&self) -> bool {
        lock(&self.state).sweep.is_some()
    }

    pub fn current_sweep_id(&self) -> Option<u64> {
        lock(&self.state).current_sweep_id
    }

    pub fn started_sweep_count(&self) -> usize {
        lock(&self.state).started_sweep_count
    }

    pub fn joined_request_count(&self) -> usize {
        lock(&self.state).joined_request_count
    }

    pub fn skipped_manual_count(&self) -> usize {
        lock(&self.state).skipped_manual_count
    }

    pub fn last_sweep_succeeded(&self) -> bool {
        lock(&self.state).last_sweep_succeeded
    }

    pub fn last_attempt_at(&self) -> Option<Date> {
        lock(&self.state).last_attempt_at
    }

    pub fn last_retry_class(&self) -> Option<CatalogRetryClass> {
        lock(&self.state).last_retry_class
    }

    pub fn last_report(&self) -> Option<SweepReport> {
        lock(&self.state).last_report.clone()
    }

    pub fn boot_settled_at(&self) -> Option<Date> {
        lock(&self.state).boot_settled_at
    }

    pub fn demand_kinds(&self) -> HashSet<CatalogDemandKind> {
        lock(&self.state).holders.iter().map(|h| h.kind).collect()
    }

    pub fn has_demand(&self) -> bool {
        !lock(&self.state).holders.is_empty()
    }

    pub fn summary(&self) -> Option<LiveCatalogSummary> {
        lock(&self.state).summary.clone()
    }

    pub fn growth_rates(&self) -> HashMap<String, f64> {
        lock(&self.state).growth_rates.clone()
    }

    pub fn summary_revision(&self) -> u64 {
        lock(&self.state).summary_revision
    }

    /// 첫 그림에 쓸 요약 — 1일을 넘겼으면 `None`. 미래 시각(시계 되돌림)은 낡지 않은 것으로 본다.
    pub fn usable_summary(&self, now: Option<Date>) -> Option<LiveCatalogSummary> {
        let now = now.unwrap_or_else(|| (self.clock)());
        let state = lock(&self.state);
        let summary = state.summary.as_ref()?;
        (interval_since(&now, &summary.completed_at) < Self::SUMMARY_FIRST_PAINT_MAX_AGE)
            .then(|| summary.clone())
    }

    /// 요약·증가율·revision 을 한 번에 커밋한다.
    pub fn commit_summary(&self, built: LiveCatalogSummary) {
        let rates = built.growth_by_category();
        let mut state = lock(&self.state);
        state.summary = Some(built);
        if state.growth_rates != rates {
            state.growth_rates = rates;
        }
        state.summary_revision = state.summary_revision.wrapping_add(1);
    }

    // MARK: 수요

    /// 화면이 카탈로그를 요구한다(같은 요구자의 중복 호출은 한 번). 필요하면 수집을 일으킨다.
    pub fn retain_demand(&self, kind: CatalogDemandKind, instance: Uuid) {
        let should_check = {
            let mut state = lock(&self.state);
            if !state.holders.insert(CatalogDemandHolder { kind, instance }) {
                return;
            }
            if !kind.starts_sweep() {
                return;
            }
            state.should_sweep_on_demand.clone()
        };
        if should_check.is_none_or(|f| f()) {
            self.request_sweep(CatalogSweepReason::Demand);
        }
    }

    /// 수요를 놓는다. **진행 중인 수집은 취소하지 않는다.**
    pub fn release_demand(&self, kind: CatalogDemandKind, instance: Uuid) {
        lock(&self.state)
            .holders
            .remove(&CatalogDemandHolder { kind, instance });
    }

    // MARK: 요청

    /// 수집을 요청한다. 이미 돌고 있으면 합류한다. 그 요청을 맡은 `sweep_id` 를 돌려준다.
    /// tokio 런타임 안에서 불러야 한다.
    pub fn request_sweep(&self, reason: CatalogSweepReason) -> u64 {
        let now = (self.clock)();
        let mut state = lock(&self.state);
        if state.sweep.is_some() {
            if let Some(id) = state.current_sweep_id {
                state.joined_request_count += 1;
                return id;
            }
        }
        // 수동 규칙 — 마지막 정상 수집이 창 안이면 그 수집을 돌려준다.
        if reason == CatalogSweepReason::Manual
            && state.manual_skip_window > 0.0
            && state.last_sweep_succeeded
            && state.last_sweep_id > 0
        {
            if let Some(last) = state.last_attempt_at {
                let age = interval_since(&now, &last);
                if (0.0..state.manual_skip_window).contains(&age) {
                    state.skipped_manual_count += 1;
                    return state.last_sweep_id;
                }
            }
        }
        state.last_sweep_id += 1;
        state.started_sweep_count += 1;
        let id = state.last_sweep_id;
        state.current_sweep_id = Some(id);
        let previous_quality = state.load.quality.clone();
        state.load = CatalogLoadState {
            is_loading: true,
            progress: 0.0,
            collected_count: 0,
            estimated_total: None,
            quality: StatsCollectionQuality::Collecting,
            load_error: None,
        };
        let this = self.clone();
        let handle = tokio::spawn(async move {
            this.run_sweep(id, reason, previous_quality).await;
        });
        let shared: SharedSweep = async move {
            let _ = handle.await;
        }
        .boxed()
        .shared();
        state.sweep = Some(shared);
        id
    }

    /// 지금 도는 수집이 끝날 때까지 기다린다(없으면 즉시). 기다리는 쪽이 버려져도 수집은 계속된다.
    pub async fn await_current_sweep(&self) {
        let sweep = lock(&self.state).sweep.clone();
        if let Some(sweep) = sweep {
            sweep.await;
        }
    }

    /// 사용자가 누른 새로고침 — 진행 중이면 합류해 끝나길 기다린다.
    pub async fn run_manual(&self) {
        self.request_sweep(CatalogSweepReason::Manual);
        self.await_current_sweep().await;
    }

    /// 진행률 — 단조 증가시킨다. `offset` 은 이어받은 수집에서 이미 가진 행 수다.
    pub fn record_progress(&self, progress: AllLivesProgress, offset: usize) {
        let mut state = lock(&self.state);
        let current = progress.current_count + offset;
        state.load.collected_count = state.load.collected_count.max(current);
        if let Some(total) = progress.estimated_total.filter(|t| *t > 0) {
            state.load.estimated_total = Some(total);
            let ratio = (current as f64 / total as f64).min(1.0);
            state.load.progress = state.load.progress.max(ratio);
        }
    }

    async fn run_sweep(
        &self,
        id: u64,
        reason: CatalogSweepReason,
        previous_quality: StatsCollectionQuality,
    ) {
        let started_at = (self.clock)();
        let pause_store = self.clone();
        let progress_store = self.clone();
        let result = self
            .sweeper
            .sweep(
                Arc::new(move || pause_store.should_pause(reason)),
                Arc::new(move |p| progress_store.record_progress(p, 0)),
            )
            .await;
        let handler = lock(&self.state).on_sweep_result.clone();
        let report = SweepReport::new(&result, 0, false);
        if let Some(handler) = handler {
            handler(result, previous_quality.clone(), reason).await;
        }
        let finished_at = (self.clock)();
        let unscheduled = {
            let mut state = lock(&self.state);
            // 처리기가 없거나 결과를 채택하지 않아 수집 중 표식이 남았으면 이전 품질로 되돌린다.
            if state.load.quality.is_collecting() {
                state.load.quality = previous_quality;
            }
            state.last_attempt_at = Some(finished_at);
            state.last_sweep_succeeded = report.completed_normally;
            state.last_retry_class = report.retry_class;
            state.load.is_loading = false;
            state.sweep = None;
            state.current_sweep_id = None;
            state.last_report = Some(report.clone());
            (reason != CatalogSweepReason::Scheduled)
                .then(|| state.on_unscheduled_sweep_finished.clone())
                .flatten()
                .map(|f| (f, report.completed_normally))
        };
        if let Some((f, ok)) = unscheduled {
            f(ok);
        }
        tracing::debug!(
            sweep_id = id,
            trigger = reason.raw_value(),
            outcome = %report.outcome,
            pages = report.pages,
            rows = report.rows,
            ms = interval_since(&finished_at, &started_at) * 1000.0,
            reason = report.detail.as_deref().unwrap_or(""),
            "catalog_sweep"
        );
    }

    // MARK: 게이트·스케줄 (+Activity)

    /// 재생 상태가 바뀌었다 — 예약·수요 수집이 다음 페이지 경계에서 멈추게 한다.
    pub fn note_gate_changed(
        &self,
        is_playing: bool,
        is_offline: bool,
        reduce_resource_work: bool,
    ) {
        lock(&self.state).activity_gate = ActivityGate {
            is_playing,
            is_offline,
            reduce_resource_work,
        };
    }

    /// 스플래시가 끝났다 — 부팅 유예 20초의 시작. 두 번 불러도 처음 시각을 지킨다.
    pub fn mark_boot_settled(&self) {
        let now = (self.clock)();
        let mut state = lock(&self.state);
        if state.boot_settled_at.is_none() {
            state.boot_settled_at = Some(now);
        }
    }

    /// 예약 수집을 지금 돌릴지 정한다. 체크포인트는 이 포팅에 없으므로 늘 `None` 이다.
    pub fn scheduled_decision(&self, context: &RefreshContext) -> CatalogSweepDecision {
        if context.is_offline {
            return CatalogSweepDecision::Idle(CatalogSweepIdleReason::Offline);
        }
        if context.reduce_resource_work {
            return CatalogSweepDecision::Idle(CatalogSweepIdleReason::ResourcePressure);
        }
        let now = (self.clock)();
        let state = lock(&self.state);
        let attempt = match (state.last_retry_class, state.last_attempt_at) {
            (Some(retry_class), Some(at)) => Some(CatalogSweepAttempt { at, retry_class }),
            _ => None,
        };
        let mut inputs = CatalogSweepInputs::new(now, context.demands.clone());
        inputs.last_promoted_at = state.summary.as_ref().map(|s| s.completed_at);
        inputs.last_attempt = attempt;
        inputs.checkpoint = None;
        inputs.app_build = state.app_build.clone();
        inputs.is_playing = context.is_playing;
        inputs.is_suspended = context.is_suspended;
        inputs.is_low_power = context.is_low_power;
        inputs.on_battery = context.on_battery;
        inputs.boot_settled_at = state.boot_settled_at;
        inputs.has_summary = state.summary.is_some();
        inputs.auto_refresh_enabled = context.auto_refresh_enabled;
        catalog_sweep::decide(&inputs)
    }

    /// 재생 중(예약·요약이 있는 수요 수집)이거나 종료 중이면 멈춘다. 수동 수집과 첫 채움은 끝까지 받는다.
    pub fn should_pause(&self, reason: CatalogSweepReason) -> bool {
        let state = lock(&self.state);
        let gate = state.activity_gate;
        if state.is_terminating || gate.is_offline {
            return true;
        }
        if gate.reduce_resource_work
            && matches!(
                reason,
                CatalogSweepReason::Scheduled | CatalogSweepReason::Demand
            )
        {
            return true;
        }
        if !gate.is_playing {
            return false;
        }
        match reason {
            CatalogSweepReason::Scheduled => true,
            CatalogSweepReason::Demand => state.summary.is_some(),
            CatalogSweepReason::Manual | CatalogSweepReason::RowsMissing => false,
        }
    }
}

#[cfg(test)]
mod tests;
