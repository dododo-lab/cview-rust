//! 앱 자원 사용률 — 앱과 도우미 프로세스(웹뷰·GPU·네트워크)의 CPU·메모리·GPU·네트워크 수신을 주기적으로 재어
//! 시간 기준 링 버퍼([`history::History`])에 남기고, 화면이 보고 있을 때만 `usage://sample` 로 보낸다.
//!
//! - 설정(외관 › `usageMonitor`·`usageIntervalMs`·`usageHistoryMinutes`)이 꺼져 있으면 표본 스레드는 조건 변수에서
//!   잠들어 CPU 를 쓰지 않는다. 표본기(sysinfo·IOKit·PDH)도 처음 켤 때 만든다.
//! - 화면(app-ui/js/usage-monitor.js)이 보이는 동안만 `usage_watch(true)` — 창이 숨으면 이벤트를 보내지 않는다
//!   (기록은 켜져 있으면 계속).
//! - GPU 수집이 느린 PC(인스턴스가 많은 Windows PDH)에서는 걸린 시간을 보고 GPU 만 몇 표본에 한 번 읽는다.

pub mod history;
pub mod procs;

#[cfg(target_os = "macos")]
mod gpu_macos;
// 순수 함수(인스턴스 이름 해석)는 모든 플랫폼에서 테스트한다.
#[cfg_attr(not(windows), allow(dead_code))]
mod gpu_windows;

use std::collections::HashSet;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;

use history::{History, Sample};
use procs::{GroupUsage, ProcessSet};

/// 이벤트 이름 — [`LiveUsage`].
pub const SAMPLE_EVENT: &str = "usage://sample";

/// GPU 한 번 읽은 값.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GpuReading {
    pub app: Option<f32>,
    pub system: Option<f32>,
    pub memory: Option<u64>,
}

/// 측정 설정.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsageConfig {
    pub enabled: bool,
    pub interval: Duration,
    pub keep: Duration,
}

impl UsageConfig {
    pub fn from_appearance(a: &cview_core::models::settings::AppearanceSettings) -> Self {
        use cview_core::models::settings::usage_limits;
        Self {
            enabled: a.usage_monitor,
            interval: Duration::from_millis(u64::from(usage_limits::interval_ms(i64::from(
                a.usage_interval_ms,
            )))),
            keep: Duration::from_secs(
                u64::from(usage_limits::history_minutes(i64::from(
                    a.usage_history_minutes,
                ))) * 60,
            ),
        }
    }
}

impl Default for UsageConfig {
    fn default() -> Self {
        Self::from_appearance(&Default::default())
    }
}

/// GPU 를 어디까지 잴 수 있나(화면의 '지원 안 됨' 표시).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuSupport {
    /// 측정을 켜서 확인했나 — 아직이면 화면은 '확인 중'.
    pub probed: bool,
    pub app: bool,
    pub system: bool,
    /// GPU 메모리 범위 — `app`(앱 프로세스 합) · `system`(시스템 전체) · 없음.
    pub memory_scope: Option<&'static str>,
    pub name: Option<String>,
}

/// 실시간 표본 — 종류별 내역을 함께.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveUsage {
    pub sample: Sample,
    pub groups: Vec<GroupUsage>,
}

/// 화면이 처음 그릴 때 쓰는 상태.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageStatus {
    pub enabled: bool,
    pub interval_ms: u64,
    pub history_minutes: u64,
    /// 논리 코어 수 — CPU 합(코어 하나 = 100)을 전체 대비로 나눌 때.
    pub cores: usize,
    pub platform: &'static str,
    pub gpu: GpuSupport,
    /// 기록에 든 표본 수·최대 칸 수.
    pub samples: usize,
    pub capacity: usize,
    pub latest: Option<LiveUsage>,
}

