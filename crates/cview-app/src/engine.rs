//! WebView hls.js 플레이어를 감싼 엔진 — `Sources/CViewPlayer/HLSJSPlayerEngine.swift` 의 최소 대응.
//!
//! 명령(네이티브 → JS)은 [`JsCommand::script`] 한 줄을 명령 싱크로 보낸다. Tauri 앱에서 싱크는
//! `player://command` 이벤트를 메인 창으로 보내고, 프런트엔드가 플레이어 iframe 에서 그 스크립트를
//! 실행한다(Swift `evaluateJavaScript` 대응). 반대 방향(JS → 네이티브)은 `player_message` 명령으로
//! 들어와 [`EngineSnapshot::apply`] 가 상태를 갱신하고 코디네이터에 넘길 신호를 돌려준다.
//!
//! 저지연 PID 제어면([`HlsEngineControl`])과 뷰모델 설정면([`LivePlayerEngine`])도 같은 명령으로
//! 구현한다. 저지연 명령은 세대 토큰이 현재 세션과 다르면 버린다(늦게 착지한 rate/seek 차단).
//! 이퀄라이저·비디오 보정·녹화는 페이지 쪽 명령이 없어 기본 no-op 이다.

use std::ops::RangeInclusive;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use async_trait::async_trait;
use cview_app_core::viewmodels::player::engine::LivePlayerEngine;
use cview_core::models::player::PlayerError;
use cview_core::protocols::{PlayerEngineCapabilities, PlayerEngineProtocol};
use cview_player::coordinator::{CoordinatedEngine, HlsEngineControl};
use cview_player::hlsjs::{
    HlsJsForcedQualityTarget, HlsJsMatchWebV2Config, HlsJsStreamingProfile, JsCommand, JsMessage,
};
use url::Url;

/// `set_rate_awaiting_application` 이 페이지 적용을 기다리는 최대 시간.
const RATE_APPLY_TIMEOUT: Duration = Duration::from_millis(1_200);
const RATE_APPLY_POLL: Duration = Duration::from_millis(100);

/// 스크립트 한 줄을 플레이어 페이지로 보내는 싱크.
pub type CommandSink = Arc<dyn Fn(String) + Send + Sync>;

/// JS 메시지가 코디네이터에 주는 신호.
#[derive(Debug, Clone, PartialEq)]
pub enum EngineSignal {
    /// 첫 프레임 표시 또는 재버퍼 종료 — `report_engine_playing`.
    Playing,
    /// 재버퍼 시작.
    Buffering,
    /// 복구 불가 오류.
    Fatal(String),
}

/// JS 가 보고한 엔진 상태.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EngineSnapshot {
    pub page_ready: bool,
    pub playing: bool,
    pub stalled: bool,
    pub current_time: f64,
    pub duration: f64,
    pub rate: f32,
    pub latency: f64,
    pub buffer_health: f64,
    pub seekable: Option<(f64, f64)>,
    pub error: Option<String>,
    /// 마지막 metrics 시점에 페이지가 가려져 있었는가.
    pub hidden: bool,
    /// 실측 프래그먼트 길이(초). 0 이면 미측정.
    pub segment_duration: f64,
    /// 이번 재생에서 metrics 를 받았는가.
    pub has_metrics: bool,
}

impl EngineSnapshot {
    /// JS 메시지를 반영한다. 코디네이터에 알릴 것이 있으면 신호를 돌려준다.
    pub fn apply(&mut self, message: &JsMessage) -> Option<EngineSignal> {
        match message {
            JsMessage::Ready { .. } => {
                self.page_ready = true;
                None
            }
            JsMessage::PlayStarted { .. } => {
                self.playing = true;
                self.stalled = false;
                self.error = None;
                Some(EngineSignal::Playing)
            }
            JsMessage::PlaybackState { stalled } => {
                self.stalled = *stalled;
                if *stalled {
                    Some(EngineSignal::Buffering)
                } else if self.playing {
                    Some(EngineSignal::Playing)
                } else {
                    None
                }
            }
            JsMessage::Metrics(s) => {
                self.current_time = s.current_time;
                self.duration = s.media_duration;
                self.rate = s.playback_rate;
                self.latency = s.latency;
                self.buffer_health = s.buffer_health;
                self.seekable = (s.seekable_end > s.seekable_start)
                    .then_some((s.seekable_start, s.seekable_end));
                self.hidden = s.hidden;
                self.segment_duration = s.frag_duration_seconds;
                self.has_metrics = true;
                None
            }
            JsMessage::FatalError {
                error_type,
                details,
            } => {
                let text = if details.is_empty() {
                    error_type.clone()
                } else {
                    format!("{error_type}: {details}")
                };
                self.playing = false;
                self.error = Some(text.clone());
                Some(EngineSignal::Fatal(text))
            }
            _ => None,
        }
    }
}

