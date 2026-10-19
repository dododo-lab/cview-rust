//! 원본: `PlayerOverlayStateTests`, `MultiLiveQualityTargetTests`, `StreamEventObservationTests`,
//! `PlayerViewModelLiveTelemetryTests`, `ScreenshotSaveTests`, `Audit0919xrecordingstatusTests`,
//! `LiveRewindViewModelTests`, `Audit0919vmplayerAdvancedSettingsTests` 핵심.

use std::ops::RangeInclusive;
use std::path::Path;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use url::Url;

use cview_app_core::viewmodels::player::{
    equalizer_bands_fitting_frequencies, format_time_interval, screenshot_file_base_name,
    selectable_qualities, write_screenshot, LivePlayerEngine, PlayerOverlayState, PlayerViewModel,
    RewindSource, SharedLiveEngine, StartStreamRequest, VideoAdjust,
};
use cview_core::models::equalizer::EqualizerPreset;
use cview_core::models::player::{PlayerError, PlayerToolStatusKind};
use cview_core::models::settings::{PlayerSettings, ScreenshotFormat};
use cview_core::models::stream_proxy::StreamProxyMode;
use cview_core::protocols::{CapturedFrame, PlayerEngineCapabilities, PlayerEngineProtocol};
use cview_player::coordinator::{
    BufferHealth, CoordinatedEngine, LatencyInfo, StreamEvent, StreamPhase, StreamQualityInfo,
};
use cview_player::hlsjs::HlsJsForcedQualityTarget;

// MARK: - 가짜 엔진

#[derive(Default)]
struct EngineState {
    playing: bool,
    current_time: f64,
    duration: f64,
    seek_updates_current_time: bool,
    last_seek: Option<f64>,
    last_volume: Option<f32>,
    volume_writes: usize,
    eq: Vec<(bool, Vec<f32>, f32)>,
    adjust: Vec<VideoAdjust>,
    snapped: usize,
    recording: bool,
    recording_failure: Option<String>,
    fail_play: bool,
}

struct FakeEngine {
    s: Mutex<EngineState>,
    hlsjs: bool,
}

impl FakeEngine {
    fn new(hlsjs: bool) -> Arc<Self> {
        Arc::new(Self {
            s: Mutex::new(EngineState {
                playing: true,
                seek_updates_current_time: true,
                ..Default::default()
            }),
            hlsjs,
        })
    }
    fn at(current: f64, duration: f64) -> Arc<Self> {
        let e = Self::new(true);
        {
            let mut s = e.s.lock().unwrap();
            s.current_time = current;
            s.duration = duration;
        }
        e
    }
    fn st(&self) -> std::sync::MutexGuard<'_, EngineState> {
        self.s.lock().unwrap()
    }
}

#[async_trait]
impl PlayerEngineProtocol for FakeEngine {
    fn is_playing(&self) -> bool {
        self.st().playing
    }
    fn current_time(&self) -> f64 {
        self.st().current_time
    }
    fn duration(&self) -> f64 {
        self.st().duration
    }
    fn seekable_range(&self) -> Option<RangeInclusive<f64>> {
        let d = self.st().duration;
        (d > 0.0).then_some(0.0..=d)
    }
    fn rate(&self) -> f32 {
        1.0
    }
    async fn play(&self, _url: &Url) -> Result<(), PlayerError> {
        if self.st().fail_play {
            return Err(PlayerError::ConnectionLost);
        }
        Ok(())
    }
    fn pause(&self) {
        self.st().playing = false;
    }
    fn resume(&self) {
        self.st().playing = true;
    }
    fn stop(&self) {
        self.st().playing = false;
    }
    fn seek(&self, position: f64) {
        let mut s = self.st();
        s.last_seek = Some(position);
        if s.seek_updates_current_time {
            s.current_time = position;
        }
    }
    fn set_rate(&self, _rate: f32) {}
    fn set_volume(&self, volume: f32) {
        let mut s = self.st();
        s.last_volume = Some(volume);
        s.volume_writes += 1;
    }
    async fn start_recording(&self, _path: &Path) -> Result<(), PlayerError> {
        self.st().recording = true;
        Ok(())
    }
    async fn stop_recording(&self) {
        self.st().recording = false;
    }
    fn is_in_error_state(&self) -> bool {
        false
    }
    fn reset_retries(&self) {}
    async fn capture_screenshot(
        &self,
        format: ScreenshotFormat,
    ) -> Result<CapturedFrame, PlayerError> {
        Ok(CapturedFrame {
            data: vec![1, 2, 3],
            format,
            pixel_width: 1920,
            pixel_height: 1080,
        })
    }
    fn capabilities(&self) -> PlayerEngineCapabilities {
        PlayerEngineCapabilities::SCREENSHOT
            | PlayerEngineCapabilities::RECORDING
            | PlayerEngineCapabilities::EQUALIZER
    }
}

