//! 시스템 연동 — 원본 NotificationService / BackgroundUpdateService / MentionNotificationService /
//! DeepLinkRouter·DeepLinkInbox / SingleInstanceGuard / UpdateService.
//!
//! - 알림(SYS-04): 로그인 상태에서 팔로잉 목록을 주기적으로 받아 직전 폴과 비교해 방송 시작·카테고리 변경·
//!   제목 변경을 알린다. 전역 스위치(일반 설정)와 채널별 설정(채널 알림)을 모두 따른다. 첫 폴은 기준선만 잡는다.
//! - 멘션 알림(SYS-05): 프런트가 멘션을 감지하면 `notify_send` 로 보낸다(창이 앞에 있을 때는 프런트가 거른다).
//! - 딥 링크(SYS-06): `cview://…` 를 파싱해 `deeplink://open` 으로 보낸다. 프런트가 준비되기 전에 온 링크는
//!   쌓아 두었다가 `deeplink_take_pending` 으로 건넨다.
//! - 단일 인스턴스(SYS-07): 두 번째 실행은 기존 창을 앞으로 가져오고 인자 속 링크를 넘긴다.
//! - 업데이트 확인(SYS-08): GitHub 최신 릴리스와 현재 버전을 비교한다(설치는 하지 않고 페이지를 연다).

use std::sync::Mutex;
use std::time::Duration;

use cview_app_core::actions::cview_action::LiveOpenMode;
use cview_app_core::caching::following_store::to_live_channel_item;
use cview_app_core::navigation::deep_link::DeepLink;
use cview_app_core::services::background_update::{BackgroundUpdateBaseline, BackgroundUpdateDiff};
use cview_app_core::services::notification_service::{NoopNotifier, NotificationService};
use cview_app_core::services::update_service::{
    compare_versions, GitHubAsset, GitHubRelease, UpdateService,
};
use cview_core::utils::following_poll::FollowingPollReason;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tauri_plugin_notification::NotificationExt;

use crate::commands::{CmdResult, CommandError};
use crate::state::AppServices;

pub const DEEP_LINK_OPEN: &str = "deeplink://open";

/// 프런트 준비 전에 들어온 딥 링크.
#[derive(Default)]
pub struct PendingDeepLinks(pub Mutex<Vec<Value>>);

/// 딥 링크 → 프런트 페이로드. 순수 함수.
pub fn deep_link_payload(link: &DeepLink) -> Value {
    match link {
        DeepLink::Live { channel_id, mode } => json!({
            "kind": "live",
            "channelId": channel_id,
            "mode": match mode {
                LiveOpenMode::Watch => "watch",
                LiveOpenMode::Multi => "multi",
                LiveOpenMode::ChatOnly => "chatonly",
            },
        }),
        DeepLink::Channel { channel_id } => json!({ "kind": "channel", "channelId": channel_id }),
        DeepLink::Following { mode } => json!({
            "kind": "following",
            "mode": mode.as_ref().and_then(|m| serde_json::to_value(m).ok()),
        }),
        DeepLink::MultiLive => json!({ "kind": "multilive" }),
        DeepLink::Home => json!({ "kind": "home" }),
        DeepLink::Search { query } => json!({ "kind": "search", "query": query }),
        DeepLink::Clip { uid } => json!({ "kind": "clip", "uid": uid }),
        DeepLink::Login => json!({ "kind": "login" }),
    }
}

fn focus_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window(crate::MAIN_WINDOW) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// URL 문자열들을 딥 링크로 처리한다. 프런트가 준비 전이면 쌓는다.
pub fn handle_urls<R: Runtime>(app: &AppHandle<R>, urls: impl IntoIterator<Item = String>) {
    let payloads: Vec<Value> = urls
        .into_iter()
        .filter_map(|u| DeepLink::parse(&u))
        .map(|l| deep_link_payload(&l))
        .collect();
    if payloads.is_empty() {
        return;
    }
    focus_main(app);
    if let Some(p) = app.try_state::<PendingDeepLinks>() {
        p.0.lock()
            .unwrap_or_else(|e| e.into_inner())
            .extend(payloads.iter().cloned());
    }
    for p in payloads {
        let _ = app.emit(DEEP_LINK_OPEN, p);
    }
}

