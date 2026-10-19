//! `StreamCoordinator+Recovery.swift`(복구 supervisor) · `+Reconnection.swift`(네트워크 경로 복구 판정) 포팅.

use futures_util::future::BoxFuture;

use super::{StreamCoordinator, StreamPhase};
use crate::policy::manifest_refresh_policy;
use crate::policy::{RecoveryDecision, RecoverySignal};

/// signal → decision 매핑.
pub fn decide_recovery(signal: &RecoverySignal) -> RecoveryDecision {
    match signal {
        RecoverySignal::StreamEndedConfirmed => RecoveryDecision::MarkEnded,
        // hls.js 는 자체 ABR 로 variant 를 다시 고르므로 재연결 시 마스터를 재해석한다.
        RecoverySignal::EngineError { .. } => RecoveryDecision::Reconnect,
        RecoverySignal::PlaybackStall { .. } => RecoveryDecision::Reconnect,
        // 403/401 — 토큰 만료 가능성.
        RecoverySignal::UpstreamAuthFailure { .. } => RecoveryDecision::Reconnect,
        // 갱신 cycle 이 다음 시도를 예약하므로 4회까지는 무시, 5회째 재연결로 승격.
        RecoverySignal::ManifestRefreshFailure { count } => {
            if manifest_refresh_policy::should_escalate(*count) {
                RecoveryDecision::Reconnect
            } else {
                RecoveryDecision::Ignore {
                    reason: format!("manifest refresh retry-cycle ({count}/5)"),
                }
            }
        }
    }
}

fn reason_text(signal: &RecoverySignal) -> String {
    match signal {
        RecoverySignal::EngineError { engine, detail } => {
            format!("engine error ({:?}): {detail}", engine)
        }
        RecoverySignal::PlaybackStall { source } => format!("playback stall ({source})"),
        RecoverySignal::ManifestRefreshFailure { count } => {
            format!("manifest refresh {count}회 연속 실패")
        }
        RecoverySignal::UpstreamAuthFailure { status } => {
            format!("upstream auth failure (HTTP {status})")
        }
        RecoverySignal::StreamEndedConfirmed => "stream ended confirmed".into(),
    }
}

/// 오프라인에서 복구됐을 때 재연결 루프가 할 일.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NetworkRecoveryAction {
    Ignore,
    SkipReconnectBackoff,
    RestartAfterExhaustion,
    ResetStallTracking,
}

/// 네트워크 경로 종류 (Swift `AVPlayerNetworkMonitor.InterfaceType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InterfaceType {
    WiredEthernet,
    Wifi,
    Cellular,
    Other,
    Offline,
}

/// 오프라인 → 온라인만 재연결을 앞당긴다. 인터페이스 전환·정상 재생 중 경로 변경은 대상이 아니다.
pub fn network_recovery_decision(
    previous: InterfaceType,
    current: InterfaceType,
    phase: &StreamPhase,
    user_paused: bool,
    has_stream_url: bool,
    suspend_intent: bool,
) -> NetworkRecoveryAction {
    if !has_stream_url || user_paused || suspend_intent {
        return NetworkRecoveryAction::Ignore;
    }
    if previous != InterfaceType::Offline || current == InterfaceType::Offline {
        return NetworkRecoveryAction::Ignore;
    }
    match phase {
        StreamPhase::Reconnecting => NetworkRecoveryAction::SkipReconnectBackoff,
        StreamPhase::Error(_) => NetworkRecoveryAction::RestartAfterExhaustion,
        StreamPhase::Playing | StreamPhase::Buffering => NetworkRecoveryAction::ResetStallTracking,
        _ => NetworkRecoveryAction::Ignore,
    }
}

impl StreamCoordinator {
    /// 외부 복구 신호를 평가해 결정을 실행한다. 호출 측은 cooldown·phase·종료 확인을 다루지 않는다.
    pub async fn handle_recovery_signal(&self, signal: RecoverySignal) -> RecoveryDecision {
        let decision = decide_recovery(&signal);
        self.apply_decision(&decision, &signal).await;
        decision
    }

    fn apply_decision<'a>(
        &'a self,
        decision: &'a RecoveryDecision,
        signal: &'a RecoverySignal,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            match decision {
                RecoveryDecision::Ignore { reason } => {
                    tracing::debug!("recovery: ignore ({reason}) signal={signal:?}");
                }
                RecoveryDecision::RefreshManifest => self.refresh_master_manifest().await,
                RecoveryDecision::Reconnect => self.trigger_reconnect(&reason_text(signal), false),
                RecoveryDecision::MarkEnded => self.handle_stream_ended(),
                RecoveryDecision::FailUserVisible(msg) => {
                    self.update_phase(StreamPhase::Error(msg.clone()))
                }
            }
        })
    }

    /// 앱의 경로 감시자가 부른다(Swift 는 `AVPlayerNetworkMonitor` 구독).
    pub fn handle_transport_path_change(&self, current: InterfaceType) {
        let (previous, phase, paused, has_url, engine) = {
            let mut s = self.st();
            let previous = s.last_network_interface.replace(current);
            (
                previous,
                s.phase.clone(),
                s.user_paused,
                s.stream_url.is_some(),
                s.engine.clone(),
            )
        };
        let Some(previous) = previous else { return };
        let suspend = engine
            .as_ref()
            .is_some_and(|e| e.is_suspend_intent_active());
        let action = network_recovery_decision(previous, current, &phase, paused, has_url, suspend);
        let note_recovered = || {
            if let Some(h) = engine.as_ref().and_then(|e| e.hls()) {
                h.note_network_transport_recovered();
            }
        };
        match action {
            NetworkRecoveryAction::Ignore => {}
            NetworkRecoveryAction::SkipReconnectBackoff => {
                tracing::info!("StreamCoordinator: 네트워크 복구 — 재연결 백오프 생략");
                self.inner.reconnection.request_immediate_retry();
                note_recovered();
            }
            NetworkRecoveryAction::RestartAfterExhaustion => {
                tracing::info!("StreamCoordinator: 네트워크 복구 — 소진된 재연결 재시작");
                self.trigger_reconnect("network recovered", true);
            }
            NetworkRecoveryAction::ResetStallTracking => note_recovered(),
        }
    }

    pub(crate) fn unsubscribe_network_monitor(&self) {
        self.st().last_network_interface = None;
    }
}
