//! `Sources/CViewPlayer/MatchWebV2SeekArbiter.swift` 포팅 — 웹 동기화 v2 전역 시크 토큰.
//!
//! 멀티라이브 패인들이 동시에 시크하면 WebKit 시크 멈춤이 겹친다. 그래서 v2 시크는 **한 번에 한 패인**만,
//! 해제 뒤 최소 간격을 두고, 선택 패인(높은 priority)을 먼저 허가한다. 단일 시청 패인도 같은 중재자를
//! 거친다 — 경합이 없으면 즉시 부여된다.
//!
//! 토큰을 못 받은 패인은 JS 가 5 s 에 `tokenTimeout` 으로 요청을 거두고 다시 요청한다. 대기 요청
//! 만료(6 s)를 JS 대기(5 s)보다 길게 두는 이유다. Swift 의 `ObjectIdentifier` owner 는 호출자가
//! 고르는 [`OwnerId`] 로 대체한다.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use tokio::task::JoinHandle;

use crate::clock::{SharedClock, TokioMonotonicClock};

/// 요청 소유자(패인) 식별자.
pub type OwnerId = u64;

/// 페이지 세대(page_gen)를 키에 넣는다 — 페이지가 재로드되면 같은 request_id 라도 다른 요청이다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SeekKey {
    pub owner: OwnerId,
    pub page_gen: u64,
    pub request_id: i64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeekRequest {
    pub key: SeekKey,
    pub priority: i32,
    pub enqueued_at: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActiveSeek {
    pub request: SeekRequest,
    pub granted_at: f64,
}

/// 부동소수 격자에서 경계값이 한 틱 밀리지 않게 하는 허용오차.
const EPSILON: f64 = 1e-9;

/// 순수 대기열 — 시계는 호출자가 준다.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchWebV2SeekQueue {
    /// 직전 해제(또는 자동 해제) 뒤 다음 부여까지.
    pub min_spacing: f64,
    /// 부여 뒤 응답이 없으면 자동 해제.
    pub grant_timeout: f64,
    /// 대기 요청 만료 — JS 대기(5 s)보다 길다.
    pub pending_ttl: f64,
    pending: Vec<SeekRequest>,
    active: Option<ActiveSeek>,
    last_release_at: f64,
}

impl Default for MatchWebV2SeekQueue {
    fn default() -> Self {
        Self {
            min_spacing: 1.0,
            grant_timeout: 4.0,
            pending_ttl: 6.0,
            pending: Vec::new(),
            active: None,
            last_release_at: f64::NEG_INFINITY,
        }
    }
}

impl MatchWebV2SeekQueue {
    pub fn pending(&self) -> &[SeekRequest] {
        &self.pending
    }

    pub fn active(&self) -> Option<&ActiveSeek> {
        self.active.as_ref()
    }

    pub fn last_release_at(&self) -> f64 {
        self.last_release_at
    }

    /// 같은 owner 의 기존 대기 요청은 교체한다.
    pub fn enqueue(&mut self, request: SeekRequest) {
        if self.active.map(|a| a.request.key) == Some(request.key) {
            return;
        }
        self.pending.retain(|r| r.key.owner != request.key.owner);
        self.pending.push(request);
    }

    /// active 면 해제(last_release_at = now), 대기면 제거.
    pub fn release(&mut self, key: SeekKey, now: f64) {
        if self.active.map(|a| a.request.key) == Some(key) {
            self.active = None;
            self.last_release_at = now;
        }
        self.pending.retain(|r| r.key != key);
    }

    pub fn cancel_all(&mut self, owner: OwnerId, now: f64) {
        if self.active.map(|a| a.request.key.owner) == Some(owner) {
            self.active = None;
            self.last_release_at = now;
        }
        self.pending.retain(|r| r.key.owner != owner);
    }

    /// 만료 정리. 자동 해제된 active 를 돌려준다. 해제 시각은 논리 시각(부여 + 제한)이다.
    pub fn expire(&mut self, now: f64) -> Option<ActiveSeek> {
        let ttl = self.pending_ttl;
        self.pending
            .retain(|r| now - r.enqueued_at <= ttl + EPSILON);
        let current = self.active?;
        if now - current.granted_at + EPSILON < self.grant_timeout {
            return None;
        }
        self.active = None;
        self.last_release_at = current.granted_at + self.grant_timeout;
        Some(current)
    }

