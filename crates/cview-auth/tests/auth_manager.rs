//! 인증 관리자 테스트.
//!
//! 원본:
//! - `Tests/CViewAuthTests/Audit0919authTokenRefreshBackoffTests.swift` (갱신 실패 백오프)
//! - `Tests/CViewAuthTests/AuthSessionRaceTests.swift` (세대 경합)
//! - `Tests/CViewAuthTests/StartupTokenRefreshTests.swift` (부팅 갱신)
//! - `Tests/CViewAuthTests/PrecisionReview0926AuthStartupTests.swift` (오프라인 부팅 뒤 재동기화)

mod common;

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{
    fixture, fixture_config, keychain, keychain_with, logged_in, profile_fn, token_fn,
    unclassified, wait_until, Fixture, RecordingStore,
};
use cview_auth::manager::{OFFLINE_REFRESH_RETRY, REFRESH_BACKOFF_INITIAL};
use cview_auth::{
    AuthServiceError, KeychainService, OAuthConfig, OAuthCredentialStore, OAuthTokens,
    OAuthUserProfile, StoredCookie, TOKEN_KEYCHAIN_KEY,
};
use cview_core::protocols::{AuthState, AuthTokenProvider};
use cview_networking::TransportError;
use tokio::sync::oneshot;
use tokio::time::Instant;

// MARK: - Audit 0919: 갱신 실패 백오프

/// code 교환은 즉시 만료된 토큰, refresh 는 항상 (미분류로) 실패.
#[derive(Default)]
struct ExpiringEndpoint {
    exchanges: AtomicUsize,
    refreshes: AtomicUsize,
}

impl ExpiringEndpoint {
    fn handle(&self, body: &BTreeMap<String, String>) -> Result<OAuthTokens, AuthServiceError> {
        if let Some(code) = body.get("code") {
            self.exchanges.fetch_add(1, Ordering::SeqCst);
            return Ok(OAuthTokens::bearer(
                code.clone(),
                Some(format!("refresh-{code}")),
                -1,
            ));
        }
        self.refreshes.fetch_add(1, Ordering::SeqCst);
        Err(unclassified())
    }
}

fn expiring_fixture(endpoint: Arc<ExpiringEndpoint>) -> Fixture {
    fixture(
        fixture_config(),
        keychain(),
        token_fn(move |body| {
            let e = endpoint.clone();
            async move { e.handle(&body) }
        }),
        None,
        None,
    )
}

#[tokio::test]
async fn failed_refresh_does_not_retry_on_every_request() {
    let endpoint = Arc::new(ExpiringEndpoint::default());
    let f = expiring_fixture(endpoint.clone());
    f.manager
        .handle_oauth_login_success("expired", "expired")
        .await
        .unwrap();
    assert_eq!(f.manager.oauth_tokens().unwrap().access_token, "expired");

    for _ in 0..6 {
        assert_eq!(f.manager.access_token().await, None);
    }
    assert_eq!(endpoint.refreshes.load(Ordering::SeqCst), 1);
    assert_eq!(endpoint.exchanges.load(Ordering::SeqCst), 1);
    assert_eq!(f.manager.auth_state(), AuthState::Expired);
    // 백오프는 토큰을 폐기하지 않는다.
    assert_eq!(f.manager.oauth_tokens().unwrap().access_token, "expired");
    f.manager.logout().await;
}

#[tokio::test]
async fn new_login_clears_backoff() {
    let endpoint = Arc::new(ExpiringEndpoint::default());
    let f = expiring_fixture(endpoint.clone());
    f.manager
        .handle_oauth_login_success("first", "first")
        .await
        .unwrap();
    assert_eq!(f.manager.access_token().await, None);
    assert_eq!(endpoint.refreshes.load(Ordering::SeqCst), 1);
    f.manager
        .handle_oauth_login_success("second", "second")
        .await
        .unwrap();
    assert_eq!(f.manager.access_token().await, None);
    assert_eq!(endpoint.refreshes.load(Ordering::SeqCst), 2);
    f.manager.logout().await;
}

// MARK: - AuthSessionRaceTests

type Reply = oneshot::Sender<Result<OAuthTokens, AuthServiceError>>;

