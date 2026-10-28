//! 스토어 파일(본 파일·-wal·-shm·외부 저장 폴더) 백업·복원.
//!
//! 원본: `Sources/CViewPersistence/StoreBackup.swift`.
//!
//! 컨테이너를 열기 **전**에만 찍는다. `Backups/.incomplete-<UUID>/` 에 복사 → manifest.json →
//! 완성 이름 `<stem>-<yyyyMMdd-HHmmss>-b<build>/` 로 rename. 중간에 죽어도 완성본처럼 보이는 폴더가
//! 남지 않고, 잔재는 다음 snapshot 이 지운다.
//! ⚠ manifest·Entry 에 절대경로를 넣지 않는다. 앱이 만든 이름만 지운다.

use crate::time::{now, Date};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 백업 이유.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BackupReason {
    BeforeUpgrade,
    BeforeVersionStamp,
    Manual,
}

/// 완성된 백업 하나. `id` 는 폴더 **이름**이다(경로 아님).
#[derive(Debug, Clone, PartialEq)]
pub struct BackupEntry {
    pub id: String,
    pub created_at: Date,
    pub app_build: String,
    pub reason: BackupReason,
    /// 백업을 찍게 한 사전 점검 결과(raw value).
    pub preflight: Option<String>,
    pub bytes: i64,
    /// 복사한 파일·폴더 이름.
    pub files: Vec<String>,
}

/// 복원 실패.
#[derive(Debug, thiserror::Error)]
pub enum BackupFailure {
    /// 완성본 이름이 아니거나 manifest 를 읽을 수 없다.
    #[error("백업을 찾을 수 없습니다")]
    EntryNotFound,
    /// 백업 폴더에 본 스토어 파일이 없다.
    #[error("백업에 스토어 파일이 없습니다")]
    StoreFileMissing,
    /// 파일 연산 실패.
    #[error("IO 오류: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestFile {
    name: String,
    bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    #[serde(rename = "v")]
    version: i64,
    #[serde(with = "crate::time::iso8601")]
    created_at: Date,
    app_build: String,
    reason: BackupReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    preflight: Option<String>,
    files: Vec<ManifestFile>,
}

impl BackupEntry {
    fn from_manifest(id: String, m: Manifest) -> Self {
        Self {
            id,
            created_at: m.created_at,
            app_build: m.app_build,
            reason: m.reason,
            preflight: m.preflight,
            bytes: m.files.iter().map(|f| f.bytes).sum(),
            files: m.files.into_iter().map(|f| f.name).collect(),
        }
    }
}

type CapacityFn = Arc<dyn Fn(&Path) -> Option<i64> + Send + Sync>;
type CopyFn = Arc<dyn Fn(&Path, &Path) -> std::io::Result<()> + Send + Sync>;
type ClockFn = Arc<dyn Fn() -> Date + Send + Sync>;

/// 스토어 백업기(`StoreBackup`).
#[derive(Clone)]
pub struct StoreBackup {
    pub store_path: PathBuf,
    /// 기본값은 스토어 옆 `Backups/`.
    pub directory: PathBuf,
    /// 남길 완성본 수. `replaced-*` 는 따로 1개만 남긴다.
    pub keep: usize,
    available_capacity: CapacityFn,
    copy_item: CopyFn,
    now: ClockFn,
}

impl std::fmt::Debug for StoreBackup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoreBackup")
            .field("store_path", &self.store_path)
            .field("directory", &self.directory)
            .field("keep", &self.keep)
            .finish()
    }
}

const MANIFEST_VERSION: i64 = 1;
const MANIFEST_NAME: &str = "manifest.json";
const INCOMPLETE_PREFIX: &str = ".incomplete-";
const REPLACED_PREFIX: &str = "replaced-";

impl StoreBackup {
    pub const DEFAULT_KEEP: usize = 2;

    pub fn new(store_path: impl Into<PathBuf>) -> Self {
        Self::with_options(store_path, None, Self::DEFAULT_KEEP)
    }

    pub fn with_options(
        store_path: impl Into<PathBuf>,
        directory: Option<PathBuf>,
        keep: usize,
    ) -> Self {
        Self::with_hooks(
            store_path,
            directory,
            keep,
            Arc::new(volume_available_capacity),
            Arc::new(copy_item),
            Arc::new(now),
        )
    }

