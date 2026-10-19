//! 엔진 테스트 — `ChatEngineDedupeTests`, `ChatTokenRefreshTests`, `ChatEngineIntegrationTests`,
//! `ChatProtocolPrecisionTests` 의 엔진 부분.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use futures_util::FutureExt;

use cview_core::models::chat::{ChatConnectionState, ChatMessage, MessageType};

use crate::constants::chat_defaults;
use crate::engine::{ChatEngine, ChatEngineConfiguration, ChatEngineEvent, ChatEventStream, TokenProvider};
use crate::error::ChatError;
use crate::parser::ChatMessageParser;
use crate::protocol::ChatEvent;
use crate::reconnection::ReconnectionConfiguration;

fn make_engine(max_buffer: usize) -> ChatEngine {
    ChatEngine::new(ChatEngineConfiguration::new("test-channel", "token").max_message_buffer(max_buffer))
}

fn test_config() -> ChatEngineConfiguration {
    ChatEngineConfiguration::new("test-channel-001", "test-access-token")
        .extra_token(Some("test-extra-token".into()))
        .uid(Some("test-uid".into()))
        .channel_id(Some("channel-id".into()))
        .server_url("wss://localhost:0/chat")
        .reconnection_config(ReconnectionConfiguration {
            initial_delay: 0.01,
            max_delay: 0.05,
            max_attempts: 3,
            backoff_multiplier: 1.5,
            jitter_factor: 0.0,
            ..ReconnectionConfiguration::default()
        })
}

fn msg(id: &str, text: &str) -> ChatMessage {
    ChatMessage::with_id(id, "n", text)
}

fn ids(engine: &ChatEngine) -> Vec<String> {
    engine.messages().into_iter().map(|m| m.id).collect()
}

/// `clearMessages()` 센티넬까지 쌓인 이벤트를 모은다 (타임아웃 의존 없음).
async fn drain_until_cleared(stream: &ChatEventStream) -> Vec<ChatEngineEvent> {
    let mut out = Vec::new();
    while let Some(event) = stream.recv().await {
        if event == ChatEngineEvent::MessagesCleared {
            break;
        }
        out.push(event);
    }
    out
}

// MARK: - Dedupe

#[test]
fn duplicate_ids_are_rejected() {
    let engine = make_engine(chat_defaults::MAX_MESSAGE_BUFFER);
    engine.append_messages(vec![msg("a", "hello"), msg("b", "hello")]);
    engine.append_messages(vec![msg("a", "hello"), msg("b", "hello"), msg("c", "hello")]);
    assert_eq!(ids(&engine), ["a", "b", "c"]);
}

#[test]
fn duplicates_within_one_batch_are_rejected() {
    let engine = make_engine(chat_defaults::MAX_MESSAGE_BUFFER);
    engine.append_messages(vec![msg("x", "hello"), msg("x", "hello"), msg("y", "hello")]);
    assert_eq!(ids(&engine), ["x", "y"]);
}

#[test]
fn empty_ids_bypass_dedupe() {
    let engine = make_engine(chat_defaults::MAX_MESSAGE_BUFFER);
    engine.append_messages(vec![msg("", "sys1"), msg("", "sys2")]);
    let contents: Vec<String> = engine.messages().into_iter().map(|m| m.content).collect();
    assert_eq!(contents, ["sys1", "sys2"]);
}

#[test]
fn trimmed_ids_can_be_re_added() {
    let max_buffer = 10;
    let engine = make_engine(max_buffer);
    let total = max_buffer + 256 + 5;
    engine.append_messages((0..total).map(|i| msg(&format!("m{i}"), "hello")).collect());
    let after = engine.messages();
    assert_eq!(after.len(), max_buffer);
    assert!(!after.iter().any(|m| m.id == "m0"), "트림이 일어나지 않았다");

    engine.append_messages(vec![msg("m0", "재수신")]);
    let contents: Vec<String> = engine.messages().into_iter().filter(|m| m.id == "m0").map(|m| m.content).collect();
    assert_eq!(contents, ["재수신"], "트림으로 축출된 id 가 디듑 집합에 유령으로 남았다");
}

#[test]
fn removed_id_can_be_re_added() {
    let engine = make_engine(chat_defaults::MAX_MESSAGE_BUFFER);
    engine.append_messages(vec![msg("del", "원본")]);
    engine.remove_message("del");
    assert!(engine.messages().is_empty());
    engine.append_messages(vec![msg("del", "재수신")]);
    let contents: Vec<String> = engine.messages().into_iter().map(|m| m.content).collect();
    assert_eq!(contents, ["재수신"]);
}

