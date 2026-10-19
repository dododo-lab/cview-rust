//! `Sources/CViewNetworking/ChzzkAPIClient+LiveCollection.swift` 의 `AllLivesCollector` ·
//! `StableCursorAllLivesVerifier` 포팅 — 치지직 전체 라이브 커서 수집기와 그 검증기.
//!
//! 네트워크 호출을 클로저로 분리해 실제 HTTP 없이 회귀 테스트한다. 정상 완료 판정은 명시적인
//! terminal cursor 뿐이다. 취소는 `fetch_page` 가 [`ApiError::Cancelled`] 를 돌려주거나 호출자가
//! future 를 drop 하는 것으로 표현한다(Swift `Task.isCancelled` 대응).

use std::collections::HashSet;
use std::future::Future;
use std::time::Duration;

use cview_core::error::ApiError;
use cview_core::models::LiveInfo;

use crate::response::{
    AllLivesCollectionResult, AllLivesProgress, AllLivesResumePoint, AllLivesStopReason,
    LivePageCursor, NextMetadataState, PageMetadataState, PagedContent,
};

/// 진행률 콜백 디바운스 간격(페이지) — Swift `PerfTuning.Networking.progressCallbackPageInterval`.
pub const PROGRESS_CALLBACK_PAGE_INTERVAL: usize = 5;

/// 수집 옵션.
#[derive(Debug, Clone)]
pub struct CollectorOptions {
    pub max_pages: usize,
    pub retry_delay: Duration,
    pub page_delay: Duration,
    /// `LATEST` 계약(liveId 엄격 내림차순 + `page.next.liveId == 페이지 마지막 ID`)을 매 응답에서 검증한다.
    pub validate_descending_live_id_cursor: bool,
    /// 이어 받을 지점 (NET-4).
    pub start: Option<AllLivesResumePoint>,
    /// 이미 가진 liveId — 겹치면 `deduplicated_count` 로 잡힌다.
    pub seed_live_ids: HashSet<i64>,
    /// 이 값 이하 원소를 만나면 그 앞까지만 받고 `ReachedBoundary` 로 끝낸다.
    pub boundary_live_id: Option<i64>,
}

impl CollectorOptions {
    pub fn new(max_pages: usize) -> Self {
        Self {
            max_pages,
            retry_delay: Duration::from_millis(300),
            page_delay: Duration::from_millis(50),
            validate_descending_live_id_cursor: false,
            start: None,
            seed_live_ids: HashSet::new(),
            boundary_live_id: None,
        }
    }
}

fn make_resume_point(
    cursor: Option<LivePageCursor>,
    last_ordered: Option<i64>,
    already_received: usize,
    received: usize,
) -> AllLivesResumePoint {
    AllLivesResumePoint {
        cursor_concurrent_user_count: cursor.and_then(|c| c.concurrent_user_count),
        cursor_live_id: cursor.and_then(|c| c.live_id),
        last_ordered_live_id: last_ordered,
        received_pages: already_received + received,
    }
}

