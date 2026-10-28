//! `Sources/CViewPlayer/LocalStreamProxy+ByteRange.swift` 포팅 (+ `+Response.swift` 의 헤더 조립 부분).
//!
//! - **단일·단순 구간**(`bytes=a-b`, `bytes=a-`)만 상류로 넘긴다. multi-range·접미 구간·형식 오류는
//!   넘기지 않는다(전체 200 응답은 HTTP 상 합법).
//! - 상류가 Range 를 무시하고 200 전체를 주면 요청 창만 떼어 206 으로 보낸다([`ByteRangeWindow`]).
//! - 길이를 아는데 시작이 끝을 넘으면 416 `bytes */total`.
//! - CORS 사전 요청(OPTIONS)은 `Range` 하나만 허락하는 204 로 답한다.

/// 요청 헤더 원문에서 전달할 수 있는 `Range` 값. 없거나 단순 구간이 아니면 `None`.
pub fn client_byte_range(request: &str) -> Option<String> {
    request_header_value("range", request).filter(|v| is_simple_byte_range(v))
}

/// 요청 헤더 `name`(소문자) 의 첫 값 — 첫 빈 줄(헤더 끝)에서 멈춘다.
pub fn request_header_value(name: &str, request: &str) -> Option<String> {
    for line in request.split("\r\n").skip(1) {
        if line.is_empty() {
            return None;
        }
        let Some(colon) = line.find(':') else {
            continue;
        };
        if line[..colon].trim().to_lowercase() == name {
            return Some(line[colon + 1..].trim().to_string());
        }
    }
    None
}

pub fn is_simple_byte_range(value: &str) -> bool {
    parse_simple_byte_range(value).is_some()
}

/// `bytes=START-` 또는 `bytes=START-END`(END ≥ START). 숫자는 ASCII 숫자 1~19자리만.
pub fn parse_simple_byte_range(value: &str) -> Option<(u64, Option<u64>)> {
    if value.len() > 64 {
        return None;
    }
    let spec = value.strip_prefix("bytes=")?;
    let (a, b) = spec.split_once('-')?;
    fn number(t: &str) -> Option<u64> {
        if !(1..=19).contains(&t.len()) || !t.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        t.parse().ok()
    }
    let start = number(a)?;
    if b.is_empty() {
        return Some((start, None));
    }
    let end = number(b)?;
    (end >= start).then_some((start, Some(end)))
}

/// 상류 `Content-Range` 를 실어도 되는지 — 빈 값·제어문자는 버린다(헤더 주입 방지).
pub fn relayable_content_range(value: Option<&str>) -> Option<String> {
    let v = value?;
    if v.is_empty() || v.chars().any(|c| (c as u32) < 0x20 || c as u32 == 0x7F) {
        return None;
    }
    Some(v.to_string())
}

/// 상류가 Range 를 무시하고 200 전체를 줄 때 클라이언트에 보낼 구간.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRangeWindow {
    pub first: i64,
    /// 포함 끝.
    pub last: i64,
    pub total: i64,
}

impl ByteRangeWindow {
    pub fn new(requested: &str, total: i64) -> Option<Self> {
        if total <= 0 {
            return None;
        }
        let (start, end) = parse_simple_byte_range(requested)?;
        if start >= total as u64 {
            return None;
        }
        let end = end.map_or(total - 1, |e| e.min(i64::MAX as u64) as i64);
        Some(Self {
            first: start as i64,
            last: end.min(total - 1),
            total,
        })
    }

    /// 길이를 아는데 요청 시작이 끝을 넘는다 — 416 으로 답할 경우.
    pub fn is_unsatisfiable(requested: &str, total: i64) -> bool {
        if total <= 0 {
            return false;
        }
        parse_simple_byte_range(requested).is_some_and(|(start, _)| start >= total as u64)
    }

    pub fn content_length(&self) -> i64 {
        self.last - self.first + 1
    }

    pub fn content_range(&self) -> String {
        format!("bytes {}-{}/{}", self.first, self.last, self.total)
    }

    /// 상류 본문의 `offset` 부터 온 조각 중 창에 드는 부분.
    pub fn slice<'a>(&self, data: &'a [u8], offset: i64) -> Option<&'a [u8]> {
        let lower = self.first.max(offset);
        let upper = (self.last + 1).min(offset + data.len() as i64);
        if lower >= upper {
            return None;
        }
        Some(&data[(lower - offset) as usize..(upper - offset) as usize])
    }
}

/// 상태줄 사유 구절.
pub fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        416 => "Range Not Satisfiable",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        s => match s / 100 {
            2 => "Success",
            3 => "Redirection",
            4 => "Client Error",
            _ => "Server Error",
        },
    }
}

