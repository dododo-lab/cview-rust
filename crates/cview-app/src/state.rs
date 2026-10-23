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

use crate::dto::{ChatEventDto, PlayerEventDto, TaggedChatEvent};
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
    pub chat_event: Arc<dyn Fn(TaggedChatEvent) + Send + Sync>,
    /// `chat://session-event` — 추가 채팅 세션(멀티채팅) `{ session, event }`.
    pub chat_session_event: Arc<dyn Fn(String, TaggedChatEvent) + Send + Sync>,
}

impl ShellSinks {
    /// 아무 데도 보내지 않는 통로(테스트).
    pub fn noop() -> Self {
        Self {
            player_command: Arc::new(|_| {}),
            player_event: Arc::new(|_| {}),
            chat_event: Arc::new(|_| {}),
            chat_session_event: Arc::new(|_, _| {}),
        }
    }
}

/// 채팅 엔진 어댑터 — 뷰모델이 이벤트를 꺼낼 때 같은 이벤트를 `chat://event` 로도 흘린다.
/// 엔진 이벤트 큐는 소비자가 하나뿐이라 뷰모델과 셸이 따로 구독할 수 없다.
/// 이벤트에는 이 엔진의 채팅 채널 ID 를 붙인다 — 채널을 바꾼 뒤 늦게 온 이전 채널 이벤트를 프런트엔드가 버린다.
struct TappedChatEngine {
    inner: LiveChatEngine,
    chat_channel_id: String,
    sink: Arc<dyn Fn(TaggedChatEvent) + Send + Sync>,
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
            (self.sink)(TaggedChatEvent {
                event: ChatEventDto::from_engine(e),
                chat_channel_id: self.chat_channel_id.clone(),
            });
        }
        event
    }
}

fn chat_engine_factory(
    cookies: Arc<CookieManager>,
    sink: Arc<dyn Fn(TaggedChatEvent) + Send + Sync>,
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
        let chat_channel_id = config.chat_channel_id.clone();
        Arc::new(TappedChatEngine {
            inner: LiveChatEngine::new(config.cookie_provider(Some(provider))),
            chat_channel_id,
            sink: sink.clone(),
        }) as Arc<dyn ChatEngineHandle>
    })
}

/// 기본 채팅 세션 키.
pub const MAIN_CHAT_SESSION: &str = "main";

/// 명령 인자의 세션 키를 정규화한다 — 없거나 빈 값은 기본 세션.
pub fn chat_session_key(session: Option<&str>) -> &str {
    session
        .filter(|k| !k.is_empty())
        .unwrap_or(MAIN_CHAT_SESSION)
}

/// 세션별 채팅 연결·해제 요청 순번 — 프런트엔드가 마지막으로 보낸 요청이 이긴다.
///
/// Tauri 비동기 명령은 런타임 작업으로 따로 돌아 보낸 순서대로 실행된다는 보장이 없다. 패인 전환처럼
/// `chat_disconnect` 바로 뒤에 `chat_connect` 를 보내면 해제가 늦게 돌아 방금 연 연결을 끊거나, 해제가
/// 세션 뷰모델을 표에서 지운 뒤 연결이 지워진 뷰모델에 붙어(표에 없는 고아 연결) 다음 해제로도 끊기지 않고
/// 이전 채널 메시지를 같은 세션 키로 계속 흘렸다. 프런트엔드가 요청마다 단조 증가 순번(`op`)을 붙이고,
/// 셸은 이미 본 순번보다 오래된 요청을 버린다. 순번 없는 호출(`None`)은 예전처럼 늘 받는다.
#[derive(Default)]
pub struct ChatOpGate {
    last: Mutex<std::collections::HashMap<String, u64>>,
}

