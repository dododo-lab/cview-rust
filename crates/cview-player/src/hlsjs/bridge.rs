//! `Sources/CViewPlayer/HLSJSVideoView.swift`(메시지 타입·`handleMessage`·`parsePlaybackSnapshot`·
//! Swift→JS API) · `HLSJSVideoView+MatchWebV2.swift`(v2 메시지 파싱) 포팅 — hls.js 페이지와의 브리지 프로토콜.
//!
//! JS 는 `postToSwift(type, data)` 로 `{ type, data }` 를 보낸다(`Resources/hlsjs-player.html`).
//! Tauri 에서는 같은 객체가 IPC 로 온다. 이 모듈은 그 객체를 [`JsMessage`] 로 풀고, 반대 방향의
//! 전역 함수 호출(`setRate(1.0)` 등)을 [`JsCommand::script`] 로 만든다.
//!
//! - 파싱은 Swift 의 `as? Bool`/`as? NSNumber` 처럼 관대하게 한다(불리언 자리에 숫자가 와도 읽는다).
//! - 문자열 리터럴은 JSON 인코딩으로 만든다. Swift `JSONEncoder` 는 `/` 를 `\/` 로 바꾸지만
//!   serde_json 은 그대로 둔다 — 둘 다 같은 JS 문자열이다.

use std::collections::BTreeMap;

use cview_core::models::player::PlayerPhase;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::network_metrics::HlsJsNetworkMetrics;

// MARK: - 값 읽기 도우미

fn number(data: &Map<String, Value>, key: &str) -> Option<f64> {
    match data.get(key)? {
        Value::Number(n) => n.as_f64().filter(|v| v.is_finite()),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn double(data: &Map<String, Value>, key: &str, fallback: f64) -> f64 {
    number(data, key).unwrap_or(fallback)
}

/// Swift `as? Int` — 정수로 정확히 표현되는 숫자만.
fn int(data: &Map<String, Value>, key: &str) -> Option<i64> {
    match data.get(key)? {
        Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_f64().filter(|v| v.fract() == 0.0).map(|v| v as i64)),
        _ => None,
    }
}

/// Swift `(x as? NSNumber)?.intValue` — 소수는 버림.
fn int_lenient(data: &Map<String, Value>, key: &str, fallback: i64) -> i64 {
    number(data, key).map(|v| v as i64).unwrap_or(fallback)
}

fn boolean(data: &Map<String, Value>, key: &str) -> Option<bool> {
    match data.get(key)? {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => n.as_f64().map(|v| v != 0.0),
        _ => None,
    }
}

fn string(data: &Map<String, Value>, key: &str) -> Option<String> {
    data.get(key)?.as_str().map(str::to_string)
}

// MARK: - 메시지 타입

/// JS → 네이티브 메시지 타입 (Swift `HLSJSMessageType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JsMessageType {
    Ready,
    Metrics,
    Error,
    Diagnostic,
    DiagnosticBatch,
    FatalError,
    PlayStarted,
    CodecDiagnostics,
    ForcedHeight,
    Levels,
    Screenshot,
    Recording,
    Equalizer,
    PlaybackState,
    SurfaceFirstFrame,
    V2SeekTokenRequest,
    V2Event,
}

impl JsMessageType {
    pub const ALL: [JsMessageType; 17] = [
        Self::Ready,
        Self::Metrics,
        Self::Error,
        Self::Diagnostic,
        Self::DiagnosticBatch,
        Self::FatalError,
        Self::PlayStarted,
        Self::CodecDiagnostics,
        Self::ForcedHeight,
        Self::Levels,
        Self::Screenshot,
        Self::Recording,
        Self::Equalizer,
        Self::PlaybackState,
        Self::SurfaceFirstFrame,
        Self::V2SeekTokenRequest,
        Self::V2Event,
    ];

    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Metrics => "metrics",
            Self::Error => "error",
            Self::Diagnostic => "diagnostic",
            Self::DiagnosticBatch => "diagnosticBatch",
            Self::FatalError => "fatalError",
            Self::PlayStarted => "playStarted",
            Self::CodecDiagnostics => "codecDiagnostics",
            Self::ForcedHeight => "forcedHeight",
            Self::Levels => "levels",
            Self::Screenshot => "screenshot",
            Self::Recording => "recording",
            Self::Equalizer => "equalizer",
            Self::PlaybackState => "playbackState",
            Self::SurfaceFirstFrame => "surfaceFirstFrame",
            Self::V2SeekTokenRequest => "v2SeekTokenRequest",
            Self::V2Event => "v2Event",
        }
    }

    pub fn from_raw(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.raw_value() == raw)
    }

    /// 재생 phase 를 바꾸는 메시지 — `playStarted` 하나뿐이다(Swift `playbackStateTransition`).
    pub fn playback_state_transition(self) -> Option<PlayerPhase> {
        (self == Self::PlayStarted).then_some(PlayerPhase::Playing)
    }
}

