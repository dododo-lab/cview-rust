//! `Sources/CViewApp/ViewModels/VODPlayerViewModel.swift` 포팅 — VOD 재생 뷰모델.
//!
//! 이어 보기 영속화는 원본과 같은 단일 직렬 writer([`VodProgressPersistenceWriter`])가 맡는다.
//! 같은 VOD 의 대기 스냅샷은 최신 값으로 합치고, 서로 다른 VOD 는 각자 슬롯을 지킨다.
//! 세대(generation)는 "같은 VOD 를 다시 재생" 과 "늦게 도착한 이전 재생의 콜백" 을 구별한다.
//!
//! 엔진은 [`VodPlayerEngine`] 트레이트로 받는다(macOS 의 `AVPlayerEngine` 자리). 엔진 콜백
//! (`onStateChange`/`onTimeChange`)은 셸이 [`VodPlayerViewModel::on_engine_phase`] /
//! [`VodPlayerViewModel::on_engine_time`] 으로 넘기고, 토큰으로 "지금 엔진" 인지 확인한다.
//!
//! 버린 것: 전체화면 토글의 `NSWindow` 호출(상태 플래그만 남김), PiP 표면 게이트,
//! 진행 막대 애니메이션 트랜잭션.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures_util::future::BoxFuture;
use serde::Serialize;
use tokio::sync::watch;
use url::Url;

use cview_core::models::player::PlayerPhase;
use cview_core::models::vod::{PlaybackSpeed, VodQuality, VodStreamInfo};
use cview_core::utils::time::{interval_since, Date, TimeInterval};
use cview_persistence::models::playback::PersistedVodPlaybackData;
use cview_persistence::DataStore;
use cview_player::vod::{VodDetailSource, VodStreamResolver};

use super::support::{cancel_slot, slot_is, Observed, ObservedGuard, TaskSlot};
use crate::services::support::{lock, system_clock, Clock, KeyValueStore};
use crate::services::vod_playback_progress::{VodPlaybackProgressStore, VodProgressDisposition};

// MARK: - 영속화 스냅샷 / writer

/// 이어 보기 영속화에 필요한 값 스냅샷.
#[derive(Debug, Clone, PartialEq)]
pub struct VodProgressPersistenceSnapshot {
    pub video_no: i64,
    pub generation: u64,
    pub position: TimeInterval,
    pub duration: TimeInterval,
    pub video_title: String,
    pub channel_name: String,
    pub thumbnail_url_string: Option<String>,
    pub playback_ended: bool,
}

impl VodProgressPersistenceSnapshot {
    pub fn disposition(&self) -> VodProgressDisposition {
        VodProgressDisposition::classify(self.position, self.duration, self.playback_ended)
    }
}

/// 실제 쓰기 함수.
pub type PersistFn =
    Arc<dyn Fn(VodProgressPersistenceSnapshot) -> BoxFuture<'static, ()> + Send + Sync>;

#[derive(Default)]
struct WriterQueue {
    pending_by_key: HashMap<(i64, u64), VodProgressPersistenceSnapshot>,
    pending_order: VecDeque<(i64, u64)>,
    newest_generation: HashMap<i64, u64>,
    terminal_generation: HashMap<i64, u64>,
    running: bool,
}

/// 단일 직렬 writer. 복제본은 같은 큐를 공유한다.
#[derive(Clone)]
pub struct VodProgressPersistenceWriter {
    queue: Arc<Mutex<WriterQueue>>,
    persist: PersistFn,
    mirror: Option<Arc<Mutex<VodPlaybackProgressStore>>>,
    busy: Arc<watch::Sender<bool>>,
}

impl VodProgressPersistenceWriter {
    pub fn new(mirror: Option<Arc<Mutex<VodPlaybackProgressStore>>>, persist: PersistFn) -> Self {
        let (busy, _) = watch::channel(false);
        Self {
            queue: Arc::new(Mutex::new(WriterQueue::default())),
            persist,
            mirror,
            busy: Arc::new(busy),
        }
    }

