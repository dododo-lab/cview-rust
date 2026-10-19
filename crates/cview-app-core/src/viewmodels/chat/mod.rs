//! 채팅 뷰모델.
//!
//! 원본: `Sources/CViewApp/ViewModels/ChatViewModel.swift` 와 확장 전부.
//!
//! | Swift | Rust |
//! |---|---|
//! | `ChatViewModel.swift` (상태·백그라운드 모드·채팅 제한) | 이 파일 |
//! | `+Connection` | [`connection`] |
//! | `+EventListen` | [`event_listen`] |
//! | `+Processing` | [`processing`] |
//! | `+Mutation` | [`mutation`] |
//! | `+Batch` | [`batch`] |
//! | `+Interaction` | [`interaction`] |
//! | `+ReadDivider` | [`read_divider`] |
//! | `+Stats` | [`stats`] |
//! | `+Autocomplete` (+ `CViewCore/Models/AutocompleteModels.swift`) | [`autocomplete`] |
//! | `+Mention` | [`mention`] |
//! | `+Moderation` | [`moderation`] |
//! | `+DonationRanking`, `+LiveInfo` | [`header`] |
//! | `+StreamAlert` | [`stream_alert`] |
//! | `+SettingsSync` | [`settings_sync`] |
//! | `ChatTTSService.swift` | [`tts`] (trait 만) |
//! | `Services/MergedChatBudgetCoordinator.swift` | [`merged_budget`] |
//! | `CViewCore/Utilities/ChatFlushPacing.swift`, `ChatDripPacing.swift` | [`pacing`] |
//! | `CViewCore/Models/SettingsModels+Chat.swift` (`ChatSettings`) | [`settings`] |
//!
//! 구조: 상태는 [`ChatState`] 한 덩어리로 [`Observed`] 잠금 안에 있고, [`ChatViewModel`] 은
//! 그 공유 핸들이다. 동기 정책(커밋·드립·모더레이션 반영)은 `ChatState` 메서드, 엔진과 주고받는
//! 비동기 흐름(연결·전송)은 `ChatViewModel` 메서드다. 버려진 것: SwiftUI 트랜잭션(스프링 억제),
//! 로깅 계측의 단계별 시간 측정 문구, `@ObservationIgnored` 구분.
//!
//! Swift 에서는 `ChatModerationService` 가 actor 라 필터링 중에 다른 작업이 끼어들 수 있었고,
//! 그 틈을 세대 번호와 테스트 barrier 로 막았다. Rust 의 모더레이션 서비스는 동기 API 라 필터·재필터가
//! 잠금 한 번 안에서 끝난다 — 세대 번호는 실제 `await`(엔진 연결·전송) 경계에서만 의미를 가진다.

pub mod autocomplete;
pub mod batch;
pub mod connection;
pub mod deps;
pub mod event_listen;
pub mod header;
pub mod interaction;
pub mod mention;
pub mod merged_budget;
pub mod moderation;
pub mod mutation;
pub mod pacing;
pub mod processing;
pub mod read_divider;
pub mod settings;
pub mod settings_sync;
pub mod snapshot;
pub mod stats;
pub mod stream_alert;
pub mod tts;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};

use cview_chat::engine::TokenProvider;
use cview_chat::utils::{ChatCommitBackpressure, ChatMessageBuffer};
use cview_chat::ChatModerationService;
use cview_core::logic::mention_detector::MentionMatch;
use cview_core::models::chat::{
    ChatConnectionState, ChatMessageItem, ChatSystemNoticeKind, DonationRankItem, EmoticonPack,
};
use cview_core::models::settings::{ChatDisplayMode, ChatMessageAlignment};
use cview_core::models::stream_alert::StreamAlertItem;

