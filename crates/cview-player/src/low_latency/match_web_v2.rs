//! `Sources/CViewPlayer/LowLatencyController+MatchWebV2.swift` 포팅 — 웹 동기화 v2 백오프 전용 루프.
//!
//! v2 세션에서는 JS 의 "드문 시크" 제어기가 재생 위치를 소유한다. Rust 는 PID 배속·시크를 돌리지
//! 않고 재버퍼 백오프와 구조적 하한만 계산해 실효 목표를 게시한다.
//! ⚠ 이 루프에서 `process_latency` 를 부르지 말 것.

use std::sync::{Arc, Weak};

use futures_util::future::BoxFuture;

use super::{lock, with_state, Effect, LowLatencyController, State, SyncState};

/// v2 백오프 루프가 UI 로 흘리는 표본(ingest 기저).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatchWebV2DisplaySample {
    /// JS `ingestLatencyMs / 1000`.
    pub latency: f64,
    /// JS `v2TargetMs / 1000`.
    pub target: f64,
}

/// v2 표본 공급자.
pub type SampleProvider =
    Arc<dyn Fn() -> BoxFuture<'static, Option<MatchWebV2DisplaySample>> + Send + Sync>;

impl State {
    pub(crate) fn process_backoff_only_tick(
        &mut self,
        sample: Option<MatchWebV2DisplaySample>,
        now: f64,
    ) {
        self.decay_latency_backoff_if_due(now);
        self.publish_effective_target_latency_if_changed();
        if let Some(p) = &self.buffer_health_provider {
            let _ = p();
        }
        if let Some(sample) = sample {
            if self.on_latency_measured.is_some() {
                self.outbox.push(Effect::Measured(
                    sample.latency,
                    sample.latency,
                    sample.target,
                ));
            }
        }
    }
}

impl LowLatencyController {
    /// 한 틱: 백오프 감쇠 → 실효 목표 게시 → 버퍼 건강도 1회 → 표본 방출.
    pub fn process_backoff_only_tick(&self, sample: Option<MatchWebV2DisplaySample>) {
        self.with(|s| {
            let now = s.now();
            s.process_backoff_only_tick(sample, now)
        });
    }

    pub fn process_backoff_only_tick_at(&self, sample: Option<MatchWebV2DisplaySample>, now: f64) {
        self.with(|s| s.process_backoff_only_tick(sample, now));
    }

    /// PID·시크 없이 재버퍼 백오프만 돌린다. 소유 세대 검사는 `start_sync` 와 같다.
    pub fn start_backoff_only_sync(&self, owner_generation: Option<u64>, provider: SampleProvider) {
        if let Some(owner) = owner_generation {
            if lock(&self.inner).active_binding_generation != Some(owner) {
                return;
            }
        }
        self.stop_sync(false);
        self.with(|s| {
            s.state = SyncState::Synced;
            s.publish_effective_target_latency_if_changed();
        });
        let weak: Weak<_> = Arc::downgrade(&self.inner);
        let interval = self.sync_interval.clone();
        let handle = tokio::spawn(async move {
            loop {
                let sleep = *interval.lock().unwrap_or_else(|e| e.into_inner());
                tokio::time::sleep(sleep.max(std::time::Duration::from_millis(500))).await;
                if Weak::strong_count(&weak) == 0 {
                    break;
                }
                let sample = provider().await;
                let Some(state) = weak.upgrade() else { break };
                with_state(&state, |s| {
                    let now = s.now();
                    s.process_backoff_only_tick(sample, now)
                });
            }
        });
        *self.sync_task.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle);
    }
}

#[cfg(test)]
mod tests {
    //! `Tests/CViewPlayerTests/LowLatencyControllerMatchWebV2Tests.swift` 포팅.
    use std::sync::Mutex;

    use super::super::*;
    use super::*;
    use crate::clock::{Clock, SystemClock};

    #[derive(Default)]
    struct Rec {
        rates: Vec<f64>,
        seeks: Vec<f64>,
        targets: Vec<f64>,
        measured: Vec<(f64, f64, f64)>,
        buffer_health_calls: usize,
    }

    fn bound(rec: &Arc<Mutex<Rec>>, generation: u64, part: Option<f64>) -> LowLatencyController {
        let c = LowLatencyController::new(LowLatencyConfiguration::MATCH_WEB_V2_HLSJS);
        let (r1, r2, r3, r4, r5) = (
            rec.clone(),
            rec.clone(),
            rec.clone(),
            rec.clone(),
            rec.clone(),
        );
        let installed = c.install_bindings(LowLatencyBindings {
            owner_generation: generation,
            debug_tag: "v2test".into(),
            on_rate_change: Arc::new(move |v| r1.lock().unwrap().rates.push(v)),
            on_seek_required: Arc::new(move |v| r2.lock().unwrap().seeks.push(v)),
            on_latency_measured: Arc::new(move |a, b, t| {
                r3.lock().unwrap().measured.push((a, b, t))
            }),
            buffer_health_provider: Arc::new(move || {
                r4.lock().unwrap().buffer_health_calls += 1;
                1.0
            }),
            on_target_latency_changed: Arc::new(move |v| r5.lock().unwrap().targets.push(v)),
            segment_duration_provider: part.map(|p| Arc::new(move || p) as F64Provider),
        });
        assert!(installed);
        c.set_latency_backoff_enabled(true);
        c
    }

