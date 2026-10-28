//! `StreamCoordinator+ManifestRefresh.swift` 포팅 — 마스터 매니페스트 갱신(토큰 리프레시 + variant URL)·
//! 품질 정보 백그라운드 로드.

use std::time::Duration;

use async_trait::async_trait;
use cview_core::utils::media_url_policy;
use cview_networking::headers::{CHZZK_REFERER, SAFARI_USER_AGENT};
use url::Url;

use super::{
    select_1080p60_variant, StreamCoordinator, StreamEvent, StreamPhase, StreamQualityInfo,
};
use crate::manifest::{HlsManifestParser, MasterPlaylist};
use crate::policy::{ManifestLease, ManifestLeaseSource, RecoveryDecision, RecoverySignal};

/// 받은 재생목록.
#[derive(Debug, Clone, PartialEq)]
pub struct FetchedManifest {
    pub body: String,
    /// 리다이렉트를 따라간 최종 응답 URL.
    pub final_url: Option<Url>,
}

/// 마스터 매니페스트 다운로더 (Swift `hlsSession`).
#[async_trait]
pub trait ManifestFetcher: Send + Sync {
    async fn fetch(&self, url: &Url, timeout: Option<Duration>) -> Result<FetchedManifest, String>;
}

/// reqwest 기본 구현 — 쿠키 없음·캐시 없음, 리다이렉트를 따라간다. 요청 10초·전체 30초.
///
/// 이 빌드의 reqwest 에는 압축 해제 기능이 없어 `Accept-Encoding: identity` 로 요청한다
/// (Swift 는 `gzip, deflate` 를 URLSession 이 풀었다).
pub struct HttpManifestFetcher {
    client: reqwest::Client,
}

