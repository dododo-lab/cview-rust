//! `Tests/CViewPlayerTests/StreamCoordinatorTests.swift` · `StreamCoordinatorExtensionTests.swift` ·
//! `RecoverySignalTests.swift` · `ReconnectSuccessCancellationTests.swift` · `PlaybackNetworkRecoveryTests.swift`
//! (코디네이터 부분) 포팅 + 매니페스트 갱신·역할별 목표·타임시프트 배선 테스트.
//!
//! 실제 네트워크는 쓰지 않는다 — 매니페스트는 모의 다운로더, 프록시는 루프백에만 바인드한다.

use std::collections::VecDeque;
use std::ops::RangeInclusive;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use cview_core::models::live::StreamQuality;
use cview_core::models::player::{PlayerEngineType, PlayerError, PlayerPhase};
use cview_core::models::stream_proxy::{StreamProxyMode, StreamProxyRouteReason};
use cview_core::protocols::{PlayerEngineCapabilities, PlayerEngineProtocol};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::Notify;
use url::Url;

use super::*;
use crate::manifest::parser::Variant;
use crate::policy::{RecoveryDecision, RecoverySignal};
use crate::proxy::timeshift::{TeeDestination, TimeshiftBackend};
use crate::reconnection::ReconnectionConfig;

// MARK: - 모의 엔진

#[derive(Default)]
struct EngineState {
    play_count: usize,
    stop_count: usize,
    pause_count: usize,
    resume_count: usize,
    seek_positions: Vec<f64>,
    last_play_url: Option<Url>,
    last_volume: f32,
    rate: f32,
    is_playing: bool,
    current_time: f64,
    play_error: Option<PlayerError>,
    suspend_intent: bool,
    error_state: bool,
    // hls.js 면
    token: u64,
    loading_suspended: bool,
    correction_enabled: bool,
    owned_by_controller: bool,
    controller_target: f64,
    live_latency: f64,
    seek_to_latency: Vec<f64>,
    timeshift_urls: Vec<Url>,
}

/// `MockPlayerEngine` + hls.js 제어 면(선택).
struct MockEngine {
    state: Mutex<EngineState>,
    hls: bool,
    hold_next_play: AtomicBool,
    holding: AtomicBool,
    gate: Notify,
}

impl MockEngine {
    fn new() -> Arc<Self> {
        Self::build(false)
    }

    fn hls() -> Arc<Self> {
        Self::build(true)
    }

    fn build(hls: bool) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(EngineState {
                rate: 1.0,
                last_volume: 1.0,
                correction_enabled: true,
                ..Default::default()
            }),
            hls,
            hold_next_play: AtomicBool::new(false),
            holding: AtomicBool::new(false),
            gate: Notify::new(),
        })
    }

    fn s(&self) -> std::sync::MutexGuard<'_, EngineState> {
        self.state.lock().unwrap()
    }

    fn reset(&self) {
        let mut s = self.s();
        s.play_count = 0;
        s.stop_count = 0;
        s.pause_count = 0;
        s.resume_count = 0;
        s.last_play_url = None;
        s.suspend_intent = false;
        s.is_playing = false;
    }

    fn suspend_loading(&self) {
        let mut s = self.s();
        s.loading_suspended = true;
        s.token += 1;
    }

    fn resume_loading(&self) {
        self.s().loading_suspended = false;
    }

    fn shared(self: &Arc<Self>) -> SharedEngine {
        self.clone()
    }
}

#[async_trait]
impl PlayerEngineProtocol for MockEngine {
    fn is_playing(&self) -> bool {
        self.s().is_playing
    }
    fn current_time(&self) -> f64 {
        self.s().current_time
    }
    fn duration(&self) -> f64 {
        0.0
    }
    fn seekable_range(&self) -> Option<RangeInclusive<f64>> {
        None
    }
    fn rate(&self) -> f32 {
        self.s().rate
    }
    async fn play(&self, url: &Url) -> Result<(), PlayerError> {
        {
            let mut s = self.s();
            s.play_count += 1;
            s.last_play_url = Some(url.clone());
        }
        if self.hold_next_play.swap(false, Ordering::SeqCst) {
            let wait = self.gate.notified();
            self.holding.store(true, Ordering::SeqCst);
            wait.await;
            self.holding.store(false, Ordering::SeqCst);
        }
        let mut s = self.s();
        if let Some(e) = s.play_error.clone() {
            return Err(e);
        }
        s.is_playing = true;
        Ok(())
    }
    fn pause(&self) {
        let mut s = self.s();
        s.pause_count += 1;
        s.is_playing = false;
    }
    fn resume(&self) {
        let mut s = self.s();
        s.resume_count += 1;
        s.is_playing = true;
    }
    fn stop(&self) {
        let mut s = self.s();
        s.stop_count += 1;
        s.is_playing = false;
        s.current_time = 0.0;
    }
    fn seek(&self, position: f64) {
        let mut s = self.s();
        s.seek_positions.push(position);
        s.current_time = position;
    }
    fn set_rate(&self, rate: f32) {
        self.s().rate = rate;
    }
    fn set_volume(&self, volume: f32) {
        self.s().last_volume = volume;
    }
    fn is_in_error_state(&self) -> bool {
        self.s().error_state
    }
    fn reset_retries(&self) {}
    fn is_suspend_intent_active(&self) -> bool {
        self.s().suspend_intent
    }
    fn capabilities(&self) -> PlayerEngineCapabilities {
        PlayerEngineCapabilities::default()
    }
}

impl CoordinatedEngine for MockEngine {
    fn hls(&self) -> Option<&dyn HlsEngineControl> {
        self.hls.then_some(self as &dyn HlsEngineControl)
    }
}

#[async_trait]
impl HlsEngineControl for MockEngine {
    fn invalidate_latency_control_session(&self) -> u64 {
        let mut s = self.s();
        s.token += 1;
        s.token
    }
    fn begin_latency_control_session(&self) -> u64 {
        let mut s = self.s();
        s.token += 1;
        s.token
    }
    fn is_latency_control_session_current(&self, generation: u64) -> bool {
        let s = self.s();
        s.token == generation && !s.loading_suspended
    }
    fn set_rate_for_latency(&self, rate: f32, generation: u64) {
        let mut s = self.s();
        if s.token == generation {
            s.rate = rate;
        }
    }
    async fn set_rate_awaiting_application(&self, rate: f32, generation: u64) -> bool {
        let mut s = self.s();
        if s.token != generation {
            return false;
        }
        s.rate = rate;
        true
    }
    fn seek_to_latency(&self, target_seconds: f64, generation: u64) {
        let mut s = self.s();
        if s.token == generation {
            s.seek_to_latency.push(target_seconds);
        }
    }
    fn set_latency_owned_by_controller(&self, owned: bool, generation: u64) {
        let mut s = self.s();
        if s.token == generation {
            s.owned_by_controller = owned;
        }
    }
    fn set_controller_target_latency(&self, seconds: f64, generation: u64) {
        let mut s = self.s();
        if s.token == generation {
            s.controller_target = seconds;
        }
    }
    fn set_live_latency_correction_enabled(&self, enabled: bool) {
        self.s().correction_enabled = enabled;
    }
    fn is_live_latency_correction_enabled(&self) -> bool {
        self.s().correction_enabled
    }
    fn is_loading_suspended(&self) -> bool {
        self.s().loading_suspended
    }
    fn live_latency_seconds(&self) -> f64 {
        self.s().live_latency
    }
    fn seek_timeshift(&self, seconds_behind: f64) {
        self.s().seek_positions.push(-seconds_behind);
    }
    async fn play_timeshift(&self, url: &Url) -> Result<(), PlayerError> {
        let mut s = self.s();
        s.timeshift_urls.push(url.clone());
        s.is_playing = true;
        Ok(())
    }
}

