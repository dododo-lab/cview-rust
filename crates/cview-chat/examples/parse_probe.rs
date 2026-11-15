//! 채팅 프레임 파싱 시간 재기(성능 측정용, 앱에는 들어가지 않는다) — 실제 치지직 93101 모양(프로필·extras 가
//! 문자열로 든 JSON)의 프레임을 메시지 1개·10개로 만들어 메시지당 시간을 본다.
//!
//! `cargo run --release -p cview-chat --example parse_probe`
use cview_chat::parser::ChatMessageParser;
use std::time::Instant;

fn frame(n: usize) -> String {
    let one = |i: usize| {
        let profile = serde_json::json!({"userIdHash": format!("hash{i:032}"), "nickname": format!("시청자{i}"), "profileImageUrl": "https://nng-phinf.pstatic.net/MjAyMzEy/profile.png", "userRoleCode": "common_user", "badge": null, "title": null, "verifiedMark": false, "activityBadges": [{"badgeNo": 1, "badgeId": "subscription", "imageUrl": "https://ssl.pstatic.net/static/nng/glive/badge/sub.png", "activated": true}], "streamingProperty": {"realTimeDonationRanking": {"badge": null}, "subscription": {"accumulativeMonth": 3, "tier": 1, "badge": {"imageUrl": "https://x/sub.png"}}, "nicknameColor": {"colorCode": "CC000"}}});
        let extras = serde_json::json!({"chatType": "STREAMING", "osType": "PC", "extraToken": "abcdefghijklmnopqrstuvwxyz0123456789", "streamingChannelId": "abcdef0123456789", "emojis": if i.is_multiple_of(5) { serde_json::json!({"d_1": "https://ssl.pstatic.net/static/nng/glive/icon/d_1.png"}) } else { serde_json::json!({}) }});
        serde_json::json!({"svcid": "game", "cid": "N1abcd", "mbrCnt": 1234, "uid": format!("hash{i:032}"), "profile": profile.to_string(), "msg": format!("채팅 메시지 {i} ㅋㅋㅋㅋ 가나다라 {{:d_1:}}"), "msgTypeCode": 1, "msgStatusType": "NORMAL", "extras": extras.to_string(), "ctime": 1731700000000i64 + i as i64, "utime": 1731700000000i64 + i as i64, "msgTid": null, "msgTime": 1731700000000i64 + i as i64})
    };
    serde_json::json!({"svcid": "game", "ver": "1", "cmd": 93101, "tid": null, "cid": "N1abcd", "bdy": (0..n).map(one).collect::<Vec<_>>()}).to_string()
}

fn main() {
    let p = ChatMessageParser::new();
    for n in [1usize, 10] {
        let f = frame(n);
        let iters = 20000 / n;
        for _ in 0..200 {
            std::hint::black_box(p.parse_events(&f).unwrap());
        }
        let mut best = f64::MAX;
        for _ in 0..5 {
            let s = Instant::now();
            for _ in 0..iters {
                std::hint::black_box(p.parse_events(&f).unwrap());
            }
            best = best.min(s.elapsed().as_secs_f64() * 1e6 / (iters * n) as f64);
        }
        println!("frame of {n:2} msgs ({} B): {best:.2} µs/msg", f.len());
    }
}
