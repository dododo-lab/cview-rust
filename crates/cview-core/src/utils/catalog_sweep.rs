//! 카탈로그 전수 수집(sweep)의 **실행 여부 결정**(순수 값 타입).
//!
//! 원본: `Sources/CViewCore/Utilities/CatalogSweepPolicy.swift`
//!
//! 결정 순서(앞이 이기면 뒤는 보지 않는다):
//! 1. 재생 중 → idle(playing)
//! 2. 가림 → idle(suspended)
//! 3. 수요 없음 → idle(noDemand) (liveWatch · liveOverview 는 수요가 아니다)
//! 4. 부팅 유예(20초) → wait(until) (카테고리 화면이 비어 있으면 예외)
//! 5. 이어받기 체크포인트 → resume
//! 6. 재시도 쿨다운 → wait(until) (transient 90초 · ordering 600초)
//! 7. 간격 → run / wait(until) (배터리 ×1.5, 저전력은 900초 하한 + 요약이 신선하면 idle)

use std::collections::HashSet;

use super::refresh_schedule::RefreshDemand;
use super::time::{adding, interval_since, Date, TimeInterval};

/// 카테고리 화면이 떠 있을 때 수집 간격(초).
pub const CATEGORY_INTERVAL: TimeInterval = 600.0;
/// 홈만 떠 있을 때 수집 간격(초).
pub const HOME_INTERVAL: TimeInterval = 1800.0;
/// 부팅 게이트가 끝난 뒤 수집을 미루는 시간(초).
pub const BOOT_GRACE: TimeInterval = 20.0;
/// 체크포인트 갱신이 이 안(초)이면 이어받는다.
pub const RESUME_WINDOW: TimeInterval = 300.0;
/// 한 수집 세대의 이어받기 수명(초).
pub const RESUME_GENERATION_LIMIT: TimeInterval = 600.0;
pub const TRANSIENT_RETRY: TimeInterval = 90.0;
pub const ORDERING_RETRY: TimeInterval = 600.0;
/// 요약이 이보다 오래되면 저전력에서도 갱신한다(초, 1일).
pub const SUMMARY_MAX_AGE: TimeInterval = 86_400.0;
/// 이어받기를 포기하는 중단 횟수.
pub const PAUSE_LIMIT: u32 = 3;
/// 저전력 모드에서의 간격 하한(초).
pub const LOW_POWER_INTERVAL: TimeInterval = 900.0;
/// 수동 새로고침을 건너뛰는 창(초).
pub const MANUAL_SKIP_WINDOW: TimeInterval = 60.0;
/// 배터리 구동 간격 배수.
pub const BATTERY_MULTIPLIER: f64 = 1.5;

/// 불완전 수집 뒤 자동 재시도 분류.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CatalogRetryClass {
    /// 네트워크·타임아웃 등 일시 오류 — 짧게(90초) 다시 시도한다.
    Transient,
    /// 서버 정렬 drift 로 페이지가 어긋남 — 길게(600초) 쉰다.
    Ordering,
}

/// 마지막 수집 시도(불완전·실패 포함).
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogSweepAttempt {
    pub at: Date,
    pub retry_class: CatalogRetryClass,
}

