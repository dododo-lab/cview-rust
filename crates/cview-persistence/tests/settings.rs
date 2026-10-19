//! 원본: `Tests/CViewPersistenceTests/SettingsSnapshotOrderingTests.swift`,
//! `PrecisionReview0926SettingsRestoreTests.swift`, `SettingsBatchLoadTests.swift`,
//! `SettingsSnapshotVersionGateTests.swift`, `KeyboardShortcutEncodingTests.swift`,
//! `ChatBackgroundPersistenceTests.swift`·`ChatDesignStylePersistenceTests.swift`(채팅 도메인 왕복).
//!
//! `ChatSettings` 는 Rust core 에 없어 채팅 도메인은 불투명 JSON 이다 — 채팅 배경·디자인 스타일
//! 테스트는 "임의 채팅 필드가 DB·미러를 거쳐 바이트 그대로 왕복한다" 로 옮겼다.

mod common;

use common::*;
use cview_core::models::settings::PlayerSettings;
use cview_core::models::settings_ui::{
    KeyBinding, KeyboardShortcutSettings, ShortcutAction, ShortcutModifiers, SyncSettings,
};
use cview_persistence::settings_store::{
    persisted_domain_keys, MemoryDefaults, PERSISTED_DOMAIN_KEYS,
};
use cview_persistence::store::SETTING_KEY_CHUNK_SIZE;
use cview_persistence::{DataStore, PersistenceError, SettingsSnapshotFile, SettingsStore};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct Fixture {
    _dir: tempfile::TempDir,
    store: SettingsStore,
    data: DataStore,
    url: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let url = dir.path().join("settings.json");
    let data = common::store();
    let store = SettingsStore::new(
        Some(data.clone()),
        Some(url.clone()),
        Arc::new(MemoryDefaults::default()),
    );
    Fixture {
        _dir: dir,
        store,
        data,
        url,
    }
}

fn mirror_player(url: &Path) -> Option<PlayerSettings> {
    let raw = SettingsSnapshotFile::read(Some(url))?.remove("player")?;
    serde_json::from_slice(&raw).ok()
}

fn seed_cache_rows_only(data: &DataStore) {
    data.save_setting("cache.liveChannels", &Vec::<String>::new())
        .unwrap();
    data.save_setting("cache.liveChannels.ts", &1_780_000_000.0)
        .unwrap();
}

fn write_snapshot(volume: f32, url: &Path, extra: &[(&str, Vec<u8>)]) {
    let player = PlayerSettings {
        volume_level: volume,
        ..PlayerSettings::default()
    };
    let mut domains = HashMap::from([
        ("player".to_string(), serde_json::to_vec(&player).unwrap()),
        ("chat".to_string(), b"{\"fontSize\":14}".to_vec()),
    ]);
    for (k, v) in extra {
        domains.insert(k.to_string(), v.clone());
    }
    assert!(SettingsSnapshotFile::write(&domains, 1.0, Some(url)));
    assert!(SettingsSnapshotFile::read(Some(url)).is_some());
}

fn domain_keys() -> Vec<String> {
    PERSISTED_DOMAIN_KEYS
        .iter()
        .map(|s| s.to_string())
        .collect()
}

fn bump_chat_font(store: &SettingsStore) {
    store.update_chat(|c| {
        let next = c.get("fontSize").and_then(|v| v.as_i64()).unwrap_or(14) + 1;
        c["fontSize"] = json!(next);
    });
}

// MARK: - DB 와 복구 파일 일관성

#[tokio::test]
async fn save_completion_includes_snapshot() {
    let f = fixture();
    f.store.load().await;
    f.store.update_player(|p| p.volume_level = 0.37);
    assert!(f.store.save().await);
    let mirror = mirror_player(&f.url).unwrap();
    let persisted: PlayerSettings = f.data.load_setting("player").unwrap().unwrap();
    assert_eq!(mirror.volume_level, 0.37);
    assert_eq!(mirror, persisted);
}