/// 뷰모델이 정한 재생 설정 — 다음 `loadSource` 에 싣거나 즉시 명령으로 보낸다.
#[derive(Debug, Clone)]
struct EngineConfig {
    profile: HlsJsStreamingProfile,
    prefer_source_quality: bool,
    force_highest_quality: bool,
    match_web_v2: Option<HlsJsMatchWebV2Config>,
    live_latency_correction: bool,
    sharp_pixel: bool,
}

/// WebView hls.js 엔진.
pub struct WebViewHlsEngine {
    sink: CommandSink,
    config: Mutex<EngineConfig>,
    snapshot: Mutex<EngineSnapshot>,
    volume: Mutex<f32>,
    /// 저지연 제어 세대 토큰.
    latency_generation: AtomicU64,
}

impl std::fmt::Debug for WebViewHlsEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebViewHlsEngine")
            .field("config", &*self.cfg())
            .finish_non_exhaustive()
    }
}

impl WebViewHlsEngine {
    pub fn new(
        sink: CommandSink,
        profile: HlsJsStreamingProfile,
        prefer_source_quality: bool,
    ) -> Self {
        Self {
            sink,
            config: Mutex::new(EngineConfig {
                profile,
                prefer_source_quality,
                force_highest_quality: false,
                match_web_v2: None,
                live_latency_correction: true,
                sharp_pixel: false,
            }),
            snapshot: Mutex::new(EngineSnapshot {
                rate: 1.0,
                buffer_health: 1.0,
                ..Default::default()
            }),
            volume: Mutex::new(1.0),
            latency_generation: AtomicU64::new(0),
        }
    }

    fn snap(&self) -> MutexGuard<'_, EngineSnapshot> {
        self.snapshot.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn cfg(&self) -> MutexGuard<'_, EngineConfig> {
        self.config.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 현재 hls.js 프로필.
    pub fn streaming_profile(&self) -> HlsJsStreamingProfile {
        self.cfg().profile
    }

    fn generation(&self) -> u64 {
        self.latency_generation.load(Ordering::SeqCst)
    }

    fn bump_generation(&self) -> u64 {
        self.latency_generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    fn match_web_v2_literal(&self, config: Option<HlsJsMatchWebV2Config>) -> Option<String> {
        config.map(|c| c.js_object_literal(self.generation()))
    }

    /// 다음 `play` 에 실을 볼륨만 기억한다(명령은 보내지 않는다).
    pub fn set_volume_silently(&self, volume: f32) {
        *self.volume.lock().unwrap_or_else(|e| e.into_inner()) = volume.clamp(0.0, 1.0);
    }

    /// 명령 한 줄을 보낸다.
    pub fn send(&self, command: JsCommand) {
        (self.sink)(command.script());
    }

    /// JS → 네이티브 메시지 반영.
    pub fn handle_message(&self, message: &JsMessage) -> Option<EngineSignal> {
        self.snap().apply(message)
    }

    pub fn snapshot(&self) -> EngineSnapshot {
        self.snap().clone()
    }
}

#[async_trait]
impl PlayerEngineProtocol for WebViewHlsEngine {
    fn is_playing(&self) -> bool {
        let s = self.snap();
        s.playing && !s.stalled
    }

    fn current_time(&self) -> f64 {
        self.snap().current_time
    }

    fn duration(&self) -> f64 {
        self.snap().duration
    }

    fn seekable_range(&self) -> Option<RangeInclusive<f64>> {
        self.snap().seekable.map(|(a, b)| a..=b)
    }

    fn rate(&self) -> f32 {
        self.snap().rate
    }

