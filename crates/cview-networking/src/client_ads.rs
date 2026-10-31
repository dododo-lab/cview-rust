//! 치지직 프리롤 광고 — 치지직 웹 플레이어가 쓰는 공개 광고 경로를 그대로 따른다.
//!
//! 1. 광고 노출 여부: `GET /service/v1/seoraksan?pgType=CHZZK_LIVE|CHZZK_VIDEO&pgId=…` (로그인 쿠키 포함).
//!    서버가 계정(치트키 등)을 보고 프리롤·미드롤 노출을 정한다. 응답은 웹 번들처럼 두 모양을 모두 받는다.
//! 2. 광고 일정: 네이버 GFP `GET https://nam.veta.naver.com/vas` — 광고 단위(`adUnitId`)와 슬롯 수.
//! 3. 광고 목록: GFP `GET /gfp/v1` — 워터폴. `GV`(네이버 VAST 본문)와 `GOOGLE_IMA`(구글 애드 매니저 VAST 태그)를
//!    재생 후보로 돌려준다. 광고 자체의 재생·추적은 표준 VAST 로 UI 가 맡는다.
//!
//! 광고 응답은 암호화·우회 없이 받은 그대로 쓴다. 추적 URL 은 GFP 규칙대로 `encrypted` 토큰을 붙인다.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use cview_core::error::ApiError;
use cview_core::protocols::{HttpCookie, HttpMethod};
use rand::Rng;
use serde::Serialize;
use serde_json::Value;
use url::Url;

use crate::client::{transport_to_api_error, ChzzkApiClient};
use crate::headers;
use crate::transport::HttpRequest;

/// 네이버 GFP 광고 서버.
pub const GFP_BASE: &str = "https://nam.veta.naver.com";
/// 구글 애드 매니저 VAST 태그(웹 SDK 의 `IMA_TAG_URL`).
const IMA_TAG_URL: &str = "https://pubads.g.doubleclick.net/gampad/ads";
/// 광고 응답·VAST 본문 상한.
const MAX_AD_BODY: usize = 1024 * 1024;

/// 광고가 붙는 콘텐츠.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdContentKind {
    Live,
    Vod,
}

impl AdContentKind {
    fn pg_type(self) -> &'static str {
        match self {
            AdContentKind::Live => "CHZZK_LIVE",
            AdContentKind::Vod => "CHZZK_VIDEO",
        }
    }
}

/// 서버가 정한 광고 노출 여부.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdDisplayFlags {
    pub pre_roll: bool,
    pub mid_roll: bool,
}

/// `seoraksan` 응답의 `content` → 노출 여부. 웹 번들과 같이 `playerAdDisplayResponse` 가 있으면 그것을,
/// 없으면 같은 값이 다른 이름으로 담긴 `livePlaybackJson.{liveId, chatChannelId}` 를 읽는다.
pub fn parse_ad_display_flags(content: &Value) -> AdDisplayFlags {
    let flag = |v: &Value, key: &str| v.get(key).and_then(Value::as_bool).unwrap_or(false);
    if let Some(r) = content.get("playerAdDisplayResponse") {
        return AdDisplayFlags {
            pre_roll: flag(r, "preRoll"),
            mid_roll: flag(r, "midRoll"),
        };
    }
    match content.get("livePlaybackJson") {
        Some(r) => AdDisplayFlags {
            pre_roll: flag(r, "liveId"),
            mid_roll: flag(r, "chatChannelId"),
        },
        None => AdDisplayFlags::default(),
    }
}

