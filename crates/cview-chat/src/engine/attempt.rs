//! 엔진의 순수 소유권 상태. 원본: `Sources/CViewChat/ChatEngine.swift` 상단의
//! `ConnectAckAttemptToken`/`ConnectAckAttemptState`/`ReconnectionOwnerToken`/`ReconnectionOwnerState`.
//!
//! 시간·비동기에 의존하지 않는 값 상태라 stale timeout/close 경쟁을 결정적으로 단위 테스트할 수 있다.

/// cmd 100 → 10100 대기 한 번의 식별자.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectAckAttemptToken {
    pub generation: u64,
    pub nonce: u64,
}

/// connect ack 성공/실패/timeout 판정 상태.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ConnectAckAttemptState {
    nonce: u64,
    received: bool,
    failed: bool,
}

impl ConnectAckAttemptState {
    pub fn nonce(&self) -> u64 {
        self.nonce
    }

    pub fn received(&self) -> bool {
        self.received
    }

    pub fn failed(&self) -> bool {
        self.failed
    }

    pub fn begin(&mut self, generation: u64) -> ConnectAckAttemptToken {
        self.nonce = self.nonce.wrapping_add(1);
        self.received = false;
        self.failed = false;
        ConnectAckAttemptToken { generation, nonce: self.nonce }
    }

    /// 첫 terminal 결과일 때만 `true`.
    pub fn mark_received(&mut self) -> bool {
        if self.received || self.failed {
            return false;
        }
        self.received = true;
        true
    }

    /// 첫 terminal 결과일 때만 `true`.
    pub fn mark_failed(&mut self) -> bool {
        if self.received || self.failed {
            return false;
        }
        self.failed = true;
        true
    }

    pub fn accepts_timeout(&self, token: ConnectAckAttemptToken, current_generation: u64) -> bool {
        token.generation == current_generation && token.nonce == self.nonce && !self.received && !self.failed
    }
}

/// 재연결 태스크 소유권 토큰.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReconnectionOwnerToken {
    pub generation: u64,
    pub nonce: u64,
}

/// 재연결 태스크의 `isReconnecting` 소유권. 취소된 구 태스크의 정리 코드가 새 태스크
/// 시작 뒤 실행되더라도 자기 토큰이 아니면 active 상태를 해제하지 못한다.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReconnectionOwnerState {
    nonce: u64,
    active: Option<ReconnectionOwnerToken>,
    pending_owner: Option<ReconnectionOwnerToken>,
    pending_handshake_nonce: Option<u64>,
}

impl ReconnectionOwnerState {
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub fn begin(&mut self, generation: u64) -> ReconnectionOwnerToken {
        self.nonce = self.nonce.wrapping_add(1);
        let token = ReconnectionOwnerToken { generation, nonce: self.nonce };
        self.active = Some(token);
        self.pending_owner = None;
        self.pending_handshake_nonce = None;
        token
    }

    pub fn invalidate(&mut self) {
        self.active = None;
        self.pending_owner = None;
        self.pending_handshake_nonce = None;
    }

    pub fn owns(&self, token: ReconnectionOwnerToken) -> bool {
        self.active == Some(token)
    }

    /// 재연결 진행 중 들어온 close 를 기록한다.
    pub fn request_reconnect(&mut self, handshake_nonce: u64) -> bool {
        let Some(active) = self.active else { return false };
        self.pending_owner = Some(active);
        self.pending_handshake_nonce = Some(handshake_nonce);
        true
    }

    /// 현재 성공한 handshake 와 같은 nonce 의 close 만 소비한다.
    pub fn consume_pending_reconnect(&mut self, owner: ReconnectionOwnerToken, current_handshake_nonce: u64) -> bool {
        if self.pending_owner != Some(owner) {
            return false;
        }
        let matched = self.pending_handshake_nonce == Some(current_handshake_nonce);
        self.pending_owner = None;
        self.pending_handshake_nonce = None;
        matched
    }

    pub fn finish(&mut self, token: ReconnectionOwnerToken) {
        if !self.owns(token) {
            return;
        }
        self.active = None;
        if self.pending_owner == Some(token) {
            self.pending_owner = None;
            self.pending_handshake_nonce = None;
        }
    }
}