impl CoordinatedEngine for FakeEngine {}

impl LivePlayerEngine for FakeEngine {
    fn is_hlsjs(&self) -> bool {
        self.hlsjs
    }
    fn seek_to_live_edge(&self) {
        let mut s = self.st();
        s.snapped += 1;
        s.playing = true;
    }
    fn set_equalizer_enabled(&self, enabled: bool, bands: &[f32], pre_amp_db: f32) {
        self.st().eq.push((enabled, bands.to_vec(), pre_amp_db));
    }
    fn apply_video_adjust(&self, adjust: VideoAdjust) {
        self.st().adjust.push(adjust);
    }
    fn recording_failure_reason(&self) -> Option<String> {
        self.st().recording_failure.clone()
    }
}

fn vm_with(engine: &Arc<FakeEngine>) -> PlayerViewModel {
    let vm = PlayerViewModel::new(None);
    vm.inject_engine(engine.clone() as SharedLiveEngine);
    vm
}

fn latency(current: f64) -> StreamEvent {
    StreamEvent::LatencyUpdate(LatencyInfo {
        current,
        target: 3.5,
        ewma: current,
    })
}

// MARK: - 오버레이

#[test]
fn overlay_state_mapping() {
    use PlayerOverlayState as O;
    assert_eq!(
        O::from(true, Some(&StreamPhase::Playing), true),
        O::InitialLoading
    );
    assert_eq!(O::from(false, Some(&StreamPhase::Playing), true), O::Ended);
    assert_eq!(
        O::from(false, Some(&StreamPhase::Connecting), false),
        O::Connecting
    );
    assert_eq!(
        O::from(false, Some(&StreamPhase::Buffering), false),
        O::Buffering
    );
    assert_eq!(
        O::from(false, Some(&StreamPhase::Reconnecting), false),
        O::Reconnecting
    );
    assert_eq!(
        O::from(false, Some(&StreamPhase::StreamEnded), false),
        O::Ended
    );
    assert_eq!(
        O::from(false, Some(&StreamPhase::Error("x".into())), false),
        O::Error("x".into())
    );
    for p in [
        StreamPhase::Playing,
        StreamPhase::Paused,
        StreamPhase::Idle,
        StreamPhase::LoadingInfo,
        StreamPhase::LoadingManifest,
    ] {
        assert_eq!(O::from(false, Some(&p), false), O::None);
    }
    assert_eq!(O::from(false, None, false), O::None);
    assert!(O::Buffering.needs_loading_overlay());
    assert!(!O::Ended.needs_loading_overlay());
    for o in [
        O::InitialLoading,
        O::Connecting,
        O::Buffering,
        O::Reconnecting,
        O::Ended,
        O::Error("e".into()),
    ] {
        assert!(o.has_overlay());
    }
    assert!(!O::None.has_overlay());
}

// MARK: - 멀티라이브 화질

fn q(name: &str, res: &str, bw: i64, fps: Option<f64>) -> StreamQualityInfo {
    let mut q = StreamQualityInfo::new(name, res, bw);
    q.frame_rate = fps;
    q
}

#[test]
fn multi_live_menu_exposes_single_closest_1080p() {
    let all = vec![
        q("1080p60", "1920x1080", 8_200_000, Some(60.0)),
        q("1080p", "1920x1080", 6_000_000, Some(30.0)),
        q("720p", "1280x720", 3_000_000, Some(60.0)),
    ];
    assert_eq!(
        selectable_qualities(
            &all,
            false,
            HlsJsForcedQualityTarget::MULTI_LIVE_1080P_8MBPS
        )
        .len(),
        3
    );
    let one = selectable_qualities(&all, true, HlsJsForcedQualityTarget::MULTI_LIVE_1080P_8MBPS);
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].name, "1080p60");
    let small = selectable_qualities(&all, true, HlsJsForcedQualityTarget::new(720, 3_000));
    assert_eq!(small[0].name, "720p");
}

