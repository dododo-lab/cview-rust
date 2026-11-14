//! 진단 보고서 — 설정 › 정보 › "보고서 복사". 이슈에 붙여 넣을 짧은 글을 만든다.
//!
//! - 넣는 것: 앱 버전·빌드, 운영체제·아키텍처, 웹뷰 버전, 정해 둔 설정 값([`SETTINGS`] 허용 목록), 최근 디버그 로그 몇 줄.
//! - 넣지 않는 것: 로그인 쿠키·토큰, 계정 이름, 채팅 내용, 팔로잉·시청한 채널, 파일·폴더 경로, 서버 주소.
//! - 로그 줄([`sanitize_log_line`])은 숫자·참거짓과 허용한 키의 짧은 낱말만 남긴다. 채널·사용자·문구·주소가 담길 수 있는
//!   키는 통째로 빼고, 요청 경로의 채널·영상 식별자는 `:id` 로 바꾼다. 화면에 내용을 먼저 보여 줄 수 있게 글로 돌려준다.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use tauri::{AppHandle, Manager, State};

use crate::commands::{CmdResult, CommandError};
use crate::state::AppServices;

/// 보고서에 넣는 설정 — (영역, 키). 경로·주소·자유 글(스크린샷 폴더, 동기화 서버, 금지어·차단 목록, 단축키)은 넣지 않는다.
const SETTINGS: &[(&str, &[&str])] = &[
    (
        "player",
        &[
            "quality",
            "autoPlay",
            "startMuted",
            "forceHighestQuality",
            "continuePlaybackInBackground",
            "streamProxyMode",
            "hardwareDecoding",
            "lowLatencyMode",
            "latencyPreset",
            "bufferDuration",
            "liveRewindEnabled",
            "experimentalServerTimeMachine",
            "experimentalMatchWebV2",
            "sharpnessLevel",
            "sharpnessEnlargedOnly",
            "autoPictureEnhancement",
            "videoAdjustEnabled",
        ],
    ),
    (
        "multiLive",
        &[
            "maxConcurrentSessions",
            "maxLiveChannels",
            "defaultLayoutMode",
            "multiAudioEnabled",
            "bandwidthCoordinationEnabled",
            "levelCappingEnabled",
            "backgroundQualityReduction",
            "autoReconnect",
        ],
    ),
    (
        "general",
        &[
            "autoRefreshEnabled",
            "autoRefreshInterval",
            "restoreWindowOnLaunch",
            "alwaysOnTop",
            "launchAtLogin",
            "showInMenuBar",
            "autoCheckUpdates",
            "notificationsEnabled",
        ],
    ),
    (
        "appearance",
        &[
            "theme",
            "compactMode",
            "motionQuality",
            "transparencyMode",
            "frameRateMode",
            "maxMemoryMB",
            "debugMode",
        ],
    ),
    (
        "network",
        &[
            "autoPreset",
            "requestRateLimit",
            "maxConcurrentAPIRequests",
            "maxConnectionsPerHost",
            "connectionTimeout",
            "streamConnectionTimeout",
            "apiResourceTimeout",
            "retryCount",
            "autoReconnect",
            "forceStreamProxy",
            "pauseRefreshWhenOffline",
            "reduceBackgroundActivity",
        ],
    ),
    (
        "chat",
        &[
            "fontSize",
            "chatAvatars",
            "mergedChatStyle",
            "donationList",
            "maxVisibleMessages",
            "showDonation",
            "ttsEnabled",
            "chatFilterEnabled",
        ],
    ),
];

/// 로그 줄에서 글자 값을 남겨도 되는 키 — 종류·시각·상태 낱말뿐이다(그래도 [`safe_word`] 를 통과해야 한다).
const LOG_TEXT_KEYS: &[&str] = &[
    "kind",
    "ts",
    "method",
    "page",
    "action",
    "type",
    "details",
    "errorName",
    "profile",
    "recoveryAction",
    "visibility",
    "hlsVersion",
    "state",
    "phase",
    "where",
    "source",
    "level",
    "os",
    "version",
    "mode",
    "preset",
    "status",
    "error",
];

/// 이 낱말이 키에 들어 있으면 값이 숫자여도 뺀다 — 채널·사용자·문구·주소·자격 증명이 담길 수 있다.
const LOG_DROP_HINTS: &[&str] = &[
    "tag", "stack", "message", "reason", "title", "name", "nick", "user", "channel", "url", "uri",
    "query", "text", "content", "cookie", "token", "auth", "session", "nid", "secret", "password",
    "email", "file", "dir", "host",
];

