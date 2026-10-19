//! 앱 전반 성능 튜닝 상수의 단일 진입점.
//!
//! 원본: `Sources/CViewCore/Utilities/PerfTuningConstants.swift` (`PerfTuning.<섹션>.<상수>`)
//!
//! 값은 원본과 같다. 각 값의 실측 근거는 원본 주석을 참고한다. (`CGFloat`/`TimeInterval` → `f64`)

/// 멀티라이브.
pub mod multi_live {
    /// BW Smoothing 에서 세션별 최대 비트레이트 호출 사이 분산 간격(ns). 80ms → 20ms.
    pub const ABR_SPREAD_NS: u64 = 20_000_000;
    /// addSession 시 settings 디스크 로드 대기 단위(ns).
    pub const SETTINGS_LOAD_POLL_NS: u64 = 100_000_000;
    /// addSession settings 로드 최대 폴 횟수(1초 캡).
    pub const SETTINGS_LOAD_MAX_RETRIES: u32 = 10;
    /// addSession 내 liveDetail 호출 타임아웃(ns). 5초.
    pub const LIVE_DETAIL_TIMEOUT_NS: u64 = 5_000_000_000;
    /// 시청 탭에서 다음 콜드 전환용 엔진을 예열하기까지의 지연(ms). ⚠ 0 으로 내리지 말 것.
    pub const WATCH_PREWARM_DELAY_MS: u64 = 2_500;
    /// 스테이지 핸드오프 백스톱(ms) — 직전 채널 영상을 깔아 두는 최대 시간.
    pub const STAGE_HANDOFF_TIMEOUT_MS: u64 = 2_000;
}

/// 백그라운드 알림 폴.
pub mod background_poll {
    /// 알림 폴의 live-detail 동시 수 — 첨두 제거가 목적이다. ⚠ 되돌리지 말 것.
    pub const FOLLOWING_DETAIL_CONCURRENCY: usize = 3;
}

/// UI / 메뉴 전환 / 검색.
pub mod ui {
    /// 메뉴 전환 시 글로벌 spring transaction 차단 지속 시간(초).
    pub const MENU_GATE_DURATION: f64 = 0.12;
    /// 메뉴 게이트 활성 동안 데이터 로딩 시작 지연(ms).
    pub const MENU_GATE_DATA_LOAD_DELAY_MS: u64 = 0;
    /// 홈 진입 후 reloadStore 시작 지연(ms).
    pub const HOME_BOOT_RELOAD_DELAY_MS: u64 = 180;
    /// 라이브 콜드 시작 시 홈을 숨긴 채 선마운트하는 지연(ms).
    pub const HOME_RESIDENT_PREWARM_DELAY_MS: u64 = 900;
    /// 상주 라이브 메뉴를 떠난 뒤 로더를 완전 정지하기까지 유예(ms).
    pub const RESIDENT_MENU_FULL_SUSPEND_DELAY_MS: u64 = 2500;
    /// 멀티채팅 작업영역을 떠난 뒤 채팅 소켓을 끊기까지의 따뜻한 유지(ms).
    pub const MULTI_CHAT_WARM_HOLD_MS: u64 = 2500;
    /// 카테고리/글로벌 검색 디바운스(ns).
    pub const SEARCH_DEBOUNCE_NS: u64 = 300_000_000;
    /// 글로벌 자동 spring 의 response(초).
    pub const CONTENT_TRANSITION_RESPONSE: f64 = 0.18;
}

/// 라이브 메뉴(Following hub).
pub mod live_menu {
    /// 히어로 스포트라이트 백드롭 blur 반경.
    pub const HERO_BACKDROP_BLUR_RADIUS: f64 = 10.0;
    /// 히어로 백드롭 썸네일 불투명도.
    pub const HERO_BACKDROP_THUMBNAIL_OPACITY: f64 = 0.40;
    /// hubMode 스테이지 cross-fade(초).
    pub const HUB_MODE_TRANSITION_RESPONSE: f64 = 0.18;
}

/// 네트워킹.
pub mod networking {
    /// 전체 라이브 진행률 콜백 빈도 — N페이지마다 1회.
    pub const PROGRESS_CALLBACK_PAGE_INTERVAL: usize = 5;
}

/// 라이브 진입 / 자식 프로세스 init.
pub mod live_stream {
    /// 초기화 폴링 단위(ns).
    pub const INIT_POLL_NS: u64 = 100_000_000;
    /// 최대 폴 횟수(80 × 100ms = 8초).
    pub const INIT_MAX_RETRIES: u32 = 80;
}
