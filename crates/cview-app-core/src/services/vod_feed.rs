//! `Sources/CViewApp/Services/VODFeedAggregator.swift` 포팅 — 팔로우 채널 VOD 팬아웃과 구독 피드 커서.
//!
//! - [`ChannelPageResult`] — 채널별 한 페이지 결과. 실패는 "더 없음" 이 아니라 **모름**이다(`has_more` = true).
//! - [`VodFeedAggregator::fetch_pages`] — 슬라이딩 창(기본 동시 6)으로 팬아웃한다. 연속 실패가 임계에 닿거나
//!   429 를 보면 새 요청을 멈춘다. 결과 순서는 완료 순.
//! - [`VodSubscriptionFeedCursor`] — 채널별 커서와 받아 둔 항목. 모든 페이지 요청은 같은 크기(8)를 쓴다.
//!
//! 원본의 `URLError` 분류는 Rust 클라이언트가 전송 오류를 `ApiError::NetworkError` 로 감싸므로 그 경로만 남겼다.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use cview_core::error::ApiError;
use cview_core::models::search::VodInfo;
use cview_core::utils::time::TimeInterval;
use cview_networking::ChzzkApiClient;
use futures_util::stream::{FuturesUnordered, StreamExt};

// MARK: - ChannelPageResult

/// 실패 종류 — 차단기와 백오프가 고른다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VodFailureKind {
    /// 429. 값은 서버가 준 Retry-After(초).
    RateLimited(TimeInterval),
    /// 전송 단계 실패. 한 번의 패스가 모두 이것이면 오프라인으로 본다.
    Offline,
    /// 그 밖(4xx·5xx·디코드·취소).
    Other,
}

/// 채널별 페이지 페치 결과.
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelPageResult {
    pub channel_id: String,
    pub items: Vec<VodInfo>,
    /// 실제로 요청한 페이지 번호(0부터).
    pub page: i64,
    /// 요청한 채널당 개수 — `has_more` 판단 기준.
    pub requested_size: usize,
    /// 요청이 실패해서 빈 결과인가.
    pub failed: bool,
    /// 실패의 종류 — 성공이면 `None`.
    pub failure_kind: Option<VodFailureKind>,
}

impl ChannelPageResult {
    pub fn success(
        channel_id: impl Into<String>,
        items: Vec<VodInfo>,
        page: i64,
        requested_size: usize,
    ) -> Self {
        Self {
            channel_id: channel_id.into(),
            items,
            page,
            requested_size,
            failed: false,
            failure_kind: None,
        }
    }

    /// 실패 결과. 종류가 없으면 `Other`.
    pub fn failure(
        channel_id: impl Into<String>,
        page: i64,
        requested_size: usize,
        kind: Option<VodFailureKind>,
    ) -> Self {
        Self {
            channel_id: channel_id.into(),
            items: Vec::new(),
            page,
            requested_size,
            failed: true,
            failure_kind: Some(kind.unwrap_or(VodFailureKind::Other)),
        }
    }

    /// 같은 결과에 항목만 바꾼 사본.
    pub fn replacing_items(&self, items: Vec<VodInfo>) -> Self {
        Self {
            items,
            ..self.clone()
        }
    }

    /// 오류 → 실패 종류.
    pub fn classify_failure(error: &ApiError) -> VodFailureKind {
        match error {
            ApiError::RateLimited { retry_after } => VodFailureKind::RateLimited(*retry_after),
            // 녹화 재생의 빈칸은 네트워크 상태와 무관하다.
            ApiError::NetworkError(message) if message == "API fixture miss" => {
                VodFailureKind::Other
            }
            ApiError::NetworkError(_) => VodFailureKind::Offline,
            _ => VodFailureKind::Other,
        }
    }

    /// 실패는 **모름**이라 true. 그 밖에는 `items.len() >= requested_size`.
    pub fn has_more(&self) -> bool {
        self.failed || self.items.len() >= self.requested_size
    }
}

// MARK: - VodFeedAggregator

