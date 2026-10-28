//! 원본: `Tests/CViewAppTests/RefreshCoordinatorTests.swift`(코디네이터 동작 스위트).
use super::*;
use crate::services::support::ManualClock;
use cview_core::utils::time::from_unix;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::watch;

#[tokio::test]
async fn demand_sink_maps_u64_instances_onto_coordinator_holders() {
    let c = RefreshCoordinator::default();
    let sink: &dyn RefreshDemandSink = &c;
    sink.retain_demand(RefreshDemand::Clips, 7);
    sink.retain_demand(RefreshDemand::Clips, 7);
    assert!(c.context().demands.contains(&RefreshDemand::Clips));
    sink.release_demand(RefreshDemand::Clips, 8);
    assert!(c.context().demands.contains(&RefreshDemand::Clips));
    sink.release_demand(RefreshDemand::Clips, 7);
    assert!(!c.context().demands.contains(&RefreshDemand::Clips));
}

/// 수동 sleeper — 길이를 기록하고 `fire()` 가 오기 전까지 잔다.
#[derive(Clone)]
struct ManualSleeper {
    log: Arc<Mutex<Vec<f64>>>,
    gen: Arc<watch::Sender<u64>>,
}

impl ManualSleeper {
    fn new() -> Self {
        Self {
            log: Arc::new(Mutex::new(Vec::new())),
            gen: Arc::new(watch::channel(0).0),
        }
    }
    fn requests(&self) -> Vec<f64> {
        lock(&self.log).clone()
    }
    fn last(&self) -> Option<f64> {
        self.requests().last().copied()
    }
    fn fire(&self) {
        self.gen.send_modify(|g| *g += 1);
    }
    fn sleep(&self) -> Sleep {
        let me = self.clone();
        Arc::new(move |secs| {
            lock(&me.log).push(secs);
            let mut rx = me.gen.subscribe();
            async move {
                let _ = rx.changed().await;
            }
            .boxed()
        })
    }
}

struct Fixture {
    clock: ManualClock,
    sleeper: ManualSleeper,
    coordinator: RefreshCoordinator,
}

fn fixture() -> Fixture {
    let clock = ManualClock::new(from_unix(1_000.0));
    let sleeper = ManualSleeper::new();
    let coordinator = RefreshCoordinator::new(sleeper.sleep(), clock.uptime_fn());
    Fixture {
        clock,
        sleeper,
        coordinator,
    }
}

fn counter(c: &RefreshCoordinator, job: RefreshJobId, success: bool) -> Arc<AtomicUsize> {
    let count = Arc::new(AtomicUsize::new(0));
    let n = count.clone();
    c.register(job, move || {
        n.fetch_add(1, Ordering::SeqCst);
        async move { success }
    });
    count
}

async fn settle(mut cond: impl FnMut() -> bool) {
    for _ in 0..5_000 {
        if cond() {
            return;
        }
        tokio::task::yield_now().await;
    }
}

async fn settle_briefly() {
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
}

fn base(mutate: impl FnOnce(&mut RefreshContext)) -> RefreshContext {
    let mut ctx = RefreshContext {
        is_logged_in: true,
        auto_refresh_enabled: true,
        auto_refresh_interval: 300.0,
        ..Default::default()
    };
    mutate(&mut ctx);
    ctx
}

#[tokio::test]
async fn poke_wakes_the_loop() {
    let f = fixture();
    let runs = counter(&f.coordinator, RefreshJobId::Following, true);
    f.coordinator
        .retain(RefreshDemand::LiveOverview, Uuid::new_v4());
    f.coordinator
        .update(base(|c| c.auto_refresh_interval = 60.0));
    f.coordinator.start();
    settle(|| !f.sleeper.requests().is_empty()).await;
    assert_eq!(f.sleeper.last(), Some(60.0), "등록은 방금 돈 것으로 친다");
    assert_eq!(runs.load(Ordering::SeqCst), 0);

    f.clock.advance(60.0);
    f.sleeper.fire();
    settle(|| runs.load(Ordering::SeqCst) == 1).await;
    assert_eq!(runs.load(Ordering::SeqCst), 1);

    settle(|| f.sleeper.requests().len() >= 2).await;
    let parked = f.sleeper.requests().len();
    f.coordinator
        .update(base(|c| c.auto_refresh_interval = 30.0));
    settle(|| f.sleeper.requests().len() > parked && f.sleeper.last() == Some(30.0)).await;
    assert_eq!(
        f.sleeper.last(),
        Some(30.0),
        "간격 변경이 잠 도중에 반영돼야 한다"
    );
    f.coordinator.stop();
}

