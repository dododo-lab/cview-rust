//! `Sources/CViewPlayer/StreamRecordingService+Segments.swift` 포팅 — 녹화의 한 폴.
//!
//! - 순서 보존: 가운데 조각이 실패하면 그 폴의 뒤 조각을 쓰지 않는다 — 다음 폴이 그 조각부터 다시.
//!   같은 조각이 [`MAX_BLOCKED_POLLS`] 폴 연속 실패하면 건너뛴다(빈틈).
//! - 식별 키: 호스트·토큰만 빼고 그 밖의 쿼리·구간은 남긴다([`StreamRecordingService::recording_key`]).
//! - `#EXT-X-DISCONTINUITY` 조각 앞에는 MAP 경로가 같아도 init 을 다시 쓴다. 그 조각을 건너뛰면
//!   MAP 이 있는 다음 조각 앞에서 대신 쓴다.

use std::time::Duration;

use percent_encoding::percent_decode_str;
use url::Url;

use super::{RecordingState, StreamRecordingService};
use crate::manifest::parser::{InitSegment, Segment};
use crate::manifest::HlsByteSpan;

/// 같은 조각이 이만큼 폴 연속으로 실패하면 건너뛴다.
pub const MAX_BLOCKED_POLLS: u32 = 3;
/// 조각 다운로드 시도 횟수(한 폴 안).
const MAX_DOWNLOAD_ATTEMPTS: u32 = 3;
/// 재시도 대기 상한.
const MAX_RETRY_DELAY: Duration = Duration::from_secs(64);

/// 재발급마다 바뀌지만 같은 파일을 가리키는 토큰·서명 쿼리 이름(소문자).
pub const VOLATILE_QUERY_NAMES: &[&str] = &[
    "hdnts",
    "hdnea",
    "hdntl",
    "__token__",
    "__bgda__",
    "token",
    "hmac",
    "signature",
    "sig",
    "expires",
    "policy",
    "key-pair-id",
];

fn decode(s: &str) -> String {
    percent_decode_str(s).decode_utf8_lossy().into_owned()
}

/// `TimeshiftRing.fileKey(for:)` — 호스트·쿼리·`hdntl=` 경로 토큰을 뺀 경로 키.
pub fn file_key(url: &Url) -> String {
    url.path()
        .split('/')
        .filter(|c| !c.is_empty())
        .map(decode)
        .filter(|c| !c.to_lowercase().starts_with("hdntl="))
        .collect::<Vec<_>>()
        .join("/")
}

/// `TimeshiftRing.fileKey(for:span:)` — 구간이 있으면 `경로키#offset-length`.
pub fn file_key_with_span(url: &Url, span: &HlsByteSpan) -> String {
    match span {
        HlsByteSpan::Whole => file_key(url),
        HlsByteSpan::Range(r) => {
            format!("{}#{}-{}", file_key(url), r.offset.unwrap_or(0), r.length)
        }
        HlsByteSpan::Invalid => format!("{}#invalid", file_key(url)),
    }
}

impl StreamRecordingService {
    /// 녹화의 조각·init 식별 키 — 경로 키에 **토큰이 아닌 쿼리**를 원래 순서로 더한다.
    pub fn recording_key(url: &Url, span: &HlsByteSpan) -> String {
        let key = file_key_with_span(url, span);
        let kept: Vec<&str> = url
            .query()
            .unwrap_or("")
            .split('&')
            .filter(|item| !item.is_empty())
            .filter(|item| {
                let name = item.split('=').next().unwrap_or("");
                !VOLATILE_QUERY_NAMES.contains(&decode(name).to_lowercase().as_str())
            })
            .collect();
        if kept.is_empty() {
            key
        } else {
            format!("{key}?{}", kept.join("&"))
        }
    }

    fn still_recording(&self) -> bool {
        self.st().state == RecordingState::Recording
    }

