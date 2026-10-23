//! ChatViewModel 테스트.
//!
//! 원본: `Tests/CViewAppTests/ChatViewModelMutationTests.swift`, `ChatViewModelEchoReclaimTests.swift`,
//! `ChatBacklogCommitTests.swift`, `ChatViewModelUnreadTests.swift`, `ChatViewModelPenaltyTests.swift`,
//! `ChatViewModelKickRestrictionTests.swift`, `ChatNoticeRemovalTests.swift`,
//! `ChatMentionStreamerGateTests.swift`, `MergedChatBudgetCoordinatorTests.swift`(조정자 부분),
//! `ChatSameChannelReconnectTests.swift`(가짜 엔진).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};

use cview_app_core::viewmodels::chat::connection::ConnectRequest;
use cview_app_core::viewmodels::chat::interaction::MessageSendOutcome;
use cview_app_core::viewmodels::chat::*;
use cview_chat::{ChatEngineConfiguration, ChatEngineEvent};
use cview_core::logic::mention_detector::MentionMatch;
use cview_core::models::chat::{
    ChatMessage, ChatMessageItem, ChatSystemNoticeKind, EmojiMap, MessageType, UserRole,
};

fn item_at(id: &str, user: &str, content: &str, ts: DateTime<Utc>) -> ChatMessageItem {
    ChatMessageItem::new(
        id,
        user,
        format!("닉{user}"),
        content,
        ts,
        MessageType::Normal,
        EmojiMap::new(),
        false,
        false,
    )
}

fn item(id: &str, user: &str, content: &str) -> ChatMessageItem {
    item_at(id, user, content, Utc::now())
}

fn epoch(secs: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(secs, 0).unwrap()
}

fn backlog(n: usize, prefix: &str) -> Vec<ChatMessageItem> {
    (0..n)
        .map(|i| item_at(&format!("{prefix}{i}"), "u", &format!("past-{i}"), epoch(0)))
        .collect()
}

fn ids(s: &ChatState) -> Vec<String> {
    s.messages.iter().map(|m| m.id.clone()).collect()
}

const PLACEHOLDER: &str = "관리자에 의해 삭제된 메시지입니다.";

// MARK: - 변형

#[test]
fn blind_across_all_buffers() {
    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    let target = item("m1", "u1", "원문 텍스트");
    s.messages.push(target.clone());
    s.chat_history = vec![target.clone()];
    s.pending_messages = vec![
        item("dead", "u0", "x"),
        target.clone(),
        item("m2", "u2", "x"),
    ];
    s.pending_head = 1;
    s.apply_mutation(ChatMessageMutation::blind("m1"));
    let m = s.messages.iter().find(|m| m.id == "m1").unwrap();
    assert!(m.is_blinded);
    assert_eq!(m.content, PLACEHOLDER);
    assert_eq!(s.chat_history[0].content, PLACEHOLDER);
    assert!(s.pending_messages[1].is_blinded);
    assert!(!s.pending_messages[2].is_blinded);
}

#[test]
fn purge_user_keeps_head_and_dead_prefix() {
    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    s.messages
        .extend([item("a", "bad", "x"), item("b", "ok", "x")]);
    s.chat_history = vec![item("a", "bad", "x"), item("b", "ok", "x")];
    s.pending_messages = vec![
        item("dead", "u0", "x"),
        item("c", "bad", "x"),
        item("d", "ok", "x"),
        item("e", "bad", "x"),
    ];
    s.pending_head = 1;
    s.apply_mutation(ChatMessageMutation::PurgeUser {
        user_id: "bad".into(),
    });
    assert!(!s.messages.iter().any(|m| m.user_id == "bad"));
    assert_eq!(
        s.chat_history
            .iter()
            .map(|m| m.id.as_str())
            .collect::<Vec<_>>(),
        ["b"]
    );
    assert_eq!(s.pending_head, 1);
    assert_eq!(
        s.pending_messages[1..]
            .iter()
            .map(|m| m.id.as_str())
            .collect::<Vec<_>>(),
        ["d"]
    );
    assert_eq!(s.pending_messages[0].id, "dead");
}

#[test]
fn blind_carries_server_reason_and_revision_only_on_mutation() {
    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    let target = item("m1", "u1", "원문");
    s.messages.push(target.clone());
    s.replay_hold_queue = vec![target];
    let before = s.moderation_revision;
    s.mutate_messages(|m| m.push(item("m2", "u2", "x")));
    assert_eq!(s.moderation_revision, before);
    let clean = "클린봇이 부적절한 표현을 감지했습니다.";
    s.apply_mutation(ChatMessageMutation::Blind {
        message_id: "m1".into(),
        text: Some(clean.into()),
    });
    assert_eq!(s.messages.first().unwrap().content, clean);
    assert_eq!(s.replay_hold_queue[0].content, clean);
    assert_eq!(s.moderation_revision, before + 1);
    s.apply_mutation(ChatMessageMutation::blind("m1"));
    assert_eq!(
        s.messages.first().unwrap().content,
        clean,
        "이미 가려진 행은 다시 바꾸지 않는다"
    );
}

