
//! `MatchWebV2BridgeTests.swift`(메시지·스냅샷 파싱) 와 `HLSJSVideoView.handleMessage` 계약 포팅.
use super::*;
use serde_json::json;

fn obj(text: &str) -> Map<String, Value> {
    serde_json::from_str::<Value>(text)
        .unwrap()
        .as_object()
        .unwrap()
        .clone()
}

const TOKEN_REQUEST: &str =
    r#"{"requestId":1,"reason":"start","errMs":500,"sessionId":7,"pageGen":3}"#;
const SEEK: &str = r#"{"kind":"seek","requestId":1,"reason":"start","errMs":500,"moveMs":940,"appliedMs":940,"clamped":false,"lossCompMs":440,"fwdBeforeMs":3000,"fwdAfterMs":2060,"targetMs":3300,"hlsLatencyMs":3600,"sessionId":7,"pageGen":3}"#;
const SEEK_SKIPPED: &str =
    r#"{"kind":"seekSkipped","requestId":1,"why":"tokenTimeout","sessionId":7,"pageGen":3}"#;
const SEEK_RESULT: &str = r#"{"kind":"seekResult","lossMs":500,"errBeforeMs":500,"errAfterMs":60,"lossCompMs":500,"valid":true,"sessionId":7,"pageGen":3}"#;
const SEEK_RESULT_INVALID: &str = r#"{"kind":"seekResult","lossMs":null,"errBeforeMs":500,"errAfterMs":null,"lossCompMs":440,"valid":false,"sessionId":7,"pageGen":3}"#;
const FALLBACK: &str = r#"{"kind":"fallback","reason":"noParts","sessionId":7,"pageGen":3}"#;
const WARN: &str = r#"{"kind":"warn","reason":"clock","errMs":3000,"kMs":3200,"delayMs":null,"tBumpMs":0,"sessionId":7,"pageGen":3}"#;

fn parse(t: JsMessageType, text: &str) -> Option<MatchWebV2Envelope> {
    parse_match_web_v2_message(t, &obj(text))
}

fn fields(pairs: &[(&str, f64)]) -> BTreeMap<String, f64> {
    pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
}

#[test]
fn snapshot_parses_v2_metrics() {
    let off = HlsJsPlaybackSnapshot::parse(&obj(r#"{"currentTime":1.0}"#), 0.0);
    assert_eq!(off.match_web_v2, HlsJsMatchWebV2Metrics::OFF);
    assert!(off.match_web_v2.v2_state.is_empty());
    let m = HlsJsPlaybackSnapshot::parse(
            &obj(
                r#"{"currentTime":12.0,"ingestLatencyMs":3312,"pdtLatencyMs":3100,"llActive":true,
                "partDurationSeconds":1.0,"v2State":"steady","v2TargetMs":3300,"v2HlsTargetMs":3100,
                "v2SeekCount":2,"v2LastSeekLossMs":480,"v2LossCompMs":460,"ingestHlsOffsetMs":200}"#,
            ),
            0.0,
        )
        .match_web_v2;
    assert_eq!(m.ingest_latency_ms, 3312.0);
    assert_eq!(m.pdt_latency_ms, 3100.0);
    assert!(m.ll_active);
    assert_eq!(m.part_duration_seconds, 1.0);
    assert_eq!(m.v2_state, "steady");
    assert_eq!(m.v2_target_ms, 3300.0);
    assert_eq!(m.v2_hls_target_ms, 3100.0);
    assert_eq!(m.v2_seek_count, 2);
    assert_eq!(m.v2_last_seek_loss_ms, 480.0);
    assert_eq!(m.v2_loss_comp_ms, 460.0);
    assert_eq!(m.ingest_hls_offset_ms, 200.0);
}

#[test]
fn stub_off_values_equal_off() {
    let data = obj(
        r#"{"ingestLatencyMs":-1,"pdtLatencyMs":-1,"llActive":false,"partDurationSeconds":0,
            "v2State":"","v2TargetMs":-1,"v2HlsTargetMs":-1,"v2SeekCount":0,"v2LastSeekLossMs":-1,
            "v2LossCompMs":-1,"ingestHlsOffsetMs":-1}"#,
    );
    assert_eq!(
        HlsJsMatchWebV2Metrics::parse(&data),
        HlsJsMatchWebV2Metrics::OFF
    );
}

#[test]
fn parses_token_request() {
    let e = parse(JsMessageType::V2SeekTokenRequest, TOKEN_REQUEST).unwrap();
    assert_eq!((e.session_id, e.page_gen), (7, 3));
    assert_eq!(
        e.message,
        MatchWebV2Message::SeekTokenRequest {
            request_id: 1,
            reason: "start".into(),
            err_ms: 500.0
        }
    );
    assert!(parse(
        JsMessageType::V2SeekTokenRequest,
        r#"{"reason":"start","sessionId":7,"pageGen":3}"#
    )
    .is_none());
}

