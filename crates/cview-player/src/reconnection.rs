//! `Sources/CViewPlayer/PlaybackReconnectionHandler.swift` 포팅 — 라이브 스트림 자동 재연결.
//!
//! - 지수 백오프: `delay = min(base * multiplier^(attempt-1), max)`
//! - 최대 재시도 횟수 초과 시 `on_exhausted` 호출
//! - Swift actor → `Arc<Mutex<State>>` + tokio 태스크. 세대(generation) 가드로 취소된 이전 루프가
//!   늦게 끝나도 새 루프 상태를 건드리지 못한다. `cancel` 은 Swift 의 `Task.cancel` 처럼 실행 중인
//!   시도 future 를 중단(abort)한다.

use std::sync::{Arc, Mutex, MutexGuard};

use futures_util::future::BoxFuture;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

/// 재연결 전략 설정.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReconnectionConfig {
    pub max_retries: u32,
    pub base_delay: f64,
    pub max_retry_delay: f64,
    pub backoff_multiplier: f64,
    pub retry_on_network_recovery: bool,
}

impl ReconnectionConfig {
    /// 공격적 — 빠른 재연결, 높은 재시도 횟수.
    pub const AGGRESSIVE: Self = Self {
        max_retries: 10,
        base_delay: 0.5,
        max_retry_delay: 15.0,
        backoff_multiplier: 1.5,
        retry_on_network_recovery: true,
    };
    /// 균형 — 기본 전략.
    pub const BALANCED: Self = Self {
        max_retries: 5,
        base_delay: 1.0,
        max_retry_delay: 30.0,
        backoff_multiplier: 2.0,
        retry_on_network_recovery: true,
    };
    /// 보수적 — 느리지만 안정적.
    pub const CONSERVATIVE: Self = Self {
        max_retries: 3,
        base_delay: 2.0,
        max_retry_delay: 60.0,
        backoff_multiplier: 3.0,
        retry_on_network_recovery: false,
    };

    /// `retry_on_network_recovery` 기본값 `true` 로 생성.
    pub fn new(max_retries: u32, base_delay: f64, max_retry_delay: f64, multiplier: f64) -> Self {
        Self {
            max_retries,
            base_delay,
            max_retry_delay,
            backoff_multiplier: multiplier,
            retry_on_network_recovery: true,
        }
    }

    /// 시도 번호(1-based)의 지연.
    pub fn delay_for(&self, attempt: u32) -> f64 {
        let exponent = attempt.saturating_sub(1) as i32;
        (self.base_delay * self.backoff_multiplier.powi(exponent)).min(self.max_retry_delay)
    }
}

impl Default for ReconnectionConfig {
    fn default() -> Self {
        Self::BALANCED
    }
}

/// 각 재시도 직전 호출 — (시도 번호, 지연 초). 백오프와 실제 재생 시도를 함께 소유한다.
pub type AttemptCallback = Arc<dyn Fn(u32, f64) -> BoxFuture<'static, ()> + Send + Sync>;
/// 최대 재시도 소진 시 호출.
pub type ExhaustedCallback = Arc<dyn Fn() -> BoxFuture<'static, ()> + Send + Sync>;

struct State {
    attempt: u32,
    is_reconnecting: bool,
    config: ReconnectionConfig,
    task: Option<JoinHandle<()>>,
    generation: u64,
    immediate_retry_requested: bool,
    /// 현재 백오프 대기의 식별자(재진입 가드).
    backoff_sleep: Option<u64>,
    sleep_counter: u64,
}

struct Inner {
    state: Mutex<State>,
    wake: Notify,
}

/// 라이브 스트림 자동 재연결 핸들러.
#[derive(Clone)]
pub struct PlaybackReconnectionHandler {
    inner: Arc<Inner>,
}

impl Default for PlaybackReconnectionHandler {
    fn default() -> Self {
        Self::new(ReconnectionConfig::BALANCED)
    }
}

impl PlaybackReconnectionHandler {
    pub fn new(config: ReconnectionConfig) -> Self {
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State {
                    attempt: 0,
                    is_reconnecting: false,
                    config,
                    task: None,
                    generation: 0,
                    immediate_retry_requested: false,
                    backoff_sleep: None,
                    sleep_counter: 0,
                }),
                wake: Notify::new(),
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.inner.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set_config(&self, config: ReconnectionConfig) {
        self.lock().config = config;
    }

    pub fn config(&self) -> ReconnectionConfig {
        self.lock().config
    }

    pub fn current_attempt(&self) -> u32 {
        self.lock().attempt
    }

