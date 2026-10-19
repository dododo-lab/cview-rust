//! 설정 저장소 — 도메인별 설정 + DB 영속 + 외부 미러(settings.json).
//!
//! 원본: `Sources/CViewPersistence/SettingsStore.swift` 와 `SettingsStore+SnapshotRestore.swift`.
//!
//! Swift `@Observable @MainActor final class` 는 `Arc` 공유 구조체 + 내부 `Mutex` 다. 도메인 대입은
//! `set_*`/`update_*` 로 하고, 그 뒤 자동으로 0.5초 디바운스 저장을 예약한다(tokio 런타임이 있을 때).
//!
//! 차이:
//! - `ChatSettings` 는 Rust `cview-core` 에 아직 없어 채팅 도메인을 불투명 JSON(`serde_json::Value`,
//!   기본 `{}`)으로 보관·왕복한다.
//! - `NotificationCenter` 의 `cviewStreamProxyModeChanged` 는 `tokio::sync::broadcast` 채널이다.
//! - `UserDefaults` 는 [`SettingsDefaults`] trait 이다(키 `multiChatShouldClear`).
//! - 저장 직렬화(`_persistChain`)는 비동기 뮤텍스 + 인코딩 순번이다. 나중에 인코딩한 스냅샷이 먼저
//!   쓰였으면 앞 스냅샷은 건너뛴다(마지막 인코딩 승자 — 원본 불변식과 같다).

use crate::error::Result;
use crate::settings_snapshot_file::SettingsSnapshotFile;
use crate::store::DataStore;
use cview_core::models::settings::{
    AppearanceSettings, ExperimentalPlaybackFlags, GeneralSettings, NetworkSettings, PlayerSettings,
};
use cview_core::models::settings_ui::{
    ChannelNotificationSettings, KeyboardShortcutSettings, MultiChatSettings, MultiLiveSettings,
    SyncSettings,
};
use cview_core::models::StreamProxyMode;
use serde::de::DeserializeOwned;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// 멀티채팅 초기화 플래그 키.
pub const MULTI_CHAT_SHOULD_CLEAR_KEY: &str = "multiChatShouldClear";

/// `load()` 가 배치로 읽는 도메인 키(`persistedDomainKeys`).
pub const PERSISTED_DOMAIN_KEYS: [&str; 11] = [
    "player",
    "chat",
    "general",
    "appearance",
    "network",
    "keyboard",
    "channelNotifications",
    "multiLive",
    "multiChat",
    "multiChatStandalone",
    "sync",
];

/// 도메인 키 집합.
pub fn persisted_domain_keys() -> HashSet<String> {
    PERSISTED_DOMAIN_KEYS
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// `UserDefaults` 대응.
pub trait SettingsDefaults: Send + Sync {
    fn bool(&self, key: &str) -> bool;
    fn set_bool(&self, key: &str, value: bool);
    fn remove(&self, key: &str);
}

/// 메모리 `UserDefaults`.
#[derive(Debug, Default)]
pub struct MemoryDefaults {
    values: Mutex<HashMap<String, bool>>,
}

impl SettingsDefaults for MemoryDefaults {
    fn bool(&self, key: &str) -> bool {
        self.values
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(key)
            .copied()
            .unwrap_or(false)
    }
    fn set_bool(&self, key: &str, value: bool) {
        self.values
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key.into(), value);
    }
    fn remove(&self, key: &str) {
        self.values
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(key);
    }
}

/// 설정 도메인 묶음.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsDomains {
    pub player: PlayerSettings,
    /// `ChatSettings` 미포팅 — 불투명 JSON.
    pub chat: serde_json::Value,
    pub general: GeneralSettings,
    pub appearance: AppearanceSettings,
    pub network: NetworkSettings,
    pub keyboard: KeyboardShortcutSettings,
    pub channel_notifications: ChannelNotificationSettings,
    pub multi_live: MultiLiveSettings,
    pub multi_chat: MultiChatSettings,
    /// 별도 사이드바 "멀티채팅" 메뉴 전용 독립 슬롯.
    pub multi_chat_standalone: MultiChatSettings,
    pub sync: SyncSettings,
}