    /// 목록을 한 번 받아 새 조각을 순서대로 기록한다(폴링 루프의 한 바퀴).
    pub async fn poll_once(&self, playlist_url: &Url) {
        let segments = match self.fetch_parsed_segments(playlist_url).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Playlist 폴링 실패: {e}");
                return;
            }
        };
        let spans =
            HlsByteSpan::resolve(&segments, |u| Self::recording_key(u, &HlsByteSpan::Whole));
        for (segment, span) in segments.iter().zip(spans) {
            if !self.still_recording() {
                break;
            }
            // 순서 보존 — 다음 폴이 여기부터.
            if !self.record_segment(segment, span).await {
                break;
            }
        }
    }

    /// 조각 하나 — 이 폴을 계속해도 되면 true, 멈춰야 하면 false.
    async fn record_segment(&self, segment: &Segment, span: HlsByteSpan) -> bool {
        let key = Self::recording_key(&segment.uri, &span);
        if self.st().downloaded.contains(&key) {
            return true;
        }
        // 풀 수 없는 BYTERANGE(조각·MAP) — 다시 받아도 같으니 순서를 막지 않는다. 한 번만 처리.
        let init_invalid = segment
            .init_segment
            .as_ref()
            .is_some_and(|i| HlsByteSpan::of_init(i) == HlsByteSpan::Invalid);
        if span == HlsByteSpan::Invalid || init_invalid {
            let mut s = self.st();
            s.downloaded.insert(key);
            if segment.discontinuity {
                s.pending_init_restart = true;
            }
            return true;
        }
        if self.write_segment(segment, span, &key).await {
            let mut s = self.st();
            if s.blocked.as_ref().is_some_and(|(k, _)| *k == key) {
                s.blocked = None;
            }
            s.downloaded.insert(key);
            return true;
        }
        let mut s = self.st();
        let failures = match &s.blocked {
            Some((k, n)) if *k == key => n + 1,
            _ => 1,
        };
        if failures < MAX_BLOCKED_POLLS {
            s.blocked = Some((key, failures));
            return false;
        }
        tracing::warn!("세그먼트 {failures}회 폴 연속 실패 — 건너뛰고 다음 조각부터 기록 (빈틈)");
        s.blocked = None;
        // 불연속 조각을 init 없이 건너뛰면 인코더 재시작을 잊는다 — 기억해 둔다.
        if segment.discontinuity && s.init_rewritten_before.as_deref() != Some(key.as_str()) {
            s.pending_init_restart = true;
        }
        s.downloaded.insert(key);
        true
    }

    /// init(필요할 때)과 조각을 받아 쓴다. init 을 못 받으면 조각도 미룬다.
    async fn write_segment(&self, segment: &Segment, span: HlsByteSpan, key: &str) -> bool {
        if let Some(init) = &segment.init_segment {
            let init_span = HlsByteSpan::of_init(init);
            let init_key = Self::recording_key(&init.uri, &init_span);
            let needs_init = {
                let s = self.st();
                let restart = (segment.discontinuity
                    && s.init_rewritten_before.as_deref() != Some(key))
                    || s.pending_init_restart;
                s.last_written_init_key.as_deref() != Some(init_key.as_str()) || restart
            };
            if needs_init {
                if !self.write_init(init, init_span, init_key).await {
                    return false;
                }
                let mut s = self.st();
                if segment.discontinuity {
                    s.init_rewritten_before = Some(key.to_string());
                }
                s.pending_init_restart = false;
            }
        }
        let Some(mut data) = self.download(&segment.uri, span).await else {
            return false;
        };
        if let Some(enc) = &segment.encryption_info {
            match self.decrypt_segment(&data, enc).await {
                Ok(plain) => data = plain,
                Err(e) => {
                    tracing::warn!("세그먼트 복호화 실패: {e}");
                    return false;
                }
            }
        }
        match self.write_data(&data) {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!("세그먼트 쓰기 실패: {e}");
                false
            }
        }
    }

    async fn write_init(&self, init: &InitSegment, span: HlsByteSpan, key: String) -> bool {
        let Some(data) = self.download(&init.uri, span).await else {
            return false;
        };
        match self.write_data(&data) {
            Ok(()) => {
                self.st().last_written_init_key = Some(key);
                true
            }
            Err(e) => {
                tracing::warn!("init 세그먼트 쓰기 실패: {e}");
                false
            }
        }
    }

    /// 지수 백오프 재시도. 2xx 이고 구간이 맞는 본문만 돌려준다(4xx 본문은 쓰지 않는다).
    /// 최종 4xx 는 백오프 없이 이 폴에서 포기한다(다음 폴이 재시도).
    async fn download(&self, url: &Url, span: HlsByteSpan) -> Option<Vec<u8>> {
        let mut retry_delay = self.inner.retry_base_delay;
        for attempt in 0..MAX_DOWNLOAD_ATTEMPTS {
            if !self.still_recording() {
                return None;
            }
            let mut request = self.inner.client.get(url.clone());
            if let Some(range) = span.range_header() {
                request = request.header(reqwest::header::RANGE, range);
            }
            match request.send().await {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let content_range = resp
                        .headers()
                        .get(reqwest::header::CONTENT_RANGE)
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_string);
                    let body = resp.bytes().await.ok();
                    if let Some(b) = HlsByteSpan::recordable_body(
                        body.as_deref(),
                        status,
                        span.byte_range(),
                        content_range.as_deref(),
                    ) {
                        return Some(b.to_vec());
                    }
                    tracing::warn!(
                        "세그먼트 응답 거부 (HTTP {status}, 시도 {}/{MAX_DOWNLOAD_ATTEMPTS})",
                        attempt + 1
                    );
                    if HlsByteSpan::is_final_failure(status) {
                        return None;
                    }
                }
                Err(e) => tracing::warn!(
                    "세그먼트 다운로드 실패 (시도 {}/{MAX_DOWNLOAD_ATTEMPTS}): {e}",
                    attempt + 1
                ),
            }
            if attempt + 1 >= MAX_DOWNLOAD_ATTEMPTS {
                break;
            }
            tokio::time::sleep(retry_delay).await;
            retry_delay = (retry_delay * 2).min(MAX_RETRY_DELAY);
        }
        None
    }
}
