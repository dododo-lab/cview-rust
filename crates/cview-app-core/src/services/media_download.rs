//! `Sources/CViewApp/Services/MediaDownloadActions.swift` 와 `VideoDownloadService.swift` 의 값 부분 포팅.
//!
//! 옮긴 것: 다운로드 종류와 job id 규칙([`MediaDownloadKind`]), job 상태와 활성 판정, 종료된 job 보관
//! 상한(30, 오래된 것부터), 진행 콜백 수용 규칙, 저장 폴더 키·기본 폴더, 오류 문구.
//!
//! 건너뛴 것: 실제 다운로더(`VideoDownloader` — HLS 재생목록 파싱·AES-128 세그먼트 복호화·직접 다운로드·
//! 부분 파일 처리), job 큐 실행, Finder 열기·완료 알림, SwiftUI 컨텍스트 메뉴. 워크스페이스에 AES 크레이트가
//! 없고 큐는 셸의 파일 대화상자·알림과 묶여 있어 셸(Tauri) 쪽 몫으로 남긴다.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// 다운로드 대상.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MediaDownloadKind {
    Vod { video_no: i64 },
    Clip { clip_uid: String },
}

impl MediaDownloadKind {
    pub fn job_id(&self) -> String {
        match self {
            Self::Vod { video_no } => format!("vod-{video_no}"),
            Self::Clip { clip_uid } => format!("clip-{clip_uid}"),
        }
    }

    /// 플레이어 다운로드 job id 를 재해석 경로로 되돌린다 — VOD 만(클립은 받아 둔 URL 유지).
    pub fn from_re_resolvable_job_id(id: &str) -> Option<Self> {
        let video_no = id.strip_prefix("vod-")?.parse().ok()?;
        Some(Self::Vod { video_no })
    }

    /// 다운로드 기록 행의 id 를 '다시 받기' 경로로 되돌린다 — VOD·클립 둘 다.
    pub fn from_history_job_id(id: &str) -> Option<Self> {
        if let Some(vod) = Self::from_re_resolvable_job_id(id) {
            return Some(vod);
        }
        let uid = id.strip_prefix("clip-").filter(|u| !u.is_empty())?;
        Some(Self::Clip {
            clip_uid: uid.to_string(),
        })
    }
}

/// job 상태. 사용자가 누른 취소는 실패가 아니다(`Cancelled`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadJobState {
    Queued,
    Preparing,
    Downloading,
    Assembling,
    Completed(PathBuf),
    Failed(String),
    Cancelled,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DownloadJob {
    pub id: String,
    pub title: String,
    pub channel_name: Option<String>,
    /// 종료 전이에서만 바뀐다(완료 = 1).
    pub progress: f64,
    pub state: DownloadJobState,
}

impl DownloadJob {
    pub fn is_active(&self) -> bool {
        matches!(
            self.state,
            DownloadJobState::Queued
                | DownloadJobState::Preparing
                | DownloadJobState::Downloading
                | DownloadJobState::Assembling
        )
    }

    /// '다시 시도' 를 보일 상태인가(실패·취소).
    pub fn is_retryable_state(&self) -> bool {
        matches!(
            self.state,
            DownloadJobState::Failed(_) | DownloadJobState::Cancelled
        )
    }

    /// 진행 콜백을 받아도 되는가 — 종료된 job·결합 중인 job 은 되살리지 않는다.
    pub fn accepts_progress(&self, progress: f64) -> bool {
        progress.is_finite() && self.is_active() && self.state != DownloadJobState::Assembling
    }
}

/// 보관하는 **종료된** job 수 상한. 진행 중 job 은 버리지 않는다.
pub const MAX_FINISHED_JOBS: usize = 30;

/// 종료된 job 이 상한을 넘으면 오래된 것(앞쪽)부터 버린다. 버린 id 를 돌려준다.
pub fn trim_finished_jobs(jobs: &mut Vec<DownloadJob>) -> HashSet<String> {
    let finished: Vec<String> = jobs
        .iter()
        .filter(|j| !j.is_active())
        .map(|j| j.id.clone())
        .collect();
    if finished.len() <= MAX_FINISHED_JOBS {
        return HashSet::new();
    }
    let drop: HashSet<String> = finished
        .into_iter()
        .take(jobs.iter().filter(|j| !j.is_active()).count() - MAX_FINISHED_JOBS)
        .collect();
    jobs.retain(|j| !drop.contains(&j.id));
    drop
}

