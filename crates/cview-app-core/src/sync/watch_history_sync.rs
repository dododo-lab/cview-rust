//! `Sources/CViewApp/Sync/WatchHistorySyncService.swift` (+ `AppState+Sync.swift` 의 설정 반영) 포팅.
//!
//! cv.dododo.app push 전용 동기화.
//!
//! - 미동기화 `WatchHistory` 행을 주기적으로 batch POST 한다.
//! - 성공 시 서버가 accept 한 id 만 보낸 판본(`updated_at`)으로 synced 표시한다 — 부분 성공 안전.
//! - 실패 시 지수 백오프(1s → 2s → 4s … 상한 5분), 다음 틱에 재시도.
//! - 설정 OFF / endpoint 미설정 / install ID 미발급 → no-op.
//!
//! 서버 계약: `POST {endpoint}/api/sync/watch-history`, 헤더 `Idempotency-Key: <uuid>`,
//! 본문 `{"installId": "<UUID>", "rows": [...]}`, 응답 200/202 `{"success": true, "acceptedIds": [...]}`.
//!
//! push 는 단일 실행 memo 로 직렬화한다 — 겹친 두 push 가 같은 batch 를 서로 다른 키로 두 번
//! 올리는 일이 없다. 도는 동안 들어온 요청은 끝난 뒤 한 번 더 돌게 표시한다.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cview_core::protocols::HttpMethod;
use cview_core::utils::time::{Date, TimeInterval};
use cview_networking::{HttpRequest, HttpTransport};
use cview_persistence::models::WatchHistorySyncRow;
use cview_persistence::DataStore;
use futures_util::future::{BoxFuture, FutureExt, Shared};
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use super::install_identity::InstallIdentity;
use crate::services::support::{lock, system_clock, Clock};

/// 동기화 큐 저장소 경계 — 운영은 `DataStore`.
pub trait WatchHistorySyncStore: Send + Sync {
    fn unsynced_count(&self) -> Result<i64, String>;
    fn fetch_unsynced(&self, limit: usize) -> Result<Vec<WatchHistorySyncRow>, String>;
    fn mark_synced(&self, versions: &HashMap<String, Date>) -> Result<(), String>;
}

impl WatchHistorySyncStore for DataStore {
    fn unsynced_count(&self) -> Result<i64, String> {
        self.unsynced_watch_history_count()
            .map_err(|e| e.to_string())
    }
    fn fetch_unsynced(&self, limit: usize) -> Result<Vec<WatchHistorySyncRow>, String> {
        self.fetch_unsynced_watch_history(limit)
            .map_err(|e| e.to_string())
    }
    fn mark_synced(&self, versions: &HashMap<String, Date>) -> Result<(), String> {
        self.mark_watch_history_synced(versions)
            .map_err(|e| e.to_string())
    }
}

/// UI 에 노출할 sync 상태 스냅샷.
#[derive(Debug, Clone, PartialEq)]
pub struct WatchHistorySyncStatus {
    pub enabled: bool,
    pub pushed_row_count: usize,
    pub queue_depth: i64,
    pub last_attempt_at: Option<Date>,
    pub last_success_at: Option<Date>,
    pub last_error_message: Option<String>,
    pub backoff_seconds: TimeInterval,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SyncConfig {
    /// 서버 base URL (예: "https://cv.dododo.app"). `None` 이면 서비스 비활성.
    pub endpoint: Option<Url>,
    /// 사용자 동기화 토글 — 기본 OFF.
    pub enabled: bool,
    /// batch 당 최대 row 수.
    pub batch_size: usize,
    /// 정상 동작 시 tick 주기(초).
    pub tick_interval: TimeInterval,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            endpoint: None,
            enabled: false,
            batch_size: 50,
            tick_interval: 60.0,
        }
    }
}

