//! 메뉴 막대(macOS)·알림 영역(Windows) 아이콘 — 원본 MenuBarView(방송 중 팔로잉 목록·열기·종료) (SYS-20),
//! AppIconRenderer 의 라이브 수 배지(SYS-23, Dock 배지 숫자로 근사), 그리고 창을 여기로 숨기기(설정 › 일반).
//!
//! - 아이콘: 왼쪽 클릭은 메인 창 열기, 오른쪽 클릭(macOS 는 ⌃클릭도)은 메뉴. macOS 는 단색 템플릿 이미지라
//!   메뉴 막대 외형(밝음·어두움)에 맞춰 시스템이 색을 입힌다. Windows 는 컬러 아이콘을 화면 배율에 맞는 크기
//!   (16~48px)로 골라 흐리지 않게 한다. 방송 중인 팔로잉이 있으면 오른쪽 위에 점이 붙는다(원본 app-icons/tray-*.svg).
//! - 메뉴: 지금 시청 중(누르면 창)·음소거 / 방송 중 즐겨찾기 / 방송 중 팔로잉(넘치면 "더 보기") / 창 열기·숨기기·
//!   멀티라이브·설정·업데이트 확인 / 종료. 모양은 순수 함수 [`menu_spec`] 가 정하고, 바뀌었을 때만 OS 메뉴를 다시 만든다.
//! - 숨기기: 설정에서 켜면 메인 창 닫기·최소화가 종료·Dock 대신 창을 숨긴다(macOS 는 Dock 아이콘도 숨길 수 있다).
//!   창만 숨기므로 웹 화면은 그대로 돌아 재생·채팅·알림이 이어진다 — 가려진 페이지 타이머는 플레이어가 Worker 로
//!   돌려 라이브 지연도 웹과 같게 유지한다(#163, app-ui/player/bg-timers.js). 숨겨 있는 동안에는 "메뉴 막대에 표시"를
//!   꺼 두었어도 아이콘을 보인다(다시 열 길이 없어지지 않게).
//! - 1분마다 팔로잉 결과(화면 진입 폴 — 60초 안이면 캐시)와 즐겨찾기를 읽어 메뉴를 다시 만든다. 항목을 고르면 딥 링크와
//!   같은 경로로 화면에 넘긴다.
//!
//! 잠금 규칙: 창·아이콘 조회와 OS 호출은 메인 스레드를 기다릴 수 있다. 메인 스레드(창 이벤트)도 [`sync`] 를 부르므로
//! 잠금을 쥔 채로는 그런 호출을 하지 않는다(교착 방지).

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use cview_app_core::caching::following_store::to_live_channel_item;
use cview_core::models::settings::GeneralSettings;
use cview_core::utils::following_poll::FollowingPollReason;
use serde::{Deserialize, Serialize};
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime, WebviewWindow, WindowEvent};

use crate::state::AppServices;

const TRAY_ID: &str = "cview-tray";
const LIVE_PREFIX: &str = "tray-live:";
/// 맨 위 단계에 바로 보이는 방송 수 — 즐겨찾기·팔로잉 각각. 나머지 팔로잉은 "더 보기" 하위 메뉴로.
const MAX_FAVORITES: usize = 8;
const MAX_INLINE: usize = 8;
const MAX_INLINE_WITH_FAVORITES: usize = 5;
/// "더 보기" 안의 최대 수 — OS 메뉴를 너무 길게 만들지 않는다.
const MAX_MORE: usize = 30;
/// 메뉴 한 줄 글자 수 상한(채널명·제목·카테고리) — 긴 방송 제목이 메뉴를 화면 너비만큼 늘이지 않게.
const NAME_CHARS: usize = 18;
const CATEGORY_CHARS: usize = 16;
const TITLE_CHARS: usize = 28;
/// macOS 템플릿 아이콘 크기(px) — tray-icon 이 높이 18pt 로 그리므로 Retina 에서 1:1.
const TEMPLATE_PX: u32 = 36;

/// "…로 숨기기" — 상태 아이콘 자리 이름(화면의 `TRAY_TO`, platform.js)에 조사까지.
#[cfg(target_os = "macos")]
const HIDE_LABEL: &str = "메뉴 막대로 숨기기";
#[cfg(not(target_os = "macos"))]
const HIDE_LABEL: &str = "알림 영역으로 숨기기";

/// 창을 이 아이콘으로 숨긴 상태(설정의 닫기·최소화, 또는 메뉴의 "숨기기"). 다시 열면 지운다.
static HIDDEN: AtomicBool = AtomicBool::new(false);
/// 메뉴의 "종료"를 고른 뒤 — 창 닫기 요청이 와도 숨기지 않고 닫히게 둔다.
static QUITTING: AtomicBool = AtomicBool::new(false);

// ── 모델(순수) ─────────────────────────────────────

/// 화면이 알려 주는 상태 — 지금 시청 중·음소거·재생·볼륨·PiP(app-ui/js/tray-state.js).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TrayFrontState {
    pub now_playing: Option<NowPlaying>,
    /// 음소거할 대상(단독 보기·시청 화면)이 있으면 그 상태, 없으면 `None`(메뉴에 음소거를 두지 않는다).
    pub muted: Option<bool>,
    /// 일시정지 여부(제어 가능한 재생 대상이 있을 때만 `Some`).
    pub paused: Option<bool>,
    /// 현재 볼륨 0..=100(조절 가능한 대상이 있을 때만 `Some`).
    pub volume: Option<u8>,
    /// 화면 속 화면(PiP) 상태(지원·재생 중일 때만 `Some`).
    pub pip: Option<bool>,
}

