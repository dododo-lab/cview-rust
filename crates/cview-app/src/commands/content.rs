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
    /// 다음 페이지가 있다 — 전체 개수를 알면 `시작 위치 + 받은 개수 < 전체`, 모르면 요청한 크기만큼 왔는지로 추정.
    pub has_more: bool,
}

impl<T> PageDto<T> {
    /// `start` 는 이 페이지 앞에 있는 항목 수(offset, 또는 page × size).
    pub fn new(items: Vec<T>, total_count: Option<i64>, start: i64, requested: i64) -> Self {
        let received = items.len() as i64;
        let has_more = match total_count {
            // 빈 페이지면 total 이 남아 있어도 멈춘다(오래된 total 로 빈 요청을 되풀이하지 않게).
            Some(total) => received > 0 && start.saturating_add(received) < total,
            None => received >= requested,
        };
        Self {
            items,
            total_count,
            has_more,
        }
    }
}

/// 0부터 세는 페이지 번호의 시작 위치.
fn page_start(page: i64, size: i64) -> i64 {
    page.saturating_mul(size)
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
    let page = page.unwrap_or(0).max(0);
    let res = services
        .api
        .vod_list(&channel_id, page, size, false)
        .await?;
    Ok(PageDto::new(
        res.data,
        res.total_count,
        page_start(page, size),
        size,
    ))
}

#[tauri::command]
pub async fn channel_clips(
    services: State<'_, AppServices>,
    channel_id: String,
    page: Option<i64>,
    size: Option<i64>,
) -> CmdResult<PageDto<ClipInfo>> {
    let size = size.unwrap_or(24).clamp(1, 50);
    let page = page.unwrap_or(0).max(0);
    let res = services.api.clip_list(&channel_id, page, size).await?;
    Ok(PageDto::new(
        res.data,
        res.total_count,
        page_start(page, size),
        size,
    ))
}