// MARK: - 모의 다운로더

#[derive(Default)]
struct MockFetcher {
    responses: Mutex<VecDeque<Result<FetchedManifest, String>>>,
    fallback: Mutex<Option<Result<FetchedManifest, String>>>,
    calls: AtomicUsize,
}

impl MockFetcher {
    fn always(result: Result<&str, &str>) -> Arc<Self> {
        let f = Self::default();
        *f.fallback.lock().unwrap() = Some(
            result
                .map(|b| FetchedManifest {
                    body: b.to_string(),
                    final_url: None,
                })
                .map_err(str::to_string),
        );
        Arc::new(f)
    }

    fn push(&self, body: &str) {
        self.responses
            .lock()
            .unwrap()
            .push_back(Ok(FetchedManifest {
                body: body.to_string(),
                final_url: None,
            }));
    }
}

#[async_trait]
impl ManifestFetcher for MockFetcher {
    async fn fetch(
        &self,
        _url: &Url,
        _timeout: Option<Duration>,
    ) -> Result<FetchedManifest, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(r) = self.responses.lock().unwrap().pop_front() {
            return r;
        }
        self.fallback
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| Err("offline".into()))
    }
}

// MARK: - 도우미

fn test_url() -> Url {
    Url::parse("https://example.com/live/stream.m3u8").unwrap()
}

fn route_url() -> Url {
    Url::parse("https://video.pstatic.net/live/route.m3u8").unwrap()
}

fn config(enable_low_latency: bool) -> Configuration {
    Configuration {
        enable_low_latency,
        ..Configuration::new("test-channel")
    }
}

fn coordinator_with(config: Configuration, fetcher: Arc<MockFetcher>) -> StreamCoordinator {
    StreamCoordinator::with_fetcher(config, PlaybackKeyAccess::anonymous(), fetcher)
}

fn coordinator(enable_low_latency: bool) -> StreamCoordinator {
    coordinator_with(config(enable_low_latency), Arc::new(MockFetcher::default()))
}

fn drain(rx: &mut UnboundedReceiver<StreamEvent>) -> Vec<StreamEvent> {
    let mut out = Vec::new();
    while let Ok(e) = rx.try_recv() {
        out.push(e);
    }
    out
}

fn variant(
    bandwidth: i64,
    resolution: &str,
    frame_rate: Option<f64>,
    name: Option<&str>,
) -> Variant {
    Variant {
        bandwidth,
        average_bandwidth: None,
        resolution: resolution.into(),
        codecs: Some("avc1.4d401f".into()),
        frame_rate,
        uri: Url::parse(&format!("https://cdn.example.com/live/v{bandwidth}.m3u8")).unwrap(),
        name: name.map(str::to_string),
    }
}

const MASTER: &str = "#EXTM3U\n\
#EXT-X-STREAM-INF:BANDWIDTH=8000000,RESOLUTION=1920x1080,FRAME-RATE=60.000\n\
1080p/chunklist.m3u8?token=new\n\
#EXT-X-STREAM-INF:BANDWIDTH=2500000,RESOLUTION=1280x720\n\
720p/chunklist.m3u8?token=new\n\
#EXT-X-STREAM-INF:BANDWIDTH=128000,CODECS=\"mp4a.40.2\"\n\
audio/chunklist.m3u8\n";

async fn settle() {
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
}

// MARK: - 초기화

#[tokio::test]
async fn initial_state_is_idle() {
    let c = coordinator(false);
    assert_eq!(c.phase(), StreamPhase::Idle);
    assert!(c.current_quality().is_none());
    assert!(!c.is_playing());
    assert_eq!(c.uptime(), 0.0);
    assert!(c.available_qualities().is_empty());
}

#[tokio::test]
async fn time_shift_suspends_low_latency_sync_gate() {
    let c = coordinator(true);
    assert!(c.should_run_low_latency_sync());
    c.set_time_shift_active(true).await;
    assert!(!c.should_run_low_latency_sync());
    c.set_time_shift_active(false).await;
    assert!(c.should_run_low_latency_sync());
}

#[tokio::test]
async fn time_shift_gate_also_stops_hlsjs_self_correction() {
    let c = coordinator(true);
    let engine = MockEngine::hls();
    c.set_player_engine(engine.shared()).await;
    assert!(engine.is_live_latency_correction_enabled());
    c.set_time_shift_active(true).await;
    assert!(!engine.is_live_latency_correction_enabled());
    // 엔진 교체 중에도 타임머신 상태가 새 인스턴스로 전달돼야 한다.
    let replacement = MockEngine::hls();
    c.set_player_engine(replacement.shared()).await;
    assert!(!replacement.is_live_latency_correction_enabled());
    c.set_time_shift_active(false).await;
    assert!(replacement.is_live_latency_correction_enabled());
}

#[tokio::test]
async fn enter_timeshift_requires_active_phase() {
    let c = coordinator(true);
    let engine = MockEngine::hls();
    c.set_player_engine(engine.shared()).await;
    assert!(!c.enter_timeshift(60.0).await);
    assert!(!c.is_local_timeshift_active());
    assert!(
        engine.is_live_latency_correction_enabled(),
        "게이트를 올린 채 남기면 안 된다"
    );
}

#[tokio::test]
async fn live_reload_ends_time_shift() {
    let c = coordinator(true);
    let engine = MockEngine::hls();
    c.set_player_engine(engine.shared()).await;
    let mut events = c.events();
    c.set_time_shift_active(true).await;
    assert!(!engine.is_live_latency_correction_enabled());
    assert!(c.end_time_shift_after_live_reload("재연결"));
    assert!(c.should_run_low_latency_sync());
    assert!(engine.is_live_latency_correction_enabled());
    assert!(drain(&mut events).contains(&StreamEvent::TimeShiftEnded));
    assert!(!c.end_time_shift_after_live_reload("화질 전환"));
    assert!(c.should_run_low_latency_sync());
}

#[tokio::test]
async fn new_low_latency_session_resets_inherited_playback_rate() {
    let c = coordinator(true);
    let engine = MockEngine::hls();
    c.set_player_engine(engine.shared()).await;
    engine.set_rate(1.03);
    assert_eq!(engine.rate(), 1.03);
    c.update_phase(StreamPhase::Playing);
    c.start_low_latency_sync().await;
    assert_eq!(
        engine.rate(),
        1.0,
        "새 PID 토큰이 이전 세션의 catch-up 배율을 초기화하지 못했다"
    );
    assert!(engine.s().owned_by_controller);
    assert!(c.low_latency_controller().unwrap().has_sync_task());
    c.stop_latency_subsystems();
    assert!(!c.low_latency_controller().unwrap().has_sync_task());
}

#[tokio::test]
async fn collapsed_suspend_resume_repairs_latency_control_token() {
    let c = coordinator(true);
    let engine = MockEngine::hls();
    c.set_player_engine(engine.shared()).await;
    c.update_phase(StreamPhase::Playing);
    c.start_low_latency_sync().await;
    // 코디네이터가 suspended=true 를 한 번도 관측하지 못한 빠른 왕복.
    engine.suspend_loading();
    engine.resume_loading();
    engine.set_rate(1.03);
    c.reconcile_network_activity().await;
    assert_eq!(
        engine.rate(),
        1.0,
        "최종 suspended 플래그가 같다는 이유로 폐기된 PID 토큰을 복구하지 않았다"
    );
    c.stop_latency_subsystems();
}