    pub fn is_reconnecting(&self) -> bool {
        self.lock().is_reconnecting
    }

    /// 재연결 시작. 이미 재연결 중이면 무시한다.
    pub fn start_reconnecting(&self, on_attempt: AttemptCallback, on_exhausted: ExhaustedCallback) {
        let mut s = self.lock();
        if s.is_reconnecting {
            return;
        }
        s.generation = s.generation.wrapping_add(1);
        let owner = s.generation;
        s.is_reconnecting = true;
        s.attempt = 0;
        let weak = Arc::downgrade(&self.inner);
        s.task = Some(tokio::spawn(async move {
            let Some(inner) = weak.upgrade() else { return };
            let handler = PlaybackReconnectionHandler { inner };
            handler.run(owner, on_attempt, on_exhausted).await;
        }));
    }

    async fn run(&self, owner: u64, on_attempt: AttemptCallback, on_exhausted: ExhaustedCallback) {
        loop {
            let next = {
                let mut s = self.lock();
                if s.generation != owner || !s.is_reconnecting {
                    None
                } else if s.attempt >= s.config.max_retries {
                    s.is_reconnecting = false;
                    s.task = None;
                    Some(Err(()))
                } else {
                    s.attempt += 1;
                    let delay = s.config.delay_for(s.attempt);
                    tracing::info!(
                        "PlaybackReconnectionHandler: 재시도 {}/{} — {:.1}초 후",
                        s.attempt,
                        s.config.max_retries,
                        delay
                    );
                    Some(Ok((s.attempt, delay)))
                }
            };
            match next {
                None => break,
                Some(Err(())) => {
                    on_exhausted().await;
                    return;
                }
                Some(Ok((attempt, delay))) => on_attempt(attempt, delay).await,
            }
        }
        let mut s = self.lock();
        if s.generation == owner {
            s.is_reconnecting = false;
            s.task = None;
        }
    }

    /// 재연결 중단 및 상태 초기화.
    pub fn cancel(&self) {
        let task = {
            let mut s = self.lock();
            s.generation = s.generation.wrapping_add(1);
            s.immediate_retry_requested = false;
            s.backoff_sleep = None;
            s.is_reconnecting = false;
            s.attempt = 0;
            s.task.take()
        };
        self.inner.wake.notify_waiters();
        if let Some(task) = task {
            task.abort();
        }
    }

    /// 재시도 카운터만 초기화 (재연결은 계속).
    pub fn reset(&self) {
        self.lock().attempt = 0;
    }

    /// 실제 첫 프레임 재생을 확인하면 재시도 루프를 끝낸다.
    pub fn handle_success(&self) {
        self.cancel();
        tracing::info!("PlaybackReconnectionHandler: 재연결 첫 프레임 확인");
    }

    /// 네트워크 복구 시 남은 백오프를 건너뛴다. 재연결 중이 아니거나 옵션이 꺼져 있으면 `false`.
    pub fn request_immediate_retry(&self) -> bool {
        {
            let mut s = self.lock();
            if !s.config.retry_on_network_recovery || !s.is_reconnecting {
                return false;
            }
            s.immediate_retry_requested = true;
        }
        self.inner.wake.notify_waiters();
        tracing::info!("PlaybackReconnectionHandler: 네트워크 복구 — 백오프 생략");
        true
    }

    /// `on_attempt` 앞의 지수 백오프. `request_immediate_retry` 가 오면 남은 시간을 끊는다.
    pub async fn wait_for_backoff(&self, delay: f64) {
        let id = {
            let mut s = self.lock();
            if s.immediate_retry_requested {
                s.immediate_retry_requested = false;
                return;
            }
            if delay <= 0.0 || !delay.is_finite() {
                return;
            }
            s.sleep_counter += 1;
            s.backoff_sleep = Some(s.sleep_counter);
            s.sleep_counter
        };
        let sleep = tokio::time::sleep(std::time::Duration::from_secs_f64(delay));
        tokio::pin!(sleep);
        loop {
            let notified = self.inner.wake.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let s = self.lock();
                if s.backoff_sleep != Some(id) || s.immediate_retry_requested {
                    break;
                }
            }
            tokio::select! {
                _ = &mut sleep => break,
                _ = notified => {}
            }
        }
        let mut s = self.lock();
        // 재진입: cancel 뒤 새 루프가 이미 자기 대기를 걸었으면 그 상태를 지우지 않는다.
        if s.backoff_sleep != Some(id) {
            return;
        }
        s.backoff_sleep = None;
        s.immediate_retry_requested = false;
    }
}

