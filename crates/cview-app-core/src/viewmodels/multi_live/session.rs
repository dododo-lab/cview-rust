//! 멀티라이브 세션 — 한 채널의 플레이어·채팅·상태 폴링·시청 기록.
//!
//! 원본: `Sources/CViewApp/ViewModels/MultiLiveSession.swift`.
//! `PlayerViewModel`·`ChatViewModel` 은 [`MultiLivePlayer`]·[`MultiLiveChat`] 로,
//! `ChzzkAPIClient`·`AppState` 는 [`MultiLiveSessionBackend`] 로 받는다.
//! Swift 의 `await previousTask?.value` 는 세대 카운터 재확인으로 대신한다.

use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use chrono::{DateTime, Utc};
use futures_util::future::BoxFuture;
use serde::Serialize;
use tokio::time::Instant;
use url::Url;
use uuid::Uuid;

use cview_core::models::match_web_v2::MatchWebV2Flag;
use cview_core::models::{format_viewer_count, LiveInfo, LiveStatus};
use cview_player::coordinator::StreamPhase;

use super::deps::{
    LiveMetadata, MultiLiveBackendError, MultiLiveChat, MultiLivePlayer, MultiLiveSessionBackend,
    MultiLiveStreamRequest, StreamEndedProbe, WatchRecordStart,
};
use super::policy::proactive_refresh_delay_secs;
use crate::viewmodels::support::{cancel_slot, slot_is, Observed, TaskSlot};

/// 세션 로드 상태.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(tag = "state", content = "detail", rename_all = "camelCase")]
pub enum MultiLiveLoadState {
    #[default]
    Idle,
    Loading,
    #[serde(rename_all = "camelCase")]
    Playing {
        channel_name: String,
        live_title: String,
    },
    Offline,
    Error(String),
}

impl MultiLiveLoadState {
    pub fn is_playing(&self) -> bool {
        matches!(self, Self::Playing { .. })
    }
}

/// 폴링 재시도 상한·예산 초기화 간격.
pub const POLL_MAX_TOTAL_RETRIES: u32 = 10;
pub const POLL_BUDGET_RESET_INTERVAL: Duration = Duration::from_secs(15 * 60);
pub const BACKGROUND_POLL_INTERVAL: Duration = Duration::from_secs(180);
pub const OFFLINE_RETRY_GRACE: Duration = Duration::from_secs(120);

/// 세션 관찰 상태.
#[derive(Debug, Default)]
pub struct SessionState {
    pub channel_name: String,
    pub live_title: String,
    pub thumbnail_url: Option<Url>,
    pub profile_image_url: Option<Url>,
    viewer_count: i64,
    formatted_viewer_count: String,
    pub accumulate_count: i64,
    pub open_date: Option<DateTime<Utc>>,
    pub is_offline: bool,
    pub is_audio_only: bool,
    pub is_watch_party: bool,
    pub load_state: MultiLiveLoadState,
    pub time_machine_desired: bool,
    time_machine_eligible: bool,
    time_machine_active: bool,
    time_machine_suppressed: bool,
    is_background: bool,
    is_menu_suspended: bool,
    chat_managed_by_multi_chat: bool,
    prefetched_live_info: Option<LiveInfo>,
    lifecycle_generation: u64,
    invalidated: bool,
    chat_connection_generation: u64,
    poll_total_retries: u32,
    poll_last_retry_at: Option<Instant>,
    watch_started_at: Option<DateTime<Utc>>,
    start_task: Option<TaskSlot>,
    poll_task: Option<TaskSlot>,
    refresh_task: Option<TaskSlot>,
    offline_retry_task: Option<TaskSlot>,
    chat_connection_task: Option<TaskSlot>,
    watch_checkpoint_task: Option<TaskSlot>,
}

impl SessionState {
    pub fn viewer_count(&self) -> i64 {
        self.viewer_count
    }

    pub fn set_viewer_count(&mut self, n: i64) {
        if self.viewer_count != n {
            self.viewer_count = n;
            self.formatted_viewer_count = format_viewer_count(n);
        }
    }

    pub fn formatted_viewer_count(&self) -> &str {
        if self.formatted_viewer_count.is_empty() {
            "0"
        } else {
            &self.formatted_viewer_count
        }
    }

    pub fn formatted_accumulate_count(&self) -> String {
        format_viewer_count(self.accumulate_count)
    }

    pub fn time_machine_eligible(&self) -> bool {
        self.time_machine_eligible
    }

    pub fn time_machine_active(&self) -> bool {
        self.time_machine_active
    }

    pub fn is_background(&self) -> bool {
        self.is_background
    }

    pub fn is_menu_suspended(&self) -> bool {
        self.is_menu_suspended
    }

    pub fn is_lifecycle_invalidated(&self) -> bool {
        self.invalidated
    }