/// 채널 VOD 페이지 조회원. 팔로우 VOD 팬아웃은 공용 응답 캐시에 넣지 않는다.
#[async_trait]
pub trait VodPageSource: Send + Sync {
    async fn vod_page(
        &self,
        channel_id: &str,
        page: i64,
        size: i64,
    ) -> Result<Vec<VodInfo>, ApiError>;
}

#[async_trait]
impl VodPageSource for ChzzkApiClient {
    async fn vod_page(
        &self,
        channel_id: &str,
        page: i64,
        size: i64,
    ) -> Result<Vec<VodInfo>, ApiError> {
        Ok(self.vod_list(channel_id, page, size, false).await?.data)
    }
}

/// 연속 실패 차단기 — 완료 순으로 본다. 성공 하나가 연속을 끊는다.
#[derive(Debug, Clone)]
pub struct FailureBreaker {
    threshold: usize,
    consecutive_failures: usize,
    saw_rate_limit: bool,
}

impl FailureBreaker {
    pub fn new(threshold: usize) -> Self {
        Self {
            threshold: threshold.max(1),
            consecutive_failures: 0,
            saw_rate_limit: false,
        }
    }

    pub fn is_tripped(&self) -> bool {
        self.saw_rate_limit || self.consecutive_failures >= self.threshold
    }

    pub fn observe(&mut self, result: &ChannelPageResult) {
        if !result.failed {
            self.consecutive_failures = 0;
            return;
        }
        self.consecutive_failures += 1;
        if matches!(result.failure_kind, Some(VodFailureKind::RateLimited(_))) {
            self.saw_rate_limit = true;
        }
    }
}

#[derive(Clone)]
pub struct VodFeedAggregator {
    source: Arc<dyn VodPageSource>,
}

impl VodFeedAggregator {
    pub const DEFAULT_SIZE: usize = 8;
    pub const DEFAULT_MAX_CONCURRENT: usize = 6;

    pub fn new(source: Arc<dyn VodPageSource>) -> Self {
        Self { source }
    }

    /// 채널별 지정 페이지를 동시 페치한다(슬라이딩 창). 시작하지 않은 채널은 결과에 없다.
    pub async fn fetch_pages(
        &self,
        channel_ids: &[String],
        pages: &HashMap<String, i64>,
        size: usize,
        max_concurrent: usize,
        stop_after_failures: usize,
    ) -> Vec<ChannelPageResult> {
        if channel_ids.is_empty() {
            return Vec::new();
        }
        let limit = max_concurrent.max(1);
        let mut results = Vec::with_capacity(channel_ids.len());
        let mut breaker = FailureBreaker::new(stop_after_failures);
        let mut in_flight = FuturesUnordered::new();
        for cid in channel_ids {
            if in_flight.len() >= limit {
                if let Some(done) = in_flight.next().await {
                    breaker.observe(&done);
                    results.push(done);
                }
            }
            if breaker.is_tripped() {
                break;
            }
            let page = pages.get(cid).copied().unwrap_or(0);
            let source = self.source.clone();
            let cid = cid.clone();
            in_flight.push(async move {
                match source.vod_page(&cid, page, size as i64).await {
                    Ok(items) => ChannelPageResult::success(cid, items, page, size),
                    Err(e) => ChannelPageResult::failure(
                        cid,
                        page,
                        size,
                        Some(ChannelPageResult::classify_failure(&e)),
                    ),
                }
            });
        }
        while let Some(done) = in_flight.next().await {
            results.push(done);
        }
        results
    }
}

// MARK: - VodSubscriptionFeedCursor

/// 구독 VOD 피드의 채널별 커서와 받아 둔 항목.
#[derive(Debug, Clone, Default)]
pub struct VodSubscriptionFeedCursor {
    last_page: HashMap<String, i64>,
    active: Vec<String>,
    pending: HashMap<String, Vec<VodInfo>>,
    seen_video_nos: HashSet<i64>,
}

impl VodSubscriptionFeedCursor {
    /// 채널당 요청 크기 — **모든** 페이지 요청에 같은 값을 쓴다(오프셋 = page × size).
    pub const CHANNEL_PAGE_SIZE: usize = 8;

