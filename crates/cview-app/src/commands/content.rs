//! 채널 상세·VOD·클립·개인 데이터(즐겨찾기·메모·북마크·최근 시청)·파일 저장 명령.
//!
//! 원본 화면: `ChannelInfoView`, `ChannelVODClipTab`, `PopularClipsView`, `VODBrowseView`,
//! `RecentFavoritesView`, `ChannelMemoSheet`, 북마크 단축키, 스크린샷·채팅 내보내기 저장.
//! 하위 크레이트 모델(`VodInfo`, `ClipInfo`, `ChannelInfo` …)은 이미 camelCase 로 직렬화되므로
//! 그대로 내보내고, 페이지 응답만 [`PageDto`] 로 평평하게 만든다.

use std::path::{Path, PathBuf};

use base64::Engine;
use chrono::Utc;
use cview_core::models::clip::ClipDetail;
use cview_core::models::live::LiveStatus;
use cview_core::models::search::{ClipInfo, VodInfo};
use cview_core::models::ChannelInfo;
use serde::Serialize;
use tauri::State;

use super::{CmdResult, CommandError};
use crate::state::AppServices;

/// 페이지 응답.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageDto<T> {
    pub items: Vec<T>,
    pub total_count: Option<i64>,
    /// 요청한 크기만큼 왔으면 다음 페이지가 있을 수 있다.
    pub has_more: bool,
}

impl<T> PageDto<T> {
    pub fn new(items: Vec<T>, total_count: Option<i64>, requested: i64) -> Self {
        let has_more = match total_count {
            Some(total) => (items.len() as i64) >= requested && total > items.len() as i64,
            None => (items.len() as i64) >= requested,
        };
        Self {
            items,
            total_count,
            has_more,
        }
    }
}

/// 채널 상세 — 채널 정보 + 내 즐겨찾기·메모.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelDetailDto {
    pub channel: ChannelInfo,
    pub is_favorite: bool,
    pub memo: String,
    /// 방송 중이면 라이브 제목·시청자 등.
    pub live: Option<crate::dto::LiveDetailDto>,
}

fn store_err(e: impl std::fmt::Display) -> CommandError {
    CommandError(format!("저장소 오류: {e}"))
}

#[tauri::command]
pub async fn channel_detail(
    services: State<'_, AppServices>,
    channel_id: String,
) -> CmdResult<ChannelDetailDto> {
    let channel = services.api.channel_info(&channel_id).await?;
    let live = if channel.open_live {
        services
            .api
            .live_detail(&channel_id, false)
            .await
            .ok()
            .filter(|l| l.status == LiveStatus::Open)
            .map(|l| crate::dto::LiveDetailDto::from_live_info(&l, &channel_id))
    } else {
        None
    };
    let (is_favorite, memo) = match &services.data_store {
        Some(store) => (
            store.is_favorite(&channel_id).unwrap_or(false),
            store.fetch_memo(&channel_id).unwrap_or_default(),
        ),
        None => (false, String::new()),
    };
    Ok(ChannelDetailDto {
        channel,
        is_favorite,
        memo,
        live,
    })
}

#[tauri::command]
pub async fn channel_vods(
    services: State<'_, AppServices>,
    channel_id: String,
    page: Option<i64>,
    size: Option<i64>,
) -> CmdResult<PageDto<VodInfo>> {
    let size = size.unwrap_or(24).clamp(1, 50);
    let res = services
        .api
        .vod_list(&channel_id, page.unwrap_or(0).max(0), size, false)
        .await?;
    Ok(PageDto::new(res.data, res.total_count, size))
}

#[tauri::command]
pub async fn channel_clips(
    services: State<'_, AppServices>,
    channel_id: String,
    page: Option<i64>,
    size: Option<i64>,
) -> CmdResult<PageDto<ClipInfo>> {
    let size = size.unwrap_or(24).clamp(1, 50);
    let res = services
        .api
        .clip_list(&channel_id, page.unwrap_or(0).max(0), size)
        .await?;
    Ok(PageDto::new(res.data, res.total_count, size))
}

/// 인기 클립 — `filter`: `WITHIN_ONE_DAY` / `WITHIN_SEVEN_DAYS` / `WITHIN_THIRTY_DAYS` / `ALL`,
/// `order`: `POPULAR` / `RECENT`.
#[tauri::command]
pub async fn popular_clips(
    services: State<'_, AppServices>,
    filter: Option<String>,
    order: Option<String>,
) -> CmdResult<Vec<ClipInfo>> {
    Ok(services
        .api
        .home_popular_clips(
            filter.as_deref().unwrap_or("WITHIN_SEVEN_DAYS"),
            order.as_deref().unwrap_or("POPULAR"),
        )
        .await?)
}