    /// 테스트 주입용 — 여유 공간·복사 실패·시각을 결정적으로 만든다.
    pub fn with_hooks(
        store_path: impl Into<PathBuf>,
        directory: Option<PathBuf>,
        keep: usize,
        available_capacity: CapacityFn,
        copy_item: CopyFn,
        now: ClockFn,
    ) -> Self {
        let store_path = store_path.into();
        let directory = directory.unwrap_or_else(|| store_dir(&store_path).join("Backups"));
        Self {
            store_path,
            directory,
            keep,
            available_capacity,
            copy_item,
            now,
        }
    }

    // MARK: - 찍기

    /// 지금 스토어 파일을 백업한다. **실패해도 오류를 내지 않는다** — `None` 과 경고 로그.
    /// 원본이 없거나(첫 실행) 여유 공간이 복사량의 2배보다 적으면 건너뛴다.
    pub fn snapshot(
        &self,
        reason: BackupReason,
        app_build: &str,
        preflight: Option<&str>,
    ) -> Option<BackupEntry> {
        if self.keep == 0 {
            return None;
        }
        self.remove_incomplete_leftovers();
        let sources = self.present_store_files();
        let store_name = self.store_name();
        if !sources.iter().any(|(n, _)| *n == store_name) {
            return None;
        }
        let total: i64 = sources.iter().map(|(_, b)| *b).sum();
        if let Some(available) = (self.available_capacity)(&store_dir(&self.store_path)) {
            if available < total * 2 {
                tracing::warn!(
                    "store_backup skipped reason=low_space need={} available={}",
                    total * 2,
                    available
                );
                return None;
            }
        }
        let staging = self.directory.join(format!(
            "{INCOMPLETE_PREFIX}{}",
            uuid::Uuid::new_v4().to_string().to_uppercase()
        ));
        let result = (|| -> std::io::Result<BackupEntry> {
            std::fs::create_dir_all(&staging)?;
            let dir = store_dir(&self.store_path);
            for (name, _) in &sources {
                (self.copy_item)(&dir.join(name), &staging.join(name))?;
            }
            let manifest = Manifest {
                version: MANIFEST_VERSION,
                created_at: (self.now)(),
                app_build: app_build.to_string(),
                reason,
                preflight: preflight.map(str::to_string),
                files: sources
                    .iter()
                    .map(|(n, b)| ManifestFile {
                        name: n.clone(),
                        bytes: *b,
                    })
                    .collect(),
            };
            let data =
                crate::json::to_sorted_vec_pretty(&manifest).map_err(std::io::Error::other)?;
            crate::json::write_atomic(&staging.join(MANIFEST_NAME), &data)?;
            let final_path =
                self.unique_path(&self.completed_name(&manifest.created_at, app_build));
            std::fs::rename(&staging, &final_path)?;
            self.prune_completed();
            // 목록과 같은 값이 되도록 방금 쓴 바이트를 다시 읽는다(날짜 정밀도 일치).
            let decoded: Manifest = serde_json::from_slice(&data).map_err(std::io::Error::other)?;
            Ok(BackupEntry::from_manifest(file_name(&final_path), decoded))
        })();
        match result {
            Ok(entry) => Some(entry),
            Err(e) => {
                let _ = remove_item(&staging);
                tracing::warn!("store_backup failed code={}", e.raw_os_error().unwrap_or(0));
                None
            }
        }
    }

    // MARK: - 목록·삭제

    /// manifest 가 있는 완성본만, 최신순.
    pub fn list(&self) -> Vec<BackupEntry> {
        let Ok(names) = list_names(&self.directory) else {
            return Vec::new();
        };
        let mut entries: Vec<BackupEntry> = names
            .into_iter()
            .filter(|n| self.is_completed_name(n))
            .filter_map(|n| {
                let m = self.read_manifest(&n)?;
                Some(BackupEntry::from_manifest(n, m))
            })
            .collect();
        entries.sort_by(|a, b| (b.created_at, &b.id).cmp(&(a.created_at, &a.id)));
        entries
    }

