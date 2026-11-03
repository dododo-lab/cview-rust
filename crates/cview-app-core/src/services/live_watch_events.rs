//! 치지직 라이브 시청 이벤트 — 웹 플레이어와 같은 순서로 `watch-event/live` 를 보내 치지직이 이 앱의
//! 시청을 시청자 수·계정 시청 기록·시청 시간에 넣게 한다.
//!
//! 웹(`chzzk.naver.com` 번들의 시청 훅)이 하는 일:
//! - 처음 재생되면 `WATCH_STARTED`, 일시정지 뒤 다시 재생되면 `WATCH_RESUMED` — 이때부터 30초 주기를 다시 센다.
//! - 재생 중 30초마다 `WATCH_CONTINUED`(직전 전송 뒤 29초가 안 지났으면 건너뛴다).
//! - 일시정지면 `WATCH_PAUSED` 를 보내고 주기를 멈춘다. 끝나거나 화면을 떠나면 `WATCH_ENDED`.
//! - `awt` 는 이번 시청에서 실제로 재생한 누적 초.
//!
//! 앱은 웹과 달리 창이 오래 가려지면(백그라운드) 일시정지와 같이 다룬다 — 판단은 플레이어 페이지가 해서
//! `watchActivity` 로 알린다. 같은 채널을 여러 화면(단독 시청·멀티라이브 패인)이 동시에 재생해도 채널마다
//! 시청 세션은 하나다(중복 세션을 만들지 않는다).

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cview_core::error::ApiError;
use cview_networking::client_watch::{
    new_watch_session_id, LiveWatchEvent, LiveWatchStatus, WatchEventType,
};
use cview_networking::ChzzkApiClient;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::support::lock;

/// 웹과 같은 `WATCH_CONTINUED` 주기.
pub const WATCH_EVENT_PERIOD: Duration = Duration::from_secs(30);
/// 주기 타이머가 조금 일찍 깨어도 같은 주기로 본다(웹: `f - 1e3`).
const PERIOD_SLACK: Duration = Duration::from_secs(1);
/// `liveTokenList` 를 다시 읽는 최소 간격(웹은 live-status 를 10초마다 다시 읽는다).
const STATUS_MAX_AGE: Duration = Duration::from_secs(10);
/// 종료 시 `WATCH_ENDED` 전송을 기다리는 상한 — 앱 종료 정리 전체 상한(3초)보다 짧다.
const END_FLUSH_TIMEOUT: Duration = Duration::from_secs(2);
/// 보내지 못하고 쌓아 두는 이벤트 상한.
const MAX_PENDING: usize = 8;

/// 한 시청 세션의 이벤트 순서와 누적 재생 시간 — 네트워크 없는 순수 상태기계.
#[derive(Debug, Clone, Default)]
pub struct WatchEventTracker {
    started: bool,
    ended: bool,
    active_since: Option<Instant>,
    accumulated: Duration,
    last_sent: Option<Instant>,
}

impl WatchEventTracker {
    pub fn is_active(&self) -> bool {
        self.active_since.is_some()
    }

    pub fn is_ended(&self) -> bool {
        self.ended
    }

    /// 실제로 재생 중인지 바뀜 → 보낼 이벤트.
    pub fn set_active(&mut self, active: bool, now: Instant) -> Option<WatchEventType> {
        if self.ended || active == self.is_active() {
            return None;
        }
        if active {
            self.active_since = Some(now);
            self.last_sent = Some(now);
            if self.started {
                Some(WatchEventType::Resumed)
            } else {
                self.started = true;
                Some(WatchEventType::Started)
            }
        } else {
            self.bank(now);
            self.active_since = None;
            Some(WatchEventType::Paused)
        }
    }

    /// 다음 `WATCH_CONTINUED` 시각(재생 중일 때만).
    pub fn next_due(&self) -> Option<Instant> {
        self.active_since?;
        self.last_sent.map(|t| t + WATCH_EVENT_PERIOD)
    }