    pub fn has_poll_task(&self) -> bool {
        self.poll_task.is_some()
    }

    pub fn watch_started_at(&self) -> Option<DateTime<Utc>> {
        self.watch_started_at
    }

    pub fn is_chat_presentation_managed_by_multi_chat(&self) -> bool {
        self.chat_managed_by_multi_chat
    }

    fn cancel_all_tasks(&mut self) {
        cancel_slot(&mut self.start_task);
        cancel_slot(&mut self.chat_connection_task);
        cancel_slot(&mut self.poll_task);
        cancel_slot(&mut self.refresh_task);
        cancel_slot(&mut self.offline_retry_task);
    }
}

/// 직렬화 스냅샷.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiLiveSessionSnapshot {
    pub id: Uuid,
    pub channel_id: String,
    pub channel_name: String,
    pub live_title: String,
    pub thumbnail_url: Option<String>,
    pub profile_image_url: Option<String>,
    pub viewer_count: i64,
    pub formatted_viewer_count: String,
    pub accumulate_count: i64,
    pub formatted_accumulate_count: String,
    pub open_date: Option<DateTime<Utc>>,
    pub is_offline: bool,
    pub is_audio_only: bool,
    pub is_watch_party: bool,
    pub is_background: bool,
    pub is_muted: bool,
    pub load_state: MultiLiveLoadState,
}

/// 멀티라이브 세션.
pub struct MultiLiveSession {
    id: Uuid,
    channel_id: String,
    player: Arc<dyn MultiLivePlayer>,
    chat: Arc<dyn MultiLiveChat>,
    backend: Mutex<Option<Arc<dyn MultiLiveSessionBackend>>>,
    started_hook: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    obs: Observed<SessionState>,
    this: Weak<MultiLiveSession>,
}

impl std::fmt::Debug for MultiLiveSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiLiveSession")
            .field("id", &self.id)
            .field("channel_id", &self.channel_id)
            .finish()
    }
}

impl MultiLiveSession {
    /// `init(channelId:engine:)` — 플레이어·채팅은 호출 측이 만든다.
    pub fn new(
        channel_id: impl Into<String>,
        player: Arc<dyn MultiLivePlayer>,
        chat: Arc<dyn MultiLiveChat>,
    ) -> Arc<Self> {
        let channel_id = channel_id.into();
        Arc::new_cyclic(|this| Self {
            id: Uuid::new_v4(),
            channel_id,
            player,
            chat,
            backend: Mutex::new(None),
            started_hook: Mutex::new(None),
            obs: Observed::new(SessionState::default()),
            this: this.clone(),
        })
    }

    /// 편의 init — 이름·프로필·미리 받은 LiveInfo·백엔드.
    pub fn with_live_info(
        channel_id: impl Into<String>,
        channel_name: impl Into<String>,
        profile_image_url: Option<Url>,
        live_info: LiveInfo,
        backend: Arc<dyn MultiLiveSessionBackend>,
        player: Arc<dyn MultiLivePlayer>,
        chat: Arc<dyn MultiLiveChat>,
    ) -> Arc<Self> {
        let session = Self::new(channel_id, player, chat);
        {
            let mut st = session.obs.lock();
            st.channel_name = channel_name.into();
            st.profile_image_url = profile_image_url;
            st.live_title = live_info.live_title.clone();
            st.thumbnail_url = live_info.live_image_url.clone();
            st.prefetched_live_info = Some(live_info);
        }
        *session.backend.lock().unwrap_or_else(|e| e.into_inner()) = Some(backend);
        session
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn channel_id(&self) -> &str {
        &self.channel_id
    }

    pub fn player(&self) -> &Arc<dyn MultiLivePlayer> {
        &self.player
    }

    pub fn chat(&self) -> &Arc<dyn MultiLiveChat> {
        &self.chat
    }

    pub fn lock(&self) -> crate::viewmodels::support::ObservedGuard<'_, SessionState> {
        self.obs.lock()
    }