// MARK: - 에코 회수

fn seed_echo(s: &mut ChatState, tid: i64, content: &str) -> String {
    let id = format!("me_{tid}");
    let it = item(&id, "me", content);
    let key = format!("me_{content}");
    s.append_to_history(vec![it.clone()]);
    s.messages.push(it);
    s.recent_sent_echo_keys
        .insert(key.clone(), Utc::now() + chrono::Duration::seconds(5));
    s.remember_pending_echo(tid, &id, &key);
    id
}

#[tokio::test]
async fn rejection_reclaims_only_the_named_echo() {
    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    s.current_user_uid = Some("me".into());
    s.handle_event(ChatEngineEvent::Connected);
    let first = seed_echo(&mut s, 3, "첫 번째");
    let second = seed_echo(&mut s, 4, "두 번째");
    s.handle_event(ChatEngineEvent::SendRejected {
        tid: Some(4),
        ret_code: -1,
        message: Some("차단된 사용자입니다.".into()),
    });
    assert!(s.messages.iter().any(|m| m.id == first));
    assert!(!s.messages.iter().any(|m| m.id == second));
    assert!(!s.chat_history.iter().any(|m| m.id == second));
    assert_eq!(s.error_message.as_deref(), Some("차단된 사용자입니다."));
    assert!(s.messages.last().unwrap().is_system);
    assert!(!s.recent_sent_echo_keys.contains_key("me_두 번째"));
    assert!(!s.pending_sent_echoes.contains_key(&4));
}

#[tokio::test]
async fn nil_or_unknown_tid_removes_nothing() {
    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    s.current_user_uid = Some("me".into());
    let id = seed_echo(&mut s, 3, "안녕");
    s.handle_event(ChatEngineEvent::SendRejected {
        tid: None,
        ret_code: -1,
        message: None,
    });
    s.handle_event(ChatEngineEvent::SendRejected {
        tid: Some(99),
        ret_code: -1,
        message: Some("실패".into()),
    });
    assert!(s.messages.iter().any(|m| m.id == id));
    assert!(s.error_message.as_deref().unwrap().contains("실패"));
}

#[test]
fn reclaim_covers_pending_and_hold_and_closes_history_flag() {
    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    let id = seed_echo(&mut s, 3, "안녕");
    let it = s.messages.iter().find(|m| m.id == id).unwrap().clone();
    s.pending_messages.push(it.clone());
    s.replay_hold_queue.push(it);
    assert!(s.has_chat_history);
    s.handle_event(ChatEngineEvent::SendRejected {
        tid: Some(3),
        ret_code: -1,
        message: None,
    });
    assert!(!s.pending_messages.iter().any(|m| m.id == id));
    assert!(!s.replay_hold_queue.iter().any(|m| m.id == id));
    assert!(s.chat_history.is_empty());
    assert!(!s.has_chat_history);
}

#[test]
fn early_rejection_then_register_reclaims_immediately() {
    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    s.handle_event(ChatEngineEvent::SendRejected {
        tid: Some(7),
        ret_code: -1,
        message: None,
    });
    s.messages.push(item("me_7", "me", "x"));
    assert!(s.remember_pending_echo(7, "me_7", "me_x"));
    assert!(!s.messages.iter().any(|m| m.id == "me_7"));
}

#[test]
fn echo_table_is_bounded_and_has_expiry() {
    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    let before = Utc::now();
    for tid in 0..(ChatState::PENDING_SENT_ECHO_CAP as i64 * 3) {
        s.remember_pending_echo(tid, &format!("m{tid}"), &format!("k{tid}"));
    }
    assert!(s.pending_sent_echoes.len() <= ChatState::PENDING_SENT_ECHO_CAP);
    let e = s.pending_sent_echoes.values().next().unwrap();
    assert!(e.expiry > before);
    assert!(
        e.expiry <= before + chrono::Duration::seconds(ChatState::PENDING_SENT_ECHO_TTL_SECS + 1)
    );
}

// MARK: - backlog

#[test]
fn backlog_commits_at_once_live_drips() {
    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    s.commit_backlog_messages(backlog(50, "old"));
    assert_eq!(s.messages.len(), 50);
    assert_eq!(s.pending_count(), 0);
    assert!(s.batch_flush_task.is_none());

    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    s.enqueue_batched_messages(backlog(50, "live"));
    assert!(s.messages.len() < 50);
    assert!(s.pending_count() > 0);
}