use super::support::{cancel_slot, Observed, ObservedGuard, TaskSlot};
pub use autocomplete::{AutocompleteTrigger, EmoticonSuggestion, MentionSuggestion};
pub use deps::*;
pub use header::HeaderPollArgs;
pub use merged_budget::MergedChatBudgetCoordinator;
pub use mutation::ChatMessageMutation;
pub use settings::ChatSettings;
pub use snapshot::ChatSnapshot;

/// 프로세스 기준 단조 시계(초) — Swift `ProcessInfo.processInfo.systemUptime`.
pub fn uptime() -> f64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64() + 1.0
}

/// 채팅 참여 제한 사유.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatRestrictionReason {
    Kicked,
    Penalized,
}

/// 마지막 연결 인자 (Swift `lastConnectionConfig` 튜플).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConnectionConfig {
    pub chat_channel_id: String,
    pub access_token: String,
    pub extra_token: Option<String>,
    pub uid: Option<String>,
    pub channel_id: Option<String>,
    pub channel_name: Option<String>,
}

/// 보낸 메시지와 13101 응답(tid) 의 상관 항목.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingSentEcho {
    pub message_id: String,
    pub echo_key: String,
    pub expiry: DateTime<Utc>,
    pub session_generation: u64,
}

/// tid 가 등록되기 전에 먼저 도착한 거절.
#[derive(Debug, Clone, PartialEq)]
pub struct EarlySendRejection {
    pub expiry: DateTime<Utc>,
    pub session_generation: u64,
}

/// 이벤트 출처 세션 정체성.
#[derive(Clone)]
pub struct ChatEventSessionIdentity {
    pub session_generation: u64,
    pub engine: Arc<dyn ChatEngineHandle>,
    pub channel_identity: Option<String>,
}

/// 차단 목록 변경 콜백.
pub type BlockedUsersCallback = Arc<dyn Fn(Vec<String>) + Send + Sync>;
/// 단일 차단/해제 콜백 `(userId, isBlocked)`.
pub type BlockedUserMutationCallback = Arc<dyn Fn(&str, bool) + Send + Sync>;

/// 공유 내부.
pub struct ChatShared {
    pub(crate) obs: Observed<ChatState>,
}

/// ChatViewModel 의 전체 상태.
pub struct ChatState {
    pub(crate) this: Weak<ChatShared>,
    pub deps: ChatDeps,

    // MARK: 메시지
    pub messages: ChatMessageBuffer,
    pub commit_backpressure: ChatCommitBackpressure,
    pub last_live_flush_uptime: Option<f64>,
    pub messages_revision: u64,
    pub moderation_revision: u64,
    pub connection_state: ChatConnectionState,
    pub multi_chat_scope: MultiChatScope,
    pub input_text: String,
    pub is_auto_scroll_enabled: bool,
    pub(crate) show_donations_only: bool,
    pub font_size: f64,
    pub opacity: f64,
    pub show_timestamp: bool,
    pub show_badge: bool,
    pub line_spacing: f64,
    pub highlight_mentions: bool,
    pub emoticon_enabled: bool,
    pub(crate) show_donation: bool,
    pub display_mode: ChatDisplayMode,
    pub message_alignment: ChatMessageAlignment,
    pub overlay_width: f64,
    pub overlay_height: f64,
    pub overlay_background_opacity: f64,
    pub overlay_show_input: bool,
    pub(crate) max_visible_messages: usize,
    pub blocked_words: Vec<String>,
    pub message_count: i64,
    pub messages_per_second: f64,
    pub unread_while_background: i64,
    pub chat_history: Vec<ChatMessageItem>,
    pub has_chat_history: bool,
    pub is_replay_mode: bool,
    pub unread_count: i64,
    pub replay_hold_queue: Vec<ChatMessageItem>,
    pub cached_last_read_state: Option<(String, Option<String>)>,
    pub last_read_evicted: bool,
    pub pending_scroll_message_id: Option<String>,
    pub highlighted_users: HashSet<String>,
    pub pinned_message: Option<ChatMessageItem>,
    pub streamer_notice: Option<ChatMessageItem>,
    pub is_filter_enabled: bool,
    pub mention_matches: HashMap<String, HashSet<MentionMatch>>,
    pub mention_matches_queue: Vec<String>,
    pub local_user_nickname: Option<String>,

