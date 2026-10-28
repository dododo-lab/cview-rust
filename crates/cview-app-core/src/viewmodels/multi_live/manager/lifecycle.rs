//! 세션 추가·회수·제거·전체 해제 (`addSession`·`removeSession`·`stopAll`).

use std::collections::HashSet;
use std::sync::Arc;

use cview_core::models::LiveStatus;
use uuid::Uuid;

use super::{AddSessionOptions, LiveDetailFailure, ManagerState, MultiLiveManager};
use crate::viewmodels::multi_live::add_session_result::MultiLiveAddSessionResult;
use crate::viewmodels::multi_live::deps::{
    MultiLiveBackendError, MultiLiveSessionBackend, SharedHlsEngine,
};
use crate::viewmodels::multi_live::layout::{
    MultiLiveFocusDirection, MultiLiveGridLayoutMode, MultiLiveLayoutRatios,
};
use crate::viewmodels::multi_live::policy::{
    self, has_session_capacity, tuning, EvictionCandidate,
};
use crate::viewmodels::multi_live::session::MultiLiveSession;
use crate::viewmodels::support::cancel_slot;

impl MultiLiveManager {
    fn generation_ok(&self, generation: u64) -> bool {
        let st = self.read();
        st.teardown_generation == generation && !st.is_stopping_all
    }

    async fn return_uncommitted_engine(&self, engine: SharedHlsEngine, add_generation: u64) {
        let pool = self.shared.core.deps.engine_pool.clone();
        if self.generation_ok(add_generation) {
            pool.release(engine).await;
        } else {
            pool.discard(engine).await;
        }
    }

    async fn fetch_live_detail(
        backend: &Arc<dyn MultiLiveSessionBackend>,
        channel_id: &str,
    ) -> Result<cview_core::models::LiveInfo, LiveDetailFailure> {
        match tokio::time::timeout(tuning::LIVE_DETAIL_TIMEOUT, backend.live_detail(channel_id))
            .await
        {
            Err(_) => Err(LiveDetailFailure::Timeout),
            Ok(Err(MultiLiveBackendError::Cancelled)) => Err(LiveDetailFailure::Cancelled),
            Ok(Err(MultiLiveBackendError::Message(m))) => Err(LiveDetailFailure::Message(m)),
            Ok(Ok(info)) => Ok(info),
        }
    }

    /// 세션 추가. `watch_request_is_current` 가 `false` 를 돌려주면 대체된 요청으로 보고 취소한다.
    pub async fn add_session(
        &self,
        channel_id: &str,
        options: AddSessionOptions,
        watch_request_is_current: Option<&(dyn Fn() -> bool + Send + Sync)>,
    ) -> MultiLiveAddSessionResult {
        use MultiLiveAddSessionResult as R;
        let (gen_at_start, settings) = {
            let st = self.read();
            if st.is_stopping_all {
                tracing::info!("MultiLive: 전체 해제 진행 중 addSession 생략");
                return R::Cancelled;
            }
            (st.teardown_generation, st.settings.clone())
        };
        if let Some(store) = settings {
            let mut waited = 0;
            while !store.is_loaded() && waited < tuning::SETTINGS_LOAD_MAX_RETRIES {
                tokio::time::sleep(tuning::SETTINGS_LOAD_POLL).await;
                waited += 1;
            }
        }
        if !self.generation_ok(gen_at_start) {
            return R::Cancelled;
        }
        self.lock().apply_initial_layout_prefs_if_needed();
        self.sync_engine_pool_max().await;
        if !self.generation_ok(gen_at_start) {
            return R::Cancelled;
        }
        {
            let mut st = self.lock();
            if options.watch_transition && st.session_for_channel(channel_id).is_some() {
                return R::ActivatedExisting;
            }
            if !options.watch_transition
                && st.focus_existing_session(channel_id, options.start_immediately)
            {
                tracing::info!("MultiLive: 이미 존재하는 채널 — 재선택/재시작 처리");
                return R::ActivatedExisting;
            }
            if st.adding_channel_ids.contains(channel_id) {
                tracing::warn!("MultiLive: 이미 추가 진행 중");
                return R::AlreadyInProgress;
            }
            st.adding_channel_ids.insert(channel_id.to_string());
        }
        let result = self
            .add_session_inner(channel_id, options, watch_request_is_current, gen_at_start)
            .await;
        self.lock().adding_channel_ids.remove(channel_id);
        result
    }

