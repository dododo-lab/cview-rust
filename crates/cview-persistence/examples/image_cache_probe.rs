//! 썸네일 디스크 캐시(`ImageCache`) 시간 재기(성능 측정용, 앱에는 들어가지 않는다).
//!
//! `cargo run --release -p cview-persistence --example image_cache_probe -- [채워 둘 항목 수]`
//!
//! 임시 폴더에 항목을 채운 뒤, 한 화면 분량(60장)을 여러 번 다시 읽는 적중과 새 항목 넣기를 잰다.
//! 출력 줄 모양은 `perf_probe` 와 같다(`scripts/perf-gate.py` 가 읽는다).
use cview_persistence::image_cache::{ImageCache, ImageKind, ImageMeta, DEFAULT_CAP_BYTES};
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
    let entries: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(5_000);
    let dir = std::env::temp_dir().join(format!("cview-image-cache-probe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let cache = ImageCache::open(&dir, DEFAULT_CAP_BYTES).expect("open");
    let meta = ImageMeta {
        content_type: "image/jpeg".into(),
        etag: None,
        last_modified: None,
    };
    let blob = vec![0x5au8; 24 * 1024];
    let url = |i: usize| format!("https://livecloud-thumb.akamaized.net/probe/{i}/image_480.jpg");
    let base_ms: i64 = 1_700_000_000_000;
    for i in 0..entries {
        cache
            .put(&url(i), ImageKind::Live, &meta, &blob, base_ms + i as i64)
            .expect("put");
    }
    println!("entries {entries}");
    // 같은 화면 60장을 다시 읽는다 — 시각은 실제처럼 조금씩만 흐른다.
    let mut clock = base_ms + entries as i64;
    time("get 60 hits (same screen)", 30, || {
        for i in 0..60 {
            clock += 5;
            std::hint::black_box(cache.get(&url(i), clock).expect("get"));
        }
    });
    let mut next = entries;
    time("put 20 new", 15, || {
        for _ in 0..20 {
            next += 1;
            clock += 5;
            cache
                .put(&url(next), ImageKind::Live, &meta, &blob, clock)
                .expect("put");
        }
    });
    let _ = std::fs::remove_dir_all(&dir);
}
