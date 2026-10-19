//! `Sources/CViewApp/Services/NotificationService.swift`
//! (+ `NotificationService+AuthorizationState.swift`, `MentionNotificationService.swift`) 포팅.
//!
//! UserNotifications/AppKit 에 묶인 부분은 [`Notifier`] trait 뒤로 옮겼다. 여기 남은 것은
//! 플랫폼과 무관한 규칙이다 — 알림 문구, 요청 식별자, 카테고리·액션 식별자, 권한 래치(전달 가드),
//! 알림 탭 처리(`WATCH_ACTION`/기본 액션 → 시청 콜백), 멘션 알림의 사운드·센터 토글.
//!
//! 플랫폼 전용이라 옮기지 않은 것: LaunchServices 재등록(`lsregister`), 채널 아이콘 256px 변환·
//! 24시간 디스크 캐시, 앱 아이콘 첨부·임시 파일 정리 — 셸의 `Notifier` 구현 몫이다.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use cview_core::logic::mention_detector::MentionMatch;
use cview_core::models::chat::ChatMessageItem;
use cview_core::utils::time::Date;

use super::background_update::{ChannelChangeInfo, OnlineChannel};
use super::multi_chat_preferences::MultiChatPreferences;
use super::support::{lock, system_clock, Clock};

/// 권한 상태 세 갈래.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationAuthorization {
    /// 전달 가능.
    Authorized,
    /// 한 번도 묻지 않았다.
    NotDetermined,
    /// 거부·제한(또는 알림 센터를 쓸 수 없는 환경).
    Denied,
}

/// 보낼 로컬 알림 한 건.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalNotification {
    pub identifier: String,
    pub title: String,
    pub body: String,
    pub category: Option<String>,
    pub user_info: HashMap<String, String>,
    /// 첨부할 채널 프로필 이미지(없으면 구현이 앱 아이콘으로 대신한다).
    pub image_url: Option<String>,
    pub sound: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NotifierError {
    /// 시스템이 알림을 허용하지 않는다 — 래치를 내린다.
    #[error("notifications not allowed")]
    NotAllowed,
    #[error("{0}")]
    Other(String),
}

/// 플랫폼 알림 전달자. 셸(Tauri 등)이 구현한다.
#[async_trait]
pub trait Notifier: Send + Sync {
    /// 수신 배선(델리게이트·카테고리 등록). 권한 대화상자는 띄우지 않는다.
    fn prepare_delivery(&self) {}
    /// 현재 권한 상태(대화상자 없음).
    async fn authorization(&self) -> NotificationAuthorization;
    /// 권한 요청 — 사용자가 알림을 켜는 시점에만.
    async fn request_authorization(&self) -> bool;
    async fn deliver(&self, notification: LocalNotification) -> Result<(), NotifierError>;
    /// 짧은 효과음(예: "Glass").
    fn play_sound(&self, _name: &str) {}
}

/// 알림을 쓸 수 없는 환경용.
#[derive(Debug, Default)]
pub struct NoopNotifier;

#[async_trait]
impl Notifier for NoopNotifier {
    async fn authorization(&self) -> NotificationAuthorization {
        NotificationAuthorization::Denied
    }
    async fn request_authorization(&self) -> bool {
        false
    }
    async fn deliver(&self, _: LocalNotification) -> Result<(), NotifierError> {
        Err(NotifierError::NotAllowed)
    }
}

pub const STREAMER_ONLINE_CATEGORY: &str = "STREAMER_ONLINE";
pub const WATCH_ACTION: &str = "WATCH_ACTION";
pub const DISMISS_ACTION: &str = "DISMISS_ACTION";
/// 시스템 기본 액션(알림 본문 클릭).
pub const DEFAULT_ACTION: &str = "com.apple.UNNotificationDefaultActionIdentifier";
pub const WATCH_ACTION_TITLE: &str = "시청하기";
pub const DISMISS_ACTION_TITLE: &str = "닫기";

