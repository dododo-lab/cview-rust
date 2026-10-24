//! `Sources/CViewPlayer/StreamCoordinator.swift` 포팅 (+ `+Lifecycle`·`+Recovery`·`+QualityABR`·
//! `+RoleLatency`·`+Reconnection`·`+ManifestRefresh`·`+LowLatency`·`+MatchWebV2`·`+ProxyRoute`·`+Timeshift`).
//!
//! 스트림 재생 오케스트레이터: 매니페스트 → 프록시 경로 → 엔진 → 저지연 싱크 → 재연결.
//!
//! Swift `actor` 를 `Arc<Inner>` + `std::sync::Mutex<State>` 로 옮겼다. 잠금은 `await` 를 넘어
//! 잡지 않으며(액터의 재진입 지점과 같은 자리에서 놓는다), 엔진·저지연 컨트롤러 호출도 잠금 밖에서
//! 한다. 비구조적 `Task { [weak self] … }` 는 `Weak<Inner>` 를 쥔 tokio 태스크다.
//!
//! 차이점:
//! - Swift `Task.isCancelled` 협조 취소는 tokio `abort` 로 바뀌었다 — 재연결 시도는 성공 판정
//!   (`handle_success` → 핸들러 태스크 abort)에서 다음 `await` 지점에 그대로 끝난다.
//! - 엔진 타입 캐스트(`as? HLSJSPlayerEngine`)는 [`CoordinatedEngine::hls`] 로 바뀌었다.
//! - 매니페스트 다운로드는 주입 가능한 [`ManifestFetcher`] 를 쓴다(기본: reqwest).
//! - 네트워크 경로 감시(`AVPlayerNetworkMonitor`)는 앱 계층 몫이다 — 앱이
//!   [`StreamCoordinator::handle_transport_path_change`] 를 부른다.

pub mod engine;
mod latency;
mod lifecycle;
mod manifest;
mod proxy_route;
mod reconnection;
mod recovery;
mod timeshift;

#[cfg(test)]
mod tests;

use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Instant;

use cview_core::models::live::StreamQuality;
use cview_core::models::player::PlayerError;
use cview_core::models::proxy_stats::ProxyNetworkStats;
use cview_core::models::stream_proxy::{StreamProxyMode, StreamProxyRouteStatus};
use futures_util::future::BoxFuture;
use serde::Serialize;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use crate::low_latency::{LowLatencyConfiguration, LowLatencyController};
use crate::manifest::parser::Variant;
use crate::manifest::{HlsManifestParser, MasterPlaylist};
use crate::policy::{ManifestLease, PlaybackKeyAccess, QualityPolicyMode};
use crate::proxy::transfer_diagnostics::ProxyTransferDiagnostic;
use crate::proxy::LocalStreamProxy;
use crate::reconnection::{PlaybackReconnectionHandler, ReconnectionConfig};

pub use engine::{same_engine, CoordinatedEngine, HlsEngineControl, SharedEngine};
pub use manifest::{
    fetch_stream_variants, variant_base_url, FetchedManifest, HttpManifestFetcher, ManifestFetcher,
};
pub use recovery::{
    decide_recovery, network_recovery_decision, InterfaceType, NetworkRecoveryAction,
};
pub use timeshift::{TimeshiftRecorderFactory, TimeshiftRecordingConfig};

// MARK: - 설정

/// 저지연 싱크 시작 정책.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LowLatencyActivation {
    /// 재생 시작/재연결 시 코디네이터가 스스로 sync 루프를 시작(단일 라이브).
    #[default]
    Automatic,
    /// 외부(멀티라이브 매니저)가 [`StreamCoordinator::set_low_latency_sync_active`] 로 제어.
    Managed,
}

/// 코디네이터 설정 (Swift `StreamCoordinator.Configuration`).
#[derive(Debug, Clone, PartialEq)]
pub struct Configuration {
    pub channel_id: String,
    pub enable_low_latency: bool,
    pub low_latency_activation: LowLatencyActivation,
    /// 사용자 화질 상한(실제 적용은 hls.js 캡).
    pub preferred_quality: Option<StreamQuality>,
    pub low_latency_config: LowLatencyConfiguration,
    /// 하위 호환 입력 — `quality_policy` 가 있으면 그것이 우선.
    pub force_highest_quality: bool,
    pub quality_policy: Option<QualityPolicyMode>,
    pub stream_proxy_mode: StreamProxyMode,
}

