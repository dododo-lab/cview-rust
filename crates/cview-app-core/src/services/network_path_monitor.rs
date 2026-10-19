//! `Sources/CViewApp/Services/NetworkPathMonitor.swift` 포팅 — 네트워크 경로 감시(오프라인 배너의 원천).
//!
//! - 오프라인으로 **들어가는** 전환은 1.5초 디바운스한다. 온라인 복귀는 즉시 반영한다.
//! - 원본의 `NWPathMonitor`(macOS Network 프레임워크)는 옮기지 않았다. 셸이 OS 경로 변화를
//!   [`PathState`] 로 만들어 [`NetworkPathMonitor::start`] 의 채널이나 [`NetworkPathMonitor::handle_path`]
//!   로 넣는다. 오프라인일 때 폴을 건너뛰는 일은 `RefreshCoordinator` 영역이다.

use std::sync::{Arc, Mutex};

use cview_core::utils::time::{Date, TimeInterval};
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

use super::refresh_coordinator::{system_sleep, Sleep};
use super::support::{lock, system_clock, Clock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NetworkInterface {
    Wifi,
    Wired,
    Cellular,
    #[default]
    Other,
}

impl NetworkInterface {
    pub fn label(self) -> &'static str {
        match self {
            Self::Wifi => "Wi-Fi",
            Self::Wired => "유선 네트워크",
            Self::Cellular => "셀룰러",
            Self::Other => "기타 네트워크",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PathState {
    pub is_offline: bool,
    pub is_constrained: bool,
    pub is_expensive: bool,
    pub interface: NetworkInterface,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    Checking,
    Online,
    Offline,
}

impl ConnectionStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Checking => "확인 중",
            Self::Online => "온라인",
            Self::Offline => "오프라인",
        }
    }
}

#[derive(Default)]
struct State {
    is_offline: bool,
    since: Option<Date>,
    current_path: Option<PathState>,
    consume_task: Option<AbortHandle>,
    pending_offline: Option<(u64, AbortHandle)>,
    pending_serial: u64,
}

/// 네트워크 경로 감시. `Clone` 은 같은 감시를 가리킨다.
#[derive(Clone)]
pub struct NetworkPathMonitor {
    state: Arc<Mutex<State>>,
    debounce: TimeInterval,
    sleep: Sleep,
    now: Clock,
}

impl Default for NetworkPathMonitor {
    fn default() -> Self {
        Self::new(Self::OFFLINE_DEBOUNCE, system_sleep(), system_clock())
    }
}

impl NetworkPathMonitor {
    /// 오프라인으로 들어가는 전환의 디바운스(초).
    pub const OFFLINE_DEBOUNCE: TimeInterval = 1.5;

    pub fn new(debounce: TimeInterval, sleep: Sleep, now: Clock) -> Self {
        Self {
            state: Arc::new(Mutex::new(State::default())),
            debounce,
            sleep,
            now,
        }
    }

    /// 디바운스를 통과한 오프라인 여부.
    pub fn is_offline(&self) -> bool {
        lock(&self.state).is_offline
    }
    /// 현재 상태로 바뀐 시각.
    pub fn since(&self) -> Option<Date> {
        lock(&self.state).since
    }
    /// 첫 경로 응답 전에는 `None`.
    pub fn current_path(&self) -> Option<PathState> {
        lock(&self.state).current_path
    }

    pub fn connection_status(&self) -> ConnectionStatus {
        let st = lock(&self.state);
        let Some(path) = st.current_path else {
            return ConnectionStatus::Checking;
        };
        if path.is_offline && !st.is_offline {
            return ConnectionStatus::Checking;
        }
        if st.is_offline {
            ConnectionStatus::Offline
        } else {
            ConnectionStatus::Online
        }
    }

    /// 운영체제의 힌트만 파생한다. 사용자의 작업 절약 설정은 소비자가 적용한다.
    pub fn should_conserve_data(&self) -> bool {
        match lock(&self.state).current_path {
            Some(p) if !p.is_offline => p.is_constrained || p.is_expensive,
            _ => false,
        }
    }

    /// 감시 시작 — 여러 번 불러도 한 번만 시작한다.
    pub fn start(&self, mut paths: mpsc::Receiver<PathState>) {
        let mut st = lock(&self.state);
        if st.consume_task.is_some() {
            return;
        }
        let me = self.clone();
        let handle = tokio::spawn(async move {
            while let Some(path) = paths.recv().await {
                me.handle_path(path);
            }
        });
        st.consume_task = Some(handle.abort_handle());
    }

    /// 경로 하나를 반영한다.
    pub fn handle_path(&self, path: PathState) {
        let mut st = lock(&self.state);
        st.current_path = Some(path);
        if !path.is_offline {
            // 복구 — 대기 중이던 오프라인 진입은 취소하고, 이미 오프라인이면 즉시 해제한다.
            if let Some((_, h)) = st.pending_offline.take() {
                h.abort();
            }
            if st.is_offline {
                st.is_offline = false;
                st.since = Some((self.now)());
            }
            return;
        }
        if st.is_offline || st.pending_offline.is_some() {
            return;
        }
        st.pending_serial += 1;
        let serial = st.pending_serial;
        let me = self.clone();
        let wait = self.debounce;
        let handle = tokio::spawn(async move {
            (me.sleep)(wait).await;
            let mut st = lock(&me.state);
            if st.pending_offline.as_ref().map(|(s, _)| *s) != Some(serial) {
                return;
            }
            st.pending_offline = None;
            st.is_offline = true;
            st.since = Some((me.now)());
        });
        st.pending_offline = Some((serial, handle.abort_handle()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cview_core::utils::time::from_unix;
    use futures_util::FutureExt;

    fn offline() -> PathState {
        PathState {
            is_offline: true,
            ..Default::default()
        }
    }

    fn monitor(sleep: Sleep) -> NetworkPathMonitor {
        let clock: Clock = Arc::new(|| from_unix(100.0));
        NetworkPathMonitor::new(1.5, sleep, clock)
    }

    async fn settle() {
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test]
    async fn offline_is_debounced_and_recovery_is_immediate() {
        let m = monitor(Arc::new(|_| async {}.boxed()));
        assert_eq!(m.connection_status(), ConnectionStatus::Checking);
        m.handle_path(PathState::default());
        assert_eq!(m.connection_status(), ConnectionStatus::Online);
        m.handle_path(offline());
        assert_eq!(
            m.connection_status(),
            ConnectionStatus::Checking,
            "디바운스 중"
        );
        settle().await;
        assert!(m.is_offline());
        assert_eq!(m.connection_status(), ConnectionStatus::Offline);
        m.handle_path(PathState {
            is_expensive: true,
            ..Default::default()
        });
        assert!(!m.is_offline(), "복구는 즉시");
        assert!(m.should_conserve_data());
    }

    #[tokio::test]
    async fn blip_shorter_than_debounce_never_shows_offline() {
        let m = monitor(Arc::new(|_| futures_util::future::pending().boxed()));
        let (tx, rx) = mpsc::channel(4);
        m.start(rx);
        tx.send(offline()).await.unwrap();
        tx.send(PathState::default()).await.unwrap();
        drop(tx);
        settle().await;
        assert!(!m.is_offline());
        assert_eq!(m.connection_status(), ConnectionStatus::Online);
    }
}