    pub fn new(channel_ids: &[String]) -> Self {
        let mut seen = HashSet::new();
        Self {
            active: channel_ids
                .iter()
                .filter(|c| seen.insert((*c).clone()))
                .cloned()
                .collect(),
            ..Default::default()
        }
    }

    /// 로컬 사본(채널별 최신 행)으로 세운다. `order` 에 있고 사본이 있는 채널만 넣는다.
    pub fn seeded(
        seed: &HashMap<String, Vec<VodInfo>>,
        order: &[String],
        page_size: usize,
        exhausted: &HashSet<String>,
    ) -> Self {
        let size = page_size.max(1);
        let mut me = Self::default();
        let mut seen = HashSet::new();
        for channel_id in order {
            if !seen.insert(channel_id.clone()) {
                continue;
            }
            let Some(rows) = seed.get(channel_id).filter(|r| !r.is_empty()) else {
                continue;
            };
            let mut sorted = rows.clone();
            sorted.sort_by(newer_first);
            me.seen_video_nos.extend(sorted.iter().map(|v| v.video_no));
            if sorted.len() >= size {
                me.last_page
                    .insert(channel_id.clone(), (sorted.len() / size) as i64 - 1);
                if !exhausted.contains(channel_id) {
                    me.active.push(channel_id.clone());
                }
            }
            me.pending.insert(channel_id.clone(), sorted);
        }
        me
    }

    pub fn last_page(&self) -> &HashMap<String, i64> {
        &self.last_page
    }
    pub fn active(&self) -> &[String] {
        &self.active
    }
    pub fn pending(&self) -> &HashMap<String, Vec<VodInfo>> {
        &self.pending
    }

    fn pending_is_empty(&self, channel_id: &str) -> bool {
        self.pending.get(channel_id).is_none_or(Vec::is_empty)
    }

    /// 받아 둔 것이든 받을 것이든 더 내보낼 항목이 있는가.
    pub fn has_more(&self) -> bool {
        !self.active.is_empty() || self.pending.values().any(|v| !v.is_empty())
    }

    /// 다음에 받아야 할 (채널 → 페이지) — 받아 둔 항목이 떨어진 활성 채널만.
    pub fn pages_to_fetch(&self, skipped: &HashSet<String>) -> HashMap<String, i64> {
        self.active
            .iter()
            .filter(|c| !skipped.contains(*c) && self.pending_is_empty(c))
            .map(|c| (c.clone(), self.last_page.get(c).copied().unwrap_or(-1) + 1))
            .collect()
    }

    /// 페이지 결과를 반영한다. 실패는 커서를 전진시키지 않는다.
    pub fn apply(&mut self, results: &[ChannelPageResult]) {
        for result in results.iter().filter(|r| !r.failed) {
            let expected = self
                .last_page
                .get(&result.channel_id)
                .copied()
                .unwrap_or(-1)
                + 1;
            if result.page != expected {
                continue; // 늦게 도착한 옛 페이지·중복 응답
            }
            self.last_page
                .insert(result.channel_id.clone(), result.page);
            let fresh: Vec<VodInfo> = result
                .items
                .iter()
                .filter(|v| self.seen_video_nos.insert(v.video_no))
                .cloned()
                .collect();
            self.pending
                .entry(result.channel_id.clone())
                .or_default()
                .extend(fresh);
            if !result.has_more() {
                self.active.retain(|c| c != &result.channel_id);
            }
        }
    }

    /// 받아 둔 항목 중 가장 최신 하나를 꺼낸다. `strict` 면 받아 둔 것이 떨어진 활성 채널이 있을 때 `None`.
    pub fn pop_newest(&mut self, strict: bool, skipped: &HashSet<String>) -> Option<VodInfo> {
        if strict
            && self
                .active
                .iter()
                .any(|c| !skipped.contains(c) && self.pending_is_empty(c))
        {
            return None;
        }
        let mut best: Option<(&String, &VodInfo)> = None;
        for (channel_id, items) in &self.pending {
            let Some(head) = items.first() else { continue };
            if let Some((_, current)) = best {
                if !Self::is_newer(head, current) {
                    continue;
                }
            }
            best = Some((channel_id, head));
        }
        let channel_id = best?.0.clone();
        Some(self.pending.get_mut(&channel_id)?.remove(0))
    }

