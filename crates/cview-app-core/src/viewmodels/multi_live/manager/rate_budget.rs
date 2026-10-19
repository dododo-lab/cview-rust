//! 재생 속도 예산 배분.
//!
//! 원본: `Sources/CViewApp/ViewModels/MultiLiveManager+RateBudget.swift`.

use super::ManagerState;
use crate::viewmodels::multi_live::policy::rate_budget_plan;

impl ManagerState {
    /// 저지연 컨트롤러 최대 재생 속도 — 총 +12% 중 선택 세션에 60%.
    pub fn apply_rate_budget(&self) {
        let ids: Vec<_> = self.sessions.iter().map(|s| s.id()).collect();
        match rate_budget_plan(&ids, self.selected_session_id) {
            None => {
                if let Some(s) = self.sessions.first() {
                    s.player().set_max_rate_override(None);
                }
            }
            Some(plan) => {
                for (id, rate) in plan {
                    if let Some(s) = self.session(id) {
                        s.player().set_max_rate_override(Some(rate));
                    }
                }
            }
        }
    }
}
