//! OS 메모리 압박 신호 → 앱 캐시 줄이기.
//!
//! [메모리 2026-11-24] 예전엔 OS 압박 신호를 받지 않았다(`app_lifecycle.rs` 가 "메모리 압박 DispatchSource 는 건너뜀").
//! 만들어 둔 줄이기 함수(`ChzzkApiClient::handle_memory_pressure`·`MultiLiveManager::handle_memory_pressure`)도
//! 부르는 곳이 없었다. 장시간 멀티라이브에서 macOS 가 메모리 압박에 이르러도 앱은 양보하지 않다가 WebKit GPU 프로세스가
//! 강제 종료되기도 했다(`ui_watchdog.rs` 머리 주석의 27시간 5패인 사례).
//!
//! - macOS: libdispatch `DISPATCH_SOURCE_TYPE_MEMORYPRESSURE`(경고·심각).
//! - Windows: `CreateMemoryResourceNotification(LowMemoryResourceNotification)` 을 15초마다 본다(시스템 물리 메모리 부족 = 경고).
//!
//! 받으면([`relieve`]) 메모리에 둔 이미지를 줄이고(경고 4 MiB, 심각 0), API 응답 캐시를 절반(심각이면 전부) 비우고,
//! SQLite 두 DB 의 페이지·문장 캐시를 놓고(`PRAGMA shrink_memory`),
//! 메인 창에 알려 화면 밖 페이지의 이미지를 놓게 한다. 심각이면 멀티라이브 비선택 패인 디코드를 20초 멈춘다
//! (`MultiLiveManager::handle_memory_pressure` — 원본 Swift 와 같은 규칙, 시간이 지나면 스스로 돌아온다).
//! 설정의 "메모리 목표(maxMemoryMB)" 는 여기에 잇지 않았다 — 기본 512 MB 가 보통 사용량보다 낮아 늘 줄이게 된다.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

use crate::image_service::{ImageService, HOT_TRIM_WARN_BYTES};
use crate::multi_live::MultiLiveShell;
use crate::state::AppServices;
use crate::MAIN_WINDOW;

/// 메인 창에 보내는 이벤트 — 값은 `"warn"` / `"critical"`.
pub const PRESSURE_EVENT: &str = "app://memory-pressure";
/// 같은 단계(또는 더 낮은 단계)는 이 간격 안에 다시 처리하지 않는다 — 신호가 몰려도 한 번만 줄인다.
const COOLDOWN: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Warn,
    Critical,
}

impl Level {
    fn name(self) -> &'static str {
        match self {
            Level::Warn => "warn",
            Level::Critical => "critical",
        }
    }
}

/// 직전 처리 — 같은 단계 이하가 `COOLDOWN` 안에 또 오면 건너뛴다(심각은 경고 뒤에도 바로 처리).
fn should_handle(last: &mut Option<(Instant, Level)>, now: Instant, level: Level) -> bool {
    if let Some((at, prev)) = *last {
        if level <= prev && now.duration_since(at) < COOLDOWN {
            return false;
        }
    }
    *last = Some((now, level));
    true
}

static LAST: Mutex<Option<(Instant, Level)>> = Mutex::new(None);

/// 압박을 받았을 때 — 어느 스레드에서 불러도 된다(tokio 작업은 앱 런타임으로 넘긴다).
pub fn relieve(app: &AppHandle, level: Level) {
    {
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if !should_handle(&mut last, Instant::now(), level) {
            return;
        }
    }
    let critical = level == Level::Critical;
    tracing::warn!("메모리 압박({}) — 캐시를 줄입니다", level.name());
    if let Some(images) = app.try_state::<Arc<ImageService>>() {
        let before = images.hot_bytes();
        images.trim_hot(if critical { 0 } else { HOT_TRIM_WARN_BYTES });
        tracing::info!("메모리 이미지 {before} → {} bytes", images.hot_bytes());
        let images = images.inner().clone();
        tauri::async_runtime::spawn_blocking(move || images.release_cache_memory());
    }
    if let Some(services) = app.try_state::<AppServices>() {
        services.api.handle_memory_pressure(critical);
        if let Some(store) = services.data_store.clone() {
            tauri::async_runtime::spawn_blocking(move || store.release_memory());
        }
    }
    if critical {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Some(shell) = app.try_state::<MultiLiveShell>() {
                shell.manager.lock().handle_memory_pressure(true);
            }
        });
    }
    let _ = app.emit_to(MAIN_WINDOW, PRESSURE_EVENT, level.name());
}

