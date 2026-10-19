//! `Sources/CViewApp/Services/MultiChatWorkspaceStore.swift` 포팅(+ 파일 형식
//! `Sources/CViewCore/Models/MultiChatWorkspace.swift` — Rust `cview-core` 에 아직 없어 여기 둔다).
//!
//! 멀티채팅 작업영역(세션·선택·그리드)을 설정 도메인 밖의 작은 파일로 옮긴다.
//! 위치: `<app support>/CView/multichat-workspace.v1.json`(`UserRecordFile` 봉투 v=1).
//!
//! 채우기:
//! 1. 파일이 있으면 그것이 정본이다.
//! 2. 없으면(또는 손상됐고 도메인이 아직 이관 전이면) 설정 도메인에서 옮긴다 — 설정 로드를 기다린다.
//!    파일을 쓰고 **다시 읽어 같을 때만** 도메인의 세션·선택을 비우고 이관 표시를 세운다.
//! 3. `multiChatWorkspaceShouldClear`(정상 종료 표시)가 있으면 두 스코프의 세션·선택을 비우고 레이아웃은
//!    남긴다. 표시는 비운 문서를 쓴 **뒤에만** 지운다.
//! 4. 손상 파일은 빈 문서로 시작한다. 이 앱보다 새 버전 파일은 이번 실행 동안 쓰지 않는다.
//!
//! 채우기 전에 들어온 `update` 는 모아 두었다가 채운 뒤 순서대로 적용한다. 쓰기는 500ms 묶음, 같은 바이트면 생략.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cview_core::models::settings_ui::MultiChatSettings;
use cview_core::utils::time::Date;
use cview_persistence::settings_store::SettingsStore;
use cview_persistence::user_record_file::{ReadOutcome, UserRecordFile};
use futures_util::future::{BoxFuture, FutureExt, Shared};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use tokio::task::AbortHandle;

use super::multi_chat_preferences::MultiChatScope;
use super::support::{lock, system_clock, Clock, KeyValueStore};

// MARK: - 파일 형식

/// 저장 세션 한 줄.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkspaceSession {
    #[serde(rename = "id")]
    pub channel_id: String,
    #[serde(rename = "name")]
    pub channel_name: String,
    #[serde(rename = "img", default, skip_serializing_if = "Option::is_none")]
    pub channel_image_url: Option<String>,
    /// 표시·진단용 힌트다. 연결할 때는 늘 liveDetail 로 다시 확인한다.
    #[serde(rename = "chat", default, skip_serializing_if = "Option::is_none")]
    pub chat_channel_id_hint: Option<String>,
}

/// 워크스페이스 하나(라이브 도크 · 독립 멀티채팅 메뉴)의 상태.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceScope {
    pub sessions: Vec<WorkspaceSession>,
    pub selected_channel_id: Option<String>,
    /// false = 탭, true = 그리드.
    pub is_grid_layout: bool,
    /// "preset" | "custom" | "focusLeft"
    pub grid_layout_mode: String,
    pub grid_horizontal_ratio: f64,
    pub grid_vertical_ratio: f64,
    pub grid_focus_ratio: f64,
}

impl Default for WorkspaceScope {
    fn default() -> Self {
        Self {
            sessions: Vec::new(),
            selected_channel_id: None,
            is_grid_layout: false,
            grid_layout_mode: "preset".into(),
            grid_horizontal_ratio: 0.5,
            grid_vertical_ratio: 0.5,
            grid_focus_ratio: MultiChatSettings::DEFAULT_GRID_FOCUS_RATIO,
        }
    }
}

