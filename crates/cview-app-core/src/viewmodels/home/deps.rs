//! HomeViewModel 이 기대는 협력자 — 앱 서비스·캐싱 계층(다른 포팅 작업) 대신 작은 trait 으로 둔다.
//!
//! | Swift 협력자 | Rust 경계 |
//! |---|---|
//! | `LiveCatalogStore` (전수 수집·요약·행 파일) | [`LiveCatalogPort`] (구현: [`LiveCatalogStore`]) |
//! | `FollowingStore` (팔로잉 폴) | [`FollowingPoller`] (구현: [`FollowingStore`]) |
//! | `MenuSnapshotStore.shared` (메뉴 스냅샷 파일) | [`MenuSnapshots`] |
//! | `ChzzkAPIClient.topLivesWithAge` | [`PopularLiveSource`] (기본: `ChzzkApiClient`) |
//! | `LiveThumbnailService`·`ImageCacheService` 무효화 | [`HomeImageCache`] |
//! | `LiveEventBus.shared.restore` | [`LiveActivityRestorer`] |
//! | `RefreshCoordinator` retain/release | [`crate::services::refresh_coordinator::RefreshDemandSink`] |

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use cview_core::models::live_catalog::{LiveCatalogRowsSnapshot, LiveCatalogSummary};
use cview_core::models::{LiveChannelItem, LiveInfo};
use cview_core::utils::following_poll::{FollowingPollReason, FollowingPollResult};
use cview_core::utils::time::{Date, TimeInterval};
use cview_networking::{AgedResponse, ChzzkApiClient, PagedContent};

use super::promotion::CatalogPromotionDecision;
use crate::caching::following_store::{to_live_channel_item, FollowingStore};
use crate::caching::live_catalog_store::LiveCatalogStore;

pub use crate::caching::live_catalog_store::CatalogSweepReason;

/// 지연 로드한 행.
#[derive(Debug, Clone, PartialEq)]
pub struct RestoredCatalogRows {
    pub snapshot: LiveCatalogRowsSnapshot,
    pub items: Vec<LiveChannelItem>,
    pub migrated: bool,
}

/// `LiveCatalogStore` 가 홈에 보여 주는 면.
#[async_trait]
pub trait LiveCatalogPort: Send + Sync {
    fn usable_summary(&self) -> Option<LiveCatalogSummary>;
    fn summary_revision(&self) -> u64;
    /// `summary != nil`.
    fn has_summary(&self) -> bool;
    fn is_sweeping(&self) -> bool;
    fn last_attempt_at(&self) -> Option<Date>;
    /// `demandKinds.contains(where: \.startsSweep)`.
    fn demand_starts_sweep(&self) -> bool;
    fn request_sweep(&self, reason: CatalogSweepReason);
    fn growth_rates(&self) -> HashMap<String, f64>;
    async fn run_manual(&self);
    async fn load_persisted_rows(&self) -> Option<RestoredCatalogRows>;
    async fn promote(
        &self,
        decision: &CatalogPromotionDecision,
        items: &[LiveInfo],
        completed_at: Date,
        reason: CatalogSweepReason,
    );
    async fn hydrate_summary(&self);
    async fn hydrate_checkpoint(&self);
}

/// 아무 카탈로그도 없는 기본값(테스트·초기화용). 시도 시각만 기록할 수 있다.
#[derive(Debug, Default)]
pub struct NullCatalog {
    pub last_attempt_at: Mutex<Option<Date>>,
    pub summary: Mutex<Option<LiveCatalogSummary>>,
    pub requested: Mutex<Vec<CatalogSweepReason>>,
    pub restored_rows: Mutex<Option<RestoredCatalogRows>>,
    pub demand_starts: Mutex<bool>,
}

#[async_trait]
impl LiveCatalogPort for NullCatalog {
    fn usable_summary(&self) -> Option<LiveCatalogSummary> {
        self.summary.lock().unwrap().clone()
    }
    fn summary_revision(&self) -> u64 {
        0
    }
    fn has_summary(&self) -> bool {
        self.summary.lock().unwrap().is_some()
    }
    fn is_sweeping(&self) -> bool {
        false
    }
    fn last_attempt_at(&self) -> Option<Date> {
        *self.last_attempt_at.lock().unwrap()
    }
    fn demand_starts_sweep(&self) -> bool {
        *self.demand_starts.lock().unwrap()
    }
    fn request_sweep(&self, reason: CatalogSweepReason) {
        self.requested.lock().unwrap().push(reason);
    }
    fn growth_rates(&self) -> HashMap<String, f64> {
        HashMap::new()
    }
    async fn run_manual(&self) {}
    async fn load_persisted_rows(&self) -> Option<RestoredCatalogRows> {
        self.restored_rows.lock().unwrap().clone()
    }
    async fn promote(
        &self,
        _decision: &CatalogPromotionDecision,
        _items: &[LiveInfo],
        _completed_at: Date,
        _reason: CatalogSweepReason,
    ) {
    }
    async fn hydrate_summary(&self) {}
    async fn hydrate_checkpoint(&self) {}
}