#[tokio::test]
async fn unchanged_domains_skip_mirror_rewrite() {
    let f = fixture();
    f.store.load().await;
    f.store.update_player(|p| p.volume_level = 0.51);
    assert!(f.store.save().await);
    assert!(f.url.exists());
    std::fs::remove_file(&f.url).unwrap();
    assert!(f.store.save().await);
    assert!(!f.url.exists(), "같은 설정인데 복구 파일을 다시 썼다");
    f.store.update_player(|p| p.volume_level = 0.52);
    assert!(f.store.save().await);
    assert_eq!(mirror_player(&f.url).unwrap().volume_level, 0.52);
    let persisted: PlayerSettings = f.data.load_setting("player").unwrap().unwrap();
    assert_eq!(persisted.volume_level, 0.52);
}

#[tokio::test]
async fn unchanged_save_keeps_setting_rows() {
    let f = fixture();
    f.store.load().await;
    f.store.update_player(|p| p.volume_level = 0.61);
    assert!(f.store.save().await);
    let before = f.data.load_settings_raw(&persisted_domain_keys()).unwrap();
    let history = f.data.history_transaction_count();
    for _ in 0..3 {
        assert!(f.store.save().await);
    }
    let after = f.data.load_settings_raw(&persisted_domain_keys()).unwrap();
    assert_eq!(before, after);
    assert_eq!(f.data.history_transaction_count(), history);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn overlapping_saves_keep_latest_snapshot() {
    let f = fixture();
    f.store.load().await;
    let mut writes = Vec::new();
    for i in 1..=20 {
        f.store.update_player(|p| p.volume_level = i as f32 / 100.0);
        let s = f.store.clone();
        writes.push(tokio::spawn(async move { s.save().await }));
        tokio::task::yield_now().await;
    }
    for w in writes {
        assert!(w.await.unwrap());
    }
    assert!(f.store.save().await);
    let mirror = mirror_player(&f.url).unwrap();
    let persisted: PlayerSettings = f.data.load_setting("player").unwrap().unwrap();
    assert_eq!(mirror.volume_level, 0.2);
    assert_eq!(mirror, persisted);
}

#[tokio::test]
async fn partial_decode_does_not_poison_snapshot() {
    let f = fixture();
    f.data
        .save_settings(&HashMap::from([
            ("player".to_string(), b"{bad-json".to_vec()),
            ("chat".to_string(), b"{}".to_vec()),
        ]))
        .unwrap();
    f.store.load().await;
    assert!(f.store.is_loaded());
    assert!(!SettingsSnapshotFile::exists(Some(&f.url)));
}

#[tokio::test]
async fn healthy_load_seeds_snapshot() {
    let f = fixture();
    let player = PlayerSettings {
        volume_level: 0.42,
        ..PlayerSettings::default()
    };
    f.data.save_setting("player", &player).unwrap();
    f.store.load().await;
    assert_eq!(mirror_player(&f.url).unwrap(), player);
}

// MARK: - 캐시 행만 남은 store 의 복원

#[tokio::test]
async fn cache_only_store_restores_snapshot() {
    let f = fixture();
    seed_cache_rows_only(&f.data);
    write_snapshot(0.42, &f.url, &[]);
    f.store.load().await;
    assert_eq!(f.store.player().volume_level, 0.42);
    let persisted: PlayerSettings = f.data.load_setting("player").unwrap().unwrap();
    assert_eq!(persisted.volume_level, 0.42);
    assert!(f.store.can_persist());
    let cache = f
        .data
        .load_settings_raw(&HashSet::from(["cache.liveChannels".to_string()]))
        .unwrap();
    assert!(cache.contains_key("cache.liveChannels"));
}

#[tokio::test]
async fn save_after_restore_keeps_snapshot_values() {
    let f = fixture();
    seed_cache_rows_only(&f.data);
    write_snapshot(0.42, &f.url, &[]);
    f.store.load().await;
    bump_chat_font(&f.store);
    assert!(f.store.save().await);
    assert_eq!(mirror_player(&f.url).unwrap().volume_level, 0.42);
}

#[tokio::test]
async fn domain_rows_win_over_snapshot() {
    let f = fixture();
    let player = PlayerSettings {
        volume_level: 0.30,
        ..PlayerSettings::default()
    };
    f.data.save_setting("player", &player).unwrap();
    seed_cache_rows_only(&f.data);
    write_snapshot(0.42, &f.url, &[]);
    f.store.load().await;
    assert_eq!(f.store.player().volume_level, 0.30);
}

#[test]
fn has_any_setting_counts_domain_keys_only() {
    let data = store();
    let keys = domain_keys();
    assert!(!data.has_any_setting(&keys));
    seed_cache_rows_only(&data);
    assert!(!data.has_any_setting(&keys));
    data.save_setting("sync", &SyncSettings::default()).unwrap();
    assert!(data.has_any_setting(&keys));
}

#[tokio::test]
async fn snapshot_non_domain_keys_are_ignored() {
    let f = fixture();
    f.data
        .save_setting("cache.liveChannels", &Vec::<String>::new())
        .unwrap();
    write_snapshot(
        0.42,
        &f.url,
        &[("cache.liveChannels", serde_json::to_vec(&["x"]).unwrap())],
    );
    f.store.load().await;
    assert_eq!(f.store.player().volume_level, 0.42);
    let cache: Vec<String> = f.data.load_setting("cache.liveChannels").unwrap().unwrap();
    assert!(cache.is_empty());
}

#[tokio::test]
async fn failed_restore_write_blocks_persistence() {
    let f = fixture();
    seed_cache_rows_only(&f.data);
    write_snapshot(0.42, &f.url, &[]);
    let mirror_before = std::fs::read(&f.url).unwrap();
    f.store.set_snapshot_restore_writer(Some(Arc::new(|_, _| {
        Err(PersistenceError::Other("injected restore failure".into()))
    })));
    f.store.load().await;
    assert!(f.store.is_loaded());
    assert_eq!(f.store.player().volume_level, 0.42);
    assert!(!f.store.can_persist());
    assert!(!f.data.has_any_setting(&domain_keys()));

    bump_chat_font(&f.store);
    assert!(!f.store.save().await);
    assert_eq!(std::fs::read(&f.url).unwrap(), mirror_before);
    assert!(!f.data.has_any_setting(&domain_keys()));
}

// MARK: - 채팅 도메인(불투명 JSON) 왕복

#[tokio::test]
async fn chat_domain_round_trips_through_db_and_mirror() {
    let f = fixture();
    f.store.load().await;
    let chat = json!({
        "backgroundStyle": {"kind": "image", "opacity": 0.6},
        "designStyle": "bubble",
        "fontSize": 15,
    });
    f.store.set_chat(chat.clone());
    assert!(f.store.save().await);
    let persisted: serde_json::Value = f.data.load_setting("chat").unwrap().unwrap();
    assert_eq!(persisted, chat);
    let mirror = SettingsSnapshotFile::read(Some(&f.url)).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&mirror["chat"]).unwrap(),
        chat
    );

    let reloaded = SettingsStore::new(
        Some(f.data.clone()),
        Some(f.url.clone()),
        Arc::new(MemoryDefaults::default()),
    );
    reloaded.load().await;
    assert_eq!(reloaded.chat(), chat);
}