#[tokio::test]
async fn dropped_pending_returns_with_reconnect_backlog() {
    let vm = ChatViewModel::new();
    {
        let mut s = vm.lock();
        s.begin_backlog_hold();
        s.enqueue_batched_messages(vec![
            item_at("q1", "u", "a", epoch(0)),
            item_at("q2", "u", "b", epoch(0)),
        ]);
        assert_eq!(s.pending_count(), 2);
    }
    vm.disconnect().await;
    let mut s = vm.lock();
    assert_eq!(s.pending_count(), 0);
    s.commit_backlog_messages(vec![
        item_at("q1", "u", "a", epoch(0)),
        item_at("q2", "u", "b", epoch(0)),
    ]);
    assert_eq!(ids(&s), ["q1", "q2"]);
}

#[test]
fn internal_reconnect_backlog_follows_older_queue() {
    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    s.begin_backlog_hold();
    s.enqueue_batched_messages(vec![
        item_at("before-1", "u", "a", epoch(10)),
        item_at("before-2", "u", "b", epoch(11)),
    ]);
    s.commit_backlog_messages(vec![
        item_at("during-1", "u", "c", epoch(20)),
        item_at("during-2", "u", "d", epoch(21)),
    ]);
    assert_eq!(ids(&s), ["before-1", "before-2", "during-1", "during-2"]);
}

#[test]
fn backlog_precedes_queued_live_and_dedupes() {
    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    s.begin_backlog_hold();
    s.enqueue_batched_messages(vec![item("live-1", "u", "new")]);
    assert_eq!(s.messages.len(), 0);
    s.commit_backlog_messages(backlog(3, "old"));
    let contents: Vec<_> = s.messages.iter().map(|m| m.content.clone()).collect();
    assert_eq!(contents, ["past-0", "past-1", "past-2", "new"]);
    assert_eq!(s.pending_count(), 0);

    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    s.enqueue_batched_messages(vec![item("dup-1", "u", "same")]);
    s.commit_backlog_messages(vec![item("dup-1", "u", "same"), item("old-9", "u", "past")]);
    assert_eq!(ids(&s).iter().filter(|i| *i == "dup-1").count(), 1);
    assert!(ids(&s).contains(&"old-9".to_string()));
}

fn config(chat: &str, stream: &str) -> ConnectionConfig {
    ConnectionConfig {
        chat_channel_id: chat.into(),
        access_token: "t".into(),
        uid: Some("me".into()),
        channel_id: Some(stream.into()),
        channel_name: Some("테스트채널".into()),
        ..Default::default()
    }
}

#[test]
fn backlog_does_not_raise_mentions_unread_or_badge() {
    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    s.last_connection_config = Some(config("chat-a", "stream-a"));
    s.local_user_nickname = Some("나".into());
    s.commit_backlog_messages(vec![item("m1", "other", "@나 안녕하세요")]);
    assert!(s.mention_matches.is_empty());
    s.enqueue_batched_messages(vec![item("m2", "other", "@나 다시 안녕")]);
    s.flush_pending_messages();
    assert!(!s.mention_matches.is_empty());

    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    s.enter_replay_mode();
    s.commit_backlog_messages(backlog(10, "o"));
    assert_eq!(s.unread_count, 0);

    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    s.set_background_mode(true);
    s.commit_backlog_messages(backlog(7, "o"));
    assert_eq!(s.unread_while_background, 0);
    assert_eq!(s.message_count, 7);
}

#[tokio::test]
async fn hold_releases_on_timeout() {
    let vm = ChatViewModel::new();
    {
        let mut s = vm.lock();
        s.begin_backlog_hold();
        s.enqueue_batched_messages(vec![item("live-1", "u", "new")]);
        assert_eq!(s.messages.len(), 0);
    }
    tokio::time::sleep(ChatState::BACKLOG_HOLD_TIMEOUT + Duration::from_millis(250)).await;
    let s = vm.lock();
    assert!(!s.is_holding_for_backlog);
    assert_eq!(s.messages.len(), 1);
}

#[test]
fn empty_backlog_releases_hold() {
    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    s.begin_backlog_hold();
    s.enqueue_batched_messages(vec![item("live-1", "u", "new")]);
    s.commit_backlog_messages(vec![]);
    assert!(!s.is_holding_for_backlog);
    assert_eq!(s.messages.len(), 1);
}

#[test]
fn live_during_hold_keeps_mention_semantics() {
    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    s.last_connection_config = Some(config("chat-test", "stream-test"));
    s.local_user_nickname = Some("나".into());
    s.begin_backlog_hold();
    s.enqueue_batched_messages(vec![item("fresh-mention", "other", "@나 새 메시지")]);
    s.commit_backlog_messages(vec![item_at(
        "old-mention",
        "other",
        "@나 과거 메시지",
        epoch(0),
    )]);
    assert_eq!(ids(&s), ["old-mention", "fresh-mention"]);
    assert!(s.mention_matches.contains_key("fresh-mention"));
    assert!(!s.mention_matches.contains_key("old-mention"));
}