// MARK: - metrics 스냅샷

/// metrics 끝의 matchWeb v2 필드 11개 (Swift `HLSJSMatchWebV2Metrics`). v2 가 아니면 [`Self::OFF`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HlsJsMatchWebV2Metrics {
    pub ingest_latency_ms: f64,
    pub pdt_latency_ms: f64,
    pub ll_active: bool,
    pub part_duration_seconds: f64,
    pub v2_state: String,
    pub v2_target_ms: f64,
    pub v2_hls_target_ms: f64,
    pub v2_seek_count: i64,
    pub v2_last_seek_loss_ms: f64,
    pub v2_loss_comp_ms: f64,
    pub ingest_hls_offset_ms: f64,
}

impl Default for HlsJsMatchWebV2Metrics {
    fn default() -> Self {
        Self::OFF
    }
}

impl HlsJsMatchWebV2Metrics {
    pub const OFF: Self = Self {
        ingest_latency_ms: -1.0,
        pdt_latency_ms: -1.0,
        ll_active: false,
        part_duration_seconds: 0.0,
        v2_state: String::new(),
        v2_target_ms: -1.0,
        v2_hls_target_ms: -1.0,
        v2_seek_count: 0,
        v2_last_seek_loss_ms: -1.0,
        v2_loss_comp_ms: -1.0,
        ingest_hls_offset_ms: -1.0,
    };

    /// metrics 사전에서 읽는다. 키가 없거나 형이 다르면 꺼짐 값.
    pub fn parse(data: &Map<String, Value>) -> Self {
        Self {
            ingest_latency_ms: double(data, "ingestLatencyMs", -1.0),
            pdt_latency_ms: double(data, "pdtLatencyMs", -1.0),
            ll_active: boolean(data, "llActive").unwrap_or(false),
            part_duration_seconds: double(data, "partDurationSeconds", 0.0),
            v2_state: string(data, "v2State").unwrap_or_default(),
            v2_target_ms: double(data, "v2TargetMs", -1.0),
            v2_hls_target_ms: double(data, "v2HlsTargetMs", -1.0),
            v2_seek_count: int_lenient(data, "v2SeekCount", 0),
            v2_last_seek_loss_ms: double(data, "v2LastSeekLossMs", -1.0),
            v2_loss_comp_ms: double(data, "v2LossCompMs", -1.0),
            ingest_hls_offset_ms: double(data, "ingestHlsOffsetMs", -1.0),
        }
    }
}

/// 2초 metrics 하트비트 한 장 (Swift `HLSJSPlaybackSnapshot`).
#[derive(Debug, Clone, PartialEq)]
pub struct HlsJsPlaybackSnapshot {
    pub current_time: f64,
    pub playback_rate: f32,
    pub latency: f64,
    pub buffer_health: f64,
    pub media_duration: f64,
    pub seekable_start: f64,
    pub seekable_end: f64,
    pub paused: bool,
    /// 이 스냅샷 시점에 페이지가 가려져 있었는가(`document.hidden`).
    pub hidden: bool,
    /// 현재 재생 레벨의 실제 비트레이트(bps).
    pub bitrate: f64,
    pub bandwidth_estimate: f64,
    pub buffer_ahead_seconds: f64,
    pub frag_ttfb_p50: f64,
    pub frag_ttfb_p90: f64,
    pub frag_total_p50: f64,
    pub frag_total_p90: f64,
    pub frag_samples: i64,
    /// 실측 프래그먼트 길이(초) 중앙값. 0 이면 미측정.
    pub frag_duration_seconds: f64,
    pub pl_end_sn_age_ms: f64,
    pub pl_reload_p50_ms: f64,
    pub pl_reload_p90_ms: f64,
    pub pl_target_duration: f64,
    pub pl_stale_ratio: f64,
    pub dropped_frames_delta: i64,
    pub decoded_frames_delta: i64,
    pub decoded_frames_total: i64,
    pub dropped_frames_total: i64,
    pub zero_decode_windows: i64,
    pub dropped_frame_ratio: f64,
    pub decoder_pressure_cap: i64,
    pub match_web_v2: HlsJsMatchWebV2Metrics,
    pub network_metrics: HlsJsNetworkMetrics,
}