struct State {
    config: UsageConfig,
    /// 설정이 바뀔 때마다 늘린다 — 잠든 표본 스레드가 다음 표본을 기다리다 깨어 새 간격으로 다시 잡는다.
    generation: u64,
    /// 화면이 실시간 표본을 받고 있나.
    live: bool,
    history: History,
    latest: Option<LiveUsage>,
    gpu: GpuSupport,
    started: bool,
}

pub struct UsageMonitor {
    state: Mutex<State>,
    cv: Condvar,
}

fn lock(m: &Mutex<State>) -> MutexGuard<'_, State> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// 논리 코어 수(0 이면 1).
pub fn cores() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        "other"
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

impl UsageMonitor {
    pub fn new(config: UsageConfig) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                config,
                generation: 0,
                live: false,
                history: History::new(config.keep, config.interval),
                latest: None,
                gpu: GpuSupport::default(),
                started: false,
            }),
            cv: Condvar::new(),
        })
    }

    /// 표본 스레드를 띄운다(한 번만). `emit` 은 화면이 보고 있을 때 표본마다 불린다.
    pub fn start(self: &Arc<Self>, emit: impl Fn(&LiveUsage) + Send + 'static) {
        {
            let mut st = lock(&self.state);
            if st.started {
                return;
            }
            st.started = true;
        }
        let me = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("cview-usage".into())
            .spawn(move || me.run(emit));
        if let Err(e) = spawned {
            tracing::warn!("사용률 표본 스레드를 띄우지 못했습니다: {e}");
            lock(&self.state).started = false;
        }
    }

    /// 설정을 바꾼다(켜기·끄기·간격·기간). 같으면 아무것도 하지 않는다.
    pub fn configure(&self, config: UsageConfig) {
        let mut st = lock(&self.state);
        if st.config == config {
            return;
        }
        st.config = config;
        st.history.set_window(config.keep, config.interval);
        st.generation = st.generation.wrapping_add(1);
        drop(st);
        self.cv.notify_all();
    }

    /// 화면이 실시간 표본을 받을지(보일 때 켜고 숨으면 끈다).
    pub fn set_live(&self, on: bool) {
        lock(&self.state).live = on;
    }

    pub fn status(&self) -> UsageStatus {
        let st = lock(&self.state);
        UsageStatus {
            enabled: st.config.enabled,
            interval_ms: st.config.interval.as_millis() as u64,
            history_minutes: st.config.keep.as_secs() / 60,
            cores: cores(),
            platform: platform(),
            gpu: st.gpu.clone(),
            samples: st.history.len(),
            capacity: st.history.capacity(),
            latest: st.latest.clone(),
        }
    }

    /// `since`(유닉스 ms) 뒤 표본. `None` 이면 기록 전체.
    pub fn history(&self, since: Option<i64>) -> Vec<Sample> {
        lock(&self.state).history.since(since)
    }

    pub fn clear(&self) {
        let mut st = lock(&self.state);
        st.history.clear();
        st.latest = None;
    }

    /// 표본 하나를 기록한다(표본 스레드, 테스트). 화면이 보고 있으면 `true`.
    fn record(&self, live: LiveUsage) -> bool {
        let mut st = lock(&self.state);
        if !st.config.enabled {
            return false; // 재는 사이에 꺼졌다
        }
        st.history.push(live.sample);
        st.latest = Some(live);
        st.live
    }

    fn run(self: Arc<Self>, emit: impl Fn(&LiveUsage)) {
        let mut sampler: Option<Sampler> = None;
        let mut warm = false;
        loop {
            // 꺼져 있으면 여기서 잠든다 — 켜질 때까지 CPU 를 쓰지 않는다.
            let (interval, generation) = {
                let mut st = lock(&self.state);
                while !st.config.enabled {
                    warm = false;
                    st = self.cv.wait(st).unwrap_or_else(|e| e.into_inner());
                }
                (st.config.interval, st.generation)
            };
            let tick = Instant::now();
            let s = sampler.get_or_insert_with(Sampler::new);
            if !warm {
                // 켜진 직후 — 기준(CPU 시간·GPU 누계·수신 바이트)만 잡고 버린다. 꺼져 있던 동안의 평균이 튀지 않게.
                s.reset();
                let _ = s.sample();
                lock(&self.state).gpu = s.support();
                warm = true;
            } else {
                let live = s.sample();
                {
                    let mut st = lock(&self.state);
                    st.gpu = s.support();
                }
                if self.record(live.clone()) {
                    emit(&live);
                }
            }
            // 다음 표본까지 — 설정이 바뀌면(간격·끄기) 바로 깨어 다시 잡는다.
            let deadline = tick + interval;
            let mut st = lock(&self.state);
            loop {
                if st.generation != generation || !st.config.enabled {
                    break;
                }
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                st = self
                    .cv
                    .wait_timeout(st, deadline - now)
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
            }
        }
    }
}

