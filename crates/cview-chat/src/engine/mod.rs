//! WebSocket·파싱·재연결을 묶는 채팅 엔진. 원본: `Sources/CViewChat/ChatEngine.swift`.
//!
//! Swift `actor ChatEngine` → `Arc<EngineInner>` + `Mutex<EngineState>` + tokio 태스크.
//! 잠금은 await 경계를 넘지 않으며, 액터 재진입 창에서 Swift 가 하던 세대(generation)·소켓
//! identity 재검사를 await 뒤마다 그대로 한다. 이벤트는 [`ChatEventStream`] 으로 내보낸다.
//!
//! 바뀐 점:
//! - `HTTPCookieStorage` 대신 [`ChatEngineConfiguration::cookie_provider`] 가 쿠키를 공급한다.
//! - 시간 상수(ack timeout, open timeout, watchdog 주기, 소진 후 대기)는 설정으로 주입할 수
//!   있다 — 기본값은 Swift 와 같다.
//! - `ChatConnectTrace`(signpost/요청 장부 계측)는 포팅하지 않았다(tracing 로그로 대체).

mod attempt;
mod events;

use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Instant;

use futures_util::future::BoxFuture;
use tokio::task::JoinHandle;

pub use attempt::{ConnectAckAttemptState, ConnectAckAttemptToken, ReconnectionOwnerState, ReconnectionOwnerToken};
pub use events::{ChatEngineEvent, ChatEventStream, EVENT_BUFFER_LIMIT};

use crate::constants::{chat_defaults, secs, ws_defaults};
use crate::error::{ChatError, ChatResult};
use crate::parser::ChatMessageParser;
use crate::protocol::ChatEvent;
use crate::reconnection::{ReconnectionConfiguration, ReconnectionPolicy};
use crate::websocket::{BackgroundModePolicy, WebSocketConfiguration, WebSocketService, WebSocketState};
use cview_core::models::chat::{ChatConnectionState, ChatMessage, EmojiMap};
use events::EventQueue;

/// 재연결 시점에 최신 채팅 토큰 `(accessToken, extraToken)` 을 조달하는 공급자.
pub type TokenProvider = Arc<dyn Fn() -> BoxFuture<'static, ChatResult<(String, Option<String>)>> + Send + Sync>;

/// 쿠키 저장소에서 현재 쿠키를 읽어 오는 공급자 (Swift `HTTPCookieStorage.shared`).
pub type CookieProvider = Arc<dyn Fn() -> Vec<ChatCookie> + Send + Sync>;

/// 공급자가 돌려주는 쿠키 한 개.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
}

/// 엔진 설정.
#[derive(Clone)]
pub struct ChatEngineConfiguration {
    pub chat_channel_id: String,
    pub access_token: String,
    pub extra_token: Option<String>,
    pub uid: Option<String>,
    pub channel_id: Option<String>,
    pub server_url: String,
    pub reconnection_config: ReconnectionConfiguration,
    pub max_message_buffer: usize,
    /// `None` 이면 초기 토큰을 계속 쓴다. 주입하면 **두 번째 시도부터** 매 연결 전에 갱신한다.
    pub token_provider: Option<TokenProvider>,
    /// 연결 시 `NID_AUT`/`NID_SES` 쿠키를 읽어 `Cookie` 헤더로 보낸다.
    pub cookie_provider: Option<CookieProvider>,
    /// WebSocket control ping 간격 (초).
    pub ping_interval: f64,
    /// HTTP 101 Upgrade 대기 상한 (초).
    pub open_timeout: f64,
    /// cmd 10100 대기 상한 (초).
    pub connect_ack_timeout: f64,
    /// 인바운드 watchdog 점검 주기 (초, Swift 15초).
    pub watchdog_check_interval: f64,
    /// 재연결 소진 뒤 정책을 리셋하기 전 대기 (초, Swift 60초).
    pub exhausted_retry_delay: f64,
}

impl std::fmt::Debug for ChatEngineConfiguration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatEngineConfiguration")
            .field("chat_channel_id", &self.chat_channel_id)
            .field("uid", &self.uid)
            .field("channel_id", &self.channel_id)
            .field("server_url", &self.server_url)
            .field("max_message_buffer", &self.max_message_buffer)
            .finish_non_exhaustive()
    }
}

impl ChatEngineConfiguration {
    pub fn new(chat_channel_id: impl Into<String>, access_token: impl Into<String>) -> Self {
        let chat_channel_id = chat_channel_id.into();
        let server_url = Self::compute_server_url(&chat_channel_id);
        Self {
            chat_channel_id,
            access_token: access_token.into(),
            extra_token: None,
            uid: None,
            channel_id: None,
            server_url,
            reconnection_config: ReconnectionConfiguration::default(),
            max_message_buffer: chat_defaults::MAX_MESSAGE_BUFFER,
            token_provider: None,
            cookie_provider: None,
            ping_interval: chat_defaults::PING_INTERVAL,
            open_timeout: ws_defaults::OPEN_TIMEOUT,
            connect_ack_timeout: chat_defaults::CONNECT_ACK_TIMEOUT,
            watchdog_check_interval: 15.0,
            exhausted_retry_delay: 60.0,
        }
    }

    pub fn extra_token(mut self, v: Option<String>) -> Self {
        self.extra_token = v;
        self
    }

    pub fn uid(mut self, v: Option<String>) -> Self {
        self.uid = v;
        self
    }

