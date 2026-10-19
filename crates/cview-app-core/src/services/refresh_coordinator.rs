//! `Sources/CViewApp/Services/RefreshCoordinator.swift` 포팅 — 정기 작업의 **단일 소유자**.
//!
//! 컨텍스트(수요·가림·재생·전원·슬립·로그인·설정) → [`RefreshSchedule`](간격·게이트·동시 상한, 순수)
//! → 작업 실행.
//!
//! - 루프는 하나다. 기한이 된 작업을 띄우고, `next_wake` 까지 자거나 poke(컨텍스트·수요 변화,
//!   작업 종료)가 올 때까지 기다린다. 잠은 주입한 [`Sleep`] 하나이고 시계는 주입한 uptime 이다.
//! - 화면을 오가는 것(retain/release)은 `last_run` 을 바꾸지 않는다.
//! - 등록(`register`)과 로그아웃(`reset_for_logout`) 뒤에는 방금 돈 것으로 친다.
//!
//! 원본은 작업을 `RequestOrigin.backgroundPoll` 로 감싸 요청 장부에 태그한다. Rust 네트워킹 크레이트에는
//! 요청 장부가 없어 그 태그는 생략했다.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, Mutex, Weak};

use cview_core::utils::refresh_schedule::{
    RefreshContext, RefreshDemand, RefreshJobId, RefreshSchedule,
};
use cview_core::utils::time::TimeInterval;
use futures_util::future::{BoxFuture, FutureExt, Shared};
use tokio::sync::{mpsc, Notify};
use tokio::task::AbortHandle;
use uuid::Uuid;

use super::support::{lock, system_uptime, Uptime};

/// 주입 가능한 잠(초). 취소는 감싼 task 를 abort 해서 한다.
pub type Sleep = Arc<dyn Fn(TimeInterval) -> BoxFuture<'static, ()> + Send + Sync>;
/// 작업 본문 — 성공 여부를 돌려준다.
pub type Job = Arc<dyn Fn() -> BoxFuture<'static, bool> + Send + Sync>;
/// 화면 수요가 잡히거나 놓일 때(같은 요구자의 중복 호출은 제외).
pub type DemandObserver = Arc<dyn Fn(RefreshDemand, Uuid, bool) + Send + Sync>;

type FlightFuture = Shared<BoxFuture<'static, bool>>;

/// 깨어난 뒤 네트워크가 돌아올 때까지 기다리는 시간(초).
pub const WAKE_SETTLE_DELAY: TimeInterval = 3.0;

/// 기본 잠 — tokio 타이머.
pub fn system_sleep() -> Sleep {
    Arc::new(|secs| tokio::time::sleep(std::time::Duration::from_secs_f64(secs.max(0.0))).boxed())
}

struct Flight {
    serial: u64,
    future: FlightFuture,
    abort: AbortHandle,
}

struct State {
    context: RefreshContext,
    schedule: RefreshSchedule,
    evaluation_count: u64,
    jobs: HashMap<RefreshJobId, Job>,
    flights: HashMap<RefreshJobId, Flight>,
    next_serial: u64,
    holders: HashSet<(RefreshDemand, Uuid)>,
    loop_task: Option<AbortHandle>,
    wake_settle_task: Option<AbortHandle>,
    demand_observer: Option<DemandObserver>,
    tick_streams: Vec<(RefreshJobId, mpsc::Sender<()>)>,
}

struct Inner {
    state: Mutex<State>,
    notify: Arc<Notify>,
    sleep: Sleep,
    uptime: Uptime,
}

/// 정기 작업 코디네이터. `Clone` 은 같은 코디네이터를 가리킨다.
#[derive(Clone)]
pub struct RefreshCoordinator {
    inner: Arc<Inner>,
}

impl Default for RefreshCoordinator {
    fn default() -> Self {
        Self::new(system_sleep(), system_uptime())
    }
}