    /// 한 채널의 로컬 사본에 받은 페이지를 합친다.
    pub fn merged_local_rows(
        rows: &[VodInfo],
        page: i64,
        items: &[VodInfo],
        exhausted: bool,
        limit: usize,
    ) -> Vec<VodInfo> {
        let incoming: HashSet<i64> = items.iter().map(|v| v.video_no).collect();
        let oldest = items
            .iter()
            .reduce(|a, b| if Self::is_newer(a, b) { b } else { a });
        let kept: Vec<VodInfo> = if page > 0 {
            rows.iter()
                .filter(|r| !incoming.contains(&r.video_no))
                .cloned()
                .collect()
        } else if let (false, Some(oldest)) = (exhausted, oldest) {
            rows.iter()
                .filter(|r| !incoming.contains(&r.video_no) && !Self::is_newer(r, oldest))
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        let mut seen = HashSet::new();
        let mut merged: Vec<VodInfo> = items
            .iter()
            .chain(kept.iter())
            .filter(|v| seen.insert(v.video_no))
            .cloned()
            .collect();
        merged.sort_by(newer_first);
        merged.truncate(limit.max(1));
        merged
    }

    /// 게시 시각 내림차순, 같으면 videoNo 내림차순.
    pub fn is_newer(lhs: &VodInfo, rhs: &VodInfo) -> bool {
        match (lhs.publish_date, rhs.publish_date) {
            (Some(l), Some(r)) => {
                if l != r {
                    l > r
                } else {
                    lhs.video_no > rhs.video_no
                }
            }
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => lhs.video_no > rhs.video_no,
        }
    }
}

fn newer_first(a: &VodInfo, b: &VodInfo) -> std::cmp::Ordering {
    if VodSubscriptionFeedCursor::is_newer(a, b) {
        std::cmp::Ordering::Less
    } else if VodSubscriptionFeedCursor::is_newer(b, a) {
        std::cmp::Ordering::Greater
    } else {
        std::cmp::Ordering::Equal
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/VODSubscriptionFeedCursorTests.swift`,
    //! `VODFeedFanoutWindowTests.swift`
    use super::*;
    use cview_core::utils::time::from_unix;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const SIZE: usize = VodSubscriptionFeedCursor::CHANNEL_PAGE_SIZE;

    fn channel_seed(channel: &str) -> i64 {
        channel
            .bytes()
            .fold(0i64, |a, b| (a * 31 + b as i64) & 0xFFFF)
    }

    fn page(channel: &str, page: i64, count: usize, base: f64, failed: bool) -> ChannelPageResult {
        if failed {
            return ChannelPageResult::failure(channel, page, SIZE, None);
        }
        let items = (0..count)
            .map(|i| {
                let order = page * SIZE as i64 + i as i64;
                let mut v = VodInfo::new(
                    channel_seed(channel) * 1_000 + order,
                    format!("{channel}-{order}"),
                );
                v.publish_date = Some(from_unix(base - order as f64 * 3_600.0));
                v
            })
            .collect();
        ChannelPageResult::success(channel, items, page, SIZE)
    }

    fn p(channel: &str, n: i64, base: f64) -> ChannelPageResult {
        page(channel, n, SIZE, base, false)
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn none() -> HashSet<String> {
        HashSet::new()
    }

    #[test]
    fn next_page_continues_without_gap() {
        let mut c = VodSubscriptionFeedCursor::new(&ids(&["a"]));
        assert_eq!(c.pages_to_fetch(&none()), HashMap::from([("a".into(), 0)]));
        c.apply(&[p("a", 0, 1e6)]);
        while c.pop_newest(true, &none()).is_some() {}
        assert_eq!(c.pages_to_fetch(&none()), HashMap::from([("a".into(), 1)]));
    }

    #[test]
    fn leftover_items_are_preserved_in_order() {
        let mut c = VodSubscriptionFeedCursor::new(&ids(&["new", "old"]));
        c.apply(&[p("new", 0, 2e6), p("old", 0, 1e6)]);
        let first: Vec<String> = (0..10)
            .filter_map(|_| c.pop_newest(false, &none()).map(|v| v.video_title))
            .collect();
        assert!(first[..8].iter().all(|t| t.starts_with("new-")));
        assert_eq!(first[8..], ["old-0".to_string(), "old-1".to_string()]);
        let mut rest = Vec::new();
        while let Some(v) = c.pop_newest(false, &none()) {
            rest.push(v.video_title);
        }
        assert_eq!(rest, (2..8).map(|i| format!("old-{i}")).collect::<Vec<_>>());
    }

    #[test]
    fn strict_mode_waits_for_exhausted_active_channel() {
        let mut c = VodSubscriptionFeedCursor::new(&ids(&["a", "b"]));
        c.apply(&[p("a", 0, 2e6), p("b", 0, 1e6)]);
        for _ in 0..SIZE {
            c.pop_newest(true, &none());
        }
        assert!(c.pop_newest(true, &none()).is_none());
        assert_eq!(c.pages_to_fetch(&none()), HashMap::from([("a".into(), 1)]));
        let skip = HashSet::from(["a".to_string()]);
        assert_eq!(c.pop_newest(true, &skip).unwrap().video_title, "b-0");
    }

    #[test]
    fn failure_retries_and_short_page_exhausts() {
        let mut c = VodSubscriptionFeedCursor::new(&ids(&["a", "b"]));
        c.apply(&[page("a", 0, SIZE, 1e6, true), page("b", 0, 3, 9e5, false)]);
        assert_eq!(c.pages_to_fetch(&none()), HashMap::from([("a".into(), 0)]));
        assert_eq!(c.active(), ["a".to_string()]);
        c.apply(&[p("a", 0, 1e6)]);
        c.apply(&[p("a", 0, 1e6)]);
        assert_eq!(c.pending()["a"].len(), SIZE);
        assert!(c.has_more());
    }

    #[test]
    fn seed_computes_next_pages() {
        let mut full = p("full", 0, 3e6).items;
        full.extend(p("full", 1, 3e6).items);
        full.reverse();
        let mut grown = p("grown", 0, 2e6).items;
        grown.push(p("grown", 1, 2e6).items[0].clone());
        let seed = HashMap::from([
            ("full".to_string(), full),
            ("grown".to_string(), grown),
            ("short".to_string(), page("short", 0, 3, 1e6, false).items),
            ("done".to_string(), p("done", 0, 5e5).items),
        ]);
        let mut c = VodSubscriptionFeedCursor::seeded(
            &seed,
            &ids(&["full", "grown", "short", "unsynced", "done"]),
            SIZE,
            &HashSet::from(["done".to_string()]),
        );
        assert_eq!(c.active(), ids(&["full", "grown"]));
        assert_eq!(c.last_page()["full"], 1);
        assert_eq!(c.last_page()["grown"], 0);
        assert!(!c.pending().contains_key("unsynced"));
        assert_eq!(c.pending()["full"][0].video_title, "full-0");
        assert!(c.pages_to_fetch(&none()).is_empty());
        while c.pop_newest(false, &none()).is_some() {}
        assert_eq!(
            c.pages_to_fetch(&none()),
            HashMap::from([("full".into(), 2), ("grown".into(), 1)])
        );
    }

    #[test]
    fn overlapping_page_is_deduplicated() {
        let mut grown = p("g", 0, 1e6).items;
        grown.push(p("g", 1, 1e6).items[0].clone());
        let mut c = VodSubscriptionFeedCursor::seeded(
            &HashMap::from([("g".to_string(), grown)]),
            &ids(&["g"]),
            SIZE,
            &none(),
        );
        let mut titles = Vec::new();
        while let Some(v) = c.pop_newest(true, &none()) {
            titles.push(v.video_title);
        }
        assert_eq!(c.pages_to_fetch(&none()), HashMap::from([("g".into(), 1)]));
        c.apply(&[p("g", 1, 1e6)]);
        while let Some(v) = c.pop_newest(false, &none()) {
            titles.push(v.video_title);
        }
        assert_eq!(
            titles,
            (0..16).map(|i| format!("g-{i}")).collect::<Vec<_>>()
        );
    }

    #[test]
    fn merged_local_rows_follow_store_rules() {
        let mut rows = p("a", 0, 1e6).items;
        rows.extend(p("a", 1, 1e6).items);
        let refreshed = rows[1..9].to_vec();
        let merged = VodSubscriptionFeedCursor::merged_local_rows(&rows, 0, &refreshed, false, 24);
        let titles: Vec<_> = merged.iter().map(|v| v.video_title.clone()).collect();
        assert_eq!(
            titles,
            (1..16).map(|i| format!("a-{i}")).collect::<Vec<_>>()
        );

        let appended =
            VodSubscriptionFeedCursor::merged_local_rows(&rows[..8], 1, &rows[8..], false, 24);
        assert_eq!(appended.len(), 16);
        assert!(VodSubscriptionFeedCursor::merged_local_rows(&rows, 0, &[], true, 24).is_empty());
        let capped = VodSubscriptionFeedCursor::merged_local_rows(&rows, 1, &rows, false, 10);
        let titles: Vec<_> = capped.iter().map(|v| v.video_title.clone()).collect();
        assert_eq!(
            titles,
            (0..10).map(|i| format!("a-{i}")).collect::<Vec<_>>()
        );
    }

    struct Probe {
        current: AtomicUsize,
        max_seen: AtomicUsize,
        calls: AtomicUsize,
        fail: bool,
    }

    #[async_trait]
    impl VodPageSource for Probe {
        async fn vod_page(&self, _: &str, _: i64, _: i64) -> Result<Vec<VodInfo>, ApiError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let now = self.current.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_seen.fetch_max(now, Ordering::SeqCst);
            tokio::task::yield_now().await;
            tokio::task::yield_now().await;
            self.current.fetch_sub(1, Ordering::SeqCst);
            if self.fail {
                Err(ApiError::NetworkError("offline".into()))
            } else {
                Ok(Vec::new())
            }
        }
    }

    fn probe(fail: bool) -> Arc<Probe> {
        Arc::new(Probe {
            current: AtomicUsize::new(0),
            max_seen: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
            fail,
        })
    }

    #[tokio::test]
    async fn fanout_keeps_concurrency_limit_and_covers_every_channel() {
        let pr = probe(false);
        let agg = VodFeedAggregator::new(pr.clone());
        let channels: Vec<String> = (0..20).map(|i| format!("c{i}")).collect();
        let results = agg
            .fetch_pages(&channels, &HashMap::new(), 8, 6, usize::MAX)
            .await;
        assert_eq!(results.len(), 20);
        assert!(pr.max_seen.load(Ordering::SeqCst) <= 6);
        assert!(results.iter().all(|r| !r.has_more()), "빈 성공은 소진");
    }

    #[tokio::test]
    async fn breaker_stops_new_requests_when_offline() {
        let pr = probe(true);
        let agg = VodFeedAggregator::new(pr.clone());
        let channels: Vec<String> = (0..50).map(|i| format!("c{i}")).collect();
        let results = agg.fetch_pages(&channels, &HashMap::new(), 8, 2, 3).await;
        assert!(results.len() < 50);
        assert!(pr.calls.load(Ordering::SeqCst) <= 3 + 2 - 1 + 1);
        assert!(results
            .iter()
            .all(|r| r.failed && r.failure_kind == Some(VodFailureKind::Offline) && r.has_more()));
    }

    #[test]
    fn classify_failure_kinds() {
        assert_eq!(
            ChannelPageResult::classify_failure(&ApiError::RateLimited { retry_after: 5.0 }),
            VodFailureKind::RateLimited(5.0)
        );
        assert_eq!(
            ChannelPageResult::classify_failure(&ApiError::NetworkError("API fixture miss".into())),
            VodFailureKind::Other
        );
        assert_eq!(
            ChannelPageResult::classify_failure(&ApiError::HttpError { status_code: 500 }),
            VodFailureKind::Other
        );
    }
}
