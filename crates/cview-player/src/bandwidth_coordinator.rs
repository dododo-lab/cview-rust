//! `Sources/CViewPlayer/MultiLiveBandwidthCoordinator.swift` 포팅 — 멀티라이브 세션 간 대역폭 분배,
//! 패인 크기 기반 화질 캡, 버퍼 히스테리시스(flashls AutoLevelManager/AutoBufferManager 참조).
//!
//! 1. **실측 처리량**: 미디어 바이트를 겹치는 body 전송 시간의 합집합으로 나눠 관측
//! 2. **대역폭 분배**: 캡을 면제한 메인·PiP·수동 화질 소비량을 예약한 뒤 나머지를 분배
//! 3. **화면 캡핑**: 실제 패인 크기 이상은 불필요
//! 4. **버퍼 히스테리시스**: 2단계 임계값으로 재버퍼링 핑퐁 방지
//! 5. **긴급 강등**: 버퍼 위기에서 메인·PiP·선택 채널을 보호하며 보조 패인에 캡 적용
//!
//! Swift actor 는 `&mut self` 메서드를 가진 일반 구조체로 옮겼다. 공유가 필요하면 호출자가
//! `Mutex` 로 감싼다. 시각은 단조 시계(초)이며 `_at` 변형으로 주입할 수 있다.

use std::collections::{BTreeSet, HashMap, HashSet};

use uuid::Uuid;

use crate::clock::{SharedClock, TokioMonotonicClock};
use crate::hlsjs::network_metrics::StreamTransferSample;
use crate::policy::constants::multi_live_bw_defaults as defaults;

/// 개별 스트림의 대역폭/버퍼 상태.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamBandwidthState {
    pub session_id: Uuid,
    pub current_bitrate: f64,
    pub buffer_length: f64,
    pub max_level_bitrate: f64,
    pub capped_max_bitrate: f64,
    pub pane_width: i64,
    pub pane_height: i64,
    pub is_selected: bool,
    pub last_fetch_duration: f64,
    pub last_segment_duration: f64,
    pub transfers: Vec<StreamTransferSample>,
    pub protects_quality: bool,
    pub has_manual_quality: bool,
    pub playback_rate: f64,
    /// buffer_length 신뢰도 (0~1).
    pub buffer_confidence: f64,
    /// 배치 스냅숏에서 명시한 실제 로딩 활성 상태.
    pub is_active: bool,
    /// 단조 시계 기준 마지막 샘플 시각.
    pub last_sampled_at: f64,
}

impl StreamBandwidthState {
    pub fn new(session_id: Uuid, sampled_at: f64) -> Self {
        Self {
            session_id,
            current_bitrate: 0.0,
            buffer_length: 0.0,
            max_level_bitrate: 0.0,
            capped_max_bitrate: 0.0,
            pane_width: 1920,
            pane_height: 1080,
            is_selected: false,
            last_fetch_duration: 0.0,
            last_segment_duration: 0.0,
            transfers: Vec::new(),
            protects_quality: false,
            has_manual_quality: false,
            playback_rate: 1.0,
            buffer_confidence: 1.0,
            is_active: true,
            last_sampled_at: sampled_at,
        }
    }
}

/// 한 coordinator tick 의 세션별 원자적 입력.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamBandwidthSample {
    pub session_id: Uuid,
    pub bitrate: f64,
    pub buffer_length: f64,
    pub fetch_duration: f64,
    pub segment_duration: f64,
    pub buffer_confidence: f64,
    pub playback_rate: f64,
    pub transfers: Vec<StreamTransferSample>,
    pub protects_quality: bool,
    pub has_manual_quality: bool,
}

impl StreamBandwidthSample {
    /// Swift 기본 인자(confidence 1, rate 1, 전송 없음, 보호·수동 없음)로 생성.
    pub fn new(
        session_id: Uuid,
        bitrate: f64,
        buffer_length: f64,
        fetch_duration: f64,
        segment_duration: f64,
    ) -> Self {
        Self {
            session_id,
            bitrate,
            buffer_length,
            fetch_duration,
            segment_duration,
            buffer_confidence: 1.0,
            playback_rate: 1.0,
            transfers: Vec::new(),
            protects_quality: false,
            has_manual_quality: false,
        }
    }
}

/// 한 레이아웃 세대에서 함께 적용할 패인 픽셀 크기.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamPaneSizeUpdate {
    pub session_id: Uuid,
    pub width: i64,
    pub height: i64,
}