impl Default for SettingsDomains {
    fn default() -> Self {
        Self {
            player: PlayerSettings::default(),
            chat: serde_json::Value::Object(Default::default()),
            general: GeneralSettings::default(),
            appearance: AppearanceSettings::default(),
            network: NetworkSettings::default(),
            keyboard: KeyboardShortcutSettings::default(),
            channel_notifications: ChannelNotificationSettings::default(),
            multi_live: MultiLiveSettings::default(),
            multi_chat: MultiChatSettings::default(),
            multi_chat_standalone: MultiChatSettings::default(),
            sync: SyncSettings::default(),
        }
    }
}

impl SettingsDomains {
    /// 전 도메인을 키별 키 정렬 JSON 바이트로(`encodeAllDomains`).
    pub fn encode_all(&self) -> Result<HashMap<String, Vec<u8>>> {
        use crate::json::to_sorted_vec as enc;
        Ok(HashMap::from([
            ("player".to_string(), enc(&self.player)?),
            ("chat".to_string(), enc(&self.chat)?),
            ("general".to_string(), enc(&self.general)?),
            ("appearance".to_string(), enc(&self.appearance)?),
            ("network".to_string(), enc(&self.network)?),
            ("keyboard".to_string(), enc(&self.keyboard)?),
            (
                "channelNotifications".to_string(),
                enc(&self.channel_notifications)?,
            ),
            ("multiLive".to_string(), enc(&self.multi_live)?),
            ("multiChat".to_string(), enc(&self.multi_chat)?),
            (
                "multiChatStandalone".to_string(),
                enc(&self.multi_chat_standalone)?,
            ),
            ("sync".to_string(), enc(&self.sync)?),
        ]))
    }
}

/// 복원 write 경로 — 기본은 `DataStore::save_settings`. 테스트가 실패를 주입하는 seam.
pub type RestoreWriter =
    Arc<dyn Fn(&DataStore, &HashMap<String, Vec<u8>>) -> Result<()> + Send + Sync>;

/// 복원 단계의 결과(`SnapshotRestore`).
#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotRestore {
    /// store 에 설정 도메인 행이 하나도 없었는가.
    pub store_is_empty: bool,
    /// 복원 write 가 실패해 쓰지 못한 스냅샷. 있으면 이번 세션은 표시 전용이다.
    pub unrestored_snapshot: Option<HashMap<String, Vec<u8>>>,
}

struct State {
    domains: SettingsDomains,
    is_loaded: bool,
    can_persist: bool,
    data_store: Option<DataStore>,
    debounce: Option<tokio::task::JoinHandle<()>>,
}

struct PersistState {
    last_written_seq: u64,
    last_mirrored: Option<HashMap<String, Vec<u8>>>,
}

struct Inner {
    state: Mutex<State>,
    snapshot_path: Option<PathBuf>,
    defaults: Arc<dyn SettingsDefaults>,
    persist_chain: tokio::sync::Mutex<PersistState>,
    encode_seq: AtomicU64,
    proxy_tx: tokio::sync::broadcast::Sender<StreamProxyMode>,
    restore_writer: Mutex<Option<RestoreWriter>>,
}

/// 설정 저장소. 복제는 같은 상태를 공유한다.
#[derive(Clone)]
pub struct SettingsStore {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for SettingsStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsStore")
            .field("snapshot_path", &self.inner.snapshot_path)
            .finish()
    }
}

macro_rules! domain_accessors {
    ($( $field:ident, $set:ident, $update:ident : $ty:ty );* $(;)?) => {
        $(
            #[doc = concat!("`", stringify!($field), "` 도메인 사본.")]
            pub fn $field(&self) -> $ty {
                self.state().domains.$field.clone()
            }

            #[doc = concat!("`", stringify!($field), "` 도메인 대입 — 자동 디바운스 저장을 예약한다.")]
            pub fn $set(&self, value: $ty) {
                self.state().domains.$field = value;
                self.autosave();
            }

            #[doc = concat!("`", stringify!($field), "` 도메인을 고친다 — 자동 디바운스 저장을 예약한다.")]
            pub fn $update(&self, f: impl FnOnce(&mut $ty)) {
                {
                    let mut state = self.state();
                    f(&mut state.domains.$field);
                }
                self.autosave();
            }
        )*
    };
}