/// 광고 요청에 실을 콘텐츠 정보 — 웹 플레이어의 `customParam`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdRequestContext {
    pub kind: AdContentKind,
    pub channel_id: String,
    /// 라이브는 liveId, 다시보기는 videoNo.
    pub content_id: String,
    /// 다시보기 videoId(라이브는 liveId 를 쓴다).
    pub video_id: String,
    /// 다시보기 videoType(소문자). 라이브는 `live`.
    pub video_type: String,
    pub category_type: String,
    pub category_id: String,
    /// 초. 라이브는 0.
    pub content_duration: i64,
    /// 응답의 `adParameter` — 그대로 덧붙인다(`tag` 등).
    pub ad_parameter: Vec<(String, String)>,
    /// 라이브 응답의 `skipPreRollAd`·`liveConnecting` — 웹은 이때 프리롤을 건너뛴다.
    pub skip_pre_roll: bool,
}

impl AdRequestContext {
    /// `live-detail` 응답 `content` 에서.
    pub fn from_live_detail(channel_id: &str, content: &Value) -> Option<Self> {
        let live_id = json_string(content.get("liveId")?)?;
        Some(Self {
            kind: AdContentKind::Live,
            channel_id: channel_id.to_string(),
            content_id: live_id.clone(),
            video_id: live_id,
            video_type: "live".into(),
            category_type: str_field(content, "categoryType"),
            category_id: str_field(content, "liveCategory"),
            content_duration: 0,
            ad_parameter: ad_parameter(content),
            skip_pre_roll: bool_field(content, "skipPreRollAd")
                || bool_field(content, "liveConnecting"),
        })
    }

    /// `videos/{videoNo}` 응답 `content` 에서.
    pub fn from_video_detail(content: &Value) -> Option<Self> {
        let video_no = json_string(content.get("videoNo")?)?;
        let channel_id = content
            .get("channel")
            .and_then(|c| c.get("channelId"))
            .and_then(json_string)
            .or_else(|| content.get("channelId").and_then(json_string))
            .unwrap_or_default();
        Some(Self {
            kind: AdContentKind::Vod,
            channel_id,
            content_id: video_no,
            video_id: str_field(content, "videoId"),
            video_type: str_field(content, "videoType").to_lowercase(),
            category_type: str_field(content, "categoryType"),
            category_id: str_field(content, "videoCategory"),
            content_duration: content.get("duration").and_then(Value::as_i64).unwrap_or(0),
            ad_parameter: ad_parameter(content),
            skip_pre_roll: false,
        })
    }

    /// 광고 일정 ID — 라이브는 이벤트 태그면 이벤트 일정.
    fn schedule_id(&self) -> &'static str {
        match self.kind {
            AdContentKind::Vod => "CHZZK_NDP_SCH",
            AdContentKind::Live
                if self
                    .ad_parameter
                    .iter()
                    .any(|(k, v)| k == "tag" && v == "asiangames") =>
            {
                "LIVE_CHZZK_NDP_SCH_EVENT"
            }
            AdContentKind::Live => "LIVE_CHZZK_NDP_SCH",
        }
    }

    /// 라이브 프리롤 자리 — 웹 `w1(…)` 과 같은 광고 단위·일정 ID·요청 ID.
    fn live_preroll_slot(&self) -> AdBreakSlot {
        let event = self.schedule_id() == "LIVE_CHZZK_NDP_SCH_EVENT";
        AdBreakSlot {
            ad_unit_id: if event {
                "event_w_live_chzzk_naver_va"
            } else {
                "w_live_chzzk_naver_va"
            }
            .into(),
            schedule_id: self.schedule_id().into(),
            request_id: format!("vas-{}", random_uuid_v4()),
            with_remind_ad: 0,
        }
    }

    /// 웹 플레이어 주소(`c`·`description_url`).
    fn page_url(&self) -> String {
        match self.kind {
            AdContentKind::Live => format!("https://chzzk.naver.com/live/{}", self.channel_id),
            AdContentKind::Vod => format!("https://chzzk.naver.com/video/{}", self.content_id),
        }
    }

    /// 웹 플레이어의 `customParam` 순서 그대로(`adParameter` 가 같은 키를 덮어쓴다).
    fn custom_params(&self) -> Vec<(String, String)> {
        let svc = match self.kind {
            AdContentKind::Live => "chzzk_live",
            AdContentKind::Vod => "chzzk_video",
        };
        let mut params: Vec<(String, String)> = [
            ("ac", "HTML5_PC"),
            ("lo", "Y"),
            ("cc", "N"),
            ("svc", svc),
            ("type", self.video_type.as_str()),
            ("cp", "chzzk"),
            ("chl", self.channel_id.as_str()),
            ("cl", self.content_id.as_str()),
            ("vid", self.video_id.as_str()),
            ("pcmo", "pc"),
            ("gcat", self.category_type.as_str()),
            ("cat", self.category_id.as_str()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        for (k, v) in &self.ad_parameter {
            match params.iter_mut().find(|(key, _)| key == k) {
                Some(slot) => slot.1 = v.clone(),
                None => params.push((k.clone(), v.clone())),
            }
        }
        params
    }
}

fn json_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn str_field(v: &Value, key: &str) -> String {
    v.get(key).and_then(json_string).unwrap_or_default()
}

fn bool_field(v: &Value, key: &str) -> bool {
    v.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn ad_parameter(content: &Value) -> Vec<(String, String)> {
    content
        .get("adParameter")
        .and_then(Value::as_object)
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| json_string(v).map(|s| (k.clone(), s)))
                .collect()
        })
        .unwrap_or_default()
}

