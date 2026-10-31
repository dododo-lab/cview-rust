//! 썸네일 호버 미리보기 — 카드 위에 잠시 머물면 소리 없이 짧게 재생할 주소를 돌려준다.
//!
//! 미리보기는 작은 썸네일 자리에서 잠깐 보는 것이라 본 재생 경로(저지연 제어)를 타지 않는다. 대신
//! 미리보기 전용 로컬 프록시([`LocalStreamProxy`])를 거친 주소를 돌려준다. Windows WebView2 는 페이지
//! 출처(`http://tauri.localhost/`)를 Referer 로 보내 CDN 이 거절하고, HLS 를 직접 못 틀어 hls.js 로
//! 받으면 CDN CORS 에 막혔다(macOS `tauri://` 는 Referer 를 안 보내 바로 됐다). 프록시는 치지직
//! Referer·Origin 을 넣고 CORS·재생목록 주소 재작성·잘못된 Content-Type 교정을 해 준다.
//! 가벼운 주소를 고른다:
//! - 라이브: 표준 HLS(LL-HLS 보다 요청이 적다). 암호화(`hls_aes`) 방송은 인증 프록시가 있어야 해서
//!   미리보기를 하지 않는다. 19 세 방송도 하지 않는다.
//! - 다시보기·클립: 단일 MP4(어느 WebView 든 바로 재생·탐색)의 낮은 화질, 없으면 HLS 낮은 화질.
//!
//! 미리보기를 할 수 없으면 `None` — 화면은 썸네일을 그대로 둔다.

use cview_core::models::live::LiveStatus;
use std::sync::OnceLock;

use cview_core::models::vod::VodQuality;
use cview_player::proxy::LocalStreamProxy;
use cview_player::vod::{resolve_clip_streams, VodStreamResolver};
use serde::Serialize;
use tauri::State;
use url::Url;

use super::CmdResult;
use crate::playback;
use crate::state::AppServices;

/// 미리보기 화질 하한(세로 픽셀) — 카드 썸네일은 300px 안팎이라 360p 면 충분하다.
const PREVIEW_MIN_HEIGHT: u32 = 360;
/// 다시보기는 앞부분(인트로·대기 화면)을 건너뛰어 이만큼 지난 곳에서 시작한다(길이 비율, 상한 초).
const VOD_START_RATIO: f64 = 0.15;
const VOD_START_MAX_SEC: f64 = 600.0;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSourceDto {
    pub url: String,
    /// `hls` / `mp4` — 화면이 재생 방식을 고른다(HLS 를 직접 못 트는 WebView 는 hls.js).
    pub format: &'static str,
    /// 시작 위치(초). 라이브는 0.
    pub start_at: f64,
    pub live: bool,
}

/// `1280x720` / `720p` / `720` → 세로 픽셀. 모르면 None.
fn quality_height(q: &VodQuality) -> Option<u32> {
    let r = q.resolution.trim().to_ascii_lowercase();
    let part = match r.split_once('x') {
        Some((_, h)) => h,
        None => r.trim_end_matches('p'),
    };
    part.trim().parse().ok().or_else(|| {
        let n = q.name.trim().to_ascii_lowercase();
        n.trim_end_matches('p').parse().ok()
    })
}

/// 미리보기에 쓸 화질 — 하한 이상에서 가장 낮은 것, 그런 게 없으면 가장 높은 것(하한보다 작은 것뿐이면).
/// 높이를 모르는 목록이면 대역폭이 가장 작은 것.
pub fn preview_quality(list: &[VodQuality]) -> Option<&VodQuality> {
    let known: Vec<(&VodQuality, u32)> = list
        .iter()
        .filter_map(|q| quality_height(q).map(|h| (q, h)))
        .collect();
    if known.is_empty() {
        return list
            .iter()
            .filter(|q| q.bandwidth > 0)
            .min_by_key(|q| q.bandwidth)
            .or_else(|| list.last());
    }
    known
        .iter()
        .filter(|(_, h)| *h >= PREVIEW_MIN_HEIGHT)
        .min_by_key(|(_, h)| *h)
        .or_else(|| known.iter().max_by_key(|(_, h)| *h))
        .map(|(q, _)| *q)
}

/// 다시보기 미리보기 시작 위치 — 1분 미만이면 처음부터.
pub fn vod_preview_start(duration: f64) -> f64 {
    if !duration.is_finite() || duration < 60.0 {
        return 0.0;
    }
    (duration * VOD_START_RATIO).min(VOD_START_MAX_SEC).floor()
}

