//! ChatViewModel 이 기대는 외부 협력자.
//!
//! | Swift 협력자 | Rust |
//! |---|---|
//! | `ChatEngine` (actor) | [`ChatEngineHandle`] + [`ChatEngineFactory`] (기본: `cview_chat::ChatEngine`) |
//! | `MultiChatPreferences.shared` (lastRead·멘션 규칙·미읽음) | [`MultiChatPreferences`] (서비스 그대로) |
//! | `MentionInbox` · `MentionNotificationService` | [`MentionSink`] (기본 구현: [`InboxMentionSink`]) |
//! | `PinnedNoticeCache` (UserDefaults) | [`PinnedNoticeCache`] (서비스 그대로) |
//! | `ChannelHeaderHub.shared` (랭킹·라이브 상태) | [`ChannelHeaderSource`] (서비스 trait, 기본: `ChzzkApiClient`) |
//! | `ImageCacheService.shared.prefetch` | [`ImagePrefetcher`] |
//! | `RenderBudgetController` · `WindowGeometryTransitionSignal` · `SystemLoadMonitor` | [`PacingEnvironment`] |
//! | `ToastCenter.shared` | [`ToastPresenter`] |
//! | `ChatTTSService` (AVSpeechSynthesizer) | [`super::tts::ChatTts`] |

use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use cview_chat::{ChatEngine, ChatEngineConfiguration, ChatEngineEvent, ChatEventStream};
use cview_core::models::chat::{ChatMessage, EmojiMap};
use cview_core::utils::SystemLoadMode;

use crate::services::notification_service::{MentionNotificationService, Notifier};
use crate::services::support::MemoryKeyValueStore;

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

pub use crate::services::channel_header_hub::ChannelHeaderSource;
pub use crate::services::mention_inbox::{MentionEntry, MentionInbox};
pub use crate::services::multi_chat_preferences::{MultiChatPreferences, MultiChatScope};
pub use crate::services::pinned_notice_cache::PinnedNoticeCache;

/// 테스트·기본값용 메모리 저장소 위의 새 [`MultiChatPreferences`].
pub fn memory_chat_preferences() -> Arc<MultiChatPreferences> {
    Arc::new(MultiChatPreferences::new(
        Arc::new(MemoryKeyValueStore::new()),
        Arc::new(|| chrono::Utc::now().timestamp_millis() as f64 / 1000.0),
    ))
}

/// 테스트·기본값용 메모리 저장소 위의 새 [`PinnedNoticeCache`].
pub fn memory_pinned_notice_cache() -> Arc<PinnedNoticeCache> {
    Arc::new(PinnedNoticeCache::new(Arc::new(MemoryKeyValueStore::new())))
}

/// 멘션 부수효과(인박스 적재 + OS 알림).
pub trait MentionSink: Send + Sync {
    fn deliver(&self, entry: MentionEntry, scope: MultiChatScope);
}

/// 기본: 무시.
#[derive(Debug, Default)]
pub struct NoopMentionSink;
impl MentionSink for NoopMentionSink {
    fn deliver(&self, _entry: MentionEntry, _scope: MultiChatScope) {}
}

/// 실제 경로 — 스코프별 [`MentionInbox`] 에 적재하고, 알림기가 있으면
/// [`MentionNotificationService`] 로 효과음·알림 센터를 발화한다.
pub struct InboxMentionSink {
    pub notifier: Option<Arc<dyn Notifier>>,
    pub preferences: Arc<MultiChatPreferences>,
}

impl InboxMentionSink {
    pub fn new(
        notifier: Option<Arc<dyn Notifier>>,
        preferences: Arc<MultiChatPreferences>,
    ) -> Self {
        Self {
            notifier,
            preferences,
        }
    }
}

impl MentionSink for InboxMentionSink {
    fn deliver(&self, entry: MentionEntry, scope: MultiChatScope) {
        if let (Some(notifier), Ok(handle)) =
            (self.notifier.clone(), tokio::runtime::Handle::try_current())
        {
            let prefs = self.preferences.clone();
            let channel_name = entry.channel_name.clone();
            let message = entry.message.clone();
            let matches = entry.matches.clone();
            handle.spawn(async move {
                MentionNotificationService::deliver(
                    notifier.as_ref(),
                    &prefs,
                    &channel_name,
                    &message,
                    &matches,
                )
                .await;
            });
        }
        MentionInbox::scoped(scope).append(entry);
    }
}

/// 기록용 구현 (테스트).
#[derive(Debug, Default)]
pub struct RecordingMentionSink {
    pub entries: Mutex<Vec<(MentionEntry, MultiChatScope)>>,
}
impl MentionSink for RecordingMentionSink {
    fn deliver(&self, entry: MentionEntry, scope: MultiChatScope) {
        self.entries.lock().unwrap().push((entry, scope));
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
    pub preferences: Arc<MultiChatPreferences>,
    pub mention_sink: Arc<dyn MentionSink>,
    pub pinned_notices: Arc<PinnedNoticeCache>,
    pub prefetcher: Arc<dyn ImagePrefetcher>,
    pub pacing: Arc<dyn PacingEnvironment>,
    pub toast: Arc<dyn ToastPresenter>,
    pub tts: Arc<dyn ChatTts>,
}

impl Default for ChatDeps {
    fn default() -> Self {
        Self {
            engine_factory: LiveChatEngine::factory(),
            preferences: memory_chat_preferences(),
            mention_sink: Arc::new(NoopMentionSink),
            pinned_notices: memory_pinned_notice_cache(),
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