/// 시스템 '지금 재생 중'에 올린 것과 같은 정보(now-playing.js).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NowPlaying {
    /// `live` | `vod`
    pub kind: String,
    pub title: String,
    pub channel_name: String,
    pub channel_id: String,
    pub category: String,
    pub thumbnail_url: String,
    pub image_url: String,
}

/// 방송 중인 팔로잉 한 채널.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveEntry {
    pub channel_id: String,
    pub name: String,
    pub category: Option<String>,
    pub viewers: i64,
    pub favorite: bool,
}

/// 메뉴를 그리는 데 드는 모든 것.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrayModel {
    pub logged_in: bool,
    /// 시청자 많은 순.
    pub lives: Vec<LiveEntry>,
    pub front: TrayFrontState,
    /// 메인 창이 지금 안 보인다(숨김·최소화) — "숨기기" 자리가 "열기"가 된다.
    pub window_hidden: bool,
}

/// 메뉴 항목 한 줄.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// 누를 수 없는 머리말.
    Label(String),
    Item {
        id: String,
        label: String,
    },
    Check {
        id: String,
        label: String,
        checked: bool,
    },
    Submenu {
        label: String,
        items: Vec<Entry>,
    },
    Separator,
}

fn item(id: impl Into<String>, label: impl Into<String>) -> Entry {
    Entry::Item {
        id: id.into(),
        label: label.into(),
    }
}

/// 공백을 하나로 모으고 글자 수(문자 단위)로 잘라 말줄임표를 붙인다. 순수 함수.
pub fn truncate(text: &str, max: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= max {
        return text;
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.truncate(out.trim_end().len());
    out.push('…');
    out
}

/// 방송 한 줄 — "이름 — 카테고리 · 시청자". 순수 함수.
pub fn live_label(name: &str, category: Option<&str>, viewers: i64) -> String {
    let viewers = cview_core::models::live_channel_item::format_viewer_count(viewers);
    let name = truncate(name, NAME_CHARS);
    match category.map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) => format!("{name} — {} · {viewers}", truncate(c, CATEGORY_CHARS)),
        None => format!("{name} · {viewers}"),
    }
}

/// 지금 시청 중 한 줄 — "채널 — 제목"(다시보기는 앞에 표시). 순수 함수.
pub fn now_playing_label(now: &NowPlaying) -> String {
    let channel = truncate(&now.channel_name, NAME_CHARS);
    let title = truncate(&now.title, TITLE_CHARS);
    let body = if !channel.is_empty() && !title.is_empty() && title != channel {
        format!("{channel} — {title}")
    } else if !channel.is_empty() {
        channel
    } else if !title.is_empty() {
        title
    } else {
        "CView".to_string()
    };
    if now.kind == "vod" {
        format!("다시보기 · {body}")
    } else {
        body
    }
}

fn live_item(l: &LiveEntry) -> Entry {
    item(
        format!("{LIVE_PREFIX}{}", l.channel_id),
        live_label(&l.name, l.category.as_deref(), l.viewers),
    )
}

/// 메뉴 모양. 순수 함수 — 같은 모델이면 같은 결과라 바뀌었을 때만 OS 메뉴를 다시 만든다.
pub fn menu_spec(m: &TrayModel) -> Vec<Entry> {
    let mut out = Vec::new();

    // 지금 시청 중 — 누르면 창을 연다. 음소거는 대상이 있을 때만.
    if let Some(now) = &m.front.now_playing {
        out.push(Entry::Label("지금 시청 중".into()));
        out.push(item("tray-now", now_playing_label(now)));
        if let Some(muted) = m.front.muted {
            out.push(Entry::Check {
                id: "tray-mute".into(),
                label: "음소거".into(),
                checked: muted,
            });
        }
        out.push(Entry::Separator);
    }

    let (favorites, others): (Vec<&LiveEntry>, Vec<&LiveEntry>) =
        m.lives.iter().partition(|l| l.favorite);
    if !favorites.is_empty() {
        out.push(Entry::Label(format!(
            "즐겨찾기 · 방송 중 {}",
            favorites.len()
        )));
        out.extend(favorites.iter().take(MAX_FAVORITES).map(|l| live_item(l)));
        out.push(Entry::Separator);
    }

    if !m.logged_in {
        out.push(Entry::Label("로그인하면 방송 중인 팔로잉이 보여요".into()));
    } else if m.lives.is_empty() {
        out.push(Entry::Label("방송 중인 팔로잉 없음".into()));
    } else if !others.is_empty() {
        let (label, inline) = if favorites.is_empty() {
            (format!("팔로잉 · 방송 중 {}", others.len()), MAX_INLINE)
        } else {
            (
                format!("다른 팔로잉 · 방송 중 {}", others.len()),
                MAX_INLINE_WITH_FAVORITES,
            )
        };
        out.push(Entry::Label(label));
        out.extend(others.iter().take(inline).map(|l| live_item(l)));
        let rest: Vec<Entry> = others
            .iter()
            .skip(inline)
            .take(MAX_MORE)
            .map(|l| live_item(l))
            .collect();
        if !rest.is_empty() {
            out.push(Entry::Submenu {
                label: format!("더 보기 ({})", others.len() - inline),
                items: rest,
            });
        }
    }
    // 즐겨찾기만 방송 중이면 위 묶음 끝의 구분선을 그대로 쓴다.
    if !matches!(out.last(), Some(Entry::Separator)) {
        out.push(Entry::Separator);
    }

    out.push(if m.window_hidden {
        item("tray-toggle", "CView 열기")
    } else {
        item("tray-toggle", HIDE_LABEL)
    });
    out.push(item("tray-multilive", "멀티라이브"));
    out.push(item("tray-settings", "설정…"));
    out.push(item("tray-update", "업데이트 확인…"));
    out.push(Entry::Separator);
    out.push(item("tray-quit", "CView 종료"));
    out
}

