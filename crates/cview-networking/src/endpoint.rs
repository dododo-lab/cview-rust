//! `Sources/CViewNetworking/APIEndpoint.swift` 포팅 — 치지직 API 엔드포인트 정의.
//!
//! 경로·쿼리 파라미터·메서드·캐시 정책은 원본과 정확히 같다.

use cview_core::protocols::{CachePolicy, EndpointProtocol, HttpMethod, QueryItem};

/// `/service/v1/lives` 정렬 방식. 전체 통계용 cursor sweep 만 `Latest` 를 명시한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TopLivesSortType {
    Popular,
    Latest,
}

impl TopLivesSortType {
    pub fn raw_value(self) -> &'static str {
        match self {
            TopLivesSortType::Popular => "POPULAR",
            TopLivesSortType::Latest => "LATEST",
        }
    }
}

/// 치지직 API 엔드포인트.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChzzkEndpoint {
    // Channel
    ChannelInfo {
        channel_id: String,
    },
    ChannelViewerInfo {
        channel_id: String,
    },
    LiveDetail {
        channel_id: String,
    },
    TimeMachinePlayback {
        channel_id: String,
    },
    LiveStatus {
        channel_id: String,
    },
    // Following
    Following {
        size: i64,
        page: i64,
    },
    Follow {
        channel_id: String,
    },
    Unfollow {
        channel_id: String,
    },
    // Search
    SearchChannel {
        keyword: String,
        offset: i64,
        size: i64,
    },
    SearchLive {
        keyword: String,
        offset: i64,
        size: i64,
    },
    SearchVideo {
        keyword: String,
        offset: i64,
        size: i64,
    },
    // Chat
    ChatAccessToken {
        chat_channel_id: String,
    },
    // VOD
    VodList {
        channel_id: String,
        page: i64,
        size: i64,
    },
    VodDetail {
        video_no: i64,
    },
    // Clip
    ClipList {
        channel_id: String,
        page: i64,
        size: i64,
    },
    ClipDetail {
        clip_uid: String,
    },
    ClipInkey {
        clip_uid: String,
    },
    HomePopularClips {
        filter_type: String,
        order_type: String,
    },
    // Top/Trending
    TopLives {
        size: i64,
        concurrent_user_count: Option<i64>,
        live_id: Option<i64>,
        sort_type: Option<TopLivesSortType>,
    },
    // User
    UserStatus,
    // Emoticon
    EmoticonDeploy {
        channel_id: String,
    },
    EmoticonPack {
        pack_id: String,
    },
    /// 사용자가 쓸 수 있는 전체 이모티콘 팩 (인증 필요)
    UserEmoticons,
    /// 치지직 기본 이모티콘 팩 목록 (인증 불필요)
    BasicEmoticonPacks,
    // Donation
    WeeklyDonationRanking {
        channel_id: String,
        rank_count: i64,
        with_my_rank: bool,
    },
}

impl ChzzkEndpoint {
    /// `TopLives` 의 기본 형태(크기만 지정).
    pub fn top_lives(size: i64) -> Self {
        ChzzkEndpoint::TopLives {
            size,
            concurrent_user_count: None,
            live_id: None,
            sort_type: None,
        }
    }
}

fn q(name: &str, value: impl ToString) -> QueryItem {
    QueryItem::new(name, value.to_string())
}

