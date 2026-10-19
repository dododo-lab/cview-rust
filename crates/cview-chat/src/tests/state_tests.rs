//! 순수 소유권 상태 테스트 — `ChatProtocolInvariantTests.swift` 의 값 상태 부분,
//! `ChatSocketOpenHandshakeTests.swift` 의 판정 함수, `ChatConnectHandshakeTests.swift` 의 상한 검사.

use crate::constants::{chat_defaults, ws_defaults};
use crate::engine::{ChatEngine, ConnectAckAttemptState, ReconnectionOwnerState};
use crate::websocket::{
    BackgroundModePolicy, BackgroundModePolicyState, WebSocketAttemptState,
    WebSocketOpenResumeDisposition, WebSocketSendResumeDisposition, WebSocketService,
    WebSocketState,
};

#[test]
fn connect_ack_timeout_uses_per_handshake_nonce() {
    let mut state = ConnectAckAttemptState::default();
    let stale = state.begin(7);
    let current = state.begin(7);
    assert!(!state.accepts_timeout(stale, 7));
    assert!(state.accepts_timeout(current, 7));

    assert!(state.mark_failed());
    assert!(
        !state.mark_received(),
        "terminal failure 뒤의 늦은 ack를 받아서는 안 된다"
    );
    assert!(!state.accepts_timeout(current, 7));

    let next = state.begin(7);
    assert!(state.mark_received());
    assert!(
        !state.mark_failed(),
        "terminal success를 늦은 close가 뒤집어서는 안 된다"
    );
    assert!(!state.accepts_timeout(next, 7));
    assert!(!state.accepts_timeout(next, 8));
}

#[test]
fn web_socket_attempt_tokens_are_monotonic() {
    let mut state = WebSocketAttemptState::default();
    let timed_out = state.begin_connection();
    assert!(state.mark_open_failed(timed_out));
    assert!(
        !state.mark_opened(timed_out),
        "late delegate open must not reverse timeout"
    );

    let open = state.begin_connection();
    assert!(state.mark_opened(open));
    assert!(
        !state.mark_open_failed(open),
        "late timeout must not reverse open"
    );

    let first_connection = state.begin_connection();
    let first_ping = state.begin_ping();
    let replacement_ping = state.begin_ping();
    assert!(state.is_current(first_connection));
    assert!(!state.is_current_ping(first_ping));
    assert!(state.is_current_ping(replacement_ping));

    let second_connection = state.begin_connection();
    assert!(!state.is_current(first_connection));
    assert!(!state.is_current_ping(replacement_ping));
    assert!(state.is_current(second_connection));

    state.invalidate_connection(None);
    assert!(!state.is_current(second_connection));
}

#[test]
fn stale_send_completion_is_not_connected() {
    let mut state = WebSocketAttemptState::default();
    let stale = state.begin_connection();
    assert!(state.mark_opened(stale));
    let before =
        WebSocketService::send_resume_disposition(stale, &state, &WebSocketState::Connected, true);

    state.invalidate_connection(None);
    let after_disconnect = WebSocketService::send_resume_disposition(
        stale,
        &state,
        &WebSocketState::Disconnected,
        false,
    );

    let replacement = state.begin_connection();
    assert!(state.mark_opened(replacement));
    let after_reconnect =
        WebSocketService::send_resume_disposition(stale, &state, &WebSocketState::Connected, true);
    let mismatch = WebSocketService::send_resume_disposition(
        replacement,
        &state,
        &WebSocketState::Connected,
        false,
    );

    assert_eq!(before, WebSocketSendResumeDisposition::Sent);
    assert_eq!(
        after_disconnect,
        WebSocketSendResumeDisposition::NotConnected
    );
    assert_eq!(
        after_reconnect,
        WebSocketSendResumeDisposition::NotConnected
    );
    assert_eq!(mismatch, WebSocketSendResumeDisposition::NotConnected);
}