#[async_trait]
impl FollowingPoller for FollowingStore {
    fn generation(&self) -> u64 {
        FollowingStore::generation(self)
    }
    async fn poll(&self, reason: FollowingPollReason) -> Option<HomeFollowingResult> {
        let r = FollowingStore::poll(self, reason).await?;
        Some(HomeFollowingResult {
            reason: r.reason,
            channels: r.channels.iter().map(to_live_channel_item).collect(),
            fetched_at: r.fetched_at,
            generation: r.generation,
            newly_live: r.newly_live,
            is_baseline: r.is_baseline,
            list_carries_live_info: r.list_carries_live_info,
            detail_requests: r.detail_requests,
        })
    }
    fn reset_for_account_boundary(&self) {
        FollowingStore::reset_for_account_boundary(self);
    }
}

/// 실제 카탈로그 저장소. 행·요약 파일 영속화(`+Persistence`·`+Checkpoint`)와 요약 빌드(`promote`)는
/// 저장소 쪽에 아직 없어 그 면은 비어 있다 — [`LiveCatalogStore`] 모듈 문서 참고.
#[async_trait]
impl LiveCatalogPort for LiveCatalogStore {
    fn usable_summary(&self) -> Option<LiveCatalogSummary> {
        LiveCatalogStore::usable_summary(self, None)
    }
    fn summary_revision(&self) -> u64 {
        LiveCatalogStore::summary_revision(self)
    }
    fn has_summary(&self) -> bool {
        self.summary().is_some()
    }
    fn is_sweeping(&self) -> bool {
        LiveCatalogStore::is_sweeping(self)
    }
    fn last_attempt_at(&self) -> Option<Date> {
        LiveCatalogStore::last_attempt_at(self)
    }
    fn demand_starts_sweep(&self) -> bool {
        self.demand_kinds().iter().any(|k| k.starts_sweep())
    }
    fn request_sweep(&self, reason: CatalogSweepReason) {
        LiveCatalogStore::request_sweep(self, reason);
    }
    fn growth_rates(&self) -> HashMap<String, f64> {
        LiveCatalogStore::growth_rates(self)
    }
    async fn run_manual(&self) {
        LiveCatalogStore::run_manual(self).await;
    }
    async fn load_persisted_rows(&self) -> Option<RestoredCatalogRows> {
        None
    }
    async fn promote(
        &self,
        _decision: &CatalogPromotionDecision,
        _items: &[LiveInfo],
        _completed_at: Date,
        _reason: CatalogSweepReason,
    ) {
    }
    async fn hydrate_summary(&self) {}
    async fn hydrate_checkpoint(&self) {}
}

/// 팔로잉 폴 실패 원인.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FollowingError {
    pub message: String,
    pub is_unauthorized: bool,
}

pub type HomeFollowingResult = FollowingPollResult<LiveChannelItem>;
pub type HomeFollowingOutcome =
    cview_core::utils::following_poll::FollowingPollOutcome<LiveChannelItem, FollowingError>;

/// `FollowingStore` 의 홈 면. 폴 결과는 스토어가 싱크(`HomeViewModel::receive_following_poll`)로 보낸다.
#[async_trait]
pub trait FollowingPoller: Send + Sync {
    fn generation(&self) -> u64;
    async fn poll(&self, reason: FollowingPollReason) -> Option<HomeFollowingResult>;
    fn reset_for_account_boundary(&self);
}

/// 폴을 기록만 하는 기본값.
#[derive(Debug, Default)]
pub struct NullFollowingPoller {
    pub generation: Mutex<u64>,
    pub polls: Mutex<Vec<FollowingPollReason>>,
}

#[async_trait]
impl FollowingPoller for NullFollowingPoller {
    fn generation(&self) -> u64 {
        *self.generation.lock().unwrap()
    }
    async fn poll(&self, reason: FollowingPollReason) -> Option<HomeFollowingResult> {
        self.polls.lock().unwrap().push(reason);
        None
    }
    fn reset_for_account_boundary(&self) {
        let mut g = self.generation.lock().unwrap();
        *g = g.wrapping_add(1);
    }
}

/// 스냅샷 읽기 결과.
#[derive(Debug, Clone, PartialEq)]
pub enum SnapshotLoadResult<T> {
    Fresh(T, Date),
    Expired(Date),
    Missing,
    Rejected,
}

/// 인기·팔로잉 목록 스냅샷.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuChannelListSnapshot {
    pub channels: Vec<LiveChannelItem>,
    pub fetched_at: Date,
}

/// 추천 행 DTO.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecommendedRow {
    pub channel: LiveChannelItem,
    pub score: f64,
    pub reasons: Vec<String>,
}

/// 홈 부가 스냅샷(히어로·추천·활동). 활동 이벤트는 이벤트 버스가 해석하므로 그대로 둔다.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeExtrasSnapshot {
    pub hero: Option<LiveChannelItem>,
    pub hero_reasons: Vec<String>,
    pub recommended: Vec<RecommendedRow>,
    #[serde(default)]
    pub activity: serde_json::Value,
}

