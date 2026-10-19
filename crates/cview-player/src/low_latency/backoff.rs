//! `Sources/CViewPlayer/LowLatencyController+Backoff.swift` 포팅 — 적응형 목표 지연 백오프와
//! 세그먼트 길이 기반 구조적 하한.
//!
//! 재버퍼가 반복되면 PID setpoint 를 한시적으로 뒤로 물려 전방 버퍼를 벌어 준다. 한 번이라도
//! 재버퍼를 겪은 세션은 감쇠가 0 까지 내려가지 않고(바닥 = 한 단계), 600초 무사고면 바닥이 풀린다.
//! 바닥·하한의 실측 근거는 원본 Swift 주석을 참조.

use crate::clock::{DISTANT_FUTURE, DISTANT_PAST};

use super::{LowLatencyController, State};

pub const LATENCY_BACKOFF_STEP: f64 = 0.75;
pub const LATENCY_BACKOFF_MAX: f64 = 3.0;
/// 한 단계 되돌리기까지 필요한 무사고 재생 시간.
pub const LATENCY_BACKOFF_HOLD: f64 = 45.0;
/// 감쇠 바닥 해제까지의 무사고 시간.
pub const LATENCY_BACKOFF_FLOOR_RELEASE: f64 = 600.0;
/// 구조적 하한 = 실측 세그먼트 길이 + 이 여유.
pub const SEGMENT_TARGET_MARGIN: f64 = 1.0;

impl State {
    pub(crate) fn set_latency_backoff_enabled(&mut self, enabled: bool) {
        self.latency_backoff_enabled = enabled;
        if !enabled {
            self.latency_backoff_seconds = 0.0;
            self.latency_backoff_decay_at = DISTANT_PAST;
            self.latency_backoff_floor = 0.0;
            self.latency_backoff_floor_release_at = DISTANT_FUTURE;
            self.publish_effective_target_latency_if_changed();
        }
    }

    /// 실측 세그먼트 길이에서 유도한 목표 지연의 구조적 하한. 미측정이면 0.
    pub(crate) fn structural_minimum_target(&self) -> f64 {
        match self.segment_duration_provider.as_ref().map(|p| p()) {
            Some(d) if d > 0.1 && d < 30.0 => d + SEGMENT_TARGET_MARGIN,
            _ => 0.0,
        }
    }

    /// 현재 실효 목표 지연 — PID setpoint / seek 목표 / 측정 보고에 모두 같은 값을 쓴다.
    pub(crate) fn effective_target_latency(&self) -> f64 {
        (self.config.target_latency + self.latency_backoff_seconds)
            .max(self.structural_minimum_target())
    }

    /// 하드 seek 문턱 — 밴드 폭을 보존한다.
    pub(crate) fn effective_max_latency(&self) -> f64 {
        self.effective_target_latency() + (self.config.max_latency - self.config.target_latency)
    }

    /// 재버퍼 1회 관측 — 목표 지연을 한 단계 올리고 감쇠 시계를 재무장한다.
    pub(crate) fn bump_latency_backoff(&mut self, now: f64) {
        if !self.latency_backoff_enabled {
            return;
        }
        let next = LATENCY_BACKOFF_MAX.min(self.latency_backoff_seconds + LATENCY_BACKOFF_STEP);
        self.latency_backoff_decay_at = now + LATENCY_BACKOFF_HOLD;
        self.latency_backoff_floor = LATENCY_BACKOFF_STEP;
        self.latency_backoff_floor_release_at = now + LATENCY_BACKOFF_FLOOR_RELEASE;
        if next == self.latency_backoff_seconds {
            return;
        }
        self.latency_backoff_seconds = next;
        self.publish_effective_target_latency_if_changed();
        tracing::info!(
            "LowLatency: 재버퍼 감지 — 목표 지연 백오프 +{next:.2}s (실효 {:.2}s)",
            self.config.target_latency + next
        );
    }