#[test]
fn background_policy_is_latest_wins() {
    let mut socket_policy = BackgroundModePolicyState::default();
    let connect_snapshot = BackgroundModePolicy {
        enabled: false,
        revision: 4,
    };
    let latest = BackgroundModePolicy {
        enabled: true,
        revision: 5,
    };
    assert!(socket_policy.apply(latest));
    assert!(!socket_policy.apply(connect_snapshot));
    assert_eq!(socket_policy.policy(), Some(latest));
}

#[test]
fn reconnect_owner_only_finishes_itself() {
    let mut state = ReconnectionOwnerState::default();
    let stale = state.begin(11);
    state.invalidate();
    let current = state.begin(11);

    state.finish(stale);
    assert!(state.is_active());
    assert!(state.owns(current));

    assert!(state.request_reconnect(41));
    assert!(!state.consume_pending_reconnect(current, 42));
    assert!(state.request_reconnect(42));
    assert!(state.consume_pending_reconnect(current, 42));

    state.finish(current);
    assert!(!state.is_active());
}

#[test]
fn initial_failure_caller_attribution() {
    assert!(ChatEngine::should_handle_initial_connection_failure(
        3, 3, false, false, false
    ));
    assert!(!ChatEngine::should_handle_initial_connection_failure(
        3, 3, false, true, false
    ));
    assert!(!ChatEngine::should_handle_initial_connection_failure(
        3, 3, false, false, true
    ));
    assert!(!ChatEngine::should_handle_initial_connection_failure(
        2, 3, false, false, false
    ));
    assert!(!ChatEngine::should_handle_initial_connection_failure(
        3, 3, true, false, false
    ));
}

#[test]
fn watchdog_exceeds_every_effective_ping_interval() {
    let ac = chat_defaults::PING_INTERVAL;
    let battery = chat_defaults::PING_INTERVAL * 1.5;
    let intervals = [
        WebSocketService::effective_ping_interval(ac, false),
        WebSocketService::effective_ping_interval(ac, true),
        WebSocketService::effective_ping_interval(battery, false),
        WebSocketService::effective_ping_interval(battery, true),
    ];
    assert_eq!(intervals, [45.0, 90.0, 67.5, 135.0]);
    for p in intervals {
        assert!(ChatEngine::watchdog_silence_limit(p) > p);
    }
    assert_eq!(ChatEngine::watchdog_silence_limit(135.0), 165.0);
}

#[test]
fn immediate_close_after_open_preserves_failure() {
    let mut state = WebSocketAttemptState::default();
    let attempt = state.begin_connection();
    assert!(state.mark_opened(attempt));
    state.invalidate_connection(Some(attempt));

    let closed = WebSocketService::open_resume_disposition(
        attempt,
        &state,
        &WebSocketState::Failed("server closed".into()),
    );
    assert_eq!(
        closed,
        WebSocketOpenResumeDisposition::ConnectionFailed("server closed".into())
    );

    let replacement = state.begin_connection();
    let superseded =
        WebSocketService::open_resume_disposition(attempt, &state, &WebSocketState::Connecting);
    assert_ne!(replacement, attempt);
    assert_eq!(superseded, WebSocketOpenResumeDisposition::Cancelled);

    assert!(state.mark_opened(replacement));
    let connected =
        WebSocketService::open_resume_disposition(replacement, &state, &WebSocketState::Connected);
    assert_eq!(connected, WebSocketOpenResumeDisposition::Connected);
    state.invalidate_connection(None);
    let manual = WebSocketService::open_resume_disposition(
        replacement,
        &state,
        &WebSocketState::Disconnected,
    );
    assert_eq!(manual, WebSocketOpenResumeDisposition::Cancelled);
}

#[test]
fn timeouts_are_sane() {
    const {
        assert!(ws_defaults::OPEN_TIMEOUT > 0.0);
        assert!(ws_defaults::OPEN_TIMEOUT < ws_defaults::REQUEST_TIMEOUT);
        assert!(chat_defaults::CONNECT_ACK_TIMEOUT >= 3.0);
        assert!(chat_defaults::CONNECT_ACK_TIMEOUT <= 30.0);
    }
}
