//! 메뉴 막대(트레이) — 원본 MenuBarView(방송 중 팔로잉 목록·열기·종료) (SYS-20), AppIconRenderer 의
//! 라이브 수 배지(SYS-23, Dock 배지 숫자로 근사).
//!
//! 1분마다 팔로잉 결과(화면 진입 폴 — 60초 안이면 캐시)를 읽어 메뉴를 다시 만든다. 일반 설정
//! "메뉴 막대에 표시"를 끄면 아이콘을 숨긴다. 항목을 고르면 딥 링크와 같은 경로로 화면에 넘긴다.

use std::time::Duration;

use cview_app_core::caching::following_store::to_live_channel_item;
use cview_core::utils::following_poll::FollowingPollReason;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Runtime};

use crate::state::AppServices;

const TRAY_ID: &str = "cview-tray";
const LIVE_PREFIX: &str = "tray-live:";
const MAX_ITEMS: usize = 12;

/// 메뉴 한 줄 — (채널 ID, 표시). 순수 함수.
pub fn live_label(name: &str, category: Option<&str>, viewers: i64) -> String {
    let viewers = cview_core::models::live_channel_item::format_viewer_count(viewers);
    match category.filter(|c| !c.is_empty()) {
        Some(c) => format!("{name} — {c} · {viewers}"),
        None => format!("{name} · {viewers}"),
    }
}

fn build_menu<R: Runtime>(
    app: &AppHandle<R>,
    lives: &[(String, String)],
    logged_in: bool,
) -> tauri::Result<Menu<R>> {
    let menu = Menu::new(app)?;
    let header = if !logged_in {
        "로그인하면 방송 중인 팔로잉이 보입니다".to_string()
    } else if lives.is_empty() {
        "방송 중인 팔로잉 없음".to_string()
    } else {
        format!("방송 중 {}개", lives.len())
    };
    menu.append(&MenuItem::with_id(
        app,
        "tray-header",
        header,
        false,
        None::<&str>,
    )?)?;
    for (id, label) in lives.iter().take(MAX_ITEMS) {
        menu.append(&MenuItem::with_id(
            app,
            format!("{LIVE_PREFIX}{id}"),
            label,
            true,
            None::<&str>,
        )?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        "tray-open",
        "CView 열기",
        true,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(
        app,
        "tray-multilive",
        "멀티라이브",
        true,
        None::<&str>,
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&PredefinedMenuItem::quit(app, Some("CView 종료"))?)?;
    Ok(menu)
}

fn on_menu<R: Runtime>(app: &AppHandle<R>, id: &str) {
    if let Some(channel_id) = id.strip_prefix(LIVE_PREFIX) {
        crate::system::handle_urls(app, [format!("cview://live?channelId={channel_id}")]);
    } else if id == "tray-multilive" {
        crate::system::handle_urls(app, ["cview://multilive".to_string()]);
    } else if id == "tray-open" {
        if let Some(w) = app.get_webview_window(crate::MAIN_WINDOW) {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
        }
    }
}

/// 트레이 아이콘을 달고 갱신 작업을 띄운다.
pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app, &[], false)?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("CView")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| on_menu(app, event.id().as_ref()));
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(8)).await;
        loop {
            refresh(&handle).await;
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
    Ok(())
}

async fn refresh(app: &AppHandle) {
    let services = app.state::<AppServices>();
    let general = services.settings.general();
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let _ = tray.set_visible(general.show_in_menu_bar);
    let logged_in = services.auth.is_authenticated();
    let mut lives: Vec<(String, String)> = Vec::new();
    if logged_in {
        if let Some(result) = services
            .following
            .poll(FollowingPollReason::ScreenEntry)
            .await
        {
            let mut items: Vec<_> = result
                .channels
                .iter()
                .map(to_live_channel_item)
                .filter(|i| i.is_live)
                .collect();
            items.sort_by_key(|i| std::cmp::Reverse(i.viewer_count));
            lives = items
                .iter()
                .map(|i| {
                    (
                        i.channel_id.clone(),
                        live_label(&i.channel_name, i.category_name.as_deref(), i.viewer_count),
                    )
                })
                .collect();
        }
    }
    if let Ok(menu) = build_menu(app, &lives, logged_in) {
        let _ = tray.set_menu(Some(menu));
    }
    let count = lives.len();
    let _ = tray.set_title(if count > 0 {
        Some(count.to_string())
    } else {
        None
    });
    if let Some(w) = app.get_webview_window(crate::MAIN_WINDOW) {
        let _ = w.set_badge_count(if count > 0 { Some(count as i64) } else { None });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(
            live_label("한동숙", Some("마인크래프트"), 12_300),
            "한동숙 — 마인크래프트 · 1.2만"
        );
        assert_eq!(live_label("a", None, 900), "a · 900");
    }
}