    /// 주기가 됐으면 `WATCH_CONTINUED`.
    pub fn tick(&mut self, now: Instant) -> Option<WatchEventType> {
        if self.ended || !self.is_active() {
            return None;
        }
        let last = self.last_sent?;
        if now.saturating_duration_since(last) + PERIOD_SLACK < WATCH_EVENT_PERIOD {
            return None;
        }
        self.bank(now);
        self.active_since = Some(now);
        self.last_sent = Some(now);
        Some(WatchEventType::Continued)
    }

    /// 시청 끝. 시작한 적이 없으면 보낼 것이 없다.
    pub fn end(&mut self, now: Instant) -> Option<WatchEventType> {
        if self.ended || !self.started {
            self.ended = true;
            return None;
        }
        self.bank(now);
        self.active_since = None;
        self.ended = true;
        Some(WatchEventType::Ended)
    }

    /// 이번 시청의 누적 재생 초(진행 중인 구간 포함).
    pub fn accumulated_secs(&self, now: Instant) -> i64 {
        let running = self
            .active_since
            .map(|t| now.saturating_duration_since(t))
            .unwrap_or_default();
        (self.accumulated + running).as_secs() as i64
    }

    fn bank(&mut self, now: Instant) {
        if let Some(t) = self.active_since {
            self.accumulated += now.saturating_duration_since(t);
        }
    }
}

/// 시청 이벤트 전송원(테스트는 가짜를 넣는다).
#[async_trait]
pub trait WatchEventApi: Send + Sync {
    async fn live_watch_status(&self, channel_id: &str) -> Result<LiveWatchStatus, ApiError>;
    async fn current_live_id(&self, channel_id: &str) -> Result<i64, ApiError>;
    async fn post_live_watch_event(&self, event: &LiveWatchEvent) -> Result<(), ApiError>;
}

#[async_trait]
impl WatchEventApi for ChzzkApiClient {
    async fn live_watch_status(&self, channel_id: &str) -> Result<LiveWatchStatus, ApiError> {
        ChzzkApiClient::live_watch_status(self, channel_id).await
    }
    async fn current_live_id(&self, channel_id: &str) -> Result<i64, ApiError> {
        Ok(self.live_detail(channel_id, false).await?.live_id)
    }
    async fn post_live_watch_event(&self, event: &LiveWatchEvent) -> Result<(), ApiError> {
        ChzzkApiClient::post_live_watch_event(self, event).await
    }
}

/// 화면에서 상태가 바뀐 시각 — 앞 전송이 늦어 작업이 늦게 처리해도 재생 시간은 이 시각으로 센다.
#[derive(Debug, Clone, Copy)]
struct Stamp {
    at: Instant,
    wall: DateTime<Utc>,
}

impl Stamp {
    fn now() -> Self {
        Self {
            at: Instant::now(),
            wall: Utc::now(),
        }
    }
}

enum Command {
    Active(bool, Stamp),
    End(Stamp),
}

struct Source {
    channel_id: String,
    active: bool,
}

struct ChannelEntry {
    tx: mpsc::UnboundedSender<Command>,
    task: JoinHandle<()>,
    active: bool,
}

#[derive(Default)]
struct Inner {
    /// 화면(단독 시청 `viewer`, 멀티라이브 패인 등) → 그 화면이 보는 채널.
    sources: HashMap<String, Source>,
    channels: HashMap<String, ChannelEntry>,
    /// 종료를 보냈지만 아직 `WATCH_ENDED` 를 전송 중인 작업 — 앱 종료가 함께 기다린다.
    finishing: Vec<JoinHandle<()>>,
}

/// 시청 이벤트 보고자. `Clone` 은 같은 보고자를 가리킨다.
#[derive(Clone)]
pub struct LiveWatchReporter {
    inner: Arc<Mutex<Inner>>,
    api: Arc<dyn WatchEventApi>,
}