#[test]
fn multi_live_quality_click_only_accepts_target() {
    let vm = PlayerViewModel::new(None);
    vm.lock().is_multi_live = true;
    vm.lock().available_qualities = vec![
        q("1080p60", "1920x1080", 8_000_000, Some(60.0)),
        q("480p", "852x480", 1_000_000, Some(30.0)),
    ];
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    rt.block_on(vm.switch_quality(&q("480p", "852x480", 1_000_000, Some(30.0))));
    assert!(vm.read().current_quality.is_none());
    rt.block_on(vm.switch_quality(&q("1080p60", "1920x1080", 8_000_000, Some(60.0))));
    assert_eq!(vm.read().current_quality.as_ref().unwrap().name, "1080p60");
}

// MARK: - 이벤트 관찰

#[test]
fn equal_values_skip_invalidation() {
    let vm = PlayerViewModel::new(None);
    let health = BufferHealth::new(2.0, 3.0, true);
    vm.handle_stream_event(StreamEvent::BufferUpdate(health));
    vm.handle_stream_event(StreamEvent::Error("e".into()));
    let rev = vm.subscribe();
    let before = *rev.borrow();
    vm.handle_stream_event(StreamEvent::BufferUpdate(health));
    vm.handle_stream_event(StreamEvent::Error("e".into()));
    vm.handle_stream_event(StreamEvent::TimeShiftEnded);
    assert_eq!(*rev.borrow(), before, "같은 값이 구독자를 깨웠다");
}

#[test]
fn latency_history_downsamples_by_input_sequence() {
    let vm = PlayerViewModel::new(None);
    for n in 1..=21 {
        vm.handle_stream_event(latency(n as f64));
    }
    let s = vm.read();
    assert_eq!(
        s.latency_history.iter().map(|p| p.1).collect::<Vec<_>>(),
        [1.0, 10.0, 20.0]
    );
    assert_eq!(s.latency_info.unwrap().current, 21.0);
}

#[tokio::test]
async fn buffering_is_debounced_after_recent_playing() {
    let vm = PlayerViewModel::new(None);
    vm.handle_stream_event(StreamEvent::PhaseChanged(StreamPhase::Buffering));
    assert_eq!(
        vm.read().stream_phase,
        StreamPhase::Buffering,
        "시작 중 버퍼링은 즉시"
    );
    vm.handle_stream_event(StreamEvent::PhaseChanged(StreamPhase::Playing));
    vm.handle_stream_event(StreamEvent::PhaseChanged(StreamPhase::Buffering));
    assert_eq!(
        vm.read().stream_phase,
        StreamPhase::Playing,
        "재생 직후 1.5초 안 버퍼링은 무시"
    );
    vm.handle_stream_event(StreamEvent::PhaseChanged(StreamPhase::Error("boom".into())));
    assert_eq!(vm.read().error_message.as_deref(), Some("boom"));
    vm.handle_stream_event(StreamEvent::StreamEnded);
    assert_eq!(vm.read().stream_phase, StreamPhase::StreamEnded);
}

// MARK: - 볼륨

#[test]
fn volume_writes_are_bounded() {
    let engine = FakeEngine::new(true);
    let vm = vm_with(&engine);
    vm.set_volume(0.4);
    let writes = engine.st().volume_writes;
    for _ in 0..100 {
        vm.set_volume(0.4);
    }
    vm.set_volume(f32::NAN);
    vm.set_volume(f32::INFINITY);
    assert_eq!(engine.st().volume_writes, writes);
    assert_eq!(vm.read().volume, 0.4);
    vm.set_volume(2.0);
    assert_eq!(vm.read().volume, 1.0);
    vm.set_volume(-1.0);
    assert_eq!(vm.read().volume, 0.0);
    let settings = PlayerSettings {
        volume_level: f32::NAN,
        ..Default::default()
    };
    vm.apply_settings(&settings, false);
    assert_eq!(vm.read().volume, 0.0);
    assert_eq!(engine.st().last_volume, Some(0.0));
}