/// 두 번째 실행 — 기존 창을 앞으로, 인자 속 `cview://` 를 넘긴다.
pub fn on_second_instance<R: Runtime>(app: &AppHandle<R>, argv: Vec<String>) {
    focus_main(app);
    handle_urls(app, argv.into_iter().filter(|a| a.starts_with("cview://")));
}

/// 프런트가 준비되면 부른다 — 쌓인 링크를 넘기고 비운다(이벤트로도 이미 갔으면 프런트가 중복을 거른다).
#[tauri::command]
pub fn deeplink_take_pending(pending: State<'_, PendingDeepLinks>) -> Vec<Value> {
    std::mem::take(&mut *pending.0.lock().unwrap_or_else(|e| e.into_inner()))
}

// ── 알림 ─────────────────────────────────────────

/// 시스템 알림 한 건(멘션 알림 등).
#[tauri::command]
pub fn notify_send(app: AppHandle, title: String, body: String) -> CmdResult<()> {
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|e| CommandError(e.to_string()))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifyPermissionDto {
    pub state: String,
}

/// 알림 권한 상태·요청.
#[tauri::command]
pub fn notify_permission(app: AppHandle, request: Option<bool>) -> CmdResult<NotifyPermissionDto> {
    let n = app.notification();
    let state = if request.unwrap_or(false) {
        n.request_permission()
    } else {
        n.permission_state()
    }
    .map_err(|e| CommandError(e.to_string()))?;
    Ok(NotifyPermissionDto {
        state: format!("{state:?}").to_lowercase(),
    })
}

/// 팔로잉 변화 알림 루프. 문구는 원본 `NotificationService` 규칙을 그대로 쓴다.
pub fn spawn_live_notifications(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let texts = NotificationService::new(std::sync::Arc::new(NoopNotifier));
        let mut baseline: Option<BackgroundUpdateBaseline> = None;
        tokio::time::sleep(Duration::from_secs(20)).await;
        loop {
            let services = app.state::<AppServices>();
            let general = services.settings.general();
            let interval = general.auto_refresh_interval.clamp(30.0, 900.0);
            // 이 루프가 주기 갱신을 맡는 동안에는 저장소에 주기를 알려, 화면 진입(사이드바·인사이트·홈)이
            // 주기의 1.15배 안의 결과를 다시 쓰게 한다 — 주기가 60초를 조금 넘길 때마다 전체 목록(여러 페이지 +
            // 채널 상세)을 또 받던 것을 막는다(perf/baseline-2.md 대기 중 24요청/분).
            let polling = general.notifications_enabled && services.auth.is_authenticated();
            services
                .following
                .set_scheduled_cadence(polling.then_some(interval));
            if !polling {
                baseline = None;
            } else if let Some(result) = services
                .following
                .poll(FollowingPollReason::Scheduled)
                .await
            {
                let items: Vec<_> = result.channels.iter().map(to_live_channel_item).collect();
                let prev = baseline.clone().unwrap_or_default();
                let diff = BackgroundUpdateDiff::new(&items, &prev);
                if baseline.is_some() {
                    let per = services.settings.channel_notifications();
                    let mut out = Vec::new();
                    if general.notify_on_live_start {
                        out.extend(
                            diff.newly_online
                                .iter()
                                .filter(|c| per.is_live_notification_enabled(&c.channel_id))
                                .map(|c| texts.online_notification(c)),
                        );
                    }
                    if general.notify_on_category_change {
                        out.extend(
                            diff.category_changed
                                .iter()
                                .filter(|c| {
                                    per.is_category_change_notification_enabled(&c.channel_id)
                                })
                                .map(|c| texts.category_notification(c)),
                        );
                    }
                    if general.notify_on_title_change {
                        out.extend(
                            diff.title_changed
                                .iter()
                                .filter(|c| per.is_title_change_notification_enabled(&c.channel_id))
                                .map(|c| texts.title_notification(c)),
                        );
                    }
                    // 한꺼번에 몰리면(재연결 직후 등) 5건까지만.
                    for n in out.into_iter().take(5) {
                        let _ = app
                            .notification()
                            .builder()
                            .title(n.title)
                            .body(n.body)
                            .show();
                    }
                }
                baseline = Some(diff.next_baseline);
            }
            tokio::time::sleep(Duration::from_secs_f64(interval)).await;
        }
    });
}