/// 사전 요청에 허락하는 요청 헤더(소문자).
pub const PREFLIGHT_ALLOWED_HEADERS: [&str; 1] = ["range"];

/// 요청된 헤더 중 허용 목록에 든 것만(중복 제거, 순서 유지).
pub fn preflight_allowed_headers(requested: Option<&str>) -> Vec<String> {
    let mut allowed: Vec<String> = Vec::new();
    for name in requested.unwrap_or("").split(',') {
        let lower = name.trim().to_lowercase();
        if PREFLIGHT_ALLOWED_HEADERS.contains(&lower.as_str()) && !allowed.contains(&lower) {
            allowed.push(lower);
        }
    }
    allowed
}

/// 사전 요청 응답 헤더 목록(상태줄 제외). 본문 길이를 싣지 않는다.
pub fn preflight_headers(
    requested: Option<&str>,
    keep_alive_timeout: f64,
) -> Vec<(String, String)> {
    let allowed = preflight_allowed_headers(requested);
    let mut h = vec![
        ("Access-Control-Allow-Origin".to_string(), "*".to_string()),
        (
            "Access-Control-Allow-Methods".to_string(),
            "GET, OPTIONS".to_string(),
        ),
    ];
    if !allowed.is_empty() {
        h.push(("Access-Control-Allow-Headers".into(), allowed.join(", ")));
    }
    h.push(("Access-Control-Max-Age".into(), "600".into()));
    h.push(("Connection".into(), "keep-alive".into()));
    h.push((
        "Keep-Alive".into(),
        format!("timeout={}", keep_alive_timeout as i64),
    ));
    h
}

/// 사전 요청 응답 바이트(원본 `preflightResponseData`).
pub fn preflight_response_bytes(requested: Option<&str>, keep_alive_timeout: f64) -> Vec<u8> {
    let mut s = format!("HTTP/1.1 204 {}\r\n", reason_phrase(204));
    for (k, v) in preflight_headers(requested, keep_alive_timeout) {
        s.push_str(&format!("{k}: {v}\r\n"));
    }
    s.push_str("\r\n");
    s.into_bytes()
}

/// 오류 응답 헤더 목록 — 정상 응답과 같은 CORS·캐시 헤더를 싣는다.
pub fn error_headers(body_len: usize, keep_alive: bool) -> Vec<(String, String)> {
    vec![
        ("Content-Type".into(), "text/plain".into()),
        ("Content-Length".into(), body_len.to_string()),
        (
            "Connection".into(),
            if keep_alive { "keep-alive" } else { "close" }.into(),
        ),
        ("Access-Control-Allow-Origin".into(), "*".into()),
        ("Cache-Control".into(), "no-cache".into()),
    ]
}

/// 오류 응답 바이트(원본 `errorResponseData`) — 회귀 테스트가 헤더 집합을 바이트로 고정한다.
pub fn error_response_bytes(status: u16, message: &str, keep_alive: bool) -> Vec<u8> {
    let mut s = format!("HTTP/1.1 {status} {message}\r\n");
    for (k, v) in error_headers(message.len(), keep_alive) {
        s.push_str(&format!("{k}: {v}\r\n"));
    }
    s.push_str("\r\n");
    s.push_str(message);
    s.into_bytes()
}

#[cfg(test)]
mod tests {
    //! `PrecisionReview0926ProxyByteRangeTests`·`PrecisionReview0926ProxyPreflightTests`·
    //! `LocalStreamProxyErrorHeaderTests` 의 순수 부분 포팅.
    use super::*;

    #[test]
    fn byte_range_window() {
        assert!(ByteRangeWindow::new("bytes=0-99", -1).is_none());
        assert!(ByteRangeWindow::new("bytes=1000-", 1_000).is_none());
        assert!(ByteRangeWindow::new("bytes=-5", 1_000).is_none());
        assert!(ByteRangeWindow::is_unsatisfiable("bytes=1000-", 1_000));
        assert!(!ByteRangeWindow::is_unsatisfiable("bytes=999-", 1_000));
        assert!(!ByteRangeWindow::is_unsatisfiable("bytes=5000-", -1));
        assert!(!ByteRangeWindow::is_unsatisfiable("bytes=-5", 1_000));
        assert_eq!(
            ByteRangeWindow::new("bytes=990-", 1_000)
                .unwrap()
                .content_range(),
            "bytes 990-999/1000"
        );
        assert_eq!(
            ByteRangeWindow::new("bytes=10-99999999999999999", 1_000)
                .unwrap()
                .content_length(),
            990
        );
        let w = ByteRangeWindow::new("bytes=100-299", 1_000).unwrap();
        assert_eq!(w.content_length(), 200);
        let body: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        let mut sliced = Vec::new();
        for start in (0..body.len()).step_by(128) {
            let piece = &body[start..(start + 128).min(body.len())];
            if let Some(p) = w.slice(piece, start as i64) {
                sliced.extend_from_slice(p);
            }
        }
        assert_eq!(sliced, &body[100..300]);
        assert!(w.slice(&body[0..100], 0).is_none());
        assert!(w.slice(&body[300..400], 300).is_none());
        assert_eq!(w.slice(&body[50..150], 50).unwrap(), &body[100..150]);
    }

