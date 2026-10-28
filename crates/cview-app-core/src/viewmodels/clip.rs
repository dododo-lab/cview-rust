//! 클립 재생·클립 메뉴 뷰모델.
//!
//! 원본:
//! - `Sources/CViewApp/ViewModels/ClipPlayerViewModel.swift`
//! - `Sources/CViewApp/ViewModels/ClipBrowserViewModel.swift` (`ClipHubPageLifecycleEffects` 포함)
//! - 보조 타입: `Sources/CViewApp/Views/Clips/ClipBrowserSupport.swift`
//!   (`ClipBrowser.*` 열거형, `ChannelResolver`), `Views/Clips/ClipHeroComponents.swift`
//!   (`ClipMood` 키워드 매칭).
//!
//! 버린 것: WebKit 쿠키 동기화(`syncWebKitCookies` — 셸이 inkey 전에 맡는다), `NSWorkspace`
//! 열기·클립보드 복사(셸 몫 — 여기서는 링크 문자열만 만든다), `QuietDataCommit` 애니메이션 억제.
//! `UserDefaults` 는 [`KeyValueStore`] 문자열 값에 JSON 으로 넣는다.
//!
//! 클립 재생 뷰모델은 `player`, 클립 브라우저의 탭별 동작은 `trending` · `channel` · `library`
//! 하위 모듈에 있다.

mod player;
mod trending;

pub use player::{ClipPlayerSnapshot, ClipPlayerState, ClipPlayerViewModel};
mod channel;
mod library;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::Serialize;
use url::Url;

use cview_core::models::clip::ClipDetail;
use cview_core::models::search::ClipInfo;
use cview_core::models::vod::VodPlaybackState;
use cview_core::models::ChannelInfo;
use cview_networking::{ChzzkApiClient, PagedContent};
use cview_persistence::models::cached_clip::{compute_content_hash, CachedClipData};
use cview_persistence::DataStore;

use super::support::{Observed, ObservedGuard};
use crate::services::support::KeyValueStore;

// MARK: - ClipBrowser 열거형

/// 클립 메뉴 탭.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ClipTab {
    #[default]
    Trending,
    Channel,
}

impl ClipTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::Trending => "전체 인기클립",
            Self::Channel => "채널별 클립",
        }
    }
}

/// 채널 클립 정렬.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ClipSortOrder {
    #[default]
    Popular,
    Recent,
}

impl ClipSortOrder {
    /// Swift `rawValue`.
    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Popular => "인기순",
            Self::Recent => "최신순",
        }
    }
}

/// 인기 클립 기간.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TrendingFilter {
    Today,
    #[default]
    Week,
    Month,
}

impl TrendingFilter {
    pub fn label(self) -> &'static str {
        match self {
            Self::Today => "오늘",
            Self::Week => "이번 주",
            Self::Month => "이번 달",
        }
    }
    pub fn api_value(self) -> &'static str {
        match self {
            Self::Today => "WITHIN_1_DAY",
            Self::Week => "WITHIN_7_DAYS",
            Self::Month => "WITHIN_30_DAYS",
        }
    }
}

/// 인기 클립 정렬.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TrendingOrder {
    #[default]
    Popular,
    Recommend,
}

impl TrendingOrder {
    pub fn label(self) -> &'static str {
        match self {
            Self::Popular => "인기순",
            Self::Recommend => "추천순",
        }
    }
    pub fn api_value(self) -> &'static str {
        match self {
            Self::Popular => "POPULAR",
            Self::Recommend => "RECOMMEND",
        }
    }
}

/// 격자/목록.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ClipViewMode {
    #[default]
    Grid,
    List,
}

// MARK: - ChannelResolver

/// 사용자가 입력한 문자열에서 채널 식별자를 뽑아낸다 (`ChannelResolver`).
pub mod channel_resolver {
    use url::Url;

