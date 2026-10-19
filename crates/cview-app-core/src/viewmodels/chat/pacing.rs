//! 채팅 커밋 페이싱 정책.
//!
//! 원본: `Sources/CViewCore/Utilities/ChatFlushPacing.swift`, `Sources/CViewCore/Utilities/ChatDripPacing.swift`.
//! `cview-chat` 이 "UI 단계에서 옮긴다" 고 남겨 둔 두 정책을 뷰모델 쪽에 둔다(ChatViewModel 만 쓴다).

use cview_core::models::chat::{ChatMessageItem, MessageType};

/// `ChatFlushPacing` — 한 커밋에 올릴 행 수·픽셀 예산.
pub mod flush {
    use super::*;

    pub const VISIBLE_FLUSH_BUDGET_PX: i64 = 96;
    pub const NORMAL_ROW_ESTIMATE_PX: i64 = 31;
    pub const LONG_ROW_ESTIMATE_PX: i64 = 51;
    pub const LOW_LATENCY_FLUSH_BUDGET_PX: i64 = 240;
    pub const CO_ACTIVE_BUDGET_DIVISOR_CAP: i64 = 8;

    pub fn base_flush_count(pending_count: i64) -> i64 {
        match pending_count {
            i64::MIN..=10 => 1,
            11..=25 => 2,
            26..=50 => 3,
            51..=100 => 6,
            101..=200 => 12,
            201..=400 => 24,
            _ => 48,
        }
    }

    pub fn capped_flush_count(
        base_count: i64,
        messages: &[ChatMessageItem],
        head: usize,
        budget_px: i64,
    ) -> i64 {
        if base_count <= 0 || head >= messages.len() {
            return 0;
        }
        let mut budget = NORMAL_ROW_ESTIMATE_PX.max(budget_px);
        let mut count = 0i64;
        let end = (head + base_count as usize).min(messages.len());
        for m in &messages[head..end] {
            let h = estimated_row_height(m);
            if count > 0 && budget < h {
                break;
            }
            budget -= h;
            count += 1;
        }
        count.max(1)
    }

    pub fn adaptive_budget_px(pending_count: i64, prefer_low_latency: bool) -> i64 {
        if !prefer_low_latency {
            return VISIBLE_FLUSH_BUDGET_PX;
        }
        match pending_count {
            i64::MIN..=120 => VISIBLE_FLUSH_BUDGET_PX,
            121..=240 => 160,
            241..=420 => 200,
            _ => LOW_LATENCY_FLUSH_BUDGET_PX,
        }
    }

    pub fn co_active_scaled_budget_px(base: i64, co_active_sessions: i64) -> i64 {
        if co_active_sessions <= 1 {
            return base;
        }
        let divisor = co_active_sessions.min(CO_ACTIVE_BUDGET_DIVISOR_CAP);
        NORMAL_ROW_ESTIMATE_PX.max(base / divisor)
    }

    pub fn rows_fitting(budget_px: i64) -> i64 {
        (budget_px / NORMAL_ROW_ESTIMATE_PX).max(1)
    }

    pub fn co_active_aggregate_row_budget(co_active_sessions: i64) -> i64 {
        let n = co_active_sessions.max(1);
        if n <= 1 {
            return rows_fitting(VISIBLE_FLUSH_BUDGET_PX);
        }
        n * rows_fitting(co_active_scaled_budget_px(VISIBLE_FLUSH_BUDGET_PX, n))
    }

    /// 통합 채팅 세션별 행 예산 — 바닥 1행 + 잉여를 적재량 비례(최대 잉여 우선) 배분.
    pub fn load_aware_row_budgets(loads: &[i64]) -> Vec<i64> {
        let n = loads.len();
        if n <= 1 {
            return if loads.is_empty() {
                vec![]
            } else {
                vec![co_active_aggregate_row_budget(1)]
            };
        }
        let mut rows = vec![1i64; n];
        let demand: Vec<i64> = loads.iter().map(|l| (*l).clamp(0, 1_000_000)).collect();
        let active: Vec<usize> = (0..n).filter(|&i| demand[i] > 0).collect();
        if active.is_empty() {
            return rows;
        }
        let total = co_active_aggregate_row_budget(n as i64);
        if total <= active.len() as i64 {
            return rows;
        }
        let surplus = total - active.len() as i64;
        let demand_sum: i64 = active.iter().map(|&i| demand[i]).sum();
        let mut granted = 0;
        let mut remainders: Vec<(usize, i64)> = Vec::with_capacity(active.len());
        for &i in &active {
            let numerator = surplus * demand[i];
            let share = numerator / demand_sum;
            rows[i] += share;
            granted += share;
            remainders.push((i, numerator % demand_sum));
        }
        let mut leftover = surplus - granted;
        if leftover > 0 {
            remainders.sort_by(|l, r| {
                if l.1 == r.1 {
                    l.0.cmp(&r.0)
                } else {
                    r.1.cmp(&l.1)
                }
            });
            for (idx, _) in remainders {
                if leftover <= 0 {
                    break;
                }
                rows[idx] += 1;
                leftover -= 1;
            }
        }
        rows
    }

