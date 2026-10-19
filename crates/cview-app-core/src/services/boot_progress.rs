//! `Sources/CViewApp/Services/BootProgress.swift` 포팅 — 시동 진행 상태·스플래시 완료 게이트·
//! 스플래시 시간 상수.
//!
//! 원본의 `@Observable` 싱글톤은 공유 핸들(`shared()`)로 옮겼다. 화면은 값을 읽어 그린다.
//! `BootSignpost` 계측은 생략하고 `tracing` 으로 남긴다.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use cview_core::utils::time::{Date, TimeInterval};

use super::support::{lock, system_clock, system_uptime, Uptime};

/// 시동 단계 — 순서대로 진행된다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BootStep {
    /// SettingsStore 로드.
    Settings = 0,
    /// 데이터 저장소 초기화.
    DataStore = 1,
    /// 채널·클립·이미지 인덱스 캐시 워밍.
    Caches = 2,
    /// 시청 기록 동기화.
    Sync = 3,
    /// 추천·라이브 데이터 1차 로드.
    WarmingUi = 4,
    Complete = 5,
}

impl BootStep {
    pub const ALL: [Self; 6] = [
        Self::Settings,
        Self::DataStore,
        Self::Caches,
        Self::Sync,
        Self::WarmingUi,
        Self::Complete,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Settings => "환경 설정 로드",
            Self::DataStore => "데이터베이스 연결",
            Self::Caches => "채널·이미지 캐시 준비",
            Self::Sync => "시청 기록 동기화",
            Self::WarmingUi => "라이브 데이터 워밍",
            Self::Complete => "완료",
        }
    }

    /// SF Symbol 이름(셸이 자기 아이콘으로 바꿔 쓴다).
    pub fn icon(self) -> &'static str {
        match self {
            Self::Settings => "gear",
            Self::DataStore => "internaldrive.fill",
            Self::Caches => "photo.stack.fill",
            Self::Sync => "arrow.triangle.2.circlepath",
            Self::WarmingUi => "antenna.radiowaves.left.and.right",
            Self::Complete => "checkmark.circle.fill",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BootProgressSnapshot {
    /// 마지막으로 완료된 단계.
    pub current: BootStep,
    /// 0.0~1.0 진행률.
    pub fraction: f64,
    pub started_at: Date,
    pub is_complete: bool,
}

/// 앱 시동 진행 상태.
#[derive(Debug)]
pub struct BootProgress {
    state: Mutex<BootProgressSnapshot>,
}

impl BootProgress {
    pub fn new(started_at: Date) -> Self {
        Self {
            state: Mutex::new(BootProgressSnapshot {
                current: BootStep::Settings,
                fraction: 0.0,
                started_at,
                is_complete: false,
            }),
        }
    }

    pub fn shared() -> &'static BootProgress {
        static SHARED: OnceLock<BootProgress> = OnceLock::new();
        SHARED.get_or_init(|| BootProgress::new(system_clock()()))
    }

    pub fn snapshot(&self) -> BootProgressSnapshot {
        lock(&self.state).clone()
    }

    /// 한 단계 완료. 이미 더 진행된 단계가 끝나 있으면 무시한다(역방향 진행 방지).
    pub fn complete(&self, step: BootStep) {
        let mut st = lock(&self.state);
        if st.is_complete || step < st.current {
            return;
        }
        st.current = step;
        let total = (BootStep::ALL.len() - 1) as f64;
        st.fraction = (step as u8 as f64 / total).min(1.0);
        if step == BootStep::Complete {
            st.is_complete = true;
        }
    }

    /// 강제 완료 — 시동 실패 등 비정상 케이스에서 로딩 화면이 끝나도록.
    pub fn force_complete(&self) {
        let mut st = lock(&self.state);
        st.current = BootStep::Complete;
        st.fraction = 1.0;
        st.is_complete = true;
    }
}

// MARK: - Boot Gate

type Completion = Arc<dyn Fn() + Send + Sync>;

#[derive(Debug, Default, Clone, Copy)]
struct GateFlags {
    settings_resolved: bool,
    home_hydrated: bool,
    is_complete: bool,
}

/// 스플래시 완료 조건 — **설정 확정 + 홈 hydrate** 두 조건이 모두 서야 `BootProgress` 를
/// `Complete` 로 올린다. 설정 로드 실패도 확정이다. 중복 호출은 안전하다.
pub struct BootGate {
    flags: Mutex<GateFlags>,
    started_uptime: TimeInterval,
    uptime: Uptime,
    completion: Option<Completion>,
}

impl BootGate {
    /// `completion` 이 `None` 이면 `BootProgress::shared()` 를 `Complete` 로 올린다(테스트가 바꾼다).
    pub fn new(completion: Option<Completion>) -> Self {
        let uptime = system_uptime();
        Self {
            flags: Mutex::new(GateFlags::default()),
            started_uptime: uptime(),
            uptime,
            completion,
        }
    }