#[test]
fn reading_freezes_list_and_rejoins_on_exit() {
    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    s.commit_backlog_messages(backlog(5, "old"));
    let snapshot = ids(&s);
    s.enter_replay_mode();
    for n in 0..10 {
        s.enqueue_batched_messages(vec![item(&format!("live-{n}"), "u", "hi")]);
        s.flush_pending_messages();
        if let Some(t) = s.batch_flush_task.take() {
            t.cancel();
        }
    }
    assert_eq!(ids(&s), snapshot);
    assert_eq!(s.replay_hold_queue.len(), 10);
    assert_eq!(s.unread_count, 10);
    assert_eq!(s.chat_history.len(), 15);
    s.apply_mutation(ChatMessageMutation::blind("live-3"));
    s.exit_replay_mode();
    assert_eq!(s.messages.len(), 15);
    assert!(
        s.messages
            .iter()
            .find(|m| m.id == "live-3")
            .unwrap()
            .is_blinded
    );
    assert!(s.replay_hold_queue.is_empty());
    assert_eq!(s.unread_count, 0);
}

// MARK: - 미읽음·배경

fn commit_all(s: &mut ChatState, items: Vec<ChatMessageItem>) {
    s.pending_messages = items;
    s.pending_head = 0;
    let mut guard = 0;
    while s.pending_count() > 0 && guard < 1000 {
        s.flush_pending_messages();
        guard += 1;
    }
}

#[test]
fn background_unread_counts_and_resets() {
    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    s.set_background_mode(true);
    commit_all(
        &mut s,
        vec![
            item("a", "ua", "hi"),
            item("b", "ub", "hi"),
            item("c", "uc", "hi"),
        ],
    );
    assert_eq!(s.unread_while_background, 3);
    assert_eq!(s.message_count, 3);
    s.set_background_mode(false);
    assert_eq!(s.unread_while_background, 0);

    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    commit_all(&mut s, vec![item("a", "ua", "hi"), item("b", "ub", "hi")]);
    assert_eq!(s.unread_while_background, 0);
    assert_eq!(s.message_count, 2);
}

#[test]
fn foreground_lease_overrides_background_request() {
    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    s.set_background_mode(true);
    assert!(s.is_background_mode());
    s.acquire_foreground_presentation_lease();
    assert!(!s.is_background_mode());
    s.release_foreground_presentation_lease();
    assert!(s.is_background_mode());
}

// MARK: - 제재

#[tokio::test]
async fn kick_blocks_send_but_keeps_connection_state() {
    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    s.current_user_uid = Some("me".into());
    s.handle_event(ChatEngineEvent::Connected);
    assert!(s.can_send_chat());
    s.handle_event(ChatEngineEvent::Kicked);
    assert!(s.connection_state.is_connected());
    assert!(s.is_chat_restricted());
    assert!(!s.can_send_chat());
    assert!(s
        .messages
        .iter()
        .any(|m| m.is_system && m.content.contains("추방")));
    s.handle_event(ChatEngineEvent::Connected);
    assert!(s.can_send_chat());
}

#[tokio::test]
async fn penalty_messages_and_auto_release() {
    let vm = ChatViewModel::new();
    {
        let mut s = vm.lock();
        s.current_user_uid = Some("me".into());
        s.handle_event(ChatEngineEvent::Connected);
        s.handle_event(ChatEngineEvent::UserPenalized {
            user_id: "me".into(),
            duration: 0,
        });
        assert_eq!(s.chat_restriction, None, "기간 미상은 안내만");
        s.handle_event(ChatEngineEvent::UserPenalized {
            user_id: "me".into(),
            duration: 120,
        });
        assert!(s
            .messages
            .iter()
            .any(|m| m.is_system && m.content.contains("2분")));
        s.handle_event(ChatEngineEvent::UserPenalized {
            user_id: "me".into(),
            duration: 30,
        });
        assert!(s
            .messages
            .iter()
            .any(|m| m.is_system && m.content.contains("30초")));
        let before = s.messages.len();
        s.handle_event(ChatEngineEvent::UserPenalized {
            user_id: "other".into(),
            duration: 60,
        });
        assert_eq!(s.messages.len(), before);
        s.handle_event(ChatEngineEvent::UserPenalized {
            user_id: "me".into(),
            duration: 1,
        });
        assert_eq!(s.chat_restriction, Some(ChatRestrictionReason::Penalized));
    }
    tokio::time::sleep(Duration::from_millis(1300)).await;
    assert_eq!(vm.lock().chat_restriction, None);
}

