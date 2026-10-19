//! `ChzzkAPIClient.swift`(편의 메서드) · `ChzzkAPIClient+Content.swift` · `ChzzkAPIClient+Donation.swift` 포팅 —
//! 채널·검색·VOD·클립·이모티콘·후원 API.
//!
//! 채팅 모델(이모티콘·후원 랭킹 등)은 별도 포팅(`cview_core::models::chat`)이 소유하므로, 그 타입을
//! 돌려주는 메서드는 응답 타입을 제네릭 `T` 로 받는다.
//! TODO(chat-models): `resolveEmoticonPacks` · `basicEmoticonPacks(channelId:)` 는 `EmoticonPack`/
//! `EmoticonDeploy.allPacks` 가 필요해 아직 포팅하지 않았다.

use cview_core::error::ApiError;
use cview_core::models::clip::{ClipInkeyContent, NaverVodPlayResponse};
use cview_core::models::{
    ChannelInfo, ChannelSearchItem, ChannelViewerInfo, ClipDetail, ClipInfo, LiveInfo,
    LiveSearchItem, SearchResult, TimeMachinePlaybackInfo, VideoSearchItem, VodDetail, VodInfo,
};
use cview_core::protocols::{HttpCookie, HttpMethod};
use url::Url;

use crate::client::{
    transport_to_api_error, AuthContext, ChzzkApiClient, RequestOptions, ResponseBody,
};
use crate::constants::api_defaults;
use crate::endpoint::ChzzkEndpoint;
use crate::headers;
use crate::response::{ChzzkResponse, FollowingContent, PagedContent};
use crate::transport::HttpRequest;

/// 이모티콘 endpoint family (404 negative cache 키).
pub mod emoticon_family {
    pub const EMOTICON_DEPLOY: &str = "emoticon-deploy";
    pub const BASIC_EMOTICONS: &str = "basic-emoticons";
    pub const USER_EMOTICONS: &str = "user-emoticons";
}

impl ChzzkApiClient {
    // MARK: - 채널 · 라이브

    /// 채널 정보 조회.
    pub async fn channel_info(&self, channel_id: &str) -> Result<ChannelInfo, ApiError> {
        self.request(&ChzzkEndpoint::ChannelInfo {
            channel_id: channel_id.into(),
        })
        .await
    }

    /// 현재 시청자의 채널별 권한 정보(비로그인은 GUEST).
    pub async fn channel_viewer_info(
        &self,
        channel_id: &str,
    ) -> Result<ChannelViewerInfo, ApiError> {
        self.request(&ChzzkEndpoint::ChannelViewerInfo {
            channel_id: channel_id.into(),
        })
        .await
    }

    /// 라이브 상세 조회. `bypass_cache` 는 알림 폴·수동 새로고침이 넘긴다.
    pub async fn live_detail(
        &self,
        channel_id: &str,
        bypass_cache: bool,
    ) -> Result<LiveInfo, ApiError> {
        self.request_bypass(
            &ChzzkEndpoint::LiveDetail {
                channel_id: channel_id.into(),
            },
            bypass_cache,
        )
        .await
    }

    /// 치트키 타임머신용 1시간 DVR 재생 JSON. 호출자는 `is_time_machine_playback` 을 확인해야 한다.
    pub async fn time_machine_playback(
        &self,
        channel_id: &str,
    ) -> Result<TimeMachinePlaybackInfo, ApiError> {
        self.request(&ChzzkEndpoint::TimeMachinePlayback {
            channel_id: channel_id.into(),
        })
        .await
    }

    /// 팔로잉 목록 한 페이지.
    pub async fn following(
        &self,
        size: i64,
        page: i64,
        bypass_cache: bool,
    ) -> Result<FollowingContent, ApiError> {
        self.request_bypass(&ChzzkEndpoint::Following { size, page }, bypass_cache)
            .await
    }

    /// 팔로우 (POST).
    pub async fn follow(&self, channel_id: &str) -> Result<(), ApiError> {
        self.request_raw(&ChzzkEndpoint::Follow {
            channel_id: channel_id.into(),
        })
        .await
    }