/// 전체 라이브를 커서 끝까지 수집한다.
///
/// - `fetch_page`: 커서로 한 페이지를 받는다.
/// - `should_pause`: 페이지 사이(cursor 전진 뒤, 대기 전)에 묻는다. true 면 `Paused` + 재개 지점.
/// - `on_progress`: 첫 페이지, 이후 [`PROGRESS_CALLBACK_PAGE_INTERVAL`] 마다, 마지막 페이지에 호출.
pub async fn collect<F, Fut, P, PFut, G>(
    options: CollectorOptions,
    fetch_page: F,
    should_pause: P,
    on_progress: G,
) -> AllLivesCollectionResult
where
    F: Fn(Option<LivePageCursor>) -> Fut,
    Fut: Future<Output = Result<PagedContent<LiveInfo>, ApiError>>,
    P: Fn() -> PFut,
    PFut: Future<Output = bool>,
    G: Fn(AllLivesProgress),
{
    // 재개하면 이미 받은 페이지만큼 상한을 줄여, 조각을 이어 붙여도 전체 상한이 유지된다.
    let already = options.start.map(|s| s.received_pages).unwrap_or(0);
    let safety_max_pages = options.max_pages.max(1).saturating_sub(already).max(1);
    let mut all: Vec<LiveInfo> = Vec::new();
    let mut seen_ids = options.seed_live_ids.clone();
    let mut seen_cursors: HashSet<LivePageCursor> = HashSet::new();
    let mut cursor: Option<LivePageCursor> = options.start.and_then(|s| s.cursor());
    let mut first_live_id: Option<i64> = None;
    let mut resume_point: Option<AllLivesResumePoint> = None;
    let mut requested_pages = 0usize;
    let mut received_pages = 0usize;
    let mut raw_item_count = 0usize;
    let mut estimated_total: Option<i64> = None;
    let stop_reason;
    let mut previous_ordered: Option<i64> = options.start.and_then(|s| s.last_ordered_live_id);
    let interval = PROGRESS_CALLBACK_PAGE_INTERVAL;
    let mut last_reported_page: isize = -(interval as isize);

    loop {
        requested_pages += 1;
        let response = match fetch_page(cursor).await {
            Ok(r) => r,
            Err(ApiError::Cancelled) => {
                stop_reason = AllLivesStopReason::Cancelled;
                resume_point = Some(make_resume_point(
                    cursor,
                    previous_ordered,
                    already,
                    received_pages,
                ));
                break;
            }
            Err(_) => {
                // 일시 실패 1회 재시도. 재시도도 실패하면 현재까지의 결과는 partial 이다.
                if !options.retry_delay.is_zero() {
                    tokio::time::sleep(options.retry_delay).await;
                }
                match fetch_page(cursor).await {
                    Ok(r) => r,
                    Err(ApiError::Cancelled) => {
                        stop_reason = AllLivesStopReason::Cancelled;
                        resume_point = Some(make_resume_point(
                            cursor,
                            previous_ordered,
                            already,
                            received_pages,
                        ));
                        break;
                    }
                    Err(e) => {
                        stop_reason = AllLivesStopReason::PartialFailure {
                            page: requested_pages,
                            message: e.to_string(),
                        };
                        // 네트워크 실패는 같은 cursor 부터 이어 받을 수 있다.
                        resume_point = Some(make_resume_point(
                            cursor,
                            previous_ordered,
                            already,
                            received_pages,
                        ));
                        break;
                    }
                }
            }
        };

        received_pages += 1;
        if estimated_total.is_none() {
            estimated_total = response.total_count;
        }

        let mut invalid_live_id_count = 0usize;
        let mut boundary_hit: Option<i64> = None;
        let mut accepted: Vec<&LiveInfo> = Vec::new();
        for info in &response.data {
            if info.live_id <= 0 {
                invalid_live_id_count += 1;
                continue;
            }
            if let Some(boundary) = options.boundary_live_id {
                if info.live_id <= boundary {
                    boundary_hit = Some(info.live_id);
                    break;
                }
            }
            accepted.push(info);
            if first_live_id.is_none() {
                first_live_id = Some(info.live_id);
            }
            raw_item_count += 1;
            if seen_ids.insert(info.live_id) {
                all.push(info.clone());
            }
        }

        // 관대한 decode 가 0 으로 대체한 식별자도 누락이다 — 하나라도 있으면 이 페이지에서 종료.
        let dropped = response.data_decoding_failure_count + invalid_live_id_count;
        let has_dropped = dropped > 0;

        let mut ordering_violation: Option<&'static str> = None;
        if options.validate_descending_live_id_cursor && !has_dropped {
            for info in &accepted {
                if previous_ordered.is_some_and(|p| info.live_id >= p) {
                    ordering_violation = Some("최신순 liveId가 엄격한 내림차순이 아닙니다.");
                    break;
                }
                previous_ordered = Some(info.live_id);
            }
        }

        let has_decoded_page_metadata =
            response.page_metadata_state == PageMetadataState::Decoded && response.page.is_some();
        // 실 API 는 짧은 마지막 데이터 페이지 뒤 `data: [], page: null` 을 한 번 더 반환한다.
        let has_explicit_terminal = response.page_metadata_state == PageMetadataState::Terminal
            && response.data.is_empty()
            && response.is_size_metadata_decoded
            && response.size == 0;
        let next_state = response.page.as_ref().map(|p| p.next_metadata_state);
        let has_object_terminal =
            has_decoded_page_metadata && next_state == Some(NextMetadataState::Terminal);
        let next_cursor = response.page.as_ref().and_then(|p| p.next);
        let has_decoded_next_cursor = has_decoded_page_metadata
            && next_state == Some(NextMetadataState::Decoded)
            && next_cursor.is_some();
        let cursor_malformed = has_decoded_next_cursor
            && next_cursor
                .is_some_and(|c| c.concurrent_user_count.is_none() || c.live_id.is_none());
        let cursor_exhausted = has_explicit_terminal || has_object_terminal;
        let cursor_repeated = match next_cursor {
            Some(c) if has_decoded_next_cursor && !cursor_malformed => !seen_cursors.insert(c),
            _ => false,
        };
        let reached_limit = received_pages >= safety_max_pages;
        let has_valid_page_metadata =
            has_explicit_terminal || has_object_terminal || has_decoded_next_cursor;
        let pagination_malformed = !has_valid_page_metadata || cursor_malformed;
        if options.validate_descending_live_id_cursor
            && ordering_violation.is_none()
            && boundary_hit.is_none()
            && has_decoded_next_cursor
        {
            let page_last = response.data.last().map(|l| l.live_id);
            if page_last.is_none() || page_last != next_cursor.and_then(|c| c.live_id) {
                ordering_violation = Some("다음 cursor liveId가 페이지 마지막 liveId와 다릅니다.");
            }
        }
        let is_last_page = has_dropped
            || ordering_violation.is_some()
            || boundary_hit.is_some()
            || pagination_malformed
            || cursor_exhausted
            || cursor_repeated
            || reached_limit;

        let page_for_callback = received_pages as isize;
        if page_for_callback == 1
            || page_for_callback - last_reported_page >= interval as isize
            || is_last_page
        {
            last_reported_page = page_for_callback;
            on_progress(AllLivesProgress {
                current_count: all.len(),
                estimated_total,
                current_page: received_pages,
                deduplicated_count: raw_item_count.saturating_sub(all.len()),
            });
        }

        if has_dropped {
            stop_reason = AllLivesStopReason::PartialFailure {
                page: received_pages,
                message: format!("페이지 데이터 {dropped}개를 정확히 식별하지 못했습니다."),
            };
            break;
        }
        if let Some(message) = ordering_violation {
            stop_reason = AllLivesStopReason::PartialFailure {
                page: received_pages,
                message: message.into(),
            };
            break;
        }
        if let Some(live_id) = boundary_hit {
            stop_reason = AllLivesStopReason::ReachedBoundary { live_id };
            break;
        }
        if pagination_malformed {
            let reason = if has_decoded_page_metadata {
                "다음 페이지 커서가 누락되었거나 손상되었습니다."
            } else if response.page_metadata_state == PageMetadataState::Terminal {
                "종료 페이지에 예상하지 못한 데이터가 포함되었습니다."
            } else {
                "페이지 메타데이터가 누락되었거나 손상되었습니다."
            };
            stop_reason = AllLivesStopReason::PartialFailure {
                page: received_pages,
                message: reason.into(),
            };
            break;
        }
        if cursor_exhausted {
            stop_reason = AllLivesStopReason::ReachedEnd;
            break;
        }
        if cursor_repeated {
            stop_reason = AllLivesStopReason::CursorCycle {
                page: received_pages,
            };
            break;
        }
        if reached_limit {
            stop_reason = AllLivesStopReason::ReachedPageLimit;
            break;
        }

        cursor = next_cursor;
        // cursor 를 전진시킨 뒤 확인하므로 재개 지점은 "다음에 받을 페이지"를 가리킨다.
        if should_pause().await {
            stop_reason = AllLivesStopReason::Paused;
            resume_point = Some(make_resume_point(
                cursor,
                previous_ordered,
                already,
                received_pages,
            ));
            break;
        }
        if !options.page_delay.is_zero() {
            tokio::time::sleep(options.page_delay).await;
        }
    }

    let stable_verified = options.validate_descending_live_id_cursor
        && matches!(stop_reason, AllLivesStopReason::ReachedEnd);
    let deduplicated_count = raw_item_count.saturating_sub(all.len());
    AllLivesCollectionResult {
        items: all,
        estimated_total,
        requested_pages,
        received_pages,
        deduplicated_count,
        stop_reason,
        verified_observed_count: None,
        stable_cursor_ordering_verified: stable_verified,
        resume_point,
        first_live_id,
    }
}