impl LiveWatchReporter {
    pub fn new(api: Arc<dyn WatchEventApi>) -> Self {
        Self {
            inner: Arc::default(),
            api,
        }
    }

    /// 화면이 채널 재생을 시작했다(아직 재생 중으로 보지 않는다 — `set_active` 가 알린다).
    /// `live_id` 를 모르면 보고자가 라이브 상세에서 찾는다.
    pub fn begin(&self, source: &str, channel_id: &str, live_id: Option<i64>) {
        if channel_id.is_empty() {
            return;
        }
        let mut inner = lock(&self.inner);
        if inner
            .sources
            .get(source)
            .is_some_and(|s| s.channel_id == channel_id)
        {
            return;
        }
        Self::detach_locked(&mut inner, source);
        inner.sources.insert(
            source.to_string(),
            Source {
                channel_id: channel_id.to_string(),
                active: false,
            },
        );
        if !inner.channels.contains_key(channel_id) {
            let (tx, rx) = mpsc::unbounded_channel();
            let worker = Worker {
                api: self.api.clone(),
                channel_id: channel_id.to_string(),
                live_id,
                session_id: new_watch_session_id(),
                tracker: WatchEventTracker::default(),
                status: None,
                pending: VecDeque::new(),
            };
            let task = tokio::spawn(worker.run(rx));
            inner.channels.insert(
                channel_id.to_string(),
                ChannelEntry {
                    tx,
                    task,
                    active: false,
                },
            );
        }
    }

    /// 화면의 영상이 실제로 재생 중인지(보이는 상태로). 등록되지 않은 화면은 무시한다.
    pub fn set_active(&self, source: &str, active: bool) {
        let mut inner = lock(&self.inner);
        let Some(s) = inner.sources.get_mut(source) else {
            return;
        };
        s.active = active;
        let channel_id = s.channel_id.clone();
        Self::sync_channel(&mut inner, &channel_id);
    }

    /// 화면이 재생을 멈췄다(정지·채널 전환·창 닫힘·방송 영상 끝).
    pub fn end(&self, source: &str) {
        let mut inner = lock(&self.inner);
        Self::detach_locked(&mut inner, source);
    }

    /// 앱 종료 — 모든 시청 세션에 `WATCH_ENDED` 를 보내고, 이미 종료 중인 세션까지 잠시 기다린다.
    pub async fn end_all(&self) {
        let tasks: Vec<JoinHandle<()>> = {
            let mut inner = lock(&self.inner);
            inner.sources.clear();
            let stamp = Stamp::now();
            let channels: Vec<_> = inner.channels.drain().map(|(_, e)| e).collect();
            let mut tasks = std::mem::take(&mut inner.finishing);
            for entry in channels {
                let _ = entry.tx.send(Command::End(stamp));
                tasks.push(entry.task);
            }
            tasks
        };
        let _ =
            tokio::time::timeout(END_FLUSH_TIMEOUT, futures_util::future::join_all(tasks)).await;
    }

    /// 열린 시청 세션 수(채널 단위).
    pub fn session_count(&self) -> usize {
        lock(&self.inner).channels.len()
    }

    fn detach_locked(inner: &mut Inner, source: &str) {
        let Some(old) = inner.sources.remove(source) else {
            return;
        };
        let still_used = inner
            .sources
            .values()
            .any(|s| s.channel_id == old.channel_id);
        if still_used {
            Self::sync_channel(inner, &old.channel_id);
        } else if let Some(entry) = inner.channels.remove(&old.channel_id) {
            let _ = entry.tx.send(Command::End(Stamp::now()));
            inner.finishing.retain(|t| !t.is_finished());
            inner.finishing.push(entry.task);
        }
    }