impl ChatOpGate {
    /// `op` 가 이 세션의 최신 요청이면 순번을 기록하고 `f` 를 잠금 안에서 돌린다. 더 새(같은) 순번을 이미
    /// 받았으면 `None`. 연결·해제의 동기 부분(세대 올리기·순번 잡기·세션 표 고치기)을 여기서 하면 여러 작업
    /// 스레드에서 동시에 돌아도 요청이 보낸 순서대로 한 줄로 선다. `f` 안에서 이 게이트를 다시 부르지 않는다.
    pub fn run_if_latest<R>(
        &self,
        session: &str,
        op: Option<u64>,
        f: impl FnOnce() -> R,
    ) -> Option<R> {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(op) = op {
            if last.get(session).is_some_and(|&seen| seen >= op) {
                return None;
            }
            last.insert(session.to_string(), op);
        }
        Some(f())
    }

    /// 받아들인 뒤 기다리는 동안(토큰 받기) 더 새 요청이 오지 않았는가.
    pub fn is_latest(&self, session: &str, op: Option<u64>) -> bool {
        let Some(op) = op else { return true };
        let last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        last.get(session).is_none_or(|&seen| seen == op)
    }
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
    /// 채팅(기본 세션 — 시청 화면·멀티라이브·채팅만 보기).
    pub chat: ChatViewModel,
    /// 추가 채팅 세션(멀티채팅) — 세션 키별 뷰모델(선행 작업 2: 채팅 다중 세션화).
    pub chat_sessions: Mutex<std::collections::HashMap<String, ChatViewModel>>,
    /// 추가 채팅 세션 → 그 세션을 연 창 라벨. 보조 창이 OS 닫기로 사라져도 세션을 정리하려고.
    pub chat_session_windows: Mutex<std::collections::HashMap<String, String>>,
    /// 세션별 채팅 연결·해제 요청 순번.
    pub chat_ops: ChatOpGate,
    /// 라이브 재생.
    pub player: PlayerViewModel,
    /// 플레이어 페이지 엔진(앱 수명 동안 하나, 뷰모델에 주입돼 있다).
    pub engine: Arc<WebViewHlsEngine>,
    /// 재생 시작·정지 직렬화 — 지금 재생 중인 채널.
    pub player_channel: tokio::sync::Mutex<Option<String>>,
    /// 지금 단독 재생 중인 원본 스트림 주소와 채널 이름(녹화용).
    pub playing_stream: Mutex<Option<(url::Url, String)>>,
    /// 방송 녹화(원본 StreamRecordingService).
    pub recorder: cview_player::recording::StreamRecordingService,
    /// 진행 중인 시청 기록 — (채널, 시작 시각). 정지할 때 끝낸다(원본 WatchHeartbeatCenter).
    pub watch_record: Mutex<Option<(String, chrono::DateTime<chrono::Utc>)>>,
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

/// 이 앱의 보안 저장소 서비스명. Swift 앱(`com.cview.auth`)과 같은 키체인 항목을 쓰면 서명이 다른
/// 이 앱이 읽을 때마다 macOS 가 키체인 비밀번호를 묻는다 — 번들 ID 를 따라 따로 둔다.
pub const KEYCHAIN_SERVICE_NAME: &str = "app.cview.rust.auth";

/// 번들 ID 를 따르는 키체인 서비스명 — 배포 앱(`app.cview.rust`)은 [`KEYCHAIN_SERVICE_NAME`] 그대로이고,
/// 다른 ID 로 만든 시험 빌드는 자기 항목을 쓴다(배포 앱의 항목을 읽다가 키체인 암호 창을 띄우지 않게).
pub fn keychain_service_name(identifier: &str) -> String {
    format!("{identifier}.auth")
}

impl AppServices {
    /// 서비스를 만든다. tokio 런타임 안에서 불러야 한다(API 캐시 정리 작업을 띄운다).
    /// 인증 복원은 하지 않는다 — 호출자가 `auth.initialize()` 를 따로 부른다.
    ///
    /// `data_dir` 아래에 SQLite 스토어와 설정 미러를 둔다. 스토어를 열지 못하면 저장 없이 동작한다.
    pub async fn initialize(
        data_dir: &Path,
        keychain_service: &str,
        webview: Arc<dyn WebViewDataStore>,
        sinks: ShellSinks,
    ) -> Self {
        let keychain = Arc::new(KeychainService::new(keychain_service));
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
            chat_sessions: Mutex::new(std::collections::HashMap::new()),
            chat_session_windows: Mutex::new(std::collections::HashMap::new()),
            chat_ops: ChatOpGate::default(),
            player,
            engine,
            player_channel: tokio::sync::Mutex::new(None),
            watch_record: Mutex::new(None),
            playing_stream: Mutex::new(None),
            recorder: cview_player::recording::StreamRecordingService::new(),
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
        self.recorder.stop_recording();
        *self
            .playing_stream
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
        self.end_watch_record();
    }