    async fn add_session_inner(
        &self,
        channel_id: &str,
        options: AddSessionOptions,
        watch_request_is_current: Option<&(dyn Fn() -> bool + Send + Sync)>,
        gen_at_start: u64,
    ) -> MultiLiveAddSessionResult {
        use MultiLiveAddSessionResult as R;
        let is_current = || watch_request_is_current.is_none_or(|f| f());
        let Some(backend) = self.read().backend.clone() else {
            tracing::error!("MultiLive: API 클라이언트 없음");
            return R::ApiUnavailable;
        };
        let pool = self.shared.core.deps.engine_pool.clone();
        let mut validated_watch_info = None;
        if options.watch_transition {
            match Self::fetch_live_detail(&backend, channel_id).await {
                Ok(info) => {
                    if !self.generation_ok(gen_at_start) || !is_current() {
                        return R::Cancelled;
                    }
                    if info.status != LiveStatus::Open {
                        return R::NotLive;
                    }
                    validated_watch_info = Some(info);
                }
                Err(LiveDetailFailure::Timeout) => return R::LiveDetailTimeout,
                Err(LiveDetailFailure::Cancelled) => return R::Cancelled,
                Err(LiveDetailFailure::Message(m)) => return R::ApiError { message: m },
            }
        }
        let has_room_excluding_self = |st: &ManagerState| {
            !st.is_stopping_all
                && has_session_capacity(
                    st.sessions.len() as i64,
                    st.adding_channel_ids.len() as i64 - 1,
                    st.effective_max_sessions() as i64,
                )
        };
        if !is_current() {
            return R::Cancelled;
        }
        let (has_room, can_evict) = {
            let st = self.read();
            (
                has_room_excluding_self(&st),
                !st.is_grid_layout && !options.restoring_from_persistence,
            )
        };
        let allows_provisional_overflow = options.watch_transition && !has_room;
        if !has_room && !options.watch_transition {
            let evicted = if can_evict {
                self.evict_least_recently_watched_session(false, None).await
            } else {
                None
            };
            if !self.generation_ok(gen_at_start) || !is_current() {
                return R::Cancelled;
            }
            if evicted.is_none() {
                tracing::warn!("MultiLive: 최대 세션 수 도달");
                return R::MaxSessionsReached;
            }
        }
        if allows_provisional_overflow {
            let max = self.read().effective_max_sessions();
            pool.set_max_pool_size(max + 1).await;
        }
        let acquired = pool.acquire().await;
        if allows_provisional_overflow {
            self.sync_engine_pool_max().await;
        }
        let Some(engine) = acquired else {
            let active = pool.total_active_count().await;
            tracing::error!("MultiLive: 엔진 풀 할당 실패 — active={active}");
            return R::EngineUnavailable;
        };
        if !self.generation_ok(gen_at_start) {
            pool.discard(engine).await;
            return R::Cancelled;
        }
        if !is_current() {
            pool.release(engine).await;
            return R::Cancelled;
        }
        let live_info = match validated_watch_info {
            Some(info) => info,
            None => match Self::fetch_live_detail(&backend, channel_id).await {
                Ok(info) => info,
                Err(LiveDetailFailure::Timeout) => {
                    self.return_uncommitted_engine(engine, gen_at_start).await;
                    tracing::warn!("MultiLive: liveDetail 타임아웃 — 추가 취소");
                    return R::LiveDetailTimeout;
                }
                Err(LiveDetailFailure::Cancelled) => {
                    self.return_uncommitted_engine(engine, gen_at_start).await;
                    return R::Cancelled;
                }
                Err(LiveDetailFailure::Message(m)) => {
                    self.return_uncommitted_engine(engine, gen_at_start).await;
                    tracing::error!("MultiLive: 세션 추가 실패 — {m}");
                    return R::ApiError { message: m };
                }
            },
        };
        let torn_down = self.read().teardown_generation != gen_at_start;
        if torn_down || !is_current() {
            self.return_uncommitted_engine(engine, gen_at_start).await;
            return R::Cancelled;
        }
        enum Commit {
            Full,
            Existing,
            Added(Arc<MultiLiveSession>, bool),
        }
        let commit = {
            let mut st = self.lock();
            let limit = st.effective_max_sessions() + usize::from(allows_provisional_overflow);
            if st.sessions.len() >= limit {
                Commit::Full
            } else if (options.watch_transition && st.session_for_channel(channel_id).is_some())
                || (!options.watch_transition
                    && st.focus_existing_session(channel_id, options.start_immediately))
            {
                Commit::Existing
            } else {
                let channel_name = live_info
                    .channel
                    .as_ref()
                    .map(|c| c.channel_name.clone())
                    .unwrap_or_else(|| channel_id.to_string());
                let profile = live_info
                    .channel
                    .as_ref()
                    .and_then(|c| c.channel_image_url.clone());
                let factory = &st.core.deps.session_factory;
                let session = MultiLiveSession::with_live_info(
                    channel_id,
                    channel_name,
                    profile,
                    live_info,
                    backend.clone(),
                    factory.make_player(Some(engine.clone())),
                    factory.make_chat(),
                );
                session
                    .chat()
                    .set_user(st.user_uid.clone(), st.user_nickname.clone());
                let weak = self.shared.this_weak();
                session.set_started_hook(Some(Arc::new(move || {
                    if let Some(shared) = weak.upgrade() {
                        shared.obs.lock().reconcile_picture_enhancement();
                    }
                })));
                let is_first = st.sessions.is_empty();
                if !is_first || options.watch_transition || options.defer_auto_selection {
                    session.set_muted(true);
                    session.set_background_mode(true);
                }
                st.push_session(session.clone());
                Commit::Added(session, is_first)
            }
        };
        let (session, is_first) = match commit {
            Commit::Full => {
                tracing::warn!("MultiLive: await 이후 최대 세션 수 도달 — 추가 취소");
                pool.release(engine).await;
                return R::MaxSessionsReached;
            }
            Commit::Existing => {
                pool.release(engine).await;
                return R::ActivatedExisting;
            }
            Commit::Added(s, f) => (s, f),
        };
        let sid = session.id();
        let auto_select = is_first && !options.watch_transition && !options.defer_auto_selection;
        self.shared.core.bw().register_stream(sid, auto_select);
        {
            let st = self.read();
            if st.teardown_generation != gen_at_start || st.is_stopping_all || !st.contains(sid) {
                drop(st);
                self.shared.core.bw().unregister_stream(sid);
                return R::Cancelled;
            }
        }
        if watch_request_is_current.is_some_and(|f| !f()) {
            if self.read().selected_session_id != Some(sid) {
                self.remove_session(sid).await;
            }
            return R::Cancelled;
        }
        let over = {
            let st = self.read();
            st.sessions.len() > st.effective_max_sessions()
        };
        if over {
            if self
                .evict_least_recently_watched_session(true, Some(sid))
                .await
                .is_none()
            {
                self.remove_session(sid).await;
                return R::MaxSessionsReached;
            }
            let st = self.read();
            if st.teardown_generation != gen_at_start || st.is_stopping_all || !st.contains(sid) {
                return R::Cancelled;
            }
        }
        let mut st = self.lock();
        if auto_select {
            st.set_selected_session_id(Some(sid));
            if st.is_multi_audio_mode {
                st.audio_enabled_session_ids.insert(sid);
            }
            session.player().set_precision_sync_active(true);
            if !st.is_grid_layout {
                session.set_time_machine_desired_flag(true);
                st.stage_session_id = Some(sid);
            }
        }
        st.synchronize_bandwidth_selection();
        st.mru.note_added(sid, options.restoring_from_persistence);
        if options.start_immediately {
            let pane_count = st.sessions.len();
            st.schedule_session_start(&session, pane_count, None);
        }
        if st.sessions.len() >= 2 {
            if st.bw_coordinator_task.is_none() {
                st.start_bandwidth_coordination();
            } else {
                st.update_estimated_pane_sizes();
            }
        }
        st.save_state();
        st.reconcile_decode_budget();
        st.reconcile_hlsjs_pane_offscreen_surfaces();
        st.prewarm_next_watch_engine();
        tracing::info!("MultiLive: 세션 추가 — {channel_id}");
        R::Added { session_id: sid }
    }