    pub fn channel_id(mut self, v: Option<String>) -> Self {
        self.channel_id = v;
        self
    }

    pub fn server_url(mut self, v: impl Into<String>) -> Self {
        self.server_url = v.into();
        self
    }

    pub fn reconnection_config(mut self, v: ReconnectionConfiguration) -> Self {
        self.reconnection_config = v;
        self
    }

    pub fn max_message_buffer(mut self, v: usize) -> Self {
        self.max_message_buffer = v;
        self
    }

    pub fn token_provider(mut self, v: Option<TokenProvider>) -> Self {
        self.token_provider = v;
        self
    }

    pub fn cookie_provider(mut self, v: Option<CookieProvider>) -> Self {
        self.cookie_provider = v;
        self
    }

    /// chatChannelId 의 UTF-8 바이트 합으로 서버 번호(1~9)를 정한다.
    pub fn compute_server_url(chat_channel_id: &str) -> String {
        let sum: u64 = chat_channel_id.bytes().map(u64::from).sum();
        let server_id = (sum % 9) + 1;
        format!("wss://kr-ss{server_id}.chat.naver.com/chat")
    }
}

/// `Sources/CViewCore/Utilities/NaverCookiePolicy.swift` `matchesDomain`.
pub fn naver_cookie_domain_matches(domain: &str) -> bool {
    let mut host = domain.to_lowercase();
    if host.starts_with('.') {
        host.remove(0);
    }
    if !(host == "naver.com" || host.ends_with(".naver.com")) {
        return false;
    }
    host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

/// 버퍼 트림 히스테리시스 여유분 — 포화 후 매 메시지 O(N) 시프트를 피한다.
const BUFFER_TRIM_SLACK: usize = 256;

struct EngineState {
    current_access_token: String,
    current_extra_token: Option<String>,
    has_consumed_initial_token: bool,

    web_socket: Option<WebSocketService>,
    message_listen_task: Option<JoinHandle<()>>,
    state_listen_task: Option<JoinHandle<()>>,
    pong_response_task: Option<JoinHandle<()>>,
    recent_messages_task: Option<JoinHandle<()>>,
    watchdog_task: Option<JoinHandle<()>>,
    reconnection_task: Option<JoinHandle<()>>,

    last_inbound_activity: Instant,
    connection_state: ChatConnectionState,
    messages: Vec<ChatMessage>,
    /// `messages` 안의 비어 있지 않은 id 집합 (디듑 미러). 4경로에서 함께 갱신해야 한다:
    /// clear · append · trim · remove.
    message_ids: HashSet<String>,
    session_id: Option<String>,
    is_manual_disconnect: bool,
    next_tid: i64,
    background_mode_policy: BackgroundModePolicy,
    connection_generation: u64,

    connect_ack_waiter: Option<tokio::sync::oneshot::Sender<ChatResult<()>>>,
    connect_ack_state: ConnectAckAttemptState,
    connect_ack_failure: Option<ChatError>,
    reconnection_owner: ReconnectionOwnerState,
}

impl EngineState {
    fn abort_all_tasks(&mut self) {
        for task in [
            &mut self.message_listen_task,
            &mut self.state_listen_task,
            &mut self.pong_response_task,
            &mut self.recent_messages_task,
            &mut self.watchdog_task,
            &mut self.reconnection_task,
        ] {
            abort(task);
        }
    }

    fn is_reconnecting(&self) -> bool {
        self.reconnection_owner.is_active()
    }
}

fn abort(task: &mut Option<JoinHandle<()>>) {
    if let Some(t) = task.take() {
        t.abort();
    }
}

struct EngineInner {
    config: ChatEngineConfiguration,
    parser: ChatMessageParser,
    reconnection: ReconnectionPolicy,
    st: Mutex<EngineState>,
    events: Arc<EventQueue>,
    weak_self: Weak<EngineInner>,
}

impl Drop for EngineInner {
    fn drop(&mut self) {
        let st = self.st.get_mut().unwrap_or_else(|e| e.into_inner());
        st.abort_all_tasks();
        self.events.close();
        // 엔진이 disconnect() 없이 해제돼도 소켓은 닫는다.
        if let Some(socket) = st.web_socket.take() {
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move { socket.disconnect().await });
            }
        }
    }
}

/// 채팅 엔진. 복제본은 같은 엔진을 가리킨다.
#[derive(Clone)]
pub struct ChatEngine {
    inner: Arc<EngineInner>,
}

impl std::fmt::Debug for ChatEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatEngine").field("config", &self.inner.config).finish()
    }
}

impl ChatEngine {
    pub fn new(config: ChatEngineConfiguration) -> Self {
        let state = EngineState {
            current_access_token: config.access_token.clone(),
            current_extra_token: config.extra_token.clone(),
            has_consumed_initial_token: false,
            web_socket: None,
            message_listen_task: None,
            state_listen_task: None,
            pong_response_task: None,
            recent_messages_task: None,
            watchdog_task: None,
            reconnection_task: None,
            last_inbound_activity: Instant::now(),
            connection_state: ChatConnectionState::Disconnected,
            messages: Vec::new(),
            message_ids: HashSet::new(),
            session_id: None,
            is_manual_disconnect: false,
            next_tid: 3,
            background_mode_policy: BackgroundModePolicy { enabled: false, revision: 0 },
            connection_generation: 0,
            connect_ack_waiter: None,
            connect_ack_state: ConnectAckAttemptState::default(),
            connect_ack_failure: None,
            reconnection_owner: ReconnectionOwnerState::default(),
        };
        let inner = Arc::new_cyclic(|weak| EngineInner {
            reconnection: ReconnectionPolicy::new(config.reconnection_config),
            config,
            parser: ChatMessageParser::new(),
            st: Mutex::new(state),
            events: Arc::new(EventQueue::default()),
            weak_self: weak.clone(),
        });
        Self { inner }
    }