    /// 입력값을 trim 하고 채널 ID 를 추출한다. 채널명 등은 `None` — 호출부가 이름 검색으로 폴백.
    pub fn resolve_channel_id(raw: &str) -> Option<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        if let Some(id) = parse_channel_id_from_url(trimmed) {
            return Some(id);
        }
        if looks_like_channel_id(trimmed) {
            return Some(trimmed.to_string());
        }
        None
    }

    /// URL 에서 channelId 를 추출한다 (chzzk.naver.com path 첫 segment).
    pub fn parse_channel_id_from_url(raw: &str) -> Option<String> {
        let lower = raw.to_lowercase();
        if !raw.contains('/') && !lower.starts_with("http") {
            return None;
        }
        let normalized = if lower.starts_with("http://") || lower.starts_with("https://") {
            raw.to_string()
        } else {
            format!("https://{raw}")
        };
        let url = Url::parse(&normalized).ok()?;
        let host = url.host_str()?.to_lowercase();
        if !host.contains("chzzk.naver.com") {
            return None;
        }
        let first = url.path_segments()?.find(|s| !s.is_empty())?.to_string();
        if first == "clips" || first == "live" || first == "video" {
            return None;
        }
        looks_like_channel_id(&first).then_some(first)
    }

    /// 채널 ID 후보 형태인지 — ASCII hex 16~64자.
    pub fn looks_like_channel_id(s: &str) -> bool {
        let count = s.chars().count();
        if !(16..=64).contains(&count) {
            return false;
        }
        s.chars().all(|c| c.is_ascii_hexdigit())
    }
}

// MARK: - ClipMood

/// mood chip 정의 (`ClipMood`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClipMood {
    pub id: String,
    pub label: String,
    pub emoji: String,
    pub count: usize,
}

impl ClipMood {
    fn new(id: &str, label: &str, emoji: &str, count: usize) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            emoji: emoji.into(),
            count,
        }
    }

    /// 정적 mood 8개.
    pub fn default_static_moods(total_count: usize) -> Vec<ClipMood> {
        vec![
            Self::new("all", "전체 인기", "🔥", total_count),
            Self::new("funny", "웃긴 순간", "😂", 0),
            Self::new("highlight", "하이라이트", "🎯", 0),
            Self::new("music", "노래/음악", "🎤", 0),
            Self::new("talk", "저챗", "💬", 0),
            Self::new("valorant", "발로란트", "🎮", 0),
            Self::new("lol", "롤", "⚔️", 0),
            Self::new("cook", "먹방/쿠킹", "🍳", 0),
        ]
    }

    /// mood id → 제목/카테고리 매칭 키워드. `all`·미지 id 는 필터 없음.
    pub fn keywords(mood_id: &str) -> Option<&'static [&'static str]> {
        Some(match mood_id {
            "funny" => &[
                "ㅋㅋ",
                "ㅋ",
                "웃긴",
                "웃음",
                "레전드",
                "꿀잼",
                "개웃",
                "fun",
                "lol",
                "lmao",
            ],
            "highlight" => &[
                "하이라이트",
                "highlight",
                "베스트",
                "best",
                "압권",
                "정수",
                "명장면",
            ],
            "music" => &[
                "노래",
                "음악",
                "music",
                "보컬",
                "가사",
                "립싱",
                "라이브 cover",
                "송",
                "곡",
            ],
            "talk" => &["저챗", "talk", "토크", "수다", "대화", "썰", "이야기"],
            "valorant" => &["valorant", "발로란트", "발로", "에이스", "클러치"],
            "lol" => &["league of legends", "리그오브", "롤 ", "lol "],
            "cook" => &["먹방", "쿠킹", "요리", "맛집", "음식", "쿡방", "레시피"],
            _ => return None,
        })
    }

    /// 클립이 mood 와 맞는지. `None`/"all" 이면 항상 참.
    pub fn matches(clip_title: &str, category: Option<&str>, mood_id: Option<&str>) -> bool {
        let Some(keywords) = mood_id.and_then(Self::keywords) else {
            return true;
        };
        if keywords.is_empty() {
            return true;
        }
        let haystack = format!("{} {}", clip_title, category.unwrap_or("")).to_lowercase();
        keywords
            .iter()
            .any(|k| haystack.contains(&k.to_lowercase()))
    }
}

// MARK: - Hub Page Lifecycle

/// 허브가 방문한 탭을 상주시킬 때 클립 탭의 부수효과를 활성 탭에만 한정한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ClipHubPageLifecycleEffects {
    pub maintains_auto_refresh: bool,
    pub dismisses_playback: bool,
}

impl ClipHubPageLifecycleEffects {
    pub fn resolve(is_active: bool) -> Self {
        Self {
            maintains_auto_refresh: is_active,
            dismisses_playback: !is_active,
        }
    }
}

// MARK: - 협력자