    // MARK: 헤더
    pub weekly_donation_ranking: Vec<DonationRankItem>,
    pub my_weekly_donation_rank: Option<DonationRankItem>,
    pub donation_ranking_task: Option<TaskSlot>,
    pub donation_ranking_poll_args: Option<HeaderPollArgs>,
    pub live_info_poll_args: Option<HeaderPollArgs>,
    pub live_info_task: Option<TaskSlot>,
    pub live_viewer_count: i64,
    pub live_accumulate_count: i64,
    pub live_open_date: Option<DateTime<Utc>>,
    pub stream_alert_dismiss_tasks: HashMap<String, TaskSlot>,

    // MARK: 자동완성
    pub emoticon_suggestions: Vec<EmoticonSuggestion>,
    pub mention_suggestions: Vec<MentionSuggestion>,
    pub autocomplete_selected_index: usize,
    pub autocomplete_trigger: AutocompleteTrigger,
    pub emoticon_autocomplete_search_index: Vec<autocomplete::EmoticonAutocompleteSearchEntry>,
    pub is_emoticon_autocomplete_search_index_dirty: bool,
    pub emoticon_autocomplete_search_index_rebuild_count: u64,
    pub recent_chatters: Vec<MentionSuggestion>,
    pub(crate) channel_emoticons: HashMap<String, String>,
    pub(crate) emoticon_packs: Vec<EmoticonPack>,
    /// 받은 메시지에서 모은 이모티콘 (삽입 순서 유지 — Swift Dictionary.suffix 근사).
    pub collected_emoticons: Vec<(String, String)>,
    pub collected_emoticon_index: HashMap<String, usize>,

    // MARK: 알림·오류·제한
    pub stream_alerts: Vec<StreamAlertItem>,
    pub is_stream_alert_enabled: bool,
    pub error_message: Option<String>,
    pub chat_restriction: Option<ChatRestrictionReason>,
    pub chat_restriction_task: Option<TaskSlot>,

    // MARK: 엔진·세대
    pub chat_engine: Option<Arc<dyn ChatEngineHandle>>,
    pub moderation_service: Option<Arc<ChatModerationService>>,
    pub chat_session_generation: u64,
    pub connection_request_generation: u64,
    pub refilter_generation: u64,
    pub message_filter_generation: u64,
    pub keyword_policy_generation: u64,
    pub moderation_state_generation: u64,
    pub last_connection_config: Option<ConnectionConfig>,
    pub last_token_provider: Option<TokenProvider>,
    pub has_shown_welcome: bool,
    pub on_blocked_users_changed: Option<BlockedUsersCallback>,
    pub on_blocked_user_mutation: Option<BlockedUserMutationCallback>,
    pub initial_blocked_users: Vec<String>,
    pub current_user_uid: Option<String>,
    pub current_user_nickname: Option<String>,
    pub event_listen_task: Option<TaskSlot>,
    pub stats_task: Option<TaskSlot>,
    pub display_policy_refilter_task: Option<TaskSlot>,
    pub display_policy_task_generation: u64,
    pub settings_moderation_sync_task: Option<TaskSlot>,
    pub settings_sync_task_generation: u64,
    pub is_applying_chat_settings: bool,
    pub recent_message_timestamps: Vec<DateTime<Utc>>,
    pub background_mode_generation: u64,
    pub last_diagnostic_log_at: Option<DateTime<Utc>>,
    pub last_diagnostic_dropped_count: i64,
    pub last_slow_commit_diagnostic_at: Option<DateTime<Utc>>,
    pub suppressed_slow_commit_diagnostics: i64,
    pub has_active_moderation: bool,
    requested_background_mode: bool,
    foreground_presentation_lease_count: usize,