#[test]
fn reconnect_preserves_session_audio() {
    let engine = FakeEngine::new(true);
    let vm = vm_with(&engine);
    let settings = PlayerSettings {
        volume_level: 0.8,
        ..Default::default()
    };
    vm.initialize_session_volume(&settings);
    assert_eq!(vm.read().volume, 0.8);
    vm.set_volume(0.23);
    vm.set_muted(true);
    for _ in 0..3 {
        vm.initialize_session_volume(&settings);
        vm.apply_settings(&settings, true);
        assert_eq!(vm.read().volume, 0.23);
        assert!(vm.read().is_muted);
        assert_eq!(engine.st().last_volume, Some(0.0));
    }
    vm.set_muted(false);
    assert_eq!(engine.st().last_volume, Some(0.23));
    vm.apply_settings(&settings, false);
    assert_eq!(vm.read().volume, 0.8);
    assert_eq!(engine.st().last_volume, Some(0.8));
}

#[test]
fn user_volume_before_start_wins() {
    let vm = PlayerViewModel::new(None);
    vm.set_volume(0.0);
    vm.initialize_session_volume(&PlayerSettings::default());
    assert_eq!(vm.read().volume, 0.0);
}

// MARK: - 타임머신·되감기

fn tm_vm(engine: &Arc<FakeEngine>) -> PlayerViewModel {
    let vm = vm_with(engine);
    vm.lock().stream_phase = StreamPhase::Playing;
    vm.configure_time_machine_access(true, true);
    vm
}

#[tokio::test]
async fn time_machine_window_and_seek() {
    let engine = FakeEngine::at(4_000.0, 4_000.0);
    let vm = tm_vm(&engine);
    assert_eq!(vm.read().time_machine_window_duration(), 3_600.0);
    vm.seek_time_machine(900.0).await;
    assert_eq!(engine.st().last_seek, Some(3_100.0));
    assert!(vm.read().is_time_shifting);
}

#[test]
fn multi_live_watch_session_exposes_live_timeline() {
    let engine = FakeEngine::at(3_100.0, 4_000.0);
    let vm = PlayerViewModel::new(None);
    vm.lock().is_multi_live = true;
    vm.inject_engine(engine.clone());
    vm.lock().stream_phase = StreamPhase::Playing;
    vm.configure_time_machine_access(true, true);
    let s = vm.read();
    assert_eq!(s.time_machine_window_duration(), 3_600.0);
    assert_eq!(s.seconds_behind_live(), 900.0);
    assert!(s.is_behind_live());
}

#[test]
fn seconds_behind_falls_back_to_latency_without_window() {
    let engine = FakeEngine::at(3_999.0, 4_000.0);
    let vm = vm_with(&engine);
    vm.lock().stream_phase = StreamPhase::Playing;
    vm.handle_stream_event(StreamEvent::LatencyUpdate(LatencyInfo {
        current: 7.4,
        target: 3.5,
        ewma: 7.4,
    }));
    let s = vm.read();
    assert_eq!(s.time_machine_window_duration(), 0.0);
    assert_eq!(s.seconds_behind_live(), 7.4);
    assert!(s.is_behind_live());
}

#[tokio::test]
async fn time_shift_ended_event_clears_rewind_state() {
    let engine = FakeEngine::at(4_000.0, 4_000.0);
    let vm = tm_vm(&engine);
    vm.seek_time_machine(600.0).await;
    assert!(vm.read().is_time_shifting);
    vm.handle_stream_event(StreamEvent::TimeShiftEnded);
    assert!(!vm.read().is_time_shifting);
}

#[tokio::test]
async fn rapid_steps_accumulate_before_metrics_catch_up() {
    let engine = FakeEngine::at(4_000.0, 4_000.0);
    engine.st().seek_updates_current_time = false;
    let vm = tm_vm(&engine);
    vm.step_rewind(-60.0).await;
    assert_eq!(engine.st().last_seek, Some(3_940.0));
    vm.step_rewind(-60.0).await;
    assert_eq!(engine.st().last_seek, Some(3_880.0));
    assert_eq!(vm.read().seconds_behind_live(), 120.0);
}

#[tokio::test]
async fn first_step_counts_current_delay() {
    let engine = FakeEngine::at(3_996.0, 4_000.0);
    let vm = tm_vm(&engine);
    vm.step_rewind(-10.0).await;
    assert_eq!(engine.st().last_seek, Some(3_986.0));
}

