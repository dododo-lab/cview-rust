//! `Sources/CViewAuth/AuthSessionGeneration.swift` 포팅 — 로그인 세션 세대.
//!
//! 인증 관리자와 저장 계층이 함께 쓴다. "소유권 확인 + 동기 저장 트랜잭션" 은 무효화(`advance`)에 대해
//! 원자적이어야 하므로, 저장 작업을 세대 락 **안에서** 실행한다([`AuthSessionGeneration::with_current`]).
//!
//! Swift 의 `Task.checkCancellation()` 은 Rust 에서 future drop 으로 대체되므로 세대 비교만 한다.

use std::sync::Mutex;

/// 세대가 바뀌어 작업 결과를 버려야 할 때의 오류 (Swift `CancellationError`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("작업이 취소되었습니다")]
pub struct Cancelled;

/// 로그인/로그아웃마다 올라가는 세션 세대.
#[derive(Debug, Default)]
pub struct AuthSessionGeneration {
    value: Mutex<u64>,
}

impl AuthSessionGeneration {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, u64> {
        self.value.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 현재 세대.
    pub fn current(&self) -> u64 {
        *self.lock()
    }

    /// 세대를 올리고 새 값을 돌려준다(오버플로는 감싼다).
    pub fn advance(&self) -> u64 {
        let mut v = self.lock();
        *v = v.wrapping_add(1);
        *v
    }

    /// 세대가 `expected` 일 때만 `operation` 을 락 안에서 실행한다.
    pub fn with_current<T, E>(
        &self,
        expected: u64,
        operation: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E>
    where
        E: From<Cancelled>,
    {
        let current = self.lock();
        if *current != expected {
            return Err(Cancelled.into());
        }
        operation()
    }

    /// 세대가 `expected` 가 아니면 [`Cancelled`].
    pub fn check(&self, expected: u64) -> Result<(), Cancelled> {
        if self.current() == expected {
            Ok(())
        } else {
            Err(Cancelled)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advance_and_check() {
        let g = AuthSessionGeneration::new();
        assert_eq!(g.current(), 0);
        let a = g.advance();
        assert_eq!(a, 1);
        assert!(g.check(1).is_ok());
        assert_eq!(g.check(0), Err(Cancelled));
        let r: Result<i32, Cancelled> = g.with_current(0, || Ok(1));
        assert_eq!(r, Err(Cancelled));
        let r: Result<i32, Cancelled> = g.with_current(1, || Ok(7));
        assert_eq!(r, Ok(7));
    }
}