    /// 앱이 만든 백업(완성본·잔재·replaced)을 모두 지운다. 첫 삭제 실패를 돌려준다.
    pub fn remove_all(&self) -> std::io::Result<()> {
        let Ok(names) = list_names(&self.directory) else {
            return Ok(());
        };
        let mut first_error = None;
        for name in names.iter().filter(|n| self.is_app_made_name(n)) {
            if let Err(e) = remove_item(&self.directory.join(name)) {
                first_error.get_or_insert(e);
            }
        }
        if list_names(&self.directory).is_ok_and(|n| n.is_empty()) {
            let _ = std::fs::remove_dir(&self.directory);
        }
        match first_error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    // MARK: - 복원

    /// `entry` 로 스토어를 되돌린다. **호출자는 이 스토어를 연 연결이 없음을 보장해야 한다.**
    ///
    /// 1. 지금 파일을 `Backups/replaced-<ts>/` 로 옮긴다(지우지 않는다).
    /// 2. 백업 파일을 스토어 자리에 복사한다(백업은 그대로 남는다).
    /// 3. 하나라도 실패하면 옮긴 파일을 제자리로 되돌리고 오류.
    pub fn restore(&self, entry: &BackupEntry) -> Result<(), BackupFailure> {
        if !self.is_completed_name(&entry.id) || self.read_manifest(&entry.id).is_none() {
            return Err(BackupFailure::EntryNotFound);
        }
        let source = self.directory.join(&entry.id);
        let dir = store_dir(&self.store_path);
        let names = self.store_file_names();
        let restoring: Vec<&String> = names.iter().filter(|n| source.join(n).exists()).collect();
        if !restoring.iter().any(|n| **n == self.store_name()) {
            return Err(BackupFailure::StoreFileMissing);
        }
        let current: Vec<&String> = names.iter().filter(|n| dir.join(n).exists()).collect();
        let replaced = self.unique_path(&format!("{REPLACED_PREFIX}{}", timestamp(&(self.now)())));
        std::fs::create_dir_all(&replaced)?;
        std::fs::create_dir_all(&dir)?;

        let mut moved: Vec<String> = Vec::new();
        for name in &current {
            if let Err(e) = std::fs::rename(dir.join(name), replaced.join(name)) {
                self.put_back(&moved, &replaced);
                return Err(e.into());
            }
            moved.push((*name).clone());
        }
        for name in &restoring {
            if let Err(e) = (self.copy_item)(&source.join(name), &dir.join(name)) {
                for n in &names {
                    let _ = remove_item(&dir.join(n));
                }
                self.put_back(&moved, &replaced);
                return Err(e.into());
            }
        }
        self.prune_replaced(&file_name(&replaced));
        Ok(())
    }

    // MARK: - 크기

    /// 스토어 파일 바이트 합. 없으면 0.
    pub fn store_bytes(store_path: &Path) -> i64 {
        Self::new(store_path)
            .present_store_files()
            .iter()
            .map(|(_, b)| *b)
            .sum()
    }

    // MARK: - 내부

    fn store_name(&self) -> String {
        file_name(&self.store_path)
    }

    fn stem(&self) -> String {
        self.store_path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// 본 파일, WAL, 공유 메모리, 외부 저장(`.<stem>_SUPPORT`) 폴더 순.
    pub fn store_file_names(&self) -> Vec<String> {
        let name = self.store_name();
        vec![
            name.clone(),
            format!("{name}-wal"),
            format!("{name}-shm"),
            format!(".{}_SUPPORT", self.stem()),
        ]
    }

    fn present_store_files(&self) -> Vec<(String, i64)> {
        let dir = store_dir(&self.store_path);
        self.store_file_names()
            .into_iter()
            .filter_map(|n| item_bytes(&dir.join(&n)).map(|b| (n, b)))
            .collect()
    }

    /// 경로 구분자가 든 이름은 받지 않는다.
    fn is_completed_name(&self, name: &str) -> bool {
        name.starts_with(&format!("{}-", self.stem()))
            && !name.contains('/')
            && !name.contains("..")
    }

    fn is_app_made_name(&self, name: &str) -> bool {
        self.is_completed_name(name)
            || name.starts_with(INCOMPLETE_PREFIX)
            || name.starts_with(REPLACED_PREFIX)
    }

    fn completed_name(&self, date: &Date, app_build: &str) -> String {
        let safe: String = app_build
            .chars()
            .take(32)
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        format!(
            "{}-{}-b{}",
            self.stem(),
            timestamp(date),
            if safe.is_empty() { "unknown" } else { &safe }
        )
    }

    /// 같은 초에 두 번 찍어도 덮어쓰지 않도록 `-2`, `-3` … 을 붙인다.
    fn unique_path(&self, base: &str) -> PathBuf {
        let mut candidate = self.directory.join(base);
        let mut suffix = 2;
        while candidate.exists() {
            candidate = self.directory.join(format!("{base}-{suffix}"));
            suffix += 1;
        }
        candidate
    }

    fn read_manifest(&self, folder: &str) -> Option<Manifest> {
        let data = std::fs::read(self.directory.join(folder).join(MANIFEST_NAME)).ok()?;
        let manifest: Manifest = serde_json::from_slice(&data).ok()?;
        (manifest.version == MANIFEST_VERSION).then_some(manifest)
    }

    fn remove_incomplete_leftovers(&self) {
        if let Ok(names) = list_names(&self.directory) {
            for name in names.iter().filter(|n| n.starts_with(INCOMPLETE_PREFIX)) {
                let _ = remove_item(&self.directory.join(name));
            }
        }
    }

    fn prune_completed(&self) {
        for stale in self.list().into_iter().skip(self.keep) {
            let _ = remove_item(&self.directory.join(stale.id));
        }
    }

    fn prune_replaced(&self, current: &str) {
        if let Ok(names) = list_names(&self.directory) {
            for name in names
                .iter()
                .filter(|n| n.starts_with(REPLACED_PREFIX) && n.as_str() != current)
            {
                let _ = remove_item(&self.directory.join(name));
            }
        }
    }

    fn put_back(&self, moved: &[String], replaced: &Path) {
        let dir = store_dir(&self.store_path);
        for name in moved {
            let _ = std::fs::rename(replaced.join(name), dir.join(name));
        }
        if list_names(replaced).is_ok_and(|n| n.is_empty()) {
            let _ = std::fs::remove_dir(replaced);
        }
    }
}

fn store_dir(store_path: &Path) -> PathBuf {
    store_path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn list_names(dir: &Path) -> std::io::Result<Vec<String>> {
    Ok(std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect())
}

fn remove_item(path: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// 파일 또는 폴더(재귀) 복사. 대상이 이미 있으면 실패(Swift `FileManager.copyItem`).
pub fn copy_item(from: &Path, to: &Path) -> std::io::Result<()> {
    if to.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "대상이 이미 있습니다",
        ));
    }
    let meta = std::fs::metadata(from)?;
    if meta.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_item(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(|_| ())
    }
}

/// 파일 크기, 폴더면 안의 일반 파일 크기 합. 없으면 `None`.
fn item_bytes(path: &Path) -> Option<i64> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_dir() {
        return Some(meta.len() as i64);
    }
    fn walk(dir: &Path) -> i64 {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return 0;
        };
        rd.filter_map(|e| e.ok())
            .map(|e| match e.metadata() {
                Ok(m) if m.is_dir() => walk(&e.path()),
                Ok(m) if m.is_file() => m.len() as i64,
                _ => 0,
            })
            .sum()
    }
    Some(walk(path))
}

/// 볼륨 여유 공간(바이트).
#[cfg(unix)]
pub fn volume_available_capacity(path: &Path) -> Option<i64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: c 는 NUL 종료 경로, stat 은 유효한 출력 버퍼다.
    let rc = unsafe { libc::statvfs(c.as_ptr(), &mut stat) };
    if rc != 0 {
        return None;
    }
    #[allow(clippy::unnecessary_cast)]
    Some((stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64) as i64)
}

/// 볼륨 여유 공간(알 수 없음).
#[cfg(not(unix))]
pub fn volume_available_capacity(_path: &Path) -> Option<i64> {
    None
}

/// `yyyyMMdd-HHmmss`(현지 시각).
pub fn timestamp(date: &Date) -> String {
    date.with_timezone(&chrono::Local)
        .format("%Y%m%d-%H%M%S")
        .to_string()
}