// MARK: - 배치 로드

#[test]
fn load_settings_raw_returns_only_requested_keys() {
    let s = store();
    s.save_settings(&HashMap::from([
        ("player".to_string(), b"{\"a\":1}".to_vec()),
        ("chat".to_string(), b"{\"b\":2}".to_vec()),
        ("cache.largeBlob".to_string(), vec![0; 512 * 1024]),
    ]))
    .unwrap();
    let raw = s
        .load_settings_raw(&HashSet::from([
            "player".to_string(),
            "chat".to_string(),
            "missing".to_string(),
        ]))
        .unwrap();
    assert_eq!(
        raw.keys().cloned().collect::<HashSet<_>>(),
        HashSet::from(["player".to_string(), "chat".to_string()])
    );
    assert_eq!(raw["player"], b"{\"a\":1}".to_vec());
}

#[test]
fn load_settings_raw_matches_load_setting() {
    let s = store();
    let settings = PlayerSettings::default();
    s.save_setting("player", &settings).unwrap();
    let raw = s
        .load_settings_raw(&HashSet::from(["player".to_string()]))
        .unwrap();
    let via_batch: PlayerSettings = serde_json::from_slice(&raw["player"]).unwrap();
    let direct: PlayerSettings = s.load_setting("player").unwrap().unwrap();
    assert_eq!(via_batch, direct);
    assert_eq!(via_batch, settings);
}

