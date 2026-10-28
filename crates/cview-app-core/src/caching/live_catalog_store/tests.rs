//! 원본: `Tests/CViewAppTests/LiveCatalogStoreOwnershipTests.swift`,
//! `LiveCatalogStoreSchedulerTests.swift` (체크포인트·배선 소스 가드 제외).
use super::*;
use crate::caching::live_catalog_promotion::CatalogSweepDecisionExt;
use crate::services::support::ManualClock;
use cview_core::models::live_catalog::LiveCatalogQuality;
use cview_core::utils::catalog_sweep::{CatalogSweepRunReason, HOME_INTERVAL};
use cview_core::utils::time::{adding, from_unix};
use cview_networking::response::AllLivesStopReason;
use std::collections::VecDeque;
use tokio::sync::Semaphore;

fn result(stop: AllLivesStopReason) -> AllLivesCollectionResult {
    let verified = stop == AllLivesStopReason::ReachedEnd;
    AllLivesCollectionResult {
        items: Vec::new(),
        estimated_total: Some(0),
        requested_pages: 1,
        received_pages: 1,
        deduplicated_count: 0,
        stop_reason: stop,
        verified_observed_count: verified.then_some(0),
        stable_cursor_ordering_verified: verified,
        resume_point: None,
        first_live_id: None,
    }
}

/// 문을 열어 줘야 끝나는 가짜 순회기. 결과는 큐에서 꺼낸다(비면 검증 완료).
struct Fake {
    gated: bool,
    gate: Semaphore,
    started: Mutex<usize>,
    queue: Mutex<VecDeque<AllLivesCollectionResult>>,
}

impl Fake {
    fn new(gated: bool) -> Arc<Self> {
        Arc::new(Self {
            gated,
            gate: Semaphore::new(0),
            started: Mutex::new(0),
            queue: Mutex::new(VecDeque::new()),
        })
    }
    fn open(&self) {
        self.gate.add_permits(1000);
    }
    async fn wait_until_started(&self) {
        while *lock(&self.started) == 0 {
            tokio::task::yield_now().await;
        }
    }
}

#[async_trait]
impl CatalogSweeping for Fake {
    async fn sweep(&self, _: PauseCheck, on_progress: ProgressFn) -> AllLivesCollectionResult {
        *lock(&self.started) += 1;
        if self.gated {
            self.gate.acquire().await.unwrap().forget();
        }
        on_progress(AllLivesProgress {
            current_count: 5,
            estimated_total: Some(10),
            current_page: 1,
            deduplicated_count: 0,
        });
        lock(&self.queue)
            .pop_front()
            .unwrap_or_else(|| result(AllLivesStopReason::ReachedEnd))
    }
}

fn base() -> Date {
    from_unix(1_800_000_000.0)
}

fn store(fake: &Arc<Fake>, clock: &ManualClock) -> LiveCatalogStore {
    let sweeper: Arc<dyn CatalogSweeping> = fake.clone();
    let s = LiveCatalogStore::with_clock(sweeper, clock.clock());
    s.set_app_build("100");
    s
}

fn summary(completed_at: Date) -> LiveCatalogSummary {
    LiveCatalogSummary {
        version: LiveCatalogSummary::CURRENT_VERSION,
        sweep_id: Uuid::nil(),
        completed_at,
        quality: LiveCatalogQuality::Verified { exact: 1 },
        live_count: 1,
        exact_live_count: Some(1),
        viewer_total: 5,
        average_viewers: 5,
        median_viewers: 5,
        viewer_buckets: vec![1, 0, 0, 0],
        type_shares: Vec::new(),
        top_lives: Vec::new(),
        categories: Vec::new(),
    }
}

#[tokio::test]
async fn demand_changes_share_one_sweep() {
    let fake = Fake::new(true);
    let clock = ManualClock::new(base());
    let s = store(&fake, &clock);
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    s.retain_demand(CatalogDemandKind::Category, a);
    fake.wait_until_started().await;
    let id = s.current_sweep_id().unwrap();
    s.release_demand(CatalogDemandKind::Category, a);
    s.retain_demand(CatalogDemandKind::Home, b);
    assert_eq!(s.request_sweep(CatalogSweepReason::Manual), id);
    assert_eq!(s.joined_request_count(), 2);
    s.release_demand(CatalogDemandKind::Home, b);
    assert!(!s.has_demand());
    assert!(s.is_sweeping(), "수요를 놓았더니 수집이 취소됐다");
    fake.open();
    s.await_current_sweep().await;
    assert_eq!(s.started_sweep_count(), 1);
    assert!(!s.is_sweeping());
    let load = s.load_state();
    assert!(!load.is_loading);
    assert_eq!(load.collected_count, 5);
    assert!((load.progress - 0.5).abs() < 1e-9);
    assert_eq!(
        load.quality,
        StatsCollectionQuality::Unknown,
        "처리기가 없으면 이전 품질로 되돌린다"
    );
}

