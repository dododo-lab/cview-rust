//! 앱 메뉴 막대 — 원본 `CViewApp` 의 CommandMenu(재생·채팅·보기·창·도움말) (SYS-03).
//!
//! macOS 에서 메뉴 단축키는 웹뷰보다 먼저 키를 가져가므로, 공용 단축키(⌘1~7, ⌘R, ⌘S, ⌘, …)는 메뉴에
//! 두고 고르면 `menu://action` `{ id }` 로 화면에 넘긴다. 화면(app.js)이 같은 동작을 실행한다.
//! 편집 메뉴(복사·붙여넣기)는 기본 항목으로 둬야 입력창의 ⌘C/⌘V 가 산다.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Runtime};

/// 화면으로 보내는 메뉴 이벤트 이름.
pub const MENU_ACTION: &str = "menu://action";

/// (id, 표시 이름, 단축키) — 단축키는 Tauri 가속기 문법.
type Item = (&'static str, &'static str, Option<&'static str>);

const VIEW: &[Item] = &[
    ("nav-home", "홈", Some("CmdOrCtrl+1")),
    ("nav-following", "팔로잉", Some("CmdOrCtrl+2")),
    ("nav-popular", "인기", Some("CmdOrCtrl+3")),
    ("nav-multilive", "멀티라이브", Some("CmdOrCtrl+4")),
    ("nav-live", "라이브", Some("CmdOrCtrl+5")),
    ("nav-library", "보관함", Some("CmdOrCtrl+6")),
    ("nav-categories", "카테고리", Some("CmdOrCtrl+7")),
    ("search", "검색", Some("CmdOrCtrl+F")),
    ("palette", "명령 팔레트", Some("CmdOrCtrl+K")),
    ("back", "뒤로", Some("CmdOrCtrl+[")),
    ("refresh", "새로고침", Some("CmdOrCtrl+R")),
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

const PLAYBACK: &[Item] = &[
    ("play-pause", "재생/일시정지", None),
    ("mute", "음소거", None),
    ("go-live", "라이브로 이동", None),
    ("screenshot", "스크린샷", Some("CmdOrCtrl+S")),
    ("bookmark", "북마크", Some("CmdOrCtrl+Shift+B")),
    ("fullscreen", "영상 전체화면", Some("CmdOrCtrl+Shift+F")),
    (
        "fullscreen-chat",
        "영상+채팅 전체화면",
        Some("CmdOrCtrl+Ctrl+F"),
    ),
    ("close-viewer", "시청 화면 닫기", Some("CmdOrCtrl+W")),
];

const CHAT: &[Item] = &[
    ("chat-clear", "채팅 지우기", Some("CmdOrCtrl+Shift+K")),
    (
        "chat-autoscroll",
        "자동 스크롤 켜기/끄기",
        Some("CmdOrCtrl+J"),
    ),
    ("chat-reconnect", "채팅 다시 연결", None),
];

const HELP: &[Item] = &[
    ("shortcuts", "키보드 단축키", Some("CmdOrCtrl+/")),
    ("report-issue", "문제 보고…", None),
    ("open-logs", "데이터 폴더 열기", None),
];

fn items<R: Runtime>(app: &AppHandle<R>, list: &[Item]) -> tauri::Result<Vec<MenuItem<R>>> {
    list.iter()
        .map(|(id, label, accel)| MenuItem::with_id(app, *id, *label, true, *accel))
        .collect()
}

/// 메뉴를 만든다.
pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let about = MenuItem::with_id(app, "about", "CView 정보", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "설정…", true, Some("CmdOrCtrl+,"))?;
    let app_menu = Submenu::with_items(
        app,
        "CView",
        true,
        &[
            &about,
            &PredefinedMenuItem::separator(app)?,
            &settings,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, Some("CView 가리기"))?,
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
            &PredefinedMenuItem::select_all(app, Some("모두 선택"))?,
        ],
    )?;
    let sub = |title: &str, list: &[Item]| -> tauri::Result<Submenu<R>> {
        let built = items(app, list)?;
        let refs: Vec<&dyn tauri::menu::IsMenuItem<R>> = built
            .iter()
            .map(|i| i as &dyn tauri::menu::IsMenuItem<R>)
            .collect();
        Submenu::with_items(app, title, true, &refs)
    };
    let view = sub("보기", VIEW)?;
    let playback = sub("재생", PLAYBACK)?;
    let chat = sub("채팅", CHAT)?;
    let window = Submenu::with_items(
        app,
        "윈도우",
        true,
        &[
            &PredefinedMenuItem::minimize(app, Some("최소화"))?,
            &PredefinedMenuItem::maximize(app, Some("확대/축소"))?,
            &PredefinedMenuItem::fullscreen(app, Some("전체 화면"))?,
        ],
    )?;
    let help = sub("도움말", HELP)?;
    Menu::with_items(
        app,
        &[&app_menu, &edit, &view, &playback, &chat, &window, &help],
    )
}

/// 메뉴 이벤트 → 화면. 알려진 ID 만 넘긴다.
pub fn forward<R: Runtime>(app: &AppHandle<R>, id: &str) {
    if is_known(id) {
        let _ = app.emit(MENU_ACTION, serde_json::json!({ "id": id }));
    }
}

pub fn is_known(id: &str) -> bool {
    matches!(id, "about" | "settings")
        || [VIEW, PLAYBACK, CHAT, HELP]
            .iter()
            .any(|list| list.iter().any(|(i, _, _)| *i == id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_known() {
        let mut seen = std::collections::HashSet::new();
        for list in [VIEW, PLAYBACK, CHAT, HELP] {
            for (id, label, _) in list {
                assert!(seen.insert(*id), "중복 ID {id}");
                assert!(!label.is_empty());
                assert!(is_known(id));
            }
        }
        assert!(is_known("settings"));
        assert!(!is_known("rm -rf"));
    }
}