#[test]
fn save_settings_leaves_unrelated_keys_untouched() {
    let s = store();
    let blob = vec![0x7A; 1_200_000];
    s.save_settings(&HashMap::from([(
        "cache.largeBlob".to_string(),
        blob.clone(),
    )]))
    .unwrap();
    s.save_settings(&HashMap::from([
        ("player".to_string(), b"{\"v\":1}".to_vec()),
        ("sync".to_string(), b"{\"v\":2}".to_vec()),
    ]))
    .unwrap();
    let raw = s
        .load_settings_raw(&HashSet::from([
            "player".to_string(),
            "sync".to_string(),
            "cache.largeBlob".to_string(),
        ]))
        .unwrap();
    assert_eq!(raw["player"], b"{\"v\":1}".to_vec());
    assert_eq!(raw["sync"], b"{\"v\":2}".to_vec());
    assert_eq!(raw["cache.largeBlob"].len(), blob.len());
}

#[test]
fn load_settings_raw_chunks_beyond_limit() {
    let s = store();
    let count = SETTING_KEY_CHUNK_SIZE + 37;
    let pairs: HashMap<String, Vec<u8>> = (0..count)
        .map(|i| (format!("k{i}"), i.to_string().into_bytes()))
        .collect();
    s.save_settings(&pairs).unwrap();
    let raw = s
        .load_settings_raw(&pairs.keys().cloned().collect())
        .unwrap();
    assert_eq!(raw.len(), count);
    assert_eq!(
        raw[&format!("k{}", count - 1)],
        (count - 1).to_string().into_bytes()
    );
}

// MARK: - 스냅샷 버전 게이트

fn snapshot(version: Option<serde_json::Value>, settings: serde_json::Value) -> Vec<u8> {
    let mut root = json!({"savedAt": 1_700_000_000.0, "settings": settings});
    if let Some(v) = version {
        root["schemaVersion"] = v;
    }
    serde_json::to_vec(&root).unwrap()
}

fn network() -> serde_json::Value {
    json!({"network": {"retryCount": 7}})
}

#[test]
fn snapshot_current_version_restores() {
    let d = SettingsSnapshotFile::parse(&snapshot(
        Some(json!(SettingsSnapshotFile::SCHEMA_VERSION)),
        network(),
    ));
    assert!(d.unwrap().contains_key("network"));
}

#[test]
fn snapshot_older_version_still_restores() {
    let d = SettingsSnapshotFile::parse(&snapshot(Some(json!(0)), network()));
    assert!(d.unwrap().contains_key("network"));
}

#[test]
fn snapshot_future_version_blocked() {
    let future = SettingsSnapshotFile::SCHEMA_VERSION + 1;
    assert!(SettingsSnapshotFile::parse(&snapshot(Some(json!(future)), network())).is_none());
}

#[test]
fn snapshot_missing_or_malformed_version_blocked() {
    assert!(SettingsSnapshotFile::parse(&snapshot(None, network())).is_none());
    assert!(SettingsSnapshotFile::parse(&snapshot(Some(json!("1")), network())).is_none());
}

