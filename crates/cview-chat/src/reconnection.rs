//! 지수 백오프 + 지터 재연결 정책. 원본: `Sources/CViewChat/ReconnectionPolicy.swift`.
//!
//! Swift `actor` 는 내부 `Mutex` 로 상태를 지키는 구조체로 옮겼다. 메서드는 모두 `&self`
//! 이고 잠금은 await 경계를 넘지 않는다 — 액터처럼 `execute_reconnection` 이 도는 동안에도
//! 다른 호출(`reset`, `state`)이 끼어들 수 있다.

use std::future::Future;
use std::sync::Mutex;
use std::time::Instant;

use rand::Rng;

use crate::constants::{reconnect_defaults, secs};
use crate::error::{ChatError, ChatResult};

/// 재연결 설정.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReconnectionConfiguration {
    pub initial_delay: f64,
    pub max_delay: f64,
    pub max_attempts: i64,
    pub backoff_multiplier: f64,
    pub jitter_factor: f64,
    pub reset_threshold: f64,
}

impl Default for ReconnectionConfiguration {
    /// Swift `.default`.
    fn default() -> Self {
        Self {
            initial_delay: 1.0,
            max_delay: reconnect_defaults::DEFAULT_MAX_DELAY,
            max_attempts: reconnect_defaults::DEFAULT_MAX_ATTEMPTS,
            backoff_multiplier: 2.0,
            jitter_factor: reconnect_defaults::DEFAULT_JITTER,
            reset_threshold: reconnect_defaults::RESET_THRESHOLD,
        }
    }
}

impl ReconnectionConfiguration {
    /// Swift `.aggressive`.
    pub fn aggressive() -> Self {
        Self {
            initial_delay: 0.5,
            max_delay: reconnect_defaults::AGGRESSIVE_MAX_DELAY,
            max_attempts: reconnect_defaults::AGGRESSIVE_MAX_ATTEMPTS,
            backoff_multiplier: 1.5,
            jitter_factor: 0.15,
            reset_threshold: 30.0,
        }
    }
}

/// 정책 상태.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReconnectionState {
    Idle,
    Waiting { attempt: i64, delay: f64 },
    Connecting { attempt: i64 },
    Connected,
    Exhausted,
}

#[derive(Debug)]
struct Inner {
    current_attempt: i64,
    last_successful_connection: Option<Instant>,
    state: ReconnectionState,
}

/// 재연결 정책. thundering herd 를 막기 위해 지수 백오프에 지터를 더한다.
#[derive(Debug)]
pub struct ReconnectionPolicy {
    config: ReconnectionConfiguration,
    inner: Mutex<Inner>,
}

impl Default for ReconnectionPolicy {
    fn default() -> Self {
        Self::new(ReconnectionConfiguration::default())
    }
}

