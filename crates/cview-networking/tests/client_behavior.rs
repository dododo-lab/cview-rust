//! 클라이언트 동작 테스트 — 실제 네트워크 없이 가짜 전송으로 검증한다.
//!
//! 포팅 출처: `ClientStaleFallbackTests.swift`, `UserStatusMemoTests.swift`, `AuthenticationCacheTests.swift`,
//! `Audit0919NetworkingAPIClientTests.swift`, `LiveDetailFanoutBurstTests.swift`, `RequestPathV2Tests.swift`,
//! `EmoticonNegativeCacheTests.swift`, `FollowingRowsDecodingTests.swift`.

use std::sync::atomic::{AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use cview_core::error::ApiError;
use cview_core::protocols::{AuthTokenProvider, HttpCookie, HttpMethod};
use cview_networking::cache::Clock;
use cview_networking::response::AllLivesStopReason;
use cview_networking::{
    ChzzkApiClient, ChzzkEndpoint, ClientConfig, HttpRequest, HttpResponse, HttpTransport,
    ResponseCache, TransportError, TraversalOptions,
};
use serde_json::{json, Value};

type Handler = dyn Fn(&HttpRequest, usize) -> Result<HttpResponse, TransportError> + Send + Sync;

/// 가짜 전송 — 요청을 기록하고, 지정한 지연 뒤 핸들러의 응답을 돌려준다.
struct MockTransport {
    handler: Box<Handler>,
    delay: Duration,
    requests: Mutex<Vec<HttpRequest>>,
    count: AtomicUsize,
}

impl MockTransport {
    fn new(
        delay: Duration,
        handler: impl Fn(&HttpRequest, usize) -> Result<HttpResponse, TransportError>
            + Send
            + Sync
            + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            handler: Box::new(handler),
            delay,
            requests: Mutex::new(vec![]),
            count: AtomicUsize::new(0),
        })
    }

    fn sends(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }

    fn request(&self, i: usize) -> HttpRequest {
        self.requests.lock().unwrap()[i].clone()
    }
}

#[async_trait]
impl HttpTransport for MockTransport {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
        let n = self.count.fetch_add(1, Ordering::SeqCst);
        self.requests.lock().unwrap().push(request.clone());
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        (self.handler)(&request, n)
    }
}

fn ok(content: Value) -> Result<HttpResponse, TransportError> {
    Ok(HttpResponse::new(
        200,
        json!({"code": 200, "message": null, "content": content}).to_string(),
    ))
}

fn status(code: u16) -> Result<HttpResponse, TransportError> {
    Ok(HttpResponse::new(code, "{}"))
}

#[derive(Default)]
struct MockAuth {
    cookies: Mutex<Option<Vec<HttpCookie>>>,
    token: Mutex<Option<String>>,
    generation: AtomicU64,
}

impl MockAuth {
    fn with_cookies() -> Arc<Self> {
        let a = Self::default();
        *a.cookies.lock().unwrap() = Some(vec![
            HttpCookie::new("NID_AUT", "a"),
            HttpCookie::new("NID_SES", "s"),
        ]);
        Arc::new(a)
    }
}

#[async_trait]
impl AuthTokenProvider for MockAuth {
    async fn cookies(&self) -> Option<Vec<HttpCookie>> {
        self.cookies.lock().unwrap().clone()
    }
    async fn access_token(&self) -> Option<String> {
        self.token.lock().unwrap().clone()
    }
    async fn is_authenticated(&self) -> bool {
        self.cookies.lock().unwrap().is_some()
    }
    async fn authentication_generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
}

fn fake_clock() -> (Clock, Arc<AtomicI64>) {
    let offset = Arc::new(AtomicI64::new(0));
    let base: DateTime<Utc> = Utc::now();
    let o = offset.clone();
    (
        Arc::new(move || base + ChronoDuration::milliseconds(o.load(Ordering::SeqCst))),
        offset,
    )
}

fn client(transport: Arc<MockTransport>, auth: Option<Arc<MockAuth>>) -> ChzzkApiClient {
    let mut config = ClientConfig::new(transport);
    config.auth_provider = auth.map(|a| a as Arc<dyn AuthTokenProvider>);
    ChzzkApiClient::new(config)
}