    /// `DataStore` 로 쓰는 writer. 쓰기는 블로킹 풀에서 돈다.
    pub fn with_data_store(
        data_store: Option<DataStore>,
        mirror: Option<Arc<Mutex<VodPlaybackProgressStore>>>,
    ) -> Self {
        let persist: PersistFn = Arc::new(move |snapshot: VodProgressPersistenceSnapshot| {
            let store = data_store.clone();
            Box::pin(async move {
                let Some(store) = store else { return };
                let result = tokio::task::spawn_blocking(move || match snapshot.disposition() {
                    VodProgressDisposition::Ignore => Ok(()),
                    VodProgressDisposition::Remove => store.remove_vod_playback(snapshot.video_no),
                    VodProgressDisposition::Record => store.record_vod_playback(
                        snapshot.video_no,
                        snapshot.position,
                        snapshot.duration,
                        &snapshot.video_title,
                        &snapshot.channel_name,
                        snapshot.thumbnail_url_string.as_deref(),
                    ),
                })
                .await;
                match result {
                    Ok(Err(e)) => tracing::error!("VOD playback 기록 실패: {e}"),
                    Err(e) => tracing::error!("VOD playback 기록 작업 실패: {e}"),
                    Ok(Ok(())) => {}
                }
            })
        });
        Self::new(mirror, persist)
    }

    pub fn enqueue(&self, snapshot: VodProgressPersistenceSnapshot) {
        let disposition = snapshot.disposition();
        if disposition == VodProgressDisposition::Ignore {
            return;
        }
        let video_no = snapshot.video_no;
        let generation = snapshot.generation;
        let start_worker = {
            let mut q = lock(&self.queue);
            let newest = q
                .newest_generation
                .get(&video_no)
                .copied()
                .unwrap_or(generation);
            if generation < newest {
                return;
            }
            if disposition == VodProgressDisposition::Record {
                if let Some(terminal) = q.terminal_generation.get(&video_no) {
                    if generation <= *terminal {
                        return;
                    }
                }
            }
            q.newest_generation.insert(video_no, generation);
            if disposition == VodProgressDisposition::Remove {
                let t = q
                    .terminal_generation
                    .get(&video_no)
                    .copied()
                    .unwrap_or(generation)
                    .max(generation);
                q.terminal_generation.insert(video_no, t);
            }
            if let Some(mirror) = &self.mirror {
                lock(mirror).apply(video_no, snapshot.position, snapshot.duration, disposition);
            }
            let key = (video_no, generation);
            if q.pending_by_key.insert(key, snapshot).is_none() {
                q.pending_order.push_back(key);
            }
            if q.running {
                false
            } else {
                q.running = true;
                true
            }
        };
        if start_worker {
            self.busy.send_replace(true);
            let me = self.clone();
            match tokio::runtime::Handle::try_current() {
                Ok(h) => {
                    h.spawn(async move { me.drain().await });
                }
                Err(_) => {
                    tracing::warn!("VOD progress writer: 런타임 없음 — 큐만 유지");
                    lock(&self.queue).running = false;
                    self.busy.send_replace(false);
                }
            }
        }
    }

    /// 현재 큐가 빌 때까지 기다린다.
    pub async fn flush(&self) {
        let mut rx = self.busy.subscribe();
        let _ = rx.wait_for(|b| !*b).await;
    }

    async fn drain(&self) {
        loop {
            let next = {
                let mut q = lock(&self.queue);
                loop {
                    match q.pending_order.pop_front() {
                        None => {
                            q.running = false;
                            break None;
                        }
                        Some(key) => {
                            if let Some(s) = q.pending_by_key.remove(&key) {
                                break Some(s);
                            }
                        }
                    }
                }
            };
            match next {
                Some(snapshot) => (self.persist)(snapshot).await,
                None => {
                    self.busy.send_replace(false);
                    return;
                }
            }
        }
    }
}

// MARK: - 엔진 / 해석기 경계

