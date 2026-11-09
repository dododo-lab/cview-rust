//! 웹 화면 멈춤 감시 — 창 전체가 검게 남는 문제를 스스로 되살린다.
//!
//! 실제 사례(2026-11-09, v0.1.83, 멀티라이브 5패인을 27시간 켜 둔 세션): WebKit GPU 프로세스가 메모리 압박 속에
//! 연달아 "응답 없음"으로 강제 종료됐고(`GPUProcessProxy::didBecomeUnresponsive … terminating it` 4번),
//! 마지막 재연결 뒤 웹 콘텐츠 프로세스가 GPU 연결을 끝내 초기화하지 못한 채 멈췄다. 프로세스는 살아 있어
//! `webViewWebContentProcessDidTerminate`(Tauri 기본 동작: 다시 불러오기)가 불리지 않았고, 창에는 그림이
//! 하나도 없어 창 배경색(#0f1012)만 보였다. 사용자는 16분 뒤 앱을 직접 껐다.
//!
//! 그래서 웹 화면이 살아 있다는 신호(heartbeat)를 Rust 가 지켜본다.
//! - 웹 화면은 시작할 때와 몇 초마다, 그리고 Rust 의 핑(`window.__cviewBeat()`)을 받을 때마다 `ui_heartbeat` 를 부른다.
//! - Rust 는 3초마다 창을 본다. 사용자에게 실제로 보이는 창(보임·최소화 아님·macOS 는 가려지지 않음, 다른 OS 는 초점)이
//!   [`STALE`] 동안 계속 보였는데 그동안 신호가 한 번도 없으면 멈춘 것으로 본다.
//! - 되살리기: macOS 는 멈춘 웹 콘텐츠 프로세스를 끝낸다 → WebKit 이 종료를 알리고 Tauri 가 다시 불러온다.
//!   그 뒤에도 신호가 없으면(또는 다른 OS) 직접 다시 불러온다.
//!
//! 오판을 막는 장치:
//! - 첫 신호 전(문서를 읽는 중·IPC 가 없는 로그인 창)은 보지 않는다(`armed`).
//! - 감시 틱 사이가 크게 벌어지면(잠자기·메뉴 추적 등으로 메인 스레드가 멈췄던 경우) 보인 시간을 처음부터 센다.
//! - 되살리기는 1분에 한 번, 10분에 세 번까지만 한다(되살려도 곧 다시 멈추는 상황에서 끝없이 반복하지 않게).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, Runtime, WebviewWindow};

/// 감시 주기.
pub const TICK: Duration = Duration::from_secs(3);
/// 보이는 창이 이만큼 신호 없이 지나면 멈춘 것으로 본다. 웹 화면은 핑마다 바로 답하므로 넉넉한 값이다.
pub const STALE: Duration = Duration::from_secs(15);
/// 틱 사이가 이보다 벌어지면 연속해서 지켜본 것이 아니다(잠자기·메인 스레드 정지).
const TICK_GAP_RESET: Duration = Duration::from_millis(TICK.as_millis() as u64 * 5 / 2);
/// 되살리기 사이의 최소 간격.
const RECOVER_COOLDOWN: Duration = Duration::from_secs(60);
/// 이 시간 안에서는 [`RECOVER_BURST`] 번까지만 되살린다.
const RECOVER_WINDOW: Duration = Duration::from_secs(600);
const RECOVER_BURST: usize = 3;
/// 프로세스를 끝낸 뒤 이만큼 지나도 신호가 없으면 직접 다시 불러온다.
const RELOAD_FALLBACK: Duration = Duration::from_secs(8);

/// 창 하나의 감시 상태.
#[derive(Debug, Clone, Default)]
pub struct WindowWatch {
    last_beat: Option<Instant>,
    /// 사용자에게 보이기 시작한 때(계속 보이는 동안 유지).
    visible_since: Option<Instant>,
    last_tick: Option<Instant>,
    recoveries: Vec<Instant>,
    /// 되살리기를 시작한 때 — 다음 신호가 오면 지운다.
    recovering_since: Option<Instant>,
    fallback_reloaded: bool,
}