/// 2단계 임계값 버퍼링 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferHysteresisPhase {
    Playing,
    Buffering,
}

/// 코디네이터가 개별 세션에 내리는 지시.
#[derive(Debug, Clone, PartialEq)]
pub struct BandwidthAdvice {
    pub session_id: Uuid,
    /// 최대 비트레이트 (bps).
    pub max_allowed_bitrate: f64,
    /// 화면 크기 기반 최대 해상도 높이 (0 이면 제한 없음).
    pub capped_max_height: i64,
    pub emergency_downgrade: bool,
    pub buffer_phase: BufferHysteresisPhase,
    /// `false` 면 신뢰할 공유 용량 관측이 없다 — 네트워크 비트레이트 캡을 풀어야 한다.
    pub has_bandwidth_estimate: bool,
}

/// 코디네이터 설정.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandwidthCoordinatorConfig {
    pub safety_factor: f64,
    pub min_per_stream_bitrate: f64,
    pub history_size: usize,
    pub low_buffer_threshold: f64,
    pub high_buffer_threshold: f64,
    pub enable_level_capping: bool,
    pub selected_session_weight: f64,
    pub emergency_buffer_threshold: f64,
    pub sample_freshness_interval: f64,
}

impl Default for BandwidthCoordinatorConfig {
    fn default() -> Self {
        Self {
            safety_factor: defaults::SAFETY_FACTOR,
            min_per_stream_bitrate: defaults::MIN_PER_STREAM_BITRATE,
            history_size: defaults::HISTORY_SIZE,
            low_buffer_threshold: defaults::LOW_BUFFER_THRESHOLD,
            high_buffer_threshold: defaults::HIGH_BUFFER_THRESHOLD,
            enable_level_capping: true,
            selected_session_weight: defaults::SELECTED_SESSION_WEIGHT,
            emergency_buffer_threshold: defaults::EMERGENCY_BUFFER_THRESHOLD,
            sample_freshness_interval: defaults::UPDATE_INTERVAL_SECS * 3.0,
        }
    }
}

fn nonnegative_finite(v: f64) -> f64 {
    if v.is_finite() {
        v.max(0.0)
    } else {
        0.0
    }
}

/// 멀티라이브 대역폭 코디네이터.
#[derive(Debug)]
pub struct MultiLiveBandwidthCoordinator {
    config: BandwidthCoordinatorConfig,
    clock: SharedClock,
    aggregate_demand_history: Vec<f64>,
    bw_sample_index: usize,
    aggregate_history_session_ids: BTreeSet<Uuid>,
    aggregate_history_write_count: usize,
    stream_states: HashMap<Uuid, StreamBandwidthState>,
    buffer_phases: HashMap<Uuid, BufferHysteresisPhase>,
    selected_state_revision: u64,
    pane_geometry_revision: u64,
}

impl Default for MultiLiveBandwidthCoordinator {
    fn default() -> Self {
        Self::new(BandwidthCoordinatorConfig::default())
    }
}

impl MultiLiveBandwidthCoordinator {
    pub fn new(config: BandwidthCoordinatorConfig) -> Self {
        Self::with_clock(config, TokioMonotonicClock::new())
    }

    pub fn with_clock(config: BandwidthCoordinatorConfig, clock: SharedClock) -> Self {
        Self {
            config,
            clock,
            aggregate_demand_history: Vec::new(),
            bw_sample_index: 0,
            aggregate_history_session_ids: BTreeSet::new(),
            aggregate_history_write_count: 0,
            stream_states: HashMap::new(),
            buffer_phases: HashMap::new(),
            selected_state_revision: 0,
            pane_geometry_revision: 0,
        }
    }

    fn now(&self) -> f64 {
        self.clock.now()
    }

    pub fn apply_config(&mut self, config: BandwidthCoordinatorConfig) {
        self.config = config;
    }

    pub fn register_stream(&mut self, session_id: Uuid, is_selected: bool) {
        let now = self.now();
        self.register_stream_at(session_id, is_selected, now);
    }

    pub fn register_stream_at(&mut self, session_id: Uuid, is_selected: bool, sampled_at: f64) {
        let mut state = StreamBandwidthState::new(session_id, sampled_at);
        state.is_selected = is_selected;
        self.stream_states.insert(session_id, state);
        self.buffer_phases
            .insert(session_id, BufferHysteresisPhase::Playing);
        tracing::info!("BWCoordinator: 스트림 등록 {session_id} (selected={is_selected})");
    }