impl WorkspaceScope {
    /// 설정 도메인(이관 원본)에서 옮긴다. 옛 저장본에는 이미지·chat 채널 ID 가 없다.
    pub fn from_legacy(legacy: &MultiChatSettings) -> Self {
        Self {
            sessions: legacy
                .saved_sessions
                .iter()
                .map(|s| WorkspaceSession {
                    channel_id: s.channel_id.clone(),
                    channel_name: s.channel_name.clone(),
                    channel_image_url: None,
                    chat_channel_id_hint: None,
                })
                .collect(),
            selected_channel_id: legacy.selected_channel_id.clone(),
            is_grid_layout: legacy.is_grid_layout,
            grid_layout_mode: legacy.grid_layout_mode.clone(),
            grid_horizontal_ratio: legacy.grid_horizontal_ratio,
            grid_vertical_ratio: legacy.grid_vertical_ratio,
            grid_focus_ratio: legacy.grid_focus_ratio,
        }
    }

    /// 정상 종료 규칙 — 세션 identity 만 비우고 레이아웃은 남긴다.
    pub fn clearing_restorable_sessions(&self) -> Self {
        Self {
            sessions: Vec::new(),
            selected_channel_id: None,
            ..self.clone()
        }
    }
}

#[derive(Serialize)]
struct ScopeOut<'a> {
    sessions: &'a [WorkspaceSession],
    #[serde(rename = "selected", skip_serializing_if = "Option::is_none")]
    selected: &'a Option<String>,
    #[serde(rename = "grid", skip_serializing_if = "Option::is_none")]
    grid: Option<bool>,
    #[serde(rename = "mode", skip_serializing_if = "Option::is_none")]
    mode: Option<&'a str>,
    #[serde(rename = "hRatio", skip_serializing_if = "Option::is_none")]
    h_ratio: Option<f64>,
    #[serde(rename = "vRatio", skip_serializing_if = "Option::is_none")]
    v_ratio: Option<f64>,
    #[serde(rename = "focusRatio", skip_serializing_if = "Option::is_none")]
    focus_ratio: Option<f64>,
}

impl Serialize for WorkspaceScope {
    /// 레이아웃 값은 기본값이면 쓰지 않는다(파일 크기 기준).
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let d = Self::default();
        ScopeOut {
            sessions: &self.sessions,
            selected: &self.selected_channel_id,
            grid: (self.is_grid_layout != d.is_grid_layout).then_some(self.is_grid_layout),
            mode: (self.grid_layout_mode != d.grid_layout_mode).then_some(&self.grid_layout_mode),
            h_ratio: (self.grid_horizontal_ratio != d.grid_horizontal_ratio)
                .then_some(self.grid_horizontal_ratio),
            v_ratio: (self.grid_vertical_ratio != d.grid_vertical_ratio)
                .then_some(self.grid_vertical_ratio),
            focus_ratio: (self.grid_focus_ratio != d.grid_focus_ratio)
                .then_some(self.grid_focus_ratio),
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for WorkspaceScope {
    /// 키가 빠져도 기본값으로 살리고, 세션 한 줄이 깨져도 나머지는 살린다.
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(default)]
            sessions: Vec<serde_json::Value>,
            #[serde(default)]
            selected: Option<String>,
            grid: Option<bool>,
            mode: Option<String>,
            #[serde(rename = "hRatio")]
            h_ratio: Option<f64>,
            #[serde(rename = "vRatio")]
            v_ratio: Option<f64>,
            #[serde(rename = "focusRatio")]
            focus_ratio: Option<f64>,
        }
        let raw = Raw::deserialize(d)?;
        let f = Self::default();
        Ok(Self {
            sessions: raw
                .sessions
                .into_iter()
                .filter_map(|v| serde_json::from_value(v).ok())
                .collect(),
            selected_channel_id: raw.selected,
            is_grid_layout: raw.grid.unwrap_or(f.is_grid_layout),
            grid_layout_mode: raw.mode.unwrap_or(f.grid_layout_mode),
            grid_horizontal_ratio: raw.h_ratio.unwrap_or(f.grid_horizontal_ratio),
            grid_vertical_ratio: raw.v_ratio.unwrap_or(f.grid_vertical_ratio),
            grid_focus_ratio: raw.focus_ratio.unwrap_or(f.grid_focus_ratio),
        })
    }
}