/// 한 스레드가 쥐는 표본기 — 프로세스 묶음, GPU 탐침, 수신 바이트 기준.
struct Sampler {
    procs: ProcessSet,
    #[cfg(target_os = "macos")]
    gpu: Option<gpu_macos::GpuProbe>,
    #[cfg(windows)]
    gpu: Option<gpu_windows::GpuProbe>,
    last_rx: u64,
    last_at: Instant,
    /// GPU 를 몇 표본에 한 번 읽나(수집이 느리면 늘린다).
    gpu_stride: u32,
    tick: u32,
    last_gpu: GpuReading,
}

/// GPU 수집 한 번이 이보다 오래 걸리면 간격을 늘린다.
const GPU_SLOW: Duration = Duration::from_millis(6);

impl Sampler {
    fn new() -> Self {
        Self {
            procs: ProcessSet::new(),
            #[cfg(target_os = "macos")]
            gpu: gpu_macos::GpuProbe::new(),
            #[cfg(windows)]
            gpu: gpu_windows::GpuProbe::new(),
            last_rx: cview_core::utils::traffic::received_bytes(),
            last_at: Instant::now(),
            gpu_stride: 1,
            tick: 0,
            last_gpu: GpuReading::default(),
        }
    }

    fn reset(&mut self) {
        self.procs.invalidate();
        self.last_rx = cview_core::utils::traffic::received_bytes();
        self.last_at = Instant::now();
        self.tick = 0;
    }

    fn support(&self) -> GpuSupport {
        #[cfg(target_os = "macos")]
        if let Some(g) = &self.gpu {
            let system = g.system_supported();
            return GpuSupport {
                probed: true,
                app: g.app_supported().unwrap_or(false),
                system,
                memory_scope: system.then_some("system"),
                name: g.name.clone(),
            };
        }
        #[cfg(windows)]
        if let Some(g) = &self.gpu {
            return GpuSupport {
                probed: true,
                app: true,
                system: true,
                memory_scope: g.memory_supported().then_some("app"),
                name: g.name.clone(),
            };
        }
        GpuSupport {
            probed: true,
            ..GpuSupport::default()
        }
    }

    fn read_gpu(&mut self, pids: &HashSet<u32>, rediscovered: bool) -> GpuReading {
        #[cfg(target_os = "macos")]
        if let Some(g) = self.gpu.as_mut() {
            if rediscovered {
                g.rescan(pids);
            }
            return g.sample();
        }
        #[cfg(windows)]
        if let Some(g) = self.gpu.as_mut() {
            let _ = rediscovered;
            return g.sample(pids);
        }
        let _ = (pids, rediscovered);
        GpuReading::default()
    }