/// 다시보기 검색.
#[tauri::command]
pub async fn search_videos(
    services: State<'_, AppServices>,
    keyword: String,
    offset: Option<i64>,
    size: Option<i64>,
) -> CmdResult<PageDto<VodInfo>> {
    let size = size.unwrap_or(24).clamp(1, 50);
    let res = services
        .api
        .search_videos(keyword.trim(), offset.unwrap_or(0).max(0), size)
        .await?;
    Ok(PageDto::new(res.data, res.total_count, size))
}

/// 클립 재생 정보 — 상세 + 재생 URL(있으면).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipPlayDto {
    pub detail: ClipDetail,
    pub stream_url: Option<String>,
}

#[tauri::command]
pub async fn clip_play(
    services: State<'_, AppServices>,
    clip_uid: String,
) -> CmdResult<ClipPlayDto> {
    let detail = services.api.clip_detail(&clip_uid).await?;
    let stream_url = match detail.video_id.as_deref() {
        Some(video_id) if !video_id.is_empty() => services
            .api
            .clip_stream_url(&clip_uid, video_id)
            .await
            .ok()
            .map(|u| u.to_string()),
        _ => None,
    };
    Ok(ClipPlayDto { detail, stream_url })
}

#[tauri::command]
pub async fn vod_detail(
    services: State<'_, AppServices>,
    video_no: i64,
) -> CmdResult<cview_core::models::VodDetail> {
    Ok(services.api.vod_detail(video_no).await?)
}

#[tauri::command]
pub async fn channel_follow(
    services: State<'_, AppServices>,
    channel_id: String,
    follow: bool,
) -> CmdResult<()> {
    if !services.auth.is_authenticated() {
        return Err(CommandError("팔로우하려면 로그인이 필요합니다".into()));
    }
    if follow {
        services.api.follow(&channel_id).await?;
    } else {
        services.api.unfollow(&channel_id).await?;
    }
    Ok(())
}

// MARK: - 개인 데이터

/// 즐겨찾기·최근 시청 한 줄.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedChannelDto {
    pub channel_id: String,
    pub channel_name: String,
    pub channel_image_url: Option<String>,
    pub is_favorite: bool,
    /// 유닉스 밀리초.
    pub last_watched: Option<i64>,
}

impl From<&cview_persistence::models::ChannelListData> for SavedChannelDto {
    fn from(c: &cview_persistence::models::ChannelListData) -> Self {
        Self {
            channel_id: c.channel_id.clone(),
            channel_name: c.channel_name.clone(),
            channel_image_url: c.image_url.clone().filter(|u| !u.is_empty()),
            is_favorite: c.is_favorite,
            last_watched: c.last_watched.map(|d| d.timestamp_millis()),
        }
    }
}

fn channel_stub(channel_id: &str, name: Option<String>, image: Option<String>) -> ChannelInfo {
    let mut info = ChannelInfo::new(channel_id, name.unwrap_or_default());
    info.channel_image_url = image.and_then(|u| url::Url::parse(&u).ok());
    info
}

#[tauri::command]
pub async fn favorite_set(
    services: State<'_, AppServices>,
    channel_id: String,
    favorite: bool,
    channel_name: Option<String>,
    channel_image_url: Option<String>,
) -> CmdResult<bool> {
    let store = services
        .data_store
        .as_ref()
        .ok_or_else(|| CommandError("저장소를 열 수 없습니다".into()))?;
    let info = channel_stub(&channel_id, channel_name, channel_image_url);
    store
        .set_favorite(&channel_id, favorite, Some(&info))
        .map_err(store_err)
}

#[tauri::command]
pub async fn favorites_list(services: State<'_, AppServices>) -> CmdResult<Vec<SavedChannelDto>> {
    let Some(store) = &services.data_store else {
        return Ok(Vec::new());
    };
    Ok(store
        .fetch_favorite_items()
        .map_err(store_err)?
        .iter()
        .map(SavedChannelDto::from)
        .collect())
}

#[tauri::command]
pub async fn recent_channels(
    services: State<'_, AppServices>,
    limit: Option<usize>,
) -> CmdResult<Vec<SavedChannelDto>> {
    let Some(store) = &services.data_store else {
        return Ok(Vec::new());
    };
    Ok(store
        .fetch_recent_items(limit.unwrap_or(20).clamp(1, 100))
        .map_err(store_err)?
        .iter()
        .map(SavedChannelDto::from)
        .collect())
}

