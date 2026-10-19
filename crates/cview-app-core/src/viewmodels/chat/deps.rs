//! ChatViewModel 이 기대는 외부 협력자 — 서비스 계층(다른 포팅 작업) 대신 작은 trait 으로 둔다.
//!
//! | Swift 협력자 | Rust 경계 |
//! |---|---|
//! | `ChatEngine` (actor) | [`ChatEngineHandle`] + [`ChatEngineFactory`] (기본: `cview_chat::ChatEngine`) |
//! | `MultiChatPreferences.shared` (lastRead·멘션 규칙·미읽음) | [`ChatPreferences`] |
//! | `MentionInbox` · `MentionNotificationService` | [`MentionSink`] |
//! | `PinnedNoticeCache` (UserDefaults) | [`PinnedNoticeStore`] + [`MemoryPinnedNoticeStore`] |
//! | `ChannelHeaderHub.shared` (랭킹·라이브 상태) | [`ChannelHeaderSource`] (기본: `ChzzkApiClient`) |
//! | `ImageCacheService.shared.prefetch` | [`ImagePrefetcher`] |
//! | `RenderBudgetController` · `WindowGeometryTransitionSignal` · `SystemLoadMonitor` | [`PacingEnvironment`] |
//! | `ToastCenter.shared` | [`ToastPresenter`] |
//! | `ChatTTSService` (AVSpeechSynthesizer) | [`super::tts::ChatTts`] |

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};

use cview_chat::{ChatEngine, ChatEngineConfiguration, ChatEngineEvent, ChatEventStream};
use cview_core::logic::mention_detector::{MentionDetectionRule, MentionMatch};
use cview_core::models::chat::{
    ChatMessage, ChatMessageItem, DonationRankResponse, EmojiMap, MessageType,
};
use cview_core::models::LiveInfo;
use cview_core::utils::SystemLoadMode;
use cview_networking::ChzzkApiClient;

use super::tts::{ChatTts, NoopTts};

/// 채팅 엔진 핸들. Swift 는 `chatEngine === engine` 으로 정체성을 비교했다 —
/// Rust 에서는 `Arc::ptr_eq` 로 비교한다.
#[async_trait]
pub trait ChatEngineHandle: Send + Sync {
    async fn connect(&self);
    async fn disconnect(&self);
    /// 전송 tid 반환. 오류는 사용자에게 보일 설명 문자열.
    async fn send_message(&self, text: &str, emojis: &EmojiMap) -> Result<i64, String>;
    fn set_background_mode(&self, enabled: bool);
    fn clear_messages(&self);
    /// 엔진이 보관한 서버 메시지(재필터 원본).
    fn messages(&self) -> Vec<ChatMessage>;
    /// 다음 이벤트. 스트림이 끝나면 `None`.
    async fn next_event(&self) -> Option<ChatEngineEvent>;
}

/// `ChatEngine(configuration:)` 생성자 주입점.
pub type ChatEngineFactory =
    Arc<dyn Fn(ChatEngineConfiguration) -> Arc<dyn ChatEngineHandle> + Send + Sync>;

/// 실제 `cview_chat::ChatEngine` 어댑터.
pub struct LiveChatEngine {
    engine: ChatEngine,
    events: ChatEventStream,
}

impl LiveChatEngine {
    pub fn new(config: ChatEngineConfiguration) -> Self {
        let engine = ChatEngine::new(config);
        let events = engine.events();
        Self { engine, events }
    }

    pub fn engine(&self) -> &ChatEngine {
        &self.engine
    }

    pub fn factory() -> ChatEngineFactory {
        Arc::new(|config| Arc::new(LiveChatEngine::new(config)) as Arc<dyn ChatEngineHandle>)
    }
}

#[async_trait]
impl ChatEngineHandle for LiveChatEngine {
    async fn connect(&self) {
        self.engine.connect().await;
    }
    async fn disconnect(&self) {
        self.engine.disconnect().await;
    }
    async fn send_message(&self, text: &str, emojis: &EmojiMap) -> Result<i64, String> {
        self.engine
            .send_message(text, Some(emojis))
            .await
            .map_err(|e| e.to_string())
    }
    fn set_background_mode(&self, enabled: bool) {
        self.engine.set_background_mode(enabled);
    }
    fn clear_messages(&self) {
        self.engine.clear_messages();
    }
    fn messages(&self) -> Vec<ChatMessage> {
        self.engine.messages()
    }
    async fn next_event(&self) -> Option<ChatEngineEvent> {
        self.events.recv().await
    }
}