    /// 채널을 보는 화면 중 하나라도 재생 중이면 채널 세션도 재생 중이다.
    fn sync_channel(inner: &mut Inner, channel_id: &str) {
        let active = inner
            .sources
            .values()
            .any(|s| s.channel_id == channel_id && s.active);
        if let Some(entry) = inner.channels.get_mut(channel_id) {
            if entry.active != active {
                entry.active = active;
                let _ = entry.tx.send(Command::Active(active, Stamp::now()));
            }
        }
    }
}

/// 보낼 이벤트 — 종류·누적 재생 시간·시각은 상태가 바뀐 그때 정한다.
#[derive(Debug, Clone, Copy)]
struct PendingEvent {
    kind: WatchEventType,
    awt: i64,
    wall: DateTime<Utc>,
}

/// 채널 하나의 시청 세션 — 이벤트를 순서대로 보낸다.
struct Worker {
    api: Arc<dyn WatchEventApi>,
    channel_id: String,
    live_id: Option<i64>,
    session_id: String,
    tracker: WatchEventTracker,
    status: Option<(Instant, LiveWatchStatus)>,
    /// 방송 상태·라이브 ID 를 아직 못 읽어 보내지 못한 이벤트(시작 이벤트를 잃지 않게 순서대로 둔다).
    pending: VecDeque<PendingEvent>,
}

impl Worker {
    async fn run(mut self, mut rx: mpsc::UnboundedReceiver<Command>) {
        loop {
            let due = self.tracker.next_due();
            let command = match due {
                Some(at) => tokio::select! {
                    // 밀린 명령이 주기보다 먼저다 — 그 사이 일시정지했으면 주기 이벤트가 없다.
                    biased;
                    c = rx.recv() => c,
                    _ = tokio::time::sleep_until(at) => {
                        let stamp = Stamp::now();
                        if let Some(kind) = self.tracker.tick(stamp.at) {
                            self.queue(kind, stamp);
                        }
                        self.flush().await;
                        continue;
                    }
                },
                None => rx.recv().await,
            };
            match command {
                Some(Command::Active(active, stamp)) => {
                    if let Some(kind) = self.tracker.set_active(active, stamp.at) {
                        self.queue(kind, stamp);
                    }
                    self.flush().await;
                }
                Some(Command::End(stamp)) => {
                    self.finish(stamp).await;
                    return;
                }
                None => {
                    self.finish(Stamp::now()).await;
                    return;
                }
            }
        }
    }

    async fn finish(&mut self, stamp: Stamp) {
        if let Some(kind) = self.tracker.end(stamp.at) {
            self.queue(kind, stamp);
        }
        self.flush().await;
    }

    fn queue(&mut self, kind: WatchEventType, stamp: Stamp) {
        // 계속 실패하는 동안 쌓이지 않게 상한을 둔다(시작 이벤트는 남긴다).
        if self.pending.len() >= MAX_PENDING {
            let drop_at =
                usize::from(self.pending.front().map(|e| e.kind) == Some(WatchEventType::Started));
            self.pending.remove(drop_at);
        }
        self.pending.push_back(PendingEvent {
            kind,
            awt: self.tracker.accumulated_secs(stamp.at),
            wall: stamp.wall,
        });
    }

    async fn fresh_status(&mut self) -> Option<LiveWatchStatus> {
        if let Some((at, status)) = &self.status {
            if at.elapsed() < STATUS_MAX_AGE {
                return Some(status.clone());
            }
        }
        match self.api.live_watch_status(&self.channel_id).await {
            Ok(status) => {
                self.status = Some((Instant::now(), status.clone()));
                Some(status)
            }
            Err(e) => {
                tracing::debug!("시청 이벤트: 방송 상태 조회 실패 {e}");
                // 토큰이 조금 묵어도 웹처럼 마지막 값으로 보낸다.
                self.status.as_ref().map(|(_, s)| s.clone())
            }
        }
    }