fn client_with_clock(transport: Arc<MockTransport>, clock: Clock) -> ChzzkApiClient {
    let mut config = ClientConfig::new(transport);
    config.cache = Arc::new(ResponseCache::with_clock(100, clock));
    ChzzkApiClient::new(config)
}

fn channel_json(id: &str) -> Value {
    json!({"channelId": id, "channelName": format!("name-{id}"), "verifiedMark": false, "followerCount": 1})
}

fn live_json(id: i64, title: &str) -> Value {
    json!({"liveId": id, "liveTitle": title, "status": "OPEN", "concurrentUserCount": 10})
}

async fn wait_until(mut cond: impl FnMut() -> bool) {
    for _ in 0..10_000 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    panic!("condition not met");
}

#[tokio::test]
async fn fresh_cache_hit_avoids_second_send_and_headers() {
    let t = MockTransport::new(Duration::ZERO, |_, _| ok(channel_json("c1")));
    let c = client(t.clone(), None);
    let a = c.channel_info("c1").await.unwrap();
    let b = c.channel_info("c1").await.unwrap();
    assert_eq!(a, b);
    assert_eq!(t.sends(), 1);
    let req = t.request(0);
    assert_eq!(
        req.url.as_str(),
        "https://api.chzzk.naver.com/service/v1/channels/c1"
    );
    assert_eq!(req.header("Referer"), Some("https://chzzk.naver.com"));
    assert_eq!(req.header("Origin"), Some("https://chzzk.naver.com"));
    assert!(req.header("Cookie").is_none());
    // bypass 는 읽기만 우회한다.
    c.request_bypass::<cview_core::models::ChannelInfo>(
        &ChzzkEndpoint::ChannelInfo {
            channel_id: "c1".into(),
        },
        true,
    )
    .await
    .unwrap();
    assert_eq!(t.sends(), 2);
}

#[tokio::test]
async fn query_encoding_and_paths() {
    let t = MockTransport::new(Duration::ZERO, |_, _| ok(json!({"size": 0, "data": []})));
    let c = client(t.clone(), None);
    c.search_channels("발로 란트&x", 0, 20).await.unwrap();
    assert_eq!(
        t.request(0).url.as_str(),
        "https://api.chzzk.naver.com/service/v1/search/channels?keyword=%EB%B0%9C%EB%A1%9C%20%EB%9E%80%ED%8A%B8%26x&offset=0&size=20"
    );
}

#[tokio::test(start_paused = true)]
async fn concurrent_gets_share_one_flight() {
    let t = MockTransport::new(Duration::from_millis(200), |_, _| ok(live_json(1, "t")));
    let c = client(t.clone(), None);
    let mut handles = Vec::new();
    for _ in 0..5 {
        let c = c.clone();
        handles.push(tokio::spawn(
            async move { c.live_detail("ch", false).await },
        ));
    }
    for h in handles {
        assert_eq!(h.await.unwrap().unwrap().live_title, "t");
    }
    assert_eq!(t.sends(), 1);
    wait_until(|| c.running_leader_count() == 0).await;
    assert_eq!(c.get_wait_count(), 0);
}

#[tokio::test]
async fn state_changing_methods_are_never_cached() {
    let t = MockTransport::new(Duration::ZERO, |_, _| ok(json!({"ok": true})));
    let auth = MockAuth::with_cookies();
    let c = client(t.clone(), Some(auth));
    let follow = ChzzkEndpoint::Follow {
        channel_id: "c".into(),
    };
    let unfollow = ChzzkEndpoint::Unfollow {
        channel_id: "c".into(),
    };
    c.request::<Value>(&follow).await.unwrap();
    c.request::<Value>(&unfollow).await.unwrap();
    c.request::<Value>(&follow).await.unwrap();
    assert_eq!(t.sends(), 3);
    assert_eq!(t.request(0).method, HttpMethod::Post);
    assert_eq!(t.request(1).method, HttpMethod::Delete);
    assert_eq!(c.cache_stats().entries, 0);
}