    // MARK: 페이싱
    pub is_owner_presentation_suspended: bool,
    pub co_active_foreground_sessions: i64,
    pub merged_budget_coordinator: Option<Weak<MergedChatBudgetCoordinator>>,
    pub merged_row_budget: i64,
    pub pending_messages: Vec<ChatMessageItem>,
    pub pending_head: usize,
    pub pending_first_enqueued_at: Option<DateTime<Utc>>,
    pub pending_hard_cap: i64,
    pub last_drip_interval_ns: u64,
    pub is_drip_catching_up: bool,
    pub drip_projected_lag_ms: f64,
    pub dropped_message_count: i64,
    pub pending_drop_notice_backlog: i64,
    pub last_drop_notice_at: Option<DateTime<Utc>>,
    pub batch_flush_task: Option<TaskSlot>,
    pub is_holding_for_backlog: bool,
    pub backlog_hold_task: Option<TaskSlot>,
    pub max_chat_update_rate: i64,

    // MARK: 통계
    pub unique_user_count: i64,
    pub unique_users: HashSet<String>,
    pub donation_count: i64,
    pub total_donation_amount: i64,
    pub subscription_count: i64,
    pub seen_message_ids: HashSet<String>,
    pub seen_message_id_queue: Vec<String>,
    pub recent_sent_echo_keys: HashMap<String, DateTime<Utc>>,
    pub pending_sent_echoes: HashMap<i64, PendingSentEcho>,
    pub early_rejected_sends: HashMap<i64, EarlySendRejection>,
    pub show_export_sheet: bool,
    pub replay_debounce_task: Option<TaskSlot>,
}

impl ChatState {
    pub const MAX_HISTORY_SIZE: usize = 2500;
    pub const BACKGROUND_HISTORY_SIZE: usize = 700;
    pub const HISTORY_TRIM_SLACK: usize = 256;
    pub const MAX_RECENT_CHATTERS_COUNT: usize = 100;
    pub const MAX_CHAT_RESTRICTION_SECONDS: i64 = 3600;
    pub const BACKGROUND_FLUSH_INTERVAL_NS: u64 = 3_000_000_000;
    pub const SUSPENDED_FLUSH_INTERVAL_NS: u64 = 1_000_000_000;
    pub const ECONOMY_DRIP_FLOOR_NS: u64 = 100_000_000;
    pub const DROP_NOTICE_MIN_INTERVAL_SEC: f64 = 5.0;
    pub const BACKLOG_HOLD_TIMEOUT: Duration = Duration::from_millis(800);
    pub const BATCH_FLUSH_INTERVAL_NS: u64 = 33_000_000;
    pub const PENDING_SENT_ECHO_TTL_SECS: i64 = 30;
    pub const PENDING_SENT_ECHO_CAP: usize = 64;
    pub const EARLY_SEND_REJECTION_TTL_SECS: i64 = 30;
    pub const EARLY_SEND_REJECTION_CAP: usize = 64;
    pub const WELCOME_TEXT: &'static str = "채팅방에 오신 것을 환영합니다!\n쾌적한 시청 환경을 위해 불필요한 메시지는 필터링 됩니다. 올바른 라이브 채팅 문화 만들기에 동참해 주세요.";