/// 클립 관련 API 면.
#[async_trait]
pub trait ClipApi: Send + Sync {
    async fn home_popular_clips(
        &self,
        filter_type: &str,
        order_type: &str,
    ) -> Result<Vec<ClipInfo>, String>;
    async fn clip_list(
        &self,
        channel_id: &str,
        page: i64,
        size: i64,
    ) -> Result<PagedContent<ClipInfo>, String>;
    async fn search_channels(
        &self,
        keyword: &str,
        offset: i64,
        size: i64,
    ) -> Result<Vec<ChannelInfo>, String>;
    async fn clip_detail(&self, clip_uid: &str) -> Result<ClipDetail, String>;
    async fn clip_stream_url(&self, clip_uid: &str, video_id: &str) -> Result<Url, String>;
}

#[async_trait]
impl ClipApi for ChzzkApiClient {
    async fn home_popular_clips(
        &self,
        filter_type: &str,
        order_type: &str,
    ) -> Result<Vec<ClipInfo>, String> {
        ChzzkApiClient::home_popular_clips(self, filter_type, order_type)
            .await
            .map_err(|e| e.to_string())
    }
    async fn clip_list(
        &self,
        channel_id: &str,
        page: i64,
        size: i64,
    ) -> Result<PagedContent<ClipInfo>, String> {
        ChzzkApiClient::clip_list(self, channel_id, page, size)
            .await
            .map_err(|e| e.to_string())
    }
    async fn search_channels(
        &self,
        keyword: &str,
        offset: i64,
        size: i64,
    ) -> Result<Vec<ChannelInfo>, String> {
        ChzzkApiClient::search_channels(self, keyword, offset, size)
            .await
            .map(|r| r.data)
            .map_err(|e| e.to_string())
    }
    async fn clip_detail(&self, clip_uid: &str) -> Result<ClipDetail, String> {
        ChzzkApiClient::clip_detail(self, clip_uid)
            .await
            .map_err(|e| e.to_string())
    }
    async fn clip_stream_url(&self, clip_uid: &str, video_id: &str) -> Result<Url, String> {
        ChzzkApiClient::clip_stream_url(self, clip_uid, video_id)
            .await
            .map_err(|e| e.to_string())
    }
}

/// 클립 DB 캐시 면 (`DataStore+ClipCache`).
pub trait ClipCacheStore: Send + Sync {
    fn fetch_cached_clips(&self, source: &str, limit: usize)
        -> Result<Vec<CachedClipData>, String>;
    /// upsert 후 이번 묶음에 없는 행의 source 태그를 뗀다. 바뀐 행 수.
    fn upsert_and_retain(
        &self,
        data: &[CachedClipData],
        source: &str,
        keep: &HashSet<String>,
    ) -> Result<usize, String>;
}

impl ClipCacheStore for DataStore {
    fn fetch_cached_clips(
        &self,
        source: &str,
        limit: usize,
    ) -> Result<Vec<CachedClipData>, String> {
        DataStore::fetch_cached_clips(self, source, limit).map_err(|e| e.to_string())
    }
    fn upsert_and_retain(
        &self,
        data: &[CachedClipData],
        source: &str,
        keep: &HashSet<String>,
    ) -> Result<usize, String> {
        let n = self.upsert_cached_clips(data).map_err(|e| e.to_string())?;
        self.retain_cached_clip_source(source, keep)
            .map_err(|e| e.to_string())?;
        Ok(n)
    }
}

pub use crate::services::refresh_coordinator::RefreshDemandSink;

// MARK: - VOD 재생 상태 직렬화 보조

/// `VODPlaybackState` 직렬화 표현.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", content = "detail", rename_all = "camelCase")]
pub enum PlaybackStateSnapshot {
    Idle,
    Loading,
    Playing,
    Paused,
    Seeking,
    Buffering,
    Ended,
    Error(String),
}

impl From<&VodPlaybackState> for PlaybackStateSnapshot {
    fn from(s: &VodPlaybackState) -> Self {
        match s {
            VodPlaybackState::Idle => Self::Idle,
            VodPlaybackState::Loading => Self::Loading,
            VodPlaybackState::Playing => Self::Playing,
            VodPlaybackState::Paused => Self::Paused,
            VodPlaybackState::Seeking => Self::Seeking,
            VodPlaybackState::Buffering => Self::Buffering,
            VodPlaybackState::Ended => Self::Ended,
            VodPlaybackState::Error(m) => Self::Error(m.clone()),
        }
    }
}