impl EndpointProtocol for ChzzkEndpoint {
    fn path(&self) -> String {
        use ChzzkEndpoint::*;
        match self {
            ChannelInfo { channel_id } => format!("/service/v1/channels/{channel_id}"),
            ChannelViewerInfo { channel_id } => {
                format!("/service/v1.1/channels/{channel_id}/my-info")
            }
            LiveDetail { channel_id } => format!("/service/v3/channels/{channel_id}/live-detail"),
            TimeMachinePlayback { channel_id } => {
                format!("/service/v1.1/channels/{channel_id}/live-playback-json")
            }
            // [WatchCount 2026-07-17] v1 은 폐기(9004) — v3 사용.
            LiveStatus { channel_id } => format!("/polling/v3/channels/{channel_id}/live-status"),
            Following { .. } => "/service/v1/channels/followings".into(),
            Follow { channel_id } | Unfollow { channel_id } => {
                format!("/service/v1/channels/{channel_id}/follow")
            }
            SearchChannel { .. } => "/service/v1/search/channels".into(),
            SearchLive { .. } => "/service/v1/search/lives".into(),
            SearchVideo { .. } => "/service/v1/search/videos".into(),
            ChatAccessToken { chat_channel_id } => {
                format!("/polling/v3/channels/{chat_channel_id}/access-token")
            }
            VodList { channel_id, .. } => format!("/service/v1/channels/{channel_id}/videos"),
            VodDetail { video_no } => format!("/service/v3/videos/{video_no}"),
            ClipList { channel_id, .. } => format!("/service/v1/channels/{channel_id}/clips"),
            ClipDetail { clip_uid } => format!("/service/v1/clips/{clip_uid}/detail"),
            ClipInkey { clip_uid } => format!("/service/v1/clips/{clip_uid}/inkey"),
            HomePopularClips { .. } => "/service/v1/home/recommended/clips".into(),
            TopLives { .. } => "/service/v1/lives".into(),
            UserStatus => "/service/v1/users/me".into(),
            EmoticonDeploy { channel_id } => {
                format!("/service/v1/channels/{channel_id}/emoticon-deploy")
            }
            EmoticonPack { pack_id } => format!("/service/v1/emoticon-packs/{pack_id}"),
            UserEmoticons => "/service/v2/emoticons".into(),
            BasicEmoticonPacks => "/service/v1/emoticons".into(),
            WeeklyDonationRanking { channel_id, .. } => {
                format!("/commercial/v1/channels/{channel_id}/donation/rank/weekly")
            }
        }
    }

    fn method(&self) -> HttpMethod {
        match self {
            ChzzkEndpoint::Follow { .. } | ChzzkEndpoint::ClipInkey { .. } => HttpMethod::Post,
            ChzzkEndpoint::Unfollow { .. } => HttpMethod::Delete,
            _ => HttpMethod::Get,
        }
    }

    fn body(&self) -> Option<Vec<u8>> {
        match self {
            ChzzkEndpoint::ClipInkey { .. } => Some(b"{}".to_vec()),
            _ => None,
        }
    }

    fn query_items(&self) -> Option<Vec<QueryItem>> {
        use ChzzkEndpoint::*;
        match self {
            // 타임머신 지원 플레이어임을 tm=true 로 명시.
            TimeMachinePlayback { .. } => Some(vec![q("tm", "true")]),
            Following { size, page } => Some(vec![
                q("size", size),
                q("sortType", "FOLLOW"),
                q("page", page),
            ]),
            SearchChannel {
                keyword,
                offset,
                size,
            }
            | SearchLive {
                keyword,
                offset,
                size,
            }
            | SearchVideo {
                keyword,
                offset,
                size,
            } => Some(vec![
                q("keyword", keyword),
                q("offset", offset),
                q("size", size),
            ]),
            VodList { page, size, .. } => Some(vec![
                q("page", page),
                q("size", size),
                q("sortType", "LATEST"),
            ]),
            ClipList { page, size, .. } => Some(vec![q("page", page), q("size", size)]),
            ClipDetail { .. } => Some(
                [
                    "COMMENT",
                    "PRIVATE_USER_BLOCK",
                    "PENALTY",
                    "MAKER_CHANNEL",
                    "OWNER_CHANNEL",
                ]
                .into_iter()
                .map(|v| q("optionalProperties", v))
                .collect(),
            ),
            HomePopularClips {
                filter_type,
                order_type,
            } => Some(vec![
                q("filterType", filter_type),
                q("orderType", order_type),
                q("optionalProperties", "OWNER_CHANNEL"),
                q("optionalProperties", "MAKER_CHANNEL"),
            ]),
            TopLives {
                size,
                concurrent_user_count,
                live_id,
                sort_type,
            } => {
                let mut items = vec![q("size", size)];
                if let Some(s) = sort_type {
                    items.push(q("sortType", s.raw_value()));
                }
                if let Some(c) = concurrent_user_count {
                    items.push(q("concurrentUserCount", c));
                }
                if let Some(l) = live_id {
                    items.push(q("liveId", l));
                }
                Some(items)
            }
            WeeklyDonationRanking {
                channel_id,
                rank_count,
                with_my_rank,
            } => Some(vec![
                q("channelId", channel_id),
                q("withMyRank", if *with_my_rank { "true" } else { "false" }),
                q("rankCount", rank_count),
            ]),
            _ => None,
        }
    }

