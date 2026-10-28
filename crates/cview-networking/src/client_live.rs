//! `Sources/CViewNetworking/ChzzkAPIClient+LiveCollection.swift` 포팅 — 채팅 토큰, 라이브 수집,
//! 사용자 상태(메모 + 공유 요청), 팔로잉 목록.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use cview_core::error::ApiError;
use cview_core::models::chat::ChatAccessToken;
use cview_core::models::live::LiveStatus;
use cview_core::models::{LiveChannelItem, LiveInfo};
use cview_core::protocols::{HttpCookie, HttpMethod, QueryItem};
use tokio::task::JoinSet;

use crate::client::{
    build_url, transport_to_api_error, AgedResponse, ChzzkApiClient, RequestOptions,
    UserStatusFlight,
};
use crate::collector::{self, CollectorOptions};
use crate::constants::api_defaults;
use crate::endpoint::{ChzzkEndpoint, TopLivesSortType};
use crate::headers;
use crate::response::{
    AllLivesCollectionResult, AllLivesProgress, AllLivesResumePoint, ChzzkResponse,
    FollowingContent, FollowingRow, FollowingRowsPage, PagedContent, UserStatusInfo,
};
use crate::transport::HttpRequest;
use crate::wait::GetWait;

/// 팔로잉 목록 페이지 크기.
pub const FOLLOWING_PAGE_SIZE: i64 = 50;
/// 팔로잉 채널 수 상한. 최대 페이지 수는 ceil(이 값 / 페이지 크기)다.
pub const FOLLOWING_ROW_LIMIT: i64 = 5_000;

/// 페이지 크기별 최대 페이지 수 — ceil(5000 / size).
pub fn following_max_pages(page_size: i64) -> i64 {
    let size = page_size.max(1);
    (FOLLOWING_ROW_LIMIT + size - 1) / size
}

/// 전수 순회 옵션 (Swift `liveCatalogTraversal` 인자).
#[derive(Debug, Clone, Default)]
pub struct TraversalOptions {
    pub batch_size: i64,
    pub from: Option<AllLivesResumePoint>,
    pub seed_live_ids: HashSet<i64>,
    pub boundary_live_id: Option<i64>,
}

/// userStatus 공유 작업의 진행 수를 끝(정상·abort)에서 줄인다.
struct UserStatusLoadGuard {
    client: ChzzkApiClient,
    wait: GetWait,
}

impl Drop for UserStatusLoadGuard {
    fn drop(&mut self) {
        self.wait.fail(ApiError::Cancelled);
        let mut st = self.client.lock_state();
        if st
            .user_status_flight
            .as_ref()
            .is_some_and(|f| f.wait.ptr_eq(&self.wait))
        {
            st.user_status_flight = None;
        }
        st.user_status_loads_running = st.user_status_loads_running.saturating_sub(1);
    }
}

impl ChzzkApiClient {
    // MARK: - 채팅 토큰

    /// 채팅 접속 토큰 (game API). 쿠키가 있으면 SEND 권한, 없으면 READ 토큰.
    pub async fn chat_access_token(
        &self,
        chat_channel_id: &str,
    ) -> Result<ChatAccessToken, ApiError> {
        let query = [
            QueryItem::new("channelId", chat_channel_id),
            QueryItem::new("chatType", "STREAMING"),
        ];
        let url = build_url(
            &self.inner.game_api_base_url,
            "/v1/chats/access-token",
            Some(&query),
        )?;
        let mut request = HttpRequest::new(HttpMethod::Get, url);
        request.set_header("User-Agent", Some(headers::CHROME_USER_AGENT));
        request.set_header("Origin", Some(headers::CHZZK_ORIGIN));
        request.set_header("Referer", Some(headers::CHZZK_REFERER));
        if let Some(cookies) = self.auth_cookies().await {
            request.set_header("Cookie", HttpCookie::header_value(&cookies).as_deref());
        }
        let response = self
            .send(request, None)
            .await
            .map_err(transport_to_api_error)?;
        if !(200..=299).contains(&response.status) {
            tracing::error!("Chat token HTTP {}", response.status);
            return Err(ApiError::http(response.status as i64));
        }
        let parsed: ChzzkResponse<ChatAccessToken> = serde_json::from_slice(&response.body)
            .map_err(|e| {
                tracing::error!("Chat token decode error: {e}");
                ApiError::DecodingFailed(e.to_string())
            })?;
        parsed.content.ok_or(ApiError::EmptyContent)
    }

    // MARK: - 라이브 수집