    /// 무사고 재생이 이어지면 한 단계씩 저지연 기본값(바닥)으로 복귀.
    pub(crate) fn decay_latency_backoff_if_due(&mut self, now: f64) {
        if !self.latency_backoff_enabled
            || self.latency_backoff_seconds <= 0.0
            || now < self.latency_backoff_decay_at
        {
            return;
        }
        if self.latency_backoff_floor > 0.0 && now >= self.latency_backoff_floor_release_at {
            self.latency_backoff_floor = 0.0;
            self.latency_backoff_floor_release_at = DISTANT_FUTURE;
        }
        let next = self
            .latency_backoff_floor
            .max(self.latency_backoff_seconds - LATENCY_BACKOFF_STEP);
        self.latency_backoff_decay_at = now + LATENCY_BACKOFF_HOLD;
        if next == self.latency_backoff_seconds {
            return;
        }
        self.latency_backoff_seconds = next;
        self.publish_effective_target_latency_if_changed();
        tracing::info!(
            "LowLatency: 안정 재생 — 목표 지연 백오프 완화 {:.2}s (바닥 {:.2}s)",
            self.latency_backoff_seconds,
            self.latency_backoff_floor
        );
    }
}

impl LowLatencyController {
    pub fn set_latency_backoff_enabled(&self, enabled: bool) {
        self.with(|s| s.set_latency_backoff_enabled(enabled));
    }

    /// 테스트 전용 주입점 — 프로덕션은 `install_bindings(segment_duration_provider)` 를 쓴다.
    pub fn set_segment_duration_provider_for_tests(
        &self,
        f: impl Fn() -> f64 + Send + Sync + 'static,
    ) {
        self.with(|s| s.segment_duration_provider = Some(std::sync::Arc::new(f)));
    }

    pub fn effective_target_latency(&self) -> f64 {
        self.with(|s| s.effective_target_latency())
    }

    /// Swift `currentEffectiveTargetLatency`.
    pub fn current_effective_target_latency(&self) -> f64 {
        self.effective_target_latency()
    }

    pub fn effective_max_latency(&self) -> f64 {
        self.with(|s| s.effective_max_latency())
    }

    pub fn latency_backoff_seconds(&self) -> f64 {
        self.with(|s| s.latency_backoff_seconds)
    }

    pub fn latency_backoff_floor(&self) -> f64 {
        self.with(|s| s.latency_backoff_floor)
    }

    /// 현재 시각으로 백오프 한 단계.
    pub fn bump_latency_backoff(&self) {
        self.with(|s| {
            let now = s.now();
            s.bump_latency_backoff(now)
        });
    }

    pub fn bump_latency_backoff_at(&self, now: f64) {
        self.with(|s| s.bump_latency_backoff(now));
    }

    pub fn decay_latency_backoff_if_due(&self) {
        self.with(|s| {
            let now = s.now();
            s.decay_latency_backoff_if_due(now)
        });
    }

    pub fn decay_latency_backoff_if_due_at(&self, now: f64) {
        self.with(|s| s.decay_latency_backoff_if_due(now));
    }
}

#[cfg(test)]
mod tests {
    //! `Tests/CViewPlayerTests/LowLatencyBackoffTests.swift` 포팅.
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    use super::super::*;
    use super::*;

    fn make() -> LowLatencyController {
        LowLatencyController::new(LowLatencyConfiguration::WEB_PARITY_HLSJS)
    }

    #[derive(Default)]
    struct Cell(AtomicU64);
    impl Cell {
        fn get(&self) -> f64 {
            f64::from_bits(self.0.load(Ordering::SeqCst))
        }
        fn set(&self, v: f64) {
            self.0.store(v.to_bits(), Ordering::SeqCst)
        }
    }

    #[test]
    fn disabled_by_default() {
        let c = make();
        c.pause_for_buffering(true);
        c.pause_for_buffering(true);
        assert_eq!(c.effective_target_latency(), 3.0);
        assert_eq!(c.effective_max_latency(), 8.0);
        assert_eq!(c.latency_backoff_seconds(), 0.0);
    }

