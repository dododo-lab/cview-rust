//! 정기 갱신 작업의 **간격·게이트·동시 상한**(순수 값 타입).
//!
//! 원본: `Sources/CViewCore/Utilities/RefreshSchedule.swift`
//!
//! 어떤 작업이 몇 초 간격으로 돌아야 하는지(`interval`)와 지금 무엇을 시작할지(`due`)를
//! 한 곳에서 정한다. 시계는 호출자가 넘기는 **uptime** 초다 — 벽시계는 NTP·시간대에 흔들린다.
//! 이 모듈은 타이머도 네트워크도 만들지 않는다.

use std::collections::{HashMap, HashSet};

use super::catalog_sweep::{CATEGORY_INTERVAL, HOME_INTERVAL};
use super::time::TimeInterval;

/// 정기 갱신 작업.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RefreshJobId {
    Following,
    Popular,
    CatalogSweep,
    ClipTrending,
    Insights,
}

impl RefreshJobId {
    /// 선언 순서(Swift `allCases`).
    pub const ALL: [Self; 5] =
        [Self::Following, Self::Popular, Self::CatalogSweep, Self::ClipTrending, Self::Insights];

    /// 우선순위 순서(작을수록 먼저).
    const ORDERED: [Self; 5] =
        [Self::Following, Self::Popular, Self::ClipTrending, Self::CatalogSweep, Self::Insights];

    /// 원본 `rawValue`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Following => "following",
            Self::Popular => "popular",
            Self::CatalogSweep => "catalogSweep",
            Self::ClipTrending => "clipTrending",
            Self::Insights => "insights",
        }
    }

    /// 동시 상한에 세는 네트워크 작업인가. 팔로잉은 항상 허용, 인사이트는 로컬 집계라 세지 않는다.
    fn counts_against_network_limit(self) -> bool {
        matches!(self, Self::Popular | Self::ClipTrending | Self::CatalogSweep)
    }
}

/// 어떤 화면이 데이터를 필요로 하는가. 사이드바 빠른 작업은 수요를 등록하지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RefreshDemand {
    Home,
    /// 라이브 개요(팔로잉 목록 화면).
    LiveOverview,
    /// 라이브 시청 중 독(dock)에 보이는 팔로잉.
    LiveWatch,
    Popular,
    SearchLanding,
    Category,
    Clips,
    Insights,
}

impl RefreshDemand {
    pub const ALL: [Self; 8] = [
        Self::Home,
        Self::LiveOverview,
        Self::LiveWatch,
        Self::Popular,
        Self::SearchLanding,
        Self::Category,
        Self::Clips,
        Self::Insights,
    ];

    /// 라이브(팔로잉) 화면 — 종전 `AutoRefreshScreen.live`. 라이브 개요 수요와 같다.
    pub const LIVE: Self = Self::LiveOverview;

    /// 원본 `rawValue`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::LiveOverview => "liveOverview",
            Self::LiveWatch => "liveWatch",
            Self::Popular => "popular",
            Self::SearchLanding => "searchLanding",
            Self::Category => "category",
            Self::Clips => "clips",
            Self::Insights => "insights",
        }
    }

    /// 팔로잉을 "보이는" 수요로 만드는 화면(가벼운 갱신 수요).
    pub(crate) fn is_following_visible(self) -> bool {
        matches!(self, Self::Home | Self::LiveOverview | Self::LiveWatch)
    }
}

/// 간격 판정 입력.
#[derive(Debug, Clone, PartialEq)]
pub struct RefreshContext {
    pub demands: HashSet<RefreshDemand>,
    /// 창이 가려졌거나 앱이 비활성(화면에 안 보임).
    pub is_suspended: bool,
    pub is_playing: bool,
    pub is_low_power: bool,
    pub on_battery: bool,
    pub is_asleep: bool,
    pub is_offline: bool,
    pub reduce_resource_work: bool,
    pub is_logged_in: bool,
    pub auto_refresh_enabled: bool,
    /// 사용자 설정 간격(초).
    pub auto_refresh_interval: TimeInterval,
    /// 새 라이브·제목 알림을 받는 채널이 있다.
    pub notifications_wanted: bool,
}