#[tokio::test]
async fn soft_auth_401_retries_once_without_cookies() {
    let t = MockTransport::new(Duration::ZERO, |_, n| {
        if n == 0 {
            status(401)
        } else {
            ok(channel_json("c"))
        }
    });
    let c = client(t.clone(), Some(MockAuth::with_cookies()));
    let mut expired = c.subscribe_session_expired();
    c.channel_info("c").await.unwrap();
    assert_eq!(t.sends(), 2);
    assert_eq!(t.request(0).header("Cookie"), Some("NID_AUT=a; NID_SES=s"));
    assert!(t.request(1).header("Cookie").is_none(), "재시도는 비인증");
    assert!(
        expired.try_recv().is_err(),
        "soft-auth 401 은 세션 만료 알림을 보내지 않는다"
    );
}

#[tokio::test]
async fn auth_required_paths() {
    // 자격 증명이 없으면 보내지 않는다.
    let t = MockTransport::new(Duration::ZERO, |_, _| ok(json!({})));
    let c = client(t.clone(), None);
    assert_eq!(
        c.request::<Value>(&ChzzkEndpoint::UserStatus).await,
        Err(ApiError::Unauthorized)
    );
    assert_eq!(t.sends(), 0);

    // 토큰만 있으면 Bearer.
    let auth = Arc::new(MockAuth::default());
    *auth.token.lock().unwrap() = Some("tok".into());
    let t = MockTransport::new(Duration::ZERO, |_, _| ok(json!({})));
    let c = client(t.clone(), Some(auth));
    c.request::<Value>(&ChzzkEndpoint::UserStatus)
        .await
        .unwrap();
    assert_eq!(t.request(0).header("Authorization"), Some("Bearer tok"));

    // requiresAuth 401 → unauthorized + 세션 만료 알림, 재시도 없음.
    let t = MockTransport::new(Duration::ZERO, |_, _| status(401));
    let c = client(t.clone(), Some(MockAuth::with_cookies()));
    let mut expired = c.subscribe_session_expired();
    assert_eq!(
        c.request::<Value>(&ChzzkEndpoint::UserStatus).await,
        Err(ApiError::Unauthorized)
    );
    assert_eq!(t.sends(), 1);
    assert!(expired.try_recv().is_ok());
}

#[tokio::test(start_paused = true)]
async fn rate_limit_uses_separate_budget_and_is_terminal() {
    let t = MockTransport::new(Duration::ZERO, |_, n| {
        if n == 0 {
            Ok(HttpResponse::new(429, "{}").with_header("Retry-After", "2"))
        } else {
            ok(channel_json("c"))
        }
    });
    let c = client(t.clone(), None);
    c.channel_info("c").await.unwrap();
    assert_eq!(t.sends(), 2);

    let t = MockTransport::new(Duration::ZERO, |_, _| {
        Ok(HttpResponse::new(429, "{}").with_header("Retry-After", "7"))
    });
    let c = client(t.clone(), None);
    assert_eq!(
        c.channel_info("c").await,
        Err(ApiError::RateLimited { retry_after: 7.0 })
    );
    assert_eq!(
        t.sends(),
        3,
        "429 전용 예산(2회) 뒤 끝낸다 — normal 예산으로 또 때리지 않는다"
    );
}

#[tokio::test(start_paused = true)]
async fn server_errors_retry_and_client_errors_do_not() {
    let t = MockTransport::new(Duration::ZERO, |_, n| {
        if n < 2 {
            status(503)
        } else {
            ok(channel_json("c"))
        }
    });
    let c = client(t.clone(), None);
    c.channel_info("c").await.unwrap();
    assert_eq!(t.sends(), 3);

    let t = MockTransport::new(Duration::ZERO, |_, _| status(404));
    let c = client(t.clone(), None);
    assert_eq!(c.channel_info("c").await, Err(ApiError::http(404)));
    assert_eq!(t.sends(), 1);

    let t = MockTransport::new(Duration::ZERO, |_, _| {
        Err(TransportError::TimedOut("t".into()))
    });
    let c = client(t.clone(), None);
    assert_eq!(
        c.channel_info("c").await,
        Err(ApiError::NetworkError("Request timed out".into()))
    );
    assert_eq!(t.sends(), 3);

    let t = MockTransport::new(Duration::ZERO, |_, _| {
        Ok(HttpResponse::new(200, "not json"))
    });
    let c = client(t.clone(), None);
    assert!(matches!(
        c.channel_info("c").await,
        Err(ApiError::DecodingFailed(_))
    ));
    assert_eq!(t.sends(), 1);

    let t = MockTransport::new(Duration::ZERO, |_, _| ok(Value::Null));
    let c = client(t.clone(), None);
    assert_eq!(c.channel_info("c").await, Err(ApiError::EmptyContent));
}