/// 아이콘 툴팁. 순수 함수.
pub fn tooltip(m: &TrayModel) -> String {
    match (&m.front.now_playing, m.lives.len()) {
        (Some(now), _) => format!("CView — {} 시청 중", now_playing_label(now)),
        (None, 0) => "CView".to_string(),
        (None, n) => format!("CView — 팔로잉 방송 중 {n}"),
    }
}

/// Windows 알림 영역 아이콘 크기(px) — 화면 배율의 작은 아이콘 크기(16 × 배율)보다 작지 않은 것 중 가장 작은 것.
/// 미리 그린 크기만 쓰고(작은 그림을 늘여 흐리게 하지 않게), 그보다 크면 가장 큰 것을 시스템이 줄인다. 순수 함수.
pub fn color_icon_px(scale: f64) -> u32 {
    const SIZES: [u32; 6] = [16, 20, 24, 32, 40, 48];
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let want = (16.0 * scale - 0.01).ceil() as u32;
    SIZES
        .into_iter()
        .find(|&s| s >= want)
        .unwrap_or(SIZES[SIZES.len() - 1])
}

/// 아이콘을 보일지 — 설정에서 켰거나, 창을 아이콘으로 숨겨 둔 동안(다시 열 길이 있어야 한다). 순수 함수.
pub fn tray_visible(general: &GeneralSettings, hidden_to_tray: bool) -> bool {
    general.show_in_menu_bar || hidden_to_tray
}

/// 창 이벤트가 숨기기로 이어지는가 — 닫기 요청은 설정 "닫으면 숨기기", 최소화는 "최소화하면 숨기기". 순수 함수.
pub fn should_hide(general: &GeneralSettings, close_requested: bool, minimized: bool) -> bool {
    (close_requested && general.close_to_tray) || (minimized && general.minimize_to_tray)
}

// ── 상태 ──────────────────────────────────────────

/// 앱 관리 상태 — 마지막 모델 재료와 마지막으로 그린 것.
#[derive(Default)]
pub struct TrayHub {
    model: Mutex<TrayModel>,
    shown: Mutex<Option<Shown>>,
}

#[derive(Clone, PartialEq)]
struct Shown {
    spec: Vec<Entry>,
    live_icon: bool,
    icon_px: u32,
    tooltip: String,
    count: usize,
    visible: bool,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn general<R: Runtime>(app: &AppHandle<R>) -> GeneralSettings {
    app.try_state::<AppServices>()
        .map(|s| s.settings.general())
        .unwrap_or_default()
}

/// 메인 창이 숨겨 있는가(아이콘으로 숨김·최소화·안 보임).
pub(crate) fn is_main_hidden<R: Runtime>(app: &AppHandle<R>) -> bool {
    main_hidden(app)
}

/// 팝오버 스냅샷용 상태 읽기(로그인 여부·메인 창 숨김·프런트 재생 상태).
/// 로그인 여부는 주기 폴링 캐시 대신 인증 관리자의 실시간 상태를 우선 읽는다.
pub(crate) fn snapshot_state<R: Runtime>(app: &AppHandle<R>) -> (bool, bool, TrayFrontState) {
    let window_hidden = main_hidden(app);
    let live_auth = app
        .try_state::<AppServices>()
        .map(|s| s.auth.is_authenticated());
    let Some(hub) = app.try_state::<TrayHub>() else {
        return (
            live_auth.unwrap_or(false),
            window_hidden,
            TrayFrontState::default(),
        );
    };
    let m = lock(&hub.model);
    (
        live_auth.unwrap_or(m.logged_in),
        window_hidden,
        m.front.clone(),
    )
}

/// 인증 상태가 바뀌었을 때(`auth://changed` · 로그아웃) 트레이 모델과 팝오버를 지체 없이 갱신한다.
pub fn on_auth_changed_generic<R: Runtime>(app: &AppHandle<R>, logged_in: bool) {
    let changed = if let Some(hub) = app.try_state::<TrayHub>() {
        let mut m = lock(&hub.model);
        let prev = m.logged_in;
        m.logged_in = logged_in;
        if !logged_in {
            m.lives.clear();
        }
        prev != logged_in
    } else {
        false
    };
    sync(app);
    if logged_in && changed {
        let handle = app.clone();
        tauri::async_runtime::spawn(async move {
            refresh_generic(&handle).await;
        });
    }
}

/// 앱 종료 요청 — 창 닫기 가로채기(`close_to_tray`)를 건너뛰고 끝낸다.
pub(crate) fn request_quit<R: Runtime>(app: &AppHandle<R>) {
    QUITTING.store(true, Ordering::Relaxed);
    app.exit(0);
}

fn main_hidden<R: Runtime>(app: &AppHandle<R>) -> bool {
    if HIDDEN.load(Ordering::Relaxed) {
        return true;
    }
    match app.get_webview_window(crate::MAIN_WINDOW) {
        Some(w) => !w.is_visible().unwrap_or(true) || w.is_minimized().unwrap_or(false),
        None => true,
    }
}

/// 아이콘 그림 크기 — macOS 템플릿은 고정, Windows 는 주 모니터(알림 영역이 있는 곳) 배율.
fn icon_px<R: Runtime>(app: &AppHandle<R>) -> u32 {
    if cfg!(target_os = "macos") {
        return TEMPLATE_PX;
    }
    let scale = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| m.scale_factor())
        .or_else(|| {
            app.get_webview_window(crate::MAIN_WINDOW)
                .and_then(|w| w.scale_factor().ok())
        })
        .unwrap_or(1.0);
    color_icon_px(scale)
}

