//! Tauri 관리 상태 — 하위 크레이트 서비스와 `cview-app-core` 뷰모델을 한데 묶는다
//! (Swift `AppDependencies` 의 최소판).
//!
//! 화면 로직은 app-core 쪽에 있다: 팔로잉은 [`FollowingStore`], 인기는 [`AllBroadcastsViewModel`],
//! 검색은 [`SearchViewModel`], 채팅은 [`ChatViewModel`], 재생은 [`PlayerViewModel`]. 명령 계층
//! (`commands`)은 그 상태를 기존 DTO 로 바꿔 돌려준다. 전역 상태 규칙은 app-core [`AppState`].

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use cview_app_core::app_state::AppState;
use cview_app_core::caching::following_store::{FollowingPollSinkSlot, FollowingStore};
use cview_app_core::services::support::MemoryKeyValueStore;
use cview_app_core::viewmodels::all_broadcasts::AllBroadcastsViewModel;
use cview_app_core::viewmodels::chat::{
    ChatDeps, ChatEngineFactory, ChatEngineHandle, ChatViewModel, LiveChatEngine,
};
use cview_app_core::viewmodels::player::{PlayerCallbacks, PlayerViewModel};
use cview_app_core::viewmodels::search::{MemoryRecentSearchStore, SearchViewModel};
use cview_auth::{
    AuthManager, ChzzkOAuthService, CookieJar, CookieManager, KeychainService, OAuthConfig,
    WebViewDataStore,
};
use cview_chat::engine::{ChatCookie, CookieProvider};
use cview_chat::{ChatEngineConfiguration, ChatEngineEvent};
use cview_core::error::ApiError;
use cview_core::models::chat::{ChatMessage, EmojiMap};
use cview_core::utils::following_poll::FollowingPollOutcome;
use cview_networking::{ChzzkApiClient, ClientConfig, ReqwestTransport};
use cview_persistence::settings_store::MemoryDefaults;
use cview_persistence::{DataStore, SettingsStore};
use cview_player::hlsjs::HlsJsStreamingProfile;
use futures_util::FutureExt;
use tauri::async_runtime::JoinHandle;

use crate::dto::{ChatEventDto, PlayerEventDto};
use crate::engine::{CommandSink, WebViewHlsEngine};

/// 스토어 파일 이름 (Swift `CView_v2.store`).
pub const STORE_FILE_NAME: &str = "CView_v2.store";
/// 설정 외부 미러 파일 이름.
pub const SETTINGS_SNAPSHOT_FILE_NAME: &str = "settings.json";

/// 플레이어 페이지 hls.js 기본 프로필(뷰모델이 설정의 지연 프리셋으로 덮어쓴다).
pub const PLAYER_PROFILE: HlsJsStreamingProfile = HlsJsStreamingProfile::LowLatency;

/// 셸이 프런트엔드로 내보내는 통로 — `lib.rs` 가 `AppHandle` 로 만든다.
#[derive(Clone)]
pub struct ShellSinks {
    /// `player://command` — 플레이어 페이지에서 실행할 스크립트.
    pub player_command: CommandSink,
    /// `player://event`.
    pub player_event: Arc<dyn Fn(PlayerEventDto) + Send + Sync>,
    /// `chat://event`.
    pub chat_event: Arc<dyn Fn(ChatEventDto) + Send + Sync>,
}

impl ShellSinks {
    /// 아무 데도 보내지 않는 통로(테스트).
    pub fn noop() -> Self {
        Self {
            player_command: Arc::new(|_| {}),
            player_event: Arc::new(|_| {}),
            chat_event: Arc::new(|_| {}),
        }
    }
}

/// 채팅 엔진 어댑터 — 뷰모델이 이벤트를 꺼낼 때 같은 이벤트를 `chat://event` 로도 흘린다.
/// 엔진 이벤트 큐는 소비자가 하나뿐이라 뷰모델과 셸이 따로 구독할 수 없다.
struct TappedChatEngine {
    inner: LiveChatEngine,
    sink: Arc<dyn Fn(ChatEventDto) + Send + Sync>,
}