    #[test]
    fn backoff_publishes_targets_without_rate_or_seek() {
        let rec = Arc::new(Mutex::new(Rec::default()));
        let c = bound(&rec, 1, None);
        assert_eq!(rec.lock().unwrap().targets, vec![2.0]);
        for _ in 0..3 {
            c.pause_for_buffering(true);
        }
        c.process_backoff_only_tick(None);
        let r = rec.lock().unwrap();
        assert_eq!(r.targets, vec![2.0, 2.75, 3.5, 4.25]);
        assert!(r.rates.is_empty());
        assert!(r.seeks.is_empty());
    }

    #[test]
    fn tick_decays_backoff() {
        let rec = Arc::new(Mutex::new(Rec::default()));
        let c = bound(&rec, 1, None);
        for _ in 0..3 {
            c.pause_for_buffering(true);
        }
        assert_eq!(c.current_effective_target_latency(), 4.25);
        let now = SystemClock.now();
        c.process_backoff_only_tick_at(None, now + 10.0);
        assert_eq!(c.current_effective_target_latency(), 4.25);
        c.process_backoff_only_tick_at(None, now + LATENCY_BACKOFF_HOLD + 1.0);
        assert_eq!(c.current_effective_target_latency(), 3.5);
        let r = rec.lock().unwrap();
        assert_eq!(r.targets.last(), Some(&3.5));
        assert!(r.rates.is_empty() && r.seeks.is_empty());
    }

    #[test]
    fn part_duration_floor() {
        let rec = Arc::new(Mutex::new(Rec::default()));
        let c = bound(&rec, 1, Some(1.0));
        assert_eq!(c.current_effective_target_latency(), 2.0);
        let rec2 = Arc::new(Mutex::new(Rec::default()));
        let c2 = bound(&rec2, 1, Some(2.0));
        assert_eq!(c2.current_effective_target_latency(), 3.0);
        assert_eq!(rec2.lock().unwrap().targets, vec![3.0]);
    }

    #[test]
    fn tick_emits_sample_and_buffer_health() {
        let rec = Arc::new(Mutex::new(Rec::default()));
        let c = bound(&rec, 1, None);
        assert_eq!(rec.lock().unwrap().buffer_health_calls, 0);
        c.process_backoff_only_tick(Some(MatchWebV2DisplaySample {
            latency: 3.4,
            target: 3.3,
        }));
        {
            let r = rec.lock().unwrap();
            assert_eq!(r.measured, vec![(3.4, 3.4, 3.3)]);
            assert_eq!(r.buffer_health_calls, 1);
        }
        c.process_backoff_only_tick(None);
        let r = rec.lock().unwrap();
        assert_eq!(r.measured.len(), 1);
        assert_eq!(r.buffer_health_calls, 2);
    }

    fn sample_provider(sample: Option<MatchWebV2DisplaySample>) -> SampleProvider {
        Arc::new(move || Box::pin(async move { sample }))
    }

    #[tokio::test]
    async fn ownership() {
        let rec = Arc::new(Mutex::new(Rec::default()));
        let c = bound(&rec, 5, None);
        c.start_backoff_only_sync(Some(4), sample_provider(None));
        assert!(!c.has_sync_task());
        c.start_backoff_only_sync(Some(5), sample_provider(None));
        assert!(c.has_sync_task());
        assert_eq!(c.sync_state(), SyncState::Synced);
        c.stop_sync(true);
        assert!(!c.has_sync_task());
    }

    #[tokio::test(start_paused = true)]
    async fn backoff_loop_actually_ticks() {
        let rec = Arc::new(Mutex::new(Rec::default()));
        let c = bound(&rec, 3, None);
        c.start_backoff_only_sync(
            Some(3),
            sample_provider(Some(MatchWebV2DisplaySample {
                latency: 3.4,
                target: 3.3,
            })),
        );
        for _ in 0..100 {
            if !rec.lock().unwrap().measured.is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        c.stop_sync(true);
        let r = rec.lock().unwrap();
        assert!(!r.measured.is_empty());
        assert_eq!(r.measured[0], (3.4, 3.4, 3.3));
        assert!(r.buffer_health_calls >= r.measured.len());
        assert!(r.rates.is_empty() && r.seeks.is_empty());
    }
}