impl Configuration {
    /// Swift 기본 인자와 같은 값.
    pub fn new(channel_id: impl Into<String>) -> Self {
        Self {
            channel_id: channel_id.into(),
            enable_low_latency: true,
            low_latency_activation: LowLatencyActivation::Automatic,
            preferred_quality: None,
            low_latency_config: LowLatencyConfiguration::WEB_SYNC,
            force_highest_quality: true,
            quality_policy: None,
            stream_proxy_mode: StreamProxyMode::LocalProxy,
        }
    }

    /// 실제 화질 정책 — `quality_policy` 가 없으면 `force_highest_quality` 에서 파생.
    pub fn effective_quality_policy(&self) -> QualityPolicyMode {
        self.quality_policy.unwrap_or_else(|| {
            QualityPolicyMode::from_legacy_force_highest_quality(self.force_highest_quality)
        })
    }
}

// MARK: - 상태·이벤트

/// 재생 단계.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "phase", content = "message", rename_all = "camelCase")]
pub enum StreamPhase {
    Idle,
    LoadingInfo,
    LoadingManifest,
    Connecting,
    Playing,
    Paused,
    Buffering,
    Reconnecting,
    StreamEnded,
    Error(String),
}

impl StreamPhase {
    /// App Nap 방지 등 재생 자원이 계속 필요한 단계.
    pub fn requires_playback_resources(&self) -> bool {
        matches!(
            self,
            Self::LoadingInfo
                | Self::LoadingManifest
                | Self::Connecting
                | Self::Playing
                | Self::Buffering
                | Self::Reconnecting
        )
    }

    pub(crate) fn is_playing_or_buffering(&self) -> bool {
        matches!(self, Self::Playing | Self::Buffering)
    }
}

/// UI 로 가는 이벤트.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    PhaseChanged(StreamPhase),
    ProxyRouteChanged(StreamProxyRouteStatus),
    QualitySelected(StreamQualityInfo),
    QualityChanged(StreamQualityInfo),
    LatencyUpdate(LatencyInfo),
    BufferUpdate(BufferHealth),
    Error(String),
    StreamEnded,
    Stopped,
    /// 되감기가 재연결·화질 전환으로 끝났다 — UI 의 되감기 표시를 거둔다.
    TimeShiftEnded,
}

/// 코디네이터 오류.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum StreamCoordinatorError {
    #[error("요청한 화질을 찾을 수 없습니다")]
    QualityNotFound,
    /// 더 새 시작/정지가 이 요청을 대체했다 (Swift `CancellationError`).
    #[error("취소됨")]
    Cancelled,
    /// Swift `ProxyRouteError.authenticationUnavailable`.
    #[error("암호화 방송 인증에 필요한 로컬 프록시를 시작하지 못했습니다.")]
    ProxyAuthenticationUnavailable,
    #[error(transparent)]
    Player(#[from] PlayerError),
}

/// 화질 정보.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamQualityInfo {
    pub name: String,
    pub resolution: String,
    pub bandwidth: i64,
    pub frame_rate: Option<f64>,
}

impl StreamQualityInfo {
    pub fn new(name: impl Into<String>, resolution: impl Into<String>, bandwidth: i64) -> Self {
        Self {
            name: name.into(),
            resolution: resolution.into(),
            bandwidth,
            frame_rate: None,
        }
    }

    pub fn id(&self) -> &str {
        &self.name
    }

    pub fn from_variant(variant: &Variant) -> Self {
        Self {
            name: variant.quality_label(),
            resolution: variant.resolution.clone(),
            bandwidth: variant.bandwidth,
            frame_rate: variant.frame_rate,
        }
    }
}

/// 지연 측정값.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct LatencyInfo {
    pub current: f64,
    pub target: f64,
    pub ewma: f64,
}

/// 버퍼 건강도.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BufferHealth {
    pub current_level: f64,
    pub target_level: f64,
    pub is_healthy: bool,
    /// 측정 신뢰도 0~1.
    pub confidence: f64,
    pub is_measurable: bool,
}

impl BufferHealth {
    pub fn new(current_level: f64, target_level: f64, is_healthy: bool) -> Self {
        Self {
            current_level,
            target_level,
            is_healthy,
            confidence: 1.0,
            is_measurable: true,
        }
    }

    /// 작은 부동 소수 노이즈(0.005 미만)는 같은 값으로 본다 — 중복 발행 억제.
    pub fn approximately_equals(&self, other: &BufferHealth) -> bool {
        self.is_healthy == other.is_healthy
            && self.is_measurable == other.is_measurable
            && (self.current_level - other.current_level).abs() < 0.005
            && (self.target_level - other.target_level).abs() < 0.005
            && (self.confidence - other.confidence).abs() < 0.01
    }
}