    async fn play(&self, url: &Url) -> Result<(), PlayerError> {
        {
            let mut s = self.snap();
            *s = EngineSnapshot {
                page_ready: s.page_ready,
                rate: 1.0,
                buffer_health: 1.0,
                ..Default::default()
            };
        }
        let config = self.cfg().clone();
        let match_web_v2 = self.match_web_v2_literal(config.match_web_v2);
        self.send(JsCommand::SetMatchWebV2Config(match_web_v2));
        self.send(JsCommand::LoadSource {
            url: url.to_string(),
            profile: config.profile.profile_key().to_string(),
            prefer_source_quality: config.prefer_source_quality,
            lock_top_level: config.force_highest_quality,
            autoplay: true,
        });
        let volume = *self.volume.lock().unwrap_or_else(|e| e.into_inner());
        self.send(JsCommand::SetVolume(volume));
        Ok(())
    }

    fn pause(&self) {
        self.send(JsCommand::Pause);
    }

    fn resume(&self) {
        self.send(JsCommand::Play);
    }

    fn stop(&self) {
        self.snap().playing = false;
        self.send(JsCommand::Stop);
    }

    fn seek(&self, position: f64) {
        self.send(JsCommand::Seek(position));
    }

    fn set_rate(&self, rate: f32) {
        self.send(JsCommand::SetRate(rate));
    }

    fn set_volume(&self, volume: f32) {
        *self.volume.lock().unwrap_or_else(|e| e.into_inner()) = volume.clamp(0.0, 1.0);
        self.send(JsCommand::SetVolume(volume.clamp(0.0, 1.0)));
    }

    fn is_in_error_state(&self) -> bool {
        self.snap().error.is_some()
    }

    fn reset_retries(&self) {
        self.snap().error = None;
    }

    fn capabilities(&self) -> PlayerEngineCapabilities {
        PlayerEngineCapabilities::SUSPEND_LOADING
    }
}

impl CoordinatedEngine for WebViewHlsEngine {
    fn hls(&self) -> Option<&dyn HlsEngineControl> {
        Some(self)
    }
}

#[async_trait]
impl HlsEngineControl for WebViewHlsEngine {
    fn invalidate_latency_control_session(&self) -> u64 {
        self.bump_generation()
    }

    fn begin_latency_control_session(&self) -> u64 {
        self.bump_generation()
    }

    fn is_latency_control_session_current(&self, generation: u64) -> bool {
        self.generation() == generation
    }

    fn set_rate_for_latency(&self, rate: f32, generation: u64) {
        if self.is_latency_control_session_current(generation) {
            self.send(JsCommand::SetRate(rate));
        }
    }

    async fn set_rate_awaiting_application(&self, rate: f32, generation: u64) -> bool {
        if !self.is_latency_control_session_current(generation) {
            return false;
        }
        self.send(JsCommand::SetRate(rate));
        // 페이지는 metrics 로만 실제 배속을 알린다 — 반영되거나 시간이 다 될 때까지 기다린다.
        let mut waited = Duration::ZERO;
        while waited < RATE_APPLY_TIMEOUT {
            {
                let s = self.snap();
                if !s.has_metrics || (s.rate - rate).abs() < 0.01 {
                    break;
                }
            }
            tokio::time::sleep(RATE_APPLY_POLL).await;
            waited += RATE_APPLY_POLL;
        }
        self.is_latency_control_session_current(generation)
    }

    fn seek_to_latency(&self, target_seconds: f64, generation: u64) {
        if self.is_latency_control_session_current(generation) {
            self.send(JsCommand::SeekToLatency(target_seconds));
        }
    }

    fn set_latency_owned_by_controller(&self, owned: bool, generation: u64) {
        if self.is_latency_control_session_current(generation) {
            self.send(JsCommand::SetLatencyOwnedByController(owned));
        }
    }

    fn set_controller_target_latency(&self, seconds: f64, generation: u64) {
        if self.is_latency_control_session_current(generation) {
            self.send(JsCommand::SetControllerTargetLatency(seconds));
        }
    }

    fn set_live_latency_correction_enabled(&self, enabled: bool) {
        self.cfg().live_latency_correction = enabled;
        self.send(JsCommand::SetLiveLatencyCorrectionEnabled(enabled));
    }

