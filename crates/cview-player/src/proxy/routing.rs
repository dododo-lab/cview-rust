//! `Sources/CViewPlayer/LocalStreamProxy+Routing.swift`·`+Requests.swift`·`+Upstream.swift`·`+Response.swift`
//! 의 순수 라우팅/재작성 부분 포팅.
//!
//! - `/_p_/HOST/path` 크로스CDN 라우팅과 상류 URL 검증
//! - Akamai `hdntl` 경로 토큰 정규화
//! - 재생목록 URI 재작성(RFC 3986 참조 해석, URI 속성 태그)
//!
//! 모든 함수는 주 CDN 호스트(`target_host`)를 인자로 받는다 — 상태가 없어 테스트가 쉽다.

use cview_core::utils::media_url_policy;
use url::Url;

/// 크로스CDN 라우팅 접두사.
pub const CROSS_CDN_PREFIX: &str = "/_p_/";

/// 원본 URL 을 로컬 프록시 URL 로 바꾼다. 허용 외 URL 이면 `None`.
pub fn proxy_url(original: &Url, port: u16, target_host: &str) -> Option<String> {
    if port == 0 || !media_url_policy::allows(original) {
        return None;
    }
    let host = original.host_str()?;
    let path = if original.path().is_empty() {
        "/"
    } else {
        original.path()
    };
    let query = original
        .query()
        .map(|q| format!("?{q}"))
        .unwrap_or_default();
    let prefix = if !target_host.is_empty() && host == target_host {
        String::new()
    } else {
        format!("/_p_/{host}")
    };
    Some(format!("http://127.0.0.1:{port}{prefix}{path}{query}"))
}

/// 요청 대상 경로를 상류 URL 로. 권한(authority)을 URL 생성 전에 검증한다.
pub fn upstream_url(path: &str, target_host: &str) -> Option<Url> {
    if !path.starts_with('/')
        || path.starts_with("//")
        || path.contains('#')
        || path.contains('\\')
        || path.bytes().any(|b| b <= 32 || b == 127)
    {
        return None;
    }
    let mut host = target_host.to_string();
    let mut resource = path.to_string();
    if let Some(rest) = path.strip_prefix(CROSS_CDN_PREFIX) {
        let boundary = rest.find(['/', '?']).unwrap_or(rest.len());
        host = rest[..boundary].to_string();
        resource = rest[boundary..].to_string();
        if !resource.starts_with('/') {
            resource.insert(0, '/');
        }
    }
    if !media_url_policy::is_allowed_host(&host) {
        return None;
    }
    let url = Url::parse(&format!("https://{host}{resource}")).ok()?;
    media_url_policy::allows(&url).then_some(url)
}

/// 요청줄에서 경로를 꺼낸다 — `METHOD SP target SP HTTP/1.x` 의 origin-form 만.
pub fn extract_path(request: &str, method: &str) -> Option<String> {
    let first = request.split("\r\n").next()?;
    let parts: Vec<&str> = first.splitn(3, ' ').collect();
    if parts.len() != 3 || parts[0] != method || !matches!(parts[2], "HTTP/1.1" | "HTTP/1.0") {
        return None;
    }
    Some(parts[1].to_string())
}

/// 경로가 미디어 세그먼트인지 — 계측 전용.
pub fn path_looks_like_segment(path: &str) -> bool {
    let lower = path.to_lowercase();
    if lower.contains(".m3u8") {
        return false;
    }
    [".m4s", ".m4v", ".ts", ".mp4", ".cmfv", ".cmfa"]
        .iter()
        .any(|ext| lower.ends_with(ext) || lower.contains(&format!("{ext}?")))
}

/// Akamai `hdntl` 경로 토큰 정규화 — WKWebView 가 인코딩한 `~ = *` 를 되돌리고 `%2f` 를 `%2F` 로.
/// 쿼리는 건드리지 않는다.
pub fn canonicalize_akamai_token_path(request_path: &str) -> String {
    if !request_path.contains('%') {
        return request_path.to_string();
    }
    match request_path.find('?') {
        Some(q) => normalized_token_path(&request_path[..q]) + &request_path[q..],
        None => normalized_token_path(request_path),
    }
}