    /// 밀린 이벤트를 순서대로 보낸다. 방송 상태나 라이브 ID 를 못 읽으면 다음 기회까지 남겨 둔다.
    async fn flush(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let Some(status) = self.fresh_status().await else {
            return;
        };
        let live_id = match self.live_id {
            Some(id) => id,
            None => match self.api.current_live_id(&self.channel_id).await {
                Ok(id) => *self.live_id.insert(id),
                Err(e) => {
                    tracing::debug!("시청 이벤트: 라이브 ID 조회 실패 {e}");
                    return;
                }
            },
        };
        while let Some(pending) = self.pending.pop_front() {
            // 방송이 끝났으면 집계할 시청이 없다(종료 알림은 남은 세션을 닫으려고 그대로 보낸다).
            if !status.is_open && pending.kind != WatchEventType::Ended {
                continue;
            }
            self.post(pending, live_id, &status).await;
        }
    }

    async fn post(&self, pending: PendingEvent, live_id: i64, status: &LiveWatchStatus) {
        let kind = pending.kind;
        let position_at = status
            .open_date
            .map(|d| (pending.wall - d).num_seconds().max(0))
            .unwrap_or(0);
        let event = LiveWatchEvent {
            channel_id: self.channel_id.clone(),
            live_id,
            event: kind,
            session_id: self.session_id.clone(),
            duration_sec: WATCH_EVENT_PERIOD.as_secs() as i64,
            position_at,
            accumulated_watch_sec: pending.awt,
            live_tokens: status.live_tokens.clone(),
        };
        // 시작·종료·실패는 진단 로그에서 확인할 수 있게 info, 30초 주기는 debug.
        // 전송 실패는 다시 보내지 않는다(서버가 받았는데 응답만 잃었으면 중복이 된다).
        match self.api.post_live_watch_event(&event).await {
            Ok(()) if kind == WatchEventType::Continued => tracing::debug!(
                "시청 이벤트 {} {} awt={}",
                kind.raw_value(),
                prefix8(&self.channel_id),
                event.accumulated_watch_sec
            ),
            Ok(()) => tracing::info!(
                "시청 이벤트 {} {} awt={}",
                kind.raw_value(),
                prefix8(&self.channel_id),
                event.accumulated_watch_sec
            ),
            Err(e) => tracing::info!("시청 이벤트 {} 실패: {e}", kind.raw_value()),
        }
    }
}