    fn new(this: Weak<ChatShared>, deps: ChatDeps) -> Self {
        Self {
            this,
            deps,
            messages: ChatMessageBuffer::new(200),
            commit_backpressure: ChatCommitBackpressure::new(),
            last_live_flush_uptime: None,
            messages_revision: 0,
            moderation_revision: 0,
            connection_state: ChatConnectionState::Disconnected,
            multi_chat_scope: MultiChatScope::Live,
            input_text: String::new(),
            is_auto_scroll_enabled: true,
            show_donations_only: false,
            font_size: 13.0,
            opacity: 1.0,
            show_timestamp: true,
            show_badge: true,
            line_spacing: 2.0,
            highlight_mentions: true,
            emoticon_enabled: true,
            show_donation: true,
            display_mode: ChatDisplayMode::Side,
            message_alignment: ChatMessageAlignment::Auto,
            overlay_width: 340.0,
            overlay_height: 400.0,
            overlay_background_opacity: 0.5,
            overlay_show_input: false,
            max_visible_messages: 200,
            blocked_words: vec![],
            message_count: 0,
            messages_per_second: 0.0,
            unread_while_background: 0,
            chat_history: vec![],
            has_chat_history: false,
            is_replay_mode: false,
            unread_count: 0,
            replay_hold_queue: vec![],
            cached_last_read_state: None,
            last_read_evicted: false,
            pending_scroll_message_id: None,
            highlighted_users: HashSet::new(),
            pinned_message: None,
            streamer_notice: None,
            is_filter_enabled: true,
            mention_matches: HashMap::new(),
            mention_matches_queue: vec![],
            local_user_nickname: None,
            weekly_donation_ranking: vec![],
            my_weekly_donation_rank: None,
            donation_ranking_task: None,
            donation_ranking_poll_args: None,
            live_info_poll_args: None,
            live_info_task: None,
            live_viewer_count: 0,
            live_accumulate_count: 0,
            live_open_date: None,
            stream_alert_dismiss_tasks: HashMap::new(),
            emoticon_suggestions: vec![],
            mention_suggestions: vec![],
            autocomplete_selected_index: 0,
            autocomplete_trigger: AutocompleteTrigger::None,
            emoticon_autocomplete_search_index: vec![],
            is_emoticon_autocomplete_search_index_dirty: true,
            emoticon_autocomplete_search_index_rebuild_count: 0,
            recent_chatters: vec![],
            channel_emoticons: HashMap::new(),
            emoticon_packs: vec![],
            collected_emoticons: vec![],
            collected_emoticon_index: HashMap::new(),
            stream_alerts: vec![],
            is_stream_alert_enabled: true,
            error_message: None,
            chat_restriction: None,
            chat_restriction_task: None,
            chat_engine: None,
            moderation_service: None,
            chat_session_generation: 0,
            connection_request_generation: 0,
            refilter_generation: 0,
            message_filter_generation: 0,
            keyword_policy_generation: 0,
            moderation_state_generation: 0,
            last_connection_config: None,
            last_token_provider: None,
            has_shown_welcome: false,
            on_blocked_users_changed: None,
            on_blocked_user_mutation: None,
            initial_blocked_users: vec![],
            current_user_uid: None,
            current_user_nickname: None,
            event_listen_task: None,
            stats_task: None,
            display_policy_refilter_task: None,
            display_policy_task_generation: 0,
            settings_moderation_sync_task: None,
            settings_sync_task_generation: 0,
            is_applying_chat_settings: false,
            recent_message_timestamps: vec![],
            background_mode_generation: 0,
            last_diagnostic_log_at: None,
            last_diagnostic_dropped_count: 0,
            last_slow_commit_diagnostic_at: None,
            suppressed_slow_commit_diagnostics: 0,
            has_active_moderation: false,
            requested_background_mode: false,
            foreground_presentation_lease_count: 0,
            is_owner_presentation_suspended: false,
            co_active_foreground_sessions: 1,
            merged_budget_coordinator: None,
            merged_row_budget: 0,
            pending_messages: Vec::with_capacity(64),
            pending_head: 0,
            pending_first_enqueued_at: None,
            pending_hard_cap: 1000,
            last_drip_interval_ns: pacing::drip::NOMINAL_DRIP_INTERVAL_NS,
            is_drip_catching_up: false,
            drip_projected_lag_ms: 0.0,
            dropped_message_count: 0,
            pending_drop_notice_backlog: 0,
            last_drop_notice_at: None,
            batch_flush_task: None,
            is_holding_for_backlog: false,
            backlog_hold_task: None,
            max_chat_update_rate: 30,
            unique_user_count: 0,
            unique_users: HashSet::new(),
            donation_count: 0,
            total_donation_amount: 0,
            subscription_count: 0,
            seen_message_ids: HashSet::new(),
            seen_message_id_queue: vec![],
            recent_sent_echo_keys: HashMap::new(),
            pending_sent_echoes: HashMap::new(),
            early_rejected_sends: HashMap::new(),
            show_export_sheet: false,
            replay_debounce_task: None,
        }
    }

