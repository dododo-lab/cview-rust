//! 연결·해제·재연결.
//!
//! 원본: `Sources/CViewApp/ViewModels/ChatViewModel+Connection.swift`.
//! 실제 `await` 는 엔진 해제·연결 두 곳뿐이다 — 그 경계마다 요청/세션 세대와 엔진 정체성을
//! 다시 확인하는 Swift 가드를 그대로 둔다.

use std::sync::Arc;

use futures_util::FutureExt;

use cview_chat::engine::TokenProvider;
use cview_chat::{ChatEngineConfiguration, ChatModerationService};
use cview_core::models::chat::ChatConnectionState;
use cview_networking::ChzzkApiClient;

use super::{ChatEngineHandle, ChatState, ChatViewModel, ConnectionConfig};
use crate::viewmodels::support::cancel_slot;

/// `connect(...)` 인자.
#[derive(Clone, Default)]
pub struct ConnectRequest {
    pub chat_channel_id: String,
    pub access_token: String,
    pub extra_token: Option<String>,
    pub uid: Option<String>,
    pub channel_id: Option<String>,
    pub channel_name: Option<String>,
    pub token_provider: Option<TokenProvider>,
}

impl ConnectRequest {
    pub fn new(chat_channel_id: impl Into<String>, access_token: impl Into<String>) -> Self {
        Self {
            chat_channel_id: chat_channel_id.into(),
            access_token: access_token.into(),
            ..Default::default()
        }
    }
}

fn same_engine(a: &Option<Arc<dyn ChatEngineHandle>>, b: &Arc<dyn ChatEngineHandle>) -> bool {
    a.as_ref().is_some_and(|a| Arc::ptr_eq(a, b))
}

impl ChatViewModel {
    /// 재연결 때 새 채팅 토큰을 받아 오는 공급자.
    pub fn make_token_provider(
        api_client: ChzzkApiClient,
        chat_channel_id: String,
    ) -> TokenProvider {
        Arc::new(move || {
            let client = api_client.clone();
            let id = chat_channel_id.clone();
            async move {
                client
                    .chat_access_token(&id)
                    .await
                    .map(|t| (t.access_token, t.extra_token))
                    .map_err(|e| cview_chat::ChatError::ConnectionFailed(e.to_string()))
            }
            .boxed()
        })
    }

    /// 연결 요청 순번을 잡는다. 셸은 토큰·사용자 정보를 받아 오는 동안(`await`) 다른 요청이 끼어들 수 있어,
    /// 받기 전에 순번을 잡고 [`Self::connect_if_current`] 로 넘긴다 — 늦게 끝난 이전 요청이 새 채널을 덮지 않게.
    /// 순번을 올리는 것만으로 진행 중인 이전 `connect` 는 다음 확인 지점에서 물러난다.
    pub fn begin_connect_request(&self) -> u64 {
        let mut s = self.lock();
        s.connection_request_generation = s.connection_request_generation.wrapping_add(1);
        s.connection_request_generation
    }

    /// `ticket` 이 아직 마지막 연결·해제 요청인가.
    pub fn is_current_connect_request(&self, ticket: u64) -> bool {
        self.lock().connection_request_generation == ticket
    }

    /// [`Self::begin_connect_request`] 로 잡은 순번이 아직 마지막일 때만 연결한다(확인과 시작이 한 잠금 안).
    pub async fn connect_if_current(&self, ticket: u64, req: ConnectRequest) {
        self.connect_inner(req, Some(ticket)).await;
    }

    pub async fn connect(&self, req: ConnectRequest) {
        self.connect_inner(req, None).await;
    }