#[async_trait]
impl ChatEngineHandle for TappedChatEngine {
    async fn connect(&self) {
        self.inner.connect().await;
    }
    async fn disconnect(&self) {
        self.inner.disconnect().await;
    }
    async fn send_message(&self, text: &str, emojis: &EmojiMap) -> Result<i64, String> {
        self.inner.send_message(text, emojis).await
    }
    fn set_background_mode(&self, enabled: bool) {
        self.inner.set_background_mode(enabled);
    }
    fn clear_messages(&self) {
        self.inner.clear_messages();
    }
    fn messages(&self) -> Vec<ChatMessage> {
        self.inner.messages()
    }
    async fn next_event(&self) -> Option<ChatEngineEvent> {
        let event = self.inner.next_event().await;
        if let Some(e) = &event {
            (self.sink)(ChatEventDto::from_engine(e));
        }
        event
    }
}

fn chat_engine_factory(
    cookies: Arc<CookieManager>,
    sink: Arc<dyn Fn(ChatEventDto) + Send + Sync>,
) -> ChatEngineFactory {
    Arc::new(move |config: ChatEngineConfiguration| {
        let cookies = cookies.clone();
        let provider: CookieProvider = Arc::new(move || {
            cookies
                .auth_cookies()
                .into_iter()
                .map(|c| ChatCookie {
                    name: c.name,
                    value: c.value,
                    domain: c.domain,
                })
                .collect()
        });
        Arc::new(TappedChatEngine {
            inner: LiveChatEngine::new(config.cookie_provider(Some(provider))),
            sink: sink.clone(),
        }) as Arc<dyn ChatEngineHandle>
    })
}

/// 앱 전역 서비스.
pub struct AppServices {
    pub api: ChzzkApiClient,
    pub auth: AuthManager,
    pub cookies: Arc<CookieManager>,
    pub oauth: Arc<ChzzkOAuthService>,
    pub data_store: Option<DataStore>,
    pub settings: SettingsStore,
    /// app-core 전역 상태(로그인·저장소·인증 경계).
    pub app_state: Mutex<AppState>,
    /// 팔로잉 폴 단일 비행 스토어.
    pub following: FollowingStore,
    /// 마지막 팔로잉 폴 실패(성공하면 비운다).
    pub following_error: Arc<Mutex<Option<ApiError>>>,
    /// 인기(전체방송) 목록.
    pub popular: AllBroadcastsViewModel,
    /// 통합 검색.
    pub search: SearchViewModel,
    /// 채팅.
    pub chat: ChatViewModel,
    /// 라이브 재생.
    pub player: PlayerViewModel,
    /// 플레이어 페이지 엔진(앱 수명 동안 하나, 뷰모델에 주입돼 있다).
    pub engine: Arc<WebViewHlsEngine>,
    /// 재생 시작·정지 직렬화 — 지금 재생 중인 채널.
    pub player_channel: tokio::sync::Mutex<Option<String>>,
    pub sinks: ShellSinks,
    /// 열린 로그인 창 작업.
    pub login_task: Mutex<Option<JoinHandle<()>>>,
}

impl std::fmt::Debug for AppServices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppServices")
            .field("auth", &self.auth)
            .field("data_store", &self.data_store)
            .finish_non_exhaustive()
    }
}