    fn diagnostic_name(&self) -> &'static str {
        use ChzzkEndpoint::*;
        match self {
            ChannelInfo { .. } => "channelInfo",
            ChannelViewerInfo { .. } => "channelViewerInfo",
            LiveDetail { .. } => "liveDetail",
            TimeMachinePlayback { .. } => "timeMachinePlayback",
            LiveStatus { .. } => "liveStatus",
            Following { .. } => "following",
            Follow { .. } => "follow",
            Unfollow { .. } => "unfollow",
            SearchChannel { .. } => "searchChannel",
            SearchLive { .. } => "searchLive",
            SearchVideo { .. } => "searchVideo",
            ChatAccessToken { .. } => "chatAccessToken",
            VodList { .. } => "vodList",
            VodDetail { .. } => "vodDetail",
            ClipList { .. } => "clipList",
            ClipDetail { .. } => "clipDetail",
            ClipInkey { .. } => "clipInkey",
            HomePopularClips { .. } => "homePopularClips",
            TopLives { .. } => "topLives",
            UserStatus => "userStatus",
            EmoticonDeploy { .. } => "emoticonDeploy",
            EmoticonPack { .. } => "emoticonPack",
            UserEmoticons => "userEmoticons",
            BasicEmoticonPacks => "basicEmoticonPacks",
            WeeklyDonationRanking { .. } => "weeklyDonationRanking",
        }
    }

    fn requires_auth(&self) -> bool {
        // emoticonDeploy: 공개 이모티콘은 인증 불필요(쿠키 있으면 softAuth 로 구독 팩 포함).
        // chatAccessToken: 인증 없이도 READ 토큰 발급 가능.
        matches!(
            self,
            ChzzkEndpoint::Following { .. }
                | ChzzkEndpoint::UserStatus
                | ChzzkEndpoint::Follow { .. }
                | ChzzkEndpoint::Unfollow { .. }
                | ChzzkEndpoint::ClipInkey { .. }
                | ChzzkEndpoint::UserEmoticons
        )
    }

    fn cache_policy(&self) -> CachePolicy {
        use ChzzkEndpoint::*;
        match self {
            ChannelViewerInfo { .. }
            | TimeMachinePlayback { .. }
            | LiveStatus { .. }
            | ChatAccessToken { .. } => CachePolicy::ReloadIgnoringCache,
            ChannelInfo { .. } => CachePolicy::ReturnCacheElseLoad { ttl: 300.0 },
            // 랭킹은 신선도가 높아야 하므로 짧은 TTL.
            WeeklyDonationRanking { .. } => CachePolicy::ReturnCacheElseLoad { ttl: 30.0 },
            _ => CachePolicy::ReturnCacheElseLoad { ttl: 60.0 },
        }
    }

    /// 원천별 stale 기한 표(초). ⚠ 새 엔드포인트는 기본(`None`)이 안전하다.
    fn stale_ttl(&self) -> Option<f64> {
        match self {
            ChzzkEndpoint::TopLives { .. } | ChzzkEndpoint::Following { .. } => Some(600.0),
            ChzzkEndpoint::ChannelInfo { .. } => Some(3600.0),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    //! `CViewNetworkingTests.swift` · `RequestPathV2Tests.swift` 의 엔드포인트 계약 포팅.
    use super::*;

    fn query_string(e: &ChzzkEndpoint) -> String {
        e.query_items()
            .unwrap_or_default()
            .into_iter()
            .map(|i| format!("{}={}", i.name, i.value.unwrap_or_default()))
            .collect::<Vec<_>>()
            .join("&")
    }

    #[test]
    fn paths() {
        let id = "abc".to_string();
        assert_eq!(
            ChzzkEndpoint::ChannelInfo {
                channel_id: id.clone()
            }
            .path(),
            "/service/v1/channels/abc"
        );
        assert_eq!(
            ChzzkEndpoint::LiveDetail {
                channel_id: id.clone()
            }
            .path(),
            "/service/v3/channels/abc/live-detail"
        );
        assert_eq!(
            ChzzkEndpoint::LiveStatus {
                channel_id: id.clone()
            }
            .path(),
            "/polling/v3/channels/abc/live-status"
        );
        assert_eq!(
            ChzzkEndpoint::ChatAccessToken {
                chat_channel_id: id.clone()
            }
            .path(),
            "/polling/v3/channels/abc/access-token"
        );
        assert_eq!(
            ChzzkEndpoint::VodDetail { video_no: 42 }.path(),
            "/service/v3/videos/42"
        );
        assert_eq!(ChzzkEndpoint::UserStatus.path(), "/service/v1/users/me");
        assert_eq!(ChzzkEndpoint::UserEmoticons.path(), "/service/v2/emoticons");
        assert_eq!(
            ChzzkEndpoint::WeeklyDonationRanking {
                channel_id: id,
                rank_count: 10,
                with_my_rank: true
            }
            .path(),
            "/commercial/v1/channels/abc/donation/rank/weekly"
        );
    }

    #[test]
    fn methods_and_bodies() {
        assert_eq!(
            ChzzkEndpoint::Follow {
                channel_id: "a".into()
            }
            .method(),
            HttpMethod::Post
        );
        assert_eq!(
            ChzzkEndpoint::Unfollow {
                channel_id: "a".into()
            }
            .method(),
            HttpMethod::Delete
        );
        let inkey = ChzzkEndpoint::ClipInkey {
            clip_uid: "u".into(),
        };
        assert_eq!(inkey.method(), HttpMethod::Post);
        assert_eq!(inkey.body().as_deref(), Some(&b"{}"[..]));
        assert!(inkey.requires_auth());
        assert_eq!(ChzzkEndpoint::UserStatus.method(), HttpMethod::Get);
        assert!(ChzzkEndpoint::UserStatus.body().is_none());
    }

    #[test]
    fn query_items() {
        assert_eq!(
            query_string(&ChzzkEndpoint::Following { size: 50, page: 2 }),
            "size=50&sortType=FOLLOW&page=2"
        );
        assert_eq!(
            query_string(&ChzzkEndpoint::SearchLive {
                keyword: "롤".into(),
                offset: 0,
                size: 20
            }),
            "keyword=롤&offset=0&size=20"
        );
        assert_eq!(
            query_string(&ChzzkEndpoint::VodList {
                channel_id: "c".into(),
                page: 0,
                size: 30
            }),
            "page=0&size=30&sortType=LATEST"
        );
        assert_eq!(
            query_string(&ChzzkEndpoint::TopLives {
                size: 50,
                concurrent_user_count: Some(10),
                live_id: Some(99),
                sort_type: Some(TopLivesSortType::Latest),
            }),
            "size=50&sortType=LATEST&concurrentUserCount=10&liveId=99"
        );
        assert_eq!(query_string(&ChzzkEndpoint::top_lives(20)), "size=20");
        assert_eq!(
            query_string(&ChzzkEndpoint::ClipDetail {
                clip_uid: "x".into()
            })
            .matches("optionalProperties")
            .count(),
            5
        );
        assert_eq!(
            query_string(&ChzzkEndpoint::WeeklyDonationRanking {
                channel_id: "c".into(),
                rank_count: 3,
                with_my_rank: false
            }),
            "channelId=c&withMyRank=false&rankCount=3"
        );
        assert_eq!(
            query_string(&ChzzkEndpoint::TimeMachinePlayback {
                channel_id: "c".into()
            }),
            "tm=true"
        );
        assert!(ChzzkEndpoint::UserStatus.query_items().is_none());
    }

    #[test]
    fn cache_and_auth_policy() {
        assert_eq!(
            ChzzkEndpoint::ChannelInfo {
                channel_id: "c".into()
            }
            .cache_policy(),
            CachePolicy::ReturnCacheElseLoad { ttl: 300.0 }
        );
        assert_eq!(
            ChzzkEndpoint::LiveStatus {
                channel_id: "c".into()
            }
            .cache_policy(),
            CachePolicy::ReloadIgnoringCache
        );
        assert_eq!(
            ChzzkEndpoint::UserStatus.cache_policy(),
            CachePolicy::ReturnCacheElseLoad { ttl: 60.0 }
        );
        assert_eq!(ChzzkEndpoint::top_lives(1).stale_ttl(), Some(600.0));
        assert_eq!(
            ChzzkEndpoint::ChannelInfo {
                channel_id: "c".into()
            }
            .stale_ttl(),
            Some(3600.0)
        );
        assert_eq!(
            ChzzkEndpoint::LiveDetail {
                channel_id: "c".into()
            }
            .stale_ttl(),
            None
        );
        assert!(!ChzzkEndpoint::ChatAccessToken {
            chat_channel_id: "c".into()
        }
        .requires_auth());
        assert!(!ChzzkEndpoint::EmoticonDeploy {
            channel_id: "c".into()
        }
        .requires_auth());
        assert!(ChzzkEndpoint::UserEmoticons.requires_auth());
        assert_eq!(ChzzkEndpoint::top_lives(1).diagnostic_name(), "topLives");
    }
}
