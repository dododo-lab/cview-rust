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
    SEP,
    // 화면 크기(설정 외관 → 화면 크기) — 4K 1배율·Retina 에서 글자·단추 크기를 맞춘다.
    ("zoom-in", "확대", Some("CmdOrCtrl+=")),
    ("zoom-out", "축소", Some("CmdOrCtrl+-")),
    ("zoom-reset", "실제 크기", Some("CmdOrCtrl+0")),
];

/// 이동 — 페이지 이동, 뒤로, 찾아 들어가기(검색·명령 팔레트).
/// ⌘1~7 은 사이드바 순서 그대로(app.js ROUTES), 사이드바에 없는 멀티라이브는 ⌘8(2026-11-23 감사 UX-H04).
const GO: &[Item] = &[
    ("nav-home", "홈", Some("CmdOrCtrl+1")),
    ("nav-following", "팔로잉", Some("CmdOrCtrl+2")),
    // 메뉴 제목의 `&` 하나는 니모닉(muda) — `&&` 로 써야 "&" 로 보인다.
    ("nav-popular", "방송&&클립", Some("CmdOrCtrl+3")),
    ("nav-categories", "카테고리", Some("CmdOrCtrl+4")),
    ("nav-live", "라이브", Some("CmdOrCtrl+5")),
    ("nav-multichat", "멀티채팅", Some("CmdOrCtrl+6")),
    ("nav-library", "보관함", Some("CmdOrCtrl+7")),
    ("nav-multilive", "멀티라이브", Some("CmdOrCtrl+8")),
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

/// 문의 메일 주소 — 도움말 › 문의하기, 알림 영역 메뉴, 화면의 정보 시트(notice.js `CONTACT_EMAIL`)와 같다.
pub const CONTACT_EMAIL: &str = "do@dododo.app";
/// 공개 저장소 — 소스 코드(GPL-3.0-only).
pub const REPO_URL: &str = "https://github.com/dododo-lab/cview-rust";

/// 도움말 — 사용법 안내 / 프로젝트(문의·저장소·라이선스) / 문제 해결.
/// 화면(app.js MENU_ACTIONS)이 같은 id 로 연다 — `contact` 는 mailto, `github-repo` 는 브라우저, 나머지는 시트.
const HELP: &[Item] = &[
    ("shortcuts", "키보드 단축키", Some("CmdOrCtrl+/")),
    ("welcome", "처음 안내 다시 보기", None),
    ("notice", "이용 고지 다시 보기…", None),
    SEP,
    ("contact", "문의하기 (do@dododo.app)", None),
    ("github-repo", "GitHub 저장소", None),
    ("licenses", "오픈소스 라이선스…", None),
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
        .map(|(id, label, accel)| {
            let accel = menu_accelerator(user_accelerator(&keyboard, id), *accel);
            MenuItem::with_id(app, *id, *label, true, accel.as_deref())
        })
        .collect()
}

/// 메뉴 항목 ↔ 사용자 단축키 동작 (SET-12).
fn shortcut_action(menu_id: &str) -> Option<ShortcutAction> {
    match menu_id {
        "screenshot" => Some(ShortcutAction::Screenshot),
        "bookmark" => Some(ShortcutAction::BookmarkStream),
        "fullscreen" => Some(ShortcutAction::ToggleFullscreen),
        "pip" => Some(ShortcutAction::TogglePiP),
        _ => None,
    }
}

/// 사용자 바인딩이 걸린 메뉴 항목이면 `Some(가속기)`(수식 키가 없으면 `Some(None)`), 아니면 `None`.
fn user_accelerator(keyboard: &KeyboardShortcutSettings, menu_id: &str) -> Option<Option<String>> {
    let action = shortcut_action(menu_id)?;
    Some(accelerator(&keyboard.binding(action)))
}

/// 메뉴 항목에 달 가속기 — 수식 키가 있는 사용자 단축키가 있으면 그것, 아니면 메뉴 기본값. 순수 함수.
///
/// 사용자 단축키가 맨 글자(기본 전체 화면 `F`·PiP `P`)면 메뉴에는 달지 않는다(입력을 빼앗는다). 그때 메뉴 기본값
/// (⇧⌘F·⌥⌘P)까지 없애면 그 단축키가 통째로 사라졌다(감사 UX-C12) — 맨 글자는 시청 화면이 받고 메뉴는 기본값을 둔다.
fn menu_accelerator(user: Option<Option<String>>, default: Option<&str>) -> Option<String> {
    match user {
        Some(Some(user)) => Some(user),
        _ => default.map(str::to_owned),
    }
}

/// `KeyBinding` → Tauri 가속기 문자열. 수식 키가 없거나 ⇧ 만 있거나 키가 비면 `None`. 순수 함수.
///
/// ⇧ 하나뿐인 바인딩(예: ⇧F)은 맨 글자처럼 시청 화면이 받는다(shortcuts.js matchPlainAction). 메뉴 가속기로 달면
/// 채팅 입력·검색란에서 대문자 F 를 칠 때마다 메뉴가 가로챘다.
pub fn accelerator(b: &KeyBinding) -> Option<String> {
    if b.modifiers == ShortcutModifiers::empty()
        || b.modifiers == ShortcutModifiers::SHIFT
        || b.key.is_empty()
    {
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
    // 화면 좌표는 CSS 픽셀이다 — '화면 크기'(웹뷰 확대, apply_saved_zoom)를 100% 가 아니게 두면 논리 좌표와 배율만큼 달라,
    // 예전엔 125% 에서 메뉴가 단추보다 한참 왼쪽·위에 떴다.
    let scale = tauri::Manager::try_state::<crate::state::AppServices>(window.app_handle())
        .map(|s| s.settings.appearance().ui_scale)
        .unwrap_or(1.0);
    let (lx, ly) = css_to_logical(x, y, scale);
    window
        .popup_menu_at(&menu, tauri::LogicalPosition::new(lx, ly))
        .map_err(|e| crate::commands::CommandError(e.to_string()))
}

/// 웹뷰 확대(`scale`) 아래의 CSS 좌표 → 창 논리 좌표. 배율이 이상하면(0·NaN) 1 로 본다. 순수 함수.
pub fn css_to_logical(x: f64, y: f64, scale: f64) -> (f64, f64) {
    let s = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    (x * s, y * s)
}

/// 가리기·기타 가리기·모두 보기·전체 화면 기본 항목을 넣는가 — macOS 에서만(Windows muda 는 가리기를 SW_HIDE 로,
/// 나머지는 아무 일도 하지 않는다). 순수 함수.
pub fn app_menu_has_hide_items(macos: bool) -> bool {
    macos
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
    let macos = cfg!(target_os = "macos");
    let mut app_items: Vec<Box<dyn tauri::menu::IsMenuItem<R>>> = vec![
        Box::new(about),
        Box::new(check_update),
        Box::new(PredefinedMenuItem::separator(app)?),
        Box::new(settings),
        Box::new(notification_settings),
        Box::new(PredefinedMenuItem::separator(app)?),
    ];
    // 가리기·기타 가리기·모두 보기는 macOS 에만 — Windows 에서 '가리기'(⌃H)는 창을 SW_HIDE 로 숨기면서 앱의 숨김 상태
    // (tray::hide_main)를 거치지 않아, 알림 영역 아이콘을 꺼 두었으면 되돌릴 길이 없었다. 나머지 둘은 Windows 에서 아무 일도 하지 않았다.
    if app_menu_has_hide_items(macos) {
        app_items.push(Box::new(PredefinedMenuItem::hide(
            app,
            Some("CView 가리기"),
        )?));
        app_items.push(Box::new(PredefinedMenuItem::hide_others(
            app,
            Some("기타 가리기"),
        )?));
        app_items.push(Box::new(PredefinedMenuItem::show_all(
            app,
            Some("모두 보기"),
        )?));
        app_items.push(Box::new(PredefinedMenuItem::separator(app)?));
    }
    app_items.push(Box::new(PredefinedMenuItem::quit(app, Some("CView 종료"))?));
    let app_refs: Vec<&dyn tauri::menu::IsMenuItem<R>> =
        app_items.iter().map(|i| i.as_ref()).collect();
    let app_menu = Submenu::with_items(app, "CView", true, &app_refs)?;
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
        {
            let mut items: Vec<Box<dyn tauri::menu::IsMenuItem<R>>> = vec![
                Box::new(PredefinedMenuItem::minimize(app, Some("최소화"))?),
                Box::new(PredefinedMenuItem::maximize(app, Some("확대/축소"))?),
            ];
            // '전체 화면'(기본 항목)은 Windows 에서 muda 가 처리하지 않아 눌러도 아무 일도 없었다.
            if app_menu_has_hide_items(macos) {
                items.push(Box::new(PredefinedMenuItem::fullscreen(
                    app,
                    Some("전체 화면"),
                )?));
            }
            items
        },
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
    if !is_known(id) {
        return;
    }
    // 화면은 앞에 있는(초점 있는) 창만 메뉴 동작을 따른다. 메인 창을 내리거나 트레이로 숨긴 채 ⌘Tab 으로 CView 를
    // 앞으로 가져오면 초점 있는 창이 없어 CView › 설정…·정보·업데이트 확인이 아무 일도 안 했다 — 메인 창을 보이고 그리로.
    let any_focused = app.webview_windows().iter().any(|(label, w)| {
        label != crate::tray_popover::TRAY_POPOVER_WINDOW && w.is_focused().unwrap_or(false)
    });
    if needs_main_window(id, any_focused) {
        crate::tray::show_main(app);
        crate::tray_popover::send_main_control(
            app,
            serde_json::json!({ "kind": "menu", "id": id }),
        );
        return;
    }
    let _ = app.emit(MENU_ACTION, serde_json::json!({ "id": id }));
}

/// 초점 있는 CView 창이 없을 때 메인 창으로 보내 여는 앱 메뉴 항목인가. 순수 함수.
fn needs_main_window(id: &str, any_focused: bool) -> bool {
    !any_focused && APP_ITEMS.contains(&id)
}

/// 화면으로 넘기는 항목이 든 메뉴 목록 전부.
const ALL: [&[Item]; 6] = [VIEW, GO, PLAYBACK, CHAT, WINDOWS, HELP];

pub fn is_known(id: &str) -> bool {
    !id.is_empty()
        && (APP_ITEMS.contains(&id) || ALL.iter().any(|list| list.iter().any(|(i, _, _)| *i == id)))
}

#[cfg(test)]
mod tests {

    #[test]
    fn app_menu_items_go_to_the_main_window_when_no_window_has_focus() {
        for id in ["about", "check-update", "settings", "notification-settings"] {
            assert!(super::needs_main_window(id, false), "{id}");
            assert!(!super::needs_main_window(id, true), "{id}");
        }
        // 재생·채팅처럼 앞 창이 대상인 항목은 그대로(초점 없으면 아무 창도 하지 않는다)
        assert!(!super::needs_main_window("play-pause", false));
        assert!(!super::needs_main_window("close-viewer", false));
    }

    #[test]
    fn popup_position_follows_the_web_view_zoom() {
        assert_eq!(super::css_to_logical(100.0, 40.0, 1.25), (125.0, 50.0));
        assert_eq!(super::css_to_logical(100.0, 40.0, 0.9), (90.0, 36.0));
        assert_eq!(super::css_to_logical(100.0, 40.0, 0.0), (100.0, 40.0));
        assert_eq!(super::css_to_logical(100.0, 40.0, f64::NAN), (100.0, 40.0));
    }

    #[test]
    fn hide_and_fullscreen_predefined_items_are_macos_only() {
        assert!(super::app_menu_has_hide_items(true));
        assert!(!super::app_menu_has_hide_items(false));
    }
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
mod help_tests {
    use super::*;

    fn help_ids() -> Vec<&'static str> {
        HELP.iter().map(|(id, _, _)| *id).collect()
    }

    #[test]
    fn help_menu_has_project_items() {
        let ids = help_ids();
        for id in ["contact", "github-repo", "licenses", "notice"] {
            assert!(ids.contains(&id), "도움말에 {id} 없음");
            assert!(is_known(id), "{id} 가 화면으로 가지 않음");
        }
        // 문의·저장소·라이선스는 한 묶음(앞뒤 구분선) — 문제 보고 묶음과 섞이지 않는다.
        let at = |id: &str| ids.iter().position(|x| *x == id).unwrap();
        assert_eq!(ids[at("contact") - 1], "");
        assert_eq!(at("github-repo"), at("contact") + 1);
        assert_eq!(at("licenses"), at("contact") + 2);
        assert_eq!(ids[at("licenses") + 1], "");
        // 고지 다시 보기는 사용법 안내 묶음에.
        assert_eq!(at("notice"), at("welcome") + 1);
    }

    #[test]
    fn contact_label_shows_the_address() {
        let label = HELP.iter().find(|(id, _, _)| *id == "contact").unwrap().1;
        assert!(label.contains(CONTACT_EMAIL), "{label}");
        assert!(label.starts_with("문의하기"));
        // 시트를 여는 항목만 '…'(macOS 관례) — 메일 앱·브라우저로 바로 가는 항목은 붙이지 않는다.
        for (id, label, _) in HELP {
            let opens_sheet = matches!(*id, "notice" | "licenses" | "report-issue");
            if !id.is_empty() && matches!(*id, "contact" | "github-repo" | "notice" | "licenses") {
                assert_eq!(label.ends_with('…'), opens_sheet, "{id}: {label}");
            }
        }
        assert_eq!(REPO_URL, "https://github.com/dododo-lab/cview-rust");
    }
}

#[cfg(test)]
mod accel_tests {
    use super::*;

    #[test]
    fn plain_user_binding_keeps_menu_default() {
        // 기본 전체 화면(맨 글자 F) — 메뉴의 ⇧⌘F 는 남는다.
        assert_eq!(
            menu_accelerator(Some(None), Some("CmdOrCtrl+Shift+F")).as_deref(),
            Some("CmdOrCtrl+Shift+F")
        );
        // 수식 키가 있는 사용자 단축키는 기본값을 대신한다.
        assert_eq!(
            menu_accelerator(
                Some(Some("Alt+CmdOrCtrl+F".into())),
                Some("CmdOrCtrl+Shift+F")
            )
            .as_deref(),
            Some("Alt+CmdOrCtrl+F")
        );
        // 사용자 단축키가 없는 항목은 기본값.
        assert_eq!(
            menu_accelerator(None, Some("CmdOrCtrl+W")).as_deref(),
            Some("CmdOrCtrl+W")
        );
        assert_eq!(menu_accelerator(None, None), None);
    }

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
        // ⇧ 만 있는 바인딩은 메뉴가 아니라 시청 화면이 받는다 — 메뉴 기본값이 남는다.
        assert_eq!(
            accelerator(&KeyBinding::new("f", ShortcutModifiers::SHIFT)),
            None
        );
        assert_eq!(
            menu_accelerator(
                Some(accelerator(&KeyBinding::new("f", ShortcutModifiers::SHIFT))),
                Some("CmdOrCtrl+Shift+F")
            )
            .as_deref(),
            Some("CmdOrCtrl+Shift+F")
        );
    }
}