/// `MultiChatScope` — 시청 화면(live) 과 독립 채팅 창(standalone) 의 선호 저장 분리.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MultiChatScope {
    #[default]
    Live,
    Standalone,
}

/// `MultiChatPreferences` 중 ChatViewModel 이 쓰는 부분.
pub trait ChatPreferences: Send + Sync {
    fn last_read_message_id(&self, chat_channel_id: &str, scope: MultiChatScope) -> Option<String>;
    fn set_last_read_message_id(
        &self,
        chat_channel_id: &str,
        message_id: &str,
        scope: MultiChatScope,
    );
    fn current_mention_rule(&self, nickname: Option<&str>) -> MentionDetectionRule;
    fn increment_mention_unread(&self, chat_channel_id: &str, scope: MultiChatScope);
}

/// 메모리 구현 (테스트·기본값).
#[derive(Debug, Default)]
pub struct MemoryChatPreferences {
    inner: Mutex<MemoryPrefsInner>,
}

#[derive(Debug, Default)]
struct MemoryPrefsInner {
    last_read: HashMap<(String, MultiChatScope), String>,
    keywords: Vec<String>,
    muted_channels: HashSet<String>,
    unread: HashMap<(String, MultiChatScope), i64>,
}

impl MemoryChatPreferences {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn set_keywords(&self, keywords: Vec<String>) {
        self.inner.lock().unwrap().keywords = keywords;
    }
    pub fn set_muted_channels(&self, muted: HashSet<String>) {
        self.inner.lock().unwrap().muted_channels = muted;
    }
    pub fn mention_unread(&self, chat_channel_id: &str, scope: MultiChatScope) -> i64 {
        let g = self.inner.lock().unwrap();
        g.unread
            .get(&(chat_channel_id.to_string(), scope))
            .copied()
            .unwrap_or(0)
    }
}

impl ChatPreferences for MemoryChatPreferences {
    fn last_read_message_id(&self, chat_channel_id: &str, scope: MultiChatScope) -> Option<String> {
        self.inner
            .lock()
            .unwrap()
            .last_read
            .get(&(chat_channel_id.to_string(), scope))
            .cloned()
    }
    fn set_last_read_message_id(
        &self,
        chat_channel_id: &str,
        message_id: &str,
        scope: MultiChatScope,
    ) {
        self.inner
            .lock()
            .unwrap()
            .last_read
            .insert((chat_channel_id.to_string(), scope), message_id.to_string());
    }
    fn current_mention_rule(&self, nickname: Option<&str>) -> MentionDetectionRule {
        let g = self.inner.lock().unwrap();
        MentionDetectionRule::new(
            nickname.map(str::to_string),
            g.keywords.clone(),
            g.muted_channels.clone(),
        )
    }
    fn increment_mention_unread(&self, chat_channel_id: &str, scope: MultiChatScope) {
        *self
            .inner
            .lock()
            .unwrap()
            .unread
            .entry((chat_channel_id.to_string(), scope))
            .or_insert(0) += 1;
    }
}

/// `MentionInbox.Entry`.
#[derive(Debug, Clone, PartialEq)]
pub struct MentionInboxEntry {
    pub id: String,
    pub stream_channel_id: String,
    pub chat_channel_id: String,
    pub channel_name: String,
    pub message: ChatMessageItem,
    pub matches: HashSet<MentionMatch>,
    pub received_at: DateTime<Utc>,
}

/// 멘션 부수효과(인박스 적재 + OS 알림).
pub trait MentionSink: Send + Sync {
    fn deliver(&self, entry: MentionInboxEntry, scope: MultiChatScope);
}

/// 기본: 무시.
#[derive(Debug, Default)]
pub struct NoopMentionSink;
impl MentionSink for NoopMentionSink {
    fn deliver(&self, _entry: MentionInboxEntry, _scope: MultiChatScope) {}
}

/// 기록용 구현 (테스트).
#[derive(Debug, Default)]
pub struct RecordingMentionSink {
    pub entries: Mutex<Vec<(MentionInboxEntry, MultiChatScope)>>,
}
impl MentionSink for RecordingMentionSink {
    fn deliver(&self, entry: MentionInboxEntry, scope: MultiChatScope) {
        self.entries.lock().unwrap().push((entry, scope));
    }
}