#[test]
fn stale_fallback_error_table() {
    assert!(ChzzkApiClient::allows_stale_on_error(
        &ApiError::NetworkError("x".into())
    ));
    assert!(!ChzzkApiClient::allows_stale_on_error(
        &ApiError::NetworkError("API fixture miss".into())
    ));
    assert!(ChzzkApiClient::allows_stale_on_error(
        &ApiError::RateLimited { retry_after: 1.0 }
    ));
    assert!(ChzzkApiClient::allows_stale_on_error(&ApiError::http(503)));
    assert!(!ChzzkApiClient::allows_stale_on_error(&ApiError::http(404)));
    assert!(!ChzzkApiClient::allows_stale_on_error(
        &ApiError::Unauthorized
    ));
    assert!(!ChzzkApiClient::allows_stale_on_error(
        &ApiError::DecodingFailed("x".into())
    ));
    assert!(!ChzzkApiClient::allows_stale_on_error(&ApiError::Cancelled));
    let top = ChzzkEndpoint::top_lives(10);
    assert_eq!(ChzzkApiClient::stale_window(&top, 60.0), 600.0);
    assert_eq!(
        ChzzkApiClient::stale_window(
            &ChzzkEndpoint::LiveDetail {
                channel_id: "c".into()
            },
            60.0
        ),
        60.0
    );
}

#[tokio::test]
async fn stale_while_revalidate_returns_old_value_and_refreshes() {
    let (clock, offset) = fake_clock();
    let t = MockTransport::new(Duration::ZERO, |_, n| {
        ok(json!({"size": 1, "data": [live_json(1, &format!("v{n}"))], "page": null}))
    });
    let c = client_with_clock(t.clone(), clock);
    let first = c.top_lives_with_age(10, None, None, false).await.unwrap();
    assert!(!first.is_stale);
    offset.fetch_add(61_000, Ordering::SeqCst);
    let second = c.top_lives_with_age(10, None, None, false).await.unwrap();
    assert!(second.is_stale);
    assert_eq!(second.value.data[0].live_title, "v0");
    assert_eq!(second.fetched_at, first.fetched_at);
    wait_until(|| t.sends() == 2 && c.stale_revalidation_snapshot().running == 0).await;
    assert_eq!(c.stale_revalidation_snapshot().started, 1);
    let third = c.top_lives_with_age(10, None, None, false).await.unwrap();
    assert!(!third.is_stale);
    assert_eq!(third.value.data[0].live_title, "v1");

    // stale 창(600초)을 넘으면 miss → 네트워크.
    offset.fetch_add(700_000, Ordering::SeqCst);
    c.top_lives(10, None, None, false).await.unwrap();
    assert_eq!(t.sends(), 3);
}

#[tokio::test]
async fn failed_revalidation_keeps_previous_value() {
    let (clock, offset) = fake_clock();
    let t = MockTransport::new(Duration::ZERO, |_, n| {
        if n == 0 {
            ok(json!({"size": 1, "data": [live_json(1, "old")], "page": null}))
        } else {
            status(404)
        }
    });
    let c = client_with_clock(t.clone(), clock);
    c.top_lives(10, None, None, false).await.unwrap();
    offset.fetch_add(61_000, Ordering::SeqCst);
    assert_eq!(
        c.top_lives(10, None, None, false).await.unwrap().data[0].live_title,
        "old"
    );
    wait_until(|| t.sends() == 2 && c.stale_revalidation_snapshot().running == 0).await;
    assert_eq!(
        c.top_lives(10, None, None, false).await.unwrap().data[0].live_title,
        "old"
    );
}