/// 최근 시청 기록 지우기.
#[tauri::command]
pub async fn recent_channels_clear(services: State<'_, AppServices>) -> CmdResult<()> {
    if let Some(store) = &services.data_store {
        store.clear_all_last_watched().map_err(store_err)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn memo_set(
    services: State<'_, AppServices>,
    channel_id: String,
    memo: String,
) -> CmdResult<()> {
    let store = services
        .data_store
        .as_ref()
        .ok_or_else(|| CommandError("저장소를 열 수 없습니다".into()))?;
    store.save_memo(&channel_id, memo.trim()).map_err(store_err)
}

/// 스트림 북마크.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkDto {
    pub id: String,
    pub channel_id: String,
    pub channel_name: String,
    pub live_title: String,
    pub category_name: Option<String>,
    pub thumbnail_url: Option<String>,
    /// 유닉스 밀리초.
    pub captured_at: i64,
    /// 북마크 시점의 방송 경과 초.
    pub live_uptime: f64,
    pub note: Option<String>,
}

impl From<&cview_persistence::models::StreamBookmarkData> for BookmarkDto {
    fn from(b: &cview_persistence::models::StreamBookmarkData) -> Self {
        Self {
            id: b.id.clone(),
            channel_id: b.channel_id.clone(),
            channel_name: b.channel_name.clone(),
            live_title: b.live_title.clone(),
            category_name: b.category_name.clone(),
            thumbnail_url: b.thumbnail_url.clone(),
            captured_at: b.captured_at.timestamp_millis(),
            live_uptime: b.live_uptime,
            note: b.note.clone(),
        }
    }
}

/// 지금 방송을 북마크한다(방송 경과 시간 포함).
#[tauri::command]
pub async fn bookmark_add(
    services: State<'_, AppServices>,
    channel_id: String,
    note: Option<String>,
) -> CmdResult<BookmarkDto> {
    let store = services
        .data_store
        .as_ref()
        .ok_or_else(|| CommandError("저장소를 열 수 없습니다".into()))?;
    let live = services.api.live_detail(&channel_id, false).await?;
    let uptime = live
        .open_date
        .map(|d| (Utc::now() - d).num_milliseconds() as f64 / 1000.0)
        .unwrap_or(0.0)
        .max(0.0);
    let name = live
        .channel
        .as_ref()
        .map(|c| c.channel_name.clone())
        .unwrap_or_default();
    let thumb = live.resolved_live_image_url().map(|u| u.to_string());
    let saved = store
        .save_bookmark(
            &channel_id,
            &name,
            &live.live_title,
            live.live_category_value.as_deref(),
            thumb.as_deref(),
            uptime,
            note.as_deref().filter(|n| !n.trim().is_empty()),
        )
        .map_err(store_err)?;
    Ok(BookmarkDto::from(&saved))
}

#[tauri::command]
pub async fn bookmarks_list(services: State<'_, AppServices>) -> CmdResult<Vec<BookmarkDto>> {
    let Some(store) = &services.data_store else {
        return Ok(Vec::new());
    };
    Ok(store
        .fetch_bookmarks(200)
        .map_err(store_err)?
        .iter()
        .map(BookmarkDto::from)
        .collect())
}

#[tauri::command]
pub async fn bookmark_delete(services: State<'_, AppServices>, id: String) -> CmdResult<()> {
    if let Some(store) = &services.data_store {
        store.delete_bookmark(&id).map_err(store_err)?;
    }
    Ok(())
}

// MARK: - 파일 저장

/// 저장 종류별 기본 폴더(사용자 폴더 아래). 스크린샷은 설정 `screenshotPath` 가 있으면 그곳.
pub fn default_save_dir(home: &Path, kind: &str) -> PathBuf {
    match kind {
        "screenshot" => home.join("Pictures").join("CView"),
        "recording" => home.join("Movies").join("CView"),
        _ => home.join("Downloads").join("CView"),
    }
}

/// 파일 이름 정리 — 경로 구분자·제어 문자를 지우고 길이를 줄인다.
pub fn sanitize_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_start_matches('.').to_string();
    let limited: String = trimmed.chars().take(120).collect();
    if limited.is_empty() {
        "cview".into()
    } else {
        limited
    }
}

/// 같은 이름이 있으면 ` (2)` 를 붙인다.
fn unique_path(dir: &Path, file_name: &str) -> PathBuf {
    let candidate = dir.join(file_name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match file_name.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (file_name.to_string(), String::new()),
    };
    (2..1000)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .unwrap_or(candidate)
}

