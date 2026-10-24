//! 프록시 계측 — `Sources/CViewPlayer/LocalStreamProxy.swift` 의 카운터·공급 공백 계측,
//! `LocalStreamProxy+SegmentMetrics.swift`, `LocalStreamProxy+SupplyGapCause.swift` 포팅.
//!
//! 비정상일 때만 한 줄씩 남긴다:
//! - `공급 공백` — 세그먼트 요청 자체가 오지 않은 구간(또는 상류 재생목록 보류)
//! - `상류 지연` — 요청은 왔는데 첫 바이트/전체 전송이 느린 경우

use std::sync::Mutex;

use chrono::Utc;
use cview_core::models::proxy_stats::ProxyNetworkStats;

/// 공백 판정의 하한(초).
pub const SEGMENT_GAP_THRESHOLD: f64 = 3.0;
/// 평소 주기의 몇 배를 넘으면 공백인가.
pub const SEGMENT_GAP_CADENCE_FACTOR: f64 = 1.8;
/// 평소 주기를 모를 때(표본 3개 미만)의 기준(초).
pub const SEGMENT_GAP_WARMUP_THRESHOLD: f64 = 8.0;
/// 첫 바이트가 이 값보다 늦으면 상류 지연(초).
pub const SEGMENT_TTFB_THRESHOLD: f64 = 1.5;
/// 세그먼트 하나에 이 값보다 오래 걸리면 상류 지연(초).
pub const SEGMENT_FETCH_THRESHOLD: f64 = 3.0;
/// 평소 주기를 잴 표본 수.
pub const SEGMENT_INTERVAL_WINDOW: usize = 8;
/// 상류 재생목록 대기가 이 값(초) 이상이어야 원인으로 본다.
pub const PLAYLIST_HOLD_CAUSE_FLOOR: f64 = 1.5;
/// 공백의 이 비율 이상을 재생목록 대기가 차지해야 원인으로 본다.
pub const PLAYLIST_HOLD_CAUSE_RATIO: f64 = 0.5;
/// 연속 403 이 이 횟수에 이르면 토큰 만료로 본다.
pub const CONSECUTIVE_403_THRESHOLD: i64 = 3;

const RESPONSE_TIME_WINDOW: usize = 20;
const SEGMENT_FETCH_WINDOW: usize = 12;

/// 공급 공백의 원인.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupplyGapCause {
    /// 세그먼트 요청 자체가 오지 않았다 — 클라이언트(hls.js) 측.
    Client,
    /// 상류가 재생목록 요청을 오래 붙잡아 다음 요청이 막혔다.
    UpstreamPlaylist,
}

/// 최근 요청 간격의 중앙값 — 표본 3개 미만이면 `None`.
pub fn typical_segment_interval(intervals: &[f64]) -> Option<f64> {
    if intervals.len() < 3 {
        return None;
    }
    let mut sorted = intervals.to_vec();
    sorted.sort_by(f64::total_cmp);
    Some(sorted[sorted.len() / 2])
}

/// 이 스트림의 평소 요청 주기 1.8배(하한 3초)를 넘을 때만 공백. 주기를 모르면 8초.
pub fn is_supply_gap(gap: f64, recent: &[f64]) -> bool {
    if !gap.is_finite() || gap < SEGMENT_GAP_THRESHOLD {
        return false;
    }
    match typical_segment_interval(recent) {
        None => gap >= SEGMENT_GAP_WARMUP_THRESHOLD,
        Some(t) => gap >= SEGMENT_GAP_THRESHOLD.max(t * SEGMENT_GAP_CADENCE_FACTOR),
    }
}

