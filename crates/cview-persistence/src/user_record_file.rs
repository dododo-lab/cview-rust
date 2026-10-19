//! 사용자 기록 파일 공용 헬퍼 — 다운로드 기록·멀티채팅 작업영역이 함께 쓴다.
//!
//! 원본: `Sources/CViewPersistence/UserRecordFile.swift`.
//!
//! 형식: 루트 객체의 `"v"` 가 봉투 버전이다. 손상은 `Corrupt`, 이 앱보다 **높은** 버전은 거부(`Newer`),
//! 낮은 버전은 허용. 쓰기는 키 정렬 압축 JSON 을 원자적으로, 같은 바이트면 디스크를 건드리지 않는다.
//! 날짜 형식은 값 타입이 정한다(Swift 는 `secondsSince1970` — [`crate::time::epoch_seconds`] 참고).

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// `load` 의 결과.
#[derive(Debug, Clone, PartialEq)]
pub enum ReadOutcome<T> {
    Missing,
    Corrupt,
    Newer {
        version: i64,
    },
    /// 디코드한 값과 읽은 원본 바이트(다음 쓰기의 `skip_if_equal_to` 기준).
    Loaded(T, Vec<u8>),
}

#[derive(Deserialize)]
struct Envelope {
    v: i64,
}

/// `UserRecordFile` 네임스페이스.
pub struct UserRecordFile;

impl UserRecordFile {
    /// `<ApplicationSupport>/CView` — 없으면 만든다. 접근할 수 없으면 `None`.
    pub fn directory() -> Option<PathBuf> {
        let dir = crate::store::application_support_dir().ok()?.join("CView");
        std::fs::create_dir_all(&dir).ok()?;
        Some(dir)
    }

    /// 없거나 손상됐거나 새 버전이면 `None`.
    pub fn read<T: DeserializeOwned>(path: &Path, max_version: i64) -> Option<T> {
        match Self::load(path, max_version) {
            ReadOutcome::Loaded(v, _) => Some(v),
            _ => None,
        }
    }

    pub fn load<T: DeserializeOwned>(path: &Path, max_version: i64) -> ReadOutcome<T> {
        if !path.exists() {
            return ReadOutcome::Missing;
        }
        match std::fs::read(path) {
            Ok(data) => Self::decode(&data, max_version),
            Err(_) => {
                tracing::warn!("User record read failed");
                ReadOutcome::Corrupt
            }
        }
    }

    /// `load` 의 순수 부분.
    pub fn decode<T: DeserializeOwned>(data: &[u8], max_version: i64) -> ReadOutcome<T> {
        let Ok(envelope) = serde_json::from_slice::<Envelope>(data) else {
            tracing::warn!("User record has no envelope version (corrupt JSON)");
            return ReadOutcome::Corrupt;
        };
        if envelope.v > max_version {
            tracing::warn!(
                "User record is newer than this app (v{} > v{}) — ignoring",
                envelope.v,
                max_version
            );
            return ReadOutcome::Newer {
                version: envelope.v,
            };
        }
        match serde_json::from_slice::<T>(data) {
            Ok(value) => ReadOutcome::Loaded(value, data.to_vec()),
            Err(_) => {
                tracing::warn!("User record decode failed (v{})", envelope.v);
                ReadOutcome::Corrupt
            }
        }
    }

    /// 값을 인코딩해 원자적으로 쓴다. `previous` 와 같은 바이트면 쓰지 않는다.
    /// 파일에 있는 것과 같은 바이트를 돌려준다(인코딩·쓰기 실패면 `None`).
    pub fn write<T: Serialize>(
        value: &T,
        path: &Path,
        skip_if_equal_to: Option<&[u8]>,
    ) -> Option<Vec<u8>> {
        let data = Self::encode(value)?;
        if skip_if_equal_to == Some(data.as_slice()) {
            return Some(data);
        }
        let result = (|| -> std::io::Result<()> {
            if let Some(dir) = path.parent() {
                if !dir.as_os_str().is_empty() {
                    std::fs::create_dir_all(dir)?;
                }
            }
            crate::json::write_atomic(path, &data)
        })();
        match result {
            Ok(()) => Some(data),
            Err(e) => {
                tracing::warn!("User record write failed: {e}");
                None
            }
        }
    }

    /// 키 정렬 압축 JSON.
    pub fn encode<T: Serialize>(value: &T) -> Option<Vec<u8>> {
        crate::json::to_sorted_vec(value)
            .map_err(|e| tracing::warn!("User record encode failed: {e}"))
            .ok()
    }
}