/// 방송 종료 확인 콜백 — true 면 재연결 대신 종료 처리.
pub type CheckStreamEndedCallback = Arc<dyn Fn() -> BoxFuture<'static, bool> + Send + Sync>;

// MARK: - 본체

pub(crate) struct State {
    pub(crate) phase: StreamPhase,
    /// 엔진의 일시적 paused 와 사용자의 일시정지 의도를 구분한다.
    pub(crate) user_paused: bool,
    pub(crate) master_playlist: Option<MasterPlaylist>,
    pub(crate) current_quality: Option<StreamQualityInfo>,
    pub(crate) stream_url: Option<url::Url>,
    pub(crate) start_time: Option<Instant>,
    pub(crate) is_proxy_active: bool,
    pub(crate) requested_proxy_mode: StreamProxyMode,
    pub(crate) proxy_route_status: Option<StreamProxyRouteStatus>,
    pub(crate) proxy_route_generation: u64,
    pub(crate) applying_proxy_mode: bool,
    /// 엔진에 전달한 현재 variant URL.
    pub(crate) current_variant_url: Option<url::Url>,
    pub(crate) last_reconnect_time: Option<tokio::time::Instant>,
    /// 엔진 자체 정지(hls.js `suspendLoading`) — 코디네이터 폴러 게이트.
    pub(crate) network_activity_suspended: bool,
    pub(crate) network_activity_generation: u64,
    /// 저지연 콜백 세대.
    pub(crate) low_latency_sync_generation: u64,
    /// 현재 HLS 엔진에 결합한 PID 명령 토큰.
    pub(crate) active_hls_latency_control_generation: Option<u64>,
    pub(crate) managed_sync_active: bool,
    pub(crate) time_shift_active: bool,
    pub(crate) timeshift_recording_config: Option<TimeshiftRecordingConfig>,
    pub(crate) timeshift_factory: Option<TimeshiftRecorderFactory>,
    pub(crate) local_timeshift_playing: bool,
    pub(crate) is_server_time_machine_stream: bool,
    pub(crate) timeshift_transition: u64,
    /// 사용자가 수동으로 고른 화질 — 토큰 갱신·재연결이 덮지 못한다.
    pub(crate) user_selected_variant: Option<Variant>,
    pub(crate) manifest_refresh_fail_count: i64,
    pub(crate) last_published_buffer_health: Option<BufferHealth>,
    pub(crate) manifest_generation: i64,
    pub(crate) current_manifest_lease: Option<ManifestLease>,
    pub(crate) engine: Option<SharedEngine>,
    pub(crate) on_check_stream_ended: Option<CheckStreamEndedCallback>,
    pub(crate) events: Option<UnboundedSender<StreamEvent>>,
    pub(crate) last_network_interface: Option<InterfaceType>,
}

pub(crate) struct Inner {
    pub(crate) config: Configuration,
    pub(crate) state: Mutex<State>,
    pub(crate) proxy: LocalStreamProxy,
    pub(crate) low_latency: Option<LowLatencyController>,
    pub(crate) reconnection: PlaybackReconnectionHandler,
    pub(crate) fetcher: Arc<dyn ManifestFetcher>,
    pub(crate) parser: HlsManifestParser,
    pub(crate) requires_authenticated_proxy: bool,
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.proxy.stop();
        self.reconnection.cancel();
        if let Some(c) = &self.low_latency {
            c.stop_sync(true);
        }
    }
}

/// 스트림 재생 코디네이터. 복제하면 같은 코디네이터를 가리킨다.
#[derive(Clone)]
pub struct StreamCoordinator {
    pub(crate) inner: Arc<Inner>,
}

impl std::fmt::Debug for StreamCoordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamCoordinator")
            .field("channel_id", &self.inner.config.channel_id)
            .field("phase", &self.phase())
            .finish()
    }
}

/// 재생 자원 단계 비교 상수 — 백그라운드 복귀 시 매니페스트 갱신을 건너뛰는 TTL(초).
pub const BACKGROUND_RECOVERY_MANIFEST_TTL: f64 = 20.0;
/// 재연결 이중 트리거 쿨다운(초).
pub const RECONNECT_COOLDOWN: f64 = 4.0;
/// 멀티라이브 **비선택** 패인의 목표 지연(초) — 안정성 우선(`+RoleLatency`).
pub const SECONDARY_PANE_STABILITY_TARGET_SECONDS: f64 = 10.5;
/// [`StreamCoordinator::set_target_latency_margin`] 상한(초) — 백오프 최대(+3초)와 겹쳐도 지나치게 밀리지 않게.
pub const MAX_TARGET_LATENCY_MARGIN_SECONDS: f64 = 3.0;