    /// MRU 최하위 비보호 세션을 닫고 그 이름을 돌려준다.
    async fn evict_least_recently_watched_session(
        &self,
        allow_grid: bool,
        excluding: Option<Uuid>,
    ) -> Option<String> {
        let (victim_id, name) = {
            let st = self.read();
            if st.is_grid_layout && !allow_grid {
                return None;
            }
            let pinned: HashSet<Uuid> = [
                st.selected_session_id,
                st.stage_session_id,
                st.previous_stage_session_id,
                st.pip_session_id,
                st.audio_session_id,
                excluding,
            ]
            .into_iter()
            .flatten()
            .collect();
            let ordered: Vec<EvictionCandidate> = st
                .sessions
                .iter()
                .map(|s| EvictionCandidate {
                    id: s.id(),
                    tick: st.mru.tick(s.id()),
                    is_audible: st.is_audible_session(s),
                })
                .collect();
            let Some(victim) = policy::watch_eviction_victim(&ordered, &pinned)
                .and_then(|v| st.session(v).cloned())
            else {
                tracing::warn!("MultiLive: 시청 전환 회수 대상 없음 — 전부 선택/스테이지/PiP/가청");
                return None;
            };
            let channel_name = victim.channel_name();
            let name = if channel_name.is_empty() {
                victim.channel_id().to_string()
            } else {
                channel_name
            };
            (victim.id(), name)
        };
        tracing::info!("MultiLive: 시청 전환 자리 확보 — {name} 닫음 (MRU 최하위)");
        self.remove_session(victim_id).await;
        if let Some(toast) = &self.shared.core.deps.toast {
            toast.show(
                format!("채널 상한에 도달해 {name} 채널을 닫았습니다"),
                None,
                None,
            );
        }
        Some(name)
    }