impl Default for RefreshContext {
    fn default() -> Self {
        Self {
            demands: HashSet::new(),
            is_suspended: false,
            is_playing: false,
            is_low_power: false,
            on_battery: false,
            is_asleep: false,
            is_offline: false,
            reduce_resource_work: false,
            is_logged_in: true,
            auto_refresh_enabled: true,
            auto_refresh_interval: 180.0,
            notifications_wanted: false,
        }
    }
}

/// 작업별 마지막 시작·실행 중·연속 실패·화면 수요 참조 카운트.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RefreshSchedule {
    last_run: HashMap<RefreshJobId, TimeInterval>,
    running: HashSet<RefreshJobId>,
    failure_streak: HashMap<RefreshJobId, u32>,
    demand_counts: HashMap<RefreshDemand, u32>,
}

impl RefreshSchedule {
    pub const FOLLOWING_VISIBLE_INTERVAL: TimeInterval = 180.0;
    pub const POPULAR_VISIBLE_INTERVAL: TimeInterval = 120.0;
    pub const CLIP_TRENDING_INTERVAL: TimeInterval = 300.0;
    pub const INSIGHTS_INTERVAL: TimeInterval = 30.0;
    /// 동시에 도는 네트워크 작업 상한(팔로잉은 제외).
    pub const NETWORK_CONCURRENCY_LIMIT: usize = 2;
    pub const BACKOFF_BASE: TimeInterval = 30.0;
    pub const BACKOFF_CAP: TimeInterval = 120.0;

    pub fn new() -> Self {
        Self::default()
    }

    /// 작업별 마지막 시작 시각(uptime 초). 없으면 한 번도 안 돌았다.
    pub fn last_run(&self) -> &HashMap<RefreshJobId, TimeInterval> {
        &self.last_run
    }

    pub fn running(&self) -> &HashSet<RefreshJobId> {
        &self.running
    }

    pub fn failure_streak(&self) -> &HashMap<RefreshJobId, u32> {
        &self.failure_streak
    }

    /// 화면이 붙어 있는 수요 집합(카운트 > 0).
    pub fn retained_demands(&self) -> HashSet<RefreshDemand> {
        self.demand_counts.iter().filter(|(_, &c)| c > 0).map(|(&d, _)| d).collect()
    }

    // MARK: 간격 규칙

    fn power_aware(base: TimeInterval, ctx: &RefreshContext) -> TimeInterval {
        if ctx.is_low_power || ctx.reduce_resource_work {
            base * 2.0
        } else {
            base
        }
    }

    /// 작업의 현재 간격(초). `None` = 지금은 돌지 않는다.
    pub fn interval(job: RefreshJobId, ctx: &RefreshContext) -> Option<TimeInterval> {
        if ctx.is_offline && job != RefreshJobId::Insights {
            return None;
        }
        match job {
            RefreshJobId::Following => Self::following_interval(ctx),
            RefreshJobId::Popular => Self::popular_interval(ctx),
            RefreshJobId::CatalogSweep => Self::sweep_interval(ctx),
            RefreshJobId::ClipTrending => Self::gated_interval(Self::CLIP_TRENDING_INTERVAL, RefreshDemand::Clips, ctx),
            RefreshJobId::Insights => Self::gated_interval(Self::INSIGHTS_INTERVAL, RefreshDemand::Insights, ctx),
        }
    }

    fn following_interval(ctx: &RefreshContext) -> Option<TimeInterval> {
        if !ctx.is_logged_in || ctx.is_asleep {
            return None;
        }
        if ctx.is_suspended {
            // 가림: 알림용 백그라운드 폴만 허용한다. 자동 새로고침을 껐으면 알림 폴도 끈다.
            return (ctx.auto_refresh_enabled && ctx.notifications_wanted)
                .then(|| Self::power_aware(ctx.auto_refresh_interval, ctx));
        }
        let visible = ctx.demands.iter().any(|d| d.is_following_visible());
        let visible_interval = Self::FOLLOWING_VISIBLE_INTERVAL;
        if ctx.auto_refresh_enabled {
            let base =
                if visible { ctx.auto_refresh_interval.min(visible_interval) } else { ctx.auto_refresh_interval };
            return Some(Self::power_aware(base, ctx));
        }
        visible.then(|| Self::power_aware(visible_interval, ctx))
    }

