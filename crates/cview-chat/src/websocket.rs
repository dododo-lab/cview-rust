//! WebSocket 연결 관리자. 원본: `Sources/CViewChat/WebSocketService.swift`.
//!
//! Swift 는 `URLSessionWebSocketTask` + actor 였다. 여기서는 `tokio-tungstenite` 와
//! `Mutex` 로 보호되는 공유 상태 + 수신/핑 태스크로 옮겼다.
//!
//! 유지한 계약:
//! - `connect()` 는 **HTTP 101 Upgrade 완료**까지 기다린다(상한 `open_timeout`). 실패한 연결은
//!   `Connected` 상태를 거치지 않는다.
//! - 연결 시도마다 nonce 토큰([`WebSocketAttemptState`])을 발급하고, 늦게 돌아온 구 시도의
//!   수신·핑·종료 신호는 토큰이 현재가 아니면 상태를 바꾸지 못한다.
//! - 핑은 `ping_interval`(백그라운드면 2배)마다 control ping 을 보내고 10초 안에 pong 이 없으면
//!   2초 뒤 1회 재시도, 다시 실패하면 연결 실패로 처리한다.
//!
//! 바뀐 점:
//! - `PowerAwareInterval.scaled`(배터리일 때 1.5배)는 플랫폼 전원 감시가 없으므로 적용하지
//!   않는다(AC 기준값). [`WebSocketService::effective_ping_interval`] 은 배율 입력을 그대로 받는다.
//! - Swift 가 `httpAdditionalHeaders` 에 넣던 `Connection: keep-alive` 는 WebSocket 업그레이드의
//!   `Connection: Upgrade` 를 덮어 핸드셰이크를 깨므로 보내지 않는다(URLSession 도 실제로는
//!   덮어쓴다). `Keep-Alive` 헤더는 그대로 보낸다.

use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, WebSocketConfig};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::constants::{common_headers, secs, ws_defaults};
use crate::error::{ChatError, ChatResult};

// MARK: - 순수 상태 (시도 토큰)

/// 연결 시도 하나의 식별자.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WebSocketConnectionAttemptToken {
    pub nonce: u64,
}

/// 핑 타이머 하나의 식별자.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WebSocketPingAttemptToken {
    pub connection: WebSocketConnectionAttemptToken,
    pub nonce: u64,
}

/// 업그레이드 대기 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebSocketOpenAttemptState {
    Pending,
    Opened,
    Failed,
}

/// 업그레이드 대기에서 깨어났을 때의 판정.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebSocketOpenResumeDisposition {
    Connected,
    ConnectionFailed(String),
    Cancelled,
}

/// 전송 완료 시점의 판정.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebSocketSendResumeDisposition {
    Sent,
    NotConnected,
}

/// 백그라운드 모드 정책 (revision 이 큰 것이 이긴다).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackgroundModePolicy {
    pub enabled: bool,
    pub revision: u64,
}

/// 최신 정책만 적용하는 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BackgroundModePolicyState {
    policy: Option<BackgroundModePolicy>,
}

impl BackgroundModePolicyState {
    pub fn policy(&self) -> Option<BackgroundModePolicy> {
        self.policy
    }

    /// 더 새로운 revision 이면 적용하고 `true`.
    pub fn apply(&mut self, candidate: BackgroundModePolicy) -> bool {
        if let Some(policy) = self.policy {
            if candidate.revision <= policy.revision {
                return false;
            }
        }
        self.policy = Some(candidate);
        true
    }
}

/// reconnect / open timeout / ping timer 교체를 구분하는 순수 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebSocketAttemptState {
    connection_nonce: u64,
    ping_nonce: u64,
    open_state: WebSocketOpenAttemptState,
    failed_connection_attempt: Option<WebSocketConnectionAttemptToken>,
}

impl Default for WebSocketAttemptState {
    fn default() -> Self {
        Self {
            connection_nonce: 0,
            ping_nonce: 0,
            open_state: WebSocketOpenAttemptState::Failed,
            failed_connection_attempt: None,
        }
    }
}