impl SyncConfig {
    /// `AppState.applyWatchHistorySyncSettings` — 설정 문자열 endpoint 를 해석한다.
    pub fn from_settings(endpoint: &str, enabled: bool) -> Self {
        Self {
            endpoint: Url::parse(endpoint).ok(),
            enabled,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SyncError {
    #[error("Invalid response")]
    InvalidResponse,
    #[error("Server endpoint not ready (404)")]
    EndpointNotReady,
    #[error("Unauthorized (401/403)")]
    Unauthorized,
    #[error("Server error {0}")]
    ServerError(u16),
    #[error("Unexpected status {0}")]
    UnexpectedStatus(u16),
    #[error("{0}")]
    Transport(String),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncRequestBody<'a> {
    install_id: &'a str,
    rows: &'a [WatchHistorySyncRow],
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncResponseBody {
    accepted_ids: Vec<String>,
}

type SharedPush = Shared<BoxFuture<'static, ()>>;

struct State {
    config: SyncConfig,
    loop_task: Option<tokio::task::JoinHandle<()>>,
    push_in_flight: Option<SharedPush>,
    push_requested_while_in_flight: bool,
    backoff_seconds: TimeInterval,
    pushed_row_count: usize,
    last_attempt_at: Option<Date>,
    last_success_at: Option<Date>,
    last_error_message: Option<String>,
}

/// 시청 기록 동기화 서비스 핸들.
#[derive(Clone)]
pub struct WatchHistorySyncService {
    store: Arc<dyn WatchHistorySyncStore>,
    transport: Arc<dyn HttpTransport>,
    identity: Arc<InstallIdentity>,
    clock: Clock,
    state: Arc<Mutex<State>>,
}

impl WatchHistorySyncService {
    pub const MAX_BACKOFF: TimeInterval = 300.0;
    pub const PATH: &'static str = "/api/sync/watch-history";

    pub fn new(
        store: Arc<dyn WatchHistorySyncStore>,
        transport: Arc<dyn HttpTransport>,
        identity: Arc<InstallIdentity>,
        config: SyncConfig,
    ) -> Self {
        Self::with_clock(store, transport, identity, config, system_clock())
    }

    pub fn with_clock(
        store: Arc<dyn WatchHistorySyncStore>,
        transport: Arc<dyn HttpTransport>,
        identity: Arc<InstallIdentity>,
        config: SyncConfig,
        clock: Clock,
    ) -> Self {
        Self {
            store,
            transport,
            identity,
            clock,
            state: Arc::new(Mutex::new(State {
                config,
                loop_task: None,
                push_in_flight: None,
                push_requested_while_in_flight: false,
                backoff_seconds: 0.0,
                pushed_row_count: 0,
                last_attempt_at: None,
                last_success_at: None,
                last_error_message: None,
            })),
        }
    }

    pub fn config(&self) -> SyncConfig {
        lock(&self.state).config.clone()
    }

    /// 설정 갱신 — 토글 ON/OFF 또는 endpoint 변경 시.
    pub fn update_config(&self, new: SyncConfig) {
        let was_running = lock(&self.state).loop_task.is_some();
        let should_start = new.enabled && new.endpoint.is_some();
        lock(&self.state).config = new;
        if was_running {
            self.stop();
        }
        if should_start {
            self.start();
        }
    }

    /// 설정 반영 후 켜져 있으면 바로 한 번 민다(`AppState.applyWatchHistorySyncSettings`).
    pub fn apply_settings(&self, endpoint: &str, enabled: bool) {
        self.update_config(SyncConfig::from_settings(endpoint, enabled));
        if enabled {
            self.nudge();
        }
    }

    /// 주기 루프 시작. tokio 런타임 안에서 불러야 한다.
    pub fn start(&self) {
        let mut state = lock(&self.state);
        if state.loop_task.is_some() || !state.config.enabled || state.config.endpoint.is_none() {
            return;
        }
        let this = self.clone();
        state.loop_task = Some(tokio::spawn(async move { this.run_loop().await }));
    }

    pub fn stop(&self) {
        if let Some(task) = lock(&self.state).loop_task.take() {
            task.abort();
        }
    }

    pub fn is_running(&self) -> bool {
        lock(&self.state).loop_task.is_some()
    }

    /// 외부에서 즉시 sync — 시청 종료 직후 등.
    pub fn nudge(&self) {
        {
            let state = lock(&self.state);
            if !state.config.enabled || state.config.endpoint.is_none() {
                return;
            }
        }
        let this = self.clone();
        tokio::spawn(async move { this.push_once_coalesced().await });
    }

    pub fn current_status(&self) -> WatchHistorySyncStatus {
        let queue_depth = self.store.unsynced_count().unwrap_or(0);
        let state = lock(&self.state);
        WatchHistorySyncStatus {
            enabled: state.config.enabled,
            pushed_row_count: state.pushed_row_count,
            queue_depth,
            last_attempt_at: state.last_attempt_at,
            last_success_at: state.last_success_at,
            last_error_message: state.last_error_message.clone(),
            backoff_seconds: state.backoff_seconds,
        }
    }

    /// push 단일 실행기 — 이미 도는 push 가 있으면 그것을 기다리고 끝난 뒤 한 번 더 돌게 표시한다.
    pub async fn push_once_coalesced(&self) {
        let existing = {
            let mut state = lock(&self.state);
            state.push_in_flight.clone().inspect(|_| {
                state.push_requested_while_in_flight = true;
            })
        };
        if let Some(running) = existing {
            running.await;
            return;
        }
        loop {
            let push = {
                let mut state = lock(&self.state);
                state.push_requested_while_in_flight = false;
                let this = self.clone();
                let push: SharedPush = async move { this.push_once().await }.boxed().shared();
                state.push_in_flight = Some(push.clone());
                push
            };
            push.await;
            let again = {
                let mut state = lock(&self.state);
                state.push_in_flight = None;
                state.push_requested_while_in_flight
                    && state.backoff_seconds == 0.0
                    && state.config.enabled
            };
            if !again {
                break;
            }
        }
    }

    async fn run_loop(&self) {
        loop {
            self.push_once_coalesced().await;
            let delay = {
                let state = lock(&self.state);
                if state.backoff_seconds > 0.0 {
                    state.backoff_seconds
                } else {
                    state.config.tick_interval
                }
            };
            tokio::time::sleep(Duration::from_secs_f64(delay.max(0.0))).await;
        }
    }

    /// 1회 push — 미동기화 행을 한 batch 만큼 끌어와 전송.
    async fn push_once(&self) {
        let (endpoint, batch_size) = {
            let state = lock(&self.state);
            match (&state.config.endpoint, state.config.enabled) {
                (Some(e), true) => (e.clone(), state.config.batch_size),
                _ => return,
            }
        };
        let install_id = self.identity.install_id().await;
        if install_id.is_empty() {
            return;
        }
        lock(&self.state).last_attempt_at = Some((self.clock)());
        let rows = match self.store.fetch_unsynced(batch_size) {
            Ok(rows) => rows,
            Err(e) => {
                self.record_failure(format!("fetchUnsynced: {e}"));
                return;
            }
        };
        if rows.is_empty() {
            // 큐가 빈 정상 상태 — 백오프 초기화.
            lock(&self.state).backoff_seconds = 0.0;
            return;
        }
        match self.post_batch(&rows, &endpoint, &install_id).await {
            Ok(accepted_ids) => {
                if !accepted_ids.is_empty() {
                    // 보낸 판본으로 표시한다 — POST 중에 바뀐 행은 다음 push 가 다시 싣는다.
                    let mut pushed: HashMap<String, Date> = HashMap::new();
                    for row in &rows {
                        let entry = pushed.entry(row.id.clone()).or_insert(row.updated_at);
                        if row.updated_at > *entry {
                            *entry = row.updated_at;
                        }
                    }
                    let versions: HashMap<String, Date> = pushed
                        .into_iter()
                        .filter(|(id, _)| accepted_ids.contains(id))
                        .collect();
                    if let Err(e) = self.store.mark_synced(&versions) {
                        self.record_failure(e);
                        return;
                    }
                    lock(&self.state).pushed_row_count += accepted_ids.len();
                }
                let now = (self.clock)();
                let mut state = lock(&self.state);
                state.last_success_at = Some(now);
                state.last_error_message = None;
                state.backoff_seconds = 0.0;
            }
            Err(e) => self.record_failure(e.to_string()),
        }
    }

    async fn post_batch(
        &self,
        rows: &[WatchHistorySyncRow],
        endpoint: &Url,
        install_id: &str,
    ) -> Result<Vec<String>, SyncError> {
        let mut url = endpoint.clone();
        let base = url.path().trim_end_matches('/').to_string();
        url.set_path(&format!("{base}{}", Self::PATH));
        let mut request = HttpRequest::new(HttpMethod::Post, url);
        request.set_header("Content-Type", Some("application/json"));
        request.set_header(
            "Idempotency-Key",
            Some(&Uuid::new_v4().to_string().to_uppercase()),
        );
        request.timeout = Some(Duration::from_secs(15));
        request.body = Some(
            serde_json::to_vec(&SyncRequestBody { install_id, rows })
                .map_err(|e| SyncError::Transport(e.to_string()))?,
        );
        let response = self
            .transport
            .send(request)
            .await
            .map_err(|e| SyncError::Transport(e.to_string()))?;
        match response.status {
            200 | 202 => serde_json::from_slice::<SyncResponseBody>(&response.body)
                .map(|b| b.accepted_ids)
                .map_err(|_| SyncError::InvalidResponse),
            404 => Err(SyncError::EndpointNotReady),
            401 | 403 => Err(SyncError::Unauthorized),
            500..=599 => Err(SyncError::ServerError(response.status)),
            other => Err(SyncError::UnexpectedStatus(other)),
        }
    }

    fn record_failure(&self, message: String) {
        let mut state = lock(&self.state);
        state.last_error_message = Some(message);
        state.backoff_seconds = Self::next_backoff(state.backoff_seconds);
    }

    /// 1 → 2 → 4 … 상한 300초.
    pub fn next_backoff(current: TimeInterval) -> TimeInterval {
        if current == 0.0 {
            1.0
        } else {
            (current * 2.0).min(Self::MAX_BACKOFF)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use cview_auth::secret_store::KeychainService;
    use cview_core::utils::time::from_unix;
    use cview_networking::{HttpResponse, TransportError};
    use tokio::sync::Semaphore;

    #[derive(Default)]
    struct FakeStore {
        rows: Mutex<Vec<WatchHistorySyncRow>>,
        marked: Mutex<Vec<HashMap<String, Date>>>,
    }

    impl WatchHistorySyncStore for FakeStore {
        fn unsynced_count(&self) -> Result<i64, String> {
            Ok(lock(&self.rows).len() as i64)
        }
        fn fetch_unsynced(&self, limit: usize) -> Result<Vec<WatchHistorySyncRow>, String> {
            Ok(lock(&self.rows).iter().take(limit).cloned().collect())
        }
        fn mark_synced(&self, versions: &HashMap<String, Date>) -> Result<(), String> {
            lock(&self.rows).retain(|r| !versions.contains_key(&r.id));
            lock(&self.marked).push(versions.clone());
            Ok(())
        }
    }

    struct FakeTransport {
        status: Mutex<u16>,
        accept: Mutex<Option<Vec<String>>>,
        requests: Mutex<Vec<HttpRequest>>,
        hold: bool,
        gate: Semaphore,
    }

    impl FakeTransport {
        fn new(status: u16, hold: bool) -> Arc<Self> {
            Arc::new(Self {
                status: Mutex::new(status),
                accept: Mutex::new(None),
                requests: Mutex::new(Vec::new()),
                hold,
                gate: Semaphore::new(0),
            })
        }
    }

    #[async_trait]
    impl HttpTransport for FakeTransport {
        async fn send(&self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
            let body: serde_json::Value =
                serde_json::from_slice(request.body.as_deref().unwrap()).unwrap();
            lock(&self.requests).push(request);
            if self.hold {
                self.gate.acquire().await.unwrap().forget();
            }
            let ids: Vec<String> = lock(&self.accept).clone().unwrap_or_else(|| {
                body["rows"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|r| r["id"].as_str().unwrap().to_string())
                    .collect()
            });
            let status = *lock(&self.status);
            Ok(HttpResponse::new(
                status,
                serde_json::to_vec(&serde_json::json!({"success": true, "acceptedIds": ids}))
                    .unwrap(),
            ))
        }
    }

    fn row(id: &str) -> WatchHistorySyncRow {
        WatchHistorySyncRow {
            id: id.into(),
            channel_id: "c".into(),
            channel_name: "n".into(),
            thumbnail_url: None,
            category_name: None,
            started_at: from_unix(1_700_000_000.0),
            ended_at: None,
            duration: 60.0,
            updated_at: from_unix(1_700_000_060.0),
        }
    }

    fn service(
        store: &Arc<FakeStore>,
        transport: &Arc<FakeTransport>,
        enabled: bool,
    ) -> WatchHistorySyncService {
        let store: Arc<dyn WatchHistorySyncStore> = store.clone();
        let transport: Arc<dyn HttpTransport> = transport.clone();
        WatchHistorySyncService::new(
            store,
            transport,
            Arc::new(InstallIdentity::new(Arc::new(KeychainService::in_memory()))),
            SyncConfig::from_settings("https://cv.example.invalid", enabled),
        )
    }

    #[tokio::test]
    async fn push_marks_only_accepted_rows() {
        let store = Arc::new(FakeStore::default());
        *lock(&store.rows) = vec![row("a"), row("b")];
        let transport = FakeTransport::new(200, false);
        *lock(&transport.accept) = Some(vec!["a".into()]);
        let s = service(&store, &transport, true);
        s.push_once_coalesced().await;
        let status = s.current_status();
        assert_eq!(status.pushed_row_count, 1);
        assert_eq!(status.queue_depth, 1);
        assert!(status.last_success_at.is_some());
        let requests = lock(&transport.requests);
        let req = &requests[0];
        assert_eq!(
            req.url.as_str(),
            "https://cv.example.invalid/api/sync/watch-history"
        );
        assert_eq!(req.header("Content-Type"), Some("application/json"));
        assert_eq!(req.header("Idempotency-Key").map(str::len), Some(36));
        let body: serde_json::Value = serde_json::from_slice(req.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["installId"].as_str().map(str::len), Some(36));
        assert_eq!(body["rows"][0]["thumbnailURL"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn failures_back_off_exponentially_and_disabled_is_noop() {
        let store = Arc::new(FakeStore::default());
        *lock(&store.rows) = vec![row("a")];
        let transport = FakeTransport::new(404, false);
        let s = service(&store, &transport, true);
        for expected in [1.0, 2.0, 4.0] {
            s.push_once_coalesced().await;
            assert_eq!(s.current_status().backoff_seconds, expected);
        }
        assert_eq!(
            s.current_status().last_error_message.as_deref(),
            Some("Server endpoint not ready (404)")
        );
        assert_eq!(WatchHistorySyncService::next_backoff(256.0), 300.0);
        *lock(&transport.status) = 202;
        s.push_once_coalesced().await;
        assert_eq!(s.current_status().backoff_seconds, 0.0);

        let off = service(&store, &transport, false);
        let before = lock(&transport.requests).len();
        off.push_once_coalesced().await;
        off.start();
        assert!(!off.is_running());
        assert_eq!(lock(&transport.requests).len(), before);
    }

    #[tokio::test]
    async fn overlapping_pushes_send_one_batch_then_rerun() {
        let store = Arc::new(FakeStore::default());
        *lock(&store.rows) = vec![row("a")];
        let transport = FakeTransport::new(200, true);
        let s = service(&store, &transport, true);
        let first = tokio::spawn({
            let s = s.clone();
            async move { s.push_once_coalesced().await }
        });
        while lock(&transport.requests).is_empty() {
            tokio::task::yield_now().await;
        }
        // 도는 동안 새 행이 생기고 두 번째 요청이 온다.
        lock(&store.rows).push(row("b"));
        let second = tokio::spawn({
            let s = s.clone();
            async move { s.push_once_coalesced().await }
        });
        tokio::task::yield_now().await;
        transport.gate.add_permits(10);
        first.await.unwrap();
        second.await.unwrap();
        assert_eq!(
            lock(&transport.requests).len(),
            2,
            "같은 batch 를 두 번 올리지 않고, 끝난 뒤 새 행만 한 번 더 올린다"
        );
        assert_eq!(s.current_status().pushed_row_count, 2);
    }
}
