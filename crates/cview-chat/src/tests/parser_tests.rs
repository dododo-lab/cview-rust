//! 파서 테스트. 원본: `ChatMessageParserTests.swift`, `CViewChatTests.swift`(파서·명령 코드·AnyCodable),
//! `ChatMessageParserExhaustivenessTests.swift`, `ChatKickTargetingTests.swift`,
//! `ChatProtocolPrecisionTests.swift`(파서 부분), `ChatProtocolInvariantTests.swift`(수신 예산).

use serde_json::{json, Value};

use super::json_escape;
use crate::constants::{chat_defaults, ws_defaults};
use crate::engine::ChatEngine;
use crate::error::ChatError;
use crate::parser::ChatMessageParser;
use crate::protocol::{ChatEvent, ChatProtocolMessage, ChzzkChatCommand};
use cview_core::models::chat::{ChatMessage, EmojiMap, MessageType};

fn parser() -> ChatMessageParser {
    ChatMessageParser::with_os_version("macOS/15.0.0")
}

fn messages(event: ChatEvent) -> Vec<ChatMessage> {
    match event {
        ChatEvent::Messages(m) => m,
        other => panic!("Expected .messages event, got {other:?}"),
    }
}

fn donations(event: ChatEvent) -> Vec<ChatMessage> {
    match event {
        ChatEvent::Donations(m) => m,
        other => panic!("Expected .donations event, got {other:?}"),
    }
}

fn recent(event: ChatEvent) -> Vec<ChatMessage> {
    match event {
        ChatEvent::RecentMessages(m) => m,
        other => panic!("Expected .recentMessages event, got {other:?}"),
    }
}

fn parse(json: &str) -> ChatEvent {
    parser().parse(json).expect("parse")
}

// MARK: - Raw Parsing