    pub fn unregister_stream(&mut self, session_id: Uuid) {
        self.stream_states.remove(&session_id);
        self.buffer_phases.remove(&session_id);
        tracing::info!("BWCoordinator: 스트림 해제 {session_id}");
    }

    pub fn update_selected_state(&mut self, session_id: Uuid, is_selected: bool) {
        if let Some(s) = self.stream_states.get_mut(&session_id) {
            s.is_selected = is_selected;
        }
    }

    /// 선택 교체. revision 이 오래된 요청은 무시한다.
    pub fn replace_selected_stream(&mut self, session_id: Option<Uuid>, revision: u64) {
        if revision < self.selected_state_revision {
            return;
        }
        self.selected_state_revision = revision;
        for (id, s) in self.stream_states.iter_mut() {
            s.is_selected = Some(*id) == session_id;
        }
    }

    pub fn update_pane_size(&mut self, session_id: Uuid, width: i64, height: i64) {
        if let Some(s) = self.stream_states.get_mut(&session_id) {
            s.pane_width = width;
            s.pane_height = height;
        }
    }

    /// 한 레이아웃의 기하를 원자적으로 적용하고, 늦게 도착한 이전 batch 는 버린다.
    pub fn update_pane_sizes(&mut self, updates: &[StreamPaneSizeUpdate], revision: u64) {
        if revision < self.pane_geometry_revision {
            return;
        }
        self.pane_geometry_revision = revision;
        for u in updates {
            if let Some(s) = self.stream_states.get_mut(&u.session_id) {
                s.pane_width = u.width.max(0);
                s.pane_height = u.height.max(0);
            }
        }
    }

    /// 단건 보고(호환 경로). 보고한 세션은 활성으로 간주한다.
    pub fn report_bandwidth_sample(
        &mut self,
        session_id: Uuid,
        bitrate: f64,
        buffer_length: f64,
        fetch_duration: f64,
        segment_duration: f64,
        buffer_confidence: f64,
    ) {
        let Some(state) = self.stream_states.get(&session_id) else {
            return;
        };
        let mut sample = StreamBandwidthSample::new(
            session_id,
            bitrate,
            buffer_length,
            fetch_duration,
            segment_duration,
        );
        sample.buffer_confidence = buffer_confidence;
        sample.playback_rate = state.playback_rate;
        let mut active: HashSet<Uuid> = self
            .stream_states
            .values()
            .filter(|s| s.is_active)
            .map(|s| s.session_id)
            .collect();
        active.insert(session_id);
        let now = self.now();
        self.report_snapshot_at(&[sample], &active, now);
    }

    pub fn report_snapshot(&mut self, samples: &[StreamBandwidthSample], active: &HashSet<Uuid>) {
        let now = self.now();
        self.report_snapshot_at(samples, active, now);
    }

    /// 한 tick 의 모든 필드와 활성 집합을 갱신한 뒤 aggregate 를 정확히 한 번 기록한다.
    pub fn report_snapshot_at(
        &mut self,
        samples: &[StreamBandwidthSample],
        active: &HashSet<Uuid>,
        sampled_at: f64,
    ) {
        let now = if sampled_at.is_finite() {
            sampled_at
        } else {
            self.now()
        };
        for (id, s) in self.stream_states.iter_mut() {
            s.is_active = active.contains(id);
        }
        for sample in samples {
            let Some(state) = self.stream_states.get_mut(&sample.session_id) else {
                continue;
            };
            state.current_bitrate = nonnegative_finite(sample.bitrate);
            state.buffer_length = nonnegative_finite(sample.buffer_length);
            state.last_fetch_duration = nonnegative_finite(sample.fetch_duration);
            state.last_segment_duration = nonnegative_finite(sample.segment_duration);
            state.transfers = sample.transfers.iter().take(128).cloned().collect();
            state.protects_quality = sample.protects_quality;
            state.has_manual_quality = sample.has_manual_quality;
            state.buffer_confidence = if sample.buffer_confidence.is_finite() {
                sample.buffer_confidence.clamp(0.0, 1.0)
            } else {
                0.0
            };
            state.playback_rate = if sample.playback_rate.is_finite() && sample.playback_rate > 0.0
            {
                sample.playback_rate
            } else {
                1.0
            };
            state.last_sampled_at = now;
        }

        let contributors = self.active_fresh_streams(now);
        let ids: BTreeSet<Uuid> = contributors.iter().map(|s| s.session_id).collect();
        if ids != self.aggregate_history_session_ids {
            self.aggregate_demand_history.clear();
            self.bw_sample_index = 0;
            self.aggregate_history_session_ids = ids;
        }
        let total: f64 = contributors
            .iter()
            .map(|s| s.current_bitrate * s.playback_rate)
            .sum();
        self.record_aggregate_demand(total);
    }

