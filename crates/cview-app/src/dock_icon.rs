//! 실행 중 Dock 아이콘 — 앱 테마(밝게·어둡게·시스템)에 맞춰 밝은·어두운 아이콘을 고른다.
//!
//! - 앱 테마가 "시스템"이고 macOS 26 이상이면 손대지 않는다: 번들의 Assets.car(AppIcon 밝음·어두움·틴트)를
//!   시스템이 외형에 맞춰 그린다(Liquid Glass 효과 포함).
//! - 앱 테마를 밝게/어둡게로 고정했거나 macOS 25 이하면 NSApplication.applicationIconImage 로 직접 바꾼다.

use cview_core::models::settings::AppTheme;
use tauri::{AppHandle, Runtime};

#[cfg(target_os = "macos")]
const LIGHT_PNG: &[u8] = include_bytes!("../icons/dock-light.png");
#[cfg(target_os = "macos")]
const DARK_PNG: &[u8] = include_bytes!("../icons/dock-dark.png");

/// 어떤 아이콘을 쓸지 — `None` 이면 번들 아이콘(시스템이 외형에 맞춰 고름). 순수 함수.
pub fn choose(theme: AppTheme, macos_major: u64, window_is_dark: bool) -> Option<bool> {
    match theme {
        AppTheme::Light => Some(false),
        AppTheme::Dark => Some(true),
        AppTheme::System if macos_major >= 26 => None,
        AppTheme::System => Some(window_is_dark),
    }
}

/// 앱 테마에 맞춰 Dock 아이콘을 바꾼다(메인 스레드에서).
pub fn apply<R: Runtime>(app: &AppHandle<R>, theme: AppTheme) {
    #[cfg(target_os = "macos")]
    {
        use tauri::Manager;
        let window_is_dark = app
            .get_webview_window(crate::MAIN_WINDOW)
            .and_then(|w| w.theme().ok())
            .is_some_and(|t| t == tauri::Theme::Dark);
        let _ = app.run_on_main_thread(move || set_icon(theme, window_is_dark));
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, theme);
    }
}

#[cfg(target_os = "macos")]
fn set_icon(theme: AppTheme, window_is_dark: bool) {
    use objc2::AllocAnyThread;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::{NSData, NSProcessInfo};

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let major = NSProcessInfo::processInfo()
        .operatingSystemVersion()
        .majorVersion as u64;
    let ns_app = NSApplication::sharedApplication(mtm);
    match choose(theme, major, window_is_dark) {
        None => unsafe { ns_app.setApplicationIconImage(None) },
        Some(dark) => {
            let data = NSData::with_bytes(if dark { DARK_PNG } else { LIGHT_PNG });
            if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
                unsafe { ns_app.setApplicationIconImage(Some(&image)) };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_icon_by_app_theme_and_os() {
        assert_eq!(choose(AppTheme::Dark, 27, false), Some(true));
        assert_eq!(choose(AppTheme::Light, 27, true), Some(false));
        assert_eq!(
            choose(AppTheme::System, 27, true),
            None,
            "26+ 는 번들 Assets.car 에 맡김"
        );
        assert_eq!(choose(AppTheme::System, 15, true), Some(true));
        assert_eq!(choose(AppTheme::System, 15, false), Some(false));
    }
}
