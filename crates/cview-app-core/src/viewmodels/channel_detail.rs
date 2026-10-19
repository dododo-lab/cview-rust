//! 채널 상세 — 프로필·방송·VOD·클립을 부분별 TTL 로 갱신하고, 스냅샷으로 먼저 그린다.
//!
//! 원본: `Sources/CViewApp/ViewModels/ChannelDetailModel.swift`.
//! `ChannelDetailSnapshot`(`SnapshotDTOs+ChannelDetail.swift`)의 DTO 변환은 저장소 쪽 일이라
//! 여기서는 디코드된 값을 담는 [`ChannelDetailSnapshot`] 으로 둔다(목록 12개 상한·종료 방송 제외 규칙은 같다).
//! Swift 의 `Task.cancel()` 은 세대 비교로 대신한다 — 끝난 옛 요청 결과는 버린다.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::Serialize;

use cview_core::models::search::{ClipInfo, VodInfo};
use cview_core::models::{ChannelInfo, LiveInfo, LiveStatus};
use cview_core::utils::channel_list_merge::{append_skipping_duplicates, merge_first_page};
use cview_core::utils::time::{adding, interval_since, Date, TimeInterval};
use cview_networking::ChzzkApiClient;

use super::support::{Observed, ObservedGuard, TaskSlot};

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;
pub type Fetch<T> = Arc<dyn Fn() -> BoxFuture<Result<T, String>> + Send + Sync>;
pub type PageFetch<T> = Arc<dyn Fn(i64) -> BoxFuture<Result<Vec<T>, String>> + Send + Sync>;
pub type SnapshotLoad =
    Arc<dyn Fn(String) -> BoxFuture<Option<ChannelDetailSnapshot>> + Send + Sync>;
pub type SnapshotSave = Arc<dyn Fn(ChannelDetailSnapshot) -> BoxFuture<()> + Send + Sync>;

/// 첫 내용의 출처(`MenuContentOrigin`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MenuContentOrigin {
    Local,
    Network,
    Empty,
    Superseded,
}

/// 갱신 단위.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Part {
    Profile,
    Live,
    Vod,
    Clip,
}

impl Part {
    pub const ALL: [Part; 4] = [Part::Profile, Part::Live, Part::Vod, Part::Clip];

    /// 프로필 300초 · 방송 15초 · 목록 60초.
    pub fn ttl(self) -> TimeInterval {
        match self {
            Part::Profile => 300.0,
            Part::Live => 15.0,
            Part::Vod | Part::Clip => 60.0,
        }
    }
}

/// 헤더를 스켈레톤 없이 그리는 씨앗.
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelSeed {
    pub channel: ChannelInfo,
    pub live: Option<LiveInfo>,
}

impl ChannelSeed {
    pub fn new(channel: ChannelInfo, live: Option<LiveInfo>) -> Self {
        Self { channel, live }
    }
}

/// 채널 상세 스냅샷.
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelDetailSnapshot {
    pub channel_id: String,
    profile: Option<ChannelInfo>,
    live: Option<LiveInfo>,
    vods: Vec<VodInfo>,
    clips: Vec<ClipInfo>,
    pub profile_fetched_at: Option<Date>,
    pub lists_fetched_at: Option<Date>,
}

impl ChannelDetailSnapshot {
    pub const PAYLOAD_VERSION: i64 = 1;
    pub const LIST_LIMIT: usize = 12;

    pub fn new(
        channel_id: impl Into<String>,
        channel: Option<ChannelInfo>,
        live: Option<LiveInfo>,
        vods: Vec<VodInfo>,
        clips: Vec<ClipInfo>,
        profile_fetched_at: Option<Date>,
        lists_fetched_at: Option<Date>,
    ) -> Self {
        Self {
            channel_id: channel_id.into(),
            profile: channel,
            live: live.filter(|l| l.status == LiveStatus::Open),
            vods: vods.into_iter().take(Self::LIST_LIMIT).collect(),
            clips: clips.into_iter().take(Self::LIST_LIMIT).collect(),
            profile_fetched_at,
            lists_fetched_at,
        }
    }