/// 인기 클립 — `filter`: `WITHIN_1_DAY` / `WITHIN_7_DAYS` / `WITHIN_30_DAYS`,
/// `order`: `POPULAR` / `RECOMMEND` (원본 ClipBrowserSupport.apiValue).
#[tauri::command]
pub async fn popular_clips(
    services: State<'_, AppServices>,
    filter: Option<String>,
    order: Option<String>,
) -> CmdResult<Vec<ClipInfo>> {
    Ok(services
        .api
        .home_popular_clips(
            filter.as_deref().unwrap_or("WITHIN_7_DAYS"),
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
    let offset = offset.unwrap_or(0).max(0);
    let res = services
        .api
        .search_videos(keyword.trim(), offset, size)
        .await?;
    Ok(PageDto::new(res.data, res.total_count, offset, size))
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
    // 인키 경로가 막히면 상세의 재생 주소(videoPlayUrl → videoUrl)를 쓴다.
    let stream_url = stream_url.or_else(|| {
        detail
            .video_play_url
            .clone()
            .or_else(|| detail.video_url.clone())
            .filter(|u| u.starts_with("http"))
    });
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

// MARK: - 팔로잉 전체·다시보기 피드

/// 팔로우 채널 한 줄(오프라인 포함).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowedChannelDto {
    pub channel_id: String,
    pub channel_name: String,
    pub channel_image_url: Option<String>,
    pub is_live: bool,
    pub live_title: Option<String>,
    pub viewer_count: Option<i64>,
    pub category_name: Option<String>,
}

/// 팔로우한 채널 전체 — 원본 FollowingChannelSidebar/DenseRow 의 바탕.
#[tauri::command]
pub async fn following_channels_all(
    services: State<'_, AppServices>,
    refresh: Option<bool>,
) -> CmdResult<Vec<FollowedChannelDto>> {
    if !services.auth.is_authenticated() {
        return Err(CommandError("로그인이 필요합니다".into()));
    }
    let page = services
        .api
        .fetch_following_rows(refresh.unwrap_or(false))
        .await?;
    Ok(page
        .rows
        .iter()
        .map(|r| {
            let live = r.list_live_info.as_ref();
            FollowedChannelDto {
                channel_id: r.channel_id.clone(),
                channel_name: r.channel_name.clone(),
                channel_image_url: r.channel_image_url.clone().filter(|u| !u.is_empty()),
                is_live: r.is_live,
                live_title: live.and_then(|l| l.live_title.clone()),
                viewer_count: live.and_then(|l| l.concurrent_user_count),
                category_name: live.and_then(|l| l.live_category_value.clone()),
            }
        })
        .collect())
}

/// 팔로우 채널 최신 다시보기를 모아 최신순으로 — 원본 VODFeedAggregator(간소화).
#[tauri::command]
pub async fn following_vods(
    services: State<'_, AppServices>,
    channels: Option<usize>,
    per_channel: Option<i64>,
) -> CmdResult<Vec<VodInfo>> {
    if !services.auth.is_authenticated() {
        return Err(CommandError("로그인이 필요합니다".into()));
    }
    let page = services.api.fetch_following_rows(false).await?;
    let ids: Vec<String> = page
        .rows
        .iter()
        .take(channels.unwrap_or(24).clamp(1, 60))
        .map(|r| r.channel_id.clone())
        .collect();
    let per = per_channel.unwrap_or(3).clamp(1, 10);
    let mut all = Vec::new();
    for chunk in ids.chunks(6) {
        let futs = chunk
            .iter()
            .map(|id| services.api.vod_list(id, 0, per, true));
        for res in futures_util::future::join_all(futs)
            .await
            .into_iter()
            .flatten()
        {
            all.extend(res.data);
        }
    }
    all.sort_by_key(|v| std::cmp::Reverse(v.publish_date));
    Ok(all)
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

/// 같은 이름 뒤에 붙여 볼 번호의 끝(` (2)` … ` (999)`).
const UNIQUE_NAME_LIMIT: u32 = 1000;

/// 같은 이름이 없을 때까지 ` (2)` 를 붙여 가며 **새 파일을 원자적으로 만든다**(`create_new`).
/// 있는지 확인하고 나서 쓰는 방식과 달리 동시에 저장해도 같은 이름을 고르지 않고, 번호를 다 써도
/// 기존 파일로 돌아가 덮어쓰지 않고 `AlreadyExists` 오류로 끝난다.
pub(crate) fn create_unique_file(
    dir: &Path,
    file_name: &str,
) -> std::io::Result<(PathBuf, std::fs::File)> {
    let (stem, ext) = match file_name.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (file_name.to_string(), String::new()),
    };
    let candidates = std::iter::once(dir.join(file_name))
        .chain((2..UNIQUE_NAME_LIMIT).map(|n| dir.join(format!("{stem} ({n}){ext}"))));
    for path in candidates {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "같은 이름의 파일이 너무 많습니다",
    ))
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
    let (path, mut file) = create_unique_file(&dir, &sanitize_file_name(&file_name))
        .map_err(|e| CommandError(format!("저장하지 못했습니다: {e}")))?;
    if let Err(e) = std::io::Write::write_all(&mut file, &bytes) {
        drop(file);
        // 방금 새로 만든 파일이라 지워도 다른 내용을 잃지 않는다.
        let _ = std::fs::remove_file(&path);
        return Err(CommandError(format!("저장하지 못했습니다: {e}")));
    }
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

/// 기본 앱으로 열어도 되는 파일 — 받은 영상·녹화·스크린샷·내보낸 채팅. 웹뷰가 넘긴 경로로 앱 번들·스크립트·
/// 실행 파일을 실행하지 못하게, 일반 파일이고 이 확장자일 때만 연다(그 밖은 Finder 에서 보이기를 쓴다).
const OPENABLE_EXTENSIONS: &[&str] = &[
    "mp4", "m4v", "mov", "mkv", "webm", "ts", "m4a", "aac", "mp3", "png", "jpg", "jpeg", "gif",
    "webp", "txt", "json", "csv",
];

/// 순수 함수 — 열어도 되는 파일인지(일반 파일 + 허용 확장자).
pub fn is_openable_file(p: &std::path::Path) -> bool {
    let is_file = std::fs::symlink_metadata(p).is_ok_and(|m| m.is_file());
    let ext_ok = p
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| OPENABLE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()));
    is_file && ext_ok
}

/// 받은 영상 등 미디어 파일을 기본 앱으로 연다. 폴더·앱·실행 파일은 열지 않는다.
#[tauri::command]
pub async fn open_path(path: String) -> CmdResult<()> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(CommandError("파일이 없습니다".into()));
    }
    if !is_openable_file(&p) {
        return Err(CommandError(
            "이 파일은 열 수 없습니다 — Finder 에서 보기를 쓰세요".into(),
        ));
    }
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(&p).spawn();
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("explorer").arg(&p).spawn();
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let result = std::process::Command::new("xdg-open").arg(&p).spawn();
    result
        .map(|_| ())
        .map_err(|e| CommandError(format!("열 수 없습니다: {e}")))
}

