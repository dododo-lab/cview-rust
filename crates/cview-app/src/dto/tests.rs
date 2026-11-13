use super::*;
use cview_chat::ChatEngineEvent;
use cview_core::models::chat::ChatProfile;
use cview_player::coordinator::{BufferHealth, StreamQualityInfo};
use serde_json::json;

fn live_info(v: serde_json::Value) -> LiveInfo {
    serde_json::from_value(v).unwrap()
}

fn popular_live() -> LiveInfo {
    live_info(json!({
        "liveId": 77,
        "liveTitle": "저녁 방송",
        "status": "OPEN",
        "concurrentUserCount": 15234,
        "accumulateCount": 48210,
        "liveCategoryValue": "League of Legends",
        "chatChannelId": "N1abc",
        "liveImageUrl": "https://livecloud-thumb.akamaized.net/x/{type}/image.jpg",
        "openDate": "2026-10-19 18:00:00",
        "adult": false,
        "tags": ["롤"],
        "channel": {
            "channelId": "ch1",
            "channelName": "스트리머",
            "channelImageUrl": "https://nng-phinf.pstatic.net/a.png",
            "verifiedMark": true
        }
    }))
}

#[test]
fn live_card_from_live_info() {
    let card = LiveCardDto::from_live_info(&popular_live()).unwrap();
    assert_eq!(card.channel_id, "ch1");
    assert_eq!(card.channel_name, "스트리머");
    assert_eq!(card.live_title, "저녁 방송");
    assert_eq!(card.viewer_count, 15234);
    assert_eq!(card.viewer_count_text, "1.5만");
    assert_eq!(card.category_name.as_deref(), Some("League of Legends"));
    assert_eq!(
        card.thumbnail_url.as_deref(),
        Some("https://livecloud-thumb.akamaized.net/x/720/image.jpg")
    );
    assert!(card.is_live);

    let json = serde_json::to_value(&card).unwrap();
    assert_eq!(json["channelId"], "ch1");
    assert_eq!(json["viewerCountText"], "1.5만");
    assert!(json.get("thumbnailUrl").is_some());
}

#[test]
fn live_without_channel_is_dropped_and_duplicates_collapse() {
    let no_channel = live_info(json!({"liveId": 1, "liveTitle": "x"}));
    assert!(LiveCardDto::from_live_info(&no_channel).is_none());
    let cards = live_cards(&[popular_live(), no_channel, popular_live()]);
    assert_eq!(cards.len(), 1);
}

#[test]
fn blank_category_is_none() {
    let mut info = popular_live();
    info.live_category_value = Some("  ".into());
    assert_eq!(
        LiveCardDto::from_live_info(&info).unwrap().category_name,
        None
    );
}

fn item(id: &str, live: bool, viewers: i64) -> LiveChannelItem {
    LiveChannelItem {
        id: id.into(),
        channel_name: format!("채널{id}"),
        channel_image_url: None,
        live_title: "방송".into(),
        viewer_count: viewers,
        category_name: None,
        category_type: None,
        thumbnail_url: Some("https://t/x.jpg".into()),
        channel_id: id.into(),
        is_live: live,
        open_date: None,
        verified_badge: None,
    }
}

#[test]
fn following_cards_keep_live_only_sorted_by_viewers() {
    let cards = following_live_cards(&[
        item("a", true, 10),
        item("b", false, 999),
        item("c", true, 500),
    ]);
    let ids: Vec<_> = cards.iter().map(|c| c.channel_id.as_str()).collect();
    assert_eq!(ids, ["c", "a"]);
}

#[test]
fn live_detail_falls_back_to_requested_channel() {
    let detail = LiveDetailDto::from_live_info(&popular_live(), "ignored");
    assert_eq!(detail.channel_id, "ch1");
    assert_eq!(detail.chat_channel_id.as_deref(), Some("N1abc"));
    assert_eq!(detail.status, "OPEN");
    assert_eq!(detail.tags, vec!["롤".to_string()]);
    assert!(detail.verified);
    assert_eq!(detail.accumulate_count, 48210);
    assert_eq!(detail.accumulate_count_text, format_viewer_count(48210));

    let bare =
        live_info(json!({"liveId": 2, "liveTitle": "t", "status": "CLOSE", "chatChannelId": ""}));
    let detail = LiveDetailDto::from_live_info(&bare, "req");
    assert_eq!(detail.channel_id, "req");
    assert!(!detail.is_live);
    assert!(!detail.verified);
    assert_eq!(detail.accumulate_count, 0);
    assert_eq!(detail.chat_channel_id, None);
}

