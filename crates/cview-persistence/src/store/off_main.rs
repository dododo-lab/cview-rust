//! DataStore 대량 작업을 호출 스레드(비동기 실행기) 밖에서 돌리는 우회로.
//!
//! 원본: `Sources/CViewPersistence/DataStore+OffMain.swift`.
//!
//! Swift 는 detached Task 로 협력 풀에서 actor 에 진입했다. Rust 는 `tokio::task::spawn_blocking` 이다.
//!
//! 취소 계약: 호출 future 를 버리면(취소) 아직 시작하지 않은 본문은 **실행하지 않고** 빠진다 — 본문
//! 진입 직전에 한 번만 검사하므로 "전부 실행 or 전혀 실행 안 함" 이다(부분 쓰기 없음). 이미 시작한
//! 본문은 끝까지 간다. 따라서 취소되어 건너뛰어도 무해한 쓰기(캐시·파생 데이터)에만 쓴다.
//! 던지지 않는 판본([`DataStore::off_main_uncancellable`])은 건너뛰지 않는다.
//!
//! 계측(`DataStore+Trace.swift` 의 `datastore_op` 캡처 줄·signpost, `DataStoreQueueProbe`)은 포팅하지 않았다.

use super::DataStore;
use crate::error::{PersistenceError, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// future 가 버려지면 취소 표식을 세운다.
struct CancelOnDrop(Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

impl DataStore {
    /// 이 작업을 블로킹 풀에서 실행한다. 취소되면 본문을 시작하지 않고 [`PersistenceError::Cancelled`].
    ///
    /// ```ignore
    /// store.off_main(|s| s.upsert_cached_live_channels(&items)).await?;
    /// ```
    pub async fn off_main<T, F>(&self, body: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&DataStore) -> Result<T> + Send + 'static,
    {
        let cancelled = Arc::new(AtomicBool::new(false));
        let guard = CancelOnDrop(cancelled.clone());
        let store = self.clone();
        let job = tokio::task::spawn_blocking(move || {
            // 본문 진입 직전 체크 — 큐 대기 중 도착한 취소도 여기서 본다.
            if cancelled.load(Ordering::SeqCst) {
                return Err(PersistenceError::Cancelled);
            }
            body(&store)
        });
        let result = job
            .await
            .map_err(|e| PersistenceError::Join(e.to_string()))?;
        std::mem::forget(guard);
        result
    }

    /// 던지지 않는 본문용 — 취소돼도 본문을 건너뛰지 않는다(끝까지 돌아야 하는 경로).
    pub async fn off_main_uncancellable<T, F>(&self, body: F) -> T
    where
        T: Send + 'static,
        F: FnOnce(&DataStore) -> T + Send + 'static,
    {
        let store = self.clone();
        match tokio::task::spawn_blocking(move || body(&store)).await {
            Ok(v) => v,
            Err(e) => std::panic::resume_unwind(e.into_panic()),
        }
    }
}
