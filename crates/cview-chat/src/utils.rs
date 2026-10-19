//! 채팅 전용 순수 유틸리티.
//!
//! 원본(`Sources/CViewCore/Utilities/`):
//! - `ChatMessageBuffer.swift` → [`ChatMessageBuffer`]
//! - `ChatCommitBackpressure.swift` → [`ChatCommitBackpressure`]
//! - `ChatMessageCapacityPolicy.swift` → [`chat_message_capacity_policy`]
//!
//! `ChatFlushPacing` / `ChatDripPacing` 은 SwiftUI 표시 틱 조정용이라 UI 단계에서 옮긴다.

use std::collections::VecDeque;
use std::sync::Arc;

use cview_core::models::chat::ChatMessageItem;

/// 고정 용량 링 버퍼. 용량을 넘으면 가장 오래된 메시지를 버린다.
///
/// Swift 는 SwiftUI 스냅샷 복사를 싸게 하려고 불변 행 객체(`ChatMessageBufferRow`)를 공유했다.
/// 같은 이유로 원소를 `Arc` 로 보관한다 — 버퍼 복제는 참조 복사뿐이다.
#[derive(Debug, Clone)]
pub struct ChatMessageBuffer {
    items: VecDeque<Arc<ChatMessageItem>>,
    capacity: usize,
    restructure_count: u64,
}

impl Default for ChatMessageBuffer {
    fn default() -> Self {
        Self::new(200)
    }
}

impl ChatMessageBuffer {
    /// 최소 용량.
    pub const MIN_CAPACITY: usize = 10;

    /// 용량 지정 (최소 10).
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(Self::MIN_CAPACITY);
        Self { items: VecDeque::with_capacity(capacity), capacity, restructure_count: 0 }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 뒤에 붙이기·링 축출이 **아닌** 변경(비우기·조건 삭제·전체 교체) 횟수.
    pub fn restructure_count(&self) -> u64 {
        self.restructure_count
    }

    /// 논리 인덱스(0 = 가장 오래된 것).
    pub fn get(&self, index: usize) -> Option<&ChatMessageItem> {
        self.items.get(index).map(|a| a.as_ref())
    }

    pub fn first(&self) -> Option<&ChatMessageItem> {
        self.get(0)
    }

    pub fn last(&self) -> Option<&ChatMessageItem> {
        self.items.back().map(|a| a.as_ref())
    }

    pub fn iter(&self) -> impl Iterator<Item = &ChatMessageItem> {
        self.items.iter().map(|a| a.as_ref())
    }

    /// 공유 행 참조 (Swift `rows`).
    pub fn rows(&self) -> impl Iterator<Item = &Arc<ChatMessageItem>> {
        self.items.iter()
    }

    /// 뒤에서부터 id 를 찾는다.
    pub fn last_index_of_message_id(&self, message_id: &str) -> Option<usize> {
        self.items.iter().rposition(|i| i.id == message_id)
    }

    /// 하나 추가. 가득 차면 가장 오래된 것을 버린다.
    pub fn push(&mut self, item: ChatMessageItem) {
        if self.items.len() == self.capacity {
            self.items.pop_front();
        }
        self.items.push_back(Arc::new(item));
    }

    /// 여러 개 추가. 용량 이상이면 마지막 `capacity` 개만 남는다.
    pub fn extend<I: IntoIterator<Item = ChatMessageItem>>(&mut self, items: I) {
        for item in items {
            self.push(item);
        }
    }

    /// 모두 비운다.
    pub fn clear(&mut self) {
        self.items.clear();
        self.restructure_count = self.restructure_count.wrapping_add(1);
    }

    /// 조건에 맞는 것을 지운다. 지울 것이 없으면 아무것도 바꾸지 않는다.
    pub fn remove_where<F: FnMut(&ChatMessageItem) -> bool>(&mut self, mut predicate: F) {
        let before = self.items.len();
        let surviving: VecDeque<_> = self.items.iter().filter(|i| !predicate(i)).cloned().collect();
        if surviving.len() == before {
            return;
        }
        self.items = surviving;
        self.restructure_count = self.restructure_count.wrapping_add(1);
    }