/// 한 번의 틱에서 내릴 판단.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 할 일 없음.
    Healthy,
    /// 멈췄다 — 웹 콘텐츠 프로세스를 끝내고 다시 불러온다.
    Recover,
    /// 되살리기 뒤에도 신호가 없다 — 직접 다시 불러온다.
    ReloadFallback,
}

impl WindowWatch {
    /// 웹 화면의 신호.
    pub fn beat(&mut self, now: Instant) {
        self.last_beat = Some(now);
        self.recovering_since = None;
        self.fallback_reloaded = false;
    }

    /// 틱마다 부른다. `shown` = 지금 사용자에게 보이는 창인가.
    pub fn tick(&mut self, now: Instant, shown: bool) -> Verdict {
        let continuous = self
            .last_tick
            .is_some_and(|t| now.saturating_duration_since(t) <= TICK_GAP_RESET);
        self.last_tick = Some(now);

        // 되살리는 중 — 프로세스를 끝냈는데도 신호가 없으면 직접 다시 불러온다(한 번만).
        // 그래도 1분 넘게 조용하면 되살리기를 끝내고 다시 지켜본다(다음 되살리기는 아래 횟수 제한을 따른다).
        if let Some(since) = self.recovering_since {
            let waited = now.saturating_duration_since(since);
            if waited < RECOVER_COOLDOWN {
                if !self.fallback_reloaded && waited >= RELOAD_FALLBACK {
                    self.fallback_reloaded = true;
                    return Verdict::ReloadFallback;
                }
                return Verdict::Healthy;
            }
            self.recovering_since = None;
            self.visible_since = None;
        }

        if !shown || !continuous {
            self.visible_since = if shown { Some(now) } else { None };
            return Verdict::Healthy;
        }
        let since = *self.visible_since.get_or_insert(now);
        // 첫 신호 전에는 보지 않는다 — 문서를 읽는 중이거나 IPC 가 없는 창이다.
        let Some(beat) = self.last_beat else {
            return Verdict::Healthy;
        };
        let silent = now.saturating_duration_since(beat);
        let watched = now.saturating_duration_since(since);
        if silent < STALE || watched < STALE {
            return Verdict::Healthy;
        }
        self.recoveries
            .retain(|t| now.saturating_duration_since(*t) < RECOVER_WINDOW);
        let cooling = self
            .recoveries
            .last()
            .is_some_and(|t| now.saturating_duration_since(*t) < RECOVER_COOLDOWN);
        if cooling || self.recoveries.len() >= RECOVER_BURST {
            return Verdict::Healthy;
        }
        self.recoveries.push(now);
        self.recovering_since = Some(now);
        self.fallback_reloaded = false;
        self.visible_since = None;
        Verdict::Recover
    }
}

/// 앱 전체 감시기(창 라벨별 상태).
#[derive(Default)]
pub struct UiWatchdog {
    windows: Mutex<HashMap<String, WindowWatch>>,
}

impl UiWatchdog {
    pub fn beat(&self, label: &str) {
        if let Ok(mut map) = self.windows.lock() {
            map.entry(label.to_string())
                .or_default()
                .beat(Instant::now());
        }
    }

    /// 이 창이 신호를 보낸 적이 있는가(앱 화면인가) — 로그인 창(외부 페이지)처럼 IPC 가 없는 창에는 핑하지 않는다.
    fn armed(&self, label: &str) -> bool {
        self.windows
            .lock()
            .is_ok_and(|map| map.get(label).is_some_and(|w| w.last_beat.is_some()))
    }

    fn tick(&self, label: &str, shown: bool, now: Instant) -> Verdict {
        match self.windows.lock() {
            Ok(mut map) => map.entry(label.to_string()).or_default().tick(now, shown),
            Err(_) => Verdict::Healthy,
        }
    }

    fn forget_missing(&self, alive: &[String]) {
        if let Ok(mut map) = self.windows.lock() {
            map.retain(|label, _| alive.iter().any(|a| a == label));
        }
    }
}