impl Default for HttpManifestFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpManifestFetcher {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();
        Self { client }
    }

    pub fn with_client(client: reqwest::Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl ManifestFetcher for HttpManifestFetcher {
    async fn fetch(&self, url: &Url, timeout: Option<Duration>) -> Result<FetchedManifest, String> {
        let mut request = self
            .client
            .get(url.clone())
            .header("User-Agent", SAFARI_USER_AGENT)
            .header("Referer", CHZZK_REFERER)
            .header("Accept-Encoding", "identity")
            .header("Cache-Control", "no-cache");
        if let Some(t) = timeout {
            request = request.timeout(t);
        }
        let response = request.send().await.map_err(|e| e.to_string())?;
        let final_url = Some(response.url().clone());
        let body = response.text().await.map_err(|e| e.to_string())?;
        Ok(FetchedManifest { body, final_url })
    }
}

/// variant 기준 URL — 최종 응답 URL 이 미디어 URL 정책을 통과할 때만 그 URL, 아니면 요청 URL.
pub fn variant_base_url(request_url: &Url, response_url: Option<&Url>) -> Url {
    match response_url {
        Some(u) if media_url_policy::allows(u) => u.clone(),
        _ => request_url.clone(),
    }
}

/// 마스터 재생목록을 받아 파싱한다 — 마스터가 아니면(`#EXT-X-STREAM-INF` 없음) `Ok(None)`.
/// 상대 variant URI 는 리다이렉트를 따라간 최종 응답 URL 기준으로 푼다.
pub async fn fetch_stream_variants(
    fetcher: &dyn ManifestFetcher,
    parser: &HlsManifestParser,
    url: &Url,
    timeout: Option<Duration>,
) -> Result<Option<MasterPlaylist>, String> {
    let fetched = fetcher.fetch(url, timeout).await?;
    if !fetched.body.contains("#EXT-X-STREAM-INF") {
        return Ok(None);
    }
    let base = variant_base_url(url, fetched.final_url.as_ref());
    parser
        .parse_master_playlist(&fetched.body, &base)
        .map(Some)
        .map_err(|e| e.to_string())
}

impl StreamCoordinator {
    /// 마스터 매니페스트를 다시 받아 variant URL 을 갱신한다(토큰 리프레시). 오류 phase 에서는 건너뛴다.
    pub async fn refresh_master_manifest(&self) {
        let stream_url = {
            let s = self.st();
            let Some(url) = s.stream_url.clone() else {
                return;
            };
            if matches!(s.phase, StreamPhase::Error(_)) {
                return;
            }
            url
        };
        let result = fetch_stream_variants(
            self.inner.fetcher.as_ref(),
            &self.inner.parser,
            &stream_url,
            Some(Duration::from_secs(5)),
        )
        .await;
        match result {
            Ok(None) => {}
            Ok(Some(fetched)) => self.apply_refreshed_manifest(stream_url, fetched),
            Err(error) => {
                let count = {
                    let mut s = self.st();
                    s.manifest_refresh_fail_count += 1;
                    s.manifest_refresh_fail_count
                };
                let decision = self
                    .handle_recovery_signal(RecoverySignal::ManifestRefreshFailure { count })
                    .await;
                if decision == RecoveryDecision::Reconnect {
                    tracing::warn!(
                        "매니페스트 갱신 {count}회 연속 실패 — 재연결 (recovery supervisor)"
                    );
                    self.st().manifest_refresh_fail_count = 0;
                } else {
                    tracing::debug!("매니페스트 갱신 실패 ({count}/5): {error}");
                }
            }
        }
    }

    fn apply_refreshed_manifest(&self, stream_url: Url, fetched: MasterPlaylist) {
        let mut s = self.st();
        s.master_playlist = Some(fetched.clone());
        s.manifest_refresh_fail_count = 0;
        // 사용자가 화질을 고정했으면 그 화질의 최신 URL 로 갱신한다(1080p 로 덮지 않는다).
        let pinned = s.user_selected_variant.as_ref().and_then(|user| {
            fetched
                .variants
                .iter()
                .find(|v| v.bandwidth == user.bandwidth)
                .cloned()
        });
        let target = match pinned {
            Some(fresh) => {
                s.user_selected_variant = Some(fresh.clone());
                Some(fresh)
            }
            None => {
                let mut sorted: Vec<_> = fetched.variants.iter().collect();
                sorted.sort_by_key(|v| std::cmp::Reverse(v.bandwidth));
                sorted
                    .iter()
                    .find(|v| v.resolution.contains("1080"))
                    .or_else(|| sorted.first())
                    .map(|v| (*v).clone())
            }
        };
        let Some(variant) = target else { return };
        let changed = s.current_variant_url.as_ref() != Some(&variant.uri);
        s.current_variant_url = Some(variant.uri.clone());
        s.current_quality = Some(StreamQualityInfo::from_variant(&variant));
        s.manifest_generation += 1;
        s.current_manifest_lease = Some(ManifestLease::new(
            stream_url,
            variant.uri,
            s.manifest_generation,
            ManifestLeaseSource::PeriodicRefresh,
        ));
        tracing::debug!(
            "매니페스트 갱신: variant URL {} — lease gen={}",
            if changed { "변경됨" } else { "동일" },
            s.manifest_generation
        );
    }

    /// 품질 정보 수집용 백그라운드 파싱(재생에는 영향 없음).
    pub(crate) fn load_manifest_info(&self, url: Url) {
        let weak = self.weak();
        tokio::spawn(async move {
            let Some(this) = StreamCoordinator::from_weak(&weak) else {
                return;
            };
            match fetch_stream_variants(this.inner.fetcher.as_ref(), &this.inner.parser, &url, None)
                .await
            {
                Ok(Some(master)) => this.update_master_playlist(master),
                Ok(None) => {}
                Err(e) => tracing::debug!("Manifest info fetch skipped: {e}"),
            }
        });
    }

    /// 마스터 정보 갱신 — 1080p60 variant 를 현재 품질로 표시한다.
    pub(crate) fn update_master_playlist(&self, master: MasterPlaylist) {
        let best = select_1080p60_variant(&master.variants).map(StreamQualityInfo::from_variant);
        {
            let mut s = self.st();
            s.master_playlist = Some(master);
            if let Some(q) = &best {
                s.current_quality = Some(q.clone());
            }
        }
        if let Some(q) = best {
            self.emit_event(StreamEvent::QualitySelected(q));
        }
    }
}