/// 디스크에 남은 진행 체크포인트(이어받기 후보).
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogSweepCheckpoint {
    /// 마지막으로 페이지를 쌓은 시각.
    pub updated_at: Date,
    /// 이번 수집 세대의 시작 시각.
    pub started_at: Date,
    /// 중단(일시정지)된 횟수.
    pub pause_count: u32,
    /// 체크포인트를 만든 앱 빌드 — 다른 빌드의 부분 결과는 이어받지 않는다.
    pub app_build: String,
    /// 쌓인 행 수.
    pub row_count: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogResumeDecision {
    Resume,
    Discard(CatalogResumeDiscardReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogResumeDiscardReason {
    /// 마지막 갱신이 300초보다 오래됐거나 시계가 거꾸로 갔다.
    Stale,
    /// 세대 시작이 600초보다 오래됐다.
    TooOld,
    TooManyPauses,
    BuildChanged,
    NoRows,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CatalogSweepInputs {
    pub now: Date,
    pub demands: HashSet<RefreshDemand>,
    /// 마지막으로 요약을 승격(저장)한 시각. `None` = 한 번도 없음.
    pub last_promoted_at: Option<Date>,
    pub last_attempt: Option<CatalogSweepAttempt>,
    pub checkpoint: Option<CatalogSweepCheckpoint>,
    pub app_build: String,
    pub is_playing: bool,
    pub is_suspended: bool,
    pub is_low_power: bool,
    pub on_battery: bool,
    /// 부팅 게이트가 끝난 시각. `None` = 아직 안 끝남.
    pub boot_settled_at: Option<Date>,
    /// 화면에 그릴 카탈로그 요약이 있는가.
    pub has_summary: bool,
    pub auto_refresh_enabled: bool,
}

impl CatalogSweepInputs {
    /// 원본 이니셜라이저의 기본값(요약 있음, 자동 새로고침 켬, 나머지 없음/거짓).
    pub fn new(now: Date, demands: HashSet<RefreshDemand>) -> Self {
        Self {
            now,
            demands,
            last_promoted_at: None,
            last_attempt: None,
            checkpoint: None,
            app_build: String::new(),
            is_playing: false,
            is_suspended: false,
            is_low_power: false,
            on_battery: false,
            boot_settled_at: None,
            has_summary: true,
            auto_refresh_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogSweepRunReason {
    /// 요약이 없거나 승격 기록이 없다 — 첫 채움.
    FirstFill,
    /// 간격이 지났다.
    Interval,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogSweepIdleReason {
    Offline,
    ResourcePressure,
    Playing,
    Suspended,
    NoDemand,
    /// 홈 수요만 있는데 자동 새로고침이 꺼져 있다.
    AutoRefreshOff,
    /// 부팅 게이트가 아직 끝나지 않았다.
    BootUnsettled,
    /// 저전력이고 요약이 아직 신선하다.
    LowPower,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CatalogSweepDecision {
    Run(CatalogSweepRunReason),
    Resume,
    Wait { until: Date },
    Idle(CatalogSweepIdleReason),
}

/// 수집 수요로 치는 화면(카테고리·홈) — 나머지는 수요가 아니다.
pub fn sweep_demands(demands: &HashSet<RefreshDemand>) -> HashSet<RefreshDemand> {
    demands.iter().copied().filter(|d| matches!(d, RefreshDemand::Category | RefreshDemand::Home)).collect()
}

pub fn retry_cooldown(retry_class: CatalogRetryClass) -> TimeInterval {
    match retry_class {
        CatalogRetryClass::Transient => TRANSIENT_RETRY,
        CatalogRetryClass::Ordering => ORDERING_RETRY,
    }
}

/// 수요 기준 기본 간격. 카테고리가 있으면 600, 홈만이면 1800. 수요가 없으면 `None`.
pub fn base_interval(demands: &HashSet<RefreshDemand>) -> Option<TimeInterval> {
    if demands.contains(&RefreshDemand::Category) {
        Some(CATEGORY_INTERVAL)
    } else if demands.contains(&RefreshDemand::Home) {
        Some(HOME_INTERVAL)
    } else {
        None
    }
}

pub fn resume_decision(checkpoint: &CatalogSweepCheckpoint, now: &Date, app_build: &str) -> CatalogResumeDecision {
    use CatalogResumeDiscardReason::*;
    let since_update = interval_since(now, &checkpoint.updated_at);
    if !(0.0..=RESUME_WINDOW).contains(&since_update) {
        return CatalogResumeDecision::Discard(Stale);
    }
    if interval_since(now, &checkpoint.started_at) > RESUME_GENERATION_LIMIT {
        return CatalogResumeDecision::Discard(TooOld);
    }
    if checkpoint.pause_count >= PAUSE_LIMIT {
        return CatalogResumeDecision::Discard(TooManyPauses);
    }
    if checkpoint.app_build != app_build {
        return CatalogResumeDecision::Discard(BuildChanged);
    }
    if checkpoint.row_count <= 0 {
        return CatalogResumeDecision::Discard(NoRows);
    }
    CatalogResumeDecision::Resume
}

pub fn decide(input: &CatalogSweepInputs) -> CatalogSweepDecision {
    if let Some(idle) = gate_idle(input) {
        return CatalogSweepDecision::Idle(idle);
    }
    let category_demanded = input.demands.contains(&RefreshDemand::Category);
    // 부팅 유예 — 카테고리 화면이 비어 있으면(요약 없음) 기다리게 하지 않는다.
    if !(category_demanded && !input.has_summary) {
        let Some(settled) = input.boot_settled_at else {
            return CatalogSweepDecision::Idle(CatalogSweepIdleReason::BootUnsettled);
        };
        let grace_end = adding(&settled, BOOT_GRACE);
        if input.now < grace_end {
            return CatalogSweepDecision::Wait { until: grace_end };
        }
    }
    if let Some(cp) = &input.checkpoint {
        if resume_decision(cp, &input.now, &input.app_build) == CatalogResumeDecision::Resume {
            return CatalogSweepDecision::Resume;
        }
    }
    if let Some(attempt) = &input.last_attempt {
        let retry_at = adding(&attempt.at, retry_cooldown(attempt.retry_class));
        if input.now < retry_at {
            return CatalogSweepDecision::Wait { until: retry_at };
        }
    }
    decide_by_interval(input)
}

/// 재생 → 가림 → 수요 없음 → (홈만) 자동 새로고침 끔.
fn gate_idle(input: &CatalogSweepInputs) -> Option<CatalogSweepIdleReason> {
    use CatalogSweepIdleReason::*;
    if input.is_playing {
        return Some(Playing);
    }
    if input.is_suspended {
        return Some(Suspended);
    }
    let relevant = sweep_demands(&input.demands);
    if relevant.is_empty() {
        return Some(NoDemand);
    }
    if !relevant.contains(&RefreshDemand::Category) && !input.auto_refresh_enabled {
        return Some(AutoRefreshOff);
    }
    None
}

fn decide_by_interval(input: &CatalogSweepInputs) -> CatalogSweepDecision {
    let promoted = match (&input.last_promoted_at, input.has_summary) {
        (Some(p), true) => p,
        _ => return CatalogSweepDecision::Run(CatalogSweepRunReason::FirstFill),
    };
    let age = interval_since(&input.now, promoted);
    // 시계가 거꾸로 간 기록은 "도래" 로 본다.
    let stale = age < 0.0 || age > SUMMARY_MAX_AGE;
    if input.is_low_power && !stale {
        return CatalogSweepDecision::Idle(CatalogSweepIdleReason::LowPower);
    }
    let mut interval = base_interval(&input.demands).unwrap_or(HOME_INTERVAL);
    if input.on_battery {
        interval *= BATTERY_MULTIPLIER;
    }
    if input.is_low_power {
        interval = interval.max(LOW_POWER_INTERVAL);
    }
    if age < 0.0 || age >= interval {
        CatalogSweepDecision::Run(CatalogSweepRunReason::Interval)
    } else {
        CatalogSweepDecision::Wait { until: adding(promoted, interval) }
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewCoreTests/CatalogSweepPolicyTests.swift`
    use super::*;
    use crate::utils::time::from_unix;
    use CatalogSweepDecision as Dec;
    use CatalogSweepIdleReason as Idle;
    use CatalogSweepRunReason as Run;
    use RefreshDemand as D;

    fn t0() -> Date {
        from_unix(1_700_000_000.0)
    }

    fn at(offset: f64) -> Date {
        adding(&t0(), offset)
    }

    fn wait(offset: f64) -> Dec {
        Dec::Wait { until: at(offset) }
    }

    /// 부팅 유예가 지난 기본 입력(요약 있음, 방금 승격).
    fn inputs_at(demands: &[D], now: Date, promoted_ago: Option<f64>, has_summary: bool) -> CatalogSweepInputs {
        let mut v = CatalogSweepInputs::new(now, demands.iter().copied().collect());
        v.last_promoted_at = promoted_ago.map(|a| adding(&now, -a));
        v.app_build = "100".into();
        v.boot_settled_at = Some(adding(&now, -100.0));
        v.has_summary = has_summary;
        v
    }

    fn inputs(demands: &[D], promoted_ago: Option<f64>) -> CatalogSweepInputs {
        inputs_at(demands, t0(), promoted_ago, true)
    }

    fn with(mut v: CatalogSweepInputs, f: impl FnOnce(&mut CatalogSweepInputs)) -> CatalogSweepInputs {
        f(&mut v);
        v
    }

    #[test]
    fn constants() {
        assert_eq!(CATEGORY_INTERVAL, 600.0);
        assert_eq!(HOME_INTERVAL, 1800.0);
        assert_eq!(BOOT_GRACE, 20.0);
        assert_eq!(RESUME_WINDOW, 300.0);
        assert_eq!(TRANSIENT_RETRY, 90.0);
        assert_eq!(ORDERING_RETRY, 600.0);
        assert_eq!(SUMMARY_MAX_AGE, 86_400.0);
        assert_eq!(PAUSE_LIMIT, 3);
        assert_eq!(retry_cooldown(CatalogRetryClass::Transient), 90.0);
        assert_eq!(retry_cooldown(CatalogRetryClass::Ordering), 600.0);
    }

    #[test]
    fn gate_order() {
        let playing = with(inputs(&[D::Category], Some(0.0)), |v| {
            v.is_playing = true;
            v.is_suspended = true;
        });
        assert_eq!(decide(&playing), Dec::Idle(Idle::Playing));
        let suspended = with(inputs(&[D::Category], Some(0.0)), |v| v.is_suspended = true);
        assert_eq!(decide(&suspended), Dec::Idle(Idle::Suspended));
        assert_eq!(decide(&inputs(&[], Some(0.0))), Dec::Idle(Idle::NoDemand));
    }

    #[test]
    fn non_demands() {
        for d in [D::LiveWatch, D::LiveOverview, D::Popular, D::SearchLanding, D::Clips, D::Insights] {
            assert_eq!(decide(&inputs(&[d], Some(100_000.0))), Dec::Idle(Idle::NoDemand));
        }
        assert_eq!(decide(&inputs(&[D::LiveOverview, D::LiveWatch], Some(100_000.0))), Dec::Idle(Idle::NoDemand));
    }

    #[test]
    fn auto_refresh_off() {
        let home = with(inputs(&[D::Home], Some(5000.0)), |v| v.auto_refresh_enabled = false);
        assert_eq!(decide(&home), Dec::Idle(Idle::AutoRefreshOff));
        let cat = with(inputs(&[D::Category], Some(5000.0)), |v| v.auto_refresh_enabled = false);
        assert_eq!(decide(&cat), Dec::Run(Run::Interval));
    }

    #[test]
    fn boot_grace() {
        let settled = at(-5.0);
        let waiting = with(inputs(&[D::Home], Some(5000.0)), |v| v.boot_settled_at = Some(settled));
        assert_eq!(decide(&waiting), Dec::Wait { until: adding(&settled, 20.0) });
        let after = with(inputs(&[D::Home], Some(5000.0)), |v| v.boot_settled_at = Some(at(-20.0)));
        assert_eq!(decide(&after), Dec::Run(Run::Interval));
    }

    #[test]
    fn boot_exception_for_empty_category() {
        let unsettled = with(inputs(&[D::Home], Some(0.0)), |v| v.boot_settled_at = None);
        assert_eq!(decide(&unsettled), Dec::Idle(Idle::BootUnsettled));
        let empty = with(inputs_at(&[D::Category], t0(), None, false), |v| v.boot_settled_at = None);
        assert_eq!(decide(&empty), Dec::Run(Run::FirstFill));
        let filled = with(inputs(&[D::Category], Some(5000.0)), |v| v.boot_settled_at = Some(at(-1.0)));
        assert_eq!(decide(&filled), wait(19.0));
    }

    fn checkpoint(updated_ago: f64, started_ago: f64, pauses: u32, build: &str, rows: i64) -> CatalogSweepCheckpoint {
        CatalogSweepCheckpoint {
            updated_at: at(-updated_ago),
            started_at: at(-started_ago),
            pause_count: pauses,
            app_build: build.into(),
            row_count: rows,
        }
    }

    fn cp() -> CatalogSweepCheckpoint {
        checkpoint(10.0, 100.0, 0, "100", 500)
    }

    #[test]
    fn resume_boundaries() {
        use CatalogResumeDecision::*;
        use CatalogResumeDiscardReason::*;
        let d = |c: CatalogSweepCheckpoint| resume_decision(&c, &t0(), "100");
        assert_eq!(d(cp()), Resume);
        assert_eq!(d(checkpoint(0.0, 100.0, 0, "100", 500)), Resume);
        assert_eq!(d(checkpoint(300.0, 300.0, 0, "100", 500)), Resume);
        assert_eq!(d(checkpoint(301.0, 302.0, 0, "100", 500)), Discard(Stale));
        assert_eq!(d(checkpoint(-1.0, 100.0, 0, "100", 500)), Discard(Stale));
        assert_eq!(d(checkpoint(10.0, 600.0, 0, "100", 500)), Resume);
        assert_eq!(d(checkpoint(10.0, 601.0, 0, "100", 500)), Discard(TooOld));
        assert_eq!(d(checkpoint(10.0, 100.0, 2, "100", 500)), Resume);
        assert_eq!(d(checkpoint(10.0, 100.0, 3, "100", 500)), Discard(TooManyPauses));
        assert_eq!(d(checkpoint(10.0, 100.0, 0, "99", 500)), Discard(BuildChanged));
        assert_eq!(d(checkpoint(10.0, 100.0, 0, "100", 0)), Discard(NoRows));
    }

    #[test]
    fn checkpoint_in_decide() {
        let resumed = with(inputs(&[D::Category], Some(1.0)), |v| v.checkpoint = Some(cp()));
        assert_eq!(decide(&resumed), Dec::Resume);
        let ignored =
            with(inputs(&[D::Category], Some(1.0)), |v| v.checkpoint = Some(checkpoint(10.0, 100.0, 3, "100", 500)));
        assert_eq!(decide(&ignored), wait(599.0));
    }

    #[test]
    fn retry_cooldowns() {
        let d = |ago: f64, cls: CatalogRetryClass| {
            decide(&with(inputs(&[D::Category], Some(5000.0)), |v| {
                v.last_attempt = Some(CatalogSweepAttempt { at: at(-ago), retry_class: cls })
            }))
        };
        use CatalogRetryClass::*;
        assert_eq!(d(0.0, Transient), wait(90.0));
        assert_eq!(d(60.0, Transient), wait(30.0));
        assert_eq!(d(89.0, Transient), wait(1.0));
        assert_eq!(d(90.0, Transient), Dec::Run(Run::Interval));
        assert_eq!(d(599.0, Ordering), wait(1.0));
        assert_eq!(d(600.0, Ordering), Dec::Run(Run::Interval));
    }

    #[test]
    fn interval_boundaries() {
        assert_eq!(decide(&inputs(&[D::Category], Some(599.0))), wait(1.0));
        assert_eq!(decide(&inputs(&[D::Category], Some(600.0))), Dec::Run(Run::Interval));
        assert_eq!(decide(&inputs(&[D::Home], Some(1799.0))), wait(1.0));
        assert_eq!(decide(&inputs(&[D::Home], Some(1800.0))), Dec::Run(Run::Interval));
        assert_eq!(decide(&inputs(&[D::Home, D::Category], Some(600.0))), Dec::Run(Run::Interval));
    }

    #[test]
    fn battery_multiplier() {
        let bat = |d: D, ago: f64| decide(&with(inputs(&[d], Some(ago)), |v| v.on_battery = true));
        assert_eq!(bat(D::Category, 600.0), wait(300.0));
        assert_eq!(bat(D::Category, 900.0), Dec::Run(Run::Interval));
        assert_eq!(bat(D::Home, 2699.0), wait(1.0));
    }

    #[test]
    fn low_power() {
        let lp = |ago: f64| decide(&with(inputs(&[D::Category], Some(ago)), |v| v.is_low_power = true));
        assert_eq!(lp(5000.0), Dec::Idle(Idle::LowPower));
        assert_eq!(lp(86_400.0), Dec::Idle(Idle::LowPower));
        assert_eq!(lp(86_401.0), Dec::Run(Run::Interval));
    }

    #[test]
    fn first_fill_and_clock_skew() {
        assert_eq!(decide(&inputs(&[D::Home], None)), Dec::Run(Run::FirstFill));
        assert_eq!(decide(&inputs_at(&[D::Home], t0(), Some(5.0), false)), Dec::Run(Run::FirstFill));
        assert_eq!(decide(&inputs(&[D::Home], Some(-50.0))), Dec::Run(Run::Interval));
    }

    /// 60초 걸음으로 60분을 돌려 실행 횟수를 센다. 실행하면 승격 시각이 지금이 된다.
    fn simulate(demands: &[D], configure: impl Fn(&mut CatalogSweepInputs)) -> usize {
        let mut runs = 0;
        let mut promoted = at(-600.0);
        for step in 0..60 {
            let now = at(f64::from(step * 60));
            let mut v = inputs_at(demands, now, None, true);
            configure(&mut v);
            v.last_promoted_at = Some(promoted);
            if matches!(decide(&v), Dec::Run(_) | Dec::Resume) {
                runs += 1;
                promoted = now;
            }
        }
        runs
    }

    #[test]
    fn hour_category() {
        assert_eq!(simulate(&[D::Category], |_| {}), 6);
    }

    #[test]
    fn hour_home_only() {
        assert!(simulate(&[D::Home], |_| {}) <= 2);
    }

    #[test]
    fn hour_gated_zero() {
        assert_eq!(simulate(&[D::Category], |v| v.is_playing = true), 0);
        assert_eq!(simulate(&[D::Category], |v| v.is_suspended = true), 0);
        assert_eq!(simulate(&[D::LiveOverview], |_| {}), 0);
        assert_eq!(simulate(&[D::LiveOverview, D::LiveWatch], |_| {}), 0);
    }
}