#[tokio::test]
async fn access_reset_only_ends_server_time_machine() {
    let vm = PlayerViewModel::new(None);
    vm.lock().is_time_shifting = true;
    vm.configure_time_machine_access(false, false);
    assert!(vm.read().is_time_shifting);

    let engine = FakeEngine::at(4_000.0, 4_000.0);
    let tm = tm_vm(&engine);
    tm.seek_time_machine(600.0).await;
    assert!(tm.read().is_time_shifting);
    tm.configure_time_machine_access(true, false);
    assert!(!tm.read().is_time_shifting);
}

#[tokio::test]
async fn jump_to_live_works_while_paused_and_rewound() {
    let engine = FakeEngine::at(4_000.0, 4_000.0);
    let vm = tm_vm(&engine);
    vm.seek_time_machine(600.0).await;
    vm.lock().stream_phase = StreamPhase::Paused;
    vm.jump_to_live();
    assert!(!vm.read().is_time_shifting);
}

#[test]
fn jump_to_live_keeps_pause_on_snap() {
    let engine = FakeEngine::at(3_000.0, 4_000.0);
    let vm = vm_with(&engine);
    vm.lock().stream_phase = StreamPhase::Paused;
    vm.lock().is_time_shifting = true;
    vm.jump_to_live();
    assert_eq!(engine.st().snapped, 1);
    assert!(!engine.st().playing, "엣지 스냅이 재생을 다시 걸었다");
}

#[test]
fn rewind_recording_config_follows_settings() {
    let vm = PlayerViewModel::new(None);
    let c = vm.rewind_recording_config().expect("단일 시청은 기본 기록");
    assert_eq!(c.max_duration, 3_600.0);
    assert_eq!(c.budget_limit_bytes, 5_120 * 1_048_576);

    let off = PlayerSettings {
        live_rewind_enabled: false,
        ..Default::default()
    };
    vm.apply_rewind_settings(&off);
    assert!(vm.rewind_recording_config().is_none());

    let short = PlayerSettings {
        live_rewind_enabled: true,
        live_rewind_window_minutes: 30,
        live_rewind_disk_budget_mb: 1_024,
        ..Default::default()
    };
    vm.apply_rewind_settings(&short);
    let c = vm.rewind_recording_config().unwrap();
    assert_eq!(c.max_duration, 1_800.0);
    assert_eq!(c.budget_limit_bytes, 1_024 * 1_048_576);

    let direct = PlayerSettings {
        stream_proxy_mode: StreamProxyMode::None,
        ..short.clone()
    };
    vm.apply_rewind_settings(&direct);
    assert!(
        vm.rewind_recording_config().is_none(),
        "로컬 프록시 없이는 기록 불가"
    );

    vm.apply_rewind_settings(&short);
    let engine = FakeEngine::at(4_000.0, 4_000.0);
    vm.inject_engine(engine);
    vm.configure_time_machine_access(true, true);
    assert!(
        vm.rewind_recording_config().is_none(),
        "공식 타임머신은 따로 기록하지 않는다"
    );
}

#[test]
fn demoted_session_returns_to_live_but_keeps_recording() {
    let engine = FakeEngine::at(4_000.0, 4_000.0);
    let vm = vm_with(&engine);
    vm.lock().stream_phase = StreamPhase::Playing;
    vm.lock().is_multi_live = true;
    assert!(
        vm.rewind_recording_config().is_none(),
        "멀티라이브 기본은 기록 안 함"
    );
    vm.set_rewind_recording_desired(Some(true));
    vm.lock().is_time_shifting = true;
    vm.set_rewind_recording_desired(Some(false));
    let s = vm.read();
    assert!(!s.is_time_shifting);
    assert!(s.is_rewind_recording_release_pending());
    assert!(s.rewind_recording_config().is_some());
}

#[test]
fn rewind_source_none_or_server() {
    let vm = PlayerViewModel::new(None);
    assert_eq!(vm.read().rewind_source(), RewindSource::None);
    let engine = FakeEngine::at(4_000.0, 4_000.0);
    let tm = tm_vm(&engine);
    assert_eq!(tm.read().rewind_source(), RewindSource::ServerTimeMachine);
}