/// 파일 저장 — `data` 는 base64(바이너리) 또는 `text`(문자열) 중 하나. 저장한 경로를 돌려준다.
#[tauri::command]
pub async fn save_file(
    services: State<'_, AppServices>,
    kind: String,
    file_name: String,
    data_base64: Option<String>,
    text: Option<String>,
) -> CmdResult<String> {
    let bytes = match (data_base64, text) {
        (Some(b64), _) => {
            let raw = b64.split_once(',').map(|(_, d)| d).unwrap_or(&b64);
            base64::engine::general_purpose::STANDARD
                .decode(raw.trim())
                .map_err(|e| CommandError(format!("파일 데이터가 올바르지 않습니다: {e}")))?
        }
        (None, Some(t)) => t.into_bytes(),
        (None, None) => return Err(CommandError("저장할 내용이 없습니다".into())),
    };
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let configured = (kind == "screenshot")
        .then(|| services.settings.player().screenshot_path)
        .filter(|p| !p.trim().is_empty())
        .map(|p| PathBuf::from(shellexpand_home(&p, &home)));
    let dir = configured.unwrap_or_else(|| default_save_dir(&home, &kind));
    std::fs::create_dir_all(&dir)
        .map_err(|e| CommandError(format!("폴더를 만들 수 없습니다: {e}")))?;
    let path = unique_path(&dir, &sanitize_file_name(&file_name));
    std::fs::write(&path, bytes).map_err(|e| CommandError(format!("저장하지 못했습니다: {e}")))?;
    Ok(path.to_string_lossy().into_owned())
}

fn shellexpand_home(path: &str, home: &Path) -> String {
    match path.strip_prefix("~/") {
        Some(rest) => home.join(rest).to_string_lossy().into_owned(),
        None => path.to_string(),
    }
}

/// Finder(탐색기)에서 파일·폴더를 보여 준다.
#[tauri::command]
pub async fn reveal_path(path: String) -> CmdResult<()> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(CommandError("파일이 없습니다".into()));
    }
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg("-R").arg(&p).spawn();
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("explorer")
        .arg(format!("/select,{}", p.display()))
        .spawn();
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let result = std::process::Command::new("xdg-open")
        .arg(p.parent().unwrap_or(&p))
        .spawn();
    result
        .map(|_| ())
        .map_err(|e| CommandError(format!("열 수 없습니다: {e}")))
}

/// 외부 브라우저로 링크를 연다(치지직에서 열기 등). http(s) 만 허용한다.
#[tauri::command]
pub async fn open_external(url: String) -> CmdResult<()> {
    let parsed = url::Url::parse(&url).map_err(|_| CommandError("잘못된 주소입니다".into()))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(CommandError("http(s) 주소만 열 수 있습니다".into()));
    }
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open")
        .arg(parsed.as_str())
        .spawn();
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("cmd")
        .args(["/C", "start", "", parsed.as_str()])
        .spawn();
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let result = std::process::Command::new("xdg-open")
        .arg(parsed.as_str())
        .spawn();
    result
        .map(|_| ())
        .map_err(|e| CommandError(format!("열 수 없습니다: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_has_more_follows_total_or_size() {
        assert!(PageDto::new(vec![1, 2], Some(10), 2).has_more);
        assert!(!PageDto::new(vec![1, 2], Some(2), 2).has_more);
        assert!(!PageDto::new(vec![1], None, 2).has_more);
        assert!(PageDto::new(vec![1, 2], None, 2).has_more);
    }

    #[test]
    fn file_names_are_sanitized() {
        assert_eq!(sanitize_file_name("a/b:c?.png"), "a_b_c_.png");
        assert_eq!(sanitize_file_name("..hidden"), "hidden");
        assert_eq!(sanitize_file_name("   "), "cview");
        assert_eq!(sanitize_file_name(&"가".repeat(300)).chars().count(), 120);
    }

    #[test]
    fn save_dirs_and_unique_names() {
        let home = Path::new("/Users/x");
        assert_eq!(
            default_save_dir(home, "screenshot"),
            home.join("Pictures/CView")
        );
        assert_eq!(default_save_dir(home, "chat"), home.join("Downloads/CView"));
        let dir = std::env::temp_dir().join(format!("cview-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        assert_eq!(unique_path(&dir, "a.txt"), dir.join("a (2).txt"));
        assert_eq!(unique_path(&dir, "b.txt"), dir.join("b.txt"));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(shellexpand_home("~/Pics", home), "/Users/x/Pics");
    }
}