/// 상태 아이콘 그림 — macOS 템플릿(36px = 18pt 의 2배), Windows 컬러(배율별).
fn icon_image(live: bool, px: u32) -> Option<tauri::image::Image<'static>> {
    #[cfg(target_os = "macos")]
    let bytes: &[u8] = {
        let _ = px;
        if live {
            include_bytes!("../icons/tray/template-live.png")
        } else {
            include_bytes!("../icons/tray/template.png")
        }
    };
    #[cfg(not(target_os = "macos"))]
    let bytes: &[u8] = match (live, px) {
        (false, 16) => include_bytes!("../icons/tray/color-16.png"),
        (false, 20) => include_bytes!("../icons/tray/color-20.png"),
        (false, 24) => include_bytes!("../icons/tray/color-24.png"),
        (false, 32) => include_bytes!("../icons/tray/color-32.png"),
        (false, 40) => include_bytes!("../icons/tray/color-40.png"),
        (false, _) => include_bytes!("../icons/tray/color-48.png"),
        (true, 16) => include_bytes!("../icons/tray/color-live-16.png"),
        (true, 20) => include_bytes!("../icons/tray/color-live-20.png"),
        (true, 24) => include_bytes!("../icons/tray/color-live-24.png"),
        (true, 32) => include_bytes!("../icons/tray/color-live-32.png"),
        (true, 40) => include_bytes!("../icons/tray/color-live-40.png"),
        (true, _) => include_bytes!("../icons/tray/color-live-48.png"),
    };
    tauri::image::Image::from_bytes(bytes).ok()
}

fn build_entries<R: Runtime>(
    app: &AppHandle<R>,
    spec: &[Entry],
) -> tauri::Result<Vec<Box<dyn IsMenuItem<R>>>> {
    let mut out: Vec<Box<dyn IsMenuItem<R>>> = Vec::with_capacity(spec.len());
    for (i, e) in spec.iter().enumerate() {
        match e {
            Entry::Label(text) => out.push(Box::new(MenuItem::with_id(
                app,
                format!("tray-label-{i}"),
                text,
                false,
                None::<&str>,
            )?)),
            Entry::Item { id, label } => out.push(Box::new(MenuItem::with_id(
                app,
                id.as_str(),
                label,
                true,
                None::<&str>,
            )?)),
            Entry::Check { id, label, checked } => out.push(Box::new(CheckMenuItem::with_id(
                app,
                id.as_str(),
                label,
                true,
                *checked,
                None::<&str>,
            )?)),
            Entry::Submenu { label, items } => {
                let children = build_entries(app, items)?;
                let refs: Vec<&dyn IsMenuItem<R>> = children.iter().map(|b| b.as_ref()).collect();
                out.push(Box::new(Submenu::with_items(app, label, true, &refs)?));
            }
            Entry::Separator => out.push(Box::new(PredefinedMenuItem::separator(app)?)),
        }
    }
    Ok(out)
}

fn build_menu<R: Runtime>(app: &AppHandle<R>, spec: &[Entry]) -> tauri::Result<Menu<R>> {
    let items = build_entries(app, spec)?;
    let refs: Vec<&dyn IsMenuItem<R>> = items.iter().map(|b| b.as_ref()).collect();
    Menu::with_items(app, &refs)
}

/// 아이콘·메뉴·툴팁·배지를 지금 상태에 맞춰 바뀐 것만 다시 그린다. 어느 스레드에서 불러도 된다.
pub fn sync<R: Runtime>(app: &AppHandle<R>) {
    let Some(hub) = app.try_state::<TrayHub>() else {
        return;
    };
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    // 메인 스레드를 기다릴 수 있는 조회는 잠금 밖에서.
    let window_hidden = main_hidden(app);
    let px = icon_px(app);
    let visible = tray_visible(&general(app), HIDDEN.load(Ordering::Relaxed));
    let model = {
        let mut m = lock(&hub.model);
        m.window_hidden = window_hidden;
        m.clone()
    };
    let next = Shown {
        spec: menu_spec(&model),
        live_icon: !model.lives.is_empty(),
        icon_px: px,
        tooltip: tooltip(&model),
        count: model.lives.len(),
        visible,
    };
    let prev = {
        let mut shown = lock(&hub.shown);
        if shown.as_ref() == Some(&next) {
            return;
        }
        shown.replace(next.clone())
    };
    let changed = |f: &dyn Fn(&Shown) -> bool| prev.as_ref().is_none_or(f);
    if changed(&|p| p.visible != next.visible) {
        let _ = tray.set_visible(next.visible);
    }
    if changed(&|p| p.spec != next.spec) {
        match build_menu(app, &next.spec) {
            Ok(menu) => {
                let _ = tray.set_menu(Some(menu));
            }
            Err(e) => tracing::warn!("메뉴 막대 메뉴를 만들지 못했습니다: {e}"),
        }
    }
    if changed(&|p| (p.live_icon, p.icon_px) != (next.live_icon, next.icon_px)) {
        if let Some(icon) = icon_image(next.live_icon, next.icon_px) {
            let _ = tray.set_icon(Some(icon));
            #[cfg(target_os = "macos")]
            let _ = tray.set_icon_as_template(true);
        }
    }
    if changed(&|p| p.tooltip != next.tooltip) {
        let _ = tray.set_tooltip(Some(next.tooltip.as_str()));
    }
    if changed(&|p| p.count != next.count) {
        set_count(app, &tray, next.count);
    }
    crate::tray_popover::emit_state(app);
}