/// OS 압박 신호를 듣기 시작한다(앱 수명 동안 한 번).
pub fn start(app: AppHandle) {
    platform::start(app);
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{relieve, Level};
    use std::ffi::c_void;
    use std::sync::OnceLock;
    use tauri::AppHandle;

    #[repr(C)]
    struct DispatchSourceType {
        _private: [u8; 0],
    }

    extern "C" {
        static _dispatch_source_type_memorypressure: DispatchSourceType;
        fn dispatch_source_create(
            kind: *const DispatchSourceType,
            handle: usize,
            mask: usize,
            queue: *mut c_void,
        ) -> *mut c_void;
        fn dispatch_get_global_queue(identifier: isize, flags: usize) -> *mut c_void;
        fn dispatch_source_set_event_handler_f(
            source: *mut c_void,
            handler: extern "C" fn(*mut c_void),
        );
        fn dispatch_source_get_data(source: *mut c_void) -> usize;
        fn dispatch_resume(object: *mut c_void);
    }

    const DISPATCH_MEMORYPRESSURE_WARN: usize = 0x02;
    const DISPATCH_MEMORYPRESSURE_CRITICAL: usize = 0x04;
    /// QOS_CLASS_UTILITY
    const QOS_UTILITY: isize = 0x11;

    struct Source(usize);
    static SOURCE: OnceLock<(Source, AppHandle)> = OnceLock::new();

    extern "C" fn on_event(_context: *mut c_void) {
        let Some((source, app)) = SOURCE.get() else {
            return;
        };
        // SAFETY: SOURCE 는 앱 수명 동안 놓지 않는(취소하지 않는) 디스패치 소스다.
        let data = unsafe { dispatch_source_get_data(source.0 as *mut c_void) };
        if data & DISPATCH_MEMORYPRESSURE_CRITICAL != 0 {
            relieve(app, Level::Critical);
        } else if data & DISPATCH_MEMORYPRESSURE_WARN != 0 {
            relieve(app, Level::Warn);
        }
    }

    pub(super) fn start(app: AppHandle) {
        if SOURCE.get().is_some() {
            return;
        }
        // SAFETY: libdispatch C API — 전역 큐에 경고·심각 마스크로 소스를 만들고 처리기를 단 뒤 시작한다.
        //   처리기는 소스가 SOURCE 에 들어간 뒤에만 불린다(dispatch_resume 이 마지막).
        unsafe {
            let queue = dispatch_get_global_queue(QOS_UTILITY, 0);
            let source = dispatch_source_create(
                &_dispatch_source_type_memorypressure,
                0,
                DISPATCH_MEMORYPRESSURE_WARN | DISPATCH_MEMORYPRESSURE_CRITICAL,
                queue,
            );
            if source.is_null() {
                tracing::warn!("메모리 압박 소스를 만들지 못했습니다");
                return;
            }
            dispatch_source_set_event_handler_f(source, on_event);
            if SOURCE.set((Source(source as usize), app)).is_err() {
                return;
            }
            dispatch_resume(source);
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{relieve, Level};
    use std::time::Duration;
    use tauri::AppHandle;
    use windows_sys::Win32::System::Memory::{
        CreateMemoryResourceNotification, LowMemoryResourceNotification,
        QueryMemoryResourceNotification,
    };

    const POLL: Duration = Duration::from_secs(15);

    pub(super) fn start(app: AppHandle) {
        let spawned = std::thread::Builder::new()
            .name("cview-mem-pressure".into())
            .stack_size(64 * 1024)
            .spawn(move || {
                // SAFETY: 커널 객체 핸들을 하나 열어 앱이 끝날 때까지 상태만 읽는다.
                let handle =
                    unsafe { CreateMemoryResourceNotification(LowMemoryResourceNotification) };
                if handle.is_null() {
                    tracing::warn!("메모리 부족 알림을 만들지 못했습니다");
                    return;
                }
                loop {
                    std::thread::sleep(POLL);
                    let mut low = 0;
                    // SAFETY: 위에서 연 핸들, 결과는 지역 변수에 쓴다.
                    let ok = unsafe { QueryMemoryResourceNotification(handle, &mut low) };
                    if ok != 0 && low != 0 {
                        relieve(&app, Level::Warn);
                    }
                }
            });
        if let Err(e) = spawned {
            tracing::warn!("메모리 압박 스레드를 시작하지 못했습니다: {e}");
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    pub(super) fn start(_app: tauri::AppHandle) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cooldown_skips_repeats_but_lets_escalation_through() {
        let mut last = None;
        let t0 = Instant::now();
        assert!(should_handle(&mut last, t0, Level::Warn));
        assert!(!should_handle(
            &mut last,
            t0 + Duration::from_secs(5),
            Level::Warn
        ));
        // 경고 뒤 심각은 바로 처리한다.
        assert!(should_handle(
            &mut last,
            t0 + Duration::from_secs(6),
            Level::Critical
        ));
        assert!(!should_handle(
            &mut last,
            t0 + Duration::from_secs(7),
            Level::Warn
        ));
        assert!(!should_handle(
            &mut last,
            t0 + Duration::from_secs(8),
            Level::Critical
        ));
        // 간격이 지나면 다시 처리한다.
        assert!(should_handle(
            &mut last,
            t0 + Duration::from_secs(40),
            Level::Warn
        ));
    }
}
