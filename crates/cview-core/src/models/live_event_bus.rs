//! `Sources/CViewCore/Models/LiveEventBus.swift` 포팅 — Live 메뉴 Overview 활동 타임라인용 인메모리 이벤트 버스.
//!
//! in-memory only. 재시작 뒤에는 스냅샷에서 [`LiveEventBus::restore`] 로 직전 활동을 되살린다.

use std::collections::{HashSet, VecDeque};
use std::sync::Mutex;

use chrono::{DateTime, Duration as ChronoDuration, Utc};

/// Live 메뉴에서 트래킹하는 이벤트 종류.
#[derive(Debug, Clone, PartialEq)]
pub enum LiveEvent {
    ChannelStartedLive {
        channel_id: String,
        channel_name: String,
        category: Option<String>,
        viewer_count: i64,
        at: DateTime<Utc>,
    },
    MultiViewerJump {
        percent: f64,
        sessions: i64,
        average: i64,
        at: DateTime<Utc>,
    },
    EngineFallback {
        channel_id: String,
        channel_name: String,
        from: String,
        to: String,
        at: DateTime<Utc>,
    },
    MultipleFollowingsLive {
        count: i64,
        at: DateTime<Utc>,
    },
    NetworkHandoff {
        from: String,
        to: String,
        bitrate_change_percent: f64,
        at: DateTime<Utc>,
    },
}

/// 이벤트 심각도.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LiveEventSeverity {
    Info,
    Success,
    Warning,
    Live,
}

/// Swift `Double.description` 과 같은 표기 — 정수 값도 `.0` 을 붙인다.
fn swift_double(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 1e16 {
        format!("{value:.1}")
    } else {
        format!("{value}")
    }
}

fn epoch_seconds(at: &DateTime<Utc>) -> f64 {
    at.timestamp() as f64 + f64::from(at.timestamp_subsec_nanos()) / 1e9
}

impl LiveEvent {
    /// 식별자 — Swift 와 같은 문자열 (`"live:{cid}:{epoch}"` 등).
    pub fn id(&self) -> String {
        match self {
            LiveEvent::ChannelStartedLive { channel_id, at, .. } => {
                format!("live:{channel_id}:{}", swift_double(epoch_seconds(at)))
            }
            LiveEvent::MultiViewerJump { at, .. } => {
                format!("mvj:{}", swift_double(epoch_seconds(at)))
            }
            LiveEvent::EngineFallback { channel_id, at, .. } => {
                format!("ef:{channel_id}:{}", swift_double(epoch_seconds(at)))
            }
            LiveEvent::MultipleFollowingsLive { at, .. } => {
                format!("mfl:{}", swift_double(epoch_seconds(at)))
            }
            LiveEvent::NetworkHandoff { at, .. } => {
                format!("nh:{}", swift_double(epoch_seconds(at)))
            }
        }
    }

    pub fn timestamp(&self) -> DateTime<Utc> {
        match self {
            LiveEvent::ChannelStartedLive { at, .. }
            | LiveEvent::MultiViewerJump { at, .. }
            | LiveEvent::EngineFallback { at, .. }
            | LiveEvent::MultipleFollowingsLive { at, .. }
            | LiveEvent::NetworkHandoff { at, .. } => *at,
        }
    }

    pub fn severity(&self) -> LiveEventSeverity {
        match self {
            LiveEvent::ChannelStartedLive { .. } => LiveEventSeverity::Live,
            LiveEvent::MultiViewerJump { .. } => LiveEventSeverity::Success,
            LiveEvent::EngineFallback { .. } => LiveEventSeverity::Warning,
            LiveEvent::MultipleFollowingsLive { .. } | LiveEvent::NetworkHandoff { .. } => {
                LiveEventSeverity::Info
            }
        }
    }
}

/// 이벤트 링 버퍼 (thread-safe).
#[derive(Debug, Default)]
pub struct LiveEventBus {
    ring: Mutex<VecDeque<LiveEvent>>,
}

