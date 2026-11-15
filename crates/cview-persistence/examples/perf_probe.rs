//! 저장소 조회 시간 재기(성능 측정용, 앱에는 들어가지 않는다).
//!
//! `cargo run --release -p cview-persistence --example perf_probe -- <스토어 사본 경로> [덧붙일 합성 시청 기록 수]`
//!
//! 합성 기록을 그 파일에 실제로 넣으므로 사용 중인 스토어가 아니라 사본(`sqlite3 … ".backup 사본"`)에 돌린다.
use cview_persistence::models::watch_history::WatchHistory;
use cview_persistence::watch_report::ReportPeriod;
use cview_persistence::DataStore;
use std::time::Instant;

fn time<T>(name: &str, n: u32, mut f: impl FnMut() -> T) {
    let _ = f();
    let mut v = Vec::new();
    for _ in 0..n {
        let s = Instant::now();
        std::hint::black_box(f());
        v.push(s.elapsed().as_secs_f64() * 1000.0);
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "{name:32} median {:8.3}ms  min {:8.3}ms",
        v[v.len() / 2],
        v[0]
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = &args[1];
    let synth: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let store = DataStore::open(path).expect("open");
    if synth > 0 {
        let now = chrono::Utc::now();
        let rows: Vec<WatchHistory> = (0..synth)
            .map(|i| {
                let start = now - chrono::Duration::minutes((i as i64) * 97);
                let mut r = WatchHistory::new(
                    format!("ch{}", i % 120),
                    format!("채널{}", i % 120),
                    Some("https://x/t.jpg".into()),
                    Some(format!("카테고리{}", i % 30)),
                    start,
                );
                r.ended_at = Some(start + chrono::Duration::minutes(40 + (i as i64 % 90)));
                r.duration = ((40 + i % 90) * 60) as f64;
                r
            })
            .collect();
        store.insert_watch_history_rows(&rows).expect("insert");
    }
    println!("rows {}", store.table_row_count("WatchHistory").unwrap());
    let now = chrono::Utc::now();
    time("aggregate_insights", 15, || {
        store.aggregate_insights(now).unwrap()
    });
    time("recent_watch_history_rows(60)", 15, || {
        store.recent_watch_history_rows(60).unwrap()
    });
    time("all_watch_history_rows", 15, || {
        store.all_watch_history_rows().unwrap()
    });
    for p in ["today", "7d", "30d", "all"] {
        time(&format!("watch_report {p}"), 15, || {
            store
                .watch_report(ReportPeriod::parse(p).unwrap(), now)
                .unwrap()
        });
    }
    time("fetch_favorite_items", 30, || {
        store.fetch_favorite_items().unwrap()
    });
    time("fetch_recent_items(20)", 30, || {
        store.fetch_recent_items(20).unwrap()
    });
    time("fetch_recent_vod_playback(40)", 30, || {
        store.fetch_recent_vod_playback(40).unwrap()
    });
    time("watch_history_fingerprint", 30, || {
        store.watch_history_fingerprint().unwrap()
    });
    time("table_row_count", 30, || {
        store.table_row_count("StreamBookmark").unwrap()
    });
}
