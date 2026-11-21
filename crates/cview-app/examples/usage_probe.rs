//! 실행 중인 CView 의 사용률을 밖에서 잰다 — 앱 안 사용률 측정(설정 › 성능)과 같은 표본기(`cview_app::usage::Probe`).
//!
//! ```sh
//! cargo run -p cview-app --example usage_probe -- <pid> [초=30] [간격ms=1000] [이름]
//! ```
//! 표본마다 한 줄(시각·CPU·GPU·메모리), 끝에 종류별(앱 본체·웹 화면·GPU 처리·네트워크·기타) 평균·최대를
//! JSON 한 줄(`SUMMARY {...}`)로 낸다. CPU 는 코어 하나 = 100. 네트워크 수신은 앱 안 계수기라 밖에서는 잴 수 없다.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use cview_app::usage::procs::ProcKind;
use cview_app::usage::Probe;

#[derive(Default)]
struct Acc {
    n: u32,
    sum: f64,
    max: f64,
    mem: f64,
}

impl Acc {
    fn add(&mut self, cpu: f64, mem: f64) {
        self.n += 1;
        self.sum += cpu;
        self.max = self.max.max(cpu);
        self.mem += mem;
    }

    fn json(&self) -> serde_json::Value {
        let n = f64::from(self.n.max(1));
        serde_json::json!({
            "cpu_avg": (self.sum / n * 10.0).round() / 10.0,
            "cpu_max": (self.max * 10.0).round() / 10.0,
            "mem_mb": (self.mem / n / 1_048_576.0).round(),
        })
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(pid) = args.first().and_then(|a| a.parse::<u32>().ok()) else {
        eprintln!("사용법: usage_probe <pid> [초=30] [간격ms=1000] [이름]");
        std::process::exit(2);
    };
    let secs: u64 = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(30);
    let every = Duration::from_millis(args.get(2).and_then(|a| a.parse().ok()).unwrap_or(1000));
    let label = args.get(3).cloned().unwrap_or_default();

    let mut probe = Probe::new(pid, every);
    let started = Instant::now();
    let mut total = Acc::default();
    let mut groups: BTreeMap<ProcKind, Acc> = BTreeMap::new();
    let (mut gpu_sum, mut gpu_max, mut gpu_n, mut sys_sum) = (0.0f64, 0.0f64, 0u32, 0.0f64);
    let mut next = Instant::now() + every;
    while started.elapsed() < Duration::from_secs(secs) {
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
        next += every;
        let live = probe.sample();
        let s = live.sample;
        total.add(f64::from(s.cpu), s.mem as f64);
        for g in &live.groups {
            groups
                .entry(g.kind)
                .or_default()
                .add(f64::from(g.cpu), g.mem as f64);
        }
        if let Some(g) = s.gpu {
            gpu_sum += f64::from(g);
            gpu_max = gpu_max.max(f64::from(g));
            gpu_n += 1;
        }
        sys_sum += f64::from(s.gpu_sys.unwrap_or(0.0));
        println!(
            "{:>5.1}s cpu {:>6.1}  gpu {:>5}  mem {:>6.0}MB  procs {}",
            started.elapsed().as_secs_f64(),
            s.cpu,
            s.gpu
                .map(|g| format!("{g:.1}"))
                .unwrap_or_else(|| "-".into()),
            s.mem as f64 / 1_048_576.0,
            s.procs
        );
    }
    let n = f64::from(total.n.max(1));
    let by_kind: serde_json::Map<String, serde_json::Value> = groups
        .iter()
        .map(|(k, a)| {
            (
                serde_json::to_value(k)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_default(),
                a.json(),
            )
        })
        .collect();
    let summary = serde_json::json!({
        "label": label,
        "pid": pid,
        "samples": total.n,
        "seconds": secs,
        "total": total.json(),
        "groups": by_kind,
        "gpu_app_avg": (gpu_sum / f64::from(gpu_n.max(1)) * 10.0).round() / 10.0,
        "gpu_app_max": (gpu_max * 10.0).round() / 10.0,
        "gpu_sys_avg": (sys_sum / n * 10.0).round() / 10.0,
        "gpu": probe.gpu_support(),
    });
    println!("SUMMARY {summary}");
}