/// `longest_playlist_wait >= max(1.5, gap × 0.5)` 이면 상류 재생목록 보류.
pub fn supply_gap_cause(gap: f64, longest_playlist_wait: f64) -> SupplyGapCause {
    if !gap.is_finite() || !longest_playlist_wait.is_finite() {
        return SupplyGapCause::Client;
    }
    let threshold = PLAYLIST_HOLD_CAUSE_FLOOR.max(gap * PLAYLIST_HOLD_CAUSE_RATIO);
    if longest_playlist_wait >= threshold {
        SupplyGapCause::UpstreamPlaylist
    } else {
        SupplyGapCause::Client
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct Counters {
    total_requests: i64,
    cache_hits: i64,
    cache_misses: i64,
    error_count: i64,
    total_bytes_received: i64,
    total_bytes_served: i64,
    client_aborts: i64,
}

/// 성능 로그 한 창(보통 10초) 동안의 세그먼트 수신 요약 — `take_perf_window` 가 꺼내고 비운다.
/// 멈춤이 상류(CDN 첫 바이트·전송) 탓인지, 요청 자체가 늦은(hls.js·재생목록 보류) 탓인지 가르는 데 쓴다.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProxyPerfWindow {
    /// 받은 세그먼트 수.
    pub segments: u32,
    /// 첫 바이트(초) 중앙값·최대.
    pub ttfb_p50: f64,
    pub ttfb_max: f64,
    /// 세그먼트 하나 전송(초) 중앙값·최대.
    pub fetch_p50: f64,
    pub fetch_max: f64,
    /// 상류 지연 문턱(첫 바이트 1.5초·전송 3초)을 넘은 세그먼트 수.
    pub slow: u32,
    /// 공급 공백 — 요청이 안 온(hls.js 측) 횟수 / 상류 재생목록 보류 횟수.
    pub gaps_client: u32,
    pub gaps_upstream: u32,
    /// HTTP 오류·연결 오류 수.
    pub errors: u32,
    /// 상류에서 받은 바이트.
    pub bytes: i64,
}

#[derive(Debug, Default)]
struct PerfAccumulator {
    ttfb: Vec<f64>,
    fetch: Vec<f64>,
    slow: u32,
    gaps_client: u32,
    gaps_upstream: u32,
    errors: u32,
    bytes: i64,
}

#[derive(Debug, Default)]
struct State {
    counters: Counters,
    perf: PerfAccumulator,
    consecutive_403: i64,
    response_times: Vec<f64>,
    /// (fetch, ttfb)
    segment_fetch_samples: Vec<(f64, f64)>,
    upstream_summary_counter: u32,
    last_segment_request_at: Option<f64>,
    playlist_wait_in_gap: f64,
    segment_interval_history: Vec<f64>,
}

/// 프록시 하나의 계측 상태.
#[derive(Debug, Default)]
pub struct ProxyTelemetry {
    state: Mutex<State>,
}

fn quantile(values: &[f64], q: f64) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let idx = ((sorted.len() as f64 * q) as usize).min(sorted.len() - 1);
    sorted[idx]
}

impl ProxyTelemetry {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 세그먼트 요청 도착 — 평소 주기보다 크게 벌어졌으면 한 줄 남기고 원인을 돌려준다.
    pub fn note_segment_request_arrived(&self, now: f64) -> Option<SupplyGapCause> {
        let (previous, playlist_wait, recent) = {
            let mut s = self.lock();
            let previous = s.last_segment_request_at.replace(now);
            let wait = std::mem::take(&mut s.playlist_wait_in_gap);
            let previous = previous?;
            let gap = now - previous;
            if !gap.is_finite() || gap < 0.0 {
                return None;
            }
            let snapshot = s.segment_interval_history.clone();
            s.segment_interval_history.push(gap);
            let len = s.segment_interval_history.len();
            if len > SEGMENT_INTERVAL_WINDOW {
                s.segment_interval_history
                    .drain(..len - SEGMENT_INTERVAL_WINDOW);
            }
            (previous, wait, snapshot)
        };
        let gap = now - previous;
        if !is_supply_gap(gap, &recent) {
            return None;
        }
        let typical = typical_segment_interval(&recent).unwrap_or(0.0);
        let cause = supply_gap_cause(gap, playlist_wait);
        {
            let mut s = self.lock();
            match cause {
                SupplyGapCause::Client => s.perf.gaps_client += 1,
                SupplyGapCause::UpstreamPlaylist => s.perf.gaps_upstream += 1,
            }
        }
        match cause {
            SupplyGapCause::Client => tracing::warn!(
                "공급 공백: 세그먼트 요청이 {gap:.2}초(평소 {typical:.2}초) 동안 없었다 — hls.js 미요청 구간"
            ),
            SupplyGapCause::UpstreamPlaylist => tracing::warn!(
                "공급 공백: 세그먼트 요청이 {gap:.2}초(평소 {typical:.2}초) 동안 없었다 — 상류 재생목록 보류 {playlist_wait:.2}초"
            ),
        }
        Some(cause)
    }