#[tokio::test(start_paused = true)]
async fn account_switch_discards_in_flight_result() {
    let auth = MockAuth::with_cookies();
    let t = MockTransport::new(Duration::from_millis(100), |_, _| ok(channel_json("c")));
    let c = client(t.clone(), Some(auth.clone()));
    let c2 = c.clone();
    let h = tokio::spawn(async move { c2.channel_info("c").await });
    wait_until(|| t.sends() == 1).await;
    auth.generation.fetch_add(1, Ordering::SeqCst);
    assert_eq!(h.await.unwrap(), Err(ApiError::Cancelled));
    // 새 세대에서는 이전 계정의 캐시 키에 닿지 않는다.
    c.channel_info("c").await.unwrap();
    assert_eq!(t.sends(), 2);
}

#[tokio::test]
async fn invalidation_changes_cache_namespace() {
    let t = MockTransport::new(Duration::ZERO, |_, _| ok(channel_json("c")));
    let c = client(t.clone(), None);
    let ep = ChzzkEndpoint::ChannelInfo {
        channel_id: "c".into(),
    };
    let before = c
        .response_cache_key_for::<cview_core::models::ChannelInfo>(&ep)
        .await;
    assert!(before.starts_with("0:0:GET:"));
    assert!(before.ends_with(":/service/v1/channels/c?"));
    c.channel_info("c").await.unwrap();
    c.invalidate_authentication_cache().await;
    assert_ne!(
        c.response_cache_key_for::<cview_core::models::ChannelInfo>(&ep)
            .await,
        before
    );
    c.channel_info("c").await.unwrap();
    assert_eq!(t.sends(), 2);
}

#[tokio::test(start_paused = true)]
async fn sole_caller_leaving_stops_retries() {
    let t = MockTransport::new(Duration::ZERO, |_, _| status(503));
    let c = client(t.clone(), None);
    let c2 = c.clone();
    let h = tokio::spawn(async move { c2.channel_info("c").await });
    wait_until(|| t.sends() == 1).await;
    h.abort();
    let _ = h.await;
    wait_until(|| c.running_leader_count() == 0).await;
    tokio::time::sleep(Duration::from_secs(30)).await;
    assert_eq!(t.sends(), 1, "기다리는 호출자가 없으면 공유 작업을 멈춘다");
    assert_eq!(c.get_wait_count(), 0);
}

#[tokio::test(start_paused = true)]
async fn user_status_memo_and_single_flight() {
    let auth = MockAuth::with_cookies();
    let t = MockTransport::new(Duration::from_millis(50), |_, _| {
        ok(json!({"userIdHash": "hash", "nickname": "nick", "loggedIn": true}))
    });
    let c = client(t.clone(), Some(auth.clone()));
    let mut handles = Vec::new();
    for _ in 0..9 {
        let c = c.clone();
        handles.push(tokio::spawn(async move { c.user_status().await }));
    }
    for h in handles {
        assert_eq!(
            h.await.unwrap().unwrap().user_id_hash.as_deref(),
            Some("hash")
        );
    }
    assert_eq!(t.sends(), 1);
    assert!(c.has_user_status_memo());
    c.user_status().await.unwrap();
    assert_eq!(t.sends(), 1);
    assert_eq!(
        t.request(0).url.as_str(),
        "https://comm-api.game.naver.com/nng_main/v1/user/getUserStatus"
    );

    auth.generation.fetch_add(1, Ordering::SeqCst);
    c.user_status().await.unwrap();
    assert_eq!(t.sends(), 2);
    wait_until(|| c.user_status_loads_running() == 0).await;
}

#[tokio::test(start_paused = true)]
async fn user_status_first_caller_cancel_does_not_break_others() {
    let t = MockTransport::new(Duration::from_millis(100), |_, _| {
        ok(json!({"userIdHash": "h"}))
    });
    let c = client(t.clone(), Some(MockAuth::with_cookies()));
    let c1 = c.clone();
    let first = tokio::spawn(async move { c1.user_status().await });
    wait_until(|| t.sends() == 1).await;
    let c2 = c.clone();
    let second = tokio::spawn(async move { c2.user_status().await });
    wait_until(|| c.user_status_waiter_count() == 2).await;
    first.abort();
    assert_eq!(
        second.await.unwrap().unwrap().user_id_hash.as_deref(),
        Some("h")
    );
    assert_eq!(t.sends(), 1);
}

