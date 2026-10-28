//! `Sources/CViewApp/Services/WatchHeartbeatCenter.swift` 포팅 — 치지직 시청 하트비트.
//!
//! 재생 중 `live-status`(v3)를 서버 지정 주기(5~30초 클램프)로 인증 폴링해 시청자 집계·시청기록에
//! 반영한다. 채널 단위 참조카운트([`WatchHeartbeatRefState`])로 같은 채널을 보는 화면이 여럿이어도
//! 루프는 하나다. 방송 종료(CLOSE)면 30초로 늦춰 계속 폴링하고, 연속 3회 실패부터 ×1.5 백오프(상한 30초).
//!
//! 원본의 `RequestOrigin.heartbeat` 태그는 Rust 네트워킹에 요청 장부가 없어 생략했다.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use async_trait::async_trait;
use chrono::Utc;
use cview_core::error::ApiError;
use cview_core::models::live::{LiveInfo, LiveStatus};
use cview_core::utils::time::{Date, TimeInterval};
use cview_networking::ChzzkApiClient;
use tokio::task::AbortHandle;

use super::refresh_coordinator::{system_sleep, Sleep};
use super::support::lock;

/// 채널별 하트비트 참조 상태 — 폴링 루프의 수명은 오직 이 상태가 결정한다.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WatchHeartbeatRefState {
    ref_count: i64,
    suspend_count: i64,
}

impl WatchHeartbeatRefState {
    pub fn ref_count(&self) -> i64 {
        self.ref_count
    }
    pub fn suspend_count(&self) -> i64 {
        self.suspend_count
    }
    /// 정지되지 않은 참조가 하나라도 있으면 폴링해야 한다.
    pub fn should_poll(&self) -> bool {
        self.ref_count > 0 && self.suspend_count < self.ref_count
    }
    pub fn is_released(&self) -> bool {
        self.ref_count <= 0
    }
    pub fn retain(&mut self) {
        self.ref_count += 1;
    }
    /// `was_suspended`: 해제되는 그 참조가 정지 상태였는지.
    pub fn release(&mut self, was_suspended: bool) {
        self.ref_count -= 1;
        if was_suspended {
            self.suspend_count = (self.suspend_count - 1).max(0);
        }
        self.suspend_count = self.suspend_count.min(self.ref_count.max(0));
    }
    pub fn suspend(&mut self) {
        self.suspend_count = self.ref_count.min(self.suspend_count + 1);
    }
    pub fn resume(&mut self) {
        self.suspend_count = (self.suspend_count - 1).max(0);
    }
}

/// live-status 조회원(테스트는 가짜를 넣는다).
#[async_trait]
pub trait LiveStatusSource: Send + Sync {
    async fn live_status(&self, channel_id: &str) -> Result<LiveInfo, ApiError>;
}

#[async_trait]
impl LiveStatusSource for ChzzkApiClient {
    async fn live_status(&self, channel_id: &str) -> Result<LiveInfo, ApiError> {
        ChzzkApiClient::live_status(self, channel_id).await
    }
}

struct Entry {
    /// `None` = 폴링 정지 상태.
    task: Option<AbortHandle>,
    state: WatchHeartbeatRefState,
    latest_status: Option<LiveInfo>,
    latest_status_received_at: Option<Date>,
}

#[derive(Default)]
struct Inner {
    api: Option<Weak<dyn LiveStatusSource>>,
    entries: HashMap<String, Entry>,
}

/// 하트비트 센터. `Clone` 은 같은 센터를 가리킨다.
#[derive(Clone)]
pub struct WatchHeartbeatCenter {
    inner: Arc<Mutex<Inner>>,
    sleep: Sleep,
}

impl Default for WatchHeartbeatCenter {
    fn default() -> Self {
        Self::new(system_sleep())
    }
}

impl WatchHeartbeatCenter {
    /// 서버가 주기를 알려주기 전 기본값(초).
    pub const DEFAULT_INTERVAL: TimeInterval = 10.0;
    /// 방송 종료 상태의 폴링 주기(초).
    pub const CLOSED_INTERVAL: TimeInterval = 30.0;
    /// 화면 폴러가 하트비트 상태를 재사용하는 기본 최대 나이(초).
    pub const DEFAULT_REUSE_MAX_AGE: TimeInterval = 15.0;

