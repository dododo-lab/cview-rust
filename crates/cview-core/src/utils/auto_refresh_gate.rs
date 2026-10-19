//! 공유 홈 뷰모델 자동 갱신의 **소유권 게이트**(순수 값 타입).
//!
//! 원본: `Sources/CViewCore/Utilities/AutoRefreshGate.swift`
//!
//! 두 축을 합성한다.
//! - 뷰 축: 마운트마다 고유한 요구자 집합(Set) 기반 참조 카운트. 같은 마운트의 중복 retain 은
//!   접히고, 서로 다른 마운트는 접히지 않는다.
//! - 설정 축: 자동 새로고침 설정은 알림 폴과 전수 수집(sweep)만 끈다. 홈·라이브가 떠 있으면
//!   데이터 폴은 유지한다.
//!
//! [AC-7] 게이트는 타이머를 돌리지 않는다. 주기 실행은 `RefreshCoordinator` 가 맡고, 게이트는
//! `is_active`·`is_sweep_active` 판정만 낸다.

use std::collections::HashSet;

use super::refresh_schedule::RefreshDemand;

/// 자동 갱신을 요구하는 화면 종류 — [`RefreshDemand`] 의 별칭(종전 이름).
pub type AutoRefreshScreen = RefreshDemand;

/// 자동 갱신 요구자 — 화면 종류 + **마운트 인스턴스**.
///
/// 원본은 인스턴스 식별자로 `UUID` 를 쓴다. 여기서는 호출부가 마운트마다 고유하게 만든
/// `u64` 를 받는다(같은 마운트의 retain/release 는 같은 값을 쓴다).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AutoRefreshHolder {
    pub screen: AutoRefreshScreen,
    pub instance: u64,
}