#[tokio::test]
async fn user_status_without_hash_is_not_memoized_and_failures_are_not() {
    let t = MockTransport::new(Duration::ZERO, |_, n| {
        if n == 0 {
            status(500)
        } else {
            ok(json!({"loggedIn": false}))
        }
    });
    let c = client(t.clone(), None);
    assert_eq!(c.user_status().await, Err(ApiError::http(500)));
    assert!(c.user_status().await.unwrap().user_id_hash.is_none());
    assert!(!c.has_user_status_memo());
    c.user_status().await.unwrap();
    assert_eq!(t.sends(), 3);
}

#[tokio::test(start_paused = true)]
async fn invalidation_cancels_user_status_waiters() {
    let t = MockTransport::new(Duration::from_millis(100), |_, _| {
        ok(json!({"userIdHash": "h"}))
    });
    let c = client(t.clone(), Some(MockAuth::with_cookies()));
    let c1 = c.clone();
    let waiter = tokio::spawn(async move { c1.user_status().await });
    wait_until(|| t.sends() == 1).await;
    c.invalidate_authentication_cache().await;
    assert_eq!(waiter.await.unwrap(), Err(ApiError::Cancelled));
    wait_until(|| c.user_status_loads_running() == 0).await;
    assert!(!c.has_user_status_memo());
}

#[tokio::test]
async fn chat_access_token_uses_game_api() {
    let t = MockTransport::new(Duration::ZERO, |_, _| {
        ok(json!({"accessToken": "a", "extraToken": "e"}))
    });
    let c = client(t.clone(), Some(MockAuth::with_cookies()));
    let token: Value = c.chat_access_token("chat1").await.unwrap();
    assert_eq!(token["accessToken"], "a");
    let req = t.request(0);
    assert_eq!(
        req.url.as_str(),
        "https://comm-api.game.naver.com/nng_main/v1/chats/access-token?channelId=chat1&chatType=STREAMING"
    );
    assert_eq!(req.header("Referer"), Some("https://chzzk.naver.com/"));
    assert!(req.header("Cookie").is_some());
}

#[tokio::test]
async fn emoticon_negative_cache_per_generation() {
    let auth = MockAuth::with_cookies();
    let t = MockTransport::new(Duration::ZERO, |_, _| status(404));
    let c = client(t.clone(), Some(auth.clone()));
    assert_eq!(
        c.emoticon_deploy::<Value>("c").await,
        Err(ApiError::http(404))
    );
    assert_eq!(
        c.emoticon_deploy::<Value>("c").await,
        Err(ApiError::http(404))
    );
    assert_eq!(t.sends(), 1, "404 뒤에는 같은 family 를 건너뛴다");
    assert!(
        c.emoticon_family_unsupported(cview_networking::emoticon_family::EMOTICON_DEPLOY)
            .await
    );
    assert!(
        !c.emoticon_family_unsupported(cview_networking::emoticon_family::USER_EMOTICONS)
            .await
    );
    auth.generation.fetch_add(1, Ordering::SeqCst);
    assert!(
        !c.emoticon_family_unsupported(cview_networking::emoticon_family::EMOTICON_DEPLOY)
            .await
    );
    let _ = c.emoticon_deploy::<Value>("c").await;
    assert_eq!(t.sends(), 2);
}

fn following_item(id: &str, live: bool) -> Value {
    json!({
        "channel": {"channelId": id, "channelName": format!("n-{id}")},
        "streamer": {"openLive": live},
        "liveInfo": {"liveTitle": format!("list-{id}"), "concurrentUserCount": 3}
    })
}