    pub fn new(sleep: Sleep) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner::default())),
            sleep,
        }
    }

    /// 앱 부팅 시 1회 주입(약한 참조).
    pub fn configure(&self, api: &Arc<dyn LiveStatusSource>) {
        lock(&self.inner).api = Some(Arc::downgrade(api));
    }

    pub fn loop_count(&self) -> usize {
        lock(&self.inner).entries.len()
    }

    pub fn is_polling(&self, channel_id: &str) -> bool {
        lock(&self.inner)
            .entries
            .get(channel_id)
            .is_some_and(|e| e.task.is_some())
    }

    pub fn ref_state(&self, channel_id: &str) -> Option<WatchHeartbeatRefState> {
        lock(&self.inner).entries.get(channel_id).map(|e| e.state)
    }

    /// 상태 → task 정합. 모든 변경 경로가 이 한 곳을 지난다.
    fn reconcile(&self, channel_id: &str, entry: &mut Entry) {
        if entry.state.should_poll() {
            if entry.task.is_none() {
                let me = self.clone();
                let id = channel_id.to_string();
                entry.task =
                    Some(tokio::spawn(async move { me.run_loop(id).await }).abort_handle());
            }
        } else if let Some(t) = entry.task.take() {
            t.abort();
        }
    }

    /// 채널 재생 시작 — 참조 +1. 첫 참조면 루프를 띄운다.
    pub fn begin_watching(&self, channel_id: &str) {
        if channel_id.is_empty() {
            return;
        }
        let mut inner = lock(&self.inner);
        let is_new = !inner.entries.contains_key(channel_id);
        let mut entry = inner.entries.remove(channel_id).unwrap_or(Entry {
            task: None,
            state: WatchHeartbeatRefState::default(),
            latest_status: None,
            latest_status_received_at: None,
        });
        entry.state.retain();
        self.reconcile(channel_id, &mut entry);
        inner.entries.insert(channel_id.to_string(), entry);
        if is_new {
            tracing::info!(
                "WatchHeartbeat: begin {} (loops={})",
                prefix8(channel_id),
                inner.entries.len()
            );
        }
    }

    /// 채널 재생 종료 — 참조 -1. 0 이 되면 루프 취소·제거.
    pub fn end_watching(&self, channel_id: &str, was_suspended: bool) {
        let mut inner = lock(&self.inner);
        let Some(mut entry) = inner.entries.remove(channel_id) else {
            return;
        };
        entry.state.release(was_suspended);
        if entry.state.is_released() {
            if let Some(t) = entry.task.take() {
                t.abort();
            }
            tracing::info!(
                "WatchHeartbeat: end {} (loops={})",
                prefix8(channel_id),
                inner.entries.len()
            );
            return;
        }
        self.reconcile(channel_id, &mut entry);
        inner.entries.insert(channel_id.to_string(), entry);
    }

    /// 재생이 실제로 정지된 동안 폴링만 중단한다(등록은 유지).
    pub fn suspend(&self, channel_id: &str) {
        self.mutate(channel_id, |s| s.suspend());
    }

    /// `suspend` 의 짝. 재개 시 루프가 처음부터 시작해 즉시 1회 폴링한다.
    pub fn resume(&self, channel_id: &str) {
        self.mutate(channel_id, |s| s.resume());
    }

    fn mutate(&self, channel_id: &str, f: impl FnOnce(&mut WatchHeartbeatRefState)) {
        let mut inner = lock(&self.inner);
        let Some(mut entry) = inner.entries.remove(channel_id) else {
            return;
        };
        f(&mut entry.state);
        self.reconcile(channel_id, &mut entry);
        inner.entries.insert(channel_id.to_string(), entry);
    }

    /// 하트비트 주기 안에서만 화면 폴러가 최신 상태를 공유한다.
    pub fn recent_live_status(
        &self,
        channel_id: &str,
        max_age: TimeInterval,
        now: Date,
    ) -> Option<LiveInfo> {
        if max_age < 0.0 {
            return None;
        }
        let inner = lock(&self.inner);
        let entry = inner.entries.get(channel_id)?;
        let status = entry.latest_status.as_ref()?;
        let at = entry.latest_status_received_at?;
        Self::is_reusable_status(at, now, max_age).then(|| status.clone())
    }

    /// 미래 timestamp(시계 역행)는 재사용하지 않는다.
    pub fn is_reusable_status(received_at: Date, now: Date, max_age: TimeInterval) -> bool {
        if max_age < 0.0 {
            return false;
        }
        let age = (now - received_at).num_milliseconds() as f64 / 1000.0;
        age >= 0.0 && age <= max_age
    }

    fn record_latest_status(&self, status: LiveInfo, channel_id: &str) {
        if let Some(entry) = lock(&self.inner).entries.get_mut(channel_id) {
            entry.latest_status = Some(status);
            entry.latest_status_received_at = Some(Utc::now());
        }
    }

    /// 다음 주기 — 성공이면 서버 지정값(종료 상태면 30초), 실패가 3회 이상 이어지면 ×1.5(상한 30초).
    pub fn next_interval(
        current: TimeInterval,
        outcome: Result<(LiveStatus, TimeInterval), ()>,
        consecutive_failures: &mut u32,
    ) -> TimeInterval {
        match outcome {
            Ok((status, server_interval)) => {
                *consecutive_failures = 0;
                if status == LiveStatus::Close {
                    Self::CLOSED_INTERVAL
                } else {
                    server_interval
                }
            }
            Err(()) => {
                *consecutive_failures += 1;
                if *consecutive_failures >= 3 {
                    (current * 1.5).min(30.0)
                } else {
                    current
                }
            }
        }
    }

    async fn run_loop(self, channel_id: String) {
        let mut interval = Self::DEFAULT_INTERVAL;
        let mut failures = 0u32;
        loop {
            let api = lock(&self.inner).api.as_ref().and_then(Weak::upgrade);
            let Some(api) = api else { break };
            let outcome = match api.live_status(&channel_id).await {
                Ok(info) => {
                    let status = info.status;
                    let server = info.watch_heartbeat_interval().as_secs_f64();
                    self.record_latest_status(info, &channel_id);
                    Ok((status, server))
                }
                Err(_) => Err(()),
            };
            interval = Self::next_interval(interval, outcome, &mut failures);
            (self.sleep)(interval).await;
        }
    }
}