/// `VODPlayerViewModel.formatTime` — "H:MM:SS" 또는 "M:SS".
pub fn format_time(seconds: f64) -> String {
    let total = if seconds.is_finite() {
        seconds.max(0.0) as i64
    } else {
        0
    };
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

// MARK: - ClipBrowserViewModel

/// 표시 페이지 크기.
pub const DISPLAY_PAGE_SIZE: usize = 24;
/// 채널 클립 요청 크기.
pub const CHANNEL_FETCH_PAGE_SIZE: i64 = 20;
/// 최근 채널 최대 수.
pub const MAX_RECENT_CHANNELS: usize = 8;
/// 채널 후보 검색 디바운스.
pub const CHANNEL_SUGGESTION_DEBOUNCE: Duration = Duration::from_millis(250);
/// 채널 후보 검색 크기.
pub const CHANNEL_SUGGESTION_SIZE: i64 = 8;
/// `fetchCachedClips(source:limit:)` 기본 한도.
pub const CACHED_CLIP_FETCH_LIMIT: usize = 200;

pub const WATCH_LATER_DEFAULTS_KEY: &str = "ClipBrowser.watchLaterUIDs";
pub const SAVED_SNAPSHOTS_DEFAULTS_KEY: &str = "ClipBrowser.savedClipSnapshots.v1";
pub const QUEUE_DEFAULTS_KEY: &str = "ClipBrowser.queueClips.v1";
pub const RECENT_CHANNELS_DEFAULTS_KEY: &str = "ClipBrowser.recentChannels.v1";

fn div_ceil(n: usize, d: usize) -> usize {
    n.div_ceil(d)
}

/// 클립 메뉴 상태.
pub struct ClipBrowserState {
    pub selected_tab: ClipTab,
    pub view_mode: ClipViewMode,

    pub trending_clips: Vec<ClipInfo>,
    pub trending_is_loading: bool,
    pub trending_error: Option<String>,
    pub trending_filter: TrendingFilter,
    pub trending_order: TrendingOrder,
    selected_mood_id: Option<String>,

    pub channel_input: String,
    pub resolved_channel_id: Option<String>,
    pub channel_clips: Vec<ClipInfo>,
    pub channel_is_loading: bool,
    pub channel_error: Option<String>,
    pub channel_page: i64,
    pub channel_total_count: Option<i64>,
    pub channel_has_more: bool,
    pub channel_sort_order: ClipSortOrder,

    pub channel_suggestions: Vec<ChannelInfo>,
    pub is_searching_channels: bool,
    suggestion_token: u64,

    pub selected_clip: Option<ClipInfo>,
    pub preview_clip: Option<ClipInfo>,

    pub did_hydrate_watch_later: bool,
    pub watch_later_uids: HashSet<String>,
    pub queue_clips: Vec<ClipInfo>,
    pub saved_clip_snapshots: HashMap<String, ClipInfo>,
    pub recent_channels: Vec<ChannelInfo>,

    pub channel_display_page: usize,
    pub trending_display_page: usize,

    trending_request_token: u64,
    channel_request_token: u64,
    is_trending_auto_refresh_active: bool,

    /// API 클라이언트 (외부 주입, 교체 가능).
    pub api: Option<Arc<dyn ClipApi>>,
    /// DB 캐시.
    pub data_store: Option<Arc<dyn ClipCacheStore>>,
    /// 정기 작업 코디네이터.
    pub refresh_coordinator: Option<Arc<dyn RefreshDemandSink>>,
}

impl ClipBrowserState {
    fn new() -> Self {
        Self {
            selected_tab: ClipTab::Trending,
            view_mode: ClipViewMode::Grid,
            trending_clips: Vec::new(),
            trending_is_loading: false,
            trending_error: None,
            trending_filter: TrendingFilter::Week,
            trending_order: TrendingOrder::Popular,
            selected_mood_id: None,
            channel_input: String::new(),
            resolved_channel_id: None,
            channel_clips: Vec::new(),
            channel_is_loading: false,
            channel_error: None,
            channel_page: 0,
            channel_total_count: None,
            channel_has_more: true,
            channel_sort_order: ClipSortOrder::Popular,
            channel_suggestions: Vec::new(),
            is_searching_channels: false,
            suggestion_token: 0,
            selected_clip: None,
            preview_clip: None,
            did_hydrate_watch_later: false,
            watch_later_uids: HashSet::new(),
            queue_clips: Vec::new(),
            saved_clip_snapshots: HashMap::new(),
            recent_channels: Vec::new(),
            channel_display_page: 0,
            trending_display_page: 0,
            trending_request_token: 0,
            channel_request_token: 0,
            is_trending_auto_refresh_active: false,
            api: None,
            data_store: None,
            refresh_coordinator: None,
        }
    }

    pub fn selected_mood_id(&self) -> Option<&str> {
        self.selected_mood_id.as_deref()
    }

    /// didSet — 바뀌면 인기 표시 페이지를 0 으로.
    pub fn set_selected_mood_id(&mut self, id: Option<String>) {
        if self.selected_mood_id != id {
            self.selected_mood_id = id;
            self.trending_display_page = 0;
        }
    }

    pub fn is_trending_auto_refresh_active(&self) -> bool {
        self.is_trending_auto_refresh_active
    }

    /// 인기 클립 캐시 source 키 — `trending-{filter}-{order}` (소문자).
    pub fn trending_source(&self) -> String {
        format!(
            "trending-{}-{}",
            self.trending_filter.api_value().to_lowercase(),
            self.trending_order.api_value().to_lowercase()
        )
    }

    /// 표시 중인 채널 클립 슬라이스.
    pub fn displayed_channel_clips(&self) -> &[ClipInfo] {
        let start = self.channel_display_page * DISPLAY_PAGE_SIZE;
        if start >= self.channel_clips.len() {
            return &[];
        }
        let end = (start + DISPLAY_PAGE_SIZE).min(self.channel_clips.len());
        &self.channel_clips[start..end]
    }

    /// 채널 클립 표시 총 페이지 수.
    pub fn channel_total_display_pages(&self) -> usize {
        if let Some(total) = self.channel_total_count.filter(|t| *t > 0) {
            return div_ceil(total as usize, DISPLAY_PAGE_SIZE);
        }
        div_ceil(self.channel_clips.len(), DISPLAY_PAGE_SIZE).max(1)
    }

    /// mood 필터 적용 후 인기 클립.
    pub fn filtered_trending_clips(&self) -> Vec<ClipInfo> {
        match self.selected_mood_id.as_deref() {
            None | Some("all") => self.trending_clips.clone(),
            Some(mood) => self
                .trending_clips
                .iter()
                .filter(|c| {
                    ClipMood::matches(
                        &c.clip_title,
                        c.channel.as_ref().map(|ch| ch.channel_name.as_str()),
                        Some(mood),
                    )
                })
                .cloned()
                .collect(),
        }
    }

    pub fn displayed_trending_clips(&self) -> Vec<ClipInfo> {
        let source = self.filtered_trending_clips();
        let start = self.trending_display_page * DISPLAY_PAGE_SIZE;
        if start >= source.len() {
            return Vec::new();
        }
        let end = (start + DISPLAY_PAGE_SIZE).min(source.len());
        source[start..end].to_vec()
    }

    pub fn trending_total_display_pages(&self) -> usize {
        div_ceil(self.filtered_trending_clips().len(), DISPLAY_PAGE_SIZE).max(1)
    }

    pub fn go_to_trending_display_page(&mut self, page: usize) {
        if page < self.trending_total_display_pages() {
            self.trending_display_page = page;
        }
    }

    pub fn clear_channel_selection(&mut self) {
        self.resolved_channel_id = None;
        self.channel_input.clear();
        self.channel_clips.clear();
        self.channel_total_count = None;
        self.channel_display_page = 0;
        self.channel_error = None;
    }

    /// 카드 클릭 — 인스펙터가 있으면 미리보기, 없으면 바로 재생.
    pub fn handle_clip_tap(&mut self, clip: ClipInfo, inspector_available: bool) {
        if inspector_available {
            self.preview_clip = Some(clip);
        } else {
            self.selected_clip = Some(clip);
        }
    }

    pub fn play_clip(&mut self, clip: ClipInfo) {
        self.selected_clip = Some(clip);
    }

    pub fn is_watch_later(&self, clip: &ClipInfo) -> bool {
        self.watch_later_uids.contains(&clip.clip_uid)
    }

    pub fn is_in_queue(&self, clip: &ClipInfo) -> bool {
        self.queue_clips.iter().any(|c| c.clip_uid == clip.clip_uid)
    }

    /// 큐의 다음 클립 (현재 클립 다음, 없으면 첫 항목).
    pub fn play_next_in_queue(&self, current: Option<&ClipInfo>) -> Option<ClipInfo> {
        if self.queue_clips.is_empty() {
            return None;
        }
        if let Some(cur) = current {
            if let Some(idx) = self
                .queue_clips
                .iter()
                .position(|c| c.clip_uid == cur.clip_uid)
            {
                if idx + 1 < self.queue_clips.len() {
                    return Some(self.queue_clips[idx + 1].clone());
                }
            }
        }
        self.queue_clips.first().cloned()
    }
}

/// 원본 클립 페이지 링크.
pub fn original_clip_link(clip: &ClipInfo) -> String {
    format!("https://chzzk.naver.com/clips/{}", clip.clip_uid)
}

/// 클립 메뉴 스냅샷.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipBrowserSnapshot {
    pub selected_tab: ClipTab,
    pub view_mode: ClipViewMode,
    pub trending_clips: Vec<ClipInfo>,
    pub displayed_trending_clips: Vec<ClipInfo>,
    pub trending_is_loading: bool,
    pub trending_error: Option<String>,
    pub trending_filter: TrendingFilter,
    pub trending_order: TrendingOrder,
    pub selected_mood_id: Option<String>,
    pub moods: Vec<ClipMood>,
    pub trending_display_page: usize,
    pub trending_total_display_pages: usize,
    pub channel_input: String,
    pub resolved_channel_id: Option<String>,
    pub displayed_channel_clips: Vec<ClipInfo>,
    pub channel_clip_count: usize,
    pub channel_is_loading: bool,
    pub channel_error: Option<String>,
    pub channel_has_more: bool,
    pub channel_sort_order: ClipSortOrder,
    pub channel_display_page: usize,
    pub channel_total_display_pages: usize,
    pub channel_suggestions: Vec<ChannelInfo>,
    pub is_searching_channels: bool,
    pub selected_clip: Option<ClipInfo>,
    pub preview_clip: Option<ClipInfo>,
    pub watch_later_uids: Vec<String>,
    pub queue_clips: Vec<ClipInfo>,
    pub recent_channels: Vec<ChannelInfo>,
}