impl RefreshCoordinator {
    /// 로그인 전에는 팔로잉이 돌지 않는다 — 앱이 로그인 뒤 컨텍스트를 올린다.
    pub fn new(sleep: Sleep, uptime: Uptime) -> Self {
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State {
                    context: RefreshContext {
                        is_logged_in: false,
                        ..Default::default()
                    },
                    schedule: RefreshSchedule::new(),
                    evaluation_count: 0,
                    jobs: HashMap::new(),
                    flights: HashMap::new(),
                    next_serial: 0,
                    holders: HashSet::new(),
                    loop_task: None,
                    wake_settle_task: None,
                    demand_observer: None,
                    tick_streams: Vec::new(),
                }),
                notify: Arc::new(Notify::new()),
                sleep,
                uptime,
            }),
        }
    }

    // MARK: 조회

    pub fn context(&self) -> RefreshContext {
        lock(&self.inner.state).context.clone()
    }

    pub fn schedule(&self) -> RefreshSchedule {
        lock(&self.inner.state).schedule.clone()
    }

    /// 루프가 판정한 횟수 — 테스트가 깨어났는지 확인한다.
    pub fn evaluation_count(&self) -> u64 {
        lock(&self.inner.state).evaluation_count
    }

    pub fn is_running(&self) -> bool {
        lock(&self.inner.state).loop_task.is_some()
    }

    pub fn is_in_flight(&self, job: RefreshJobId) -> bool {
        lock(&self.inner.state).flights.contains_key(&job)
    }

    pub fn set_demand_observer(&self, observer: Option<DemandObserver>) {
        lock(&self.inner.state).demand_observer = observer;
    }

    // MARK: 등록·수명

    /// 작업 본문을 건다. 등록은 방금 돈 것으로 친다.
    pub fn register<F, Fut>(&self, job: RefreshJobId, run: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = bool> + Send + 'static,
    {
        let run: Job = Arc::new(move || run().boxed());
        let mut st = lock(&self.inner.state);
        st.jobs.insert(job, run);
        self.seed_last_run(&mut st, job);
    }

    /// 루프를 시작한다(idempotent). tokio 런타임 안에서 부른다.
    pub fn start(&self) {
        let mut st = lock(&self.inner.state);
        if st.loop_task.is_some() {
            return;
        }
        let weak = Arc::downgrade(&self.inner);
        let notify = self.inner.notify.clone();
        let sleep = self.inner.sleep.clone();
        let handle = tokio::spawn(run_loop(weak, notify, sleep));
        st.loop_task = Some(handle.abort_handle());
    }

    /// 루프를 멈추고 진행 중인 작업을 취소한다. 등록·수요는 남는다.
    pub fn stop(&self) {
        let mut st = lock(&self.inner.state);
        if let Some(t) = st.loop_task.take() {
            t.abort();
        }
        if let Some(t) = st.wake_settle_task.take() {
            t.abort();
        }
        Self::cancel_flights(&mut st);
        drop(st);
        self.wake();
    }

    // MARK: 수요

    /// 화면이 데이터를 요구한다. 같은 (수요, 인스턴스)의 중복 호출은 한 번으로 센다.
    pub fn retain(&self, demand: RefreshDemand, instance: Uuid) {
        let observer = {
            let mut st = lock(&self.inner.state);
            if !st.holders.insert((demand, instance)) {
                return;
            }
            st.schedule.retain(demand);
            self.demands_changed(&mut st);
            st.demand_observer.clone()
        };
        if let Some(f) = observer {
            f(demand, instance, true);
        }
    }

    pub fn release(&self, demand: RefreshDemand, instance: Uuid) {
        let observer = {
            let mut st = lock(&self.inner.state);
            if !st.holders.remove(&(demand, instance)) {
                return;
            }
            st.schedule.release(demand);
            self.demands_changed(&mut st);
            st.demand_observer.clone()
        };
        if let Some(f) = observer {
            f(demand, instance, false);
        }
    }

    fn demands_changed(&self, st: &mut State) {
        let demands = st.schedule.retained_demands();
        if st.context.demands == demands {
            return;
        }
        st.context.demands = demands;
        self.wake();
    }

    // MARK: 컨텍스트

    /// 수요를 뺀 컨텍스트를 갱신한다(수요는 retain/release 가 소유). 값이 바뀌었을 때만 깨운다.
    pub fn update(&self, new: RefreshContext) {
        let mut st = lock(&self.inner.state);
        let mut next = new;
        next.demands = st.schedule.retained_demands();
        if next == st.context {
            return;
        }
        // 로그인으로 넘어가는 순간은 부팅·로그인 폴이 방금 끝난 뒤다.
        if next.is_logged_in && !st.context.is_logged_in {
            let jobs: Vec<_> = st.jobs.keys().copied().collect();
            for job in jobs {
                self.seed_last_run(&mut st, job);
            }
        }
        if next.is_offline && !st.context.is_offline {
            Self::cancel_network_flights(&mut st);
        }
        st.context = next;
        drop(st);
        self.wake();
    }

    /// 시스템이 잠든다 — 깨어난 뒤 대기 중이던 재개는 취소한다.
    pub fn note_system_will_sleep(&self) {
        if let Some(t) = lock(&self.inner.state).wake_settle_task.take() {
            t.abort();
        }
        self.set_asleep(true);
    }

    /// 시스템이 깼다 — 네트워크가 돌아올 시간(`WAKE_SETTLE_DELAY`)을 준 뒤 재개한다.
    pub fn note_system_did_wake(&self) {
        let mut st = lock(&self.inner.state);
        if let Some(t) = st.wake_settle_task.take() {
            t.abort();
        }
        let sleep = self.inner.sleep.clone();
        let weak = Arc::downgrade(&self.inner);
        let handle = tokio::spawn(async move {
            sleep(WAKE_SETTLE_DELAY).await;
            if let Some(inner) = weak.upgrade() {
                RefreshCoordinator { inner }.set_asleep(false);
            }
        });
        st.wake_settle_task = Some(handle.abort_handle());
    }

    fn set_asleep(&self, asleep: bool) {
        {
            let mut st = lock(&self.inner.state);
            if st.context.is_asleep == asleep {
                return;
            }
            st.context.is_asleep = asleep;
        }
        self.wake();
    }

    // MARK: 실행

    /// 지금 돌린다 — 이미 도는 작업에는 합류한다(단일 비행). 지금 돌 수 없는 작업은 건너뛴다.
    pub async fn run_now(&self, ids: &[RefreshJobId]) {
        let futures: Vec<FlightFuture> = {
            let mut st = lock(&self.inner.state);
            let mut out = Vec::new();
            for &id in ids {
                if let Some(flight) = st.flights.get(&id) {
                    out.push(flight.future.clone());
                } else if RefreshSchedule::interval(id, &st.context).is_some() {
                    if let Some(f) = launch(&self.inner, &mut st, id) {
                        out.push(f);
                    }
                }
            }
            out
        };
        for f in futures {
            f.await;
        }
    }

    /// 코디네이터 밖에서 끝난 실행을 알린다 — 마지막 실행 시각이 앞당겨져 다음 정기 실행이 밀린다.
    pub fn note_completed(&self, job: RefreshJobId, success: bool) {
        {
            let mut st = lock(&self.inner.state);
            if st.flights.contains_key(&job)
                || (st.context.is_offline && job != RefreshJobId::Insights)
            {
                return;
            }
            let now = (self.inner.uptime)();
            st.schedule.mark_started(job, now);
            st.schedule.mark_finished(job, success);
        }
        self.wake();
    }

    /// 작업이 시작될 때마다 한 번 값을 내는 스트림(최신 1개만 버퍼).
    pub fn ticks(&self, job: RefreshJobId) -> mpsc::Receiver<()> {
        let (tx, rx) = mpsc::channel(1);
        lock(&self.inner.state).tick_streams.push((job, tx));
        rx
    }

    /// 계정 경계 — 진행 중인 작업을 취소하고 실행 기록·실패 이력을 비운다. 화면 수요는 남는다.
    pub fn reset_for_logout(&self) {
        {
            let mut st = lock(&self.inner.state);
            Self::cancel_flights(&mut st);
            let mut fresh = RefreshSchedule::new();
            let demands: Vec<RefreshDemand> = st.holders.iter().map(|(d, _)| *d).collect();
            for d in demands {
                fresh.retain(d);
            }
            st.schedule = fresh;
            let jobs: Vec<_> = st.jobs.keys().copied().collect();
            for job in jobs {
                self.seed_last_run(&mut st, job);
            }
            st.context.is_logged_in = false;
        }
        self.wake();
    }

    /// 기한이 된 작업을 띄운다.
    pub fn evaluate(&self) {
        evaluate(&self.inner);
    }

    fn wake(&self) {
        self.inner.notify.notify_one();
    }

    fn cancel_network_flights(st: &mut State) {
        let jobs: Vec<_> = st
            .flights
            .keys()
            .copied()
            .filter(|j| *j != RefreshJobId::Insights)
            .collect();
        for job in jobs {
            if let Some(f) = st.flights.remove(&job) {
                f.abort.abort();
            }
            st.schedule.mark_finished(job, true);
        }
    }

    fn cancel_flights(st: &mut State) {
        for (job, flight) in std::mem::take(&mut st.flights) {
            flight.abort.abort();
            st.schedule.mark_finished(job, true);
        }
    }

    fn seed_last_run(&self, st: &mut State, job: RefreshJobId) {
        if st.flights.contains_key(&job) || (st.context.is_offline && job != RefreshJobId::Insights)
        {
            return;
        }
        let now = (self.inner.uptime)();
        st.schedule.mark_started(job, now);
        st.schedule.mark_finished(job, true);
    }
}