    /// 인기 라이브.
    pub async fn top_lives(
        &self,
        size: i64,
        concurrent_user_count: Option<i64>,
        live_id: Option<i64>,
        bypass_cache: bool,
    ) -> Result<PagedContent<LiveInfo>, ApiError> {
        self.top_lives_with_age(size, concurrent_user_count, live_id, bypass_cache)
            .await
            .map(|a| a.value)
    }

    /// 인기 라이브 — 받은 시각과 stale 여부를 함께 돌려준다.
    pub async fn top_lives_with_age(
        &self,
        size: i64,
        concurrent_user_count: Option<i64>,
        live_id: Option<i64>,
        bypass_cache: bool,
    ) -> Result<AgedResponse<PagedContent<LiveInfo>>, ApiError> {
        self.request_with_age(
            &ChzzkEndpoint::TopLives {
                size,
                concurrent_user_count,
                live_id,
                sort_type: None,
            },
            bypass_cache,
            true,
        )
        .await
    }

    /// 전체 cursor sweep 전용 — 최신순, 캐시 읽기/쓰기 모두 하지 않는다.
    async fn uncached_top_lives(
        &self,
        size: i64,
        concurrent_user_count: Option<i64>,
        live_id: Option<i64>,
    ) -> Result<PagedContent<LiveInfo>, ApiError> {
        self.request_opts(
            &ChzzkEndpoint::TopLives {
                size,
                concurrent_user_count,
                live_id,
                sort_type: Some(TopLivesSortType::Latest),
            },
            RequestOptions::new(true, false),
        )
        .await
    }

    /// 전체 라이브 수집 (진행률 콜백 + 중복 제거 + 완전성 메타데이터).
    pub async fn all_live_channels_progressive(
        &self,
        batch_size: i64,
        on_progress: impl Fn(AllLivesProgress),
    ) -> AllLivesCollectionResult {
        self.live_catalog_traversal(
            TraversalOptions {
                batch_size,
                ..Default::default()
            },
            || async { false },
            on_progress,
        )
        .await
    }

    /// 재개 가능한 전수 순회 (NET-4).
    ///
    /// 정확성 검증은 처음부터 받는 순회(`from == None`, seed 없음)에만 적용한다.
    pub async fn live_catalog_traversal<P, PFut, G>(
        &self,
        options: TraversalOptions,
        should_pause: P,
        on_progress: G,
    ) -> AllLivesCollectionResult
    where
        P: Fn() -> PFut,
        PFut: std::future::Future<Output = bool>,
        G: Fn(AllLivesProgress),
    {
        let batch = options.batch_size.clamp(1, 50);
        let fresh_start = options.from.is_none() && options.seed_live_ids.is_empty();
        let collector_options = CollectorOptions {
            validate_descending_live_id_cursor: true,
            start: options.from,
            seed_live_ids: options.seed_live_ids,
            boundary_live_id: options.boundary_live_id,
            ..CollectorOptions::new(api_defaults::ALL_LIVES_MAX_PAGES)
        };
        let traversal = collector::collect(
            collector_options,
            |cursor| {
                let client = self.clone();
                async move {
                    client
                        .uncached_top_lives(
                            batch,
                            cursor.and_then(|c| c.concurrent_user_count),
                            cursor.and_then(|c| c.live_id),
                        )
                        .await
                }
            },
            should_pause,
            on_progress,
        )
        .await;
        if fresh_start {
            collector::verify_stable_cursor(traversal)
        } else {
            traversal
        }
    }

    /// 라이브 상태 폴링.
    pub async fn live_status(&self, channel_id: &str) -> Result<LiveInfo, ApiError> {
        self.request(&ChzzkEndpoint::LiveStatus {
            channel_id: channel_id.into(),
        })
        .await
    }

    // MARK: - 사용자 상태

    /// 사용자 상태 (game API). 같은 로그인 세대의 중복 호출은 메모와 진행 중 공유 요청으로 한 번만 보낸다.
    /// 공유 요청은 호출자와 떨어진 작업에서 돌아, 호출자 취소는 자기 대기만 푼다.
    pub async fn user_status(&self) -> Result<UserStatusInfo, ApiError> {
        let generation = self.provider_generation().await;
        let ticket = {
            let mut st = self.lock_state();
            if let Some((g, value)) = &st.user_status_memo {
                if *g == generation {
                    return Ok(value.clone());
                }
            }
            match &st.user_status_flight {
                Some(f) if f.generation == generation => f.wait.reserve(),
                _ => {
                    let wait = GetWait::new(None);
                    let ticket = wait.reserve();
                    let client = self.clone();
                    let task_wait = wait.clone();
                    let task = tokio::spawn(async move {
                        let _guard = UserStatusLoadGuard {
                            client: client.clone(),
                            wait: task_wait.clone(),
                        };
                        match client.load_user_status().await {
                            Ok(value) => {
                                client
                                    .finish_user_status_flight(&task_wait, generation, value)
                                    .await
                            }
                            Err(e) => task_wait.fail(e),
                        }
                    });
                    st.user_status_loads_running += 1;
                    st.user_status_flight = Some(UserStatusFlight {
                        generation,
                        wait,
                        task: Some(task),
                    });
                    ticket
                }
            }
        };
        let any = ticket.value(true).await?;
        let typed = any
            .downcast::<UserStatusInfo>()
            .map_err(|_| ApiError::DecodingFailed("공유 응답 타입이 다릅니다".into()))?;
        // 기다리는 사이 로그인 세대가 바뀌었으면 이전 계정의 값이다.
        if self.provider_generation().await != generation {
            return Err(ApiError::Cancelled);
        }
        Ok((*typed).clone())
    }