    /// 만료 정리 → active 타임아웃 → 간격 → (priority 내림, enqueued_at 오름, 삽입 순) 하나.
    pub fn next_grant(&mut self, now: f64) -> Option<SeekRequest> {
        self.expire(now);
        if self.active.is_some() || now - self.last_release_at + EPSILON < self.min_spacing {
            return None;
        }
        let mut best: Option<usize> = None;
        for (index, candidate) in self.pending.iter().enumerate() {
            match best {
                None => best = Some(index),
                Some(current) => {
                    let b = &self.pending[current];
                    if candidate.priority > b.priority
                        || (candidate.priority == b.priority
                            && candidate.enqueued_at < b.enqueued_at)
                    {
                        best = Some(index);
                    }
                }
            }
        }
        let granted = self.pending.remove(best?);
        self.active = Some(ActiveSeek {
            request: granted,
            granted_at: now,
        });
        Some(granted)
    }

    /// 다음에 상태가 바뀔 수 있는 시각.
    pub fn next_wakeup(&self, now: f64) -> Option<f64> {
        if let Some(current) = self.active {
            return Some(now.max(current.granted_at + self.grant_timeout));
        }
        if self.pending.is_empty() {
            return None;
        }
        Some(now.max(self.last_release_at + self.min_spacing))
    }
}

/// 부여 콜백 — 요청 id 를 받는다.
pub type GrantCallback = Arc<dyn Fn(i64) + Send + Sync>;

struct ArbiterState {
    queue: MatchWebV2SeekQueue,
    grants: HashMap<SeekKey, GrantCallback>,
    wake_task: Option<JoinHandle<()>>,
    wake_at: Option<f64>,
}

struct ArbiterInner {
    state: Mutex<ArbiterState>,
    clock: SharedClock,
}

/// 전역 시크 토큰 중재자. 부여 콜백은 락 밖에서 호출된다. 깨우기 예약은 tokio 태스크다.
#[derive(Clone)]
pub struct MatchWebV2SeekArbiter {
    inner: Arc<ArbiterInner>,
}

impl MatchWebV2SeekArbiter {
    /// tokio 단조 시계와 기본 대기열로 생성.
    pub fn new() -> Self {
        Self::with(TokioMonotonicClock::new(), MatchWebV2SeekQueue::default())
    }