    /// 언팔로우 (DELETE).
    pub async fn unfollow(&self, channel_id: &str) -> Result<(), ApiError> {
        self.request_raw(&ChzzkEndpoint::Unfollow {
            channel_id: channel_id.into(),
        })
        .await
    }

    // MARK: - 검색

    /// 채널 검색.
    pub async fn search_channels(
        &self,
        keyword: &str,
        offset: i64,
        size: i64,
    ) -> Result<SearchResult<ChannelInfo>, ApiError> {
        let raw: SearchResult<ChannelSearchItem> = self
            .request(&ChzzkEndpoint::SearchChannel {
                keyword: keyword.into(),
                offset,
                size,
            })
            .await?;
        Ok(raw.map(|i| i.channel))
    }

    /// 라이브 검색.
    pub async fn search_lives(
        &self,
        keyword: &str,
        offset: i64,
        size: i64,
    ) -> Result<SearchResult<LiveInfo>, ApiError> {
        let raw: SearchResult<LiveSearchItem> = self
            .request(&ChzzkEndpoint::SearchLive {
                keyword: keyword.into(),
                offset,
                size,
            })
            .await?;
        Ok(raw.map(|i| i.live))
    }

    /// 비디오 검색.
    pub async fn search_videos(
        &self,
        keyword: &str,
        offset: i64,
        size: i64,
    ) -> Result<SearchResult<VodInfo>, ApiError> {
        let raw: SearchResult<VideoSearchItem> = self
            .request(&ChzzkEndpoint::SearchVideo {
                keyword: keyword.into(),
                offset,
                size,
            })
            .await?;
        Ok(raw.map(|i| i.video))
    }

    // MARK: - VOD · 클립

    /// VOD 목록. `store_in_cache = false` 는 팬아웃처럼 재사용되지 않는 응답용.
    pub async fn vod_list(
        &self,
        channel_id: &str,
        page: i64,
        size: i64,
        store_in_cache: bool,
    ) -> Result<PagedContent<VodInfo>, ApiError> {
        self.request_opts(
            &ChzzkEndpoint::VodList {
                channel_id: channel_id.into(),
                page,
                size,
            },
            RequestOptions::new(false, store_in_cache),
        )
        .await
    }

    /// VOD 상세.
    pub async fn vod_detail(&self, video_no: i64) -> Result<VodDetail, ApiError> {
        self.request(&ChzzkEndpoint::VodDetail { video_no }).await
    }

    /// 클립 목록.
    pub async fn clip_list(
        &self,
        channel_id: &str,
        page: i64,
        size: i64,
    ) -> Result<PagedContent<ClipInfo>, ApiError> {
        self.request(&ChzzkEndpoint::ClipList {
            channel_id: channel_id.into(),
            page,
            size,
        })
        .await
    }

    /// 홈 추천/인기 클립. `filter_type`: WITHIN_1_DAY / WITHIN_7_DAYS / WITHIN_30_DAYS / ALL_TIME,
    /// `order_type`: POPULAR / RECOMMEND.
    pub async fn home_popular_clips(
        &self,
        filter_type: &str,
        order_type: &str,
    ) -> Result<Vec<ClipInfo>, ApiError> {
        let paged: PagedContent<ClipInfo> = self
            .request(&ChzzkEndpoint::HomePopularClips {
                filter_type: filter_type.into(),
                order_type: order_type.into(),
            })
            .await?;
        Ok(paged.data)
    }

    /// 클립 상세.
    pub async fn clip_detail(&self, clip_uid: &str) -> Result<ClipDetail, ApiError> {
        self.request(&ChzzkEndpoint::ClipDetail {
            clip_uid: clip_uid.into(),
        })
        .await
    }