    /// 상류 재생목록 대기(초)를 이번 간격의 최장값에 넣는다.
    pub fn note_playlist_upstream_wait(&self, waited: f64) {
        if waited.is_finite() && waited > 0.0 {
            let mut s = self.lock();
            s.playlist_wait_in_gap = s.playlist_wait_in_gap.max(waited);
        }
    }

    /// 이번 간격의 최장 재생목록 대기를 꺼내고 비운다.
    pub fn take_longest_playlist_wait(&self) -> f64 {
        std::mem::take(&mut self.lock().playlist_wait_in_gap)
    }

    /// 세그먼트 fetch 표본 1건.
    pub fn record_segment_fetch_sample(&self, fetch: f64, ttfb: f64) {
        if !fetch.is_finite() || fetch <= 0.0 {
            return;
        }
        let ttfb = if ttfb.is_finite() && ttfb >= 0.0 {
            ttfb
        } else {
            0.0
        };
        let summary = {
            let mut s = self.lock();
            s.perf.fetch.push(fetch);
            s.perf.ttfb.push(ttfb);
            if ttfb >= SEGMENT_TTFB_THRESHOLD || fetch >= SEGMENT_FETCH_THRESHOLD {
                s.perf.slow += 1;
            }
            s.segment_fetch_samples.push((fetch, ttfb));
            let len = s.segment_fetch_samples.len();
            if len > SEGMENT_FETCH_WINDOW {
                s.segment_fetch_samples.drain(..len - SEGMENT_FETCH_WINDOW);
            }
            s.upstream_summary_counter += 1;
            if s.upstream_summary_counter >= 30 {
                s.upstream_summary_counter = 0;
                Some((s.segment_fetch_samples.clone(), s.counters.client_aborts))
            } else {
                None
            }
        };
        if ttfb >= SEGMENT_TTFB_THRESHOLD || fetch >= SEGMENT_FETCH_THRESHOLD {
            tracing::warn!("상류 지연: ttfb={ttfb:.2}초 fetch={fetch:.2}초");
        }
        if let Some((samples, aborts)) = summary {
            if samples.len() >= 4 {
                let ttfbs: Vec<f64> = samples.iter().map(|s| s.1).collect();
                let fetches: Vec<f64> = samples.iter().map(|s| s.0).collect();
                tracing::debug!(
                    "상류 타이밍: n={} ttfb_p50={} ttfb_p90={} fetch_p50={} fetch_p90={} client_aborts={aborts}",
                    samples.len(),
                    quantile(&ttfbs, 0.5),
                    quantile(&ttfbs, 0.9),
                    quantile(&fetches, 0.5),
                    quantile(&fetches, 0.9)
                );
            }
        }
    }

    pub fn record_segment_ok(&self) {
        let mut s = self.lock();
        s.consecutive_403 = 0;
        s.counters.total_requests += 1;
        s.counters.cache_misses += 1;
    }

    /// HTTP 오류 응답. 연속 403 이 임계값에 이르면 `true`(상위 통보) 를 돌려주고 카운터를 비운다.
    pub fn record_segment_http_error(&self, status: u16) -> bool {
        let mut s = self.lock();
        s.counters.total_requests += 1;
        s.counters.cache_misses += 1;
        s.counters.error_count += 1;
        s.perf.errors += 1;
        if status != 403 {
            return false;
        }
        s.consecutive_403 += 1;
        if s.consecutive_403 >= CONSECUTIVE_403_THRESHOLD {
            let count = s.consecutive_403;
            s.consecutive_403 = 0;
            tracing::warn!("Proxy(stream): CDN 403 {count}회 연속 — 토큰 만료 의심, 상위 통보");
            return true;
        }
        false
    }

    pub fn record_segment_error(&self) {
        let mut s = self.lock();
        s.counters.total_requests += 1;
        s.counters.cache_misses += 1;
        s.counters.error_count += 1;
        s.perf.errors += 1;
    }

    pub fn record_segment_metrics(&self, elapsed: f64, bytes_received: i64) {
        let mut s = self.lock();
        s.response_times.push(elapsed);
        let len = s.response_times.len();
        if len > RESPONSE_TIME_WINDOW {
            s.response_times.drain(..len - RESPONSE_TIME_WINDOW);
        }
        if bytes_received > 0 {
            s.counters.total_bytes_received += bytes_received;
            s.perf.bytes += bytes_received;
        }
    }