    pub fn read(&self) -> crate::viewmodels::support::ObservedGuard<'_, SessionState> {
        self.obs.read()
    }

    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.obs.subscribe()
    }

    pub fn channel_name(&self) -> String {
        self.obs.read().channel_name.clone()
    }

    pub fn load_state(&self) -> MultiLiveLoadState {
        self.obs.read().load_state.clone()
    }

    pub fn is_muted(&self) -> bool {
        self.player.is_muted()
    }

    pub fn time_machine_desired(&self) -> bool {
        self.obs.read().time_machine_desired
    }

    pub fn set_time_machine_desired_flag(&self, desired: bool) {
        self.obs.lock().time_machine_desired = desired;
    }

    pub fn is_background(&self) -> bool {
        self.obs.read().is_background
    }

    pub fn set_backend(&self, backend: Option<Arc<dyn MultiLiveSessionBackend>>) {
        *self.backend.lock().unwrap_or_else(|e| e.into_inner()) = backend;
    }

    fn backend(&self) -> Option<Arc<dyn MultiLiveSessionBackend>> {
        self.backend
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// 시작 완료 후 호출 (`appState.multiLiveManager.reconcilePictureEnhancement()`).
    pub fn set_started_hook(&self, hook: Option<Arc<dyn Fn() + Send + Sync>>) {
        *self.started_hook.lock().unwrap_or_else(|e| e.into_inner()) = hook;
    }

    pub fn snapshot(&self) -> MultiLiveSessionSnapshot {
        let st = self.obs.read();
        MultiLiveSessionSnapshot {
            id: self.id,
            channel_id: self.channel_id.clone(),
            channel_name: st.channel_name.clone(),
            live_title: st.live_title.clone(),
            thumbnail_url: st.thumbnail_url.as_ref().map(|u| u.to_string()),
            profile_image_url: st.profile_image_url.as_ref().map(|u| u.to_string()),
            viewer_count: st.viewer_count,
            formatted_viewer_count: st.formatted_viewer_count().to_string(),
            accumulate_count: st.accumulate_count,
            formatted_accumulate_count: st.formatted_accumulate_count(),
            open_date: st.open_date,
            is_offline: st.is_offline,
            is_audio_only: st.is_audio_only,
            is_watch_party: st.is_watch_party,
            is_background: st.is_background,
            is_muted: self.player.is_muted(),
            load_state: st.load_state.clone(),
        }
    }

    fn apply_live_info_metadata(&self, info: &LiveMetadata) {
        let (viewers, accumulate, open_date) = {
            let mut st = self.obs.lock();
            st.set_viewer_count(info.concurrent_user_count);
            if info.accumulate_count > 0 && st.accumulate_count != info.accumulate_count {
                st.accumulate_count = info.accumulate_count;
            }
            if let Some(d) = info.open_date {
                if st.open_date != Some(d) {
                    st.open_date = Some(d);
                }
            }
            (
                info.concurrent_user_count,
                (info.accumulate_count > 0).then_some(info.accumulate_count),
                info.open_date,
            )
        };
        self.chat
            .set_live_metadata(Some(viewers), accumulate, open_date);
    }

    /// 제거 직전 — 이후 시작/재시도/폴링을 모두 막는다.
    pub fn invalidate_for_removal(&self) {
        let mut st = self.obs.lock();
        if st.invalidated {
            return;
        }
        st.invalidated = true;
        st.lifecycle_generation = st.lifecycle_generation.wrapping_add(1);
        st.chat_connection_generation = st.chat_connection_generation.wrapping_add(1);
        st.cancel_all_tasks();
    }

    /// `start(paneCount:)`.
    pub async fn start(&self, pane_count: usize) {
        {
            let st = self.obs.read();
            if st.invalidated || st.load_state == MultiLiveLoadState::Loading {
                return;
            }
        }
        let Some(backend) = self.backend() else {
            self.obs.lock().load_state = MultiLiveLoadState::Error("API 클라이언트 없음".into());
            self.finish_watch_tracking().await;
            return;
        };
        let Some(this) = self.this.upgrade() else {
            return;
        };
        this.start_using(backend, pane_count, false).await;
    }

    fn generation_is(&self, generation: u64) -> bool {
        let st = self.obs.read();
        st.lifecycle_generation == generation
    }

    fn generation_live(&self, generation: u64) -> bool {
        let st = self.obs.read();
        st.lifecycle_generation == generation && !st.invalidated
    }

    /// `start(using:appState:paneCount:reuseChat:)`.
    pub fn start_using(
        self: Arc<Self>,
        backend: Arc<dyn MultiLiveSessionBackend>,
        pane_count: usize,
        reuse_chat: bool,
    ) -> BoxFuture<'static, ()> {
        Box::pin(async move { self.start_inner(backend, pane_count, reuse_chat).await })
    }

    async fn start_inner(
        self: Arc<Self>,
        backend: Arc<dyn MultiLiveSessionBackend>,
        pane_count: usize,
        reuse_chat: bool,
    ) {
        let (generation, prefetched) = {
            let mut st = self.obs.lock();
            if st.invalidated || st.load_state == MultiLiveLoadState::Loading {
                return;
            }
            st.lifecycle_generation = st.lifecycle_generation.wrapping_add(1);
            st.load_state = MultiLiveLoadState::Loading;
            (st.lifecycle_generation, st.prefetched_live_info.take())
        };
        self.set_backend(Some(backend.clone()));
        {
            let weak = self.this.clone();
            let b = Arc::downgrade(&backend);
            self.player
                .set_on_playback_state_changed(Some(Arc::new(move || {
                    if let Some(b) = b.upgrade() {
                        b.note_playback_activity();
                    }
                    if let Some(s) = weak.upgrade() {
                        s.sync_load_state_with_stream_phase();
                    }
                })));
        }
        let tm_enabled = backend.time_machine_playback_enabled();
        let channel_id = self.channel_id.clone();
        let viewer_fut = async {
            if tm_enabled {
                backend.viewer_cheat_key(&channel_id).await
            } else {
                None
            }
        };
        let live_fut = async {
            match prefetched {
                Some(info) => Ok(info),
                None => backend.live_detail(&channel_id).await,
            }
        };
        let (cheat_key, live) = tokio::join!(viewer_fut, live_fut);
        let live = match live {
            Ok(l) => l,
            Err(MultiLiveBackendError::Cancelled) => {
                let mut st = self.obs.lock();
                if st.lifecycle_generation == generation {
                    st.load_state = MultiLiveLoadState::Idle;
                }
                return;
            }
            Err(MultiLiveBackendError::Message(m)) => {
                if !self.generation_is(generation) {
                    return;
                }
                self.obs.lock().load_state = MultiLiveLoadState::Error(m);
                self.finish_watch_tracking().await;
                return;
            }
        };
        if !self.generation_is(generation) {
            return;
        }
        let mut plan = match backend.build_plan(&live, &channel_id) {
            Ok(p) => p,
            Err(e) => {
                self.obs.lock().load_state = MultiLiveLoadState::Error(e.message().into());
                self.finish_watch_tracking().await;
                return;
            }
        };
        let is_cheat_key_user = cheat_key == Some(true);
        let mut is_time_machine_available = false;
        let live_plan = plan.clone();
        let (suppressed, desired) = {
            let mut st = self.obs.lock();
            st.time_machine_eligible = tm_enabled && is_cheat_key_user && live.time_machine_active;
            (st.time_machine_suppressed, st.time_machine_desired)
        };
        if tm_enabled && is_cheat_key_user && live.time_machine_active && !suppressed && desired {
            let json = if live.time_machine_playback {
                live.live_playback_json.clone()
            } else {
                backend.time_machine_playback_json(&channel_id).await
            };
            if let Some(tm_plan) = json.and_then(|j| backend.enabling_time_machine(&plan, &j)) {
                plan = tm_plan;
                is_time_machine_available = true;
            }
        }
        if !self.generation_is(generation) {
            return;
        }
        self.player
            .configure_time_machine_access(is_cheat_key_user, is_time_machine_available);
        let is_watch_party = live.is_watch_party();
        let (channel_name, live_title, is_audio_only) = {
            let mut st = self.obs.lock();
            st.time_machine_active = is_time_machine_available;
            st.is_audio_only = plan.is_audio_only;
            st.is_watch_party = is_watch_party;
            st.channel_name = if plan.channel_name.is_empty() {
                channel_id.clone()
            } else {
                plan.channel_name.clone()
            };
            st.live_title = plan.live_title.clone();
            st.thumbnail_url = live.live_image_url.clone();
            (
                st.channel_name.clone(),
                st.live_title.clone(),
                st.is_audio_only,
            )
        };
        self.apply_live_info_metadata(&LiveMetadata::from(&live));
        let ps = backend.player_settings();
        self.obs.lock().load_state = MultiLiveLoadState::Playing {
            channel_name: channel_name.clone(),
            live_title: live_title.clone(),
        };
        {
            let b = Arc::downgrade(&backend);
            let cid = channel_id.clone();
            let probe: StreamEndedProbe = Arc::new(move || {
                let b = b.clone();
                let cid = cid.clone();
                Box::pin(async move {
                    let Some(api) = b.upgrade() else {
                        return false;
                    };
                    api.live_status(&cid)
                        .await
                        .map(|s| s.is_closed)
                        .unwrap_or(false)
                })
            });
            self.player.set_stream_ended_probe(Some(probe));
        }
        if !reuse_chat {
            if let Some(chat_channel_id) = live.chat_channel_id.clone() {
                self.schedule_chat_connection(backend.clone(), chat_channel_id);
            }
        }
        let request = |plan: &super::deps::LaunchPlan, audio_only: bool| MultiLiveStreamRequest {
            channel_id: channel_id.clone(),
            stream_url: plan.stream_url.clone(),
            channel_name: channel_name.clone(),
            live_title: live_title.clone(),
            plan: plan.clone(),
            match_web_v2_source_eligible: MatchWebV2Flag::source_eligible(
                is_watch_party,
                audio_only,
            ),
        };
        self.player
            .start_stream(request(&plan, is_audio_only))
            .await;
        if !self.generation_is(generation) {
            return;
        }
        if self.player.error_message().is_some() && is_time_machine_available {
            tracing::warn!("멀티라이브 타임머신 스트림 시작 실패 — 일반 라이브로 폴백");
            {
                let mut st = self.obs.lock();
                st.time_machine_suppressed = true;
                st.time_machine_active = false;
                st.is_audio_only = live_plan.is_audio_only;
            }
            self.player
                .configure_time_machine_access(is_cheat_key_user, false);
            self.player
                .start_stream(request(&live_plan, live_plan.is_audio_only))
                .await;
            if !self.generation_is(generation) {
                return;
            }
        }
        if let Some(error) = self.player.error_message() {
            self.obs.lock().load_state = MultiLiveLoadState::Error(error);
            self.finish_watch_tracking().await;
            return;
        }
        self.begin_watch_tracking(&backend, generation).await;
        if !self.generation_is(generation) {
            return;
        }
        self.player.apply_started_settings(&ps);
        let hook = self
            .started_hook
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(hook) = hook {
            hook();
        }
        self.start_polling();
        let should_switch = {
            let st = self.obs.read();
            st.lifecycle_generation == generation
                && st.time_machine_eligible
                && !st.time_machine_suppressed
                && st.time_machine_active != st.time_machine_desired
        };
        if should_switch {
            let weak = self.this.clone();
            tokio::spawn(async move {
                if let Some(s) = weak.upgrade() {
                    s.perform_time_machine_stream_switch(pane_count).await;
                }
            });
        }
    }

    /// 스테이지 여부에 따른 타임머신 희망 전환.
    pub async fn set_time_machine_desired(&self, desired: bool, pane_count: usize) {
        {
            let mut st = self.obs.lock();
            if st.invalidated || st.time_machine_desired == desired {
                return;
            }
            st.time_machine_desired = desired;
        }
        if let Some(this) = self.this.upgrade() {
            this.perform_time_machine_stream_switch(pane_count).await;
        }
    }

    fn perform_time_machine_stream_switch(
        self: Arc<Self>,
        pane_count: usize,
    ) -> BoxFuture<'static, ()> {
        Box::pin(async move {
            let generation = {
                let mut st = self.obs.lock();
                if st.invalidated || !st.time_machine_eligible {
                    return;
                }
                if st.time_machine_desired && st.time_machine_suppressed {
                    return;
                }
                if st.time_machine_active == st.time_machine_desired || !st.load_state.is_playing()
                {
                    return;
                }
                st.lifecycle_generation = st.lifecycle_generation.wrapping_add(1);
                cancel_slot(&mut st.start_task);
                st.lifecycle_generation
            };
            let Some(backend) = self.backend() else {
                return;
            };
            tracing::info!(
                "멀티라이브 타임머신 스트림 {} 전환",
                if self.time_machine_desired() {
                    "활성"
                } else {
                    "해제"
                }
            );
            self.player.stop_stream().await;
            if !self.generation_live(generation) {
                return;
            }
            self.clone().start_using(backend, pane_count, true).await;
        })
    }

    fn sync_load_state_with_stream_phase(&self) {
        let phase = self.player.stream_phase();
        let tm_available =
            matches!(phase, StreamPhase::Error(_)) && self.player.is_time_machine_available();
        let mut st = self.obs.lock();
        match phase {
            StreamPhase::Error(message) => {
                if tm_available {
                    st.time_machine_suppressed = true;
                }
                if st.load_state.is_playing() {
                    st.load_state = MultiLiveLoadState::Error(message);
                }
            }
            StreamPhase::Playing
                if matches!(st.load_state, MultiLiveLoadState::Error(_)) && !st.is_offline =>
            {
                st.load_state = MultiLiveLoadState::Playing {
                    channel_name: st.channel_name.clone(),
                    live_title: st.live_title.clone(),
                };
            }
            _ => {}
        }
    }

    fn schedule_chat_connection(
        &self,
        backend: Arc<dyn MultiLiveSessionBackend>,
        chat_channel_id: String,
    ) {
        let mut st = self.obs.lock();
        st.chat_connection_generation = st.chat_connection_generation.wrapping_add(1);
        let generation = st.chat_connection_generation;
        cancel_slot(&mut st.chat_connection_task);
        let weak = self.this.clone();
        let chat = self.chat.clone();
        let stream_channel_id = self.channel_id.clone();
        let display_name = st.channel_name.clone();
        let backend = Arc::downgrade(&backend);
        st.chat_connection_task = Some(TaskSlot::spawn(move |_| async move {
            let current = |w: &Weak<MultiLiveSession>| {
                w.upgrade().is_some_and(|s| {
                    let st = s.obs.read();
                    st.chat_connection_generation == generation && !st.invalidated
                })
            };
            if !current(&weak) {
                return;
            }
            let Some(api) = backend.upgrade() else {
                return;
            };
            let result = api
                .connect_chat(chat, &chat_channel_id, &stream_channel_id, &display_name)
                .await;
            if let Err(e) = result {
                if current(&weak) {
                    tracing::error!("멀티라이브 채팅 연결 실패: {e}");
                }
            }
        }));
    }

    pub fn restart_chat_header_polling(&self) {
        let Some(backend) = self.backend() else {
            return;
        };
        if !self.chat.has_last_connection_config() {
            return;
        }
        backend.restart_chat_header_polling(&self.chat, &self.channel_id);
    }

    async fn begin_watch_tracking(
        &self,
        backend: &Arc<dyn MultiLiveSessionBackend>,
        generation: u64,
    ) {
        let record = {
            let st = self.obs.read();
            if st.watch_started_at.is_some() {
                return;
            }
            WatchRecordStart {
                channel_id: self.channel_id.clone(),
                channel_name: st.channel_name.clone(),
                thumbnail_url: st.thumbnail_url.as_ref().map(|u| u.to_string()),
                avatar_url: st.profile_image_url.as_ref().map(|u| u.to_string()),
            }
        };
        match backend.start_watch_record(&record).await {
            None => {}
            Some(Ok(started_at)) => {
                if !self.generation_is(generation) {
                    let _ = backend.end_watch_record(&self.channel_id, started_at).await;
                    return;
                }
                self.obs.lock().watch_started_at = Some(started_at);
                self.start_watch_checkpointing(backend, started_at);
            }
            Some(Err(e)) => tracing::error!("멀티라이브 시청 기록 시작 실패: {e}"),
        }
    }

    fn start_watch_checkpointing(
        &self,
        backend: &Arc<dyn MultiLiveSessionBackend>,
        started_at: DateTime<Utc>,
    ) {
        let weak = self.this.clone();
        let api = Arc::downgrade(backend);
        let interval = backend.watch_checkpoint_interval();
        let channel_id = self.channel_id.clone();
        let mut st = self.obs.lock();
        cancel_slot(&mut st.watch_checkpoint_task);
        st.watch_checkpoint_task = Some(TaskSlot::spawn(move |_| async move {
            loop {
                tokio::time::sleep(interval).await;
                let Some(s) = weak.upgrade() else { break };
                let Some(api) = api.upgrade() else { break };
                let (still_tracking, suspended, playing) = {
                    let st = s.obs.read();
                    (
                        st.watch_started_at == Some(started_at),
                        st.is_menu_suspended,
                        st.load_state.is_playing(),
                    )
                };
                if !still_tracking {
                    break;
                }
                if suspended {
                    continue;
                }
                if !playing || !s.player.stream_phase().requires_playback_resources() {
                    continue;
                }
                match api.checkpoint_watch_record(&channel_id, started_at).await {
                    None => break,
                    Some(Ok(false)) => break,
                    Some(Ok(true)) => {}
                    Some(Err(e)) => tracing::warn!("멀티라이브 시청 체크포인트 실패: {e}"),
                }
            }
        }));
    }

    pub fn suspend_polling_for_menu_leave(&self) {
        self.obs.lock().is_menu_suspended = true;
    }

    pub fn resume_polling_for_menu_return(&self) {
        {
            let mut st = self.obs.lock();
            if !st.is_menu_suspended {
                return;
            }
            st.is_menu_suspended = false;
        }
        let weak = self.this.clone();
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::spawn(async move {
                if let Some(s) = weak.upgrade() {
                    s.poll_live_status_once().await;
                }
            });
        }
    }

    async fn poll_live_status_once(&self) {
        let Some(backend) = self.backend() else {
            return;
        };
        if let Ok(status) = backend.live_status(&self.channel_id).await {
            self.apply_live_info_metadata(&LiveMetadata::from(&status));
        }
    }

    async fn finish_watch_tracking(&self) {
        let started_at = {
            let mut st = self.obs.lock();
            cancel_slot(&mut st.watch_checkpoint_task);
            st.watch_started_at.take()
        };
        let (Some(started_at), Some(backend)) = (started_at, self.backend()) else {
            return;
        };
        match backend.end_watch_record(&self.channel_id, started_at).await {
            None => {}
            Some(Ok(did_end)) => {
                if !did_end {
                    tracing::warn!("멀티라이브 종료 시청 기록 없음");
                }
                backend.nudge_watch_history_sync().await;
            }
            Some(Err(e)) => tracing::error!("멀티라이브 시청 기록 종료 실패: {e}"),
        }
    }

    pub async fn stop(&self) {
        {
            let mut st = self.obs.lock();
            st.lifecycle_generation = st.lifecycle_generation.wrapping_add(1);
            st.time_machine_suppressed = false;
            st.time_machine_active = false;
            cancel_slot(&mut st.start_task);
            cancel_slot(&mut st.poll_task);
            cancel_slot(&mut st.refresh_task);
            cancel_slot(&mut st.offline_retry_task);
            st.chat_connection_generation = st.chat_connection_generation.wrapping_add(1);
            cancel_slot(&mut st.chat_connection_task);
        }
        self.player.stop_stream().await;
        self.chat.disconnect().await;
        self.finish_watch_tracking().await;
        self.obs.lock().load_state = MultiLiveLoadState::Idle;
    }

    pub async fn retry(&self) {
        let generation = {
            let mut st = self.obs.lock();
            if st.invalidated {
                return;
            }
            st.lifecycle_generation = st.lifecycle_generation.wrapping_add(1);
            cancel_slot(&mut st.start_task);
            st.lifecycle_generation
        };
        self.player.stop_stream().await;
        if !self.generation_live(generation) {
            return;
        }
        let (Some(backend), Some(this)) = (self.backend(), self.this.upgrade()) else {
            return;
        };
        this.perform_retry(backend).await;
    }

    fn schedule_automatic_retry(&self, reason: &'static str) {
        let mut st = self.obs.lock();
        if st.invalidated {
            return;
        }
        cancel_slot(&mut st.start_task);
        let weak = self.this.clone();
        st.start_task = Some(TaskSlot::spawn(move |id| async move {
            let Some(s) = weak.upgrade() else { return };
            if s.obs.read().invalidated {
                return;
            }
            let Some(backend) = s.backend() else { return };
            tracing::info!("멀티라이브 자동 재시작 예약 reason={reason}");
            s.clone().perform_retry(backend).await;
            let mut st = s.obs.lock();
            if slot_is(&st.start_task, id) {
                st.start_task = None;
            }
        }));
    }

    fn schedule_player_only_refresh(&self, reason: &'static str) {
        let mut st = self.obs.lock();
        if st.invalidated {
            return;
        }
        cancel_slot(&mut st.start_task);
        let weak = self.this.clone();
        st.start_task = Some(TaskSlot::spawn(move |id| async move {
            let Some(s) = weak.upgrade() else { return };
            if s.obs.read().invalidated {
                return;
            }
            let Some(backend) = s.backend() else { return };
            let playing = s.obs.read().load_state.is_playing();
            if !playing {
                tracing::info!("멀티라이브 자동 재시작 예약 reason={reason}");
                s.clone().perform_retry(backend).await;
            } else {
                tracing::info!("멀티라이브 플레이어 URL 재취득 reason={reason}");
                let generation = {
                    let mut st = s.obs.lock();
                    st.lifecycle_generation = st.lifecycle_generation.wrapping_add(1);
                    cancel_slot(&mut st.poll_task);
                    cancel_slot(&mut st.refresh_task);
                    st.lifecycle_generation
                };
                s.player.stop_stream().await;
                if !s.generation_live(generation) {
                    return;
                }
                s.clone().start_using(backend, 1, true).await;
            }
            let mut st = s.obs.lock();
            if slot_is(&st.start_task, id) {
                st.start_task = None;
            }
        }));
    }

    fn perform_retry(
        self: Arc<Self>,
        backend: Arc<dyn MultiLiveSessionBackend>,
    ) -> BoxFuture<'static, ()> {
        Box::pin(async move {
            let generation = {
                let mut st = self.obs.lock();
                if st.invalidated {
                    return;
                }
                st.lifecycle_generation = st.lifecycle_generation.wrapping_add(1);
                cancel_slot(&mut st.poll_task);
                cancel_slot(&mut st.refresh_task);
                cancel_slot(&mut st.offline_retry_task);
                st.chat_connection_generation = st.chat_connection_generation.wrapping_add(1);
                cancel_slot(&mut st.chat_connection_task);
                st.lifecycle_generation
            };
            if !self.generation_live(generation) {
                return;
            }
            self.player.stop_stream().await;
            if !self.generation_live(generation) {
                return;
            }
            self.chat.disconnect().await;
            if !self.generation_live(generation) {
                return;
            }
            {
                let mut st = self.obs.lock();
                st.load_state = MultiLiveLoadState::Idle;
                st.is_offline = false;
            }
            self.clone().start_using(backend, 1, false).await;
        })
    }

    pub async fn refresh_stream(&self) {
        if self.obs.read().invalidated {
            return;
        }
        self.stop().await;
        if self.obs.read().invalidated {
            return;
        }
        let (Some(backend), Some(this)) = (self.backend(), self.this.upgrade()) else {
            return;
        };
        this.start_using(backend, 1, false).await;
    }

    pub fn set_background_mode(&self, background: bool) {
        let managed = {
            let mut st = self.obs.lock();
            if st.is_background != background {
                st.is_background = background;
            }
            st.chat_managed_by_multi_chat
        };
        if !managed {
            self.chat.set_background_mode(background);
        }
    }

    pub fn set_chat_presentation_managed_by_multi_chat(&self, managed: bool) {
        let background = {
            let mut st = self.obs.lock();
            if st.chat_managed_by_multi_chat == managed {
                return;
            }
            st.chat_managed_by_multi_chat = managed;
            if managed {
                return;
            }
            st.is_background
        };
        self.chat.restore_owner_presentation(background);
    }

    pub fn set_muted(&self, muted: bool) {
        self.player.set_muted(muted);
    }

    fn start_polling(&self) {
        let weak = self.this.clone();
        {
            let mut st = self.obs.lock();
            cancel_slot(&mut st.poll_task);
            st.poll_task = Some(TaskSlot::spawn(move |_| poll_loop(weak)));
        }
        self.schedule_proactive_refresh();
    }

    fn schedule_proactive_refresh(&self) {
        let delay = Duration::from_secs_f64(proactive_refresh_delay_secs(rand::random::<f64>()));
        let weak = self.this.clone();
        let mut st = self.obs.lock();
        cancel_slot(&mut st.refresh_task);
        st.refresh_task = Some(TaskSlot::spawn(move |_| async move {
            tokio::time::sleep(delay).await;
            loop {
                let offline = weak.upgrade().is_none_or(|s| s.obs.read().is_offline);
                if offline {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    continue;
                }
                let Some(s) = weak.upgrade() else { break };
                if s.backend().is_none() {
                    break;
                }
                tracing::debug!("멀티라이브 주기적 URL 재취득 — CDN 토큰 만료 예방");
                if let Some(engine) = s.player.engine() {
                    engine.reset_retries();
                }
                s.schedule_player_only_refresh("CDN token refresh");
                return;
            }
        }));
    }
}

