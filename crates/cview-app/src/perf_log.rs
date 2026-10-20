//! 디버그 로그 — 설정 "디버그 로그"(또는 `CVIEW_DEBUG_LOG=1`)가 켜져 있을 때만 정밀 데이터를 JSON 줄로 남긴다.
//!
//! - 위치: `<앱 데이터>/logs/debug.jsonl`, 10MB 마다 `debug.1.jsonl` … `debug.4.jsonl` 로 밀어 5개까지.
//! - 기록은 채널로 넘기고 별도 스레드가 모아 1초마다 쓴다 — 호출하는 쪽은 줄을 만들어 보내기만 한다.
//! - 꺼져 있으면 [`enabled`] 원자 깃발 하나만 보고 돌아간다(줄도 만들지 않는다).
//! - 쿠키·토큰·NID 같은 값은 키 이름으로 가린다([`redact`]). 요청 경로는 쿼리를 뺀다.
//!
//! 종류(`kind`): `startup.*`(시작 단계) · `net.request`(API 요청) · `process`(10초 표본: CPU·메모리·스레드,
//! 앱 + 웹뷰 도우미, 채팅 처리량) · `chat.state` · `ui.*`(화면 수집기: 프레임·DOM·IPC·플레이어·시작 표시).
//! 표로 바꾸기: `perf/tools/analyze-log.py <debug.jsonl>`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

const FILE_NAME: &str = "debug.jsonl";
const MAX_BYTES: u64 = 10 * 1024 * 1024;
const KEEP_FILES: usize = 5;
const SAMPLE_EVERY: Duration = Duration::from_secs(10);

static ENABLED: AtomicBool = AtomicBool::new(false);
static PROCESS_START: OnceLock<Instant> = OnceLock::new();
static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();
static SENDER: Mutex<Option<Sender<String>>> = Mutex::new(None);
static CHAT_MESSAGES: AtomicU64 = AtomicU64::new(0);
static CHAT_BATCHES: AtomicU64 = AtomicU64::new(0);
static UI_EVENTS: AtomicU64 = AtomicU64::new(0);

/// 프로세스 시작 시각을 잡는다(`run()` 맨 앞).
pub fn mark_process_start() {
    let _ = PROCESS_START.set(Instant::now());
}

