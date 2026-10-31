//! 앱 메뉴 막대 — 원본 `CViewApp` 의 CommandMenu(보기·이동·재생·채팅·창·도움말) (SYS-03).
//! 메뉴마다 종류가 같은 항목끼리 묶고 묶음 사이에 구분선([`SEP`])을 둔다.
//!
//! macOS 에서 메뉴 단축키는 웹뷰보다 먼저 키를 가져가므로, 공용 단축키(⌘1~7, ⌘R, ⌘S, ⌘, …)는 메뉴에
//! 두고 고르면 `menu://action` `{ id }` 로 화면에 넘긴다. 화면(app.js)이 같은 동작을 실행한다.
//! 편집 메뉴(복사·붙여넣기)는 기본 항목으로 둬야 입력창의 ⌘C/⌘V 가 산다.

use cview_core::models::settings_ui::{
    KeyBinding, KeyboardShortcutSettings, ShortcutAction, ShortcutModifiers,
};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// 화면으로 보내는 메뉴 이벤트 이름.
pub const MENU_ACTION: &str = "menu://action";

/// (id, 표시 이름, 단축키) — 단축키는 Tauri 가속기 문법. id 가 빈 [`SEP`] 는 구분선.
type Item = (&'static str, &'static str, Option<&'static str>);

/// 메뉴 안에서 종류가 다른 항목 묶음 사이에 넣는 구분선.
const SEP: Item = ("", "", None);

/// 보기 — 화면 다시 그리기와 겉모습(사이드바·테마).
const VIEW: &[Item] = &[
    ("refresh", "새로고침", Some("CmdOrCtrl+R")),
    SEP,
    (
        "toggle-sidebar",
        "사이드바 접기/펼치기",
        Some("CmdOrCtrl+\\"),
    ),
    (
        "theme-toggle",
        "다크/라이트 전환",
        Some("CmdOrCtrl+Shift+L"),
    ),
];

/// 이동 — 페이지 이동, 뒤로, 찾아 들어가기(검색·명령 팔레트).
const GO: &[Item] = &[
    ("nav-home", "홈", Some("CmdOrCtrl+1")),
    ("nav-following", "팔로잉", Some("CmdOrCtrl+2")),
    ("nav-popular", "인기", Some("CmdOrCtrl+3")),
    ("nav-multilive", "멀티라이브", Some("CmdOrCtrl+4")),
    ("nav-live", "라이브", Some("CmdOrCtrl+5")),
    ("nav-library", "보관함", Some("CmdOrCtrl+6")),
    ("nav-categories", "카테고리", Some("CmdOrCtrl+7")),
    SEP,
    ("back", "뒤로", Some("CmdOrCtrl+[")),
    SEP,
    ("search", "검색", Some("CmdOrCtrl+F")),
    ("palette", "명령 팔레트", Some("CmdOrCtrl+K")),
];

/// 재생 — 재생 제어 / 기록 / 화면 모드 / 닫기.
const PLAYBACK: &[Item] = &[
    ("play-pause", "재생/일시정지", None),
    ("mute", "음소거", None),
    ("go-live", "라이브로 이동", None),
    SEP,
    ("screenshot", "스크린샷", Some("CmdOrCtrl+S")),
    ("bookmark", "북마크", Some("CmdOrCtrl+Shift+B")),
    SEP,
    ("fullscreen", "영상 전체 화면", Some("CmdOrCtrl+Shift+F")),
    (
        "fullscreen-chat",
        "영상+채팅 전체 화면",
        Some(FULLSCREEN_CHAT_ACCEL),
    ),
    ("pip", "화면 속 화면(PiP)", Some("Alt+CmdOrCtrl+P")),
    SEP,
    ("close-viewer", "시청 화면 닫기", Some("CmdOrCtrl+W")),
];

/// ⌃⌘F — Windows 에는 ⌘·⌃ 가 따로 없어(CmdOrCtrl+Ctrl = Ctrl+F, 검색과 겹침) Ctrl+Shift+Alt+F.
#[cfg(target_os = "macos")]
const FULLSCREEN_CHAT_ACCEL: &str = "Cmd+Ctrl+F";
#[cfg(not(target_os = "macos"))]
const FULLSCREEN_CHAT_ACCEL: &str = "Ctrl+Shift+Alt+F";

/// 채팅 — 보기 설정·연결 / 지우기(되돌릴 수 없는 동작은 맨 끝).
const CHAT: &[Item] = &[
    (
        "chat-autoscroll",
        "자동 스크롤 켜기/끄기",
        Some("CmdOrCtrl+J"),
    ),
    ("chat-reconnect", "채팅 다시 연결", None),
    SEP,
    ("chat-clear", "채팅 지우기", Some("CmdOrCtrl+Shift+K")),
];

/// 윈도우 — 기본 창 조작 뒤에 붙는 앱 창 열기.
const WINDOWS: &[Item] = &[
    ("window-player", "새 플레이어 창", Some("CmdOrCtrl+N")),
    SEP,
    ("window-chat", "채팅 창", Some("CmdOrCtrl+Shift+C")),
    ("window-multichat", "멀티채팅 창", Some("CmdOrCtrl+Shift+M")),
    SEP,
    ("window-stats", "통계 창", Some("CmdOrCtrl+Shift+T")),
];

/// 도움말 — 사용법 안내 / 문제 해결.
const HELP: &[Item] = &[
    ("shortcuts", "키보드 단축키", Some("CmdOrCtrl+/")),
    ("welcome", "처음 안내 다시 보기", None),
    SEP,
    ("report-issue", "문제 보고…", None),
    ("open-logs", "데이터 폴더 열기", None),
];

/// 앱 메뉴(CView)에 직접 다는 항목.
const APP_ITEMS: &[&str] = &["about", "check-update", "settings", "notification-settings"];

/// 목록 → 메뉴 항목. [`SEP`] 는 구분선이 된다.
fn entries<R: Runtime>(
    app: &AppHandle<R>,
    list: &[Item],
) -> tauri::Result<Vec<Box<dyn tauri::menu::IsMenuItem<R>>>> {
    let built = items(app, list)?;
    let mut built = built.into_iter();
    let mut out: Vec<Box<dyn tauri::menu::IsMenuItem<R>>> = Vec::new();
    for (id, _, _) in list {
        if id.is_empty() {
            out.push(Box::new(PredefinedMenuItem::separator(app)?));
        } else if let Some(item) = built.next() {
            out.push(Box::new(item));
        }
    }
    Ok(out)
}

fn items<R: Runtime>(app: &AppHandle<R>, list: &[Item]) -> tauri::Result<Vec<MenuItem<R>>> {
    let keyboard = app
        .try_state::<crate::state::AppServices>()
        .map(|s| s.settings.keyboard())
        .unwrap_or_default();
    list.iter()
        .filter(|(id, _, _)| !id.is_empty())
        .map(|(id, label, accel)| match user_accelerator(&keyboard, id) {
            // 사용자 단축키가 있는 항목 — 수식 키가 있을 때만 메뉴에 단다(맨 글자는 입력을 빼앗는다).
            Some(user) => MenuItem::with_id(app, *id, *label, true, user.as_deref()),
            None => MenuItem::with_id(app, *id, *label, true, *accel),
        })
        .collect()
}

/// 메뉴 항목 ↔ 사용자 단축키 동작 (SET-12).
fn shortcut_action(menu_id: &str) -> Option<ShortcutAction> {
    match menu_id {
        "screenshot" => Some(ShortcutAction::Screenshot),
        "bookmark" => Some(ShortcutAction::BookmarkStream),
        "fullscreen" => Some(ShortcutAction::ToggleFullscreen),
        _ => None,
    }
}

/// 사용자 바인딩이 걸린 메뉴 항목이면 `Some(가속기)`(수식 키가 없으면 `Some(None)`), 아니면 `None`.
fn user_accelerator(keyboard: &KeyboardShortcutSettings, menu_id: &str) -> Option<Option<String>> {
    let action = shortcut_action(menu_id)?;
    Some(accelerator(&keyboard.binding(action)))
}

/// `KeyBinding` → Tauri 가속기 문자열. 수식 키가 없거나 키가 비면 `None`. 순수 함수.
pub fn accelerator(b: &KeyBinding) -> Option<String> {
    if b.modifiers == ShortcutModifiers::empty() || b.key.is_empty() {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    let cmd = b.modifiers.contains(ShortcutModifiers::COMMAND);
    let ctrl = b.modifiers.contains(ShortcutModifiers::CONTROL);
    let mut alt = b.modifiers.contains(ShortcutModifiers::OPTION);
    let mut shift = b.modifiers.contains(ShortcutModifiers::SHIFT);
    // Windows 에는 ⌘·⌃ 가 따로 없다 — ⌃⌘ 는 화면 규칙과 같게 Ctrl+Shift+Alt 로.
    if cfg!(not(target_os = "macos")) && cmd && ctrl {
        parts.push("Ctrl".into());
        alt = true;
        shift = true;
    } else {
        if cmd {
            parts.push("CmdOrCtrl".into());
        }
        if ctrl {
            parts.push("Ctrl".into());
        }
    }
    if alt {
        parts.push("Alt".into());
    }
    if shift {
        parts.push("Shift".into());
    }
    let key = match b.key.as_str() {
        "space" => "Space".to_string(),
        "upArrow" => "Up".into(),
        "downArrow" => "Down".into(),
        "leftArrow" => "Left".into(),
        "rightArrow" => "Right".into(),
        "return" => "Enter".into(),
        "escape" => "Escape".into(),
        "tab" => "Tab".into(),
        "delete" => "Backspace".into(),
        k => k.to_uppercase(),
    };
    parts.push(key);
    Some(parts.join("+"))
}

/// 단축키 설정이 바뀌면 메뉴를 다시 만든다.
pub fn rebuild<R: Runtime>(app: &AppHandle<R>) {
    match build(app) {
        Ok(menu) => {
            if let Err(e) = app.set_menu(menu) {
                tracing::warn!("메뉴 갱신 실패: {e}");
            }
            // 새 메뉴를 달면 창마다 메뉴 막대가 다시 보인다 — Windows 는 다시 숨긴다.
            hide_menu_bars(app);
        }
        Err(e) => tracing::warn!("메뉴 만들기 실패: {e}"),
    }
}

/// Windows — 열린 창의 메뉴 막대를 모두 숨긴다. 단축키(가속기 표)는 메뉴 막대와 따로 살아 있고,
/// 메뉴는 화면 오른쪽 위의 메뉴 버튼이 [`popup`] 으로 띄운다. macOS 는 화면 위 메뉴 막대라 그대로 둔다.
pub fn hide_menu_bars<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(windows)]
    for window in app.webview_windows().values() {
        let _ = window.hide_menu();
    }
    #[cfg(not(windows))]
    let _ = app;
}