/// 저장 폴더 키(사용자 지정 경로).
pub const DOWNLOAD_FOLDER_DEFAULTS_KEY: &str = "CView.downloadFolderPath";

/// 사용자 지정이 없을 때의 기본 폴더 — `~/Movies/CView`(없으면 `~/Downloads/CView`, 그다음 임시 폴더).
pub fn default_download_folder(home: Option<&Path>) -> PathBuf {
    let base = home
        .map(|h| {
            let movies = h.join("Movies");
            if movies.is_dir() {
                movies
            } else {
                h.join("Downloads")
            }
        })
        .unwrap_or_else(std::env::temp_dir);
    base.join("CView")
}

/// 저장 폴더 — 지정 경로가 있으면 그것, 없으면 기본 폴더.
pub fn download_folder(saved: Option<&str>, home: Option<&Path>) -> PathBuf {
    match saved.filter(|s| !s.is_empty()) {
        Some(path) => PathBuf::from(path),
        None => default_download_folder(home),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VideoDownloadError {
    #[error("재생 가능한 세그먼트를 찾지 못했습니다.")]
    NoSegments,
    #[error("세그먼트 복호화에 실패했습니다.")]
    DecryptFailed,
    #[error("파일 저장에 실패했습니다.")]
    WriteFailed,
    #[error("서버 응답 오류 ({0})")]
    BadResponse(i64),
    #[error("다운로드 주소를 가져오지 못했습니다 (로그인이 필요할 수 있어요).")]
    Unresolved,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(id: &str, state: DownloadJobState) -> DownloadJob {
        DownloadJob {
            id: id.into(),
            title: id.into(),
            channel_name: None,
            progress: 0.0,
            state,
        }
    }

    #[test]
    fn job_id_round_trips() {
        let vod = MediaDownloadKind::Vod { video_no: 42 };
        assert_eq!(vod.job_id(), "vod-42");
        assert_eq!(
            MediaDownloadKind::from_re_resolvable_job_id("vod-42"),
            Some(vod.clone())
        );
        assert_eq!(MediaDownloadKind::from_re_resolvable_job_id("clip-x"), None);
        assert_eq!(
            MediaDownloadKind::from_history_job_id("clip-abc"),
            Some(MediaDownloadKind::Clip {
                clip_uid: "abc".into()
            })
        );
        assert_eq!(MediaDownloadKind::from_history_job_id("clip-"), None);
        assert_eq!(MediaDownloadKind::from_history_job_id("vod-x"), None);
    }

    #[test]
    fn trim_keeps_active_and_newest_finished() {
        let mut jobs = vec![job("active", DownloadJobState::Downloading)];
        for i in 0..32 {
            jobs.push(job(&format!("f{i}"), DownloadJobState::Failed("x".into())));
        }
        let dropped = trim_finished_jobs(&mut jobs);
        assert_eq!(dropped, HashSet::from(["f0".to_string(), "f1".to_string()]));
        assert_eq!(jobs.len(), 31);
        assert!(jobs[0].is_active());
        assert!(!jobs[0].accepts_progress(f64::NAN));
        assert!(job("a", DownloadJobState::Cancelled).is_retryable_state());
        assert!(!job("a", DownloadJobState::Assembling).accepts_progress(0.5));
    }

    #[test]
    fn folders() {
        assert_eq!(download_folder(Some("/x/y"), None), PathBuf::from("/x/y"));
        let home = std::env::temp_dir().join(format!("home-{}", uuid::Uuid::new_v4()));
        assert_eq!(
            download_folder(Some(""), Some(&home)),
            home.join("Downloads").join("CView")
        );
        std::fs::create_dir_all(home.join("Movies")).unwrap();
        assert_eq!(
            default_download_folder(Some(&home)),
            home.join("Movies").join("CView")
        );
        let _ = std::fs::remove_dir_all(&home);
    }
}