impl ReconnectionPolicy {
    pub fn new(config: ReconnectionConfiguration) -> Self {
        Self {
            config,
            inner: Mutex::new(Inner {
                current_attempt: 0,
                last_successful_connection: None,
                state: ReconnectionState::Idle,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn configuration(&self) -> ReconnectionConfiguration {
        self.config
    }

    pub fn state(&self) -> ReconnectionState {
        self.lock().state
    }

    pub fn attempts_remaining(&self) -> i64 {
        (self.config.max_attempts - self.lock().current_attempt).max(0)
    }

    /// 다음 재연결 지연(초)을 계산하고 상태를 갱신한다. 시도를 소진하면 `None`.
    ///
    /// `mark_connected()` 가 남긴 성공 시각은 여기서 **한 번만** 소비한다. 연결이
    /// `reset_threshold` 이상 유지됐을 때만 누적 실패를 지운다.
    pub fn next_delay(&self) -> Option<f64> {
        let mut inner = self.lock();
        if let Some(connected_at) = inner.last_successful_connection.take() {
            if connected_at.elapsed().as_secs_f64() >= self.config.reset_threshold {
                inner.current_attempt = 0;
                tracing::info!("Stable connection reached reset threshold; reconnection attempts reset");
            }
        }

        inner.current_attempt += 1;

        if inner.current_attempt > self.config.max_attempts {
            inner.state = ReconnectionState::Exhausted;
            tracing::error!(max_attempts = self.config.max_attempts, "Reconnection exhausted");
            return None;
        }

        let exponent = (inner.current_attempt - 1) as f64;
        let base = self.config.initial_delay * self.config.backoff_multiplier.powf(exponent);
        let clamped = base.min(self.config.max_delay);

        // 지터: delay ± (jitterFactor * delay)
        let jitter = clamped * self.config.jitter_factor;
        let offset = if jitter > 0.0 { rand::thread_rng().gen_range(-jitter..=jitter) } else { 0.0 };
        let safe = (clamped + offset).max(0.1);

        inner.state = ReconnectionState::Waiting { attempt: inner.current_attempt, delay: safe };
        tracing::info!(
            attempt = inner.current_attempt,
            max_attempts = self.config.max_attempts,
            delay = format!("{safe:.1}"),
            "Reconnect attempt scheduled"
        );
        Some(safe)
    }

    /// 연결 성공 시각을 기록한다. attempt 는 즉시 지우지 않는다(flapping 누적 유지).
    pub fn mark_connected(&self) {
        let mut inner = self.lock();
        inner.last_successful_connection = Some(Instant::now());
        inner.state = ReconnectionState::Connected;
        tracing::info!("Connection established; waiting for stability threshold before reset");
    }

    /// 상태를 처음으로 되돌린다.
    pub fn reset(&self) {
        let mut inner = self.lock();
        inner.current_attempt = 0;
        inner.last_successful_connection = None;
        inner.state = ReconnectionState::Idle;
    }

    /// 재연결을 시도해야 하는가. max 번째 시도에서 성공했더라도 안정 연결이었다면
    /// `next_delay()` 가 예산을 되돌릴 기회를 가져야 한다.
    pub fn should_reconnect(&self) -> bool {
        let inner = self.lock();
        inner.last_successful_connection.is_some() || inner.current_attempt < self.config.max_attempts
    }

    /// 지연 → `action` 실행을 성공하거나 소진할 때까지 반복한다.
    ///
    /// `action` 이 [`ChatError::Cancelled`] 를 내면 일반 실패로 세지 않고 즉시 전파한다.
    pub async fn execute_reconnection<F, Fut>(&self, mut action: F) -> ChatResult<()>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = ChatResult<()>>,
    {
        while self.should_reconnect() {
            let Some(delay) = self.next_delay() else {
                return Err(ChatError::ConnectionFailed("재연결 지연 계산 실패".into()));
            };
            let attempt = {
                let mut inner = self.lock();
                inner.state = ReconnectionState::Waiting { attempt: inner.current_attempt, delay };
                inner.current_attempt
            };

            tokio::time::sleep(secs(delay)).await;

            self.lock().state = ReconnectionState::Connecting { attempt };

            match action().await {
                Ok(()) => {
                    self.mark_connected();
                    return Ok(());
                }
                Err(ChatError::Cancelled) => return Err(ChatError::Cancelled),
                Err(error) => {
                    tracing::warn!(attempt, %error, "Reconnect attempt failed");
                    continue;
                }
            }
        }

        self.lock().state = ReconnectionState::Exhausted;
        Err(ChatError::ConnectionFailed("최대 재연결 시도 횟수 초과".into()))
    }

    /// 정책 상태를 `Idle` 로 되돌린다. 진행 중인 재연결 작업을 취소하지는 않는다
    /// (그 소유자는 `ChatEngine` 의 재연결 태스크다).
    pub fn cancel(&self) {
        self.lock().state = ReconnectionState::Idle;
    }

    pub fn is_exhausted(&self) -> bool {
        matches!(self.lock().state, ReconnectionState::Exhausted)
    }

    /// UI 표시용 상태 문자열.
    pub fn status_description(&self) -> String {
        match self.state() {
            ReconnectionState::Idle => "대기 중".into(),
            ReconnectionState::Waiting { attempt, delay } => {
                format!("재연결 대기 ({attempt}/{}) - {delay:.0}초", self.config.max_attempts)
            }
            ReconnectionState::Connecting { attempt } => {
                format!("재연결 중 ({attempt}/{})", self.config.max_attempts)
            }
            ReconnectionState::Connected => "연결됨".into(),
            ReconnectionState::Exhausted => "재연결 실패".into(),
        }
    }
}