/// 공유 셀.
pub struct ClipBrowserShared {
    obs: Observed<ClipBrowserState>,
    defaults: Arc<dyn KeyValueStore>,
    instance: u64,
}

static NEXT_INSTANCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// 클립 메뉴 뷰모델 핸들.
#[derive(Clone)]
pub struct ClipBrowserViewModel {
    shared: Arc<ClipBrowserShared>,
}

impl ClipBrowserViewModel {
    pub fn new(defaults: Arc<dyn KeyValueStore>) -> Self {
        Self {
            shared: Arc::new(ClipBrowserShared {
                obs: Observed::new(ClipBrowserState::new()),
                defaults,
                instance: NEXT_INSTANCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            }),
        }
    }

    pub fn lock(&self) -> ObservedGuard<'_, ClipBrowserState> {
        self.shared.obs.lock()
    }

    pub fn read(&self) -> ObservedGuard<'_, ClipBrowserState> {
        self.shared.obs.read()
    }

    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.shared.obs.subscribe()
    }

    /// 코디네이터에 수요를 잡을 때 쓰는 인스턴스 id.
    pub fn refresh_demand_instance(&self) -> u64 {
        self.shared.instance
    }

    pub fn snapshot(&self) -> ClipBrowserSnapshot {
        let st = self.read();
        let mut later: Vec<String> = st.watch_later_uids.iter().cloned().collect();
        later.sort();
        ClipBrowserSnapshot {
            selected_tab: st.selected_tab,
            view_mode: st.view_mode,
            trending_clips: st.trending_clips.clone(),
            displayed_trending_clips: st.displayed_trending_clips(),
            trending_is_loading: st.trending_is_loading,
            trending_error: st.trending_error.clone(),
            trending_filter: st.trending_filter,
            trending_order: st.trending_order,
            selected_mood_id: st.selected_mood_id.clone(),
            moods: ClipMood::default_static_moods(st.trending_clips.len()),
            trending_display_page: st.trending_display_page,
            trending_total_display_pages: st.trending_total_display_pages(),
            channel_input: st.channel_input.clone(),
            resolved_channel_id: st.resolved_channel_id.clone(),
            displayed_channel_clips: st.displayed_channel_clips().to_vec(),
            channel_clip_count: st.channel_clips.len(),
            channel_is_loading: st.channel_is_loading,
            channel_error: st.channel_error.clone(),
            channel_has_more: st.channel_has_more,
            channel_sort_order: st.channel_sort_order,
            channel_display_page: st.channel_display_page,
            channel_total_display_pages: st.channel_total_display_pages(),
            channel_suggestions: st.channel_suggestions.clone(),
            is_searching_channels: st.is_searching_channels,
            selected_clip: st.selected_clip.clone(),
            preview_clip: st.preview_clip.clone(),
            watch_later_uids: later,
            queue_clips: st.queue_clips.clone(),
            recent_channels: st.recent_channels.clone(),
        }
    }
}