/// 앱 메뉴를 창 안 (x, y)(논리 좌표)에 띄운다 — Windows 화면의 메뉴 버튼. 고른 항목은 메뉴 막대와 같은
/// 경로([`forward`])로 화면에 간다.
#[tauri::command]
pub fn window_menu_popup<R: Runtime>(
    window: tauri::WebviewWindow<R>,
    x: f64,
    y: f64,
) -> crate::commands::CmdResult<()> {
    let menu = window
        .app_handle()
        .menu()
        .ok_or_else(|| crate::commands::CommandError("메뉴가 없습니다".into()))?;
    window
        .popup_menu_at(&menu, tauri::LogicalPosition::new(x, y))
        .map_err(|e| crate::commands::CommandError(e.to_string()))
}

/// 메뉴를 만든다.
pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let about = MenuItem::with_id(app, "about", "CView 정보", true, None::<&str>)?;
    let check_update =
        MenuItem::with_id(app, "check-update", "업데이트 확인…", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "설정…", true, Some("CmdOrCtrl+,"))?;
    let notification_settings = MenuItem::with_id(
        app,
        "notification-settings",
        "알림 설정…",
        true,
        None::<&str>,
    )?;
    let app_menu = Submenu::with_items(
        app,
        "CView",
        true,
        &[
            &about,
            &check_update,
            &PredefinedMenuItem::separator(app)?,
            &settings,
            &notification_settings,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, Some("CView 가리기"))?,
            &PredefinedMenuItem::hide_others(app, Some("기타 가리기"))?,
            &PredefinedMenuItem::show_all(app, Some("모두 보기"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, Some("CView 종료"))?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "편집",
        true,
        &[
            &PredefinedMenuItem::undo(app, Some("실행 취소"))?,
            &PredefinedMenuItem::redo(app, Some("다시 실행"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, Some("오려두기"))?,
            &PredefinedMenuItem::copy(app, Some("복사"))?,
            &PredefinedMenuItem::paste(app, Some("붙여넣기"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::select_all(app, Some("모두 선택"))?,
        ],
    )?;
    // `head` 는 목록 앞에 붙는 기본 항목(윈도우 메뉴의 최소화 등). 둘 다 있으면 사이에 구분선.
    let sub = |title: &str,
               head: Vec<Box<dyn tauri::menu::IsMenuItem<R>>>,
               list: &[Item]|
     -> tauri::Result<Submenu<R>> {
        let mut all = head;
        if !all.is_empty() {
            all.push(Box::new(PredefinedMenuItem::separator(app)?));
        }
        all.extend(entries(app, list)?);
        let refs: Vec<&dyn tauri::menu::IsMenuItem<R>> = all.iter().map(|i| i.as_ref()).collect();
        Submenu::with_items(app, title, true, &refs)
    };
    let view = sub("보기", Vec::new(), VIEW)?;
    let go = sub("이동", Vec::new(), GO)?;
    let playback = sub("재생", Vec::new(), PLAYBACK)?;
    let chat = sub("채팅", Vec::new(), CHAT)?;
    let window = sub(
        "윈도우",
        vec![
            Box::new(PredefinedMenuItem::minimize(app, Some("최소화"))?),
            Box::new(PredefinedMenuItem::maximize(app, Some("확대/축소"))?),
            Box::new(PredefinedMenuItem::fullscreen(app, Some("전체 화면"))?),
        ],
        WINDOWS,
    )?;
    let help = sub("도움말", Vec::new(), HELP)?;
    Menu::with_items(
        app,
        &[
            &app_menu, &edit, &view, &go, &playback, &chat, &window, &help,
        ],
    )
}

/// 메뉴 이벤트 → 화면. 알려진 ID 만 넘긴다.
pub fn forward<R: Runtime>(app: &AppHandle<R>, id: &str) {
    if is_known(id) {
        let _ = app.emit(MENU_ACTION, serde_json::json!({ "id": id }));
    }
}

/// 화면으로 넘기는 항목이 든 메뉴 목록 전부.
const ALL: [&[Item]; 6] = [VIEW, GO, PLAYBACK, CHAT, WINDOWS, HELP];

pub fn is_known(id: &str) -> bool {
    !id.is_empty()
        && (APP_ITEMS.contains(&id) || ALL.iter().any(|list| list.iter().any(|(i, _, _)| *i == id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_known() {
        let mut seen = std::collections::HashSet::new();
        for list in ALL {
            for (id, label, _) in list.iter().filter(|(id, _, _)| !id.is_empty()) {
                assert!(seen.insert(*id), "중복 ID {id}");
                assert!(!label.is_empty());
                assert!(is_known(id));
            }
        }
        assert!(is_known("settings"));
        assert!(is_known("check-update"));
        assert!(is_known("notification-settings"));
        assert!(!is_known("rm -rf"));
        assert!(!is_known(""));
    }

    #[test]
    fn separators_only_between_groups() {
        for list in ALL {
            assert!(!list.first().unwrap().0.is_empty(), "맨 앞 구분선");
            assert!(!list.last().unwrap().0.is_empty(), "맨 끝 구분선");
            assert!(
                !list
                    .windows(2)
                    .any(|w| w[0].0.is_empty() && w[1].0.is_empty()),
                "구분선 연속"
            );
        }
    }

    #[test]
    fn shortcuts_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for (_, _, accel) in ALL.iter().flat_map(|l| l.iter()) {
            if let Some(a) = accel {
                assert!(seen.insert(*a), "중복 단축키 {a}");
            }
        }
    }
}

#[cfg(test)]
mod accel_tests {
    use super::*;

    #[test]
    fn bindings_to_accelerators() {
        let b = KeyBinding::new("s", ShortcutModifiers::COMMAND);
        assert_eq!(accelerator(&b).as_deref(), Some("CmdOrCtrl+S"));
        let b = KeyBinding::new(
            "upArrow",
            ShortcutModifiers::COMMAND | ShortcutModifiers::SHIFT,
        );
        assert_eq!(accelerator(&b).as_deref(), Some("CmdOrCtrl+Shift+Up"));
        assert_eq!(
            accelerator(&KeyBinding::new("f", ShortcutModifiers::empty())),
            None
        );
    }
}
