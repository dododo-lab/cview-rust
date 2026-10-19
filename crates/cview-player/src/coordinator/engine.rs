//! 코디네이터가 엔진에 기대는 면 — `PlayerEngineProtocol`(cview-core) + hls.js 전용 제어
//! (Swift 에서는 `playerEngine as? HLSJSPlayerEngine` 캐스트로 닿던 API).
//!
//! Tauri 앱에서는 WebView 의 hls.js 페이지를 감싼 엔진이 두 trait 을 구현하고, 명령은
//! [`crate::hlsjs::JsCommand`] 로 페이지에 보낸다. 테스트는 모의 엔진을 쓴다.

use std::sync::Arc;

use async_trait::async_trait;
use cview_core::models::player::PlayerError;
use cview_core::protocols::PlayerEngineProtocol;
use url::Url;

use crate::hlsjs::HlsJsMatchWebV2Config;
use crate::low_latency::MatchWebV2DisplaySample;

/// 코디네이터에 주입하는 엔진.
pub trait CoordinatedEngine: PlayerEngineProtocol {
    /// hls.js 엔진이면 저지연·타임시프트 제어 면. 아니면 `None`(Swift 의 `as? HLSJSPlayerEngine` 실패).
    fn hls(&self) -> Option<&dyn HlsEngineControl> {
        None
    }
}

/// 공유 엔진 핸들.
pub type SharedEngine = Arc<dyn CoordinatedEngine>;

/// 같은 엔진 인스턴스인가 (Swift `===`).
pub fn same_engine(a: &SharedEngine, b: &SharedEngine) -> bool {
    std::ptr::addr_eq(Arc::as_ptr(a), Arc::as_ptr(b))
}

/// hls.js 엔진의 저지연·matchWeb v2·타임시프트 제어 (`HLSJSPlayerEngine` 의 코디네이터 사용 면).
///
/// 저지연 명령은 **세대 토큰**을 함께 받는다. 엔진은 현재 세션 토큰과 다르면 명령을 버린다 —
/// stop·엔진 교체·DVR·수동 pause 뒤 늦게 착지한 rate/seek 가 새 재생을 조작하지 못하게 한다.
#[async_trait]
pub trait HlsEngineControl: Send + Sync {
    /// 현재 제어 세션을 폐기하고 새 토큰을 돌려준다(이 토큰으로는 1.0x 복원 같은 정리만 한다).
    fn invalidate_latency_control_session(&self) -> u64;
    /// 새 제어 세션을 연다.
    fn begin_latency_control_session(&self) -> u64;
    fn is_latency_control_session_current(&self, generation: u64) -> bool;
    /// 세대 토큰을 실은 배속 명령.
    fn set_rate_for_latency(&self, rate: f32, generation: u64);
    /// 배속을 걸고 페이지가 실제로 적용했는지까지 기다린다. 토큰이 낡았으면 `false`.
    async fn set_rate_awaiting_application(&self, rate: f32, generation: u64) -> bool;
    fn seek_to_latency(&self, target_seconds: f64, generation: u64);
    fn set_latency_owned_by_controller(&self, owned: bool, generation: u64);
    /// 0 이면 JS 가 프로파일 목표로 돌아간다.
    fn set_controller_target_latency(&self, seconds: f64, generation: u64);
    /// JS 자기보정·max-latency 자동 점프 게이트(타임머신 동안 끈다).
    fn set_live_latency_correction_enabled(&self, enabled: bool);
    fn is_live_latency_correction_enabled(&self) -> bool;
    /// 엔진이 스스로 로딩을 멈췄는가(`suspendLoading`).
    fn is_loading_suspended(&self) -> bool;
    /// hls.js `latency`(라이브 엣지 대비 지연, 초).
    fn live_latency_seconds(&self) -> f64;
    /// 실측 세그먼트 길이(초). 0 이면 미측정.
    fn measured_segment_duration(&self) -> f64 {
        0.0
    }
    /// hls.js 실측 버퍼 비율 0~1.
    fn buffer_health(&self) -> f64 {
        1.0
    }
    /// 가려지지 않았고 스냅샷이 신선한가 — 아니면 seek 만 억제한다.
    fn latency_sample_is_actionable(&self) -> bool {
        true
    }

    // matchWeb v2
    fn is_match_web_v2_active(&self) -> bool {
        false
    }
    fn match_web_v2_config(&self) -> Option<HlsJsMatchWebV2Config> {
        None
    }
    fn match_web_v2_display_sample(&self) -> Option<MatchWebV2DisplaySample> {
        None
    }
    fn measured_part_duration(&self) -> f64 {
        0.0
    }
    fn set_match_web_v2_targets(&self, _hls: f64, _ingest: f64, _generation: u64) {}

    /// 네트워크 경로가 복구됐다 — 스톨 추적을 리셋한다.
    fn note_network_transport_recovered(&self) {}

    // 로컬 타임시프트
    /// 로컬 재생목록 안에서 재로드 없이 옮긴다.
    fn seek_timeshift(&self, _seconds_behind: f64) {}
    /// 로컬 타임시프트 재생목록을 `timeshift` 프로필로 싣는다.
    async fn play_timeshift(&self, _url: &Url) -> Result<(), PlayerError> {
        Err(PlayerError::UnsupportedFormat("timeshift".into()))
    }
}
