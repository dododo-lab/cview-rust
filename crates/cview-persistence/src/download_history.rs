//! 다운로드 기록 파일(downloads-history.v1.json)의 형식과 파일 확인.
//!
//! 원본: `Sources/CViewPersistence/DownloadHistoryRecord.swift`.
//!
//! 파일 확인 규칙: 앱이 접근할 수 있는 루트 안의 경로만 missing 으로 단정하고, 밖이면 unverified.
//! ⚠ macOS 일반 북마크(`URL.bookmarkData` / `resolvingBookmarkData`)는 포팅하지 않았다 — `file_bookmark`
//! 바이트는 그대로 보존(base64)하지만 경로 추적·갱신은 하지 않는다.

use crate::time::Date;
use base64::Engine;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

/// 완료/실패.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadState {
    Completed,
    Failed,
}

mod epoch_secs {
    use crate::time::{epoch_seconds, from_unix, Date};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(d: &Date, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_f64(epoch_seconds(d))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Date, D::Error> {
        Ok(from_unix(f64::deserialize(d)?))
    }
}

fn ser_b64<S: Serializer>(v: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
    match v {
        Some(b) => s.serialize_str(&base64::engine::general_purpose::STANDARD.encode(b)),
        None => s.serialize_none(),
    }
}

fn de_b64<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
    match Option::<String>::deserialize(d)? {
        None => Ok(None),
        Some(t) => base64::engine::general_purpose::STANDARD
            .decode(t)
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

/// 다운로드 기록 한 행.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadHistoryEntry {
    /// job id(`vod-123`·`clip-abc`). 같은 id 는 한 행이다.
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_name: Option<String>,
    pub state: DownloadState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    /// 일반 북마크 바이트(base64).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "ser_b64",
        deserialize_with = "de_b64"
    )]
    pub file_bookmark: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_size: Option<i64>,
    /// epoch 초(`secondsSince1970`).
    #[serde(with = "epoch_secs")]
    pub finished_at: Date,
}

impl DownloadHistoryEntry {
    pub fn file_url(&self) -> Option<PathBuf> {
        self.file_path.as_ref().map(PathBuf::from)
    }
}

/// 다운로드 기록 문서. 행 하나가 깨져도 나머지는 살린다.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DownloadHistoryDocument {
    /// 봉투 버전 — JSON 키 `"v"`.
    #[serde(rename = "v")]
    pub format_version: i64,
    /// 최신순.
    pub entries: Vec<DownloadHistoryEntry>,
}

impl DownloadHistoryDocument {
    pub const VERSION: i64 = 1;
    pub const FILE_NAME: &'static str = "downloads-history.v1.json";

    pub fn new(entries: Vec<DownloadHistoryEntry>) -> Self {
        Self {
            format_version: Self::VERSION,
            entries,
        }
    }
}

impl<'de> Deserialize<'de> for DownloadHistoryDocument {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            v: i64,
            entries: Vec<serde_json::Value>,
        }
        let raw = Raw::deserialize(d)?;
        Ok(Self {
            format_version: raw.v,
            entries: raw
                .entries
                .into_iter()
                .filter_map(|v| serde_json::from_value(v).ok())
                .collect(),
        })
    }
}

/// 파일 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DownloadHistoryFileStatus {
    Present,
    /// 앱이 볼 수 있는 루트 안인데 파일이 없다.
    Missing,
    /// 루트 밖이라 없다고 단정할 수 없다.
    Unverified,
}

/// 확인 결과.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DownloadHistoryCheckResult {
    pub entries: Vec<DownloadHistoryEntry>,
    pub statuses: HashMap<String, DownloadHistoryFileStatus>,
    /// 경로가 바뀌었거나 낡은 북마크를 새로 만든 행(북마크 미포팅이라 현재는 항상 비어 있다).
    pub repaired_ids: HashSet<String>,
}

/// 완료 직후 메타데이터.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileMetadata {
    pub size: Option<i64>,
    pub bookmark: Option<Vec<u8>>,
}

/// `DownloadHistoryFileCheck`.
pub struct DownloadHistoryFileCheck;

impl DownloadHistoryFileCheck {
    /// 완료 행마다 존재를 확인한다. 실패 행은 건드리지 않는다.
    pub fn verify(
        entries: &[DownloadHistoryEntry],
        roots: &[PathBuf],
    ) -> DownloadHistoryCheckResult {
        let mut result = DownloadHistoryCheckResult {
            entries: entries.to_vec(),
            ..Default::default()
        };
        for entry in result.entries.iter_mut() {
            if entry.state != DownloadState::Completed {
                continue;
            }
            let before = entry.clone();
            let status = Self::check(entry, roots);
            result.statuses.insert(before.id.clone(), status);
            if *entry != before {
                result.repaired_ids.insert(before.id);
            }
        }
        result
    }

    pub(crate) fn check(
        entry: &mut DownloadHistoryEntry,
        roots: &[PathBuf],
    ) -> DownloadHistoryFileStatus {
        let Some(path) = &entry.file_path else {
            return DownloadHistoryFileStatus::Unverified;
        };
        if Path::new(path).exists() {
            return DownloadHistoryFileStatus::Present;
        }
        if Self::is_inside_accessible_root(path, roots) {
            DownloadHistoryFileStatus::Missing
        } else {
            DownloadHistoryFileStatus::Unverified
        }
    }

    /// 완료 직후 한 번 — 크기(북마크는 macOS 전용이라 `None`).
    pub fn metadata(path: &str) -> FileMetadata {
        FileMetadata {
            size: std::fs::metadata(path).ok().map(|m| m.len() as i64),
            bookmark: None,
        }
    }

    pub fn is_inside_accessible_root(path: &str, roots: &[PathBuf]) -> bool {
        let targets = path_variants(Path::new(path));
        roots.iter().any(|root| {
            path_variants(root).iter().any(|root_path| {
                let prefix = if root_path.ends_with('/') {
                    root_path.clone()
                } else {
                    format!("{root_path}/")
                };
                targets.iter().any(|t| t.starts_with(&prefix))
            })
        })
    }
}

/// `.`·`..` 를 정리한 경로(파일 시스템을 보지 않는다).
fn standardized(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// 표준화 경로와 심볼릭 링크를 푼 경로. 없는 파일은 부모 폴더를 풀고 이름을 붙인다.
fn path_variants(path: &Path) -> HashSet<String> {
    let mut set = HashSet::new();
    set.insert(canonical(&standardized(path).to_string_lossy()));
    if let Ok(resolved) = std::fs::canonicalize(path) {
        set.insert(canonical(&resolved.to_string_lossy()));
    }
    if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
        let parent = std::fs::canonicalize(parent).unwrap_or_else(|_| standardized(parent));
        set.insert(canonical(&parent.join(name).to_string_lossy()));
    }
    set
}

/// `/private/var` 와 `/var` 는 같은 곳이다.
fn canonical(path: &str) -> String {
    for alias in ["/private/var/", "/private/tmp/", "/private/etc/"] {
        if path.starts_with(alias) {
            return path["/private".len()..].to_string();
        }
    }
    path.to_string()
}
