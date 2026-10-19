//! 뷰모델 공통 기반 — Swift `@Observable @MainActor` 클래스 대체.
//!
//! Swift 원본에는 대응 파일이 없다. `@MainActor` 는 "중단점(await) 사이에서는 상태가 일관된다"
//! 는 보장이었다. Rust 에서는 상태를 [`Observed`] 의 `Mutex` 안에 두고, `await` 를 건너
//! 잠금을 쥐지 않는 방식으로 같은 보장을 얻는다. 잠금을 놓을 때마다 리비전이 올라가
//! [`tokio::sync::watch`] 구독자(Tauri 브리지)에게 "상태가 바뀌었다" 신호가 간다.
//!
//! [`TaskSlot`] 은 Swift `Task<Void, Never>?` 저장 프로퍼티 대체다. `cancel()` 은 tokio 작업을
//! 중단하고, 타이머 본문은 슬롯 id 를 다시 확인해 "이미 취소/교체된 작업" 을 무시한다
//! (`Task.isCancelled` 확인과 같은 의미). tokio 런타임이 없으면(동기 단위 테스트) 작업은
//! 예약 표식으로만 남는다.

use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use tokio::sync::watch;
use tokio::task::AbortHandle;

/// 관찰 가능한 상태 셀. 잠금 해제 시 리비전을 올린다.
#[derive(Debug)]
pub struct Observed<T> {
    state: Mutex<T>,
    revision: watch::Sender<u64>,
}

impl<T> Observed<T> {
    pub fn new(value: T) -> Self {
        let (revision, _) = watch::channel(0);
        Self {
            state: Mutex::new(value),
            revision,
        }
    }

    /// 상태를 잠근다. 가드가 해제될 때 변경 알림이 나간다.
    pub fn lock(&self) -> ObservedGuard<'_, T> {
        ObservedGuard {
            guard: Some(self.state.lock().unwrap_or_else(|e| e.into_inner())),
            revision: &self.revision,
            notify: true,
        }
    }

    /// 알림 없이 읽기 전용으로 잠근다.
    pub fn read(&self) -> ObservedGuard<'_, T> {
        ObservedGuard {
            guard: Some(self.state.lock().unwrap_or_else(|e| e.into_inner())),
            revision: &self.revision,
            notify: false,
        }
    }

    /// 잠겨 있으면 기다리지 않고 `None` (교차 잠금 교착 회피용).
    pub fn try_lock(&self) -> Option<ObservedGuard<'_, T>> {
        let guard = match self.state.try_lock() {
            Ok(g) => g,
            Err(std::sync::TryLockError::Poisoned(e)) => e.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return None,
        };
        Some(ObservedGuard {
            guard: Some(guard),
            revision: &self.revision,
            notify: true,
        })
    }

    /// 변경 알림 구독. 값은 단조 증가하는 리비전이다.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.revision.subscribe()
    }

    pub fn revision(&self) -> u64 {
        *self.revision.borrow()
    }
}

impl<T: Default> Default for Observed<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

/// [`Observed::lock`] 가드.
pub struct ObservedGuard<'a, T> {
    guard: Option<MutexGuard<'a, T>>,
    revision: &'a watch::Sender<u64>,
    notify: bool,
}

impl<T> Deref for ObservedGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.guard.as_ref().expect("guard")
    }
}

impl<T> DerefMut for ObservedGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.guard.as_mut().expect("guard")
    }
}

impl<T> Drop for ObservedGuard<'_, T> {
    fn drop(&mut self) {
        // 잠금을 먼저 놓아야 구독자가 곧바로 상태를 읽을 수 있다.
        drop(self.guard.take());
        if self.notify {
            self.revision.send_modify(|r| *r = r.wrapping_add(1));
        }
    }
}

static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

/// Swift `Task` 저장 프로퍼티 대체.
#[derive(Debug)]
pub struct TaskSlot {
    id: u64,
    abort: Option<AbortHandle>,
}

impl TaskSlot {
    /// 새 id 를 받아 본문을 만들고 spawn 한다. 런타임이 없으면 표식만 남긴다.
    pub fn spawn<F, Fut>(make: F) -> Self
    where
        F: FnOnce(u64) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let id = NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed);
        let fut = make(id);
        let abort = tokio::runtime::Handle::try_current()
            .ok()
            .map(|h| h.spawn(fut).abort_handle());
        Self { id, abort }
    }

    /// 런타임 없이 예약 표식만 만든다(테스트에서 `task != nil` 상태를 흉내 낼 때).
    pub fn marker() -> Self {
        Self {
            id: NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed),
            abort: None,
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn cancel(&self) {
        if let Some(a) = &self.abort {
            a.abort();
        }
    }

    pub fn is_finished(&self) -> bool {
        self.abort.as_ref().is_none_or(|a| a.is_finished())
    }
}

/// `slot?.cancel(); slot = nil`.
pub fn cancel_slot(slot: &mut Option<TaskSlot>) {
    if let Some(t) = slot.take() {
        t.cancel();
    }
}

/// 슬롯이 아직 이 id 의 작업을 가리키는지.
pub fn slot_is(slot: &Option<TaskSlot>, id: u64) -> bool {
    slot.as_ref().is_some_and(|t| t.id == id)
}

/// 숫자 천 단위 구분 (Swift `formatted(.number)`, ko_KR).
pub fn format_number(n: i64) -> String {
    let s = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_number_groups() {
        assert_eq!(format_number(0), "0");
        assert_eq!(format_number(999), "999");
        assert_eq!(format_number(1000), "1,000");
        assert_eq!(format_number(30_000), "30,000");
        assert_eq!(format_number(-1234567), "-1,234,567");
    }

    #[test]
    fn observed_bumps_revision_on_lock_drop() {
        let o = Observed::new(1);
        let rx = o.subscribe();
        {
            let mut g = o.lock();
            *g = 2;
        }
        assert_eq!(*rx.borrow(), 1);
        {
            let _g = o.read();
        }
        assert_eq!(o.revision(), 1);
    }
}
