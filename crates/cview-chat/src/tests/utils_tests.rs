//! 채팅 전용 유틸 테스트.

use cview_core::models::chat::{ChatMessage, ChatMessageItem};

use crate::utils::{chat_message_capacity_policy as policy, ChatCommitBackpressure, ChatMessageBuffer};

fn item(id: &str) -> ChatMessageItem {
    ChatMessageItem::from_message(&ChatMessage::with_id(id, "n", format!("c-{id}")), false)
}

fn buffer_ids(b: &ChatMessageBuffer) -> Vec<String> {
    b.iter().map(|i| i.id.clone()).collect()
}

#[test]
fn buffer_minimum_capacity_and_eviction() {
    let mut b = ChatMessageBuffer::new(3);
    assert_eq!(b.capacity(), ChatMessageBuffer::MIN_CAPACITY);
    b.extend((0..15).map(|i| item(&format!("m{i}"))));
    assert_eq!(b.len(), 10);
    assert_eq!(b.first().unwrap().id, "m5");
    assert_eq!(b.last().unwrap().id, "m14");
    assert_eq!(b.restructure_count(), 0, "append/eviction is not a restructure");
}

#[test]
fn buffer_remove_where_and_restructure_count() {
    let mut b = ChatMessageBuffer::new(20);
    b.extend((0..5).map(|i| item(&format!("m{i}"))));
    b.remove_where(|i| i.id == "nope");
    assert_eq!(b.restructure_count(), 0);
    b.remove_where(|i| i.id == "m2");
    assert_eq!(b.restructure_count(), 1);
    assert_eq!(buffer_ids(&b), ["m0", "m1", "m3", "m4"]);
    assert_eq!(b.last_index_of_message_id("m3"), Some(2));
    assert_eq!(b.last_index_of_message_id("m2"), None);
    b.replace_all(vec![item("x")]);
    assert_eq!(buffer_ids(&b), ["x"]);
    assert_eq!(b.restructure_count(), 2);
    b.clear();
    assert!(b.is_empty());
}

#[test]
fn buffer_resize_keeps_most_recent() {
    let mut b = ChatMessageBuffer::new(30);
    b.extend((0..30).map(|i| item(&format!("m{i}"))));
    b.resize(12);
    assert_eq!(b.capacity(), 12);
    assert_eq!(b.len(), 12);
    assert_eq!(b.first().unwrap().id, "m18");
}

#[test]
fn buffer_map_in_place_replaces_only_changed_rows() {
    let mut b = ChatMessageBuffer::new(10);
    b.extend([item("a"), item("b")]);
    let before: Vec<_> = b.rows().cloned().collect();
    b.map_in_place(|i| if i.id == "b" { i.blinded("x") } else { i.clone() });
    let after: Vec<_> = b.rows().cloned().collect();
    assert!(std::sync::Arc::ptr_eq(&before[0], &after[0]));
    assert!(!std::sync::Arc::ptr_eq(&before[1], &after[1]));
    assert!(b.get(1).unwrap().is_blinded);
}

#[test]
fn backpressure() {
    let mut bp = ChatCommitBackpressure::new();
    assert_eq!(bp.minimum_interval_ns(0.0), 0);
    bp.record(4.0, 1.0);
    assert_eq!(bp.minimum_interval_ns(1.0), 0, "cheap commits add no interval");
    bp.record(20.0, 1.1);
    assert_eq!(bp.minimum_interval_ns(1.1), 80_000_000);
    bp.record(500.0, 1.2);
    assert_eq!(bp.minimum_interval_ns(1.2), ChatCommitBackpressure::MAXIMUM_INTERVAL_NS);
    // Spike decays: 100 * 0.75 = 75 -> 300ms capped to 100ms; then stale after 2s.
    bp.record(0.0, 1.3);
    assert_eq!(bp.minimum_interval_ns(1.3), ChatCommitBackpressure::MAXIMUM_INTERVAL_NS);
    assert_eq!(bp.minimum_interval_ns(10.0), 0);
    bp.record(f64::NAN, 11.0);
    bp.record(-1.0, 11.0);
    assert_eq!(bp.minimum_interval_ns(10.0), 0);
}

#[test]
fn capacity_policy() {
    assert_eq!(policy::clamped(-5), policy::MINIMUM);
    assert_eq!(policy::clamped(10_000), policy::MAXIMUM);
    assert_eq!(policy::clamped(700), 700);
    assert!(policy::range().contains(&policy::DEFAULT));
    assert_eq!(policy::range_hint(), "50~2000개");
}