/// 웹 화면이 살아 있다는 신호.
#[tauri::command]
pub fn ui_heartbeat(window: WebviewWindow, watchdog: tauri::State<'_, UiWatchdog>) {
    watchdog.beat(window.label());
}

/// 감시를 시작한다(앱 시작 때 한 번).
pub fn spawn<R: Runtime>(app: AppHandle<R>) {
    app.manage(UiWatchdog::default());
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(TICK);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let app = app.clone();
            // 창 상태 조회는 메인 스레드를 기다린다 — 비동기 실행기를 막지 않게 따로 돈다.
            let _ = tauri::async_runtime::spawn_blocking(move || check_windows(&app)).await;
        }
    });
}

fn check_windows<R: Runtime>(app: &AppHandle<R>) {
    let Some(watchdog) = app.try_state::<UiWatchdog>() else {
        return;
    };
    let windows = app.webview_windows();
    let labels: Vec<String> = windows.keys().cloned().collect();
    watchdog.forget_missing(&labels);
    for (label, window) in windows {
        let shown = shown_to_user(&window);
        if shown && watchdog.armed(&label) {
            // 핑 — 살아 있으면 웹 화면이 바로 `ui_heartbeat` 로 답한다(숨은 창의 타이머 지연과 무관).
            let _ = window.eval("window.__cviewBeat && window.__cviewBeat()");
        }
        // 조회가 메인 스레드를 기다리느라 늦었을 수 있다 — 판단 시각은 조회 뒤로 잡는다.
        let verdict = watchdog.tick(&label, shown, Instant::now());
        tracing::trace!(window = %label, shown, armed = watchdog.armed(&label), ?verdict, "화면 감시");
        match verdict {
            Verdict::Healthy => {}
            Verdict::Recover => recover(&window),
            Verdict::ReloadFallback => {
                tracing::warn!(window = %label, "웹 화면이 되살아나지 않아 다시 불러옵니다");
                crate::perf_log::emit(
                    "ui.watchdog",
                    serde_json::json!({ "window": label, "action": "reload_fallback" }),
                );
                if let Err(e) = window.reload() {
                    tracing::warn!("다시 불러오기 실패: {e}");
                }
            }
        }
    }
}

/// 사용자에게 지금 보이는 창인가 — 보임·최소화 아님, 그리고 macOS 는 화면에 실제로 그려지는 중(가려지지 않음),
/// 다른 OS 는 초점이 있음. macOS 에서 초점만 보지 않는 것은 화면 잠금·디스플레이 꺼짐 때 초점은 남아도 WebKit 이
/// 숨은 페이지를 멈출 수 있어서다(그때 신호가 끊긴 것을 멈춤으로 오판하지 않게).
fn shown_to_user<R: Runtime>(window: &WebviewWindow<R>) -> bool {
    let visible = window.is_visible().unwrap_or(false);
    let minimized = window.is_minimized().unwrap_or(true);
    if !visible || minimized {
        return false;
    }
    on_screen(window)
}

