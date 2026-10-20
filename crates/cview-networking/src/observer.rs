//! 요청 관찰자 — 앱의 디버그 로그(perf_log)가 요청마다 경로·상태·시간·크기·캐시 적중을 받는다.
//!
//! 관찰자가 없으면(디버그 로그 꺼짐) 원자 깃발 하나만 보고 돌아간다. 경로는 쿼리·조각을 뺀 것만 넘긴다
//! (쿠키·토큰은 헤더에만 있고 여기로 오지 않는다).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

/// 요청 하나의 기록.
#[derive(Debug, Clone)]
pub struct RequestRecord {
    pub method: &'static str,
    /// 호스트 + 경로(쿼리 제외).
    pub path: String,
    /// HTTP 상태(전송 실패면 `None`).
    pub status: Option<u16>,
    /// 전송 실패 종류(`timeout`·`connection_lost`·`not_connected`·`cancelled`·`other`).
    pub error: Option<&'static str>,
    pub duration_ms: f64,
    pub bytes: usize,
    /// 응답 캐시에서 바로 돌려줌(네트워크 없음).
    pub cache_hit: bool,
}

type Observer = Arc<dyn Fn(&RequestRecord) + Send + Sync>;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static OBSERVER: RwLock<Option<Observer>> = RwLock::new(None);

/// 관찰자를 건다/뗀다(`None`).
pub fn set_request_observer(observer: Option<Observer>) {
    let on = observer.is_some();
    *OBSERVER.write().unwrap_or_else(|e| e.into_inner()) = observer;
    ACTIVE.store(on, Ordering::Release);
}

/// 관찰자가 있는지 — 기록을 만들기 전에 본다(꺼져 있으면 문자열도 만들지 않게).
#[inline]
pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Acquire)
}

pub(crate) fn observe(record: &RequestRecord) {
    if !is_active() {
        return;
    }
    if let Some(f) = OBSERVER.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        f(record);
    }
}

/// URL → 호스트 + 경로(쿼리·조각 제외). 순수 함수.
pub fn redacted_path(url: &url::Url) -> String {
    format!("{}{}", url.host_str().unwrap_or(""), url.path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_drops_query_and_observer_toggles() {
        let u = url::Url::parse("https://api.chzzk.naver.com/service/v1/lives?size=1&token=abc")
            .unwrap();
        assert_eq!(redacted_path(&u), "api.chzzk.naver.com/service/v1/lives");
        let seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let s = seen.clone();
        set_request_observer(Some(Arc::new(move |_| {
            s.fetch_add(1, Ordering::SeqCst);
        })));
        let rec = RequestRecord {
            method: "GET",
            path: "x".into(),
            status: Some(200),
            error: None,
            duration_ms: 1.0,
            bytes: 1,
            cache_hit: false,
        };
        observe(&rec);
        set_request_observer(None);
        observe(&rec);
        assert_eq!(seen.load(Ordering::SeqCst), 1);
    }
}
