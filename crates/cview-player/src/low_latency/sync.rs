//! `Sources/CViewPlayer/LowLatencyController+Sync.swift` 포팅 (+ `+Drift.swift` 의 `snapshot`/`reset`,
//! 둘은 `mod.rs` 에 있다) — rate 조정 상태기계와 sync 루프.

use std::sync::{Arc, Weak};

use cview_core::utils::EwmaCalculator;

use crate::clock::{DISTANT_FUTURE, DISTANT_PAST};

use super::{
    lock, with_state, Effect, LatencyProvider, LowLatencyController, State, SyncState,
    MAX_BUFFERING_PAUSE,
};

impl State {
    pub(crate) fn stop_sync_state(&mut self, invalidate_bindings: bool) {
        if invalidate_bindings {
            self.active_binding_generation = None;
        }
        self.state = SyncState::Idle;
        self.current_rate = 1.0;
        self.initial_seek_done = false;
        self.is_paused_for_buffering = false;
        self.buffering_pause_started_at = None;
        self.seek_grace_auto_resume_at = None;
        self.latency_backoff_seconds = 0.0;
        self.latency_backoff_decay_at = DISTANT_PAST;
        self.latency_backoff_floor = 0.0;
        self.latency_backoff_floor_release_at = DISTANT_FUTURE;
        self.published_target_latency = 0.0;
        self.pid.reset();
        self.ewma = EwmaCalculator::new(0.15);
        self.last_process_instant = None;
    }

    /// 지연 측정 1회 처리.
    pub(crate) fn process_latency(&mut self, current_latency: f64) {
        if self.is_paused_for_buffering {
            let now = self.now();
            let stuck = self
                .buffering_pause_started_at
                .is_some_and(|t| now - t >= MAX_BUFFERING_PAUSE);
            let grace_elapsed = self.seek_grace_auto_resume_at.is_some_and(|t| now >= t);
            if !(grace_elapsed || stuck) {
                return;
            }
            self.seek_grace_auto_resume_at = None;
            self.is_paused_for_buffering = false;
            self.buffering_pause_started_at = None;
            if stuck {
                tracing::warn!(
                    "LowLatency: 버퍼링 pause 가 {}s 지속 — PID 안전 재개",
                    MAX_BUFFERING_PAUSE
                );
            }
        }

        let now = self.now();
        self.decay_latency_backoff_if_due(now);
        self.publish_effective_target_latency_if_changed();
        let active_target = self.effective_target_latency();
        let smoothed = self.ewma.update(current_latency);

        if self.on_latency_measured.is_some() {
            self.outbox
                .push(Effect::Measured(current_latency, smoothed, active_target));
        }

        if self.oscillation_max_rate.is_some() && now > self.oscillation_reset_time {
            self.oscillation_max_rate = None;
            self.recent_buffering_timestamps.clear();
        }

        self.post_grace_sample_count = self.post_grace_sample_count.saturating_add(1);
        self.post_grace_last_raw_latency = Some(current_latency);
        let may_issue_seek = self.is_seek_authorized() && self.post_grace_sample_count >= 2;

        if !self.initial_seek_done {
            self.initial_seek_done = true;
            let initial_error = smoothed - active_target;
            if initial_error > 2.0 && self.is_seek_authorized() {
                self.state = SyncState::SeekRequired;
                if self.on_seek_required.is_some() {
                    self.outbox.push(Effect::Seek(active_target));
                }
                self.enter_post_seek_grace();
                return;
            }
        }

        let effective_max = self.effective_max_latency();
        if smoothed > effective_max && !may_issue_seek {
            tracing::debug!(
                "Latency[{}] seek 억제 — authorized={} postGraceSamples={} ewma={smoothed:.1}s",
                self.debug_tag,
                self.is_seek_authorized(),
                self.post_grace_sample_count
            );
        } else if smoothed > effective_max {
            self.state = SyncState::SeekRequired;
            if self.on_seek_required.is_some() {
                self.outbox.push(Effect::Seek(active_target));
            }
            self.enter_post_seek_grace();
            return;
        }

        let error = smoothed - active_target;
        let measured_dt = match self.last_process_instant {
            Some(last) => now - last,
            None => 5.0,
        };
        self.last_process_instant = Some(now);
        let dt = measured_dt.clamp(0.5, 15.0);
        let pid_output = self.pid.update(error, dt);

        let cfg = self.config;
        let min_step = 0.005f64.max(cfg.rate_significance_threshold);
        let mut new_rate;
        if error > cfg.catch_up_threshold {
            let bh = self.buffer_health_provider.as_ref().map_or(1.0, |p| p());
            let damping = if bh < 0.3 {
                0.0
            } else if bh < 0.6 {
                0.5
            } else {
                1.0
            };
            let rate_offset = (pid_output * 0.3).min(cfg.max_playback_rate - 1.0) * damping;
            new_rate = (1.0 + (min_step * damping).max(rate_offset)).min(cfg.max_playback_rate);
            self.state = SyncState::CatchingUp;
        } else if error < -cfg.slow_down_threshold {
            let rate_offset = (pid_output * 0.3).max(cfg.min_playback_rate - 1.0);
            new_rate = (1.0 + (-min_step).min(rate_offset)).max(cfg.min_playback_rate);
            self.state = SyncState::SlowingDown;
        } else {
            new_rate = 1.0;
            self.state = SyncState::Synced;
        }

        if new_rate > 1.0 {
            new_rate = new_rate.min(self.effective_max_rate());
        }

        if (new_rate - self.current_rate).abs() > cfg.rate_significance_threshold {
            self.current_rate = new_rate;
            self.emit_rate(new_rate);
        }
    }
}