type WatchChannel = Arc<dyn Fn(String) + Send + Sync>;

/// 로컬 알림 서비스.
pub struct NotificationService {
    notifier: Arc<dyn Notifier>,
    is_authorized: AtomicBool,
    on_watch_channel: Mutex<Option<WatchChannel>>,
    clock: Clock,
}

impl NotificationService {
    pub fn new(notifier: Arc<dyn Notifier>) -> Self {
        Self::with_clock(notifier, system_clock())
    }

    pub fn with_clock(notifier: Arc<dyn Notifier>, clock: Clock) -> Self {
        Self {
            notifier,
            is_authorized: AtomicBool::new(false),
            on_watch_channel: Mutex::new(None),
            clock,
        }
    }

    pub fn is_authorized(&self) -> bool {
        self.is_authorized.load(Ordering::SeqCst)
    }

    pub fn set_on_watch_channel(&self, f: Option<WatchChannel>) {
        *lock(&self.on_watch_channel) = f;
    }

    /// 수신 배선만 — 부팅 경로는 이것만 부른다.
    pub fn prepare_delivery(&self) {
        self.notifier.prepare_delivery();
    }

    /// 권한 요청 — 사용자가 알림을 켜는 시점에만.
    pub async fn request_authorization(&self) -> bool {
        self.prepare_delivery();
        let granted = self.notifier.request_authorization().await;
        self.is_authorized.store(granted, Ordering::SeqCst);
        tracing::info!("Notification authorization: {granted}");
        granted
    }

    /// 현재 권한 상태를 읽고 래치를 맞춘다.
    pub async fn check_authorization_status(&self) -> bool {
        let ok = self.notifier.authorization().await == NotificationAuthorization::Authorized;
        self.is_authorized.store(ok, Ordering::SeqCst);
        ok
    }

    pub async fn authorization_state(&self) -> NotificationAuthorization {
        let state = self.notifier.authorization().await;
        self.is_authorized.store(
            state == NotificationAuthorization::Authorized,
            Ordering::SeqCst,
        );
        state
    }

    /// 전달 가드 — 래치가 꺼져 있으면 시스템 상태를 한 번 다시 읽는다.
    pub async fn delivery_allowed(&self) -> bool {
        if self.is_authorized() {
            return true;
        }
        self.check_authorization_status().await
    }

    // MARK: 문구

    pub fn online_notification(&self, channel: &OnlineChannel) -> LocalNotification {
        let body = if channel.live_title.is_empty() {
            format!("{}님이 방송을 시작했습니다.", channel.channel_name)
        } else {
            channel.live_title.clone()
        };
        self.live_notification(
            format!("{} 방송 시작!", channel.channel_name),
            body,
            &channel.channel_id,
            &channel.channel_name,
            channel.channel_image_url.clone(),
            "online",
        )
    }

    pub fn category_notification(&self, change: &ChannelChangeInfo) -> LocalNotification {
        self.live_notification(
            format!("{} 카테고리 변경", change.channel_name),
            format!("{} → {}", change.old_value, change.new_value),
            &change.channel_id,
            &change.channel_name,
            change.channel_image_url.clone(),
            "category",
        )
    }

    pub fn title_notification(&self, change: &ChannelChangeInfo) -> LocalNotification {
        self.live_notification(
            format!("{} 방송 제목 변경", change.channel_name),
            change.new_value.clone(),
            &change.channel_id,
            &change.channel_name,
            change.channel_image_url.clone(),
            "title",
        )
    }

    fn live_notification(
        &self,
        title: String,
        body: String,
        channel_id: &str,
        channel_name: &str,
        image_url: Option<String>,
        prefix: &str,
    ) -> LocalNotification {
        let now: Date = (self.clock)();
        let secs = now.timestamp_millis() as f64 / 1000.0;
        LocalNotification {
            identifier: format!("{prefix}-{channel_id}-{secs:?}"),
            title,
            body,
            category: Some(STREAMER_ONLINE_CATEGORY.into()),
            user_info: HashMap::from([
                ("channelId".to_string(), channel_id.to_string()),
                ("channelName".to_string(), channel_name.to_string()),
            ]),
            image_url,
            sound: true,
        }
    }