/// 최신순 전체 라이브 sweep 의 검증기 (Swift `StableCursorAllLivesVerifier`).
///
/// 명시적 terminal 까지 도달했고 ID 중복·서버 총계 충돌이 없으면 unique 개수를 검증 관측값으로
/// 확정한다. 예상 밖 중복은 `UnstableOrdering` 으로 강등한다.
pub fn verify_stable_cursor(result: AllLivesCollectionResult) -> AllLivesCollectionResult {
    if !result.is_complete() {
        return result;
    }
    let agrees_with_server = result
        .estimated_total
        .is_none_or(|t| t == result.items.len() as i64);
    if !result.stable_cursor_ordering_verified
        || result.deduplicated_count != 0
        || !agrees_with_server
    {
        let duplicate_count = result.deduplicated_count;
        return AllLivesCollectionResult {
            stop_reason: AllLivesStopReason::UnstableOrdering {
                attempts: 1,
                duplicate_count,
            },
            verified_observed_count: None,
            stable_cursor_ordering_verified: false,
            resume_point: None,
            ..result
        };
    }
    let count = result.items.len();
    AllLivesCollectionResult {
        verified_observed_count: Some(count),
        stable_cursor_ordering_verified: true,
        resume_point: None,
        ..result
    }
}

#[cfg(test)]
mod tests {
    //! `AllLivesCollectorTests.swift` 포팅(수집기·재개·검증기 부분).
    use super::*;
    use crate::response::LivePageInfo;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;