impl HomeExtrasSnapshot {
    pub const PAYLOAD_VERSION: i64 = 1;
    pub const RECOMMENDED_LIMIT: usize = 14;
}

/// `MenuSnapshotStore`.
#[async_trait]
pub trait MenuSnapshots: Send + Sync {
    async fn load_channel_list(
        &self,
        source: &str,
        scope: &str,
        max_age: TimeInterval,
    ) -> SnapshotLoadResult<MenuChannelListSnapshot>;
    async fn load_home_extras(
        &self,
        scope: &str,
        max_age: TimeInterval,
    ) -> SnapshotLoadResult<HomeExtrasSnapshot>;
    async fn save_channel_list(&self, source: &str, scope: &str, payload: MenuChannelListSnapshot);
    /// 진단용 누적 읽은 바이트.
    async fn bytes_read(&self) -> u64;
}

/// 메모리 스냅샷 저장소(기본·테스트).
#[derive(Debug, Default)]
pub struct MemoryMenuSnapshots {
    pub lists: Mutex<HashMap<(String, String), SnapshotLoadResult<MenuChannelListSnapshot>>>,
    pub extras: Mutex<HashMap<String, SnapshotLoadResult<HomeExtrasSnapshot>>>,
    pub saved: Mutex<Vec<(String, String, MenuChannelListSnapshot)>>,
}

#[async_trait]
impl MenuSnapshots for MemoryMenuSnapshots {
    async fn load_channel_list(
        &self,
        source: &str,
        scope: &str,
        _max_age: TimeInterval,
    ) -> SnapshotLoadResult<MenuChannelListSnapshot> {
        self.lists
            .lock()
            .unwrap()
            .get(&(source.to_string(), scope.to_string()))
            .cloned()
            .unwrap_or(SnapshotLoadResult::Missing)
    }
    async fn load_home_extras(
        &self,
        scope: &str,
        _max_age: TimeInterval,
    ) -> SnapshotLoadResult<HomeExtrasSnapshot> {
        self.extras
            .lock()
            .unwrap()
            .get(scope)
            .cloned()
            .unwrap_or(SnapshotLoadResult::Missing)
    }
    async fn save_channel_list(&self, source: &str, scope: &str, payload: MenuChannelListSnapshot) {
        self.saved
            .lock()
            .unwrap()
            .push((source.to_string(), scope.to_string(), payload));
    }
    async fn bytes_read(&self) -> u64 {
        0
    }
}

/// 인기 첫 페이지.
#[async_trait]
pub trait PopularLiveSource: Send + Sync {
    async fn top_lives_with_age(
        &self,
        size: i64,
    ) -> Result<AgedResponse<PagedContent<LiveInfo>>, String>;
}

#[async_trait]
impl PopularLiveSource for ChzzkApiClient {
    async fn top_lives_with_age(
        &self,
        size: i64,
    ) -> Result<AgedResponse<PagedContent<LiveInfo>>, String> {
        ChzzkApiClient::top_lives_with_age(self, size, None, None, false)
            .await
            .map_err(|e| e.to_string())
    }
}

/// 썸네일·프로필 이미지 캐시 무효화.
#[async_trait]
pub trait HomeImageCache: Send + Sync {
    async fn invalidate_all_thumbnails(&self);
    async fn invalidate(&self, urls: &[String]);
}

#[derive(Debug, Default)]
pub struct NoopImageCache;

#[async_trait]
impl HomeImageCache for NoopImageCache {
    async fn invalidate_all_thumbnails(&self) {}
    async fn invalidate(&self, _urls: &[String]) {}
}

/// 이벤트 버스 활동 복원.
#[async_trait]
pub trait LiveActivityRestorer: Send + Sync {
    /// 스냅샷의 활동(창 안·최근순 상한은 구현이 거른다)을 버스에 되살린다.
    async fn restore(&self, activity: &serde_json::Value, now: Date);
}

#[derive(Debug, Default)]
pub struct NoopActivityRestorer;

#[async_trait]
impl LiveActivityRestorer for NoopActivityRestorer {
    async fn restore(&self, _activity: &serde_json::Value, _now: Date) {}
}

/// HomeViewModel 협력자 묶음.
#[derive(Clone)]
pub struct HomeDeps {
    pub popular: Arc<dyn PopularLiveSource>,
    pub catalog: Arc<dyn LiveCatalogPort>,
    pub following: Arc<dyn FollowingPoller>,
    pub snapshots: Arc<dyn MenuSnapshots>,
    pub images: Arc<dyn HomeImageCache>,
    pub activity: Arc<dyn LiveActivityRestorer>,
}

impl HomeDeps {
    /// 네트워크 없는 기본값 — 인기 소스만 넣는다.
    pub fn with_popular(popular: Arc<dyn PopularLiveSource>) -> Self {
        Self {
            popular,
            catalog: Arc::new(NullCatalog::default()),
            following: Arc::new(NullFollowingPoller::default()),
            snapshots: Arc::new(MemoryMenuSnapshots::default()),
            images: Arc::new(NoopImageCache),
            activity: Arc::new(NoopActivityRestorer),
        }
    }
}