    // MARK: 발송

    pub async fn notify_streamer_online(&self, channels: &[OnlineChannel]) {
        for channel in channels {
            self.dispatch(self.online_notification(channel)).await;
        }
    }

    pub async fn notify_category_change(&self, changes: &[ChannelChangeInfo]) {
        for change in changes {
            self.dispatch(self.category_notification(change)).await;
        }
    }

    pub async fn notify_title_change(&self, changes: &[ChannelChangeInfo]) {
        for change in changes {
            self.dispatch(self.title_notification(change)).await;
        }
    }

    async fn dispatch(&self, notification: LocalNotification) {
        if !self.delivery_allowed().await {
            return;
        }
        match self.notifier.deliver(notification).await {
            Ok(()) => {}
            Err(NotifierError::NotAllowed) => {
                // 실행 중 시스템 설정에서 꺼졌다 — 래치를 내려 다음부터 다시 읽게 한다.
                self.is_authorized.store(false, Ordering::SeqCst);
                tracing::error!("Failed to send notification: not allowed");
            }
            Err(e) => tracing::error!("Failed to send notification: {e}"),
        }
    }

    /// 알림 탭 처리 — 시청 액션이나 기본 액션이면 채널 ID 로 시청 콜백을 부른다.
    pub fn handle_response(&self, action: &str, user_info: &HashMap<String, String>) -> bool {
        if action != WATCH_ACTION && action != DEFAULT_ACTION {
            return false;
        }
        let Some(channel_id) = user_info.get("channelId") else {
            return false;
        };
        let callback = lock(&self.on_watch_channel).clone();
        if let Some(f) = callback {
            f(channel_id.clone());
        }
        true
    }
}

/// `MentionNotificationService` — 멘션 효과음·알림 센터 발화.
pub struct MentionNotificationService;

impl MentionNotificationService {
    pub fn notification(channel_name: &str, message: &ChatMessageItem) -> LocalNotification {
        LocalNotification {
            identifier: format!("mention.{}", message.id),
            title: format!("[{channel_name}] @{}", message.nickname),
            body: message.plain_text_preview(),
            category: None,
            user_info: HashMap::new(),
            image_url: None,
            sound: false,
        }
    }