impl WebSocketAttemptState {
    pub fn begin_connection(&mut self) -> WebSocketConnectionAttemptToken {
        self.connection_nonce = self.connection_nonce.wrapping_add(1);
        self.ping_nonce = self.ping_nonce.wrapping_add(1);
        self.open_state = WebSocketOpenAttemptState::Pending;
        self.failed_connection_attempt = None;
        WebSocketConnectionAttemptToken {
            nonce: self.connection_nonce,
        }
    }

    pub fn invalidate_connection(
        &mut self,
        failed_attempt: Option<WebSocketConnectionAttemptToken>,
    ) {
        self.connection_nonce = self.connection_nonce.wrapping_add(1);
        self.ping_nonce = self.ping_nonce.wrapping_add(1);
        self.open_state = WebSocketOpenAttemptState::Failed;
        self.failed_connection_attempt = failed_attempt;
    }

    pub fn current_connection(&self) -> WebSocketConnectionAttemptToken {
        WebSocketConnectionAttemptToken {
            nonce: self.connection_nonce,
        }
    }

    pub fn is_current(&self, token: WebSocketConnectionAttemptToken) -> bool {
        token.nonce == self.connection_nonce
    }

    pub fn mark_opened(&mut self, token: WebSocketConnectionAttemptToken) -> bool {
        if !self.is_current(token) || self.open_state != WebSocketOpenAttemptState::Pending {
            return false;
        }
        self.open_state = WebSocketOpenAttemptState::Opened;
        true
    }

    pub fn mark_open_failed(&mut self, token: WebSocketConnectionAttemptToken) -> bool {
        if !self.is_current(token) || self.open_state != WebSocketOpenAttemptState::Pending {
            return false;
        }
        self.open_state = WebSocketOpenAttemptState::Failed;
        true
    }

    pub fn is_open(&self, token: WebSocketConnectionAttemptToken) -> bool {
        self.is_current(token) && self.open_state == WebSocketOpenAttemptState::Opened
    }

    pub fn is_open_pending(&self, token: WebSocketConnectionAttemptToken) -> bool {
        self.is_current(token) && self.open_state == WebSocketOpenAttemptState::Pending
    }

    pub fn owns_failure(&self, token: WebSocketConnectionAttemptToken) -> bool {
        self.failed_connection_attempt == Some(token)
    }

    pub fn begin_ping(&mut self) -> WebSocketPingAttemptToken {
        self.ping_nonce = self.ping_nonce.wrapping_add(1);
        WebSocketPingAttemptToken {
            connection: self.current_connection(),
            nonce: self.ping_nonce,
        }
    }

    pub fn is_current_ping(&self, token: WebSocketPingAttemptToken) -> bool {
        self.is_current(token.connection) && token.nonce == self.ping_nonce
    }
}

// MARK: - 공개 타입

/// 연결 상태.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebSocketState {
    Disconnected,
    Connecting,
    Connected,
    Disconnecting,
    Failed(String),
}

impl WebSocketState {
    fn is_live(&self) -> bool {
        matches!(self, Self::Connected | Self::Connecting)
    }
}

/// 수신 메시지.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebSocketMessage {
    Text(String),
    Data(Vec<u8>),
}

impl WebSocketMessage {
    pub fn text_value(&self) -> Option<&str> {
        match self {
            Self::Text(t) => Some(t),
            Self::Data(_) => None,
        }
    }

    pub fn data_value(&self) -> Option<&[u8]> {
        match self {
            Self::Data(d) => Some(d),
            Self::Text(_) => None,
        }
    }
}

/// 연결 설정.
#[derive(Debug, Clone)]
pub struct WebSocketConfiguration {
    pub url: String,
    pub ping_interval: f64,
    pub max_message_size: usize,
    /// 추가 헤더 (쿠키 `NID_AUT`/`NID_SES` 등).
    pub http_headers: Vec<(String, String)>,
    /// HTTP 101 Upgrade 완료 대기 상한 (초).
    pub open_timeout: f64,
}

impl WebSocketConfiguration {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            ping_interval: ws_defaults::PING_INTERVAL,
            max_message_size: ws_defaults::MAX_MESSAGE_SIZE,
            http_headers: Vec::new(),
            open_timeout: ws_defaults::OPEN_TIMEOUT,
        }
    }
}