    /// 다른 채널의 프로필이면 없는 것으로 본다.
    pub fn channel_info(&self) -> Option<ChannelInfo> {
        self.profile
            .clone()
            .filter(|p| p.channel_id == self.channel_id)
    }

    pub fn live_info(&self) -> Option<LiveInfo> {
        self.live.clone().map(|mut l| {
            if l.channel.is_none() {
                l.channel = self.channel_info();
            }
            l
        })
    }

    pub fn vod_infos(&self) -> Vec<VodInfo> {
        let channel = self.channel_info();
        self.vods
            .iter()
            .map(|v| {
                let mut v = v.clone();
                if v.channel.is_none() {
                    v.channel = channel.clone();
                }
                v
            })
            .collect()
    }

    pub fn clip_infos(&self) -> Vec<ClipInfo> {
        let channel = self.channel_info();
        self.clips
            .iter()
            .map(|c| {
                let mut c = c.clone();
                if c.channel.is_none() {
                    c.channel = channel.clone();
                }
                c
            })
            .collect()
    }
}

/// 부분별 첫 페이지 로더.
#[derive(Clone)]
pub struct ChannelDetailLoader {
    pub profile: Fetch<ChannelInfo>,
    pub live: Fetch<LiveInfo>,
    pub vod: Fetch<Vec<VodInfo>>,
    pub clip: Fetch<Vec<ClipInfo>>,
}

impl ChannelDetailLoader {
    /// API 클라이언트로 만든 기본 로더(첫 페이지 12개).
    pub fn api(client: Arc<ChzzkApiClient>, channel_id: &str) -> Self {
        let id = channel_id.to_string();
        let (c1, c2, c3, c4) = (client.clone(), client.clone(), client.clone(), client);
        let (i1, i2, i3, i4) = (id.clone(), id.clone(), id.clone(), id);
        Self {
            profile: Arc::new(move || {
                let (c, i) = (c1.clone(), i1.clone());
                Box::pin(async move { c.channel_info(&i).await.map_err(|e| e.to_string()) })
            }),
            live: Arc::new(move || {
                let (c, i) = (c2.clone(), i2.clone());
                Box::pin(async move { c.live_detail(&i, false).await.map_err(|e| e.to_string()) })
            }),
            vod: Arc::new(move || {
                let (c, i) = (c3.clone(), i3.clone());
                Box::pin(async move {
                    c.vod_list(&i, 0, 12, true)
                        .await
                        .map(|p| p.data)
                        .map_err(|e| e.to_string())
                })
            }),
            clip: Arc::new(move || {
                let (c, i) = (c4.clone(), i4.clone());
                Box::pin(async move {
                    c.clip_list(&i, 0, 12)
                        .await
                        .map(|p| p.data)
                        .map_err(|e| e.to_string())
                })
            }),
        }
    }
}

enum Fetched {
    Profile(ChannelInfo),
    Live(Box<LiveInfo>),
    Vod(Vec<VodInfo>),
    Clip(Vec<ClipInfo>),
}

pub struct ChannelDetailState {
    pub channel_id: String,
    pub channel_info: Option<ChannelInfo>,
    pub live_info: Option<LiveInfo>,
    vod_list: Vec<VodInfo>,
    clip_list: Vec<ClipInfo>,
    pub vod_page: i64,
    pub clip_page: i64,
    pub has_more_vods: bool,
    pub has_more_clips: bool,
    pub is_loading_more_vods: bool,
    pub is_loading_more_clips: bool,
    pub is_refreshing: bool,
    pub is_live_status_pending: bool,
    pub error_message: Option<String>,
    pub refresh_error: Option<String>,
    refresh_inflight: Option<tokio::sync::watch::Receiver<bool>>,
    updated: HashMap<Part, TimeInterval>,
    generation: u64,
    api: Option<Arc<ChzzkApiClient>>,
    succeeded: HashSet<Part>,
    profile_invalidations: u64,
    pub first_content_origin: Option<MenuContentOrigin>,
    pub hydration: Option<TaskSlot>,
    pub saves_scheduled: u64,
    pub last_save: Option<TaskSlot>,
}

