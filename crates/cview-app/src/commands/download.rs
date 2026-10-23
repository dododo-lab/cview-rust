//! VOD·클립 다운로드 — 원본 VideoDownloadService / VideoDownloadButton / DownloadsLibraryView / DownloadHistoryStore.
//!
//! 작업 큐는 셸이 들고, 실제 받기는 `cview_player::download::VideoDownloader` 가 한다. 단일 MP4(PD) 주소가
//! 있으면 그것을, 없으면 HLS 세그먼트를 이어 받는다. 진행 상황은 `download://update` 로 작업 목록 전체를 보낸다.
//! 끝난 작업(완료·실패)은 다운로드 기록 파일에 남기고, 기록은 보관함 "다운로드" 탭이 보여 준다.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use cview_app_core::services::download_history_store::{
    DownloadHistoryEvent, DownloadHistoryStore,
};
use cview_app_core::services::media_download::{trim_finished_jobs, DownloadJob, DownloadJobState};
use cview_persistence::download_history::{DownloadHistoryFileStatus, DownloadState};
use cview_player::download::{DownloadError, VideoDownloader};
use cview_player::vod::{resolve_clip_streams, VodStreamResolver};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use url::Url;

use super::content::{create_unique_file, sanitize_file_name};
use super::{CmdResult, CommandError};
use crate::state::AppServices;

pub const DOWNLOAD_UPDATE: &str = "download://update";

struct Entry {
    job: DownloadJob,
    kind: String,
    target: String,
    bytes: u64,
    cancel: Arc<AtomicBool>,
}

/// 셸이 들고 있는 다운로드 큐.
pub struct Downloads {
    entries: Mutex<Vec<Entry>>,
    pub history: DownloadHistoryStore,
    folder: PathBuf,
}

