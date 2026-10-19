//! 동시 실행 상한을 지키는 백그라운드 작업 팬아웃.
//!
//! 원본: `Sources/CViewCore/Utilities/BoundedConcurrentWork.swift`
//!
//! Swift 는 `TaskGroup` 과 협조적 취소(`Task.isCancelled`)를 쓴다. Rust 에서는 tokio
//! [`JoinSet`] 으로 옮기고, 취소는 **반환된 future 를 drop** 하는 것으로 대신한다 —
//! drop 되면 아직 제출되지 않은 원소는 시작되지 않고, 실행 중인 작업은 `JoinSet` 이
//! abort 한다.

use std::future::Future;
use std::sync::Arc;

use tokio::task::JoinSet;

use super::system_load::SystemLoadMode;

/// `elements` 를 최대 `limit` 개(0 이하는 1로 본다)씩 동시에 `operation` 으로 처리한다.
///
/// 미완료 작업 수가 `limit` 를 넘지 않으며, 하나가 끝날 때마다 다음 원소를 제출한다.
/// 작업은 tokio 런타임 위에 spawn 되므로 호출 스레드(UI)를 점유하지 않는다.
/// 작업 하나가 패닉하면 그 패닉을 호출자에게 다시 던진다.
pub async fn for_each<T, F, Fut>(elements: impl IntoIterator<Item = T>, limit: usize, operation: F)
where
    T: Send + 'static,
    F: Fn(T) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let mut pending = elements.into_iter();
    let width = limit.max(1);
    let operation = Arc::new(operation);
    let mut group = JoinSet::new();

    let mut submit = |group: &mut JoinSet<()>| -> bool {
        let Some(element) = pending.next() else {
            return false;
        };
        let op = Arc::clone(&operation);
        group.spawn(async move { op(element).await });
        true
    };

    for _ in 0..width {
        if !submit(&mut group) {
            break;
        }
    }
    while let Some(result) = group.join_next().await {
        if let Err(err) = result {
            if err.is_panic() {
                std::panic::resume_unwind(err.into_panic());
            }
        }
        submit(&mut group);
    }
}

