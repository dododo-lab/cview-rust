//! WebSocket·엔진 동작 테스트 — 로컬 tokio-tungstenite 서버만 쓴다 (외부 네트워크 없음).
//!
//! Swift 의 소스 대조 배선 테스트(`ConnectAckWiringTests`, `WebSocketOpenWiringTests`,
//! `TokenRefreshWiringTests` 등)가 고정하던 계약을 실제 동작으로 검증한다:
//! - 업그레이드 실패는 `connect()` 오류이며 `Connected` 를 거치지 않는다.
//! - 엔진은 cmd 10100 이 와야 연결 성공이다 (거절·무응답은 실패).
//! - 연결 메시지는 현재 토큰을 쓰고, 성공 뒤 최근 채팅(5101)을 sid 로 요청한다.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;

use crate::engine::{ChatEngine, ChatEngineConfiguration, ChatEngineEvent, ChatEventStream};
use crate::reconnection::ReconnectionConfiguration;
use crate::websocket::{
    WebSocketConfiguration, WebSocketMessage, WebSocketService, WebSocketState,
};

const WAIT: Duration = Duration::from_secs(5);

/// 서버가 받은 텍스트 프레임을 테스트로 넘기고, 테스트가 보낸 텍스트를 클라이언트로 보낸다.
struct LocalServer {
    url: String,
    received: mpsc::UnboundedReceiver<String>,
    outgoing: mpsc::UnboundedSender<String>,
}

/// 연결 하나만 받는 로컬 WebSocket 서버.
async fn spawn_server() -> LocalServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let (recv_tx, received) = mpsc::unbounded_channel::<String>();
    let (outgoing, mut out_rx) = mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        let Ok((stream, _)) = listener.accept().await else {
            return;
        };
        let Ok(ws) = tokio_tungstenite::accept_async(stream).await else {
            return;
        };
        let (mut sink, mut source) = ws.split();
        loop {
            tokio::select! {
                incoming = source.next() => match incoming {
                    Some(Ok(Message::Text(t))) => { let _ = recv_tx.send(t); }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(_)) => {}
                },
                out = out_rx.recv() => match out {
                    Some(t) => { if sink.send(Message::Text(t)).await.is_err() { break; } }
                    None => break,
                },
            }
        }
    });
    LocalServer {
        url,
        received,
        outgoing,
    }
}

async fn next_frame(server: &mut LocalServer) -> Value {
    let text = timeout(WAIT, server.received.recv())
        .await
        .expect("server receive timed out")
        .expect("closed");
    serde_json::from_str(&text).expect("client sent invalid JSON")
}

async fn wait_for_event(
    stream: &ChatEventStream,
    pred: impl Fn(&ChatEngineEvent) -> bool,
) -> ChatEngineEvent {
    timeout(WAIT, async {
        loop {
            let e = stream.recv().await.expect("stream closed");
            if pred(&e) {
                return e;
            }
        }
    })
    .await
    .expect("event wait timed out")
}

fn engine_config(url: &str) -> ChatEngineConfiguration {
    let mut config = ChatEngineConfiguration::new("cid-1", "acc-token")
        .uid(Some("me".into()))
        .server_url(url)
        .reconnection_config(ReconnectionConfiguration {
            initial_delay: 5.0,
            max_delay: 5.0,
            max_attempts: 1,
            backoff_multiplier: 1.0,
            jitter_factor: 0.0,
            ..ReconnectionConfiguration::default()
        });
    config.open_timeout = 2.0;
    config.connect_ack_timeout = 2.0;
    config
}

// MARK: - WebSocketService

#[tokio::test]
async fn refused_connection_throws_and_never_passes_through_connected() {
    let mut config = WebSocketConfiguration::new("ws://127.0.0.1:1");
    config.open_timeout = 1.0;
    let service = WebSocketService::new(config);
    let mut states = service.take_state_changes().unwrap();

    assert!(service.connect().await.is_err());
    assert_ne!(service.state(), WebSocketState::Connected);

    let mut seen = Vec::new();
    while let Ok(s) = states.try_recv() {
        seen.push(s);
    }
    assert!(
        !seen.contains(&WebSocketState::Connected),
        "실패한 연결이 Connected 를 거쳤다: {seen:?}"
    );
}

