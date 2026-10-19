//! 인사이트 화면이 읽는 데이터의 세대 번호.
//!
//! 원본: `Sources/CViewCore/Performance/WatchInsightsRevision.swift`. Rust `cview-core` 에는 아직
//! 포팅되지 않아 이 크레이트에 둔다(같은 프로세스 메모리 값 — 의미 동일).
//!
//! 인사이트가 읽는 데이터를 바꾸는 save 가 성공하면 1 올린다(시청 기록 시작·체크포인트·종료·복구·
//! 보존 정리·전체 삭제, 북마크 저장·삭제). 바뀐 것이 없는 경로는 올리지 않는다.

use std::sync::atomic::{AtomicU64, Ordering};

static REVISION: AtomicU64 = AtomicU64::new(0);

/// `WatchInsightsRevision`.
pub struct WatchInsightsRevision;

impl WatchInsightsRevision {
    /// 현재 값.
    pub fn current() -> u64 {
        REVISION.load(Ordering::SeqCst)
    }

    /// 1 올린다(넘치면 감싼다).
    pub fn bump() {
        REVISION.fetch_add(1, Ordering::SeqCst);
    }
}