    fn popular_interval(ctx: &RefreshContext) -> Option<TimeInterval> {
        if ctx.is_suspended || ctx.is_asleep {
            return None;
        }
        let page = [RefreshDemand::Home, RefreshDemand::Popular, RefreshDemand::SearchLanding];
        page.iter()
            .any(|d| ctx.demands.contains(d))
            .then(|| Self::power_aware(Self::POPULAR_VISIBLE_INTERVAL, ctx))
    }

    fn sweep_interval(ctx: &RefreshContext) -> Option<TimeInterval> {
        if ctx.is_playing || ctx.is_suspended || ctx.is_low_power || ctx.reduce_resource_work || ctx.is_asleep {
            return None;
        }
        if ctx.demands.contains(&RefreshDemand::Category) {
            return Some(CATEGORY_INTERVAL);
        }
        if ctx.demands.contains(&RefreshDemand::Home) && ctx.auto_refresh_enabled {
            return Some(HOME_INTERVAL);
        }
        None
    }

    /// 수요가 있을 때만, 가림·슬립이 아닐 때만 도는 단순 작업(클립·인사이트).
    fn gated_interval(base: TimeInterval, demand: RefreshDemand, ctx: &RefreshContext) -> Option<TimeInterval> {
        if ctx.is_suspended || ctx.is_asleep || !ctx.demands.contains(&demand) {
            return None;
        }
        Some(Self::power_aware(base, ctx))
    }

    // MARK: 상태 전이

    /// 연속 실패 백오프를 적용한 실효 간격. 첫 실패 30초, 이후 두 배씩, 120초 상한, 간격보다 길지 않다.
    pub fn effective_interval(&self, job: RefreshJobId, ctx: &RefreshContext) -> Option<TimeInterval> {
        let base = Self::interval(job, ctx)?;
        let streak = self.failure_streak.get(&job).copied().unwrap_or(0);
        if streak == 0 {
            return Some(base);
        }
        let backoff = Self::BACKOFF_BASE * 2f64.powi((streak - 1).min(10) as i32);
        Some(base.min(backoff.min(Self::BACKOFF_CAP)))
    }

    pub fn mark_started(&mut self, job: RefreshJobId, now: TimeInterval) {
        self.running.insert(job);
        self.last_run.insert(job, now);
    }

    pub fn mark_finished(&mut self, job: RefreshJobId, success: bool) {
        self.running.remove(&job);
        if success {
            self.failure_streak.remove(&job);
        } else {
            let streak = self.failure_streak.entry(job).or_insert(0);
            *streak = streak.saturating_add(1);
        }
    }

    /// 화면 수요를 붙인다. `last_run` 은 바꾸지 않는다 — 화면을 오가는 것만으로 즉시 갱신이 터지면 안 된다.
    pub fn retain(&mut self, demand: RefreshDemand) {
        *self.demand_counts.entry(demand).or_insert(0) += 1;
    }

    /// 짝 없는 release 는 무시한다.
    pub fn release(&mut self, demand: RefreshDemand) {
        let Some(count) = self.demand_counts.get_mut(&demand) else { return };
        if *count <= 1 {
            self.demand_counts.remove(&demand);
        } else {
            *count -= 1;
        }
    }

    // MARK: 판정

    fn time_until_due(&self, job: RefreshJobId, now: TimeInterval, ctx: &RefreshContext) -> Option<TimeInterval> {
        let interval = self.effective_interval(job, ctx)?;
        let Some(&last) = self.last_run.get(&job) else { return Some(0.0) };
        // 호출자가 다른 기준을 섞은 미래 값은 "도래" 로 본다.
        if last > now {
            return Some(0.0);
        }
        Some((last + interval - now).max(0.0))
    }

