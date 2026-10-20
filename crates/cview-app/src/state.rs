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
    /// `chat://session-event` — 추가 채팅 세션(멀티채팅) `{ session, event }`.
    pub chat_session_event: Arc<dyn Fn(String, ChatEventDto) + Send + Sync>,
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

/// 기본 채팅 세션 키.
pub const MAIN_CHAT_SESSION: &str = "main";

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
        let keychain = Arc::new(KeychainService::new(KEYCHAIN_SERVICE_NAME));
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
        let sink: Arc<dyn Fn(ChatEventDto) + Send + Sync> =
            Arc::new(move |dto| emit(session_key.clone(), dto));
        let vm = ChatViewModel::with_deps(ChatDeps {
            engine_factory: chat_engine_factory(self.cookies.clone(), sink),
            ..ChatDeps::default()
        });
        map.insert(key.to_string(), vm.clone());
        vm
    }

    /// 추가 채팅 세션을 끊고 지운다.
    pub async fn drop_chat_session(&self, session: &str) {
        self.chat_session_windows
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(session);
        let vm = self
            .chat_sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(session);
        if let Some(vm) = vm {
            vm.disconnect().await;
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
    fn keychain_service_is_separate_from_swift_app() {
        assert_ne!(
            KEYCHAIN_SERVICE_NAME,
            cview_auth::secret_store::DEFAULT_SERVICE_NAME
        );
    }
}