    /// 지연 후 잠금을 다시 잡고 본문을 실행하는 타이머 작업. 본문은 `(state, task_id)` 를 받는다.
    pub(crate) fn spawn_timer<F>(&self, delay: Duration, body: F) -> TaskSlot
    where
        F: FnOnce(&mut ChatState, u64) + Send + 'static,
    {
        let weak = self.this.clone();
        TaskSlot::spawn(move |id| async move {
            tokio::time::sleep(delay).await;
            if let Some(shared) = weak.upgrade() {
                let mut g = shared.obs.lock();
                body(&mut g, id);
            }
        })
    }

    /// Swift `mutateMessagesWithoutSpring` — 행 리비전만 올린다(SwiftUI 트랜잭션은 버렸다).
    pub fn mutate_messages<R>(&mut self, body: impl FnOnce(&mut ChatMessageBuffer) -> R) -> R {
        let r = body(&mut self.messages);
        self.messages_revision = self.messages_revision.wrapping_add(1);
        r
    }

    pub fn append_message(&mut self, item: ChatMessageItem) {
        self.mutate_messages(|m| m.push(item));
    }

    pub fn append_system(&mut self, text: impl Into<String>, kind: ChatSystemNoticeKind) {
        self.append_message(ChatMessageItem::system(text, kind));
    }

    pub fn welcome_message() -> ChatMessageItem {
        ChatMessageItem::system(Self::WELCOME_TEXT, ChatSystemNoticeKind::Info)
    }

    // MARK: - 계산 프로퍼티

    pub fn active_chat_channel_id(&self) -> Option<String> {
        self.last_connection_config
            .as_ref()
            .map(|c| c.chat_channel_id.clone())
    }

    pub fn channel_preference_key(&self) -> Option<String> {
        let c = self.last_connection_config.as_ref()?;
        Some(
            c.channel_id
                .clone()
                .unwrap_or_else(|| c.chat_channel_id.clone()),
        )
    }

    pub fn mention_channel_display_name(&self) -> String {
        match self
            .last_connection_config
            .as_ref()
            .and_then(|c| c.channel_name.as_deref())
        {
            Some(raw) if !raw.trim().is_empty() => raw.trim().to_string(),
            _ => "채널".into(),
        }
    }

    pub fn is_autocomplete_active(&self) -> bool {
        !self.emoticon_suggestions.is_empty() || !self.mention_suggestions.is_empty()
    }

    pub fn is_chat_restricted(&self) -> bool {
        self.chat_restriction.is_some()
    }

    pub fn can_send_chat(&self) -> bool {
        self.connection_state.is_connected()
            && self.current_user_uid.is_some()
            && !self.is_chat_restricted()
    }

    pub fn unique_user_count_is_capped(&self) -> bool {
        self.unique_user_count >= Self::UNIQUE_USER_TRACKING_CAP as i64
    }

    pub fn unique_user_count_display(&self) -> String {
        let f = super::support::format_number(self.unique_user_count);
        if self.unique_user_count_is_capped() {
            format!("{f}+")
        } else {
            f
        }
    }

    pub fn show_donations_only(&self) -> bool {
        self.show_donations_only
    }

    /// `showDonationsOnly` didSet — 바뀌면 표시 정책 재필터 예약.
    pub fn set_show_donations_only(&mut self, value: bool) {
        let old = self.show_donations_only;
        self.show_donations_only = value;
        if old != value {
            self.schedule_display_policy_refilter();
        }
    }

