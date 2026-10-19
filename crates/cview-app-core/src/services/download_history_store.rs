//! `Sources/CViewApp/Services/DownloadHistoryStore.swift` 포팅 — 최근 완료·실패 다운로드 30개 기록.
//!
//! 위치: `<app support>/CView/downloads-history.v1.json`(`UserRecordFile` 봉투 v=1).
//! - 채우기: 처음 만질 때 스스로 시작한다. 파일 읽기·존재 확인은 blocking 스레드에서 한다.
//! - 쓰기: 완료·실패·목록 제거 때만. 직렬 체인 + 같은 바이트면 생략.
//!   ⚠ 쓰기 링크는 채우기를 먼저 기다린다 — 채우기 전에 도착한 기록이 파일의 이전 기록을 덮지 않게.
//! - 파일이 이 앱보다 새 버전이면 이번 실행은 쓰지 않는다.
//!
//! 라이브러리 화면의 `DownloadsLibraryList`(세션 job ∪ 기록)는 `VideoDownloadService.Job` 에 묶여
//! 뷰모델 쪽 몫이라 옮기지 않았다. 표시 줄(`detailLine`)의 날짜 형식은 셸이 정한다.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use cview_core::utils::time::{Date, TimeInterval};
use cview_persistence::download_history::{
    DownloadHistoryCheckResult, DownloadHistoryDocument, DownloadHistoryEntry,
    DownloadHistoryFileCheck, DownloadHistoryFileStatus, DownloadState,
};
use cview_persistence::user_record_file::{ReadOutcome, UserRecordFile};
use futures_util::future::{BoxFuture, FutureExt, Shared};

use super::support::{lock, system_clock, Clock};

/// `VideoDownloadService` 가 알리는 사건. 취소는 사건이 아니다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadHistoryEvent {
    Completed {
        id: String,
        title: String,
        channel_name: Option<String>,
        file: PathBuf,
    },
    Failed {
        id: String,
        title: String,
        channel_name: Option<String>,
        reason: String,
    },
    /// 사용자의 '목록에서 제거'·'끝난 항목 지우기'.
    Removed { ids: Vec<String> },
}

type Flight = Shared<BoxFuture<'static, ()>>;

#[derive(Default)]
struct Loaded {
    check: DownloadHistoryCheckResult,
    bytes: Option<Vec<u8>>,
    blocks_writes: bool,
}

#[derive(Default)]
struct State {
    entries: Vec<DownloadHistoryEntry>,
    file_status: HashMap<String, DownloadHistoryFileStatus>,
    is_hydrated: bool,
    hydration: Option<Flight>,
    write_chain: Option<(u64, Flight)>,
    write_serial: u64,
    last_written_bytes: Option<Vec<u8>>,
    writes_blocked: bool,
    removed_before_hydration: HashSet<String>,
    pending_metadata: HashSet<String>,
    last_verified_at: Option<Date>,
    is_verifying: bool,
}

/// 다운로드 기록 저장소. `Clone` 은 같은 저장소를 가리킨다.
#[derive(Clone)]
pub struct DownloadHistoryStore {
    inner: Arc<Mutex<State>>,
    file_path: Option<PathBuf>,
    accessible_roots: Arc<Vec<PathBuf>>,
    now: Clock,
}

impl DownloadHistoryStore {
    pub const CAPACITY: usize = 30;
    /// 라이브러리 탭 진입 때 파일 존재를 다시 확인하는 기본 간격(초).
    pub const DEFAULT_VERIFY_MAX_AGE: TimeInterval = 60.0;