fn prefix8(s: &str) -> &str {
    s.char_indices().nth(8).map_or(s, |(i, _)| &s[..i])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracker_follows_web_event_order() {
        let t0 = Instant::now();
        let s = |n: u64| t0 + Duration::from_secs(n);
        let mut t = WatchEventTracker::default();
        assert_eq!(t.set_active(false, t0), None);
        assert_eq!(t.set_active(true, t0), Some(WatchEventType::Started));
        assert_eq!(
            t.set_active(true, s(1)),
            None,
            "같은 상태는 다시 보내지 않는다"
        );
        assert_eq!(t.tick(s(20)), None);
        assert_eq!(t.next_due(), Some(s(30)));
        assert_eq!(t.tick(s(29)), Some(WatchEventType::Continued));
        assert_eq!(t.accumulated_secs(s(29)), 29);
        assert_eq!(t.set_active(false, s(40)), Some(WatchEventType::Paused));
        assert_eq!(t.next_due(), None, "일시정지 중에는 주기가 없다");
        assert_eq!(t.tick(s(100)), None);
        assert_eq!(t.set_active(true, s(100)), Some(WatchEventType::Resumed));
        assert_eq!(t.accumulated_secs(s(100)), 40, "멈춘 시간은 세지 않는다");
        assert_eq!(t.next_due(), Some(s(130)), "재개하면 주기를 다시 센다");
        assert_eq!(t.end(s(110)), Some(WatchEventType::Ended));
        assert_eq!(t.accumulated_secs(s(200)), 50);
        assert_eq!(t.set_active(true, s(200)), None);
        assert_eq!(t.end(s(200)), None);
    }

    #[test]
    fn never_started_session_sends_nothing_on_end() {
        let mut t = WatchEventTracker::default();
        assert_eq!(t.end(Instant::now()), None);
        assert!(t.is_ended());
    }

    #[derive(Default)]
    struct FakeApi {
        events: Mutex<Vec<LiveWatchEvent>>,
        open: Mutex<bool>,
        /// 남은 상태 조회 실패 횟수.
        status_failures: Mutex<u32>,
        /// 상태 조회·전송 지연.
        delay: Mutex<Duration>,
    }

    impl FakeApi {
        fn opened() -> Arc<Self> {
            let api = Arc::new(Self::default());
            *api.open.lock().unwrap() = true;
            api
        }
        fn kinds(&self) -> Vec<(String, WatchEventType)> {
            self.events
                .lock()
                .unwrap()
                .iter()
                .map(|e| (e.channel_id.clone(), e.event))
                .collect()
        }
    }

    #[async_trait]
    impl WatchEventApi for FakeApi {
        async fn live_watch_status(&self, _: &str) -> Result<LiveWatchStatus, ApiError> {
            let delay = *self.delay.lock().unwrap();
            tokio::time::sleep(delay).await;
            {
                let mut failures = self.status_failures.lock().unwrap();
                if *failures > 0 {
                    *failures -= 1;
                    return Err(ApiError::InvalidResponse);
                }
            }
            Ok(LiveWatchStatus {
                is_open: *self.open.lock().unwrap(),
                open_date: Some(Utc::now() - chrono::Duration::seconds(600)),
                live_tokens: vec!["tok".into()],
            })
        }
        async fn current_live_id(&self, _: &str) -> Result<i64, ApiError> {
            Ok(99)
        }
        async fn post_live_watch_event(&self, event: &LiveWatchEvent) -> Result<(), ApiError> {
            let delay = *self.delay.lock().unwrap();
            tokio::time::sleep(delay).await;
            self.events.lock().unwrap().push(event.clone());
            Ok(())
        }
    }

    async fn settle() {
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn reporter_sends_heartbeats_only_while_active() {
        use WatchEventType::*;
        let api = FakeApi::opened();
        let reporter = LiveWatchReporter::new(api.clone());
        reporter.begin("viewer", "ch", Some(7));
        settle().await;
        assert!(api.kinds().is_empty(), "재생 전에는 보내지 않는다");

        reporter.set_active("viewer", true);
        settle().await;
        for step in [30, 30, 1] {
            tokio::time::advance(Duration::from_secs(step)).await;
            settle().await;
        }
        reporter.set_active("viewer", false);
        settle().await;
        tokio::time::advance(Duration::from_secs(120)).await;
        settle().await;
        assert_eq!(
            api.kinds().iter().map(|k| k.1).collect::<Vec<_>>(),
            vec![Started, Continued, Continued, Paused],
            "일시정지(백그라운드) 동안은 하트비트가 없다"
        );

        reporter.set_active("viewer", true);
        settle().await;
        reporter.end("viewer");
        settle().await;
        let events = api.events.lock().unwrap().clone();
        let kinds: Vec<_> = events.iter().map(|e| e.event).collect();
        assert_eq!(&kinds[4..], &[Resumed, Ended]);
        assert!(events
            .iter()
            .all(|e| e.live_id == 7 && e.live_tokens == ["tok"]));
        assert!(events.iter().all(|e| e.session_id == events[0].session_id));
        assert_eq!(events[3].accumulated_watch_sec, 61);
        assert!(events[0].position_at >= 599, "방송 시작부터 약 600초");
        assert_eq!(reporter.session_count(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn same_channel_on_two_screens_is_one_session() {
        use WatchEventType::*;
        let api = FakeApi::opened();
        let reporter = LiveWatchReporter::new(api.clone());
        reporter.begin("viewer", "ch", None);
        reporter.begin("pane-1", "ch", None);
        reporter.set_active("viewer", true);
        reporter.set_active("pane-1", true);
        settle().await;
        assert_eq!(reporter.session_count(), 1);
        reporter.set_active("viewer", false);
        settle().await;
        reporter.end("viewer");
        settle().await;
        assert_eq!(api.kinds(), vec![("ch".to_string(), Started)]);
        reporter.end("pane-1");
        settle().await;
        assert_eq!(
            api.kinds().iter().map(|k| k.1).collect::<Vec<_>>(),
            vec![Started, Ended]
        );
        assert!(api.events.lock().unwrap().iter().all(|e| e.live_id == 99));
    }

    #[tokio::test(start_paused = true)]
    async fn switching_channel_ends_previous_session() {
        use WatchEventType::*;
        let api = FakeApi::opened();
        let reporter = LiveWatchReporter::new(api.clone());
        reporter.begin("viewer", "a", Some(1));
        reporter.set_active("viewer", true);
        settle().await;
        reporter.begin("viewer", "b", Some(2));
        reporter.set_active("viewer", true);
        settle().await;
        reporter.end_all().await;
        let mut kinds = api.kinds();
        kinds.sort_by(|x, y| x.0.cmp(&y.0));
        assert_eq!(
            kinds,
            vec![
                ("a".to_string(), Started),
                ("a".to_string(), Ended),
                ("b".to_string(), Started),
                ("b".to_string(), Ended),
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn awt_uses_transition_time_not_processing_time() {
        let api = FakeApi::opened();
        *api.delay.lock().unwrap() = Duration::from_secs(5);
        let reporter = LiveWatchReporter::new(api.clone());
        reporter.begin("viewer", "ch", Some(1));
        reporter.set_active("viewer", true);
        settle().await;
        tokio::time::advance(Duration::from_secs(1)).await;
        reporter.set_active("viewer", false);
        // 시작 이벤트 전송(조회 5초 + 전송 5초)이 끝나야 일시정지를 처리한다.
        for _ in 0..4 {
            tokio::time::advance(Duration::from_secs(5)).await;
            settle().await;
        }
        let events = api.events.lock().unwrap().clone();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].event, WatchEventType::Paused);
        assert_eq!(
            events[1].accumulated_watch_sec, 1,
            "늦게 처리해도 1초만 봤다"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn start_event_survives_failed_first_status() {
        use WatchEventType::*;
        let api = FakeApi::opened();
        *api.status_failures.lock().unwrap() = 1;
        let reporter = LiveWatchReporter::new(api.clone());
        reporter.begin("viewer", "ch", Some(1));
        reporter.set_active("viewer", true);
        settle().await;
        assert!(api.kinds().is_empty());
        tokio::time::advance(Duration::from_secs(30)).await;
        settle().await;
        assert_eq!(
            api.kinds().iter().map(|k| k.1).collect::<Vec<_>>(),
            vec![Started, Continued],
            "다음 기회에 시작 이벤트를 먼저 보낸다"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn end_all_waits_for_sessions_already_ending() {
        use WatchEventType::*;
        let api = FakeApi::opened();
        let reporter = LiveWatchReporter::new(api.clone());
        reporter.begin("viewer", "ch", Some(1));
        reporter.set_active("viewer", true);
        settle().await;
        *api.delay.lock().unwrap() = Duration::from_millis(500);
        reporter.end("viewer");
        reporter.end_all().await;
        assert_eq!(
            api.kinds().iter().map(|k| k.1).collect::<Vec<_>>(),
            vec![Started, Ended]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn closed_live_is_not_counted() {
        let api = Arc::new(FakeApi::default());
        let reporter = LiveWatchReporter::new(api.clone());
        reporter.begin("viewer", "ch", Some(1));
        reporter.set_active("viewer", true);
        settle().await;
        tokio::time::advance(Duration::from_secs(31)).await;
        settle().await;
        assert!(api.kinds().is_empty());
    }
}
