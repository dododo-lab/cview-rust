//! 멀티라이브 매니저의 순수 정책 (정적 함수·MRU 원장·포커스 화질).
//!
//! 원본: `Sources/CViewApp/ViewModels/MultiLiveManager.swift` 의 `nonisolated static` 함수들,
//! `MultiLiveManager+FocusQuality.swift` 의 `MultiLiveFocusQualityPolicy`,
//! `MultiLiveManager+CoordinationSnapshot.swift` 의 `gridPaneSize`·`shouldIncludeInBandwidthActiveSet`,
//! `MultiLiveManager+RateBudget.swift` 의 배분 계산, `MultiLiveManager+Bandwidth.swift` 의 비트레이트 평활.

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use uuid::Uuid;

use cview_player::coordinator::StreamPhase;
use cview_player::hlsjs::profile::HlsJsForcedQualityTarget;

use super::session::MultiLiveLoadState;

/// `PerfTuning.MultiLive` 중 매니저가 쓰는 값.
pub mod tuning {
    use std::time::Duration;

    pub const SETTINGS_LOAD_POLL: Duration = Duration::from_millis(100);
    pub const SETTINGS_LOAD_MAX_RETRIES: usize = 10;
    pub const LIVE_DETAIL_TIMEOUT: Duration = Duration::from_secs(5);
    pub const WATCH_PREWARM_DELAY: Duration = Duration::from_millis(2_500);
    pub const STAGE_HANDOFF_TIMEOUT: Duration = Duration::from_millis(2_000);
    /// `PerfTuning.UI.residentMenuFullSuspendDelayMs`.
    pub const RESIDENT_MENU_FULL_SUSPEND_DELAY: Duration = Duration::from_millis(2_500);
    pub const PANE_RESIZE_DEBOUNCE: Duration = Duration::from_millis(200);
    pub const TIME_MACHINE_REVERT_DELAY: Duration = Duration::from_secs(5);
}

pub const MAX_SESSIONS: usize = 4;
pub const DECODE_BUDGET_K: usize = 4;
pub const ZAP_REQUEST_MAX_AGE_SECS: f64 = 15.0;
pub const ZAP_REQUEST_CAP: usize = 8;
pub const WATCH_KEEP_ALIVE_WARM_COUNT: usize = 2;

pub fn effective_decode_budget(
    session_count: usize,
    configured_max_sessions: usize,
    memory_pressure_override: Option<usize>,
) -> usize {
    if let Some(o) = memory_pressure_override {
        return o.max(1);
    }
    session_count.max(configured_max_sessions).max(1)
}

pub fn should_bypass_bandwidth_caps(
    force_highest_quality: bool,
    forced_height_active: bool,
) -> bool {
    force_highest_quality || forced_height_active
}

pub fn has_session_capacity(active_count: i64, pending_count: i64, maximum: i64) -> bool {
    active_count.max(0) + pending_count.max(0) < maximum.max(1)
}

pub fn should_include_in_bandwidth_active_set(
    phase: &StreamPhase,
    suspend_intent_active: bool,
    in_error_state: bool,
) -> bool {
    phase.requires_playback_resources() && !suspend_intent_active && !in_error_state
}

pub fn needs_restart(load_state: &MultiLiveLoadState, stream_phase: &StreamPhase) -> bool {
    match load_state {
        MultiLiveLoadState::Idle | MultiLiveLoadState::Offline | MultiLiveLoadState::Error(_) => {
            return true
        }
        MultiLiveLoadState::Loading => return false,
        MultiLiveLoadState::Playing { .. } => {}
    }
    matches!(
        stream_phase,
        StreamPhase::Idle | StreamPhase::StreamEnded | StreamPhase::Error(_)
    )
}

/// MRU 원장 후보.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvictionCandidate {
    pub id: Uuid,
    pub tick: u64,
    pub is_audible: bool,
}

/// 보호되지 않고 가청도 아닌 세션 중 tick 이 가장 오래된 세션.
pub fn watch_eviction_victim(
    ordered: &[EvictionCandidate],
    pinned: &HashSet<Uuid>,
) -> Option<Uuid> {
    // Swift `min(by:)` 는 동률이면 첫 원소를 남긴다.
    let mut best: Option<&EvictionCandidate> = None;
    for c in ordered
        .iter()
        .filter(|c| !pinned.contains(&c.id) && !c.is_audible)
    {
        if best.is_none_or(|b| c.tick < b.tick) {
            best = Some(c);
        }
    }
    best.map(|c| c.id)
}