/// 엔진 watchdog 이 쓰는 transport 생존 스냅샷.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TransportLiveness {
    pub last_activity: Instant,
    pub ping_interval: f64,
}

// MARK: - 서비스

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsSink = Arc<tokio::sync::Mutex<SplitSink<WsStream, Message>>>;

struct Shared {
    state: WebSocketState,
    attempt: WebSocketAttemptState,
    background: BackgroundModePolicyState,
    last_transport_activity: Instant,
    scheduled_ping_interval: f64,
    sink: Option<WsSink>,
    receive_task: Option<JoinHandle<()>>,
    ping_task: Option<JoinHandle<()>>,
    /// 업그레이드 대기자를 깨우는 신호 (Swift `openContinuation`).
    open_waiter: Option<oneshot::Sender<ChatError>>,
    /// 진행 중인 control ping 의 pong 대기자.
    pending_pong: Option<oneshot::Sender<()>>,
}

struct Inner {
    config: WebSocketConfiguration,
    weak_self: Weak<Inner>,
    shared: Mutex<Shared>,
    msg_tx: mpsc::UnboundedSender<WebSocketMessage>,
    msg_rx: Mutex<Option<mpsc::UnboundedReceiver<WebSocketMessage>>>,
    state_tx: mpsc::UnboundedSender<WebSocketState>,
    state_rx: Mutex<Option<mpsc::UnboundedReceiver<WebSocketState>>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        let shared = self.shared.get_mut().unwrap_or_else(|e| e.into_inner());
        if let Some(t) = shared.ping_task.take() {
            t.abort();
        }
        if let Some(t) = shared.receive_task.take() {
            t.abort();
        }
    }
}

/// WebSocket 연결 관리자. 복제본은 같은 연결을 가리킨다(Swift actor 참조와 같다).
#[derive(Clone)]
pub struct WebSocketService {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for WebSocketService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebSocketService")
            .field("url", &self.inner.config.url)
            .finish()
    }
}

impl WebSocketService {
    pub fn new(config: WebSocketConfiguration) -> Self {
        let (msg_tx, msg_rx) = mpsc::unbounded_channel();
        let (state_tx, state_rx) = mpsc::unbounded_channel();
        let scheduled = Self::effective_ping_interval(config.ping_interval, false);
        let inner = Arc::new_cyclic(|weak| Inner {
            config,
            weak_self: weak.clone(),
            shared: Mutex::new(Shared {
                state: WebSocketState::Disconnected,
                attempt: WebSocketAttemptState::default(),
                background: BackgroundModePolicyState::default(),
                last_transport_activity: Instant::now(),
                scheduled_ping_interval: scheduled,
                sink: None,
                receive_task: None,
                ping_task: None,
                open_waiter: None,
                pending_pong: None,
            }),
            msg_tx,
            msg_rx: Mutex::new(Some(msg_rx)),
            state_tx,
            state_rx: Mutex::new(Some(state_rx)),
        });
        Self { inner }
    }

    /// 같은 서비스 인스턴스인가 (Swift `===`).
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    pub fn configuration(&self) -> &WebSocketConfiguration {
        &self.inner.config
    }

    pub fn state(&self) -> WebSocketState {
        self.inner.lock().state.clone()
    }

    /// 수신 메시지 스트림. 생성 시점부터 버퍼링되며 한 번만 꺼낼 수 있다.
    pub fn take_messages(&self) -> Option<mpsc::UnboundedReceiver<WebSocketMessage>> {
        self.inner
            .msg_rx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }

    /// 상태 변화 스트림. 한 번만 꺼낼 수 있다.
    pub fn take_state_changes(&self) -> Option<mpsc::UnboundedReceiver<WebSocketState>> {
        self.inner
            .state_rx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }

    /// 전원 배율까지 적용된 base 를 foreground/background 스케줄로 정규화한다.
    pub fn effective_ping_interval(scaled_base: f64, is_background_mode: bool) -> f64 {
        scaled_base * if is_background_mode { 2.0 } else { 1.0 }
    }

