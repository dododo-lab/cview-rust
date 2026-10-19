//! `ChatModerationServiceTests.swift` 포팅.

use std::time::Duration;

use cview_core::models::chat::{new_uuid_string, ChatMessage, MessageType};

use crate::moderation::{
    ChatCommand, ChatCommandResult, ChatFilter, ChatModerationService, FilterType,
};

fn make_message(user_id: Option<&str>, content: &str, ty: MessageType) -> ChatMessage {
    let mut m = ChatMessage::with_id(new_uuid_string(), user_id.unwrap_or("anon"), content)
        .message_type(ty);
    if let Some(u) = user_id {
        m = m.user_id(u);
    }
    m
}

fn msg(user_id: Option<&str>, content: &str) -> ChatMessage {
    make_message(user_id, content, MessageType::Normal)
}

fn kw(words: &[&str]) -> FilterType {
    FilterType::Keyword(words.iter().map(|s| s.to_string()).collect())
}

// MARK: - Blocked Users

#[test]
fn blocked_user_is_filtered() {
    let svc = ChatModerationService::new();
    svc.block_user("baduser");
    let msgs = [
        msg(Some("good"), "hello"),
        msg(Some("baduser"), "spam"),
        msg(Some("good2"), "world"),
    ];
    let result = svc.filter_messages(&msgs);
    assert_eq!(result.len(), 2);
    assert!(result
        .iter()
        .all(|m| m.user_id.as_deref() != Some("baduser")));
}

#[test]
fn initial_blocked_users_applied() {
    let svc = ChatModerationService::with_blocked_users(["pre-blocked".to_string()]);
    let result = svc.filter_messages(&[
        msg(Some("pre-blocked"), "should be blocked"),
        msg(Some("ok"), "ok msg"),
    ]);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].user_id.as_deref(), Some("ok"));
}

#[test]
fn unblock_restores_messages() {
    let svc = ChatModerationService::new();
    svc.block_user("u1");
    assert!(svc.filter_messages(&[msg(Some("u1"), "x")]).is_empty());
    svc.unblock_user("u1");
    assert_eq!(svc.filter_messages(&[msg(Some("u1"), "x")]).len(), 1);
}

// MARK: - Muted Users

#[test]
fn muted_user_blocked() {
    let svc = ChatModerationService::new();
    svc.mute_user("u1", 60.0);
    assert!(svc
        .filter_messages(&[msg(Some("u1"), "muted msg")])
        .is_empty());
    assert!(svc.is_muted("u1"));
}

#[test]
fn mute_immediately_expired() {
    let svc = ChatModerationService::new();
    svc.mute_user("u1", -1.0);
    assert_eq!(
        svc.filter_messages(&[msg(Some("u1"), "passed")]).len(),
        1,
        "음수 duration → 만료 처리되어 메시지 통과"
    );
    assert!(!svc.is_muted("u1"));
}

#[test]
fn clean_expired_mutes_prunes() {
    let svc = ChatModerationService::new();
    svc.mute_user("expired", -10.0);
    svc.mute_user("active", 60.0);
    svc.clean_expired_mutes();
    assert!(!svc.is_muted("expired"));
    assert!(svc.is_muted("active"));
    assert_eq!(svc.muted_entry_count(), 1);
}

fn local_action(svc: &ChatModerationService, input: &str) -> String {
    match svc.process_command(input) {
        Some(ChatCommandResult::LocalAction(text)) => text,
        other => panic!("{input}: localAction 이 아님 — {other:?}"),
    }
}

#[test]
fn mute_command_clamps_hostile_duration() {
    let svc = ChatModerationService::new();
    for argument in ["inf", "-inf", "nan"] {
        let text = local_action(&svc, &format!("/mute u {argument}"));
        assert!(
            text.contains("300초"),
            "{argument} → 기본값으로 폴백해야 한다: {text}"
        );
    }
    for argument in ["1e30", "99999999999999999999"] {
        let text = local_action(&svc, &format!("/mute u {argument}"));
        assert!(
            text.contains("86400초"),
            "{argument} → 상한으로 제한해야 한다: {text}"
        );
    }
    assert!(local_action(&svc, "/mute u abc").contains("300초"));
    let _ = svc.process_command("/mute neg -100");
    assert!(svc.is_muted("neg"));
}

