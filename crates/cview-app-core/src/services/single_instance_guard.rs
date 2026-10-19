//! `Sources/CViewApp/SingleInstanceGuard.swift` 포팅 — 교차 플랫폼 lock 파일.
//!
//! primary 인스턴스는 `<app support>/CView/.primary.lock` 에 배타 lock 을 잡고 프로세스 수명 동안
//! 유지한다(비정상 종료 시 OS 가 해제 → stale 처리 불필요). 같은 lock 획득에 실패하면 이미 primary
//! 가 있다는 뜻이다. lock 파일에는 primary PID 를 적어 중복 인스턴스가 전면화 대상을 알 수 있게 한다.
//!
//! macOS 전용 부분(DistributedNotification 핸드오프·ack 재전송, Apple Event 로 cold-launch URL 캡처,
//! `NSRunningApplication` 전면화, `exit`)은 옮기지 않았다. 중복 인스턴스 쪽 동작은 셸이
//! [`SingleInstanceRole::Secondary`] 를 받고 결정한다(Tauri 라면 single-instance 플러그인).
//! 수신 쪽 버퍼·nonce 중복 제거는 [`crate::navigation::deep_link_inbox`] 에 있다.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// 메인 ↔ 중복 인스턴스 IPC 이름(원본 `SingleInstanceIPC`).
pub mod ipc {
    /// 중복 인스턴스 → primary: 이 URL 을 라우팅해 달라. 페이로드 `{url, nonce}`.
    pub const OPEN_URL: &str = "dev.cview.singleinstance.openURL";
    /// primary → 중복 인스턴스: openURL 수신 확인.
    pub const ACK: &str = "dev.cview.singleinstance.ack";
}

/// lock 파일 이름.
pub const LOCK_FILE_NAME: &str = ".primary.lock";

/// primary 가 잡은 lock. 버리면 lock 이 풀린다 — 프로세스 수명 동안 보관한다.
#[derive(Debug)]
pub struct PrimaryLock {
    _file: Option<File>,
    path: PathBuf,
}

impl PrimaryLock {
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// lock 파일을 열지 못해 fail-open 으로 primary 가 된 경우 true.
    pub fn is_fail_open(&self) -> bool {
        self._file.is_none()
    }
}

#[derive(Debug)]
pub enum SingleInstanceRole {
    /// 우리가 primary.
    Primary(PrimaryLock),
    /// 이미 primary 가 있다. lock 파일에서 읽은 primary PID(읽지 못하면 `None`).
    Secondary { primary_pid: Option<u32> },
}

impl SingleInstanceRole {
    pub fn is_primary(&self) -> bool {
        matches!(self, Self::Primary(_))
    }
}

pub struct SingleInstanceGuard;

impl SingleInstanceGuard {
    /// 모든 사본이 같은 경로를 계산해야 한다 — `<app support>/CView/.primary.lock`.
    pub fn lock_file_path(app_support_dir: &Path) -> PathBuf {
        app_support_dir.join("CView").join(LOCK_FILE_NAME)
    }

    /// 중복 실행 가드. lock 파일조차 못 여는 환경(디스크·권한)이면 fail-open 으로 primary.
    pub fn engage(lock_path: &Path) -> SingleInstanceRole {
        if let Some(dir) = lock_path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let file = match OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
        {
            Ok(f) => f,
            Err(e) => {
                tracing::error!("SingleInstanceGuard: lock open 실패 ({e}) — primary 로 폴백");
                return SingleInstanceRole::Primary(PrimaryLock {
                    _file: None,
                    path: lock_path.to_path_buf(),
                });
            }
        };
        match file.try_lock() {
            Ok(()) => {
                let mut file = file;
                write_pid(&mut file);
                tracing::info!(
                    "SingleInstanceGuard: primary 확정 (pid {})",
                    std::process::id()
                );
                SingleInstanceRole::Primary(PrimaryLock {
                    _file: Some(file),
                    path: lock_path.to_path_buf(),
                })
            }
            Err(TryLockError::WouldBlock) => {
                tracing::info!("SingleInstanceGuard: 이미 primary 실행 중");
                SingleInstanceRole::Secondary {
                    primary_pid: Self::read_primary_pid(lock_path),
                }
            }
            Err(TryLockError::Error(e)) => {
                tracing::error!("SingleInstanceGuard: lock 실패 ({e}) — primary 로 폴백");
                SingleInstanceRole::Primary(PrimaryLock {
                    _file: None,
                    path: lock_path.to_path_buf(),
                })
            }
        }
    }

    /// lock 파일에 primary 가 기록한 PID. 실패 시 `None`.
    pub fn read_primary_pid(lock_path: &Path) -> Option<u32> {
        let mut s = String::new();
        File::open(lock_path).ok()?.read_to_string(&mut s).ok()?;
        s.trim().parse::<u32>().ok().filter(|pid| *pid > 0)
    }
}

fn write_pid(file: &mut File) {
    let _ = file.set_len(0);
    let _ = file.seek(SeekFrom::Start(0));
    let _ = writeln!(file, "{}", std::process::id());
    let _ = file.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cview-sig-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn second_engage_is_secondary_until_primary_drops() {
        let dir = temp_dir();
        let path = SingleInstanceGuard::lock_file_path(&dir);
        let first = SingleInstanceGuard::engage(&path);
        assert!(first.is_primary());
        match SingleInstanceGuard::engage(&path) {
            SingleInstanceRole::Secondary { primary_pid } => {
                assert_eq!(primary_pid, Some(std::process::id()));
            }
            SingleInstanceRole::Primary(_) => panic!("이미 lock 이 잡혀 있다"),
        }
        drop(first);
        assert!(SingleInstanceGuard::engage(&path).is_primary());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
