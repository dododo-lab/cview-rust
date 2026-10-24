//! 재생 속도 예산 배분.
//!
//! 원본: `Sources/CViewApp/ViewModels/MultiLiveManager+RateBudget.swift`.

use super::ManagerState;
use crate::viewmodels::multi_live::policy::{main_pane_latency_margin, rate_budget_plan};

impl ManagerState {
    /// 저지연 컨트롤러 최대 재생 속도 — 총 +12% 중 선택 세션에 60%.
    /// 같은 때(선택 변경·대역폭 코디네이션 틱) 메인 패인 목표 지연 여유도 패인 수에 맞춘다 — 비선택은 0
    /// (비선택은 따로 안정성 목표 10.5초를 쓴다).
    pub fn apply_rate_budget(&self) {
        let margin = main_pane_latency_margin(self.sessions.len());
        for s in &self.sessions {
            let mine = Some(s.id()) == self.selected_session_id;
            s.player()
                .set_latency_margin(if mine { margin } else { 0.0 });
        }
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