#[tokio::test]
async fn penalty_timer_does_not_release_later_kick() {
    let vm = ChatViewModel::new();
    {
        let mut s = vm.lock();
        s.current_user_uid = Some("me".into());
        s.handle_event(ChatEngineEvent::Connected);
        s.handle_event(ChatEngineEvent::UserPenalized {
            user_id: "me".into(),
            duration: 1,
        });
        s.handle_event(ChatEngineEvent::Kicked);
    }
    tokio::time::sleep(Duration::from_millis(1300)).await;
    assert_eq!(
        vm.lock().chat_restriction,
        Some(ChatRestrictionReason::Kicked)
    );
}

#[tokio::test]
async fn welcome_shown_once() {
    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    for _ in 0..5 {
        s.handle_event(ChatEngineEvent::Connected);
    }
    s.handle_event(ChatEngineEvent::Disconnected { reason: "x".into() });
    s.handle_event(ChatEngineEvent::Connected);
    let n = s
        .messages
        .iter()
        .filter(|m| m.is_system && m.content.contains("환영합니다"))
        .count();
    assert_eq!(n, 1);
    assert_ne!(
        ChatState::welcome_message().id,
        ChatState::welcome_message().id
    );
}

// MARK: - 공지

#[test]
fn notice_removal_clears_banner_and_only_current_cache() {
    let store = memory_pinned_notice_cache();
    let deps = ChatDeps {
        pinned_notices: store.clone(),
        ..Default::default()
    };
    let vm = ChatViewModel::with_deps(deps);
    let mut s = vm.lock();
    s.last_connection_config = Some(ConnectionConfig {
        chat_channel_id: "chan".into(),
        ..Default::default()
    });
    let notice =
        ChatMessage::with_id("test-notice", "test", "pinned").message_type(MessageType::Notice);
    s.handle_event(ChatEngineEvent::Notice(notice.clone()));
    store.save(&ChatMessageItem::from_message(&notice, true), "other");
    assert!(s.streamer_notice.is_some());
    assert!(store.load("chan").is_some());
    s.handle_event(ChatEngineEvent::NoticeCleared);
    assert!(s.streamer_notice.is_none());
    assert!(store.load("chan").is_none());
    assert!(store.load("other").is_some());
    // 닫은 공지는 다시 띄우지 않는다.
    s.handle_event(ChatEngineEvent::Notice(notice));
    s.dismiss_streamer_notice();
    assert!(store.is_dismissed("test-notice"));
}

// MARK: - 멘션

fn mention_vm(sink: Arc<RecordingMentionSink>) -> ChatViewModel {
    let vm = ChatViewModel::with_deps(ChatDeps {
        mention_sink: sink,
        ..Default::default()
    });
    {
        let mut s = vm.lock();
        s.local_user_nickname = Some("도도".into());
        s.last_connection_config = Some(ConnectionConfig {
            chat_channel_id: "ws-ch".into(),
            channel_id: Some("ch".into()),
            channel_name: Some("테스트채널".into()),
            ..Default::default()
        });
    }
    vm
}

fn role_item(id: &str, content: &str, role: UserRole) -> ChatMessageItem {
    let mut i = item(id, &format!("u-{id}"), content);
    i.user_role = role;
    i
}

#[test]
fn streamer_only_gate() {
    let sink = Arc::new(RecordingMentionSink::default());
    let vm = mention_vm(sink.clone());
    let mut s = vm.lock();
    s.detect_mention(&role_item("s1", "오늘 방송 시작합니다", UserRole::Streamer));
    s.detect_mention(&role_item("m1", "공지 확인해주세요", UserRole::Manager));
    assert!(sink.entries.lock().unwrap().is_empty());
    assert_eq!(
        s.mention_matches["s1"],
        HashSet::from([MentionMatch::Streamer {}])
    );
    s.detect_mention(&role_item("s2", "도도님 감사합니다", UserRole::Streamer));
    s.detect_mention(&role_item("v1", "도도 안녕", UserRole::Viewer));
    s.detect_mention(&role_item("v2", "ㅋㅋㅋ", UserRole::Viewer));
    let got: Vec<String> = sink
        .entries
        .lock()
        .unwrap()
        .iter()
        .map(|(e, _)| e.id.clone())
        .collect();
    assert_eq!(got, ["s2", "v1"]);
    assert!(s.mention_matches["s2"].contains(&MentionMatch::Nickname {}));
    assert!(!s.mention_matches.contains_key("v2"));
}

// MARK: - 자동완성·이모티콘

#[test]
fn autocomplete_mentions_and_apply() {
    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    s.track_recent_chatters(&[
        item("1", "a", "x"),
        item("2", "b", "x"),
        item("3", "a", "x"),
    ]);
    assert_eq!(
        s.recent_chatters
            .iter()
            .map(|c| c.user_id.as_str())
            .collect::<Vec<_>>(),
        ["b", "a"]
    );
    s.update_autocomplete_suggestions("hi @닉", None);
    assert_eq!(s.mention_suggestions.len(), 2);
    s.move_autocomplete_selection(1);
    assert_eq!(s.autocomplete_selected_index, 1);
    s.move_autocomplete_selection(1);
    assert_eq!(s.autocomplete_selected_index, 0);
    let out = s.apply_autocompletion("hi @닉", 0).unwrap();
    assert_eq!(out, "hi @닉b ");
    assert!(!s.is_autocomplete_active());
}