const LOG_MAX_LINES: usize = 40;
const LOG_TAIL_BYTES: u64 = 256 * 1024;
const LOG_LINE_CHARS: usize = 400;
const LOG_DEPTH: usize = 4;
const LOG_ARRAY_ITEMS: usize = 8;

/// 짧은 낱말인가 — 글자·숫자와 `_ - . : + , ( )`·띄어쓰기만, 64자까지. 주소(`/`)·메일(`@`)·한글 문구는 걸러진다.
fn safe_word(s: &str) -> bool {
    !s.is_empty()
        && s.chars().count() <= 64
        && s.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '_' | '-' | '.' | ':' | '+' | ',' | '(' | ')' | ' ')
        })
}

/// 낱말 속 긴 식별자(숫자가 섞인 10자 이상 덩어리)를 `:id` 로. 시각(`2026-11-14T11:20:04.852Z`)은 덩어리가 짧아 그대로다.
fn scrub_ids(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut token = String::new();
    let flush = |token: &mut String, out: &mut String| {
        if token.len() >= 10 && token.chars().any(|c| c.is_ascii_digit()) {
            out.push_str(":id");
        } else {
            out.push_str(token);
        }
        token.clear();
    };
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            token.push(c);
        } else {
            flush(&mut token, &mut out);
            out.push(c);
        }
    }
    flush(&mut token, &mut out);
    out
}

