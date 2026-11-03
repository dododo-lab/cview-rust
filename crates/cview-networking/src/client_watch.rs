//! 라이브 시청 이벤트(`POST /polling/v1/watch-event/live`) — 치지직 웹 플레이어가 시청을 알리는 호출.
//!
//! 웹(`chzzk.naver.com` 번들의 시청 훅)은 영상이 재생되면 `WATCH_STARTED`, 다시 재생되면 `WATCH_RESUMED`,
//! 일시정지면 `WATCH_PAUSED`, 재생 중 30초마다 `WATCH_CONTINUED`, 끝나거나 페이지를 떠나면
//! `WATCH_ENDED` 를 보낸다. 본문의 `liveTokens` 는 `live-status`(v3.1) 응답의 `liveTokenList` 다.
//! 로그인 쿠키(NID_AUT·NID_SES)가 있으면 함께 실어 계정 시청 기록·시청 시간에 반영되게 한다.
//! 순서·주기·재생 시간 계산은 `cview-app-core` 의 `live_watch_events` 가 맡고, 여기는 전송만 한다.

use std::sync::OnceLock;

use chrono::{DateTime, FixedOffset, NaiveDateTime, TimeZone, Utc};
use cview_core::error::ApiError;
use cview_core::protocols::{HttpCookie, HttpMethod};
use serde_json::{json, Map, Value};
use url::Url;

use crate::client::{transport_to_api_error, ChzzkApiClient};
use crate::headers;
use crate::transport::HttpRequest;

const WATCH_EVENT_URL: &str = "https://api.chzzk.naver.com/polling/v1/watch-event/live";
/// 웹이 같은 방송의 시청 상태를 다시 읽는 경로(`liveTokenList` 를 준다).
const LIVE_STATUS_BASE: &str = "https://api.chzzk.naver.com/polling/v3.1/channels";
/// 웹 라이브 시청 화면의 `scene` 헤더 값(`LIVE_END` = 25).
const LIVE_SCENE: &str = "25";

/// 웹이 보내는 시청 이벤트 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WatchEventType {
    Started,
    Resumed,
    Continued,
    Paused,
    Ended,
}

impl WatchEventType {
    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Started => "WATCH_STARTED",
            Self::Resumed => "WATCH_RESUMED",
            Self::Continued => "WATCH_CONTINUED",
            Self::Paused => "WATCH_PAUSED",
            Self::Ended => "WATCH_ENDED",
        }
    }
}

/// 시청 이벤트 한 건.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveWatchEvent {
    pub channel_id: String,
    pub live_id: i64,
    pub event: WatchEventType,
    /// 한 번의 시청(재생 시작~종료)을 묶는 무작위 UUID.
    pub session_id: String,
    /// 이벤트 주기(초). 웹은 항상 30.
    pub duration_sec: i64,
    /// 방송 시작부터 지금까지(초).
    pub position_at: i64,
    /// 이번 시청에서 실제로 재생한 누적 시간(초). 0 이면 웹처럼 싣지 않는다.
    pub accumulated_watch_sec: i64,
    pub live_tokens: Vec<String>,
}

impl LiveWatchEvent {
    /// 웹과 같은 본문(`autoPlay`·`radioMode` 는 라이브 시청 화면 값 false).
    pub fn body(&self) -> Value {
        let mut payload = Map::new();
        payload.insert("watchEventType".into(), json!(self.event.raw_value()));
        payload.insert("sessionId".into(), json!(self.session_id));
        payload.insert("duration".into(), json!(self.duration_sec));
        payload.insert("positionAt".into(), json!(self.position_at));
        if self.accumulated_watch_sec > 0 {
            payload.insert("awt".into(), json!(self.accumulated_watch_sec));
        }
        json!({
            "channelId": self.channel_id,
            "liveId": self.live_id,
            "payload": Value::Object(payload),
            "liveTokens": self.live_tokens,
            "autoPlay": false,
            "radioMode": false,
        })
    }
}

/// 시청 이벤트에 필요한 방송 상태(`live-status` 에서).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LiveWatchStatus {
    pub is_open: bool,
    pub open_date: Option<DateTime<Utc>>,
    pub live_tokens: Vec<String>,
}