#[test]
fn snapshot_corrupt_inputs() {
    assert!(SettingsSnapshotFile::parse(b"not json").is_none());
    assert!(SettingsSnapshotFile::parse(&snapshot(
        Some(json!(SettingsSnapshotFile::SCHEMA_VERSION)),
        json!({})
    ))
    .is_none());
}

// MARK: - 키보드 단축키 인코딩(설정 도메인 결정성)

fn custom_bindings() -> Vec<(ShortcutAction, KeyBinding)> {
    ShortcutAction::ALL
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let m = if i % 2 == 0 {
                ShortcutModifiers::COMMAND
            } else {
                ShortcutModifiers::SHIFT | ShortcutModifiers::OPTION
            };
            (*a, KeyBinding::new(format!("k{i}"), m))
        })
        .collect()
}

fn encode(settings: &KeyboardShortcutSettings) -> Vec<u8> {
    cview_persistence::json::to_sorted_vec(settings).unwrap()
}

#[test]
fn keyboard_same_content_same_bytes() {
    let pairs = custom_bindings();
    let mut variants = Vec::new();
    for order in [pairs.clone(), pairs.iter().rev().cloned().collect()] {
        for cap in [0, 64, 257] {
            let mut map = HashMap::with_capacity(cap);
            map.extend(order.iter().cloned());
            variants.push(encode(&KeyboardShortcutSettings { bindings: map }));
        }
    }
    assert_eq!(variants.iter().collect::<HashSet<_>>().len(), 1);
    let default = encode(&KeyboardShortcutSettings::default());
    for _ in 0..4 {
        let rebuilt = KeyboardShortcutSettings {
            bindings: KeyboardShortcutSettings::default_bindings()
                .into_iter()
                .collect(),
        };
        assert_eq!(encode(&rebuilt), default);
    }
}

#[test]
fn keyboard_keeps_unkeyed_array_sorted_by_raw_value() {
    let value: serde_json::Value =
        serde_json::from_slice(&encode(&KeyboardShortcutSettings::default())).unwrap();
    let array = value["bindings"].as_array().unwrap();
    assert_eq!(
        array.len(),
        KeyboardShortcutSettings::default_bindings().len() * 2
    );
    let keys: Vec<String> = array
        .iter()
        .step_by(2)
        .map(|k| k.as_str().unwrap().to_string())
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
}

#[test]
fn keyboard_decodes_legacy_arrays_and_round_trips() {
    let hand = br#"{"bindings":["toggleMute",{"key":"m","modifiers":0},"screenshot",{"key":"s","modifiers":1}]}"#;
    let legacy: KeyboardShortcutSettings = serde_json::from_slice(hand).unwrap();
    assert_eq!(
        legacy.bindings,
        HashMap::from([
            (
                ShortcutAction::ToggleMute,
                KeyBinding::new("m", ShortcutModifiers::empty())
            ),
            (
                ShortcutAction::Screenshot,
                KeyBinding::new("s", ShortcutModifiers::COMMAND)
            ),
        ])
    );
    let original = KeyboardShortcutSettings {
        bindings: custom_bindings().into_iter().collect(),
    };
    let decoded: KeyboardShortcutSettings = serde_json::from_slice(&encode(&original)).unwrap();
    assert_eq!(decoded, original);
    let missing: KeyboardShortcutSettings = serde_json::from_slice(b"{}").unwrap();
    assert_eq!(missing, KeyboardShortcutSettings::default());
}

#[test]
fn encode_all_domains_is_deterministic() {
    let domains = cview_persistence::SettingsDomains::default();
    let a = domains.encode_all().unwrap();
    let b = domains.clone().encode_all().unwrap();
    assert_eq!(a, b);
    assert_eq!(
        a.keys().cloned().collect::<HashSet<_>>(),
        persisted_domain_keys()
    );
}