#[test]
fn parse_raw_extracts_cmd() {
    assert_eq!(parser().parse_raw(r#"{"cmd":93101}"#).unwrap().cmd, 93101);
}

#[test]
fn parse_raw_all_fields() {
    let raw = parser()
        .parse_raw(r#"{"cmd":100,"tid":1,"svcid":"game","cid":"ch1","retCode":0,"retMsg":"OK"}"#)
        .unwrap();
    assert_eq!(raw.cmd, 100);
    assert_eq!(raw.tid, Some(1));
    assert_eq!(raw.svcid.as_deref(), Some("game"));
    assert_eq!(raw.cid.as_deref(), Some("ch1"));
    assert_eq!(raw.ret_code, Some(0));
    assert_eq!(raw.ret_msg.as_deref(), Some("OK"));
}

#[test]
fn parse_raw_body_shapes() {
    let raw = parser().parse_raw(r#"{"cmd":10100,"retCode":0,"bdy":{"sid":"abc"}}"#).unwrap();
    assert_eq!(raw.bdy.unwrap()["sid"], "abc");
    let raw = parser().parse_raw(r#"{"cmd":93101,"bdy":[{"msg":"hello","uid":"u1","msgTime":1000}]}"#).unwrap();
    assert_eq!(raw.bdy.unwrap().as_array().unwrap().len(), 1);
    assert!(parser().parse_raw(r#"{"cmd":0}"#).unwrap().bdy.is_none());
    // 명시적 null 은 None 과 구분된다 (공지 해제 판정).
    assert_eq!(parser().parse_raw(r#"{"cmd":94010,"bdy":null}"#).unwrap().bdy, Some(Value::Null));
}

#[test]
fn parse_raw_rejects_invalid_input() {
    for bad in ["", "this is not json", r#"{"bdy":{}}"#, "[1,2,3]", "not valid json", r#"{"cmd":true}"#] {
        assert_eq!(parser().parse_raw(bad).unwrap_err(), ChatError::InvalidMessage, "{bad}");
    }
}

#[test]
fn parse_raw_unicode() {
    let raw = parser().parse_raw(r#"{"cmd":93101,"bdy":[{"msg":"안녕하세요 🎉","uid":"u1","msgTime":1000}]}"#).unwrap();
    assert_eq!(raw.bdy.unwrap()[0]["msg"], "안녕하세요 🎉");
}

#[test]
fn parses_tid_in_both_formats() {
    let p = parser();
    assert_eq!(p.parse_raw(r#"{"cmd":13101,"tid":7}"#).unwrap().tid, Some(7));
    assert_eq!(p.parse_raw(r#"{"cmd":13101,"tid":"7"}"#).unwrap().tid, Some(7));
    assert_eq!(p.parse_raw(r#"{"cmd":13101,"tid":"abc"}"#).unwrap().tid, None);
    assert_eq!(p.parse_raw(r#"{"cmd":13101}"#).unwrap().tid, None);
}

// MARK: - Chat Messages

#[test]
fn parse_single_chat_with_profile() {
    let m = messages(parse(
        r#"{"cmd":93101,"bdy":[{"msg":"Hello!","uid":"user1","msgTime":1700000000000,"profile":{"nickname":"TestUser","profileImageUrl":"https://img.example.com/pic.jpg","userRoleCode":"common"},"extras":"{}"}]}"#,
    ));
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].content, "Hello!");
    assert_eq!(m[0].nickname, "TestUser");
    assert_eq!(m[0].user_id.as_deref(), Some("user1"));
    assert_eq!(m[0].profile.as_ref().unwrap().profile_image_url.as_deref(), Some("https://img.example.com/pic.jpg"));
}

#[test]
fn deterministic_message_id() {
    let json = r#"{"cmd":93101,"bdy":[{"msg":"dup test","uid":"user1","msgTime":1700000000000,"profile":{"nickname":"TestUser"},"extras":"{}"}]}"#;
    let a = messages(parse(json));
    let b = messages(parse(json));
    assert_eq!(a[0].id, b[0].id);
    assert!(a[0].id.starts_with("user1_1700000000000_"));
}

#[test]
fn server_message_id_preferred() {
    let m = messages(parse(
        r#"{"cmd":93101,"bdy":[{"msg":"hi","uid":"user1","msgTime":1000,"messageId":"server-stable-id","extras":"{}"}]}"#,
    ));
    assert_eq!(m[0].id, "server-stable-id");
}

#[test]
fn anonymous_id_prefix_when_uid_missing() {
    let m = messages(parse(r#"{"cmd":93101,"bdy":[{"msg":"hi","msgTime":1000}]}"#));
    assert_eq!(m[0].content, "hi");
    assert!(m[0].id.starts_with("anon_1000_"));
}

#[test]
fn clean_bot_blinded_message() {
    let m = messages(parse(
        r#"{"cmd":93101,"bdy":[{"msg":"원문내용유출금지","uid":"user1","msgTime":1700000000000,"msgStatusType":"CBOTBLIND","profile":{"nickname":"TestUser"},"extras":"{\"emojis\":{\"e1\":\"https://x/e.png\"}}"}]}"#,
    ));
    assert_eq!(m.len(), 1);
    assert!(m[0].is_blinded);
    assert_eq!(m[0].content, "클린봇이 부적절한 표현을 감지했습니다.");
    assert_eq!(m[0].nickname, "TestUser");
    assert!(m[0].extras.is_none());
}

#[test]
fn normal_status_not_blinded() {
    let m = messages(parse(r#"{"cmd":93101,"bdy":[{"msg":"hello","uid":"u1","msgTime":1000,"msgStatusType":"NORMAL","extras":"{}"}]}"#));
    assert!(!m[0].is_blinded);
    assert_eq!(m[0].content, "hello");
}

#[test]
fn multiple_and_empty_messages() {
    let m = messages(parse(
        r#"{"cmd":93101,"bdy":[{"msg":"msg1","uid":"u1","msgTime":1000},{"msg":"msg2","uid":"u2","msgTime":2000},{"msg":"msg3","uid":"u3","msgTime":3000}]}"#,
    ));
    assert_eq!(m.iter().map(|x| x.content.as_str()).collect::<Vec<_>>(), ["msg1", "msg2", "msg3"]);
    assert_eq!(messages(parse(r#"{"cmd":93101,"bdy":[{"msg":"","uid":"u1","msgTime":1000}]}"#))[0].content, "");
    assert_eq!(messages(parse(r#"{"cmd":93101,"bdy":[{"uid":"u1","msgTime":1000}]}"#))[0].content, "");
    assert!(messages(parse(r#"{"cmd":93101,"bdy":[]}"#)).is_empty());
    assert!(messages(parse(r#"{"cmd":93101}"#)).is_empty());
}

#[test]
fn timestamp_conversion() {
    let m = messages(parse(r#"{"cmd":93101,"bdy":[{"msg":"test","uid":"u1","msgTime":1700000000000}]}"#));
    assert_eq!(m[0].timestamp.timestamp(), 1_700_000_000);
}

// MARK: - Donations

#[test]
fn parse_donation_with_amount() {
    let extras = r#"{"payAmount":1000,"donationType":"CHAT","chatType":"STREAMING"}"#;
    let json = format!(
        r#"{{"cmd":93102,"bdy":[{{"msg":"donation msg","uid":"donor1","msgTime":1000,"extras":"{}"}}]}}"#,
        json_escape(extras)
    );
    let m = donations(parse(&json));
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].content, "donation msg");
    assert_eq!(m[0].r#type, MessageType::Donation);
    let d = m[0].extras.as_ref().unwrap().donation.as_ref().unwrap();
    assert_eq!(d.amount, 1000);
    assert_eq!(d.currency, "KRW");
    // 닉네임 없는 후원은 익명.
    assert_eq!(m[0].nickname, "익명");
}

#[test]
fn parse_donation_zero_amount_is_normal() {
    let extras = r#"{"payAmount":0,"chatType":"STREAMING"}"#;
    let json = format!(
        r#"{{"cmd":93102,"bdy":[{{"msg":"not a real donation","uid":"u1","msgTime":1000,"extras":"{}"}}]}}"#,
        json_escape(extras)
    );
    let m = donations(parse(&json));
    assert_eq!(m[0].r#type, MessageType::Normal);
}

#[test]
fn parse_donation_nested_extras_and_double_amounts() {
    let m = donations(parse(
        r#"{"cmd":93102,"bdy":[{"msg":"hello donation","uid":"d1","msgTime":2000,"extras":{"payAmount":5000,"donationType":"CHAT","chatType":"STREAMING"}}]}"#,
    ));
    assert_eq!(m[0].r#type, MessageType::Donation);
    assert_eq!(m[0].extras.as_ref().unwrap().donation.as_ref().unwrap().amount, 5000);

    let m = donations(parse(
        r#"{"cmd":93102,"bdy":[{"msg":"double amount","uid":"d2","msgTime":3000,"extras":{"payAmount":2500.0,"donationType":"VIDEO"}}]}"#,
    ));
    let d = m[0].extras.as_ref().unwrap().donation.as_ref().unwrap();
    assert_eq!((d.amount, d.r#type.as_str()), (2500, "VIDEO"));

    let m = donations(parse(
        r#"{"cmd":93102,"bdy":[{"msg":"mission","uid":"d3","msgTime":4000,"extras":{"payAmount":10000,"donationType":"MISSION"}}]}"#,
    ));
    let d = m[0].extras.as_ref().unwrap().donation.as_ref().unwrap();
    assert_eq!((d.amount, d.r#type.as_str()), (10000, "MISSION"));
}

#[test]
fn parse_subscription_nested_extras() {
    let m = messages(parse(
        r#"{"cmd":93101,"bdy":[{"msg":"subbed","uid":"s1","msgTime":5000,"extras":{"chatType":"SUBSCRIPTION","month":3.0,"tierName":"Tier 1"}}]}"#,
    ));
    assert_eq!(m[0].r#type, MessageType::Subscription);
    let s = m[0].extras.as_ref().unwrap().subscription.as_ref().unwrap();
    assert_eq!(s.months, 3);
    assert_eq!(s.tier_name.as_deref(), Some("Tier 1"));
}

#[test]
fn anonymous_flag_overrides_nickname() {
    let m = donations(parse(
        r#"{"cmd":93102,"bdy":[{"msg":"x","uid":"d","msgTime":1,"profile":{"nickname":"Real"},"extras":{"payAmount":"₩1,000원","isAnonymous":true}}]}"#,
    ));
    assert_eq!(m[0].nickname, "익명");
    assert_eq!(m[0].extras.as_ref().unwrap().donation.as_ref().unwrap().amount, 1000);
}

// MARK: - Profiles & Badges

#[test]
fn parse_profile_json_string() {
    let profile = r#"{"nickname":"StreamerNick","profileImageUrl":"https://img.example.com/avatar.jpg","userRoleCode":"streamer"}"#;
    let json = format!(
        r#"{{"cmd":93101,"bdy":[{{"msg":"hi","uid":"u1","msgTime":1000,"profile":"{}"}}]}}"#,
        json_escape(profile)
    );
    let m = messages(parse(&json));
    assert_eq!(m[0].nickname, "StreamerNick");
    assert_eq!(m[0].profile.as_ref().unwrap().user_role_code.as_deref(), Some("streamer"));
}

#[test]
fn parse_manager_profile_keeps_raw_role() {
    let profile = r#"{"nickname":"ManagerNick","userRoleCode":"streaming_chat_manager"}"#;
    let json = format!(
        r#"{{"cmd":93101,"bdy":[{{"msg":"hi","uid":"u1","msgTime":1000,"profile":"{}"}}]}}"#,
        json_escape(profile)
    );
    let m = messages(parse(&json));
    assert_eq!(m[0].profile.as_ref().unwrap().user_role_code.as_deref(), Some("streaming_chat_manager"));
}

#[test]
fn no_profile_defaults_to_unknown() {
    let m = messages(parse(r#"{"cmd":93101,"bdy":[{"msg":"anonymous","uid":"u1","msgTime":1000}]}"#));
    assert_eq!(m[0].nickname, "Unknown");
}

#[test]
fn subscription_badge_from_streaming_property() {
    let profile = r#"{"nickname":"BadgeUser","streamingProperty":{"subscription":{"accumulativeMonth":3,"tier":1,"badge":{"imageUrl":"https://badge.example.com/sub.png"}}}}"#;
    let json = format!(
        r#"{{"cmd":93101,"bdy":[{{"msg":"badged","uid":"u1","msgTime":1000,"profile":"{}"}}]}}"#,
        json_escape(profile)
    );
    let m = messages(parse(&json));
    let badge = m[0].profile.as_ref().unwrap().badge.as_ref().unwrap();
    assert_eq!(badge.image_url.as_deref(), Some("https://badge.example.com/sub.png"));
    assert_eq!(badge.badge_id.as_deref(), Some("subscription_1"));
    assert_eq!(badge.alt_text.as_deref(), Some("3개월 구독"));
}

#[test]
fn viewer_badges() {
    let profile = r#"{"nickname":"ViewerBadgeUser","viewerBadges":[{"badge":{"badgeId":"donation_newbie","imageUrl":"https://badge.example.com/fan.png","scope":"CHANNEL"},"type":"STANDARD"}]}"#;
    let json = format!(
        r#"{{"cmd":93101,"bdy":[{{"msg":"hi","uid":"u1","msgTime":1000,"profile":"{}"}}]}}"#,
        json_escape(profile)
    );
    let m = messages(parse(&json));
    let badges = &m[0].profile.as_ref().unwrap().badges;
    assert!(!badges.is_empty());
    assert_eq!(badges[0].image_url.as_deref(), Some("https://badge.example.com/fan.png"));
    assert_eq!(badges[0].badge_id.as_deref(), Some("donation_newbie"));
}

// MARK: - Emojis

fn extras_message(extras: &str, msg: &str) -> ChatMessage {
    let json = format!(
        r#"{{"cmd":93101,"bdy":[{{"msg":"{msg}","uid":"u1","msgTime":1000,"extras":"{}"}}]}}"#,
        json_escape(extras)
    );
    messages(parse(&json)).remove(0)
}

#[test]
fn emoji_map_parsing() {
    let m = extras_message(
        r#"{"emojis":{"happy_emote":"https://emoji.example.com/happy.png","sad_emote":"https://emoji.example.com/sad.png"},"chatType":"STREAMING"}"#,
        "{:happy_emote:} {:sad_emote:}",
    );
    let emojis = m.extras.unwrap().emojis.unwrap();
    assert_eq!(emojis.len(), 2);
    assert_eq!(emojis["happy_emote"], "https://emoji.example.com/happy.png");

    assert!(extras_message(r#"{"emojis":{},"chatType":"STREAMING"}"#, "no emojis").extras.unwrap().emojis.is_none());
    assert!(extras_message(r#"{"chatType":"STREAMING"}"#, "text").extras.unwrap().emojis.is_none());
}

#[test]
fn non_string_emoji_values_are_dropped() {
    let m = extras_message(
        r#"{"emojis":{"good":"https://emoji.example.com/g.png","bad":null,"num":42},"chatType":"STREAMING"}"#,
        "{:good:} {:bad:} {:num:}",
    );
    let emojis = m.extras.unwrap().emojis.unwrap();
    assert_eq!(emojis.len(), 1);
    assert_eq!(emojis["good"], "https://emoji.example.com/g.png");
}

#[test]
fn subscription_and_system_types_from_extras() {
    assert_eq!(extras_message(r#"{"chatType":"SUBSCRIPTION"}"#, "subscribed!").r#type, MessageType::Subscription);
    assert_eq!(extras_message(r#"{"chatType":"SYSTEM"}"#, "system notice").r#type, MessageType::SystemMessage);
}

// MARK: - Event Types

#[test]
fn connected_event_and_rejections() {
    assert_eq!(
        parse(r#"{"cmd":10100,"retCode":0,"bdy":{"sid":"sess-xyz-789"}}"#),
        ChatEvent::Connected { session_id: "sess-xyz-789".into() }
    );
    assert!(matches!(
        parse(r#"{"cmd":10100,"retCode":0,"bdy":{}}"#),
        ChatEvent::ConnectionRejected { ret_code: Some(0), .. }
    ));
    assert_eq!(
        parse(r#"{"cmd":10100,"retCode":401,"retMsg":"invalid token","bdy":{"sid":"must-not-pass"}}"#),
        ChatEvent::ConnectionRejected { ret_code: Some(401), ret_msg: Some("invalid token".into()) }
    );
    assert!(matches!(
        parse(r#"{"cmd":10100,"bdy":{"sid":"must-not-pass"}}"#),
        ChatEvent::ConnectionRejected { ret_code: None, .. }
    ));
    assert!(matches!(
        parse(r#"{"cmd":10100,"retCode":0,"bdy":{"sid":"  \n "}}"#),
        ChatEvent::ConnectionRejected { ret_code: Some(0), .. }
    ));
}

#[test]
fn recent_chat_variants() {
    let m = recent(parse(
        r#"{"cmd":15101,"bdy":{"messageList":[{"msg":"recent1","uid":"u1","msgTime":1000},{"msg":"recent2","uid":"u2","msgTime":2000}]}}"#,
    ));
    assert_eq!(m.len(), 2);

    let m = recent(parse(
        r#"{"cmd":15101,"bdy":{"messageList":[
          {"content":"recent-long","userId":"long-user","messageTime":1700000000123,"messageTypeCode":10,"messageStatusType":"NORMAL","profile":{"nickname":"LongUser"},"extras":"{}"},
          {"content":"secret","userId":"blind-user","messageTime":1700000000456,"messageTypeCode":1,"messageStatusType":"CBOTBLIND","profile":{"nickname":"BlindUser"},"extras":"{}"}
        ]}}"#,
    ));
    assert_eq!(m.len(), 2);
    assert_eq!(m[0].content, "recent-long");
    assert_eq!(m[0].user_id.as_deref(), Some("long-user"));
    assert_eq!(m[0].timestamp.timestamp_millis(), 1_700_000_000_123);
    assert_eq!(m[0].r#type, MessageType::Donation);
    assert_eq!(m[1].user_id.as_deref(), Some("blind-user"));
    assert_eq!(m[1].content, ChatMessageParser::CLEAN_BOT_BLIND_TEXT);
    assert!(m[1].is_blinded);
}

#[test]
fn notice_events() {
    let ChatEvent::Notice(n) = parse(r#"{"cmd":94010,"bdy":{"msg":"This is a notice"}}"#) else { panic!() };
    assert_eq!(n.content, "This is a notice");
    assert_eq!(n.r#type, MessageType::Notice);
    assert_eq!(n.nickname, "공지");

    let ChatEvent::Notice(n) = parse(
        r#"{"cmd":94010,"bdy":[{"msg":"Array body notice","extras":"{\"registerProfile\":{\"nickname\":\"스트리머닉\"}}"}]}"#,
    ) else {
        panic!()
    };
    assert_eq!(n.content, "Array body notice");
    assert_eq!(n.nickname, "스트리머닉");

    let ChatEvent::Notice(n) = parse(r#"{"cmd":94010,"bdy":{"notice":{"message":"  nested  "},"noticeId":"77"}}"#) else {
        panic!()
    };
    assert_eq!(n.content, "nested");
    assert_eq!(n.id, "notice_77");
}

#[test]
fn notice_id_is_stable_by_content() {
    let json = r#"{"cmd":94010,"bdy":{"msg":"고정 공지입니다"}}"#;
    let (ChatEvent::Notice(a), ChatEvent::Notice(b)) = (parse(json), parse(json)) else { panic!() };
    assert_eq!(a.id, b.id);
    assert!(a.id.starts_with("notice_"));
    let ChatEvent::Notice(c) = parse(r#"{"cmd":94010,"bdy":{"msg":"다른 공지"}}"#) else { panic!() };
    assert_ne!(c.id, a.id);
}

#[test]
fn blind_kick_penalty_pong_system_emote_unknown() {
    assert_eq!(
        parse(r#"{"cmd":94008,"bdy":{"messageId":"msg-123","userId":"user-456"}}"#),
        ChatEvent::Blind { message_id: "msg-123".into(), user_id: "user-456".into() }
    );
    assert_eq!(
        parse(r#"{"cmd":94005,"bdy":{"targetUserId":"kicked-user"}}"#),
        ChatEvent::Kick { user_id: "kicked-user".into() }
    );
    assert_eq!(
        parse(r#"{"cmd":94015,"bdy":{"targetUserId":"bad-user","duration":300}}"#),
        ChatEvent::Penalty { user_id: "bad-user".into(), duration: 300 }
    );
    assert_eq!(parse(r#"{"cmd":10000}"#), ChatEvent::Pong);
    assert_eq!(parse(r#"{"cmd":0}"#), ChatEvent::Ping);
    assert_eq!(
        parse(r#"{"cmd":93006,"bdy":{"msg":"System: maintenance in 5 min"}}"#),
        ChatEvent::System("System: maintenance in 5 min".into())
    );
    assert_eq!(messages(parse(r#"{"cmd":93103,"bdy":[{"msg":"emote","uid":"u1","msgTime":1000}]}"#)).len(), 1);
    assert_eq!(parse(r#"{"cmd":12345}"#), ChatEvent::Unknown { cmd: 12345 });
    assert_eq!(parse(r#"{"cmd":99999}"#), ChatEvent::Unknown { cmd: 99999 });
}

#[test]
fn send_confirmed_variants() {
    assert_eq!(
        parse(r#"{"cmd":13101,"retCode":0,"retMsg":"OK"}"#),
        ChatEvent::SendConfirmed { tid: None, ret_code: 0, ret_msg: Some("OK".into()) }
    );
    assert_eq!(
        parse(r#"{"cmd":13101,"retCode":-1,"retMsg":"error"}"#),
        ChatEvent::SendConfirmed { tid: None, ret_code: -1, ret_msg: Some("error".into()) }
    );
    assert_eq!(
        parse(r#"{"cmd":13101,"retCode":5}"#),
        ChatEvent::SendConfirmed { tid: None, ret_code: 5, ret_msg: None }
    );
    assert!(matches!(parse(r#"{"cmd":13101,"tid":9,"retCode":0}"#), ChatEvent::SendConfirmed { tid: Some(9), .. }));
    assert!(matches!(parse(r#"{"cmd":13101,"tid":"9","retCode":-1}"#), ChatEvent::SendConfirmed { tid: Some(9), .. }));
    assert!(matches!(parse(r#"{"cmd":13101,"retCode":-1}"#), ChatEvent::SendConfirmed { tid: None, .. }));
}

// MARK: - Build (와이어 바이트)

#[test]
fn build_connect_message_is_byte_exact() {
    let p = parser();
    let msg = p.build_connect_message("ch-abc", "tok-123", Some("uid-456"));
    assert_eq!(
        msg,
        r#"{"bdy":{"accTkn":"tok-123","auth":"SEND","devName":"CView_v2\/1.0.0","devType":2001,"libVer":"4.9.3","locale":"ko","osVer":"macOS\/15.0.0","timezone":"Asia\/Seoul","uid":"uid-456"},"cid":"ch-abc","cmd":100,"svcid":"game","tid":1,"ver":"3"}"#
    );
    let anon = p.build_connect_message("ch-abc", "tok", None);
    assert!(anon.contains(r#""auth":"READ""#));
    assert!(anon.contains(r#""uid":"""#));
}

#[test]
fn build_send_message_is_byte_exact() {
    let mut emojis = EmojiMap::new();
    emojis.insert("smile".into(), "https://emoji.com/smile.png".into());
    let msg = parser().build_send_message_at("ch-1", Some("chan"), Some("sid-1"), Some("xt"), "Hello \"World\"!", 42, Some(&emojis), 1_700_000_000_000);
    let extras = r#"{"chatType":"STREAMING","emojis":{"smile":"https:\/\/emoji.com\/smile.png"},"extraToken":"xt","osType":"PC","streamingChannelId":"chan"}"#;
    let expected = format!(
        r#"{{"bdy":{{"ctime":1700000000000,"extras":"{}","msg":"Hello \"World\"!","msgTime":1700000000000,"msgTypeCode":1}},"cid":"ch-1","cmd":3101,"retry":false,"sid":"sid-1","svcid":"game","tid":"42","ver":"3"}}"#,
        extras.replace('\\', "\\\\").replace('"', "\\\"").replace('/', "\\/")
    );
    assert_eq!(msg, expected);

    // 기본값: channelId 없으면 chatChannelId, extraToken 없으면 키 생략, 빈 emojis 객체.
    let msg = parser().build_send_message_at("ch-1", None, None, None, "hi", 3, None, 1);
    let v: Value = serde_json::from_str(&msg).unwrap();
    let extras: Value = serde_json::from_str(v["bdy"]["extras"].as_str().unwrap()).unwrap();
    assert_eq!(extras, json!({"chatType":"STREAMING","osType":"PC","streamingChannelId":"ch-1","emojis":{}}));
    assert_eq!(v["sid"], "");
    assert!(msg.contains("3101"));
}

#[test]
fn send_frame_tid_is_string_and_round_trips() {
    let frame = parser().build_send_message("cid1", None, None, None, "hi", 42, None);
    assert!(frame.contains(r#""tid":"42""#));
    let v: Value = serde_json::from_str(&frame).unwrap();
    assert_eq!(ChatMessageParser::parse_tid(v.get("tid")), Some(42));
}

#[test]
fn build_recent_chat_and_pong() {
    let msg = parser().build_recent_chat_request("ch-1", "session-verified", chat_defaults::RECENT_CHAT_DEFAULT_COUNT);
    assert_eq!(
        msg,
        r#"{"bdy":{"recentMessageCount":50},"cid":"ch-1","cmd":5101,"sid":"session-verified","svcid":"game","tid":2,"ver":"3"}"#
    );
    assert_eq!(parser().build_pong(), r#"{"ver":"3","cmd":10000}"#);
}

// MARK: - Command codes & exhaustiveness

#[test]
fn command_codes() {
    use ChzzkChatCommand::*;
    assert_eq!(Ping.raw_value(), 0);
    assert_eq!(Connect.raw_value(), 100);
    assert_eq!(SendChat.raw_value(), 3101);
    assert_eq!(Connected.raw_value(), 10100);
    assert_eq!(ChatMessage.raw_value(), 93101);
    assert_eq!(Donation.raw_value(), 93102);
    assert_eq!(Notice.raw_value(), 94010);
    assert_eq!(SystemMessage.raw_value(), 93006);
    assert_eq!(ChzzkChatCommand::from_raw(99999), None);
    assert_eq!(ChzzkChatCommand::ALL.len(), 17);
    let unique: std::collections::HashSet<i64> = ChzzkChatCommand::ALL.iter().map(|c| c.raw_value()).collect();
    assert_eq!(unique.len(), 17);
    assert!(ChzzkChatCommand::ALL.iter().all(|c| !c.description().is_empty()));
}

#[test]
fn inbound_commands_map_to_known_events() {
    let outbound = [
        ChzzkChatCommand::Connect,
        ChzzkChatCommand::RequestRecentChat,
        ChzzkChatCommand::SendChat,
        ChzzkChatCommand::SendEmote,
    ];
    let p = parser();
    for cmd in ChzzkChatCommand::ALL {
        let raw = ChatProtocolMessage::new(cmd.raw_value()).with_ret_code(0);
        match p.parse_to_domain_event(&raw) {
            Err(_) => assert!(!outbound.contains(&cmd)),
            Ok(ChatEvent::Unknown { cmd: c }) => {
                assert!(outbound.contains(&cmd), "{cmd:?} 가 .unknown 으로 떨어졌다");
                assert_eq!(c, cmd.raw_value());
            }
            Ok(_) => assert!(!outbound.contains(&cmd), "outbound {cmd:?} 가 inbound 로 분류됐다"),
        }
    }
    for raw in [99999, -1, 1234567] {
        assert_eq!(parse(&format!(r#"{{"cmd":{raw}}}"#)), ChatEvent::Unknown { cmd: raw });
    }
}

#[test]
fn core_command_mapping() {
    assert!(matches!(parse(r#"{"cmd":93102,"bdy":[]}"#), ChatEvent::Donations(_)));
    assert!(matches!(parse(r#"{"cmd":15101,"bdy":{"messageList":[]}}"#), ChatEvent::RecentMessages(_)));
    assert!(matches!(parse(r#"{"cmd":94005}"#), ChatEvent::Kick { .. }));
    assert!(matches!(parse(r#"{"cmd":94015,"bdy":{"targetUserId":"u","duration":60}}"#), ChatEvent::Penalty { .. }));
    assert!(matches!(parse(r#"{"cmd":94010,"bdy":{"msg":"hi"}}"#), ChatEvent::Notice(_)));
    assert!(matches!(parse(r#"{"cmd":93103,"bdy":[]}"#), ChatEvent::Messages(_)));
}

// MARK: - Kick targeting

fn kick_target(json: &str) -> String {
    match parse(json) {
        ChatEvent::Kick { user_id } => user_id,
        other => panic!("expected kick, got {other:?}"),
    }
}

#[test]
fn kick_target_fields_and_fallbacks() {
    assert_eq!(kick_target(r#"{"cmd":94005,"bdy":{"targetUserId":"victim-1"}}"#), "victim-1");
    for key in ["userId", "uid", "kickUserId"] {
        assert_eq!(kick_target(&format!(r#"{{"cmd":94005,"bdy":{{"{key}":"victim-2"}}}}"#)), "victim-2");
    }
    for json in [
        r#"{"cmd":94005}"#,
        r#"{"cmd":94005,"bdy":{}}"#,
        r#"{"cmd":94005,"bdy":{"unrelated":"x"}}"#,
        r#"{"cmd":94005,"bdy":{"targetUserId":""}}"#,
        r#"{"cmd":94005,"bdy":[]}"#,
    ] {
        assert_eq!(kick_target(json), "", "{json}");
    }
    assert_eq!(kick_target(r#"{"cmd":94005,"bdy":{"uid":"someone-else","targetUserId":"victim-3"}}"#), "victim-3");
}

#[test]
fn self_kick_gate() {
    assert!(ChatEngine::is_self_kick("me", Some("me")));
    assert!(!ChatEngine::is_self_kick("someone-else", Some("me")));
    assert!(!ChatEngine::is_self_kick("", Some("me")));
    assert!(!ChatEngine::is_self_kick("victim", None));
    assert!(!ChatEngine::is_self_kick("victim", Some("")));
    assert!(!ChatEngine::is_self_kick("", Some("")));
    assert!(!ChatEngine::is_self_kick("", None));
    assert!(!ChatEngine::is_self_kick(&kick_target(r#"{"cmd":94005}"#), Some("me")));
}

// MARK: - Precision

fn precision_message(extras: Value, profile: Value) -> ChatMessage {
    let body = json!({"uid":"user","msg":"hello","msgTime":1720000000000_i64,"msgTypeCode":1,"extras":extras,"profile":profile});
    let text = serde_json::to_string(&json!({"cmd":93101,"bdy":[body]})).unwrap();
    messages(parse(&text)).remove(0)
}

#[test]
fn equivalent_extras_have_same_identity() {
    let object = json!({"emojis":{"a":"https://example.com/a","b":"https://example.com/b"},"metadata":{"nested":{"x":1,"y":2}}});
    let text = r#"{ "metadata": {"nested": {"y":2,"x":1}}, "emojis": {"b":"https://example.com/b","a":"https://example.com/a"} }"#;
    let a = precision_message(object, json!({}));
    let b = precision_message(Value::String(text.into()), json!({}));
    assert_eq!(a.id, b.id);
    assert_eq!(a.extras.as_ref().unwrap().emojis, b.extras.as_ref().unwrap().emojis);
    let changed = precision_message(json!({"emojis":{"a":"https://example.com/changed"}}), json!({}));
    assert_ne!(changed.id, a.id);
}

#[test]
fn profile_representations_use_same_rules() {
    let object = json!({"nickname":"viewer","userRoleCode":"not_a_manager","badge":{"imageUrl":"https://example.com/badge","badgeId":"badge"},"title":{"name":"title","color":"#ffffff"}});
    let text = serde_json::to_string(&object).unwrap();
    let a = precision_message(json!({}), object);
    let b = precision_message(json!({}), Value::String(text));
    assert_eq!(a.nickname, b.nickname);
    assert_eq!(a.profile, b.profile);
    assert_eq!(a.profile.as_ref().unwrap().badges.len(), 1);
    assert_eq!(a.profile.as_ref().unwrap().title.as_ref().unwrap().name, "title");
}

#[test]
fn numeric_fields_cannot_trap_or_misidentify() {
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(ChatMessageParser::parse_amount_f64(v), None);
    }
    assert_eq!(ChatMessageParser::parse_amount_f64(f64::MAX), Some(i64::MAX));
    assert_eq!(ChatMessageParser::parse_amount_f64(-f64::MAX), Some(i64::MIN));
    assert_eq!(ChatMessageParser::parse_amount(Some(&json!(f64::MAX))), Some(i64::MAX));
    assert_eq!(ChatMessageParser::parse_amount(Some(&json!(-f64::MAX))), Some(i64::MIN));
    assert_eq!(ChatMessageParser::parse_amount(Some(&json!(1234.6))), Some(1235));
    assert_eq!(ChatMessageParser::parse_amount(Some(&json!("₩1,000원"))), Some(1000));
    assert_eq!(ChatMessageParser::parse_amount(Some(&json!("99999999999999999999999"))), Some(i64::MAX));
    assert_eq!(ChatMessageParser::parse_amount(Some(&json!(true))), None);
    assert_eq!(ChatMessageParser::parse_tid(Some(&json!(true))), None);
    assert_eq!(ChatMessageParser::parse_tid(Some(&json!(3.5))), None);
    assert_eq!(ChatMessageParser::parse_tid(Some(&json!("3"))), Some(3));
    assert_eq!(ChatMessageParser::parse_tid(Some(&json!(i64::MAX))), Some(i64::MAX));
}

#[test]
fn notice_removal_and_recent_snapshot_are_preserved() {
    assert_eq!(parse(r#"{"cmd":94010,"bdy":{}}"#), ChatEvent::NoticeCleared);
    assert_eq!(parse(r#"{"cmd":94010,"bdy":null}"#), ChatEvent::NoticeCleared);
    let events = parser().parse_events(r#"{"cmd":15101,"bdy":{"messageList":[],"notice":{"msg":"pinned"}}}"#).unwrap();
    assert_eq!(events.len(), 2);
    assert!(matches!(events[0], ChatEvent::RecentMessages(_)));
    let ChatEvent::Notice(n) = &events[1] else { panic!("Recent snapshot lost its notice or ordering") };
    assert_eq!(n.content, "pinned");
    let malformed = parser().parse_events(r#"{"cmd":15101,"bdy":{"messageList":[],"notice":42}}"#).unwrap();
    assert_eq!(malformed.len(), 1);
    assert!(parser().parse(r#"{"cmd":94010}"#).is_err());
}

#[test]
fn hidden_messages_never_expose_payload() {
    for command in [93101, 15101, 93102] {
        let row = json!({"msg":"hidden payload","uid":"user","msgTime":1,"msgTypeCode":10,"msgStatusType":"HIDDEN","extras":{"payAmount":1000,"emojis":{"x":"https://example.com/x"}}});
        let body = if command == 15101 { json!({"messageList":[row]}) } else { json!([row]) };
        let event = parse(&serde_json::to_string(&json!({"cmd":command,"bdy":body})).unwrap());
        let items = match event {
            ChatEvent::Messages(m) | ChatEvent::RecentMessages(m) | ChatEvent::Donations(m) => m,
            other => panic!("Expected message event, got {other:?}"),
        };
        assert_eq!(items.len(), 1);
        assert!(items[0].is_blinded);
        assert_eq!(items[0].content, "관리자에 의해 삭제된 메시지입니다.");
        assert!(items[0].extras.is_none());
        assert_eq!(items[0].r#type, MessageType::Normal);
    }
}

#[test]
fn large_recent_batch_within_receive_budget() {
    let long_image_url = format!("https://ssl.pstatic.net/static/badge/{}.png", "a".repeat(480));
    let profile = json!({
        "nickname": "긴닉네임".repeat(20),
        "profileImageUrl": long_image_url,
        "viewerBadges": [
            {"badge": {"imageUrl": long_image_url, "badgeId": "supporter"}},
            {"badge": {"imageUrl": long_image_url, "badgeId": "activity"}},
        ],
    });
    let list: Vec<Value> = (0..chat_defaults::RECENT_CHAT_DEFAULT_COUNT)
        .map(|i| {
            json!({
                "messageId": format!("recent-{i}"),
                "content": "가".repeat(180),
                "userId": format!("user-{i}"),
                "messageTime": 1_700_000_000_000_i64 + i,
                "messageTypeCode": 1,
                "messageStatusType": "NORMAL",
                "profile": profile,
                "extras": {},
            })
        })
        .collect();
    let text = serde_json::to_string(&json!({"cmd":15101,"bdy":{"messageList":list}})).unwrap();
    assert!(text.len() > 65_536, "fixture 가 과거 64KiB 제한을 넘어야 한다");
    assert!(text.len() < ws_defaults::MAX_MESSAGE_SIZE);
    let parsed = recent(parse(&text));
    assert_eq!(parsed.len() as i64, chat_defaults::RECENT_CHAT_DEFAULT_COUNT);
    assert_eq!(parsed[0].content, "가".repeat(180));
    assert_eq!(parsed.last().unwrap().user_id.as_deref(), Some("user-49"));
}