#[tokio::test]
async fn following_rows_paginate_by_total_count_and_dedup() {
    let t = MockTransport::new(Duration::ZERO, |req, _| {
        let page = req
            .url
            .query_pairs()
            .find(|(k, _)| k == "page")
            .map(|(_, v)| v.to_string())
            .unwrap();
        match page.as_str() {
            "0" => ok(json!({"totalCount": 3, "followingList": [following_item("a", true)]})),
            "1" => ok(
                json!({"totalCount": 3, "followingList": [following_item("a", true), following_item("b", false)]}),
            ),
            "2" => ok(json!({"totalCount": 3, "followingList": [following_item("c", true)]})),
            _ => ok(json!({"followingList": []})),
        }
    });
    let c = client(t.clone(), Some(MockAuth::with_cookies()));
    let page = c
        .fetch_following_rows_with_page_size(false, 2)
        .await
        .unwrap();
    let ids: Vec<_> = page.rows.iter().map(|r| r.channel_id.as_str()).collect();
    assert_eq!(ids, vec!["a", "b", "c"]);
    assert_eq!(page.total_count, Some(3));
    assert!(page.carries_live_info);
    assert_eq!(
        t.sends(),
        3,
        "totalCount 에 닿으면 멈춘다(받은 개수가 size 보다 작아도 계속)"
    );
}

#[tokio::test(start_paused = true)]
async fn following_channels_keep_api_order_with_details() {
    let t = MockTransport::new(Duration::from_millis(10), |req, _| {
        let path = req.url.path().to_string();
        if path.ends_with("/followings") {
            ok(json!({"totalCount": 4, "followingList": [
                following_item("a", true), following_item("off", false),
                following_item("b", true), following_item("c", true)
            ]}))
        } else if path.contains("/channels/b/") {
            status(404)
        } else if path.contains("/channels/c/") {
            ok(json!({"liveId": 3, "liveTitle": "ended", "status": "CLOSE"}))
        } else {
            ok(live_json(1, "detail-a"))
        }
    });
    let c = client(t.clone(), Some(MockAuth::with_cookies()));
    let items = c.fetch_following_channels(false, 2).await.unwrap();
    let summary: Vec<_> = items
        .iter()
        .map(|i| (i.channel_id.as_str(), i.live_title.as_str(), i.is_live))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("a", "detail-a", true),
            ("b", "list-b", true),
            ("c", "ended", false),
            ("off", "", false)
        ]
    );
}

#[tokio::test]
async fn live_catalog_traversal_requests_latest_and_verifies() {
    let t = MockTransport::new(Duration::ZERO, |req, _| {
        let has_cursor = req.url.query_pairs().any(|(k, _)| k == "liveId");
        if has_cursor {
            ok(json!({"size": 1, "data": [live_json(1, "b")], "page": {"next": null}}))
        } else {
            ok(
                json!({"size": 2, "data": [live_json(3, "a"), live_json(2, "a2")],
                      "page": {"next": {"concurrentUserCount": 5, "liveId": 2}}}),
            )
        }
    });
    let c = client(t.clone(), None);
    let r = c
        .live_catalog_traversal(
            TraversalOptions {
                batch_size: 100,
                ..Default::default()
            },
            || async { false },
            |_| {},
        )
        .await;
    assert_eq!(r.stop_reason, AllLivesStopReason::ReachedEnd);
    assert_eq!(r.exact_live_count(), Some(3));
    assert_eq!(t.request(0).url.query(), Some("size=50&sortType=LATEST"));
    assert_eq!(
        t.request(1).url.query(),
        Some("size=50&sortType=LATEST&concurrentUserCount=5&liveId=2")
    );
    assert_eq!(c.cache_stats().entries, 0, "전수 수집은 캐시에 넣지 않는다");
}

#[tokio::test]
async fn retry_count_and_network_settings_apply() {
    let t = MockTransport::new(Duration::ZERO, |_, _| {
        Err(TransportError::Other("boom".into()))
    });
    let c = client(t.clone(), None);
    c.update_retry_count(1);
    assert!(c.channel_info("c").await.is_err());
    assert_eq!(t.sends(), 1);
    let settings = cview_core::models::settings::NetworkSettings::default();
    c.update_network_settings(&settings);
    let snap = c.request_gate().snapshot();
    assert_eq!(snap.starts_per_second, settings.effective_api_rate_limit());
    assert_eq!(snap.concurrent_limit, settings.effective_api_concurrency());
}