    /// 세대·flight 신원이 그대로이고 userIdHash 가 있을 때만 메모한다.
    async fn finish_user_status_flight(
        &self,
        wait: &GetWait,
        generation: u64,
        value: UserStatusInfo,
    ) {
        let current = self.provider_generation().await;
        {
            let mut st = self.lock_state();
            let is_current = st
                .user_status_flight
                .as_ref()
                .is_some_and(|f| f.wait.ptr_eq(wait));
            if current == generation && is_current && value.user_id_hash.is_some() {
                st.user_status_memo = Some((generation, value.clone()));
            }
        }
        wait.succeed(Arc::new(value), None);
    }

    async fn load_user_status(&self) -> Result<UserStatusInfo, ApiError> {
        let url = build_url(&self.inner.game_api_base_url, "v1/user/getUserStatus", None)?;
        let mut request = HttpRequest::new(HttpMethod::Get, url);
        request.timeout = Some(Duration::from_secs(10));
        request.set_header("Referer", Some(headers::API_REFERER));
        request.set_header("Origin", Some(headers::CHZZK_ORIGIN));
        if let Some(cookies) = self.auth_cookies().await {
            request.set_header("Cookie", HttpCookie::header_value(&cookies).as_deref());
        }
        let response = self
            .send(request, None)
            .await
            .map_err(transport_to_api_error)?;
        if !(200..=299).contains(&response.status) {
            return Err(ApiError::http(response.status as i64));
        }
        let parsed: ChzzkResponse<UserStatusInfo> = serde_json::from_slice(&response.body)
            .map_err(|e| ApiError::DecodingFailed(e.to_string()))?;
        parsed.content.ok_or(ApiError::EmptyContent)
    }

    /// 진행 중인 userStatus 공유 요청에 매달린 호출자 수.
    pub fn user_status_waiter_count(&self) -> usize {
        self.lock_state()
            .user_status_flight
            .as_ref()
            .map_or(0, |f| f.wait.waiter_count())
    }

    /// 아직 끝나지 않은 userStatus 공유 작업 수.
    pub fn user_status_loads_running(&self) -> usize {
        self.lock_state().user_status_loads_running
    }

    /// userStatus 메모가 있는가.
    pub fn has_user_status_memo(&self) -> bool {
        self.lock_state().user_status_memo.is_some()
    }

    // MARK: - 팔로잉

    /// 팔로잉 목록을 페이지 끝까지 받아 행으로 돌려준다. 라이브 상세는 조회하지 않는다.
    ///
    /// 종료 조건: 빈 페이지 → totalCount 도달 → (totalCount 가 없을 때만) 요청보다 적은 페이지.
    /// 같은 채널이 여러 페이지에 나오면 처음 것만 남긴다. 페이지마다 인증 문맥을 다시 확인한다.
    pub async fn fetch_following_rows(
        &self,
        bypass_cache: bool,
    ) -> Result<FollowingRowsPage, ApiError> {
        self.fetch_following_rows_with_page_size(bypass_cache, FOLLOWING_PAGE_SIZE)
            .await
    }