impl AutoRefreshHolder {
    pub fn new(screen: AutoRefreshScreen, instance: u64) -> Self {
        Self { screen, instance }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoRefreshGate {
    holders: HashSet<AutoRefreshHolder>,
    is_enabled_by_settings: bool,
}

impl Default for AutoRefreshGate {
    fn default() -> Self {
        Self::new(true)
    }
}

impl AutoRefreshGate {
    pub fn new(is_enabled_by_settings: bool) -> Self {
        Self { holders: HashSet::new(), is_enabled_by_settings }
    }

    /// 현재 자동 갱신을 요구 중인 마운트들.
    pub fn holders(&self) -> &HashSet<AutoRefreshHolder> {
        &self.holders
    }

    /// `설정 › 자동 새로고침` 토글.
    pub fn is_enabled_by_settings(&self) -> bool {
        self.is_enabled_by_settings
    }

    /// 홈·라이브(개요·시청)가 떠 있으면 데이터 폴이 돈다 — 설정과 무관.
    pub fn is_active(&self) -> bool {
        self.holders.iter().any(|h| h.screen.is_following_visible())
    }

    /// 전수 수집을 요구하는가 — 카테고리가 떠 있으면 설정과 무관하게, 홈만이면 설정이 켜져 있을 때만.
    pub fn is_sweep_active(&self) -> bool {
        self.holders.iter().any(|h| h.screen == RefreshDemand::Category)
            || (self.is_enabled_by_settings && self.holders.iter().any(|h| h.screen == RefreshDemand::Home))
    }

    /// `is_active` 가 실제로 바뀌었으면 `true`(= 태스크 시작/정지 필요).
    /// 바뀌지 않았으면 호출부는 아무것도 하지 않아야 한다 — 매번 재시작하면 주기 타이머가 리셋된다.
    pub fn retain(&mut self, holder: AutoRefreshHolder) -> bool {
        let before = self.is_active();
        self.holders.insert(holder);
        self.is_active() != before
    }

    pub fn release(&mut self, holder: AutoRefreshHolder) -> bool {
        let before = self.is_active();
        self.holders.remove(&holder);
        self.is_active() != before
    }

    pub fn set_enabled_by_settings(&mut self, enabled: bool) -> bool {
        let before = self.is_active();
        self.is_enabled_by_settings = enabled;
        self.is_active() != before
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewCoreTests/AutoRefreshGateTests.swift`
    use super::*;

    const HOME_A: AutoRefreshHolder = AutoRefreshHolder { screen: RefreshDemand::Home, instance: 1 };
    const LIVE_A: AutoRefreshHolder = AutoRefreshHolder { screen: RefreshDemand::LIVE, instance: 2 };
    const LIVE_B: AutoRefreshHolder = AutoRefreshHolder { screen: RefreshDemand::LIVE, instance: 3 };
    const CATEGORY_A: AutoRefreshHolder = AutoRefreshHolder { screen: RefreshDemand::Category, instance: 4 };

    #[test]
    fn idle_gate_is_inactive() {
        assert!(!AutoRefreshGate::default().is_active());
    }

    #[test]
    fn shared_holder_survives_late_release() {
        let mut gate = AutoRefreshGate::default();
        gate.retain(HOME_A);
        assert!(gate.is_active());
        gate.retain(LIVE_A);
        gate.release(HOME_A);
        assert!(gate.is_active(), "새 화면(라이브)이 요구 중인데 폴링이 꺼졌다");
        gate.release(LIVE_A);
        assert!(!gate.is_active(), "마지막 화면이 나갔는데도 폴링이 남았다");
    }

    #[test]
    fn concurrent_mounts_are_not_collapsed() {
        let mut gate = AutoRefreshGate::default();
        gate.retain(LIVE_A);
        gate.retain(LIVE_B);
        gate.release(LIVE_B);
        assert!(gate.is_active(), "아직 메인 창이 라이브 화면을 띄우고 있는데 폴링이 멈췄다");
        gate.release(LIVE_A);
        assert!(!gate.is_active());
    }

    #[test]
    fn gate_is_idempotent() {
        let mut gate = AutoRefreshGate::default();
        gate.retain(HOME_A);
        gate.retain(HOME_A);
        gate.release(HOME_A);
        assert!(!gate.is_active());
        gate.release(LIVE_A);
        assert!(!gate.is_active());
    }

    #[test]
    fn reports_only_real_transitions() {
        let mut gate = AutoRefreshGate::default();
        assert!(gate.retain(HOME_A));
        assert!(!gate.retain(LIVE_A));
        assert!(!gate.release(HOME_A));
        assert!(gate.release(LIVE_A));
    }

    #[test]
    fn settings_off_keeps_surface_poll() {
        let mut gate = AutoRefreshGate::default();
        gate.retain(HOME_A);
        assert!(gate.is_active());
        assert!(!gate.set_enabled_by_settings(false));
        assert!(gate.is_active());
        assert!(!gate.is_enabled_by_settings());
        gate.release(HOME_A);
        assert!(!gate.is_active());
    }

    #[test]
    fn settings_off_home_only_keeps_light_but_not_sweep() {
        let mut gate = AutoRefreshGate::new(false);
        gate.retain(HOME_A);
        assert!(gate.is_active());
        assert!(!gate.is_sweep_active());
    }

    #[test]
    fn settings_on_home_keeps_both() {
        let mut gate = AutoRefreshGate::new(true);
        gate.retain(HOME_A);
        assert!(gate.is_active());
        assert!(gate.is_sweep_active());
        gate.set_enabled_by_settings(false);
        assert!(!gate.is_sweep_active());
        assert!(gate.is_active());
    }

    #[test]
    fn category_demands_sweep_regardless_of_settings() {
        let mut gate = AutoRefreshGate::new(false);
        gate.retain(CATEGORY_A);
        assert!(gate.is_sweep_active());
        assert!(!gate.is_active());
        gate.release(CATEGORY_A);
        assert!(!gate.is_sweep_active());
    }

    #[test]
    fn legacy_live_name_maps_to_live_overview() {
        assert_eq!(AutoRefreshScreen::LIVE, RefreshDemand::LiveOverview);
        assert_eq!(LIVE_A.screen, RefreshDemand::LiveOverview);
    }
}