    async fn connect_inner(&self, req: ConnectRequest, ticket: Option<u64>) {
        let (request_generation, session_generation, old_engine) = {
            let mut s = self.lock();
            if ticket.is_some_and(|t| s.connection_request_generation != t) {
                return;
            }
            s.connection_request_generation = s.connection_request_generation.wrapping_add(1);
            s.chat_session_generation = s.chat_session_generation.wrapping_add(1);
            s.message_filter_generation = s.message_filter_generation.wrapping_add(1);
            s.moderation_state_generation = s.moderation_state_generation.wrapping_add(1);
            let old = s.disconnect_for_connection_change();
            (
                s.connection_request_generation,
                s.chat_session_generation,
                old,
            )
        };
        if let Some(e) = old_engine {
            e.disconnect().await;
        }
        let engine = {
            let mut s = self.lock();
            if s.connection_request_generation != request_generation
                || s.chat_session_generation != session_generation
            {
                return;
            }
            s.prepare_connection_state(&req);
            let config =
                ChatEngineConfiguration::new(req.chat_channel_id.clone(), req.access_token.clone())
                    .extra_token(req.extra_token.clone())
                    .uid(req.uid.clone())
                    .channel_id(req.channel_id.clone())
                    .token_provider(req.token_provider.clone());
            let engine = (s.deps.engine_factory)(config);
            s.chat_engine = Some(engine.clone());
            engine.set_background_mode(s.is_background_mode());

            let service = Arc::new(ChatModerationService::with_blocked_users(
                s.initial_blocked_users.clone(),
            ));
            s.moderation_service = Some(service.clone());
            service.set_keyword_filter(s.blocked_words.clone(), s.is_filter_enabled);
            s.moderation_state_generation = s.moderation_state_generation.wrapping_add(1);
            s.has_active_moderation = service.has_active_rules();
            s.start_event_listening(engine.clone());
            s.start_stats_tracking();
            engine
        };
        engine.connect().await;
        let stale = {
            let s = self.lock();
            s.connection_request_generation != request_generation
                || s.chat_session_generation != session_generation
                || !same_engine(&s.chat_engine, &engine)
        };
        if stale {
            self.abandon_stale_engine(engine).await;
        }
    }

    pub async fn disconnect(&self) {
        self.detach().await;
    }

    /// 해제의 동기 부분 — 세대를 올리고 엔진·작업을 지금 떼어 낸다. 돌려준 future 는 떼어 낸 엔진의 소켓만 닫는다.
    /// 셸이 여러 요청의 순서를 한 잠금 안에서 정할 때 쓴다(잠금을 쥔 채 `await` 하지 않게). 진행 중인 `connect` 는
    /// 세대가 바뀌어 다음 확인 지점에서 물러난다.
    pub fn detach(&self) -> impl std::future::Future<Output = ()> + Send + 'static {
        let old = {
            let mut s = self.lock();
            s.connection_request_generation = s.connection_request_generation.wrapping_add(1);
            s.chat_session_generation = s.chat_session_generation.wrapping_add(1);
            s.message_filter_generation = s.message_filter_generation.wrapping_add(1);
            s.moderation_state_generation = s.moderation_state_generation.wrapping_add(1);
            s.disconnect_for_connection_change()
        };
        async move {
            if let Some(e) = old {
                e.disconnect().await;
            }
        }
    }

    async fn abandon_stale_engine(&self, engine: Arc<dyn ChatEngineHandle>) {
        {
            let mut s = self.lock();
            if same_engine(&s.chat_engine, &engine) {
                s.chat_engine = None;
                s.moderation_service = None;
                s.connection_state = ChatConnectionState::Disconnected;
            }
        }
        engine.disconnect().await;
    }

    /// 마지막 설정으로 다시 연결하고, 같은 채널이면 헤더 폴링을 복구한다.
    pub async fn reconnect(&self) {
        let (config, provider, ranking, live) = {
            let s = self.lock();
            let Some(config) = s.last_connection_config.clone() else {
                return;
            };
            (
                config,
                s.last_token_provider.clone(),
                s.donation_ranking_poll_args.clone(),
                s.live_info_poll_args.clone(),
            )
        };
        self.connect(ConnectRequest {
            chat_channel_id: config.chat_channel_id.clone(),
            access_token: config.access_token.clone(),
            extra_token: config.extra_token.clone(),
            uid: config.uid.clone(),
            channel_id: config.channel_id.clone(),
            channel_name: config.channel_name.clone(),
            token_provider: provider,
        })
        .await;
        let mut s = self.lock();
        let same = s.last_connection_config.as_ref().is_some_and(|c| {
            c.chat_channel_id == config.chat_channel_id && c.channel_id == config.channel_id
        });
        if !same {
            return;
        }
        if let Some(args) = ranking {
            if Some(&args.channel_id) == config.channel_id.as_ref()
                && s.donation_ranking_task.is_none()
            {
                s.start_donation_ranking_polling(
                    args.channel_id,
                    args.source,
                    args.rank_count,
                    args.interval,
                );
            }
        }
        if let Some(args) = live {
            if Some(&args.channel_id) == config.channel_id.as_ref() && s.live_info_task.is_none() {
                s.start_live_info_polling(args.channel_id, args.source, args.interval);
            }
        }
    }
}