    /// 페이지 크기 주입점(테스트).
    pub async fn fetch_following_rows_with_page_size(
        &self,
        bypass_cache: bool,
        page_size: i64,
    ) -> Result<FollowingRowsPage, ApiError> {
        let context = self.auth_context().await;
        self.check_auth_context(context).await?;
        let size = page_size.max(1);
        let mut rows: Vec<FollowingRow> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut total_count: Option<i64> = None;
        let mut oldest = None;

        for page in 0..following_max_pages(size) {
            let aged: AgedResponse<FollowingContent> = self
                .request_with_age(&ChzzkEndpoint::Following { size, page }, bypass_cache, true)
                .await?;
            self.check_auth_context(context).await?;
            oldest = Some(match oldest {
                Some(o) if o < aged.fetched_at => o,
                _ => aged.fetched_at,
            });
            if let Some(reported) = aged.value.total_count {
                total_count = Some(reported);
            }
            let items = aged.value.following_list.unwrap_or_default();
            let item_count = items.len() as i64;
            for item in items {
                let Some(channel) = item.channel else {
                    continue;
                };
                let follow = channel.follow_state();
                let Some(channel_id) = channel.channel_id else {
                    continue;
                };
                if !seen.insert(channel_id.clone()) {
                    continue;
                }
                rows.push(FollowingRow {
                    channel_id,
                    verified: channel.verified_mark.unwrap_or(false),
                    follow,
                    channel_name: channel.channel_name.unwrap_or_else(|| "Unknown".into()),
                    channel_image_url: channel.channel_image_url,
                    is_live: item.streamer.is_some_and(|s| s.is_actually_live()),
                    list_live_info: item.live_info,
                });
            }
            if item_count == 0 {
                break;
            }
            match total_count {
                Some(total) => {
                    if seen.len() as i64 >= total {
                        break;
                    }
                }
                None => {
                    if item_count < size {
                        break;
                    }
                }
            }
        }

        self.check_auth_context(context).await?;
        let carries_live_info = rows.iter().any(|r| {
            r.is_live
                && r.list_live_info
                    .as_ref()
                    .is_some_and(|i| i.has_list_fields())
        });
        Ok(FollowingRowsPage {
            rows,
            total_count,
            carries_live_info,
            fetched_at: oldest.unwrap_or_else(|| self.inner.cache.now()),
        })
    }

    /// 모든 팔로잉 채널을 수집하고, 라이브 행의 상세를 슬라이딩 윈도우로 병렬 조회한다.
    ///
    /// 결과는 **API 순서**(라이브 행 먼저, 오프라인 행 뒤)다. 상세가 실패한 행은 목록 값으로 남는다.
    pub async fn fetch_following_channels(
        &self,
        bypass_cache: bool,
        detail_concurrency: usize,
    ) -> Result<Vec<LiveChannelItem>, ApiError> {
        let context = self.auth_context().await;
        self.check_auth_context(context).await?;
        let listed = self.fetch_following_rows(bypass_cache).await?;
        self.check_auth_context(context).await?;

        let mut live_items: VecDeque<(usize, LiveChannelItem)> = VecDeque::new();
        let mut offline_items: Vec<LiveChannelItem> = Vec::new();
        for row in &listed.rows {
            if row.is_live {
                live_items.push_back((live_items.len(), row.list_channel_item()));
            } else {
                offline_items.push(row.list_channel_item());
            }
        }
        let limit = detail_concurrency.max(1);
        let mut collected: Vec<(usize, LiveChannelItem)> = Vec::with_capacity(live_items.len());
        let mut set: JoinSet<(usize, LiveChannelItem)> = JoinSet::new();
        while let Some((index, base)) = live_items.pop_front() {
            if set.len() >= limit {
                if let Some(Ok(done)) = set.join_next().await {
                    collected.push(done);
                }
            }
            let client = self.clone();
            set.spawn(async move {
                match client.live_detail(&base.channel_id, bypass_cache).await {
                    Ok(info) => {
                        // 1차 응답 이후 방송이 종료됐을 수 있으므로 상세 상태를 반영한다.
                        let actually_live = info.status == LiveStatus::Open;
                        let item = LiveChannelItem {
                            id: base.id.clone(),
                            channel_name: base.channel_name.clone(),
                            channel_image_url: base.channel_image_url.clone(),
                            live_title: info.live_title.clone(),
                            viewer_count: info.concurrent_user_count,
                            category_name: info.live_category_value.clone(),
                            category_type: None,
                            thumbnail_url: info.resolved_live_image_url().map(|u| u.to_string()),
                            channel_id: base.channel_id.clone(),
                            is_live: actually_live,
                            open_date: if actually_live { info.open_date } else { None },
                        };
                        (index, item)
                    }
                    Err(e) => {
                        tracing::warn!("라이브 상세 조회 실패: {} — {e:?}", base.channel_id);
                        (index, base)
                    }
                }
            });
        }
        while let Some(done) = set.join_next().await {
            if let Ok(done) = done {
                collected.push(done);
            }
        }
        // 완료 순서가 아니라 API 순서로 돌려준다.
        collected.sort_by_key(|(i, _)| *i);
        self.check_auth_context(context).await?;
        let mut result: Vec<LiveChannelItem> =
            collected.into_iter().map(|(_, item)| item).collect();
        result.extend(offline_items);
        Ok(result)
    }
}