impl HlsJsPlaybackSnapshot {
    /// Swift `parsePlaybackSnapshot(_:)`. `sampled_at` 은 네트워크 메트릭 표본 시각(단조 초).
    pub fn parse(data: &Map<String, Value>, sampled_at: f64) -> Self {
        let d = |k: &str| double(data, k, 0.0);
        let i = |k: &str, f: i64| int_lenient(data, k, f);
        Self {
            current_time: d("currentTime"),
            playback_rate: double(data, "playbackRate", 1.0) as f32,
            latency: d("latency"),
            buffer_health: d("bufferHealth"),
            media_duration: d("mediaDuration"),
            seekable_start: d("seekableStart"),
            seekable_end: d("seekableEnd"),
            paused: boolean(data, "paused").unwrap_or(false),
            hidden: boolean(data, "hidden").unwrap_or(false),
            bitrate: d("currentLevelBitrateBps"),
            bandwidth_estimate: d("bandwidthEstimateBps"),
            buffer_ahead_seconds: d("bufferAheadSeconds"),
            frag_ttfb_p50: d("fragTtfbP50"),
            frag_ttfb_p90: d("fragTtfbP90"),
            frag_total_p50: d("fragTotalP50"),
            frag_total_p90: d("fragTotalP90"),
            frag_samples: i("fragSamples", 0),
            frag_duration_seconds: d("fragDurationSeconds"),
            pl_end_sn_age_ms: double(data, "plEndSnAgeMs", -1.0),
            pl_reload_p50_ms: d("plReloadP50Ms"),
            pl_reload_p90_ms: d("plReloadP90Ms"),
            pl_target_duration: d("plTargetDuration"),
            pl_stale_ratio: d("plStaleRatio"),
            dropped_frames_delta: i("droppedFramesDelta", 0),
            decoded_frames_delta: i("decodedFramesDelta", 0),
            decoded_frames_total: i("decodedFramesTotal", -1),
            dropped_frames_total: i("droppedFramesTotal", -1),
            zero_decode_windows: i("zeroDecodeWindows", 0),
            dropped_frame_ratio: d("droppedFrameRatio"),
            decoder_pressure_cap: i("decoderPressureCap", -1),
            match_web_v2: HlsJsMatchWebV2Metrics::parse(data),
            network_metrics: data
                .get("networkMetrics")
                .map(|v| HlsJsNetworkMetrics::from_value(v, sampled_at))
                .unwrap_or_else(HlsJsNetworkMetrics::empty),
        }
    }

    /// 실측 구간 길이로 나눈 디코드 프레임율. 구간이 0 이거나 델타가 음수면 `None`.
    pub fn decoded_fps(&self, interval_seconds: f64) -> Option<f64> {
        (interval_seconds > 0.0 && self.decoded_frames_delta >= 0)
            .then(|| self.decoded_frames_delta as f64 / interval_seconds)
    }

    /// 이 구간의 드롭 비율(%).
    pub fn dropped_percent(&self) -> Option<f64> {
        (self.decoded_frames_delta > 0
            && self.dropped_frames_delta >= 0
            && self.dropped_frames_delta <= self.decoded_frames_delta)
            .then(|| self.dropped_frames_delta as f64 / self.decoded_frames_delta as f64 * 100.0)
    }
}

// MARK: - 레벨

/// hls.js levels 배열의 한 항목 (Swift `HLSJSLevelInfo`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HlsJsLevelInfo {
    pub index: i64,
    pub width: i64,
    pub height: i64,
    pub bitrate: i64,
    pub name: String,
}

// MARK: - matchWeb v2 메시지

/// v2Event 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MatchWebV2EventKind {
    Seek,
    SeekSkipped,
    SeekResult,
    Fallback,
    Warn,
}

