//! `Sources/CViewChat/ChatConstants.swift` 포팅 — CViewChat 매직 넘버 상수.
//!
//! Swift `TimeInterval`(초, `Double`)은 `f64` 초 그대로 둔다. `Duration` 이 필요한 곳은
//! [`secs`] 로 바꾼다.

use std::time::Duration;

/// 초 단위 `f64` → `Duration` (음수·NaN 은 0).
pub fn secs(seconds: f64) -> Duration {
    if seconds.is_finite() && seconds > 0.0 {
        Duration::from_secs_f64(seconds)
    } else {
        Duration::ZERO
    }
}

/// WebSocket 기본값 (Swift `WSDefaults`).
pub mod ws_defaults {
    /// 핑 전송 간격 (초).
    pub const PING_INTERVAL: f64 = 45.0;
    /// 최대 수신 메시지 크기 (512KiB). 최근 채팅(cmd 15101) 50개 묶음이 64KiB 를 넘을 수 있다.
    pub const MAX_MESSAGE_SIZE: usize = 512 * 1024;
    /// WS 요청 타임아웃 (초).
    pub const REQUEST_TIMEOUT: f64 = 90.0;
    /// WS 리소스 타임아웃 (초, 15분).
    pub const RESOURCE_TIMEOUT: f64 = 900.0;
    /// Keep-Alive 헤더 값.
    pub const KEEP_ALIVE_HEADER: &str = "timeout=120, max=200";
    /// HTTP 101 Upgrade 완료 대기 상한 (초).
    pub const OPEN_TIMEOUT: f64 = 10.0;
}

/// 채팅 엔진 기본값 (Swift `ChatDefaults`).
pub mod chat_defaults {
    /// 최대 메시지 버퍼 크기.
    pub const MAX_MESSAGE_BUFFER: usize = 500;
    /// 채팅 엔진 핑 간격 (초).
    pub const PING_INTERVAL: f64 = 45.0;
    /// 디바이스 타입 코드.
    pub const DEVICE_TYPE: i64 = 2001;
    /// 최근 채팅 기본 요청 수.
    pub const RECENT_CHAT_DEFAULT_COUNT: i64 = 50;
    /// 최대 표시 메시지 수 (ViewModel).
    pub const MAX_VISIBLE_MESSAGES: usize = 500;
    /// 기본 채팅 금지 지속 시간 (초, 5분).
    pub const DEFAULT_MUTE_DURATION_SECS: f64 = 300.0;
    /// `/mute` 가 허용하는 최대 채팅 금지 시간 (초, 24시간).
    pub const MAX_MUTE_DURATION_SECS: f64 = 86_400.0;
    /// cmd 100 전송 후 cmd 10100(connected) 응답 대기 상한 (초).
    pub const CONNECT_ACK_TIMEOUT: f64 = 10.0;
    /// 인바운드 watchdog 이 실제 transport ping 주기 뒤에 더 기다리는 여유 시간 (초).
    pub const INBOUND_WATCHDOG_GRACE: f64 = 30.0;
}

/// 재연결 정책 기본값 (Swift `ReconnectDefaults`).
pub mod reconnect_defaults {
    /// 기본 최대 재시도 딜레이 (초).
    pub const DEFAULT_MAX_DELAY: f64 = 30.0;
    /// 기본 최대 재시도 횟수.
    pub const DEFAULT_MAX_ATTEMPTS: i64 = 10;
    /// 기본 지터 팩터.
    pub const DEFAULT_JITTER: f64 = 0.25;
    /// 재시도 횟수 리셋 임계값 (초).
    pub const RESET_THRESHOLD: f64 = 60.0;
    /// 공격적 모드 최대 딜레이 (초).
    pub const AGGRESSIVE_MAX_DELAY: f64 = 10.0;
    /// 공격적 모드 최대 재시도.
    pub const AGGRESSIVE_MAX_ATTEMPTS: i64 = 20;
}

/// 공통 HTTP 헤더 (`Sources/CViewCore/Utilities/CommonHeaders.swift` 중 채팅이 쓰는 값).
pub mod common_headers {
    /// Chrome User-Agent (`WebSocketService` 가 사용).
    pub const CHROME_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36";
    /// Origin 헤더 값 (trailing slash 없음).
    pub const CHZZK_ORIGIN: &str = "https://chzzk.naver.com";
}