#[test]
fn unmute_restores() {
    let svc = ChatModerationService::new();
    svc.mute_user("u1", 60.0);
    svc.unmute_user("u1");
    assert_eq!(svc.filter_messages(&[msg(Some("u1"), "ok")]).len(), 1);
}

// MARK: - Keyword Filter

#[test]
fn keyword_filter_matches() {
    let svc = ChatModerationService::new();
    svc.add_filter(ChatFilter::new(kw(&["spam", "bad"]), true));
    let result = svc.filter_messages(&[
        msg(None, "this is spam alert"),
        msg(None, "BAD WORD HERE"),
        msg(None, "clean message"),
    ]);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].content, "clean message");
}

#[test]
fn empty_keyword_list_passes_all() {
    let svc = ChatModerationService::new();
    svc.add_filter(ChatFilter::new(kw(&[]), true));
    let result = svc.filter_messages(&[
        msg(None, "anything"),
        msg(None, "spam"),
        msg(None, "filter test"),
    ]);
    assert_eq!(result.len(), 3);
}

#[test]
fn disabled_filter_is_bypassed() {
    let svc = ChatModerationService::new();
    svc.add_filter(ChatFilter::new(kw(&["spam"]), false));
    assert_eq!(svc.filter_messages(&[msg(None, "spam spam spam")]).len(), 1);
}

#[test]
fn privileged_messages_bypass_keyword_filter() {
    let svc = ChatModerationService::new();
    svc.add_filter(ChatFilter::new(kw(&["block"]), true));
    let msgs = [
        make_message(None, "block this", MessageType::Donation),
        make_message(None, "block this", MessageType::Subscription),
        make_message(None, "block this", MessageType::Notice),
        make_message(None, "block this", MessageType::SystemMessage),
        make_message(None, "block this", MessageType::Normal),
    ];
    let result = svc.filter_messages(&msgs);
    assert_eq!(result.len(), 4);
    assert!(result.iter().all(|m| m.r#type != MessageType::Normal));
}

#[test]
fn set_keyword_filter_replaces() {
    let svc = ChatModerationService::new();
    svc.set_keyword_filter(vec!["spam".into()], true);
    svc.set_keyword_filter(vec!["banana".into()], true);
    let result = svc.filter_messages(&[
        msg(None, "spam should now pass"),
        msg(None, "banana should be blocked"),
    ]);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].content, "spam should now pass");
}

#[test]
fn set_keyword_filter_empty() {
    let svc = ChatModerationService::new();
    svc.set_keyword_filter(vec!["spam".into()], true);
    svc.set_keyword_filter(vec![], true);
    assert!(!svc.has_active_rules());
}

#[test]
fn set_keyword_filter_enabled_false() {
    let svc = ChatModerationService::new();
    svc.set_keyword_filter(vec!["spam".into()], true);
    svc.set_keyword_filter_enabled(false);
    assert_eq!(
        svc.filter_messages(&[msg(None, "spam should pass when disabled")])
            .len(),
        1
    );
    assert!(
        !svc.has_active_rules(),
        "disabled keyword filter 는 활성 규칙으로 카운트되지 않는다"
    );
}

#[test]
fn has_active_rules_union() {
    let svc = ChatModerationService::new();
    assert!(!svc.has_active_rules());
    svc.block_user("u1");
    assert!(svc.has_active_rules());
    svc.unblock_user("u1");
    assert!(!svc.has_active_rules());
    svc.set_keyword_filter(vec!["k".into()], true);
    assert!(svc.has_active_rules());
}

// MARK: - Regex Filter

#[test]
fn valid_regex_filters_matching_messages() {
    let svc = ChatModerationService::new();
    svc.add_filter(ChatFilter::new(
        FilterType::Regex(r"\d{3}-\d{4}".into()),
        true,
    ));
    let result = svc.filter_messages(&[
        msg(None, "call me at 010-1234"),
        msg(None, "no numbers here"),
        msg(None, "another 123-4567 hit"),
    ]);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].content, "no numbers here");
}

#[test]
fn invalid_regex_is_silent_no_op() {
    let svc = ChatModerationService::new();
    svc.add_filter(ChatFilter::new(FilterType::Regex("[unclosed".into()), true));
    let result = svc.filter_messages(&[
        msg(None, "abc"),
        msg(None, "any text"),
        msg(None, "[bracket]"),
    ]);
    assert_eq!(result.len(), 3);
}