#[tokio::test]
async fn engine_self_suspension_stops_pid_and_resume_restarts_it() {
    let c = coordinator(true);
    let engine = MockEngine::hls();
    c.set_player_engine(engine.shared()).await;
    c.update_phase(StreamPhase::Playing);
    c.start_low_latency_sync().await;
    let controller = c.low_latency_controller().unwrap().clone();
    assert!(controller.has_sync_task());
    engine.suspend_loading();
    c.reconcile_network_activity().await;
    assert!(!controller.has_sync_task(), "정지된 패인에서 PID 가 돌았다");
    engine.resume_loading();
    c.reconcile_network_activity().await;
    assert!(controller.has_sync_task());
    c.stop_latency_subsystems();
}

// MARK: - 수명

#[tokio::test]
async fn start_stream_waits_for_engine_playing() {
    let c = coordinator(false);
    let engine = MockEngine::new();
    c.set_player_engine(engine.shared()).await;
    c.start_stream(test_url()).await.unwrap();
    assert_eq!(c.phase(), StreamPhase::Connecting);
    assert_eq!(engine.s().play_count, 1);
    assert_eq!(
        engine.s().last_play_url,
        Some(test_url()),
        "프록시 대상 밖은 직접 재생"
    );
    c.report_engine_playing().await;
    assert_eq!(c.phase(), StreamPhase::Playing);
    assert!(c.is_playing());
    assert!(c.uptime() >= 0.0);
}

#[tokio::test]
async fn stop_stream_goes_idle_and_resets_uptime() {
    let c = coordinator(false);
    let engine = MockEngine::new();
    c.set_player_engine(engine.shared()).await;
    c.start_stream(test_url()).await.unwrap();
    assert!(c.uptime() >= 0.0);
    c.stop_stream().await;
    assert_eq!(c.phase(), StreamPhase::Idle);
    assert_eq!(engine.s().stop_count, 1);
    assert_eq!(c.uptime(), 0.0);
}

#[tokio::test]
async fn pause_and_resume_wait_for_engine_playing() {
    let c = coordinator(false);
    let engine = MockEngine::new();
    c.set_player_engine(engine.shared()).await;
    c.start_stream(test_url()).await.unwrap();
    c.pause().await;
    assert_eq!(c.phase(), StreamPhase::Paused);
    assert_eq!(engine.s().pause_count, 1);
    c.resume().await;
    assert_eq!(c.phase(), StreamPhase::Paused);
    assert_eq!(engine.s().resume_count, 1);
    c.report_engine_playing().await;
    assert_eq!(c.phase(), StreamPhase::Playing);
}

#[tokio::test]
async fn late_first_frame_cannot_resurrect_stopped_stream() {
    let c = coordinator(false);
    let engine = MockEngine::new();
    c.set_player_engine(engine.shared()).await;
    c.start_stream(test_url()).await.unwrap();
    c.stop_stream().await;
    c.report_engine_playing().await;
    assert_eq!(c.phase(), StreamPhase::Idle);
}

#[tokio::test]
async fn failing_engine_goes_to_error_phase() {
    let c = coordinator(false);
    let engine = MockEngine::new();
    engine.s().play_error = Some(PlayerError::StreamNotFound);
    c.set_player_engine(engine.shared()).await;
    let err = c.start_stream(test_url()).await.unwrap_err();
    assert_eq!(
        err,
        StreamCoordinatorError::Player(PlayerError::StreamNotFound)
    );
    assert!(matches!(c.phase(), StreamPhase::Error(_)));
}

#[tokio::test]
async fn missing_engine_is_an_error_phase_not_a_throw() {
    let c = coordinator(false);
    c.start_stream(test_url()).await.unwrap();
    assert_eq!(
        c.phase(),
        StreamPhase::Error("Player engine unavailable".into())
    );
}

#[tokio::test]
async fn start_loads_manifest_info_in_background() {
    let fetcher = MockFetcher::always(Ok(MASTER));
    let c = coordinator_with(config(false), fetcher.clone());
    let engine = MockEngine::new();
    c.set_player_engine(engine.shared()).await;
    let mut events = c.events();
    c.start_stream(test_url()).await.unwrap();
    settle().await;
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 1);
    let names: Vec<_> = c
        .available_qualities()
        .into_iter()
        .map(|q| q.name)
        .collect();
    assert_eq!(names, vec!["1080p", "720p"], "오디오 전용 variant 는 뺀다");
    assert_eq!(c.current_quality().unwrap().bandwidth, 8_000_000);
    assert!(drain(&mut events)
        .iter()
        .any(|e| matches!(e, StreamEvent::QualitySelected(q) if q.name == "1080p")));
}

// MARK: - 이벤트

#[tokio::test]
async fn start_and_stop_emit_events() {
    let c = coordinator(false);
    let engine = MockEngine::new();
    c.set_player_engine(engine.shared()).await;
    let mut events = c.events();
    c.start_stream(test_url()).await.unwrap();
    c.report_engine_playing().await;
    c.stop_stream().await;
    let all = drain(&mut events);
    let phases: Vec<_> = all
        .iter()
        .filter_map(|e| match e {
            StreamEvent::PhaseChanged(p) => Some(p.clone()),
            _ => None,
        })
        .collect();
    assert!(phases.contains(&StreamPhase::Connecting));
    assert!(phases.contains(&StreamPhase::Playing));
    assert_eq!(all.last(), Some(&StreamEvent::Stopped));
}

// MARK: - 화질

#[tokio::test]
async fn switch_quality_by_bandwidth_not_found() {
    let c = coordinator(false);
    c.set_player_engine(MockEngine::new().shared()).await;
    assert_eq!(
        c.switch_quality_by_bandwidth(9_999_999).await,
        Err(StreamCoordinatorError::QualityNotFound)
    );
}

#[tokio::test]
async fn switch_quality_pins_variant_and_skips_same_variant() {
    let c = coordinator(false);
    let engine = MockEngine::new();
    c.set_player_engine(engine.shared()).await;
    let master = HlsManifestParser::new()
        .parse_master_playlist(MASTER, &test_url())
        .unwrap();
    c.set_prefetched_manifest(master);
    c.start_stream(test_url()).await.unwrap();
    let mut events = c.events();
    c.switch_quality_by_bandwidth(2_500_000).await.unwrap();
    assert_eq!(engine.s().play_count, 2);
    assert!(engine
        .s()
        .last_play_url
        .as_ref()
        .unwrap()
        .path()
        .ends_with("720p/chunklist.m3u8"));
    assert!(drain(&mut events)
        .iter()
        .any(|e| matches!(e, StreamEvent::QualityChanged(q) if q.name == "720p")));
    // 같은 화질 재선택은 재로드하지 않는다.
    c.switch_quality_by_bandwidth(2_500_000).await.unwrap();
    assert_eq!(engine.s().play_count, 2);
    // 고정한 화질로 재연결한다.
    let pinned = c.reconnect_playback_url(&test_url());
    assert!(pinned.path().ends_with("720p/chunklist.m3u8"));
}

#[test]
fn quality_info_basics() {
    let info = StreamQualityInfo::new("1080p", "1920x1080", 5_000_000);
    assert_eq!(info.id(), "1080p");
    assert_eq!(info.resolution, "1920x1080");
    let a = StreamQualityInfo::new("720p", "1280x720", 3_000_000);
    assert_eq!(a, StreamQualityInfo::new("720p", "1280x720", 3_000_000));
    assert_ne!(a, info);
}