    fn sample(&mut self) -> LiveUsage {
        let (reading, rediscovered) = self.procs.refresh();
        let pids: HashSet<u32> = reading.pids.iter().copied().collect();
        // GPU — 느린 PC 에서는 몇 표본에 한 번. 건너뛴 표본은 지난 값을 쓴다(묶음을 다시 찾은 표본은 꼭 읽는다).
        if rediscovered || self.tick.is_multiple_of(self.gpu_stride) {
            let t = Instant::now();
            self.last_gpu = self.read_gpu(&pids, rediscovered);
            let took = t.elapsed();
            self.gpu_stride = if took > GPU_SLOW * 3 {
                5
            } else if took > GPU_SLOW {
                2
            } else {
                1
            };
        }
        self.tick = self.tick.wrapping_add(1);
        let rx = cview_core::utils::traffic::received_bytes();
        let now = Instant::now();
        let secs = now.duration_since(self.last_at).as_secs_f64().max(0.001);
        let net_rx = (rx.saturating_sub(self.last_rx) as f64 / secs).round() as u64;
        self.last_rx = rx;
        self.last_at = now;
        LiveUsage {
            sample: Sample {
                t: now_ms(),
                cpu: (reading.cpu * 10.0).round() / 10.0,
                mem: reading.mem,
                gpu: self.last_gpu.app.map(|v| (v * 10.0).round() / 10.0),
                gpu_sys: self.last_gpu.system.map(|v| (v * 10.0).round() / 10.0),
                gpu_mem: self.last_gpu.memory,
                net_rx,
                procs: reading.pids.len().min(u16::MAX as usize) as u16,
            },
            groups: reading.groups,
        }
    }
}