fn uptime_ms() -> f64 {
    PROCESS_START
        .get()
        .map(|s| s.elapsed().as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

/// 켜져 있는지 — 기록을 만들기 전에 본다.
#[inline]
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

/// 환경 변수로 강제로 켰는지.
pub fn forced_by_env() -> bool {
    std::env::var("CVIEW_DEBUG_LOG").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

/// 로그 폴더.
pub fn log_dir() -> Option<&'static Path> {
    LOG_DIR.get().map(PathBuf::as_path)
}

/// 지금 쓰는 파일 경로.
pub fn log_file() -> Option<PathBuf> {
    log_dir().map(|d| d.join(FILE_NAME))
}

/// 켜고 끈다. 처음 켤 때 쓰기 스레드·표본 스레드를 띄우고 요청 관찰자를 건다.
pub fn set_enabled(data_dir: &Path, on: bool) {
    let _ = LOG_DIR.set(data_dir.join("logs"));
    if on == enabled() {
        return;
    }
    if on {
        let Some(dir) = log_dir() else { return };
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
        let mut sender = SENDER.lock().unwrap_or_else(|e| e.into_inner());
        if sender.is_none() {
            let (tx, rx) = mpsc::channel::<String>();
            let path = dir.join(FILE_NAME);
            std::thread::Builder::new()
                .name("cview-debug-log".into())
                .spawn(move || writer_loop(&path, rx))
                .ok();
            *sender = Some(tx);
            std::thread::Builder::new()
                .name("cview-debug-sampler".into())
                .spawn(sampler_loop)
                .ok();
        }
        drop(sender);
        ENABLED.store(true, Ordering::Release);
        cview_networking::observer::set_request_observer(Some(std::sync::Arc::new(|r| {
            emit(
                "net.request",
                json!({
                    "method": r.method,
                    "path": r.path,
                    "status": r.status,
                    "error": r.error,
                    "ms": round1(r.duration_ms),
                    "bytes": r.bytes,
                    "cache_hit": r.cache_hit,
                }),
            );
        })));
        emit(
            "log.enabled",
            json!({ "version": env!("CARGO_PKG_VERSION"), "os": std::env::consts::OS }),
        );
    } else {
        emit("log.disabled", json!({}));
        cview_networking::observer::set_request_observer(None);
        ENABLED.store(false, Ordering::Release);
    }
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// 한 줄 남긴다. 꺼져 있으면 아무것도 하지 않는다.
pub fn emit(kind: &str, data: Value) {
    if !enabled() {
        return;
    }
    let mut line = Map::new();
    line.insert(
        "ts".into(),
        Value::String(
            chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string(),
        ),
    );
    line.insert("up_ms".into(), json!(round1(uptime_ms())));
    line.insert("kind".into(), Value::String(kind.to_string()));
    if let Value::Object(map) = redact(data) {
        for (k, v) in map {
            line.insert(k, v);
        }
    }
    let Ok(text) = serde_json::to_string(&Value::Object(line)) else {
        return;
    };
    if let Some(tx) = SENDER.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = tx.send(text);
    }
}

/// 비밀이 들어 있을 수 있는 키 이름이면 값을 가린다. 순수 함수.
pub fn redact(value: Value) -> Value {
    const SECRET: &[&str] = &[
        "cookie",
        "token",
        "nid",
        "auth",
        "secret",
        "password",
        "session_key",
    ];
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| {
                    let lower = k.to_ascii_lowercase();
                    if SECRET.iter().any(|s| lower.contains(s)) {
                        (k, Value::String("[redacted]".into()))
                    } else {
                        (k, redact(v))
                    }
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(redact).collect()),
        other => other,
    }
}

/// 채팅 묶음 하나(메시지 수) — 표본 때 초당 값으로 남긴다.
#[inline]
pub fn count_chat_batch(messages: usize) {
    if enabled() {
        CHAT_BATCHES.fetch_add(1, Ordering::Relaxed);
        CHAT_MESSAGES.fetch_add(messages as u64, Ordering::Relaxed);
    }
}

/// 화면이 보낸 수집 기록 수.
#[inline]
pub fn count_ui_event() {
    UI_EVENTS.fetch_add(1, Ordering::Relaxed);
}

fn writer_loop(path: &Path, rx: Receiver<String>) {
    let open = |p: &Path| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
            .ok()
            .map(std::io::BufWriter::new)
    };
    let mut out = open(path);
    let mut written = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    loop {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(line) => {
                if written + line.len() as u64 + 1 > MAX_BYTES {
                    if let Some(mut w) = out.take() {
                        let _ = w.flush();
                    }
                    rotate(path);
                    out = open(path);
                    written = 0;
                }
                if let Some(w) = out.as_mut() {
                    if writeln!(w, "{line}").is_ok() {
                        written += line.len() as u64 + 1;
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Some(w) = out.as_mut() {
                    let _ = w.flush();
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    if let Some(mut w) = out {
        let _ = w.flush();
    }
}

/// debug.jsonl → debug.1.jsonl … 가장 오래된 것은 지운다.
fn rotate(path: &Path) {
    let name = |i: usize| path.with_file_name(format!("debug.{i}.jsonl"));
    let _ = std::fs::remove_file(name(KEEP_FILES - 1));
    for i in (1..KEEP_FILES - 1).rev() {
        let _ = std::fs::rename(name(i), name(i + 1));
    }
    let _ = std::fs::rename(path, name(1));
}

/// 10초 표본 — 앱과 웹뷰 도우미(이 앱 뒤에 시작된 WebKit/WebView2 프로세스)의 CPU·메모리·스레드, 채팅 처리량.
fn sampler_loop() {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let mut sys = System::new();
    let me = Pid::from_u32(std::process::id());
    // 이름·시작 시각은 PID 찾기에만 — 10초 표본은 찾아 둔 몇 개 프로세스의 CPU·메모리만 다시 읽는다
    // (시스템 전체 목록을 10초마다 새로 읽으면 그것만으로 앱 CPU 가 몇 % 올랐다).
    let find_kind = ProcessRefreshKind::nothing();
    let sample_kind = ProcessRefreshKind::nothing()
        .with_cpu()
        .with_memory()
        .with_tasks();
    let mut watched: Vec<Pid> = vec![me];
    let mut my_start = 0;
    let mut tick: u64 = 0;
    loop {
        std::thread::sleep(SAMPLE_EVERY);
        if !enabled() {
            continue;
        }
        // 60초마다(처음 포함) 도우미 PID 를 다시 찾는다 — 웹뷰가 프로세스를 새로 띄울 수 있다.
        if tick.is_multiple_of(6) {
            sys.refresh_processes_specifics(ProcessesToUpdate::All, true, find_kind);
            if my_start == 0 {
                my_start = sys.process(me).map(|p| p.start_time()).unwrap_or(0);
            }
            watched = sys
                .processes()
                .iter()
                .filter(|(pid, p)| {
                    let name = p.name().to_string_lossy();
                    **pid == me
                        || ((name.contains("WebKit") || name.contains("msedgewebview2"))
                            && p.start_time() + 2 >= my_start)
                })
                .map(|(pid, _)| *pid)
                .collect();
        }
        tick += 1;
        sys.refresh_processes_specifics(ProcessesToUpdate::Some(&watched), true, sample_kind);
        let mut procs = Vec::new();
        let (mut cpu, mut rss) = (0.0f64, 0u64);
        for pid in &watched {
            let Some(p) = sys.process(*pid) else { continue };
            cpu += f64::from(p.cpu_usage());
            rss += p.memory();
            procs.push(json!({
                "name": p.name().to_string_lossy(),
                "pid": pid.as_u32(),
                "cpu": round1(f64::from(p.cpu_usage())),
                "rss_mb": round1(p.memory() as f64 / 1_048_576.0),
                "threads": p.tasks().map(|t| t.len()),
            }));
        }
        let secs = SAMPLE_EVERY.as_secs_f64();
        emit(
            "process",
            json!({
                "cpu_total": round1(cpu),
                "rss_total_mb": round1(rss as f64 / 1_048_576.0),
                "procs": procs,
                "chat_msgs_per_s": round1(CHAT_MESSAGES.swap(0, Ordering::Relaxed) as f64 / secs),
                "chat_batches_per_s": round1(CHAT_BATCHES.swap(0, Ordering::Relaxed) as f64 / secs),
                "ui_events_per_s": round1(UI_EVENTS.swap(0, Ordering::Relaxed) as f64 / secs),
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_hides_secret_keys_deeply() {
        let v = redact(
            json!({"path": "/a", "Cookie": "NID_SES=x", "inner": {"accessToken": "t", "n": 1}, "list": [{"nid_aut": "y"}]}),
        );
        assert_eq!(v["path"], "/a");
        assert_eq!(v["Cookie"], "[redacted]");
        assert_eq!(v["inner"]["accessToken"], "[redacted]");
        assert_eq!(v["inner"]["n"], 1);
        assert_eq!(v["list"][0]["nid_aut"], "[redacted]");
    }

    #[test]
    fn emit_is_noop_when_disabled() {
        // 꺼진 상태에서는 보내지도, 막히지도 않는다.
        assert!(!enabled());
        emit("x", json!({"a": 1}));
        count_chat_batch(3);
        assert_eq!(CHAT_MESSAGES.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn rotate_shifts_files() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(FILE_NAME);
        std::fs::write(&p, "a").unwrap();
        rotate(&p);
        assert!(!p.exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("debug.1.jsonl")).unwrap(),
            "a"
        );
    }
}