    /// 전송 완료 시점에 그 전송이 여전히 현재 연결에 귀속되는가.
    pub fn send_resume_disposition(
        attempt: WebSocketConnectionAttemptToken,
        attempt_state: &WebSocketAttemptState,
        state: &WebSocketState,
        socket_matches: bool,
    ) -> WebSocketSendResumeDisposition {
        if socket_matches && attempt_state.is_open(attempt) && *state == WebSocketState::Connected {
            WebSocketSendResumeDisposition::Sent
        } else {
            WebSocketSendResumeDisposition::NotConnected
        }
    }

    /// 업그레이드 대기에서 깨어났을 때의 판정.
    pub fn open_resume_disposition(
        attempt: WebSocketConnectionAttemptToken,
        attempt_state: &WebSocketAttemptState,
        state: &WebSocketState,
    ) -> WebSocketOpenResumeDisposition {
        if attempt_state.is_open(attempt) && *state == WebSocketState::Connected {
            return WebSocketOpenResumeDisposition::Connected;
        }
        if attempt_state.owns_failure(attempt) {
            if let WebSocketState::Failed(reason) = state {
                return WebSocketOpenResumeDisposition::ConnectionFailed(reason.clone());
            }
        }
        WebSocketOpenResumeDisposition::Cancelled
    }

    pub(crate) fn transport_liveness(&self) -> TransportLiveness {
        let s = self.inner.lock();
        TransportLiveness {
            last_activity: s.last_transport_activity,
            ping_interval: s.scheduled_ping_interval,
        }
    }

    /// 서버에 연결한다. 업그레이드가 끝나야 `Ok` — 실패하면 아무것도 남기지 않는다.
    pub async fn connect(&self) -> ChatResult<()> {
        let (attempt, cancel_rx) = {
            let mut s = self.inner.lock();
            match s.state {
                WebSocketState::Disconnected | WebSocketState::Failed(_) => {}
                ref other => {
                    tracing::warn!(state = ?other, "WebSocket already active");
                    return Ok(());
                }
            }
            let previous = s.attempt.current_connection();
            Inner::fail_open(&mut s, ChatError::Cancelled, previous);
            abort(&mut s.ping_task);
            abort(&mut s.receive_task);
            let attempt = s.attempt.begin_connection();
            self.inner.update_state(&mut s, WebSocketState::Connecting);
            s.sink = None;
            s.pending_pong = None;
            let (tx, rx) = oneshot::channel();
            s.open_waiter = Some(tx);
            (attempt, rx)
        };

        let request = match self.build_request() {
            Ok(r) => r,
            Err(e) => {
                self.inner.handle_disconnection(e.clone(), attempt);
                return Err(e);
            }
        };
        let ws_config = WebSocketConfig {
            max_message_size: Some(self.inner.config.max_message_size),
            max_frame_size: Some(self.inner.config.max_message_size),
            ..Default::default()
        };
        let open_timeout = secs(self.inner.config.open_timeout);

        let result: ChatResult<WsStream> = tokio::select! {
            r = tokio::time::timeout(
                open_timeout,
                tokio_tungstenite::connect_async_with_config(request, Some(ws_config), false),
            ) => match r {
                Ok(Ok((stream, _response))) => Ok(stream),
                Ok(Err(e)) => Err(ChatError::ConnectionFailed(e.to_string())),
                Err(_) => Err(ChatError::ConnectionFailed("WebSocket 업그레이드 시간 초과".into())),
            },
            e = cancel_rx => Err(e.unwrap_or(ChatError::Cancelled)),
        };

        match result {
            Ok(stream) => {
                let mut s = self.inner.lock();
                // 업그레이드 완료 — 여기가 유일한 `Connected` 선언 지점.
                if s.state == WebSocketState::Connecting && s.attempt.mark_opened(attempt) {
                    s.open_waiter = None;
                    s.last_transport_activity = Instant::now();
                    self.inner.update_state(&mut s, WebSocketState::Connected);
                    let (sink, stream) = stream.split();
                    s.sink = Some(Arc::new(tokio::sync::Mutex::new(sink)));
                    let weak = self.inner.weak_self.clone();
                    s.receive_task = Some(tokio::spawn(receive_loop(weak, attempt, stream)));
                    self.inner.start_ping_timer(&mut s, attempt);
                    tracing::debug!("WebSocket connected");
                    Ok(())
                } else {
                    let disposition = Self::open_resume_disposition(attempt, &s.attempt, &s.state);
                    drop(s);
                    drop(stream);
                    match disposition {
                        WebSocketOpenResumeDisposition::Connected => Ok(()),
                        WebSocketOpenResumeDisposition::ConnectionFailed(reason) => {
                            Err(ChatError::ConnectionFailed(reason))
                        }
                        WebSocketOpenResumeDisposition::Cancelled => Err(ChatError::Cancelled),
                    }
                }
            }
            Err(error) => {
                // 업그레이드 실패 — 이미 정리됐다면 상태 가드에 걸려 무해한 no-op.
                self.inner.handle_disconnection(error.clone(), attempt);
                Err(error)
            }
        }
    }