/// 내보내기 JSON — 장치·단위 설명과 표본 전체.
pub fn export_json(status: &UsageStatus, samples: &[Sample]) -> String {
    let v = serde_json::json!({
        "app": "CView",
        "version": env!("CARGO_PKG_VERSION"),
        "platform": status.platform,
        "arch": std::env::consts::ARCH,
        "exportedAt": chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        "intervalMs": status.interval_ms,
        "cores": status.cores,
        "gpu": status.gpu,
        "units": {
            "t": "unix ms",
            "cpu": "percent of one core, summed over app processes (0..cores*100)",
            "mem": "bytes",
            "gpu": "percent (this app)",
            "gpuSys": "percent (whole GPU)",
            "gpuMem": "bytes (scope: gpu.memoryScope)",
            "netRx": "bytes per second received by the app",
            "procs": "processes measured",
        },
        "samples": samples,
    });
    serde_json::to_string_pretty(&v).unwrap_or_else(|_| "{}".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(enabled: bool, interval_ms: u64, keep_s: u64) -> UsageConfig {
        UsageConfig {
            enabled,
            interval: Duration::from_millis(interval_ms),
            keep: Duration::from_secs(keep_s),
        }
    }

    fn live(t: i64) -> LiveUsage {
        LiveUsage {
            sample: Sample {
                t,
                cpu: 1.0,
                mem: 1,
                gpu: None,
                gpu_sys: None,
                gpu_mem: None,
                net_rx: 0,
                procs: 1,
            },
            groups: vec![],
        }
    }

    #[test]
    fn config_from_settings_clamps() {
        let mut a = cview_core::models::settings::AppearanceSettings {
            usage_interval_ms: 10,
            usage_history_minutes: 1_000,
            ..Default::default()
        };
        let c = UsageConfig::from_appearance(&a);
        assert!(c.enabled);
        assert_eq!(c.interval, Duration::from_secs(1));
        assert_eq!(c.keep, Duration::from_secs(120 * 60));
        a.usage_monitor = false;
        assert!(!UsageConfig::from_appearance(&a).enabled);
    }

    #[test]
    fn record_respects_enabled_and_live() {
        let m = UsageMonitor::new(cfg(true, 1000, 60));
        assert!(!m.record(live(1_000)), "화면이 안 보면 보내지 않는다");
        m.set_live(true);
        assert!(m.record(live(2_000)));
        assert_eq!(m.history(None).len(), 2);
        assert_eq!(m.history(Some(1_000)).len(), 1);
        assert_eq!(m.status().latest.map(|l| l.sample.t), Some(2_000));
        m.configure(cfg(false, 1000, 60));
        assert!(!m.record(live(3_000)), "꺼진 뒤 들어온 표본은 버린다");
        // 끄면 기록은 남는다(내보낼 수 있게) — 지우기는 따로.
        assert_eq!(m.history(None).len(), 2);
        m.clear();
        assert!(m.history(None).is_empty());
        assert!(m.status().latest.is_none());
    }

    #[test]
    fn configure_shrinks_window_and_reports_status() {
        let m = UsageMonitor::new(cfg(true, 1000, 600));
        for i in 0..100 {
            m.record(live(i * 1000));
        }
        m.configure(cfg(true, 2000, 60));
        let st = m.status();
        assert_eq!(st.interval_ms, 2000);
        assert_eq!(st.history_minutes, 1);
        assert!(st.samples <= st.capacity && st.samples <= 61);
        assert!(st.cores >= 1);
        assert!(!st.gpu.probed, "측정 전엔 GPU 지원을 모른다");
    }

    #[test]
    fn thread_samples_when_enabled_and_sleeps_when_disabled() {
        let m = UsageMonitor::new(cfg(true, 1000, 60));
        m.set_live(true);
        let (tx, rx) = std::sync::mpsc::channel();
        m.start(move |l| {
            let _ = tx.send(l.sample.t);
        });
        // 첫 표본은 기준만 잡고 버리므로 두 번째(약 1초 뒤)가 온다.
        let t = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("켜져 있으면 표본이 온다");
        assert!(t > 0);
        let st = m.status();
        assert!(st.gpu.probed);
        assert!(st.latest.as_ref().is_some_and(|l| l.sample.procs >= 1));
        assert!(st.latest.as_ref().is_some_and(|l| l.sample.mem > 0));
        // 끄면 더 오지 않는다.
        m.configure(cfg(false, 1000, 60));
        while rx.try_recv().is_ok() {}
        assert!(rx.recv_timeout(Duration::from_millis(1_600)).is_err());
    }

    /// 표본 한 번의 비용 — `cargo test -p cview-app usage::tests::sample_cost -- --ignored --nocapture`.
    #[test]
    #[ignore = "측정용 — 기기마다 다르다"]
    fn sample_cost() {
        let mut s = Sampler::new();
        s.reset();
        let _ = s.sample();
        let n = 30;
        let cpu0 = cpu_time();
        let t = Instant::now();
        for _ in 0..n {
            std::thread::sleep(Duration::from_millis(20));
            let _ = s.sample();
        }
        let wall = t.elapsed().saturating_sub(Duration::from_millis(20 * n));
        println!(
            "표본 1회 평균: 벽시계 {:.2} ms, CPU {:.2} ms (GPU 간격 {})",
            wall.as_secs_f64() * 1000.0 / n as f64,
            (cpu_time() - cpu0) * 1000.0 / n as f64,
            s.gpu_stride
        );
        // 5초마다 도는 재탐색(전체 PID 훑기·GPU 클라이언트 다시 찾기)까지 매번 하면.
        let cpu1 = cpu_time();
        for _ in 0..10 {
            s.procs.invalidate();
            let _ = s.sample();
        }
        println!(
            "재탐색 포함 1회: CPU {:.2} ms",
            (cpu_time() - cpu1) * 1000.0 / 10.0
        );
        println!("{:?}", s.sample());
    }

    fn cpu_time() -> f64 {
        let mut sys = sysinfo::System::new();
        let me = sysinfo::Pid::from_u32(std::process::id());
        sys.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::Some(&[me]),
            false,
            sysinfo::ProcessRefreshKind::nothing().with_cpu(),
        );
        sys.process(me)
            .map(|p| p.accumulated_cpu_time() as f64 / 1000.0)
            .unwrap_or(0.0)
    }

    #[test]
    fn export_json_describes_units() {
        let m = UsageMonitor::new(cfg(true, 1000, 60));
        m.record(live(5));
        let json: serde_json::Value =
            serde_json::from_str(&export_json(&m.status(), &m.history(None))).unwrap();
        assert_eq!(json["app"], "CView");
        assert_eq!(json["samples"][0]["t"], 5);
        assert!(json["units"]["netRx"].is_string());
    }
}