    /// 세션 키 → 채팅 뷰모델. 없거나 `main` 이면 기본 세션, 아니면 없을 때 새로 만든다.
    pub fn chat_vm(&self, session: Option<&str>) -> ChatViewModel {
        let Some(key) = session.filter(|k| !k.is_empty() && *k != MAIN_CHAT_SESSION) else {
            return self.chat.clone();
        };
        let mut map = self.chat_sessions.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(vm) = map.get(key) {
            return vm.clone();
        }
        let emit = self.sinks.chat_session_event.clone();
        let session_key = key.to_string();
        let sink: Arc<dyn Fn(TaggedChatEvent) + Send + Sync> =
            Arc::new(move |dto| emit(session_key.clone(), dto));
        let vm = ChatViewModel::with_deps(ChatDeps {
            engine_factory: chat_engine_factory(self.cookies.clone(), sink),
            ..ChatDeps::default()
        });
        map.insert(key.to_string(), vm.clone());
        vm
    }

    /// `vm` 이 아직 이 세션 키의 뷰모델인가. 기다리는 동안 `drop_chat_session` 이 지웠다면 `false` —
    /// 그대로 연결하면 표에 없는 고아 연결이 된다.
    pub fn is_live_chat_vm(&self, session: Option<&str>, vm: &ChatViewModel) -> bool {
        let key = chat_session_key(session);
        if key == MAIN_CHAT_SESSION {
            return vm.same_as(&self.chat);
        }
        self.chat_sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(key)
            .is_some_and(|v| v.same_as(vm))
    }

    /// 추가 채팅 세션을 끊고 지운다.
    pub async fn drop_chat_session(&self, session: &str) {
        self.detach_chat_session(session).await;
    }

    /// [`Self::drop_chat_session`] 의 동기 부분 — 표에서 지우고 엔진을 떼어 낸다. 돌려준 future 가 소켓을 닫는다.
    pub fn detach_chat_session(
        &self,
        session: &str,
    ) -> impl std::future::Future<Output = ()> + Send + 'static {
        self.chat_session_windows
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(session);
        let vm = self
            .chat_sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(session);
        let detached = vm.map(|vm| vm.detach());
        async move {
            if let Some(d) = detached {
                d.await;
            }
        }
    }