/// 요청을 붙잡아 두고 테스트가 원하는 순서로 끝내는 토큰 엔드포인트.
#[derive(Default)]
struct TokenGate {
    pending: Mutex<HashMap<String, Reply>>,
    calls: Mutex<HashMap<String, usize>>,
}

impl TokenGate {
    async fn request(
        &self,
        body: BTreeMap<String, String>,
    ) -> Result<OAuthTokens, AuthServiceError> {
        let key = body
            .get("code")
            .or_else(|| body.get("refreshToken"))
            .cloned()
            .unwrap_or_else(|| "missing".into());
        let (tx, rx) = oneshot::channel();
        *self.calls.lock().unwrap().entry(key.clone()).or_default() += 1;
        self.pending.lock().unwrap().insert(key, tx);
        rx.await.unwrap_or(Err(AuthServiceError::Cancelled))
    }

    async fn started(&self, key: &str) {
        wait_until(|| self.pending.lock().unwrap().contains_key(key)).await;
    }

    fn finish(&self, key: &str, expires: i64, failure: bool) {
        let Some(tx) = self.pending.lock().unwrap().remove(key) else {
            return;
        };
        let _ = tx.send(if failure {
            Err(unclassified())
        } else {
            Ok(OAuthTokens::bearer(
                key,
                Some(format!("refresh-{key}")),
                expires,
            ))
        });
    }

    fn calls(&self, key: &str) -> usize {
        self.calls.lock().unwrap().get(key).copied().unwrap_or(0)
    }
}

fn gate_fixture(gate: Arc<TokenGate>) -> Fixture {
    fixture(
        fixture_config(),
        keychain(),
        token_fn(move |body| {
            let g = gate.clone();
            async move { g.request(body).await }
        }),
        None,
        None,
    )
}