fn evaluate(inner: &Arc<Inner>) {
    let mut st = lock(&inner.state);
    let now = (inner.uptime)();
    let due = st.schedule.due(now, &st.context);
    for job in due {
        launch(inner, &mut st, job);
    }
}

async fn run_loop(weak: Weak<Inner>, notify: Arc<Notify>, sleep: Sleep) {
    loop {
        let wait = {
            let Some(inner) = weak.upgrade() else { return };
            lock(&inner.state).evaluation_count += 1;
            evaluate(&inner);
            let st = lock(&inner.state);
            st.schedule.next_wake((inner.uptime)(), &st.context)
        };
        // 대기 중 poke 가 왔으면 notify 에 허가가 남아 있어 즉시 돌아온다.
        match wait {
            Some(secs) => {
                tokio::select! {
                    _ = sleep(secs.max(0.0)) => {}
                    _ = notify.notified() => {}
                }
            }
            None => notify.notified().await,
        }
    }
}

fn launch(inner: &Arc<Inner>, st: &mut State, job: RefreshJobId) -> Option<FlightFuture> {
    if let Some(flight) = st.flights.get(&job) {
        return Some(flight.future.clone());
    }
    emit_tick(st, job);
    let now = (inner.uptime)();
    let Some(run) = st.jobs.get(&job).cloned() else {
        // 본문이 없는 작업은 간격만 소비한다.
        st.schedule.mark_started(job, now);
        st.schedule.mark_finished(job, true);
        return None;
    };
    st.schedule.mark_started(job, now);
    st.next_serial += 1;
    let serial = st.next_serial;
    let weak = Arc::downgrade(inner);
    let handle = tokio::spawn(async move {
        let success = run().await;
        if let Some(inner) = weak.upgrade() {
            finish(&inner, job, serial, success);
        }
        success
    });
    let abort = handle.abort_handle();
    let future = async move { handle.await.unwrap_or(false) }
        .boxed()
        .shared();
    st.flights.insert(
        job,
        Flight {
            serial,
            future: future.clone(),
            abort,
        },
    );
    Some(future)
}

fn finish(inner: &Arc<Inner>, job: RefreshJobId, serial: u64, success: bool) {
    {
        let mut st = lock(&inner.state);
        // 계정 경계·정지로 버려진 옛 비행의 늦은 종료는 새 상태를 건드리지 않는다.
        if st.flights.get(&job).map(|f| f.serial) != Some(serial) {
            return;
        }
        st.flights.remove(&job);
        st.schedule.mark_finished(job, success);
    }
    inner.notify.notify_one();
}

fn emit_tick(st: &mut State, job: RefreshJobId) {
    st.tick_streams.retain(|(_, tx)| !tx.is_closed());
    for (j, tx) in &st.tick_streams {
        if *j == job {
            let _ = tx.try_send(());
        }
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/RefreshCoordinatorTests.swift`(코디네이터 동작 스위트).
    use super::*;
    use crate::services::support::ManualClock;
    use cview_core::utils::time::from_unix;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::watch;

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
}
