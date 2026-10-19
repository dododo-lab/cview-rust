//! 엔진 이벤트와 이벤트 스트림.
//!
//! Swift 는 `AsyncStream<ChatEngineEvent>(bufferingPolicy: .bufferingNewest(2000))` 를
//! init 에서 만들어 구독 전 이벤트도 보존했다. 같은 의미를 위해 상한이 있는 drop-oldest
//! 큐 + `Notify` 로 구현한다(tokio `broadcast` 는 구독 전 이벤트를 잃고, `mpsc` 는 가장
//! 새로운 것을 버린다).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

use cview_core::models::chat::{ChatConnectionState, ChatMessage};

/// UI 계층으로 내보내는 엔진 이벤트.
#[derive(Debug, Clone, PartialEq)]
// 공지 한 건은 드물고 나머지는 Vec/String 이라 Box 로 API 를 비틀 이유가 없다.
#[allow(clippy::large_enum_variant)]
pub enum ChatEngineEvent {
    Connected,
    Disconnected { reason: String },
    Reconnecting,
    StateChanged(ChatConnectionState),
    NewMessages(Vec<ChatMessage>),
    RecentMessages(Vec<ChatMessage>),
    Donations(Vec<ChatMessage>),
    Notice(ChatMessage),
    NoticeCleared,
    /// 삭제된 메시지 id 와 서버가 알려준 삭제 사유 본문(있을 때).
    MessageBlinded { message_id: String, blind_text: Option<String> },
    Kicked,
    UserPenalized { user_id: String, duration: i64 },
    /// cmd 13101 `retCode != 0` — 서버가 내 메시지를 버렸다. `tid` 가 `None` 이면 아무것도 지우지 말 것.
    SendRejected { tid: Option<i64>, ret_code: i64, message: Option<String> },
    SystemMessage(String),
    MessagesCleared,
}

/// 버퍼 상한 (Swift `.bufferingNewest(2000)`).
pub const EVENT_BUFFER_LIMIT: usize = 2000;

#[derive(Debug, Default)]
struct QueueState {
    items: VecDeque<ChatEngineEvent>,
    closed: bool,
}

#[derive(Debug, Default)]
pub(crate) struct EventQueue {
    state: Mutex<QueueState>,
    notify: Notify,
}

impl EventQueue {
    pub(crate) fn push(&self, event: ChatEngineEvent) {
        {
            let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if s.closed {
                return;
            }
            if s.items.len() >= EVENT_BUFFER_LIMIT {
                s.items.pop_front();
            }
            s.items.push_back(event);
        }
        self.notify.notify_one();
    }

    pub(crate) fn close(&self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).closed = true;
        self.notify.notify_waiters();
        self.notify.notify_one();
    }

    fn try_pop(&self) -> Result<Option<ChatEngineEvent>, ()> {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        match s.items.pop_front() {
            Some(e) => Ok(Some(e)),
            None if s.closed => Err(()),
            None => Ok(None),
        }
    }
}

/// 엔진 이벤트 수신 핸들 (Swift `events()` 가 돌려주던 `AsyncStream`).
///
/// 단일 소비자를 전제로 한다 — 복제본들은 같은 큐를 나눠 읽는다.
#[derive(Debug, Clone)]
pub struct ChatEventStream {
    pub(crate) queue: Arc<EventQueue>,
}

impl ChatEventStream {
    /// 다음 이벤트. 엔진이 해제되고 큐가 비면 `None`.
    pub async fn recv(&self) -> Option<ChatEngineEvent> {
        loop {
            let notified = self.queue.notify.notified();
            match self.queue.try_pop() {
                Ok(Some(event)) => return Some(event),
                Err(()) => return None,
                Ok(None) => notified.await,
            }
        }
    }

    /// 기다리지 않고 꺼낸다.
    pub fn try_recv(&self) -> Option<ChatEngineEvent> {
        self.queue.try_pop().ok().flatten()
    }
}