mod epoch_opt {
    use super::Date;
    use cview_core::utils::time::from_unix;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(d: &Option<Date>, s: S) -> Result<S::Ok, S::Error> {
        match d {
            Some(d) => s.serialize_f64(d.timestamp_millis() as f64 / 1000.0),
            None => s.serialize_none(),
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Date>, D::Error> {
        Ok(Option::<f64>::deserialize(d)?.map(from_unix))
    }
}

/// 작업영역 파일 전체 — 두 워크스페이스를 한 파일에 둔다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MultiChatWorkspaceDocument {
    #[serde(rename = "v")]
    pub format_version: i64,
    /// 라이브 메뉴 인라인 멀티채팅.
    #[serde(default)]
    pub live: WorkspaceScope,
    /// 사이드바 "멀티채팅" 메뉴.
    #[serde(default)]
    pub standalone: WorkspaceScope,
    /// 설정 도메인에서 옮겨 온 시각(epoch 초).
    #[serde(
        rename = "migratedAt",
        default,
        skip_serializing_if = "Option::is_none",
        with = "epoch_opt"
    )]
    pub migrated_at: Option<Date>,
}

impl Default for MultiChatWorkspaceDocument {
    fn default() -> Self {
        Self {
            format_version: Self::VERSION,
            live: WorkspaceScope::default(),
            standalone: WorkspaceScope::default(),
            migrated_at: None,
        }
    }
}

impl MultiChatWorkspaceDocument {
    pub const VERSION: i64 = 1;
    pub const FILE_NAME: &'static str = "multichat-workspace.v1.json";

    pub fn scope(&self, scope: MultiChatScope) -> &WorkspaceScope {
        match scope {
            MultiChatScope::Live => &self.live,
            MultiChatScope::Standalone => &self.standalone,
        }
    }

    pub fn scope_mut(&mut self, scope: MultiChatScope) -> &mut WorkspaceScope {
        match scope {
            MultiChatScope::Live => &mut self.live,
            MultiChatScope::Standalone => &mut self.standalone,
        }
    }
}

// MARK: - 저장소

type Mutation = Box<dyn FnOnce(&mut WorkspaceScope) + Send>;
type Flight = Shared<BoxFuture<'static, ()>>;

#[derive(Default)]
struct State {
    document: MultiChatWorkspaceDocument,
    is_hydrated: bool,
    disk_write_count: usize,
    legacy: Option<SettingsStore>,
    hydration: Option<Flight>,
    pending_mutations: Vec<(MultiChatScope, Mutation)>,
    debounce: Option<AbortHandle>,
    write_chain: Option<(u64, Flight)>,
    write_serial: u64,
    last_written_bytes: Option<Vec<u8>>,
    writes_blocked: bool,
}

/// 저장소 설정.
#[derive(Clone)]
pub struct MultiChatWorkspaceConfig {
    pub debounce: Duration,
    pub legacy_poll_interval: Duration,
    pub legacy_poll_limit: usize,
}

impl Default for MultiChatWorkspaceConfig {
    fn default() -> Self {
        Self {
            debounce: Duration::from_millis(500),
            legacy_poll_interval: Duration::from_millis(100),
            legacy_poll_limit: 300,
        }
    }
}

/// 멀티채팅 작업영역 파일 저장소. `Clone` 은 같은 저장소를 가리킨다.
#[derive(Clone)]
pub struct MultiChatWorkspaceStore {
    inner: Arc<Mutex<State>>,
    file_path: Option<PathBuf>,
    defaults: Arc<dyn KeyValueStore>,
    config: MultiChatWorkspaceConfig,
    now: Clock,
}

impl MultiChatWorkspaceStore {
    /// 정상 종료 표시 키. 세우는 쪽은 앱 종료 경로, 지우는 쪽은 이 저장소다.
    pub const SHOULD_CLEAR_DEFAULTS_KEY: &'static str = "multiChatWorkspaceShouldClear";