#[tokio::test]
async fn run_now_is_single_flight() {
    let f = fixture();
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let starts = Arc::new(AtomicUsize::new(0));
    let (g, s) = (gate.clone(), starts.clone());
    f.coordinator.register(RefreshJobId::Following, move || {
        s.fetch_add(1, Ordering::SeqCst);
        let g = g.clone();
        async move {
            g.acquire().await.unwrap().forget();
            true
        }
    });
    f.coordinator.update(base(|_| {}));
    let c1 = f.coordinator.clone();
    let c2 = f.coordinator.clone();
    let first = tokio::spawn(async move { c1.run_now(&[RefreshJobId::Following]).await });
    let second = tokio::spawn(async move { c2.run_now(&[RefreshJobId::Following]).await });
    settle(|| starts.load(Ordering::SeqCst) == 1).await;
    settle_briefly().await;
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert!(f.coordinator.is_in_flight(RefreshJobId::Following));
    gate.add_permits(1);
    first.await.unwrap();
    second.await.unwrap();
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert!(!f.coordinator.is_in_flight(RefreshJobId::Following));
}

#[tokio::test]
async fn run_now_skips_gated_jobs() {
    let f = fixture();
    let runs = counter(&f.coordinator, RefreshJobId::Following, true);
    f.coordinator.update(base(|c| c.is_logged_in = false));
    f.coordinator.run_now(&[RefreshJobId::Following]).await;
    assert_eq!(runs.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn note_completed_defers_next_run() {
    let f = fixture();
    let runs = counter(&f.coordinator, RefreshJobId::Following, true);
    f.coordinator
        .retain(RefreshDemand::LiveOverview, Uuid::new_v4());
    f.coordinator
        .update(base(|c| c.auto_refresh_interval = 60.0));
    f.coordinator.start();
    settle(|| !f.sleeper.requests().is_empty()).await;

    f.clock.advance(50.0);
    f.coordinator.note_completed(RefreshJobId::Following, true);
    settle(|| f.sleeper.requests().len() >= 2).await;
    assert_eq!(f.sleeper.last(), Some(60.0));

    f.clock.advance(11.0);
    f.sleeper.fire();
    settle(|| f.sleeper.requests().len() >= 3).await;
    assert_eq!(runs.load(Ordering::SeqCst), 0);
    assert_eq!(f.sleeper.last(), Some(49.0));
    f.coordinator.stop();
}

#[tokio::test]
async fn failed_run_backs_off() {
    let f = fixture();
    let runs = counter(&f.coordinator, RefreshJobId::Following, false);
    f.coordinator
        .retain(RefreshDemand::LiveOverview, Uuid::new_v4());
    f.coordinator.update(base(|_| {}));
    f.coordinator.start();
    settle(|| !f.sleeper.requests().is_empty()).await;
    assert_eq!(f.sleeper.last(), Some(180.0), "보이는 팔로잉은 180초");

    f.clock.advance(180.0);
    f.sleeper.fire();
    settle(|| runs.load(Ordering::SeqCst) == 1).await;
    settle(|| f.sleeper.requests().len() >= 2).await;
    assert_eq!(f.sleeper.last(), Some(30.0), "첫 실패 뒤 재시도는 30초");
    f.coordinator.stop();
}

#[tokio::test]
async fn sweep_is_zero_when_setting_off_and_only_home_visible() {
    let f = fixture();
    let sweep = counter(&f.coordinator, RefreshJobId::CatalogSweep, true);
    f.coordinator.retain(RefreshDemand::Home, Uuid::new_v4());
    f.coordinator
        .update(base(|c| c.auto_refresh_enabled = false));
    f.clock.advance(100_000.0);
    f.coordinator.start();
    settle(|| f.coordinator.evaluation_count() >= 1).await;
    settle_briefly().await;
    assert_eq!(sweep.load(Ordering::SeqCst), 0);

    f.coordinator
        .retain(RefreshDemand::Category, Uuid::new_v4());
    settle(|| sweep.load(Ordering::SeqCst) == 1).await;
    assert_eq!(sweep.load(Ordering::SeqCst), 1);
    f.coordinator.stop();
}

#[tokio::test]
async fn logout_cancels_flights_and_keeps_demands() {
    let f = fixture();
    let started = Arc::new(AtomicUsize::new(0));
    let s = started.clone();
    f.coordinator.register(RefreshJobId::Following, move || {
        s.fetch_add(1, Ordering::SeqCst);
        futures_util::future::pending::<bool>()
    });
    f.coordinator.retain(RefreshDemand::Home, Uuid::new_v4());
    f.coordinator.update(base(|_| {}));
    let c = f.coordinator.clone();
    let running = tokio::spawn(async move { c.run_now(&[RefreshJobId::Following]).await });
    settle(|| started.load(Ordering::SeqCst) == 1).await;
    assert!(f.coordinator.is_in_flight(RefreshJobId::Following));

    f.coordinator.reset_for_logout();
    assert!(!f.coordinator.is_in_flight(RefreshJobId::Following));
    assert!(!f.coordinator.context().is_logged_in);
    assert_eq!(
        f.coordinator.context().demands,
        HashSet::from([RefreshDemand::Home])
    );
    running.await.unwrap();
    assert!(!f
        .coordinator
        .schedule()
        .running()
        .contains(&RefreshJobId::Following));
}

#[tokio::test]
async fn demands_are_counted_per_instance() {
    let f = fixture();
    let (mount, other) = (Uuid::new_v4(), Uuid::new_v4());
    let c = &f.coordinator;
    c.retain(RefreshDemand::Home, mount);
    c.retain(RefreshDemand::Home, mount);
    c.retain(RefreshDemand::Home, other);
    c.release(RefreshDemand::Home, mount);
    assert_eq!(c.context().demands, HashSet::from([RefreshDemand::Home]));
    c.release(RefreshDemand::Home, mount);
    assert_eq!(c.context().demands, HashSet::from([RefreshDemand::Home]));
    c.release(RefreshDemand::Home, other);
    assert!(c.context().demands.is_empty());
}

#[tokio::test]
async fn unchanged_update_does_not_poke() {
    let f = fixture();
    let _ = counter(&f.coordinator, RefreshJobId::Following, true);
    f.coordinator.retain(RefreshDemand::Home, Uuid::new_v4());
    let ctx = base(|_| {});
    f.coordinator.update(ctx.clone());
    f.coordinator.start();
    settle(|| f.coordinator.evaluation_count() >= 1).await;
    settle_briefly().await;
    let before = f.coordinator.evaluation_count();
    f.coordinator.update(ctx);
    settle_briefly().await;
    assert_eq!(f.coordinator.evaluation_count(), before);
    f.coordinator.stop();
}

#[tokio::test]
async fn login_transition_seeds_last_run() {
    let f = fixture();
    let following = counter(&f.coordinator, RefreshJobId::Following, true);
    f.coordinator
        .retain(RefreshDemand::LiveOverview, Uuid::new_v4());
    f.coordinator.update(base(|c| c.is_logged_in = false));
    f.clock.advance(100_000.0);
    f.coordinator.start();
    settle(|| f.coordinator.evaluation_count() >= 1).await;
    f.coordinator.update(base(|_| {}));
    settle(|| f.coordinator.evaluation_count() >= 2).await;
    settle_briefly().await;
    assert_eq!(following.load(Ordering::SeqCst), 0);
    f.coordinator.stop();
}

#[tokio::test]
async fn ticks_fire_for_unregistered_jobs() {
    let f = fixture();
    let mut ticks = f.coordinator.ticks(RefreshJobId::Insights);
    f.coordinator.start();
    f.coordinator
        .retain(RefreshDemand::Insights, Uuid::new_v4());
    f.coordinator.update(base(|_| {}));
    assert_eq!(ticks.recv().await, Some(()));
    settle(|| f.sleeper.last() == Some(30.0)).await;
    assert_eq!(
        f.sleeper.last(),
        Some(30.0),
        "틱 뒤 다음 기한은 인사이트 간격 30초"
    );
    f.coordinator.stop();
}

#[tokio::test]
async fn sleep_and_wake_settle() {
    let f = fixture();
    f.coordinator.update(base(|_| {}));
    f.coordinator.note_system_will_sleep();
    assert!(f.coordinator.context().is_asleep);
    f.coordinator.note_system_did_wake();
    settle(|| f.sleeper.requests().contains(&WAKE_SETTLE_DELAY)).await;
    assert!(
        f.coordinator.context().is_asleep,
        "깬 직후에는 아직 재개하지 않는다"
    );
    f.sleeper.fire();
    settle(|| !f.coordinator.context().is_asleep).await;
    assert!(!f.coordinator.context().is_asleep);
}

#[tokio::test]
async fn sleep_during_wake_settle_cancels_resume() {
    let f = fixture();
    f.coordinator.update(base(|_| {}));
    f.coordinator.note_system_will_sleep();
    f.coordinator.note_system_did_wake();
    settle(|| !f.sleeper.requests().is_empty()).await;
    f.coordinator.note_system_will_sleep();
    f.sleeper.fire();
    settle_briefly().await;
    assert!(f.coordinator.context().is_asleep);
}