fn prefix8(s: &str) -> &str {
    s.char_indices().nth(8).map_or(s, |(i, _)| &s[..i])
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/WatchHeartbeatRefStateTests.swift`
    use super::*;
    use chrono::Duration;
    use cview_core::utils::time::from_unix;
    use futures_util::FutureExt;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn reusable_status_window() {
        let now = from_unix(1_000.0);
        assert!(WatchHeartbeatCenter::is_reusable_status(
            now - Duration::seconds(15),
            now,
            15.0
        ));
        assert!(!WatchHeartbeatCenter::is_reusable_status(
            now - Duration::seconds(16),
            now,
            15.0
        ));
        assert!(!WatchHeartbeatCenter::is_reusable_status(
            now + Duration::seconds(1),
            now,
            15.0
        ));
        assert!(!WatchHeartbeatCenter::is_reusable_status(now, now, -1.0));
    }

    #[test]
    fn ref_state_rules() {
        let mut s = WatchHeartbeatRefState::default();
        assert!(!s.should_poll());
        s.retain();
        assert!(s.should_poll() && !s.is_released());
        s.suspend();
        s.suspend();
        assert!(!s.should_poll());
        s.resume();
        assert!(
            s.should_poll(),
            "중복 suspend 가 하트비트를 영구 사망시켰다"
        );

        let mut s = WatchHeartbeatRefState::default();
        s.retain();
        s.retain();
        s.suspend();
        assert!(s.should_poll());
        s.release(false);
        assert!(!s.is_released() && !s.should_poll(), "정지된 참조만 남았다");

        let mut s = WatchHeartbeatRefState::default();
        s.retain();
        s.retain();
        s.suspend();
        s.release(true);
        assert!(
            s.should_poll(),
            "정지된 참조 해제가 활성 참조의 폴링까지 죽였다"
        );

        let mut s = WatchHeartbeatRefState::default();
        s.retain();
        s.retain();
        s.suspend();
        s.suspend();
        s.release(true);
        assert!(!s.should_poll());
        s.release(true);
        assert!(s.is_released());
        assert_eq!(s.suspend_count(), 0);
    }

    #[test]
    fn interval_backoff() {
        let mut f = 0;
        let mut i = WatchHeartbeatCenter::DEFAULT_INTERVAL;
        i = WatchHeartbeatCenter::next_interval(i, Err(()), &mut f);
        i = WatchHeartbeatCenter::next_interval(i, Err(()), &mut f);
        assert_eq!(i, 10.0);
        i = WatchHeartbeatCenter::next_interval(i, Err(()), &mut f);
        assert_eq!(i, 15.0);
        i = WatchHeartbeatCenter::next_interval(i, Err(()), &mut f);
        assert_eq!(i, 22.5);
        i = WatchHeartbeatCenter::next_interval(i, Err(()), &mut f);
        assert_eq!(i, 30.0);
        let i = WatchHeartbeatCenter::next_interval(i, Ok((LiveStatus::Close, 10.0)), &mut f);
        assert_eq!((i, f), (30.0, 0));
        let i = WatchHeartbeatCenter::next_interval(i, Ok((LiveStatus::Open, 7.0)), &mut f);
        assert_eq!(i, 7.0);
    }

    struct FailingSource(AtomicUsize);

    #[async_trait]
    impl LiveStatusSource for FailingSource {
        async fn live_status(&self, _: &str) -> Result<LiveInfo, ApiError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(ApiError::InvalidResponse)
        }
    }

    #[tokio::test]
    async fn center_dedups_loops_and_suspends() {
        let source = Arc::new(FailingSource(AtomicUsize::new(0)));
        let api: Arc<dyn LiveStatusSource> = source.clone();
        // 잠은 영원히 — 루프는 첫 폴 한 번만 한다.
        let center =
            WatchHeartbeatCenter::new(Arc::new(|_| futures_util::future::pending().boxed()));
        center.configure(&api);
        center.begin_watching("ch");
        center.begin_watching("ch");
        assert_eq!(center.loop_count(), 1);
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
        assert_eq!(source.0.load(Ordering::SeqCst), 1, "같은 채널은 루프 하나");
        center.suspend("ch");
        assert!(center.is_polling("ch"), "다른 화면이 아직 재생 중");
        center.suspend("ch");
        assert!(!center.is_polling("ch"));
        center.resume("ch");
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
        assert_eq!(source.0.load(Ordering::SeqCst), 2, "재개는 즉시 1회 폴링");
        center.end_watching("ch", true);
        center.end_watching("ch", false);
        assert_eq!(center.loop_count(), 0);
        assert!(center.recent_live_status("ch", 15.0, Utc::now()).is_none());
    }
}