    fn build_request(
        &self,
    ) -> ChatResult<tokio_tungstenite::tungstenite::handshake::client::Request> {
        let mut request = self
            .inner
            .config
            .url
            .as_str()
            .into_client_request()
            .map_err(|e| ChatError::ConnectionFailed(e.to_string()))?;
        let headers = request.headers_mut();
        headers.insert(
            "Origin",
            HeaderValue::from_static(common_headers::CHZZK_ORIGIN),
        );
        headers.insert(
            "User-Agent",
            HeaderValue::from_static(common_headers::CHROME_USER_AGENT),
        );
        headers.insert(
            "Keep-Alive",
            HeaderValue::from_static(ws_defaults::KEEP_ALIVE_HEADER),
        );
        for (key, value) in &self.inner.config.http_headers {
            let name = tokio_tungstenite::tungstenite::http::HeaderName::from_bytes(key.as_bytes())
                .map_err(|e| ChatError::ConnectionFailed(e.to_string()))?;
            let value = HeaderValue::from_str(value)
                .map_err(|e| ChatError::ConnectionFailed(e.to_string()))?;
            headers.insert(name, value);
        }
        Ok(request)
    }

    /// 연결을 닫는다. 업그레이드 대기 중이면 그 대기를 먼저 끊는다.
    pub async fn disconnect(&self) {
        let sink = {
            let mut s = self.inner.lock();
            let current = s.attempt.current_connection();
            Inner::fail_open(&mut s, ChatError::Cancelled, current);
            if !s.state.is_live() {
                return;
            }
            s.attempt.invalidate_connection(None);
            self.inner
                .update_state(&mut s, WebSocketState::Disconnecting);
            abort(&mut s.ping_task);
            abort(&mut s.receive_task);
            s.pending_pong = None;
            let sink = s.sink.take();
            self.inner
                .update_state(&mut s, WebSocketState::Disconnected);
            sink
        };
        if let Some(sink) = sink {
            // goingAway close frame — 최선 노력, 짧은 상한.
            let _ = tokio::time::timeout(Duration::from_secs(1), async move {
                let mut sink = sink.lock().await;
                let frame = CloseFrame {
                    code: CloseCode::Away,
                    reason: "".into(),
                };
                let _ = sink.send(Message::Close(Some(frame))).await;
                let _ = sink.close().await;
            })
            .await;
        }
        tracing::debug!("WebSocket disconnected");
    }

    /// 텍스트 프레임을 보낸다.
    pub async fn send(&self, text: &str) -> ChatResult<()> {
        let (attempt, sink) = {
            let s = self.inner.lock();
            let attempt = s.attempt.current_connection();
            match (&s.state, s.attempt.is_open(attempt), &s.sink) {
                (WebSocketState::Connected, true, Some(sink)) => (attempt, sink.clone()),
                _ => return Err(ChatError::NotConnected),
            }
        };
        let result = sink.lock().await.send(Message::Text(text.to_owned())).await;
        let disposition = {
            let s = self.inner.lock();
            let matches = s.sink.as_ref().is_some_and(|cur| Arc::ptr_eq(cur, &sink));
            Self::send_resume_disposition(attempt, &s.attempt, &s.state, matches)
        };
        match (result, disposition) {
            (_, WebSocketSendResumeDisposition::NotConnected) => Err(ChatError::NotConnected),
            (Err(e), WebSocketSendResumeDisposition::Sent) => {
                Err(ChatError::SendFailed(e.to_string()))
            }
            (Ok(()), WebSocketSendResumeDisposition::Sent) => {
                tracing::debug!(chars = text.chars().count(), "Sent");
                Ok(())
            }
        }
    }