impl ChannelDetailState {
    pub fn vod_list(&self) -> &[VodInfo] {
        &self.vod_list
    }
    pub fn clip_list(&self) -> &[ClipInfo] {
        &self.clip_list
    }
    pub fn set_vod_list(&mut self, v: Vec<VodInfo>) {
        self.vod_list = v;
    }
    pub fn set_clip_list(&mut self, v: Vec<ClipInfo>) {
        self.clip_list = v;
    }
    pub fn retained_media_count(&self) -> usize {
        self.vod_list.len() + self.clip_list.len()
    }
    pub fn is_loading(&self) -> bool {
        self.channel_info.is_none() && self.error_message.is_none()
    }

    fn seed(&mut self, channel: ChannelInfo, live: Option<LiveInfo>) {
        if self.channel_info.is_some() {
            return;
        }
        self.channel_info = Some(channel);
        if live.is_some() {
            self.is_live_status_pending = false;
        }
        self.live_info = live;
        if self.first_content_origin.is_none() {
            self.first_content_origin = Some(MenuContentOrigin::Local);
        }
    }

    fn apply_fetched(&mut self, fetched: Fetched) {
        match fetched {
            Fetched::Profile(v) => {
                self.channel_info = Some(v);
                if self.first_content_origin.is_none() {
                    self.first_content_origin = Some(MenuContentOrigin::Network);
                }
            }
            Fetched::Live(v) => {
                self.live_info = (v.status == LiveStatus::Open).then_some(*v);
                self.is_live_status_pending = false;
            }
            Fetched::Vod(v) => {
                let r = merge_first_page(
                    &self.vod_list,
                    &v,
                    ChannelDetailModel::LIST_PAGE_SIZE,
                    |x: &VodInfo| x.video_no,
                );
                if r.merged != self.vod_list {
                    self.vod_list = r.merged;
                }
                if r.replaced {
                    self.vod_page = 0;
                    self.has_more_vods = v.len() >= ChannelDetailModel::LIST_PAGE_SIZE;
                }
            }
            Fetched::Clip(v) => {
                let r = merge_first_page(
                    &self.clip_list,
                    &v,
                    ChannelDetailModel::LIST_PAGE_SIZE,
                    |x: &ClipInfo| x.clip_uid.clone(),
                );
                if r.merged != self.clip_list {
                    self.clip_list = r.merged;
                }
                if r.replaced {
                    self.clip_page = 0;
                    self.has_more_clips = v.len() >= ChannelDetailModel::LIST_PAGE_SIZE;
                }
            }
        }
    }
}

pub struct ChannelDetailShared {
    obs: Observed<ChannelDetailState>,
    now: Arc<dyn Fn() -> TimeInterval + Send + Sync>,
    wall_clock: Arc<dyn Fn() -> Date + Send + Sync>,
    snapshot_save: Option<SnapshotSave>,
}

#[derive(Clone)]
pub struct ChannelDetailModel {
    shared: Arc<ChannelDetailShared>,
}

impl ChannelDetailModel {
    pub const LIST_PAGE_SIZE: usize = 12;
    pub const LIST_PAGE_BATCH: i64 = 3;
    pub const REFRESH_FAILURE_MESSAGE: &'static str =
        "일부 정보를 갱신하지 못했습니다. 다시 시도해 주세요.";

    /// 기본 시계(단조 = 프로세스 시작 기준 초, 벽시계 = `Utc::now`).
    pub fn new(channel_id: impl Into<String>) -> Self {
        let start = std::time::Instant::now();
        Self::with_clocks(
            channel_id,
            Arc::new(move || start.elapsed().as_secs_f64()),
            Arc::new(chrono::Utc::now),
            None,
        )
    }