    fn cursor(id: i64) -> LivePageCursor {
        LivePageCursor {
            concurrent_user_count: Some(100_000 - id),
            live_id: Some(id),
        }
    }

    fn page(ids: &[i64], next: Option<LivePageCursor>) -> PagedContent<LiveInfo> {
        PagedContent::new(
            50,
            None,
            ids.iter()
                .map(|&i| LiveInfo::new(i, format!("live-{i}")))
                .collect(),
            Some(LivePageInfo::new(next, None)),
        )
    }

    fn terminal_null() -> PagedContent<LiveInfo> {
        PagedContent {
            page_metadata_state: PageMetadataState::Terminal,
            ..PagedContent::new(0, None, vec![], None)
        }
    }

    struct Source {
        pages: Mutex<VecDeque<Result<PagedContent<LiveInfo>, ApiError>>>,
        cursors: Mutex<Vec<Option<LivePageCursor>>>,
    }

    impl Source {
        fn new(pages: Vec<Result<PagedContent<LiveInfo>, ApiError>>) -> Self {
            Self {
                pages: Mutex::new(pages.into()),
                cursors: Mutex::new(vec![]),
            }
        }
        async fn next(
            &self,
            c: Option<LivePageCursor>,
        ) -> Result<PagedContent<LiveInfo>, ApiError> {
            self.cursors.lock().unwrap().push(c);
            self.pages
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Err(ApiError::NetworkError("exhausted".into())))
        }
        fn count(&self) -> usize {
            self.cursors.lock().unwrap().len()
        }
    }

    fn ok_pages(pages: Vec<PagedContent<LiveInfo>>) -> Source {
        Source::new(pages.into_iter().map(Ok).collect())
    }

    fn opts(max_pages: usize, validate: bool) -> CollectorOptions {
        CollectorOptions {
            retry_delay: Duration::ZERO,
            page_delay: Duration::ZERO,
            validate_descending_live_id_cursor: validate,
            ..CollectorOptions::new(max_pages)
        }
    }

    async fn run(source: &Source, options: CollectorOptions) -> AllLivesCollectionResult {
        collect(options, |c| source.next(c), || async { false }, |_| {}).await
    }

    fn ids(r: &AllLivesCollectionResult) -> Vec<i64> {
        r.items.iter().map(|i| i.live_id).collect()
    }

    #[tokio::test]
    async fn continues_beyond_legacy_forty_page_limit() {
        let pages = (0..42)
            .map(|i| page(&[i + 1], if i == 41 { None } else { Some(cursor(i + 1)) }))
            .collect();
        let source = ok_pages(pages);
        let r = run(&source, opts(200, false)).await;
        assert_eq!(r.stop_reason, AllLivesStopReason::ReachedEnd);
        assert_eq!(
            (r.received_pages, r.requested_pages, r.items.len()),
            (42, 42, 42)
        );
        assert_eq!(r.exact_live_count(), None);
        assert!(r.is_complete());
    }

    #[tokio::test]
    async fn short_intermediate_page_continues_until_null_terminal() {
        let source = ok_pages(vec![
            page(&[1, 2], Some(cursor(1))),
            page(&[3], Some(cursor(2))),
            terminal_null(),
        ]);
        let r = run(&source, opts(200, false)).await;
        assert_eq!(r.stop_reason, AllLivesStopReason::ReachedEnd);
        assert_eq!(source.count(), 3);
        assert_eq!(ids(&r), vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn deduplicates_and_detects_cycle() {
        let source = ok_pages(vec![page(&[1, 2], Some(cursor(1))), page(&[2, 3], None)]);
        let r = run(&source, opts(200, false)).await;
        assert_eq!(ids(&r), vec![1, 2, 3]);
        assert_eq!(r.deduplicated_count, 1);

        let source = ok_pages(vec![
            page(&[1], Some(cursor(10))),
            page(&[2], Some(cursor(20))),
            page(&[3], Some(cursor(10))),
        ]);
        let r = run(&source, opts(200, false)).await;
        assert_eq!(r.stop_reason, AllLivesStopReason::CursorCycle { page: 3 });
        assert!(r.is_partial());
    }

    #[tokio::test]
    async fn page_limit_missing_metadata_and_malformed_cursor() {
        let source = ok_pages(
            (0..5)
                .map(|i| page(&[i + 1], Some(cursor(i + 1))))
                .collect(),
        );
        let r = run(&source, opts(3, false)).await;
        assert_eq!(r.stop_reason, AllLivesStopReason::ReachedPageLimit);
        assert_eq!(r.received_pages, 3);

        let missing = PagedContent::new(50, None, vec![LiveInfo::new(1, "a")], None);
        let r = run(&ok_pages(vec![missing]), opts(200, false)).await;
        assert!(matches!(
            r.stop_reason,
            AllLivesStopReason::PartialFailure { page: 1, .. }
        ));

        let bad_cursor = LivePageCursor {
            concurrent_user_count: None,
            live_id: Some(1),
        };
        let r = run(
            &ok_pages(vec![page(&[1], Some(bad_cursor))]),
            opts(200, false),
        )
        .await;
        assert!(matches!(
            r.stop_reason,
            AllLivesStopReason::PartialFailure { .. }
        ));

        // 데이터가 있는 page null 은 terminal 이 아니다.
        let with_data = PagedContent {
            page_metadata_state: PageMetadataState::Terminal,
            ..PagedContent::new(0, None, vec![LiveInfo::new(1, "a")], None)
        };
        let r = run(&ok_pages(vec![with_data]), opts(200, false)).await;
        assert!(r.is_partial());
    }

    #[tokio::test]
    async fn decode_drops_and_zero_ids_are_partial() {
        let mut p = page(&[1, 2], None);
        p.data_decoding_failure_count = 1;
        let r = run(&ok_pages(vec![p]), opts(200, false)).await;
        assert!(matches!(
            r.stop_reason,
            AllLivesStopReason::PartialFailure { page: 1, .. }
        ));
        assert_eq!(ids(&r), vec![1, 2], "유효 원소는 보존");

        let r = run(&ok_pages(vec![page(&[1, 0], None)]), opts(200, false)).await;
        assert!(r.is_partial());

        // size 가 디코드되지 않은 page null 은 terminal 로 승격하지 않는다.
        let undecoded = PagedContent {
            is_size_metadata_decoded: false,
            ..terminal_null()
        };
        let r = run(&ok_pages(vec![undecoded]), opts(200, false)).await;
        assert!(r.is_partial());
    }

    #[tokio::test]
    async fn retries_once_then_partial_with_resume_point() {
        let source = Source::new(vec![
            Ok(page(&[3], Some(cursor(3)))),
            Err(ApiError::http(503)),
            Ok(page(&[2, 1], None)),
        ]);
        let r = run(&source, opts(200, true)).await;
        assert_eq!(r.stop_reason, AllLivesStopReason::ReachedEnd);
        assert_eq!(ids(&r), vec![3, 2, 1]);

        let source = Source::new(vec![
            Ok(page(&[3], Some(cursor(3)))),
            Err(ApiError::http(503)),
            Err(ApiError::http(503)),
        ]);
        let r = run(&source, opts(200, true)).await;
        assert!(matches!(
            r.stop_reason,
            AllLivesStopReason::PartialFailure { page: 2, .. }
        ));
        let rp = r.resume_point.unwrap();
        assert_eq!(rp.cursor(), Some(cursor(3)));
        assert_eq!(rp.last_ordered_live_id, Some(3));
        assert_eq!(rp.received_pages, 1);

        let source = Source::new(vec![Err(ApiError::Cancelled)]);
        let r = run(&source, opts(200, true)).await;
        assert_eq!(r.stop_reason, AllLivesStopReason::Cancelled);
        assert!(!r.is_partial() && !r.is_complete());
        assert_eq!(r.resume_point.unwrap().cursor(), None);
    }

    #[tokio::test]
    async fn resume_from_start_and_boundary_checks() {
        let source = ok_pages(vec![page(&[4, 3], Some(cursor(3))), page(&[2, 1], None)]);
        let start = AllLivesResumePoint {
            cursor_concurrent_user_count: cursor(5).concurrent_user_count,
            cursor_live_id: Some(5),
            last_ordered_live_id: Some(5),
            received_pages: 3,
        };
        let r = run(
            &source,
            CollectorOptions {
                start: Some(start),
                ..opts(200, true)
            },
        )
        .await;
        assert_eq!(source.cursors.lock().unwrap()[0], Some(cursor(5)));
        assert_eq!(ids(&r), vec![4, 3, 2, 1]);
        assert!(r.stable_cursor_ordering_verified);
        assert_eq!(r.first_live_id, Some(4));

        // 조각 경계에서 이전 조각의 마지막 ID 이상이 오면 순서 위반, 재개 지점 없음.
        let source = ok_pages(vec![page(&[6, 5], None)]);
        let r = run(
            &source,
            CollectorOptions {
                start: Some(start),
                ..opts(200, true)
            },
        )
        .await;
        assert!(matches!(
            r.stop_reason,
            AllLivesStopReason::PartialFailure { .. }
        ));
        assert!(r.resume_point.is_none());

        // 받은 페이지만큼 상한이 줄어든다.
        let source = ok_pages(
            (0..5)
                .map(|i| page(&[100 - i], Some(cursor(100 - i))))
                .collect(),
        );
        let start2 = AllLivesResumePoint {
            received_pages: 8,
            ..start
        };
        let mut start2 = start2;
        start2.last_ordered_live_id = None;
        let r = run(
            &source,
            CollectorOptions {
                start: Some(start2),
                ..opts(10, true)
            },
        )
        .await;
        assert_eq!(r.stop_reason, AllLivesStopReason::ReachedPageLimit);
        assert_eq!(r.received_pages, 2);
    }

    #[tokio::test]
    async fn seed_and_boundary() {
        let source = ok_pages(vec![page(&[3, 2, 1], None)]);
        let r = run(
            &source,
            CollectorOptions {
                seed_live_ids: [2].into_iter().collect(),
                ..opts(200, true)
            },
        )
        .await;
        assert_eq!(ids(&r), vec![3, 1]);
        assert_eq!(r.deduplicated_count, 1);

        let source = ok_pages(vec![page(&[5, 4, 3, 2], Some(cursor(2)))]);
        let r = run(
            &source,
            CollectorOptions {
                boundary_live_id: Some(3),
                ..opts(200, true)
            },
        )
        .await;
        assert_eq!(
            r.stop_reason,
            AllLivesStopReason::ReachedBoundary { live_id: 3 }
        );
        assert_eq!(ids(&r), vec![5, 4]);
        assert!(!r.is_partial() && !r.is_complete() && r.exact_live_count().is_none());

        let source = ok_pages(vec![page(&[3, 2], Some(cursor(2)))]);
        let r = run(
            &source,
            CollectorOptions {
                boundary_live_id: Some(3),
                ..opts(200, true)
            },
        )
        .await;
        assert!(r.items.is_empty());
        assert_eq!(r.first_live_id, None);
    }

    #[tokio::test]
    async fn pause_and_resume() {
        let source = ok_pages(vec![page(&[4, 3], Some(cursor(3))), page(&[2, 1], None)]);
        let paused = AtomicBool::new(false);
        let r = collect(
            opts(200, true),
            |c| source.next(c),
            || async { !paused.swap(true, Ordering::SeqCst) },
            |_| {},
        )
        .await;
        assert_eq!(r.stop_reason, AllLivesStopReason::Paused);
        let rp = r.resume_point.unwrap();
        assert_eq!(rp.cursor(), Some(cursor(3)));
        assert_eq!(rp.received_pages, 1);
        let rest = run(
            &source,
            CollectorOptions {
                start: Some(rp),
                ..opts(200, true)
            },
        )
        .await;
        assert_eq!(ids(&rest), vec![2, 1]);
        assert_eq!(rest.stop_reason, AllLivesStopReason::ReachedEnd);

        // 마지막 페이지에서는 pause 를 묻지 않는다.
        let asked = AtomicUsize::new(0);
        let source = ok_pages(vec![page(&[1], None)]);
        let r = collect(
            opts(200, true),
            |c| source.next(c),
            || async {
                asked.fetch_add(1, Ordering::SeqCst);
                true
            },
            |_| {},
        )
        .await;
        assert_eq!(r.stop_reason, AllLivesStopReason::ReachedEnd);
        assert_eq!(asked.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn progress_is_debounced() {
        let pages = (0..12)
            .map(|i| page(&[i + 1], if i == 11 { None } else { Some(cursor(i + 1)) }))
            .collect();
        let source = ok_pages(pages);
        let reported = Mutex::new(Vec::new());
        collect(
            opts(200, false),
            |c| source.next(c),
            || async { false },
            |p| reported.lock().unwrap().push(p.current_page),
        )
        .await;
        assert_eq!(*reported.lock().unwrap(), vec![1, 6, 11, 12]);
    }

    #[tokio::test]
    async fn verifier_cases() {
        // clean terminal 한 번으로 확정.
        let source = ok_pages(vec![page(&[3, 2], Some(cursor(2))), page(&[1], None)]);
        let v = verify_stable_cursor(run(&source, opts(200, true)).await);
        assert_eq!(v.exact_live_count(), Some(3));

        // 예상 밖 중복은 unstable.
        let source = ok_pages(vec![page(&[3, 2], Some(cursor(2))), page(&[1], None)]);
        let mut r = run(&source, opts(200, true)).await;
        r.deduplicated_count = 1;
        let v = verify_stable_cursor(r);
        assert_eq!(
            v.stop_reason,
            AllLivesStopReason::UnstableOrdering {
                attempts: 1,
                duplicate_count: 1
            }
        );
        assert!(v.is_partial());

        // 서버 총계 충돌.
        let mut p = page(&[2, 1], None);
        p.total_count = Some(5);
        let v = verify_stable_cursor(run(&ok_pages(vec![p]), opts(200, true)).await);
        assert!(matches!(
            v.stop_reason,
            AllLivesStopReason::UnstableOrdering { .. }
        ));

        // 비terminal 은 원래 사유 보존.
        let v = verify_stable_cursor(
            run(&ok_pages(vec![page(&[1], Some(cursor(1)))]), opts(1, true)).await,
        );
        assert_eq!(v.stop_reason, AllLivesStopReason::ReachedPageLimit);

        // 빈 clean terminal 은 0 개로 검증.
        let v = verify_stable_cursor(run(&ok_pages(vec![terminal_null()]), opts(200, true)).await);
        assert_eq!(v.exact_live_count(), Some(0));

        // liveId 증가 → exact 아님.
        let v =
            verify_stable_cursor(run(&ok_pages(vec![page(&[1, 2], None)]), opts(200, true)).await);
        assert_eq!(v.exact_live_count(), None);

        // next.liveId 가 페이지 마지막과 다름 → exact 아님.
        let source = ok_pages(vec![page(&[5, 4], Some(cursor(3))), page(&[2], None)]);
        let v = verify_stable_cursor(run(&source, opts(200, true)).await);
        assert_eq!(v.exact_live_count(), None);

        // LATEST 계약을 검증하지 않은 clean terminal 은 exact 아님.
        let v =
            verify_stable_cursor(run(&ok_pages(vec![page(&[2, 1], None)]), opts(200, false)).await);
        assert_eq!(v.exact_live_count(), None);
    }
}
