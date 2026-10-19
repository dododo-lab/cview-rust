//! 라이브 플레이어 엔진 면 — Swift `playerEngine as? HLSJSPlayerEngine` 캐스트로 닿던 API.
//!
//! [`LivePlayerEngine`] 은 코디네이터가 쓰는 [`CoordinatedEngine`] 위에 뷰모델이 직접 만지는
//! hls.js 설정(화질 의도·프로파일·EQ·비디오 보정·표면)을 더한다. hls.js 가 아닌 엔진은 기본
//! no-op 을 그대로 쓴다(Swift 의 `as?` 실패와 같은 의미). Tauri 셸의 WebView 엔진이 `JsCommand`
//! 로 구현하고, 테스트는 기록용 가짜를 쓴다.

use std::path::PathBuf;
use std::sync::Arc;

use cview_player::coordinator::CoordinatedEngine;
use cview_player::hlsjs::{HlsJsForcedQualityTarget, HlsJsMatchWebV2Config, HlsJsStreamingProfile};

/// 비디오 보정 값 묶음.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoAdjust {
    pub enabled: bool,
    pub brightness: f64,
    pub contrast: f64,
    pub saturation: f64,
    pub hue: f64,
    pub gamma: f64,
    pub sharp_pixel: bool,
}

/// 뷰모델이 쓰는 라이브 엔진.
pub trait LivePlayerEngine: CoordinatedEngine {
    /// hls.js 엔진인가 — 아니면 아래 설정은 모두 무시된다.
    fn is_hlsjs(&self) -> bool {
        false
    }
    fn set_streaming_profile(&self, _profile: HlsJsStreamingProfile) {}
    fn set_match_web_v2_config(&self, _config: Option<HlsJsMatchWebV2Config>) {}
    fn set_match_web_v2_seek_priority(&self, _priority: i32) {}
    fn set_parity_tag(&self, _tag: &str) {}
    fn set_prefers_source_quality(&self, _prefers: bool) {}
    fn set_user_quality_ceiling_height(&self, _height: i64) {}
    fn set_force_highest_quality(&self, _enabled: bool) {}
    fn force_highest_quality(&self) -> bool {
        false
    }
    fn set_forced_quality(&self, _target: Option<HlsJsForcedQualityTarget>) {}
    /// gap-crossing 하드 리싱크.
    fn force_live_resync(&self) {}
    fn seek_to_live_edge(&self) {}
    /// 실효 라이브 목표 지연(초) — 백그라운드 리싱크 선게이트용.
    fn effective_live_target_latency_seconds(&self) -> f64 {
        0.0
    }
    fn set_video_surface_visible(&self, _visible: bool) {}
    fn set_sharp_pixel_scaling(&self, _enabled: bool) {}
    fn sharp_pixel_scaling_enabled(&self) -> bool {
        false
    }
    fn set_picture_enhancement(&self, _auto: bool, _upscale: bool) {}
    fn set_equalizer_enabled(&self, _enabled: bool, _bands: &[f32], _pre_amp_db: f32) {}
    fn set_equalizer_pre_amp(&self, _db: f32) {}
    fn set_equalizer_band(&self, _index: usize, _gain_db: f32) {}
    fn apply_video_adjust(&self, _adjust: VideoAdjust) {}
    /// 엔진이 실제로 쓴 녹화 파일(컨테이너가 바뀌었을 수 있다).
    fn recording_destination(&self) -> Option<PathBuf> {
        None
    }
    /// 마지막 녹화 쓰기 실패 사유.
    fn recording_failure_reason(&self) -> Option<String> {
        None
    }
}

/// 공유 라이브 엔진.
pub type SharedLiveEngine = Arc<dyn LivePlayerEngine>;

/// 엔진 팩토리(Swift `makeEngine`).
pub type LiveEngineFactory = Arc<dyn Fn() -> SharedLiveEngine + Send + Sync>;
