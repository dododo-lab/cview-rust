//! 앱이 네트워크에서 받은 바이트 수 — 프로세스 전체 누적 계수기.
//!
//! 영상 프록시(세그먼트·재생목록)·API·채팅·이미지가 받은 본문 크기를 더한다. 사용률 표본(cview-app `usage`)이
//! 이 값의 차이로 초당 수신량을 낸다. 운영체제에서 프로세스별 네트워크 양을 가볍게 읽을 길이 없어(macOS·Windows 모두
//! 관리자 권한·비공개 API 가 필요) 앱이 직접 센다 — 웹뷰가 앱을 거치지 않고 직접 받은 양은 빠진다.
//!
//! 덧셈은 `Relaxed` 원자 연산 하나라 받는 쪽 성능에 영향이 없다.

use std::sync::atomic::{AtomicU64, Ordering};

static RECEIVED: AtomicU64 = AtomicU64::new(0);

/// 받은 바이트를 더한다(본문 조각·메시지를 받을 때마다).
#[inline]
pub fn add_received(bytes: usize) {
    RECEIVED.fetch_add(bytes as u64, Ordering::Relaxed);
}

/// 프로세스가 시작된 뒤 받은 바이트 누계.
#[inline]
pub fn received_bytes() -> u64 {
    RECEIVED.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_monotonically() {
        let before = received_bytes();
        add_received(1500);
        add_received(0);
        add_received(24);
        // 다른 테스트가 같은 계수기를 함께 늘릴 수 있어 '적어도'만 본다.
        assert!(received_bytes() >= before + 1524);
    }
}