#[test]
fn select_1080p60_priorities() {
    let v = |bw, res, fr| variant(bw, res, fr, None);
    let pick = |vs: Vec<Variant>| select_1080p60_variant(&vs).cloned();
    let r = pick(vec![
        v(8_000_000, "1920x1080", Some(60.0)),
        v(4_000_000, "1280x720", Some(30.0)),
        v(2_000_000, "854x480", None),
    ])
    .unwrap();
    assert_eq!(
        (r.resolution.as_str(), r.frame_rate),
        ("1920x1080", Some(60.0))
    );
    let r = pick(vec![
        v(8_000_000, "1920x1080", Some(59.94)),
        v(4_000_000, "1280x720", None),
    ])
    .unwrap();
    assert_eq!(r.frame_rate, Some(59.94), "59fps 도 60fps 범주");
    let r = pick(vec![
        v(6_000_000, "1920x1080", Some(30.0)),
        v(4_000_000, "1280x720", Some(60.0)),
        v(2_000_000, "854x480", None),
    ])
    .unwrap();
    assert_eq!(r.frame_rate, Some(30.0), "2순위: 1080p any fps");
    let r = pick(vec![
        v(4_000_000, "1280x720", Some(60.0)),
        v(2_000_000, "854x480", None),
        v(1_000_000, "640x360", None),
    ])
    .unwrap();
    assert_eq!(r.bandwidth, 4_000_000, "3순위: 최고 bandwidth");
    assert!(pick(vec![]).is_none());
    let r = pick(vec![
        v(6_000_000, "1920x1080", Some(60.0)),
        v(8_000_000, "1920x1080", Some(60.0)),
        v(4_000_000, "1280x720", None),
    ])
    .unwrap();
    assert_eq!(r.bandwidth, 8_000_000);
    let r = pick(vec![
        v(6_000_000, "1920x1080", None),
        v(4_000_000, "1280x720", Some(60.0)),
    ])
    .unwrap();
    assert_eq!(r.bandwidth, 6_000_000);
}

#[tokio::test]
async fn select_initial_quality_rules() {
    let make = |preferred| {
        coordinator_with(
            Configuration {
                enable_low_latency: false,
                preferred_quality: preferred,
                force_highest_quality: false,
                ..Configuration::new("test")
            },
            Arc::new(MockFetcher::default()),
        )
    };
    let master = |vs: Vec<Variant>| MasterPlaylist {
        variants: vs,
        uri: test_url(),
    };
    let named = master(vec![
        variant(8_000_000, "1920x1080", None, Some("원본 (1080p)")),
        variant(4_000_000, "1280x720", None, Some("고화질 (720p)")),
        variant(2_000_000, "854x480", None, Some("중화질 (480p)")),
    ]);
    assert_eq!(
        make(Some(StreamQuality::High))
            .select_initial_quality(&named)
            .resolution,
        "1280x720"
    );
    let three = master(vec![
        variant(8_000_000, "1920x1080", None, None),
        variant(4_000_000, "1280x720", None, None),
        variant(2_000_000, "854x480", None, None),
    ]);
    assert_eq!(
        make(Some(StreamQuality::Low))
            .select_initial_quality(&three)
            .resolution,
        "1280x720",
        "매칭 실패 → 중간"
    );
    let four = master(vec![
        variant(8_000_000, "1920x1080", None, None),
        variant(4_000_000, "1280x720", None, None),
        variant(2_000_000, "854x480", None, None),
        variant(1_000_000, "640x360", None, None),
    ]);
    assert_eq!(
        make(None).select_initial_quality(&four).resolution,
        "854x480"
    );
    let one = master(vec![variant(4_000_000, "1280x720", None, None)]);
    assert_eq!(
        make(None).select_initial_quality(&one).resolution,
        "1280x720"
    );
    // 원본 선호(기본 force_highest_quality)면 1080p60 우선.
    let source = coordinator(false);
    assert_eq!(source.select_initial_quality(&four).resolution, "1920x1080");
}

#[test]
fn effective_quality_policy_prefers_explicit_policy() {
    let mut cfg = Configuration::new("c");
    assert_eq!(
        cfg.effective_quality_policy(),
        QualityPolicyMode::from_legacy_force_highest_quality(true)
    );
    cfg.force_highest_quality = false;
    assert_eq!(
        cfg.effective_quality_policy(),
        QualityPolicyMode::from_legacy_force_highest_quality(false)
    );
    let explicit = QualityPolicyMode::from_legacy_force_highest_quality(true);
    cfg.quality_policy = Some(explicit);
    assert_eq!(cfg.effective_quality_policy(), explicit);
}

#[test]
fn video_variant_filter() {
    let mut audio = variant(128_000, "", None, None);
    audio.codecs = Some("mp4a.40.2".into());
    assert!(!is_video_variant(&audio));
    audio.codecs = Some("avc1.64001f,mp4a.40.2".into());
    assert!(is_video_variant(&audio));
    assert!(is_video_variant(&variant(1, "640X360", None, None)));
}

// MARK: - phase·버퍼

#[test]
fn phases_equatable_and_resource_requirements() {
    assert_eq!(
        StreamPhase::Error("x".into()),
        StreamPhase::Error("x".into())
    );
    assert_ne!(
        StreamPhase::Error("x".into()),
        StreamPhase::Error("y".into())
    );
    assert_ne!(StreamPhase::Idle, StreamPhase::Playing);
    for p in [
        StreamPhase::Connecting,
        StreamPhase::Buffering,
        StreamPhase::Reconnecting,
        StreamPhase::Playing,
        StreamPhase::LoadingInfo,
        StreamPhase::LoadingManifest,
    ] {
        assert!(p.requires_playback_resources(), "{p:?}");
    }
    for p in [
        StreamPhase::Paused,
        StreamPhase::StreamEnded,
        StreamPhase::Idle,
        StreamPhase::Error("x".into()),
    ] {
        assert!(!p.requires_playback_resources(), "{p:?}");
    }
}

#[test]
fn buffer_health_approximately_equals() {
    let base = BufferHealth::new(0.8, 1.0, true);
    assert!(!BufferHealth::new(0.3, 1.0, false).is_healthy);
    assert!(base.approximately_equals(&base));
    assert!(base.approximately_equals(&BufferHealth::new(0.804, 1.0, true)));
    assert!(!base.approximately_equals(&BufferHealth::new(0.81, 1.0, true)));
    assert!(!base.approximately_equals(&BufferHealth::new(0.8, 1.0, false)));
    let mut unmeasurable = base;
    unmeasurable.is_measurable = false;
    assert!(!base.approximately_equals(&unmeasurable));
}

#[tokio::test]
async fn buffer_health_publish_is_deduplicated() {
    let c = coordinator(false);
    let mut events = c.events();
    c.publish_buffer_health(0.8);
    c.publish_buffer_health(0.802);
    c.publish_buffer_health(1.7);
    let updates: Vec<_> = drain(&mut events)
        .into_iter()
        .filter_map(|e| match e {
            StreamEvent::BufferUpdate(h) => Some(h.current_level),
            _ => None,
        })
        .collect();
    assert_eq!(updates, vec![0.8, 1.0], "노이즈는 생략, 1 초과는 잘라낸다");
}

// MARK: - 복구 supervisor

#[test]
fn recovery_signal_mapping() {
    assert_eq!(
        decide_recovery(&RecoverySignal::StreamEndedConfirmed),
        RecoveryDecision::MarkEnded
    );
    for count in 1..5 {
        assert!(matches!(
            decide_recovery(&RecoverySignal::ManifestRefreshFailure { count }),
            RecoveryDecision::Ignore { .. }
        ));
    }
    for count in [5, 6, 100] {
        assert_eq!(
            decide_recovery(&RecoverySignal::ManifestRefreshFailure { count }),
            RecoveryDecision::Reconnect
        );
    }
    assert_eq!(
        decide_recovery(&RecoverySignal::UpstreamAuthFailure { status: 403 }),
        RecoveryDecision::Reconnect
    );
    assert_eq!(
        decide_recovery(&RecoverySignal::EngineError {
            engine: PlayerEngineType::Hlsjs,
            detail: "x".into()
        }),
        RecoveryDecision::Reconnect
    );
    assert_eq!(
        decide_recovery(&RecoverySignal::PlaybackStall {
            source: "watchdog".into()
        }),
        RecoveryDecision::Reconnect
    );
}