    pub fn with_clocks(
        channel_id: impl Into<String>,
        now: Arc<dyn Fn() -> TimeInterval + Send + Sync>,
        wall_clock: Arc<dyn Fn() -> Date + Send + Sync>,
        snapshot_save: Option<SnapshotSave>,
    ) -> Self {
        let state = ChannelDetailState {
            channel_id: channel_id.into(),
            channel_info: None,
            live_info: None,
            vod_list: Vec::new(),
            clip_list: Vec::new(),
            vod_page: 0,
            clip_page: 0,
            has_more_vods: true,
            has_more_clips: true,
            is_loading_more_vods: false,
            is_loading_more_clips: false,
            is_refreshing: false,
            is_live_status_pending: true,
            error_message: None,
            refresh_error: None,
            refresh_inflight: None,
            updated: HashMap::new(),
            generation: 0,
            api: None,
            succeeded: HashSet::new(),
            profile_invalidations: 0,
            first_content_origin: None,
            hydration: None,
            saves_scheduled: 0,
            last_save: None,
        };
        Self {
            shared: Arc::new(ChannelDetailShared {
                obs: Observed::new(state),
                now,
                wall_clock,
                snapshot_save,
            }),
        }
    }

    pub fn lock(&self) -> ObservedGuard<'_, ChannelDetailState> {
        self.shared.obs.lock()
    }
    pub fn read(&self) -> ObservedGuard<'_, ChannelDetailState> {
        self.shared.obs.read()
    }
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.shared.obs.subscribe()
    }
    pub fn snapshot(&self) -> ChannelDetailViewSnapshot {
        ChannelDetailViewSnapshot::from_state(&self.read())
    }

    pub fn adopt_hint(&self, hint: ChannelSeed) {
        self.lock().seed(hint.channel, hint.live);
    }

    pub async fn load_with_api(&self, client: Arc<ChzzkApiClient>, force: bool) {
        let id = {
            let mut s = self.lock();
            s.api = Some(client.clone());
            s.channel_id.clone()
        };
        self.load(force, ChannelDetailLoader::api(client, &id))
            .await;
    }

    /// 기한이 지난 부분만 동시에 받는다. 진행 중이면 그 완료를 기다린다.
    pub async fn load(&self, force: bool, loader: ChannelDetailLoader) {
        let inflight = self.read().refresh_inflight.clone();
        if let Some(mut rx) = inflight {
            let _ = rx.wait_for(|d| *d).await;
            return;
        }
        let now = (self.shared.now)();
        let (token, due, tx) = {
            let mut s = self.lock();
            let due: Vec<Part> = Part::ALL
                .into_iter()
                .filter(|part| {
                    if (*part == Part::Vod && s.is_loading_more_vods)
                        || (*part == Part::Clip && s.is_loading_more_clips)
                    {
                        return false;
                    }
                    force || s.updated.get(part).is_none_or(|u| now - u >= part.ttl())
                })
                .collect();
            if due.is_empty() {
                return;
            }
            s.is_refreshing = true;
            s.error_message = None;
            s.refresh_error = None;
            s.succeeded.clear();
            let (tx, rx) = tokio::sync::watch::channel(false);
            s.refresh_inflight = Some(rx);
            (s.generation, due, tx)
        };
        futures_util::join!(
            self.refresh_part(Part::Profile, &due, &loader, token),
            self.refresh_part(Part::Live, &due, &loader, token),
            self.refresh_part(Part::Vod, &due, &loader, token),
            self.refresh_part(Part::Clip, &due, &loader, token),
        );
        let _ = tx.send(true);
        let mut s = self.lock();
        if s.generation != token {
            return;
        }
        s.refresh_inflight = None;
        s.is_refreshing = false;
        self.persist_snapshot(&mut s);
    }

    async fn refresh_part(
        &self,
        part: Part,
        due: &[Part],
        loader: &ChannelDetailLoader,
        token: u64,
    ) {
        let invalidations = {
            let s = self.read();
            if !due.contains(&part) || s.generation != token {
                return;
            }
            s.profile_invalidations
        };
        let fetched = match part {
            Part::Profile => (loader.profile)().await.map(Fetched::Profile),
            Part::Live => (loader.live)().await.map(|l| Fetched::Live(Box::new(l))),
            Part::Vod => (loader.vod)().await.map(Fetched::Vod),
            Part::Clip => (loader.clip)().await.map(Fetched::Clip),
        };
        let now = (self.shared.now)();
        let mut s = self.lock();
        if s.generation != token {
            return;
        }
        match fetched {
            Ok(f) => {
                s.apply_fetched(f);
                if part == Part::Profile && invalidations != s.profile_invalidations {
                    return;
                }
                s.updated.insert(part, now);
                s.succeeded.insert(part);
            }
            Err(e) => {
                if part == Part::Profile && s.channel_info.is_none() {
                    s.error_message = Some(e);
                } else {
                    s.refresh_error = Some(Self::REFRESH_FAILURE_MESSAGE.into());
                }
            }
        }
    }

    /// 로그아웃·채널 전환 — 진행 중 결과를 모두 버리고 처음 상태로.
    pub fn invalidate(&self) {
        let mut s = self.lock();
        s.generation = s.generation.wrapping_add(1);
        s.refresh_inflight = None;
        if let Some(h) = s.hydration.take() {
            h.cancel();
        }
        s.updated.clear();
        s.succeeded.clear();
        s.first_content_origin = None;
        s.api = None;
        s.vod_page = 0;
        s.clip_page = 0;
        s.has_more_vods = true;
        s.has_more_clips = true;
        s.is_live_status_pending = true;
        s.channel_info = None;
        s.live_info = None;
        s.vod_list = Vec::new();
        s.clip_list = Vec::new();
        s.error_message = None;
        s.refresh_error = None;
        s.is_refreshing = false;
        s.is_loading_more_vods = false;
        s.is_loading_more_clips = false;
    }

    fn api_page_fetch<T: Send + 'static>(
        &self,
        f: impl Fn(Arc<ChzzkApiClient>, String, i64) -> BoxFuture<Result<Vec<T>, String>>
            + Send
            + Sync
            + 'static,
    ) -> Option<PageFetch<T>> {
        let s = self.read();
        let client = s.api.clone()?;
        let id = s.channel_id.clone();
        Some(Arc::new(move |page| f(client.clone(), id.clone(), page)))
    }

    /// 다음 VOD 3페이지를 동시에 받아 순서대로 붙인다. 중간이 비거나 짧으면 거기서 멈춘다.
    pub async fn load_more_vods(&self, fetch_page: Option<PageFetch<VodInfo>>) {
        let loader = fetch_page.or_else(|| {
            self.api_page_fetch(|c, id, page| {
                Box::pin(async move {
                    c.vod_list(&id, page, Self::LIST_PAGE_SIZE as i64, true)
                        .await
                        .map(|p| p.data)
                        .map_err(|e| e.to_string())
                })
            })
        });
        let (token, start_page, loader) = {
            let mut s = self.lock();
            let Some(loader) = loader else { return };
            if !s.updated.contains_key(&Part::Vod)
                || !s.has_more_vods
                || s.is_refreshing
                || s.is_loading_more_vods
            {
                return;
            }
            s.is_loading_more_vods = true;
            (s.generation, s.vod_page + 1, loader)
        };
        let pages = fetch_batch(&loader, start_page).await;
        let mut s = self.lock();
        if token != s.generation {
            return;
        }
        s.is_loading_more_vods = false;
        match pages {
            Ok(pages) => {
                let mut received = Vec::new();
                for (offset, data) in pages.into_iter().enumerate() {
                    if data.is_empty() {
                        s.has_more_vods = false;
                        break;
                    }
                    let short = data.len() < Self::LIST_PAGE_SIZE;
                    received.extend(data);
                    s.vod_page = start_page + offset as i64;
                    if short {
                        s.has_more_vods = false;
                        break;
                    }
                }
                let merged = append_skipping_duplicates(&s.vod_list, &received, |v| v.video_no);
                if merged.appended > 0 {
                    s.vod_list = merged.merged;
                }
            }
            Err(e) => tracing::warn!("VOD 목록 로드 실패: {e}"),
        }
    }

    pub async fn load_more_clips(&self, fetch_page: Option<PageFetch<ClipInfo>>) {
        let loader = fetch_page.or_else(|| {
            self.api_page_fetch(|c, id, page| {
                Box::pin(async move {
                    c.clip_list(&id, page, Self::LIST_PAGE_SIZE as i64)
                        .await
                        .map(|p| p.data)
                        .map_err(|e| e.to_string())
                })
            })
        });
        let (token, start_page, loader) = {
            let mut s = self.lock();
            let Some(loader) = loader else { return };
            if !s.updated.contains_key(&Part::Clip)
                || !s.has_more_clips
                || s.is_refreshing
                || s.is_loading_more_clips
            {
                return;
            }
            s.is_loading_more_clips = true;
            (s.generation, s.clip_page + 1, loader)
        };
        let pages = fetch_batch(&loader, start_page).await;
        let mut s = self.lock();
        if token != s.generation {
            return;
        }
        s.is_loading_more_clips = false;
        match pages {
            Ok(pages) => {
                let mut received = Vec::new();
                for (offset, data) in pages.into_iter().enumerate() {
                    if data.is_empty() {
                        s.has_more_clips = false;
                        break;
                    }
                    let short = data.len() < Self::LIST_PAGE_SIZE;
                    received.extend(data);
                    s.clip_page = start_page + offset as i64;
                    if short {
                        s.has_more_clips = false;
                        break;
                    }
                }
                let merged =
                    append_skipping_duplicates(&s.clip_list, &received, |c| c.clip_uid.clone());
                if merged.appended > 0 {
                    s.clip_list = merged.merged;
                }
            }
            Err(e) => tracing::warn!("클립 목록 로드 실패: {e}"),
        }
    }

    // MARK: 스냅샷

    /// 백그라운드 hydrate — 시작 시점의 세대를 잡는다.
    pub fn begin_hydration(&self, load: SnapshotLoad) {
        let mut s = self.lock();
        if let Some(h) = s.hydration.take() {
            h.cancel();
        }
        let token = s.generation;
        let invalidations = s.profile_invalidations;
        let vm = self.clone();
        s.hydration = Some(TaskSlot::spawn(move |id| async move {
            vm.hydrate_with(load, token, invalidations).await;
            let mut s = vm.lock();
            if s.hydration.as_ref().is_some_and(|h| h.id() == id) {
                s.hydration = None;
            }
        }));
    }

    pub async fn hydrate(&self, load: SnapshotLoad) {
        let (token, invalidations) = {
            let s = self.read();
            (s.generation, s.profile_invalidations)
        };
        self.hydrate_with(load, token, invalidations).await;
    }

    async fn hydrate_with(&self, load: SnapshotLoad, token: u64, invalidations: u64) {
        let id = self.read().channel_id.clone();
        let Some(snapshot) = load(id).await else {
            return;
        };
        let mono = (self.shared.now)();
        let wall = (self.shared.wall_clock)();
        let mut s = self.lock();
        if s.generation != token || snapshot.channel_id != s.channel_id {
            return;
        }
        let profile_still_valid = invalidations == s.profile_invalidations;
        apply_snapshot(&mut s, &snapshot, profile_still_valid, mono, wall);
    }

    /// 프로필만 낡았다고 표시한다(화면 내용은 그대로).
    pub fn invalidate_profile(&self) {
        let mut s = self.lock();
        s.profile_invalidations = s.profile_invalidations.wrapping_add(1);
        s.updated.remove(&Part::Profile);
    }

    fn persist_snapshot(&self, s: &mut ChannelDetailState) {
        let succeeded = std::mem::take(&mut s.succeeded);
        let Some(save) = self.shared.snapshot_save.clone() else {
            return;
        };
        if succeeded.is_empty() {
            return;
        }
        let mono = (self.shared.now)();
        let wall = (self.shared.wall_clock)();
        let fetched_at = |part: Part| s.updated.get(&part).map(|u| adding(&wall, -(mono - u)));
        let lists = [fetched_at(Part::Vod), fetched_at(Part::Clip)];
        let lists_fetched_at = if lists.iter().any(|l| l.is_none()) {
            None
        } else {
            lists.iter().flatten().min().copied()
        };
        let snapshot = ChannelDetailSnapshot::new(
            s.channel_id.clone(),
            s.channel_info.clone(),
            if s.updated.contains_key(&Part::Live) {
                s.live_info.clone()
            } else {
                None
            },
            s.vod_list.clone(),
            s.clip_list.clone(),
            fetched_at(Part::Profile),
            lists_fetched_at,
        );
        s.saves_scheduled += 1;
        s.last_save = Some(TaskSlot::spawn(
            move |_| async move { save(snapshot).await },
        ));
    }
}

