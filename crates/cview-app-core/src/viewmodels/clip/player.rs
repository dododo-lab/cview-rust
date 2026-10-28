//! 클립 재생 뷰모델 (`ClipPlayerViewModel`).

use std::sync::{Arc, Weak};

use cview_core::models::search::ClipInfo;
use cview_core::models::vod::VodPlaybackState;
use serde::Serialize;
use url::Url;

use super::{ClipApi, PlaybackStateSnapshot};
use crate::viewmodels::support::{Observed, ObservedGuard};

/// 클립 재생 상태.
#[derive(Debug, Clone)]
pub struct ClipPlayerState {
    pub playback_state: VodPlaybackState,
    pub current_time: f64,
    pub duration: f64,
    pub volume: f32,
    pub is_muted: bool,
    pub clip_title: String,
    pub channel_name: String,
    pub error_message: Option<String>,
    pub is_seeking: bool,
    /// embed WebView 재생 URL.
    pub embed_fallback_url: Option<Url>,
    /// 다운로드용 미디어 URL.
    pub download_url: Option<Url>,
}

impl Default for ClipPlayerState {
    fn default() -> Self {
        Self {
            playback_state: VodPlaybackState::Idle,
            current_time: 0.0,
            duration: 0.0,
            volume: 1.0,
            is_muted: false,
            clip_title: String::new(),
            channel_name: String::new(),
            error_message: None,
            is_seeking: false,
            embed_fallback_url: None,
            download_url: None,
        }
    }
}

impl ClipPlayerState {
    /// 진행률.
    pub fn progress(&self) -> f64 {
        if self.duration <= 0.0 {
            return 0.0;
        }
        self.current_time / self.duration
    }

    /// 재생 중/일시정지/버퍼링 여부.
    pub fn is_playback_active(&self) -> bool {
        matches!(
            self.playback_state,
            VodPlaybackState::Playing | VodPlaybackState::Paused | VodPlaybackState::Buffering
        )
    }
}

/// 클립 재생 스냅샷.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipPlayerSnapshot {
    pub playback_state: PlaybackStateSnapshot,
    pub current_time: f64,
    pub duration: f64,
    pub progress: f64,
    pub volume: f32,
    pub is_muted: bool,
    pub clip_title: String,
    pub channel_name: String,
    pub error_message: Option<String>,
    pub embed_fallback_url: Option<String>,
    pub download_url: Option<String>,
    pub is_playback_active: bool,
}

/// 클립 재생 뷰모델 (embed 단일 경로 + 다운로드 URL 해석).
#[derive(Clone)]
pub struct ClipPlayerViewModel {
    obs: Arc<Observed<ClipPlayerState>>,
    api: Option<Arc<dyn ClipApi>>,
}

impl ClipPlayerViewModel {
    pub fn new(api: Option<Arc<dyn ClipApi>>) -> Self {
        Self {
            obs: Arc::new(Observed::new(ClipPlayerState::default())),
            api,
        }
    }

    pub fn lock(&self) -> ObservedGuard<'_, ClipPlayerState> {
        self.obs.lock()
    }

    pub fn read(&self) -> ObservedGuard<'_, ClipPlayerState> {
        self.obs.read()
    }

    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.obs.subscribe()
    }

    pub fn snapshot(&self) -> ClipPlayerSnapshot {
        let st = self.read();
        ClipPlayerSnapshot {
            playback_state: (&st.playback_state).into(),
            current_time: st.current_time,
            duration: st.duration,
            progress: st.progress(),
            volume: st.volume,
            is_muted: st.is_muted,
            clip_title: st.clip_title.clone(),
            channel_name: st.channel_name.clone(),
            error_message: st.error_message.clone(),
            embed_fallback_url: st.embed_fallback_url.as_ref().map(|u| u.to_string()),
            download_url: st.download_url.as_ref().map(|u| u.to_string()),
            is_playback_active: st.is_playback_active(),
        }
    }

    /// 클립 재생 시작. embed URL 을 즉시 세우고, 다운로드용 URL 해석은 백그라운드로 돌린다
    /// (돌려준 핸들은 그 해석 작업 — 기다리지 않아도 된다).
    pub fn start_clip(&self, clip: &ClipInfo) -> Option<tokio::task::JoinHandle<()>> {
        let uid = clip.clip_uid.clone();
        {
            let mut st = self.lock();
            if uid.is_empty() {
                st.playback_state = VodPlaybackState::Error("클립 ID가 없습니다".into());
                st.error_message = Some("클립 ID가 없습니다".into());
                return None;
            }
            st.clip_title = clip.clip_title.clone();
            st.channel_name = clip
                .channel
                .as_ref()
                .map(|c| c.channel_name.clone())
                .unwrap_or_default();
            st.error_message = None;
            let Ok(embed) = Url::parse(&format!("https://chzzk.naver.com/embed/clip/{uid}")) else {
                st.playback_state = VodPlaybackState::Error("잘못된 클립 URL".into());
                st.error_message = Some("잘못된 클립 URL".into());
                return None;
            };
            st.embed_fallback_url = Some(embed);
            st.playback_state = VodPlaybackState::Paused;
        }
        let api = self.api.clone()?;
        let weak: Weak<Observed<ClipPlayerState>> = Arc::downgrade(&self.obs);
        Some(tokio::spawn(async move {
            let detail = match api.clip_detail(&uid).await {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!("Clip detail fetch failed (embed already playing): {e}");
                    return;
                }
            };
            {
                let Some(obs) = weak.upgrade() else { return };
                let mut st = obs.lock();
                if !detail.clip_title.is_empty() {
                    st.clip_title = detail.clip_title.clone();
                }
                if let Some(ch) = detail.channel() {
                    if !ch.channel_name.is_empty() {
                        st.channel_name = ch.channel_name.clone();
                    }
                }
            }
            if let Some(url) = detail.best_playback_url() {
                if let Some(obs) = weak.upgrade() {
                    obs.lock().download_url = Some(url);
                }
            } else if let Some(video_id) = detail.video_id.as_deref().filter(|v| !v.is_empty()) {
                match api.clip_stream_url(&uid, video_id).await {
                    Ok(url) => {
                        if let Some(obs) = weak.upgrade() {
                            obs.lock().download_url = Some(url);
                        }
                    }
                    Err(_) => tracing::info!("Clip inkey failed (embed 재생은 계속): {uid}"),
                }
            }
        }))
    }

    /// 정지.
    pub fn stop(&self) {
        let mut st = self.lock();
        st.embed_fallback_url = None;
        st.playback_state = VodPlaybackState::Idle;
        st.current_time = 0.0;
    }

    /// embed 가 추출한 미디어 URL 을 다운로드 대상으로 기록. http 만, 먼저 해석된 값 우선.
    pub fn note_extracted_media_url(&self, url: &Url) {
        let text = url.as_str();
        if !text.starts_with("http") {
            tracing::warn!(
                "클립 추출 URL 무시(http 아님): {}",
                text.chars().take(80).collect::<String>()
            );
            return;
        }
        let mut st = self.lock();
        if st.download_url.is_none() {
            st.download_url = Some(url.clone());
        }
    }
}
