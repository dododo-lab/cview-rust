//! 시간 공급자 — Swift `Date()` / `ContinuousClock` 대체.
//!
//! 원본 Swift 코드는 곳곳에서 `Date()` 를 직접 읽는다. Rust 포팅은 테스트가 시간을 주입할 수
//! 있게 [`Clock`] 하나로 모은다. 값은 "초" 단위 `f64` (UNIX epoch 기준)이며
//! `distantPast`/`distantFuture` 는 `f64::NEG_INFINITY`/`f64::INFINITY` 로 표현한다.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Swift `Date.distantPast`.
pub const DISTANT_PAST: f64 = f64::NEG_INFINITY;
/// Swift `Date.distantFuture`.
pub const DISTANT_FUTURE: f64 = f64::INFINITY;

/// 현재 시각(초) 공급자.
pub trait Clock: Send + Sync + std::fmt::Debug {
    fn now(&self) -> f64;
}

/// 시스템 벽시계.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> f64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
    }
}

/// 테스트용 수동 시계 — `advance`/`set` 으로만 움직인다.
#[derive(Debug, Default)]
pub struct ManualClock {
    bits: AtomicU64,
}

impl ManualClock {
    pub fn new(start: f64) -> Arc<Self> {
        Arc::new(Self {
            bits: AtomicU64::new(start.to_bits()),
        })
    }

    pub fn set(&self, value: f64) {
        self.bits.store(value.to_bits(), Ordering::SeqCst);
    }

    pub fn advance(&self, seconds: f64) {
        self.set(self.now() + seconds);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> f64 {
        f64::from_bits(self.bits.load(Ordering::SeqCst))
    }
}

/// 공유 시계 핸들.
pub type SharedClock = Arc<dyn Clock>;

/// 기본 시스템 시계 핸들.
pub fn system() -> SharedClock {
    Arc::new(SystemClock)
}