    pub fn update_playback_rate(&mut self, session_id: Uuid, rate: f64) {
        if let Some(s) = self.stream_states.get_mut(&session_id) {
            s.playback_rate = if rate.is_finite() && rate > 0.0 {
                rate
            } else {
                1.0
            };
        }
    }

    pub fn compute_advice(&mut self) -> Vec<BandwidthAdvice> {
        let now = self.now();
        self.compute_advice_at(now)
    }

    /// 활성·신선한 세션들에 대한 분배 어드바이스.
    pub fn compute_advice_at(&mut self, sampled_at: f64) -> Vec<BandwidthAdvice> {
        let now = if sampled_at.is_finite() {
            sampled_at
        } else {
            self.now()
        };
        let streams = self.active_fresh_streams(now);
        if streams.is_empty() {
            return Vec::new();
        }
        let cfg = self.config;
        let count = streams.len() as f64;
        let capacity = observed_capacity(
            &streams,
            now,
            cfg.sample_freshness_interval.clamp(0.0, 20.0),
        );
        let usable = capacity.map_or(0.0, |c| (c * cfg.safety_factor.clamp(0.0, 1.0)).max(0.0));

        let avg_buffer = streams.iter().map(|s| s.buffer_length).sum::<f64>() / count;
        let is_emergency = avg_buffer < cfg.emergency_buffer_threshold;

        let reserves = |s: &StreamBandwidthState| {
            s.protects_quality || (s.has_manual_quality && !is_emergency)
        };
        let adaptive: Vec<&StreamBandwidthState> =
            streams.iter().filter(|s| !reserves(s)).collect();
        let reserved_bw: f64 = streams
            .iter()
            .filter(|s| reserves(s))
            .map(|s| s.current_bitrate * s.playback_rate)
            .sum();
        let adaptive_bw = (usable - reserved_bw).max(0.0);
        let adaptive_count = adaptive.len() as f64;
        let selected_count = adaptive.iter().filter(|s| s.is_selected).count();
        let unselected_count = adaptive.len() - selected_count;
        let mixed = selected_count > 0 && unselected_count > 0;
        let selected_weight = cfg.selected_session_weight.max(0.0);
        let weight = |s: &StreamBandwidthState| {
            if mixed && s.is_selected {
                selected_weight
            } else {
                1.0
            }
        };
        let total_weight: f64 = adaptive.iter().map(|s| weight(s)).sum();

        let configured_floor = cfg.min_per_stream_bitrate.max(0.0);
        let feasible_floor = if adaptive_count > 0.0 {
            configured_floor.min(adaptive_bw / adaptive_count)
        } else {
            0.0
        };
        let distributable = (adaptive_bw - feasible_floor * adaptive_count).max(0.0);

        let mut advices = Vec::with_capacity(streams.len());
        let mut remaining = adaptive_bw;
        let mut adaptive_index = 0usize;
        for stream in &streams {
            let weighted_share = if total_weight > 0.0 {
                weight(stream) / total_weight * distributable
            } else if adaptive_count > 0.0 {
                distributable / adaptive_count
            } else {
                0.0
            };
            let proposed = feasible_floor + weighted_share;
            let allocated = if capacity.is_none() {
                0.0
            } else if reserves(stream) {
                stream.current_bitrate * stream.playback_rate
            } else {
                let a = if adaptive_index + 1 == adaptive.len() {
                    remaining.max(0.0)
                } else {
                    proposed.max(0.0).min(remaining.max(0.0))
                };
                remaining = (remaining - a).max(0.0);
                adaptive_index += 1;
                a
            };
            let capped_height = if cfg.enable_level_capping {
                cap_level_by_pane_size(stream.pane_width, stream.pane_height, stream.is_selected)
            } else {
                0
            };
            let phase = self.compute_buffer_phase(stream.session_id, stream.buffer_length);
            advices.push(BandwidthAdvice {
                session_id: stream.session_id,
                max_allowed_bitrate: allocated,
                capped_max_height: capped_height,
                emergency_downgrade: is_emergency
                    && !stream.is_selected
                    && !stream.protects_quality,
                buffer_phase: phase,
                has_bandwidth_estimate: capacity.is_some(),
            });
        }
        advices
    }

