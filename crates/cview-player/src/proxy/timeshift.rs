//! `Sources/CViewPlayer/LocalStreamProxy+Timeshift.swift` 포팅 — 로컬 타임시프트 연결부.
//!
//! 프록시가 hls.js 에 넘기는 라이브 조각을 레코더가 기록하고, 되감으면 `/_ts/<sid>/…` 경로로 기록을
//! 다시 서빙한다. 이 모듈은 레코더를 추상 trait([`TimeshiftBackend`])으로 받는다 — 레코더 구현은
//! 프록시와 독립적으로 붙일 수 있다.

use url::Url;

/// 로컬 타임시프트 경로 접두사.
pub const TIMESHIFT_PATH_PREFIX: &str = "/_ts/";

/// 링 슬롯 대상(Swift `TimeshiftRing.TeeTarget`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TeeTarget {
    Segment { seq: i64 },
    InitSegment { id: i64 },
}

/// 복사본을 넘길 곳 — 링 슬롯 또는 LL 파트 캐시.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TeeDestination {
    Slot(TeeTarget),
    Part { key: String },
}

/// 프록시가 쓰는 레코더 면(Swift `TimeshiftRecorder` 의 공개 API 일부).
pub trait TimeshiftBackend: Send + Sync {
    fn session_id(&self) -> String;
    fn local_playlist(&self, start_offset: Option<f64>) -> Option<String>;
    fn local_segment(&self, seq: i64) -> Option<Vec<u8>>;
    fn local_init(&self, id: i64) -> Option<Vec<u8>>;
    /// hls.js 가 받은 재생목록을 본다(`from_client = true`).
    fn observe_playlist(&self, text: &str, url: &Url, from_client: bool, base_url: Option<&Url>);
    /// 기록할 조각이면 대상(세그먼트·init 먼저, 아니면 LL 파트).
    fn tee_destination(&self, upstream_url: &Url) -> Option<TeeDestination>;
    fn commit(&self, destination: &TeeDestination, data: Vec<u8>);
    fn abandon(&self, destination: &TeeDestination);
    fn stop(&self);

    // 코디네이터(`StreamCoordinator+Timeshift`)가 쓰는 면 — 기본값은 기록 없는 레코더.

    /// 되감을 수 있는 기록 길이(초) — Swift `window.recordedDuration`.
    fn recorded_duration(&self) -> f64 {
        0.0
    }
    /// 기록 한도 변경.
    fn update_limits(&self, _max_duration: f64, _budget_limit_bytes: i64) {}
    /// 레코더의 상류 직접 요청 허용 여부(재생·버퍼링 중이고 로딩이 멈추지 않았을 때만).
    fn set_upstream_fetch_enabled(&self, _enabled: bool) {}
    /// 라이브 복귀 — 로컬 재생 위치 표식을 지운다.
    fn clear_served_position(&self) {}
}