// ── 업데이트 확인 ─────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckDto {
    pub current: String,
    pub latest: Option<String>,
    pub newer: bool,
    pub url: Option<String>,
    pub notes: Option<String>,
    pub published_at: Option<String>,
    /// 이 플랫폼용 설치 파일(macOS .dmg · Windows .exe/.msi) — 피드에 없으면 `None`.
    pub download_url: Option<String>,
    pub download_name: Option<String>,
    pub download_size: Option<i64>,
}

/// 릴리스 자산 중 이 OS·아키텍처에 맞는 설치 파일. 아키텍처 표기가 맞는 것을 먼저 고른다. 순수 함수.
fn platform_asset<'a>(assets: &'a [GitHubAsset], os: &str, arch: &str) -> Option<&'a GitHubAsset> {
    let exts: &[&str] = match os {
        "macos" => &[".dmg"],
        "windows" => &[".exe", ".msi"],
        _ => return None,
    };
    let arch_tags: &[&str] = match arch {
        "aarch64" => &["arm64", "aarch64"],
        "x86_64" => &["x64", "x86_64", "amd64"],
        _ => &[],
    };
    let installers: Vec<&GitHubAsset> = exts
        .iter()
        .flat_map(|ext| {
            assets
                .iter()
                .filter(move |a| a.name.to_lowercase().ends_with(ext))
        })
        .collect();
    installers
        .iter()
        .find(|a| {
            let n = a.name.to_lowercase();
            arch_tags.iter().any(|t| n.contains(t))
        })
        .or_else(|| installers.first())
        .copied()
}

fn validated_update_url(value: &str) -> Result<url::Url, CommandError> {
    let parsed = url::Url::parse(value)
        .map_err(|_| CommandError("업데이트 서버 주소가 올바르지 않습니다".into()))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
    {
        return Err(CommandError(
            "업데이트 주소는 인증 정보 없는 HTTPS 주소여야 합니다".into(),
        ));
    }
    Ok(parsed)
}

fn update_feed_url(
    runtime: Option<&str>,
    embedded: Option<&str>,
) -> Result<url::Url, CommandError> {
    match runtime.or(embedded) {
        Some(value) => validated_update_url(value),
        None => validated_update_url(&format!(
            "https://api.github.com/repos/{}/releases/latest",
            UpdateService::REPOSITORY
        )),
    }
}