    /// 클립 HLS 스트림 URL (inkey → Naver VOD play API).
    ///
    /// 1. `POST /service/v1/clips/{uid}/inkey` → inKey
    /// 2. `GET apis.naver.com/rmcnmv/rmcnmv/vod/play/v2.0/{videoId}?key={inKey}&serviceId=nng_chzzk_clip&cc=kr`
    /// 3. HLS masterPlaylistUrl 추출
    ///
    /// 원본은 공유 쿠키 저장소의 NID 쿠키를 먼저 쓴다. Rust 포팅은 인증 제공자 쿠키 중 NID 쿠키를,
    /// 없으면 제공자 쿠키 전체를 붙인다. 쿠키가 없어도 시도한다.
    pub async fn clip_stream_url(&self, clip_uid: &str, video_id: &str) -> Result<Url, ApiError> {
        let inkey_url = Url::parse(&format!(
            "https://api.chzzk.naver.com/service/v1/clips/{clip_uid}/inkey"
        ))
        .map_err(|_| ApiError::NetworkError("Invalid inkey URL".into()))?;
        let mut inkey = HttpRequest::new(HttpMethod::Post, inkey_url);
        inkey.body = Some(b"{}".to_vec());
        inkey.timeout = Some(std::time::Duration::from_secs_f64(
            api_defaults::CLIP_INKEY_TIMEOUT,
        ));
        inkey.set_header("Content-Type", Some("application/json"));
        inkey.set_header("Referer", Some(headers::API_REFERER));
        inkey.set_header("Origin", Some(headers::CHZZK_ORIGIN));
        inkey.set_header("User-Agent", Some(headers::CHROME_USER_AGENT));
        if let Some(cookies) = self.auth_cookies().await {
            let nid: Vec<HttpCookie> = cookies
                .iter()
                .filter(|c| c.name == "NID_AUT" || c.name == "NID_SES")
                .cloned()
                .collect();
            let chosen = if nid.is_empty() { cookies } else { nid };
            inkey.set_header("Cookie", HttpCookie::header_value(&chosen).as_deref());
        } else {
            tracing::warn!("Clip inkey: no cookies found — trying without auth");
        }

        let response = self
            .send(inkey, None)
            .await
            .map_err(transport_to_api_error)?;
        if response.status != 200 {
            tracing::error!("Clip inkey HTTP {}", response.status);
            return Err(ApiError::http(response.status as i64));
        }
        let parsed: ChzzkResponse<ClipInkeyContent> = serde_json::from_slice(&response.body)
            .map_err(|e| ApiError::DecodingFailed(e.to_string()))?;
        let in_key = match parsed.content.map(|c| c.in_key) {
            Some(k) if !k.is_empty() => k,
            _ => {
                tracing::error!("Clip inkey: content nil or empty (code={})", parsed.code);
                return Err(ApiError::EmptyContent);
            }
        };

        let vod_url = Url::parse(&format!(
            "https://apis.naver.com/rmcnmv/rmcnmv/vod/play/v2.0/{video_id}?key={in_key}&serviceId=nng_chzzk_clip&cc=kr"
        ))
        .map_err(|_| ApiError::NetworkError("Invalid VOD URL".into()))?;
        let mut vod = HttpRequest::new(HttpMethod::Get, vod_url);
        vod.set_header("Referer", Some(headers::API_REFERER));
        vod.set_header("Origin", Some(headers::CHZZK_ORIGIN));
        let response = self.send(vod, None).await.map_err(transport_to_api_error)?;
        if response.status != 200 {
            return Err(ApiError::http(response.status as i64));
        }
        let play: NaverVodPlayResponse = serde_json::from_slice(&response.body)
            .map_err(|e| ApiError::DecodingFailed(e.to_string()))?;
        if let Some(code) = &play.error_code {
            tracing::error!("Naver VOD error: {code}");
            return Err(ApiError::NetworkError(format!("VOD error: {code}")));
        }
        play.best_hls_url()
            .and_then(|s| Url::parse(&s).ok())
            .ok_or(ApiError::EmptyContent)
    }

    // MARK: - 이모티콘 (404 negative cache)

    pub(crate) async fn is_emoticon_family_unsupported(
        &self,
        family: &str,
        context: Option<AuthContext>,
    ) -> bool {
        let context = match context {
            Some(c) => c,
            None => self.auth_context().await,
        };
        if self.check_auth_context(context).await.is_err() {
            return false;
        }
        let mut st = self.lock_state();
        if st.unsupported_emoticon_context != Some(context) {
            st.unsupported_emoticon_families.clear();
            st.unsupported_emoticon_context = Some(context);
        }
        st.unsupported_emoticon_families.contains(family)
    }