    pub fn configuration(&self) -> &ChatEngineConfiguration {
        &self.inner.config
    }

    // MARK: - 정적 판정 함수 (순수)

    /// ping 간격에 timeout/retry 여유를 더한 watchdog 한계.
    pub fn watchdog_silence_limit(ping_interval: f64) -> f64 {
        ping_interval + chat_defaults::INBOUND_WATCHDOG_GRACE
    }

    /// cmd 94005 가 **나를** 대상으로 하는가. 불확실하면 무조건 `false`.
    pub fn is_self_kick(target: &str, my_uid: Option<&str>) -> bool {
        match my_uid {
            Some(uid) if !uid.is_empty() && !target.is_empty() => target == uid,
            _ => false,
        }
    }

    /// 늦게 돌아온 초기 connect 실패를 처리할지.
    pub fn should_handle_initial_connection_failure(
        expected_generation: u64,
        current_generation: u64,
        is_manual_disconnect: bool,
        is_reconnecting: bool,
        is_connected: bool,
    ) -> bool {
        expected_generation == current_generation && !is_manual_disconnect && !is_reconnecting && !is_connected
    }

    // MARK: - 접근자

    pub fn connection_state(&self) -> ChatConnectionState {
        self.inner.lock().connection_state.clone()
    }

    pub fn messages(&self) -> Vec<ChatMessage> {
        self.inner.lock().messages.clone()
    }

    pub fn session_id(&self) -> Option<String> {
        self.inner.lock().session_id.clone()
    }

    pub fn is_connected(&self) -> bool {
        self.inner.lock().connection_state.is_connected()
    }

    pub fn is_background_mode(&self) -> bool {
        self.inner.lock().background_mode_policy.enabled
    }

    /// 현재 사용 중인 accessToken (최초 값 또는 갱신 값).
    pub fn current_access_token(&self) -> String {
        self.inner.lock().current_access_token.clone()
    }

    pub fn current_extra_token(&self) -> Option<String> {
        self.inner.lock().current_extra_token.clone()
    }

    /// 이벤트 스트림. 엔진 생성 시점부터 버퍼링된다.
    pub fn events(&self) -> ChatEventStream {
        ChatEventStream { queue: self.inner.events.clone() }
    }

    // MARK: - 공개 API

    /// 채팅 서버에 연결한다. 실패하면 내부 재연결 루프로 넘긴다.
    pub async fn connect(&self) {
        let generation = {
            let mut st = self.inner.lock();
            st.connection_generation = st.connection_generation.wrapping_add(1);
            st.is_manual_disconnect = false;
            st.reconnection_owner.invalidate();
            self.inner.fail_connect_ack(&mut st, ChatError::Cancelled);
            abort(&mut st.reconnection_task);
            st.connection_generation
        };
        self.inner.reconnection.reset();
        {
            let mut st = self.inner.lock();
            if generation != st.connection_generation {
                return;
            }
            self.inner.update_connection_state(&mut st, ChatConnectionState::Connecting);
        }

        match self.inner.establish_connection(generation).await {
            Ok(()) | Err(ChatError::Cancelled) => {}
            Err(error) => self.inner.handle_initial_connection_failure(error, generation),
        }
    }

    /// 연결을 끊는다 (수동). 재연결도 멈춘다.
    pub async fn disconnect(&self) {
        let (generation, socket) = {
            let mut st = self.inner.lock();
            st.connection_generation = st.connection_generation.wrapping_add(1);
            st.is_manual_disconnect = true;
            st.reconnection_owner.invalidate();
            self.inner.fail_connect_ack(&mut st, ChatError::Cancelled);
            st.abort_all_tasks();
            (st.connection_generation, st.web_socket.clone())
        };

        if let Some(ws) = &socket {
            ws.disconnect().await;
        }
        {
            let mut st = self.inner.lock();
            if generation != st.connection_generation {
                return;
            }
            if let (Some(cur), Some(ws)) = (&st.web_socket, &socket) {
                if cur.ptr_eq(ws) {
                    st.web_socket = None;
                }
            }
        }
        self.inner.reconnection.cancel();
        let mut st = self.inner.lock();
        if generation != st.connection_generation {
            return;
        }
        st.session_id = None;
        self.inner.update_connection_state(&mut st, ChatConnectionState::Disconnected);
        tracing::debug!("Chat disconnected (manual)");
    }

    /// 채팅을 보낸다. 반환값은 이 전송의 `tid` — 13101 응답과 대조할 때 쓴다.
    pub async fn send_message(&self, text: &str, emojis: Option<&EmojiMap>) -> ChatResult<i64> {
        let (tid, message, socket) = {
            let mut st = self.inner.lock();
            if !st.connection_state.is_connected() {
                return Err(ChatError::NotConnected);
            }
            let tid = st.next_tid;
            st.next_tid = if st.next_tid == i64::MAX { 3 } else { st.next_tid + 1 };
            let message = self.inner.parser.build_send_message(
                &self.inner.config.chat_channel_id,
                self.inner.config.channel_id.as_deref(),
                st.session_id.as_deref(),
                st.current_extra_token.as_deref(),
                text,
                tid,
                emojis,
            );
            // 소켓이 없으면 무음 성공 대신 명시적 실패.
            let socket = st.web_socket.clone().ok_or(ChatError::NotConnected)?;
            (tid, message, socket)
        };
        socket.send(&message).await?;
        Ok(tid)
    }