#[test]
fn parses_seek_with_bools_as_numbers() {
    let e = parse(JsMessageType::V2Event, SEEK).unwrap();
    assert_eq!(
        e.message,
        MatchWebV2Message::Event {
            kind: MatchWebV2EventKind::Seek,
            request_id: Some(1),
            reason: Some("start".into()),
            fields: fields(&[
                ("errMs", 500.0),
                ("moveMs", 940.0),
                ("appliedMs", 940.0),
                ("clamped", 0.0),
                ("lossCompMs", 440.0),
                ("fwdBeforeMs", 3000.0),
                ("fwdAfterMs", 2060.0),
                ("targetMs", 3300.0),
                ("hlsLatencyMs", 3600.0),
            ]),
        }
    );
}

#[test]
fn parses_seek_skipped_result_fallback_warn() {
    assert_eq!(
        parse(JsMessageType::V2Event, SEEK_SKIPPED).unwrap().message,
        MatchWebV2Message::Event {
            kind: MatchWebV2EventKind::SeekSkipped,
            request_id: Some(1),
            reason: Some("tokenTimeout".into()),
            fields: BTreeMap::new(),
        }
    );
    assert_eq!(
        parse(JsMessageType::V2Event, SEEK_RESULT).unwrap().message,
        MatchWebV2Message::Event {
            kind: MatchWebV2EventKind::SeekResult,
            request_id: None,
            reason: None,
            fields: fields(&[
                ("lossMs", 500.0),
                ("errBeforeMs", 500.0),
                ("errAfterMs", 60.0),
                ("lossCompMs", 500.0),
                ("valid", 1.0),
            ]),
        }
    );
    assert_eq!(
        parse(JsMessageType::V2Event, SEEK_RESULT_INVALID)
            .unwrap()
            .message,
        MatchWebV2Message::Event {
            kind: MatchWebV2EventKind::SeekResult,
            request_id: None,
            reason: None,
            fields: fields(&[
                ("errBeforeMs", 500.0),
                ("lossCompMs", 440.0),
                ("valid", 0.0)
            ]),
        }
    );
    let fallback = parse(JsMessageType::V2Event, FALLBACK).unwrap();
    assert_eq!(fallback.session_id, 7);
    assert_eq!(
        fallback.message,
        MatchWebV2Message::Event {
            kind: MatchWebV2EventKind::Fallback,
            request_id: None,
            reason: Some("noParts".into()),
            fields: BTreeMap::new(),
        }
    );
    assert_eq!(
        parse(JsMessageType::V2Event, WARN).unwrap().message,
        MatchWebV2Message::Event {
            kind: MatchWebV2EventKind::Warn,
            request_id: None,
            reason: Some("clock".into()),
            fields: fields(&[("errMs", 3000.0), ("kMs", 3200.0), ("tBumpMs", 0.0)]),
        }
    );
}