    /// 세션 제거.
    pub async fn remove_session(&self, id: Uuid) {
        let (session, teardown_at_removal, chat_sessions) = {
            let mut st = self.lock();
            if st.removing_session_ids.contains(&id) || !st.contains(id) {
                return;
            }
            st.removing_session_ids.insert(id);
            let Some(session) = st.session(id).cloned() else {
                return;
            };
            session.invalidate_for_removal();
            let teardown = st.teardown_generation;
            if let Some(t) = st.session_startup_tasks.remove(&id) {
                t.cancel();
            }
            st.session_startup_generations.remove(&id);
            if let Some(t) = st.session_menu_lifecycle_tasks.remove(&id) {
                t.cancel();
            }
            st.session_menu_lifecycle_generations.remove(&id);
            if st.pip_session_id == Some(id) {
                st.core.deps.pip.stop_pip();
            }
            let Some(index) = st.sessions.iter().position(|s| s.id() == id) else {
                st.removing_session_ids.remove(&id);
                return;
            };
            st.sessions.remove(index);
            st.sessions_did_change();
            if st.selected_session_id == Some(id) {
                let first = st.sessions.first().map(|s| s.id());
                st.set_selected_session_id(first);
                if let Some(new_id) = first {
                    st.mru.touch(new_id);
                    st.synchronize_bandwidth_selection();
                }
                if let Some(new_selected) = st.selected_session().cloned() {
                    new_selected.set_background_mode(false);
                    new_selected.player().recover_from_background();
                    new_selected.player().set_precision_sync_active(true);
                }
            }
            if st.focus_main_session_id == Some(id) {
                let next = st
                    .selected_session_id
                    .or_else(|| st.sessions.first().map(|s| s.id()));
                st.set_focus_main_session_id(next);
            }
            if st.is_multi_audio_mode {
                st.audio_enabled_session_ids.remove(&id);
                if st.audio_session_id == Some(id) {
                    st.audio_session_id = st
                        .audio_enabled_session_ids
                        .iter()
                        .next()
                        .copied()
                        .or_else(|| st.sessions.first().map(|s| s.id()));
                }
            } else {
                let surviving = st.audio_session_id.filter(|a| st.contains(*a));
                if let Some(surviving) = surviving {
                    for s in &st.sessions {
                        if s.id() != surviving {
                            s.set_muted(true);
                        }
                    }
                } else if let Some(target) = st
                    .selected_session()
                    .or_else(|| st.sessions.first())
                    .map(|s| s.id())
                {
                    st.route_audio(target);
                } else {
                    st.audio_session_id = None;
                }
                st.audio_enabled_session_ids.remove(&id);
            }
            if st.sessions.len() <= 1 {
                st.set_grid_layout(false);
            }
            if st.sessions.is_empty() {
                st.has_applied_layout_prefs = false;
                st.is_multi_audio_mode = false;
                st.audio_enabled_session_ids.clear();
                st.audio_session_id = None;
            }
            if st.dragging_session_id == Some(id) {
                st.dragging_session_id = None;
            }
            if st.sessions.len() < 2 {
                st.stop_bandwidth_coordination();
                for s in &st.sessions {
                    s.player().set_max_rate_override(None);
                }
            } else {
                st.update_estimated_pane_sizes();
            }
            st.mru.forget(id);
            st.decode_active.remove(&id);
            if let Some(t) = st.tm_revert_tasks.remove(&id) {
                t.cancel();
            }
            if st.stage_session_id == Some(id) {
                st.stage_session_id = None;
            }
            if st.previous_stage_session_id == Some(id) {
                st.previous_stage_session_id = None;
                cancel_slot(&mut st.stage_handoff_hide_task);
            }
            st.reconcile_decode_budget();
            st.save_state();
            (session, teardown, st.chat_sessions.clone())
        };
        if let Some(cs) = chat_sessions {
            cs.remove_session_if_matching(session.channel_id(), session.chat());
        }
        tracing::info!("MultiLive: 세션 제거 — {}", session.channel_name());
        session.stop().await;
        let pool = self.shared.core.deps.engine_pool.clone();
        if let Some(engine) = session.player().detach_engine() {
            if self.read().teardown_generation == teardown_at_removal {
                pool.release(engine).await;
            } else {
                pool.discard(engine).await;
            }
        } else {
            pool.release_slot().await;
        }
        self.shared.core.bw().unregister_stream(id);
        let empty = {
            let mut st = self.lock();
            st.last_applied_cap_height.remove(&id);
            st.last_applied_hls_kbps.remove(&id);
            st.menu_paused_session_ids.remove(&id);
            st.menu_parked_session_ids.remove(&id);
            st.occlusion_suspended_session_ids.remove(&id);
            st.removing_session_ids.remove(&id);
            st.sessions.is_empty()
        };
        if empty {
            pool.trim_idle(1).await;
        }
    }