async fn fetch_batch<T: Send + 'static>(
    loader: &PageFetch<T>,
    start_page: i64,
) -> Result<Vec<Vec<T>>, String> {
    let futures: Vec<_> = (0..ChannelDetailModel::LIST_PAGE_BATCH)
        .map(|offset| loader(start_page + offset))
        .collect();
    futures_util::future::try_join_all(futures).await
}

fn apply_snapshot(
    s: &mut ChannelDetailState,
    snapshot: &ChannelDetailSnapshot,
    profile_still_valid: bool,
    mono: TimeInterval,
    wall: Date,
) {
    let stamp =
        |updated: &mut HashMap<Part, TimeInterval>, part: Part, fetched_at: Option<Date>| {
            if updated.contains_key(&part) {
                return;
            }
            let Some(fetched_at) = fetched_at else { return };
            let age = interval_since(&wall, &fetched_at);
            if !age.is_finite() || age < 0.0 {
                return;
            }
            updated.insert(part, mono - age);
        };
    if profile_still_valid && !s.updated.contains_key(&Part::Profile) {
        if let Some(channel) = snapshot.channel_info() {
            s.channel_info = Some(channel);
            if s.error_message.is_some() {
                s.error_message = None;
                s.refresh_error = Some(ChannelDetailModel::REFRESH_FAILURE_MESSAGE.into());
            }
            stamp(&mut s.updated, Part::Profile, snapshot.profile_fetched_at);
            if s.first_content_origin.is_none() {
                s.first_content_origin = Some(MenuContentOrigin::Local);
            }
        }
    }
    if s.vod_list.is_empty() && !s.updated.contains_key(&Part::Vod) {
        s.vod_list = snapshot.vod_infos();
        s.vod_page = 0;
        s.has_more_vods = s.vod_list.len() >= ChannelDetailModel::LIST_PAGE_SIZE;
        stamp(&mut s.updated, Part::Vod, snapshot.lists_fetched_at);
    }
    if s.clip_list.is_empty() && !s.updated.contains_key(&Part::Clip) {
        s.clip_list = snapshot.clip_infos();
        s.clip_page = 0;
        s.has_more_clips = s.clip_list.len() >= ChannelDetailModel::LIST_PAGE_SIZE;
        stamp(&mut s.updated, Part::Clip, snapshot.lists_fetched_at);
    }
    if s.live_info.is_none() && s.is_live_status_pending && !s.updated.contains_key(&Part::Live) {
        if let Some(live) = snapshot.live_info() {
            s.live_info = Some(live);
        }
    }
}

