//! WebView hls.js 플레이어를 감싼 엔진 — `Sources/CViewPlayer/HLSJSPlayerEngine.swift` 의 최소 대응.
//!
//! 명령(네이티브 → JS)은 [`JsCommand::script`] 한 줄을 명령 싱크로 보낸다. Tauri 앱에서 싱크는
//! `player://command` 이벤트를 메인 창으로 보내고, 프런트엔드가 플레이어 iframe 에서 그 스크립트를
//! 실행한다(Swift `evaluateJavaScript` 대응). 반대 방향(JS → 네이티브)은 `player_message` 명령으로
//! 들어와 [`EngineSnapshot::apply`] 가 상태를 갱신하고 코디네이터에 넘길 신호를 돌려준다.
//!
//! 저지연 PID 제어면(`HlsEngineControl`)은 아직 연결하지 않는다 — hls.js 페이지의 자체 지연 보정
//! (프로필 `lowLatency`)에 맡긴다. 그래서 [`CoordinatedEngine::hls`] 는 기본값 `None` 이다.

use std::ops::RangeInclusive;
use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;
use cview_core::models::player::PlayerError;
use cview_core::protocols::{PlayerEngineCapabilities, PlayerEngineProtocol};
use cview_player::coordinator::CoordinatedEngine;
use cview_player::hlsjs::{HlsJsStreamingProfile, JsCommand, JsMessage};
use url::Url;

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

/// WebView hls.js 엔진.
pub struct WebViewHlsEngine {
    sink: CommandSink,
    profile: HlsJsStreamingProfile,
    prefer_source_quality: bool,
    snapshot: Mutex<EngineSnapshot>,
    volume: Mutex<f32>,
}

impl std::fmt::Debug for WebViewHlsEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebViewHlsEngine")
            .field("profile", &self.profile)
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
            profile,
            prefer_source_quality,
            snapshot: Mutex::new(EngineSnapshot {
                rate: 1.0,
                buffer_health: 1.0,
                ..Default::default()
            }),
            volume: Mutex::new(1.0),
        }
    }

    fn snap(&self) -> MutexGuard<'_, EngineSnapshot> {
        self.snapshot.lock().unwrap_or_else(|e| e.into_inner())
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
            s.playing = false;
            s.stalled = false;
            s.error = None;
        }
        self.send(JsCommand::LoadSource {
            url: url.to_string(),
            profile: self.profile.profile_key().to_string(),
            prefer_source_quality: self.prefer_source_quality,
            lock_top_level: false,
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

impl CoordinatedEngine for WebViewHlsEngine {}

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
        assert_eq!(
            sent[0],
            r#"loadSource("http://127.0.0.1:1234/a.m3u8", "lowLatency", false, true, false, true)"#
        );
        assert_eq!(sent[1], "setVolume(1.0)");
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
