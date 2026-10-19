//! NAVER 미디어 매니페스트와 CDN 리다이렉트 호스트 정책.
//!
//! 원본: `Sources/CViewCore/Utilities/MediaURLPolicy.swift`
//!
//! 부분 문자열이 아니라 DNS 레이블 단위로 비교한다 — 로컬 프록시가 임의의 네트워크
//! 클라이언트가 되면 안 된다.
//!
//! `url` 크레이트는 기본 포트(443)를 정규화해 `port()` 가 `None` 이 되고, 국제화 도메인을
//! punycode(`xn--`)로 바꾼다. 둘 다 원본 판정을 느슨하게 만들지 않는다.

use url::Url;

const ALLOWED_SUFFIXES: [&str; 4] = ["navercdn.com", "pstatic.net", "naver.com", "akamaized.net"];

/// 소문자 a-z, 숫자, '-' 로만 된 1~63바이트 레이블들인지(앞뒤 '-' 금지), 전체 253바이트 이하인지.
pub(crate) fn is_valid_dns_name(host: &str) -> bool {
    host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

/// `host == domain` 이거나 `host` 가 `.domain` 으로 끝나는가.
pub(crate) fn host_matches(host: &str, domain: &str) -> bool {
    host == domain || host.strip_suffix(domain).is_some_and(|p| p.ends_with('.'))
}

/// 허용 호스트인가(대소문자 무시).
pub fn is_allowed_host(host: &str) -> bool {
    let host = host.to_lowercase();
    is_valid_dns_name(&host) && ALLOWED_SUFFIXES.iter().any(|d| host_matches(&host, d))
}

/// https · 사용자 정보 없음 · 포트 없음/443 · fragment 없음 · 허용 호스트.
pub fn allows(url: &Url) -> bool {
    url.scheme().eq_ignore_ascii_case("https")
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none_or(|p| p == 443)
        && url.fragment().is_none()
        && url.host_str().is_some_and(is_allowed_host)
}

/// 클립 문서로 신뢰하는 URL 인가.
pub fn is_clip_document(url: Option<&Url>) -> bool {
    let Some(url) = url else { return false };
    if !allows(url) {
        return false;
    }
    let Some(host) = url.host_str().map(str::to_lowercase) else {
        return false;
    };
    host_matches(&host, "naver.com") || host == "ssl.pstatic.net" || host == "video.pstatic.net"
}

/// 신뢰 문서가 알려 준 클립 미디어 URL 을 검증해 반환한다.
pub fn clip_media_url(value: &str, document_url: Option<&Url>) -> Option<Url> {
    if !is_clip_document(document_url) || value.len() > 16_384 {
        return None;
    }
    Url::parse(value).ok().filter(allows)
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewCoreTests/MediaURLPolicyTests.swift`
    use super::*;

    #[test]
    fn only_https_media_from_trusted_document() {
        let document = Url::parse("https://chzzk.naver.com/embed/clip/abc").ok();
        let media = "https://video.pstatic.net/clip/index.m3u8?token=a%2Bb";
        assert_eq!(
            clip_media_url(media, document.as_ref())
                .map(|u| u.to_string())
                .as_deref(),
            Some(media)
        );
        for value in [
            "file:///tmp/a.m3u8",
            "http://video.pstatic.net/a.m3u8",
            "blob:https://chzzk.naver.com/abc",
            "https://127.0.0.1/a.m3u8",
            "https://video.pstatic.net.evil.com/a.m3u8",
        ] {
            assert!(
                clip_media_url(value, document.as_ref()).is_none(),
                "{value}"
            );
        }
        for value in [
            "https://evil.com",
            "http://chzzk.naver.com",
            "https://chzzk.naver.com.evil.com",
            "https://user@chzzk.naver.com",
        ] {
            let doc = Url::parse(value).ok();
            assert!(clip_media_url(media, doc.as_ref()).is_none(), "{value}");
        }
    }

    #[test]
    fn rejects_fragment_and_port() {
        assert!(!allows(
            &Url::parse("https://video.pstatic.net/a#x").unwrap()
        ));
        assert!(!allows(
            &Url::parse("https://video.pstatic.net:8443/a").unwrap()
        ));
        assert!(allows(
            &Url::parse("https://video.pstatic.net:443/a").unwrap()
        ));
    }
}