#[test]
fn channel_dto() {
    let mut c = ChannelInfo::new("id", "이름");
    c.follower_count = 1234;
    c.channel_description = Some(String::new());
    let dto = ChannelDto::from(&c);
    assert_eq!(dto.follower_count_text, "1.2천");
    assert_eq!(dto.description, None);
}

#[test]
fn auth_status_mapping() {
    let out = AuthStatusDto::from_state(&AuthState::LoggedOut);
    assert!(!out.logged_in);
    assert_eq!(out.state, "loggedOut");
    let inn = AuthStatusDto::from_state(&AuthState::LoggedIn {
        user_id: "cookie".into(),
    })
    .with_profile(Some("닉네임".into()), Some(String::new()));
    assert!(inn.logged_in);
    assert_eq!(inn.method.as_deref(), Some("cookie"));
    assert_eq!(inn.nickname.as_deref(), Some("닉네임"));
    assert_eq!(inn.profile_image_url, None);
    let json = serde_json::to_value(&inn).unwrap();
    assert_eq!(json["loggedIn"], true);
}

fn chat_message() -> ChatMessage {
    let mut m = ChatMessage::new("시청자", "안녕하세요");
    m.user_id = Some("u1".into());
    m.profile = Some(ChatProfile::new(
        "시청자",
        None,
        Some("streaming_chat_manager".into()),
        None,
        Vec::new(),
        None,
        Vec::new(),
    ));
    m
}

#[test]
fn chat_message_dto() {
    let dto = ChatMessageDto::from_message(&chat_message(), false);
    assert_eq!(dto.nickname, "시청자");
    assert_eq!(dto.content, "안녕하세요");
    assert_eq!(dto.user_id, "u1");
    assert_eq!(dto.kind, "normal");
    assert_eq!(dto.role, "manager");
    let notice = ChatMessageDto::from_message(&chat_message(), true);
    assert_eq!(notice.kind, "notice");
    assert_eq!(dto.profile_image_url, None);
}

#[test]
fn chat_message_dto_carries_profile_image_unless_hidden() {
    let with_image = |url: &str| {
        let mut m = chat_message();
        m.profile.as_mut().unwrap().profile_image_url = Some(url.into());
        m
    };
    let dto =
        ChatMessageDto::from_message(&with_image("https://nng-phinf.pstatic.net/a.png"), false);
    assert_eq!(
        dto.profile_image_url.as_deref(),
        Some("https://nng-phinf.pstatic.net/a.png")
    );
    let json = serde_json::to_value(&dto).unwrap();
    assert_eq!(
        json["profileImageUrl"],
        "https://nng-phinf.pstatic.net/a.png"
    );
    // https 가 아니면 싣지 않는다
    let plain = ChatMessageDto::from_message(&with_image("http://x/a.png"), false);
    assert_eq!(plain.profile_image_url, None);
    // 가려진 메시지·익명 후원은 사진을 싣지 않는다
    let mut blinded = with_image("https://x/a.png");
    blinded.is_blinded = true;
    assert_eq!(
        ChatMessageDto::from_message(&blinded, false).profile_image_url,
        None
    );
    let mut anon = with_image("https://x/a.png");
    anon.extras = Some(cview_core::models::chat::ChatExtras {
        is_anonymous: Some(true),
        ..Default::default()
    });
    assert_eq!(
        ChatMessageDto::from_message(&anon, false).profile_image_url,
        None
    );
}