    /// 최근 채팅을 요청한다. 연결 전이면 조용히 반환, sid 가 없으면 실패.
    pub async fn request_recent_messages(&self) -> ChatResult<()> {
        self.inner.request_recent_messages().await
    }

    /// 백그라운드 모드 — WebSocket ping 주기 감쇄. 재연결 후에도 유지된다.
    pub fn set_background_mode(&self, enabled: bool) {
        let (policy, socket) = {
            let mut st = self.inner.lock();
            let policy = BackgroundModePolicy {
                enabled,
                revision: st.background_mode_policy.revision.wrapping_add(1),
            };
            st.background_mode_policy = policy;
            (policy, st.web_socket.clone())
        };
        if let Some(ws) = socket {
            ws.set_background_mode_policy(policy);
        }
    }

    /// 메시지 버퍼를 비운다.
    pub fn clear_messages(&self) {
        let mut st = self.inner.lock();
        st.messages.clear();
        st.message_ids.clear();
        self.inner.emit(ChatEngineEvent::MessagesCleared);
    }

    // MARK: - 테스트 seam (Swift `internal`)

    #[cfg(test)]
    pub(crate) async fn refresh_tokens_if_needed(&self, expected_generation: Option<u64>) {
        self.inner.refresh_tokens_if_needed(expected_generation).await
    }

    #[cfg(test)]
    pub(crate) fn append_messages(&self, messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
        let mut st = self.inner.lock();
        self.inner.append_messages(&mut st, messages)
    }

    #[cfg(test)]
    pub(crate) fn remove_message(&self, id: &str) {
        let mut st = self.inner.lock();
        EngineInner::remove_message(&mut st, id);
    }

    #[cfg(test)]
    pub(crate) fn handle_parsed_event(&self, event: ChatEvent) {
        let mut st = self.inner.lock();
        self.inner.handle_parsed_event(&mut st, event);
    }
}