/// 최근 시청 원장.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MruLedger {
    counter: u64,
    ticks: HashMap<Uuid, u64>,
}

impl MruLedger {
    pub fn counter(&self) -> u64 {
        self.counter
    }

    pub fn tick(&self, id: Uuid) -> u64 {
        self.ticks.get(&id).copied().unwrap_or(0)
    }

    pub fn touch(&mut self, id: Uuid) {
        self.counter = self.counter.wrapping_add(1);
        self.ticks.insert(id, self.counter);
    }

    pub fn note_added(&mut self, id: Uuid, restoring: bool) {
        if !restoring {
            self.touch(id);
        }
    }

    pub fn forget(&mut self, id: Uuid) {
        self.ticks.remove(&id);
    }

    pub fn remove_all(&mut self) {
        self.ticks.clear();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeRankInput {
    pub id: Uuid,
    pub insertion_index: usize,
    pub tick: u64,
}

pub fn rank_inputs(ids: &[Uuid], mru: &MruLedger) -> Vec<DecodeRankInput> {
    ids.iter()
        .enumerate()
        .map(|(index, id)| DecodeRankInput {
            id: *id,
            insertion_index: index,
            tick: mru.tick(*id),
        })
        .collect()
}

/// 선택 → PiP → 포커스 메인 → 가청 → 최근 시청 → 삽입 순.
pub fn rank_for_decode_budget(
    inputs: &[DecodeRankInput],
    selected: Option<Uuid>,
    pip: Option<Uuid>,
    focus_main: Option<Uuid>,
    audible: &HashSet<Uuid>,
) -> Vec<Uuid> {
    use std::cmp::Ordering;
    let mut sorted = inputs.to_vec();
    sorted.sort_by(|a, b| {
        if a.id == b.id {
            return Ordering::Equal;
        }
        for pinned in [selected, pip, focus_main].into_iter().flatten() {
            if a.id == pinned {
                return Ordering::Less;
            }
            if b.id == pinned {
                return Ordering::Greater;
            }
        }
        if !audible.is_empty() {
            let (aa, ba) = (audible.contains(&a.id), audible.contains(&b.id));
            if aa != ba {
                return if aa {
                    Ordering::Less
                } else {
                    Ordering::Greater
                };
            }
        }
        if a.tick != b.tick {
            return b.tick.cmp(&a.tick);
        }
        a.insertion_index.cmp(&b.insertion_index)
    });
    sorted.into_iter().map(|i| i.id).collect()
}

/// 그리드 패인 크기 추정.
pub fn grid_pane_size(count: usize, width: i64, height: i64) -> (i64, i64) {
    match count {
        1 => (width, height),
        2 => (width / 2, height),
        3 | 4 => (width / 2, height / 2),
        _ => (width / 3, height / 2),
    }
}

/// 디코드 정지 사유.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DecodeSuspensionReason {
    WindowOcclusion,
    ThermalPressure,
}

/// 패인 정지 원인 (패인 위 안내 문구).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PaneSuspensionCause {
    Thermal,
    Occlusion,
    MemoryPressure,
    DecodeBudget,
}

impl PaneSuspensionCause {
    pub fn message(self) -> Option<&'static str> {
        match self {
            Self::Thermal => Some("발열로 일시 정지 — 선택하면 재생됩니다"),
            Self::MemoryPressure => Some("메모리 절약 중 — 선택하면 재생됩니다"),
            Self::DecodeBudget => Some("동시 재생 한도 — 선택하면 재생됩니다"),
            Self::Occlusion => None,
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Self::Thermal => "thermometer.high",
            Self::MemoryPressure => "memorychip",
            Self::DecodeBudget | Self::Occlusion => "pause.circle",
        }
    }
}

/// `MultiLiveFocusQualityPolicy`.
pub mod focus_quality {
    use super::HlsJsForcedQualityTarget;

    pub const THERMAL_SECONDARY_MAX_HEIGHT: i64 = 360;
    const TIERS: [i64; 3] = [360, 480, 720];

