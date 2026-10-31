//! 썸네일 호버 미리보기 — 카드 위에 잠시 머물면 소리 없이 짧게 재생할 주소를 돌려준다.
//!
//! 미리보기는 작은 썸네일 자리에서 잠깐 보는 것이라 본 재생 경로(프록시·저지연 제어)를 타지 않고
//! WebView `<video>` 가 바로 받는다. 그래서 가벼운 주소를 고른다:
//! - 라이브: 표준 HLS(LL-HLS 보다 요청이 적다). 암호화(`hls_aes`) 방송은 인증 프록시가 있어야 해서
//!   미리보기를 하지 않는다. 19 세 방송도 하지 않는다.
//! - 다시보기·클립: 단일 MP4(어느 WebView 든 바로 재생·탐색)의 낮은 화질, 없으면 HLS 낮은 화질.
//!
//! 미리보기를 할 수 없으면 `None` — 화면은 썸네일을 그대로 둔다.

use cview_core::models::live::LiveStatus;
use cview_core::models::vod::VodQuality;
use cview_player::vod::{resolve_clip_streams, VodStreamResolver};
use serde::Serialize;
use tauri::State;

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
fn pick(
    progressive: &[VodQuality],
    hls: &[VodQuality],
    master: &url::Url,
) -> (String, &'static str) {
    if let Some(q) = preview_quality(progressive) {
        return (q.url.to_string(), "mp4");
    }
    let url = preview_quality(hls).map(|q| &q.url).unwrap_or(master);
    (url.to_string(), "hls")
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
                url: url.to_string(),
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
                url,
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
                url,
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
}