impl MatchWebV2EventKind {
    pub fn from_raw(raw: &str) -> Option<Self> {
        Some(match raw {
            "seek" => Self::Seek,
            "seekSkipped" => Self::SeekSkipped,
            "seekResult" => Self::SeekResult,
            "fallback" => Self::Fallback,
            "warn" => Self::Warn,
            _ => return None,
        })
    }
}

/// JS → 네이티브 v2 메시지(§3.2).
#[derive(Debug, Clone, PartialEq)]
pub enum MatchWebV2Message {
    SeekTokenRequest {
        request_id: i64,
        reason: String,
        err_ms: f64,
    },
    Event {
        kind: MatchWebV2EventKind,
        request_id: Option<i64>,
        reason: Option<String>,
        fields: BTreeMap<String, f64>,
    },
}

/// 공통 키(sessionId·pageGen)를 두른 v2 메시지.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchWebV2Envelope {
    pub session_id: u64,
    pub page_gen: u64,
    pub message: MatchWebV2Message,
}

const V2_RESERVED_KEYS: [&str; 6] = ["kind", "reason", "why", "requestId", "sessionId", "pageGen"];

/// 숫자 → u64. 없거나 음수·비유한이면 0.
pub fn match_web_v2_u64(value: Option<&Value>) -> u64 {
    match value.and_then(Value::as_f64) {
        Some(v) if v.is_finite() && v > 0.0 => v as u64,
        _ => 0,
    }
}

/// §3.2 매핑 표. 모르는 kind·필수 키 누락이면 `None`(메시지 무시).
pub fn parse_match_web_v2_message(
    message_type: JsMessageType,
    data: &Map<String, Value>,
) -> Option<MatchWebV2Envelope> {
    let message = match message_type {
        JsMessageType::V2SeekTokenRequest => MatchWebV2Message::SeekTokenRequest {
            request_id: int_lenient_opt(data, "requestId")?,
            reason: string(data, "reason").unwrap_or_default(),
            err_ms: double(data, "errMs", 0.0),
        },
        JsMessageType::V2Event => {
            let kind = MatchWebV2EventKind::from_raw(data.get("kind")?.as_str()?)?;
            let request_id = matches!(
                kind,
                MatchWebV2EventKind::Seek | MatchWebV2EventKind::SeekSkipped
            )
            .then(|| int_lenient_opt(data, "requestId"))
            .flatten();
            let reason = match kind {
                MatchWebV2EventKind::Seek
                | MatchWebV2EventKind::Fallback
                | MatchWebV2EventKind::Warn => string(data, "reason"),
                MatchWebV2EventKind::SeekSkipped => string(data, "why"),
                MatchWebV2EventKind::SeekResult => None,
            };
            // 예약 키를 뺀 나머지 중 숫자 값만(불리언은 1/0). null·문자열은 넣지 않는다.
            let fields = data
                .iter()
                .filter(|(k, _)| !V2_RESERVED_KEYS.contains(&k.as_str()))
                .filter_map(|(k, _)| number(data, k).map(|v| (k.clone(), v)))
                .collect();
            MatchWebV2Message::Event {
                kind,
                request_id,
                reason,
                fields,
            }
        }
        _ => return None,
    };
    Some(MatchWebV2Envelope {
        session_id: match_web_v2_u64(data.get("sessionId")),
        page_gen: match_web_v2_u64(data.get("pageGen")),
        message,
    })
}

fn int_lenient_opt(data: &Map<String, Value>, key: &str) -> Option<i64> {
    number(data, key).map(|v| v as i64)
}

// MARK: - 메시지