#[tokio::test]
async fn service_round_trip_with_local_server() {
    let mut server = spawn_server().await;
    let service = WebSocketService::new(WebSocketConfiguration::new(server.url.clone()));
    let mut messages = service.take_messages().unwrap();

    // 연결 전 전송은 실패.
    assert!(service.send("x").await.is_err());

    service.connect().await.unwrap();
    assert_eq!(service.state(), WebSocketState::Connected);
    // 이미 연결된 상태에서의 connect 는 no-op.
    service.connect().await.unwrap();

    service.send(r#"{"cmd":0}"#).await.unwrap();
    assert_eq!(next_frame(&mut server).await["cmd"], 0);

    server.outgoing.send("hello".into()).unwrap();
    let got = timeout(WAIT, messages.recv()).await.unwrap().unwrap();
    assert_eq!(got, WebSocketMessage::Text("hello".into()));

    service.set_background_mode(true);
    assert!(service.is_background_mode());

    service.disconnect().await;
    assert_eq!(service.state(), WebSocketState::Disconnected);
    assert!(service.send("x").await.is_err());
}

#[tokio::test]
async fn server_close_moves_service_out_of_connected() {
    let server = spawn_server().await;
    let service = WebSocketService::new(WebSocketConfiguration::new(server.url.clone()));
    let mut states = service.take_state_changes().unwrap();
    service.connect().await.unwrap();
    drop(server); // 서버 태스크의 송신 채널이 닫혀 연결을 끊는다.

    let terminal = timeout(WAIT, async {
        loop {
            match states.recv().await {
                Some(WebSocketState::Failed(r)) => return WebSocketState::Failed(r),
                Some(WebSocketState::Disconnected) => return WebSocketState::Disconnected,
                Some(_) => continue,
                None => panic!("state stream closed"),
            }
        }
    })
    .await
    .expect("no terminal state after server close");
    assert_ne!(terminal, WebSocketState::Connected);
    assert_ne!(service.state(), WebSocketState::Connected);
}

// MARK: - ChatEngine end-to-end

#[tokio::test]
async fn engine_handshake_recent_chat_and_send() {
    let mut server = spawn_server().await;
    let engine = ChatEngine::new(engine_config(&server.url));
    let events = engine.events();

    let server_task = {
        let engine = engine.clone();
        tokio::spawn(async move { engine.connect().await })
    };

    // cmd 100 — 현재 토큰과 uid 를 담는다.
    let connect = next_frame(&mut server).await;
    assert_eq!(connect["cmd"], 100);
    assert_eq!(connect["cid"], "cid-1");
    assert_eq!(connect["bdy"]["accTkn"], "acc-token");
    assert_eq!(connect["bdy"]["uid"], "me");
    assert_eq!(connect["bdy"]["auth"], "SEND");
    assert!(!engine.is_connected(), "10100 전에는 연결 성공이 아니다");

    server
        .outgoing
        .send(r#"{"cmd":10100,"retCode":0,"bdy":{"sid":"s1"}}"#.into())
        .unwrap();
    timeout(WAIT, server_task)
        .await
        .expect("connect() did not return")
        .unwrap();
    assert!(engine.is_connected());
    assert_eq!(engine.session_id().as_deref(), Some("s1"));
    wait_for_event(&events, |e| *e == ChatEngineEvent::Connected).await;

    // 성공 직후 최근 채팅 요청.
    let recent = next_frame(&mut server).await;
    assert_eq!(recent["cmd"], 5101);
    assert_eq!(recent["sid"], "s1");

    // 서버 → 채팅 메시지.
    server
        .outgoing
        .send(
            r#"{"cmd":93101,"bdy":[{"uid":"u1","profile":"{\"nickname\":\"User1\"}","msg":"Hello","msgTime":1700000000000,"msgTypeCode":1,"extras":"{}"}]}"#
                .into(),
        )
        .unwrap();
    let new = wait_for_event(&events, |e| matches!(e, ChatEngineEvent::NewMessages(_))).await;
    let ChatEngineEvent::NewMessages(msgs) = new else {
        unreachable!()
    };
    assert_eq!(msgs[0].content, "Hello");
    assert_eq!(engine.messages().len(), 1);

    // 서버 ping(cmd 0) → pong(cmd 10000).
    server.outgoing.send(r#"{"cmd":0}"#.into()).unwrap();
    let pong = next_frame(&mut server).await;
    assert_eq!(pong["cmd"], 10000);

    // 클라이언트 → 채팅 전송 (cmd 3101, sid 포함).
    let tid = engine.send_message("hi there", None).await.unwrap();
    let sent = next_frame(&mut server).await;
    assert_eq!(sent["cmd"], 3101);
    assert_eq!(sent["sid"], "s1");
    // 전송 프레임의 tid 는 Swift 와 같이 문자열이다.
    assert_eq!(sent["tid"], tid.to_string());
    assert_eq!(sent["bdy"]["msg"], "hi there");

    engine.disconnect().await;
    assert!(!engine.is_connected());
    assert_eq!(engine.session_id(), None);
}

#[tokio::test]
async fn engine_rejected_handshake_is_failure() {
    let mut server = spawn_server().await;
    let engine = ChatEngine::new(engine_config(&server.url));

    let connect_task = {
        let engine = engine.clone();
        tokio::spawn(async move { engine.connect().await })
    };
    assert_eq!(next_frame(&mut server).await["cmd"], 100);
    server
        .outgoing
        .send(r#"{"cmd":10100,"retCode":-1,"retMsg":"denied"}"#.into())
        .unwrap();

    // 거절은 ack 타임아웃(2초)을 기다리지 않고 곧장 실패로 끝난다.
    timeout(Duration::from_millis(1500), connect_task)
        .await
        .expect("rejection waited for timeout")
        .unwrap();
    assert!(!engine.is_connected());
    assert_eq!(engine.session_id(), None);
    engine.disconnect().await;
}

#[tokio::test]
async fn engine_ack_timeout_is_failure() {
    let mut server = spawn_server().await;
    let mut config = engine_config(&server.url);
    config.connect_ack_timeout = 0.3;
    let engine = ChatEngine::new(config);

    let connect_task = {
        let engine = engine.clone();
        tokio::spawn(async move { engine.connect().await })
    };
    assert_eq!(next_frame(&mut server).await["cmd"], 100);
    // 응답하지 않는다.
    timeout(WAIT, connect_task)
        .await
        .expect("ack timeout did not fire")
        .unwrap();
    assert!(!engine.is_connected());
    engine.disconnect().await;
}