    /// 백그라운드 모드 전환 — 핑 주기를 2배로.
    pub fn set_background_mode(&self, enabled: bool) {
        let next_revision = {
            let s = self.inner.lock();
            s.background
                .policy()
                .map(|p| p.revision.wrapping_add(1))
                .unwrap_or(0)
        };
        self.set_background_mode_policy(BackgroundModePolicy {
            enabled,
            revision: next_revision,
        });
    }

    /// revision 이 더 새로운 정책만 적용한다.
    pub(crate) fn set_background_mode_policy(&self, policy: BackgroundModePolicy) {
        let mut s = self.inner.lock();
        let previous = s.background.policy().is_some_and(|p| p.enabled);
        if !s.background.apply(policy) || previous == policy.enabled {
            return;
        }
        if s.state == WebSocketState::Connected {
            let current = s.attempt.current_connection();
            self.inner.start_ping_timer(&mut s, current);
        }
    }

    pub fn is_background_mode(&self) -> bool {
        self.inner
            .lock()
            .background
            .policy()
            .is_some_and(|p| p.enabled)
    }
}

fn abort(task: &mut Option<JoinHandle<()>>) {
    if let Some(t) = task.take() {
        t.abort();
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn update_state(&self, s: &mut Shared, state: WebSocketState) {
        s.state = state.clone();
        let _ = self.state_tx.send(state);
    }

    /// 업그레이드 대기를 포기시킨다. 현재 시도가 아직 pending 일 때만.
    fn fail_open(s: &mut Shared, error: ChatError, expected: WebSocketConnectionAttemptToken) {
        if !s.attempt.mark_open_failed(expected) {
            return;
        }
        if let Some(tx) = s.open_waiter.take() {
            let _ = tx.send(error);
        }
    }

    fn start_ping_timer(&self, s: &mut Shared, connection: WebSocketConnectionAttemptToken) {
        if !s.attempt.is_current(connection) || s.state != WebSocketState::Connected {
            return;
        }
        abort(&mut s.ping_task);
        let ping_attempt = s.attempt.begin_ping();
        let background = s.background.policy().is_some_and(|p| p.enabled);
        let interval =
            WebSocketService::effective_ping_interval(self.config.ping_interval, background);
        s.scheduled_ping_interval = interval;
        s.ping_task = Some(tokio::spawn(ping_loop(
            self.weak_self.clone(),
            interval,
            ping_attempt,
        )));
    }

    fn is_current_ping(&self, attempt: WebSocketPingAttemptToken) -> bool {
        self.lock().attempt.is_current_ping(attempt)
    }

    fn handle_disconnection(&self, error: ChatError, attempt: WebSocketConnectionAttemptToken) {
        let mut s = self.lock();
        // 상태 가드보다 먼저 대기를 끊는다.
        Self::fail_open(&mut s, error.clone(), attempt);
        if !s.attempt.is_current(attempt) || !s.state.is_live() {
            return;
        }
        s.attempt.invalidate_connection(Some(attempt));
        abort(&mut s.ping_task);
        abort(&mut s.receive_task);
        s.sink = None;
        s.pending_pong = None;
        self.update_state(&mut s, WebSocketState::Failed(error.to_string()));
        tracing::warn!(%error, "WebSocket disconnected due to error");
    }

    fn handle_server_close(&self, attempt: WebSocketConnectionAttemptToken, code: Option<u16>) {
        {
            let s = self.lock();
            if !s.attempt.is_current(attempt) || !s.state.is_live() {
                return;
            }
        }
        tracing::info!(?code, "WS server close frame received");
        self.handle_disconnection(
            ChatError::ConnectionFailed("네트워크 연결이 끊어졌습니다".into()),
            attempt,
        );
    }

    /// control ping 을 보내고 pong 을 기다린다 (10초 상한).
    async fn send_ping(&self, attempt: WebSocketPingAttemptToken) -> ChatResult<()> {
        let (sink, rx) = {
            let mut s = self.lock();
            if !s.attempt.is_current_ping(attempt) || s.state != WebSocketState::Connected {
                return Err(ChatError::Cancelled);
            }
            let Some(sink) = s.sink.clone() else {
                return Err(ChatError::Cancelled);
            };
            let (tx, rx) = oneshot::channel();
            s.pending_pong = Some(tx);
            (sink, rx)
        };

        sink.lock()
            .await
            .send(Message::Ping(Vec::new()))
            .await
            .map_err(|e| ChatError::ConnectionFailed(e.to_string()))?;

        match tokio::time::timeout(Duration::from_secs(10), rx).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => return Err(ChatError::Cancelled),
            Err(_) => {
                return Err(ChatError::ConnectionFailed(
                    "요청 시간이 초과되었습니다".into(),
                ))
            }
        }

        // pong 은 서버 생존 증명이다. 그 사이 소켓이 바뀌었다면 새 연결의 watchdog 을 연장하지 않는다.
        let mut s = self.lock();
        let same_socket = s.sink.as_ref().is_some_and(|cur| Arc::ptr_eq(cur, &sink));
        if !s.attempt.is_current_ping(attempt)
            || s.state != WebSocketState::Connected
            || !same_socket
        {
            return Err(ChatError::Cancelled);
        }
        s.last_transport_activity = Instant::now();
        Ok(())
    }
}