/// 직렬화 스냅샷.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelDetailViewSnapshot {
    pub channel_id: String,
    pub channel_info: Option<ChannelInfo>,
    pub live_info: Option<LiveInfo>,
    pub vod_list: Vec<VodInfo>,
    pub clip_list: Vec<ClipInfo>,
    pub has_more_vods: bool,
    pub has_more_clips: bool,
    pub is_loading_more_vods: bool,
    pub is_loading_more_clips: bool,
    pub is_loading: bool,
    pub is_refreshing: bool,
    pub is_live_status_pending: bool,
    pub error_message: Option<String>,
    pub refresh_error: Option<String>,
}

impl ChannelDetailViewSnapshot {
    pub fn from_state(s: &ChannelDetailState) -> Self {
        Self {
            channel_id: s.channel_id.clone(),
            channel_info: s.channel_info.clone(),
            live_info: s.live_info.clone(),
            vod_list: s.vod_list.clone(),
            clip_list: s.clip_list.clone(),
            has_more_vods: s.has_more_vods,
            has_more_clips: s.has_more_clips,
            is_loading_more_vods: s.is_loading_more_vods,
            is_loading_more_clips: s.is_loading_more_clips,
            is_loading: s.is_loading(),
            is_refreshing: s.is_refreshing,
            is_live_status_pending: s.is_live_status_pending,
            error_message: s.error_message.clone(),
            refresh_error: s.refresh_error.clone(),
        }
    }
}