    /// 전체 교체.
    pub fn replace_all(&mut self, items: Vec<ChatMessageItem>) {
        self.clear();
        self.extend(items);
    }

    /// 모든 원소를 변환한다. 바뀐 것만 새 행으로 교체.
    pub fn map_in_place<F: FnMut(&ChatMessageItem) -> ChatMessageItem>(&mut self, mut transform: F) {
        for slot in self.items.iter_mut() {
            let updated = transform(slot);
            if updated != **slot {
                *slot = Arc::new(updated);
            }
        }
    }

    /// 용량 변경 — 최근 `min(len, new)` 개를 유지한다.
    pub fn resize(&mut self, new_capacity: usize) {
        let new_capacity = new_capacity.max(Self::MIN_CAPACITY);
        if new_capacity == self.capacity {
            return;
        }
        self.capacity = new_capacity;
        while self.items.len() > new_capacity {
            self.items.pop_front();
        }
    }

    /// 오래된 것 → 새것 순서의 배열.
    pub fn to_vec(&self) -> Vec<ChatMessageItem> {
        self.iter().cloned().collect()
    }
}

/// 표시 커밋이 느린데 큐 길이만 보고 더 자주 갱신하는 역방향 피드백을 막는다.
#[derive(Debug, Clone, Copy, Default)]
pub struct ChatCommitBackpressure {
    recent_cost_milliseconds: f64,
    sampled_at: Option<f64>,
}

impl ChatCommitBackpressure {
    pub const MAXIMUM_INTERVAL_NS: u64 = 100_000_000;
    pub const EXPIRATION_SECONDS: f64 = 2.0;

    pub fn new() -> Self {
        Self::default()
    }

    /// 커밋 비용 표본을 기록한다. `now` 는 단조 시계 초.
    pub fn record(&mut self, milliseconds: f64, now: f64) {
        if !milliseconds.is_finite() || milliseconds < 0.0 || !now.is_finite() {
            return;
        }
        let previous = if self.is_fresh(now) { self.recent_cost_milliseconds } else { 0.0 };
        // 느려질 때 즉시 반응하고, 단일 스파이크는 여러 틱에 걸쳐 복원한다.
        self.recent_cost_milliseconds = milliseconds.min(100.0).max(previous * 0.75);
        self.sampled_at = Some(now);
    }

    /// 다음 표시 틱의 최소 간격 (ns).
    pub fn minimum_interval_ns(&self, now: f64) -> u64 {
        if !self.is_fresh(now) || self.recent_cost_milliseconds <= 5.0 {
            return 0;
        }
        let ms = (Self::MAXIMUM_INTERVAL_NS as f64 / 1_000_000.0).min(self.recent_cost_milliseconds * 4.0);
        (ms * 1_000_000.0) as u64
    }

    fn is_fresh(&self, now: f64) -> bool {
        match self.sampled_at {
            Some(at) if now.is_finite() => now >= at && now - at <= Self::EXPIRATION_SECONDS,
            _ => false,
        }
    }
}

/// 설정 화면 "최대 메시지 수" 와 링 버퍼 용량의 단일 계약.
pub mod chat_message_capacity_policy {
    /// 하한.
    pub const MINIMUM: usize = 50;
    /// 상한 (기본값의 두 배).
    pub const MAXIMUM: usize = 2_000;
    /// 기본값.
    pub const DEFAULT: usize = 1_000;

    pub fn range() -> std::ops::RangeInclusive<usize> {
        MINIMUM..=MAXIMUM
    }

    /// 저장·적용에 쓸 수 있는 값으로 제한한다.
    pub fn clamped(value: i64) -> usize {
        value.clamp(MINIMUM as i64, MAXIMUM as i64) as usize
    }

    /// 범위 안내 문구.
    pub fn range_hint() -> String {
        format!("{MINIMUM}~{MAXIMUM}개")
    }
}