    pub fn record_bytes_served(&self, bytes: i64) {
        if bytes > 0 {
            self.lock().counters.total_bytes_served += bytes;
        }
    }

    pub fn record_client_abort(&self) {
        self.lock().counters.client_aborts += 1;
    }

    pub fn client_aborts(&self) -> i64 {
        self.lock().counters.client_aborts
    }

    pub fn consecutive_403(&self) -> i64 {
        self.lock().consecutive_403
    }

    pub fn reset_consecutive_403(&self) {
        self.lock().consecutive_403 = 0;
    }

    /// 통계 스냅샷.
    pub fn network_stats(&self, active_connections: i64) -> ProxyNetworkStats {
        let s = self.lock();
        let (avg, max) = if s.response_times.is_empty() {
            (0.0, 0.0)
        } else {
            let sum: f64 = s.response_times.iter().sum();
            (
                sum / s.response_times.len() as f64,
                s.response_times.iter().copied().fold(0.0, f64::max),
            )
        };
        let n = s.segment_fetch_samples.len();
        let (seg_fetch, seg_ttfb) = if n == 0 {
            (0.0, 0.0)
        } else {
            (
                s.segment_fetch_samples.iter().map(|x| x.0).sum::<f64>() / n as f64,
                s.segment_fetch_samples.iter().map(|x| x.1).sum::<f64>() / n as f64,
            )
        };
        ProxyNetworkStats {
            total_requests: s.counters.total_requests,
            cache_hits: s.counters.cache_hits,
            cache_misses: s.counters.cache_misses,
            error_count: s.counters.error_count,
            total_bytes_received: s.counters.total_bytes_received,
            total_bytes_served: s.counters.total_bytes_served,
            active_connections,
            consecutive_403_count: s.consecutive_403,
            avg_response_time: avg,
            max_response_time: max,
            avg_segment_fetch_duration: seg_fetch,
            avg_segment_ttfb: seg_ttfb,
            segment_sample_count: n as i64,
            timestamp: Utc::now(),
        }
    }

    /// 지난 창의 세그먼트 수신 요약을 꺼내고 비운다. 이 창에 아무 일도 없었으면 `None`.
    pub fn take_perf_window(&self) -> Option<ProxyPerfWindow> {
        let p = std::mem::take(&mut self.lock().perf);
        if p.fetch.is_empty() && p.gaps_client == 0 && p.gaps_upstream == 0 && p.errors == 0 {
            return None;
        }
        let stat = |v: &[f64]| {
            if v.is_empty() {
                (0.0, 0.0)
            } else {
                (quantile(v, 0.5), v.iter().copied().fold(0.0, f64::max))
            }
        };
        let (ttfb_p50, ttfb_max) = stat(&p.ttfb);
        let (fetch_p50, fetch_max) = stat(&p.fetch);
        Some(ProxyPerfWindow {
            segments: p.fetch.len() as u32,
            ttfb_p50,
            ttfb_max,
            fetch_p50,
            fetch_max,
            slow: p.slow,
            gaps_client: p.gaps_client,
            gaps_upstream: p.gaps_upstream,
            errors: p.errors,
            bytes: p.bytes,
        })
    }

    /// 통계 리셋(세션 종료 시) — 직전 세그먼트 시각·평소 주기·재생목록 대기도 버린다.
    pub fn reset(&self) {
        let mut s = self.lock();
        let consecutive = s.consecutive_403;
        *s = State::default();
        s.consecutive_403 = consecutive;
    }
}

#[cfg(test)]
mod tests {
    //! `Tests/CViewPlayerTests/ProxySupplyGapTelemetryTests.swift` 포팅.
    use super::*;

    #[test]
    fn perf_window_summarizes_and_resets() {
        let t = ProxyTelemetry::default();
        assert_eq!(t.take_perf_window(), None);
        t.record_segment_fetch_sample(0.4, 0.1);
        t.record_segment_fetch_sample(0.6, 0.2);
        t.record_segment_fetch_sample(3.5, 1.8); // 상류 지연
        t.record_segment_metrics(0.5, 1000);
        t.record_segment_http_error(500);
        let w = t.take_perf_window().expect("창이 있다");
        assert_eq!(w.segments, 3);
        assert_eq!(w.slow, 1);
        assert_eq!(w.errors, 1);
        assert_eq!(w.bytes, 1000);
        assert!((w.fetch_p50 - 0.6).abs() < 1e-9 && (w.fetch_max - 3.5).abs() < 1e-9);
        assert!((w.ttfb_max - 1.8).abs() < 1e-9);
        // 꺼내면 비운다
        assert_eq!(t.take_perf_window(), None);
    }