    pub fn merged_drip_floor_ns(base_interval_ns: u64, co_active_sessions: i64) -> u64 {
        if co_active_sessions <= 1 {
            return 0;
        }
        let n = co_active_sessions.min(8) as u64;
        base_interval_ns.min(base_interval_ns.saturating_mul(n) / 8)
    }

    pub fn render_contention_limited_interval_ns(
        interval_ns: u64,
        floor_ns: u64,
        uses_coalesced_commit_pacing: bool,
        is_economy_tier: bool,
        is_window_geometry_transitioning: bool,
    ) -> u64 {
        if uses_coalesced_commit_pacing || !(is_economy_tier || is_window_geometry_transitioning) {
            return interval_ns;
        }
        interval_ns.max(floor_ns)
    }

    pub fn estimated_row_height(item: &ChatMessageItem) -> i64 {
        match item.r#type {
            MessageType::Donation | MessageType::Subscription => LONG_ROW_ESTIMATE_PX,
            _ => {
                if !item.emojis.is_empty() || item.content.len() > 200 {
                    LONG_ROW_ESTIMATE_PX
                } else {
                    NORMAL_ROW_ESTIMATE_PX
                }
            }
        }
    }
}

/// `ChatDripPacing` — 하단 고정 시 한 행씩 흘리고, 지연 예산을 넘으면 따라잡기.
pub mod drip {
    use super::flush;
    use super::*;

    pub const LAG_BUDGET_MS: f64 = 1_500.0;
    pub const CATCH_UP_RELEASE_MS: f64 = 900.0;
    pub const NOMINAL_DRIP_INTERVAL_NS: u64 = 33_000_000;
    pub const PER_FLUSH_BUDGET_CEILING_PX: i64 = 400;
    pub const DROP_LAG_CEILING_SECONDS: f64 = 2.0;
    pub const PENDING_SOFT_CAP_FLOOR: i64 = 120;

    pub fn catch_up_budget_px_per_second() -> f64 {
        flush::LOW_LATENCY_FLUSH_BUDGET_PX as f64
            / (NOMINAL_DRIP_INTERVAL_NS as f64 / 1_000_000_000.0)
    }

    pub fn dadada_burst_factor(pending_count: i64) -> f64 {
        match pending_count {
            i64::MIN..=1 => 1.0,
            2..=8 => 0.72,
            9..=30 => 0.55,
            _ => 0.42,
        }
    }

    pub fn projected_lag_ms(pending_count: i64, interval_ns: u64) -> f64 {
        if pending_count <= 0 {
            return 0.0;
        }
        pending_count as f64 * interval_ns as f64 / 1_000_000.0
    }

    pub fn next_catch_up_state(was_catching_up: bool, projected_lag_ms: f64) -> bool {
        if was_catching_up {
            projected_lag_ms > CATCH_UP_RELEASE_MS
        } else {
            projected_lag_ms > LAG_BUDGET_MS
        }
    }

    pub fn desired_flush_count(pending_count: i64, interval_ns: u64) -> i64 {
        if pending_count <= 0 {
            return 0;
        }
        let tick_ms = interval_ns as f64 / 1_000_000.0;
        if tick_ms <= 0.0 {
            return pending_count;
        }
        let ticks = (CATCH_UP_RELEASE_MS / tick_ms).max(1.0);
        ((pending_count as f64 / ticks).ceil() as i64).max(1)
    }

    pub fn per_flush_budget_px(
        interval_ns: u64,
        co_active_sessions: i64,
        assigned_rows: Option<i64>,
    ) -> i64 {
        let seconds = interval_ns as f64 / 1_000_000_000.0;
        let raw = (catch_up_budget_px_per_second() * seconds).round() as i64;
        let clamped = PER_FLUSH_BUDGET_CEILING_PX.min(flush::VISIBLE_FLUSH_BUDGET_PX.max(raw));
        if co_active_sessions <= 1 {
            return clamped;
        }
        match assigned_rows {
            Some(rows) if rows > 0 => {
                let divisor = co_active_sessions.min(flush::CO_ACTIVE_BUDGET_DIVISOR_CAP);
                let share = clamped.min(clamped * rows / divisor);
                flush::NORMAL_ROW_ESTIMATE_PX.max(share)
            }
            _ => flush::co_active_scaled_budget_px(clamped, co_active_sessions),
        }
    }

    pub fn max_drain_rows_per_second(
        interval_ns: u64,
        co_active_sessions: i64,
        assigned_rows: Option<i64>,
    ) -> f64 {
        let budget = per_flush_budget_px(interval_ns, co_active_sessions, assigned_rows);
        let rows = (budget / flush::NORMAL_ROW_ESTIMATE_PX).max(1);
        let seconds = interval_ns as f64 / 1_000_000_000.0;
        if seconds <= 0.0 {
            return rows as f64;
        }
        rows as f64 / seconds
    }

