//! `Sources/CViewApp/DeepLinkInbox.swift` 포팅.
//!
//! primary 인스턴스가 다른(중복 실행된) 인스턴스로부터 forward 된 `cview://` URL 을 받아
//! 라우터로 전달하는 릴레이. macOS `DistributedNotificationCenter` 옵저버는
//! [`crate::services::single_instance_guard`] 의 IPC 가 대신하고, 여기서는 검증·중복 제거·
//! bind 이전 버퍼링만 맡는다.

use std::collections::{HashSet, VecDeque};

use super::deep_link::DeepLink;

/// 라우팅 대상 — scene `.onAppear` 에서 주입되던 클로저.
pub type DeepLinkSink = Box<dyn FnMut(&str) + Send>;

/// 중복 인스턴스로부터 forward 된 deep-link URL 의 수신함.
#[derive(Default)]
pub struct DeepLinkInbox {
    sink: Option<DeepLinkSink>,
    pending: VecDeque<String>,
    seen_nonces: HashSet<String>,
    nonce_order: VecDeque<String>,
}

impl std::fmt::Debug for DeepLinkInbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeepLinkInbox")
            .field("bound", &self.sink.is_some())
            .field("pending", &self.pending)
            .finish()
    }
}

impl DeepLinkInbox {
    pub const PENDING_LIMIT: usize = 32;
    pub const NONCE_LIMIT: usize = 256;

    pub fn new() -> Self {
        Self::default()
    }

    /// forward 된 URL 을 받는다. 형식이 맞으면(같은 nonce 재전송 포함) true — 송신측에 ack 한다.
    pub fn receive(&mut self, url_string: &str, nonce: &str) -> bool {
        if uuid::Uuid::parse_str(nonce).is_err()
            || url_string.len() > 4096
            || DeepLink::parse(url_string).is_none()
        {
            return false;
        }
        if self.seen_nonces.contains(nonce) {
            return true;
        }
        self.seen_nonces.insert(nonce.to_string());
        self.nonce_order.push_back(nonce.to_string());
        if self.nonce_order.len() > Self::NONCE_LIMIT {
            if let Some(old) = self.nonce_order.pop_front() {
                self.seen_nonces.remove(&old);
            }
        }
        self.deliver(url_string);
        true
    }

    /// 라우팅 클로저를 주입하고 버퍼된 URL 을 flush 한다.
    pub fn bind(&mut self, mut sink: DeepLinkSink) {
        for url in self.pending.drain(..) {
            sink(&url);
        }
        self.sink = Some(sink);
    }

    /// URL 전달 — sink 가 있으면 즉시, 없으면 버퍼링(가장 오래된 것부터 밀어낸다).
    pub fn deliver(&mut self, url: &str) {
        if DeepLink::parse(url).is_none() {
            return;
        }
        if let Some(sink) = self.sink.as_mut() {
            sink(url);
        } else {
            if self.pending.len() == Self::PENDING_LIMIT {
                self.pending.pop_front();
            }
            self.pending.push_back(url.to_string());
            tracing::info!(
                "DeepLinkInbox: bind 전 수신 → 버퍼링 ({}건)",
                self.pending.len()
            );
        }
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn validates_deduplicates_and_bounds_cold_start_queue() {
        let mut inbox = DeepLinkInbox::new();
        let nonce = uuid::Uuid::new_v4().to_string();
        assert!(!inbox.receive("https://evil.com", &nonce));
        assert!(!inbox.receive("cview://home", "not-a-nonce"));
        assert!(inbox.receive("cview://home", &nonce));
        assert!(inbox.receive("cview://home", &nonce));
        let received = Arc::new(Mutex::new(Vec::<String>::new()));
        let r = received.clone();
        inbox.bind(Box::new(move |u| r.lock().unwrap().push(u.to_string())));
        assert_eq!(received.lock().unwrap().len(), 1);

        let mut cold = DeepLinkInbox::new();
        for index in 0..100 {
            cold.deliver(&format!("cview://clip?id={index}"));
        }
        let r = received.clone();
        cold.bind(Box::new(move |u| r.lock().unwrap().push(u.to_string())));
        let got = received.lock().unwrap();
        assert_eq!(got.len(), 1 + DeepLinkInbox::PENDING_LIMIT);
        assert!(got.last().unwrap().ends_with("id=99"));
    }
}