/// 네 가지 토큰 이스케이프만 바꾸는 단일 UTF-8 패스.
pub fn normalized_token_path(path: &str) -> String {
    let input = path.as_bytes();
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'%' && i + 2 < input.len() {
            if let Some(r) = token_escape_replacement(input[i + 1], input[i + 2]) {
                if r == b'/' {
                    out.extend_from_slice(b"%2F");
                } else {
                    out.push(r);
                }
                i += 3;
                continue;
            }
        }
        out.push(input[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| path.to_string())
}

fn token_escape_replacement(a: u8, b: u8) -> Option<u8> {
    match (a, b) {
        (b'7', b'e') | (b'7', b'E') => Some(b'~'),
        (b'3', b'd') | (b'3', b'D') => Some(b'='),
        (b'2', b'a') | (b'2', b'A') => Some(b'*'),
        (b'2', b'f') => Some(b'/'),
        _ => None,
    }
}

// MARK: - 재생목록 URI 재작성

/// `URI="…"` 속성으로 다른 자원을 가리키는 태그.
pub const URI_ATTRIBUTE_TAGS: [&str; 8] = [
    "#EXT-X-MAP:",
    "#EXT-X-KEY:",
    "#EXT-X-SESSION-KEY:",
    "#EXT-X-MEDIA:",
    "#EXT-X-I-FRAME-STREAM-INF:",
    "#EXT-X-PART:",
    "#EXT-X-PRELOAD-HINT:",
    "#EXT-X-RENDITION-REPORT:",
];

/// 줄 URI 와 URI 속성만 변환한다. 주석·미지원 속성은 원문 보존.
fn transform_playlist_uris(content: &str, transform: impl Fn(&str) -> Option<String>) -> String {
    content
        .split('\n')
        .map(|line| {
            let trimmed = line.trim();
            if URI_ATTRIBUTE_TAGS.iter().any(|t| trimmed.starts_with(t)) {
                if let Some((s, e)) = uri_attribute_value_range(line) {
                    if let Some(v) = transform(&line[s..e]) {
                        return format!("{}{}{}", &line[..s], v, &line[e..]);
                    }
                }
                return line.to_string();
            }
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return line.to_string();
            }
            transform(trimmed).unwrap_or_else(|| line.to_string())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 절대 `https://` 허용 CDN URI 의 authority 만 프록시로 바꾼다.
pub fn rewrite_m3u8_urls(content: &str, proxy_base: &str, target_host: &str) -> String {
    transform_playlist_uris(content, |uri| {
        if !uri.to_lowercase().starts_with("https://") {
            return None;
        }
        let url = Url::parse(uri).ok()?;
        if !media_url_policy::allows(&url) {
            return None;
        }
        let host = url.host_str()?;
        let rest = &uri[8..];
        let tail_start = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let tail = &rest[tail_start..];
        let path = if tail.starts_with('/') {
            tail.to_string()
        } else {
            format!("/{tail}")
        };
        let route = if host == target_host {
            String::new()
        } else {
            format!("/_p_/{host}")
        };
        Some(format!("{proxy_base}{route}{path}"))
    })
}

/// URL 문자열의 디렉터리 경로("/a/b/") — 쿼리 제거, 실제 '/' 기준.
pub fn extract_directory_path(url: &str) -> String {
    let Some(p) = url.find("://") else {
        return "/".into();
    };
    let rest = &url[p + 3..];
    let Some(start) = rest.find('/') else {
        return "/".into();
    };
    let mut path = &rest[start..];
    if let Some(q) = path.find('?') {
        path = &path[..q];
    }
    match path.rfind('/') {
        Some(i) => path[..=i].to_string(),
        None => "/".into(),
    }
}

fn host_of(url: &str) -> Option<String> {
    Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
}

/// 재생목록 상대 URI 의 기준 경로 — 최종 응답 URL 기준. 주 CDN 이 아니면 크로스CDN 접두사.
pub fn playlist_base_path(
    request_url: &str,
    response_url: Option<&Url>,
    target_host: &str,
) -> String {
    let base = response_url.map_or_else(|| request_url.to_string(), |u| u.as_str().to_string());
    let dir = extract_directory_path(&base);
    match host_of(&base) {
        Some(host) if host != target_host => format!("/_p_/{host}{dir}"),
        _ => dir,
    }
}

/// 재생목록 안의 URI 참조 하나를 프록시 절대 URL 로. `None` 이면 원문 유지.
pub fn proxied_playlist_reference(
    uri: &str,
    proxy_base: &str,
    base_path: &str,
    target_host: &str,
) -> Option<String> {
    if uri.is_empty() || uri.starts_with('?') || uri.starts_with('#') || has_uri_scheme(uri) {
        return None;
    }
    let mut route = String::new();
    let mut directory = base_path.to_string();
    if let Some(rest) = base_path.strip_prefix(CROSS_CDN_PREFIX) {
        let slash = rest.find('/').unwrap_or(rest.len());
        route = format!("/_p_/{}", &rest[..slash]);
        directory = if slash == rest.len() {
            "/".into()
        } else {
            rest[slash..].to_string()
        };
    }
    if let Some(rest) = uri.strip_prefix("//") {
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let authority = &rest[..end];
        let tail = &rest[end..];
        let path = if tail.starts_with('/') {
            tail.to_string()
        } else {
            format!("/{tail}")
        };
        let prefix = if authority == target_host {
            String::new()
        } else {
            format!("/_p_/{authority}")
        };
        return Some(format!(
            "{proxy_base}{prefix}{}",
            remove_dot_segments(&path)
        ));
    }
    if uri.starts_with('/') {
        return Some(format!("{proxy_base}{route}{}", remove_dot_segments(uri)));
    }
    Some(format!(
        "{proxy_base}{route}{}",
        remove_dot_segments(&format!("{directory}{uri}"))
    ))
}

/// 모든 상대 URI 를 절대 프록시 URL 로. `?`·`#` 참조는 최종 재생목록 파일명을 유지한다.
pub fn make_relative_urls_absolute(
    content: &str,
    proxy_base: &str,
    base_path: &str,
    response_proxy_url: Option<&str>,
    target_host: &str,
) -> String {
    transform_playlist_uris(content, |uri| {
        if uri.starts_with('?') || uri.starts_with('#') {
            if let Some(full) = response_proxy_url {
                let without_fragment = full.split('#').next().unwrap_or(full);
                let base = if uri.starts_with('?') {
                    without_fragment
                        .split('?')
                        .next()
                        .unwrap_or(without_fragment)
                } else {
                    without_fragment
                };
                return Some(format!("{base}{uri}"));
            }
        }
        proxied_playlist_reference(uri, proxy_base, base_path, target_host)
    })
}

/// 태그 속성 목록에서 이름이 정확히 `URI` 인 따옴표 값의 바이트 범위(따옴표 제외).
pub fn uri_attribute_value_range(line: &str) -> Option<(usize, usize)> {
    let colon = line.find(':')?;
    let mut cursor = colon + 1;
    while let Some(eq_rel) = line[cursor..].find('=') {
        let equals = cursor + eq_rel;
        let name = line[cursor..equals].trim_matches(' ');
        cursor = equals + 1;
        if line[cursor..].starts_with('"') {
            let value_start = cursor + 1;
            let close = value_start + line[value_start..].find('"')?;
            if name == "URI" {
                return Some((value_start, close));
            }
            cursor = close + 1;
        }
        let comma = cursor + line[cursor..].find(',')?;
        cursor = comma + 1;
    }
    None
}

/// RFC 3986 §3.1 스킴으로 시작하는지 — `:` 가 첫 `/`·`?`·`#` 보다 앞에 있어야 한다.
pub fn has_uri_scheme(uri: &str) -> bool {
    let mut saw_letter = false;
    for b in uri.bytes() {
        match b {
            b':' => return saw_letter,
            b'a'..=b'z' | b'A'..=b'Z' => saw_letter = true,
            b'0'..=b'9' | b'+' | b'-' | b'.' => {
                if !saw_letter {
                    return false;
                }
            }
            _ => return false,
        }
    }
    false
}

/// RFC 3986 §5.2.4 점 세그먼트 제거 — 쿼리는 바이트 그대로.
pub fn remove_dot_segments(path_and_query: &str) -> String {
    let q = path_and_query.find('?').unwrap_or(path_and_query.len());
    let path = &path_and_query[..q];
    if !(path.contains("/./")
        || path.contains("/../")
        || path.ends_with("/.")
        || path.ends_with("/.."))
    {
        return path_and_query.to_string();
    }
    let segments: Vec<&str> = path.split('/').skip(1).collect();
    let mut output: Vec<&str> = Vec::new();
    for seg in &segments {
        match *seg {
            "." => {}
            ".." => {
                output.pop();
            }
            s => output.push(s),
        }
    }
    let mut resolved = format!("/{}", output.join("/"));
    if matches!(segments.last(), Some(&".") | Some(&"..")) && !resolved.ends_with('/') {
        resolved.push('/');
    }
    resolved + &path_and_query[q..]
}

#[cfg(test)]
mod tests {
    //! `PrecisionReview0926PlaylistURIResolutionTests`·`ProxySecurityTests`·`ProxyTokenNormalizationTests`·
    //! `ProxySupplyGapTelemetryTests`(경로 판정)·`ProxyPlaylistRedirectBaseTests` 포팅.
    use super::*;

    const BASE: &str = "http://127.0.0.1:1234";
    const HOST: &str = "a.navercdn.com";

    fn rewrite(line: &str, base_path: &str) -> String {
        make_relative_urls_absolute(line, BASE, base_path, None, HOST)
    }

    fn target(abs: &str) -> &str {
        &abs[BASE.len()..]
    }

    #[test]
    fn root_relative_on_root_playlist() {
        let base = playlist_base_path("https://a.navercdn.com/index.m3u8", None, HOST);
        assert_eq!(base, "/");
        let out = rewrite("/segments/a.m4s", &base);
        assert_eq!(out, "http://127.0.0.1:1234/segments/a.m4s");
        assert_eq!(
            upstream_url(target(&out), HOST).unwrap().as_str(),
            "https://a.navercdn.com/segments/a.m4s"
        );
    }

    #[test]
    fn root_relative_variants() {
        assert_eq!(
            rewrite("/segments/a.m4s", "/live/dir/"),
            "http://127.0.0.1:1234/segments/a.m4s"
        );
        assert_eq!(
            rewrite(
                "#EXT-X-MAP:URI=\"/init/a_init.m4v\",BYTERANGE=\"720@0\"",
                "/live/dir/"
            ),
            "#EXT-X-MAP:URI=\"http://127.0.0.1:1234/init/a_init.m4v\",BYTERANGE=\"720@0\""
        );
        assert_eq!(
            rewrite("/segments/a.m4s", "/_p_/b.pstatic.net/edge/7/"),
            "http://127.0.0.1:1234/_p_/b.pstatic.net/segments/a.m4s"
        );
    }

    #[test]
    fn dot_segments_resolve_against_upstream_path() {
        assert_eq!(
            rewrite("../../seg.m4s?t=1", "/_p_/b.pstatic.net/edge/"),
            "http://127.0.0.1:1234/_p_/b.pstatic.net/seg.m4s?t=1"
        );
        assert_eq!(
            rewrite("../c/seg.m4s", "/live/a/b/"),
            "http://127.0.0.1:1234/live/a/c/seg.m4s"
        );
        assert_eq!(
            rewrite("./seg.m4s?x=/../y", "/live/"),
            "http://127.0.0.1:1234/live/seg.m4s?x=/../y"
        );
    }

    #[test]
    fn network_path_reference() {
        let allowed = rewrite("//b.pstatic.net/x/seg.m4s", "/live/");
        assert_eq!(allowed, "http://127.0.0.1:1234/_p_/b.pstatic.net/x/seg.m4s");
        assert_eq!(
            rewrite("//a.navercdn.com/x/seg.m4s", "/live/"),
            "http://127.0.0.1:1234/x/seg.m4s"
        );
        assert_eq!(
            upstream_url(target(&allowed), HOST).unwrap().as_str(),
            "https://b.pstatic.net/x/seg.m4s"
        );
        let evil = rewrite("//evil.com/x", "/live/");
        assert_eq!(evil, "http://127.0.0.1:1234/_p_/evil.com/x");
        assert!(upstream_url(target(&evil), HOST).is_none());
    }

    #[test]
    fn uri_attributes_on_other_tags() {
        let base = "/_p_/b.pstatic.net/edge/7/";
        let cases = [
            ("#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"ko\",URI=\"/audio/chunklist.m3u8\"",
             "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"ko\",URI=\"http://127.0.0.1:1234/_p_/b.pstatic.net/audio/chunklist.m3u8\""),
            ("#EXT-X-I-FRAME-STREAM-INF:BANDWIDTH=1,CODECS=\"avc1.4d401f,mp4a.40.2\",URI=\"iframe.m3u8\"",
             "#EXT-X-I-FRAME-STREAM-INF:BANDWIDTH=1,CODECS=\"avc1.4d401f,mp4a.40.2\",URI=\"http://127.0.0.1:1234/_p_/b.pstatic.net/edge/7/iframe.m3u8\""),
            ("#EXT-X-KEY:METHOD=AES-128,URI=\"../key.bin\",IV=0x1",
             "#EXT-X-KEY:METHOD=AES-128,URI=\"http://127.0.0.1:1234/_p_/b.pstatic.net/edge/key.bin\",IV=0x1"),
            ("#EXT-X-PART:DURATION=0.5,URI=\"/segments/p1.m4s\",INDEPENDENT=YES",
             "#EXT-X-PART:DURATION=0.5,URI=\"http://127.0.0.1:1234/_p_/b.pstatic.net/segments/p1.m4s\",INDEPENDENT=YES"),
            ("#EXT-X-PRELOAD-HINT:TYPE=PART,URI=\"/segments/p2.m4s\"",
             "#EXT-X-PRELOAD-HINT:TYPE=PART,URI=\"http://127.0.0.1:1234/_p_/b.pstatic.net/segments/p2.m4s\""),
            ("#EXT-X-RENDITION-REPORT:URI=\"../8/chunklist.m3u8\",LAST-MSN=10",
             "#EXT-X-RENDITION-REPORT:URI=\"http://127.0.0.1:1234/_p_/b.pstatic.net/edge/8/chunklist.m3u8\",LAST-MSN=10"),
        ];
        for (line, expected) in cases {
            assert_eq!(rewrite(line, base), expected);
        }
    }

    #[test]
    fn uri_attribute_boundaries() {
        for line in [
            "#EXT-X-DATERANGE:ID=\"ad\",X-ASSET-URI=\"/ads/a.m3u8\"",
            "#EXT-X-MEDIA:TYPE=CLOSED-CAPTIONS,GROUP-ID=\"cc\",NAME=\"x\",INSTREAM-ID=\"CC1\"",
            "#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://key-id\",KEYFORMAT=\"com.apple.streamingkeydelivery\"",
            "#EXT-X-MEDIA:TYPE=AUDIO,NAME=\",URI=x\",GROUP-ID=\"a\"",
        ] {
            assert_eq!(rewrite(line, "/live/"), line);
        }
        let line = "#EXT-X-MEDIA:X-URI=\"a\",URI=\"b\"";
        let (s, e) = uri_attribute_value_range(line).unwrap();
        assert_eq!(&line[s..e], "b");
    }

    #[test]
    fn unchanged_common_cases() {
        assert_eq!(
            rewrite("http_0.m4s", "/live/"),
            "http://127.0.0.1:1234/live/http_0.m4s"
        );
        assert_eq!(
            rewrite("seg.m4s?token=a%2Bb", "/live/1080p/"),
            "http://127.0.0.1:1234/live/1080p/seg.m4s?token=a%2Bb"
        );
        let token = "hdntl=exp=1~acl=*%2F*~hmac=abc/720p_0.m4s";
        assert_eq!(
            rewrite(token, "/live/"),
            format!("http://127.0.0.1:1234/live/{token}")
        );
        assert_eq!(
            rewrite("#EXT-X-MAP:URI=\"init.mp4\"", "/_p_/b.pstatic.net/edge/"),
            "#EXT-X-MAP:URI=\"http://127.0.0.1:1234/_p_/b.pstatic.net/edge/init.mp4\""
        );
        assert_eq!(
            rewrite("https://evil.com/a.m4s", "/live/"),
            "https://evil.com/a.m4s"
        );
        assert_eq!(
            rewrite("HTTPS://evil.com/a.m4s", "/live/"),
            "HTTPS://evil.com/a.m4s"
        );
        assert_eq!(
            rewrite("#EXTM3U\n#EXTINF:1.0,\n\nseg.m4s", "/live/"),
            "#EXTM3U\n#EXTINF:1.0,\n\nhttp://127.0.0.1:1234/live/seg.m4s"
        );
    }

    #[test]
    fn dot_segment_removal() {
        assert_eq!(remove_dot_segments("/a/b/c/./../../g"), "/a/g");
        assert_eq!(remove_dot_segments("/a/../../g"), "/g");
        assert_eq!(remove_dot_segments("/a/b/."), "/a/b/");
        assert_eq!(remove_dot_segments("/a/b/.."), "/a/");
        assert_eq!(remove_dot_segments("/.."), "/");
        assert_eq!(remove_dot_segments("/a/./b?q=/../x"), "/a/b?q=/../x");
        assert_eq!(remove_dot_segments("/a//b.m4s"), "/a//b.m4s");
    }

    #[test]
    fn scheme_detection() {
        assert!(has_uri_scheme("https://a/b"));
        assert!(has_uri_scheme("skd://key"));
        assert!(has_uri_scheme("data:text/plain,x"));
        assert!(!has_uri_scheme("a/b:c.m4s"));
        assert!(!has_uri_scheme("http_0.m4s"));
        assert!(!has_uri_scheme("1a:b"));
        assert!(!has_uri_scheme("seg.m4s?x=a:b"));
    }

    #[test]
    fn rejects_cross_cdn_authority() {
        for host in [
            "evil.com",
            "127.0.0.1",
            "[::1]",
            "169.254.169.254",
            "navercdn.com.evil.com",
            "evilnavercdn.com",
            "nlive-streaming.evil.com",
            "cdn.navercdn.com@evil.com",
            "cdn.navercdn.com:8080",
            "%65x.navercdn.com",
            "cdn.navercdn.com#evil",
        ] {
            assert!(
                upstream_url(&format!("/_p_/{host}/index.m3u8"), "").is_none(),
                "{host}"
            );
        }
        assert!(upstream_url("//evil.com/a", HOST).is_none());
    }

    #[test]
    fn preserves_signed_paths_and_queries() {
        let path = "/_p_/ex-nlive-streaming.navercdn.com/v/hdntl=exp=1~acl=*%2F*~hmac=abc/seg.m4s?sig=a%2Bb%3D";
        assert_eq!(
            upstream_url(path, "").unwrap().as_str(),
            "https://ex-nlive-streaming.navercdn.com/v/hdntl=exp=1~acl=*%2F*~hmac=abc/seg.m4s?sig=a%2Bb%3D"
        );
    }

    #[test]
    fn rewrites_only_whole_allowed_authorities() {
        let manifest = "https://video.pstatic.net/a.m3u8\nhttps://video.pstatic.net.evil.com/b.m3u8\nhttps://video.pstatic.net@evil.com/c.m3u8";
        assert_eq!(
            rewrite_m3u8_urls(manifest, BASE, ""),
            "http://127.0.0.1:1234/_p_/video.pstatic.net/a.m3u8\nhttps://video.pstatic.net.evil.com/b.m3u8\nhttps://video.pstatic.net@evil.com/c.m3u8"
        );
    }

    #[test]
    fn accepts_only_get_origin_form() {
        assert_eq!(
            extract_path("GET /index.m3u8 HTTP/1.1\r\nHost: localhost\r\n", "GET").as_deref(),
            Some("/index.m3u8")
        );
        assert!(extract_path("POST /index.m3u8 HTTP/1.1\r\n", "GET").is_none());
    }

    /// 대소문자 무시 치환(원본 테스트의 `replacingOccurrences(options: .caseInsensitive)`).
    fn replace_ci(s: &str, pat: &str, rep: &str) -> String {
        let lower = s.to_ascii_lowercase();
        let mut out = String::new();
        let mut i = 0;
        while i < s.len() {
            if lower[i..].starts_with(pat) {
                out.push_str(rep);
                i += pat.len();
            } else {
                let ch = s[i..].chars().next().unwrap();
                out.push(ch);
                i += ch.len_utf8();
            }
        }
        out
    }

    fn legacy(path: &str) -> String {
        let s = replace_ci(path, "%7e", "~");
        let s = replace_ci(&s, "%3d", "=");
        let s = replace_ci(&s, "%2a", "*");
        s.replace("%2f", "%2F")
    }

    #[test]
    fn token_substitutions_match_legacy() {
        for a in 32u8..=126 {
            for b in 32u8..=126 {
                let path = format!(
                    "/방송/hdntl=%{}/seg.m4s",
                    String::from_utf8(vec![a, b]).unwrap()
                );
                assert_eq!(normalized_token_path(&path), legacy(&path), "{path}");
            }
        }
        for path in [
            "/%",
            "/%2",
            "/%257e",
            "/%%7E%3D%2A%2f%2F",
            "/%7%7e",
            "/%00%20%3F",
        ] {
            assert_eq!(normalized_token_path(path), legacy(path), "{path}");
        }
        let path = "/_p_/livecloud.pstatic.net/hdntl%3Dexp%3d1%7Eacl%3d%2A%2fch%2F%2a/seg.m4s";
        let query = "?hdnts=exp%3d1%7eacl%2a%2f&sig=%3D%2F";
        assert_eq!(
            canonicalize_akamai_token_path(&format!("{path}{query}")),
            format!("{}{query}", legacy(path))
        );
        assert_eq!(
            canonicalize_akamai_token_path(&format!("/seg.m4s{query}")),
            format!("/seg.m4s{query}")
        );
    }

    #[test]
    fn segment_path_detection() {
        for p in [
            "/chzzk/lip1_kr/x/1080p/1080p_0_0_0.m4s",
            "/chzzk/lip1_kr/x/1080p/1080p_0_0_0.m4v",
            "/x/1080p_0_0.m4s?hdntl=exp%3D1",
            "/x/seg_12345.ts",
        ] {
            assert!(path_looks_like_segment(p), "{p}");
        }
        for p in [
            "/chzzk/lip1_kr/x/chunklist.m3u8",
            "/chzzk/lip1_kr/x/playlist.m3u8?token=1",
            "/chzzk/lip1_kr/x/",
            "/",
        ] {
            assert!(!path_looks_like_segment(p), "{p}");
        }
    }

    #[test]
    fn playlist_base_path_follows_redirects() {
        assert_eq!(
            playlist_base_path(
                "https://a.cdn.example/live/1080p/chunklist.m3u8?token=x",
                None,
                "a.cdn.example"
            ),
            "/live/1080p/"
        );
        let final_url = Url::parse("https://b.cdn.example/edge/7/chunklist.m3u8?token=y").unwrap();
        assert_eq!(
            playlist_base_path(
                "https://a.cdn.example/live/1080p/chunklist.m3u8",
                Some(&final_url),
                "a.cdn.example"
            ),
            "/_p_/b.cdn.example/edge/7/"
        );
    }

    #[test]
    fn proxy_url_routes_cross_cdn() {
        let same = Url::parse("https://a.navercdn.com/live/x.m3u8?t=1").unwrap();
        assert_eq!(
            proxy_url(&same, 9000, HOST).unwrap(),
            "http://127.0.0.1:9000/live/x.m3u8?t=1"
        );
        let other = Url::parse("https://b.pstatic.net/live/x.m3u8").unwrap();
        assert_eq!(
            proxy_url(&other, 9000, HOST).unwrap(),
            "http://127.0.0.1:9000/_p_/b.pstatic.net/live/x.m3u8"
        );
        assert!(proxy_url(&same, 0, HOST).is_none());
        assert!(proxy_url(&Url::parse("https://evil.com/x").unwrap(), 9000, HOST).is_none());
    }
}