impl AppServices {
    /// 서비스를 만든다. tokio 런타임 안에서 불러야 한다(API 캐시 정리 작업을 띄운다).
    /// 인증 복원은 하지 않는다 — 호출자가 `auth.initialize()` 를 따로 부른다.
    ///
    /// `data_dir` 아래에 SQLite 스토어와 설정 미러를 둔다. 스토어를 열지 못하면 저장 없이 동작한다.
    pub async fn initialize(
        data_dir: &Path,
        webview: Arc<dyn WebViewDataStore>,
        sinks: ShellSinks,
    ) -> Self {
        let keychain = Arc::new(KeychainService::default());
        let cookies = Arc::new(
            CookieManager::new(keychain.clone(), Arc::new(CookieJar::new()))
                .with_webview_store(webview),
        );
        let oauth = Arc::new(ChzzkOAuthService::new(OAuthConfig::chzzk(), keychain));
        let auth = AuthManager::new(cookies.clone(), oauth.clone(), None);

        let transport = match ReqwestTransport::new() {
            Ok(t) => t,
            Err(e) => panic!("HTTP 클라이언트를 만들 수 없습니다: {e}"),
        };
        let mut config = ClientConfig::new(Arc::new(transport));
        config.auth_provider = Some(Arc::new(auth.clone()));
        let api = ChzzkApiClient::new(config);

        let data_store = open_store(data_dir);
        let settings = SettingsStore::new(
            data_store.clone(),
            Some(data_dir.join(SETTINGS_SNAPSHOT_FILE_NAME)),
            Arc::new(MemoryDefaults::default()),
        );
        settings.load().await;
        api.update_network_settings(&settings.network());
        // 인증 복원(`AuthManager::initialize`)은 웹뷰 쿠키 저장소를 읽을 수 있어 메인 스레드를 막지 않도록
        // 앱 시작 뒤 별도 작업에서 한다(lib.rs).

        let mut app_state = AppState::new(Arc::new(MemoryKeyValueStore::default()));
        app_state.has_data_store = data_store.is_some();
        app_state.data_store_initialization_failed = data_store.is_none();
        app_state.is_settings_loaded = true;

        let api_arc = Arc::new(api.clone());
        let following = FollowingStore::new(Some(api_arc.clone()));
        let following_error = Arc::new(Mutex::new(None));
        let error_slot = following_error.clone();
        following.set_sink(
            FollowingPollSinkSlot::Home,
            Some(Arc::new(move |outcome| {
                let error = match outcome {
                    FollowingPollOutcome::Success(_) => None,
                    FollowingPollOutcome::Failure(f) => Some(f.error),
                };
                *error_slot.lock().unwrap_or_else(|e| e.into_inner()) = error;
                async {}.boxed()
            })),
        );
        let popular = AllBroadcastsViewModel::new(api_arc.clone());
        let search = SearchViewModel::new(api_arc, Arc::new(MemoryRecentSearchStore::default()));
        let chat = ChatViewModel::with_deps(ChatDeps {
            engine_factory: chat_engine_factory(cookies.clone(), sinks.chat_event.clone()),
            ..ChatDeps::default()
        });

        let engine = Arc::new(WebViewHlsEngine::new(
            sinks.player_command.clone(),
            PLAYER_PROFILE,
            false,
        ));
        let player = PlayerViewModel::new(None);
        player.inject_engine(engine.clone());
        player.apply_settings(&settings.player(), false);
        let player_event = sinks.player_event.clone();
        player.set_callbacks(PlayerCallbacks {
            on_stream_event: Some(Arc::new(move |event| {
                if let Some(dto) = PlayerEventDto::from_stream_event(event) {
                    player_event(dto);
                }
            })),
            ..PlayerCallbacks::default()
        });

        Self {
            api,
            auth,
            cookies,
            oauth,
            data_store,
            settings,
            app_state: Mutex::new(app_state),
            following,
            following_error,
            popular,
            search,
            chat,
            player,
            engine,
            player_channel: tokio::sync::Mutex::new(None),
            sinks,
            login_task: Mutex::new(None),
        }
    }

    /// app-core 전역 상태 잠금.
    pub fn app_state(&self) -> std::sync::MutexGuard<'_, AppState> {
        self.app_state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 인증 상태 반영 — 로그인 여부가 바뀌었으면 app-core 인증 경계를 넘고 계정별 캐시를 비운다.
    /// 경계를 넘었으면 `true`.
    pub async fn sync_authentication(&self) -> bool {
        let authenticated = self.auth.is_authenticated();
        let crossed = {
            let mut st = self.app_state();
            st.is_auth_initialized = true;
            if st.is_logged_in == authenticated {
                false
            } else {
                let boundary = st.begin_authentication_ui_transition(None);
                st.apply_login_result(boundary.epoch, authenticated);
                true
            }
        };
        if crossed {
            self.following.reset_for_account_boundary();
            let mut chat = self.chat.lock();
            chat.current_user_uid = None;
            chat.current_user_nickname = None;
        }
        crossed
    }

    /// 재생 정지.
    pub async fn stop_player(&self) {
        let mut channel = self.player_channel.lock().await;
        if channel.take().is_some() {
            self.player.stop_stream().await;
        }
    }

    /// 종료 정리 — 재생·채팅을 멈추고 대기 중인 설정 저장을 마친다.
    pub async fn shutdown(&self) {
        self.stop_player().await;
        self.chat.disconnect().await;
        self.settings.flush_pending_save().await;
    }
}

/// 스토어 경로.
pub fn store_path(data_dir: &Path) -> PathBuf {
    data_dir.join(STORE_FILE_NAME)
}

fn open_store(data_dir: &Path) -> Option<DataStore> {
    match DataStore::open(store_path(data_dir)) {
        Ok(store) => Some(store),
        Err(e) => {
            tracing::error!("데이터 스토어를 열지 못했습니다 — 저장 없이 실행: {e}");
            None
        }
    }
}