impl LiveWatchStatus {
    /// `live-status` 응답의 `content`.
    pub fn from_content(content: &Value) -> Self {
        Self {
            is_open: content.get("status").and_then(Value::as_str) == Some("OPEN"),
            open_date: content
                .get("openDate")
                .and_then(Value::as_str)
                .and_then(parse_kst),
            live_tokens: content
                .get("liveTokenList")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
}

/// 치지직 날짜(`2026-11-03 19:32:22`, 한국 시간) → UTC.
pub fn parse_kst(raw: &str) -> Option<DateTime<Utc>> {
    let naive = NaiveDateTime::parse_from_str(raw.trim(), "%Y-%m-%d %H:%M:%S").ok()?;
    let kst = FixedOffset::east_opt(9 * 3600)?;
    kst.from_local_datetime(&naive)
        .single()
        .map(|d| d.with_timezone(&Utc))
}

/// 이 실행 동안 쓰는 기기 식별자(`deviceId` 헤더) — 웹이 `ba.uuid` 쿠키에 두는 무작위 UUID 와 같은 역할.
pub fn watch_device_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(crate::client_ads::random_uuid_v4)
}

/// 웹 시청 세션 ID(무작위 UUID v4).
pub fn new_watch_session_id() -> String {
    crate::client_ads::random_uuid_v4()
}

impl ChzzkApiClient {
    fn watch_request(
        &self,
        method: HttpMethod,
        url: Url,
        cookies: Option<&[HttpCookie]>,
    ) -> HttpRequest {
        let mut req = HttpRequest::new(method, url);
        req.set_header("User-Agent", Some(headers::CHROME_USER_AGENT));
        req.set_header("Referer", Some(headers::CHZZK_REFERER));
        req.set_header("Origin", Some(headers::CHZZK_ORIGIN));
        req.set_header("Accept", Some("application/json, text/plain, */*"));
        // 웹 공용 헤더(axios 기본값과 같다).
        req.set_header("If-Modified-Since", Some("Mon, 26 Jul 1997 05:00:00 GMT"));
        req.set_header("Cache-Control", Some("no-cache"));
        req.set_header("Pragma", Some("no-cache"));
        req.set_header("deviceId", Some(watch_device_id()));
        req.set_header("Front-Client-Product-Type", Some("web"));
        req.set_header("Front-Client-Platform-Type", Some("PC"));
        if let Some(c) = cookies {
            req.set_header("Cookie", HttpCookie::header_value(c).as_deref());
        }
        req
    }

    /// 시청 이벤트용 방송 상태 — 로그인 쿠키를 실어 웹과 같은 `live-status`(v3.1)를 읽는다(캐시 안 함).
    pub async fn live_watch_status(&self, channel_id: &str) -> Result<LiveWatchStatus, ApiError> {
        let url = Url::parse(&format!("{LIVE_STATUS_BASE}/{channel_id}/live-status"))
            .map_err(|e| ApiError::NetworkError(e.to_string()))?;
        let cookies = self.auth_cookies().await;
        let req = self.watch_request(HttpMethod::Get, url, cookies.as_deref());
        let res = self.send(req, None).await.map_err(transport_to_api_error)?;
        if res.status != 200 {
            return Err(ApiError::http(res.status as i64));
        }
        let body: Value = serde_json::from_slice(&res.body)
            .map_err(|e| ApiError::DecodingFailed(e.to_string()))?;
        let content = body
            .get("content")
            .filter(|c| !c.is_null())
            .ok_or(ApiError::EmptyContent)?;
        Ok(LiveWatchStatus::from_content(content))
    }

    /// 시청 이벤트 전송. 로그인돼 있으면 쿠키를 실어 계정 시청으로 집계되게 한다.
    pub async fn post_live_watch_event(&self, event: &LiveWatchEvent) -> Result<(), ApiError> {
        let url = Url::parse(WATCH_EVENT_URL).map_err(|e| ApiError::NetworkError(e.to_string()))?;
        let cookies = self.auth_cookies().await;
        let mut req = self.watch_request(HttpMethod::Post, url, cookies.as_deref());
        req.set_header("Content-Type", Some("application/json"));
        req.set_header("scene", Some(LIVE_SCENE));
        req.body = Some(event.body().to_string().into_bytes());
        let res = self.send(req, None).await.map_err(transport_to_api_error)?;
        if !(200..300).contains(&res.status) {
            return Err(ApiError::http(res.status as i64));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: WatchEventType, awt: i64) -> LiveWatchEvent {
        LiveWatchEvent {
            channel_id: "ch".into(),
            live_id: 7,
            event: kind,
            session_id: "sid".into(),
            duration_sec: 30,
            position_at: 120,
            accumulated_watch_sec: awt,
            live_tokens: vec!["tok".into()],
        }
    }

    #[test]
    fn body_matches_web_shape() {
        assert_eq!(
            event(WatchEventType::Continued, 30).body(),
            json!({
                "channelId": "ch",
                "liveId": 7,
                "payload": {
                    "watchEventType": "WATCH_CONTINUED",
                    "sessionId": "sid",
                    "duration": 30,
                    "positionAt": 120,
                    "awt": 30,
                },
                "liveTokens": ["tok"],
                "autoPlay": false,
                "radioMode": false,
            })
        );
        // 누적 0 이면 웹처럼 awt 를 뺀다.
        let started = event(WatchEventType::Started, 0).body();
        assert!(started["payload"].get("awt").is_none());
        assert_eq!(started["payload"]["watchEventType"], "WATCH_STARTED");
    }

    #[test]
    fn status_reads_tokens_and_kst_open_date() {
        let s = LiveWatchStatus::from_content(&json!({
            "status": "OPEN",
            "openDate": "2026-11-03 19:32:22",
            "liveTokenList": ["a", "b"],
        }));
        assert!(s.is_open);
        assert_eq!(s.live_tokens, vec!["a", "b"]);
        assert_eq!(
            s.open_date.unwrap().to_rfc3339(),
            "2026-11-03T10:32:22+00:00"
        );
        let closed = LiveWatchStatus::from_content(&json!({"status": "CLOSE"}));
        assert!(!closed.is_open && closed.live_tokens.is_empty() && closed.open_date.is_none());
    }

    #[test]
    fn device_id_is_stable_uuid() {
        let id = watch_device_id();
        assert_eq!(id.len(), 36);
        assert_eq!(id, watch_device_id());
        assert_ne!(new_watch_session_id(), new_watch_session_id());
    }
}