/// 다음 [`sync`] 가 메뉴·배지를 다시 만들게 한다(OS 가 체크 표시를 먼저 뒤집었거나 Dock 이 배지를 잃었을 때).
fn invalidate<R: Runtime>(app: &AppHandle<R>, menu: bool, count: bool) {
    if let Some(hub) = app.try_state::<TrayHub>() {
        if let Some(shown) = lock(&hub.shown).as_mut() {
            if menu {
                shown.spec.clear();
            }
            if count {
                shown.count = usize::MAX;
            }
        }
    }
}

/// 방송 중 수 — macOS 메뉴 막대 아이콘 옆 숫자와 Dock 배지.
fn set_count<R: Runtime>(app: &AppHandle<R>, tray: &tauri::tray::TrayIcon<R>, count: usize) {
    let _ = tray.set_title(if count > 0 {
        Some(count.to_string())
    } else {
        None
    });
    if let Some(w) = app.get_webview_window(crate::MAIN_WINDOW) {
        let _ = w.set_badge_count(if count > 0 { Some(count as i64) } else { None });
    }
}

/// 화면이 알려 준 지금 시청 중·음소거(바뀌었을 때만 온다).
#[tauri::command]
pub fn tray_set_state(app: AppHandle, state: TrayFrontState) {
    let Some(hub) = app.try_state::<TrayHub>() else {
        return;
    };
    {
        let mut m = lock(&hub.model);
        if m.front == state {
            return;
        }
        m.front = state;
    }
    sync(&app);
}

// ── 창 숨기기·열기 ──────────────────────────────────

/// 메인 창을 아이콘으로 숨긴다(재생·채팅은 계속). macOS 는 설정에 따라 Dock 아이콘도 숨긴다.
pub fn hide_main<R: Runtime>(app: &AppHandle<R>) {
    let Some(w) = app.get_webview_window(crate::MAIN_WINDOW) else {
        return;
    };
    HIDDEN.store(true, Ordering::Relaxed);
    // 전체 화면이면 먼저 빠져나온다 — 숨긴 채 빈 전체 화면 공간이 남지 않게.
    if w.is_fullscreen().unwrap_or(false) {
        let _ = w.set_fullscreen(false);
    }
    let _ = w.hide();
    #[cfg(target_os = "macos")]
    if general(app).hide_dock_in_tray {
        let _ = app.set_dock_visibility(false);
    }
    crate::perf_log::emit("tray.hide", serde_json::json!({}));
    sync(app);
}

/// 메인 창을 다시 보이고 앞으로(숨김·최소화 모두). Dock 아이콘을 숨겼으면 되돌린다.
pub fn show_main<R: Runtime>(app: &AppHandle<R>) {
    let was_hidden = HIDDEN.swap(false, Ordering::Relaxed);
    #[cfg(target_os = "macos")]
    if was_hidden {
        // Dock 아이콘이 돌아오면 실행 중 아이콘(앱 테마)과 배지를 다시 건다 — 정책을 바꾸면 사라진다.
        let _ = app.set_dock_visibility(true);
        if let Some(services) = app.try_state::<AppServices>() {
            crate::dock_icon::apply(app, services.settings.appearance().theme);
        }
        invalidate(app, false, true);
    }
    if let Some(w) = app.get_webview_window(crate::MAIN_WINDOW) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
    if was_hidden {
        crate::perf_log::emit("tray.show", serde_json::json!({}));
    }
    sync(app);
}

/// 메인 창 이벤트 — 설정에 따라 닫기·최소화를 숨기기로 바꾼다. 메인 창 `on_window_event` 에서 부른다.
pub fn on_main_window_event<R: Runtime>(window: &WebviewWindow<R>, event: &WindowEvent) {
    let app = window.app_handle();
    match event {
        WindowEvent::CloseRequested { api, .. } => {
            if !QUITTING.load(Ordering::Relaxed) && should_hide(&general(app), true, false) {
                api.prevent_close();
                hide_main(app);
            }
        }
        // Windows — 최소화하면 WM_SIZE(0×0)가 Resized 로 온다.
        WindowEvent::Resized(_) => {
            if cfg!(not(target_os = "macos"))
                && !HIDDEN.load(Ordering::Relaxed)
                && window.is_minimized().unwrap_or(false)
                && should_hide(&general(app), false, true)
            {
                hide_main(app);
            } else {
                sync(app);
            }
        }
        // 메인 창이 정말 닫혔다(숨기기 아님) — 보조 창(채팅·플레이어·통계)이 남으면 앱이 살아 있는데 메인 창을 다시 열
        // 길이 없었다(Dock·트레이 '열기'는 메인 창이 있어야 한다). 메인이 돌리던 멀티라이브·채팅 세션도 그대로 돌았다.
        // 보조 창도 닫아 마지막 창이 닫힐 때처럼 앱을 끝낸다.
        WindowEvent::Destroyed => {
            let labels: Vec<String> = app.webview_windows().into_keys().collect();
            for label in secondary_windows_after_main_closed(labels.iter().map(String::as_str)) {
                if let Some(w) = app.get_webview_window(label) {
                    let _ = w.close();
                }
            }
        }
        // macOS 최소화는 tao 이벤트가 없어 observe_minimize 가 AppKit 알림으로 받는다.
        WindowEvent::Focused(_) => sync(app),
        WindowEvent::ScaleFactorChanged { .. } => sync(app),
        _ => {}
    }
}