#[tokio::test]
async fn search_landing_and_refused_demand_do_not_sweep() {
    let fake = Fake::new(false);
    let clock = ManualClock::new(base());
    let s = store(&fake, &clock);
    s.retain_demand(CatalogDemandKind::SearchLanding, Uuid::new_v4());
    assert!(!s.is_sweeping());
    s.set_should_sweep_on_demand(Some(Arc::new(|| false)));
    s.retain_demand(CatalogDemandKind::Category, Uuid::new_v4());
    assert!(!s.is_sweeping());
    assert_eq!(s.started_sweep_count(), 0);
    assert_eq!(s.demand_kinds().len(), 2);

    // 같은 요구자의 중복 retain 은 한 번만 센다.
    s.set_should_sweep_on_demand(None);
    let same = Uuid::new_v4();
    s.retain_demand(CatalogDemandKind::Home, same);
    s.await_current_sweep().await;
    s.retain_demand(CatalogDemandKind::Home, same);
    assert_eq!(s.started_sweep_count(), 1);

    assert_eq!(
        CatalogDemandKind::from_demand(RefreshDemand::LiveOverview),
        None
    );
    assert_eq!(
        CatalogDemandKind::from_demand(RefreshDemand::Category),
        Some(CatalogDemandKind::Category)
    );
}

#[tokio::test]
async fn partial_failure_reports_failure_and_unscheduled_callback() {
    let fake = Fake::new(false);
    lock(&fake.queue).push_back(result(AllLivesStopReason::PartialFailure {
        page: 1,
        message: "x".into(),
    }));
    let clock = ManualClock::new(base());
    let s = store(&fake, &clock);
    let seen: Arc<Mutex<Vec<bool>>> = Arc::default();
    let sink = seen.clone();
    s.set_on_unscheduled_sweep_finished(Some(Arc::new(move |ok| lock(&sink).push(ok))));
    let handled: Arc<Mutex<Vec<CatalogSweepReason>>> = Arc::default();
    let h = handled.clone();
    s.set_on_sweep_result(Some(Arc::new(move |_, _, reason| {
        lock(&h).push(reason);
        async {}.boxed()
    })));
    s.request_sweep(CatalogSweepReason::Demand);
    s.await_current_sweep().await;
    assert!(!s.last_sweep_succeeded());
    assert_eq!(s.last_retry_class(), Some(CatalogRetryClass::Transient));
    s.request_sweep(CatalogSweepReason::Scheduled);
    s.await_current_sweep().await;
    assert!(s.last_sweep_succeeded());
    assert_eq!(*lock(&seen), vec![false], "예약 수집은 알리지 않는다");
    assert_eq!(
        *lock(&handled),
        vec![CatalogSweepReason::Demand, CatalogSweepReason::Scheduled]
    );
}

#[tokio::test]
async fn pause_gate_matrix() {
    let fake = Fake::new(false);
    let clock = ManualClock::new(base());
    let s = store(&fake, &clock);
    assert!(!s.should_pause(CatalogSweepReason::Scheduled));
    s.note_gate_changed(true, false, false);
    assert!(s.should_pause(CatalogSweepReason::Scheduled));
    assert!(
        !s.should_pause(CatalogSweepReason::Demand),
        "첫 채움이 재생 때문에 멈췄다"
    );
    assert!(!s.should_pause(CatalogSweepReason::Manual));
    assert!(!s.should_pause(CatalogSweepReason::RowsMissing));
    s.commit_summary(summary(base()));
    assert!(s.should_pause(CatalogSweepReason::Demand));
    s.note_gate_changed(false, false, false);
    assert!(!s.should_pause(CatalogSweepReason::Scheduled));
    s.set_terminating(true);
    assert!(s.should_pause(CatalogSweepReason::Manual));
    s.set_terminating(false);

    s.note_gate_changed(false, true, false);
    assert!(s.should_pause(CatalogSweepReason::Manual));
    let offline = RefreshContext {
        demands: [RefreshDemand::Category].into(),
        is_offline: true,
        ..RefreshContext::default()
    };
    assert_eq!(
        s.scheduled_decision(&offline),
        CatalogSweepDecision::Idle(CatalogSweepIdleReason::Offline)
    );
    s.note_gate_changed(false, false, true);
    assert!(s.should_pause(CatalogSweepReason::Demand));
    assert!(!s.should_pause(CatalogSweepReason::Manual));
}