impl EngineInner {
    fn lock(&self) -> MutexGuard<'_, EngineState> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn emit(&self, event: ChatEngineEvent) {
        self.events.push(event);
    }

    fn update_connection_state(&self, st: &mut EngineState, state: ChatConnectionState) {
        st.connection_state = state.clone();
        self.emit(ChatEngineEvent::StateChanged(state));
    }

    fn is_current_connection(st: &EngineState, ws: &WebSocketService, generation: u64) -> bool {
        generation == st.connection_generation && st.web_socket.as_ref().is_some_and(|cur| cur.ptr_eq(ws))
    }

    fn handle_initial_connection_failure(&self, error: ChatError, generation: u64) {
        let mut st = self.lock();
        if !ChatEngine::should_handle_initial_connection_failure(
            generation,
            st.connection_generation,
            st.is_manual_disconnect,
            st.is_reconnecting(),
            st.connection_state.is_connected(),
        ) {
            tracing::debug!("Ignoring stale initial connection failure callback");
            return;
        }
        tracing::error!(%error, "Initial connection failed");
        self.handle_web_socket_closed(&mut st);
    }

    async fn request_recent_messages(&self) -> ChatResult<()> {
        let (request, socket) = {
            let st = self.lock();
            if !st.connection_state.is_connected() {
                return Ok(());
            }
            let session_id = st
                .session_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or(ChatError::NotConnected)?
                .to_owned();
            let request = self.parser.build_recent_chat_request(
                &self.config.chat_channel_id,
                &session_id,
                chat_defaults::RECENT_CHAT_DEFAULT_COUNT,
            );
            (request, st.web_socket.clone().ok_or(ChatError::NotConnected)?)
        };
        socket.send(&request).await
    }

    async fn fetch_recent_messages(&self, generation: u64) {
        if generation != self.lock().connection_generation {
            return;
        }
        if let Err(error) = self.request_recent_messages().await {
            tracing::warn!(%error, "Recent messages fetch failed");
        }
    }

    // MARK: - Connection

    /// 매 연결 시도 직전 토큰 갱신. 첫 시도는 건너뛰고, 실패해도 이전 토큰으로 계속한다.
    async fn refresh_tokens_if_needed(&self, expected_generation: Option<u64>) {
        let provider = {
            let mut st = self.lock();
            let consumed = st.has_consumed_initial_token;
            st.has_consumed_initial_token = true;
            if !consumed {
                return;
            }
            match &self.config.token_provider {
                Some(p) => p.clone(),
                None => return,
            }
        };

        match provider().await {
            Ok((access_token, extra_token)) => {
                let mut st = self.lock();
                // provider 가 suspend 된 동안 세대가 바뀌었으면 구 결과를 커밋하지 않는다.
                if expected_generation.is_some_and(|g| g != st.connection_generation) {
                    return;
                }
                if access_token.is_empty() {
                    tracing::warn!("Token refresh returned empty accessToken — keeping previous");
                    return;
                }
                let changed = access_token != st.current_access_token;
                st.current_access_token = access_token;
                st.current_extra_token = extra_token;
                tracing::info!(changed, "Chat token refreshed");
            }
            Err(error) => tracing::warn!(%error, "Token refresh failed, reusing previous token"),
        }
    }

    fn cookie_headers(&self) -> Vec<(String, String)> {
        let Some(provider) = &self.config.cookie_provider else { return Vec::new() };
        let cookies: Vec<String> = provider()
            .into_iter()
            .filter(|c| {
                (c.name == "NID_AUT" || c.name == "NID_SES")
                    && naver_cookie_domain_matches(&c.domain)
                    && !c.value.is_empty()
            })
            .map(|c| format!("{}={}", c.name, c.value))
            .collect();
        if cookies.is_empty() {
            tracing::warn!("No NID_AUT/NID_SES cookies found");
            return Vec::new();
        }
        tracing::info!(count = cookies.len(), "WS cookie header attached");
        vec![("Cookie".to_string(), cookies.join("; "))]
    }

    async fn establish_connection(&self, expected_generation: u64) -> ChatResult<()> {
        tracing::info!(chat_channel_id = %self.config.chat_channel_id, "Chat connecting");

        self.refresh_tokens_if_needed(Some(expected_generation)).await;
        if expected_generation != self.lock().connection_generation {
            return Err(ChatError::Cancelled);
        }

        let mut ws_config = WebSocketConfiguration::new(self.config.server_url.clone());
        ws_config.ping_interval = self.config.ping_interval;
        ws_config.open_timeout = self.config.open_timeout;
        ws_config.http_headers = self.cookie_headers();

        // 기존 WebSocket 정리 — identity 를 먼저 무효화해 구 listener 가 gate 를 통과하지 못하게.
        let old = {
            let mut st = self.lock();
            let old = st.web_socket.take();
            if old.is_some() {
                abort(&mut st.message_listen_task);
                abort(&mut st.state_listen_task);
                abort(&mut st.watchdog_task);
                abort(&mut st.pong_response_task);
                abort(&mut st.recent_messages_task);
            }
            old
        };
        if let Some(old) = old {
            old.disconnect().await;
            if expected_generation != self.lock().connection_generation {
                return Err(ChatError::Cancelled);
            }
        }

        let ws = WebSocketService::new(ws_config);
        let background_policy = {
            let mut st = self.lock();
            st.web_socket = Some(ws.clone());
            st.background_mode_policy
        };
        // 재연결로 소켓이 바뀌어도 비선택 세션의 완화된 ping 정책을 유지한다.
        ws.set_background_mode_policy(background_policy);
        self.ensure_current(&ws, expected_generation).await?;

        ws.connect().await?;
        self.ensure_current(&ws, expected_generation).await?;

        let connect_msg = {
            let st = self.lock();
            self.parser.build_connect_message(
                &self.config.chat_channel_id,
                &st.current_access_token,
                self.config.uid.as_deref(),
            )
        };
        tracing::info!(auth = if self.config.uid.is_some() { "SEND" } else { "READ" }, "Connect msg");
        ws.send(&connect_msg).await?;
        self.ensure_current(&ws, expected_generation).await?;

        // ack latch 는 리스너 기동 직전에 내린다.
        let connect_ack_attempt = {
            let mut st = self.lock();
            let attempt = self.prepare_connect_ack(&mut st);
            self.start_message_listening(&mut st, &ws, expected_generation);
            self.start_state_listening(&mut st, &ws, expected_generation);
            st.last_inbound_activity = Instant::now();
            self.start_watchdog(&mut st, &ws, expected_generation);
            attempt
        };

        // 소켓이 열리고 cmd 100 을 보낸 것까지가 전부다 — 세션 승인은 cmd 10100 이 와야 안다.
        if let Err(error) = self.await_connect_ack(connect_ack_attempt).await {
            ws.disconnect().await;
            let mut st = self.lock();
            if st.web_socket.as_ref().is_some_and(|cur| cur.ptr_eq(&ws)) {
                st.web_socket = None;
            }
            return Err(error);
        }
        Ok(())
    }

    /// await 뒤 세대·소켓 identity 재검사. 아니면 소켓을 닫고 `Cancelled`.
    async fn ensure_current(&self, ws: &WebSocketService, generation: u64) -> ChatResult<()> {
        if Self::is_current_connection(&self.lock(), ws, generation) {
            return Ok(());
        }
        ws.disconnect().await;
        let mut st = self.lock();
        if st.web_socket.as_ref().is_some_and(|cur| cur.ptr_eq(ws)) {
            st.web_socket = None;
        }
        Err(ChatError::Cancelled)
    }

    // MARK: - Connect Handshake (cmd 100 → 10100)

    fn prepare_connect_ack(&self, st: &mut EngineState) -> ConnectAckAttemptToken {
        if let Some(waiter) = st.connect_ack_waiter.take() {
            let _ = waiter.send(Err(ChatError::Cancelled));
        }
        st.connect_ack_failure = None;
        let generation = st.connection_generation;
        st.connect_ack_state.begin(generation)
    }

    fn fulfill_connect_ack(&self, st: &mut EngineState) -> bool {
        // timeout/거절이 먼저 terminal 을 차지했다면 늦은 10100 은 성공으로 뒤집지 않는다.
        if !st.connect_ack_state.mark_received() {
            return false;
        }
        st.connect_ack_failure = None;
        if let Some(waiter) = st.connect_ack_waiter.take() {
            let _ = waiter.send(Ok(()));
        }
        true
    }

    /// 대기를 포기시킨다. 대기자가 없으면 실패 latch 만 남긴다.
    fn fail_connect_ack(&self, st: &mut EngineState, error: ChatError) {
        if !st.connect_ack_state.mark_failed() {
            return;
        }
        st.connect_ack_failure = Some(error.clone());
        if let Some(waiter) = st.connect_ack_waiter.take() {
            let _ = waiter.send(Err(error));
        }
    }

    async fn await_connect_ack(&self, attempt: ConnectAckAttemptToken) -> ChatResult<()> {
        let rx = {
            let mut st = self.lock();
            if st.connect_ack_state.received() {
                return Ok(());
            }
            // 실패도 대기 시작보다 먼저 도착할 수 있다 — 원인을 즉시 전달.
            if let Some(failure) = st.connect_ack_failure.clone() {
                return Err(failure);
            }
            if let Some(stale) = st.connect_ack_waiter.take() {
                let _ = stale.send(Err(ChatError::Cancelled));
            }
            let (tx, rx) = tokio::sync::oneshot::channel();
            st.connect_ack_waiter = Some(tx);
            rx
        };

        let result = match tokio::time::timeout(secs(self.config.connect_ack_timeout), rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(ChatError::Cancelled),
            Err(_) => {
                let mut st = self.lock();
                let generation = st.connection_generation;
                if st.connect_ack_state.accepts_timeout(attempt, generation) && st.connect_ack_waiter.is_some() {
                    let error = ChatError::ConnectionFailed("연결 응답(10100) 시간 초과".into());
                    self.fail_connect_ack(&mut st, error.clone());
                    Err(error)
                } else if st.connect_ack_state.received() {
                    Ok(())
                } else {
                    Err(st.connect_ack_failure.clone().unwrap_or(ChatError::Cancelled))
                }
            }
        };
        result?;
        if attempt.generation != self.lock().connection_generation {
            return Err(ChatError::Cancelled);
        }
        Ok(())
    }

    // MARK: - Listeners

    fn start_message_listening(&self, st: &mut EngineState, ws: &WebSocketService, generation: u64) {
        abort(&mut st.message_listen_task);
        let Some(mut rx) = ws.take_messages() else { return };
        let weak = self.weak_self.clone();
        let ws = ws.clone();
        st.message_listen_task = Some(tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                let Some(inner) = weak.upgrade() else { return };
                if let Some(text) = message.text_value() {
                    let result = inner.parser.parse_events(text);
                    inner.handle_parsed_result(result, text, &ws, generation);
                }
            }
            // 스트림 종료 = 연결 종료.
            if let Some(inner) = weak.upgrade() {
                let mut st = inner.lock();
                if Self::is_current_connection(&st, &ws, generation) {
                    inner.handle_web_socket_closed(&mut st);
                }
            }
        }));
    }

    fn start_state_listening(&self, st: &mut EngineState, ws: &WebSocketService, generation: u64) {
        abort(&mut st.state_listen_task);
        let Some(mut rx) = ws.take_state_changes() else { return };
        let weak = self.weak_self.clone();
        let ws = ws.clone();
        st.state_listen_task = Some(tokio::spawn(async move {
            while let Some(state) = rx.recv().await {
                let Some(inner) = weak.upgrade() else { return };
                let mut st = inner.lock();
                if !Self::is_current_connection(&st, &ws, generation) {
                    continue;
                }
                if let WebSocketState::Failed(reason) = state {
                    tracing::error!(%reason, "WS state failed — triggering reconnection");
                    inner.handle_web_socket_closed(&mut st);
                }
            }
        }));
    }

    fn start_watchdog(&self, st: &mut EngineState, ws: &WebSocketService, generation: u64) {
        abort(&mut st.watchdog_task);
        let weak = self.weak_self.clone();
        let ws = ws.clone();
        let period = secs(self.config.watchdog_check_interval);
        st.watchdog_task = Some(tokio::spawn(async move {
            loop {
                tokio::time::sleep(period).await;
                let Some(inner) = weak.upgrade() else { return };
                inner.check_inbound_activity(&ws, generation);
            }
        }));
    }

    /// 마지막 JSON 프레임 또는 성공한 transport pong 이후 동적 silence limit 을 넘으면 재연결.
    fn check_inbound_activity(&self, ws: &WebSocketService, generation: u64) {
        let mut st = self.lock();
        if !Self::is_current_connection(&st, ws, generation) || st.is_manual_disconnect || st.is_reconnecting() {
            return;
        }
        let transport = ws.transport_liveness();
        let latest = st.last_inbound_activity.max(transport.last_activity);
        let silence = latest.elapsed().as_secs_f64();
        let limit = ChatEngine::watchdog_silence_limit(transport.ping_interval);
        if silence <= limit {
            return;
        }
        tracing::warn!(silence = silence as i64, limit = limit as i64, "Chat inactivity watchdog — forcing reconnect");
        st.last_inbound_activity = Instant::now();
        self.handle_web_socket_closed(&mut st);
    }

    // MARK: - Message Handling

    fn handle_parsed_result(&self, result: ChatResult<Vec<ChatEvent>>, raw: &str, ws: &WebSocketService, generation: u64) {
        let mut st = self.lock();
        if !Self::is_current_connection(&st, ws, generation) {
            return;
        }
        st.last_inbound_activity = Instant::now();
        match result {
            Ok(events) => {
                for event in events {
                    self.handle_parsed_event(&mut st, event);
                }
            }
            Err(error) => tracing::warn!(%error, len = raw.len(), "Failed to parse message"),
        }
    }

    fn handle_parsed_event(&self, st: &mut EngineState, event: ChatEvent) {
        match event {
            ChatEvent::Connected { session_id } => {
                if !self.fulfill_connect_ack(st) {
                    tracing::debug!("Ignoring late/duplicate Chat CONNECT ack");
                    return;
                }
                st.session_id = Some(session_id);
                self.update_connection_state(st, ChatConnectionState::Connected { server_index: 0 });
                // 핸들을 보관해 취소 가능하게 — 채널 전환 시 이전 요청이 새 소켓으로 나가지 않게.
                let generation = st.connection_generation;
                abort(&mut st.recent_messages_task);
                let weak = self.weak_self.clone();
                st.recent_messages_task = Some(tokio::spawn(async move {
                    if let Some(inner) = weak.upgrade() {
                        inner.fetch_recent_messages(generation).await;
                    }
                }));
                self.emit(ChatEngineEvent::Connected);
            }
            ChatEvent::ConnectionRejected { ret_code, ret_msg } => {
                let code = ret_code.map(|c| c.to_string()).unwrap_or_else(|| "missing".into());
                let suffix = ret_msg
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| format!(": {s}"))
                    .unwrap_or_default();
                tracing::warn!(%code, "Chat CONNECT rejected");
                self.fail_connect_ack(st, ChatError::ConnectionFailed(format!("연결 응답 거절 (retCode={code}){suffix}")));
            }
            ChatEvent::Messages(msgs) => {
                self.apply_repeated_blind_states(st, &msgs);
                let fresh = self.append_messages(st, msgs);
                if !fresh.is_empty() {
                    self.emit(ChatEngineEvent::NewMessages(fresh));
                }
            }
            ChatEvent::RecentMessages(msgs) => {
                self.apply_repeated_blind_states(st, &msgs);
                let fresh = self.append_messages(st, msgs);
                // 빈 응답도 backlog hold 를 해제하는 완료 신호이므로 반드시 전달한다.
                self.emit(ChatEngineEvent::RecentMessages(fresh));
            }
            ChatEvent::Donations(msgs) => {
                self.apply_repeated_blind_states(st, &msgs);
                let fresh = self.append_messages(st, msgs);
                if !fresh.is_empty() {
                    self.emit(ChatEngineEvent::Donations(fresh));
                }
            }
            // 공지는 상단 배너 전용 — 메시지 버퍼에 넣지 않는다.
            ChatEvent::Notice(msg) => self.emit(ChatEngineEvent::Notice(msg)),
            ChatEvent::NoticeCleared => self.emit(ChatEngineEvent::NoticeCleared),
            ChatEvent::Blind { message_id, .. } => {
                Self::remove_message(st, &message_id);
                self.emit(ChatEngineEvent::MessageBlinded { message_id, blind_text: None });
            }
            ChatEvent::Kick { user_id } => {
                if ChatEngine::is_self_kick(&user_id, self.config.uid.as_deref()) {
                    self.emit(ChatEngineEvent::Kicked);
                } else {
                    tracing::debug!("Kick broadcast for another user — ignored");
                }
            }
            ChatEvent::Penalty { user_id, duration } => {
                self.emit(ChatEngineEvent::UserPenalized { user_id, duration });
            }
            ChatEvent::Ping => {
                // 단일 pong 태스크 — 새 ping 이 오면 이전 미완료 태스크를 취소.
                let pong = self.parser.build_pong();
                let socket = st.web_socket.clone();
                abort(&mut st.pong_response_task);
                st.pong_response_task = Some(tokio::spawn(async move {
                    if let Some(ws) = socket {
                        if let Err(error) = ws.send(&pong).await {
                            tracing::warn!(%error, "Pong send failed");
                        }
                    }
                }));
            }
            ChatEvent::Pong => {}
            ChatEvent::SendConfirmed { tid, ret_code, ret_msg } => {
                if ret_code == 0 {
                    tracing::debug!("Chat send confirmed");
                } else {
                    tracing::warn!(ret_code, "Chat send failed");
                    self.emit(ChatEngineEvent::SendRejected { tid, ret_code, message: ret_msg });
                }
            }
            ChatEvent::System(msg) => self.emit(ChatEngineEvent::SystemMessage(msg)),
            ChatEvent::Unknown { cmd } => tracing::debug!(cmd, "Unknown chat command"),
        }
    }

    /// 재연결 동안 놓친 삭제가 같은 id 의 HIDDEN/CBOTBLIND 스냅샷으로 올 수 있다.
    /// 원문으로 되돌리는 역방향 갱신은 허용하지 않는다.
    fn apply_repeated_blind_states(&self, st: &mut EngineState, messages: &[ChatMessage]) {
        for message in messages.iter().filter(|m| m.is_blinded && !m.id.is_empty()) {
            if !st.message_ids.contains(&message.id) {
                continue;
            }
            let Some(index) = st.messages.iter().position(|m| m.id == message.id) else { continue };
            if st.messages[index].is_blinded {
                continue;
            }
            st.messages[index] = message.clone();
            // 서버가 준 삭제 사유(클린봇/관리자)를 그대로 싣는다.
            self.emit(ChatEngineEvent::MessageBlinded {
                message_id: message.id.clone(),
                blind_text: Some(message.content.clone()),
            });
        }
    }

    /// id 디듑 후 버퍼에 붙이고 새로 들어간 것만 돌려준다. 빈 id 는 디듑하지 않는다.
    fn append_messages(&self, st: &mut EngineState, new_messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
        let mut seen_in_batch: HashSet<String> = HashSet::new();
        let deduped: Vec<ChatMessage> = new_messages
            .into_iter()
            .filter(|m| m.id.is_empty() || (!st.message_ids.contains(&m.id) && seen_in_batch.insert(m.id.clone())))
            .collect();
        if deduped.is_empty() {
            return deduped;
        }
        st.messages.extend(deduped.iter().cloned());
        for m in deduped.iter().filter(|m| !m.id.is_empty()) {
            st.message_ids.insert(m.id.clone());
        }

        let max = self.config.max_message_buffer;
        if st.messages.len() > max + BUFFER_TRIM_SLACK {
            let excess = st.messages.len() - max;
            // 축출될 id 를 제거 **전에** 걷어낸다.
            let EngineState { messages, message_ids, .. } = st;
            for m in messages[..excess].iter().filter(|m| !m.id.is_empty()) {
                message_ids.remove(&m.id);
            }
            messages.drain(..excess);
        }
        deduped
    }

    fn remove_message(st: &mut EngineState, id: &str) {
        if let Some(index) = st.messages.iter().position(|m| m.id == id) {
            st.messages.remove(index);
            st.message_ids.remove(id);
        }
    }

    // MARK: - Reconnection

    /// 소켓 종료 공통 진입점 (호출측이 세대/identity gate 를 먼저 통과시킨다).
    fn handle_web_socket_closed(&self, st: &mut EngineState) {
        // guard 보다 먼저 핸드셰이크 대기자를 깨운다.
        self.fail_connect_ack(st, ChatError::ConnectionFailed("핸드셰이크 중 연결 종료".into()));
        abort(&mut st.recent_messages_task);
        if st.is_manual_disconnect {
            return;
        }
        if st.is_reconnecting() {
            // ack 와 executeReconnection 성공 반환 사이의 close 를 버리지 않는다.
            let nonce = st.connect_ack_state.nonce();
            st.reconnection_owner.request_reconnect(nonce);
            return;
        }
        let generation = st.connection_generation;
        let owner = st.reconnection_owner.begin(generation);
        self.update_connection_state(st, ChatConnectionState::Reconnecting { attempt: 0 });
        st.reconnection_task = Some(tokio::spawn(start_reconnection(self.weak_self.clone(), generation, owner)));
    }
}