impl LowLatencyController {
    /// 측정 1회 처리.
    pub fn process_latency(&self, current_latency: f64) {
        self.with(|s| s.process_latency(current_latency));
    }

    /// sync 모니터링 중지. `invalidate_bindings` 가 true 면 소유 세대도 해제한다(Swift 기본값).
    pub fn stop_sync(&self, invalidate_bindings: bool) {
        if let Some(task) = self
            .sync_task
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            task.abort();
        }
        self.with(|s| s.stop_sync_state(invalidate_bindings));
    }

    /// sync 루프 시작. `owner_generation` 이 설치된 배선과 다르면 무시한다. tokio 런타임 안에서 불러야 한다.
    pub fn start_sync(&self, owner_generation: Option<u64>, latency_provider: LatencyProvider) {
        if let Some(owner) = owner_generation {
            if lock(&self.inner).active_binding_generation != Some(owner) {
                return;
            }
        }
        self.stop_sync(false);
        self.with(|s| {
            s.state = SyncState::Synced;
            s.initial_seek_done = false;
            s.publish_effective_target_latency_if_changed();
        });
        let weak = Arc::downgrade(&self.inner);
        let interval = self.sync_interval.clone();
        let handle = tokio::spawn(async move {
            loop {
                let sleep = *interval.lock().unwrap_or_else(|e| e.into_inner());
                tokio::time::sleep(sleep.max(std::time::Duration::from_millis(500))).await;
                if Weak::strong_count(&weak) == 0 {
                    break;
                }
                if let Some(latency) = latency_provider().await {
                    let Some(state) = weak.upgrade() else { break };
                    with_state(&state, |s| s.process_latency(latency));
                }
            }
        });
        *self.sync_task.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle);
    }
}

/// 동기 클로저를 [`LatencyProvider`] 로 감싼다.
pub fn latency_provider(f: impl Fn() -> Option<f64> + Send + Sync + 'static) -> LatencyProvider {
    Arc::new(move || {
        let v = f();
        Box::pin(async move { v })
    })
}

#[cfg(test)]
mod tests {
    //! `Tests/CViewPlayerTests/LowLatencyControllerTests.swift` · `LatencyPairingParityTests`(발화 지점) 포팅.
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::Mutex;

    use super::super::*;
    use super::*;

    fn cfg(target: f64) -> LowLatencyConfiguration {
        LowLatencyConfiguration {
            target_latency: target,
            ..Default::default()
        }
    }

    #[test]
    fn initial_state() {
        let c = LowLatencyController::default();
        assert_eq!(c.sync_state(), SyncState::Idle);
        assert_eq!(c.current_rate(), 1.0);
        let u = LowLatencyController::new(LowLatencyConfiguration::ULTRA_LOW);
        assert_eq!(u.sync_state(), SyncState::Idle);
    }

    #[test]
    fn synced_at_target() {
        let c = LowLatencyController::default();
        for _ in 0..50 {
            c.process_latency(3.0);
        }
        assert_eq!(c.sync_state(), SyncState::Synced);
        assert!((c.current_rate() - 1.0).abs() < 0.02);
        let snap = c.snapshot(3.0);
        assert_eq!(snap.sync_state, SyncState::Synced);
    }

    #[test]
    fn catching_up_and_slowing_down() {
        let config = LowLatencyConfiguration {
            target_latency: 3.0,
            catch_up_threshold: 1.5,
            slow_down_threshold: 0.5,
            ..Default::default()
        };
        let c = LowLatencyController::new(config);
        c.process_latency(3.0);
        for _ in 0..10 {
            c.process_latency(6.0);
        }
        assert_eq!(c.sync_state(), SyncState::CatchingUp);
        assert!(c.current_rate() > 1.0);

        let c = LowLatencyController::new(config);
        for _ in 0..10 {
            c.process_latency(1.0);
        }
        assert_eq!(c.sync_state(), SyncState::SlowingDown);
        assert!(c.current_rate() < 1.0);
    }