impl StreamCoordinator {
    /// 기본 매니페스트 다운로더(reqwest)로 만든다.
    pub fn new(config: Configuration, access: PlaybackKeyAccess) -> Self {
        Self::with_fetcher(config, access, Arc::new(HttpManifestFetcher::new()))
    }

    /// 매니페스트 다운로더를 주입한다(테스트·앱 공용 클라이언트).
    pub fn with_fetcher(
        config: Configuration,
        access: PlaybackKeyAccess,
        fetcher: Arc<dyn ManifestFetcher>,
    ) -> Self {
        Self::build(config, access, fetcher, None)
    }

    /// 프록시를 주입한다(테스트의 모의 CDN 상류).
    pub fn with_proxy(
        config: Configuration,
        access: PlaybackKeyAccess,
        fetcher: Arc<dyn ManifestFetcher>,
        proxy: LocalStreamProxy,
    ) -> Self {
        Self::build(config, access, fetcher, Some(proxy))
    }

    fn build(
        config: Configuration,
        access: PlaybackKeyAccess,
        fetcher: Arc<dyn ManifestFetcher>,
        proxy: Option<LocalStreamProxy>,
    ) -> Self {
        let low_latency = config
            .enable_low_latency
            .then(|| LowLatencyController::new(config.low_latency_config));
        let state = State {
            phase: StreamPhase::Idle,
            user_paused: false,
            master_playlist: None,
            current_quality: None,
            stream_url: None,
            start_time: None,
            is_proxy_active: false,
            requested_proxy_mode: config.stream_proxy_mode,
            proxy_route_status: None,
            proxy_route_generation: 0,
            applying_proxy_mode: false,
            current_variant_url: None,
            last_reconnect_time: None,
            network_activity_suspended: false,
            network_activity_generation: 0,
            low_latency_sync_generation: 0,
            active_hls_latency_control_generation: None,
            managed_sync_active: false,
            time_shift_active: false,
            timeshift_recording_config: None,
            timeshift_factory: None,
            local_timeshift_playing: false,
            is_server_time_machine_stream: false,
            timeshift_transition: 0,
            user_selected_variant: None,
            manifest_refresh_fail_count: 0,
            last_published_buffer_health: None,
            manifest_generation: 0,
            current_manifest_lease: None,
            engine: None,
            on_check_stream_ended: None,
            events: None,
            last_network_interface: None,
        };
        let proxy = proxy.unwrap_or_else(|| LocalStreamProxy::new(access.cookies.clone()));
        proxy.set_perf_tag(&config.channel_id.chars().take(6).collect::<String>());
        Self {
            inner: Arc::new(Inner {
                requires_authenticated_proxy: access.requires_proxy,
                config,
                state: Mutex::new(state),
                proxy,
                low_latency,
                reconnection: PlaybackReconnectionHandler::new(ReconnectionConfig::AGGRESSIVE),
                fetcher,
                parser: HlsManifestParser::new(),
            }),
        }
    }

    pub(crate) fn st(&self) -> MutexGuard<'_, State> {
        self.inner.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn weak(&self) -> Weak<Inner> {
        Arc::downgrade(&self.inner)
    }

    pub(crate) fn from_weak(weak: &Weak<Inner>) -> Option<Self> {
        weak.upgrade().map(|inner| Self { inner })
    }

    pub(crate) fn engine(&self) -> Option<SharedEngine> {
        self.st().engine.clone()
    }

    // MARK: - 공개 조회

    pub fn configuration(&self) -> &Configuration {
        &self.inner.config
    }

    pub fn phase(&self) -> StreamPhase {
        self.st().phase.clone()
    }

    pub fn current_quality(&self) -> Option<StreamQualityInfo> {
        self.st().current_quality.clone()
    }

    pub fn is_playing(&self) -> bool {
        self.st().phase == StreamPhase::Playing
    }

    /// 시작 후 경과 초.
    pub fn uptime(&self) -> f64 {
        self.st()
            .start_time
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(0.0)
    }

    pub fn proxy_route_status(&self) -> Option<StreamProxyRouteStatus> {
        self.st().proxy_route_status
    }

    pub fn current_manifest_lease(&self) -> Option<ManifestLease> {
        self.st().current_manifest_lease.clone()
    }

    pub fn low_latency_controller(&self) -> Option<&LowLatencyController> {
        self.inner.low_latency.as_ref()
    }

