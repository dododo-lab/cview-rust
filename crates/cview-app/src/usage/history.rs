//! 사용률 표본과 기록(시간 기준 링 버퍼) — 순수 자료 구조. 내보내기(CSV·JSON)도 여기서 만든다.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::time::Duration;

use serde::Serialize;

/// 기록이 아무리 길어도 이보다 많이 두지 않는다(2시간 × 1초 = 7,200 + 여유). 표본 하나는 50바이트 남짓이다.
pub const HARD_CAP: usize = 7_300;

/// 표본 하나 — 앱과 도우미 프로세스를 합친 값.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    /// 잰 시각(유닉스 ms).
    pub t: i64,
    /// CPU — 코어 하나를 100 으로 센 합(0 ~ 코어 수 × 100). 화면은 코어 수로 나눠 '전체 대비'로도 보인다.
    pub cpu: f32,
    /// 메모리(바이트) — macOS 는 물리 사용량(활성 상태 보기의 '메모리'), Windows 는 작업 집합.
    pub mem: u64,
    /// 이 앱의 GPU 사용률(%) — 잴 수 없으면 `None`.
    pub gpu: Option<f32>,
    /// GPU 전체 사용률(%) — 다른 앱 포함. 잴 수 없으면 `None`.
    pub gpu_sys: Option<f32>,
    /// GPU 메모리(바이트) — 범위는 상태의 `gpu.memoryScope`(앱·시스템). 잴 수 없으면 `None`.
    pub gpu_mem: Option<u64>,
    /// 네트워크 수신(바이트/초) — 앱이 직접 받은 양(영상·API·채팅·이미지).
    pub net_rx: u64,
    /// 잰 프로세스 수(앱 + 도우미).
    pub procs: u16,
}

/// 최근 `keep` 만큼의 표본. 간격이 바뀌어도 시간으로 자르므로 기간은 그대로다.
#[derive(Debug, Clone)]
pub struct History {
    samples: VecDeque<Sample>,
    keep_ms: i64,
    cap: usize,
}

impl History {
    pub fn new(keep: Duration, interval: Duration) -> Self {
        let mut h = Self {
            samples: VecDeque::new(),
            keep_ms: 0,
            cap: 0,
        };
        h.set_window(keep, interval);
        h
    }

    /// 기간·간격을 바꾼다. 줄어들면 오래된 것부터 버린다.
    pub fn set_window(&mut self, keep: Duration, interval: Duration) {
        self.keep_ms = keep.as_millis().min(i64::MAX as u128) as i64;
        let every = interval.as_millis().max(1);
        self.cap = ((keep.as_millis() / every) as usize + 2).min(HARD_CAP);
        if let Some(last) = self.samples.back().map(|s| s.t) {
            self.trim(last);
        }
        while self.samples.len() > self.cap {
            self.samples.pop_front();
        }
    }

    pub fn push(&mut self, sample: Sample) {
        // 시계가 뒤로 갔으면(시간대·수동 변경) 그 뒤 표본과 순서가 꼬이지 않게 더 새것들을 버린다.
        while self.samples.back().is_some_and(|s| s.t > sample.t) {
            self.samples.pop_back();
        }
        self.samples.push_back(sample);
        self.trim(sample.t);
        while self.samples.len() > self.cap {
            self.samples.pop_front();
        }
    }

    fn trim(&mut self, now: i64) {
        let oldest = now.saturating_sub(self.keep_ms);
        while self.samples.front().is_some_and(|s| s.t < oldest) {
            self.samples.pop_front();
        }
    }

    /// `since`(유닉스 ms) 보다 뒤 표본. `None` 이면 모두.
    pub fn since(&self, since: Option<i64>) -> Vec<Sample> {
        match since {
            None => self.samples.iter().copied().collect(),
            Some(t) => {
                let start = self.samples.partition_point(|s| s.t <= t);
                self.samples.range(start..).copied().collect()
            }
        }
    }