/// 프리페치 배치 예산 — 어떤 코어에서 돌지가 아니라 동시에 몇 개를 띄울지의 상한이다.
///
/// 원본의 `currentLimit()` 은 `PowerSourceMonitor`·`SystemLoadMonitor` 싱글톤(플랫폼 종속)을
/// 읽으므로 옮기지 않았다. 호출부가 값을 넘겨 [`prefetch_concurrency_limit`] 를 부른다.
pub fn prefetch_concurrency_limit(
    requested: i64,
    active_processors: i64,
    high_performance: bool,
    load: SystemLoadMode,
) -> i64 {
    let base = requested
        .max(1)
        .min(active_processors.max(1))
        .min(if high_performance { 4 } else { 2 });
    load.adjusted_decoder_threads(base)
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewCoreTests/BoundedConcurrentWorkTests.swift`
    use super::*;
    use std::sync::Mutex;
    use tokio::sync::{Notify, Semaphore};

    /// 시작·종료·동시 수를 기록하고, 열릴 때까지 작업을 붙잡아 두는 탐침.
    struct Probe {
        state: Mutex<ProbeState>,
        started_notify: Notify,
        gate: Semaphore,
    }

    #[derive(Default)]
    struct ProbeState {
        active: usize,
        peak: usize,
        started: Vec<i32>,
        finished: Vec<i32>,
    }

    /// 작업이 abort 되어도 active 를 되돌리는 가드.
    struct ActiveGuard<'a>(&'a Probe);
    impl Drop for ActiveGuard<'_> {
        fn drop(&mut self) {
            self.0.state.lock().unwrap().active -= 1;
        }
    }

    impl Probe {
        fn new(open: bool) -> Arc<Self> {
            let probe = Arc::new(Self {
                state: Mutex::default(),
                started_notify: Notify::new(),
                gate: Semaphore::new(0),
            });
            if open {
                probe.release();
            }
            probe
        }

        async fn run(&self, item: i32) {
            {
                let mut s = self.state.lock().unwrap();
                s.active += 1;
                s.peak = s.peak.max(s.active);
                s.started.push(item);
            }
            let _guard = ActiveGuard(self);
            self.started_notify.notify_waiters();
            drop(self.gate.acquire().await.unwrap());
            self.state.lock().unwrap().finished.push(item);
        }

        async fn wait_for_starts(&self, count: usize) {
            loop {
                let notified = self.started_notify.notified();
                if self.state.lock().unwrap().started.len() >= count {
                    return;
                }
                notified.await;
            }
        }

        fn release(&self) {
            self.gate.add_permits(Semaphore::MAX_PERMITS / 2);
        }

        fn snapshot<R>(&self, f: impl FnOnce(&ProbeState) -> R) -> R {
            f(&self.state.lock().unwrap())
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn bounded_fanout() {
        let probe = Probe::new(false);
        let p = Arc::clone(&probe);
        let task = tokio::spawn(for_each(0..120, 4, move |i| {
            let p = Arc::clone(&p);
            async move { p.run(i).await }
        }));
        probe.wait_for_starts(4).await;
        tokio::task::yield_now().await;
        assert_eq!(probe.snapshot(|s| s.started.len()), 4);
        probe.release();
        task.await.unwrap();
        assert_eq!(probe.snapshot(|s| s.peak), 4);
        let mut finished = probe.snapshot(|s| s.finished.clone());
        finished.sort_unstable();
        assert_eq!(finished, (0..120).collect::<Vec<_>>());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn cancellation_stops_refill() {
        let probe = Probe::new(false);
        let p = Arc::clone(&probe);
        let task = tokio::spawn(for_each(0..120, 4, move |i| {
            let p = Arc::clone(&p);
            async move { p.run(i).await }
        }));
        probe.wait_for_starts(4).await;
        task.abort();
        let _ = task.await;
        probe.release();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(probe.snapshot(|s| s.started.len()), 4);
        assert_eq!(probe.snapshot(|s| s.active), 0);
    }

    #[tokio::test]
    async fn invalid_limit_runs_serially() {
        let output = Probe::new(true);
        let p = Arc::clone(&output);
        for_each([1, 2, 3], 0, move |i| {
            let p = Arc::clone(&p);
            async move { p.run(i).await }
        })
        .await;
        assert_eq!(output.snapshot(|s| s.finished.clone()), vec![1, 2, 3]);
        assert_eq!(output.snapshot(|s| s.peak), 1);
    }

    #[tokio::test]
    async fn empty_input_is_noop() {
        for_each(Vec::<i32>::new(), 4, |_| async {}).await;
    }

    #[test]
    fn budget_boundaries() {
        for base in [-1, 0, 1, 2, 3, 4, 6, i64::MAX] {
            let normal = SystemLoadMode::Normal.adjusted_decoder_threads(base);
            let warm = SystemLoadMode::Warm.adjusted_decoder_threads(base);
            let hot = SystemLoadMode::Hot.adjusted_decoder_threads(base);
            assert!(1 <= hot && hot <= warm && warm <= normal, "base={base}");
        }
        assert_eq!(SystemLoadMode::Warm.adjusted_decoder_threads(1), 1);
        assert_eq!(SystemLoadMode::Warm.adjusted_decoder_threads(6), 4);
        assert_eq!(
            prefetch_concurrency_limit(120, 12, true, SystemLoadMode::Normal),
            4
        );
        assert_eq!(
            prefetch_concurrency_limit(120, 12, false, SystemLoadMode::Normal),
            2
        );
        assert_eq!(
            prefetch_concurrency_limit(120, 1, true, SystemLoadMode::Warm),
            1
        );
        assert_eq!(
            prefetch_concurrency_limit(4, 12, true, SystemLoadMode::Hot),
            1
        );
    }
}
