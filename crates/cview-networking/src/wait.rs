//! `ChzzkAPIClient.swift` 의 `GETWait` 포팅 — 같은 GET 을 기다리는 쪽(single-flight 대기).
//!
//! 호출자 취소(future drop)는 자기 대기만 끝내고 공유 요청은 그대로 둔다. 기다리는 호출자가 모두
//! 떠나면(`reserve` 한 수가 0) `on_abandon` 을 부른다 — 클라이언트는 그때 공유 작업을 멈춘다.
//! 오류 뒤 이전 값으로 성공했으면 원래 오류를 함께 보관해, 이전 값을 받지 않는 호출자(bypassCache)
//! 에게는 원래 오류를 준다.

use std::any::Any;
use std::sync::{Arc, Mutex, MutexGuard};

use cview_core::error::ApiError;
use tokio::sync::Notify;

pub(crate) type AnyValue = Arc<dyn Any + Send + Sync>;
type AbandonFn = Box<dyn Fn(&GetWait) + Send + Sync>;

#[derive(Clone)]
struct Payload {
    value: AnyValue,
    stale_fallback_error: Option<ApiError>,
}

#[derive(Default)]
struct WaitState {
    result: Option<Result<Payload, ApiError>>,
    reserved: usize,
    waiters: usize,
}

struct Shared {
    state: Mutex<WaitState>,
    notify: Notify,
    on_abandon: Option<AbandonFn>,
}

/// 공유 대기. 복제하면 같은 대기를 가리킨다.
#[derive(Clone)]
pub(crate) struct GetWait(Arc<Shared>);

impl GetWait {
    pub(crate) fn new(on_abandon: Option<AbandonFn>) -> Self {
        Self(Arc::new(Shared {
            state: Mutex::new(WaitState::default()),
            notify: Notify::new(),
            on_abandon,
        }))
    }

    fn lock(&self) -> MutexGuard<'_, WaitState> {
        self.0.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn ptr_eq(&self, other: &GetWait) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub(crate) fn succeed(&self, value: AnyValue, stale_fallback_error: Option<ApiError>) {
        self.finish(Ok(Payload {
            value,
            stale_fallback_error,
        }));
    }

    pub(crate) fn fail(&self, error: ApiError) {
        self.finish(Err(error));
    }

    fn finish(&self, result: Result<Payload, ApiError>) {
        {
            let mut st = self.lock();
            if st.result.is_some() {
                return;
            }
            st.result = Some(result);
        }
        self.0.notify.notify_waiters();
    }

    /// 이 대기에 붙는다. 붙을 대기를 찾은 임계 구역 안에서 부른다. 표는 drop 될 때 반납된다.
    pub(crate) fn reserve(&self) -> WaitTicket {
        self.lock().reserved += 1;
        WaitTicket { wait: self.clone() }
    }

    /// 기다리는 호출자가 없고 결과도 아직 없다.
    pub(crate) fn is_abandoned(&self) -> bool {
        let st = self.lock();
        st.reserved == 0 && st.result.is_none()
    }

    /// 지금 결과를 기다리며 매달린 호출자 수(테스트 폴링용).
    pub(crate) fn waiter_count(&self) -> usize {
        self.lock().waiters
    }

    fn release(&self) {
        let abandoned = {
            let mut st = self.lock();
            st.reserved = st.reserved.saturating_sub(1);
            st.reserved == 0 && st.result.is_none()
        };
        if abandoned {
            if let Some(f) = &self.0.on_abandon {
                f(self);
            }
        }
    }
}

/// `reserve` 로 받은 대기표. `value` 를 한 번 부르거나 drop 하면 반납된다.
pub(crate) struct WaitTicket {
    wait: GetWait,
}

impl Drop for WaitTicket {
    fn drop(&mut self) {
        self.wait.release();
    }
}

struct WaiterGuard<'a>(&'a GetWait);

impl Drop for WaiterGuard<'_> {
    fn drop(&mut self) {
        let mut st = self.0.lock();
        st.waiters = st.waiters.saturating_sub(1);
    }
}

impl WaitTicket {
    /// 결과를 기다린다. `accepts_stale_fallback` 이 false 면 오류 뒤 이전 값으로 끝난 결과 대신
    /// 원래 오류를 돌려준다.
    pub(crate) async fn value(self, accepts_stale_fallback: bool) -> Result<AnyValue, ApiError> {
        loop {
            let notified = self.wait.0.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut st = self.wait.lock();
                if let Some(result) = &st.result {
                    return match result {
                        Ok(p) => match (&p.stale_fallback_error, accepts_stale_fallback) {
                            (Some(original), false) => Err(original.clone()),
                            _ => Ok(p.value.clone()),
                        },
                        Err(e) => Err(e.clone()),
                    };
                }
                st.waiters += 1;
            }
            let _guard = WaiterGuard(&self.wait);
            notified.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn waiters_share_result_and_stale_fallback() {
        let wait = GetWait::new(None);
        let t1 = wait.reserve();
        let t2 = wait.reserve();
        let h1 = tokio::spawn(t1.value(true));
        let h2 = tokio::spawn(t2.value(false));
        tokio::task::yield_now().await;
        wait.succeed(Arc::new(5_i64), Some(ApiError::http(503)));
        let v = h1.await.unwrap().unwrap();
        assert_eq!(*v.downcast::<i64>().unwrap(), 5);
        assert_eq!(h2.await.unwrap().unwrap_err(), ApiError::http(503));
        // 이미 끝난 대기에 붙으면 곧바로 결과를 받는다.
        assert!(wait.reserve().value(true).await.is_ok());
    }

    #[tokio::test]
    async fn abandon_fires_when_last_waiter_leaves() {
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        let wait = GetWait::new(Some(Box::new(move |_| {
            c.fetch_add(1, Ordering::SeqCst);
        })));
        let a = wait.reserve();
        let b = wait.reserve();
        drop(a);
        assert_eq!(count.load(Ordering::SeqCst), 0);
        let h = tokio::spawn(b.value(true));
        tokio::task::yield_now().await;
        assert_eq!(wait.waiter_count(), 1);
        h.abort();
        let _ = h.await;
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert!(wait.is_abandoned());
        assert_eq!(wait.waiter_count(), 0);
    }
}