/// `/_ts/` 요청 종류.
#[derive(Debug, Clone, PartialEq)]
pub enum TimeshiftResource {
    Playlist { start_offset: Option<f64> },
    Segment { seq: i64 },
    InitSegment { id: i64 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimeshiftRequest {
    pub session_id: String,
    pub resource: TimeshiftResource,
}

fn parse_index(text: &str, ext: &str) -> Option<i64> {
    let digits = text.strip_suffix(ext)?;
    // Swift `Int(String)` 은 앞의 '+'/'-' 부호를 받는다 — 음수는 아래에서 거른다.
    let v: i64 = digits.parse().ok()?;
    (v >= 0).then_some(v)
}

/// `/_ts/<sid>/index.m3u8?start=S` · `/_ts/<sid>/seg/<n>.m4v` · `/_ts/<sid>/init/<n>.mp4`.
pub fn parse_timeshift_path(path: &str) -> Option<TimeshiftRequest> {
    let rest = path.strip_prefix(TIMESHIFT_PATH_PREFIX)?;
    let (path_part, query) = match rest.find('?') {
        Some(q) => (&rest[..q], Some(&rest[q + 1..])),
        None => (rest, None),
    };
    let parts: Vec<&str> = path_part.split('/').collect();
    if parts.len() < 2 || parts[0].is_empty() {
        return None;
    }
    let session_id = parts[0].to_string();
    if parts.len() == 2 && parts[1] == "index.m3u8" {
        let start_offset = query
            .and_then(|q| q.split('&').find(|p| p.starts_with("start=")))
            .and_then(|p| p[6..].parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v >= 0.0);
        return Some(TimeshiftRequest {
            session_id,
            resource: TimeshiftResource::Playlist { start_offset },
        });
    }
    if parts.len() != 3 {
        return None;
    }
    let resource = match parts[1] {
        "seg" => TimeshiftResource::Segment {
            seq: parse_index(parts[2], ".m4v")?,
        },
        "init" => TimeshiftResource::InitSegment {
            id: parse_index(parts[2], ".mp4")?,
        },
        _ => return None,
    };
    Some(TimeshiftRequest {
        session_id,
        resource,
    })
}

/// hls.js 가 받는 세그먼트 하나의 복사본. 온전히 받은 200 응답만 기록한다.
pub struct TimeshiftTee {
    backend: std::sync::Arc<dyn TimeshiftBackend>,
    pub destination: TeeDestination,
    buffer: Vec<u8>,
    finished: bool,
    expected_length: i64,
}

impl TimeshiftTee {
    pub fn new(backend: std::sync::Arc<dyn TimeshiftBackend>, destination: TeeDestination) -> Self {
        Self {
            backend,
            destination,
            buffer: Vec::new(),
            finished: false,
            expected_length: -1,
        }
    }

    /// 응답 헤더. 기록은 200 전체 응답만.
    pub fn note_response(&mut self, status: u16, expected_length: i64) {
        if status != 200 {
            self.abandon();
            return;
        }
        self.expected_length = expected_length;
    }

    pub fn append(&mut self, data: &[u8]) {
        if !self.finished {
            self.buffer.extend_from_slice(data);
        }
    }

    /// 전송 종료 — 본문이 있고 길이가 맞을 때만 기록, 아니면 비운다.
    pub fn finish(&mut self, transfer_succeeded: bool) {
        let intact = !self.buffer.is_empty()
            && (self.expected_length < 0 || self.buffer.len() as i64 == self.expected_length);
        if transfer_succeeded && intact {
            self.commit();
        } else {
            self.abandon();
        }
    }

    pub fn commit(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.backend
            .commit(&self.destination, std::mem::take(&mut self.buffer));
    }

    pub fn abandon(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.buffer.clear();
        self.backend.abandon(&self.destination);
    }
}

impl Drop for TimeshiftTee {
    fn drop(&mut self) {
        // 완료 콜백 없이 사라지면(클라이언트 이탈 등) 레코더가 나중에 직접 받게 비워 둔다.
        self.abandon();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn parses_timeshift_paths() {
        assert_eq!(
            parse_timeshift_path("/_ts/abc/index.m3u8?start=12.5"),
            Some(TimeshiftRequest {
                session_id: "abc".into(),
                resource: TimeshiftResource::Playlist {
                    start_offset: Some(12.5)
                }
            })
        );
        assert_eq!(
            parse_timeshift_path("/_ts/abc/index.m3u8?start=-1")
                .unwrap()
                .resource,
            TimeshiftResource::Playlist { start_offset: None }
        );
        assert_eq!(
            parse_timeshift_path("/_ts/abc/seg/7.m4v").unwrap().resource,
            TimeshiftResource::Segment { seq: 7 }
        );
        assert_eq!(
            parse_timeshift_path("/_ts/abc/init/2.mp4")
                .unwrap()
                .resource,
            TimeshiftResource::InitSegment { id: 2 }
        );
        for bad in [
            "/_ts/",
            "/_ts//index.m3u8",
            "/_ts/abc/seg/x.m4v",
            "/_ts/abc/seg/-1.m4v",
            "/_ts/abc/seg/1.mp4",
            "/_ts/abc/other/1.m4v",
            "/_ts/abc/seg/1/2.m4v",
            "/x/abc/index.m3u8",
        ] {
            assert!(parse_timeshift_path(bad).is_none(), "{bad}");
        }
    }

    #[derive(Default)]
    struct Recorder {
        log: Mutex<Vec<String>>,
    }

    impl TimeshiftBackend for Recorder {
        fn session_id(&self) -> String {
            "s".into()
        }
        fn local_playlist(&self, _: Option<f64>) -> Option<String> {
            None
        }
        fn local_segment(&self, _: i64) -> Option<Vec<u8>> {
            None
        }
        fn local_init(&self, _: i64) -> Option<Vec<u8>> {
            None
        }
        fn observe_playlist(&self, _: &str, _: &Url, _: bool, _: Option<&Url>) {}
        fn tee_destination(&self, _: &Url) -> Option<TeeDestination> {
            None
        }
        fn commit(&self, _: &TeeDestination, data: Vec<u8>) {
            self.log
                .lock()
                .unwrap()
                .push(format!("commit {}", data.len()));
        }
        fn abandon(&self, _: &TeeDestination) {
            self.log.lock().unwrap().push("abandon".into());
        }
        fn stop(&self) {}
    }

    #[test]
    fn tee_commits_only_intact_200() {
        let rec = Arc::new(Recorder::default());
        let dest = TeeDestination::Slot(TeeTarget::Segment { seq: 1 });
        let mut t = TimeshiftTee::new(rec.clone(), dest.clone());
        t.note_response(200, 4);
        t.append(b"abcd");
        t.finish(true);
        let mut short = TimeshiftTee::new(rec.clone(), dest.clone());
        short.note_response(200, 10);
        short.append(b"abc");
        short.finish(true);
        let mut partial = TimeshiftTee::new(rec.clone(), dest.clone());
        partial.note_response(206, 4);
        partial.append(b"abcd");
        partial.finish(true);
        drop(TimeshiftTee::new(rec.clone(), dest));
        assert_eq!(
            *rec.log.lock().unwrap(),
            vec!["commit 4", "abandon", "abandon", "abandon"]
        );
    }
}
