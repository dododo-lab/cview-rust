//! `Sources/CViewNetworking/APIRequestGate.swift` 포팅 — JSON API 전송 속도·동시 수 제한.
//!
//! CDN 영상·WebSocket 은 이 대기열을 통과하지 않는다. 빈 큐에는 타이머가 없고, 슬롯이 차면
//! 완료 이벤트(허가 해제)로만 깨어난다. 허가는 RAII([`GatePermit`])로 해제된다.

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use cview_core::error::ApiError;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::Instant;

struct Waiter {
    id: u64,
    tx: oneshot::Sender<()>,
}

struct GateState {
    queue: VecDeque<Waiter>,
    active: HashSet<u64>,
    rate: i64,
    limit: i64,
    next_start: Instant,
    timer: Option<JoinHandle<()>>,
    timer_generation: u64,
    policy_generation: u64,
    next_id: u64,
}

/// 게이트 상태 스냅샷(전송 수·취소·유휴 검증용).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GateSnapshot {
    pub active: usize,
    pub queued: usize,
    pub has_timer: bool,
    pub concurrent_limit: i64,
    pub starts_per_second: i64,
}

/// API 요청 게이트. 복제하면 같은 게이트를 공유한다.
#[derive(Clone)]
pub struct RequestGate {
    inner: Arc<Mutex<GateState>>,
}

impl Default for RequestGate {
    fn default() -> Self {
        Self::new()
    }
}

/// 획득한 전송 슬롯. drop 되면 슬롯을 돌려준다.
pub struct GatePermit {
    gate: RequestGate,
    id: u64,
}

impl Drop for GatePermit {
    fn drop(&mut self) {
        self.gate.release(self.id);
    }
}

impl RequestGate {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(GateState {
                queue: VecDeque::new(),
                active: HashSet::new(),
                rate: 10,
                limit: 4,
                next_start: Instant::now(),
                timer: None,
                timer_generation: 0,
                policy_generation: 0,
                next_id: 0,
            })),
        }
    }

    fn lock(&self) -> MutexGuard<'_, GateState> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 초당 시작 수(1–30)와 동시 수(1–12)를 바꾼다. `generation` 이 이전보다 작으면 무시한다.
    pub fn configure(&self, rate: i64, concurrent: i64, generation: Option<u64>) {
        let mut st = self.lock();
        if let Some(g) = generation {
            if g < st.policy_generation {
                return;
            }
            st.policy_generation = g;
        }
        st.rate = rate.clamp(1, 30);
        st.limit = concurrent.clamp(1, 12);
        if let Some(t) = st.timer.take() {
            t.abort();
        }
        st.timer_generation += 1;
        self.drain(&mut st);
    }

    /// 슬롯을 기다린다. 기다리는 동안 future 가 drop 되면 대기열에서 빠진다.
    pub async fn acquire(&self) -> Result<GatePermit, ApiError> {
        let (tx, rx) = oneshot::channel();
        let id = {
            let mut st = self.lock();
            st.next_id += 1;
            let id = st.next_id;
            st.queue.push_back(Waiter { id, tx });
            self.drain(&mut st);
            id
        };
        let mut guard = PendingGuard {
            gate: self.clone(),
            id,
            armed: true,
        };
        match rx.await {
            Ok(()) => {
                guard.armed = false;
                Ok(GatePermit {
                    gate: self.clone(),
                    id,
                })
            }
            Err(_) => Err(ApiError::Cancelled),
        }
    }

    fn release(&self, id: u64) {
        let mut st = self.lock();
        st.active.remove(&id);
        self.drain(&mut st);
    }

    /// 대기 중 취소. 이미 허가가 났는데 받지 못했으면 슬롯을 돌려준다.
    fn cancel(&self, id: u64) {
        let mut st = self.lock();
        if let Some(index) = st.queue.iter().position(|w| w.id == id) {
            st.queue.remove(index);
            if st.queue.is_empty() {
                if let Some(t) = st.timer.take() {
                    t.abort();
                }
                st.timer_generation += 1;
            }
        } else if st.active.remove(&id) {
            self.drain(&mut st);
        }
    }

    fn drain(&self, st: &mut GateState) {
        loop {
            if st.timer.is_some() || st.queue.is_empty() || st.active.len() as i64 >= st.limit {
                return;
            }
            let now = Instant::now();
            if now < st.next_start {
                let deadline = st.next_start;
                st.timer_generation += 1;
                let generation = st.timer_generation;
                let gate = self.clone();
                st.timer = Some(tokio::spawn(async move {
                    tokio::time::sleep_until(deadline).await;
                    gate.timer_fired(generation);
                }));
                return;
            }
            let Some(waiter) = st.queue.pop_front() else {
                return;
            };
            st.active.insert(waiter.id);
            st.next_start = now + Duration::from_secs_f64(1.0 / st.rate as f64);
            if waiter.tx.send(()).is_err() {
                // 받는 쪽이 이미 떠났다 — 슬롯을 돌려준다.
                st.active.remove(&waiter.id);
            }
        }
    }

    fn timer_fired(&self, generation: u64) {
        let mut st = self.lock();
        if generation != st.timer_generation {
            return;
        }
        st.timer = None;
        self.drain(&mut st);
    }

    pub fn snapshot(&self) -> GateSnapshot {
        let st = self.lock();
        GateSnapshot {
            active: st.active.len(),
            queued: st.queue.len(),
            has_timer: st.timer.is_some(),
            concurrent_limit: st.limit,
            starts_per_second: st.rate,
        }
    }
}

struct PendingGuard {
    gate: RequestGate,
    id: u64,
    armed: bool,
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        if self.armed {
            self.gate.cancel(self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    //! `APIRequestGateTests.swift` 포팅.
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn concurrency_limit_and_release() {
        let gate = RequestGate::new();
        gate.configure(30, 2, None);
        let a = gate.acquire().await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let _b = gate.acquire().await.unwrap();
        let g2 = gate.clone();
        let third = tokio::spawn(async move { g2.acquire().await.map(|_| ()) });
        tokio::time::sleep(Duration::from_millis(100)).await;
        let snap = gate.snapshot();
        assert_eq!(snap.active, 2);
        assert_eq!(snap.queued, 1);
        assert!(!snap.has_timer, "슬롯이 차면 타이머 없이 해제를 기다린다");
        drop(a);
        third.await.unwrap().unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn cancellation_removes_waiter() {
        let gate = RequestGate::new();
        gate.configure(30, 1, None);
        let _a = gate.acquire().await.unwrap();
        let g2 = gate.clone();
        let waiting = tokio::spawn(async move { g2.acquire().await.map(|_| ()) });
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(gate.snapshot().queued, 1);
        waiting.abort();
        let _ = waiting.await;
        assert_eq!(gate.snapshot().queued, 0);
        assert!(!gate.snapshot().has_timer);
    }

    #[tokio::test(start_paused = true)]
    async fn rate_spacing_and_clamps() {
        let gate = RequestGate::new();
        gate.configure(100, 100, None);
        assert_eq!(gate.snapshot().starts_per_second, 30);
        assert_eq!(gate.snapshot().concurrent_limit, 12);
        gate.configure(2, 4, Some(5));
        gate.configure(10, 4, Some(3));
        assert_eq!(
            gate.snapshot().starts_per_second,
            2,
            "이전 세대 설정은 무시"
        );
        let start = Instant::now();
        let _p1 = gate.acquire().await.unwrap();
        let _p2 = gate.acquire().await.unwrap();
        assert!(
            Instant::now() - start >= Duration::from_millis(500),
            "초당 2회 → 0.5초 간격"
        );
    }
}
