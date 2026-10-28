//! `Sources/CViewPlayer/MediaResponsePolicy.swift` 포팅.
//!
//! 응답 메타데이터가 우선이고 URL 경로는 일반 CDN 헤더일 때의 폴백일 뿐이다.
//! 암호화된 바이트를 들여다보거나 컨테이너를 추측하려고 미디어 조각을 붙잡지 않는다.

use url::Url;

use super::playback_key_access::is_key_url;

/// 재생목록 본문 상한(2 MiB).
pub const MAXIMUM_PLAYLIST_BYTES: usize = 2 * 1024 * 1024;

/// `Content-Type` 의 미디어 타입 부분(소문자, 파라미터 제거).
pub fn media_type(value: Option<&str>) -> String {
    value
        .unwrap_or("application/octet-stream")
        .split(';')
        .next()
        .map_or_else(
            || "application/octet-stream".into(),
            |s| s.trim_matches([' ', '\t']).to_lowercase(),
        )
}

/// URL 마지막 경로 요소의 확장자(소문자). Foundation `pathExtension` 과 같다.
pub fn path_extension(url: &Url) -> String {
    let last = url
        .path_segments()
        .and_then(|mut s| s.next_back())
        .unwrap_or("");
    match last.rfind('.') {
        Some(i) if i > 0 && i + 1 < last.len() => last[i + 1..].to_lowercase(),
        _ => String::new(),
    }
}

pub fn playlist_path(url: &Url) -> bool {
    matches!(path_extension(url).as_str(), "m3u8" | "m3u")
}

/// 200 응답이 재생목록인가.
pub fn is_playlist(
    status: u16,
    content_type: Option<&str>,
    response_url: Option<&Url>,
    requested_url: &Url,
) -> bool {
    if status != 200 {
        return false;
    }
    let mime = media_type(content_type);
    if matches!(
        mime.as_str(),
        "application/vnd.apple.mpegurl"
            | "application/x-mpegurl"
            | "audio/mpegurl"
            | "audio/x-mpegurl"
    ) {
        return true;
    }
    matches!(mime.as_str(), "application/octet-stream" | "text/plain")
        && (playlist_path(response_url.unwrap_or(requested_url)) || playlist_path(requested_url))
}

/// 클라이언트로 보낼 `Content-Type` — fMP4 를 잘못 표시하는 CDN 을 교정한다.
pub fn content_type(
    status: u16,
    content_type: Option<&str>,
    response_url: Option<&Url>,
    requested_url: &Url,
) -> String {
    let original = content_type
        .unwrap_or("application/octet-stream")
        .to_string();
    if !(200..300).contains(&status) || is_key_url(requested_url) {
        return original;
    }
    let mime = media_type(Some(&original));
    if !matches!(
        mime.as_str(),
        "video/mp2t" | "video/quicktime" | "application/octet-stream"
    ) {
        return original;
    }
    match path_extension(response_url.unwrap_or(requested_url)).as_str() {
        "cmfa" | "m4a" => "audio/mp4".into(),
        "m4s" | "m4v" | "mp4" | "cmfv" => "video/mp4".into(),
        _ => original,
    }
}

/// LL-HLS 블로킹 재생목록 요청(`_HLS_msn` 쿼리)인가.
pub fn is_blocking_playlist(url: &Url) -> bool {
    url.query_pairs().any(|(k, _)| k == "_HLS_msn")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn playlist_detection() {
        let req = u("https://a.pstatic.net/live/x.m3u8");
        assert!(is_playlist(
            200,
            Some("application/vnd.apple.mpegurl"),
            None,
            &u("https://a/x.bin")
        ));
        assert!(is_playlist(
            200,
            Some("text/plain; charset=utf-8"),
            None,
            &req
        ));
        assert!(!is_playlist(206, Some("application/x-mpegURL"), None, &req));
        assert!(!is_playlist(200, Some("video/mp4"), None, &req));
        assert!(!is_playlist(200, None, None, &u("https://a/x.ts")));
    }

    #[test]
    fn content_type_correction() {
        let seg = u("https://a.pstatic.net/live/seg.m4s");
        assert_eq!(
            content_type(200, Some("video/MP2T"), None, &seg),
            "video/mp4"
        );
        assert_eq!(
            content_type(
                200,
                Some("application/octet-stream"),
                None,
                &u("https://a/x.m4a")
            ),
            "audio/mp4"
        );
        assert_eq!(
            content_type(200, Some("video/mp2t"), None, &u("https://a/x.ts")),
            "video/mp2t"
        );
        assert_eq!(
            content_type(404, Some("video/mp2t"), None, &seg),
            "video/mp2t"
        );
        assert_eq!(
            content_type(200, Some("text/html"), None, &seg),
            "text/html"
        );
        let key = u("https://api.chzzk.naver.com/service/v1/encryption/lives/1/aes_key");
        assert_eq!(
            content_type(200, Some("application/octet-stream"), None, &key),
            "application/octet-stream"
        );
    }

    #[test]
    fn blocking_and_extension() {
        assert!(is_blocking_playlist(&u(
            "https://a/x.m3u8?_HLS_msn=10&_HLS_part=1"
        )));
        assert!(!is_blocking_playlist(&u("https://a/x.m3u8?msn=10")));
        assert_eq!(path_extension(&u("https://a/b/c.M3U8?x=1")), "m3u8");
        assert_eq!(path_extension(&u("https://a/b/")), "");
    }
}