    pub fn show_donation(&self) -> bool {
        self.show_donation
    }

    pub fn set_show_donation(&mut self, value: bool) {
        let old = self.show_donation;
        self.show_donation = value;
        if old != value {
            self.schedule_display_policy_refilter();
        }
    }

    pub fn max_visible_messages(&self) -> usize {
        self.max_visible_messages
    }

    pub fn set_max_visible_messages(&mut self, value: usize) {
        self.max_visible_messages = value;
        if value != self.messages.capacity() {
            self.messages.resize(value);
        }
    }

    pub fn channel_emoticons(&self) -> &HashMap<String, String> {
        &self.channel_emoticons
    }

    /// `channelEmoticons` didSet — 검색 색인 무효화 + 이미 보이는 행에 이모티콘 보강.
    pub fn set_channel_emoticons(&mut self, value: HashMap<String, String>) {
        let changed = self.channel_emoticons != value;
        self.channel_emoticons = value;
        if changed {
            self.invalidate_emoticon_autocomplete_search_index();
        }
        if self.channel_emoticons.is_empty() {
            return;
        }
        if !self.messages.is_empty() {
            let emotes = self.channel_emoticons.clone();
            self.messages
                .map_in_place(|i| Self::enrich_with_channel_emoticons_pure(i, &emotes));
        }
    }

    pub fn emoticon_packs(&self) -> &[EmoticonPack] {
        &self.emoticon_packs
    }

    pub fn set_emoticon_packs(&mut self, value: Vec<EmoticonPack>) {
        let changed = self.emoticon_packs != value;
        self.emoticon_packs = value;
        if changed {
            self.invalidate_emoticon_autocomplete_search_index();
        }
        if self.emoticon_packs.is_empty() {
            return;
        }
        self.prefetch_emoticon_images();
    }

    /// 피커용 팩. 팩이 없으면 스트림에서 모은 이모티콘으로 가상 팩 하나를 만든다.
    pub fn emoticon_picker_packs(&self) -> Vec<EmoticonPack> {
        if !self.emoticon_packs.is_empty() {
            return self.emoticon_packs.clone();
        }
        let mut merged: HashMap<String, String> =
            self.collected_emoticons.iter().cloned().collect();
        for (id, url) in &self.channel_emoticons {
            if !merged.contains_key(id) && url::Url::parse(url).is_ok() {
                merged.insert(id.clone(), url.clone());
            }
        }
        if merged.is_empty() {
            return vec![];
        }
        let mut items: Vec<cview_core::models::chat::EmoticonItem> = merged
            .into_iter()
            .map(|(id, url)| {
                let mut item = cview_core::models::chat::EmoticonItem::new(id);
                item.image_url = Some(url);
                item
            })
            .collect();
        items.sort_by(|a, b| a.emoticon_id.cmp(&b.emoticon_id));
        let mut pack = EmoticonPack::new("_stream", "스트림 이모티콘");
        pack.emoticons = Some(items);
        vec![pack]
    }

    // MARK: - 채팅 제한

    pub fn set_chat_restriction(
        &mut self,
        reason: Option<ChatRestrictionReason>,
        auto_release_after: Option<i64>,
    ) {
        cancel_slot(&mut self.chat_restriction_task);
        self.chat_restriction = reason;
        let (Some(reason), Some(seconds)) = (reason, auto_release_after) else {
            return;
        };
        if seconds <= 0 {
            return;
        }
        let capped = seconds.min(Self::MAX_CHAT_RESTRICTION_SECONDS) as u64;
        self.chat_restriction_task =
            Some(self.spawn_timer(Duration::from_secs(capped), move |s, id| {
                if !super::support::slot_is(&s.chat_restriction_task, id) {
                    return;
                }
                if s.chat_restriction == Some(reason) {
                    s.chat_restriction = None;
                }
            }));
    }

    // MARK: - 백그라운드 모드

