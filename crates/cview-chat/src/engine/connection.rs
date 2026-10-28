//! 연결 수립과 접속 핸드셰이크(cmd 100 → 10100).

use std::time::Instant;

use super::{abort, naver_cookie_domain_matches, ConnectAckAttemptToken, EngineInner, EngineState};
use crate::constants::secs;
use crate::error::{ChatError, ChatResult};
use crate::websocket::{WebSocketConfiguration, WebSocketService};

impl EngineInner {
    /// 매 연결 시도 직전 토큰 갱신. 첫 시도는 건너뛰고, 실패해도 이전 토큰으로 계속한다.
    pub(super) async fn refresh_tokens_if_needed(&self, expected_generation: Option<u64>) {
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
        let Some(provider) = &self.config.cookie_provider else {
            return Vec::new();
        };
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

    pub(super) async fn establish_connection(&self, expected_generation: u64) -> ChatResult<()> {
        tracing::info!(chat_channel_id = %self.config.chat_channel_id, "Chat connecting");

        self.refresh_tokens_if_needed(Some(expected_generation))
            .await;
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
        tracing::info!(
            auth = if self.config.uid.is_some() {
                "SEND"
            } else {
                "READ"
            },
            "Connect msg"
        );
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

    pub(super) fn fulfill_connect_ack(&self, st: &mut EngineState) -> bool {
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
    pub(super) fn fail_connect_ack(&self, st: &mut EngineState, error: ChatError) {
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
                if st.connect_ack_state.accepts_timeout(attempt, generation)
                    && st.connect_ack_waiter.is_some()
                {
                    let error = ChatError::ConnectionFailed("연결 응답(10100) 시간 초과".into());
                    self.fail_connect_ack(&mut st, error.clone());
                    Err(error)
                } else if st.connect_ack_state.received() {
                    Ok(())
                } else {
                    Err(st
                        .connect_ack_failure
                        .clone()
                        .unwrap_or(ChatError::Cancelled))
                }
            }
        };
        result?;
        if attempt.generation != self.lock().connection_generation {
            return Err(ChatError::Cancelled);
        }
        Ok(())
    }
}