    #[test]
    fn seek_required_and_callback() {
        let config = LowLatencyConfiguration {
            target_latency: 3.0,
            max_latency: 10.0,
            ..Default::default()
        };
        let c = LowLatencyController::new(config);
        let seek = Arc::new(Mutex::new(None));
        let s2 = seek.clone();
        c.set_on_seek_required(move |t| *s2.lock().unwrap() = Some(t));
        for _ in 0..10 {
            c.process_latency(15.0);
        }
        assert_eq!(c.sync_state(), SyncState::SeekRequired);
        assert_eq!(*seek.lock().unwrap(), Some(3.0));
    }

    #[test]
    fn seek_suppressed_without_authority_and_resumes() {
        let config = LowLatencyConfiguration {
            target_latency: 3.0,
            max_latency: 10.0,
            ..Default::default()
        };
        let c = LowLatencyController::new(config);
        c.set_seek_authority_provider(|| false);
        for i in 0..10 {
            c.process_latency(15.0 + i as f64 * 0.1);
        }
        assert_ne!(c.sync_state(), SyncState::SeekRequired);

        let c = LowLatencyController::new(config);
        let allowed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let a2 = allowed.clone();
        c.set_seek_authority_provider(move || a2.load(Ordering::SeqCst));
        for i in 0..6 {
            c.process_latency(15.0 + i as f64 * 0.1);
        }
        assert_ne!(c.sync_state(), SyncState::SeekRequired);
        allowed.store(true, Ordering::SeqCst);
        for i in 0..6 {
            c.process_latency(15.0 + i as f64 * 0.1);
        }
        assert_eq!(c.sync_state(), SyncState::SeekRequired);

        let c = LowLatencyController::new(config);
        for i in 0..10 {
            c.process_latency(15.0 + i as f64 * 0.1);
        }
        assert_eq!(c.sync_state(), SyncState::SeekRequired);
    }

    #[test]
    fn rate_clamped() {
        let config = LowLatencyConfiguration {
            target_latency: 3.0,
            max_playback_rate: 1.15,
            catch_up_threshold: 1.5,
            ..Default::default()
        };
        let c = LowLatencyController::new(config);
        for _ in 0..20 {
            c.process_latency(9.0);
        }
        assert!(c.current_rate() <= 1.15);
        let config = LowLatencyConfiguration {
            target_latency: 3.0,
            min_playback_rate: 0.9,
            slow_down_threshold: 0.5,
            ..Default::default()
        };
        let c = LowLatencyController::new(config);
        for _ in 0..20 {
            c.process_latency(0.1);
        }
        assert!(c.current_rate() >= 0.9);
    }

    #[test]
    fn rate_change_callback() {
        let c = LowLatencyController::default();
        let rate = Arc::new(Mutex::new(None));
        let r2 = rate.clone();
        c.set_on_rate_change(move |v| *r2.lock().unwrap() = Some(v));
        c.process_latency(3.0);
        for _ in 0..20 {
            c.process_latency(6.0);
        }
        let v = rate.lock().unwrap().unwrap();
        assert_ne!(v, 1.0);
    }

    #[test]
    fn snapshot_fields() {
        let c = LowLatencyController::new(cfg(3.0));
        let snap = c.snapshot(4.5);
        assert_eq!(snap.current_latency, 4.5);
        assert_eq!(snap.target_latency, 3.0);
        assert_eq!(snap.playback_rate, 1.0);
        assert_eq!(snap.sync_state, SyncState::Idle);
    }

    #[test]
    fn reset_restores_idle() {
        let c = LowLatencyController::default();
        for _ in 0..10 {
            c.process_latency(8.0);
        }
        c.reset();
        assert_eq!(c.sync_state(), SyncState::Idle);
        assert_eq!(c.current_rate(), 1.0);
    }

    #[tokio::test]
    async fn stop_sync_to_idle() {
        let c = LowLatencyController::default();
        c.start_sync(None, latency_provider(|| Some(3.0)));
        c.stop_sync(true);
        assert_eq!(c.sync_state(), SyncState::Idle);
        assert_eq!(c.current_rate(), 1.0);
    }

    fn noop_bindings(generation: u64, tag: &str, health: f64) -> LowLatencyBindings {
        LowLatencyBindings {
            owner_generation: generation,
            debug_tag: tag.into(),
            on_rate_change: Arc::new(|_| {}),
            on_seek_required: Arc::new(|_| {}),
            on_latency_measured: Arc::new(|_, _, _| {}),
            buffer_health_provider: Arc::new(move || health),
            on_target_latency_changed: Arc::new(|_| {}),
            segment_duration_provider: None,
        }
    }

