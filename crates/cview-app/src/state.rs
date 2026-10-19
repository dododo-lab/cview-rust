//! Tauri 관리 상태 — 하위 크레이트 서비스를 한데 묶는다 (Swift `AppDependencies` 의 최소판).
//!
//! `cview-app-core` 가 이식되면 이 구조체를 그 쪽 서비스 컨테이너로 바꾸고, 명령 계층
//! (`commands`)은 같은 DTO 를 돌려주도록 유지한다.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use cview_auth::{
    AuthManager, ChzzkOAuthService, CookieJar, CookieManager, KeychainService, OAuthConfig,
    WebViewDataStore,
};
use cview_chat::ChatEngine;
use cview_networking::{ChzzkApiClient, ClientConfig, ReqwestTransport};
use cview_persistence::settings_store::MemoryDefaults;
use cview_persistence::{DataStore, SettingsStore};
use cview_player::coordinator::StreamCoordinator;
use tauri::async_runtime::JoinHandle;

use crate::engine::WebViewHlsEngine;

/// 스토어 파일 이름 (Swift `CView_v2.store`).
pub const STORE_FILE_NAME: &str = "CView_v2.store";
/// 설정 외부 미러 파일 이름.
pub const SETTINGS_SNAPSHOT_FILE_NAME: &str = "settings.json";

/// 진행 중인 채팅 연결.
pub struct ChatSession {
    pub engine: ChatEngine,
    pub chat_channel_id: String,
    pub pump: JoinHandle<()>,
}

/// 진행 중인 라이브 재생.
pub struct PlayerSession {
    pub channel_id: String,
    pub coordinator: StreamCoordinator,
    pub engine: Arc<WebViewHlsEngine>,
    pub pump: JoinHandle<()>,
}

/// 앱 전역 서비스.
pub struct AppServices {
    pub api: ChzzkApiClient,
    pub auth: AuthManager,
    pub cookies: Arc<CookieManager>,
    pub oauth: Arc<ChzzkOAuthService>,
    pub data_store: Option<DataStore>,
    pub settings: SettingsStore,
    pub chat: tokio::sync::Mutex<Option<ChatSession>>,
    pub player: tokio::sync::Mutex<Option<PlayerSession>>,
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
    pub async fn initialize(data_dir: &Path, webview: Arc<dyn WebViewDataStore>) -> Self {
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

        Self {
            api,
            auth,
            cookies,
            oauth,
            data_store,
            settings,
            chat: tokio::sync::Mutex::new(None),
            player: tokio::sync::Mutex::new(None),
            login_task: Mutex::new(None),
        }
    }

    /// 종료 정리 — 재생·채팅을 멈추고 대기 중인 설정 저장을 마친다.
    pub async fn shutdown(&self) {
        if let Some(session) = self.player.lock().await.take() {
            session.coordinator.stop_stream().await;
            session.pump.abort();
        }
        if let Some(session) = self.chat.lock().await.take() {
            session.engine.disconnect().await;
            session.pump.abort();
        }
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
