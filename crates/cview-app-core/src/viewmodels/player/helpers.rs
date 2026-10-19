//! 플레이어 뷰모델의 순수 함수 — 화질 메뉴·오버레이·시간 표기·스크린샷 파일명·EQ 밴드 맞춤.

use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use cview_core::models::equalizer::EqualizerPreset;
use cview_core::protocols::CapturedFrame;
use cview_player::coordinator::{StreamPhase, StreamQualityInfo};
use cview_player::hlsjs::HlsJsForcedQualityTarget;

use super::engine::LivePlayerEngine;
use crate::logic::effective_playback_policy::EffectivePlaybackPolicy;

/// 멀티라이브 메인은 1080p/8Mbps, 단일 라이브는 사용자 설정.
pub fn forced_quality_target(is_multi_live: bool) -> Option<HlsJsForcedQualityTarget> {
    is_multi_live.then_some(HlsJsForcedQualityTarget::MULTI_LIVE_1080P_8MBPS)
}

/// 화질 의도를 엔진에 투영한다(시작 직전·런타임 재적용 공통).
pub fn apply_quality_intent(
    engine: &dyn LivePlayerEngine,
    policy: &EffectivePlaybackPolicy,
    is_multi_live: bool,
    multi_live_target: HlsJsForcedQualityTarget,
) -> Option<HlsJsForcedQualityTarget> {
    engine.set_prefers_source_quality(policy.quality_policy.prefers_source_quality());
    engine.set_user_quality_ceiling_height(if is_multi_live {
        0
    } else {
        policy
            .preferred_quality
            .map(|q| q.max_height_cap())
            .unwrap_or(0)
    });
    engine.set_force_highest_quality(policy.force_highest_quality());
    let target = is_multi_live.then_some(multi_live_target);
    engine.set_forced_quality(target);
    target
}

fn height_of(q: &StreamQualityInfo) -> Option<i64> {
    q.resolution
        .to_lowercase()
        .split('x')
        .next_back()
        .and_then(|s| s.parse().ok())
}

/// 멀티라이브 메뉴에는 현재 역할의 목표에 가장 가까운 하나만 노출한다.
pub fn selectable_qualities(
    qualities: &[StreamQualityInfo],
    is_multi_live: bool,
    target: HlsJsForcedQualityTarget,
) -> Vec<StreamQualityInfo> {
    if !is_multi_live {
        return qualities.to_vec();
    }
    let target_bps = target.target_bitrate_kbps * 1_000;
    let heights: Vec<i64> = qualities.iter().filter_map(height_of).collect();
    let height = heights
        .iter()
        .copied()
        .filter(|h| *h <= target.height)
        .max()
        .or_else(|| heights.iter().copied().min());
    let fps = |q: &StreamQualityInfo| q.frame_rate.unwrap_or(0.0);
    let is60 = |q: &StreamQualityInfo| (59.0..=61.0).contains(&fps(q));
    let less = |l: &StreamQualityInfo, r: &StreamQualityInfo| -> bool {
        if target == HlsJsForcedQualityTarget::MULTI_LIVE_1080P_8MBPS && is60(l) != is60(r) {
            return is60(l);
        }
        let ld = (l.bandwidth - target_bps).abs();
        let rd = (r.bandwidth - target_bps).abs();
        if ld != rd {
            return ld < rd;
        }
        if fps(l) != fps(r) {
            return fps(l) > fps(r);
        }
        let lb = l.bandwidth <= target_bps;
        let rb = r.bandwidth <= target_bps;
        if lb != rb {
            return lb;
        }
        l.bandwidth < r.bandwidth
    };
    let mut best: Option<&StreamQualityInfo> = None;
    for q in qualities.iter().filter(|q| height_of(q) == height) {
        if best.is_none_or(|b| less(q, b)) {
            best = Some(q);
        }
    }
    best.cloned().into_iter().collect()
}

/// 플레이어 위 오버레이 상태.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
pub enum PlayerOverlayState {
    None,
    InitialLoading,
    Connecting,
    Buffering,
    Reconnecting,
    Ended,
    Error(String),
}

impl PlayerOverlayState {
    pub fn from(api_loading: bool, phase: Option<&StreamPhase>, is_stream_offline: bool) -> Self {
        if api_loading {
            return Self::InitialLoading;
        }
        if is_stream_offline {
            return Self::Ended;
        }
        match phase {
            None => Self::None,
            Some(StreamPhase::Idle | StreamPhase::LoadingInfo | StreamPhase::LoadingManifest) => {
                Self::None
            }
            Some(StreamPhase::Connecting) => Self::Connecting,
            Some(StreamPhase::Buffering) => Self::Buffering,
            Some(StreamPhase::Reconnecting) => Self::Reconnecting,
            Some(StreamPhase::StreamEnded) => Self::Ended,
            Some(StreamPhase::Error(m)) => Self::Error(m.clone()),
            Some(StreamPhase::Playing | StreamPhase::Paused) => Self::None,
        }
    }

    pub fn needs_loading_overlay(&self) -> bool {
        matches!(
            self,
            Self::InitialLoading | Self::Connecting | Self::Buffering | Self::Reconnecting
        )
    }

    pub fn has_overlay(&self) -> bool {
        !matches!(self, Self::None)
    }
}

/// `h:mm:ss` / `m:ss`.
pub fn format_time_interval(t: f64) -> String {
    if !t.is_finite() || t < 0.0 {
        return "0:00".into();
    }
    let total = t as i64;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// 방송 경과 — `h:mm:ss` / `mm:ss`.
pub fn format_uptime(uptime: f64) -> String {
    let u = uptime.max(0.0) as i64;
    let (h, m, s) = (u / 3600, (u % 3600) / 60, u % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

/// 스크린샷 파일 기본 이름 — 경로 구분자·금지문자를 빼고 `CView_<채널>_<unix초>`.
pub fn screenshot_file_base_name(channel_name: &str, unix_seconds: i64) -> String {
    let safe: String = channel_name
        .chars()
        .filter(|c| !"/:\\?%*|\"<>".contains(*c))
        .collect();
    let safe = safe.trim();
    if safe.is_empty() {
        format!("CView_{unix_seconds}")
    } else {
        format!("CView_{safe}_{unix_seconds}")
    }
}

/// 프레임을 폴더에 쓴다. 폴더가 없으면 만들고, 이름이 겹치면 `-2`, `-3` … 을 붙여 덮어쓰지 않는다.
/// 확장자는 요청값이 아니라 프레임의 실제 포맷을 따른다.
pub fn write_screenshot(frame: &CapturedFrame, dir: &Path, base_name: &str) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let ext = frame.format.file_extension();
    let mut dest = dir.join(format!("{base_name}.{ext}"));
    let mut suffix = 2;
    while dest.exists() {
        dest = dir.join(format!("{base_name}-{suffix}.{ext}"));
        suffix += 1;
    }
    crate::services::support::write_atomic(&dest, &frame.data)?;
    Ok(dest)
}

/// 저장된 밴드 배열을 주파수 개수에 맞춘다(넘치면 자르고 모자라면 0 으로 채운다).
pub fn equalizer_bands_fitting_frequencies(bands: &[f32]) -> Vec<f32> {
    let count = EqualizerPreset::FREQUENCIES.len();
    let mut out: Vec<f32> = bands.iter().copied().take(count).collect();
    out.resize(count, 0.0);
    out
}

/// `~` 를 홈으로 펼친다.
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    } else if path == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home);
        }
    }
    PathBuf::from(path)
}