    #[test]
    fn bumps_on_each_rebuffer() {
        let c = make();
        c.set_latency_backoff_enabled(true);
        c.pause_for_buffering(true);
        assert_eq!(c.effective_target_latency(), 3.75);
        assert_eq!(c.effective_max_latency(), 8.75);
        c.pause_for_buffering(true);
        assert_eq!(c.effective_target_latency(), 4.5);
    }

    #[test]
    fn segment_floor() {
        let c = make();
        let d = Arc::new(Cell::default());
        d.set(4.166);
        let d2 = d.clone();
        c.set_segment_duration_provider_for_tests(move || d2.get());
        assert_eq!(c.effective_target_latency(), 4.166 + 1.0);
        assert_eq!(c.effective_max_latency(), 4.166 + 1.0 + 5.0);
        d.set(2.0);
        assert_eq!(c.effective_target_latency(), 3.0);
        assert_eq!(c.effective_max_latency(), 8.0);
        d.set(0.0);
        assert_eq!(c.effective_target_latency(), 3.0);
    }

    #[test]
    fn segment_floor_and_backoff_compose() {
        let c = make();
        c.set_segment_duration_provider_for_tests(|| 4.166);
        c.set_latency_backoff_enabled(true);
        c.bump_latency_backoff();
        assert_eq!(c.effective_target_latency(), 5.166);
        for _ in 0..5 {
            c.bump_latency_backoff();
        }
        assert_eq!(c.effective_target_latency(), 6.0);
    }

    #[test]
    fn runtime_segment_floor_is_published() {
        let c = make();
        let published = Arc::new(Cell::default());
        let p = published.clone();
        c.set_on_target_latency_changed(move |v| p.set(v));
        assert_eq!(published.get(), 3.0);
        let d = Arc::new(Cell::default());
        let d2 = d.clone();
        c.set_segment_duration_provider_for_tests(move || d2.get());
        c.process_latency(3.2);
        assert_eq!(published.get(), 3.0);
        d.set(4.166);
        c.process_latency(5.0);
        assert_eq!(published.get(), 5.166);
    }

    #[test]
    fn decay_stops_at_floor_after_rebuffer() {
        let c = make();
        c.set_latency_backoff_enabled(true);
        let t0 = 1_000_000.0;
        c.bump_latency_backoff_at(t0);
        assert_eq!(c.effective_target_latency(), 3.75);
        let mut now = t0;
        for _ in 0..6 {
            now += LATENCY_BACKOFF_HOLD + 1.0;
            c.decay_latency_backoff_if_due_at(now);
        }
        assert_eq!(c.latency_backoff_seconds(), LATENCY_BACKOFF_STEP);
        assert_eq!(c.effective_target_latency(), 3.75);
    }

    #[test]
    fn floor_not_armed_without_rebuffer() {
        let c = make();
        c.set_latency_backoff_enabled(true);
        assert_eq!(c.latency_backoff_floor(), 0.0);
        assert_eq!(c.effective_target_latency(), 3.0);
    }

    #[test]
    fn floor_releases_after_long_clean_stretch() {
        let c = make();
        c.set_latency_backoff_enabled(true);
        let t0 = 2_000_000.0;
        c.bump_latency_backoff_at(t0);
        assert_eq!(c.latency_backoff_floor(), LATENCY_BACKOFF_STEP);
        c.decay_latency_backoff_if_due_at(t0 + LATENCY_BACKOFF_FLOOR_RELEASE + 1.0);
        assert_eq!(c.latency_backoff_floor(), 0.0);
        assert_eq!(c.latency_backoff_seconds(), 0.0);
        assert_eq!(c.effective_target_latency(), 3.0);
    }