#[test]
fn cleared_ids_can_be_re_added() {
    let engine = make_engine(chat_defaults::MAX_MESSAGE_BUFFER);
    engine.append_messages(vec![msg("p", "hello"), msg("q", "hello")]);
    engine.clear_messages();
    assert!(engine.messages().is_empty());
    engine.append_messages(vec![msg("p", "hello"), msg("q", "hello")]);
    assert_eq!(ids(&engine), ["p", "q"]);
}

#[test]
fn order_is_preserved() {
    let engine = make_engine(chat_defaults::MAX_MESSAGE_BUFFER);
    engine.append_messages((0..20).map(|i| msg(&format!("o{i}"), "hello")).collect());
    let expected: Vec<String> = (0..20).map(|i| format!("o{i}")).collect();
    assert_eq!(ids(&engine), expected);
}

// MARK: - Token refresh

struct TokenStub {
    calls: Arc<AtomicUsize>,
    provider: TokenProvider,
}

fn token_stub(access: &str, extra: Option<&str>, should_fail: bool) -> TokenStub {
    let calls = Arc::new(AtomicUsize::new(0));
    let c = calls.clone();
    let access = access.to_owned();
    let extra = extra.map(str::to_owned);
    let provider: TokenProvider = Arc::new(move || {
        c.fetch_add(1, Ordering::SeqCst);
        let result = if should_fail {
            Err(ChatError::ConnectionFailed("token endpoint down".into()))
        } else {
            Ok((access.clone(), extra.clone()))
        };
        async move { result }.boxed()
    });
    TokenStub { calls, provider }
}

fn token_engine(provider: Option<TokenProvider>) -> ChatEngine {
    ChatEngine::new(
        ChatEngineConfiguration::new("test-chat-channel", "initial-token")
            .extra_token(Some("initial-extra".into()))
            .token_provider(provider),
    )
}

#[tokio::test]
async fn first_attempt_does_not_refresh() {
    let stub = token_stub("refreshed", Some("refreshed-extra"), false);
    let engine = token_engine(Some(stub.provider.clone()));
    engine.refresh_tokens_if_needed(None).await;
    assert_eq!(stub.calls.load(Ordering::SeqCst), 0);
    assert_eq!(engine.current_access_token(), "initial-token");
    assert_eq!(engine.current_extra_token().as_deref(), Some("initial-extra"));
}

#[tokio::test]
async fn retries_refresh() {
    let stub = token_stub("refreshed", Some("refreshed-extra"), false);
    let engine = token_engine(Some(stub.provider.clone()));
    engine.refresh_tokens_if_needed(None).await;
    engine.refresh_tokens_if_needed(None).await;
    assert_eq!(stub.calls.load(Ordering::SeqCst), 1);
    assert_eq!(engine.current_access_token(), "refreshed");
    assert_eq!(engine.current_extra_token().as_deref(), Some("refreshed-extra"));
}