    /// 활성·신선한 상태를 UUID 순으로.
    fn active_fresh_streams(&self, sampled_at: f64) -> Vec<StreamBandwidthState> {
        let freshness = self.config.sample_freshness_interval.max(0.0);
        let mut out: Vec<StreamBandwidthState> = self
            .stream_states
            .values()
            .filter(|s| s.is_active && (sampled_at - s.last_sampled_at).max(0.0) <= freshness)
            .cloned()
            .collect();
        out.sort_by_key(|s| s.session_id);
        out
    }

    fn record_aggregate_demand(&mut self, total: f64) {
        let size = self.config.history_size.max(1);
        if self.aggregate_demand_history.len() < size {
            self.aggregate_demand_history.push(total);
        } else {
            self.aggregate_demand_history[self.bw_sample_index % size] = total;
        }
        self.bw_sample_index += 1;
        self.aggregate_history_write_count += 1;
    }

    /// 누적 기록 수(링 버퍼 wrap 과 무관).
    pub fn aggregate_demand_write_count(&self) -> usize {
        self.aggregate_history_write_count
    }

    /// 최신 재생 수요 합.
    pub fn latest_aggregate_demand(&self) -> Option<f64> {
        if self.aggregate_demand_history.is_empty() {
            return None;
        }
        let size = self.config.history_size.max(1);
        let idx = if self.aggregate_demand_history.len() < size {
            self.aggregate_demand_history.len() - 1
        } else {
            (self.bw_sample_index + size - 1) % size
        };
        self.aggregate_demand_history.get(idx).copied()
    }

    fn compute_buffer_phase(&mut self, session_id: Uuid, buffer: f64) -> BufferHysteresisPhase {
        let current = self
            .buffer_phases
            .get(&session_id)
            .copied()
            .unwrap_or(BufferHysteresisPhase::Playing);
        let next = match current {
            BufferHysteresisPhase::Playing if buffer < self.config.low_buffer_threshold => {
                BufferHysteresisPhase::Buffering
            }
            BufferHysteresisPhase::Buffering if buffer >= self.config.high_buffer_threshold => {
                BufferHysteresisPhase::Playing
            }
            other => other,
        };
        if next != current {
            self.buffer_phases.insert(session_id, next);
        }
        next
    }

    /// 전체 상태 리셋.
    pub fn reset(&mut self) {
        self.stream_states.clear();
        self.buffer_phases.clear();
        self.aggregate_demand_history.clear();
        self.bw_sample_index = 0;
        self.aggregate_history_session_ids.clear();
        self.aggregate_history_write_count = 0;
    }
}

/// 겹치는 body 전송 시간의 합집합으로 본 공유 전달률. 모든 활성 스트림에 유효 표본이 있어야 한다.
fn observed_capacity(streams: &[StreamBandwidthState], now: f64, freshness: f64) -> Option<f64> {
    let mut samples: Vec<&StreamTransferSample> = Vec::new();
    for stream in streams {
        let mut seen = HashSet::new();
        let valid: Vec<&StreamTransferSample> = stream
            .transfers
            .iter()
            .filter(|t| t.is_usable(now, freshness) && seen.insert(t.id.as_str()))
            .collect();
        if valid.is_empty() {
            return None;
        }
        samples.extend(valid);
    }
    if samples.len() < 2 {
        return None;
    }
    samples.sort_by(|a, b| a.first_byte.total_cmp(&b.first_byte));
    let mut start = samples[0].first_byte;
    let mut end = samples[0].end;
    let mut active_seconds = 0.0;
    for s in &samples[1..] {
        if s.first_byte <= end {
            end = end.max(s.end);
        } else {
            active_seconds += end - start;
            start = s.first_byte;
            end = s.end;
        }
    }
    active_seconds += end - start;
    let bytes: f64 = samples.iter().map(|s| s.bytes).sum();
    let estimate = bytes * 8.0 / active_seconds;
    (estimate.is_finite() && estimate > 0.0).then_some(estimate)
}

/// 패인 크기 기반 최대 해상도 높이 — 선택 세션은 0(무제한). 비선택 hls.js 패인의 유일한 디코드 절감 레버.
pub fn cap_level_by_pane_size(pane_width: i64, pane_height: i64, is_selected: bool) -> i64 {
    if is_selected {
        return 0;
    }
    const TIERS: [(i64, i64); 4] = [(640, 360), (854, 480), (1280, 720), (1920, 1080)];
    TIERS
        .iter()
        .find(|(w, h)| *w >= pane_width || *h >= pane_height)
        .map_or(1080, |(_, h)| *h)
}

#[cfg(test)]
mod tests;