#[tokio::test]
async fn rewind_requests_ignored_when_ended() {
    let engine = FakeEngine::at(4_000.0, 4_000.0);
    let vm = tm_vm(&engine);
    vm.lock().stream_phase = StreamPhase::StreamEnded;
    vm.seek_rewind(600.0).await;
    assert!(engine.st().last_seek.is_none());
    assert!(!vm.read().is_time_shifting);
}

// MARK: - 스크린샷

#[test]
fn screenshot_base_names() {
    assert_eq!(
        screenshot_file_base_name("치지직", 1_700_000_000),
        "CView_치지직_1700000000"
    );
    assert_eq!(
        screenshot_file_base_name("a/b:c\\d?e%f*g|h\"i<j>", 1),
        "CView_abcdefghij_1"
    );
    assert_eq!(screenshot_file_base_name("  ", 5), "CView_5");
}

#[test]
fn write_screenshot_creates_folder_and_never_overwrites() {
    let dir = std::env::temp_dir().join(format!("cview-shot-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let frame = CapturedFrame {
        data: vec![9; 4],
        format: ScreenshotFormat::Jpeg,
        pixel_width: 2,
        pixel_height: 2,
    };
    let a = write_screenshot(&frame, &dir, "CView_x_1").unwrap();
    let b = write_screenshot(&frame, &dir, "CView_x_1").unwrap();
    assert_eq!(a.file_name().unwrap(), "CView_x_1.jpeg");
    assert_eq!(b.file_name().unwrap(), "CView_x_1-2.jpeg");
    // 파일 자리에 폴더를 만들 수 없으면 오류.
    let blocked = a.join("child");
    assert!(write_screenshot(&frame, &blocked, "n").is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn take_screenshot_reports_status() {
    let vm = PlayerViewModel::new(None);
    vm.take_screenshot().await;
    assert_eq!(
        vm.read().screenshot_status.as_ref().unwrap().message,
        "재생 중인 스트림이 없습니다"
    );
    let engine = FakeEngine::new(true);
    vm.inject_engine(engine);
    let dir = std::env::temp_dir().join(format!("cview-shot2-{}", std::process::id()));
    vm.lock().screenshot_save_path = dir.to_string_lossy().into_owned();
    vm.take_screenshot().await;
    let status = vm.read().screenshot_status.clone().unwrap();
    assert_eq!(status.kind, PlayerToolStatusKind::Success);
    assert!(status.message.starts_with("저장됨 · 1920×1080 · CView_"));
    let _ = std::fs::remove_dir_all(&dir);
}

// MARK: - 녹화 상태

#[test]
fn recording_and_screenshot_status_are_independent() {
    let vm = PlayerViewModel::new(None);
    vm.set_recording_status(PlayerToolStatusKind::Failure, "녹화 시작 실패 · x".into());
    assert!(vm.read().recording_status.as_ref().unwrap().is_failure());
    assert!(vm.read().screenshot_status.is_none());
    assert!(vm.read().error_message.is_none());
    vm.set_screenshot_status(PlayerToolStatusKind::Success, "저장됨 · CView_1.png".into());
    assert!(!vm.read().screenshot_status.as_ref().unwrap().is_failure());
    assert!(vm.read().recording_status.as_ref().unwrap().is_failure());
}

#[test]
fn failure_event_clears_recording_indicator() {
    let vm = PlayerViewModel::new(None);
    vm.lock().is_recording = true;
    vm.lock().recording_duration = 12.0;
    vm.engine_recording_failed("지원하지 않습니다");
    assert!(!vm.read().is_recording);
    assert_eq!(
        vm.read().recording_status.as_ref().unwrap().message,
        "녹화 중단됨 · 지원하지 않습니다"
    );
}

#[tokio::test]
async fn stop_path_checks_writer_failure() {
    let engine = FakeEngine::new(true);
    let vm = vm_with(&engine);
    let path = std::env::temp_dir().join("cview-rec.ts");
    vm.start_recording(Some(path.clone())).await;
    assert!(vm.read().is_recording);
    engine.st().recording_failure = Some("디스크 공간 부족".into());
    vm.stop_recording().await;
    assert!(!vm.read().is_recording);
    assert_eq!(
        vm.read().recording_status.as_ref().unwrap().message,
        "녹화 저장 실패 · 디스크 공간 부족"
    );
    engine.st().recording_failure = None;
    vm.start_recording(Some(path)).await;
    vm.stop_recording().await;
    assert_eq!(
        vm.read().recording_status.as_ref().unwrap().message,
        "녹화 저장됨 · cview-rec.ts"
    );
}

// MARK: - 고급 설정

const CUSTOM: [f32; 10] = [1.0, -1.0, 2.0, -2.0, 3.0, -3.0, 4.0, -4.0, 5.0, -5.0];

#[test]
fn video_adjust_follows_settings() {
    let vm = PlayerViewModel::new(None);
    vm.lock().is_video_adjust_enabled = true;
    vm.apply_advanced_settings(&PlayerSettings {
        video_adjust_enabled: false,
        video_brightness: 1.4,
        ..Default::default()
    });
    assert!(!vm.read().is_video_adjust_enabled);
    assert_eq!(vm.read().video_brightness, 1.4);

    let engine = FakeEngine::new(true);
    vm.inject_engine(engine.clone());
    vm.apply_advanced_settings(&PlayerSettings {
        video_adjust_enabled: true,
        video_brightness: 1.2,
        video_contrast: 0.9,
        video_saturation: 1.5,
        video_hue: 30.0,
        video_gamma: 1.1,
        ..Default::default()
    });
    let s = vm.read();
    assert!(s.is_video_adjust_enabled);
    assert_eq!(
        (
            s.video_contrast,
            s.video_saturation,
            s.video_hue,
            s.video_gamma
        ),
        (0.9, 1.5, 30.0, 1.1)
    );
    assert!(engine.st().adjust.last().unwrap().enabled);
}

#[test]
fn stored_equalizer_values_win_over_preset() {
    let vm = PlayerViewModel::new(None);
    vm.apply_advanced_settings(&PlayerSettings {
        equalizer_preset: Some("록".into()),
        equalizer_pre_amp: 3.0,
        equalizer_bands: CUSTOM.to_vec(),
        ..Default::default()
    });
    let s = vm.read();
    assert_eq!(s.equalizer_pre_amp, 3.0);
    assert_eq!(s.equalizer_bands, CUSTOM.to_vec());
    assert!(s.is_equalizer_enabled);
    assert_eq!(s.equalizer_preset_name, "");
}

#[test]
fn stored_pre_amp_alone_is_restored_and_empty_leaves_state() {
    let vm = PlayerViewModel::new(None);
    vm.apply_advanced_settings(&PlayerSettings {
        equalizer_pre_amp: -4.0,
        ..Default::default()
    });
    assert!(vm.read().is_equalizer_enabled);
    assert_eq!(vm.read().equalizer_bands, vec![0.0; 10]);

    let other = PlayerViewModel::new(None);
    other.apply_advanced_settings(&PlayerSettings::default());
    assert!(!other.read().is_equalizer_enabled);
    assert!(other.read().equalizer_bands.is_empty());
}

#[test]
fn stored_bands_are_fitted() {
    let n = EqualizerPreset::FREQUENCIES.len();
    assert_eq!(equalizer_bands_fitting_frequencies(&[]), vec![0.0; n]);
    let mut padded = vec![5.0, 5.0];
    padded.resize(n, 0.0);
    assert_eq!(equalizer_bands_fitting_frequencies(&[5.0, 5.0]), padded);
    assert_eq!(
        equalizer_bands_fitting_frequencies(&vec![2.0; n + 3]).len(),
        n
    );
    assert_eq!(
        equalizer_bands_fitting_frequencies(&CUSTOM),
        CUSTOM.to_vec()
    );
}

#[test]
fn pre_amp_builds_graph_and_keeps_preset_bands() {
    let engine = FakeEngine::new(true);
    let vm = vm_with(&engine);
    vm.lock().is_equalizer_enabled = true;
    vm.set_equalizer_pre_amp(6.0);
    assert_eq!(vm.read().equalizer_bands.len(), 10);
    assert!(engine.st().eq.last().unwrap().0);

    vm.apply_equalizer_preset("록");
    let preset = vm.read().equalizer_bands.clone();
    assert_eq!(
        preset,
        EqualizerPreset::preset_named("록").unwrap().gains.to_vec()
    );
    vm.set_equalizer_pre_amp(2.0);
    assert_eq!(vm.read().equalizer_bands, preset);
    assert_eq!(vm.read().equalizer_pre_amp, 2.0);
    vm.disable_equalizer();
    assert!(!vm.read().is_equalizer_enabled);
    assert!(!engine.st().eq.last().unwrap().0);
}

// MARK: - 시작 실패

#[tokio::test]
async fn start_failure_surfaces_error_phase() {
    let engine = FakeEngine::new(true);
    engine.st().fail_play = true;
    let factory_engine = engine.clone();
    let vm = PlayerViewModel::new(Some(Arc::new(move || {
        factory_engine.clone() as SharedLiveEngine
    })));
    let settings = PlayerSettings {
        stream_proxy_mode: StreamProxyMode::None,
        ..Default::default()
    };
    let mut req =
        StartStreamRequest::new("ch1", Url::parse("http://127.0.0.1:9/live.m3u8").unwrap());
    req.player_settings = Some(settings);
    vm.start_stream(req).await;
    let s = vm.read();
    assert!(
        matches!(s.stream_phase, StreamPhase::Error(_)),
        "{:?}",
        s.stream_phase
    );
    assert!(s
        .error_message
        .as_deref()
        .unwrap()
        .starts_with("스트림 시작 실패"));
    assert!(s.coordinator().is_none());
    drop(s);
    vm.stop_stream().await;
    assert_eq!(vm.read().stream_phase, StreamPhase::Idle);
}

#[test]
fn time_formatting() {
    assert_eq!(format_time_interval(59.0), "0:59");
    assert_eq!(format_time_interval(3_661.0), "1:01:01");
    assert_eq!(format_time_interval(-1.0), "0:00");
}

#[tokio::test]
async fn start_success_follows_coordinator_events_and_stops() {
    let engine = FakeEngine::new(true);
    let factory_engine = engine.clone();
    let vm = PlayerViewModel::new(Some(Arc::new(move || {
        factory_engine.clone() as SharedLiveEngine
    })));
    let settings = PlayerSettings {
        stream_proxy_mode: StreamProxyMode::None,
        ..Default::default()
    };
    let mut req =
        StartStreamRequest::new("ch1", Url::parse("http://127.0.0.1:9/live.m3u8").unwrap());
    req.player_settings = Some(settings);
    req.channel_name = "채널".into();
    vm.start_stream(req).await;
    assert!(vm.read().coordinator().is_some());
    vm.engine_state_changed(&cview_core::models::player::PlayerPhase::Playing)
        .await;
    for _ in 0..50 {
        if vm.read().stream_phase == StreamPhase::Playing {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(vm.read().stream_phase, StreamPhase::Playing);
    assert_eq!(vm.read().current_proxy_mode, "none");
    vm.stop_stream().await;
    assert_eq!(vm.read().stream_phase, StreamPhase::Idle);
    assert!(vm.read().engine().is_none());
    assert!(!engine.st().playing);
}

#[tokio::test]
async fn stream_event_tap_sees_current_session_events() {
    let engine = FakeEngine::new(true);
    let factory_engine = engine.clone();
    let vm = PlayerViewModel::new(Some(Arc::new(move || {
        factory_engine.clone() as SharedLiveEngine
    })));
    let seen: Arc<Mutex<Vec<StreamPhase>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    vm.set_callbacks(cview_app_core::viewmodels::player::PlayerCallbacks {
        on_stream_event: Some(Arc::new(move |event| {
            if let cview_player::coordinator::StreamEvent::PhaseChanged(p) = event {
                sink.lock().unwrap().push(p.clone());
            }
        })),
        ..Default::default()
    });
    let mut req =
        StartStreamRequest::new("ch1", Url::parse("http://127.0.0.1:9/live.m3u8").unwrap());
    req.player_settings = Some(PlayerSettings {
        stream_proxy_mode: StreamProxyMode::None,
        ..Default::default()
    });
    vm.start_stream(req).await;
    vm.engine_state_changed(&cview_core::models::player::PlayerPhase::Playing)
        .await;
    for _ in 0..50 {
        if seen.lock().unwrap().contains(&StreamPhase::Playing) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(seen.lock().unwrap().contains(&StreamPhase::Playing));
    vm.stop_stream().await;
}