    #[test]
    fn rebuffer_pushes_floor_release() {
        let c = make();
        c.set_latency_backoff_enabled(true);
        let t0 = 3_000_000.0;
        c.bump_latency_backoff_at(t0);
        c.bump_latency_backoff_at(t0 + LATENCY_BACKOFF_FLOOR_RELEASE - 1.0);
        c.decay_latency_backoff_if_due_at(t0 + LATENCY_BACKOFF_FLOOR_RELEASE + 1.0);
        assert_eq!(c.latency_backoff_floor(), LATENCY_BACKOFF_STEP);
    }

    #[test]
    fn backoff_is_capped() {
        let c = make();
        c.set_latency_backoff_enabled(true);
        for _ in 0..10 {
            c.bump_latency_backoff();
        }
        assert_eq!(c.latency_backoff_seconds(), 3.0);
        assert_eq!(c.effective_target_latency(), 6.0);
    }

    #[test]
    fn decays_stepwise_to_floor() {
        let c = make();
        c.set_latency_backoff_enabled(true);
        let start = 5_000.0;
        c.bump_latency_backoff_at(start);
        c.bump_latency_backoff_at(start);
        assert_eq!(c.latency_backoff_seconds(), 1.5);
        c.decay_latency_backoff_if_due_at(start + 44.0);
        assert_eq!(c.latency_backoff_seconds(), 1.5);
        c.decay_latency_backoff_if_due_at(start + LATENCY_BACKOFF_HOLD);
        assert_eq!(c.latency_backoff_seconds(), 0.75);
        c.decay_latency_backoff_if_due_at(start + LATENCY_BACKOFF_HOLD * 2.0);
        assert_eq!(c.latency_backoff_seconds(), LATENCY_BACKOFF_STEP);
        assert_eq!(c.effective_target_latency(), 3.75);
    }

    #[test]
    fn rebuffer_rearms_decay_clock() {
        let c = make();
        c.set_latency_backoff_enabled(true);
        let start = 9_000.0;
        c.bump_latency_backoff_at(start);
        c.bump_latency_backoff_at(start + LATENCY_BACKOFF_HOLD - 1.0);
        c.decay_latency_backoff_if_due_at(start + LATENCY_BACKOFF_HOLD + 1.0);
        assert_eq!(c.latency_backoff_seconds(), 1.5);
    }

    #[test]
    fn resets_on_session_boundary() {
        let c = make();
        c.set_latency_backoff_enabled(true);
        c.bump_latency_backoff();
        assert_eq!(c.latency_backoff_seconds(), 0.75);
        c.stop_sync(true);
        assert_eq!(c.latency_backoff_seconds(), 0.0);
        assert_eq!(c.latency_backoff_floor(), 0.0);
        c.bump_latency_backoff();
        c.set_latency_backoff_enabled(false);
        assert_eq!(c.latency_backoff_seconds(), 0.0);
        assert_eq!(c.effective_target_latency(), 3.0);
        assert_eq!(c.latency_backoff_floor(), 0.0);
    }

    #[test]
    fn disabling_publishes_lowered_target() {
        let c = make();
        let published = Arc::new(Cell::default());
        let p = published.clone();
        c.set_on_target_latency_changed(move |v| p.set(v));
        c.set_latency_backoff_enabled(true);
        c.bump_latency_backoff();
        assert_eq!(published.get(), 3.75);
        c.set_latency_backoff_enabled(false);
        assert_eq!(published.get(), 3.0);
    }

    fn settled_rate(buffer_health: f64) -> f64 {
        let c = make();
        let rate = Arc::new(Cell::default());
        rate.set(1.0);
        let r = rate.clone();
        c.set_on_rate_change(move |v| r.set(v));
        c.set_buffer_health_provider(move || buffer_health);
        for _ in 0..4 {
            c.process_latency(5.0);
        }
        rate.get()
    }

    #[test]
    fn starved_buffer_stops_catch_up() {
        let full = settled_rate(1.0);
        assert!(full > 1.0);
        assert_eq!(settled_rate(0.2), 1.0);
        let damped = settled_rate(0.45);
        assert!(damped > 1.0);
        assert!(damped < full);
    }
}