#[test]
fn chat_events_are_tagged() {
    let ev = ChatEventDto::from_engine(&ChatEngineEvent::NewMessages(vec![chat_message()]));
    let json = serde_json::to_value(&ev).unwrap();
    assert_eq!(json["type"], "messages");
    assert_eq!(json["recent"], false);
    assert_eq!(json["messages"][0]["nickname"], "시청자");

    let json = serde_json::to_value(ChatEventDto::from_engine(&ChatEngineEvent::StateChanged(
        ChatConnectionState::Reconnecting { attempt: 2 },
    )))
    .unwrap();
    assert_eq!(
        json,
        json!({"type": "state", "state": "reconnecting", "detail": "2"})
    );

    let json = serde_json::to_value(ChatEventDto::from_engine(
        &ChatEngineEvent::MessageBlinded {
            message_id: "m".into(),
            blind_text: None,
        },
    ))
    .unwrap();
    assert_eq!(json["type"], "blinded");
    assert_eq!(json["messageId"], "m");
}

#[test]
fn player_events() {
    let phase = PlayerEventDto::from_stream_event(&StreamEvent::PhaseChanged(StreamPhase::Error(
        "x".into(),
    )))
    .unwrap();
    assert_eq!(
        serde_json::to_value(phase).unwrap(),
        json!({"type": "phase", "phase": "error", "message": "x"})
    );
    let q = PlayerEventDto::from_stream_event(&StreamEvent::QualitySelected(
        StreamQualityInfo::new("1080p", "1920x1080", 8_000_000),
    ))
    .unwrap();
    assert_eq!(serde_json::to_value(q).unwrap()["type"], "quality");
    let b = PlayerEventDto::from_stream_event(&StreamEvent::BufferUpdate(BufferHealth::new(
        0.4, 1.0, false,
    )))
    .unwrap();
    assert_eq!(
        b,
        PlayerEventDto::Buffer {
            level: 0.4,
            healthy: false
        }
    );
    assert!(PlayerEventDto::from_stream_event(&StreamEvent::TimeShiftEnded).is_none());
}

#[test]
fn donation_type_and_subscription_tier() {
    use cview_core::models::chat::{ChatExtras, DonationInfo, MessageType, SubscriptionInfo};
    let mut m = ChatMessage::new("후원자", "고마워요");
    m.r#type = MessageType::Donation;
    let mut d = DonationInfo::new(1000);
    d.r#type = "VIDEO".into();
    m.extras = Some(ChatExtras {
        donation: Some(d),
        ..ChatExtras::default()
    });
    assert_eq!(donation_type(&m).as_deref(), Some("video"));
    m.r#type = MessageType::Normal;
    assert_eq!(donation_type(&m), None, "후원 메시지가 아니면 종류가 없다");

    let mut sub = ChatMessage::new("구독자", "");
    sub.r#type = MessageType::Subscription;
    sub.extras = Some(ChatExtras {
        subscription: Some(SubscriptionInfo {
            months: 3,
            tier_name: Some("티어 2".into()),
        }),
        ..ChatExtras::default()
    });
    let dto = ChatMessageDto::from_message(&sub, false);
    assert_eq!(dto.subscription_tier.as_deref(), Some("티어 2"));
}

#[test]
fn tagged_chat_event_carries_chat_channel_id() {
    let ev = TaggedChatEvent {
        event: ChatEventDto::from_engine(&ChatEngineEvent::StateChanged(
            ChatConnectionState::Reconnecting { attempt: 2 },
        )),
        chat_channel_id: "chat-a".into(),
    };
    assert_eq!(
        serde_json::to_value(&ev).unwrap(),
        json!({"type": "state", "state": "reconnecting", "detail": "2", "chatChannelId": "chat-a"})
    );
    let ev = TaggedChatEvent {
        event: ChatEventDto::Connected,
        chat_channel_id: "chat-b".into(),
    };
    assert_eq!(
        serde_json::to_value(&ev).unwrap(),
        json!({"type": "connected", "chatChannelId": "chat-b"})
    );
}

#[test]
fn session_chat_event_keeps_session_and_event_shape() {
    let ev = TaggedChatEvent {
        event: ChatEventDto::Connected,
        chat_channel_id: "chat-b".into(),
    };
    let payload = SessionChatEvent {
        session: "mc:abc",
        event: &ev,
    };
    // 예전 `json!({ "session": session, "event": event })` 과 같은 모양
    assert_eq!(
        serde_json::to_value(payload).unwrap(),
        json!({"session": "mc:abc", "event": {"type": "connected", "chatChannelId": "chat-b"}})
    );
}