    pub fn new(
        file_path: Option<PathBuf>,
        defaults: Arc<dyn KeyValueStore>,
        config: MultiChatWorkspaceConfig,
        now: Option<Clock>,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(State::default())),
            file_path,
            defaults,
            config,
            now: now.unwrap_or_else(system_clock),
        }
    }

    pub fn default_file_path() -> Option<PathBuf> {
        UserRecordFile::directory().map(|d| d.join(MultiChatWorkspaceDocument::FILE_NAME))
    }

    /// 이관 원본을 붙인다. 먼저 붙은 것이 이긴다.
    pub fn attach_legacy_settings(&self, settings: &SettingsStore) {
        let mut st = lock(&self.inner);
        if st.legacy.is_none() {
            st.legacy = Some(settings.clone());
        }
    }

    pub fn document(&self) -> MultiChatWorkspaceDocument {
        lock(&self.inner).document.clone()
    }
    pub fn scope(&self, scope: MultiChatScope) -> WorkspaceScope {
        lock(&self.inner).document.scope(scope).clone()
    }
    pub fn is_hydrated(&self) -> bool {
        lock(&self.inner).is_hydrated
    }
    /// 실제로 파일을 쓴 횟수 — 같은 바이트라 건너뛴 것은 세지 않는다.
    pub fn disk_write_count(&self) -> usize {
        lock(&self.inner).disk_write_count
    }

    // MARK: 채우기

    pub async fn wait_until_hydrated(&self) {
        self.start_hydration_if_needed().await;
    }

    fn start_hydration_if_needed(&self) -> Flight {
        let mut st = lock(&self.inner);
        if let Some(f) = &st.hydration {
            return f.clone();
        }
        let me = self.clone();
        let flight: Flight = async move {
            let path = me.file_path.clone();
            let outcome = tokio::task::spawn_blocking(move || {
                path.map(|p| {
                    UserRecordFile::load::<MultiChatWorkspaceDocument>(
                        &p,
                        MultiChatWorkspaceDocument::VERSION,
                    )
                })
            })
            .await
            .ok()
            .flatten();
            me.hydrate(outcome).await;
        }
        .boxed()
        .shared();
        st.hydration = Some(flight.clone());
        tokio::spawn(flight.clone());
        flight
    }

    async fn hydrate(&self, outcome: Option<ReadOutcome<MultiChatWorkspaceDocument>>) {
        let (mut next, migrated_from) = self.initial_document(outcome).await;
        let clear_requested = self.defaults.bool(Self::SHOULD_CLEAR_DEFAULTS_KEY);
        if clear_requested {
            next.live = next.live.clearing_restorable_sessions();
            next.standalone = next.standalone.clearing_restorable_sessions();
        }
        let had_pending = {
            let mut st = lock(&self.inner);
            let pending = std::mem::take(&mut st.pending_mutations);
            let had = !pending.is_empty();
            for (scope, mutate) in pending {
                mutate(next.scope_mut(scope));
            }
            st.document = next;
            st.is_hydrated = true;
            had
        };
        if migrated_from.is_some() || clear_requested {
            self.persist_hydration(migrated_from, clear_requested);
        } else if had_pending {
            self.schedule_write();
        }
    }

    async fn initial_document(
        &self,
        outcome: Option<ReadOutcome<MultiChatWorkspaceDocument>>,
    ) -> (MultiChatWorkspaceDocument, Option<SettingsStore>) {
        match outcome {
            None => (MultiChatWorkspaceDocument::default(), None),
            Some(ReadOutcome::Newer { .. }) => {
                lock(&self.inner).writes_blocked = true;
                (MultiChatWorkspaceDocument::default(), None)
            }
            Some(ReadOutcome::Loaded(doc, bytes)) => {
                lock(&self.inner).last_written_bytes = Some(bytes);
                (doc, None)
            }
            Some(ReadOutcome::Missing) | Some(ReadOutcome::Corrupt) => {
                let Some(legacy) = self.ready_legacy_settings().await else {
                    lock(&self.inner).writes_blocked = true;
                    return (MultiChatWorkspaceDocument::default(), None);
                };
                if !legacy.can_persist() {
                    lock(&self.inner).writes_blocked = true;
                }
                let live = legacy.multi_chat();
                let standalone = legacy.multi_chat_standalone();
                if live.workspace_migrated && standalone.workspace_migrated {
                    return (MultiChatWorkspaceDocument::default(), None);
                }
                let now_secs = (self.now)().timestamp();
                let migrated = MultiChatWorkspaceDocument {
                    format_version: MultiChatWorkspaceDocument::VERSION,
                    live: WorkspaceScope::from_legacy(&live),
                    standalone: WorkspaceScope::from_legacy(&standalone),
                    migrated_at: Some(cview_core::utils::time::from_unix(now_secs as f64)),
                };
                (migrated, Some(legacy))
            }
        }
    }

    async fn ready_legacy_settings(&self) -> Option<SettingsStore> {
        let mut polls = 0;
        loop {
            let legacy = lock(&self.inner).legacy.clone();
            if let Some(l) = legacy.filter(|l| l.is_loaded()) {
                return Some(l);
            }
            if polls >= self.config.legacy_poll_limit {
                return None;
            }
            polls += 1;
            tokio::time::sleep(self.config.legacy_poll_interval).await;
        }
    }

    fn persist_hydration(&self, legacy: Option<SettingsStore>, clear_requested: bool) {
        let Some(path) = self.file_path.clone() else {
            return;
        };
        if lock(&self.inner).writes_blocked {
            return;
        }
        let me = self.clone();
        self.enqueue_write(Some(Box::new(move |wrote: bool| {
            async move {
                if !wrote {
                    return; // 도메인·표시를 그대로 둬 다음 실행이 다시 시도한다
                }
                if clear_requested {
                    me.defaults.remove(Self::SHOULD_CLEAR_DEFAULTS_KEY);
                }
                let Some(legacy) = legacy else { return };
                let Some(expected) = lock(&me.inner).last_written_bytes.clone() else {
                    return;
                };
                let reread = tokio::task::spawn_blocking(move || {
                    UserRecordFile::load::<MultiChatWorkspaceDocument>(
                        &path,
                        MultiChatWorkspaceDocument::VERSION,
                    )
                })
                .await
                .ok();
                match reread {
                    Some(ReadOutcome::Loaded(_, bytes)) if bytes == expected => {}
                    _ => {
                        tracing::warn!(
                            "MultiChat workspace migration verify failed — 설정 도메인을 남긴다"
                        );
                        return;
                    }
                }
                let live = legacy.multi_chat();
                if !live.workspace_migrated {
                    legacy.set_multi_chat(live.marking_workspace_migrated());
                }
                let standalone = legacy.multi_chat_standalone();
                if !standalone.workspace_migrated {
                    legacy.set_multi_chat_standalone(standalone.marking_workspace_migrated());
                }
            }
            .boxed()
        })));
    }

    // MARK: 변경

    /// 메모리에 바로 반영하고 500ms 묶음으로 쓴다. 채우기 전이면 모아 두었다가 채운 뒤 적용한다.
    pub fn update(
        &self,
        scope: MultiChatScope,
        mutate: impl FnOnce(&mut WorkspaceScope) + Send + 'static,
    ) {
        drop(self.start_hydration_if_needed()); // 이미 띄웠다 — 기다리지 않는다
        let changed = {
            let mut st = lock(&self.inner);
            if !st.is_hydrated {
                st.pending_mutations.push((scope, Box::new(mutate)));
                return;
            }
            let mut next = st.document.scope(scope).clone();
            mutate(&mut next);
            if &next == st.document.scope(scope) {
                false
            } else {
                *st.document.scope_mut(scope) = next;
                true
            }
        };
        if changed {
            self.schedule_write();
        }
    }

    // MARK: 쓰기

    fn schedule_write(&self) {
        let mut st = lock(&self.inner);
        if self.file_path.is_none() || st.writes_blocked {
            return;
        }
        if let Some(d) = st.debounce.take() {
            d.abort();
        }
        let me = self.clone();
        let delay = self.config.debounce;
        let handle = tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            lock(&me.inner).debounce = None;
            me.enqueue_write(None);
        });
        st.debounce = Some(handle.abort_handle());
    }

    /// 묶음 대기를 건너뛰고 지금 쓴 뒤 모든 쓰기가 끝날 때까지 기다린다.
    pub async fn flush(&self) {
        let pending = lock(&self.inner).debounce.take();
        if let Some(d) = pending {
            d.abort();
            self.enqueue_write(None);
        }
        self.wait_for_pending_writes().await;
    }

    pub async fn wait_for_pending_writes(&self) {
        loop {
            let chain = lock(&self.inner).write_chain.clone();
            let Some((serial, flight)) = chain else { break };
            flight.await;
            if lock(&self.inner).write_chain.as_ref().map(|(s, _)| *s) == Some(serial) {
                break;
            }
        }
    }

    #[allow(clippy::type_complexity)]
    fn enqueue_write(
        &self,
        completion: Option<Box<dyn FnOnce(bool) -> BoxFuture<'static, ()> + Send>>,
    ) {
        let Some(path) = self.file_path.clone() else {
            return;
        };
        let mut st = lock(&self.inner);
        let previous = st.write_chain.as_ref().map(|(_, f)| f.clone());
        st.write_serial += 1;
        let serial = st.write_serial;
        let me = self.clone();
        let flight: Flight = async move {
            if let Some(p) = previous {
                p.await;
            }
            let wrote = me.write_now(path).await;
            if let Some(c) = completion {
                c(wrote).await;
            }
        }
        .boxed()
        .shared();
        st.write_chain = Some((serial, flight.clone()));
        tokio::spawn(flight);
    }

    async fn write_now(&self, path: PathBuf) -> bool {
        let (snapshot, last) = {
            let st = lock(&self.inner);
            if st.writes_blocked {
                return false;
            }
            (st.document.clone(), st.last_written_bytes.clone())
        };
        let last_for_write = last.clone();
        let written = tokio::task::spawn_blocking(move || {
            UserRecordFile::write(&snapshot, &path, last_for_write.as_deref())
        })
        .await
        .ok()
        .flatten();
        let Some(written) = written else {
            return false;
        };
        let mut st = lock(&self.inner);
        if Some(&written) != last.as_ref() {
            st.disk_write_count += 1;
        }
        st.last_written_bytes = Some(written);
        true
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/MultiChatWorkspaceStoreTests.swift` 핵심.
    use super::*;
    use crate::services::support::MemoryKeyValueStore;

    fn temp_path() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mchat-ws-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(MultiChatWorkspaceDocument::FILE_NAME)
    }

    fn fast() -> MultiChatWorkspaceConfig {
        MultiChatWorkspaceConfig {
            debounce: Duration::from_millis(20),
            legacy_poll_interval: Duration::from_millis(1),
            legacy_poll_limit: 3,
        }
    }

    fn session(id: &str) -> WorkspaceSession {
        WorkspaceSession {
            channel_id: id.into(),
            channel_name: format!("name-{id}"),
            channel_image_url: Some(format!("https://img/{id}.png")),
            chat_channel_id_hint: None,
        }
    }

    fn write_doc(path: &std::path::Path, doc: &MultiChatWorkspaceDocument) {
        UserRecordFile::write(doc, path, None).unwrap();
    }

    #[tokio::test]
    async fn file_is_authoritative_and_clear_flag_keeps_layout() {
        let path = temp_path();
        let mut doc = MultiChatWorkspaceDocument::default();
        doc.live.sessions = vec![session("a")];
        doc.live.selected_channel_id = Some("a".into());
        doc.live.is_grid_layout = true;
        write_doc(&path, &doc);

        let defaults = Arc::new(MemoryKeyValueStore::new());
        let store =
            MultiChatWorkspaceStore::new(Some(path.clone()), defaults.clone(), fast(), None);
        store.wait_until_hydrated().await;
        assert_eq!(store.scope(MultiChatScope::Live), doc.live);

        defaults.set_bool(MultiChatWorkspaceStore::SHOULD_CLEAR_DEFAULTS_KEY, true);
        let store =
            MultiChatWorkspaceStore::new(Some(path.clone()), defaults.clone(), fast(), None);
        store.wait_until_hydrated().await;
        store.wait_for_pending_writes().await;
        let live = store.scope(MultiChatScope::Live);
        assert!(live.sessions.is_empty() && live.selected_channel_id.is_none());
        assert!(live.is_grid_layout, "레이아웃은 남긴다");
        assert!(!defaults.contains(MultiChatWorkspaceStore::SHOULD_CLEAR_DEFAULTS_KEY));
    }

    #[tokio::test]
    async fn repeated_updates_write_once_and_scopes_are_isolated() {
        let path = temp_path();
        write_doc(&path, &MultiChatWorkspaceDocument::default());
        let store = MultiChatWorkspaceStore::new(
            Some(path.clone()),
            MemoryKeyValueStore::shared(),
            fast(),
            None,
        );
        store.wait_until_hydrated().await;
        for _ in 0..5 {
            store.update(MultiChatScope::Standalone, |s| {
                s.selected_channel_id = Some("x".into())
            });
        }
        store.update(MultiChatScope::Live, |s| s.sessions = vec![session("l")]);
        store.flush().await;
        assert_eq!(store.disk_write_count(), 1);
        let reread: MultiChatWorkspaceDocument =
            UserRecordFile::read(&path, MultiChatWorkspaceDocument::VERSION).unwrap();
        assert_eq!(reread.standalone.selected_channel_id.as_deref(), Some("x"));
        assert_eq!(reread.live.sessions, vec![session("l")]);
    }

    #[tokio::test]
    async fn updates_before_hydration_apply_after_file_values() {
        let path = temp_path();
        let mut doc = MultiChatWorkspaceDocument::default();
        doc.live.sessions = vec![session("a")];
        write_doc(&path, &doc);
        let store =
            MultiChatWorkspaceStore::new(Some(path), MemoryKeyValueStore::shared(), fast(), None);
        store.update(MultiChatScope::Live, |s| {
            s.selected_channel_id = Some("a".into())
        });
        store.wait_until_hydrated().await;
        let live = store.scope(MultiChatScope::Live);
        assert_eq!(live.sessions, vec![session("a")]);
        assert_eq!(live.selected_channel_id.as_deref(), Some("a"));
    }

    #[tokio::test]
    async fn missing_file_without_settings_blocks_writes() {
        let path = temp_path();
        let store = MultiChatWorkspaceStore::new(
            Some(path.clone()),
            MemoryKeyValueStore::shared(),
            fast(),
            None,
        );
        store.wait_until_hydrated().await;
        store.update(MultiChatScope::Live, |s| s.is_grid_layout = true);
        store.flush().await;
        assert!(
            !path.exists(),
            "설정을 믿을 수 없으면 이번 실행은 쓰지 않는다"
        );
    }

    #[test]
    fn size_budget_and_lossy_decode() {
        let mut doc = MultiChatWorkspaceDocument::default();
        doc.live.sessions = (0..4).map(|i| session(&format!("{i:032}"))).collect();
        let bytes = UserRecordFile::encode(&doc).unwrap();
        assert!(bytes.len() <= 1024, "일반 문서 {}B", bytes.len());

        let raw = r#"{"sessions":[{"id":"a","name":"A"},{"broken":1}],"grid":true}"#;
        let scope: WorkspaceScope = serde_json::from_str(raw).unwrap();
        assert_eq!(scope.sessions.len(), 1);
        assert!(scope.is_grid_layout);
        assert_eq!(scope.grid_layout_mode, "preset");
        let encoded = serde_json::to_string(&WorkspaceScope::default()).unwrap();
        assert_eq!(encoded, r#"{"sessions":[]}"#, "기본 레이아웃은 쓰지 않는다");
    }
}