#[tokio::test]
async fn stream_ended_signal_marks_ended() {
    let c = coordinator(false);
    let engine = MockEngine::new();
    c.set_player_engine(engine.shared()).await;
    c.start_stream(test_url()).await.unwrap();
    let mut events = c.events();
    let decision = c
        .handle_recovery_signal(RecoverySignal::StreamEndedConfirmed)
        .await;
    assert_eq!(decision, RecoveryDecision::MarkEnded);
    assert_eq!(c.phase(), StreamPhase::StreamEnded);
    assert_eq!(engine.s().stop_count, 1);
    assert!(drain(&mut events).contains(&StreamEvent::StreamEnded));
}

// MARK: - 매니페스트 갱신

#[tokio::test]
async fn refresh_keeps_pinned_quality_and_issues_lease() {
    let fetcher = Arc::new(MockFetcher::default());
    let c = coordinator_with(config(false), fetcher.clone());
    c.set_prefetched_manifest(
        HlsManifestParser::new()
            .parse_master_playlist(MASTER, &test_url())
            .unwrap(),
    );
    c.set_player_engine(MockEngine::new().shared()).await;
    c.start_stream(test_url()).await.unwrap();
    // 고정 없음 → 1080p 로 갱신.
    fetcher.push(&MASTER.replace("token=new", "token=t2"));
    c.refresh_master_manifest().await;
    let lease = c.current_manifest_lease().unwrap();
    assert_eq!(lease.generation, 1);
    assert!(lease
        .selected_variant_url
        .as_str()
        .contains("1080p/chunklist.m3u8?token=t2"));
    // 720p 를 고정하면 갱신이 그 화질의 새 URL 을 유지한다.
    c.switch_quality_by_bandwidth(2_500_000).await.unwrap();
    fetcher.push(&MASTER.replace("token=new", "token=t3"));
    c.refresh_master_manifest().await;
    let lease = c.current_manifest_lease().unwrap();
    assert_eq!(lease.generation, 2);
    assert!(lease
        .selected_variant_url
        .as_str()
        .contains("720p/chunklist.m3u8?token=t3"));
    assert_eq!(c.current_quality().unwrap().name, "720p");
    assert_eq!(
        c.reconnect_playback_url(&test_url()),
        lease.selected_variant_url,
        "재연결은 고정 화질의 최신 URL"
    );
}

#[tokio::test]
async fn refresh_failures_escalate_to_reconnect_on_fifth() {
    let c = coordinator_with(config(false), MockFetcher::always(Err("boom")));
    let engine = MockEngine::new();
    c.set_player_engine(engine.shared()).await;
    c.start_stream(test_url()).await.unwrap();
    c.report_engine_playing().await;
    for _ in 0..4 {
        c.refresh_master_manifest().await;
        assert_eq!(c.phase(), StreamPhase::Playing);
    }
    c.refresh_master_manifest().await;
    assert_eq!(c.phase(), StreamPhase::Reconnecting);
    assert_eq!(c.st().manifest_refresh_fail_count, 0, "승격 뒤 카운터 리셋");
    c.stop_stream().await;
}

#[tokio::test]
async fn refresh_is_skipped_in_error_phase() {
    let fetcher = MockFetcher::always(Ok(MASTER));
    let c = coordinator_with(config(false), fetcher.clone());
    c.st().stream_url = Some(test_url());
    c.update_phase(StreamPhase::Error("x".into()));
    c.refresh_master_manifest().await;
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn variant_base_url_follows_only_allowed_redirects() {
    let request = Url::parse("https://a.navercdn.com/live/master.m3u8").unwrap();
    let moved = Url::parse("https://b.navercdn.com/x/master.m3u8").unwrap();
    let outside = Url::parse("https://evil.example/x/master.m3u8").unwrap();
    assert_eq!(variant_base_url(&request, Some(&moved)), moved);
    assert_eq!(variant_base_url(&request, Some(&outside)), request);
    assert_eq!(variant_base_url(&request, None), request);
}

// MARK: - 배경 복귀

#[tokio::test]
async fn background_recovery_respects_suspend_intent() {
    let c = coordinator(false);
    let engine = MockEngine::new();
    c.set_player_engine(engine.shared()).await;
    c.update_phase(StreamPhase::Playing);
    {
        let mut s = engine.s();
        s.is_playing = false;
        s.suspend_intent = true;
    }
    c.recover_from_background();
    assert_eq!(
        engine.s().resume_count,
        0,
        "의도적으로 정지된 패인에 resume 을 걸었다"
    );
    // 대조군 — 정지 의도가 없으면 복구를 시도한다.
    engine.s().suspend_intent = false;
    c.recover_from_background();
    assert_eq!(engine.s().resume_count, 1);
    // 정상 재생 중이면 resume 을 부르지 않는다.
    c.recover_from_background();
    assert_eq!(engine.s().resume_count, 1);
}

#[tokio::test(start_paused = true)]
async fn background_recovery_reconnects_when_resume_does_not_take() {
    let c = coordinator_with(config(false), MockFetcher::always(Err("offline")));
    let engine = MockEngine::new();
    c.set_player_engine(engine.shared()).await;
    c.st().stream_url = Some(test_url());
    c.update_phase(StreamPhase::Playing);
    c.recover_from_background();
    // resume 이 먹지 않은 엔진(다시 멈춤)
    engine.s().is_playing = false;
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert_eq!(c.phase(), StreamPhase::Reconnecting);
    c.stop_stream().await;
}

#[tokio::test]
async fn background_recovery_skips_refresh_when_lease_is_fresh() {
    let fetcher = MockFetcher::always(Ok(MASTER));
    let c = coordinator_with(config(false), fetcher.clone());
    let engine = MockEngine::new();
    c.set_player_engine(engine.shared()).await;
    c.st().stream_url = Some(test_url());
    c.update_phase(StreamPhase::Playing);
    engine.s().is_playing = true;
    c.refresh_master_manifest().await;
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 1);
    c.recover_from_background();
    settle().await;
    assert_eq!(
        fetcher.calls.load(Ordering::SeqCst),
        1,
        "TTL 안의 갱신은 생략"
    );
}

// MARK: - 일시정지 의도·재연결 종료

fn prepare_reconnect_intent(c: &StreamCoordinator) {
    c.st().stream_url = Some(test_url());
    c.update_phase(StreamPhase::Reconnecting);
}

#[tokio::test]
async fn pause_rejects_late_playback_and_reconnect() {
    let c = coordinator(false);
    let engine = MockEngine::new();
    c.set_player_engine(engine.shared()).await;
    prepare_reconnect_intent(&c);
    c.pause().await;
    c.report_engine_playing().await;
    c.trigger_reconnect("late callback", false);
    c.perform_reconnect_attempt(&test_url()).await;
    assert_eq!(c.phase(), StreamPhase::Paused);
    assert_eq!(engine.s().play_count, 0);
    c.resume().await;
    c.report_engine_playing().await;
    assert_eq!(c.phase(), StreamPhase::Playing);
    assert_eq!(engine.s().resume_count, 1);
    c.stop_stream().await;
}

