//! CViewPlayer 포팅: HLS 매니페스트, 로컬 스트림 프록시, 저지연 제어.
//!
//! 최종 앱은 Tauri WebView 안에서 기존 hls.js 플레이어(`Sources/CViewPlayer/Resources/hlsjs-player.html`)
//! 를 돌리고, 이 크레이트는 로컬 스트림 프록시와 제어 로직(저지연 PID·재연결·대역폭 배분)을 맡는다.
//! AppKit/AVFoundation 의존 부분(AVPlayerEngine·PiP·뷰·스크린샷 인코더 등)은 옮기지 않는다.

pub mod bandwidth_coordinator;
pub mod clock;
pub mod coordinator;
pub mod download;
pub mod hlsjs;
pub mod low_latency;
pub mod manifest;
pub mod policy;
pub mod proxy;
pub mod reconnection;
pub mod recording;
pub mod seek_arbiter;
pub mod vod;

pub use low_latency::{LowLatencyConfiguration, LowLatencyController, SyncState};
pub use manifest::{HlsByteSpan, HlsManifestParser, MasterPlaylist, MediaPlaylist};