    #[test]
    fn parses_client_range_header() {
        assert_eq!(
            client_byte_range("GET /a.m4s HTTP/1.1\r\nHost: x\r\nRange: bytes=100-199\r\n\r\n")
                .as_deref(),
            Some("bytes=100-199")
        );
        assert_eq!(
            client_byte_range("GET /a.m4s HTTP/1.1\r\nrange:  bytes=0- \r\n\r\n").as_deref(),
            Some("bytes=0-")
        );
        assert!(client_byte_range("GET /a.m4s HTTP/1.1\r\nHost: x\r\n\r\n").is_none());
        assert!(client_byte_range(
            "GET /a.m4s HTTP/1.1\r\nHost: x\r\n\r\nGET /b.m4s HTTP/1.1\r\nRange: bytes=0-1\r\n\r\n"
        )
        .is_none());
        assert!(client_byte_range("GET /a.m4s HTTP/1.1\r\nRange: bytes=+5-9\r\n\r\n").is_none());
        for r in [
            "bytes=0-1,5-9",
            "bytes=9-1",
            "bytes=-100",
            "items=0-1",
            "bytes=+5-9",
        ] {
            assert!(!is_simple_byte_range(r), "{r}");
        }
    }

    #[test]
    fn content_range_injection_is_dropped() {
        assert!(relayable_content_range(Some("bytes 0-1/2\r\nX-Evil: y")).is_none());
        assert!(relayable_content_range(Some("")).is_none());
        assert!(relayable_content_range(None).is_none());
        assert_eq!(
            relayable_content_range(Some("bytes 0-399/1000")).as_deref(),
            Some("bytes 0-399/1000")
        );
    }

    fn lines(bytes: &[u8]) -> Vec<String> {
        let text = String::from_utf8_lossy(bytes);
        let end = text.find("\r\n\r\n").unwrap();
        text[..end].split("\r\n").map(str::to_string).collect()
    }

    #[test]
    fn preflight_allows_only_range() {
        let h = |r: Option<&str>| lines(&preflight_response_bytes(r, 15.0));
        assert!(h(Some("Range")).contains(&"Access-Control-Allow-Headers: range".to_string()));
        assert!(
            h(Some(" RANGE , range ")).contains(&"Access-Control-Allow-Headers: range".to_string())
        );
        assert!(!h(Some("x-evil\r\nSet-Cookie: a=b"))
            .iter()
            .any(|l| l.starts_with("Access-Control-Allow-Headers")));
        assert!(!h(None)
            .iter()
            .any(|l| l.starts_with("Access-Control-Allow-Headers")));
        assert_eq!(h(None)[0], "HTTP/1.1 204 No Content");
        assert!(!h(None)
            .iter()
            .any(|l| l.to_lowercase().starts_with("content-length")));
    }

    #[test]
    fn reason_phrases() {
        assert_eq!(reason_phrase(200), "OK");
        assert_eq!(reason_phrase(204), "No Content");
        assert_eq!(reason_phrase(206), "Partial Content");
        assert_eq!(reason_phrase(416), "Range Not Satisfiable");
        assert_eq!(reason_phrase(404), "Not Found");
        assert_eq!(reason_phrase(418), "Client Error");
        assert_eq!(reason_phrase(599), "Server Error");
    }

    #[test]
    fn error_response_headers() {
        let keep = lines(&error_response_bytes(502, "Bad Gateway", true));
        let close = lines(&error_response_bytes(400, "Bad Request", false));
        assert_eq!(keep[0], "HTTP/1.1 502 Bad Gateway");
        for required in [
            "Access-Control-Allow-Origin: *",
            "Cache-Control: no-cache",
            "Connection: keep-alive",
            "Content-Length: 11",
        ] {
            assert!(keep.contains(&required.to_string()), "{required}");
        }
        assert!(close.contains(&"Connection: close".to_string()));
        assert!(close.contains(&"Access-Control-Allow-Origin: *".to_string()));
    }
}