fn decode_logged<T: DeserializeOwned>(raw: &HashMap<String, Vec<u8>>, key: &str) -> Option<T> {
    let data = raw.get(key)?;
    match serde_json::from_slice(data) {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::warn!("Setting '{key}' load failed: {e}");
            None
        }
    }
}

impl SettingsStore {
    /// `data_store` 가 없으면 `configure` 전까지 저장하지 않는다.
    /// `snapshot_path` 가 `None` 이면 외부 미러를 쓰지 않는다.
    pub fn new(
        data_store: Option<DataStore>,
        snapshot_path: Option<PathBuf>,
        defaults: Arc<dyn SettingsDefaults>,
    ) -> Self {
        let (proxy_tx, _) = tokio::sync::broadcast::channel(16);
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State {
                    domains: SettingsDomains::default(),
                    is_loaded: false,
                    can_persist: false,
                    data_store,
                    debounce: None,
                }),
                snapshot_path,
                defaults,
                persist_chain: tokio::sync::Mutex::new(PersistState {
                    last_written_seq: 0,
                    last_mirrored: None,
                }),
                encode_seq: AtomicU64::new(0),
                proxy_tx,
                restore_writer: Mutex::new(None),
            }),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.inner.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// load 완료 신호(권위 있는 값을 읽었다는 뜻은 아니다).
    pub fn is_loaded(&self) -> bool {
        self.state().is_loaded
    }

    /// 영속화 허용 — 디스크에서 권위 있는 값을 실제로 읽었을 때만 true.
    pub fn can_persist(&self) -> bool {
        self.state().can_persist
    }

    /// 전 도메인 사본.
    pub fn domains(&self) -> SettingsDomains {
        self.state().domains.clone()
    }

    /// 스트림 보정 모드 변경 구독(`cviewStreamProxyModeChanged`).
    pub fn subscribe_stream_proxy_mode(&self) -> tokio::sync::broadcast::Receiver<StreamProxyMode> {
        self.inner.proxy_tx.subscribe()
    }

    /// 테스트 seam: 스냅샷 복원 write 를 바꾼다(`snapshotRestoreWriter`).
    #[doc(hidden)]
    pub fn set_snapshot_restore_writer(&self, writer: Option<RestoreWriter>) {
        *self
            .inner
            .restore_writer
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = writer;
    }

    // MARK: - 도메인 접근

    /// 플레이어 설정 사본.
    pub fn player(&self) -> PlayerSettings {
        self.state().domains.player.clone()
    }

    /// 플레이어 설정 대입 — 보정 모드가 바뀌면 알리고 실험 플래그를 반영한 뒤 자동 저장을 예약한다.
    pub fn set_player(&self, value: PlayerSettings) {
        self.apply_player(value);
        self.autosave();
    }

    /// 플레이어 설정을 고친다.
    pub fn update_player(&self, f: impl FnOnce(&mut PlayerSettings)) {
        let mut value = self.player();
        f(&mut value);
        self.set_player(value);
    }

    fn apply_player(&self, value: PlayerSettings) {
        let old_mode = {
            let mut state = self.state();
            let old = state.domains.player.stream_proxy_mode;
            state.domains.player = value.clone();
            old
        };
        if old_mode != value.stream_proxy_mode {
            let _ = self.inner.proxy_tx.send(value.stream_proxy_mode);
        }
        ExperimentalPlaybackFlags::set_server_time_machine(value.experimental_server_time_machine);
        ExperimentalPlaybackFlags::set_match_web_v2_setting_value(value.experimental_match_web_v2);
    }

    domain_accessors! {
        chat, set_chat, update_chat: serde_json::Value;
        general, set_general, update_general: GeneralSettings;
        appearance, set_appearance, update_appearance: AppearanceSettings;
        network, set_network, update_network: NetworkSettings;
        keyboard, set_keyboard, update_keyboard: KeyboardShortcutSettings;
        channel_notifications, set_channel_notifications, update_channel_notifications: ChannelNotificationSettings;
        multi_live, set_multi_live, update_multi_live: MultiLiveSettings;
        multi_chat, set_multi_chat, update_multi_chat: MultiChatSettings;
        multi_chat_standalone, set_multi_chat_standalone, update_multi_chat_standalone: MultiChatSettings;
        sync, set_sync, update_sync: SyncSettings;
    }

    /// 채널별 알림 설정 스냅샷.
    pub fn channel_notifications_snapshot(&self) -> ChannelNotificationSettings {
        self.channel_notifications()
    }

    // MARK: - 연결·로드

    /// DataStore 연결 후 설정 로드.
    pub async fn configure(&self, data_store: DataStore) {
        self.state().data_store = Some(data_store);
        self.load().await;
    }

    /// 외부 스냅샷 복원 단계 — store 에 설정 도메인 행이 없고 스냅샷이 있으면 먼저 store 로 복원한다.
    pub async fn restore_snapshot_if_store_empty(
        &self,
        store: &DataStore,
        domain_keys: &HashSet<String>,
    ) -> SnapshotRestore {
        let keys: Vec<String> = domain_keys.iter().cloned().collect();
        let store_is_empty = !store
            .off_main_uncancellable(move |s| s.has_any_setting(&keys))
            .await;
        if !store_is_empty {
            return SnapshotRestore {
                store_is_empty,
                unrestored_snapshot: None,
            };
        }
        let Some(domains) =
            SettingsSnapshotFile::read(self.inner.snapshot_path.as_deref()).map(|m| {
                m.into_iter()
                    .filter(|(k, _)| domain_keys.contains(k))
                    .collect::<HashMap<_, _>>()
            })
        else {
            return SnapshotRestore {
                store_is_empty,
                unrestored_snapshot: None,
            };
        };
        let writer: RestoreWriter = self
            .inner
            .restore_writer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .unwrap_or_else(|| {
                Arc::new(|s: &DataStore, d: &HashMap<String, Vec<u8>>| s.save_settings(d))
            });
        let payload = domains.clone();
        match store.off_main(move |s| writer(s, &payload)).await {
            Ok(()) => {
                tracing::info!(
                    "Settings restored from external snapshot ({} domains)",
                    domains.len()
                );
                SnapshotRestore {
                    store_is_empty: true,
                    unrestored_snapshot: None,
                }
            }
            Err(e) => {
                tracing::warn!("Settings snapshot restore failed: {e}");
                SnapshotRestore {
                    store_is_empty: true,
                    unrestored_snapshot: Some(domains),
                }
            }
        }
    }

    /// 저장된 설정 로드.
    pub async fn load(&self) {
        let Some(store) = self.state().data_store.clone() else {
            return;
        };
        let domain_keys = persisted_domain_keys();
        let restore = self
            .restore_snapshot_if_store_empty(&store, &domain_keys)
            .await;
        let store_is_empty = restore.store_is_empty;
        let should_clear_multi_chat = self.inner.defaults.bool(MULTI_CHAT_SHOULD_CLEAR_KEY);

        let (raw, did_load_persisted) = if let Some(unrestored) = restore.unrestored_snapshot {
            (unrestored, false)
        } else {
            let keys = domain_keys.clone();
            match store.off_main(move |s| s.load_settings_raw(&keys)).await {
                Ok(raw) => (raw, true),
                Err(e) => {
                    tracing::warn!("Settings batch load failed: {e}");
                    let mirrored = SettingsSnapshotFile::read(self.inner.snapshot_path.as_deref())
                        .map(|m| {
                            m.into_iter()
                                .filter(|(k, _)| domain_keys.contains(k))
                                .collect::<HashMap<_, _>>()
                        })
                        .unwrap_or_default();
                    if !mirrored.is_empty() {
                        tracing::warn!(
                            "Settings batch load failed — 복구 미러 {}개 도메인을 표시용으로만 적용 (이번 세션 저장 차단)",
                            mirrored.len()
                        );
                    }
                    (mirrored, false)
                }
            }
        };

        let p: Option<PlayerSettings> = decode_logged(&raw, "player");
        let c: Option<serde_json::Value> = decode_logged(&raw, "chat");
        let g: Option<GeneralSettings> = decode_logged(&raw, "general");
        let a: Option<AppearanceSettings> = decode_logged(&raw, "appearance");
        let n: Option<NetworkSettings> = decode_logged(&raw, "network");
        let k: Option<KeyboardShortcutSettings> = decode_logged(&raw, "keyboard");
        let cn: Option<ChannelNotificationSettings> = decode_logged(&raw, "channelNotifications");
        let ml: Option<MultiLiveSettings> = decode_logged(&raw, "multiLive");
        let mc: Option<MultiChatSettings> = decode_logged(&raw, "multiChat");
        let mcs: Option<MultiChatSettings> = decode_logged(&raw, "multiChatStandalone");
        let sy: Option<SyncSettings> = decode_logged(&raw, "sync");

        let present_ok = |key: &str, decoded: bool| !raw.contains_key(key) || decoded;
        let did_decode_all = present_ok("player", p.is_some())
            && present_ok("chat", c.is_some())
            && present_ok("general", g.is_some())
            && present_ok("appearance", a.is_some())
            && present_ok("network", n.is_some())
            && present_ok("keyboard", k.is_some())
            && present_ok("channelNotifications", cn.is_some())
            && present_ok("multiLive", ml.is_some())
            && present_ok("multiChat", mc.is_some())
            && present_ok("multiChatStandalone", mcs.is_some())
            && present_ok("sync", sy.is_some());
        let can_persist_multi_chat_cleanup =
            should_clear_multi_chat && did_load_persisted && did_decode_all;

        // 같은 값이면 대입하지 않는다. 대입은 원본 didSet 처럼 자동 저장을 예약할 수 있다(이미 로드된 경우).
        if let Some(v) = p {
            if v != self.player() {
                self.set_player(v);
            }
        }
        macro_rules! assign {
            ($val:expr, $get:ident, $set:ident) => {
                if let Some(v) = $val {
                    if v != self.$get() {
                        self.$set(v);
                    }
                }
            };
        }
        assign!(c, chat, set_chat);
        assign!(g, general, set_general);
        assign!(a, appearance, set_appearance);
        assign!(n, network, set_network);
        assign!(k, keyboard, set_keyboard);
        assign!(cn, channel_notifications, set_channel_notifications);
        assign!(ml, multi_live, set_multi_live);
        if should_clear_multi_chat {
            let cleared = mc
                .unwrap_or_else(|| self.multi_chat())
                .clearing_restorable_sessions();
            let cleared_standalone = mcs
                .unwrap_or_else(|| self.multi_chat_standalone())
                .clearing_restorable_sessions();
            if cleared != self.multi_chat() {
                self.set_multi_chat(cleared);
            }
            if cleared_standalone != self.multi_chat_standalone() {
                self.set_multi_chat_standalone(cleared_standalone);
            }
        } else {
            assign!(mc, multi_chat, set_multi_chat);
            assign!(mcs, multi_chat_standalone, set_multi_chat_standalone);
        }
        assign!(sy, sync, set_sync);

        let can_persist = did_load_persisted && did_decode_all;
        {
            let mut state = self.state();
            state.is_loaded = true;
            state.can_persist = can_persist;
        }
        if !can_persist {
            tracing::error!(
                "SettingsStore: 설정을 권위 있게 읽지 못해 이번 세션의 저장을 차단한다 (fetch={did_load_persisted}, decode={did_decode_all}) — 사용자 설정과 복구 미러의 기본값 클로버 방지"
            );
        }
        if can_persist_multi_chat_cleanup && self.save().await {
            self.inner.defaults.remove(MULTI_CHAT_SHOULD_CLEAR_KEY);
        }
        tracing::debug!("Settings loaded");

        // 최초 1회 시드 — store 에서 실제 설정을 읽었고 아직 외부 백업이 없을 때만.
        let path = self.inner.snapshot_path.clone();
        if !store_is_empty
            && did_load_persisted
            && did_decode_all
            && !SettingsSnapshotFile::exists(path.as_deref())
        {
            if let Ok(pairs) = self.domains().encode_all() {
                let _chain = self.inner.persist_chain.lock().await;
                let saved_at = crate::time::epoch_seconds(&crate::time::now());
                let _ = tokio::task::spawn_blocking(move || {
                    if !SettingsSnapshotFile::exists(path.as_deref()) {
                        SettingsSnapshotFile::write(&pairs, saved_at, path.as_deref());
                    }
                })
                .await;
            }
        }
    }

    // MARK: - 저장

    /// 설정 저장 — DB 와 복구 파일을 같은 스냅샷 순서로 기록한다.
    pub async fn save(&self) -> bool {
        let (store, pairs, seq) = {
            let state = self.state();
            let Some(store) = state.data_store.clone() else {
                return false;
            };
            if !state.is_loaded {
                tracing::warn!("SettingsStore: load() 완료 전 save 호출 무시 — 기본값 클로버 방지");
                return false;
            }
            if !state.can_persist {
                tracing::warn!(
                    "SettingsStore: 권위 있는 로드에 실패한 세션의 save 호출 무시 — 기본값 클로버 방지"
                );
                return false;
            }
            let pairs = match state.domains.encode_all() {
                Ok(p) => p,
                Err(e) => {
                    tracing::error!("Failed to save settings: {e}");
                    return false;
                }
            };
            debug_assert_eq!(
                pairs.keys().cloned().collect::<HashSet<_>>(),
                persisted_domain_keys(),
                "encode_all() 과 PERSISTED_DOMAIN_KEYS 가 어긋났다"
            );
            let seq = self.inner.encode_seq.fetch_add(1, Ordering::SeqCst) + 1;
            (store, pairs, seq)
        };

        let mut chain = self.inner.persist_chain.lock().await;
        if seq < chain.last_written_seq {
            // 더 나중에 인코딩한 스냅샷이 이미 쓰였다 — 이 스냅샷은 낡았다.
            return true;
        }
        let db_pairs = pairs.clone();
        match store.off_main(move |s| s.save_settings(&db_pairs)).await {
            Ok(()) => {
                chain.last_written_seq = seq;
                if chain.last_mirrored.as_ref() != Some(&pairs) {
                    let path = self.inner.snapshot_path.clone();
                    let saved_at = crate::time::epoch_seconds(&crate::time::now());
                    let mirror = pairs.clone();
                    let written = tokio::task::spawn_blocking(move || {
                        SettingsSnapshotFile::write(&mirror, saved_at, path.as_deref())
                    })
                    .await
                    .unwrap_or(false);
                    if written {
                        chain.last_mirrored = Some(pairs);
                    }
                }
                tracing::info!("Settings saved");
                true
            }
            Err(e) => {
                tracing::error!("Failed to save settings: {e}");
                false
            }
        }
    }

    /// 모든 설정 초기화 후 저장.
    pub async fn reset_all(&self) {
        let defaults = SettingsDomains::default();
        self.apply_player(defaults.player.clone());
        {
            let mut state = self.state();
            let player = state.domains.player.clone();
            state.domains = SettingsDomains { player, ..defaults };
        }
        self.autosave();
        self.save().await;
    }

    fn autosave(&self) {
        let ok = {
            let state = self.state();
            state.is_loaded && state.can_persist
        };
        if ok {
            self.schedule_debounced_save();
        }
    }

    /// 0.5초 디바운스 — 연속 변경 시 마지막 1회만 저장. tokio 런타임 밖이면 아무것도 하지 않는다.
    pub fn schedule_debounced_save(&self) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let this = self.clone();
        let task = handle.spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            // 저장 자체는 취소하지 않는다(디바운스 취소는 잠든 동안만 효과가 있다 — 원본과 같다).
            tokio::spawn(async move {
                this.save().await;
            });
        });
        if let Some(previous) = self.state().debounce.replace(task) {
            previous.abort();
        }
    }

    /// 디바운스 대기 중인 저장을 즉시 확정한다 — 종료 직전 플러시용.
    pub async fn flush_pending_save(&self) {
        let (ok, pending) = {
            let mut state = self.state();
            (state.is_loaded && state.can_persist, state.debounce.take())
        };
        if let Some(task) = pending {
            task.abort();
        }
        if ok {
            self.save().await;
        }
    }
}