    /// 이 추가 채팅 세션을 연 창을 기록한다(메인 창 세션은 기록하지 않는다).
    pub fn note_chat_session_window(&self, session: &str, window_label: &str) {
        if window_label == crate::MAIN_WINDOW {
            return;
        }
        self.chat_session_windows
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session.to_string(), window_label.to_string());
    }

    /// 창이 사라지면 그 창이 연 추가 채팅 세션을 모두 끊는다(웹소켓·핑·감시 작업이 죽은 창으로 계속 보내지 않게).
    pub async fn drop_chat_sessions_of_window(&self, window_label: &str) {
        let sessions: Vec<String> = self
            .chat_session_windows
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|(_, w)| w.as_str() == window_label)
            .map(|(s, _)| s.clone())
            .collect();
        for s in sessions {
            self.drop_chat_session(&s).await;
        }
    }

    /// 시청 기록 시작 — 채널 행을 저장하고 최근 시청을 갱신한다.
    pub fn begin_watch_record(&self, info: &cview_core::models::LiveInfo) {
        self.end_watch_record();
        let (Some(store), Some(channel)) = (&self.data_store, &info.channel) else {
            return;
        };
        let favorite = store.is_favorite(&channel.channel_id).unwrap_or(false);
        if let Err(e) = store.save_channel(channel, favorite) {
            tracing::debug!("채널 저장 실패: {e}");
        }
        if let Err(e) = store.update_last_watched(&channel.channel_id) {
            tracing::debug!("최근 시청 기록 실패: {e}");
        }
        let thumb = info.resolved_live_image_url().map(|u| u.to_string());
        match store.start_watch_record(
            &channel.channel_id,
            &channel.channel_name,
            thumb.as_deref(),
            info.live_category_value.as_deref(),
            chrono::Utc::now(),
        ) {
            Ok(started) => {
                *self.watch_record.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some((channel.channel_id.clone(), started));
            }
            Err(e) => tracing::debug!("시청 기록 시작 실패: {e}"),
        }
    }

    /// 진행 중인 시청 기록을 끝낸다.
    pub fn end_watch_record(&self) {
        let record = self
            .watch_record
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let (Some(store), Some((channel_id, started))) = (&self.data_store, record) {
            if let Err(e) = store.end_watch_record(&channel_id, started, chrono::Utc::now()) {
                tracing::debug!("시청 기록 종료 실패: {e}");
            }
        }
    }

    /// 종료 정리 — 재생·채팅을 멈추고 대기 중인 설정 저장을 마친다.
    pub async fn shutdown(&self) {
        self.stop_player().await;
        self.chat.disconnect().await;
        let sessions: Vec<_> = self
            .chat_sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain()
            .map(|(_, vm)| vm)
            .collect();
        for vm in sessions {
            vm.disconnect().await;
        }
        self.settings.flush_pending_save().await;
    }
}

/// 스토어 경로.
pub fn store_path(data_dir: &Path) -> PathBuf {
    data_dir.join(STORE_FILE_NAME)
}