#[cfg(target_os = "macos")]
fn on_screen<R: Runtime>(window: &WebviewWindow<R>) -> bool {
    use objc2_app_kit::{NSWindow, NSWindowOcclusionState};
    let Ok(ptr) = window.ns_window() else {
        return false;
    };
    if ptr.is_null() {
        return false;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let addr = ptr as usize;
    // NSWindow 는 메인 스레드에서만 만진다.
    let sent = window.run_on_main_thread(move || {
        // SAFETY: tao 가 돌려준 살아 있는 NSWindow 포인터이고, 메인 스레드에서 읽기만 한다.
        let ns_window = unsafe { &*(addr as *const NSWindow) };
        let state = ns_window.occlusionState();
        let _ = tx.send(state.contains(NSWindowOcclusionState::Visible));
    });
    sent.is_ok() && rx.recv_timeout(Duration::from_secs(2)).unwrap_or(false)
}

#[cfg(not(target_os = "macos"))]
fn on_screen<R: Runtime>(window: &WebviewWindow<R>) -> bool {
    window.is_focused().unwrap_or(false)
}

/// 멈춘 웹 화면을 되살린다.
fn recover<R: Runtime>(window: &WebviewWindow<R>) {
    let label = window.label().to_string();
    let killed = kill_web_content_process(window);
    tracing::warn!(
        window = %label,
        killed_pid = ?killed,
        "웹 화면이 {}초 넘게 응답하지 않아 되살립니다",
        STALE.as_secs()
    );
    crate::perf_log::emit(
        "ui.watchdog",
        serde_json::json!({ "window": label, "action": "recover", "killed_pid": killed }),
    );
    // 프로세스를 끝내지 못했으면 바로 다시 불러온다(끝냈으면 WebKit 종료 알림 → Tauri 가 다시 불러온다).
    if killed.is_none() {
        if let Err(e) = window.reload() {
            tracing::warn!("다시 불러오기 실패: {e}");
        }
    }
}

/// macOS — 이 창의 WKWebView 웹 콘텐츠 프로세스를 끝낸다. 끝낸 프로세스 번호를 돌려준다.
#[cfg(target_os = "macos")]
fn kill_web_content_process<R: Runtime>(window: &WebviewWindow<R>) -> Option<i32> {
    use objc2::runtime::AnyObject;
    use objc2::{msg_send, sel};

    let (tx, rx) = std::sync::mpsc::channel();
    let res = window.with_webview(move |webview| {
        let wk = webview.inner() as *mut AnyObject;
        if wk.is_null() {
            let _ = tx.send(None);
            return;
        }
        // SAFETY: Tauri 가 넘겨준 살아 있는 WKWebView 이고 메인 스레드에서 부른다. 비공개 선택자는
        // respondsToSelector 로 있는지 먼저 확인한다(없으면 아무것도 하지 않는다).
        let pid = unsafe {
            let wk = &*wk;
            let responds: bool = msg_send![wk, respondsToSelector: sel!(_webProcessIdentifier)];
            if responds {
                let pid: i32 = msg_send![wk, _webProcessIdentifier];
                pid
            } else {
                0
            }
        };
        let _ = tx.send(Some(pid));
    });
    if res.is_err() {
        return None;
    }
    let pid = rx.recv_timeout(Duration::from_secs(2)).ok().flatten()?;
    if pid <= 0 || pid as u32 == std::process::id() {
        return None;
    }
    // SAFETY: 이 앱이 띄운 WebKit 웹 콘텐츠 프로세스 번호에 신호를 보낼 뿐이다.
    let rc = unsafe { libc::kill(pid, libc::SIGKILL) };
    (rc == 0).then_some(pid)
}

#[cfg(not(target_os = "macos"))]
fn kill_web_content_process<R: Runtime>(_window: &WebviewWindow<R>) -> Option<i32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    /// `start` 부터 `until` 초까지 3초 간격으로 틱을 돌리고, 마지막 판단과 그때까지 나온 Recover 수를 돌려준다.
    fn run(
        w: &mut WindowWatch,
        start: Instant,
        from: u64,
        until: u64,
        shown: bool,
    ) -> (Verdict, usize) {
        let mut last = Verdict::Healthy;
        let mut recovers = 0;
        let mut t = from;
        while t <= until {
            last = w.tick(start + secs(t), shown);
            if last == Verdict::Recover {
                recovers += 1;
            }
            t += 3;
        }
        (last, recovers)
    }

    #[test]
    fn 신호가_계속_오면_건드리지_않는다() {
        let t0 = Instant::now();
        let mut w = WindowWatch::default();
        let mut t = 0;
        while t <= 120 {
            w.beat(t0 + secs(t));
            assert_eq!(w.tick(t0 + secs(t), true), Verdict::Healthy);
            t += 3;
        }
    }

    #[test]
    fn 보이는_창이_신호_없이_멈추면_되살린다() {
        let t0 = Instant::now();
        let mut w = WindowWatch::default();
        w.beat(t0);
        let (_, recovers) = run(&mut w, t0, 0, 30, true);
        assert_eq!(recovers, 1, "15초 넘게 조용하면 한 번 되살린다");
    }

    #[test]
    fn 첫_신호_전에는_되살리지_않는다() {
        let t0 = Instant::now();
        let mut w = WindowWatch::default();
        let (_, recovers) = run(&mut w, t0, 0, 300, true);
        assert_eq!(recovers, 0, "문서를 읽는 중이거나 IPC 없는 창");
    }

    #[test]
    fn 숨은_창은_되살리지_않고_보이기_시작한_뒤부터_센다() {
        let t0 = Instant::now();
        let mut w = WindowWatch::default();
        w.beat(t0);
        // 10분 동안 숨어 있었다(타이머가 느려져 신호가 끊겨도 정상).
        let (_, recovers) = run(&mut w, t0, 0, 600, false);
        assert_eq!(recovers, 0);
        // 다시 보이면 15초를 새로 센다 — 그 안에 신호가 오면 그대로 둔다.
        assert_eq!(w.tick(t0 + secs(603), true), Verdict::Healthy);
        assert_eq!(w.tick(t0 + secs(606), true), Verdict::Healthy);
        w.beat(t0 + secs(607));
        let (_, recovers) = run(&mut w, t0, 609, 618, true);
        assert_eq!(recovers, 0);
    }

    #[test]
    fn 틱_사이가_벌어지면_보인_시간을_다시_센다() {
        let t0 = Instant::now();
        let mut w = WindowWatch::default();
        w.beat(t0);
        assert_eq!(w.tick(t0 + secs(3), true), Verdict::Healthy);
        // 잠자기·메뉴 추적으로 메인 스레드가 1분 멈췄다가 돌아온 첫 틱 — 바로 되살리지 않는다.
        assert_eq!(w.tick(t0 + secs(63), true), Verdict::Healthy);
        assert_eq!(w.tick(t0 + secs(66), true), Verdict::Healthy);
        // 돌아온 뒤에도 15초 넘게 조용하면 그때 되살린다.
        let (_, recovers) = run(&mut w, t0, 69, 81, true);
        assert_eq!(recovers, 1);
    }

    #[test]
    fn 되살린_뒤_신호가_없으면_한_번_직접_다시_불러온다() {
        let t0 = Instant::now();
        let mut w = WindowWatch::default();
        w.beat(t0);
        let (_, recovers) = run(&mut w, t0, 0, 18, true);
        assert_eq!(recovers, 1);
        let mut fallbacks = 0;
        let mut t = 21;
        while t <= 60 {
            if w.tick(t0 + secs(t), true) == Verdict::ReloadFallback {
                fallbacks += 1;
            }
            t += 3;
        }
        assert_eq!(fallbacks, 1, "8초 뒤 한 번만");
    }

    #[test]
    fn 되살리면_다시_불러온_화면의_신호로_감시를_이어간다() {
        let t0 = Instant::now();
        let mut w = WindowWatch::default();
        w.beat(t0);
        let (_, recovers) = run(&mut w, t0, 0, 18, true);
        assert_eq!(recovers, 1);
        // 다시 불러온 화면이 신호를 보낸다 → 되살리기 끝, 다시 불러오기 대비책도 나오지 않는다.
        w.beat(t0 + secs(20));
        for t in [21u64, 24, 27, 30] {
            w.beat(t0 + secs(t));
            assert_eq!(w.tick(t0 + secs(t), true), Verdict::Healthy);
        }
    }

    #[test]
    fn 계속_멈추면_1분에_한_번_10분에_세_번까지만() {
        let t0 = Instant::now();
        let mut w = WindowWatch::default();
        w.beat(t0);
        let mut recover_times = Vec::new();
        let mut t = 0;
        while t <= 1200 {
            if w.tick(t0 + secs(t), true) == Verdict::Recover {
                recover_times.push(t);
            }
            t += 3;
        }
        // 10분 창 안에서는 세 번까지 — 첫 되살리기 뒤 10분이 지나야 네 번째가 나온다.
        let in_first_window: Vec<_> = recover_times.iter().filter(|t| **t < 600).collect();
        assert_eq!(in_first_window.len(), 3, "{recover_times:?}");
        for pair in recover_times.windows(2) {
            assert!(pair[1] - pair[0] >= 60, "1분 간격: {recover_times:?}");
        }
    }
}