#[tokio::test]
async fn exhaustion_stops_engine() {
    let c = coordinator(false);
    let engine = MockEngine::new();
    c.set_player_engine(engine.shared()).await;
    prepare_reconnect_intent(&c);
    engine.s().is_playing = true;
    let mut events = c.events();
    c.handle_reconnect_exhausted();
    assert_eq!(engine.s().stop_count, 1);
    assert!(!engine.is_playing());
    assert_eq!(
        c.phase(),
        StreamPhase::Error("재연결 최대 횟수 초과".into())
    );
    assert!(drain(&mut events).contains(&StreamEvent::Error("재연결 최대 횟수 초과".into())));
}

#[tokio::test]
async fn late_exhaustion_does_not_stop_success() {
    let c = coordinator(false);
    let engine = MockEngine::new();
    c.set_player_engine(engine.shared()).await;
    prepare_reconnect_intent(&c);
    c.report_engine_playing().await;
    c.handle_reconnect_exhausted();
    assert_eq!(engine.s().stop_count, 0);
    assert_eq!(c.phase(), StreamPhase::Playing);
    c.stop_stream().await;
}

#[tokio::test]
async fn reconnect_after_stop_does_not_revive_playback() {
    let c = coordinator(false);
    let engine = MockEngine::new();
    c.set_player_engine(engine.shared()).await;
    c.start_stream(test_url()).await.unwrap();
    c.stop_stream().await;
    assert_eq!(c.phase(), StreamPhase::Idle);
    engine.reset();
    c.perform_reconnect_attempt(&test_url()).await;
    assert_eq!(engine.s().play_count, 0, "정리된 세션에 재생을 다시 걸었다");
    assert_eq!(engine.s().stop_count, 0);
}

#[tokio::test]
async fn reconnect_on_live_session_restarts() {
    let c = coordinator(false);
    let engine = MockEngine::new();
    c.set_player_engine(engine.shared()).await;
    c.start_stream(test_url()).await.unwrap();
    // `.buffering` 은 살아 있는 세션이면서 8초 대기 루프에 걸리지 않는다.
    c.update_phase(StreamPhase::Buffering);
    engine.reset();
    c.perform_reconnect_attempt(&test_url()).await;
    assert_eq!(engine.s().stop_count, 1);
    assert_eq!(engine.s().play_count, 1);
}

#[tokio::test(start_paused = true)]
async fn reconnect_attempt_waits_then_stops_engine_if_session_was_cleared() {
    let c = coordinator(false);
    let engine = MockEngine::new();
    c.set_player_engine(engine.shared()).await;
    prepare_reconnect_intent(&c);
    let attempt = {
        let c = c.clone();
        tokio::spawn(async move { c.perform_reconnect_attempt(&test_url()).await })
    };
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(!attempt.is_finished(), "첫 프레임을 기다려야 한다");
    c.stop_stream().await;
    let stops = engine.s().stop_count;
    attempt.await.unwrap();
    assert_eq!(
        engine.s().stop_count,
        stops + 1,
        "되살린 엔진을 세우지 않았다"
    );
}