    fn is_live_latency_correction_enabled(&self) -> bool {
        self.cfg().live_latency_correction
    }

    fn is_loading_suspended(&self) -> bool {
        false
    }

    fn live_latency_seconds(&self) -> f64 {
        self.snap().latency
    }

    fn measured_segment_duration(&self) -> f64 {
        self.snap().segment_duration
    }

    fn buffer_health(&self) -> f64 {
        self.snap().buffer_health
    }

    fn latency_sample_is_actionable(&self) -> bool {
        let s = self.snap();
        s.has_metrics && !s.hidden
    }

    fn is_match_web_v2_active(&self) -> bool {
        self.cfg().match_web_v2.is_some()
    }

    fn match_web_v2_config(&self) -> Option<HlsJsMatchWebV2Config> {
        self.cfg().match_web_v2
    }

    fn set_match_web_v2_targets(&self, hls: f64, ingest: f64, generation: u64) {
        if self.is_latency_control_session_current(generation) {
            self.send(JsCommand::SetMatchWebV2Targets { hls, ingest });
        }
    }

    fn seek_timeshift(&self, seconds_behind: f64) {
        self.send(JsCommand::SeekTimeshift(seconds_behind));
    }
}

/// 뷰모델 설정면 — 설정은 기억해 두었다가 다음 `loadSource` 에 싣고, 재생 중이면 바로 보낸다.
impl LivePlayerEngine for WebViewHlsEngine {
    fn is_hlsjs(&self) -> bool {
        true
    }

    fn set_streaming_profile(&self, profile: HlsJsStreamingProfile) {
        self.cfg().profile = profile;
    }

    fn set_match_web_v2_config(&self, config: Option<HlsJsMatchWebV2Config>) {
        self.cfg().match_web_v2 = config;
    }

    fn set_prefers_source_quality(&self, prefers: bool) {
        self.cfg().prefer_source_quality = prefers;
        self.send(JsCommand::SetPreferSourceQuality(prefers));
    }

    fn set_user_quality_ceiling_height(&self, height: i64) {
        self.send(JsCommand::SetAdaptiveCaps {
            max_resolution: height.max(0),
            max_bitrate: 0,
        });
    }

    fn set_force_highest_quality(&self, enabled: bool) {
        self.cfg().force_highest_quality = enabled;
        self.send(JsCommand::SetLockTopLevel(enabled));
    }

    fn force_highest_quality(&self) -> bool {
        self.cfg().force_highest_quality
    }

    fn set_forced_quality(&self, target: Option<HlsJsForcedQualityTarget>) {
        let t = target.unwrap_or(HlsJsForcedQualityTarget::new(0, 0));
        self.send(JsCommand::SetForcedHeight {
            height: t.height,
            target_bitrate_kbps: t.target_bitrate_kbps,
        });
    }

    fn force_live_resync(&self) {
        self.send(JsCommand::ForceLiveResync);
    }

    fn seek_to_live_edge(&self) {
        self.send(JsCommand::SeekToLiveEdge);
    }

    fn effective_live_target_latency_seconds(&self) -> f64 {
        let c = self.cfg();
        match c.match_web_v2 {
            Some(v2) => v2.hls_target_seconds,
            None => c.profile.target_live_latency_seconds(),
        }
    }

    fn set_video_surface_visible(&self, visible: bool) {
        if visible {
            self.send(JsCommand::NoteSurfaceShown);
        }
    }

    fn set_sharp_pixel_scaling(&self, enabled: bool) {
        self.cfg().sharp_pixel = enabled;
        self.send(JsCommand::SetSharpPixelScaling(enabled));
    }