fn is_host(seg: &str) -> bool {
    seg.contains('.')
        && seg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

/// 요청 경로(`api.chzzk.naver.com/service/v1/…`) — 쿼리를 떼고, 호스트·영문 낱말(`live-detail`)·API 판(`v2`)이 아닌
/// 마디는 `:id` 로 바꾼다. 호스트로 시작하지 않으면(파일 경로일 수 있다 — 사용자 이름이 든다) 통째로 `:path`.
pub fn normalize_path(path: &str) -> String {
    let path = path.split(['?', '#']).next().unwrap_or_default();
    if !path.split('/').next().is_some_and(is_host) {
        return ":path".into();
    }
    path.split('/')
        .enumerate()
        .map(|(i, seg)| {
            let host = i == 0;
            let word = seg.len() <= 24
                && seg
                    .chars()
                    .all(|c| c.is_ascii_alphabetic() || c == '-' || c == '_');
            let api_version = seg.len() > 1
                && seg.len() <= 4
                && seg.starts_with('v')
                && seg[1..].chars().all(|c| c.is_ascii_digit());
            if seg.is_empty() || host || word || api_version {
                seg.to_string()
            } else {
                ":id".to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn dropped_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    LOG_DROP_HINTS.iter().any(|h| k.contains(h)) || k.ends_with("id") || k.ends_with("no")
}

fn sanitize_value(key: &str, value: Value, depth: usize) -> Option<Value> {
    if key == "path" {
        return match value {
            Value::String(s) => Some(Value::String(normalize_path(&s))),
            _ => None,
        };
    }
    // 허용한 낱말 키(`profile`·`errorName`)는 힌트(`file`·`name`)에 걸려도 남긴다 — 값은 아래에서 다시 거른다.
    let text_key = LOG_TEXT_KEYS.contains(&key);
    if !key.is_empty() && !text_key && dropped_key(key) {
        return None;
    }
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => Some(value),
        Value::String(s) => (text_key && safe_word(&s)).then(|| Value::String(scrub_ids(&s))),
        Value::Object(map) if depth < LOG_DEPTH => {
            Some(Value::Object(sanitize_map(map, depth + 1)))
        }
        Value::Array(items) if depth < LOG_DEPTH => Some(Value::Array(
            items
                .into_iter()
                .take(LOG_ARRAY_ITEMS)
                .filter_map(|v| match v {
                    // 배열 속 글자는 어느 키의 값인지 모르니 뺀다.
                    Value::String(_) => None,
                    other => sanitize_value("", other, depth + 1),
                })
                .collect(),
        )),
        _ => None,
    }
}

fn sanitize_map(map: Map<String, Value>, depth: usize) -> Map<String, Value> {
    map.into_iter()
        .filter_map(|(k, v)| sanitize_value(&k, v, depth).map(|v| (k, v)))
        .collect()
}

/// 디버그 로그 한 줄(JSON)을 보고서에 넣을 모양으로. JSON 이 아니면 `None`(무엇이 들었는지 모르므로 넣지 않는다).
pub fn sanitize_log_line(line: &str) -> Option<String> {
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(line.trim()) else {
        return None;
    };
    let text = serde_json::to_string(&Value::Object(sanitize_map(map, 0))).ok()?;
    Some(if text.chars().count() > LOG_LINE_CHARS {
        let mut cut: String = text.chars().take(LOG_LINE_CHARS).collect();
        cut.push('…');
        cut
    } else {
        text
    })
}

/// 파일 끝 `max_bytes` 안의 온전한 줄들. 앞에서 자른 첫 줄은 버린다.
fn tail_lines(path: &Path, max_bytes: u64) -> Vec<String> {
    let Ok(mut file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(max_bytes);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }
    let mut buf = Vec::new();
    if file.read_to_end(&mut buf).is_err() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    lines.retain(|l| !l.trim().is_empty());
    lines
}

/// 최근 디버그 로그 — 지금 파일이 짧으면(방금 밀었으면) 바로 앞 파일(`debug.1.jsonl`)에서 채운다. 다듬은 줄만.
pub fn recent_log_lines(dir: &Path, max_lines: usize) -> Vec<String> {
    let mut lines = tail_lines(&dir.join("debug.jsonl"), LOG_TAIL_BYTES);
    if lines.len() < max_lines {
        let mut older = tail_lines(&dir.join("debug.1.jsonl"), LOG_TAIL_BYTES);
        older.append(&mut lines);
        lines = older;
    }
    let skip = lines.len().saturating_sub(max_lines);
    lines
        .iter()
        .skip(skip)
        .filter_map(|l| sanitize_log_line(l))
        .collect()
}

fn scalar_text(v: &Value) -> Option<String> {
    match v {
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) if s.chars().count() <= 32 && safe_word(s) && !s.contains(' ') => {
            Some(s.clone())
        }
        _ => None,
    }
}

/// 허용 목록의 설정 값 — 영역마다 한 줄(`player: quality=1080p, …`). 숫자·참거짓·짧은 낱말만.
pub fn settings_lines(settings: &Value) -> Vec<String> {
    let mut out: Vec<String> = SETTINGS
        .iter()
        .filter_map(|(domain, keys)| {
            let obj = settings.get(*domain)?;
            let pairs: Vec<String> = keys
                .iter()
                .filter_map(|k| scalar_text(obj.get(*k)?).map(|v| format!("{k}={v}")))
                .collect();
            (!pairs.is_empty()).then(|| format!("{domain}: {}", pairs.join(", ")))
        })
        .collect();
    let preset = settings.get("networkPreset").and_then(scalar_text);
    let auto = settings.get("networkAutoChoice").and_then(scalar_text);
    if let Some(preset) = preset {
        out.push(match auto {
            Some(auto) if preset == "auto" => format!("networkPreset: auto ({auto})"),
            _ => format!("networkPreset: {preset}"),
        });
    }
    out
}

/// 보고서 재료 — 순수 함수 [`build_report`] 에 넘긴다.
pub struct ReportInput<'a> {
    pub generated_at: String,
    pub version: &'a str,
    pub build: &'a str,
    pub os: String,
    pub arch: &'a str,
    pub webview: Option<String>,
    pub debug_log: bool,
    pub settings: &'a Value,
    pub log_lines: Vec<String>,
}

pub fn build_report(input: ReportInput<'_>) -> String {
    let version = if input.build.is_empty() || input.build == "0" {
        input.version.to_string()
    } else {
        format!("{} (빌드 {})", input.version, input.build)
    };
    let mut out = vec![
        "CView 진단 보고서".to_string(),
        format!("만든 시각: {}", input.generated_at),
        "로그인 쿠키·토큰, 계정 이름, 채팅, 팔로잉·시청한 채널, 파일 경로는 넣지 않았습니다."
            .into(),
        String::new(),
        "[앱]".into(),
        format!("버전: {version}"),
        format!("운영체제: {} ({})", input.os, input.arch),
    ];
    if let Some(w) = input.webview.filter(|w| !w.is_empty()) {
        out.push(format!("웹뷰: {w}"));
    }
    out.push(format!(
        "디버그 로그: {}",
        if input.debug_log { "켜짐" } else { "꺼짐" }
    ));
    out.push(String::new());
    out.push("[설정]".into());
    out.extend(settings_lines(input.settings));
    out.push(String::new());
    if input.log_lines.is_empty() {
        out.push("[최근 디버그 로그]".into());
        out.push(
            "(없음 — 설정 › 고급 › 디버그 로그를 켜고 문제를 한 번 더 겪은 뒤 복사하면 들어갑니다)"
                .into(),
        );
    } else {
        out.push(format!(
            "[최근 디버그 로그 {}줄 — 채널·문구·주소는 지움]",
            input.log_lines.len()
        ));
        out.extend(input.log_lines);
    }
    out.join("\n") + "\n"
}

fn log_dir(app: &AppHandle) -> Option<PathBuf> {
    crate::perf_log::log_dir()
        .map(Path::to_path_buf)
        .or_else(|| app.path().app_data_dir().ok().map(|d| d.join("logs")))
}

/// "보고서 복사"·"내용 보기" — 보고서 글을 만든다. 클립보드에 넣는 것은 화면이 한다.
#[tauri::command]
pub async fn diagnostic_report(
    app: AppHandle,
    services: State<'_, AppServices>,
) -> CmdResult<String> {
    let settings = serde_json::to_value(crate::commands::settings::snapshot(&services))
        .map_err(|e| CommandError(e.to_string()))?;
    let dir = log_dir(&app);
    let log_lines = tauri::async_runtime::spawn_blocking(move || {
        dir.map(|d| recent_log_lines(&d, LOG_MAX_LINES))
            .unwrap_or_default()
    })
    .await
    .unwrap_or_default();
    let info = app.package_info();
    let version = info.version.to_string();
    let build = crate::commands::settings::build_number(&app);
    let os = sysinfo::System::long_os_version().unwrap_or_else(|| std::env::consts::OS.to_string());
    Ok(build_report(ReportInput {
        generated_at: chrono::Local::now()
            .format("%Y-%m-%d %H:%M:%S %:z")
            .to_string(),
        version: &version,
        build: &build,
        os,
        arch: std::env::consts::ARCH,
        webview: tauri::webview_version().ok(),
        debug_log: crate::perf_log::enabled(),
        settings: &settings,
        log_lines,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn path_ids_are_masked() {
        assert_eq!(
            normalize_path("api.chzzk.naver.com/service/v1/channels/12ee095b2163d9a838cc"),
            "api.chzzk.naver.com/service/v1/channels/:id"
        );
        assert_eq!(
            normalize_path(
                "api.chzzk.naver.com/service/v3/channels/65c3/live-detail?dt=1&cookie=x"
            ),
            "api.chzzk.naver.com/service/v3/channels/:id/live-detail"
        );
        assert_eq!(
            normalize_path("api.chzzk.naver.com/service/v2/videos/123456"),
            "api.chzzk.naver.com/service/v2/videos/:id"
        );
        assert_eq!(
            normalize_path("api.chzzk.naver.com/search/%ED%95%9C"),
            "api.chzzk.naver.com/search/:id"
        );
        // 호스트가 없으면 파일 경로일 수 있다 — 사용자 이름이 남지 않게 통째로 가린다.
        assert_eq!(normalize_path("/Users/someone/Library/x.log"), ":path");
        assert_eq!(normalize_path("C:\\Users\\someone"), ":path");
    }

    #[test]
    fn log_line_keeps_numbers_and_safe_words_only() {
        let line = json!({
            "ts": "2026-11-14T11:20:04.852Z",
            "kind": "net.request",
            "method": "GET",
            "path": "api.chzzk.naver.com/service/v1/channels/12ee095b2163d9a838cc",
            "status": 200,
            "ms": 41.5,
            "error": "error sending request for url (https://api.chzzk.naver.com/x?nid=1)",
            "Cookie": "NID_SES=secret",
            "accessToken": "t",
            "channelId": "12ee095b2163d9a838cc",
            "videoNo": 12345,
            "channelName": "릴카",
            "liveTitle": "방송 제목",
            "message": "채팅 내용",
            "streams": [{ "tag": "c8e561", "kbps": 6000, "where": "pane1" }, "c8e561"],
            "data": { "nickname": "도도", "stack": "@tauri://x", "frames": 60, "page": "settings" },
        })
        .to_string();
        let out = sanitize_log_line(&line).unwrap();
        for secret in [
            "12ee095b",
            "NID_SES",
            "secret",
            "\"t\"",
            "릴카",
            "방송 제목",
            "채팅",
            "도도",
            "c8e561",
            "tauri://",
            "https://",
            "12345",
        ] {
            assert!(!out.contains(secret), "{secret} 이(가) 남았다: {out}");
        }
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["kind"], "net.request");
        assert_eq!(v["ts"], "2026-11-14T11:20:04.852Z");
        assert_eq!(v["path"], "api.chzzk.naver.com/service/v1/channels/:id");
        assert_eq!(v["status"], 200);
        assert_eq!(v["streams"][0]["kbps"], 6000);
        assert_eq!(v["streams"][0]["where"], "pane1");
        assert_eq!(v["streams"].as_array().unwrap().len(), 1);
        assert_eq!(v["data"]["frames"], 60);
        assert_eq!(v["data"]["page"], "settings");
        assert!(v.get("error").is_none());
    }

    #[test]
    fn non_json_and_long_ids_are_not_copied() {
        assert!(sanitize_log_line("plain text with cookie=abc").is_none());
        let out = sanitize_log_line(r#"{"kind":"x","state":"room 0123456789abcdef"}"#).unwrap();
        assert!(
            out.contains(":id") && !out.contains("0123456789abcdef"),
            "{out}"
        );
        let long = format!(r#"{{"kind":"x","list":[{}]}}"#, vec!["1"; 400].join(","));
        let out = sanitize_log_line(&long).unwrap();
        assert!(out.chars().count() <= LOG_LINE_CHARS + 1);
    }

    #[test]
    fn settings_use_the_allow_list() {
        let settings = json!({
            "player": { "quality": "1080p", "lowLatencyMode": true, "screenshotPath": "/Users/me/Pictures", "volumeLevel": 0.4 },
            "general": { "autoCheckUpdates": false },
            "chat": { "fontSize": 14, "blockedWords": ["욕"], "blockedUsers": ["abc"], "blockedUserNames": { "abc": "누구" } },
            "sync": { "endpoint": "https://sync.example.com", "explicitlyOptedIn": true },
            "appearance": { "theme": "dark", "debugMode": true },
            "networkPreset": "auto",
            "networkAutoChoice": "balanced",
        });
        let text = settings_lines(&settings).join("\n");
        assert!(
            text.contains("player: quality=1080p, lowLatencyMode=true"),
            "{text}"
        );
        assert!(text.contains("general: autoCheckUpdates=false"));
        assert!(text.contains("chat: fontSize=14"));
        assert!(text.contains("appearance: theme=dark, debugMode=true"));
        assert!(text.contains("networkPreset: auto (balanced)"));
        for secret in [
            "/Users",
            "Pictures",
            "욕",
            "abc",
            "누구",
            "sync.example",
            "endpoint",
        ] {
            assert!(!text.contains(secret), "{secret}: {text}");
        }
    }

    #[test]
    fn report_has_sections_and_notes_missing_logs() {
        let settings = json!({ "appearance": { "theme": "system" } });
        let report = build_report(ReportInput {
            generated_at: "2026-11-14 20:00:00 +09:00".into(),
            version: "0.1.103",
            build: "103",
            os: "macOS 26.3".into(),
            arch: "aarch64",
            webview: Some("621.1".into()),
            debug_log: false,
            settings: &settings,
            log_lines: Vec::new(),
        });
        assert!(report.starts_with("CView 진단 보고서\n"));
        assert!(report.contains("버전: 0.1.103 (빌드 103)"));
        assert!(report.contains("운영체제: macOS 26.3 (aarch64)"));
        assert!(report.contains("웹뷰: 621.1"));
        assert!(report.contains("디버그 로그: 꺼짐"));
        assert!(report.contains("appearance: theme=system"));
        assert!(report.contains("[최근 디버그 로그]\n(없음"));
    }

    #[test]
    fn recent_logs_read_the_tail_and_previous_file() {
        let dir = tempfile::tempdir().unwrap();
        let older: String = (0..5)
            .map(|i| format!("{{\"kind\":\"old\",\"n\":{i}}}\n"))
            .collect();
        std::fs::write(dir.path().join("debug.1.jsonl"), older).unwrap();
        std::fs::write(
            dir.path().join("debug.jsonl"),
            "{\"kind\":\"new\",\"n\":1}\nnot json\n{\"kind\":\"new\",\"n\":2,\"channelId\":\"abc\"}\n",
        )
        .unwrap();
        let lines = recent_log_lines(dir.path(), 4);
        assert_eq!(
            lines,
            [
                r#"{"kind":"old","n":4}"#,
                r#"{"kind":"new","n":1}"#,
                r#"{"kind":"new","n":2}"#,
            ]
        );
        assert!(recent_log_lines(&dir.path().join("none"), 4).is_empty());
    }
}