/// 채널 클립 한 페이지를 붙인다 — 받아 둔 순서는 건드리지 않고, 인기순이면 새 묶음만 정렬.
pub fn merging_channel_page(
    existing: Vec<ClipInfo>,
    page: &[ClipInfo],
    sort_order: ClipSortOrder,
) -> Vec<ClipInfo> {
    let known: HashSet<&str> = existing.iter().map(|c| c.clip_uid.as_str()).collect();
    let mut batch: Vec<ClipInfo> = page
        .iter()
        .filter(|c| !known.contains(c.clip_uid.as_str()))
        .cloned()
        .collect();
    if sort_order == ClipSortOrder::Popular {
        batch.sort_by_key(|c| std::cmp::Reverse(c.read_count));
    }
    let mut out = existing;
    out.extend(batch);
    out
}

/// `CachedClipData` → `ClipInfo`.
pub fn to_clip_info(d: &CachedClipData) -> ClipInfo {
    let channel = d.channel_id.as_ref().filter(|c| !c.is_empty()).map(|cid| {
        let mut ch = ChannelInfo::new(cid.clone(), d.channel_name.clone().unwrap_or_default());
        ch.channel_image_url = d
            .channel_image_url
            .as_deref()
            .and_then(|u| Url::parse(u).ok());
        ch
    });
    ClipInfo {
        clip_uid: d.clip_uid.clone(),
        clip_title: d.clip_title.clone(),
        thumbnail_image_url: d
            .thumbnail_image_url
            .as_deref()
            .and_then(|u| Url::parse(u).ok()),
        clip_url: d.clip_url.as_deref().and_then(|u| Url::parse(u).ok()),
        duration: d.duration,
        read_count: d.read_count,
        created_date: d.created_date,
        channel,
    }
}