#[test]
fn emoticon_autocomplete_index_rebuilds_lazily() {
    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    s.set_channel_emoticons(HashMap::from([(
        "smile_1".to_string(),
        "https://e/x.png".to_string(),
    )]));
    s.update_autocomplete_suggestions(":smi", None);
    assert_eq!(s.emoticon_suggestions.len(), 1);
    let builds = s.emoticon_autocomplete_search_index_rebuild_count;
    s.update_autocomplete_suggestions(":smil", None);
    assert_eq!(s.emoticon_autocomplete_search_index_rebuild_count, builds);
    assert_eq!(s.apply_autocompletion(":smil", 0).unwrap(), "{:smile_1:}");
}

#[test]
fn outgoing_emojis_only_used() {
    let lookup = HashMap::from([
        ("d_54".to_string(), "https://e/d_54.png".to_string()),
        ("d_55".to_string(), "https://e/d_55.png".to_string()),
    ]);
    let used = ChatState::resolve_outgoing_emojis("hi {:d_54:} {:zz:}", &lookup);
    assert_eq!(used.len(), 1);
    assert!(used.contains_key("d_54"));
    assert!(ChatState::resolve_outgoing_emojis("plain", &lookup).is_empty());
}

// MARK: - 통계·알림

#[test]
fn incremental_stats_and_stream_alert_merge() {
    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    let mut d1 = item("d1", "u1", "고마워요");
    d1.r#type = MessageType::Donation;
    d1.donation_amount = Some(1000);
    let mut d2 = item("d2", "u1", "");
    d2.r#type = MessageType::Donation;
    d2.donation_amount = Some(500);
    s.commit_backlog_messages(vec![d1.clone(), d2.clone()]);
    assert_eq!(s.donation_count, 2);
    assert_eq!(s.total_donation_amount, 1500);
    assert_eq!(s.unique_user_count, 1);
    use cview_app_core::viewmodels::chat::processing::stream_alert_from_item;
    s.enqueue_stream_alert(stream_alert_from_item(&d1).unwrap());
    s.enqueue_stream_alert(stream_alert_from_item(&d2).unwrap());
    assert_eq!(s.stream_alerts.len(), 1);
    assert_eq!(s.stream_alerts[0].donation_amount, Some(1500));
    assert_eq!(s.stream_alerts[0].content, "고마워요");
}

#[test]
fn drop_notice_is_rate_limited() {
    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    s.pending_drop_notice_backlog = 1234;
    let now = Utc::now();
    let n = s.take_drop_notice_if_due(now).unwrap();
    assert!(n.content.contains("1,234"));
    assert_eq!(n.system_notice_kind, ChatSystemNoticeKind::Warning);
    s.pending_drop_notice_backlog = 1;
    assert!(s
        .take_drop_notice_if_due(now + chrono::Duration::seconds(1))
        .is_none());
    assert!(s
        .take_drop_notice_if_due(now + chrono::Duration::seconds(6))
        .is_some());
}

#[test]
fn settings_roundtrip() {
    let vm = ChatViewModel::new();
    let mut s = vm.lock();
    let settings = ChatSettings {
        font_size: 17.0,
        max_update_rate: 45,
        show_donations_only: true,
        max_visible_messages: 300,
        ..Default::default()
    };
    s.apply_settings(&settings);
    assert_eq!(s.messages.capacity(), 300);
    assert_eq!(s.max_chat_update_rate, 45);
    let out = s.export_settings(&ChatSettings::default());
    assert_eq!(out.font_size, 17.0);
    assert!(out.show_donations_only);
    assert_eq!(out.max_update_rate, 45);
}

// MARK: - 통합 예산

fn load_queue(vm: &ChatViewModel, n: usize) {
    let mut s = vm.lock();
    s.pending_messages = (0..n)
        .map(|i| item_at(&format!("m{i}"), "u", "hi", epoch(0)))
        .collect();
    s.pending_head = 0;
}