    /// 전체 해제.
    pub async fn stop_all(&self) {
        let to_tear_down = {
            let mut st = self.lock();
            if st.is_stopping_all {
                return;
            }
            st.is_stopping_all = true;
            st.teardown_generation = st.teardown_generation.wrapping_add(1);
            let sessions = st.sessions.clone();
            for s in &sessions {
                s.invalidate_for_removal();
            }
            for (_, t) in st.session_startup_tasks.drain() {
                t.cancel();
            }
            st.session_startup_generations.clear();
            for (_, t) in st.session_menu_lifecycle_tasks.drain() {
                t.cancel();
            }
            st.session_menu_lifecycle_generations.clear();
            if !st.sessions.is_empty() {
                st.current_layout_prefs()
                    .save(st.core.deps.layout_prefs.as_ref());
            }
            st.has_applied_layout_prefs = false;
            st.set_sessions(Vec::new());
            if let Some(cs) = &st.chat_sessions {
                cs.remove_borrowed_sessions_all();
            }
            st.set_selected_session_id(None);
            st.audio_session_id = None;
            st.is_multi_audio_mode = false;
            st.audio_enabled_session_ids.clear();
            st.set_grid_layout(false);
            st.set_grid_layout_mode(MultiLiveGridLayoutMode::Preset);
            st.set_layout_ratios(MultiLiveLayoutRatios::default());
            st.set_focus_direction(MultiLiveFocusDirection::Left);
            st.set_focus_main_session_id(None);
            st.dragging_session_index = None;
            st.dragging_session_id = None;
            st.bandwidth_loop_generation = st.bandwidth_loop_generation.wrapping_add(1);
            cancel_slot(&mut st.bw_coordinator_task);
            sessions
        };
        let pool = self.shared.core.deps.engine_pool.clone();
        for s in &to_tear_down {
            s.stop().await;
            match s.player().detach_engine() {
                Some(engine) => pool.release(engine).await,
                None => pool.release_slot().await,
            }
        }
        pool.drain().await;
        self.shared.core.bw().reset();
        let mut st = self.lock();
        st.clear_per_session_tracking();
        st.is_stopping_all = false;
    }
}