    pub fn pending_soft_cap(
        interval_ns: u64,
        co_active_sessions: i64,
        hard_cap: i64,
        assigned_rows: Option<i64>,
    ) -> i64 {
        let drain = max_drain_rows_per_second(interval_ns, co_active_sessions, assigned_rows);
        let target = (DROP_LAG_CEILING_SECONDS * drain).ceil() as i64;
        hard_cap.min(PENDING_SOFT_CAP_FLOOR.max(target))
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Plan {
        pub flush_count: i64,
        pub is_catching_up: bool,
        pub projected_lag_ms: f64,
    }

    pub fn plan(
        pending_count: i64,
        interval_ns: u64,
        messages: &[ChatMessageItem],
        head: usize,
        co_active_sessions: i64,
        was_catching_up: bool,
        assigned_rows: Option<i64>,
    ) -> Plan {
        let lag = projected_lag_ms(pending_count, interval_ns);
        let catching_up = next_catch_up_state(was_catching_up, lag);
        if !catching_up || pending_count <= 1 {
            return Plan {
                flush_count: pending_count.clamp(0, 1),
                is_catching_up: catching_up,
                projected_lag_ms: lag,
            };
        }
        let desired = desired_flush_count(pending_count, interval_ns);
        let budget_px = per_flush_budget_px(interval_ns, co_active_sessions, assigned_rows);
        let capped = flush::capped_flush_count(desired, messages, head, budget_px);
        Plan {
            flush_count: capped.max(1),
            is_catching_up: catching_up,
            projected_lag_ms: lag,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use cview_core::models::chat::EmojiMap;

    fn item(id: &str) -> ChatMessageItem {
        ChatMessageItem::new(
            id,
            "u",
            "n",
            "hi",
            Utc::now(),
            MessageType::Normal,
            EmojiMap::new(),
            false,
            false,
        )
    }

    #[test]
    fn base_counts() {
        assert_eq!(flush::base_flush_count(0), 1);
        assert_eq!(flush::base_flush_count(25), 2);
        assert_eq!(flush::base_flush_count(401), 48);
    }

    #[test]
    fn capped_respects_budget() {
        let msgs: Vec<_> = (0..10).map(|i| item(&i.to_string())).collect();
        assert_eq!(flush::capped_flush_count(10, &msgs, 0, 96), 3);
        assert_eq!(flush::capped_flush_count(10, &msgs, 0, 0), 1);
        assert_eq!(flush::capped_flush_count(0, &msgs, 0, 96), 0);
    }

    #[test]
    fn load_aware_budget_sums_to_aggregate() {
        let rows = flush::load_aware_row_budgets(&[10, 0, 30]);
        assert_eq!(rows[1], 1);
        let total = flush::co_active_aggregate_row_budget(3);
        assert_eq!(
            rows.iter().sum::<i64>(),
            total + 1,
            "비활성 세션의 바닥 1행은 총량 밖"
        );
        assert!(rows[2] > rows[0]);
        assert_eq!(flush::load_aware_row_budgets(&[5]), vec![3]);
        assert_eq!(flush::load_aware_row_budgets(&[0, 0]), vec![1, 1]);
    }

    #[test]
    fn drip_plan_single_row_until_lag_budget() {
        let msgs: Vec<_> = (0..20).map(|i| item(&i.to_string())).collect();
        let p = drip::plan(20, drip::NOMINAL_DRIP_INTERVAL_NS, &msgs, 0, 1, false, None);
        assert_eq!(p.flush_count, 1);
        assert!(!p.is_catching_up);
        let msgs: Vec<_> = (0..100).map(|i| item(&i.to_string())).collect();
        let p = drip::plan(
            100,
            drip::NOMINAL_DRIP_INTERVAL_NS,
            &msgs,
            0,
            1,
            false,
            None,
        );
        assert!(p.is_catching_up);
        assert!(p.flush_count > 1);
    }

    #[test]
    fn soft_cap_has_floor_and_ceiling() {
        let cap = drip::pending_soft_cap(drip::NOMINAL_DRIP_INTERVAL_NS, 1, 1000, None);
        assert!((drip::PENDING_SOFT_CAP_FLOOR..=1000).contains(&cap));
        assert_eq!(
            drip::pending_soft_cap(drip::NOMINAL_DRIP_INTERVAL_NS, 1, 50, None),
            50
        );
    }

    #[test]
    fn merged_floor() {
        assert_eq!(flush::merged_drip_floor_ns(33_000_000, 1), 0);
        assert_eq!(flush::merged_drip_floor_ns(32_000_000, 4), 16_000_000);
        assert_eq!(flush::merged_drip_floor_ns(32_000_000, 12), 32_000_000);
    }
}
