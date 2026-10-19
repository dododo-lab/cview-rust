//! `StreamCoordinator+ProxyRoute.swift` 포팅 — 프록시 경로 결정·설정 변경 시 경로만 재로드.

use std::sync::Arc;

use cview_core::models::stream_proxy::{StreamProxyMode, StreamProxyRouteStatus};
use url::Url;

use super::{
    resolve_proxy_mode, SharedEngine, StreamCoordinator, StreamCoordinatorError, StreamEvent,
    StreamPhase,
};
use crate::proxy::LocalStreamProxy;

impl StreamCoordinator {
    pub(crate) async fn apply_pending_proxy_mode(&self) {
        let pending = {
            let s = self.st();
            s.proxy_route_status
                .is_some_and(|st| st.pending)
                .then_some(s.requested_proxy_mode)
        };
        if let Some(mode) = pending {
            self.apply_stream_proxy_mode(mode).await;
        }
    }

    fn publish_proxy_route(&self, status: StreamProxyRouteStatus) {
        self.st().proxy_route_status = Some(status);
        self.emit_event(StreamEvent::ProxyRouteChanged(status));
    }

    /// 초기 로드와 설정 변경이 공유한다. 인증 요구는 폴백해도 유지된다(실패 시 직접 URL 로 떨어지지 않는다).
    pub(crate) async fn configure_proxy_route(
        &self,
        url: &Url,
    ) -> Result<Url, StreamCoordinatorError> {
        let supported = LocalStreamProxy::needs_proxy(url);
        let requires_auth = self.inner.requires_authenticated_proxy;
        let (requested, generation) = {
            let s = self.st();
            (s.requested_proxy_mode, s.proxy_route_generation)
        };
        let owns = |this: &Self| {
            let s = this.st();
            s.proxy_route_generation == generation && s.stream_url.is_some()
        };
        let should_proxy = supported && (requires_auth || requested == StreamProxyMode::LocalProxy);
        let mut started = false;
        if should_proxy {
            if let Some(host) = url.host_str() {
                match self.inner.proxy.start(host).await {
                    Ok(_) => {
                        if !owns(self) {
                            return Err(StreamCoordinatorError::Cancelled);
                        }
                        started = true;
                    }
                    Err(e) => tracing::warn!("Proxy: 연결 준비 실패 — {e}"),
                }
            }
        }
        if !owns(self) {
            return Err(StreamCoordinatorError::Cancelled);
        }
        let status = {
            let mut s = self.st();
            let mut status =
                StreamProxyRouteStatus::resolve(requested, requires_auth, supported, started);
            status.requested = s.requested_proxy_mode;
            status.pending = requested != s.requested_proxy_mode;
            s.is_proxy_active = status.effective == Some(StreamProxyMode::LocalProxy);
            status
        };
        self.publish_proxy_route(status);
        if status.effective.is_none() {
            return Err(StreamCoordinatorError::ProxyAuthenticationUnavailable);
        }
        if status.effective == Some(StreamProxyMode::LocalProxy) {
            let weak = self.weak();
            self.inner
                .proxy
                .set_on_upstream_auth_failure(Some(Arc::new(move || {
                    if let Some(this) = StreamCoordinator::from_weak(&weak) {
                        this.trigger_reconnect("CDN 403 토큰 만료 감지", false);
                    }
                })));
            self.attach_timeshift_recorder_if_needed();
            return Ok(self.inner.proxy.proxy_url(url));
        }
        self.inner.proxy.stop();
        Ok(url.clone())
    }

    /// 코디네이터·엔진·화질·볼륨은 그대로 두고 경로가 바뀔 때만 재로드한다.
    /// 일시정지·되감기·엔진 정지 의도 중에는 대기(pending)했다가 실제 재생 복귀 때 적용한다.
    pub async fn apply_stream_proxy_mode(&self, mode: StreamProxyMode) {
        self.st().requested_proxy_mode = mode;
        self.mark_proxy_mode_pending();
        {
            let mut s = self.st();
            if s.applying_proxy_mode {
                return;
            }
            s.applying_proxy_mode = true;
        }
        self.apply_stream_proxy_mode_loop().await;
        self.st().applying_proxy_mode = false;
    }

    async fn apply_stream_proxy_mode_loop(&self) {
        let requires_auth = self.inner.requires_authenticated_proxy;
        loop {
            let (raw_url, engine, requested, effective, proxy_active) = {
                let s = self.st();
                let (Some(raw), Some(engine)) = (s.stream_url.clone(), s.engine.clone()) else {
                    return;
                };
                (
                    raw,
                    engine,
                    s.requested_proxy_mode,
                    s.proxy_route_status.and_then(|st| st.effective),
                    s.is_proxy_active,
                )
            };
            let desired = resolve_proxy_mode(
                if requires_auth {
                    StreamProxyMode::LocalProxy
                } else {
                    requested
                },
                &raw_url,
            );
            if effective == Some(desired) {
                self.publish_proxy_route(StreamProxyRouteStatus::resolve(
                    requested,
                    requires_auth,
                    LocalStreamProxy::needs_proxy(&raw_url),
                    proxy_active,
                ));
                return;
            }
            if !self.can_apply_proxy_mode() || engine.is_suspend_intent_active() {
                return;
            }
            if !self.reload_proxy_route(&raw_url, &engine).await {
                return;
            }
            if requested == self.st().requested_proxy_mode {
                return;
            }
            // await 중 더 새 선택이 들어왔다 — 새 엔진의 playing 뒤에 적용한다.
            self.mark_proxy_mode_pending();
        }
    }

    fn can_apply_proxy_mode(&self) -> bool {
        let s = self.st();
        !s.user_paused
            && !s.time_shift_active
            && !s.local_timeshift_playing
            && s.phase.is_playing_or_buffering()
    }

    fn mark_proxy_mode_pending(&self) {
        let status = {
            let s = self.st();
            s.proxy_route_status.map(|mut st| {
                st.requested = s.requested_proxy_mode;
                st.pending = true;
                st
            })
        };
        if let Some(st) = status {
            self.publish_proxy_route(st);
        }
    }

    fn owns_proxy_route(&self, generation: u64) -> bool {
        let s = self.st();
        s.proxy_route_generation == generation && s.stream_url.is_some()
    }

    async fn reload_proxy_route(&self, raw_url: &Url, engine: &SharedEngine) -> bool {
        let generation = self.st().proxy_route_generation;
        self.inner.reconnection.cancel();
        self.stop_latency_subsystems();
        if !self.owns_proxy_route(generation)
            || !self.can_apply_proxy_mode()
            || engine.is_suspend_intent_active()
        {
            return false;
        }
        engine.stop();
        let source = self.reconnect_playback_url(raw_url);
        let fail = |this: &Self, message: String| {
            if this.owns_proxy_route(generation) {
                this.update_phase(StreamPhase::Error(message.clone()));
                this.emit_event(StreamEvent::Error(message));
            }
            false
        };
        let url = match self.configure_proxy_route(&source).await {
            Ok(url) => url,
            Err(StreamCoordinatorError::Cancelled) => return false,
            Err(e) => return fail(self, e.to_string()),
        };
        if !self.owns_proxy_route(generation) {
            return false;
        }
        if !self.st().user_paused {
            self.update_phase(StreamPhase::Connecting);
        }
        if let Err(e) = engine.play(&url).await {
            return fail(self, e.to_string());
        }
        if !self.owns_proxy_route(generation) {
            return false;
        }
        if self.st().user_paused {
            engine.pause();
        }
        self.reconcile_network_activity().await;
        true
    }
}