impl Downloads {
    pub fn new(data_dir: &std::path::Path) -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let folder =
            cview_app_core::services::media_download::default_download_folder(home.as_deref());
        let roots: Vec<PathBuf> = home
            .iter()
            .flat_map(|h| [h.join("Movies"), h.join("Downloads")])
            .collect();
        Self {
            entries: Mutex::new(Vec::new()),
            history: DownloadHistoryStore::new(
                Some(
                    data_dir.join(
                        cview_persistence::download_history::DownloadHistoryDocument::FILE_NAME,
                    ),
                ),
                roots,
                None,
            ),
            folder,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Entry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobDto {
    pub id: String,
    pub kind: String,
    pub target: String,
    pub title: String,
    pub channel_name: Option<String>,
    pub progress: f64,
    pub bytes: u64,
    /// queued · preparing · downloading · assembling · completed · failed · cancelled
    pub state: String,
    pub error: Option<String>,
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryDto {
    pub id: String,
    pub title: String,
    pub channel_name: Option<String>,
    pub completed: bool,
    pub failure_reason: Option<String>,
    pub file_path: Option<String>,
    pub file_size: Option<i64>,
    pub finished_at: String,
    /// present · missing · unverified (완료 행만)
    pub file_status: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadsDto {
    pub jobs: Vec<JobDto>,
    pub history: Vec<HistoryDto>,
    pub folder: String,
}

fn job_dto(e: &Entry) -> JobDto {
    let (state, error, file_path) = match &e.job.state {
        DownloadJobState::Queued => ("queued", None, None),
        DownloadJobState::Preparing => ("preparing", None, None),
        DownloadJobState::Downloading => ("downloading", None, None),
        DownloadJobState::Assembling => ("assembling", None, None),
        DownloadJobState::Completed(p) => ("completed", None, Some(p.display().to_string())),
        DownloadJobState::Failed(m) => ("failed", Some(m.clone()), None),
        DownloadJobState::Cancelled => ("cancelled", None, None),
    };
    JobDto {
        id: e.job.id.clone(),
        kind: e.kind.clone(),
        target: e.target.clone(),
        title: e.job.title.clone(),
        channel_name: e.job.channel_name.clone(),
        progress: e.job.progress,
        bytes: e.bytes,
        state: state.into(),
        error,
        file_path,
    }
}

fn snapshot(d: &Downloads) -> DownloadsDto {
    let jobs = d.lock().iter().map(job_dto).collect();
    let status = d.history.file_status();
    let history = d
        .history
        .entries()
        .into_iter()
        .map(|h| HistoryDto {
            file_status: status.get(&h.id).map(|s| {
                match s {
                    DownloadHistoryFileStatus::Present => "present",
                    DownloadHistoryFileStatus::Missing => "missing",
                    DownloadHistoryFileStatus::Unverified => "unverified",
                }
                .to_string()
            }),
            completed: h.state == DownloadState::Completed,
            id: h.id,
            title: h.title,
            channel_name: h.channel_name,
            failure_reason: h.failure_reason,
            file_path: h.file_path,
            file_size: h.file_size,
            finished_at: h.finished_at.to_rfc3339(),
        })
        .collect();
    DownloadsDto {
        jobs,
        history,
        folder: d.folder.display().to_string(),
    }
}

fn emit(app: &AppHandle) {
    if let Some(d) = app.try_state::<Downloads>() {
        let _ = app.emit(DOWNLOAD_UPDATE, snapshot(&d));
    }
}

fn update(app: &AppHandle, id: &str, f: impl FnOnce(&mut Entry)) {
    if let Some(d) = app.try_state::<Downloads>() {
        if let Some(e) = d.lock().iter_mut().find(|e| e.job.id == id) {
            f(e);
        }
    }
}

#[tauri::command]
pub async fn download_list(downloads: State<'_, Downloads>) -> CmdResult<DownloadsDto> {
    downloads.history.hydrate().await;
    downloads
        .history
        .refresh_file_status_if_stale(DownloadHistoryStore::DEFAULT_VERIFY_MAX_AGE);
    Ok(snapshot(&downloads))
}

/// 다운로드 시작 — `kind`: vod(target = videoNo) · clip(target = clipUID).
#[tauri::command]
pub async fn download_start(
    app: AppHandle,
    downloads: State<'_, Downloads>,
    kind: String,
    target: String,
    title: Option<String>,
    channel_name: Option<String>,
) -> CmdResult<DownloadsDto> {
    if kind != "vod" && kind != "clip" {
        return Err(CommandError(format!("알 수 없는 다운로드 종류: {kind}")));
    }
    let id = format!("{kind}-{target}");
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut list = downloads.lock();
        if list.iter().any(|e| e.job.id == id && e.job.is_active()) {
            return Err(CommandError("이미 받는 중입니다".into()));
        }
        list.retain(|e| e.job.id != id);
        list.push(Entry {
            job: DownloadJob {
                id: id.clone(),
                title: title.clone().unwrap_or_else(|| target.clone()),
                channel_name: channel_name.clone(),
                progress: 0.0,
                state: DownloadJobState::Queued,
            },
            kind: kind.clone(),
            target: target.clone(),
            bytes: 0,
            cancel: cancel.clone(),
        });
        let mut jobs: Vec<DownloadJob> = list.iter().map(|e| e.job.clone()).collect();
        let dropped = trim_finished_jobs(&mut jobs);
        list.retain(|e| !dropped.contains(&e.job.id));
    }
    let folder = downloads.folder.clone();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = run_job(&handle, &id, &kind, &target, &folder, cancel).await;
        let Some(d) = handle.try_state::<Downloads>() else {
            return;
        };
        let (title, channel) = d
            .lock()
            .iter()
            .find(|e| e.job.id == id)
            .map(|e| (e.job.title.clone(), e.job.channel_name.clone()))
            .unwrap_or_default();
        match result {
            Ok(path) => {
                update(&handle, &id, |e| {
                    e.job.progress = 1.0;
                    e.job.state = DownloadJobState::Completed(path.clone());
                });
                d.history.record(DownloadHistoryEvent::Completed {
                    id: id.clone(),
                    title,
                    channel_name: channel,
                    file: path,
                });
            }
            Err(DownloadError::Cancelled) => {
                update(&handle, &id, |e| e.job.state = DownloadJobState::Cancelled);
            }
            Err(err) => {
                let reason = err.to_string();
                update(&handle, &id, |e| {
                    e.job.state = DownloadJobState::Failed(reason.clone());
                });
                d.history.record(DownloadHistoryEvent::Failed {
                    id: id.clone(),
                    title,
                    channel_name: channel,
                    reason,
                });
            }
        }
        emit(&handle);
    });
    emit(&app);
    Ok(snapshot(&downloads))
}

async fn run_job(
    app: &AppHandle,
    id: &str,
    kind: &str,
    target: &str,
    folder: &std::path::Path,
    cancel: Arc<AtomicBool>,
) -> Result<PathBuf, DownloadError> {
    update(app, id, |e| e.job.state = DownloadJobState::Preparing);
    emit(app);
    let unresolved =
        |e: String| DownloadError::Network(format!("다운로드 주소를 가져오지 못했습니다: {e}"));
    // (제목, 채널, 단일 MP4, HLS)
    let (title, channel, progressive, hls): (String, Option<String>, Option<Url>, Url) =
        if kind == "vod" {
            let video_no: i64 = target
                .parse()
                .map_err(|_| unresolved("잘못된 영상 번호".into()))?;
            let services = app.state::<AppServices>();
            let s = VodStreamResolver::new(services.api.clone())
                .resolve_sources(video_no)
                .await
                .map_err(|e| unresolved(e.to_string()))?;
            let channel = Some(s.info.channel_name.clone()).filter(|c| !c.is_empty());
            (
                s.info.title.clone(),
                channel,
                s.progressive.first().map(|q| q.url.clone()),
                s.info.stream_url.clone(),
            )
        } else {
            let n = resolve_clip_streams(target)
                .await
                .map_err(|e| unresolved(e.to_string()))?;
            let (title, channel) = app
                .try_state::<Downloads>()
                .and_then(|d| {
                    d.lock()
                        .iter()
                        .find(|e| e.job.id == id)
                        .map(|e| (e.job.title.clone(), e.job.channel_name.clone()))
                })
                .unwrap_or_default();
            (
                title,
                channel,
                n.progressive.first().map(|q| q.url.clone()),
                n.master,
            )
        };
    update(app, id, |e| {
        if e.job.title == e.target || e.job.title.is_empty() {
            e.job.title = title.clone();
        }
        if e.job.channel_name.is_none() {
            e.job.channel_name = channel.clone();
        }
        e.job.state = DownloadJobState::Downloading;
    });
    emit(app);

    let ext = if progressive.is_some() { "mp4" } else { "ts" };
    let name = sanitize_file_name(&match &channel {
        Some(c) => format!("{c}_{title}_{target}.{ext}"),
        None => format!("{title}_{target}.{ext}"),
    });
    // 최종 이름을 빈 파일로 먼저 예약한다 — 동시에 받는 같은 이름 작업이나 기존 파일을 `.part` → 이름
    // 바꾸기로 덮어쓰지 않게. 받기에 실패하면 예약한 빈 파일을 지운다.
    std::fs::create_dir_all(folder).map_err(|e| DownloadError::WriteFailed(e.to_string()))?;
    let (output, reserved) =
        create_unique_file(folder, &name).map_err(|e| DownloadError::WriteFailed(e.to_string()))?;
    drop(reserved);
    let last_emit = Arc::new(Mutex::new(Instant::now()));
    let handle = app.clone();
    let job_id = id.to_string();
    let progress = Arc::new(move |p: f64, bytes: u64| {
        update(&handle, &job_id, |e| {
            if e.job.accepts_progress(p) {
                e.job.progress = p.clamp(0.0, 1.0);
                e.bytes = bytes;
            }
        });
        let mut last = last_emit.lock().unwrap_or_else(|e| e.into_inner());
        if last.elapsed().as_millis() >= 400 {
            *last = Instant::now();
            emit(&handle);
        }
    });
    let downloader = VideoDownloader::new(cancel);
    let result = match &progressive {
        Some(url) => downloader.download_file(url, &output, progress).await,
        None => downloader.download_hls(&hls, &output, progress).await,
    };
    match result {
        Ok(_) => Ok(output),
        Err(e) => {
            let _ = std::fs::remove_file(&output);
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn download_cancel(
    app: AppHandle,
    downloads: State<'_, Downloads>,
    id: String,
) -> CmdResult<DownloadsDto> {
    if let Some(e) = downloads.lock().iter().find(|e| e.job.id == id) {
        e.cancel.store(true, Ordering::Relaxed);
    }
    emit(&app);
    Ok(snapshot(&downloads))
}

/// 끝난 작업과 기록에서 지운다(`ids` 가 없으면 끝난 것 전부). 파일은 지우지 않는다.
#[tauri::command]
pub async fn download_clear(
    app: AppHandle,
    downloads: State<'_, Downloads>,
    ids: Option<Vec<String>>,
) -> CmdResult<DownloadsDto> {
    let set: Option<HashSet<String>> = ids.map(|v| v.into_iter().collect());
    let wanted = |id: &str| set.as_ref().is_none_or(|s| s.contains(id));
    downloads
        .lock()
        .retain(|e| e.job.is_active() || !wanted(&e.job.id));
    let history_ids: HashSet<String> = downloads
        .history
        .entries()
        .into_iter()
        .map(|h| h.id)
        .filter(|id| wanted(id))
        .collect();
    downloads.history.remove(&history_ids);
    emit(&app);
    Ok(snapshot(&downloads))
}

/// 진행 중 작업 수(메뉴 막대 배지 등).
pub fn active_count(app: &AppHandle) -> usize {
    app.try_state::<Downloads>()
        .map(|d| d.lock().iter().filter(|e| e.job.is_active()).count())
        .unwrap_or(0)
}