    /// 지금 시작할 작업(우선순위 순). 이미 도는 작업은 빼고, 네트워크 작업은 동시 상한 2 를 넘기지 않는다.
    pub fn due(&self, now: TimeInterval, ctx: &RefreshContext) -> Vec<RefreshJobId> {
        let busy = self.running.iter().filter(|j| j.counts_against_network_limit()).count();
        let mut slots = Self::NETWORK_CONCURRENCY_LIMIT.saturating_sub(busy);
        let mut result = Vec::new();
        for job in RefreshJobId::ORDERED {
            if self.running.contains(&job) {
                continue;
            }
            match self.time_until_due(job, now, ctx) {
                Some(wait) if wait <= 0.0 => {}
                _ => continue,
            }
            if job.counts_against_network_limit() {
                if slots == 0 {
                    continue;
                }
                slots -= 1;
            }
            result.push(job);
        }
        result
    }

    /// 다음에 깨어날 때까지 남은 초. 돌 작업이 하나도 없으면 `None`.
    /// 상한 때문에 밀린 작업은 세지 않는다 — 실행 중인 작업이 끝나 다시 판정할 때 잡힌다.
    pub fn next_wake(&self, now: TimeInterval, ctx: &RefreshContext) -> Option<TimeInterval> {
        let due_now: HashSet<RefreshJobId> = self.due(now, ctx).into_iter().collect();
        let mut best: Option<TimeInterval> = None;
        for job in RefreshJobId::ALL {
            if self.running.contains(&job) {
                continue;
            }
            let Some(wait) = self.time_until_due(job, now, ctx) else { continue };
            if wait <= 0.0 && !due_now.contains(&job) {
                continue;
            }
            best = Some(best.map_or(wait, |b| b.min(wait)));
        }
        best
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewCoreTests/RefreshScheduleTests.swift`
    use super::*;
    use RefreshDemand as D;
    use RefreshJobId as J;

    #[derive(Clone, Copy)]
    struct Opts {
        suspended: bool,
        playing: bool,
        low_power: bool,
        asleep: bool,
        logged_in: bool,
        auto: bool,
        interval: f64,
        notify: bool,
    }

    impl Default for Opts {
        fn default() -> Self {
            Self {
                suspended: false,
                playing: false,
                low_power: false,
                asleep: false,
                logged_in: true,
                auto: true,
                interval: 600.0,
                notify: false,
            }
        }
    }

    fn ctx_with(demands: &[D], o: Opts) -> RefreshContext {
        RefreshContext {
            demands: demands.iter().copied().collect(),
            is_suspended: o.suspended,
            is_playing: o.playing,
            is_low_power: o.low_power,
            is_asleep: o.asleep,
            is_logged_in: o.logged_in,
            auto_refresh_enabled: o.auto,
            auto_refresh_interval: o.interval,
            notifications_wanted: o.notify,
            ..RefreshContext::default()
        }
    }

    fn ctx(demands: &[D]) -> RefreshContext {
        ctx_with(demands, Opts::default())
    }

    fn logged_out(demands: &[D]) -> RefreshContext {
        ctx_with(demands, Opts { logged_in: false, ..Opts::default() })
    }

    fn iv(job: J, c: &RefreshContext) -> Option<f64> {
        RefreshSchedule::interval(job, c)
    }

    #[test]
    fn following_setting_with_home() {
        assert_eq!(iv(J::Following, &ctx(&[D::Home])), Some(180.0));
    }

    #[test]
    fn following_short_setting() {
        assert_eq!(iv(J::Following, &ctx_with(&[D::Home], Opts { interval: 60.0, ..Opts::default() })), Some(60.0));
    }

    #[test]
    fn following_setting_off() {
        let off = Opts { auto: false, ..Opts::default() };
        assert_eq!(iv(J::Following, &ctx_with(&[D::Home], off)), Some(180.0));
        assert_eq!(iv(J::Following, &ctx_with(&[], off)), None);
    }

    #[test]
    fn following_setting_no_visible_demand() {
        assert_eq!(iv(J::Following, &ctx(&[D::Category])), Some(600.0));
    }

    #[test]
    fn suspended_with_notifications() {
        let c = ctx_with(
            &[D::Home, D::Category, D::Clips, D::Insights, D::Popular],
            Opts { suspended: true, notify: true, ..Opts::default() },
        );
        assert_eq!(iv(J::Following, &c), Some(600.0));
        for job in J::ALL.into_iter().filter(|j| *j != J::Following) {
            assert_eq!(iv(job, &c), None, "{job:?}");
        }
    }

    #[test]
    fn suspended_without_notifications() {
        assert_eq!(iv(J::Following, &ctx_with(&[D::Home], Opts { suspended: true, ..Opts::default() })), None);
        let o = Opts { suspended: true, auto: false, notify: true, ..Opts::default() };
        assert_eq!(iv(J::Following, &ctx_with(&[D::Home], o)), None);
    }

    #[test]
    fn following_logged_out_or_asleep() {
        assert_eq!(iv(J::Following, &logged_out(&[D::Home])), None);
        assert_eq!(iv(J::Following, &ctx_with(&[D::Home], Opts { asleep: true, ..Opts::default() })), None);
    }

    #[test]
    fn popular_intervals() {
        let lp = Opts { low_power: true, ..Opts::default() };
        assert_eq!(iv(J::Popular, &ctx(&[D::Home])), Some(120.0));
        assert_eq!(iv(J::Popular, &ctx(&[D::Popular])), Some(120.0));
        assert_eq!(iv(J::Popular, &ctx(&[D::SearchLanding])), Some(120.0));
        assert_eq!(iv(J::Popular, &ctx(&[])), None);
        assert_eq!(iv(J::Popular, &ctx_with(&[D::Home], lp)), Some(240.0));
        assert_eq!(iv(J::Popular, &ctx_with(&[], lp)), None);
        assert_eq!(iv(J::Popular, &ctx_with(&[D::Home], Opts { suspended: true, ..Opts::default() })), None);
        assert_eq!(iv(J::Popular, &ctx(&[D::Category])), None);
    }

    #[test]
    fn sweep_intervals() {
        let off = Opts { auto: false, ..Opts::default() };
        assert_eq!(iv(J::CatalogSweep, &ctx(&[D::Category])), Some(600.0));
        assert_eq!(iv(J::CatalogSweep, &ctx_with(&[D::Category], off)), Some(600.0));
        assert_eq!(iv(J::CatalogSweep, &ctx(&[D::Home])), Some(1800.0));
        assert_eq!(iv(J::CatalogSweep, &ctx_with(&[D::Home], off)), None);
        assert_eq!(iv(J::CatalogSweep, &ctx(&[D::LiveOverview])), None);
    }

    #[test]
    fn sweep_gates() {
        for o in [
            Opts { playing: true, ..Opts::default() },
            Opts { low_power: true, ..Opts::default() },
            Opts { suspended: true, ..Opts::default() },
        ] {
            assert_eq!(iv(J::CatalogSweep, &ctx_with(&[D::Category], o)), None);
        }
    }

    #[test]
    fn clip_and_insights() {
        let sus = Opts { suspended: true, ..Opts::default() };
        assert_eq!(iv(J::ClipTrending, &ctx(&[D::Clips])), Some(300.0));
        assert_eq!(iv(J::Insights, &ctx(&[D::Insights])), Some(30.0));
        assert_eq!(iv(J::ClipTrending, &ctx(&[])), None);
        assert_eq!(iv(J::Insights, &ctx_with(&[D::Insights], sus)), None);
        assert_eq!(iv(J::ClipTrending, &ctx_with(&[D::Clips], sus)), None);
    }

    #[test]
    fn every_job_has_interval() {
        let all = ctx(&D::ALL);
        for job in J::ALL {
            assert!(iv(job, &all).is_some(), "{job:?}");
        }
    }

    #[test]
    fn offline_keeps_only_insights() {
        let mut c = ctx(&D::ALL);
        c.is_offline = true;
        for job in J::ALL {
            assert_eq!(iv(job, &c).is_some(), job == J::Insights, "{job:?}");
        }
    }

    #[test]
    fn due_ordering_and_immediate() {
        let c = ctx_with(&[D::Home, D::Category, D::Clips], Opts { interval: 180.0, ..Opts::default() });
        assert_eq!(RefreshSchedule::new().due(1000.0, &c), vec![J::Following, J::Popular, J::ClipTrending]);
    }

    #[test]
    fn concurrency_limit() {
        let c = ctx(&[D::Home, D::Category, D::Clips]);
        let mut s = RefreshSchedule::new();
        s.mark_started(J::Popular, 0.0);
        assert_eq!(s.due(0.0, &c), vec![J::Following, J::ClipTrending]);
        s.mark_started(J::ClipTrending, 0.0);
        assert_eq!(s.due(0.0, &c), vec![J::Following]);
        s.mark_finished(J::Popular, true);
        assert_eq!(s.due(0.0, &c), vec![J::Following, J::CatalogSweep]);
    }

    #[test]
    fn due_after_interval() {
        let c = logged_out(&[D::Popular]);
        let mut s = RefreshSchedule::new();
        s.mark_started(J::Popular, 100.0);
        s.mark_finished(J::Popular, true);
        assert!(s.due(219.9, &c).is_empty());
        assert_eq!(s.due(220.0, &c), vec![J::Popular]);
    }

    #[test]
    fn running_not_due_again() {
        let mut s = RefreshSchedule::new();
        s.mark_started(J::Following, 0.0);
        assert!(!s.due(10_000.0, &ctx(&[D::Home])).contains(&J::Following));
    }

    #[test]
    fn retain_release_keeps_last_run() {
        let mut s = RefreshSchedule::new();
        s.mark_started(J::Popular, 50.0);
        s.mark_finished(J::Popular, true);
        let before = s.last_run().clone();
        s.retain(D::Home);
        s.retain(D::Home);
        assert_eq!(s.retained_demands(), HashSet::from([D::Home]));
        s.release(D::Home);
        assert_eq!(s.retained_demands(), HashSet::from([D::Home]));
        s.release(D::Home);
        assert!(s.retained_demands().is_empty());
        s.release(D::Home);
        assert_eq!(s.last_run(), &before);
    }

    #[test]
    fn failure_backoff() {
        let c = logged_out(&[D::Popular]);
        let mut s = RefreshSchedule::new();
        for want in [30.0, 60.0, 120.0, 120.0] {
            s.mark_started(J::Popular, 0.0);
            s.mark_finished(J::Popular, false);
            assert_eq!(s.effective_interval(J::Popular, &c), Some(want));
        }
        let mut insights = RefreshSchedule::new();
        for _ in 0..3 {
            insights.mark_started(J::Insights, 0.0);
            insights.mark_finished(J::Insights, false);
        }
        assert_eq!(insights.effective_interval(J::Insights, &ctx(&[D::Insights])), Some(30.0));
        s.mark_started(J::Popular, 0.0);
        s.mark_finished(J::Popular, true);
        assert_eq!(s.effective_interval(J::Popular, &c), Some(120.0));
    }

    #[test]
    fn backoff_due_timing() {
        let c = logged_out(&[D::Popular]);
        let mut s = RefreshSchedule::new();
        s.mark_started(J::Popular, 0.0);
        s.mark_finished(J::Popular, false);
        assert!(s.due(29.0, &c).is_empty());
        assert_eq!(s.due(30.0, &c), vec![J::Popular]);
    }

    #[test]
    fn backoff_huge_streak() {
        let mut s = RefreshSchedule::new();
        for _ in 0..200 {
            s.mark_started(J::Popular, 0.0);
            s.mark_finished(J::Popular, false);
        }
        assert_eq!(s.effective_interval(J::Popular, &ctx(&[D::Popular])), Some(120.0));
    }

    #[test]
    fn next_wake() {
        let c = logged_out(&[D::Popular, D::Clips]);
        let mut s = RefreshSchedule::new();
        assert_eq!(s.next_wake(0.0, &c), Some(0.0));
        s.mark_started(J::Popular, 0.0);
        s.mark_finished(J::Popular, true);
        s.mark_started(J::ClipTrending, 0.0);
        s.mark_finished(J::ClipTrending, true);
        assert_eq!(s.next_wake(50.0, &c), Some(70.0));
        assert_eq!(s.next_wake(50.0, &logged_out(&[])), None);
    }

    #[test]
    fn next_wake_ignores_capped_jobs() {
        let c = logged_out(&[D::Popular, D::Clips, D::Category]);
        let mut s = RefreshSchedule::new();
        s.mark_started(J::Popular, 0.0);
        s.mark_started(J::ClipTrending, 0.0);
        assert_eq!(s.next_wake(5.0, &c), None);
    }
}