    /// 설정이 켜 둔 만큼만 발화한다. 효과음은 "Glass".
    pub async fn deliver(
        notifier: &dyn Notifier,
        prefs: &MultiChatPreferences,
        channel_name: &str,
        message: &ChatMessageItem,
        _matches: &std::collections::HashSet<MentionMatch>,
    ) {
        if prefs.mention_sound_enabled() {
            notifier.play_sound("Glass");
        }
        if !prefs.mention_notification_center_enabled() {
            return;
        }
        let _ = notifier
            .deliver(Self::notification(channel_name, message))
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::support::{ManualClock, MemoryKeyValueStore};
    use cview_core::utils::time::from_unix;

    #[derive(Default)]
    struct Fake {
        auth: Mutex<Option<NotificationAuthorization>>,
        delivered: Mutex<Vec<LocalNotification>>,
        reject: AtomicBool,
        sounds: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl Notifier for Fake {
        async fn authorization(&self) -> NotificationAuthorization {
            lock(&self.auth).unwrap_or(NotificationAuthorization::NotDetermined)
        }
        async fn request_authorization(&self) -> bool {
            *lock(&self.auth) = Some(NotificationAuthorization::Authorized);
            true
        }
        async fn deliver(&self, n: LocalNotification) -> Result<(), NotifierError> {
            if self.reject.load(Ordering::SeqCst) {
                return Err(NotifierError::NotAllowed);
            }
            lock(&self.delivered).push(n);
            Ok(())
        }
        fn play_sound(&self, name: &str) {
            lock(&self.sounds).push(name.into());
        }
    }

    fn channel(title: &str) -> OnlineChannel {
        OnlineChannel {
            channel_id: "c1".into(),
            channel_name: "채널".into(),
            live_title: title.into(),
            viewer_count: 1,
            thumbnail_url: None,
            category_name: None,
            channel_image_url: Some("https://img".into()),
            open_date: None,
        }
    }

    #[tokio::test]
    async fn wording_guard_and_latch() {
        let fake = Arc::new(Fake::default());
        let clock = ManualClock::new(from_unix(1_700_000_000.0));
        let s = NotificationService::with_clock(fake.clone(), clock.clock());
        s.notify_streamer_online(&[channel("")]).await;
        assert!(
            lock(&fake.delivered).is_empty(),
            "권한이 없으면 보내지 않는다"
        );
        assert_eq!(
            s.authorization_state().await,
            NotificationAuthorization::NotDetermined
        );
        // 나중에 시스템 설정에서 허용 — 가드가 다시 읽는다.
        *lock(&fake.auth) = Some(NotificationAuthorization::Authorized);
        s.notify_streamer_online(&[channel("")]).await;
        let n = lock(&fake.delivered)[0].clone();
        assert_eq!(n.title, "채널 방송 시작!");
        assert_eq!(n.body, "채널님이 방송을 시작했습니다.");
        assert_eq!(n.identifier, "online-c1-1700000000.0");
        assert_eq!(n.category.as_deref(), Some(STREAMER_ONLINE_CATEGORY));

        fake.reject.store(true, Ordering::SeqCst);
        s.notify_title_change(&[ChannelChangeInfo {
            channel_id: "c1".into(),
            channel_name: "채널".into(),
            old_value: "a".into(),
            new_value: "b".into(),
            channel_image_url: None,
        }])
        .await;
        assert!(!s.is_authorized(), "거부 오류는 래치를 내린다");
    }

    #[test]
    fn response_routes_watch_actions_only() {
        let s = NotificationService::new(Arc::new(NoopNotifier));
        let seen: Arc<Mutex<Vec<String>>> = Arc::default();
        let sink = seen.clone();
        s.set_on_watch_channel(Some(Arc::new(move |id| lock(&sink).push(id))));
        let info = HashMap::from([("channelId".to_string(), "c".to_string())]);
        assert!(s.handle_response(WATCH_ACTION, &info));
        assert!(s.handle_response(DEFAULT_ACTION, &info));
        assert!(!s.handle_response(DISMISS_ACTION, &info));
        assert_eq!(lock(&seen).len(), 2);
    }

    #[tokio::test]
    async fn mention_respects_toggles() {
        let fake = Fake::default();
        let prefs = MultiChatPreferences::new(MemoryKeyValueStore::shared(), Arc::new(|| 0.0));
        let msg = ChatMessageItem::new(
            "m1",
            "u",
            "닉",
            "안녕",
            from_unix(0.0),
            Default::default(),
            Default::default(),
            false,
            false,
        );
        MentionNotificationService::deliver(&fake, &prefs, "방", &msg, &Default::default()).await;
        assert!(lock(&fake.sounds).is_empty());
        assert_eq!(lock(&fake.delivered)[0].title, "[방] @닉");
        assert_eq!(lock(&fake.delivered)[0].identifier, "mention.m1");
        prefs.set_mention_sound_enabled(true);
        prefs.set_mention_notification_center_enabled(false);
        MentionNotificationService::deliver(&fake, &prefs, "방", &msg, &Default::default()).await;
        assert_eq!(*lock(&fake.sounds), vec!["Glass".to_string()]);
        assert_eq!(lock(&fake.delivered).len(), 1);
    }
}