/// (macOS) 메인 창이 Dock 으로 최소화되면(노란 단추·⌘M·창 메뉴) 설정에 따라 아이콘으로 숨긴다.
/// tao 에는 최소화 이벤트가 없고, 초점 없는 뒤쪽 창의 노란 단추는 초점 이벤트도 남기지 않아 AppKit 알림을 직접 받는다.
///
/// 최소화된 채로 숨기면(orderOut) 최소화 타일이 Dock 에 남고, 숨긴 뒤에는 최소화 풀기도 듣지 않는다. 그래서
/// 창을 완전히 투명하게 한 채 최소화를 풀고(보이지 않게 돌아온다), 다 풀렸다는 알림에서 숨긴 뒤 투명도를 되돌린다
/// (VM 연속 캡처로 화면에 그려지지 않는 것을 확인). 메인 창은 앱이 끝날 때까지 하나라 관찰을 풀지 않는다.
#[cfg(target_os = "macos")]
pub fn observe_minimize<R: Runtime>(window: &WebviewWindow<R>) {
    use objc2_app_kit::{
        NSWindow, NSWindowDidDeminiaturizeNotification, NSWindowDidMiniaturizeNotification,
    };
    use objc2_foundation::{NSNotification, NSNotificationCenter};

    /// 투명하게 최소화를 푸는 중 — 다 풀리면 숨긴다.
    static UNMINIMIZING: AtomicBool = AtomicBool::new(false);

    let Ok(ptr) = window.ns_window() else {
        return;
    };
    if ptr.is_null() {
        return;
    }
    let addr = ptr as usize;
    let app = window.app_handle().clone();
    let _ = window.run_on_main_thread(move || {
        // SAFETY: tao 가 돌려준 살아 있는 메인 창 NSWindow 이고, 알림은 모두 메인 스레드에서 온다(큐 없음).
        let ns_window = unsafe { &*(addr as *const NSWindow) };
        let window_at = move || unsafe { &*(addr as *const NSWindow) };
        let hide_app = app.clone();
        let minimized = block2::RcBlock::new(move |_: std::ptr::NonNull<NSNotification>| {
            if HIDDEN.load(Ordering::Relaxed) || !should_hide(&general(&hide_app), false, true) {
                return;
            }
            let w = window_at();
            UNMINIMIZING.store(true, Ordering::Relaxed);
            w.setAlphaValue(0.0);
            w.deminiaturize(None);
        });
        let restored = block2::RcBlock::new(move |_: std::ptr::NonNull<NSNotification>| {
            if !UNMINIMIZING.swap(false, Ordering::Relaxed) {
                return;
            }
            hide_main(&app);
            window_at().setAlphaValue(1.0);
        });
        let center = NSNotificationCenter::defaultCenter();
        for (name, block) in [
            (unsafe { NSWindowDidMiniaturizeNotification }, &minimized),
            (unsafe { NSWindowDidDeminiaturizeNotification }, &restored),
        ] {
            // SAFETY: 이름은 AppKit 상수, 대상은 살아 있는 창이다.
            let token = unsafe {
                center.addObserverForName_object_queue_usingBlock(
                    Some(name),
                    Some(ns_window),
                    None,
                    block,
                )
            };
            std::mem::forget(token);
        }
    });
}

/// (macOS) Dock 아이콘을 누름 — 숨겨 둔 창을 다시 연다.
pub fn on_reopen<R: Runtime>(app: &AppHandle<R>, has_visible_windows: bool) {
    tracing::info!(
        has_visible_windows,
        hidden = HIDDEN.load(Ordering::Relaxed),
        "Dock 다시 열기"
    );
    if HIDDEN.load(Ordering::Relaxed) || !has_visible_windows || main_hidden(app) {
        crate::tray_popover::hide(app);
        show_main(app);
    }
}

fn on_menu<R: Runtime>(app: &AppHandle<R>, id: &str) {
    if let Some(channel_id) = id.strip_prefix(LIVE_PREFIX) {
        crate::tray_popover::hide(app);
        crate::system::handle_urls(app, [format!("cview://live?channelId={channel_id}")]);
        return;
    }
    match id {
        "tray-multilive" => {
            crate::tray_popover::hide(app);
            crate::system::handle_urls(app, ["cview://multilive".to_string()]);
        }
        "tray-now" => {
            crate::tray_popover::hide(app);
            show_main(app);
        }
        "tray-toggle" => {
            if main_hidden(app) {
                crate::tray_popover::hide(app);
                show_main(app);
            } else {
                hide_main(app);
            }
        }
        "tray-mute" => {
            // OS 가 체크 표시를 먼저 뒤집는다. 메인 창이 숨어 있거나 초점이 없어도 동작하게 직접 보낸다.
            crate::tray_popover::send_main_control(app, serde_json::json!({ "kind": "mute" }));
            invalidate(app, true, false);
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_millis(1500)).await;
                sync(&app);
            });
        }
        "tray-settings" => {
            crate::tray_popover::hide(app);
            show_main(app);
            crate::tray_popover::send_main_control(
                app,
                serde_json::json!({ "kind": "navigate", "route": "settings" }),
            );
        }
        "tray-update" => {
            crate::tray_popover::hide(app);
            show_main(app);
            crate::tray_popover::send_main_control(
                app,
                serde_json::json!({ "kind": "check-update" }),
            );
        }
        "tray-quit" => {
            request_quit(app);
        }
        _ => {}
    }
}