/// 단일 MP4 가 있으면 그것, 없으면 HLS 화질 목록, 마지막으로 마스터.
fn pick(progressive: &[VodQuality], hls: &[VodQuality], master: &Url) -> (Url, &'static str) {
    if let Some(q) = preview_quality(progressive) {
        return (q.url.clone(), "mp4");
    }
    let url = preview_quality(hls).map(|q| &q.url).unwrap_or(master);
    (url.clone(), "hls")
}

/// 미리보기 전용 프록시 — 앱 수명 동안 하나. 미리보기는 늘 하나라 본 재생 프록시와 따로 둔다.
/// 암호화 방송은 미리보기를 안 하므로 로그인 쿠키는 넘기지 않는다.
static PREVIEW_PROXY: OnceLock<LocalStreamProxy> = OnceLock::new();

/// 허용 CDN 주소면 프록시 주소로. 처음 쓸 때 프록시를 띄우고, 다른 CDN 호스트는 프록시가
/// `/_p_/HOST` 로 나눠 보낸다(다시 띄우지 않는다). 프록시를 못 띄우면 원래 주소(macOS 는 그래도 된다).
async fn through_proxy(url: &Url) -> String {
    if !LocalStreamProxy::needs_proxy(url) {
        return url.to_string();
    }
    let proxy = PREVIEW_PROXY.get_or_init(|| {
        let p = LocalStreamProxy::default();
        p.set_perf_tag("preview");
        p
    });
    through(proxy, url).await
}

/// `url` 을 `proxy` 경유 주소로 — 꺼져 있으면 먼저 띄운다.
async fn through(proxy: &LocalStreamProxy, url: &Url) -> String {
    if !proxy.is_running() {
        // 첫 기동은 한 번에 하나 — 카드를 빠르게 옮기면 두 요청이 함께 여기 온다. `start` 는 다른 호스트로
        // 부르면 돌던 프록시를 멈추고 새 포트로 다시 띄워, 먼저 돌려준 주소(앞 포트)가 바로 끊겼다.
        static START: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let _guard = START.lock().await;
        if !proxy.is_running() {
            let Some(host) = url.host_str() else {
                return url.to_string();
            };
            if let Err(e) = proxy.start(host).await {
                tracing::warn!("미리보기 프록시를 띄우지 못했습니다: {e}");
                return url.to_string();
            }
        }
    }
    proxy.proxy_url(url).to_string()
}

