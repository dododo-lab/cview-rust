//! 재생 속도 예산 배분.
//!
//! 원본: `Sources/CViewApp/ViewModels/MultiLiveManager+RateBudget.swift`.

use super::ManagerState;
use crate::viewmodels::multi_live::policy::{main_pane_latency_margin, rate_budget_plan};

impl ManagerState {
    /// 저지연 컨트롤러 최대 재생 속도 — 총 +12% 중 선택 세션에 60%.
    /// 같은 때(선택 변경·대역폭 코디네이션 틱) 메인 패인 목표 지연 여유도 패인 수에 맞춘다 — 비선택은 0
    /// (비선택은 따로 안정성 목표 10.5초를 쓴다).
    /// 상한을 넘어 일부만 보이면 보이는 패인만 센다 — 숨은 세션은 재생을 멈췄으니 예산을 나누지 않는다.
    pub fn apply_rate_budget(&self) {
        let view = self.multi_view_session_ids();
        let margin = main_pane_latency_margin(view.as_ref().map_or(self.sessions.len(), Vec::len));
        for s in &self.sessions {
            let mine = Some(s.id()) == self.selected_session_id;
            s.player()
                .set_latency_margin(if mine { margin } else { 0.0 });
        }
        let ids: Vec<_> = self
            .sessions
            .iter()
            .map(|s| s.id())
            .filter(|id| view.as_ref().is_none_or(|v| v.contains(id)))
            .collect();
        // 계획에 없는 세션(하나만 보이거나 숨은 세션)은 덮어쓰기를 지운다 — 숨었다가 다시 보일 때 예전 몫
        // (예: 선택일 때 받은 +7.2%)이 남지 않게.
        let plan = rate_budget_plan(&ids, self.selected_session_id).unwrap_or_default();
        for s in &self.sessions {
            let rate = plan
                .iter()
                .find(|(id, _)| *id == s.id())
                .map(|&(_, rate)| rate);
            s.player().set_max_rate_override(rate);
        }
    }
}
