//! 통합(merged)·그리드 채팅에서 세션별 커밋 행 예산 배분.
//!
//! 원본: `Sources/CViewApp/Services/MergedChatBudgetCoordinator.swift`.
//! ChatViewModel 과 맞물려 있어(세션 적재량 읽기·예산 쓰기) 뷰모델 쪽에 둔다.
//! 멤버의 잠금 안에서 불릴 때(드립 계산 중) 다른 멤버는 `try_lock` 으로만 건드린다 —
//! 잡지 못한 멤버는 직전 적재량을 쓰고 예산 쓰기를 다음 재계산으로 미룬다.

use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use super::pacing::flush;
use super::{ChatShared, ChatState, ChatViewModel};

#[derive(Default)]
struct Inner {
    members: Vec<Weak<ChatShared>>,
    cached_loads: Vec<i64>,
    last_recompute_at: Option<Instant>,
}

/// 세션 행 예산 조정자.
#[derive(Default)]
pub struct MergedChatBudgetCoordinator {
    inner: Mutex<Inner>,
}

impl std::fmt::Debug for MergedChatBudgetCoordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MergedChatBudgetCoordinator")
            .finish_non_exhaustive()
    }
}

impl MergedChatBudgetCoordinator {
    pub const RECOMPUTE_INTERVAL: Duration = Duration::from_millis(120);

    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// 멤버 등록 후 즉시 재계산. 어떤 뷰모델 잠금도 쥐지 않은 채 불러야 한다.
    pub fn register(&self, view_models: &[ChatViewModel]) {
        {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            g.members = view_models.iter().map(|vm| vm.downgrade()).collect();
            g.cached_loads = vec![0; g.members.len()];
            g.last_recompute_at = None;
        }
        self.recompute(Instant::now());
    }

    pub fn clear(&self) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.members.clear();
        g.cached_loads.clear();
        g.last_recompute_at = None;
    }

    pub fn registered_count(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .members
            .iter()
            .filter(|w| w.strong_count() > 0)
            .count()
    }

    pub fn refresh_if_needed(&self, now: Instant) {
        if self.is_throttled(now) {
            return;
        }
        self.recompute(now);
    }

    fn is_throttled(&self, now: Instant) -> bool {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        matches!(g.last_recompute_at, Some(last) if now.saturating_duration_since(last) < Self::RECOMPUTE_INTERVAL)
    }

    /// 외부(잠금 없는 곳)에서의 재계산.
    pub fn recompute(&self, now: Instant) {
        self.recompute_with(now, None);
    }

    /// 멤버 잠금 안에서의 갱신 — `current` 는 이미 잠긴 그 멤버의 상태.
    pub(crate) fn refresh_from_member(&self, current: &mut ChatState, now: Instant) {
        if self.is_throttled(now) {
            return;
        }
        self.recompute_with(now, Some(current));
    }

    fn recompute_with(&self, now: Instant, mut current: Option<&mut ChatState>) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.last_recompute_at = Some(now);
        let current_ptr = current.as_ref().map(|s| s.this.as_ptr());
        let live: Vec<(usize, Arc<ChatShared>)> = g
            .members
            .iter()
            .enumerate()
            .filter_map(|(i, w)| w.upgrade().map(|a| (i, a)))
            .collect();
        let is_current = |a: &Arc<ChatShared>| current_ptr == Some(Arc::as_ptr(a));
        if live.len() <= 1 {
            for (_, a) in &live {
                if is_current(a) {
                    if let Some(s) = current.as_deref_mut() {
                        s.merged_row_budget = 0;
                    }
                } else if let Some(mut s) = a.obs.try_lock() {
                    s.merged_row_budget = 0;
                }
            }
            return;
        }
        // 적재량 수집.
        let mut loads = Vec::with_capacity(live.len());
        for (i, a) in &live {
            let load = if is_current(a) {
                current.as_deref().map(|s| s.merged_budget_load())
            } else {
                a.obs.try_lock().map(|s| s.merged_budget_load())
            };
            let load = load.unwrap_or_else(|| g.cached_loads.get(*i).copied().unwrap_or(0));
            if let Some(slot) = g.cached_loads.get_mut(*i) {
                *slot = load;
            }
            loads.push(load);
        }
        let rows = flush::load_aware_row_budgets(&loads);
        drop(g);
        for ((_, a), budget) in live.iter().zip(rows) {
            if is_current(a) {
                if let Some(s) = current.as_deref_mut() {
                    s.merged_row_budget = budget;
                }
            } else if let Some(mut s) = a.obs.try_lock() {
                s.merged_row_budget = budget;
            }
        }
    }
}

impl ChatState {
    /// Swift `assignedMergedRows` — 동시 활성 세션이 2개 이상이고 조정자가 있을 때의 배정 행 수.
    pub fn assigned_merged_rows(&mut self) -> Option<i64> {
        if self.co_active_foreground_sessions <= 1 {
            return None;
        }
        let coordinator = self.merged_budget_coordinator.as_ref()?.upgrade()?;
        coordinator.refresh_from_member(self, Instant::now());
        let rows = self.merged_row_budget;
        (rows > 0).then_some(rows)
    }
}