    /// 보조 패인 목표 — 360/480/720 계단, ±15% 히스테리시스, 가청 480 하한, 발열 360 상한.
    pub fn secondary_target(
        pane_size: Option<(f64, f64)>,
        previous_height: i64,
        audible: bool,
        thermal_cap: bool,
    ) -> HlsJsForcedQualityTarget {
        let required = pane_size
            .map(|(w, h)| h.min(w * 9.0 / 16.0))
            .unwrap_or(360.0);
        let mut height = TIERS
            .iter()
            .copied()
            .find(|t| *t as f64 >= required)
            .unwrap_or(720);
        if TIERS.contains(&previous_height) {
            // 올라갈 때는 이전 높이의 +15% 이내, 내려갈 때는 새 계단의 −15% 이내면 유지.
            let hold_up = height > previous_height && required <= previous_height as f64 * 1.15;
            let hold_down = height < previous_height && required >= height as f64 * 0.85;
            if hold_up || hold_down {
                height = previous_height;
            }
        }
        if audible {
            height = height.max(480);
        }
        if thermal_cap {
            height = height.min(THERMAL_SECONDARY_MAX_HEIGHT);
        }
        let kbps = match height {
            360 => 1_000,
            480 => 2_000,
            _ => 4_000,
        };
        HlsJsForcedQualityTarget {
            height,
            target_bitrate_kbps: kbps,
        }
    }

    /// 보조 패인 크기 (포인트).
    pub fn secondary_pane_size(
        stage: (f64, f64),
        count: usize,
        focus: bool,
        horizontal: bool,
        ratio: f64,
    ) -> (f64, f64) {
        let (w, h) = stage;
        if focus {
            let peers = count.saturating_sub(1).max(1) as f64;
            let share = 1.0 - ratio.clamp(0.1, 0.9);
            return if horizontal {
                (w * share, h / peers)
            } else {
                (w / peers, h * share)
            };
        }
        let columns = if count <= 4 {
            2
        } else {
            (count as f64).sqrt().ceil() as usize
        };
        let rows = (count as f64 / columns as f64).ceil() as usize;
        (w / columns as f64, h / rows.max(1) as f64)
    }
}

/// `applyRateBudget` 배분 — 총 +12%, 선택 세션 60%(+7.2%), 나머지 균등.
/// 세션이 2 미만이면 `None` (재정의 해제).
pub fn rate_budget_plan(session_ids: &[Uuid], selected: Option<Uuid>) -> Option<Vec<(Uuid, f64)>> {
    if session_ids.len() < 2 {
        return None;
    }
    let total_budget = 0.12;
    let selected_budget = total_budget * 0.6;
    let remaining_budget = total_budget - selected_budget;
    let non_selected = (session_ids.len() - 1).max(1) as f64;
    let per_non_selected = remaining_budget / non_selected;
    Some(
        session_ids
            .iter()
            .map(|id| {
                let rate = if Some(*id) == selected {
                    1.0 + selected_budget
                } else {
                    1.0 + per_non_selected
                };
                (*id, rate)
            })
            .collect(),
    )
}

/// hls.js 비트레이트 캡 결정 (`applyBandwidthAdvices` 의 평활 규칙).
/// 반환값이 `Some(kbps)` 이면 그 값으로 적용하고 기록한다.
pub fn smoothed_bitrate_cap_kbps(
    max_allowed_bitrate_bps: f64,
    emergency_downgrade: bool,
    last_kbps: Option<f64>,
) -> Option<f64> {
    let raw_kbps = (max_allowed_bitrate_bps / 1000.0).max(1.0);
    if emergency_downgrade || max_allowed_bitrate_bps <= 0.0 {
        return Some(raw_kbps);
    }
    match last_kbps {
        Some(last) if last > 0.0 => {
            let smoothed = 0.4 * raw_kbps + 0.6 * last;
            let rel_delta = (smoothed - last).abs() / last.max(1.0);
            (rel_delta >= 0.12).then_some(smoothed)
        }
        _ => Some(raw_kbps),
    }
}

/// `MultiLiveSession.proactiveRefreshDelay` — 50분 + 0~5분 지터.
pub fn proactive_refresh_delay_secs(jitter_fraction: f64) -> f64 {
    let fraction = jitter_fraction.clamp(0.0, 1.0);
    50.0 * 60.0 + fraction * 5.0 * 60.0
}