    pub fn is_background_mode(&self) -> bool {
        self.requested_background_mode && self.foreground_presentation_lease_count == 0
    }

    pub fn set_background_mode(&mut self, value: bool) {
        let previous = self.is_background_mode();
        self.requested_background_mode = value;
        self.apply_effective_background_mode_transition(previous);
    }

    pub fn has_foreground_presentation_lease(&self) -> bool {
        self.foreground_presentation_lease_count > 0
    }

    pub fn acquire_foreground_presentation_lease(&mut self) {
        let previous = self.is_background_mode();
        self.foreground_presentation_lease_count += 1;
        self.apply_effective_background_mode_transition(previous);
    }

    pub fn release_foreground_presentation_lease(&mut self) {
        if self.foreground_presentation_lease_count == 0 {
            return;
        }
        let previous = self.is_background_mode();
        self.foreground_presentation_lease_count -= 1;
        self.apply_effective_background_mode_transition(previous);
    }

    fn apply_effective_background_mode_transition(&mut self, previous: bool) {
        let enabled = self.is_background_mode();
        if previous == enabled {
            return;
        }
        if enabled && self.chat_history.len() > Self::BACKGROUND_HISTORY_SIZE {
            let excess = self.chat_history.len() - Self::BACKGROUND_HISTORY_SIZE;
            self.chat_history.drain(..excess);
        }
        if enabled && self.is_replay_mode {
            self.exit_replay_mode();
        }
        self.schedule_background_mode_propagation(enabled);
        if !enabled {
            self.unread_while_background = 0;
            cancel_slot(&mut self.batch_flush_task);
            if self.pending_count() > 0 {
                self.flush_pending_messages();
            }
        }
    }

    pub fn uses_batch_flush_mode(&self) -> bool {
        self.is_background_mode()
    }

    pub fn is_presentation_suspended(&self) -> bool {
        (self.is_owner_presentation_suspended && self.foreground_presentation_lease_count == 0)
            || self.deps.pacing.is_render_suspended()
    }

    pub fn uses_coalesced_commit_pacing(&self) -> bool {
        self.uses_batch_flush_mode() || self.is_presentation_suspended()
    }

    pub fn merged_budget_load(&self) -> i64 {
        self.pending_count() as i64
    }

    pub fn prefers_smooth_bottom_drip(&self) -> bool {
        !self.uses_coalesced_commit_pacing() && self.is_auto_scroll_enabled && !self.is_replay_mode
    }
}

/// 채팅 뷰모델 핸들. 복제본은 같은 상태를 가리킨다.
#[derive(Clone)]
pub struct ChatViewModel {
    shared: Arc<ChatShared>,
}

impl Default for ChatViewModel {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ChatViewModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatViewModel").finish_non_exhaustive()
    }
}

impl ChatViewModel {
    pub fn new() -> Self {
        Self::with_deps(ChatDeps::default())
    }

    pub fn with_deps(deps: ChatDeps) -> Self {
        let shared = Arc::new_cyclic(|weak: &Weak<ChatShared>| ChatShared {
            obs: Observed::new(ChatState::new(weak.clone(), deps)),
        });
        Self { shared }
    }

    /// 상태를 잠근다. 가드를 놓으면 변경 알림이 나간다. `await` 를 건너 쥐지 말 것.
    pub fn lock(&self) -> ObservedGuard<'_, ChatState> {
        self.shared.obs.lock()
    }

    /// 변경 알림 구독.
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.shared.obs.subscribe()
    }

    /// 직렬화 가능한 상태 스냅샷.
    pub fn snapshot(&self) -> ChatSnapshot {
        ChatSnapshot::from_state(&self.shared.obs.read())
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }

    pub(crate) fn downgrade(&self) -> Weak<ChatShared> {
        Arc::downgrade(&self.shared)
    }

    pub(crate) fn from_shared(shared: Arc<ChatShared>) -> Self {
        Self { shared }
    }
}