#[tokio::test]
async fn refreshes_on_every_retry() {
    let stub = token_stub("refreshed", None, false);
    let engine = token_engine(Some(stub.provider.clone()));
    for _ in 0..4 {
        engine.refresh_tokens_if_needed(None).await;
    }
    assert_eq!(stub.calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn failure_keeps_previous_token() {
    let stub = token_stub("never-used", None, true);
    let engine = token_engine(Some(stub.provider.clone()));
    engine.refresh_tokens_if_needed(None).await;
    engine.refresh_tokens_if_needed(None).await;
    assert_eq!(stub.calls.load(Ordering::SeqCst), 1);
    assert_eq!(engine.current_access_token(), "initial-token");
    assert_eq!(engine.current_extra_token().as_deref(), Some("initial-extra"));
}

#[tokio::test]
async fn empty_token_is_rejected() {
    let stub = token_stub("", Some("some-extra"), false);
    let engine = token_engine(Some(stub.provider.clone()));
    engine.refresh_tokens_if_needed(None).await;
    engine.refresh_tokens_if_needed(None).await;
    assert_eq!(stub.calls.load(Ordering::SeqCst), 1);
    assert_eq!(engine.current_access_token(), "initial-token");
    assert_eq!(engine.current_extra_token().as_deref(), Some("initial-extra"));
}

#[tokio::test]
async fn no_provider_keeps_initial_token() {
    let engine = token_engine(None);
    for _ in 0..3 {
        engine.refresh_tokens_if_needed(None).await;
    }
    assert_eq!(engine.current_access_token(), "initial-token");
    assert_eq!(engine.current_extra_token().as_deref(), Some("initial-extra"));
}

#[tokio::test]
async fn extra_token_can_become_none() {
    let stub = token_stub("refreshed", None, false);
    let engine = token_engine(Some(stub.provider.clone()));
    engine.refresh_tokens_if_needed(None).await;
    engine.refresh_tokens_if_needed(None).await;
    assert_eq!(engine.current_access_token(), "refreshed");
    assert_eq!(engine.current_extra_token(), None);
}

// MARK: - Parsed event handling (precision)

#[tokio::test]
async fn repeated_hidden_snapshot_updates_existing_message_once() {
    let engine = ChatEngine::new(ChatEngineConfiguration::new("test", "test"));
    let stream = engine.events();
    let original = ChatMessage::with_id("m1", "test", "original");
    let hidden = ChatMessage::with_id("m1", "test", ChatMessageParser::ADMIN_BLIND_TEXT).blinded(true);
    engine.handle_parsed_event(ChatEvent::Messages(vec![original.clone()]));
    engine.handle_parsed_event(ChatEvent::RecentMessages(vec![hidden.clone()]));
    engine.handle_parsed_event(ChatEvent::Messages(vec![hidden.clone(), original]));
    let first = engine.messages().into_iter().next().unwrap();
    assert!(first.is_blinded);
    assert_eq!(first.content, hidden.content);
    engine.clear_messages();

    let kinds: Vec<String> = drain_until_cleared(&stream)
        .await
        .into_iter()
        .map(|e| match e {
            ChatEngineEvent::NewMessages(_) => "new".to_string(),
            ChatEngineEvent::MessageBlinded { message_id, .. } => format!("blind:{message_id}"),
            ChatEngineEvent::RecentMessages(items) => format!("recent:{}", items.len()),
            other => format!("unexpected:{other:?}"),
        })
        .collect();
    assert_eq!(kinds, ["new", "blind:m1", "recent:0"]);
}

#[tokio::test]
async fn repeated_clean_bot_snapshot_carries_its_own_blind_text() {
    let engine = ChatEngine::new(ChatEngineConfiguration::new("test", "test"));
    let stream = engine.events();
    let original = ChatMessage::with_id("m1", "test", "원문");
    let clean_bot = ChatMessage::with_id("m1", "test", ChatMessageParser::CLEAN_BOT_BLIND_TEXT).blinded(true);
    engine.handle_parsed_event(ChatEvent::Messages(vec![original]));
    engine.handle_parsed_event(ChatEvent::RecentMessages(vec![clean_bot]));
    assert_eq!(engine.messages()[0].content, ChatMessageParser::CLEAN_BOT_BLIND_TEXT);
    engine.clear_messages();
    let blind_texts: Vec<Option<String>> = drain_until_cleared(&stream)
        .await
        .into_iter()
        .filter_map(|e| match e {
            ChatEngineEvent::MessageBlinded { blind_text, .. } => Some(blind_text),
            _ => None,
        })
        .collect();
    assert_eq!(blind_texts, [Some(ChatMessageParser::CLEAN_BOT_BLIND_TEXT.to_string())]);
}

#[test]
fn notice_never_enters_message_buffer() {
    let engine = ChatEngine::new(ChatEngineConfiguration::new("test", "test"));
    let notice = ChatMessage::with_id("notice_1", "공지", "공지 본문").message_type(MessageType::Notice);
    engine.handle_parsed_event(ChatEvent::Notice(notice));
    assert!(engine.messages().is_empty());
    engine.handle_parsed_event(ChatEvent::Messages(vec![ChatMessage::with_id("m1", "test", "hello")]));
    assert_eq!(engine.messages().len(), 1);
    assert_ne!(engine.messages()[0].r#type, MessageType::Notice);
}

#[tokio::test]
async fn duplicate_bursts_do_not_reach_consumers_and_empty_recent_still_completes() {
    let engine = ChatEngine::new(ChatEngineConfiguration::new("test", "test"));
    let stream = engine.events();
    let m = ChatMessage::with_id("m1", "test", "synthetic");
    engine.handle_parsed_event(ChatEvent::Messages(vec![m.clone(), m.clone()]));
    for _ in 0..1000 {
        engine.handle_parsed_event(ChatEvent::Messages(vec![m.clone()]));
    }
    engine.handle_parsed_event(ChatEvent::Donations(vec![m.clone()]));
    engine.handle_parsed_event(ChatEvent::RecentMessages(vec![m]));
    engine.handle_parsed_event(ChatEvent::NoticeCleared);
    engine.clear_messages();
    let collected = drain_until_cleared(&stream).await;
    assert_eq!(collected.len(), 3, "{collected:?}");
    match (&collected[0], &collected[1], &collected[2]) {
        (ChatEngineEvent::NewMessages(fresh), ChatEngineEvent::RecentMessages(recent), ChatEngineEvent::NoticeCleared) => {
            assert_eq!(fresh.len(), 1);
            assert!(recent.is_empty());
        }
        other => panic!("Event ordering changed: {other:?}"),
    }
}

// MARK: - Integration (no server)

#[test]
fn initial_state() {
    let engine = ChatEngine::new(test_config());
    assert_eq!(engine.connection_state(), ChatConnectionState::Disconnected);
    assert!(engine.messages().is_empty());
    assert_eq!(engine.session_id(), None);
    assert!(!engine.is_connected());
    assert!(engine.events().try_recv().is_none());
}

#[tokio::test]
async fn background_mode_is_durable() {
    let engine = ChatEngine::new(test_config());
    engine.set_background_mode(true);
    assert!(engine.is_background_mode());
    engine.disconnect().await;
    assert!(engine.is_background_mode());
    engine.set_background_mode(false);
    assert!(!engine.is_background_mode());
}

#[tokio::test]
async fn clear_messages_emits_event() {
    let engine = ChatEngine::new(test_config());
    let stream = engine.events();
    engine.clear_messages();
    assert!(engine.messages().is_empty());
    assert_eq!(stream.recv().await, Some(ChatEngineEvent::MessagesCleared));
}

#[tokio::test]
async fn send_message_fails_when_disconnected() {
    let engine = ChatEngine::new(test_config());
    assert_eq!(engine.send_message("test message", None).await, Err(ChatError::NotConnected));
}

#[tokio::test]
async fn request_recent_messages_silently_returns_when_disconnected() {
    let engine = ChatEngine::new(test_config());
    assert_eq!(engine.request_recent_messages().await, Ok(()));
}

#[tokio::test]
async fn disconnect_is_idempotent_and_emits_state_changed() {
    let engine = ChatEngine::new(test_config());
    let stream = engine.events();
    engine.disconnect().await;
    engine.disconnect().await;
    engine.disconnect().await;
    assert_eq!(engine.connection_state(), ChatConnectionState::Disconnected);
    let mut saw = false;
    while let Some(e) = stream.try_recv() {
        if e == ChatEngineEvent::StateChanged(ChatConnectionState::Disconnected) {
            saw = true;
        }
    }
    assert!(saw);
}

#[test]
fn configuration_server_url() {
    let a = ChatEngineConfiguration::new("ABCDEF", "tok");
    let b = ChatEngineConfiguration::new("ABCDEF", "tok");
    assert_eq!(a.server_url, b.server_url);
    for id in ["AAA", "ZZZ"] {
        let url = ChatEngineConfiguration::new(id, "tok").server_url;
        assert!(url.starts_with("wss://kr-ss"), "{url}");
        assert!(url.ends_with(".chat.naver.com/chat"));
    }
    // "AAA" = 65*3 = 195 → 195 % 9 + 1 = 7
    assert_eq!(ChatEngineConfiguration::compute_server_url("AAA"), "wss://kr-ss7.chat.naver.com/chat");
    let custom = ChatEngineConfiguration::new("ch1", "tok").server_url("wss://custom.server.com/chat");
    assert_eq!(custom.server_url, "wss://custom.server.com/chat");
}

#[test]
fn configuration_max_buffer() {
    assert_eq!(ChatEngineConfiguration::new("ch1", "tok").max_message_buffer, chat_defaults::MAX_MESSAGE_BUFFER);
    assert_eq!(ChatEngineConfiguration::new("ch1", "tok").max_message_buffer(100).max_message_buffer, 100);
}

#[test]
fn connection_state_display_text() {
    assert_eq!(ChatConnectionState::Disconnected.display_text(), "연결 안됨");
    assert_eq!(ChatConnectionState::Connecting.display_text(), "연결 중…");
    assert_eq!(ChatConnectionState::Connected { server_index: 3 }.display_text(), "연결됨 (서버 3)");
    assert_eq!(ChatConnectionState::Reconnecting { attempt: 2 }.display_text(), "재연결 중 (2회)");
    assert_eq!(ChatConnectionState::Failed { reason: "timeout".into() }.display_text(), "연결 실패: timeout");
    assert!(!ChatConnectionState::Disconnected.is_connected());
    assert!(ChatConnectionState::Connected { server_index: 0 }.is_connected());
    assert!(!ChatConnectionState::Reconnecting { attempt: 1 }.is_connected());
}
