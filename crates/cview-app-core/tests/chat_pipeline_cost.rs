//! 채팅 뷰모델 메시지 처리 비용(수동 측정). 실제 할당 바이트를 세려고 전역 할당자를 감싼다 — 이 테스트 바이너리에만 적용된다.
//! cargo test -p cview-app-core --release --test chat_pipeline_cost -- --ignored --nocapture

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Instant;

use cview_app_core::viewmodels::chat::*;
use cview_chat::ChatEngineEvent;
use cview_core::models::chat::{ChatMessage, EmojiMap};

struct Counting;
static LIVE: AtomicIsize = AtomicIsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE.fetch_add(l.size() as isize, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size() as isize, Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static A: Counting = Counting;

fn bench_batch(start: usize, n: usize) -> Vec<ChatMessage> {
    (start..start + n)
        .map(|i| {
            let mut emojis = EmojiMap::new();
            if i % 7 == 0 {
                emojis.insert(format!("e{}", i % 40), format!("https://x/{}.png", i % 40));
            }
            let mut m = ChatMessage::with_id(
                format!("m{i}"),
                format!("닉{}", i % 500),
                format!("합성 메시지 {i} 가나다라마바사 ㅋㅋㅋㅋ"),
            )
            .user_id(format!("u{}", i % 3000));
            if !emojis.is_empty() {
                m = m.extras(cview_core::models::chat::ChatExtras {
                    emojis: Some(emojis),
                    ..Default::default()
                });
            }
            m
        })
        .collect()
}

#[tokio::test(flavor = "current_thread")]
#[ignore]
async fn chat_pipeline_cost() {
    run(true);
    run(false);
}

fn run(track_messages: bool) {
    let before = LIVE.load(Ordering::Relaxed);
    let vm = ChatViewModel::with_deps(ChatDeps {
        track_messages,
        ..Default::default()
    });
    vm.lock().last_connection_config = Some(ConnectionConfig {
        chat_channel_id: "c".into(),
        ..Default::default()
    });
    let total = 50_000;
    // 합성 메시지를 만드는 시간은 따로 재서 뺀다.
    let t = Instant::now();
    for b in 0..total / 10 {
        std::hint::black_box(bench_batch(b * 10, 10));
    }
    let gen = t.elapsed();
    let t = Instant::now();
    for b in 0..total / 10 {
        let mut s = vm.lock();
        s.handle_event(ChatEngineEvent::NewMessages(bench_batch(b * 10, 10)));
        if b % 3 == 0 {
            s.flush_pending_messages();
        }
    }
    let el = t.elapsed().saturating_sub(gen);
    let s = vm.lock();
    println!(
        "chat_pipeline_cost(track_messages={track_messages}): {total} msgs {:.1} ms ({:.2} µs/msg) retained {:.0} KiB: messages={} history={} pending={}",
        el.as_secs_f64() * 1000.0,
        el.as_secs_f64() * 1e6 / total as f64,
        (LIVE.load(Ordering::Relaxed) - before) as f64 / 1024.0,
        s.messages.len(),
        s.chat_history.len(),
        s.pending_count()
    );
    drop(s);
    drop(vm);
}