#[test]
fn unknown_kind_and_missing_common_keys() {
    assert!(parse(
        JsMessageType::V2Event,
        r#"{"kind":"stateChanged","sessionId":7,"pageGen":3}"#
    )
    .is_none());
    assert!(parse(JsMessageType::V2Event, r#"{"sessionId":7,"pageGen":3}"#).is_none());
    let bare = parse(
        JsMessageType::V2Event,
        r#"{"kind":"fallback","reason":"noIngest","sessionId":-4}"#,
    )
    .unwrap();
    assert_eq!((bare.session_id, bare.page_gen), (0, 0));
}

#[test]
fn stale_page_filter() {
    assert!(is_stale_page_message(5, 4));
    assert!(!is_stale_page_message(4, 4));
    assert!(!is_stale_page_message(0, 4), "0 은 구 페이지 호환으로 통과");
}

#[test]
fn parses_envelopes_of_every_type() {
    let msg = |t: &str, data: Value| JsMessage::parse(&json!({"type": t, "data": data}), 1.0);
    assert!(
        msg("manifestParsed", json!({})).is_none(),
        "제거된 타입은 무시"
    );
    assert_eq!(
        msg("ready", json!({"hlsSupported": true, "pageGen": 3}))
            .unwrap()
            .1,
        JsMessage::Ready {
            hls_supported: true,
            page_gen: 3
        }
    );
    let (t, m) = msg("playStarted", json!({"muted": true, "volume": 0.4})).unwrap();
    assert_eq!(t.playback_state_transition(), Some(PlayerPhase::Playing));
    assert_eq!(
        m,
        JsMessage::PlayStarted {
            muted: true,
            volume: 0.4
        }
    );
    for t in JsMessageType::ALL {
        if t != JsMessageType::PlayStarted {
            assert_eq!(t.playback_state_transition(), None, "{t:?}");
        }
        assert_eq!(JsMessageType::from_raw(t.raw_value()), Some(t));
    }
    assert_eq!(
        msg(
            "playbackState",
            json!({"state": "stalled", "readyState": 2})
        )
        .unwrap()
        .1,
        JsMessage::PlaybackState { stalled: true }
    );
    assert_eq!(
        msg("playbackState", json!({"state": "playing"})).unwrap().1,
        JsMessage::PlaybackState { stalled: false }
    );
    match msg(
            "error",
            json!({"fatal": true, "recoverable": false, "type": "NOT_SUPPORTED", "details": "MSE not supported"}),
        )
        .unwrap()
        .1
        {
            JsMessage::Error {
                fatal,
                recoverable,
                error_type,
                details,
                ..
            } => {
                assert!(fatal && !recoverable);
                assert_eq!(error_type, "NOT_SUPPORTED");
                assert_eq!(details, "MSE not supported");
            }
            other => panic!("{other:?}"),
        }
    assert_eq!(
            msg(
                "levels",
                json!({"levels": [
                    {"index": 0, "width": 1920, "height": 1080, "bitrate": 8000000, "name": "1080p60"},
                    {"index": 1, "width": 1280, "height": 720, "bitrate": 4000000},
                    {"index": 2, "width": 1, "height": 1}
                ], "current": 0, "capped": -1})
            )
            .unwrap()
            .1,
            JsMessage::Levels {
                items: vec![
                    HlsJsLevelInfo {
                        index: 0,
                        width: 1920,
                        height: 1080,
                        bitrate: 8_000_000,
                        name: "1080p60".into()
                    },
                    HlsJsLevelInfo {
                        index: 1,
                        width: 1280,
                        height: 720,
                        bitrate: 4_000_000,
                        name: "720p".into()
                    },
                ],
                current: 0,
                capped: -1
            }
        );
    let batch = msg(
        "diagnosticBatch",
        json!({"events": (0..70).map(|i| json!({"i": i})).collect::<Vec<_>>()}),
    )
    .unwrap()
    .1;
    assert!(matches!(batch, JsMessage::DiagnosticBatch(ref v) if v.len() == 64));
    assert_eq!(
        msg(
            "recording",
            json!({"ok": true, "state": "chunk", "base64": "AAAA", "seq": 2})
        )
        .unwrap()
        .1,
        JsMessage::Recording {
            ok: true,
            state: "chunk".into(),
            mime_type: None,
            duration_sec: 0.0,
            size_bytes: 0,
            base64: Some("AAAA".into()),
            seq: Some(2),
            error: None
        }
    );
    assert_eq!(
        msg("surfaceFirstFrame", json!({"timedOut": true}))
            .unwrap()
            .1,
        JsMessage::SurfaceFirstFrame {
            timed_out: true,
            fallback: false
        }
    );
    let (_, metrics) = msg(
        "metrics",
        json!({"latency": 3.2, "bufferHealth": 0.7, "paused": false, "playbackRate": 1.05,
                   "decodedFramesDelta": 120, "droppedFramesDelta": 6, "networkMetrics": {}}),
    )
    .unwrap();
    let JsMessage::Metrics(s) = metrics else {
        panic!()
    };
    assert_eq!(s.latency, 3.2);
    assert_eq!(s.playback_rate, 1.05);
    assert_eq!(s.pl_end_sn_age_ms, -1.0);
    assert_eq!(s.decoder_pressure_cap, -1);
    assert_eq!(s.dropped_percent(), Some(5.0));
    assert_eq!(s.decoded_fps(2.0), Some(60.0));
    assert_eq!(s.decoded_fps(0.0), None);
}

#[test]
fn command_scripts() {
    assert_eq!(JsCommand::SetRate(1.0).script(), "setRate(1.0)");
    assert_eq!(
        JsCommand::SeekToLatency(f64::NAN).script(),
        "seekToLatency(0)"
    );
    assert_eq!(
        JsCommand::SetMatchWebV2Config(None).script(),
        "setMatchWebV2Config(null)"
    );
    assert_eq!(
        JsCommand::LoadSource {
            url: "http://127.0.0.1:1/a'b\n.m3u8".into(),
            profile: "lowLatency".into(),
            prefer_source_quality: true,
            lock_top_level: false,
            autoplay: true,
        }
        .script(),
        r#"loadSource("http://127.0.0.1:1/a'b\n.m3u8", "lowLatency", false, true, false, true)"#
    );
    assert_eq!(
        JsCommand::StartRecording {
            mime_type: None,
            timeslice_ms: 1000
        }
        .script(),
        "startRecording(null, 1000)"
    );
    assert_eq!(
        JsCommand::SetMatchWebV2Targets {
            hls: 2.75,
            ingest: 4.05
        }
        .script(),
        "setMatchWebV2Targets(2.75, 4.05)"
    );
}
