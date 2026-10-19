//! 재생 계획의 AES 키 접근 판정.
//!
//! 원본: `Sources/CViewApp/ViewModels/PlaybackLaunchPlan+KeyAccess.swift`.
//! `AppState.playbackKeyAccess(for:)` 의 쿠키 클로저는 셸이 인증 저장소에서 읽어 붙인다 —
//! 여기서는 "프록시가 필요한가" 판정만 옮겼다.

use cview_core::models::{LiveInfo, LivePlayback, StreamProxyMode};

/// 암호화 HLS(`hls_aes`)는 직접 모드에서도 인증 프록시로 키를 받아야 한다.
pub fn requires_authenticated_key(live_info: &LiveInfo, is_audio_only: bool) -> bool {
    let json = if is_audio_only {
        live_info.radio_mode_playback_json.as_deref()
    } else {
        live_info.live_playback_json.as_deref()
    };
    let Some(playback) = json.and_then(|j| serde_json::from_str::<LivePlayback>(j).ok()) else {
        return false;
    };
    playback
        .media
        .iter()
        .any(|m| m.aes.as_deref().map(str::to_lowercase).as_deref() == Some("hls_aes"))
}

/// `PlaybackKeyAccess` 의 판정 부분.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlaybackKeyAccess {
    pub requires_proxy: bool,
}

impl PlaybackKeyAccess {
    pub const ANONYMOUS: Self = Self {
        requires_proxy: false,
    };

    /// 키 프록시가 필요하면 요청 모드와 무관하게 로컬 프록시.
    pub fn proxy_mode(&self, requested: StreamProxyMode) -> StreamProxyMode {
        if self.requires_proxy {
            StreamProxyMode::LocalProxy
        } else {
            requested
        }
    }

    /// `AppState.playbackKeyAccess(for:)`.
    pub fn for_plan(live_info: &LiveInfo, is_audio_only: bool) -> Self {
        Self {
            requires_proxy: requires_authenticated_key(live_info, is_audio_only),
        }
    }
}

/// `PlaybackSourceOptions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackSourceOptions {
    pub eligible: bool,
    pub key_access: PlaybackKeyAccess,
}
