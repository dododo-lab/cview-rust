//! `Sources/CViewApp/Services/MenuTransitionGate.swift` + `MenuScopeEffectCoordinator.swift` 포팅.
//!
//! 사이드바·허브 탭·상세 경로 변경 직전에 초기 렌더 보호 구간을 연다. 만료는 읽을 때
//! 판정하므로 타이머가 없다. Swift 의 전역 `MenuTransitionGate.shared` 는
//! [`MenuTransitionGate::global`] 이고, 테스트는 자기 인스턴스를 만든다.

use std::sync::{Arc, Mutex, OnceLock};

use cview_core::utils::menu_scope_effect_ledger::{
    MenuEffectKey, MenuPhase, MenuScope, MenuScopeEffectLedger,
};
use cview_core::utils::menu_transition_timing::extended_gate_deadline;
use cview_core::utils::perf_tuning;
use cview_core::utils::time::{Date, TimeInterval};

use super::support::{lock, system_clock, Clock};

#[derive(Debug, Default)]
struct GateState {
    transition_deadline: Option<Date>,
    menu_change_epoch: u64,
    ledger: MenuScopeEffectLedger,
}

/// 게이트의 가변 상태와 만료 판정(`MenuTransitionGateCore`).
pub struct MenuTransitionGate {
    now: Clock,
    state: Mutex<GateState>,
}

impl std::fmt::Debug for MenuTransitionGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MenuTransitionGate")
            .field("state", &*lock(&self.state))
            .finish()
    }
}

impl Default for MenuTransitionGate {
    fn default() -> Self {
        Self::new(system_clock())
    }
}

impl MenuTransitionGate {
    pub fn new(now: Clock) -> Self {
        Self {
            now,
            state: Mutex::new(GateState::default()),
        }
    }

    /// 프로덕션 전역 인스턴스.
    pub fn global() -> Arc<MenuTransitionGate> {
        static GLOBAL: OnceLock<Arc<MenuTransitionGate>> = OnceLock::new();
        GLOBAL
            .get_or_init(|| Arc::new(MenuTransitionGate::default()))
            .clone()
    }

    /// 보호 구간 안인가. 만료됐으면 기한을 접는다.
    pub fn is_transitioning(&self) -> bool {
        let now = (self.now)();
        let mut g = lock(&self.state);
        match g.transition_deadline {
            None => false,
            Some(deadline) if now < deadline => true,
            Some(_) => {
                g.transition_deadline = None;
                false
            }
        }
    }

    /// 살아 있는 만료 시각 — 이미 지났으면 None.
    pub fn current_deadline(&self) -> Option<Date> {
        if !self.is_transitioning() {
            return None;
        }
        lock(&self.state).transition_deadline
    }

    pub fn menu_change_epoch(&self) -> u64 {
        lock(&self.state).menu_change_epoch
    }

    /// 사이드바 수명주기 변경 *직전*에 호출. 세대를 올리고 보호 구간을 연다.
    pub fn notify_menu_change(&self) {
        self.notify_menu_change_for(perf_tuning::ui::MENU_GATE_DURATION);
    }

    pub fn notify_menu_change_for(&self, duration: TimeInterval) {
        {
            let mut g = lock(&self.state);
            g.menu_change_epoch = g.menu_change_epoch.wrapping_add(1);
        }
        self.notify_content_change_for(duration);
    }

    /// 화면 안 탭/상세 경로의 초기 렌더만 보호한다.
    pub fn notify_content_change(&self) {
        self.notify_content_change_for(perf_tuning::ui::MENU_GATE_DURATION);
    }

    pub fn notify_content_change_for(&self, duration: TimeInterval) {
        let now = (self.now)();
        let mut g = lock(&self.state);
        g.transition_deadline =
            extended_gate_deadline(&now, g.transition_deadline.as_ref(), duration);
    }

    /// 보호 구간을 즉시 닫는다. 세대는 건드리지 않는다.
    pub fn reset(&self) {
        lock(&self.state).transition_deadline = None;
    }

    /// `MenuScopeEffectCoordinator.admit` — 이번 메뉴 전환에서 이 부수효과를 아직 아무도
    /// 실행하지 않았으면 true.
    pub fn admit(&self, scope: MenuScope, phase: MenuPhase) -> bool {
        let mut g = lock(&self.state);
        let epoch = g.menu_change_epoch;
        g.ledger.admit(MenuEffectKey::new(scope, phase), epoch)
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/MenuTransitionGateCoreTests.swift`
    use super::*;
    use crate::services::support::ManualClock;
    use chrono::TimeZone;

    fn gate() -> (MenuTransitionGate, ManualClock) {
        let clock = ManualClock::new(chrono::Utc.timestamp_opt(1_800_000_000, 0).unwrap());
        (MenuTransitionGate::new(clock.clock()), clock)
    }

    #[test]
    fn opens_and_expires_by_clock() {
        let (g, clock) = gate();
        assert!(!g.is_transitioning());
        g.notify_menu_change_for(0.2);
        assert!(g.is_transitioning());
        assert_eq!(g.menu_change_epoch(), 1);
        clock.advance(0.19);
        assert!(g.is_transitioning());
        clock.advance(0.02);
        assert!(!g.is_transitioning());
        assert!(g.current_deadline().is_none());
    }

    #[test]
    fn content_change_does_not_bump_epoch_and_extends() {
        let (g, clock) = gate();
        g.notify_content_change_for(0.2);
        assert_eq!(g.menu_change_epoch(), 0);
        clock.advance(0.15);
        g.notify_content_change_for(0.2);
        clock.advance(0.1);
        assert!(g.is_transitioning(), "후속 요청이 기한을 늘린다");
    }

    #[test]
    fn short_follow_up_does_not_shorten() {
        let (g, clock) = gate();
        g.notify_content_change_for(0.5);
        g.notify_content_change_for(0.1);
        clock.advance(0.3);
        assert!(g.is_transitioning());
    }

    #[test]
    fn reset_closes_window_but_keeps_epoch() {
        let (g, _) = gate();
        g.notify_menu_change_for(1.0);
        g.reset();
        assert!(!g.is_transitioning());
        assert_eq!(g.menu_change_epoch(), 1);
    }

    #[test]
    fn admit_dedupes_within_epoch() {
        let (g, _) = gate();
        g.notify_menu_change_for(0.1);
        assert!(g.admit(MenuScope::Live, MenuPhase::Leave));
        assert!(!g.admit(MenuScope::Live, MenuPhase::Leave));
        assert!(g.admit(MenuScope::Live, MenuPhase::Restore));
        g.notify_menu_change_for(0.1);
        assert!(g.admit(MenuScope::Live, MenuPhase::Restore));
    }
}