/// 트레이 아이콘을 달고 갱신 작업을 띄운다.
pub fn install(app: &AppHandle) -> tauri::Result<()> {
    app.manage(TrayHub::default());
    let menu = build_menu(app, &menu_spec(&TrayModel::default()))?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("CView")
        .menu(&menu)
        // macOS 왼쪽 클릭은 드롭다운 팝오버(tray_popover.rs), Windows 왼쪽 클릭은 창 열기. 오른쪽 클릭(⌃클릭)은 기본 메뉴.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| on_menu(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                position,
                ..
            } = event
            {
                #[cfg(target_os = "macos")]
                crate::tray_popover::toggle_from_tray(tray.app_handle(), rect, position);
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = (rect, position);
                    show_main(tray.app_handle());
                }
            }
        });
    if let Some(icon) = icon_image(false, icon_px(app)) {
        builder = builder.icon(icon);
        #[cfg(target_os = "macos")]
        {
            builder = builder.icon_as_template(true);
        }
    } else if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    sync(app);

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

/// 팔로잉 방송·즐겨찾기를 다시 읽는다.
async fn refresh(app: &AppHandle) {
    refresh_generic(app).await;
}

async fn refresh_generic<R: Runtime>(app: &AppHandle<R>) {
    let services = app.state::<AppServices>();
    let logged_in = services.auth.is_authenticated();
    let mut lives: Vec<LiveEntry> = Vec::new();
    if logged_in {
        if let Some(result) = services
            .following
            .poll(FollowingPollReason::ScreenEntry)
            .await
        {
            let favorites: HashSet<String> = services
                .data_store
                .as_ref()
                .and_then(|s| s.fetch_favorite_items().ok())
                .map(|v| v.into_iter().map(|c| c.channel_id).collect())
                .unwrap_or_default();
            let mut items: Vec<_> = result
                .channels
                .iter()
                .map(to_live_channel_item)
                .filter(|i| i.is_live)
                .collect();
            items.sort_by_key(|i| std::cmp::Reverse(i.viewer_count));
            lives = items
                .into_iter()
                .map(|i| LiveEntry {
                    favorite: favorites.contains(&i.channel_id),
                    channel_id: i.channel_id,
                    name: i.channel_name,
                    category: i.category_name,
                    viewers: i.viewer_count,
                })
                .collect();
        }
    }
    let Some(hub) = app.try_state::<TrayHub>() else {
        return;
    };
    {
        let mut m = lock(&hub.model);
        m.logged_in = logged_in;
        m.lives = lives;
    }
    sync(app);
}