#[tokio::test]
async fn success_while_reconnect_play_is_pending_keeps_engine() {
    let c = coordinator(false);
    let engine = MockEngine::new();
    c.set_player_engine(engine.shared()).await;
    c.start_stream(test_url()).await.unwrap();
    c.report_engine_playing().await;
    c.update_phase(StreamPhase::Reconnecting);
    engine.hold_next_play.store(true, Ordering::SeqCst);
    let attempt = {
        let c = c.clone();
        tokio::spawn(async move { c.perform_reconnect_attempt(&test_url()).await })
    };
    for _ in 0..1000 {
        if engine.holding.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    assert!(engine.holding.load(Ordering::SeqCst));
    let stops_before = engine.s().stop_count;
    c.report_engine_playing().await;
    engine.gate.notify_one();
    attempt.await.unwrap();
    assert_eq!(c.phase(), StreamPhase::Playing);
    assert_eq!(
        engine.s().stop_count,
        stops_before,
        "성공 직후 되살아난 엔진을 세웠다"
    );
    c.stop_stream().await;
}

#[tokio::test]
async fn reconnect_keeps_pinned_variant() {
    let c = coordinator(false);
    let fallback = Url::parse("https://example.com/live/playlist.m3u8").unwrap();
    let p720 = Url::parse("https://example.com/live/720p/chunklist.m3u8?token=new").unwrap();
    assert_eq!(c.reconnect_playback_url(&fallback), fallback);
    let mut v = variant(2_500_000, "1280x720", None, Some("720p"));
    v.uri = p720.clone();
    {
        let mut s = c.st();
        s.user_selected_variant = Some(v.clone());
        s.current_variant_url = Some(p720.clone());
    }
    assert_eq!(c.reconnect_playback_url(&fallback), p720);
    c.st().current_variant_url =
        Some(Url::parse("https://example.com/live/1080p/chunklist.m3u8").unwrap());
    assert_eq!(c.reconnect_playback_url(&fallback), fallback);
}

#[tokio::test(start_paused = true)]
async fn trigger_reconnect_runs_attempts_until_first_frame() {
    let c = coordinator_with(config(false), MockFetcher::always(Ok(MASTER)));
    let engine = MockEngine::new();
    c.set_player_engine(engine.shared()).await;
    c.start_stream(test_url()).await.unwrap();
    c.report_engine_playing().await;
    engine.reset();
    c.trigger_reconnect("test", false);
    assert_eq!(c.phase(), StreamPhase::Reconnecting);
    // 쿨다운 안의 이중 트리거는 무시된다.
    c.trigger_reconnect("duplicate", false);
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(engine.s().play_count, 1, "백오프 0.5초 뒤 첫 시도");
    assert!(c.reconnection_handler().is_reconnecting());
    c.report_engine_playing().await;
    assert_eq!(c.phase(), StreamPhase::Playing);
    assert!(
        !c.reconnection_handler().is_reconnecting(),
        "첫 프레임이 루프를 끝낸다"
    );
    tokio::time::sleep(Duration::from_secs(30)).await;
    assert_eq!(engine.s().play_count, 1);
    c.stop_stream().await;
}

#[tokio::test(start_paused = true)]
async fn trigger_reconnect_checks_stream_ended_first() {
    let c = coordinator(false);
    let engine = MockEngine::new();
    c.set_player_engine(engine.shared()).await;
    c.start_stream(test_url()).await.unwrap();
    c.report_engine_playing().await;
    c.set_check_stream_ended_callback(Arc::new(|| Box::pin(async { true })));
    let mut events = c.events();
    c.trigger_reconnect("stall", false);
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert_eq!(c.phase(), StreamPhase::StreamEnded);
    assert!(drain(&mut events).contains(&StreamEvent::StreamEnded));
    assert_eq!(engine.s().play_count, 1, "종료된 방송에 재연결하지 않는다");
}

// MARK: - 네트워크 복구

#[test]
fn network_recovery_decisions() {
    use InterfaceType::*;
    let d = |prev, cur, phase: StreamPhase, paused, url, suspend| {
        network_recovery_decision(prev, cur, &phase, paused, url, suspend)
    };
    assert_eq!(
        d(Offline, Wifi, StreamPhase::Reconnecting, false, true, false),
        NetworkRecoveryAction::SkipReconnectBackoff
    );
    assert_eq!(
        d(
            Offline,
            Wifi,
            StreamPhase::Error("재연결 최대 횟수 초과".into()),
            false,
            true,
            false
        ),
        NetworkRecoveryAction::RestartAfterExhaustion
    );
    assert_eq!(
        d(
            Offline,
            WiredEthernet,
            StreamPhase::Playing,
            false,
            true,
            false
        ),
        NetworkRecoveryAction::ResetStallTracking
    );
    assert_eq!(
        d(
            Wifi,
            WiredEthernet,
            StreamPhase::Reconnecting,
            false,
            true,
            false
        ),
        NetworkRecoveryAction::Ignore
    );
    assert_eq!(
        d(Offline, Wifi, StreamPhase::Reconnecting, true, true, false),
        NetworkRecoveryAction::Ignore
    );
    assert_eq!(
        d(Offline, Wifi, StreamPhase::Reconnecting, false, true, true),
        NetworkRecoveryAction::Ignore
    );
    assert_eq!(
        d(Offline, Wifi, StreamPhase::StreamEnded, false, true, false),
        NetworkRecoveryAction::Ignore
    );
    assert_eq!(
        d(Offline, Wifi, StreamPhase::Idle, false, false, false),
        NetworkRecoveryAction::Ignore
    );
}

#[tokio::test]
async fn path_change_interrupts_handler_backoff() {
    let c = coordinator(false);
    c.st().stream_url = Some(test_url());
    c.update_phase(StreamPhase::Reconnecting);
    let handler = c.reconnection_handler().clone();
    handler.set_config(ReconnectionConfig {
        max_retries: 3,
        base_delay: 3.0,
        max_retry_delay: 30.0,
        backoff_multiplier: 1.0,
        retry_on_network_recovery: true,
    });
    let entered = Arc::new(AtomicBool::new(false));
    let started = std::time::Instant::now();
    {
        let handler = handler.clone();
        let entered = entered.clone();
        handler.clone().start_reconnecting(
            Arc::new(move |_, delay| {
                let handler = handler.clone();
                let entered = entered.clone();
                Box::pin(async move {
                    handler.wait_for_backoff(delay).await;
                    entered.store(true, Ordering::SeqCst);
                })
            }),
            Arc::new(|| Box::pin(async {})),
        );
    }
    tokio::time::sleep(Duration::from_millis(20)).await;
    c.handle_transport_path_change(InterfaceType::Offline);
    c.handle_transport_path_change(InterfaceType::Wifi);
    for _ in 0..200 {
        if entered.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(entered.load(Ordering::SeqCst));
    assert!(started.elapsed() < Duration::from_secs(1));
    handler.cancel();
    c.stop_stream().await;
}

// MARK: - 프록시 경로

async fn route_setup(authenticated: bool) -> (StreamCoordinator, Arc<MockEngine>) {
    let c = StreamCoordinator::with_fetcher(
        Configuration {
            enable_low_latency: false,
            stream_proxy_mode: StreamProxyMode::None,
            ..Configuration::new("proxy-route-test")
        },
        PlaybackKeyAccess::new(authenticated, Arc::new(Vec::new)),
        Arc::new(MockFetcher::default()),
    );
    let engine = MockEngine::new();
    c.set_player_engine(engine.shared()).await;
    c.start_stream(route_url()).await.unwrap();
    c.report_engine_playing().await;
    (c, engine)
}

fn last_host(engine: &MockEngine) -> String {
    engine
        .s()
        .last_play_url
        .as_ref()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_default()
}

#[tokio::test]
async fn authentication_keeps_route_without_reloads() {
    let (c, engine) = route_setup(true).await;
    assert_eq!(last_host(&engine), "127.0.0.1");
    c.apply_stream_proxy_mode(StreamProxyMode::LocalProxy).await;
    c.apply_stream_proxy_mode(StreamProxyMode::None).await;
    assert_eq!(engine.s().play_count, 1);
    let status = c.proxy_route_status().unwrap();
    assert_eq!(status.requested, StreamProxyMode::None);
    assert_eq!(status.effective, Some(StreamProxyMode::LocalProxy));
    assert_eq!(
        status.reason,
        StreamProxyRouteReason::AuthenticationRequired
    );
    c.stop_stream().await;
}

#[tokio::test]
async fn pause_defers_route_change_until_playback_resumes() {
    let (c, engine) = route_setup(false).await;
    engine.set_volume(0.35);
    c.pause().await;
    c.apply_stream_proxy_mode(StreamProxyMode::LocalProxy).await;
    assert_eq!(engine.s().play_count, 1);
    assert!(c.proxy_route_status().unwrap().pending);
    c.resume().await;
    c.report_engine_playing().await;
    assert_eq!(engine.s().play_count, 2);
    assert_eq!(engine.s().last_volume, 0.35);
    assert_eq!(last_host(&engine), "127.0.0.1");
    assert!(!c.proxy_route_status().unwrap().pending);
    c.stop_stream().await;
}

#[tokio::test]
async fn dvr_and_suspended_panes_defer_route_change() {
    let (c, engine) = route_setup(false).await;
    c.set_time_shift_active(true).await;
    engine.s().current_time = 42.0;
    c.apply_stream_proxy_mode(StreamProxyMode::LocalProxy).await;
    assert_eq!(engine.current_time(), 42.0);
    assert_eq!(engine.s().play_count, 1);
    c.set_time_shift_active(false).await;
    assert_eq!(engine.s().play_count, 2);
    c.report_engine_playing().await;
    engine.s().suspend_intent = true;
    c.apply_stream_proxy_mode(StreamProxyMode::None).await;
    assert_eq!(engine.s().play_count, 2);
    engine.s().suspend_intent = false;
    c.report_engine_playing().await;
    assert_eq!(engine.s().play_count, 3);
    assert_eq!(last_host(&engine), "video.pstatic.net");
    c.stop_stream().await;
}

#[tokio::test]
async fn latest_selection_wins_and_stop_cannot_restart() {
    let (c, engine) = route_setup(false).await;
    c.apply_stream_proxy_mode(StreamProxyMode::LocalProxy).await;
    c.apply_stream_proxy_mode(StreamProxyMode::None).await;
    c.apply_stream_proxy_mode(StreamProxyMode::LocalProxy).await;
    c.report_engine_playing().await;
    assert_eq!(engine.s().play_count, 2);
    assert!(!c.proxy_route_status().unwrap().pending);
    c.stop_stream().await;
    c.apply_stream_proxy_mode(StreamProxyMode::None).await;
    c.report_engine_playing().await;
    assert_eq!(engine.s().play_count, 2);
    assert_eq!(c.phase(), StreamPhase::Idle);
    assert!(!c.is_proxy_active());
}

#[tokio::test]
async fn authenticated_playback_never_falls_back_to_direct_url() {
    let c = StreamCoordinator::with_fetcher(
        Configuration {
            enable_low_latency: false,
            ..Configuration::new("proxy-route-auth")
        },
        PlaybackKeyAccess::new(true, Arc::new(Vec::new)),
        Arc::new(MockFetcher::default()),
    );
    let engine = MockEngine::new();
    c.set_player_engine(engine.shared()).await;
    assert_eq!(
        c.start_stream(test_url()).await,
        Err(StreamCoordinatorError::ProxyAuthenticationUnavailable)
    );
    assert_eq!(engine.s().play_count, 0);
    let status = c.proxy_route_status().unwrap();
    assert_eq!(status.effective, None);
    assert_eq!(
        status.reason,
        StreamProxyRouteReason::AuthenticationUnavailable
    );
    c.stop_stream().await;
}

// MARK: - 역할별 목표·선택 싱크

fn managed_config() -> Configuration {
    Configuration {
        low_latency_activation: LowLatencyActivation::Managed,
        ..Configuration::new("managed")
    }
}

#[test]
fn target_latency_margin_shifts_band_and_resets() {
    let c = coordinator_with(managed_config(), Arc::new(MockFetcher::default()));
    let base = c
        .low_latency_controller()
        .expect("저지연 컨트롤러")
        .current_configuration();
    c.set_target_latency_margin(1.0);
    let cfg = c.low_latency_controller().unwrap().current_configuration();
    assert!((cfg.target_latency - (base.target_latency + 1.0)).abs() < 1e-9);
    assert!(
        (cfg.max_latency - (base.max_latency + 1.0)).abs() < 1e-9,
        "밴드 폭은 그대로"
    );
    assert!(
        cfg.slow_down_threshold <= 0.25 + 1e-9,
        "늦추기 문턱을 좁혀 여유가 실제 전방 버퍼가 되게"
    );
    c.set_target_latency_margin(99.0); // 상한
    let cfg = c.low_latency_controller().unwrap().current_configuration();
    assert!(
        (cfg.target_latency - (base.target_latency + MAX_TARGET_LATENCY_MARGIN_SECONDS)).abs()
            < 1e-9
    );
    c.set_target_latency_margin(0.0);
    assert_eq!(
        c.low_latency_controller().unwrap().current_configuration(),
        base
    );
}

#[tokio::test]
async fn unselected_pane_gets_stability_target_and_selection_owns_latency() {
    let c = coordinator_with(managed_config(), Arc::new(MockFetcher::default()));
    let engine = MockEngine::hls();
    c.set_player_engine(engine.shared()).await;
    assert_eq!(
        engine.s().controller_target,
        SECONDARY_PANE_STABILITY_TARGET_SECONDS,
        "비선택 패인은 재생 전에 안정성 목표를 심는다"
    );
    assert!(!c.should_run_low_latency_sync());
    c.update_phase(StreamPhase::Playing);
    c.set_low_latency_sync_active(true).await;
    assert!(c.should_run_low_latency_sync());
    assert!(engine.s().owned_by_controller);
    assert_eq!(c.idle_latency_target_seconds(), 0.0);
    c.set_low_latency_sync_active(false).await;
    assert!(!engine.s().owned_by_controller, "소유권 반납");
    assert_eq!(
        engine.s().controller_target,
        SECONDARY_PANE_STABILITY_TARGET_SECONDS
    );
    assert!(!c.low_latency_controller().unwrap().has_sync_task());
}

#[tokio::test]
async fn automatic_mode_has_no_idle_target() {
    let c = coordinator(true);
    let engine = MockEngine::hls();
    c.set_player_engine(engine.shared()).await;
    assert_eq!(c.idle_latency_target_seconds(), 0.0);
    assert_eq!(engine.s().controller_target, 0.0);
}

#[tokio::test]
async fn first_frame_starts_pid_only_for_hlsjs_and_pause_stops_it() {
    let c = coordinator(true);
    let engine = MockEngine::hls();
    engine.s().live_latency = 3.0;
    c.set_player_engine(engine.shared()).await;
    c.start_stream(test_url()).await.unwrap();
    c.report_engine_playing().await;
    settle().await;
    let controller = c.low_latency_controller().unwrap().clone();
    assert!(controller.has_sync_task(), "첫 프레임 뒤 PID 싱크 시작");
    assert_eq!(c.hls_live_latency(), Some(3.0));
    c.handle_playback_engine_state(&PlayerPhase::Paused);
    assert!(!controller.has_sync_task());
    c.stop_stream().await;
}

// MARK: - 로컬 타임시프트

#[derive(Default)]
struct FakeRecorder {
    recorded: Mutex<f64>,
    fetch_enabled: Mutex<Option<bool>>,
    cleared: AtomicU64,
    stopped: AtomicBool,
}

impl TimeshiftBackend for FakeRecorder {
    fn session_id(&self) -> String {
        "sid".into()
    }
    fn local_playlist(&self, _: Option<f64>) -> Option<String> {
        None
    }
    fn local_segment(&self, _: i64) -> Option<Vec<u8>> {
        None
    }
    fn local_init(&self, _: i64) -> Option<Vec<u8>> {
        None
    }
    fn observe_playlist(&self, _: &str, _: &Url, _: bool, _: Option<&Url>) {}
    fn tee_destination(&self, _: &Url) -> Option<TeeDestination> {
        None
    }
    fn commit(&self, _: &TeeDestination, _: Vec<u8>) {}
    fn abandon(&self, _: &TeeDestination) {}
    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
    }
    fn recorded_duration(&self) -> f64 {
        *self.recorded.lock().unwrap()
    }
    fn set_upstream_fetch_enabled(&self, enabled: bool) {
        *self.fetch_enabled.lock().unwrap() = Some(enabled);
    }
    fn clear_served_position(&self) {
        self.cleared.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn local_timeshift_enter_move_and_exit() {
    let c = coordinator_with(
        Configuration {
            enable_low_latency: false,
            ..Configuration::new("ts")
        },
        Arc::new(MockFetcher::default()),
    );
    let recorder = Arc::new(FakeRecorder::default());
    *recorder.recorded.lock().unwrap() = 120.0;
    let r = recorder.clone();
    c.set_timeshift_recorder_factory(Some(Arc::new(move |_, _| r.clone())));
    c.set_timeshift_recording(Some(TimeshiftRecordingConfig {
        max_duration: 600.0,
        budget_limit_bytes: 1 << 30,
    }))
    .await;
    let engine = MockEngine::hls();
    c.set_player_engine(engine.shared()).await;
    c.start_stream(route_url()).await.unwrap();
    assert!(
        c.stream_proxy().timeshift_backend().is_some(),
        "프록시 시작 뒤 레코더를 붙인다"
    );
    assert_eq!(
        *recorder.fetch_enabled.lock().unwrap(),
        Some(false),
        "연결 중에는 받지 않는다"
    );
    c.report_engine_playing().await;
    assert_eq!(*recorder.fetch_enabled.lock().unwrap(), Some(true));

    assert!(c.enter_timeshift(30.0).await);
    assert!(c.is_local_timeshift_active());
    assert!(!engine.is_live_latency_correction_enabled());
    let url = engine.s().timeshift_urls.last().cloned().unwrap();
    assert!(
        url.as_str().contains("/_ts/sid/index.m3u8?start=30.000"),
        "{url}"
    );
    // 이미 되감는 중이면 재로드 없이 옮긴다.
    assert!(c.enter_timeshift(10.0).await);
    assert_eq!(engine.s().timeshift_urls.len(), 1);
    assert_eq!(engine.s().seek_positions.last(), Some(&-10.0));
    c.exit_timeshift().await;
    assert!(!c.is_local_timeshift_active());
    assert!(engine.is_live_latency_correction_enabled());
    assert!(recorder.cleared.load(Ordering::SeqCst) >= 1);
    assert_eq!(last_host(&engine), "127.0.0.1", "라이브 복귀는 프록시 URL");

    // 기록을 끄면 레코더를 뗀다.
    c.set_timeshift_recording(None).await;
    assert!(c.stream_proxy().timeshift_backend().is_none());
    assert!(recorder.stopped.load(Ordering::SeqCst));
    c.stop_stream().await;
}

#[tokio::test]
async fn server_time_machine_stream_disables_local_recording() {
    let c = coordinator(false);
    let recorder = Arc::new(FakeRecorder::default());
    let r = recorder.clone();
    c.set_timeshift_recorder_factory(Some(Arc::new(move |_, _| r.clone())));
    c.set_server_time_machine_stream(true).await;
    c.set_timeshift_recording(Some(TimeshiftRecordingConfig {
        max_duration: 60.0,
        budget_limit_bytes: 1,
    }))
    .await;
    c.set_player_engine(MockEngine::hls().shared()).await;
    c.start_stream(route_url()).await.unwrap();
    assert!(c.stream_proxy().timeshift_backend().is_none());
    c.stop_stream().await;
}