/// 시스템 알림 설정 열기(SYS-19 도움말 액션).
#[tauri::command]
pub async fn open_notification_settings() -> CmdResult<()> {
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.Notifications-Settings.extension")
        .spawn();
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("explorer")
        .arg("ms-settings:notifications")
        .spawn();
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let result: std::io::Result<std::process::Child> =
        Err(std::io::Error::other("지원하지 않는 환경"));
    result
        .map(|_| ())
        .map_err(|e| CommandError(format!("알림 설정을 열 수 없습니다: {e}")))
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
    // cmd /C start 는 URL 의 & | 를 명령 구분자로 읽어 주입이 된다 — 셸을 거치지 않는 URL 처리기로.
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", parsed.as_str()])
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
    fn openable_files_only_media_regular_files() {
        let dir = tempfile::tempdir().unwrap();
        let mp4 = dir.path().join("a.MP4");
        std::fs::write(&mp4, b"x").unwrap();
        assert!(is_openable_file(&mp4));
        let sh = dir.path().join("run.command");
        std::fs::write(&sh, b"x").unwrap();
        assert!(!is_openable_file(&sh));
        let app = dir.path().join("Evil.app");
        std::fs::create_dir(&app).unwrap();
        assert!(!is_openable_file(&app));
        let fake = dir.path().join("dir.mp4");
        std::fs::create_dir(&fake).unwrap();
        assert!(!is_openable_file(&fake));
    }

    #[test]
    fn page_has_more_follows_total_or_size() {
        let full = |n: usize| vec![0; n];
        // 0개
        assert!(!PageDto::new(full(0), Some(0), 0, 24).has_more);
        assert!(!PageDto::new(full(0), None, 0, 24).has_more);
        // 한 페이지로 끝
        assert!(!PageDto::new(full(10), Some(10), 0, 24).has_more);
        assert!(!PageDto::new(full(24), Some(24), 0, 24).has_more);
        // 정확히 2페이지(48개) — 첫 페이지는 더 있고, 꽉 찬 마지막 페이지는 끝
        assert!(PageDto::new(full(24), Some(48), page_start(0, 24), 24).has_more);
        assert!(!PageDto::new(full(24), Some(48), page_start(1, 24), 24).has_more);
        // 마지막 일부 페이지(50개 → 24, 24, 2)
        assert!(PageDto::new(full(24), Some(50), page_start(1, 24), 24).has_more);
        assert!(!PageDto::new(full(2), Some(50), page_start(2, 24), 24).has_more);
        // offset 기반(검색)
        assert!(PageDto::new(full(24), Some(30), 0, 24).has_more);
        assert!(!PageDto::new(full(6), Some(30), 24, 24).has_more);
        // total 이 남아 있어도 빈 페이지면 멈춘다
        assert!(!PageDto::new(full(0), Some(100), 48, 24).has_more);
        // total 없는 응답 — 요청한 크기만큼 왔는지로 추정
        assert!(PageDto::new(full(24), None, 24, 24).has_more);
        assert!(!PageDto::new(full(5), None, 24, 24).has_more);
        // 큰 페이지 번호도 넘치지 않는다
        assert_eq!(page_start(i64::MAX, 50), i64::MAX);
        assert!(!PageDto::new(full(1), Some(10), page_start(i64::MAX, 50), 50).has_more);
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
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        assert_eq!(
            create_unique_file(dir, "a.txt").unwrap().0,
            dir.join("a (2).txt")
        );
        assert_eq!(
            create_unique_file(dir, "a.txt").unwrap().0,
            dir.join("a (3).txt")
        );
        assert_eq!(
            create_unique_file(dir, "b.txt").unwrap().0,
            dir.join("b.txt")
        );
        assert_eq!(
            create_unique_file(dir, "noext").unwrap().0,
            dir.join("noext")
        );
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "x");
        assert_eq!(shellexpand_home("~/Pics", home), "/Users/x/Pics");
    }

    #[test]
    fn exhausted_names_fail_without_touching_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        std::fs::write(dir.join("a.txt"), "원본").unwrap();
        for n in 2..UNIQUE_NAME_LIMIT {
            std::fs::write(dir.join(format!("a ({n}).txt")), format!("{n}")).unwrap();
        }
        let err = create_unique_file(dir, "a.txt").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "원본");
        assert_eq!(
            std::fs::read_to_string(dir.join("a (999).txt")).unwrap(),
            "999"
        );
        assert_eq!(std::fs::read_dir(dir).unwrap().count(), 999);
    }

    #[test]
    fn concurrent_saves_get_distinct_new_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "원본").unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(16));
        let handles: Vec<_> = (0..16)
            .map(|i| {
                let dir = dir.path().to_path_buf();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let (path, mut file) = create_unique_file(&dir, "a.txt").unwrap();
                    std::io::Write::write_all(&mut file, format!("{i}").as_bytes()).unwrap();
                    (path, i)
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let paths: std::collections::HashSet<_> = results.iter().map(|(p, _)| p.clone()).collect();
        assert_eq!(paths.len(), 16);
        assert!(!paths.contains(&dir.path().join("a.txt")));
        for (path, i) in &results {
            assert_eq!(std::fs::read_to_string(path).unwrap(), format!("{i}"));
        }
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "원본"
        );
    }
}
