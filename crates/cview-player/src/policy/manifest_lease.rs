//! `Sources/CViewPlayer/ManifestLease.swift` 포팅 — HLS 매니페스트 / variant URL lease 모델.

use chrono::{DateTime, Utc};
use url::Url;

/// lease 가 만들어진 출처.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ManifestLeaseSource {
    /// `start_stream` 초기 master fetch.
    Initial,
    /// 주기적 manifest refresh.
    PeriodicRefresh,
    /// 재연결 직전 변환.
    ReconnectPrep,
    /// hover prefetch 로 받은 manifest.
    Prefetch,
}

/// 현재 재생/연결에 사용되는 매니페스트와 선택된 variant URL 의 lease.
#[derive(Debug, Clone, PartialEq)]
pub struct ManifestLease {
    pub master_url: Url,
    pub selected_variant_url: Url,
    pub fetched_at: DateTime<Utc>,
    /// `None` 이면 만료 정보 없음.
    pub expires_at: Option<DateTime<Utc>>,
    /// 단조 증가 세대 — coordinator 내부에서 발급.
    pub generation: i64,
    pub source: ManifestLeaseSource,
}

impl ManifestLease {
    pub fn new(
        master_url: Url,
        selected_variant_url: Url,
        generation: i64,
        source: ManifestLeaseSource,
    ) -> Self {
        Self {
            master_url,
            selected_variant_url,
            fetched_at: Utc::now(),
            expires_at: None,
            generation,
            source,
        }
    }

    /// `now` 기준으로 fresh 한지.
    pub fn is_fresh_at(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.is_none_or(|e| now < e)
    }

    pub fn is_fresh(&self) -> bool {
        self.is_fresh_at(Utc::now())
    }

    /// 동일 master / selectedVariant URL 인지 (generation 무시).
    pub fn same_urls(&self, other: &ManifestLease) -> bool {
        self.master_url == other.master_url
            && self.selected_variant_url == other.selected_variant_url
    }
}

#[cfg(test)]
mod tests {
    //! `Tests/CViewPlayerTests/ManifestLeaseTests.swift` 포팅.
    use super::*;
    use chrono::Duration;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }
    fn lease(variant: &str, generation: i64, source: ManifestLeaseSource) -> ManifestLease {
        ManifestLease::new(
            u("https://cdn.example.com/live/master.m3u8"),
            u(variant),
            generation,
            source,
        )
    }

    #[test]
    fn freshness() {
        let mut l = lease(
            "https://cdn.example.com/live/v1080.m3u8",
            1,
            ManifestLeaseSource::Initial,
        );
        l.fetched_at = Utc::now() - Duration::seconds(3600);
        assert!(l.is_fresh());
        l.expires_at = Some(Utc::now() - Duration::seconds(60));
        assert!(!l.is_fresh());
        l.expires_at = Some(Utc::now() + Duration::seconds(300));
        assert!(l.is_fresh());
    }

    #[test]
    fn same_urls_ignores_generation() {
        let a = lease(
            "https://cdn.example.com/live/v1080.m3u8",
            1,
            ManifestLeaseSource::Initial,
        );
        let b = lease(
            "https://cdn.example.com/live/v1080.m3u8",
            42,
            ManifestLeaseSource::PeriodicRefresh,
        );
        let c = lease(
            "https://cdn.example.com/live/v720.m3u8",
            2,
            ManifestLeaseSource::PeriodicRefresh,
        );
        assert!(a.same_urls(&b));
        assert!(!a.same_urls(&c));
        assert_ne!(
            ManifestLeaseSource::PeriodicRefresh,
            ManifestLeaseSource::ReconnectPrep
        );
        assert_ne!(ManifestLeaseSource::Prefetch, ManifestLeaseSource::Initial);
    }
}