fn encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}

/// GFP `p` 값 — `key:value,key:value`(빈 값 제외, 각각 URL 인코딩). `pcmo` 가 맨 앞이다.
pub fn gfp_custom_param(params: &[(String, String)]) -> String {
    let mut ordered: Vec<&(String, String)> = Vec::with_capacity(params.len());
    ordered.extend(params.iter().filter(|(k, _)| k == "pcmo"));
    ordered.extend(params.iter().filter(|(k, _)| k != "pcmo"));
    ordered
        .into_iter()
        .filter(|(k, v)| !k.trim().is_empty() && !v.trim().is_empty())
        .map(|(k, v)| format!("{}:{}", encode(k.trim()), encode(v.trim())))
        .collect::<Vec<_>>()
        .join(",")
}

/// 광고 일정의 한 광고 자리.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdBreakSlot {
    pub ad_unit_id: String,
    pub schedule_id: String,
    pub request_id: String,
    pub with_remind_ad: i64,
}

/// `/vas` 응답 → 프리롤 자리들(시작 지연 0 인 첫 광고 구간).
pub fn parse_preroll_schedule(body: &Value) -> Vec<AdBreakSlot> {
    let schedule_id = str_field(body, "videoAdScheduleId");
    let request_id = str_field(body, "requestId");
    let Some(adbreak) = body
        .get("adBreaks")
        .and_then(Value::as_array)
        .and_then(|b| {
            b.iter().find(|x| {
                x.get("startDelay").and_then(Value::as_f64).unwrap_or(0.0) <= 0.0
                    || str_field(x, "id").starts_with("PREROLL")
            })
        })
    else {
        return Vec::new();
    };
    let ad_unit_id = str_field(adbreak, "adUnitId");
    if ad_unit_id.is_empty() {
        return Vec::new();
    }
    adbreak
        .get("adSources")
        .and_then(Value::as_array)
        .map(|sources| {
            sources
                .iter()
                .map(|s| AdBreakSlot {
                    ad_unit_id: ad_unit_id.clone(),
                    schedule_id: schedule_id.clone(),
                    request_id: request_id.clone(),
                    with_remind_ad: s.get("withRemindAd").and_then(Value::as_i64).unwrap_or(0),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 재생 후보 광고 하나 — UI 의 VAST 플레이어가 받는다.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdCandidate {
    pub provider: String,
    /// 인라인 VAST 본문(네이버 GV).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vast_xml: Option<String>,
    /// VAST 태그 주소(구글 애드 매니저).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vast_url: Option<String>,
    /// 응답 타임아웃(밀리초) — 이 안에 재생을 시작하지 못하면 다음 후보로.
    pub timeout_ms: i64,
    /// GFP 가 허용한 건너뛰기 시점(초). VAST `skipoffset` 이 없을 때 쓴다. 없으면 건너뛸 수 없다.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_after: Option<f64>,
    /// GFP·광고사 추적 URL — 이벤트 이름(`ackImpressions` 등) → 완성된 URL 들.
    pub tracking: BTreeMap<String, Vec<String>>,
}

fn tracking_urls(v: Option<&Value>, suffix: &str) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    let Some(obj) = v.and_then(Value::as_object) else {
        return out;
    };
    for (event, list) in obj {
        let urls: Vec<String> = list
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|x| x.get("url").and_then(Value::as_str))
            .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
            .map(|u| format!("{u}{suffix}"))
            .collect();
        if !urls.is_empty() {
            out.insert(event.clone(), urls);
        }
    }
    out
}

/// 구글 애드 매니저 VAST 태그 — 웹 SDK `buildAdTagUrl` 과 같은 키.
fn ima_tag_url(ad_info: &Value, ctx: &AdRequestContext) -> Option<String> {
    let info = ad_info.get("sdkRequestInfo")?.as_object()?;
    let unit = info.get("AD_UNIT_CODE").and_then(json_string)?;
    let network = info.get("NETWORK_CODE").and_then(json_string)?;
    let size = ad_info
        .get("requestSizes")
        .and_then(Value::as_array)
        .and_then(|s| s.first())
        .map(|s| {
            format!(
                "{}x{}",
                s.get("width").and_then(Value::as_i64).unwrap_or(1280),
                s.get("height").and_then(Value::as_i64).unwrap_or(720)
            )
        })
        .unwrap_or_else(|| "1280x720".into());
    let cust = ctx
        .custom_params()
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");
    let mut url = Url::parse(IMA_TAG_URL).ok()?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("iu", &format!("/{network}/{unit}"));
        q.append_pair("sz", &size);
        q.append_pair("ciu_szs", "300x250,728x90,320x100");
        q.append_pair(
            "output",
            &info
                .get("output")
                .and_then(json_string)
                .unwrap_or_else(|| "vast".into()),
        );
        q.append_pair("cust_params", &cust);
        q.append_pair(
            "ad_rule",
            &info
                .get("ad_rule")
                .and_then(json_string)
                .unwrap_or_else(|| "0".into()),
        );
        q.append_pair("description_url", &ctx.page_url());
        q.append_pair("hl", "ko");
        q.append_pair("vpos", "preroll");
        for (k, v) in info {
            if matches!(
                k.as_str(),
                "AD_UNIT_CODE" | "NETWORK_CODE" | "ad_rule" | "output"
            ) {
                continue;
            }
            if let Some(v) = json_string(v) {
                q.append_pair(k, &v);
            }
        }
        q.append_pair(
            "correlator",
            &rand::thread_rng()
                .gen_range(1u64..9_007_199_254_740_991)
                .to_string(),
        );
    }
    Some(url.to_string())
}

/// `/gfp/v1` 워터폴 → 재생 후보(순서 유지). 재생할 수 없는 항목(`EMPTY` 등)은 뺀다.
pub fn parse_waterfall(body: &Value, ctx: &AdRequestContext) -> Vec<AdCandidate> {
    let skip_after = body.get("videoSkipAfter").and_then(Value::as_f64);
    let gfp_events = body.get("eventTracking");
    body.get("ads")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|ad| {
            let info = ad.get("adInfo").unwrap_or(&Value::Null);
            let (vast_xml, vast_url) = match ad.get("renderType").and_then(Value::as_str)? {
                "GV" => {
                    let adm = info.get("adm").and_then(Value::as_str)?;
                    if str_field(info, "encoded") == "1" || !adm.trim_start().starts_with('<') {
                        return None;
                    }
                    (Some(adm.to_string()), None)
                }
                "GOOGLE_IMA" => (None, Some(ima_tag_url(info, ctx)?)),
                _ => return None,
            };
            let encrypted = str_field(ad, "encrypted");
            let mut tracking = tracking_urls(gfp_events, &encrypted);
            for (event, urls) in tracking_urls(ad.get("eventTracking"), "") {
                tracking.entry(event).or_default().extend(urls);
            }
            Some(AdCandidate {
                provider: str_field(ad, "adProviderName"),
                vast_xml,
                vast_url,
                timeout_ms: info
                    .get("timeout")
                    .and_then(Value::as_i64)
                    .unwrap_or(10_000)
                    .clamp(3_000, 20_000),
                skip_after: skip_after.filter(|_| bool_field(ad, "vastSkippable")),
                tracking,
            })
        })
        .collect()
}

/// 무작위 UUID v4 문자열.
fn random_uuid_v4() -> String {
    let mut b = [0u8; 16];
    rand::thread_rng().fill(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// 이 실행 동안 쓰는 GFP 광고 식별자(`iv`) — 웹 SDK 가 브라우저에 두는 무작위 UUID 와 같은 역할.
fn install_id() -> &'static str {
    static IV: OnceLock<String> = OnceLock::new();
    IV.get_or_init(random_uuid_v4)
}

impl ChzzkApiClient {
    async fn ad_get(&self, url: Url, with_cookies: bool) -> Result<Vec<u8>, ApiError> {
        let mut req = HttpRequest::new(HttpMethod::Get, url);
        req.set_header("User-Agent", Some(headers::CHROME_USER_AGENT));
        req.set_header("Referer", Some(headers::CHZZK_REFERER));
        req.set_header("Origin", Some(headers::CHZZK_ORIGIN));
        if with_cookies {
            if let Some(cookies) = self.auth_cookies().await {
                req.set_header("Cookie", HttpCookie::header_value(&cookies).as_deref());
            }
        }
        let res = self.send(req, None).await.map_err(transport_to_api_error)?;
        if res.status != 200 {
            return Err(ApiError::http(res.status as i64));
        }
        if res.body.len() > MAX_AD_BODY {
            return Err(ApiError::DecodingFailed("광고 응답이 너무 큽니다".into()));
        }
        Ok(res.body)
    }

    async fn ad_get_json(&self, url: Url, with_cookies: bool) -> Result<Value, ApiError> {
        let body = self.ad_get(url, with_cookies).await?;
        serde_json::from_slice(&body).map_err(|e| ApiError::DecodingFailed(e.to_string()))
    }

    async fn chzzk_content(&self, path: &str) -> Result<Value, ApiError> {
        let url = Url::parse(&format!("https://api.chzzk.naver.com{path}"))
            .map_err(|e| ApiError::NetworkError(e.to_string()))?;
        let body = self.ad_get_json(url, true).await?;
        body.get("content")
            .filter(|c| !c.is_null())
            .cloned()
            .ok_or(ApiError::EmptyContent)
    }

    /// 서버가 정한 광고 노출 여부(로그인 쿠키 포함 — 치트키 계정은 서버가 끈다).
    pub async fn ad_display_flags(
        &self,
        kind: AdContentKind,
        pg_id: &str,
    ) -> Result<AdDisplayFlags, ApiError> {
        let mut url = Url::parse("https://api.chzzk.naver.com/service/v1/seoraksan")
            .map_err(|e| ApiError::NetworkError(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("pgType", kind.pg_type())
            .append_pair("pgId", pg_id);
        let body = self.ad_get_json(url, true).await?;
        Ok(parse_ad_display_flags(
            body.get("content").unwrap_or(&Value::Null),
        ))
    }

    /// 광고 요청 정보(라이브 상세·다시보기 상세의 원본 응답에서).
    pub async fn ad_request_context(
        &self,
        kind: AdContentKind,
        id: &str,
    ) -> Result<AdRequestContext, ApiError> {
        let ctx = match kind {
            AdContentKind::Live => {
                let content = self
                    .chzzk_content(&format!("/service/v3/channels/{id}/live-detail"))
                    .await?;
                AdRequestContext::from_live_detail(id, &content)
            }
            AdContentKind::Vod => {
                let content = self
                    .chzzk_content(&format!("/service/v3/videos/{id}"))
                    .await?;
                AdRequestContext::from_video_detail(&content)
            }
        };
        ctx.ok_or(ApiError::EmptyContent)
    }

    /// 프리롤 광고 후보 — 일정의 각 자리마다 워터폴을 받아 순서대로 이어 붙인다(자리마다 최대 1편 재생).
    pub async fn preroll_ads(
        &self,
        ctx: &AdRequestContext,
    ) -> Result<Vec<Vec<AdCandidate>>, ApiError> {
        let slots = match ctx.kind {
            // 라이브 프리롤은 웹도 /vas 를 부르지 않고 고정 광고 단위 하나로 일정을 만든다(`w1`).
            // 라이브 일정 ID 로 /vas 를 부르면 빈 일정이 와서 광고가 나오지 않았다.
            AdContentKind::Live => vec![ctx.live_preroll_slot()],
            AdContentKind::Vod => {
                let mut vas = Url::parse(&format!("{GFP_BASE}/vas"))
                    .map_err(|e| ApiError::NetworkError(e.to_string()))?;
                vas.query_pairs_mut()
                    .append_pair("vsi", ctx.schedule_id())
                    .append_pair("vcl", &ctx.content_duration.to_string())
                    .append_pair("so", "0")
                    .append_pair("rl", "pre:1,mid:0,post:0")
                    .append_pair("ct", "web");
                parse_preroll_schedule(&self.ad_get_json(vas, false).await?)
            }
        };
        let custom = gfp_custom_param(&ctx.custom_params());
        let mut out = Vec::new();
        for (seq, slot) in slots.iter().enumerate().take(3) {
            let mut url = Url::parse(&format!("{GFP_BASE}/gfp/v1"))
                .map_err(|e| ApiError::NetworkError(e.to_string()))?;
            url.query_pairs_mut()
                .append_pair("u", &slot.ad_unit_id)
                .append_pair("vsi", &slot.schedule_id)
                .append_pair("vri", &slot.request_id)
                .append_pair("vcl", &ctx.content_duration.to_string())
                .append_pair("vsd", "0")
                .append_pair("vrr", &slot.with_remind_ad.to_string())
                .append_pair("xdid", "")
                .append_pair("p", &custom)
                .append_pair("iv", install_id())
                .append_pair("ivt", "web")
                .append_pair("c", &ctx.page_url())
                .append_pair("sn", "web")
                .append_pair("an", "chzzk.naver.com")
                .append_pair("rui", &rand::thread_rng().gen::<u32>().to_string())
                .append_pair("pp", &format!("seq:{seq}"));
            match self.ad_get_json(url, false).await {
                Ok(body) => {
                    let candidates = parse_waterfall(&body, ctx);
                    if !candidates.is_empty() {
                        out.push(candidates);
                    }
                }
                Err(e) => tracing::debug!("광고 워터폴 실패: {e}"),
            }
        }
        Ok(out)
    }

    /// VAST 래퍼·구글 태그를 받는다 — https 만, 1 MiB 이하, XML 본문만.
    pub async fn ad_fetch_vast(&self, url: &str) -> Result<String, ApiError> {
        let url = Url::parse(url).map_err(|e| ApiError::NetworkError(e.to_string()))?;
        if url.scheme() != "https" {
            return Err(ApiError::NetworkError("https 광고 주소만 받습니다".into()));
        }
        let body = self.ad_get(url, false).await?;
        let text = String::from_utf8_lossy(&body).into_owned();
        if !text.trim_start().starts_with('<') {
            return Err(ApiError::DecodingFailed("VAST 응답이 아닙니다".into()));
        }
        Ok(text)
    }

    /// 광고 노출·클릭 추적 — 응답은 보지 않는다.
    pub async fn ad_ping(&self, url: &str) {
        let Ok(url) = Url::parse(url) else { return };
        if !matches!(url.scheme(), "https" | "http") {
            return;
        }
        if let Err(e) = self.ad_get(url, false).await {
            tracing::debug!("광고 추적 실패: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn display_flags_both_shapes() {
        assert_eq!(
            parse_ad_display_flags(
                &json!({"playerAdDisplayResponse":{"preRoll":true,"midRoll":false}})
            ),
            AdDisplayFlags {
                pre_roll: true,
                mid_roll: false
            }
        );
        assert_eq!(
            parse_ad_display_flags(
                &json!({"livePlaybackJson":{"liveId":false,"chatChannelId":true}})
            ),
            AdDisplayFlags {
                pre_roll: false,
                mid_roll: true
            }
        );
        assert_eq!(
            parse_ad_display_flags(&Value::Null),
            AdDisplayFlags::default()
        );
    }

    fn live_ctx() -> AdRequestContext {
        AdRequestContext::from_live_detail(
            "ch1",
            &json!({"liveId": 77, "categoryType":"GAME", "liveCategory":"lol",
                    "adParameter":{"tag":"none"}, "skipPreRollAd": false}),
        )
        .unwrap()
    }

    #[test]
    fn custom_param_matches_web_order() {
        let ctx = live_ctx();
        assert_eq!(ctx.schedule_id(), "LIVE_CHZZK_NDP_SCH");
        assert!(!ctx.skip_pre_roll);
        assert_eq!(
            gfp_custom_param(&ctx.custom_params()),
            "pcmo:pc,ac:HTML5_PC,lo:Y,cc:N,svc:chzzk_live,type:live,cp:chzzk,chl:ch1,cl:77,vid:77,gcat:GAME,cat:lol,tag:none"
        );
        let vod = AdRequestContext::from_video_detail(&json!({
            "videoNo": 5, "videoId": "AB", "videoType": "REPLAY", "duration": 600,
            "channel": {"channelId": "ch2"}, "adParameter": {"tag": "x y"}
        }))
        .unwrap();
        assert_eq!(vod.schedule_id(), "CHZZK_NDP_SCH");
        assert_eq!(vod.content_duration, 600);
        assert_eq!(
            gfp_custom_param(&vod.custom_params()),
            "pcmo:pc,ac:HTML5_PC,lo:Y,cc:N,svc:chzzk_video,type:replay,cp:chzzk,chl:ch2,cl:5,vid:AB,tag:x%20y"
        );
        let skip =
            AdRequestContext::from_live_detail("c", &json!({"liveId":1,"liveConnecting":true}))
                .unwrap();
        assert!(skip.skip_pre_roll);
    }

    #[test]
    fn schedule_picks_preroll_slots() {
        let body = json!({"requestId":"vas-1","videoAdScheduleId":"CHZZK_NDP_SCH","adBreaks":[
            {"id":"MID-0","startDelay":300,"adUnitId":"mid","adSources":[{"id":"m"}]},
            {"id":"PREROLL-0","startDelay":0,"adUnitId":"w_chzzk_naver_va",
             "adSources":[{"id":"PREROLL-0-0","withRemindAd":0},{"id":"PREROLL-0-1","withRemindAd":1}]}]});
        let slots = parse_preroll_schedule(&body);
        assert_eq!(slots.len(), 2);
        assert_eq!(slots[0].ad_unit_id, "w_chzzk_naver_va");
        assert_eq!(slots[1].with_remind_ad, 1);
        assert_eq!(slots[0].request_id, "vas-1");
        assert!(parse_preroll_schedule(&json!({"adBreaks":[]})).is_empty());
    }

    #[test]
    fn waterfall_keeps_playable_ads_with_tracking() {
        let body = json!({
        "videoSkipAfter": 5,
        "eventTracking": {"ackImpressions":[{"url":"https://tivan.naver.com/sc2/1/"}],
                          "clicks":[{"url":"https://tivan.naver.com/sc2/3/"}]},
        "ads": [
            {"encrypted":"ENC","renderType":"GV","adProviderName":"NDP Video","vastSkippable":true,
             "eventTracking":{"ackImpressions":[{"url":"https://siape.veta.naver.com/ack"}]},
             "adInfo":{"adm":"<?xml version=\"1.0\"?><VAST version=\"3.0\"></VAST>","encoded":"0"}},
            {"encrypted":"E2","renderType":"GOOGLE_IMA","adProviderName":"Google IMA",
             "adInfo":{"timeout":10000,"requestSizes":[{"width":1280,"height":720}],
                       "sdkRequestInfo":{"AD_UNIT_CODE":"WEB_CHZZK_VIDEO","NETWORK_CODE":"123",
                                         "env":"vp","gdfp_req":"1","output":"vast"}}},
            {"encrypted":"E3","renderType":"EMPTY","adInfo":{}}
        ]});
        let ads = parse_waterfall(&body, &live_ctx());
        assert_eq!(ads.len(), 2);
        assert!(ads[0].vast_xml.as_deref().unwrap().contains("<VAST"));
        assert_eq!(ads[0].skip_after, Some(5.0));
        assert_eq!(
            ads[0].tracking["ackImpressions"],
            vec![
                "https://tivan.naver.com/sc2/1/ENC".to_string(),
                "https://siape.veta.naver.com/ack".to_string()
            ]
        );
        assert_eq!(
            ads[0].tracking["clicks"],
            vec!["https://tivan.naver.com/sc2/3/ENC".to_string()]
        );
        let tag = Url::parse(ads[1].vast_url.as_deref().unwrap()).unwrap();
        let q: BTreeMap<String, String> = tag.query_pairs().into_owned().collect();
        assert_eq!(tag.host_str(), Some("pubads.g.doubleclick.net"));
        assert_eq!(q["iu"], "/123/WEB_CHZZK_VIDEO");
        assert_eq!(q["sz"], "1280x720");
        assert_eq!(q["output"], "vast");
        assert_eq!(q["env"], "vp");
        assert_eq!(q["vpos"], "preroll");
        assert!(q["cust_params"].contains("svc=chzzk_live"));
        assert_eq!(ads[1].skip_after, None);
        assert_eq!(
            ads[1].tracking["ackImpressions"],
            vec!["https://tivan.naver.com/sc2/1/E2".to_string()]
        );
    }

    #[test]
    fn install_id_is_uuid_v4() {
        let id = install_id();
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
        assert_eq!(id, install_id());
    }

    #[test]
    fn live_preroll_uses_fixed_web_ad_unit() {
        let slot = live_ctx().live_preroll_slot();
        assert_eq!(slot.ad_unit_id, "w_live_chzzk_naver_va");
        assert_eq!(slot.schedule_id, "LIVE_CHZZK_NDP_SCH");
        assert!(slot.request_id.starts_with("vas-"));
        assert_eq!(slot.request_id.len(), 40);
        assert_eq!(slot.with_remind_ad, 0);

        let mut event = live_ctx();
        event.ad_parameter.push(("tag".into(), "asiangames".into()));
        let slot = event.live_preroll_slot();
        assert_eq!(slot.ad_unit_id, "event_w_live_chzzk_naver_va");
        assert_eq!(slot.schedule_id, "LIVE_CHZZK_NDP_SCH_EVENT");
    }
}