async fn login(f: &Fixture, gate: &TokenGate, key: &str, expires: i64) {
    let m = f.manager.clone();
    let k = key.to_string();
    let task = tokio::spawn(async move { m.handle_oauth_login_success(&k, &k).await });
    gate.started(key).await;
    gate.finish(key, expires, false);
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn late_exchange_cannot_undo_logout() {
    let gate = Arc::new(TokenGate::default());
    let f = gate_fixture(gate.clone());
    let m = f.manager.clone();
    let task = tokio::spawn(async move { m.handle_oauth_login_success("old", "old").await });
    gate.started("old").await;
    f.manager.logout().await;
    gate.finish("old", 3600, false);
    let _ = task.await.unwrap();
    assert_eq!(f.manager.auth_state(), AuthState::LoggedOut);
    assert_eq!(f.manager.oauth_tokens(), None);
    assert_eq!(f.oauth.load_stored_tokens(), None);
}

#[tokio::test]
async fn old_exchange_failure_cannot_expire_newer_login() {
    let gate = Arc::new(TokenGate::default());
    let f = gate_fixture(gate.clone());
    let m = f.manager.clone();
    let old = tokio::spawn(async move { m.handle_oauth_login_success("old", "old").await });
    gate.started("old").await;
    login(&f, &gate, "new", 3600).await;
    gate.finish("old", 3600, true);
    assert_eq!(old.await.unwrap(), Err(AuthServiceError::Cancelled));
    assert_eq!(f.manager.auth_state(), logged_in("oauth"));
    assert_eq!(f.oauth.load_stored_tokens().unwrap().access_token, "new");
    f.manager.logout().await;
}

#[tokio::test]
async fn suspended_refresh_cannot_persist_after_logout() {
    let gate = Arc::new(TokenGate::default());
    let f = gate_fixture(gate.clone());
    login(&f, &gate, "old", -1).await;
    let m = f.manager.clone();
    let refresh = tokio::spawn(async move { m.refresh_oauth_token_if_needed().await });
    gate.started("refresh-old").await;
    f.manager.logout().await;
    gate.finish("refresh-old", 3600, false);
    assert!(!refresh.await.unwrap());
    assert_eq!(f.manager.auth_state(), AuthState::LoggedOut);
    assert_eq!(f.oauth.load_stored_tokens(), None);
}

#[tokio::test]
async fn old_refresh_cannot_release_new_generation_slot() {
    let gate = Arc::new(TokenGate::default());
    let f = gate_fixture(gate.clone());
    login(&f, &gate, "old", -1).await;
    let m = f.manager.clone();
    let old = tokio::spawn(async move { m.refresh_oauth_token_if_needed().await });
    gate.started("refresh-old").await;
    login(&f, &gate, "new", -1).await;
    let m = f.manager.clone();
    let next = tokio::spawn(async move { m.refresh_oauth_token_if_needed().await });
    gate.started("refresh-new").await;
    gate.finish("refresh-old", 3600, true);
    assert!(!old.await.unwrap());
    let m = f.manager.clone();
    let coalesced = tokio::spawn(async move { m.refresh_oauth_token_if_needed().await });
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(gate.calls("refresh-new"), 1);
    gate.finish("refresh-new", 3600, false);
    assert!(next.await.unwrap());
    assert!(coalesced.await.unwrap());
    assert_eq!(
        f.oauth.load_stored_tokens().unwrap().access_token,
        "refresh-new"
    );
    f.manager.logout().await;
}

#[tokio::test]
async fn delayed_startup_restore_cannot_replace_explicit_login_or_logout() {
    let gate = Arc::new(TokenGate::default());
    let f = gate_fixture(gate.clone());
    login(&f, &gate, "explicit", 3600).await;
    f.manager.initialize().await;
    assert_eq!(f.manager.oauth_tokens().unwrap().access_token, "explicit");
    assert_eq!(
        f.oauth.load_stored_tokens().unwrap().access_token,
        "explicit"
    );
    f.manager.logout().await;
    f.manager.initialize().await;
    assert_eq!(f.manager.auth_state(), AuthState::LoggedOut);
}

#[tokio::test]
async fn token_provider_trait_reflects_state() {
    let gate = Arc::new(TokenGate::default());
    let f = gate_fixture(gate.clone());
    let provider: &dyn AuthTokenProvider = &f.manager;
    assert!(!provider.is_authenticated().await);
    assert_eq!(provider.cookies().await, None);
    let before = provider.authentication_generation().await;
    for name in ["NID_AUT", "NID_SES"] {
        f.jar.set_cookie(StoredCookie::new(name, "v", ".naver.com"));
    }
    assert_eq!(
        provider.cookies().await,
        None,
        "로그인 전에는 쿠키를 내주지 않는다"
    );
    login(&f, &gate, "tok", 3600).await;
    assert!(provider.is_authenticated().await);
    assert_eq!(provider.access_token().await.as_deref(), Some("tok"));
    assert_eq!(provider.cookies().await.map(|c| c.len()), Some(2));
    assert!(provider.authentication_generation().await > before);
    f.manager.logout().await;
    assert_eq!(provider.cookies().await, None);
}

#[tokio::test]
async fn cookie_login_success_sets_cookie_state() {
    let gate = Arc::new(TokenGate::default());
    let f = gate_fixture(gate);
    for name in ["NID_AUT", "NID_SES"] {
        f.jar
            .set_cookie(StoredCookie::new(name, "v", "nid.naver.com"));
    }
    f.manager.handle_login_success().await;
    assert_eq!(f.manager.auth_state(), logged_in("cookie"));
    assert!(f.cookies.restore_cookies_from_keychain());
}

// MARK: - StartupTokenRefreshTests / PrecisionReview0926

#[derive(Clone, Debug)]
enum Mode {
    Offline,
    Online,
    Rejected,
    ServerError,
    Failing(AuthServiceError),
}

/// refresh 결과를 도중에 바꿀 수 있는 엔드포인트. code 교환은 곧바로 만료된 토큰을 준다.
struct SwitchableStub {
    mode: Mutex<Mode>,
    refreshes: AtomicUsize,
}

impl SwitchableStub {
    fn new(mode: Mode) -> Arc<Self> {
        Arc::new(Self {
            mode: Mutex::new(mode),
            refreshes: AtomicUsize::new(0),
        })
    }
    fn set(&self, mode: Mode) {
        *self.mode.lock().unwrap() = mode;
    }
    fn refreshes(&self) -> usize {
        self.refreshes.load(Ordering::SeqCst)
    }
    fn handle(&self, body: &BTreeMap<String, String>) -> Result<OAuthTokens, AuthServiceError> {
        if let Some(code) = body.get("code") {
            return Ok(OAuthTokens::bearer(
                code.clone(),
                Some(format!("refresh-{code}")),
                -1,
            ));
        }
        self.refreshes.fetch_add(1, Ordering::SeqCst);
        match self.mode.lock().unwrap().clone() {
            Mode::Offline => Err(AuthServiceError::Transport(TransportError::NotConnected)),
            Mode::Rejected => Err(AuthServiceError::TokenRejected {
                detail: "HTTP 401".into(),
            }),
            Mode::ServerError => Err(AuthServiceError::EndpointUnavailable { status: 503 }),
            Mode::Failing(e) => Err(e),
            Mode::Online => Ok(OAuthTokens::bearer(
                "fresh",
                Some("refresh-fresh".into()),
                3600,
            )),
        }
    }
}

/// 프로필 엔드포인트 — 실린 access token 을 기록한다.
#[derive(Default)]
struct ProfileEndpoint {
    tokens: Mutex<Vec<String>>,
}

struct Opts {
    profile: Option<Arc<ProfileEndpoint>>,
    /// true 면 설정에 자격 증명을 넣지 않고 보안 저장소 조달기(환경변수 없음)로만 받는다.
    stored_credentials: bool,
}

const NO_OPTS: Opts = Opts {
    profile: None,
    stored_credentials: false,
};

fn stub_fixture(stub: Arc<SwitchableStub>, keychain: Arc<KeychainService>, opts: Opts) -> Fixture {
    let secret = if opts.stored_credentials {
        ""
    } else {
        "fixture"
    };
    let config = OAuthConfig::new(secret, secret, "http://localhost/callback", 1);
    let profile = opts.profile.map(|p| {
        profile_fn(move |token| {
            let p = p.clone();
            async move {
                p.tokens.lock().unwrap().push(token);
                Ok(serde_json::from_str::<OAuthUserProfile>(
                    r#"{"channelId":"channel-fixture","nickname":"fixture"}"#,
                )
                .unwrap())
            }
        })
    });
    fixture(
        config,
        keychain,
        token_fn(move |body| {
            let s = stub.clone();
            async move { s.handle(&body) }
        }),
        profile,
        Some(HashMap::new()),
    )
}

/// 만료된 토큰이 저장된 보안 저장소.
fn keychain_with_expired(
    refresh_token: Option<&str>,
    store: Arc<RecordingStore>,
) -> Arc<KeychainService> {
    let kc = keychain_with(store);
    let expired = OAuthTokens::bearer("stale", refresh_token.map(Into::into), -1);
    kc.save_codable(TOKEN_KEYCHAIN_KEY, &expired).unwrap();
    kc
}

fn stale_keychain() -> Arc<KeychainService> {
    keychain_with_expired(Some("refresh-stale"), RecordingStore::new())
}

fn refresh_token_of(t: Option<OAuthTokens>) -> Option<String> {
    t.and_then(|t| t.refresh_token)
}

#[tokio::test]
async fn transient_startup_failure_keeps_tokens() {
    let stub = SwitchableStub::new(Mode::Offline);
    let f = stub_fixture(stub.clone(), stale_keychain(), NO_OPTS);
    f.manager.initialize().await;
    assert_eq!(stub.refreshes(), 1);
    assert_eq!(
        refresh_token_of(f.oauth.load_stored_tokens()).as_deref(),
        Some("refresh-stale")
    );
    assert_eq!(
        refresh_token_of(f.manager.oauth_tokens()).as_deref(),
        Some("refresh-stale")
    );
    // 백오프 중에는 다시 때리지 않는다.
    assert_eq!(f.manager.access_token().await, None);
    assert_eq!(stub.refreshes(), 1);
    f.manager.logout().await;
}

#[tokio::test]
async fn rejected_startup_refresh_clears_tokens() {
    let f = stub_fixture(
        SwitchableStub::new(Mode::Rejected),
        stale_keychain(),
        NO_OPTS,
    );
    f.manager.initialize().await;
    assert_eq!(f.oauth.load_stored_tokens(), None);
    assert_eq!(f.manager.oauth_tokens(), None);
    assert_eq!(f.manager.auth_state(), AuthState::LoggedOut);
}

#[tokio::test]
async fn refresh_failure_falls_back_to_cookie_login() {
    let f = stub_fixture(SwitchableStub::new(Mode::Rejected), keychain(), NO_OPTS);
    for name in ["NID_AUT", "NID_SES"] {
        f.jar
            .set_cookie(StoredCookie::new(name, "fixture", ".naver.com"));
    }
    f.manager
        .handle_oauth_login_success("expired", "expired")
        .await
        .unwrap();
    assert_eq!(f.manager.access_token().await, None);
    assert_eq!(f.manager.auth_state(), logged_in("cookie"));
    assert_eq!(
        f.manager.cookies().map(|c| c.len()),
        Some(2),
        "쿠키 접근이 꺼지면 쿠키로 되는 요청이 나가지 못한다"
    );
    f.manager.logout().await;
}

#[tokio::test]
async fn offline_boot_with_kept_tokens_stays_logged_in() {
    let stub = SwitchableStub::new(Mode::Offline);
    let f = stub_fixture(stub.clone(), stale_keychain(), NO_OPTS);
    f.manager.initialize().await;
    assert_eq!(stub.refreshes(), 1);
    assert_eq!(
        refresh_token_of(f.manager.oauth_tokens()).as_deref(),
        Some("refresh-stale")
    );
    assert_eq!(f.manager.auth_state(), logged_in("oauth"));
    assert!(f.manager.is_authenticated());
    f.manager.logout().await;
}

#[tokio::test]
async fn offline_boot_then_recovery_never_reports_logged_out() {
    let stub = SwitchableStub::new(Mode::Offline);
    let f = stub_fixture(stub.clone(), stale_keychain(), NO_OPTS);
    let mut stream = f.manager.auth_state_stream();

    f.manager.initialize().await;
    assert!(f.manager.is_authenticated());

    stub.set(Mode::Online);
    f.manager.clear_refresh_backoff();
    assert_eq!(f.manager.access_token().await.as_deref(), Some("fresh"));
    assert_eq!(stub.refreshes(), 2);
    assert_eq!(f.manager.auth_state(), logged_in("oauth"));
    assert_eq!(f.oauth.load_stored_tokens().unwrap().access_token, "fresh");

    let mut seen = vec![];
    while let Ok(s) = stream.try_recv() {
        seen.push(s);
    }
    // 첫 원소는 구독 시점의 초기값(`LoggedOut`)이다 — 그 뒤로 로그아웃이 끼어들면 안 된다.
    assert_eq!(seen.first(), Some(&AuthState::LoggedOut));
    let transitions = &seen[1..];
    assert!(!transitions.is_empty());
    assert!(
        !transitions.contains(&AuthState::LoggedOut),
        "부팅 도중 로그아웃이 발행됐다: {transitions:?}"
    );
    assert_eq!(transitions.last(), Some(&logged_in("oauth")));
    f.manager.logout().await;
}

#[tokio::test]
async fn fetch_oauth_profile_sends_refreshed_token() {
    let stub = SwitchableStub::new(Mode::Offline);
    let profile = Arc::new(ProfileEndpoint::default());
    let f = stub_fixture(
        stub.clone(),
        stale_keychain(),
        Opts {
            profile: Some(profile.clone()),
            stored_credentials: false,
        },
    );
    f.manager.initialize().await;
    stub.set(Mode::Online);
    f.manager.clear_refresh_backoff();
    let fetched = f.manager.fetch_oauth_profile().await.unwrap();
    assert_eq!(fetched.nickname.as_deref(), Some("fixture"));
    assert_eq!(stub.refreshes(), 2);
    assert_eq!(
        *profile.tokens.lock().unwrap(),
        ["fresh"],
        "만료 토큰을 갱신 없이 그대로 실었다"
    );
    f.manager.logout().await;
}

#[tokio::test]
async fn fetch_oauth_profile_does_not_send_when_token_unavailable() {
    let stub = SwitchableStub::new(Mode::Offline);
    let profile = Arc::new(ProfileEndpoint::default());
    let f = stub_fixture(
        stub.clone(),
        stale_keychain(),
        Opts {
            profile: Some(profile.clone()),
            stored_credentials: false,
        },
    );
    f.manager.initialize().await;
    f.manager.clear_refresh_backoff();
    assert!(f.manager.fetch_oauth_profile().await.is_err());
    assert_eq!(stub.refreshes(), 2);
    assert!(
        profile.tokens.lock().unwrap().is_empty(),
        "만료 토큰으로 프로필 요청을 보냈다"
    );
    f.manager.logout().await;
}

#[tokio::test]
async fn restored_cookies_take_precedence_over_kept_tokens() {
    let f = stub_fixture(
        SwitchableStub::new(Mode::Offline),
        stale_keychain(),
        NO_OPTS,
    );
    for name in ["NID_AUT", "NID_SES"] {
        f.jar
            .set_cookie(StoredCookie::new(name, "fixture", ".naver.com"));
    }
    f.cookies.save_cookies_to_keychain().unwrap();
    f.jar.clear();
    f.manager.initialize().await;
    assert_eq!(f.manager.auth_state(), logged_in("cookie"));
    assert_eq!(
        refresh_token_of(f.manager.oauth_tokens()).as_deref(),
        Some("refresh-stale")
    );
    f.manager.logout().await;
}

#[tokio::test]
async fn rejection_after_offline_boot_drops_tokens() {
    let stub = SwitchableStub::new(Mode::Offline);
    let f = stub_fixture(stub.clone(), stale_keychain(), NO_OPTS);
    f.manager.initialize().await;
    assert_eq!(f.manager.auth_state(), logged_in("oauth"));

    stub.set(Mode::Rejected);
    f.manager.clear_refresh_backoff();
    assert_eq!(f.manager.access_token().await, None);
    assert_eq!(stub.refreshes(), 2);
    assert_eq!(f.manager.auth_state(), AuthState::Expired);
    assert_eq!(
        f.manager.oauth_tokens(),
        None,
        "죽은 토큰을 쥐면 끝난 세션을 알아챌 수 없다"
    );
    assert_eq!(
        f.oauth.load_stored_tokens(),
        None,
        "다음 부팅이 같은 거절을 되풀이한다"
    );
    f.manager.clear_refresh_backoff();
    assert_eq!(f.manager.access_token().await, None);
    assert_eq!(stub.refreshes(), 2);
    f.manager.logout().await;
}

#[tokio::test]
async fn transient_failure_after_boot_keeps_tokens() {
    let stub = SwitchableStub::new(Mode::Offline);
    let f = stub_fixture(stub.clone(), stale_keychain(), NO_OPTS);
    f.manager.initialize().await;
    f.manager.clear_refresh_backoff();
    assert_eq!(f.manager.access_token().await, None);
    assert_eq!(stub.refreshes(), 2);
    assert_eq!(f.manager.auth_state(), AuthState::Expired);
    assert_eq!(
        refresh_token_of(f.manager.oauth_tokens()).as_deref(),
        Some("refresh-stale")
    );
    f.manager.logout().await;
}

fn remaining(until: Option<Instant>) -> Duration {
    until
        .expect("백오프가 걸려 있어야 한다")
        .saturating_duration_since(Instant::now())
}

#[tokio::test]
async fn offline_failure_keeps_backoff_short() {
    let stub = SwitchableStub::new(Mode::Offline);
    let f = stub_fixture(stub.clone(), stale_keychain(), NO_OPTS);
    f.manager.initialize().await;
    assert!(remaining(f.manager.refresh_backoff_until()) <= OFFLINE_REFRESH_RETRY);
    assert_eq!(
        f.manager.refresh_backoff(),
        REFRESH_BACKOFF_INITIAL,
        "오프라인 실패가 다음 백오프를 키웠다"
    );

    stub.set(Mode::ServerError);
    f.manager.clear_refresh_backoff();
    assert_eq!(f.manager.access_token().await, None);
    assert!(remaining(f.manager.refresh_backoff_until()) > Duration::from_secs(30));
    assert_eq!(f.manager.refresh_backoff(), Duration::from_secs(120));
    f.manager.logout().await;
}

#[tokio::test]
async fn transport_failures_keep_backoff_short() {
    for err in [
        TransportError::TimedOut("t".into()),
        TransportError::ConnectionLost("l".into()),
        TransportError::Other("tls".into()),
    ] {
        let stub = SwitchableStub::new(Mode::Failing(AuthServiceError::Transport(err.clone())));
        let f = stub_fixture(stub.clone(), stale_keychain(), NO_OPTS);
        f.manager.initialize().await;
        assert_eq!(f.manager.auth_state(), logged_in("oauth"), "{err:?}");
        assert!(remaining(f.manager.refresh_backoff_until()) <= OFFLINE_REFRESH_RETRY);
        assert_eq!(f.manager.refresh_backoff(), REFRESH_BACKOFF_INITIAL);

        f.manager.clear_refresh_backoff();
        assert_eq!(f.manager.access_token().await, None);
        assert_eq!(stub.refreshes(), 2);
        assert!(
            remaining(f.manager.refresh_backoff_until()) <= OFFLINE_REFRESH_RETRY,
            "{err:?}"
        );
        assert_eq!(
            f.manager.refresh_backoff(),
            REFRESH_BACKOFF_INITIAL,
            "{err:?}"
        );
        assert_eq!(
            refresh_token_of(f.manager.oauth_tokens()).as_deref(),
            Some("refresh-stale")
        );
        f.manager.logout().await;
    }
}

#[tokio::test]
async fn missing_client_credentials_at_boot_logs_out() {
    let stub = SwitchableStub::new(Mode::Online);
    let f = stub_fixture(
        stub.clone(),
        stale_keychain(),
        Opts {
            profile: None,
            stored_credentials: true,
        },
    );
    f.manager.initialize().await;
    assert_eq!(
        stub.refreshes(),
        0,
        "자격증명 없이 토큰 엔드포인트를 때렸다"
    );
    assert_eq!(f.manager.auth_state(), AuthState::LoggedOut);
    assert_eq!(f.manager.oauth_tokens(), None);
    assert_eq!(
        f.oauth.load_stored_tokens(),
        None,
        "다음 부팅이 같은 유령 세션을 되풀이한다"
    );
    f.manager.logout().await;
}

#[tokio::test]
async fn transient_boot_failure_keeps_session() {
    for mode in [
        Mode::Offline,
        Mode::ServerError,
        Mode::Failing(AuthServiceError::Transport(TransportError::TimedOut(
            "t".into(),
        ))),
        Mode::Failing(AuthServiceError::EndpointUnavailable { status: 429 }),
    ] {
        let f = stub_fixture(SwitchableStub::new(mode.clone()), stale_keychain(), NO_OPTS);
        f.manager.initialize().await;
        assert_eq!(f.manager.auth_state(), logged_in("oauth"), "{mode:?}");
        assert_eq!(
            refresh_token_of(f.manager.oauth_tokens()).as_deref(),
            Some("refresh-stale")
        );
        assert_eq!(
            refresh_token_of(f.oauth.load_stored_tokens()).as_deref(),
            Some("refresh-stale")
        );
        f.manager.logout().await;
    }
}

#[tokio::test]
async fn unclassified_boot_failure_logs_out_but_keeps_stored_tokens() {
    for mode in [
        Mode::Failing(unclassified()),
        Mode::Failing(AuthServiceError::Decoding("점검 페이지".into())),
    ] {
        let f = stub_fixture(SwitchableStub::new(mode.clone()), stale_keychain(), NO_OPTS);
        f.manager.initialize().await;
        assert_eq!(f.manager.auth_state(), AuthState::LoggedOut, "{mode:?}");
        assert_eq!(f.manager.oauth_tokens(), None);
        assert_eq!(
            refresh_token_of(f.oauth.load_stored_tokens()).as_deref(),
            Some("refresh-stale"),
            "서버의 일시 이상 한 번에 OAuth 동의가 사라진다"
        );
        f.manager.logout().await;
    }
}

#[tokio::test]
async fn unreadable_credentials_at_boot_keep_session() {
    let store = RecordingStore::new();
    let stub = SwitchableStub::new(Mode::Online);
    let f = stub_fixture(
        stub.clone(),
        keychain_with_expired(Some("refresh-stale"), store.clone()),
        Opts {
            profile: None,
            stored_credentials: true,
        },
    );
    f.oauth
        .set_client_credentials("fixture", "fixture")
        .unwrap();
    *store.failing_key.lock().unwrap() = Some(OAuthCredentialStore::KEYCHAIN_KEY.into());

    f.manager.initialize().await;
    assert_eq!(stub.refreshes(), 0);
    assert_eq!(f.manager.auth_state(), logged_in("oauth"));
    assert_eq!(
        refresh_token_of(f.manager.oauth_tokens()).as_deref(),
        Some("refresh-stale")
    );
    assert_eq!(
        refresh_token_of(f.oauth.load_stored_tokens()).as_deref(),
        Some("refresh-stale"),
        "잠긴 키체인 한 번에 동의를 지웠다"
    );

    *store.failing_key.lock().unwrap() = None;
    f.manager.clear_refresh_backoff();
    assert_eq!(f.manager.access_token().await.as_deref(), Some("fresh"));
    f.manager.logout().await;
}

#[tokio::test]
async fn unreadable_credentials_mid_session_keep_tokens() {
    let store = RecordingStore::new();
    let stub = SwitchableStub::new(Mode::Offline);
    let f = stub_fixture(
        stub.clone(),
        keychain_with_expired(Some("refresh-stale"), store.clone()),
        Opts {
            profile: None,
            stored_credentials: true,
        },
    );
    f.oauth
        .set_client_credentials("fixture", "fixture")
        .unwrap();
    f.manager.initialize().await;
    assert_eq!(f.manager.auth_state(), logged_in("oauth"));

    *store.failing_key.lock().unwrap() = Some(OAuthCredentialStore::KEYCHAIN_KEY.into());
    f.manager.clear_refresh_backoff();
    assert_eq!(f.manager.access_token().await, None);
    assert_eq!(stub.refreshes(), 1);
    assert_eq!(f.manager.auth_state(), AuthState::Expired);
    assert_eq!(
        refresh_token_of(f.manager.oauth_tokens()).as_deref(),
        Some("refresh-stale")
    );
    assert_eq!(
        refresh_token_of(f.oauth.load_stored_tokens()).as_deref(),
        Some("refresh-stale")
    );
    f.manager.logout().await;
}

#[tokio::test]
async fn missing_client_credentials_mid_session_drops_tokens() {
    let stub = SwitchableStub::new(Mode::Offline);
    let f = stub_fixture(
        stub.clone(),
        stale_keychain(),
        Opts {
            profile: None,
            stored_credentials: true,
        },
    );
    f.oauth
        .set_client_credentials("fixture", "fixture")
        .unwrap();
    f.manager.initialize().await;
    assert_eq!(f.manager.auth_state(), logged_in("oauth"));
    assert_eq!(stub.refreshes(), 1);

    f.oauth.clear_client_credentials().unwrap();
    f.manager.clear_refresh_backoff();
    assert_eq!(f.manager.access_token().await, None);
    assert_eq!(stub.refreshes(), 1);
    assert_eq!(f.manager.auth_state(), AuthState::Expired);
    assert_eq!(
        f.manager.oauth_tokens(),
        None,
        "토큰이 남으면 끝난 세션을 '갱신 대기' 로 본다"
    );
    assert_eq!(f.oauth.load_stored_tokens(), None);
    f.manager.logout().await;
}

#[tokio::test]
async fn expired_token_without_refresh_token_logs_out() {
    let stub = SwitchableStub::new(Mode::Online);
    let f = stub_fixture(
        stub.clone(),
        keychain_with_expired(None, RecordingStore::new()),
        NO_OPTS,
    );
    f.manager.initialize().await;
    assert_eq!(f.manager.auth_state(), AuthState::LoggedOut);
    assert_eq!(f.manager.oauth_tokens(), None);
    assert_eq!(stub.refreshes(), 0);
    f.manager.logout().await;
}

#[tokio::test]
async fn valid_stored_tokens_restore_without_refresh() {
    let stub = SwitchableStub::new(Mode::Online);
    let kc = keychain();
    kc.save_codable(
        TOKEN_KEYCHAIN_KEY,
        &OAuthTokens::bearer("valid", Some("r".into()), 3600),
    )
    .unwrap();
    let f = stub_fixture(stub.clone(), kc, NO_OPTS);
    f.manager.initialize().await;
    assert_eq!(f.manager.auth_state(), logged_in("oauth"));
    assert_eq!(f.manager.access_token().await.as_deref(), Some("valid"));
    assert_eq!(stub.refreshes(), 0);
    f.manager.logout().await;
}