fn ctx(demands: &[RefreshDemand]) -> RefreshContext {
    RefreshContext {
        demands: demands.iter().copied().collect(),
        ..RefreshContext::default()
    }
}

#[tokio::test]
async fn home_demand_waits_while_summary_is_fresh_and_boot_grace() {
    let fake = Fake::new(false);
    let clock = ManualClock::new(base());
    let s = store(&fake, &clock);
    s.commit_summary(summary(adding(&base(), -7_200.0)));
    assert_eq!(
        s.scheduled_decision(&ctx(&[RefreshDemand::Home])),
        CatalogSweepDecision::Idle(CatalogSweepIdleReason::BootUnsettled)
    );
    s.mark_boot_settled();
    clock.advance(19.0);
    assert_eq!(
        s.scheduled_decision(&ctx(&[RefreshDemand::Home])),
        CatalogSweepDecision::Wait {
            until: adding(&base(), 20.0)
        }
    );
    clock.advance(1.0);
    assert_eq!(
        s.scheduled_decision(&ctx(&[RefreshDemand::Home])),
        CatalogSweepDecision::Run(CatalogSweepRunReason::Interval)
    );
    s.mark_boot_settled();
    assert_eq!(s.boot_settled_at(), Some(base()));

    let fresh_at = clock.now();
    s.commit_summary(summary(fresh_at));
    clock.advance(600.0);
    let fresh = s.scheduled_decision(&ctx(&[RefreshDemand::Home]));
    assert!(!fresh.starts_sweep());
    assert_eq!(
        fresh,
        CatalogSweepDecision::Wait {
            until: adding(&fresh_at, HOME_INTERVAL)
        }
    );
    assert_eq!(
        s.scheduled_decision(&ctx(&[RefreshDemand::LiveOverview])),
        CatalogSweepDecision::Idle(CatalogSweepIdleReason::NoDemand)
    );
}

#[tokio::test]
async fn retry_cooldown_after_incomplete_sweep() {
    let fake = Fake::new(false);
    let clock = ManualClock::new(base());
    let s = store(&fake, &clock);
    s.mark_boot_settled();
    s.commit_summary(summary(adding(&base(), -7_200.0)));
    clock.advance(100.0);
    lock(&fake.queue).push_back(result(AllLivesStopReason::PartialFailure {
        page: 1,
        message: "x".into(),
    }));
    s.request_sweep(CatalogSweepReason::Scheduled);
    s.await_current_sweep().await;
    let at = s.last_attempt_at().unwrap();
    clock.advance(89.0);
    assert_eq!(
        s.scheduled_decision(&ctx(&[RefreshDemand::Category])),
        CatalogSweepDecision::Wait {
            until: adding(&at, 90.0)
        }
    );
    clock.advance(1.0);
    assert!(s
        .scheduled_decision(&ctx(&[RefreshDemand::Category]))
        .starts_sweep());
}

#[tokio::test]
async fn manual_request_skips_within_window() {
    let fake = Fake::new(false);
    let clock = ManualClock::new(base());
    let s = store(&fake, &clock);
    assert_eq!(s.manual_skip_window(), 0.0);
    s.set_manual_skip_window(catalog_sweep::MANUAL_SKIP_WINDOW);
    let first = s.request_sweep(CatalogSweepReason::Manual);
    s.await_current_sweep().await;
    clock.advance(30.0);
    assert_eq!(s.request_sweep(CatalogSweepReason::Manual), first);
    assert_eq!(s.started_sweep_count(), 1);
    assert_eq!(s.skipped_manual_count(), 1);
    assert!(!s.is_sweeping());
    s.request_sweep(CatalogSweepReason::Demand);
    s.await_current_sweep().await;
    assert_eq!(s.started_sweep_count(), 2);
    clock.advance(61.0);
    s.run_manual().await;
    assert_eq!(s.started_sweep_count(), 3);
}

#[test]
fn usable_summary_respects_one_day() {
    let clock = ManualClock::new(base());
    let s = store(&Fake::new(false), &clock);
    assert!(s.usable_summary(None).is_none());
    s.commit_summary(summary(base()));
    assert_eq!(s.summary_revision(), 1);
    assert!(s.usable_summary(Some(adding(&base(), 86_399.0))).is_some());
    assert!(s.usable_summary(Some(adding(&base(), 86_400.0))).is_none());
    assert!(
        s.usable_summary(Some(adding(&base(), -10.0))).is_some(),
        "미래 시각은 낡지 않다"
    );
}