/// VOD 재생 엔진(macOS `AVPlayerEngine` 자리).
#[async_trait]
pub trait VodPlayerEngine: Send + Sync {
    /// VOD 전용 진입점 — `.m3u8` 을 라이브로 오감지하지 않는다.
    async fn play_vod(&self, url: &Url) -> Result<(), String>;
    fn pause(&self);
    fn resume(&self);
    fn seek(&self, to: TimeInterval);
    fn set_rate(&self, rate: f32);
    fn set_volume(&self, volume: f32);
    fn stop(&self);
}

pub type VodEngineFactory = Arc<dyn Fn() -> Arc<dyn VodPlayerEngine> + Send + Sync>;

/// VOD 스트림 해석.
#[async_trait]
pub trait VodStreamSource: Send + Sync {
    async fn resolve_vod(&self, video_no: i64) -> Result<VodStreamInfo, String>;
}

#[async_trait]
impl<S: VodDetailSource> VodStreamSource for VodStreamResolver<S> {
    async fn resolve_vod(&self, video_no: i64) -> Result<VodStreamInfo, String> {
        VodStreamResolver::resolve_vod(self, video_no)
            .await
            .map_err(|e| e.to_string())
    }
}

// MARK: - 상태

/// 직렬화 가능한 재생 상태.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", content = "message", rename_all = "camelCase")]
pub enum VodPlayback {
    Idle,
    Loading,
    Playing,
    Paused,
    Seeking,
    Buffering,
    Ended,
    Error(String),
}

pub struct VodPlayerState {
    pub playback_state: VodPlayback,
    pub current_time: TimeInterval,
    pub duration: TimeInterval,
    pub buffered_time: TimeInterval,
    pub playback_speed: PlaybackSpeed,
    pub volume: f32,
    pub is_muted: bool,
    pub is_fullscreen: bool,
    pub video_title: String,
    pub channel_name: String,
    pub channel_id: String,
    pub read_count: i64,
    pub publish_date: Option<Date>,
    pub thumbnail_url: Option<Url>,
    pub error_message: Option<String>,
    pub stream_info: Option<VodStreamInfo>,
    pub selected_quality_id: Option<String>,
    pub is_seeking: bool,
    pub resumed_from: Option<TimeInterval>,
    pub current_quality_id: Option<String>,
    // 내부
    previous_volume: f32,
    current_video_no: Option<i64>,
    generation: u64,
    last_recorded_at: Option<Date>,
    engine: Option<Arc<dyn VodPlayerEngine>>,
    engine_token: u64,
    seek_reset: Option<TaskSlot>,
}

impl Default for VodPlayerState {
    fn default() -> Self {
        Self {
            playback_state: VodPlayback::Idle,
            current_time: 0.0,
            duration: 0.0,
            buffered_time: 0.0,
            playback_speed: PlaybackSpeed::X100,
            volume: 1.0,
            is_muted: false,
            is_fullscreen: false,
            video_title: String::new(),
            channel_name: String::new(),
            channel_id: String::new(),
            read_count: 0,
            publish_date: None,
            thumbnail_url: None,
            error_message: None,
            stream_info: None,
            selected_quality_id: None,
            is_seeking: false,
            resumed_from: None,
            current_quality_id: None,
            previous_volume: 1.0,
            current_video_no: None,
            generation: 0,
            last_recorded_at: None,
            engine: None,
            engine_token: 0,
            seek_reset: None,
        }
    }
}

impl VodPlayerState {
    pub fn qualities(&self) -> &[VodQuality] {
        self.stream_info
            .as_ref()
            .map(|i| i.qualities.as_slice())
            .unwrap_or(&[])
    }

    /// 화질 메뉴 라벨 — 현재 트랙 이름 또는 "자동".
    pub fn current_quality_label(&self) -> String {
        self.current_quality_id
            .as_ref()
            .and_then(|id| self.qualities().iter().find(|q| &q.id == id))
            .map_or_else(|| "자동".into(), |q| q.name.clone())
    }

    /// 진행률 (0.0 ~ 1.0).
    pub fn progress(&self) -> f64 {
        if self.duration <= 0.0 {
            0.0
        } else {
            self.current_time / self.duration
        }
    }

    pub fn current_video_no(&self) -> Option<i64> {
        self.current_video_no
    }