    pub fn with(clock: SharedClock, queue: MatchWebV2SeekQueue) -> Self {
        Self {
            inner: Arc::new(ArbiterInner {
                state: Mutex::new(ArbiterState {
                    queue,
                    grants: HashMap::new(),
                    wake_task: None,
                    wake_at: None,
                }),
                clock,
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, ArbiterState> {
        self.inner.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn now(&self) -> f64 {
        self.inner.clock.now()
    }

    pub fn request(
        &self,
        owner: OwnerId,
        page_gen: u64,
        request_id: i64,
        priority: i32,
        grant: GrantCallback,
    ) {
        let key = SeekKey {
            owner,
            page_gen,
            request_id,
        };
        let now = self.now();
        {
            let mut s = self.lock();
            let replaced: Vec<SeekKey> = s
                .queue
                .pending
                .iter()
                .filter(|r| r.key.owner == owner)
                .map(|r| r.key)
                .collect();
            for k in replaced {
                s.grants.remove(&k);
            }
            s.queue.enqueue(SeekRequest {
                key,
                priority,
                enqueued_at: now,
            });
            s.grants.insert(key, grant);
        }
        tracing::debug!("matchweb_v2 arbiter request id={request_id} priority={priority}");
        self.pump();
    }

    pub fn release(&self, owner: OwnerId, page_gen: u64, request_id: i64) {
        self.release_key(SeekKey {
            owner,
            page_gen,
            request_id,
        });
    }

    pub fn release_key(&self, key: SeekKey) {
        let now = self.now();
        {
            let mut s = self.lock();
            s.queue.release(key, now);
            s.grants.remove(&key);
        }
        self.pump();
    }

    pub fn cancel_all(&self, owner: OwnerId) {
        let now = self.now();
        {
            let mut s = self.lock();
            s.queue.cancel_all(owner, now);
            s.grants.retain(|k, _| k.owner != owner);
        }
        self.pump();
    }

    /// 이 owner 의 active + 대기 요청 수.
    pub fn pending_count(&self, owner: OwnerId) -> usize {
        let s = self.lock();
        let active = usize::from(s.queue.active.map(|a| a.request.key.owner) == Some(owner));
        active
            + s.queue
                .pending
                .iter()
                .filter(|r| r.key.owner == owner)
                .count()
    }

    /// 이 owner 가 지금 토큰(active)을 쥐고 있는가.
    pub fn is_active(&self, owner: OwnerId) -> bool {
        self.lock().queue.active.map(|a| a.request.key.owner) == Some(owner)
    }

    /// 부여 가능한 요청이 있으면 하나 부여하고, 다음 상태 변화 시각에 깨어나도록 예약한다.
    fn pump(&self) {
        loop {
            let now = self.now();
            let (granted, callback) = {
                let mut s = self.lock();
                if let Some(expired) = s.queue.expire(now) {
                    s.grants.remove(&expired.request.key);
                    tracing::debug!(
                        "matchweb_v2 arbiter auto-release id={}",
                        expired.request.key.request_id
                    );
                }
                let granted = s.queue.next_grant(now);
                let mut live: Vec<SeekKey> = s.queue.pending.iter().map(|r| r.key).collect();
                if let Some(a) = s.queue.active {
                    live.push(a.request.key);
                }
                s.grants.retain(|k, _| live.contains(k));
                self.schedule_wakeup(&mut s, now);
                let callback = granted.and_then(|g| s.grants.get(&g.key).cloned());
                (granted, callback)
            };
            let Some(granted) = granted else { return };
            match callback {
                Some(cb) => {
                    tracing::debug!(
                        "matchweb_v2 arbiter grant id={} priority={}",
                        granted.key.request_id,
                        granted.priority
                    );
                    cb(granted.key.request_id);
                    return;
                }
                None => {
                    // 콜백 없는 부여는 즉시 되돌린다(Swift: release(key:) → pump).
                    let mut s = self.lock();
                    s.queue.release(granted.key, now);
                    s.grants.remove(&granted.key);
                }
            }
        }
    }

    fn schedule_wakeup(&self, s: &mut ArbiterState, now: f64) {
        let Some(at) = s.queue.next_wakeup(now) else {
            if let Some(t) = s.wake_task.take() {
                t.abort();
            }
            s.wake_at = None;
            return;
        };
        if s.wake_at == Some(at) && s.wake_task.is_some() {
            return;
        }
        if let Some(t) = s.wake_task.take() {
            t.abort();
        }
        s.wake_at = Some(at);
        let delay = (at - now).max(0.0);
        let weak: Weak<ArbiterInner> = Arc::downgrade(&self.inner);
        s.wake_task = Some(tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs_f64(delay)).await;
            let Some(inner) = weak.upgrade() else { return };
            let arbiter = MatchWebV2SeekArbiter { inner };
            {
                let mut s = arbiter.lock();
                s.wake_task = None;
                s.wake_at = None;
            }
            arbiter.pump();
        }));
    }
}

impl Default for MatchWebV2SeekArbiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    //! `Tests/CViewPlayerTests/MatchWebV2SeekArbiterTests.swift` 포팅.
    use super::*;

    fn key(owner: OwnerId, id: i64) -> SeekKey {
        key_gen(owner, id, 1)
    }

    fn key_gen(owner: OwnerId, id: i64, page_gen: u64) -> SeekKey {
        SeekKey {
            owner,
            page_gen,
            request_id: id,
        }
    }

    fn req(owner: OwnerId, id: i64, priority: i32, at: f64) -> SeekRequest {
        req_gen(owner, id, priority, at, 1)
    }

    fn req_gen(owner: OwnerId, id: i64, priority: i32, at: f64, page_gen: u64) -> SeekRequest {
        SeekRequest {
            key: key_gen(owner, id, page_gen),
            priority,
            enqueued_at: at,
        }
    }

    #[test]
    fn grants_one_at_a_time() {
        let mut q = MatchWebV2SeekQueue::default();
        q.enqueue(req(1, 1, 0, 0.0));
        q.enqueue(req(2, 1, 0, 0.0));
        assert_eq!(q.next_grant(0.0).map(|r| r.key), Some(key(1, 1)));
        assert_eq!(q.next_grant(0.0), None);
        assert_eq!(q.next_grant(3.0), None);
        assert_eq!(q.active().map(|a| a.request.key), Some(key(1, 1)));
    }

    #[test]
    fn spacing_after_release() {
        let mut q = MatchWebV2SeekQueue::default();
        q.enqueue(req(1, 1, 0, 0.0));
        q.enqueue(req(2, 1, 0, 0.0));
        q.next_grant(0.0);
        q.release(key(1, 1), 0.5);
        assert_eq!(q.last_release_at(), 0.5);
        assert_eq!(q.next_grant(1.49), None);
        assert_eq!(q.next_grant(1.5).map(|r| r.key), Some(key(2, 1)));
    }

    #[test]
    fn priority_then_fifo() {
        let mut q = MatchWebV2SeekQueue::default();
        q.enqueue(req(1, 1, 0, 0.0));
        q.enqueue(req(2, 1, 0, 0.1));
        q.enqueue(req(3, 1, 1, 0.2));
        assert_eq!(q.next_grant(0.2).map(|r| r.key), Some(key(3, 1)));
        q.release(key(3, 1), 0.3);
        assert_eq!(q.next_grant(1.3).map(|r| r.key), Some(key(1, 1)));
        q.release(key(1, 1), 1.4);
        assert_eq!(q.next_grant(2.4).map(|r| r.key), Some(key(2, 1)));
    }

    #[test]
    fn same_owner_replaces() {
        let mut q = MatchWebV2SeekQueue::default();
        q.enqueue(req(1, 1, 0, 0.0));
        q.next_grant(0.0);
        q.enqueue(req(2, 1, 0, 0.1));
        q.enqueue(req(2, 2, 0, 0.2));
        assert_eq!(
            q.pending().iter().map(|r| r.key).collect::<Vec<_>>(),
            vec![key(2, 2)]
        );
    }

    #[test]
    fn grant_timeout_auto_releases() {
        let mut q = MatchWebV2SeekQueue::default();
        q.enqueue(req(1, 1, 0, 0.0));
        q.next_grant(0.0);
        q.enqueue(req(2, 1, 0, 1.0));
        assert_eq!(q.next_grant(3.99), None);
        assert_eq!(q.next_grant(4.5), None);
        assert!(q.active().is_none());
        assert_eq!(q.last_release_at(), 4.0);
        assert_eq!(q.next_grant(5.0).map(|r| r.key), Some(key(2, 1)));
    }

    #[test]
    fn pending_expires() {
        let mut q = MatchWebV2SeekQueue {
            grant_timeout: 100.0,
            ..Default::default()
        };
        q.enqueue(req(1, 1, 0, 0.0));
        q.next_grant(0.0);
        q.enqueue(req(2, 1, 0, 1.0));
        q.next_grant(7.0);
        assert_eq!(q.pending().len(), 1);
        q.next_grant(7.1);
        assert!(q.pending().is_empty());
    }

    #[test]
    fn cancel_all_owner() {
        let mut q = MatchWebV2SeekQueue::default();
        q.enqueue(req(1, 1, 0, 0.0));
        q.next_grant(0.0);
        q.enqueue(req(2, 1, 0, 0.1));
        q.cancel_all(1, 0.2);
        assert!(q.active().is_none());
        assert_eq!(q.last_release_at(), 0.2);
        assert_eq!(q.pending().len(), 1);
        q.cancel_all(2, 0.3);
        assert!(q.pending().is_empty());
        assert_eq!(q.last_release_at(), 0.2);
    }

    #[test]
    fn next_wakeup_values() {
        let mut q = MatchWebV2SeekQueue::default();
        assert_eq!(q.next_wakeup(0.0), None);
        q.enqueue(req(1, 1, 0, 0.0));
        assert_eq!(q.next_wakeup(0.0), Some(0.0));
        q.next_grant(0.0);
        assert_eq!(q.next_wakeup(1.0), Some(4.0));
        q.enqueue(req(2, 1, 0, 1.0));
        q.release(key(1, 1), 2.0);
        assert_eq!(q.next_wakeup(2.0), Some(3.0));
        assert_eq!(q.next_wakeup(3.5), Some(3.5));
    }

    #[test]
    fn page_gen_distinguishes_requests() {
        let mut q = MatchWebV2SeekQueue::default();
        q.enqueue(req_gen(1, 1, 0, 0.0, 1));
        q.next_grant(0.0);
        assert_ne!(key_gen(1, 1, 1), key_gen(1, 1, 2));
        q.release(key_gen(1, 1, 2), 0.5);
        assert_eq!(q.active().map(|a| a.request.key), Some(key_gen(1, 1, 1)));
        q.release(key_gen(1, 1, 1), 0.6);
        assert!(q.active().is_none());
    }

    #[test]
    fn eight_pane_simulation() {
        #[derive(Default, Clone)]
        struct Pane {
            request_id: i64,
            enqueued_at: Option<f64>,
            requeue_at: Option<f64>,
            release_at: Option<f64>,
            grants: u32,
        }
        let mut panes = vec![
            Pane {
                request_id: 1,
                ..Default::default()
            };
            8
        ];
        let mut q = MatchWebV2SeekQueue::default();
        let mut grant_times = Vec::new();
        let mut max_active = 0;
        for (i, p) in panes.iter_mut().enumerate() {
            q.enqueue(req(i as u64, 1, 0, 0.0));
            p.enqueued_at = Some(0.0);
        }
        for step in 0..=400 {
            let now = step as f64 * 0.05;
            for (i, p) in panes.iter_mut().enumerate() {
                let owner = i as u64;
                if let Some(at) = p.release_at {
                    if now >= at - 1e-9 {
                        q.release(key(owner, p.request_id), now);
                        p.release_at = None;
                    }
                }
                if let Some(at) = p.enqueued_at {
                    if now - at > 5.0 + 1e-9 {
                        q.release(key(owner, p.request_id), now);
                        p.enqueued_at = None;
                        p.requeue_at = Some(now + 4.0);
                    }
                }
                if let Some(at) = p.requeue_at {
                    if now >= at - 1e-9 {
                        p.request_id += 1;
                        q.enqueue(req(owner, p.request_id, 0, now));
                        p.requeue_at = None;
                        p.enqueued_at = Some(now);
                    }
                }
            }
            if let Some(granted) = q.next_grant(now) {
                let p = &mut panes[granted.key.owner as usize];
                grant_times.push(now);
                p.grants += 1;
                p.enqueued_at = None;
                p.release_at = Some(now + 0.1);
            }
            max_active = max_active.max(usize::from(q.active().is_some()));
        }
        assert!(panes.iter().all(|p| p.grants >= 1));
        assert_eq!(max_active, 1);
        assert!(*grant_times.last().unwrap() <= 20.0);
        for w in grant_times.windows(2) {
            assert!(w[1] - w[0] >= 1.0 - 1e-9);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn arbiter_integration() {
        let queue = MatchWebV2SeekQueue {
            min_spacing: 0.05,
            grant_timeout: 0.2,
            pending_ttl: 3_600.0,
            ..Default::default()
        };
        let arbiter = MatchWebV2SeekArbiter::with(TokioMonotonicClock::new(), queue);
        let log = Arc::new(Mutex::new(Vec::<String>::new()));
        let logger = |name: &'static str| -> GrantCallback {
            let log = log.clone();
            Arc::new(move |id| log.lock().unwrap().push(format!("{name}#{id}")))
        };
        let entries = || log.lock().unwrap().clone();

        arbiter.request(1, 1, 1, 0, logger("first"));
        assert_eq!(entries(), vec!["first#1"]);
        arbiter.request(2, 1, 7, 0, logger("second"));
        assert_eq!(entries(), vec!["first#1"]);
        assert_eq!(arbiter.pending_count(2), 1);

        arbiter.release(1, 1, 1);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(entries(), vec!["first#1", "second#7"]);

        arbiter.request(3, 2, 3, 1, logger("third"));
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        assert_eq!(entries(), vec!["first#1", "second#7", "third#3"]);
        assert_eq!(arbiter.pending_count(2), 0);
        assert!(arbiter.is_active(3));

        arbiter.cancel_all(3);
        assert_eq!(arbiter.pending_count(3), 0);
    }
}