/// 해석한 JS → 네이티브 메시지.
#[derive(Debug, Clone, PartialEq)]
pub enum JsMessage {
    Ready {
        hls_supported: bool,
        /// 0 이면 세대 스크립트가 없는 구 페이지(필터하지 않는다).
        page_gen: u64,
    },
    Metrics(Box<HlsJsPlaybackSnapshot>),
    Error {
        fatal: bool,
        recoverable: bool,
        error_type: String,
        details: String,
        /// 진단 필드 원본(`HLSJSDiagnostics.fields` 입력).
        raw: Map<String, Value>,
    },
    Diagnostic(Map<String, Value>),
    /// 최대 64건.
    DiagnosticBatch(Vec<Map<String, Value>>),
    FatalError {
        error_type: String,
        details: String,
    },
    PlayStarted {
        muted: bool,
        volume: f64,
    },
    CodecDiagnostics {
        reason: String,
        mse_kind: String,
        tracks: String,
        levels: String,
        level_count: i64,
        current_level: i64,
        audio_tracks: i64,
    },
    ForcedHeight {
        height: i64,
        level: i64,
    },
    Levels {
        items: Vec<HlsJsLevelInfo>,
        current: i64,
        capped: i64,
    },
    Screenshot {
        ok: bool,
        /// base64 원문(디코드는 호출자 몫).
        base64: Option<String>,
        format: String,
        width: i64,
        height: i64,
        error: Option<String>,
    },
    Recording {
        ok: bool,
        state: String,
        mime_type: Option<String>,
        duration_sec: f64,
        size_bytes: i64,
        /// `chunk` 일 때만 실린 base64 원문.
        base64: Option<String>,
        seq: Option<i64>,
        error: Option<String>,
    },
    Equalizer {
        enabled: bool,
        band_count: i64,
        error: Option<String>,
    },
    /// 재버퍼 시작(true)/종료(false). JS 가 이미 디바운스·seek 제외를 마쳤다.
    PlaybackState {
        stalled: bool,
    },
    SurfaceFirstFrame {
        timed_out: bool,
        fallback: bool,
    },
    MatchWebV2(MatchWebV2Envelope),
}

impl JsMessage {
    /// `{ type, data }` 객체를 푼다. 모르는 타입이면 `None`. `sampled_at` 은 metrics 표본 시각.
    pub fn parse(message: &Value, sampled_at: f64) -> Option<(JsMessageType, JsMessage)> {
        let obj = message.as_object()?;
        let message_type = JsMessageType::from_raw(obj.get("type")?.as_str()?)?;
        let empty = Map::new();
        let data = obj.get("data").and_then(Value::as_object).unwrap_or(&empty);
        Self::from_data(message_type, data, sampled_at).map(|m| (message_type, m))
    }