#[test]
fn regex_case_insensitive() {
    let svc = ChatModerationService::new();
    svc.add_filter(ChatFilter::new(FilterType::Regex("badword".into()), true));
    let result = svc.filter_messages(&[msg(None, "BADWORD here"), msg(None, "clean")]);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].content, "clean");
}

// MARK: - Combined

#[test]
fn combined_rules_are_or() {
    let svc = ChatModerationService::new();
    svc.block_user("blocked");
    svc.mute_user("muted", 60.0);
    svc.add_filter(ChatFilter::new(kw(&["forbidden"]), true));
    let result = svc.filter_messages(&[
        msg(Some("blocked"), "x"),
        msg(Some("muted"), "x"),
        msg(Some("ok"), "forbidden text"),
        msg(Some("ok"), "good text"),
    ]);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].content, "good text");
}

#[test]
fn remove_all_filters_clears_content_only() {
    let svc = ChatModerationService::new();
    svc.add_filter(ChatFilter::new(kw(&["spam"]), true));
    svc.block_user("baduser");
    svc.remove_all_filters();
    assert!(svc.current_filters().is_empty());
    let result = svc.filter_messages(&[
        msg(Some("baduser"), "still blocked"),
        msg(Some("ok"), "spam allowed now"),
    ]);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].user_id.as_deref(), Some("ok"));
}

#[test]
fn should_show_agrees_with_filter() {
    let svc = ChatModerationService::new();
    svc.block_user("blocked");
    svc.add_filter(ChatFilter::new(kw(&["bad"]), true));
    let candidates = vec![
        msg(Some("ok"), "clean"),
        msg(Some("blocked"), "anything"),
        msg(Some("ok"), "bad word"),
        make_message(Some("ok"), "bad word", MessageType::Donation),
        make_message(Some("ok"), "bad word", MessageType::Subscription),
        make_message(Some("ok"), "bad word", MessageType::Notice),
        make_message(Some("ok"), "bad word", MessageType::SystemMessage),
        make_message(Some("blocked"), "bad word", MessageType::Donation),
    ];
    let bulk: Vec<String> = svc
        .filter_messages(&candidates)
        .into_iter()
        .map(|m| m.id)
        .collect();
    let single: Vec<String> = candidates
        .iter()
        .filter(|c| svc.should_show(c))
        .map(|m| m.id.clone())
        .collect();
    assert_eq!(bulk, single);
    assert_eq!(bulk.len(), 5);
    assert!(
        svc.should_show(&candidates[3]),
        "후원은 키워드 필터를 우회해야 한다"
    );
    assert!(
        !svc.should_show(&candidates[7]),
        "차단 사용자는 후원이라도 막힌다"
    );
}

#[test]
fn expired_mute_is_not_an_active_rule() {
    let svc = ChatModerationService::new();
    svc.mute_user("u1", 0.01);
    assert!(svc.has_active_rules());
    std::thread::sleep(Duration::from_millis(30));
    assert!(!svc.has_active_rules());
    assert!(!svc.is_muted("u1"));
}

// MARK: - Command parsing (추가 고정)

#[test]
fn command_parse_and_results() {
    assert_eq!(ChatCommand::parse("hello"), None);
    assert_eq!(ChatCommand::parse("/unknown x"), None);
    assert_eq!(
        ChatCommand::parse("  /ban   a  b "),
        Some((ChatCommand::Ban, vec!["a".into(), "b".into()]))
    );

    let svc = ChatModerationService::new();
    assert_eq!(
        svc.process_command("/clear"),
        Some(ChatCommandResult::ClearChat)
    );
    assert_eq!(
        svc.process_command("/export"),
        Some(ChatCommandResult::ExportChat)
    );
    assert!(matches!(
        svc.process_command("/ban"),
        Some(ChatCommandResult::Error(_))
    ));
    assert!(matches!(
        svc.process_command("/notice"),
        Some(ChatCommandResult::Error(_))
    ));
    assert_eq!(
        svc.process_command("/slow 5"),
        Some(ChatCommandResult::ServerCommand {
            command: ChatCommand::Slow,
            args: vec!["5".into()]
        })
    );
    local_action(&svc, "/ban x");
    assert!(svc.is_blocked("x"));
    local_action(&svc, "/filter foo bar");
    assert_eq!(svc.current_filters().len(), 1);
    let help = local_action(&svc, "/help");
    for c in ChatCommand::ALL {
        assert!(help.contains(c.raw_value()));
    }
}