/// 재연결 태스크가 어떤 경로로 끝나든(취소 포함) 자기 소유권만 해제한다 (Swift `defer`).
struct OwnerFinishGuard {
    weak: Weak<EngineInner>,
    owner: ReconnectionOwnerToken,
}

impl Drop for OwnerFinishGuard {
    fn drop(&mut self) {
        if let Some(inner) = self.weak.upgrade() {
            inner.lock().reconnection_owner.finish(self.owner);
        }
    }
}

/// 재연결 루프 — 소진 후에도 영구 포기하지 않고 장기 지연 뒤 정책을 리셋해 다시 시도한다.
async fn start_reconnection(weak: Weak<EngineInner>, generation: u64, owner: ReconnectionOwnerToken) {
    let _guard = OwnerFinishGuard { weak: weak.clone(), owner };
    loop {
        let Some(inner) = weak.upgrade() else { return };
        {
            let mut st = inner.lock();
            if generation != st.connection_generation || !st.reconnection_owner.owns(owner) || st.is_manual_disconnect {
                return;
            }
            inner.update_connection_state(&mut st, ChatConnectionState::Reconnecting { attempt: 1 });
            inner.emit(ChatEngineEvent::Reconnecting);
        }

        let result = inner
            .reconnection
            .execute_reconnection(|| {
                let inner = inner.clone();
                async move {
                    if generation != inner.lock().connection_generation {
                        return Err(ChatError::Cancelled);
                    }
                    inner.establish_connection(generation).await
                }
            })
            .await;

        match result {
            Ok(()) => {
                let mut st = inner.lock();
                let nonce = st.connect_ack_state.nonce();
                if st.reconnection_owner.consume_pending_reconnect(owner, nonce) {
                    tracing::info!("Connection closed during reconnect completion; continuing retry loop");
                    continue;
                }
                return;
            }
            Err(ChatError::Cancelled) => return,
            Err(_) => {
                tracing::error!("Reconnection exhausted — retrying after long-delay window");
                {
                    // 여기서 `Reconnecting` 이벤트를 내지 않는다 — 실패 상태가 보여야 수동 재연결 버튼이 뜬다.
                    let mut st = inner.lock();
                    inner.update_connection_state(
                        &mut st,
                        ChatConnectionState::Failed { reason: "재연결 실패 — 잠시 후 자동으로 다시 시도합니다".into() },
                    );
                }
                let delay = secs(inner.config.exhausted_retry_delay);
                drop(inner);
                tokio::time::sleep(delay).await;
                let Some(inner) = weak.upgrade() else { return };
                {
                    let st = inner.lock();
                    if generation != st.connection_generation
                        || !st.reconnection_owner.owns(owner)
                        || st.is_manual_disconnect
                    {
                        return;
                    }
                }
                inner.reconnection.reset();
            }
        }
    }
}