    pub fn has_engine(&self) -> bool {
        self.engine.is_some()
    }

    pub fn engine_token(&self) -> u64 {
        self.engine_token
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VodQualityDto {
    pub id: String,
    pub name: String,
    pub resolution: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VodPlayerSnapshot {
    pub video_no: Option<i64>,
    pub playback: VodPlayback,
    pub current_time: f64,
    pub duration: f64,
    pub progress: f64,
    pub playback_speed: f32,
    pub volume: f32,
    pub is_muted: bool,
    pub is_fullscreen: bool,
    pub video_title: String,
    pub channel_name: String,
    pub channel_id: String,
    pub read_count: i64,
    pub publish_date: Option<Date>,
    pub thumbnail_url: Option<String>,
    pub error_message: Option<String>,
    pub qualities: Vec<VodQualityDto>,
    pub current_quality_id: Option<String>,
    pub current_quality_label: String,
    pub resumed_from: Option<f64>,
    pub is_seeking: bool,
}

// MARK: - 뷰모델

pub const LAST_SPEED_KEY: &str = "vod.lastSpeed";
/// 이어 보기 기록 cadence(초).
pub const RECORD_INTERVAL: TimeInterval = 15.0;

/// 저장된 재생 위치 조회.
pub trait VodPlaybackLookup: Send + Sync {
    fn fetch_vod_playback(&self, video_no: i64) -> Option<PersistedVodPlaybackData>;
}

impl VodPlaybackLookup for DataStore {
    fn fetch_vod_playback(&self, video_no: i64) -> Option<PersistedVodPlaybackData> {
        DataStore::fetch_vod_playback(self, video_no).ok().flatten()
    }
}

#[derive(Clone)]
pub struct VodPlayerViewModel {
    state: Arc<Observed<VodPlayerState>>,
    resolver: Arc<dyn VodStreamSource>,
    engine_factory: VodEngineFactory,
    lookup: Option<Arc<dyn VodPlaybackLookup>>,
    writer: VodProgressPersistenceWriter,
    defaults: Option<Arc<dyn KeyValueStore>>,
    clock: Clock,
}

impl VodPlayerViewModel {
    pub fn new(
        resolver: Arc<dyn VodStreamSource>,
        engine_factory: VodEngineFactory,
        lookup: Option<Arc<dyn VodPlaybackLookup>>,
        writer: VodProgressPersistenceWriter,
        defaults: Option<Arc<dyn KeyValueStore>>,
    ) -> Self {
        let mut state = VodPlayerState::default();
        if let Some(raw) = defaults.as_ref().and_then(|d| d.f64(LAST_SPEED_KEY)) {
            if let Some(speed) = PlaybackSpeed::ALL
                .into_iter()
                .find(|s| (s.raw_value() as f64 - raw).abs() < 1e-6)
            {
                state.playback_speed = speed;
            }
        }
        Self {
            state: Arc::new(Observed::new(state)),
            resolver,
            engine_factory,
            lookup,
            writer,
            defaults,
            clock: system_clock(),
        }
    }

    pub fn with_clock(mut self, clock: Clock) -> Self {
        self.clock = clock;
        self
    }

    pub fn read(&self) -> ObservedGuard<'_, VodPlayerState> {
        self.state.read()
    }

    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.state.subscribe()
    }

    pub fn writer(&self) -> &VodProgressPersistenceWriter {
        &self.writer
    }

    pub fn snapshot(&self) -> VodPlayerSnapshot {
        let s = self.state.read();
        VodPlayerSnapshot {
            video_no: s.current_video_no,
            playback: s.playback_state.clone(),
            current_time: s.current_time,
            duration: s.duration,
            progress: s.progress(),
            playback_speed: s.playback_speed.raw_value(),
            volume: s.volume,
            is_muted: s.is_muted,
            is_fullscreen: s.is_fullscreen,
            video_title: s.video_title.clone(),
            channel_name: s.channel_name.clone(),
            channel_id: s.channel_id.clone(),
            read_count: s.read_count,
            publish_date: s.publish_date,
            thumbnail_url: s.thumbnail_url.as_ref().map(Url::to_string),
            error_message: s.error_message.clone(),
            qualities: s
                .qualities()
                .iter()
                .map(|q| VodQualityDto {
                    id: q.id.clone(),
                    name: q.name.clone(),
                    resolution: q.resolution.clone(),
                })
                .collect(),
            current_quality_id: s.current_quality_id.clone(),
            current_quality_label: s.current_quality_label(),
            resumed_from: s.resumed_from,
            is_seeking: s.is_seeking,
        }
    }

    /// 현재 위치를 writer 에 반영한다. 30초 이하·95% 이상 규칙은 스냅샷이 판정한다.
    fn enqueue_progress(&self, s: &VodPlayerState, playback_ended: bool) {
        if self.lookup.is_none() {
            return;
        }
        let Some(video_no) = s.current_video_no else {
            return;
        };
        if s.duration <= 0.0 {
            return;
        }
        self.writer.enqueue(VodProgressPersistenceSnapshot {
            video_no,
            generation: s.generation,
            position: s.current_time,
            duration: s.duration,
            video_title: s.video_title.clone(),
            channel_name: s.channel_name.clone(),
            thumbnail_url_string: s
                .stream_info
                .as_ref()
                .and_then(|i| i.thumbnail_url_string.clone()),
            playback_ended,
        });
    }

    /// VOD 재생 시작.
    pub async fn start_vod(&self, video_no: i64) {
        {
            let mut s = self.state.lock();
            let ended = s.playback_state == VodPlayback::Ended;
            self.enqueue_progress(&s, ended);
            s.generation += 1;
            s.playback_state = VodPlayback::Loading;
            s.error_message = None;
            s.current_video_no = Some(video_no);
            s.resumed_from = None;
            s.current_quality_id = None;
            s.last_recorded_at = None;
        }

        // 저장 위치 조회와 스트림 해석을 겹친다.
        let lookup = self.lookup.clone();
        let saved = async move {
            let lookup = lookup?;
            tokio::task::spawn_blocking(move || lookup.fetch_vod_playback(video_no))
                .await
                .ok()
                .flatten()
        };
        let resolve = self.resolver.resolve_vod(video_no);
        let (info, saved) = futures_util::join!(resolve, saved);

        let info = match info {
            Ok(info) => info,
            Err(e) => {
                let mut s = self.state.lock();
                if s.current_video_no == Some(video_no) {
                    s.playback_state = VodPlayback::Error(e.clone());
                    s.error_message = Some(e.clone());
                }
                tracing::error!("VOD start failed: {e}");
                return;
            }
        };

        let engine = (self.engine_factory)();
        let (speed, volume, url) = {
            let mut s = self.state.lock();
            if s.current_video_no != Some(video_no) {
                return; // 해석 중 다른 VOD 로 넘어감
            }
            if let Some(old) = s.engine.take() {
                old.stop();
            }
            s.video_title = info.title.clone();
            s.channel_name = info.channel_name.clone();
            s.channel_id = info.channel_id.clone().unwrap_or_default();
            s.read_count = info.read_count;
            s.publish_date = info.publish_date;
            s.thumbnail_url = info.thumbnail_url.clone();
            s.duration = info.duration.as_secs_f64();
            s.selected_quality_id = info.qualities.first().map(|q| q.id.clone());
            let url = info.stream_url.clone();
            s.stream_info = Some(info);
            s.engine = Some(engine.clone());
            s.engine_token += 1;
            (s.playback_speed, s.volume, url)
        };

        if let Err(e) = engine.play_vod(&url).await {
            let mut s = self.state.lock();
            if s.current_video_no == Some(video_no) {
                s.playback_state = VodPlayback::Error(e.clone());
                s.error_message = Some(e);
            }
            return;
        }
        engine.set_rate(speed.raw_value());
        engine.set_volume(volume);

        let mut s = self.state.lock();
        if s.current_video_no != Some(video_no) {
            return;
        }
        // 이어 보기: 의미 있는 진행 구간(2초 초과, 95% 미만)이면 그 지점부터.
        if let Some(saved) = saved {
            if saved.last_position_sec > 2.0 && saved.duration_sec > 0.0 && saved.progress() < 0.95
            {
                engine.seek(saved.last_position_sec);
                s.current_time = saved.last_position_sec;
                s.resumed_from = Some(saved.last_position_sec);
            }
        }
        s.playback_state = VodPlayback::Playing;
        tracing::info!("VOD started: {}", s.video_title);
    }

    /// 엔진 상태 콜백. `token` 이 지금 엔진이 아니면 무시한다.
    pub fn on_engine_phase(&self, token: u64, phase: &PlayerPhase) {
        let mut s = self.state.lock();
        if s.engine.is_none() || s.engine_token != token {
            return;
        }
        s.playback_state = match phase {
            PlayerPhase::Playing => VodPlayback::Playing,
            PlayerPhase::Paused => VodPlayback::Paused,
            PlayerPhase::Buffering { .. } => VodPlayback::Buffering,
            PlayerPhase::Ended => {
                self.enqueue_progress(&s, true);
                VodPlayback::Ended
            }
            PlayerPhase::Error(_) => VodPlayback::Error("재생 오류가 발생했습니다".into()),
            PlayerPhase::Idle => VodPlayback::Idle,
            PlayerPhase::Loading => VodPlayback::Loading,
        };
    }

    /// 재생 위치 콜백 — 15초 cadence 로 이어 보기 위치를 합친다.
    pub fn on_engine_time(&self, token: u64, current: TimeInterval, total: TimeInterval) {
        let mut s = self.state.lock();
        if s.engine.is_none() || s.engine_token != token || s.is_seeking {
            return;
        }
        s.current_time = current;
        if total > 0.0 && s.duration != total {
            s.duration = total;
        }
        let now = (self.clock)();
        let due = s
            .last_recorded_at
            .is_none_or(|t| interval_since(&now, &t) >= RECORD_INTERVAL);
        if due {
            s.last_recorded_at = Some(now);
            self.enqueue_progress(&s, false);
        }
    }

    /// 재생/일시정지 토글.
    pub fn toggle_play_pause(&self) {
        let mut s = self.state.lock();
        let Some(engine) = s.engine.clone() else {
            return;
        };
        match s.playback_state {
            VodPlayback::Playing => {
                engine.pause();
                s.playback_state = VodPlayback::Paused;
                self.enqueue_progress(&s, false);
            }
            VodPlayback::Paused => {
                engine.resume();
                s.playback_state = VodPlayback::Playing;
            }
            VodPlayback::Ended => {
                s.generation += 1;
                engine.seek(0.0);
                engine.resume();
                s.playback_state = VodPlayback::Playing;
            }
            _ => {}
        }
    }

    /// 특정 위치로 이동. 500ms 뒤 seek 가드를 푼다(최신 seek 의 타이머만 유효).
    pub fn seek(&self, to: TimeInterval) {
        let mut s = self.state.lock();
        let Some(engine) = s.engine.clone() else {
            return;
        };
        s.is_seeking = true;
        s.current_time = to;
        engine.seek(to);
        cancel_slot(&mut s.seek_reset);
        let state = self.state.clone();
        s.seek_reset = Some(TaskSlot::spawn(move |id| async move {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let mut s = state.lock();
            if slot_is(&s.seek_reset, id) {
                s.is_seeking = false;
                s.seek_reset = None;
            }
        }));
    }

    pub fn seek_relative(&self, offset: TimeInterval) {
        let target = {
            let s = self.state.read();
            (s.current_time + offset).min(s.duration).max(0.0)
        };
        self.seek(target);
    }

    /// 재생 속도 변경 — 마지막 속도를 기억한다.
    pub fn set_speed(&self, speed: PlaybackSpeed) {
        let mut s = self.state.lock();
        s.playback_speed = speed;
        if let Some(e) = &s.engine {
            e.set_rate(speed.raw_value());
        }
        if let Some(d) = &self.defaults {
            d.set_f64(LAST_SPEED_KEY, speed.raw_value() as f64);
        }
    }

    /// 화질 전환 — 선택 트랙 URL(자동=마스터)로 교체하되 현재 위치를 보존한다.
    pub async fn select_quality(&self, quality_id: Option<&str>) {
        let (engine, url, resume_at, speed, volume, token) = {
            let mut s = self.state.lock();
            let (Some(engine), Some(info)) = (s.engine.clone(), s.stream_info.clone()) else {
                return;
            };
            if quality_id.map(str::to_string) == s.current_quality_id {
                return;
            }
            let quality = quality_id.and_then(|id| info.qualities.iter().find(|q| q.id == id));
            if quality_id.is_some() && quality.is_none() {
                return;
            }
            let url = quality.map_or_else(|| info.stream_url.clone(), |q| q.url.clone());
            s.current_quality_id = quality.map(|q| q.id.clone());
            s.playback_state = VodPlayback::Buffering;
            (
                engine,
                url,
                s.current_time,
                s.playback_speed,
                s.volume,
                s.engine_token,
            )
        };
        let result = engine.play_vod(&url).await;
        let mut s = self.state.lock();
        if s.engine_token != token {
            return;
        }
        match result {
            Ok(()) => {
                engine.set_rate(speed.raw_value());
                engine.set_volume(volume);
                if resume_at > 1.0 {
                    engine.seek(resume_at);
                    s.current_time = resume_at;
                }
                s.playback_state = VodPlayback::Playing;
            }
            Err(e) => {
                s.playback_state = VodPlayback::Error("화질 전환에 실패했습니다".into());
                tracing::error!("VOD quality switch failed: {e}");
            }
        }
    }

    pub fn set_volume(&self, volume: f32) {
        let clamped = volume.clamp(0.0, 1.0);
        let mut s = self.state.lock();
        s.volume = clamped;
        if let Some(e) = &s.engine {
            e.set_volume(clamped);
        }
        if clamped > 0.0 {
            s.is_muted = false;
        }
    }

    pub fn toggle_mute(&self) {
        let mut s = self.state.lock();
        if s.is_muted {
            s.is_muted = false;
            let v = s.previous_volume;
            if let Some(e) = &s.engine {
                e.set_volume(v);
            }
            s.volume = v;
        } else {
            s.previous_volume = s.volume;
            s.is_muted = true;
            if let Some(e) = &s.engine {
                e.set_volume(0.0);
            }
            s.volume = 0.0;
        }
    }

    pub fn adjust_volume(&self, delta: f32) {
        let v = self.state.read().volume;
        self.set_volume((v + delta).clamp(0.0, 1.0));
    }

    pub fn toggle_fullscreen(&self) {
        let mut s = self.state.lock();
        s.is_fullscreen = !s.is_fullscreen;
    }

    /// 정지 및 정리 — 상태 초기화 전에 최종 스냅샷을 enqueue 한다.
    pub fn stop(&self) {
        let mut s = self.state.lock();
        let ended = s.playback_state == VodPlayback::Ended;
        self.enqueue_progress(&s, ended);
        if let Some(e) = s.engine.take() {
            e.stop();
        }
        cancel_slot(&mut s.seek_reset);
        s.engine_token += 1;
        s.playback_state = VodPlayback::Idle;
        s.current_time = 0.0;
        s.duration = 0.0;
        s.stream_info = None;
        s.current_video_no = None;
        s.resumed_from = None;
        s.channel_id.clear();
        s.read_count = 0;
        s.publish_date = None;
        s.thumbnail_url = None;
        s.selected_quality_id = None;
        s.is_seeking = false;
    }

    /// "처음부터" — 이어 보기 복원 위치를 버리고 0초부터.
    pub fn restart_from_beginning(&self) {
        self.state.lock().resumed_from = None;
        self.seek(0.0);
    }

    pub fn dismiss_resume_hint(&self) {
        self.state.lock().resumed_from = None;
    }
}

/// 시간 포맷 — `h:mm:ss` 또는 `m:ss`.
pub fn format_time(seconds: TimeInterval) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "0:00".into();
    }
    let total = seconds as i64;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}