    pub fn reconnection_handler(&self) -> &PlaybackReconnectionHandler {
        &self.inner.reconnection
    }

    pub fn stream_proxy(&self) -> &LocalStreamProxy {
        &self.inner.proxy
    }

    pub fn is_proxy_active(&self) -> bool {
        self.st().is_proxy_active
    }

    /// 사용 가능한 품질 목록 — 오디오 전용 variant 는 뺀다.
    pub fn available_qualities(&self) -> Vec<StreamQualityInfo> {
        self.st()
            .master_playlist
            .as_ref()
            .map(|m| {
                m.variants
                    .iter()
                    .filter(|v| is_video_variant(v))
                    .map(StreamQualityInfo::from_variant)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// per-instance 프록시 네트워크 통계.
    pub fn proxy_network_stats(&self) -> ProxyNetworkStats {
        self.inner.proxy.network_stats()
    }

    /// 진단용 전송 레코드(URL·인증 정보 없음).
    pub fn proxy_transfer_diagnostics(&self) -> Vec<ProxyTransferDiagnostic> {
        self.inner.proxy.transfer_diagnostics()
    }

    pub fn set_check_stream_ended_callback(&self, callback: CheckStreamEndedCallback) {
        self.st().on_check_stream_ended = Some(callback);
    }

    /// 호버 프리페치로 받은 마스터 매니페스트를 주입한다.
    pub fn set_prefetched_manifest(&self, manifest: MasterPlaylist) {
        self.st().master_playlist = Some(manifest);
    }

    /// 이벤트 스트림. Swift `AsyncStream` 은 단일 소비자라 여기서도 수신자는 하나다 —
    /// 다시 부르면 새 채널로 바뀌고 이전 수신자는 끝난다. 부르기 전의 이벤트는 버린다.
    pub fn events(&self) -> UnboundedReceiver<StreamEvent> {
        let (tx, rx) = unbounded_channel();
        self.st().events = Some(tx);
        rx
    }

    // MARK: - 내부 도우미

    pub(crate) fn emit_event(&self, event: StreamEvent) {
        let tx = self.st().events.clone();
        if let Some(tx) = tx {
            let _ = tx.send(event);
        }
    }

    pub(crate) fn update_phase(&self, phase: StreamPhase) {
        self.st().phase = phase.clone();
        self.sync_timeshift_fetch_policy();
        self.emit_event(StreamEvent::PhaseChanged(phase));
    }

    /// hls.js 실측 버퍼 비율을 `BufferUpdate` 로 발행한다. 실질적으로 같으면 생략.
    pub(crate) fn publish_buffer_health(&self, level: f64) {
        let clamped = level.clamp(0.0, 1.0);
        let health = BufferHealth::new(clamped, 1.0, clamped >= 0.5);
        {
            let mut s = self.st();
            if s.last_published_buffer_health
                .is_some_and(|last| last.approximately_equals(&health))
            {
                return;
            }
            s.last_published_buffer_health = Some(health);
        }
        self.emit_event(StreamEvent::BufferUpdate(health));
    }
}

/// RESOLUTION 이 있거나 CODECS 에 비디오 코덱이 있으면 비디오 variant.
pub fn is_video_variant(variant: &Variant) -> bool {
    if variant.resolution.contains(['x', 'X']) {
        return true;
    }
    variant.codecs.as_deref().is_some_and(|c| {
        let c = c.to_lowercase();
        ["avc", "hev", "hvc", "h26", "vp9", "av01"]
            .iter()
            .any(|sig| c.contains(sig))
    })
}

/// 1080p 60fps 우선 선택: 1080p·≥59fps → 1080p → 최고 bandwidth.
pub fn select_1080p60_variant(variants: &[Variant]) -> Option<&Variant> {
    let mut sorted: Vec<&Variant> = variants.iter().collect();
    sorted.sort_by_key(|v| std::cmp::Reverse(v.bandwidth));
    sorted
        .iter()
        .find(|v| v.resolution.contains("1080") && v.frame_rate.unwrap_or(0.0) >= 59.0)
        .or_else(|| sorted.iter().find(|v| v.resolution.contains("1080")))
        .or_else(|| sorted.first())
        .copied()
}

/// 사용자 선택 모드를 URL 에 맞게 보정 — chzzk CDN 이 아니면 `None` 모드.
pub fn resolve_proxy_mode(requested: StreamProxyMode, url: &url::Url) -> StreamProxyMode {
    if LocalStreamProxy::needs_proxy(url) {
        requested
    } else {
        StreamProxyMode::None
    }
}