impl LiveEventBus {
    /// ring buffer 용량 — 50건.
    pub const CAPACITY: usize = 50;
    /// 기본 retention — 30분(초).
    pub const DEFAULT_WINDOW: f64 = 30.0 * 60.0;

    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<LiveEvent>> {
        self.ring.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 이벤트 push — 용량 초과 시 가장 오래된 항목 제거.
    pub fn push(&self, event: LiveEvent) {
        let mut ring = self.lock();
        ring.push_back(event);
        while ring.len() > Self::CAPACITY {
            ring.pop_front();
        }
    }

    /// 최근 `window` 초 안의 이벤트 (최신 → 오래된 순서).
    pub fn recent(&self, window: f64) -> Vec<LiveEvent> {
        self.recent_at(Utc::now(), window)
    }

    /// 기준 시각을 지정한 [`recent`](Self::recent).
    pub fn recent_at(&self, now: DateTime<Utc>, window: f64) -> Vec<LiveEvent> {
        let cutoff = now - ChronoDuration::milliseconds((window * 1000.0) as i64);
        let mut events: Vec<LiveEvent> = self
            .lock()
            .iter()
            .filter(|e| e.timestamp() >= cutoff)
            .cloned()
            .collect();
        events.sort_by_key(|e| std::cmp::Reverse(e.timestamp()));
        events
    }

    /// 스냅샷에서 읽은 활동 기록을 링에 되살린다. 링이 **비었을 때만** 교체한다.
    /// 시각 오름차순으로 정렬하고, 같은 id 는 하나만, 용량을 넘으면 최신 것만 남긴다.
    pub fn restore(&self, events: Vec<LiveEvent>) -> bool {
        let mut ring = self.lock();
        if !ring.is_empty() || events.is_empty() {
            return false;
        }
        let mut sorted = events;
        sorted.sort_by_key(|e| e.timestamp());
        let mut seen = HashSet::new();
        let ordered: Vec<LiveEvent> = sorted.into_iter().filter(|e| seen.insert(e.id())).collect();
        let skip = ordered.len().saturating_sub(Self::CAPACITY);
        *ring = ordered.into_iter().skip(skip).collect();
        true
    }

    /// 디버그/테스트용 — 전체 초기화.
    pub fn reset(&self) {
        self.lock().clear();
    }

    pub fn count(&self) -> usize {
        self.lock().len()
    }
}

#[cfg(test)]
mod tests {
    //! `LiveEventBusTests.swift` 계약 포팅.
    use super::*;
    use chrono::TimeZone;

    fn at(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap()
    }

    fn mfl(secs: i64) -> LiveEvent {
        LiveEvent::MultipleFollowingsLive {
            count: 2,
            at: at(secs),
        }
    }

    #[test]
    fn ids_match_swift_format() {
        assert_eq!(mfl(0).id(), "mfl:1700000000.0");
        let e = LiveEvent::ChannelStartedLive {
            channel_id: "c".into(),
            channel_name: "n".into(),
            category: None,
            viewer_count: 1,
            at: Utc.timestamp_opt(1_700_000_000, 500_000_000).unwrap(),
        };
        assert_eq!(e.id(), "live:c:1700000000.5");
        assert_eq!(e.severity(), LiveEventSeverity::Live);
    }

    #[test]
    fn capacity_and_recent_order() {
        let bus = LiveEventBus::new();
        for i in 0..60 {
            bus.push(mfl(i));
        }
        assert_eq!(bus.count(), 50);
        let recent = bus.recent_at(at(60), 30.0);
        assert_eq!(recent.len(), 30);
        assert_eq!(recent[0].timestamp(), at(59));
        bus.reset();
        assert_eq!(bus.count(), 0);
    }

    #[test]
    fn restore_only_into_empty_ring_and_dedups() {
        let bus = LiveEventBus::new();
        assert!(bus.restore(vec![mfl(2), mfl(1), mfl(2)]));
        assert_eq!(bus.count(), 2);
        assert!(
            !bus.restore(vec![mfl(3)]),
            "비어 있지 않으면 교체하지 않는다"
        );
        let bus = LiveEventBus::new();
        assert!(!bus.restore(vec![]));
        assert!(bus.restore((0..70).map(mfl).collect()));
        assert_eq!(bus.count(), 50);
        assert_eq!(
            bus.recent_at(at(100), 1e9)[49].timestamp(),
            at(20),
            "최신 50개만 남는다"
        );
    }
}