    pub fn shared() -> &'static BootGate {
        static SHARED: OnceLock<BootGate> = OnceLock::new();
        SHARED.get_or_init(|| BootGate::new(None))
    }

    pub fn settings_resolved(&self) -> bool {
        lock(&self.flags).settings_resolved
    }
    pub fn home_hydrated(&self) -> bool {
        lock(&self.flags).home_hydrated
    }
    pub fn is_complete(&self) -> bool {
        lock(&self.flags).is_complete
    }

    pub fn mark_settings_loaded(&self) {
        lock(&self.flags).settings_resolved = true;
        self.complete_if_ready();
    }

    /// 설정 로드 실패 — 실패도 확정이다.
    pub fn mark_settings_failed(&self) {
        lock(&self.flags).settings_resolved = true;
        self.complete_if_ready();
    }

    pub fn mark_home_hydrated(&self) {
        lock(&self.flags).home_hydrated = true;
        self.complete_if_ready();
    }

    fn complete_if_ready(&self) {
        {
            let mut f = lock(&self.flags);
            if !f.settings_resolved || !f.home_hydrated || f.is_complete {
                return;
            }
            f.is_complete = true;
        }
        if let Some(c) = &self.completion {
            c();
            return;
        }
        BootProgress::shared().complete(BootStep::Complete);
        let elapsed = (self.uptime)() - self.started_uptime;
        tracing::info!("boot_gate_complete ms={:.0}", elapsed * 1000.0);
    }
}

// MARK: - Splash Timing

/// 런치 스플래시의 시간 상수 단일 출처.
pub struct SplashTiming;

impl SplashTiming {
    /// 본문 마운트 시작 시점.
    pub const CONTENT_MOUNT_DELAY: Duration = Duration::from_millis(700);
    /// 스플래시 최소 표시 시간. ⚠ `CONTENT_MOUNT_DELAY` 보다 작게 두지 말 것(빈 창이 보인다).
    pub const MINIMUM_DISPLAY: Duration = Duration::from_millis(1_000);
    pub const FADE_OUT: Duration = Duration::from_millis(450);
    /// '동작 줄이기' 가 켜졌을 때의 페이드 아웃(0 이 아니다).
    pub const REDUCED_FADE_OUT: Duration = Duration::from_millis(120);
    /// 부팅이 끝나지 않아도 강제로 걷는 시점.
    pub const WATCHDOG: Duration = Duration::from_secs(8);
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/BootGateTests.swift`(소스 문자열 가드 제외).
    use super::*;
    use cview_core::utils::time::from_unix;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn make_gate() -> (BootGate, Arc<AtomicUsize>) {
        let n = Arc::new(AtomicUsize::new(0));
        let c = n.clone();
        (
            BootGate::new(Some(Arc::new(move || {
                c.fetch_add(1, Ordering::SeqCst);
            }))),
            n,
        )
    }

    #[test]
    fn needs_both_flags() {
        let (g, n) = make_gate();
        g.mark_settings_loaded();
        assert!(g.settings_resolved() && !g.home_hydrated());
        assert!(!g.is_complete() && n.load(Ordering::SeqCst) == 0);
        g.mark_home_hydrated();
        assert!(g.is_complete() && n.load(Ordering::SeqCst) == 1);

        let (g, n) = make_gate();
        g.mark_home_hydrated();
        assert!(!g.is_complete());
        g.mark_settings_loaded();
        assert!(g.is_complete() && n.load(Ordering::SeqCst) == 1);
    }

    #[test]
    fn failed_settings_also_resolve() {
        let (g, n) = make_gate();
        g.mark_settings_failed();
        assert!(g.settings_resolved() && !g.is_complete());
        g.mark_home_hydrated();
        assert!(g.is_complete() && n.load(Ordering::SeqCst) == 1);
    }

    #[test]
    fn duplicate_calls_are_safe() {
        let (g, n) = make_gate();
        g.mark_settings_loaded();
        g.mark_settings_loaded();
        g.mark_home_hydrated();
        g.mark_home_hydrated();
        g.mark_settings_failed();
        assert_eq!(n.load(Ordering::SeqCst), 1);
        assert!(g.is_complete());
        assert_eq!(SplashTiming::WATCHDOG, Duration::from_secs(8));
        assert!(SplashTiming::MINIMUM_DISPLAY >= SplashTiming::CONTENT_MOUNT_DELAY);
    }

    #[test]
    fn progress_is_monotonic() {
        let p = BootProgress::new(from_unix(0.0));
        p.complete(BootStep::Caches);
        assert!((p.snapshot().fraction - 0.4).abs() < 1e-9);
        p.complete(BootStep::Settings);
        assert_eq!(p.snapshot().current, BootStep::Caches);
        p.complete(BootStep::Complete);
        assert!(p.snapshot().is_complete);
        p.complete(BootStep::Sync);
        assert_eq!(p.snapshot().fraction, 1.0);
    }
}