    #[tokio::test]
    async fn stale_binding_rejected() {
        let c = LowLatencyController::default();
        assert!(c.install_bindings(noop_bindings(2, "new", 1.0)));
        assert!(!c.install_bindings(noop_bindings(1, "old", 0.0)));
        assert_eq!(c.debug_tag(), "new");
        c.start_sync(Some(1), latency_provider(|| Some(3.0)));
        assert_eq!(c.sync_state(), SyncState::Idle);
        c.start_sync(Some(2), latency_provider(|| Some(3.0)));
        assert_eq!(c.sync_state(), SyncState::Synced);
        c.stop_sync(true);
    }

    #[test]
    fn edge_cases() {
        let c = LowLatencyController::default();
        for _ in 0..5 {
            c.process_latency(0.0);
        }
        let s = c.sync_state();
        assert!(s == SyncState::SlowingDown || s == SyncState::Synced);

        let c = LowLatencyController::new(LowLatencyConfiguration {
            target_latency: 3.0,
            max_latency: 10.0,
            ..Default::default()
        });
        for _ in 0..20 {
            c.process_latency(10.0);
        }
        assert_ne!(c.sync_state(), SyncState::Idle);
    }

    #[test]
    fn latency_spike() {
        let c = LowLatencyController::default();
        for _ in 0..10 {
            c.process_latency(3.0);
        }
        assert_eq!(c.sync_state(), SyncState::Synced);
        for _ in 0..10 {
            c.process_latency(8.0);
        }
        assert_eq!(c.sync_state(), SyncState::CatchingUp);
        assert!(c.current_rate() > 1.0);
    }

    #[test]
    fn ultra_low_faster_response() {
        let d = LowLatencyController::new(LowLatencyConfiguration::STANDARD);
        let u = LowLatencyController::new(LowLatencyConfiguration::ULTRA_LOW);
        d.process_latency(3.0);
        u.process_latency(1.5);
        for _ in 0..20 {
            d.process_latency(4.0);
            u.process_latency(4.0);
        }
        assert!((d.current_rate() - 1.0).abs() < 0.001);
        assert!(u.current_rate() > 1.0);
    }

    #[derive(Default)]
    struct Box64(AtomicU64);
    impl Box64 {
        fn get(&self) -> f64 {
            f64::from_bits(self.0.load(Ordering::SeqCst))
        }
    }

    #[test]
    fn publishes_only_on_change() {
        let c = LowLatencyController::new(LowLatencyConfiguration::WEB_PARITY_HLSJS);
        let published = Arc::new(Box64::default());
        let count = Arc::new(AtomicUsize::new(0));
        let (p, n) = (published.clone(), count.clone());
        c.set_on_target_latency_changed(move |v| {
            p.0.store(v.to_bits(), Ordering::SeqCst);
            n.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(published.get(), 3.0);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        c.process_latency(3.1);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        c.set_latency_backoff_enabled(true);
        c.bump_latency_backoff();
        assert_eq!(published.get(), 3.75);
        assert_eq!(count.load(Ordering::SeqCst), 2);
        let after_one_hold = crate::clock::SystemClock.now() + LATENCY_BACKOFF_HOLD + 1.0;
        c.decay_latency_backoff_if_due_at(after_one_hold);
        assert_eq!(published.get(), 3.75);
        assert_eq!(count.load(Ordering::SeqCst), 2);
        c.decay_latency_backoff_if_due_at(after_one_hold + LATENCY_BACKOFF_FLOOR_RELEASE + 1.0);
        assert_eq!(published.get(), 3.0);
        assert_eq!(count.load(Ordering::SeqCst), 3);
        c.update_configuration(LowLatencyConfiguration::WEB_SYNC);
        assert_eq!(published.get(), 6.0);
        assert_eq!(count.load(Ordering::SeqCst), 4);
    }

    #[tokio::test]
    async fn republishes_after_session_boundary() {
        let c = LowLatencyController::new(LowLatencyConfiguration::WEB_PARITY_HLSJS);
        let count = Arc::new(AtomicUsize::new(0));
        let n = count.clone();
        c.set_on_target_latency_changed(move |_| {
            n.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(count.load(Ordering::SeqCst), 1);
        c.stop_sync(true);
        c.start_sync(None, latency_provider(|| None));
        assert_eq!(count.load(Ordering::SeqCst), 2);
        c.stop_sync(true);
    }

    use crate::clock::Clock;
}