async fn receive_loop(
    weak: Weak<Inner>,
    attempt: WebSocketConnectionAttemptToken,
    mut stream: SplitStream<WsStream>,
) {
    loop {
        let next = stream.next().await;
        let Some(inner) = weak.upgrade() else { break };
        match next {
            Some(Ok(message)) => {
                {
                    let mut s = inner.lock();
                    if !s.attempt.is_current(attempt) {
                        break;
                    }
                    s.last_transport_activity = Instant::now();
                    if let Message::Pong(_) = message {
                        if let Some(tx) = s.pending_pong.take() {
                            let _ = tx.send(());
                        }
                    }
                }
                match message {
                    Message::Text(text) => {
                        let _ = inner.msg_tx.send(WebSocketMessage::Text(text));
                    }
                    Message::Binary(data) => {
                        let _ = inner.msg_tx.send(WebSocketMessage::Data(data));
                    }
                    Message::Close(frame) => {
                        inner.handle_server_close(attempt, frame.map(|f| u16::from(f.code)));
                        break;
                    }
                    // Ping 에는 tungstenite 가 자동으로 pong 을 보낸다.
                    Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
                }
            }
            Some(Err(error)) => {
                if !inner.lock().attempt.is_current(attempt) {
                    break;
                }
                tracing::warn!(%error, "WS receive error");
                inner.handle_disconnection(ChatError::ConnectionFailed(error.to_string()), attempt);
                break;
            }
            None => {
                inner.handle_disconnection(
                    ChatError::ConnectionFailed("네트워크 연결이 끊어졌습니다".into()),
                    attempt,
                );
                break;
            }
        }
    }
    tracing::debug!("WS receive loop ended");
}

async fn ping_loop(weak: Weak<Inner>, interval: f64, attempt: WebSocketPingAttemptToken) {
    let period = secs(interval);
    loop {
        tokio::time::sleep(period).await;
        let Some(inner) = weak.upgrade() else { return };
        if !inner.is_current_ping(attempt) {
            return;
        }
        match inner.send_ping(attempt).await {
            Ok(()) => continue,
            Err(ChatError::Cancelled) => return,
            Err(_) => {}
        }
        if !inner.is_current_ping(attempt) {
            return;
        }
        // 일시적 네트워크 지터 대응 — 2초 뒤 1회 재시도.
        tracing::warn!("Ping failed, retrying once...");
        drop(inner);
        tokio::time::sleep(Duration::from_secs(2)).await;
        let Some(inner) = weak.upgrade() else { return };
        if !inner.is_current_ping(attempt) {
            return;
        }
        match inner.send_ping(attempt).await {
            Ok(()) => {
                tracing::info!("Ping retry succeeded");
                continue;
            }
            Err(ChatError::Cancelled) => return,
            Err(error) => {
                // 구 ping 의 실패로 현재 소켓을 닫지 않는다.
                if !inner.is_current_ping(attempt) {
                    return;
                }
                inner.handle_disconnection(error, attempt.connection);
                return;
            }
        }
    }
}