#[test]
fn verified_badge_follows_mark_type() {
    // 종류 없이 verifiedMark 만 참이면(챌린저 제도 이전 응답) 파트너.
    let card = LiveCardDto::from_live_info(&popular_live()).unwrap();
    assert_eq!(card.badge, Some("partner"));

    let challenger = live_info(json!({
        "liveId": 1, "liveTitle": "t", "status": "OPEN", "concurrentUserCount": 1,
        "channel": {"channelId": "c", "channelName": "n", "verifiedMark": true, "verifiedMarkType": "CHALLENGER"}
    }));
    let card = LiveCardDto::from_live_info(&challenger).unwrap();
    assert_eq!(card.badge, Some("challenger"));
    let detail = LiveDetailDto::from_live_info(&challenger, "c");
    assert_eq!(detail.badge, Some("challenger"));
    let json = serde_json::to_value(&card).unwrap();
    assert_eq!(json["badge"], "challenger");

    let plain = live_info(json!({
        "liveId": 2, "liveTitle": "t", "status": "OPEN", "concurrentUserCount": 1,
        "channel": {"channelId": "d", "channelName": "n", "verifiedMark": false, "verifiedMarkType": null}
    }));
    assert_eq!(LiveCardDto::from_live_info(&plain).unwrap().badge, None);
}

#[test]
fn live_detail_dto_carries_chat_policy_flags_and_qualities() {
    let playback = json!({
        "media": [
            { "mediaId": "HLS", "path": "https://x/hls.m3u8", "encodingTrack": [
                { "encodingTrackId": "720p", "videoHeight": 720, "videoFrameRate": "60.0" },
                { "encodingTrackId": "480p", "videoHeight": 480, "videoFrameRate": "30.0" },
                { "encodingTrackId": "1080p", "videoHeight": 1080, "videoFrameRate": "60.0" },
                { "encodingTrackId": "alow.stream" }
            ]},
            { "mediaId": "LLHLS", "path": "https://x/ll.m3u8", "encodingTrack": [
                { "encodingTrackId": "720p", "videoHeight": 720, "videoFrameRate": "60.0" }
            ]}
        ]
    });
    let info = live_info(json!({
        "liveId": 1, "liveTitle": "t", "status": "OPEN", "categoryType": "GAME",
        "chatActive": true, "chatAvailableGroup": "FOLLOWER", "chatAvailableCondition": "REAL_NAME",
        "minFollowerMinute": 10, "krOnlyViewing": true, "clipActive": false, "timeMachineActive": true,
        "livePlaybackJson": playback.to_string()
    }));
    let dto = LiveDetailDto::from_live_info(&info, "c");
    assert_eq!(dto.category_type.as_deref(), Some("GAME"));
    assert_eq!(dto.chat_active, Some(true));
    assert_eq!(dto.chat_available_group.as_deref(), Some("FOLLOWER"));
    assert_eq!(dto.chat_available_condition.as_deref(), Some("REAL_NAME"));
    assert_eq!(dto.min_follower_minute, Some(10));
    assert!(dto.kr_only_viewing);
    assert_eq!(dto.clip_active, Some(false));
    assert!(dto.time_machine_active);
    assert_eq!(dto.qualities, vec!["1080p60", "720p60", "480p"]);
}

#[test]
fn quality_labels_tolerate_missing_or_bad_json() {
    assert!(quality_labels(None).is_empty());
    assert!(quality_labels(Some("")).is_empty());
    assert!(quality_labels(Some("{not json")).is_empty());
}

#[test]
fn live_detail_dto_policy_fields_default_to_unknown() {
    let dto = LiveDetailDto::from_live_info(
        &live_info(json!({ "liveId": 2, "chatAvailableGroup": " " })),
        "c",
    );
    assert_eq!(dto.chat_active, None);
    assert_eq!(dto.chat_available_group, None);
    assert_eq!(dto.min_follower_minute, None);
    assert!(!dto.kr_only_viewing);
    assert!(dto.qualities.is_empty());
}