/// 스토어 열기 — 사전 점검·업그레이드 전 백업·열기 기록(store-health.json)이 붙은 경로로 연다.
/// 파일이 손상돼 열리지 않으면(예전엔 매번 저장 없이 실행) 최신 백업으로 되돌리고, 백업이 없으면 손상 파일을
/// `.corrupt-<시각>` 으로 옮겨 두고 새로 시작한다. 지우지는 않는다. 손상이 아닌 오류(잠김 등)는 저장 없이 실행.
fn open_store(data_dir: &Path) -> Option<DataStore> {
    use cview_persistence::store_backup::StoreBackup;
    use cview_persistence::store_open_health::StoreOpenHealthRecorder;
    let path = store_path(data_dir);
    let health = StoreOpenHealthRecorder::new(data_dir.join(StoreOpenHealthRecorder::FILE_NAME));
    let backup = StoreBackup::new(&path);
    let build = env!("CARGO_PKG_VERSION");
    let error = match DataStore::create_container_at(&path, &health, &backup, build) {
        Ok(store) => return Some(store),
        Err(e) => e,
    };
    let message = error.to_string().to_lowercase();
    let corrupt = [
        "not a database",
        "malformed",
        "corrupt",
        "file is encrypted",
    ]
    .iter()
    .any(|k| message.contains(k));
    if !corrupt {
        tracing::error!("데이터 스토어를 열지 못했습니다 — 저장 없이 실행: {error}");
        return None;
    }
    if let Some(latest) = backup.list().into_iter().next() {
        match backup.restore(&latest) {
            Ok(()) => {
                tracing::warn!("손상된 스토어를 백업 {} 으로 되돌렸습니다", latest.id);
                if let Ok(store) = DataStore::open(&path) {
                    return Some(store);
                }
            }
            Err(e) => tracing::error!("백업 복원 실패: {e:?}"),
        }
    }
    // 백업이 없거나 복원도 열리지 않으면 손상 파일을 옆으로 옮기고 새로 시작한다.
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    for name in backup.store_file_names() {
        let file = path.with_file_name(&name);
        if file.exists() {
            let _ = std::fs::rename(
                &file,
                path.with_file_name(format!("{name}.corrupt-{stamp}")),
            );
        }
    }
    match DataStore::open(&path) {
        Ok(store) => {
            tracing::warn!("손상된 스토어를 .corrupt-{stamp} 로 옮기고 새로 시작합니다");
            Some(store)
        }
        Err(e) => {
            tracing::error!("데이터 스토어를 열지 못했습니다 — 저장 없이 실행: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupt_store_is_moved_aside_and_fresh_store_opens() {
        let dir = tempfile::tempdir().unwrap();
        let path = store_path(dir.path());
        std::fs::write(
            &path,
            b"this is not a sqlite database at all, just garbage bytes",
        )
        .unwrap();
        let store = open_store(dir.path());
        assert!(
            store.is_some(),
            "손상 파일이 있어도 새 스토어로 열려야 한다"
        );
        let moved = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains(".corrupt-"));
        assert!(moved, "손상 파일은 지우지 않고 옆으로 옮긴다");
    }

    #[test]
    fn chat_op_gate_last_request_wins() {
        let gate = ChatOpGate::default();
        let ran = |gate: &ChatOpGate, key: &str, op| gate.run_if_latest(key, op, || ()).is_some();
        // 해제(1) 뒤 연결(2)을 보냈는데 연결이 먼저 돌았다 — 늦게 온 해제는 버린다.
        assert!(ran(&gate, "ml", Some(2)));
        assert!(
            !ran(&gate, "ml", Some(1)),
            "오래된 해제가 새 연결을 끊지 않는다"
        );
        assert!(gate.is_latest("ml", Some(2)));
        // 토큰을 받는 동안 더 새 요청이 오면 먼저 받은 연결은 물러난다.
        assert!(ran(&gate, "ml", Some(3)));
        assert!(!gate.is_latest("ml", Some(2)));
        assert!(!ran(&gate, "ml", Some(3)), "같은 순번은 한 번만");
        // 세션 키마다 따로 센다.
        assert!(ran(&gate, "mlx:a", Some(1)));
        assert!(ran(&gate, MAIN_CHAT_SESSION, Some(1)));
        // 순번 없는 호출은 늘 받는다(예전 호출부).
        assert!(ran(&gate, "ml", None));
        assert!(gate.is_latest("ml", None));
    }

    /// 여러 작업 스레드에서 순번이 뒤섞여 들어와도 잠금 안 구간은 받아들인 순번이 늘어나는 순서로만 돈다.
    #[test]
    fn chat_op_gate_serializes_admitted_sections() {
        let gate = Arc::new(ChatOpGate::default());
        let order = Arc::new(Mutex::new(Vec::new()));
        let threads: Vec<_> = (1..=64u64)
            .rev()
            .map(|op| {
                let (gate, order) = (gate.clone(), order.clone());
                std::thread::spawn(move || {
                    gate.run_if_latest("ml", Some(op), || order.lock().unwrap().push(op));
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let order = order.lock().unwrap();
        assert!(!order.is_empty());
        assert!(
            order.windows(2).all(|w| w[0] < w[1]),
            "받아들인 요청은 순번 순: {order:?}"
        );
        assert!(gate.is_latest("ml", Some(*order.last().unwrap())));
    }

    #[test]
    fn chat_session_key_defaults_to_main() {
        assert_eq!(chat_session_key(None), MAIN_CHAT_SESSION);
        assert_eq!(chat_session_key(Some("")), MAIN_CHAT_SESSION);
        assert_eq!(chat_session_key(Some("ml")), "ml");
    }

    #[test]
    fn release_app_keeps_its_keychain_service() {
        assert_eq!(
            keychain_service_name("app.cview.rust"),
            KEYCHAIN_SERVICE_NAME
        );
        assert_ne!(
            keychain_service_name("app.cview.rust.test"),
            KEYCHAIN_SERVICE_NAME
        );
    }

    #[test]
    fn keychain_service_is_separate_from_swift_app() {
        assert_ne!(
            KEYCHAIN_SERVICE_NAME,
            cview_auth::secret_store::DEFAULT_SERVICE_NAME
        );
    }
}