#[tauri::command]
pub async fn preview_source(
    services: State<'_, AppServices>,
    kind: String,
    id: String,
) -> CmdResult<Option<PreviewSourceDto>> {
    let id = id.trim();
    if id.is_empty() {
        return Ok(None);
    }
    match kind.as_str() {
        "live" => {
            // 목록에서 막 본 채널이라 캐시된 상세로 충분하다(재생 주소 토큰은 몇 분 안에 끝나지 않는다).
            let info = services.api.live_detail(id, false).await?;
            if info.status != LiveStatus::Open || info.adult {
                return Ok(None);
            }
            let Some(json) = info.live_playback_json.as_deref().filter(|j| !j.is_empty()) else {
                return Ok(None);
            };
            if playback::requires_authenticated_key(Some(json)) {
                return Ok(None);
            }
            let Ok(url) = playback::media_url(json, true) else {
                return Ok(None);
            };
            Ok(Some(PreviewSourceDto {
                url: through_proxy(&url).await,
                format: "hls",
                start_at: 0.0,
                live: true,
            }))
        }
        "vod" => {
            let Ok(video_no) = id.parse::<i64>() else {
                return Ok(None);
            };
            let Ok(s) = VodStreamResolver::new(services.api.clone())
                .resolve_sources(video_no)
                .await
            else {
                return Ok(None);
            };
            let (url, format) = pick(&s.progressive, &s.info.qualities, &s.info.stream_url);
            Ok(Some(PreviewSourceDto {
                url: through_proxy(&url).await,
                format,
                start_at: vod_preview_start(s.info.duration.as_secs_f64()),
                live: false,
            }))
        }
        "clip" => {
            let Ok(n) = resolve_clip_streams(id).await else {
                return Ok(None);
            };
            let (url, format) = pick(&n.progressive, &n.hls, &n.master);
            Ok(Some(PreviewSourceDto {
                url: through_proxy(&url).await,
                format,
                start_at: 0.0,
                live: false,
            }))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(resolution: &str, bandwidth: i64) -> VodQuality {
        VodQuality {
            id: resolution.into(),
            name: resolution.into(),
            resolution: resolution.into(),
            bandwidth,
            url: url::Url::parse(&format!("https://cdn.example.com/{resolution}.mp4")).unwrap(),
        }
    }

    #[test]
    fn picks_lowest_quality_at_or_above_floor() {
        let list = [
            q("1920x1080", 6000),
            q("1280x720", 3000),
            q("640x360", 800),
            q("256x144", 200),
        ];
        assert_eq!(preview_quality(&list).unwrap().resolution, "640x360");
        let list = [q("1080p", 0), q("480p", 0)];
        assert_eq!(preview_quality(&list).unwrap().resolution, "480p");
    }

    #[test]
    fn falls_back_to_highest_when_all_below_floor() {
        let list = [q("256x144", 200), q("426x240", 400)];
        assert_eq!(preview_quality(&list).unwrap().resolution, "426x240");
    }

    #[test]
    fn unknown_heights_use_smallest_bandwidth() {
        let list = [q("high", 5000), q("low", 700), q("mid", 2000)];
        assert_eq!(preview_quality(&list).unwrap().resolution, "low");
        assert!(preview_quality(&[]).is_none());
    }

    #[test]
    fn vod_start_skips_intro_with_cap() {
        assert_eq!(vod_preview_start(30.0), 0.0);
        assert_eq!(vod_preview_start(f64::NAN), 0.0);
        assert_eq!(vod_preview_start(600.0), 90.0);
        assert_eq!(vod_preview_start(36_000.0), 600.0);
    }

    #[test]
    fn pick_prefers_progressive_mp4() {
        let master = url::Url::parse("https://cdn.example.com/master.m3u8").unwrap();
        let (url, format) = pick(
            &[q("1280x720", 0), q("640x360", 0)],
            &[q("640x360", 0)],
            &master,
        );
        assert_eq!(
            (url.as_str(), format),
            ("https://cdn.example.com/640x360.mp4", "mp4")
        );
        let (url, format) = pick(&[], &[], &master);
        assert_eq!(
            (url.as_str(), format),
            ("https://cdn.example.com/master.m3u8", "hls")
        );
    }

    #[tokio::test]
    async fn cdn_urls_go_through_the_preview_proxy() {
        let other = Url::parse("https://example.com/a.mp4").unwrap();
        assert_eq!(through_proxy(&other).await, "https://example.com/a.mp4");

        let live = Url::parse("https://livecloud.pstatic.net/live/hls/playlist.m3u8?t=1").unwrap();
        let first = through_proxy(&live).await;
        assert!(first.starts_with("http://127.0.0.1:"), "{first}");
        assert!(first.ends_with("/live/hls/playlist.m3u8?t=1"), "{first}");
        // 다른 CDN 호스트는 같은 프록시가 `/_p_/HOST` 로 받는다.
        let vod = Url::parse("https://vod.akamaized.net/v/360.mp4").unwrap();
        let second = through_proxy(&vod).await;
        let port = |u: &str| Url::parse(u).unwrap().port();
        assert_eq!(port(&first), port(&second));
        assert!(
            second.contains("/_p_/vod.akamaized.net/v/360.mp4"),
            "{second}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_first_use_keeps_one_listener() {
        // Tauri 명령은 여러 작업 스레드에서 함께 돈다. 처음 쓰는 두 요청이 서로 다른 CDN 이어도 같은 포트를
        // 돌려줘야 한다(뒤 요청이 앞 프록시를 다른 호스트로 다시 띄우면 먼저 돌려준 주소가 끊긴다).
        let a = Url::parse("https://livecloud.pstatic.net/a/playlist.m3u8").unwrap();
        let b = Url::parse("https://vod.akamaized.net/b/360.mp4").unwrap();
        let port = |u: &str| Url::parse(u).unwrap().port();
        for _ in 0..50 {
            let proxy = LocalStreamProxy::default();
            let (pa, pb) = (proxy.clone(), proxy.clone());
            let (ua, ub) = (a.clone(), b.clone());
            let x = tokio::spawn(async move { through(&pa, &ua).await });
            let y = tokio::spawn(async move { through(&pb, &ub).await });
            let (x, y) = (x.await.unwrap(), y.await.unwrap());
            assert_eq!(port(&x), port(&y), "{x} {y}");
            assert_eq!(port(&x), Some(proxy.port()), "{x}");
            proxy.stop();
        }
    }
}
