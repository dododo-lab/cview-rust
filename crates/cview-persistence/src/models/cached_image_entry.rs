//! 이미지 디스크 캐시 인덱스.
//!
//! 원본: `Sources/CViewPersistence/Models/CachedImageEntry.swift` (`CachedImageEntry`) 와
//! `DataStore.swift` 의 `CachedImageEntryData`, `ImageTouch` DTO.

use crate::db::db_model;
use crate::time::{now, Date};

db_model! {
    /// 이미지 디스크 캐시 항목 인덱스. 1행 = 1파일.
    pub struct CachedImageEntry in "CachedImageEntry" {
        /// SHA-256(URL) 의 hex — 디스크 파일명과 같다.
        pub url_hash: String => "urlHash",
        pub url_string: Option<String> => "urlString",
        /// 캐시 디렉터리 안 상대 파일명.
        pub relative_path: String => "relativePath",
        pub etag: Option<String> => "etag",
        pub last_modified: Option<String> => "lastModified",
        pub mime_type: Option<String> => "mimeType",
        pub size_bytes: i64 => "sizeBytes",
        pub fetched_at: Date => "fetchedAt",
        pub accessed_at: Date => "accessedAt",
        pub access_count: i64 => "accessCount",
        pub expires_at: Option<Date> => "expiresAt",
        /// `livethumb`, `channelimg`, `clipthumb`, `categoryimg`, `other`.
        pub source: String => "source",
    }
}

/// 이미지 캐시 인덱스 DTO(`CachedImageEntryData`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CachedImageEntryData {
    pub url_hash: String,
    pub url_string: Option<String>,
    pub relative_path: String,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub fetched_at: Date,
    pub accessed_at: Date,
    pub access_count: i64,
    pub expires_at: Option<Date>,
    pub source: String,
}

impl CachedImageEntryData {
    /// Swift 기본 인자와 같은 기본값(`source = "other"`, 시각 = now).
    pub fn new(url_hash: impl Into<String>, relative_path: impl Into<String>) -> Self {
        let t = now();
        Self {
            url_hash: url_hash.into(),
            url_string: None,
            relative_path: relative_path.into(),
            etag: None,
            last_modified: None,
            mime_type: None,
            size_bytes: 0,
            fetched_at: t,
            accessed_at: t,
            access_count: 0,
            expires_at: None,
            source: "other".into(),
        }
    }
}

impl From<&CachedImageEntry> for CachedImageEntryData {
    fn from(m: &CachedImageEntry) -> Self {
        Self {
            url_hash: m.url_hash.clone(),
            url_string: m.url_string.clone(),
            relative_path: m.relative_path.clone(),
            etag: m.etag.clone(),
            last_modified: m.last_modified.clone(),
            mime_type: m.mime_type.clone(),
            size_bytes: m.size_bytes,
            fetched_at: m.fetched_at,
            accessed_at: m.accessed_at,
            access_count: m.access_count,
            expires_at: m.expires_at,
            source: m.source.clone(),
        }
    }
}

/// 이미지 캐시 access-touch 코얼레싱 단위(`ImageTouch`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageTouch {
    pub accessed_at: Date,
    pub count: i64,
}