/// 메인 창이 닫힌 뒤 함께 닫을 창 — 메인을 뺀 나머지. 순수 함수.
fn secondary_windows_after_main_closed<'a>(labels: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    labels.filter(|l| *l != crate::MAIN_WINDOW).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn closing_main_closes_secondary_windows() {
        let labels = ["main", "chat-1", "player"];
        assert_eq!(
            super::secondary_windows_after_main_closed(labels.into_iter()),
            ["chat-1", "player"]
        );
        assert!(super::secondary_windows_after_main_closed(["main"].into_iter()).is_empty());
    }

    use super::*;

    fn live(id: &str, viewers: i64, favorite: bool) -> LiveEntry {
        LiveEntry {
            channel_id: id.into(),
            name: format!("채널{id}"),
            category: Some("talk".into()),
            viewers,
            favorite,
        }
    }

    fn ids(spec: &[Entry]) -> Vec<String> {
        spec.iter()
            .map(|e| match e {
                Entry::Label(t) => format!("#{t}"),
                Entry::Item { id, .. } | Entry::Check { id, .. } => id.clone(),
                Entry::Submenu { label, items } => format!("[{label}:{}]", items.len()),
                Entry::Separator => "-".into(),
            })
            .collect()
    }

    #[test]
    fn labels() {
        assert_eq!(
            live_label("한동숙", Some("마인크래프트"), 12_300),
            "한동숙 — 마인크래프트 · 1.2만"
        );
        assert_eq!(live_label("a", None, 900), "a · 900");
        assert_eq!(live_label("a", Some("  "), 900), "a · 900");
        assert_eq!(truncate("가나다라마바사", 5), "가나다라…");
        assert_eq!(truncate("  여러   칸  ", 10), "여러 칸");
        assert_eq!(truncate("abc d", 5), "abc d");
        assert_eq!(truncate("abc defgh", 5), "abc…", "자른 끝 공백은 지운다");
    }

    #[test]
    fn now_playing_labels() {
        let n = |kind: &str, title: &str, ch: &str| NowPlaying {
            kind: kind.into(),
            title: title.into(),
            channel_name: ch.into(),
            ..NowPlaying::default()
        };
        assert_eq!(
            now_playing_label(&n("live", "저녁 방송", "채널")),
            "채널 — 저녁 방송"
        );
        assert_eq!(now_playing_label(&n("live", "채널", "채널")), "채널");
        assert_eq!(now_playing_label(&n("live", "", "채널")), "채널");
        assert_eq!(now_playing_label(&n("live", "제목만", "")), "제목만");
        assert_eq!(
            now_playing_label(&n("vod", "다시 보기 제목", "채널")),
            "다시보기 · 채널 — 다시 보기 제목"
        );
        assert_eq!(now_playing_label(&n("live", "", "")), "CView");
    }

    #[test]
    fn logged_out_menu_keeps_controls() {
        let spec = menu_spec(&TrayModel::default());
        assert_eq!(
            ids(&spec),
            [
                "#로그인하면 방송 중인 팔로잉이 보여요",
                "-",
                "tray-toggle",
                "tray-multilive",
                "tray-settings",
                "tray-update",
                "-",
                "tray-quit"
            ]
        );
        assert!(spec.contains(&item("tray-toggle", HIDE_LABEL)));
        let hidden = menu_spec(&TrayModel {
            window_hidden: true,
            ..TrayModel::default()
        });
        assert!(hidden.contains(&item("tray-toggle", "CView 열기")));
    }

    #[test]
    fn now_playing_and_mute_on_top() {
        let mut m = TrayModel {
            logged_in: true,
            ..TrayModel::default()
        };
        m.front.now_playing = Some(NowPlaying {
            kind: "live".into(),
            title: "t".into(),
            channel_name: "c".into(),
            ..NowPlaying::default()
        });
        assert_eq!(
            ids(&menu_spec(&m))[..4],
            ["#지금 시청 중", "tray-now", "-", "#방송 중인 팔로잉 없음"]
        );
        m.front.muted = Some(true);
        assert_eq!(
            menu_spec(&m)[2],
            Entry::Check {
                id: "tray-mute".into(),
                label: "음소거".into(),
                checked: true
            }
        );
    }

    #[test]
    fn favorites_first_then_others_with_more_submenu() {
        let mut lives = vec![live("f1", 50, true), live("f2", 10, true)];
        lives.extend((0..9).map(|i| live(&format!("o{i}"), 100 - i, false)));
        let m = TrayModel {
            logged_in: true,
            lives,
            ..TrayModel::default()
        };
        assert_eq!(
            ids(&menu_spec(&m))[..12],
            [
                "#즐겨찾기 · 방송 중 2",
                "tray-live:f1",
                "tray-live:f2",
                "-",
                "#다른 팔로잉 · 방송 중 9",
                "tray-live:o0",
                "tray-live:o1",
                "tray-live:o2",
                "tray-live:o3",
                "tray-live:o4",
                "[더 보기 (4):4]",
                "-",
            ]
        );
    }

    #[test]
    fn following_only_and_favorites_only() {
        let m = TrayModel {
            logged_in: true,
            lives: (0..3).map(|i| live(&i.to_string(), 10, false)).collect(),
            ..TrayModel::default()
        };
        let got = ids(&menu_spec(&m));
        assert_eq!(got[0], "#팔로잉 · 방송 중 3");
        assert_eq!(got[4], "-");
        assert!(
            !got.iter().any(|s| s.starts_with('[')),
            "넘치지 않으면 더 보기 없음"
        );

        let m = TrayModel {
            logged_in: true,
            lives: vec![live("f", 1, true)],
            ..TrayModel::default()
        };
        let got = ids(&menu_spec(&m));
        assert_eq!(
            got[..4],
            ["#즐겨찾기 · 방송 중 1", "tray-live:f", "-", "tray-toggle"]
        );
        assert!(
            got.windows(2).all(|w| !(w[0] == "-" && w[1] == "-")),
            "구분선이 겹치지 않는다"
        );
    }

    #[test]
    fn many_followings_capped() {
        let m = TrayModel {
            logged_in: true,
            lives: (0..100)
                .map(|i| live(&i.to_string(), 1000 - i, false))
                .collect(),
            ..TrayModel::default()
        };
        let sub = menu_spec(&m).into_iter().find_map(|e| match e {
            Entry::Submenu { label, items } => Some((label, items.len())),
            _ => None,
        });
        assert_eq!(sub, Some(("더 보기 (92)".to_string(), MAX_MORE)));
    }

    #[test]
    fn tooltips() {
        let mut m = TrayModel::default();
        assert_eq!(tooltip(&m), "CView");
        m.lives = vec![live("a", 1, false), live("b", 1, false)];
        assert_eq!(tooltip(&m), "CView — 팔로잉 방송 중 2");
        m.front.now_playing = Some(NowPlaying {
            kind: "live".into(),
            title: "".into(),
            channel_name: "채널".into(),
            ..NowPlaying::default()
        });
        assert_eq!(tooltip(&m), "CView — 채널 시청 중");
    }

    #[test]
    fn icon_size_follows_dpi() {
        assert_eq!(color_icon_px(1.0), 16);
        assert_eq!(color_icon_px(1.25), 20);
        assert_eq!(color_icon_px(1.5), 24);
        assert_eq!(color_icon_px(1.75), 32);
        assert_eq!(color_icon_px(2.0), 32);
        assert_eq!(color_icon_px(2.5), 40);
        assert_eq!(color_icon_px(3.0), 48);
        assert_eq!(color_icon_px(4.0), 48);
        assert_eq!(color_icon_px(0.0), 16);
        assert_eq!(color_icon_px(f64::NAN), 16);
    }

    #[test]
    fn icons_decode_at_expected_sizes() {
        for live in [false, true] {
            for px in [16, 20, 24, 32, 40, 48] {
                let img = icon_image(live, px).expect("아이콘");
                let want = if cfg!(target_os = "macos") {
                    TEMPLATE_PX
                } else {
                    px
                };
                assert_eq!((img.width(), img.height()), (want, want));
            }
        }
    }

    #[test]
    fn visibility_and_hide_rules() {
        let mut g = GeneralSettings {
            show_in_menu_bar: false,
            ..GeneralSettings::default()
        };
        assert!(!tray_visible(&g, false));
        assert!(
            tray_visible(&g, true),
            "숨겨 둔 동안에는 꺼 두었어도 보인다"
        );
        g.show_in_menu_bar = true;
        assert!(tray_visible(&g, false));

        let off = GeneralSettings::default();
        assert!(!should_hide(&off, true, false), "기본은 예전처럼 닫힌다");
        assert!(!should_hide(&off, false, true));
        let close = GeneralSettings {
            close_to_tray: true,
            ..GeneralSettings::default()
        };
        assert!(should_hide(&close, true, false));
        assert!(!should_hide(&close, false, true));
        let min = GeneralSettings {
            minimize_to_tray: true,
            ..GeneralSettings::default()
        };
        assert!(should_hide(&min, false, true));
        assert!(!should_hide(&min, true, false));
    }
}