    pub fn new(
        file_path: Option<PathBuf>,
        accessible_roots: Vec<PathBuf>,
        now: Option<Clock>,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(State::default())),
            file_path,
            accessible_roots: Arc::new(accessible_roots),
            now: now.unwrap_or_else(system_clock),
        }
    }

    /// 기본 위치 — `UserRecordFile::directory()/downloads-history.v1.json`.
    pub fn default_file_path() -> Option<PathBuf> {
        UserRecordFile::directory().map(|d| d.join(DownloadHistoryDocument::FILE_NAME))
    }

    /// 최신순. 최대 `CAPACITY`.
    pub fn entries(&self) -> Vec<DownloadHistoryEntry> {
        lock(&self.inner).entries.clone()
    }
    /// 완료 행의 파일 상태.
    pub fn file_status(&self) -> HashMap<String, DownloadHistoryFileStatus> {
        lock(&self.inner).file_status.clone()
    }
    pub fn is_hydrated(&self) -> bool {
        lock(&self.inner).is_hydrated
    }

    // MARK: 채우기

    /// 파일을 읽어 메모리와 합친다. 이미 시작했으면 그 작업을 기다린다.
    pub async fn hydrate(&self) {
        let flight = self.start_hydration_if_needed();
        flight.await;
    }

    fn start_hydration_if_needed(&self) -> Flight {
        let mut st = lock(&self.inner);
        if let Some(f) = &st.hydration {
            return f.clone();
        }
        let path = self.file_path.clone();
        let roots = self.accessible_roots.clone();
        let me = self.clone();
        let flight: Flight = async move {
            let loaded = tokio::task::spawn_blocking(move || Self::load(path, &roots))
                .await
                .unwrap_or_default();
            me.apply_hydration(loaded);
        }
        .boxed()
        .shared();
        st.hydration = Some(flight.clone());
        // 기다리는 이가 없어도 진행하도록 한 번 띄운다.
        tokio::spawn(flight.clone());
        flight
    }

    fn load(path: Option<PathBuf>, roots: &[PathBuf]) -> Loaded {
        let Some(path) = path else {
            return Loaded::default();
        };
        match UserRecordFile::load::<DownloadHistoryDocument>(
            &path,
            DownloadHistoryDocument::VERSION,
        ) {
            ReadOutcome::Missing | ReadOutcome::Corrupt => Loaded::default(),
            ReadOutcome::Newer { .. } => Loaded {
                blocks_writes: true,
                ..Default::default()
            },
            ReadOutcome::Loaded(document, bytes) => {
                let entries: Vec<_> = document.entries.into_iter().take(Self::CAPACITY).collect();
                Loaded {
                    check: DownloadHistoryFileCheck::verify(&entries, roots),
                    bytes: Some(bytes),
                    blocks_writes: false,
                }
            }
        }
    }

    fn apply_hydration(&self, loaded: Loaded) {
        let needs_write = {
            let mut st = lock(&self.inner);
            st.writes_blocked = loaded.blocks_writes;
            st.last_written_bytes = loaded.bytes;
            // 채우는 동안 도착한 기록(메모리)이 이긴다. 그 사이 지운 id 는 파일 쪽에서도 뺀다.
            let memory_ids: HashSet<String> = st.entries.iter().map(|e| e.id.clone()).collect();
            let from_file: Vec<_> = loaded
                .check
                .entries
                .iter()
                .filter(|e| {
                    !memory_ids.contains(&e.id) && !st.removed_before_hydration.contains(&e.id)
                })
                .cloned()
                .collect();
            let merged: Vec<_> = st
                .entries
                .iter()
                .chain(from_file.iter())
                .take(Self::CAPACITY)
                .cloned()
                .collect();
            let merged_ids: HashSet<&String> = merged.iter().map(|e| &e.id).collect();
            let kept_file_ids: HashSet<&String> = from_file
                .iter()
                .map(|e| &e.id)
                .filter(|id| merged_ids.contains(id))
                .collect();
            let mut statuses: HashMap<String, DownloadHistoryFileStatus> = loaded
                .check
                .statuses
                .iter()
                .filter(|(k, _)| kept_file_ids.contains(k))
                .map(|(k, v)| (k.clone(), *v))
                .collect();
            for (k, v) in &st.file_status {
                statuses.insert(k.clone(), *v);
            }
            st.removed_before_hydration.clear();
            let needs_write =
                !loaded.check.repaired_ids.is_empty() || merged != loaded.check.entries;
            st.entries = merged;
            st.file_status = statuses;
            st.last_verified_at = Some((self.now)());
            st.is_hydrated = true;
            needs_write
        };
        if needs_write {
            self.schedule_write();
        }
    }

    // MARK: 기록

    pub fn record(&self, event: DownloadHistoryEvent) {
        drop(self.start_hydration_if_needed()); // 이미 띄웠다 — 기다리지 않는다
        let now = (self.now)();
        {
            let mut st = lock(&self.inner);
            match event {
                DownloadHistoryEvent::Completed {
                    id,
                    title,
                    channel_name,
                    file,
                } => {
                    Self::insert(
                        &mut st,
                        DownloadHistoryEntry {
                            id: id.clone(),
                            title,
                            channel_name,
                            state: DownloadState::Completed,
                            failure_reason: None,
                            file_path: Some(file.to_string_lossy().into_owned()),
                            file_bookmark: None,
                            file_size: None,
                            finished_at: now,
                        },
                    );
                    st.file_status
                        .insert(id.clone(), DownloadHistoryFileStatus::Present);
                    st.pending_metadata.insert(id);
                }
                DownloadHistoryEvent::Failed {
                    id,
                    title,
                    channel_name,
                    reason,
                } => {
                    Self::insert(
                        &mut st,
                        DownloadHistoryEntry {
                            id: id.clone(),
                            title,
                            channel_name,
                            state: DownloadState::Failed,
                            failure_reason: Some(reason),
                            file_path: None,
                            file_bookmark: None,
                            file_size: None,
                            finished_at: now,
                        },
                    );
                    st.file_status.remove(&id);
                    st.pending_metadata.remove(&id);
                }
                DownloadHistoryEvent::Removed { ids } => {
                    drop(st);
                    self.remove(&ids.into_iter().collect());
                    return;
                }
            }
        }
        self.schedule_write();
    }

    pub fn remove(&self, ids: &HashSet<String>) {
        if ids.is_empty() {
            return;
        }
        drop(self.start_hydration_if_needed()); // 이미 띄웠다 — 기다리지 않는다
        let should_write = {
            let mut st = lock(&self.inner);
            if !st.is_hydrated {
                st.removed_before_hydration.extend(ids.iter().cloned());
            }
            st.pending_metadata.retain(|id| !ids.contains(id));
            let before = st.entries.len();
            st.entries.retain(|e| !ids.contains(&e.id));
            let changed = st.entries.len() != before;
            st.file_status.retain(|k, _| !ids.contains(k));
            changed || !st.is_hydrated
        };
        if should_write {
            self.schedule_write();
        }
    }

    /// 같은 id 는 교체하고 맨 앞으로. 상한을 넘으면 가장 오래된 것부터 버린다.
    fn insert(st: &mut State, entry: DownloadHistoryEntry) {
        st.entries.retain(|e| e.id != entry.id);
        st.entries.insert(0, entry);
        if st.entries.len() > Self::CAPACITY {
            let dropped: HashSet<String> = st.entries[Self::CAPACITY..]
                .iter()
                .map(|e| e.id.clone())
                .collect();
            st.entries.truncate(Self::CAPACITY);
            st.pending_metadata.retain(|id| !dropped.contains(id));
            st.file_status.retain(|k, _| !dropped.contains(k));
        }
    }

    // MARK: 파일 확인

    /// 마지막 확인이 `max_age` 보다 오래됐으면 존재를 다시 확인한다.
    pub fn refresh_file_status_if_stale(&self, max_age: TimeInterval) {
        {
            let st = lock(&self.inner);
            if !st.is_hydrated || st.is_verifying {
                return;
            }
            if let Some(last) = st.last_verified_at {
                let age = ((self.now)() - last).num_milliseconds() as f64 / 1000.0;
                if age < max_age {
                    return;
                }
            }
        }
        let me = self.clone();
        tokio::spawn(async move { me.refresh_file_status().await });
    }

    pub async fn refresh_file_status(&self) {
        let targets: Vec<DownloadHistoryEntry> = {
            let mut st = lock(&self.inner);
            if st.is_verifying {
                return;
            }
            let targets: Vec<_> = st
                .entries
                .iter()
                .filter(|e| e.state == DownloadState::Completed)
                .cloned()
                .collect();
            if targets.is_empty() {
                st.last_verified_at = Some((self.now)());
                return;
            }
            st.is_verifying = true;
            targets
        };
        let roots = self.accessible_roots.clone();
        let check =
            tokio::task::spawn_blocking(move || DownloadHistoryFileCheck::verify(&targets, &roots))
                .await
                .unwrap_or_default();
        let changed = {
            let mut st = lock(&self.inner);
            st.is_verifying = false;
            st.last_verified_at = Some((self.now)());
            let mut changed = false;
            for checked in &check.entries {
                let Some(index) = st
                    .entries
                    .iter()
                    .position(|e| e.id == checked.id && e.finished_at == checked.finished_at)
                else {
                    continue;
                };
                if let Some(status) = check.statuses.get(&checked.id) {
                    st.file_status.insert(checked.id.clone(), *status);
                }
                if check.repaired_ids.contains(&checked.id) {
                    st.entries[index].file_path = checked.file_path.clone();
                    st.entries[index].file_bookmark = checked.file_bookmark.clone();
                    changed = true;
                }
            }
            changed
        };
        if changed {
            self.schedule_write();
        }
    }

    // MARK: 쓰기

    fn schedule_write(&self) {
        let Some(path) = self.file_path.clone() else {
            return;
        };
        let hydration = self.start_hydration_if_needed();
        let mut st = lock(&self.inner);
        let previous = st.write_chain.as_ref().map(|(_, f)| f.clone());
        let me = self.clone();
        st.write_serial += 1;
        let serial = st.write_serial;
        let flight: Flight = async move {
            hydration.await;
            if let Some(p) = previous {
                p.await;
            }
            if lock(&me.inner).writes_blocked {
                return;
            }
            me.fill_pending_metadata().await;
            // 스냅샷은 링크가 **실행될 때** 뜬다.
            let (document, last) = {
                let st = lock(&me.inner);
                (
                    DownloadHistoryDocument::new(st.entries.clone()),
                    st.last_written_bytes.clone(),
                )
            };
            let written = tokio::task::spawn_blocking(move || {
                UserRecordFile::write(&document, &path, last.as_deref())
            })
            .await
            .ok()
            .flatten();
            if let Some(bytes) = written {
                lock(&me.inner).last_written_bytes = Some(bytes);
            }
        }
        .boxed()
        .shared();
        st.write_chain = Some((serial, flight.clone()));
        tokio::spawn(flight);
    }

    async fn fill_pending_metadata(&self) {
        let targets: Vec<(String, Date, String)> = {
            let mut st = lock(&self.inner);
            if st.pending_metadata.is_empty() {
                return;
            }
            let targets = st
                .entries
                .iter()
                .filter(|e| st.pending_metadata.contains(&e.id))
                .filter_map(|e| Some((e.id.clone(), e.finished_at, e.file_path.clone()?)))
                .collect();
            st.pending_metadata.clear();
            targets
        };
        if targets.is_empty() {
            return;
        }
        let computed = tokio::task::spawn_blocking(move || {
            targets
                .into_iter()
                .map(|(id, at, path)| {
                    let meta = DownloadHistoryFileCheck::metadata(&path);
                    (id, at, meta)
                })
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();
        let mut st = lock(&self.inner);
        for (id, at, meta) in computed {
            if let Some(entry) = st
                .entries
                .iter_mut()
                .find(|e| e.id == id && e.finished_at == at)
            {
                entry.file_size = meta.size;
                entry.file_bookmark = meta.bookmark;
            }
        }
    }

    /// 대기 중인 쓰기가 모두 끝날 때까지 기다린다(테스트·종료 경로).
    pub async fn wait_for_pending_writes(&self) {
        let hydration = lock(&self.inner).hydration.clone();
        if let Some(h) = hydration {
            h.await;
        }
        loop {
            let chain = lock(&self.inner).write_chain.clone();
            let Some((serial, flight)) = chain else { break };
            flight.await;
            if lock(&self.inner).write_chain.as_ref().map(|(s, _)| *s) == Some(serial) {
                break;
            }
        }
    }

    // MARK: 라이브러리 표시

    /// 이번 세션 job 이 없는 기록 행 — 같은 id 는 세션이 이긴다.
    pub fn past_entries(
        &self,
        state: DownloadState,
        excluding_session_ids: &HashSet<String>,
    ) -> Vec<DownloadHistoryEntry> {
        lock(&self.inner)
            .entries
            .iter()
            .filter(|e| e.state == state && !excluding_session_ids.contains(&e.id))
            .cloned()
            .collect()
    }
}

/// '채널 · 크기 · 완료 시각' — 있는 것만. 크기·날짜 문자열은 호출자가 형식을 정해 넘긴다.
pub fn detail_line(
    entry: &DownloadHistoryEntry,
    formatted_size: Option<&str>,
    formatted_date: &str,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(name) = entry.channel_name.as_deref().filter(|n| !n.is_empty()) {
        parts.push(name.to_string());
    }
    if entry.file_size.is_some_and(|s| s > 0) {
        if let Some(size) = formatted_size {
            parts.push(size.to_string());
        }
    }
    parts.push(formatted_date.to_string());
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/DownloadHistoryStoreTests.swift` 핵심.
    use super::*;

    fn temp_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("dl-history-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn completed(id: &str, file: &std::path::Path) -> DownloadHistoryEvent {
        DownloadHistoryEvent::Completed {
            id: id.into(),
            title: format!("t-{id}"),
            channel_name: Some("ch".into()),
            file: file.to_path_buf(),
        }
    }

    fn failed(id: &str) -> DownloadHistoryEvent {
        DownloadHistoryEvent::Failed {
            id: id.into(),
            title: format!("t-{id}"),
            channel_name: None,
            reason: "네트워크".into(),
        }
    }

    #[tokio::test]
    async fn records_survive_restart_and_cap_at_capacity() {
        let dir = temp_dir();
        let path = dir.join(DownloadHistoryDocument::FILE_NAME);
        let video = dir.join("a.mp4");
        std::fs::write(&video, b"1234").unwrap();

        let store = DownloadHistoryStore::new(Some(path.clone()), vec![dir.clone()], None);
        store.record(completed("a", &video));
        for i in 0..DownloadHistoryStore::CAPACITY {
            store.record(failed(&format!("f{i}")));
        }
        store.wait_for_pending_writes().await;
        assert_eq!(store.entries().len(), DownloadHistoryStore::CAPACITY);
        assert!(
            store.entries().iter().all(|e| e.id != "a"),
            "가장 오래된 기록이 밀려난다"
        );

        store.record(completed("a", &video));
        store.wait_for_pending_writes().await;
        let restarted = DownloadHistoryStore::new(Some(path), vec![dir.clone()], None);
        restarted.hydrate().await;
        let entries = restarted.entries();
        assert_eq!(entries.len(), DownloadHistoryStore::CAPACITY);
        assert_eq!(entries[0].id, "a");
        assert_eq!(entries[0].file_size, Some(4));
        assert_eq!(
            restarted.file_status().get("a"),
            Some(&DownloadHistoryFileStatus::Present)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn removal_before_hydration_wins_and_missing_file_is_detected() {
        let dir = temp_dir();
        let path = dir.join(DownloadHistoryDocument::FILE_NAME);
        let video = dir.join("b.mp4");
        std::fs::write(&video, b"x").unwrap();
        let store = DownloadHistoryStore::new(Some(path.clone()), vec![dir.clone()], None);
        store.record(completed("b", &video));
        store.record(failed("c"));
        store.wait_for_pending_writes().await;

        let restarted = DownloadHistoryStore::new(Some(path.clone()), vec![dir.clone()], None);
        restarted.remove(&HashSet::from(["c".to_string()]));
        restarted.hydrate().await;
        restarted.wait_for_pending_writes().await;
        assert_eq!(
            restarted
                .entries()
                .iter()
                .map(|e| e.id.as_str())
                .collect::<Vec<_>>(),
            ["b"]
        );
        std::fs::remove_file(&video).unwrap();
        restarted.refresh_file_status().await;
        assert_eq!(
            restarted.file_status().get("b"),
            Some(&DownloadHistoryFileStatus::Missing)
        );
        let past =
            restarted.past_entries(DownloadState::Completed, &HashSet::from(["b".to_string()]));
        assert!(past.is_empty(), "같은 id 는 세션이 이긴다");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn detail_line_parts() {
        let entry = DownloadHistoryEntry {
            id: "a".into(),
            title: "t".into(),
            channel_name: Some("채널".into()),
            state: DownloadState::Completed,
            failure_reason: None,
            file_path: None,
            file_bookmark: None,
            file_size: Some(10),
            finished_at: cview_core::utils::time::from_unix(0.0),
        };
        assert_eq!(
            detail_line(&entry, Some("10 B"), "1월 1일"),
            "채널 · 10 B · 1월 1일"
        );
    }
}