#[test]
fn hot_session_gets_larger_share_and_recompute_is_throttled() {
    let coordinator = MergedChatBudgetCoordinator::new();
    let vms: Vec<ChatViewModel> = (0..8).map(|_| ChatViewModel::new()).collect();
    coordinator.register(&vms);
    load_queue(&vms[0], 600);
    coordinator.recompute(Instant::now());
    assert_eq!(vms[0].lock().merged_row_budget, 8);
    assert!(vms[1..].iter().all(|v| v.lock().merged_row_budget == 1));

    let vms: Vec<ChatViewModel> = (0..4).map(|_| ChatViewModel::new()).collect();
    coordinator.register(&vms);
    let t0 = Instant::now();
    coordinator.recompute(t0);
    let baseline: Vec<i64> = vms.iter().map(|v| v.lock().merged_row_budget).collect();
    load_queue(&vms[0], 900);
    coordinator.refresh_if_needed(t0 + Duration::from_millis(50));
    assert_eq!(
        vms.iter()
            .map(|v| v.lock().merged_row_budget)
            .collect::<Vec<_>>(),
        baseline
    );
    coordinator.refresh_if_needed(
        t0 + MergedChatBudgetCoordinator::RECOMPUTE_INTERVAL + Duration::from_millis(1),
    );
    assert!(vms[0].lock().merged_row_budget > baseline[0]);

    let single = vec![ChatViewModel::new()];
    coordinator.register(&single);
    assert_eq!(single[0].lock().merged_row_budget, 0);
}

// MARK: - 가짜 엔진으로 연결·전송

#[derive(Default)]
struct FakeEngine {
    sent: Mutex<Vec<String>>,
    next_tid: Mutex<i64>,
    events: Mutex<Vec<ChatEngineEvent>>,
    notify: tokio::sync::Notify,
    disconnected: Mutex<bool>,
}

#[async_trait]
impl ChatEngineHandle for FakeEngine {
    async fn connect(&self) {}
    async fn disconnect(&self) {
        *self.disconnected.lock().unwrap() = true;
    }
    async fn send_message(&self, text: &str, _emojis: &EmojiMap) -> Result<i64, String> {
        self.sent.lock().unwrap().push(text.into());
        let mut t = self.next_tid.lock().unwrap();
        *t += 1;
        Ok(*t)
    }
    fn set_background_mode(&self, _enabled: bool) {}
    fn clear_messages(&self) {}
    fn messages(&self) -> Vec<ChatMessage> {
        vec![]
    }
    async fn next_event(&self) -> Option<ChatEngineEvent> {
        loop {
            let notified = self.notify.notified();
            if let Some(e) = {
                let mut q = self.events.lock().unwrap();
                (!q.is_empty()).then(|| q.remove(0))
            } {
                return Some(e);
            }
            notified.await;
        }
    }
}

fn fake_deps(engines: Arc<Mutex<Vec<Arc<FakeEngine>>>>) -> ChatDeps {
    ChatDeps {
        engine_factory: Arc::new(move |_c: ChatEngineConfiguration| {
            let e = Arc::new(FakeEngine::default());
            engines.lock().unwrap().push(e.clone());
            e as Arc<dyn ChatEngineHandle>
        }),
        ..Default::default()
    }
}

#[tokio::test]
async fn same_channel_reconnect_keeps_history_other_channel_clears() {
    let engines = Arc::new(Mutex::new(vec![]));
    let vm = ChatViewModel::with_deps(fake_deps(engines.clone()));
    vm.connect(ConnectRequest::new("chat-a", "t")).await;
    vm.lock().commit_backlog_messages(backlog(5, "a"));
    vm.reconnect().await;
    assert_eq!(engines.lock().unwrap().len(), 2);
    assert!(*engines.lock().unwrap()[0].disconnected.lock().unwrap());
    assert_eq!(
        vm.lock().messages.len(),
        5,
        "같은 채널 재연결은 버퍼를 지우지 않는다"
    );
    vm.connect(ConnectRequest::new("chat-b", "t")).await;
    assert_eq!(vm.lock().messages.len(), 0);
    assert_eq!(vm.lock().message_count, 0);
}

#[tokio::test]
async fn late_finishing_older_connect_request_does_not_override_newer_channel() {
    let engines = Arc::new(Mutex::new(vec![]));
    let vm = ChatViewModel::with_deps(fake_deps(engines.clone()));
    // A·B 순으로 요청했는데(토큰 받는 중) B 가 먼저 끝나 연결하고, A 가 늦게 끝났다.
    let a = vm.begin_connect_request();
    let b = vm.begin_connect_request();
    assert!(!vm.is_current_connect_request(a));
    vm.connect_if_current(b, ConnectRequest::new("chat-b", "t"))
        .await;
    vm.connect_if_current(a, ConnectRequest::new("chat-a", "t"))
        .await;
    assert_eq!(
        engines.lock().unwrap().len(),
        1,
        "늦은 A 는 엔진을 만들지 않는다"
    );
    assert_eq!(
        vm.lock().active_chat_channel_id().as_deref(),
        Some("chat-b")
    );

    // 요청 뒤에 끊었으면(다른 패인에 채팅 정보 없음) 늦게 온 연결도 하지 않는다.
    let c = vm.begin_connect_request();
    vm.disconnect().await;
    vm.connect_if_current(c, ConnectRequest::new("chat-c", "t"))
        .await;
    assert_eq!(engines.lock().unwrap().len(), 1);
    assert!(*engines.lock().unwrap()[0].disconnected.lock().unwrap());
}