    fn sharp_pixel_scaling_enabled(&self) -> bool {
        self.cfg().sharp_pixel
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn engine() -> (WebViewHlsEngine, Arc<Mutex<Vec<String>>>) {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let log = sent.clone();
        let sink: CommandSink = Arc::new(move |s| log.lock().unwrap().push(s));
        (
            WebViewHlsEngine::new(sink, HlsJsStreamingProfile::LowLatency, true),
            sent,
        )
    }

    fn parse(v: serde_json::Value) -> JsMessage {
        JsMessage::parse(&v, 0.0).unwrap().1
    }

    #[tokio::test]
    async fn play_sends_load_source_script() {
        let (e, sent) = engine();
        e.play(&Url::parse("http://127.0.0.1:1234/a.m3u8").unwrap())
            .await
            .unwrap();
        let sent = sent.lock().unwrap();
        assert_eq!(sent[0], "setMatchWebV2Config(null)");
        assert_eq!(
            sent[1],
            r#"loadSource("http://127.0.0.1:1234/a.m3u8", "lowLatency", false, true, false, true)"#
        );
        assert_eq!(sent[2], "setVolume(1.0)");
    }

    #[tokio::test]
    async fn view_model_settings_ride_on_next_load_source() {
        let (e, sent) = engine();
        e.set_streaming_profile(HlsJsStreamingProfile::WebParity);
        e.set_force_highest_quality(true);
        assert!(e.force_highest_quality());
        assert!(e.is_hlsjs());
        e.play(&Url::parse("http://127.0.0.1:1/b.m3u8").unwrap())
            .await
            .unwrap();
        let sent = sent.lock().unwrap();
        assert!(sent.contains(&"setLockTopLevel(true)".to_string()));
        assert!(sent.iter().any(|s| s
            == r#"loadSource("http://127.0.0.1:1/b.m3u8", "webParity", false, true, true, true)"#));
        assert_eq!(e.effective_live_target_latency_seconds(), 3.0);
    }

    #[tokio::test]
    async fn latency_commands_are_gated_by_generation() {
        let (e, sent) = engine();
        assert!(e.hls().is_some());
        let stale = e.begin_latency_control_session();
        let current = e.begin_latency_control_session();
        assert!(!e.is_latency_control_session_current(stale));
        e.set_rate_for_latency(1.5, stale);
        e.seek_to_latency(3.0, stale);
        assert!(sent.lock().unwrap().is_empty());
        e.set_rate_for_latency(1.5, current);
        e.seek_to_latency(3.0, current);
        e.set_latency_owned_by_controller(true, current);
        e.set_controller_target_latency(4.5, current);
        // metrics 를 받기 전이면 적용 확인을 기다리지 않는다.
        assert!(e.set_rate_awaiting_application(1.0, current).await);
        assert!(!e.set_rate_awaiting_application(1.0, stale).await);
        let sent = sent.lock().unwrap();
        assert_eq!(
            *sent,
            vec![
                "setRate(1.5)",
                "seekToLatency(3.0)",
                "setLatencyOwnedByController(true)",
                "setControllerTargetLatency(4.5)",
                "setRate(1.0)",
            ]
        );
    }

    #[test]
    fn messages_drive_signals() {
        let (e, _) = engine();
        assert_eq!(
            e.handle_message(&parse(json!({"type":"ready","data":{}}))),
            None
        );
        assert!(e.snapshot().page_ready);
        assert_eq!(
            e.handle_message(&parse(
                json!({"type":"playStarted","data":{"muted":false,"volume":1}})
            )),
            Some(EngineSignal::Playing)
        );
        assert!(e.is_playing());
        assert_eq!(
            e.handle_message(&parse(
                json!({"type":"playbackState","data":{"state":"stalled"}})
            )),
            Some(EngineSignal::Buffering)
        );
        assert!(!e.is_playing());
        assert_eq!(
            e.handle_message(&parse(
                json!({"type":"playbackState","data":{"state":"playing"}})
            )),
            Some(EngineSignal::Playing)
        );
        e.handle_message(&parse(json!({"type":"metrics","data":{
            "currentTime": 12.5, "latency": 3.2, "seekableStart": 1.0, "seekableEnd": 20.0
        }})));
        assert_eq!(e.current_time(), 12.5);
        assert_eq!(e.seekable_range(), Some(1.0..=20.0));
        let fatal = e.handle_message(&parse(
            json!({"type":"fatalError","data":{"type":"networkError","details":"manifestLoadError"}}),
        ));
        assert_eq!(
            fatal,
            Some(EngineSignal::Fatal(
                "networkError: manifestLoadError".into()
            ))
        );
        assert!(e.is_in_error_state());
        e.reset_retries();
        assert!(!e.is_in_error_state());
    }
}