/// `PinnedNoticeCache` — 채널별 고정 공지 + 닫은 공지 id.
pub trait PinnedNoticeStore: Send + Sync {
    fn save(&self, notice: &ChatMessageItem, channel_id: &str);
    fn load(&self, channel_id: &str) -> Option<ChatMessageItem>;
    fn mark_dismissed(&self, notice_id: &str);
    fn is_dismissed(&self, notice_id: &str) -> bool;
    fn clear(&self, channel_id: &str);
}

/// `PinnedNoticeCache.PersistedNotice`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedNotice {
    pub id: String,
    pub nickname: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

impl PersistedNotice {
    pub fn from_item(item: &ChatMessageItem) -> Self {
        Self {
            id: item.id.clone(),
            nickname: item.nickname.clone(),
            content: item.content.clone(),
            timestamp: item.timestamp,
        }
    }
    fn is_same_notice(&self, other: &Self) -> bool {
        self.id == other.id && self.nickname == other.nickname && self.content == other.content
    }
    pub fn to_chat_message_item(&self) -> ChatMessageItem {
        ChatMessageItem::new(
            self.id.clone(),
            "notice",
            self.nickname.clone(),
            self.content.clone(),
            self.timestamp,
            MessageType::Notice,
            EmojiMap::new(),
            true,
            false,
        )
    }
}

/// `PinnedNoticeCache` 의 규칙(같은 공지 24h 내 재저장 생략, 채널 200개·닫힘 500개 상한)을
/// 메모리에 구현한 저장소. 영구 저장은 서비스 계층 구현이 이 trait 을 맞추면 된다.
#[derive(Debug, Default)]
pub struct MemoryPinnedNoticeStore {
    inner: Mutex<PinnedInner>,
}

#[derive(Debug, Default)]
struct PinnedInner {
    notices: HashMap<String, PersistedNotice>,
    dismissed: VecDeque<String>,
}

impl MemoryPinnedNoticeStore {
    pub const DISMISSED_IDS_MAX_COUNT: usize = 500;
    pub const NOTICE_CHANNEL_MAX_COUNT: usize = 200;
    pub const RECENCY_REFRESH_INTERVAL_SECS: i64 = 24 * 60 * 60;

    pub fn new() -> Self {
        Self::default()
    }
    pub fn stored(&self, channel_id: &str) -> Option<PersistedNotice> {
        self.inner.lock().unwrap().notices.get(channel_id).cloned()
    }
    pub fn channel_count(&self) -> usize {
        self.inner.lock().unwrap().notices.len()
    }
}

impl PinnedNoticeStore for MemoryPinnedNoticeStore {
    fn save(&self, notice: &ChatMessageItem, channel_id: &str) {
        if channel_id.is_empty() || notice.id.is_empty() {
            return;
        }
        let persisted = PersistedNotice::from_item(notice);
        let mut g = self.inner.lock().unwrap();
        if let Some(stored) = g.notices.get(channel_id) {
            if stored.is_same_notice(&persisted)
                && persisted.timestamp - stored.timestamp
                    <= ChronoDuration::seconds(Self::RECENCY_REFRESH_INTERVAL_SECS)
            {
                return;
            }
        }
        g.notices.insert(channel_id.to_string(), persisted);
        if g.notices.len() > Self::NOTICE_CHANNEL_MAX_COUNT {
            let mut dated: Vec<(String, DateTime<Utc>)> = g
                .notices
                .iter()
                .map(|(k, v)| (k.clone(), v.timestamp))
                .collect();
            dated.sort_by_key(|(_, t)| *t);
            let excess = g.notices.len() - Self::NOTICE_CHANNEL_MAX_COUNT;
            for (k, _) in dated.into_iter().take(excess) {
                g.notices.remove(&k);
            }
        }
    }
    fn load(&self, channel_id: &str) -> Option<ChatMessageItem> {
        if channel_id.is_empty() {
            return None;
        }
        self.stored(channel_id).map(|p| p.to_chat_message_item())
    }
    fn mark_dismissed(&self, notice_id: &str) {
        if notice_id.is_empty() {
            return;
        }
        let mut g = self.inner.lock().unwrap();
        g.dismissed.retain(|i| i != notice_id);
        g.dismissed.push_back(notice_id.to_string());
        while g.dismissed.len() > Self::DISMISSED_IDS_MAX_COUNT {
            g.dismissed.pop_front();
        }
    }
    fn is_dismissed(&self, notice_id: &str) -> bool {
        !notice_id.is_empty()
            && self
                .inner
                .lock()
                .unwrap()
                .dismissed
                .iter()
                .any(|i| i == notice_id)
    }
    fn clear(&self, channel_id: &str) {
        if channel_id.is_empty() {
            return;
        }
        self.inner.lock().unwrap().notices.remove(channel_id);
    }
}