/// 셸은 여러 요청의 순서를 한 잠금 안에서 정하려고 해제의 동기 부분(`detach`)만 먼저 한다 — `await` 전에
/// 엔진이 떨어지고 세대가 올라, 그 사이 끝나는 이전 연결 요청은 물러나야 한다.
#[tokio::test]
async fn detach_takes_effect_before_awaiting_engine_close() {
    let engines = Arc::new(Mutex::new(vec![]));
    let vm = ChatViewModel::with_deps(fake_deps(engines.clone()));
    vm.connect(ConnectRequest::new("chat-a", "t")).await;
    let pending = vm.begin_connect_request(); // B 로 바꾸는 중(토큰 받는 중)
    let close = vm.detach(); // 그 뒤 패인에 채팅이 없어 끊었다
    assert!(
        vm.lock().chat_engine.is_none(),
        "엔진은 기다리기 전에 떨어진다"
    );
    assert!(!vm.is_current_connect_request(pending));
    assert!(
        !*engines.lock().unwrap()[0].disconnected.lock().unwrap(),
        "소켓 닫기는 돌려준 future 가 한다"
    );
    close.await;
    assert!(*engines.lock().unwrap()[0].disconnected.lock().unwrap());
    vm.connect_if_current(pending, ConnectRequest::new("chat-b", "t"))
        .await;
    assert_eq!(
        engines.lock().unwrap().len(),
        1,
        "끊은 뒤 늦게 끝난 연결은 엔진을 만들지 않는다"
    );
    assert!(vm.same_as(&vm.clone()));
    assert!(!vm.same_as(&ChatViewModel::with_deps(fake_deps(engines))));
}

#[tokio::test]
async fn send_appends_local_echo_and_suppresses_server_echo() {
    let engines = Arc::new(Mutex::new(vec![]));
    let vm = ChatViewModel::with_deps(fake_deps(engines.clone()));
    vm.connect(ConnectRequest::new("chat-a", "t")).await;
    {
        let mut s = vm.lock();
        s.current_user_uid = Some("me".into());
        s.handle_event(ChatEngineEvent::Connected);
        s.end_backlog_hold();
        s.input_text = "  안녕하세요 ".into();
    }
    vm.send_input().await;
    {
        let s = vm.lock();
        assert_eq!(s.input_text, "");
        assert!(s
            .messages
            .iter()
            .any(|m| m.user_id == "me" && m.content == "안녕하세요"));
        assert!(s.pending_sent_echoes.contains_key(&1));
    }
    assert_eq!(
        engines.lock().unwrap()[0].sent.lock().unwrap().as_slice(),
        ["안녕하세요"]
    );
    let before = vm.lock().messages.len();
    vm.lock()
        .enqueue_batched_messages(vec![item("srv-1", "me", "안녕하세요")]);
    assert_eq!(
        vm.lock().messages.len(),
        before,
        "서버 에코는 한 번 억제된다"
    );
}

#[tokio::test]
async fn send_without_login_fails_and_commands_are_local() {
    let engines = Arc::new(Mutex::new(vec![]));
    let vm = ChatViewModel::with_deps(fake_deps(engines));
    vm.connect(ConnectRequest::new("chat-a", "t")).await;
    assert_eq!(vm.send_message("hi").await, MessageSendOutcome::Failed);
    assert!(vm
        .lock()
        .error_message
        .as_deref()
        .unwrap()
        .contains("로그인"));
    vm.lock()
        .commit_backlog_messages(vec![item("x1", "bad", "hello")]);
    assert_eq!(
        vm.send_message("/block bad").await,
        MessageSendOutcome::HandledLocally
    );
    let s = vm.lock();
    assert!(!s.messages.iter().any(|m| m.user_id == "bad"));
    assert!(s.blocked_users().contains("bad"));
}

#[tokio::test]
async fn event_loop_delivers_engine_events() {
    let engines = Arc::new(Mutex::new(vec![]));
    let vm = ChatViewModel::with_deps(fake_deps(engines.clone()));
    let mut rx = vm.subscribe();
    vm.connect(ConnectRequest::new("chat-a", "t")).await;
    let engine = engines.lock().unwrap()[0].clone();
    engine
        .events
        .lock()
        .unwrap()
        .push(ChatEngineEvent::Connected);
    engine.notify.notify_one();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !vm.lock().connection_state.is_connected() && Instant::now() < deadline {
        let _ = tokio::time::timeout(Duration::from_millis(50), rx.changed()).await;
    }
    let snap = vm.snapshot();
    assert!(snap.is_connected);
    assert!(serde_json::to_value(&snap).unwrap()["messages"].is_array());
}