    pub fn latest(&self) -> Option<&Sample> {
        self.samples.back()
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.cap
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn mb(bytes: u64) -> f64 {
    round1(bytes as f64 / 1_048_576.0)
}

/// 표본 → CSV(머리줄 포함, 줄 끝 `\n`). 시각은 현지 시간대 RFC 3339. 잴 수 없던 값은 빈 칸.
/// `cores` 는 CPU 를 '전체 대비 %'(cpu_pct)로 나눌 코어 수.
pub fn to_csv(samples: &[Sample], cores: usize) -> String {
    let cores = cores.max(1) as f64;
    let mut out = String::with_capacity(64 + samples.len() * 96);
    out.push_str("time,unix_ms,cpu_pct,cpu_core_pct,memory_mb,gpu_app_pct,gpu_total_pct,gpu_memory_mb,net_rx_mbps,processes\n");
    let opt = |v: Option<f64>| v.map(|v| format!("{v}")).unwrap_or_default();
    for s in samples {
        let time = chrono::DateTime::from_timestamp_millis(s.t)
            .map(|t| {
                t.with_timezone(&chrono::Local)
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, false)
            })
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "{time},{},{},{},{},{},{},{},{},{}",
            s.t,
            round1(f64::from(s.cpu) / cores),
            round1(f64::from(s.cpu)),
            mb(s.mem),
            opt(s.gpu.map(|v| round1(f64::from(v)))),
            opt(s.gpu_sys.map(|v| round1(f64::from(v)))),
            opt(s.gpu_mem.map(mb)),
            (s.net_rx as f64 * 8.0 / 1_000.0).round() / 1_000.0,
            s.procs,
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: i64, cpu: f32) -> Sample {
        Sample {
            t,
            cpu,
            mem: 512 * 1_048_576,
            gpu: Some(12.5),
            gpu_sys: None,
            gpu_mem: None,
            net_rx: 750_000,
            procs: 4,
        }
    }

    #[test]
    fn keeps_only_window_by_time() {
        let mut h = History::new(Duration::from_secs(10), Duration::from_secs(1));
        for i in 0..30 {
            h.push(s(i * 1000, i as f32));
        }
        // 마지막(29초) 기준 10초 — 19초 표본부터.
        assert_eq!(h.since(None).first().map(|x| x.t), Some(19_000));
        assert_eq!(h.latest().map(|x| x.t), Some(29_000));
        assert!(h.len() <= h.capacity());
    }

    #[test]
    fn capacity_bounds_bursts_and_hard_cap() {
        let mut h = History::new(Duration::from_secs(5), Duration::from_secs(1));
        // 같은 시각에 몰려 들어와도 칸 수를 넘지 않는다.
        for _ in 0..100 {
            h.push(s(1_000, 1.0));
        }
        assert_eq!(h.len(), h.capacity());
        let big = History::new(Duration::from_secs(10 * 3600), Duration::from_secs(1));
        assert_eq!(big.capacity(), HARD_CAP);
    }

    #[test]
    fn shrinking_window_drops_old() {
        let mut h = History::new(Duration::from_secs(60), Duration::from_secs(1));
        for i in 0..60 {
            h.push(s(i * 1000, 0.0));
        }
        h.set_window(Duration::from_secs(5), Duration::from_secs(1));
        assert_eq!(h.since(None).first().map(|x| x.t), Some(54_000));
        assert!(h.len() <= 7);
    }

    #[test]
    fn since_is_exclusive_and_clock_rewind_is_safe() {
        let mut h = History::new(Duration::from_secs(60), Duration::from_secs(1));
        for i in 1..=5 {
            h.push(s(i * 1000, 0.0));
        }
        let after: Vec<i64> = h.since(Some(3_000)).iter().map(|x| x.t).collect();
        assert_eq!(after, vec![4_000, 5_000]);
        assert!(h.since(Some(9_000)).is_empty());
        // 시계가 뒤로 가면 더 새 표본을 버려 순서를 지킨다.
        h.push(s(2_500, 0.0));
        let ts: Vec<i64> = h.since(None).iter().map(|x| x.t).collect();
        assert_eq!(ts, vec![1_000, 2_000, 2_500]);
        h.clear();
        assert!(h.is_empty());
    }

    #[test]
    fn csv_has_header_units_and_blanks() {
        let csv = to_csv(&[s(1_700_000_000_000, 250.0)], 10);
        let mut lines = csv.lines();
        assert_eq!(
            lines.next().unwrap(),
            "time,unix_ms,cpu_pct,cpu_core_pct,memory_mb,gpu_app_pct,gpu_total_pct,gpu_memory_mb,net_rx_mbps,processes"
        );
        let row: Vec<&str> = lines.next().unwrap().split(',').collect();
        assert_eq!(row.len(), 10);
        assert!(row[0].starts_with("2023-11-1"), "현지 RFC 3339: {}", row[0]);
        assert_eq!(row[1], "1700000000000");
        assert_eq!(row[2], "25"); // 250% / 10코어
        assert_eq!(row[3], "250");
        assert_eq!(row[4], "512");
        assert_eq!(row[5], "12.5");
        assert_eq!(row[6], ""); // 잴 수 없음
        assert_eq!(row[7], "");
        assert_eq!(row[8], "6"); // 750,000 B/s = 6 Mbps
        assert_eq!(row[9], "4");
        assert!(csv.ends_with('\n'));
    }

    #[test]
    fn sample_serializes_camel_case_with_nulls() {
        let v = serde_json::to_value(s(5, 1.5)).unwrap();
        assert_eq!(v["gpuSys"], serde_json::Value::Null);
        assert_eq!(v["netRx"], 750_000);
        assert_eq!(v["gpu"], 12.5);
    }
}