/// `ChannelHeaderHub` — 채널 헤더(랭킹·라이브 상태) 공유 조회.
#[async_trait]
pub trait ChannelHeaderSource: Send + Sync {
    async fn ranking(
        &self,
        channel_id: &str,
        rank_count: i64,
    ) -> Result<DonationRankResponse, String>;
    async fn live_status(&self, channel_id: &str) -> Result<LiveInfo, String>;
}

#[async_trait]
impl ChannelHeaderSource for ChzzkApiClient {
    async fn ranking(
        &self,
        channel_id: &str,
        rank_count: i64,
    ) -> Result<DonationRankResponse, String> {
        self.weekly_donation_ranking(channel_id, rank_count, true)
            .await
            .map_err(|e| e.to_string())
    }
    async fn live_status(&self, channel_id: &str) -> Result<LiveInfo, String> {
        ChzzkApiClient::live_status(self, channel_id)
            .await
            .map_err(|e| e.to_string())
    }
}

/// 이모티콘 이미지 선읽기.
pub trait ImagePrefetcher: Send + Sync {
    fn prefetch(&self, urls: Vec<String>);
}

#[derive(Debug, Default)]
pub struct NoopPrefetcher;
impl ImagePrefetcher for NoopPrefetcher {
    fn prefetch(&self, _urls: Vec<String>) {}
}

/// 렌더 예산·창 변형·시스템 부하 신호 (Swift 전역 싱글턴들).
pub trait PacingEnvironment: Send + Sync {
    /// `RenderBudgetController.shared.snapshot.tier == .suspended`
    fn is_render_suspended(&self) -> bool {
        false
    }
    /// `RenderBudgetController.shared.prefersRelaxedCommitPacing`
    fn prefers_relaxed_commit_pacing(&self) -> bool {
        false
    }
    /// `WindowGeometryTransitionSignal.shared.isTransitioning`
    fn is_window_geometry_transitioning(&self) -> bool {
        false
    }
    /// `SystemLoadMonitor.shared.currentMode`
    fn system_load_mode(&self) -> SystemLoadMode {
        SystemLoadMode::Normal
    }
}

#[derive(Debug, Default)]
pub struct DefaultPacingEnvironment;
impl PacingEnvironment for DefaultPacingEnvironment {}

/// 되돌리기 버튼이 달린 토스트.
pub trait ToastPresenter: Send + Sync {
    fn show(
        &self,
        message: String,
        undo_title: Option<String>,
        undo: Option<Box<dyn FnOnce() + Send>>,
    );
}

#[derive(Debug, Default)]
pub struct NoopToast;
impl ToastPresenter for NoopToast {
    fn show(&self, _m: String, _t: Option<String>, _u: Option<Box<dyn FnOnce() + Send>>) {}
}

/// ChatViewModel 협력자 묶음.
#[derive(Clone)]
pub struct ChatDeps {
    pub engine_factory: ChatEngineFactory,
    pub preferences: Arc<dyn ChatPreferences>,
    pub mention_sink: Arc<dyn MentionSink>,
    pub pinned_notices: Arc<dyn PinnedNoticeStore>,
    pub prefetcher: Arc<dyn ImagePrefetcher>,
    pub pacing: Arc<dyn PacingEnvironment>,
    pub toast: Arc<dyn ToastPresenter>,
    pub tts: Arc<dyn ChatTts>,
}

impl Default for ChatDeps {
    fn default() -> Self {
        Self {
            engine_factory: LiveChatEngine::factory(),
            preferences: Arc::new(MemoryChatPreferences::new()),
            mention_sink: Arc::new(NoopMentionSink),
            pinned_notices: Arc::new(MemoryPinnedNoticeStore::new()),
            prefetcher: Arc::new(NoopPrefetcher),
            pacing: Arc::new(DefaultPacingEnvironment),
            toast: Arc::new(NoopToast),
            tts: Arc::new(NoopTts),
        }
    }
}

impl std::fmt::Debug for ChatDeps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatDeps").finish_non_exhaustive()
    }
}