    #[test]
    fn perf_window_counts_supply_gaps() {
        let t = ProxyTelemetry::default();
        for at in [0.0, 2.0, 4.0, 6.0, 8.0] {
            t.note_segment_request_arrived(at);
        }
        t.note_segment_request_arrived(20.0); // 평소 2초의 1.8배를 크게 넘는 공백
        let w = t.take_perf_window().expect("공백이 기록된다");
        assert_eq!((w.gaps_client, w.gaps_upstream, w.segments), (1, 0, 0));
    }

    #[test]
    fn thresholds_match_segment_duration() {
        const { assert!(SEGMENT_GAP_THRESHOLD > 2.0) };
        const { assert!(SEGMENT_TTFB_THRESHOLD < 2.0) };
        const { assert!(SEGMENT_FETCH_THRESHOLD > SEGMENT_TTFB_THRESHOLD) };
    }

    #[test]
    fn cadence_aware_gap() {
        let four = [4.1, 3.9, 4.4, 4.0, 4.3, 4.6, 3.8, 4.2];
        for gap in [4.45, 5.2, 6.6] {
            assert!(!is_supply_gap(gap, &four));
        }
        assert!(is_supply_gap(9.0, &four));
        let two = [2.0, 2.1, 1.9, 2.0, 2.05];
        assert!(!is_supply_gap(2.9, &two));
        assert!(is_supply_gap(3.7, &two));
        assert!(!is_supply_gap(5.0, &[2.0, 2.0]));
        assert!(is_supply_gap(8.5, &[]));
        assert_eq!(typical_segment_interval(&[2.0, 2.0]), None);
        assert_eq!(typical_segment_interval(&[4.0, 9.0, 4.2]), Some(4.2));
    }

    #[test]
    fn classifies_measured_gaps() {
        use SupplyGapCause::*;
        assert_eq!(supply_gap_cause(8.25, 8.0), UpstreamPlaylist);
        assert_eq!(supply_gap_cause(5.88, 1.07), Client);
        assert_eq!(supply_gap_cause(6.99, 0.07), Client);
        assert_eq!(supply_gap_cause(3.0, 1.49), Client);
        assert_eq!(supply_gap_cause(3.0, 1.5), UpstreamPlaylist);
        assert_eq!(supply_gap_cause(10.0, 4.9), Client);
        assert_eq!(supply_gap_cause(f64::NAN, 8.0), Client);
    }

    #[test]
    fn playlist_wait_is_scoped_to_one_gap() {
        let t = ProxyTelemetry::default();
        let mut now = 1000.0;
        assert_eq!(t.note_segment_request_arrived(now), None);
        for _ in 0..4 {
            now += 2.0;
            assert_eq!(t.note_segment_request_arrived(now), None);
        }
        t.note_playlist_upstream_wait(1.1);
        t.note_playlist_upstream_wait(8.0);
        now += 8.25;
        assert_eq!(
            t.note_segment_request_arrived(now),
            Some(SupplyGapCause::UpstreamPlaylist)
        );
        now += 8.25;
        assert_eq!(
            t.note_segment_request_arrived(now),
            Some(SupplyGapCause::Client)
        );
        t.note_playlist_upstream_wait(8.0);
        now += 2.0;
        assert_eq!(t.note_segment_request_arrived(now), None);
        now += 9.0;
        assert_eq!(
            t.note_segment_request_arrived(now),
            Some(SupplyGapCause::Client)
        );
        t.note_playlist_upstream_wait(8.0);
        t.reset();
        assert_eq!(t.take_longest_playlist_wait(), 0.0);
    }

    #[test]
    fn consecutive_403_fires_at_threshold() {
        let t = ProxyTelemetry::default();
        assert!(!t.record_segment_http_error(403));
        assert!(!t.record_segment_http_error(403));
        assert!(t.record_segment_http_error(403));
        assert_eq!(t.consecutive_403(), 0);
        t.record_segment_http_error(403);
        t.record_segment_ok();
        assert_eq!(t.consecutive_403(), 0);
        let stats = t.network_stats(0);
        assert_eq!(stats.total_requests, 5);
        assert_eq!(stats.error_count, 4);
    }
}
