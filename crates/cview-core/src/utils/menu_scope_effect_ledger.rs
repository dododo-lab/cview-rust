//! 메뉴 이탈/복귀 부수효과의 **중복 실행 차단** 장부(순수 값 타입).
//!
//! 원본: `Sources/CViewCore/Utilities/MenuScopeEffectLedger.swift`
//!
//! 같은 메뉴 전환(epoch) 안에서 먼저 도착한 경로만 통과시킨다. 판정 기준은
//! "(scope 의) 마지막 통과가 **같은 epoch 의 같은 phase** 인가" 다 — phase 가 번갈아 오면
//! (라우트를 바꾸지 않는 씬 여닫기) epoch 이 같아도 통과시켜 suspend/resume 대칭을 지킨다.

use std::collections::HashMap;

/// 부수효과가 적용되는 스코프 — 각각 별도의 세션 매니저를 소유한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MenuScope {
    /// 라이브(팔로잉) 메뉴
    Live,
    /// 멀티채팅 메뉴
    MultiChat,
}

/// 전환의 어느 쪽인가.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MenuPhase {
    /// 이탈 — 표면 해제 · 채팅 정지
    Leave,
    /// 복귀 — 표면 재승격 · 채팅 재연결
    Restore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MenuEffectKey {
    pub scope: MenuScope,
    pub phase: MenuPhase,
}

impl MenuEffectKey {
    pub const fn new(scope: MenuScope, phase: MenuPhase) -> Self {
        Self { scope, phase }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MenuScopeEffectLedger {
    /// 스코프별 마지막 통과 기록 — (epoch, phase).
    last_admitted: HashMap<MenuScope, (u64, MenuPhase)>,
}

impl MenuScopeEffectLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// 같은 전환(`epoch`)에서 **같은 phase** 가 두 번 오면 두 번째를 막는다.
    /// 부수효과를 실행해야 하면 `true`.
    pub fn admit(&mut self, key: MenuEffectKey, epoch: u64) -> bool {
        if self.last_admitted.get(&key.scope) == Some(&(epoch, key.phase)) {
            return false;
        }
        self.last_admitted.insert(key.scope, (epoch, key.phase));
        true
    }

    /// 테스트/재초기화용.
    pub fn reset(&mut self) {
        self.last_admitted.clear();
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewCoreTests/MenuScopeEffectLedgerTests.swift`
    use super::*;
    use MenuPhase::*;
    use MenuScope::*;

    fn key(s: MenuScope, p: MenuPhase) -> MenuEffectKey {
        MenuEffectKey::new(s, p)
    }

    #[test]
    fn second_path_in_same_epoch_is_rejected() {
        let mut l = MenuScopeEffectLedger::new();
        assert!(l.admit(key(Live, Restore), 7));
        assert!(!l.admit(key(Live, Restore), 7));
    }

    #[test]
    fn different_phase_is_independent() {
        let mut l = MenuScopeEffectLedger::new();
        assert!(l.admit(key(Live, Restore), 3));
        assert!(l.admit(key(Live, Leave), 3));
    }

    #[test]
    fn different_scope_is_independent() {
        let mut l = MenuScopeEffectLedger::new();
        assert!(l.admit(key(Live, Leave), 3));
        assert!(l.admit(key(MultiChat, Leave), 3));
    }

    #[test]
    fn next_epoch_admits_again() {
        let mut l = MenuScopeEffectLedger::new();
        assert!(l.admit(key(Live, Leave), 3));
        assert!(!l.admit(key(Live, Leave), 3));
        assert!(l.admit(key(Live, Leave), 4));
    }

    #[test]
    fn alternating_phases_in_same_epoch_keep_admitting() {
        let mut l = MenuScopeEffectLedger::new();
        assert!(l.admit(key(Live, Restore), 11));
        assert!(l.admit(key(Live, Leave), 11));
        assert!(l.admit(key(Live, Restore), 11));
        assert!(l.admit(key(Live, Leave), 11));
    }

    #[test]
    fn same_epoch_same_phase_stays_blocked_across_scopes() {
        let mut l = MenuScopeEffectLedger::new();
        assert!(l.admit(key(Live, Leave), 20));
        assert!(!l.admit(key(Live, Leave), 20));
        assert!(l.admit(key(MultiChat, Restore), 20));
        assert!(!l.admit(key(Live, Leave), 20));
    }

    #[test]
    fn fresh_ledger_admits() {
        let mut l = MenuScopeEffectLedger::new();
        assert!(l.admit(key(Live, Restore), 0));
        l.reset();
        assert!(l.admit(key(Live, Restore), 0));
    }
}