#[cfg(test)]
mod tests {
    //! `PlaybackReconnectionHandlerTests` · `PlaybackReconnectionOwnershipTests` 포팅.
    use super::*;
    use std::time::Duration;
    use tokio::sync::oneshot;

    fn noop_exhausted() -> ExhaustedCallback {
        Arc::new(|| Box::pin(async {}))
    }

    fn sleeping_attempt() -> AttemptCallback {
        Arc::new(|_, delay| {
            Box::pin(async move { tokio::time::sleep(Duration::from_secs_f64(delay)).await })
        })
    }

    fn collecting<T: Send + 'static>(
        f: impl Fn(u32, f64) -> T + Send + Sync + 'static,
    ) -> (AttemptCallback, Arc<Mutex<Vec<T>>>) {
        let out = Arc::new(Mutex::new(Vec::new()));
        let o = out.clone();
        (
            Arc::new(move |a, d| {
                o.lock().unwrap().push(f(a, d));
                Box::pin(async {})
            }),
            out,
        )
    }

    async fn settle() {
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    #[test]
    fn presets() {
        let a = ReconnectionConfig::AGGRESSIVE;
        assert_eq!(
            (a.max_retries, a.base_delay, a.max_retry_delay),
            (10, 0.5, 15.0)
        );
        assert_eq!(
            (a.backoff_multiplier, a.retry_on_network_recovery),
            (1.5, true)
        );
        let b = ReconnectionConfig::BALANCED;
        assert_eq!(
            (b.max_retries, b.base_delay, b.max_retry_delay),
            (5, 1.0, 30.0)
        );
        assert_eq!(
            (b.backoff_multiplier, b.retry_on_network_recovery),
            (2.0, true)
        );
        let c = ReconnectionConfig::CONSERVATIVE;
        assert_eq!(
            (c.max_retries, c.base_delay, c.max_retry_delay),
            (3, 2.0, 60.0)
        );
        assert_eq!(
            (c.backoff_multiplier, c.retry_on_network_recovery),
            (3.0, false)
        );
        let custom = ReconnectionConfig::new(7, 0.8, 20.0, 1.8);
        assert!(custom.retry_on_network_recovery);
        assert_eq!(custom.max_retries, 7);
    }

    #[tokio::test(start_paused = true)]
    async fn initial_state_and_cancel_reset() {
        let h = PlaybackReconnectionHandler::default();
        assert_eq!(h.current_attempt(), 0);
        assert!(!h.is_reconnecting());

        let h = PlaybackReconnectionHandler::new(ReconnectionConfig::AGGRESSIVE);
        h.start_reconnecting(sleeping_attempt(), noop_exhausted());
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(h.current_attempt(), 1);
        h.cancel();
        assert_eq!(h.current_attempt(), 0);
        assert!(!h.is_reconnecting());

        h.start_reconnecting(sleeping_attempt(), noop_exhausted());
        tokio::time::sleep(Duration::from_millis(100)).await;
        h.handle_success();
        assert_eq!(h.current_attempt(), 0);
        assert!(!h.is_reconnecting());
    }

    #[tokio::test(start_paused = true)]
    async fn set_config_applies() {
        let h = PlaybackReconnectionHandler::new(ReconnectionConfig::BALANCED);
        h.set_config(ReconnectionConfig::AGGRESSIVE);
        let (cb, attempts) = collecting(|a, _| a);
        h.start_reconnecting(cb, noop_exhausted());
        settle().await;
        h.cancel();
        assert_eq!(attempts.lock().unwrap().len(), 10);
    }

    #[tokio::test(start_paused = true)]
    async fn reset_only_attempt() {
        let h = PlaybackReconnectionHandler::default();
        h.start_reconnecting(sleeping_attempt(), noop_exhausted());
        tokio::time::sleep(Duration::from_millis(100)).await;
        h.reset();
        assert_eq!(h.current_attempt(), 0);
        assert!(h.is_reconnecting());
        h.cancel();
    }

    #[tokio::test(start_paused = true)]
    async fn double_start_guard() {
        let h = PlaybackReconnectionHandler::default();
        h.start_reconnecting(sleeping_attempt(), noop_exhausted());
        let (second, seen) = collecting(|a, _| a);
        h.start_reconnecting(second, noop_exhausted());
        tokio::time::sleep(Duration::from_millis(200)).await;
        h.cancel();
        assert!(seen.lock().unwrap().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn exhausted_calls_callback() {
        let h = PlaybackReconnectionHandler::new(ReconnectionConfig::new(2, 0.01, 0.05, 1.5));
        let flag = Arc::new(Mutex::new(false));
        let f = flag.clone();
        let (cb, attempts) = collecting(|a, _| a);
        h.start_reconnecting(
            cb,
            Arc::new(move || {
                *f.lock().unwrap() = true;
                Box::pin(async {})
            }),
        );
        settle().await;
        assert!(*flag.lock().unwrap());
        assert_eq!(*attempts.lock().unwrap(), vec![1, 2]);
    }

    #[tokio::test(start_paused = true)]
    async fn exponential_and_capped_delays() {
        let h = PlaybackReconnectionHandler::new(ReconnectionConfig::new(4, 1.0, 30.0, 2.0));
        let (cb, delays) = collecting(|_, d| d);
        h.start_reconnecting(cb, noop_exhausted());
        settle().await;
        assert_eq!(*delays.lock().unwrap(), vec![1.0, 2.0, 4.0, 8.0]);

        let h = PlaybackReconnectionHandler::new(ReconnectionConfig::new(5, 10.0, 15.0, 3.0));
        let (cb, delays) = collecting(|_, d| d);
        h.start_reconnecting(cb, noop_exhausted());
        settle().await;
        h.cancel();
        assert_eq!(delays.lock().unwrap()[..3], [10.0, 15.0, 15.0]);
    }

    #[tokio::test(start_paused = true)]
    async fn old_completion_cannot_finish_replacement() {
        for success in [false, true] {
            let h = PlaybackReconnectionHandler::default();
            let (old_tx, old_rx) = oneshot::channel::<()>();
            let old_rx = Arc::new(tokio::sync::Mutex::new(Some(old_rx)));
            h.start_reconnecting(
                Arc::new(move |_, _| {
                    let rx = old_rx.clone();
                    Box::pin(async move {
                        if let Some(rx) = rx.lock().await.take() {
                            let _ = rx.await;
                        }
                    })
                }),
                noop_exhausted(),
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
            if success {
                h.handle_success();
            } else {
                h.cancel();
            }
            h.start_reconnecting(
                Arc::new(|_, _| Box::pin(futures_util::future::pending())),
                noop_exhausted(),
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
            let _ = old_tx.send(());
            tokio::time::sleep(Duration::from_millis(30)).await;
            assert!(h.is_reconnecting());
            assert_eq!(h.current_attempt(), 1);
            h.cancel();
        }
    }

    #[tokio::test(start_paused = true)]
    async fn exhausted_attempt_count_matches_real_attempts() {
        let h = PlaybackReconnectionHandler::new(ReconnectionConfig::new(2, 0.0, 0.0, 1.0));
        let (tx, rx) = oneshot::channel::<()>();
        let tx = Arc::new(Mutex::new(Some(tx)));
        h.start_reconnecting(
            Arc::new(|_, _| Box::pin(async {})),
            Arc::new(move || {
                if let Some(tx) = tx.lock().unwrap().take() {
                    let _ = tx.send(());
                }
                Box::pin(futures_util::future::pending())
            }),
        );
        rx.await.unwrap();
        assert_eq!(h.current_attempt(), 2);
        assert!(!h.is_reconnecting());
    }

    #[tokio::test(start_paused = true)]
    async fn immediate_retry_cuts_backoff() {
        let h = PlaybackReconnectionHandler::new(ReconnectionConfig::BALANCED);
        assert!(!h.request_immediate_retry(), "재연결 중이 아니면 거절");
        h.start_reconnecting(
            Arc::new(|_, _| Box::pin(futures_util::future::pending())),
            noop_exhausted(),
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
        let waiter = h.clone();
        let start = tokio::time::Instant::now();
        let wait = tokio::spawn(async move { waiter.wait_for_backoff(30.0).await });
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(h.request_immediate_retry());
        wait.await.unwrap();
        assert!(start.elapsed() < Duration::from_secs(1));
        // 대기 전에 요청이 먼저 왔으면 바로 반환.
        assert!(h.request_immediate_retry());
        h.wait_for_backoff(30.0).await;
        assert!(start.elapsed() < Duration::from_secs(1));
        h.cancel();

        let c = PlaybackReconnectionHandler::new(ReconnectionConfig::CONSERVATIVE);
        c.start_reconnecting(
            Arc::new(|_, _| Box::pin(futures_util::future::pending())),
            noop_exhausted(),
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
        assert!(!c.request_immediate_retry());
        c.cancel();
    }
}