/// 설정된 HTTPS 서버의 최신 릴리스 확인.
#[tauri::command]
pub async fn update_check(app: AppHandle) -> CmdResult<UpdateCheckDto> {
    let current = app.package_info().version.to_string();
    let runtime_url = std::env::var("CVIEW_UPDATE_URL").ok();
    let url = update_feed_url(runtime_url.as_deref(), option_env!("CVIEW_UPDATE_URL"))?;
    let client = reqwest::Client::builder()
        .https_only(true)
        .build()
        .map_err(|e| CommandError(format!("업데이트 연결 초기화 실패: {e}")))?;
    let resp = client
        .get(url)
        .header("Cache-Control", "no-cache")
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "CView-Rust")
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| CommandError(format!("업데이트 서버에 연결하지 못했습니다: {e}")))?;
    let status = resp.status().as_u16();
    let rate_limited = resp
        .headers()
        .get("x-ratelimit-remaining")
        .and_then(|v| v.to_str().ok())
        == Some("0");
    if status == 403 && rate_limited {
        return Err(CommandError(
            "GitHub 요청 한도를 넘었습니다 — 한 시간쯤 뒤에 다시 확인해 주세요".into(),
        ));
    }
    if status == 404 {
        return Ok(UpdateCheckDto {
            current,
            latest: None,
            newer: false,
            url: None,
            notes: None,
            published_at: None,
            download_url: None,
            download_name: None,
            download_size: None,
        });
    }
    if !(200..300).contains(&status) {
        return Err(CommandError(format!("업데이트 확인 실패 (HTTP {status})")));
    }
    let release: GitHubRelease = resp
        .json()
        .await
        .map_err(|e| CommandError(format!("릴리스 정보를 읽지 못했습니다: {e}")))?;
    let latest = release.version_string().to_string();
    if let Some(page) = release.html_url.as_ref() {
        validated_update_url(page.as_str())?;
    }
    // 설치 파일 주소도 HTTPS 만 — 아니면 링크를 빼고 릴리스 페이지로 안내한다.
    let asset = platform_asset(
        &release.assets,
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
    .filter(|a| validated_update_url(a.browser_download_url.as_str()).is_ok());
    let (download_url, download_name, download_size) = match asset {
        Some(a) => (
            Some(a.browser_download_url.to_string()),
            Some(a.name.clone()),
            Some(a.size),
        ),
        None => (None, None, None),
    };
    Ok(UpdateCheckDto {
        newer: compare_versions(&latest, &current) == std::cmp::Ordering::Greater,
        current,
        latest: Some(latest),
        url: release.html_url.map(|u| u.to_string()),
        notes: release.body,
        published_at: release.published_at.map(|d| d.to_rfc3339()),
        download_url,
        download_name,
        download_size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> GitHubAsset {
        GitHubAsset {
            name: name.into(),
            size: 1,
            browser_download_url: url::Url::parse(&format!("https://example.com/{name}")).unwrap(),
        }
    }

    #[test]
    fn platform_installer_selection() {
        let assets = vec![
            asset("CView_0.1.21_x64-setup.exe"),
            asset("CView_0.1.21_x64.dmg"),
            asset("CView_0.1.21_arm64.dmg"),
            asset("notes.txt"),
        ];
        let name = |os, arch| platform_asset(&assets, os, arch).map(|a| a.name.as_str());
        assert_eq!(name("macos", "aarch64"), Some("CView_0.1.21_arm64.dmg"));
        assert_eq!(name("macos", "x86_64"), Some("CView_0.1.21_x64.dmg"));
        assert_eq!(
            name("windows", "x86_64"),
            Some("CView_0.1.21_x64-setup.exe")
        );
        assert_eq!(name("linux", "x86_64"), None);
        // 아키텍처 표기가 없으면 첫 설치 파일
        let plain = vec![asset("CView.dmg")];
        assert_eq!(
            platform_asset(&plain, "macos", "aarch64").map(|a| a.name.as_str()),
            Some("CView.dmg")
        );
        assert!(platform_asset(&[], "macos", "aarch64").is_none());
    }

    #[test]
    fn update_endpoint_configuration() {
        let embedded = "https://win-7sv7uak6k5b.horse-solfege.ts.net/releases/latest.json";
        assert_eq!(
            update_feed_url(None, Some(embedded)).unwrap().as_str(),
            embedded
        );
        assert_eq!(
            update_feed_url(Some("https://example.com/feed"), Some(embedded))
                .unwrap()
                .host_str(),
            Some("example.com")
        );
        assert_eq!(
            update_feed_url(None, None).unwrap().host_str(),
            Some("api.github.com")
        );
        for bad in [
            "http://example.com/feed",
            "https://user:secret@example.com/feed",
            "invalid",
            "",
        ] {
            assert!(update_feed_url(Some(bad), Some(embedded)).is_err());
        }
    }

    #[test]
    fn deep_link_payloads() {
        let p = deep_link_payload(
            &DeepLink::parse("cview://live?channelId=abc123&mode=multi").unwrap(),
        );
        assert_eq!(p["kind"], "live");
        assert_eq!(p["channelId"], "abc123");
        assert_eq!(p["mode"], "multi");
        let s = deep_link_payload(&DeepLink::parse("cview://search?q=%ED%95%9C").unwrap());
        assert_eq!(s["query"], "한");
        assert!(DeepLink::parse("https://live?channelId=a").is_none());
    }
}