    pub(crate) async fn mark_emoticon_family_unsupported(
        &self,
        family: &str,
        context: Option<AuthContext>,
    ) {
        let context = match context {
            Some(c) => c,
            None => self.auth_context().await,
        };
        if self.check_auth_context(context).await.is_err() {
            return;
        }
        let mut st = self.lock_state();
        if st.unsupported_emoticon_context != Some(context) {
            st.unsupported_emoticon_families.clear();
            st.unsupported_emoticon_context = Some(context);
        }
        if st.unsupported_emoticon_families.insert(family.to_string()) {
            tracing::info!("Emoticon endpoint unsupported (404, will skip): {family}");
        }
    }

    /// 지금 로그인 세대에서 이 family 가 404 로 확인됐는가.
    pub async fn emoticon_family_unsupported(&self, family: &str) -> bool {
        self.is_emoticon_family_unsupported(family, None).await
    }

    async fn emoticon_request<T: ResponseBody>(
        &self,
        family: &str,
        endpoint: ChzzkEndpoint,
    ) -> Result<T, ApiError> {
        let context = self.auth_context().await;
        if self
            .is_emoticon_family_unsupported(family, Some(context))
            .await
        {
            return Err(ApiError::http(404));
        }
        match self.request::<T>(&endpoint).await {
            Err(ApiError::HttpError { status_code: 404 }) => {
                self.mark_emoticon_family_unsupported(family, Some(context))
                    .await;
                Err(ApiError::http(404))
            }
            other => other,
        }
    }

    /// 채널 이모티콘 배포 정보 (`T` = `EmoticonDeploy`).
    pub async fn emoticon_deploy<T: ResponseBody>(&self, channel_id: &str) -> Result<T, ApiError> {
        self.emoticon_request(
            emoticon_family::EMOTICON_DEPLOY,
            ChzzkEndpoint::EmoticonDeploy {
                channel_id: channel_id.into(),
            },
        )
        .await
    }

    /// 이모티콘 팩 상세 (`T` = `EmoticonPack`).
    pub async fn emoticon_pack<T: ResponseBody>(&self, pack_id: &str) -> Result<T, ApiError> {
        self.request(&ChzzkEndpoint::EmoticonPack {
            pack_id: pack_id.into(),
        })
        .await
    }

    /// 사용자가 쓸 수 있는 전체 이모티콘 (인증 필요, `T` = `EmoticonDeploy`).
    pub async fn user_emoticons<T: ResponseBody>(&self) -> Result<T, ApiError> {
        self.emoticon_request(
            emoticon_family::USER_EMOTICONS,
            ChzzkEndpoint::UserEmoticons,
        )
        .await
    }

    /// 치지직 글로벌 기본 이모티콘 (`/service/v1/emoticons`, `T` = `EmoticonDeploy`).
    /// 원본은 실패 시 빈 배열을 돌려준다 — 여기서는 `None`.
    pub async fn global_basic_emoticons<T: ResponseBody>(&self) -> Option<T> {
        match self
            .emoticon_request::<T>(
                emoticon_family::BASIC_EMOTICONS,
                ChzzkEndpoint::BasicEmoticonPacks,
            )
            .await
        {
            Ok(v) => Some(v),
            Err(e) => {
                tracing::info!("globalBasicEmoticons: API 미지원 ({e:?})");
                None
            }
        }
    }

    // MARK: - 후원

    /// 채널의 주간 후원 랭킹 (`T` = `DonationRankResponse`).
    pub async fn weekly_donation_ranking<T: ResponseBody>(
        &self,
        channel_id: &str,
        rank_count: i64,
        with_my_rank: bool,
    ) -> Result<T, ApiError> {
        self.request(&ChzzkEndpoint::WeeklyDonationRanking {
            channel_id: channel_id.into(),
            rank_count,
            with_my_rank,
        })
        .await
    }
}