/// 인기 클립 묶음을 DB 에 적재 — 내용 해시로 변화 감지, 목록에서 빠진 행은 source 태그를 뗀다.
fn persist_clips_batch(store: &dyn ClipCacheStore, clips: &[ClipInfo], source: &str) {
    if clips.is_empty() {
        return;
    }
    let now = chrono::Utc::now();
    let data: Vec<CachedClipData> = clips
        .iter()
        .map(|c| {
            let thumb = c.thumbnail_image_url.as_ref().map(|u| u.to_string());
            let ch_name = c.channel.as_ref().map(|ch| ch.channel_name.clone());
            let hash = compute_content_hash(
                &c.clip_title,
                thumb.as_deref(),
                c.read_count,
                ch_name.as_deref(),
            );
            CachedClipData {
                clip_uid: c.clip_uid.clone(),
                clip_title: c.clip_title.clone(),
                thumbnail_image_url: thumb,
                clip_url: c.clip_url.as_ref().map(|u| u.to_string()),
                duration: c.duration,
                read_count: c.read_count,
                created_date: c.created_date,
                channel_id: c.channel.as_ref().map(|ch| ch.channel_id.clone()),
                channel_name: ch_name,
                channel_image_url: c
                    .channel
                    .as_ref()
                    .and_then(|ch| ch.channel_image_url.as_ref().map(|u| u.to_string())),
                content_hash: hash,
                sources: [source.to_string()].into_iter().collect(),
                fetched_at: now,
            }
        })
        .collect();
    let keep: HashSet<String> = data.iter().map(|d| d.clip_uid.clone()).collect();
    let changed = store.upsert_and_retain(&data, source, &keep).unwrap_or(0);
    if changed > 0 {
        tracing::debug!("clips: source={source} delta={changed}");
    }
}