async fn poll_loop(weak: Weak<MultiLiveSession>) {
    let mut consecutive_errors = 0u32;
    loop {
        let interval = match weak.upgrade() {
            Some(s) if s.obs.read().is_background => BACKGROUND_POLL_INTERVAL,
            Some(s) => s
                .backend()
                .map(|b| b.foreground_poll_interval())
                .unwrap_or(Duration::from_secs(60)),
            None => break,
        };
        tokio::time::sleep(interval).await;
        let Some(s) = weak.upgrade() else { break };
        let Some(backend) = s.backend() else { break };
        if s.obs.read().is_menu_suspended {
            continue;
        }
        match backend.live_status(&s.channel_id).await {
            Ok(status) => {
                s.apply_live_info_metadata(&LiveMetadata::from(&status));
                if status.is_closed {
                    let newly_offline = {
                        let mut st = s.obs.lock();
                        if st.is_offline {
                            false
                        } else {
                            st.is_offline = true;
                            st.load_state = MultiLiveLoadState::Offline;
                            true
                        }
                    };
                    if newly_offline {
                        s.finish_watch_tracking().await;
                        let w = s.this.clone();
                        tokio::spawn(async move {
                            if let Some(s) = w.upgrade() {
                                s.player.stop_stream().await;
                            }
                        });
                        let w = s.this.clone();
                        let mut st = s.obs.lock();
                        cancel_slot(&mut st.offline_retry_task);
                        st.offline_retry_task = Some(TaskSlot::spawn(move |_| async move {
                            tokio::time::sleep(OFFLINE_RETRY_GRACE).await;
                            let Some(s) = w.upgrade() else { return };
                            if !s.obs.read().is_offline || s.backend().is_none() {
                                return;
                            }
                            s.schedule_automatic_retry("offline grace elapsed");
                        }));
                    }
                } else {
                    s.obs.lock().is_offline = false;
                }
            }
            Err(e) => tracing::debug!("멀티라이브 상태 폴링 실패: {e:?}"),
        }
        if s.obs.read().is_offline {
            consecutive_errors = 0;
            continue;
        }
        let in_error = s.player.engine().is_some_and(|e| e.is_in_error_state());
        if !in_error {
            consecutive_errors = 0;
            continue;
        }
        consecutive_errors += 1;
        tracing::warn!("멀티라이브 엔진 ERROR ({consecutive_errors}연속)");
        if consecutive_errors < 2 {
            continue;
        }
        let exceeded = {
            let mut st = s.obs.lock();
            let now = Instant::now();
            if let Some(last) = st.poll_last_retry_at {
                if now.duration_since(last) > POLL_BUDGET_RESET_INTERVAL {
                    st.poll_total_retries = 0;
                }
            }
            st.poll_total_retries += 1;
            st.poll_last_retry_at = Some(now);
            if st.poll_total_retries > POLL_MAX_TOTAL_RETRIES {
                st.load_state = MultiLiveLoadState::Error("재시도 한도 초과".into());
                true
            } else {
                false
            }
        };
        if exceeded {
            tracing::error!(
                "멀티라이브 최대 재시도 초과({POLL_MAX_TOTAL_RETRIES}회) — 재시도 중단"
            );
            s.finish_watch_tracking().await;
            break;
        }
        s.schedule_automatic_retry("engine health check");
        return;
    }
}

/// LiveInfo 상태가 OPEN 인지.
pub fn live_is_open(info: &LiveInfo) -> bool {
    info.status == LiveStatus::Open
}