impl ChatState {
    pub fn schedule_background_mode_propagation(&mut self, enabled: bool) {
        self.background_mode_generation = self.background_mode_generation.wrapping_add(1);
        if let Some(engine) = &self.chat_engine {
            engine.set_background_mode(enabled);
        }
    }

    /// 연결 직전 상태 정리 (Swift `connect` 의 첫 가드 이후 구간).
    fn prepare_connection_state(&mut self, req: &ConnectRequest) {
        let same_channel = self
            .last_connection_config
            .as_ref()
            .is_some_and(|c| c.chat_channel_id == req.chat_channel_id);
        self.last_connection_config = Some(ConnectionConfig {
            chat_channel_id: req.chat_channel_id.clone(),
            access_token: req.access_token.clone(),
            extra_token: req.extra_token.clone(),
            uid: req.uid.clone(),
            channel_id: req.channel_id.clone(),
            channel_name: req.channel_name.clone(),
        });
        self.last_token_provider = req.token_provider.clone();
        self.set_chat_restriction(None, None);
        if !same_channel {
            self.has_shown_welcome = false;
            self.messages.clear();
            self.last_read_evicted = false;
            self.replay_hold_queue.clear();
            self.cached_last_read_state = None;
        }
        self.messages_revision = self.messages_revision.wrapping_add(1);
        self.reset_pending_queue();
        if !same_channel {
            self.chat_history.clear();
            self.has_chat_history = false;
        }
        self.exit_replay_mode();
        self.streamer_notice = None;
        self.pinned_message = None;
        if !same_channel {
            self.mention_matches.clear();
            self.mention_matches_queue.clear();
            self.recent_chatters.clear();
            self.seen_message_ids.clear();
            self.seen_message_id_queue.clear();
        }
        self.recent_sent_echo_keys.clear();
        self.pending_sent_echoes.clear();
        self.early_rejected_sends.clear();
        if let Some(cached) = self.deps.pinned_notices.load(&req.chat_channel_id) {
            if !self.deps.pinned_notices.is_dismissed(&cached.id) {
                self.streamer_notice = Some(cached);
            }
        }
        self.messages_per_second = 0.0;
        self.recent_message_timestamps.clear();
        if !same_channel {
            self.message_count = 0;
            self.reset_incremental_stats();
        }
    }

    /// 엔진·작업·임시 상태를 모두 내리고 끊을 엔진을 돌려준다.
    pub(crate) fn disconnect_for_connection_change(&mut self) -> Option<Arc<dyn ChatEngineHandle>> {
        let engine = self.chat_engine.take();
        self.moderation_service = None;
        self.connection_state = ChatConnectionState::Disconnected;
        self.background_mode_generation = self.background_mode_generation.wrapping_add(1);
        cancel_slot(&mut self.event_listen_task);
        cancel_slot(&mut self.stats_task);
        cancel_slot(&mut self.batch_flush_task);
        cancel_slot(&mut self.backlog_hold_task);
        self.is_holding_for_backlog = false;
        self.display_policy_task_generation = self.display_policy_task_generation.wrapping_add(1);
        cancel_slot(&mut self.display_policy_refilter_task);
        self.settings_sync_task_generation = self.settings_sync_task_generation.wrapping_add(1);
        cancel_slot(&mut self.settings_moderation_sync_task);
        cancel_slot(&mut self.chat_restriction_task);
        self.cancel_replay_debounce();
        cancel_slot(&mut self.donation_ranking_task);
        self.weekly_donation_ranking.clear();
        self.my_weekly_donation_rank = None;
        cancel_slot(&mut self.live_info_task);
        self.cancel_all_stream_alert_dismiss_tasks();
        self.stream_alerts.clear();
        if !self.collected_emoticons.is_empty() {
            self.collected_emoticons.clear();
            self.collected_emoticon_index.clear();
            self.invalidate_emoticon_autocomplete_search_index();
        }
        self.forget_uncommitted_pending_ids();
        self.reset_pending_queue();
        self.pending_sent_echoes.clear();
        self.early_rejected_sends.clear();
        self.deps.tts.stop();
        engine
    }
}