    pub fn from_data(
        message_type: JsMessageType,
        data: &Map<String, Value>,
        sampled_at: f64,
    ) -> Option<JsMessage> {
        let s = |k: &str, f: &str| string(data, k).unwrap_or_else(|| f.to_string());
        let b = |k: &str| boolean(data, k).unwrap_or(false);
        Some(match message_type {
            JsMessageType::Ready => JsMessage::Ready {
                hls_supported: b("hlsSupported"),
                page_gen: int(data, "pageGen").filter(|v| *v > 0).unwrap_or(0) as u64,
            },
            JsMessageType::Metrics => {
                JsMessage::Metrics(Box::new(HlsJsPlaybackSnapshot::parse(data, sampled_at)))
            }
            JsMessageType::Error => JsMessage::Error {
                fatal: b("fatal"),
                recoverable: b("recoverable"),
                error_type: s("type", "unknown"),
                details: s("details", ""),
                raw: data.clone(),
            },
            JsMessageType::Diagnostic => JsMessage::Diagnostic(data.clone()),
            JsMessageType::DiagnosticBatch => JsMessage::DiagnosticBatch(
                data.get("events")
                    .and_then(Value::as_array)
                    .filter(|a| a.iter().all(Value::is_object))
                    .map(|a| {
                        a.iter()
                            .take(64)
                            .filter_map(|v| v.as_object().cloned())
                            .collect()
                    })
                    .unwrap_or_default(),
            ),
            JsMessageType::FatalError => JsMessage::FatalError {
                error_type: s("type", "unknown"),
                details: s("details", ""),
            },
            JsMessageType::PlayStarted => JsMessage::PlayStarted {
                muted: b("muted"),
                volume: double(data, "volume", 1.0),
            },
            JsMessageType::CodecDiagnostics => JsMessage::CodecDiagnostics {
                reason: s("reason", "initial"),
                mse_kind: s("mseKind", "?"),
                tracks: s("tracks", "?"),
                levels: s("levels", "?"),
                level_count: int(data, "levelCount").unwrap_or(-1),
                current_level: int(data, "currentLevel").unwrap_or(-1),
                audio_tracks: int(data, "audioTracks").unwrap_or(-1),
            },
            JsMessageType::ForcedHeight => JsMessage::ForcedHeight {
                height: int(data, "height").unwrap_or(0),
                level: int(data, "level").unwrap_or(-1),
            },
            JsMessageType::Levels => {
                let items = data
                    .get("levels")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| {
                                let d = v.as_object()?;
                                let height = int(d, "height")?;
                                Some(HlsJsLevelInfo {
                                    index: int(d, "index")?,
                                    width: int(d, "width")?,
                                    height,
                                    bitrate: int(d, "bitrate")?,
                                    name: string(d, "name").unwrap_or_else(|| format!("{height}p")),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                JsMessage::Levels {
                    items,
                    current: int(data, "current").unwrap_or(-1),
                    capped: int(data, "capped").unwrap_or(-1),
                }
            }
            JsMessageType::Screenshot => {
                let ok = b("ok");
                JsMessage::Screenshot {
                    ok,
                    base64: ok.then(|| string(data, "base64")).flatten(),
                    format: s("format", "image/png"),
                    width: int(data, "width").unwrap_or(0),
                    height: int(data, "height").unwrap_or(0),
                    error: (!ok).then(|| string(data, "error")).flatten(),
                }
            }
            JsMessageType::Recording => {
                let ok = b("ok");
                let state = s("state", "unknown");
                JsMessage::Recording {
                    ok,
                    base64: (ok && state == "chunk")
                        .then(|| string(data, "base64"))
                        .flatten(),
                    state,
                    mime_type: string(data, "mimeType"),
                    duration_sec: number(data, "durationSec").unwrap_or(0.0),
                    size_bytes: int(data, "sizeBytes").unwrap_or(0),
                    seq: int(data, "seq"),
                    error: (!ok).then(|| string(data, "error")).flatten(),
                }
            }
            JsMessageType::Equalizer => {
                let ok = b("ok");
                JsMessage::Equalizer {
                    enabled: b("enabled"),
                    band_count: int(data, "bandCount").unwrap_or(0),
                    error: (!ok).then(|| string(data, "error")).flatten(),
                }
            }
            JsMessageType::PlaybackState => JsMessage::PlaybackState {
                stalled: string(data, "state").as_deref() == Some("stalled"),
            },
            JsMessageType::SurfaceFirstFrame => JsMessage::SurfaceFirstFrame {
                timed_out: b("timedOut"),
                fallback: b("fallback"),
            },
            JsMessageType::V2SeekTokenRequest | JsMessageType::V2Event => {
                JsMessage::MatchWebV2(parse_match_web_v2_message(message_type, data)?)
            }
        })
    }
}

/// 이전 문서의 늦은 메시지인가 — `pageGen > 0` 이고 현재 세대와 다르면 버린다(0 은 구 페이지 호환).
pub fn is_stale_page_message(page_gen: u64, current_page_generation: u64) -> bool {
    page_gen > 0 && page_gen != current_page_generation
}

// MARK: - 네이티브 → JS

/// JS 문자열 리터럴(JSON 인코딩).
pub fn js_string_literal(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into())
}

/// JS 숫자 리터럴. 비유한 값은 0.
pub fn js_number(value: f64) -> String {
    if value.is_finite() {
        format!("{value:?}")
    } else {
        "0".into()
    }
}

/// 페이지 전역 함수 호출 (Swift `evaluateJS("…")` 의 대상).
#[derive(Debug, Clone, PartialEq)]
pub enum JsCommand {
    LoadSource {
        url: String,
        profile: String,
        prefer_source_quality: bool,
        lock_top_level: bool,
        autoplay: bool,
    },
    Play,
    Pause,
    Stop,
    Seek(f64),
    SetRate(f32),
    SetVolume(f32),
    SetMuted(bool),
    SeekToLatency(f64),
    SeekToLiveEdge,
    ForceLiveResync,
    SeekByDriftMs(f64),
    SeekTimeshift(f64),
    SuspendLoading,
    ResumeLoading,
    GetLevels,
    NoteSurfaceShown,
    SetLatencyOwnedByController(bool),
    SetControllerTargetLatency(f64),
    SetLiveLatencyCorrectionEnabled(bool),
    /// `None` = v2 끔(`null`). 값은 [`super::HlsJsMatchWebV2Config::js_object_literal`].
    SetMatchWebV2Config(Option<String>),
    SetMatchWebV2Targets {
        hls: f64,
        ingest: f64,
    },
    GrantMatchWebV2Seek(i64),
    SetAdaptiveCaps {
        max_resolution: i64,
        max_bitrate: i64,
    },
    SetForcedHeight {
        height: i64,
        target_bitrate_kbps: i64,
    },
    SetLockTopLevel(bool),
    SetPreferSourceQuality(bool),
    SetBufferCaps {
        max_len: f64,
        max_max_len: f64,
        max_size: i64,
    },
    SetSharpPixelScaling(bool),
    SetForcedAspectRatio(bool),
    TakeScreenshot {
        mime_type: String,
        quality: f64,
    },
    StartRecording {
        mime_type: Option<String>,
        timeslice_ms: i64,
    },
    StopRecording,
}

impl JsCommand {
    /// 평가할 스크립트 한 줄.
    pub fn script(&self) -> String {
        use JsCommand::*;
        let n = |v: f64| js_number(v);
        match self {
            LoadSource {
                url,
                profile,
                prefer_source_quality,
                lock_top_level,
                autoplay,
            } => format!(
                "loadSource({}, {}, false, {prefer_source_quality}, {lock_top_level}, {autoplay})",
                js_string_literal(url),
                js_string_literal(profile)
            ),
            Play => "play()".into(),
            Pause => "pause()".into(),
            Stop => "stop()".into(),
            Seek(t) => format!("seek({})", n(*t)),
            SetRate(r) => format!("setRate({})", n(*r as f64)),
            SetVolume(v) => format!("setVolume({})", n(*v as f64)),
            SetMuted(m) => format!("setMuted({m})"),
            SeekToLatency(t) => format!("seekToLatency({})", n(*t)),
            SeekToLiveEdge => "seekToLiveEdge()".into(),
            ForceLiveResync => "forceLiveResync()".into(),
            SeekByDriftMs(d) => format!("seekByDriftMs({})", n(*d)),
            SeekTimeshift(s) => format!("seekTimeshift({})", n(*s)),
            SuspendLoading => "suspendLoading()".into(),
            ResumeLoading => "resumeLoading()".into(),
            GetLevels => "getLevels()".into(),
            NoteSurfaceShown => "noteSurfaceShown()".into(),
            SetLatencyOwnedByController(o) => format!("setLatencyOwnedByController({o})"),
            SetControllerTargetLatency(s) => format!("setControllerTargetLatency({})", n(*s)),
            SetLiveLatencyCorrectionEnabled(e) => format!("setLiveLatencyCorrectionEnabled({e})"),
            SetMatchWebV2Config(literal) => format!(
                "setMatchWebV2Config({})",
                literal.as_deref().unwrap_or("null")
            ),
            SetMatchWebV2Targets { hls, ingest } => {
                format!("setMatchWebV2Targets({}, {})", n(*hls), n(*ingest))
            }
            GrantMatchWebV2Seek(id) => format!("grantMatchWebV2Seek({id})"),
            SetAdaptiveCaps {
                max_resolution,
                max_bitrate,
            } => format!("setAdaptiveCaps({max_resolution}, {max_bitrate})"),
            SetForcedHeight {
                height,
                target_bitrate_kbps,
            } => format!("setForcedHeight({height}, {target_bitrate_kbps})"),
            SetLockTopLevel(e) => format!("setLockTopLevel({e})"),
            SetPreferSourceQuality(e) => format!("setPreferSourceQuality({e})"),
            SetBufferCaps {
                max_len,
                max_max_len,
                max_size,
            } => format!(
                "setBufferCaps({}, {}, {max_size})",
                n(*max_len),
                n(*max_max_len)
            ),
            SetSharpPixelScaling(e) => format!("setSharpPixelScaling({e})"),
            SetForcedAspectRatio(e) => format!("setForcedAspectRatio({e})"),
            TakeScreenshot { mime_type, quality } => format!(
                "takeScreenshot({}, {})",
                js_string_literal(mime_type),
                n(*quality)
            ),
            StartRecording {
                mime_type,
                timeslice_ms,
            } => match mime_type {
                Some(m) => format!("startRecording({}, {timeslice_ms})", js_string_literal(m)),
                None => format!("startRecording(null, {timeslice_ms})"),
            },
            StopRecording => "stopRecording()".into(),
        }
    }
}

#[cfg(test)]
mod tests;
