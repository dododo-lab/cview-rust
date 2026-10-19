//! 보안 저장소 테스트.
//!
//! 원본: `Tests/CViewAuthTests/KeychainServiceTests.swift` (실제 저장소 대신 임시 디렉터리 파일 backend),
//! `KeychainServiceMigrationTests.swift`, `Audit0919authTokenRefreshBackoffTests.swift`(file mode 삭제 스위트),
//! `AuthSessionRaceTests.persistenceOwnership`.

mod common;

use std::sync::Arc;

use common::{keychain_with, RecordingStore};
use cview_auth::{
    AuthSessionGeneration, FileSecretStore, KeychainService, MemorySecretStore, OAuthTokens,
    SecretStore, StoreError,
};
use serde::{Deserialize, Serialize};

fn file_service(dir: &std::path::Path) -> KeychainService {
    KeychainService::with_backends(
        Arc::new(FileSecretStore::with_directory(dir)),
        Arc::new(MemorySecretStore::new()),
        false,
    )
}

// MARK: - KeychainServiceTests (CRUD)

#[test]
fn crud_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let svc = file_service(dir.path());

    svc.save("save-load", b"hello-keychain").unwrap();
    assert_eq!(
        svc.load("save-load").unwrap().as_deref(),
        Some(&b"hello-keychain"[..])
    );
    svc.delete("save-load").unwrap();
    assert_eq!(svc.load("save-load").unwrap(), None);

    assert_eq!(svc.load("nonexistent").unwrap(), None);
    svc.delete("never-saved").unwrap();

    svc.save("ow", b"first").unwrap();
    svc.save("ow", b"second").unwrap();
    assert_eq!(svc.load("ow").unwrap().as_deref(), Some(&b"second"[..]));

    svc.save("empty", b"").unwrap();
    assert_eq!(svc.load("empty").unwrap(), Some(vec![]));

    let large = vec![0xABu8; 1_000_000];
    svc.save("big", &large).unwrap();
    assert_eq!(svc.load("big").unwrap(), Some(large));
}

#[test]
fn string_and_codable() {
    let dir = tempfile::tempdir().unwrap();
    let svc = file_service(dir.path());
    svc.save_string("str", "안녕하세요 🌍").unwrap();
    assert_eq!(
        svc.load_string("str").unwrap().as_deref(),
        Some("안녕하세요 🌍")
    );
    svc.save_string("es", "").unwrap();
    assert_eq!(svc.load_string("es").unwrap().as_deref(), Some(""));
    assert_eq!(svc.load_string("missing").unwrap(), None);

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Token {
        access: String,
        expires_in: i64,
    }
    let token = Token {
        access: "abc123".into(),
        expires_in: 3600,
    };
    svc.save_codable("token", &token).unwrap();
    assert_eq!(svc.load_codable::<Token>("token").unwrap(), Some(token));
    assert_eq!(svc.load_codable::<Token>("nope").unwrap(), None);
    svc.save_string("bad", "not json").unwrap();
    assert!(matches!(
        svc.load_codable::<Token>("bad"),
        Err(StoreError::Decoding(_))
    ));
}

#[test]
fn special_character_keys_and_multiple_keys() {
    let dir = tempfile::tempdir().unwrap();
    let svc = file_service(dir.path());
    let k = "com.cview/auth:token@2024";
    svc.save_string(k, "secret").unwrap();
    assert_eq!(svc.load_string(k).unwrap().as_deref(), Some("secret"));
    assert!(dir
        .path()
        .join("com%2Ecview%2Fauth%3Atoken%402024")
        .exists());

    for (k, v) in [("mk1", "v1"), ("mk2", "v2"), ("mk3", "v3")] {
        svc.save_string(k, v).unwrap();
    }
    for (k, v) in [("mk1", "v1"), ("mk2", "v2"), ("mk3", "v3")] {
        assert_eq!(svc.load_string(k).unwrap().as_deref(), Some(v));
    }
}

#[test]
fn delete_all_clears_everything() {
    let dir = tempfile::tempdir().unwrap();
    let svc = file_service(dir.path());
    for k in ["da1", "da2", "da3"] {
        svc.save(k, k.as_bytes()).unwrap();
    }
    svc.delete_all().unwrap();
    for k in ["da1", "da2", "da3"] {
        assert_eq!(svc.load(k).unwrap(), None);
    }
}

// MARK: - KeychainServiceMigrationTests

const PAYLOAD: &[u8] = b"test-token";

#[test]
fn failed_save_never_writes_plaintext() {
    let file = RecordingStore::new();
    let keychain = RecordingStore::new();
    *keychain.deny_save.lock().unwrap() = true;
    let service = KeychainService::with_backends(file.clone(), keychain, true);
    assert!(service.save("token", PAYLOAD).is_err());
    assert_eq!(file.raw("token"), None);
}

#[test]
fn failed_migration_preserves_legacy_and_can_retry() {
    let file = RecordingStore::new();
    let keychain = RecordingStore::new();
    file.save("token", PAYLOAD).unwrap();
    *keychain.deny_save.lock().unwrap() = true;
    let service = KeychainService::with_backends(file.clone(), keychain.clone(), true);
    assert!(service.load("token").is_err());
    assert_eq!(file.raw("token").as_deref(), Some(PAYLOAD));
    *keychain.deny_save.lock().unwrap() = false;
    assert_eq!(service.load("token").unwrap().as_deref(), Some(PAYLOAD));
    assert_eq!(file.raw("token"), None);
    assert_eq!(service.load("token").unwrap().as_deref(), Some(PAYLOAD));
    assert_eq!(*keychain.writes.lock().unwrap(), 1);
}

#[test]
fn locked_keychain_does_not_read_or_promote_legacy() {
    let file = RecordingStore::new();
    let keychain = RecordingStore::new();
    file.save("token", PAYLOAD).unwrap();
    *keychain.deny_load.lock().unwrap() = true;
    let service = KeychainService::with_backends(file.clone(), keychain.clone(), true);
    assert!(service.load("token").is_err());
    assert_eq!(file.raw("token").as_deref(), Some(PAYLOAD));
    assert_eq!(*keychain.writes.lock().unwrap(), 0);
}

#[test]
fn existing_keychain_wins_and_missing_returns_none() {
    let file = RecordingStore::new();
    let keychain = RecordingStore::new();
    keychain.save("token", PAYLOAD).unwrap();
    file.save("token", b"stale").unwrap();
    let service = KeychainService::with_backends(file, keychain, true);
    assert_eq!(service.load("token").unwrap().as_deref(), Some(PAYLOAD));
    assert_eq!(service.load("missing").unwrap(), None);
}

#[test]
fn explicit_file_mode_does_not_touch_keychain() {
    let file = RecordingStore::new();
    let keychain = RecordingStore::new();
    *keychain.deny_save.lock().unwrap() = true;
    *keychain.deny_load.lock().unwrap() = true;
    let service = KeychainService::with_backends(file.clone(), keychain, false);
    service.save("token", PAYLOAD).unwrap();
    assert_eq!(service.load("token").unwrap().as_deref(), Some(PAYLOAD));
    service.delete("token").unwrap();
    assert_eq!(file.raw("token"), None);
}

#[cfg(unix)]
#[test]
fn file_store_restricts_permissions_and_preserves_other_services() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let other = dir.path().join("other-service");
    std::fs::create_dir_all(&other).unwrap();
    let other_token = other.join("token");
    std::fs::write(&other_token, PAYLOAD).unwrap();
    let file = FileSecretStore::with_directory(dir.path());
    file.save("token", PAYLOAD).unwrap();
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(dir.path()), 0o700);
    assert_eq!(mode(&dir.path().join("token")), 0o600);
    file.delete_all().unwrap();
    assert_eq!(file.load("token").unwrap(), None);
    assert_eq!(std::fs::read(&other_token).unwrap(), PAYLOAD);
}

// MARK: - Audit 0919: file mode 로그아웃이 시스템 저장소 잔여물까지 지운다

#[test]
fn file_mode_delete_also_clears_keychain() {
    let file = RecordingStore::new();
    let keychain = RecordingStore::new();
    keychain.save("oauth_tokens", b"stale-credential").unwrap();
    keychain
        .save("naver_auth_cookies", b"stale-credential")
        .unwrap();
    let service = KeychainService::with_backends(file.clone(), keychain.clone(), false);

    // file mode 의 load 는 시스템 저장소를 보지 않는다 — 앱에는 로그아웃 상태로 보인다.
    assert_eq!(service.load("oauth_tokens").unwrap(), None);
    service.delete("oauth_tokens").unwrap();
    service.delete("naver_auth_cookies").unwrap();

    assert_eq!(keychain.raw("oauth_tokens"), None);
    assert_eq!(keychain.raw("naver_auth_cookies"), None);
    assert_eq!(
        *keychain.deleted_keys.lock().unwrap(),
        ["oauth_tokens", "naver_auth_cookies"]
    );
    assert_eq!(
        *file.deleted_keys.lock().unwrap(),
        ["oauth_tokens", "naver_auth_cookies"]
    );
}

#[test]
fn file_mode_delete_all_also_clears_keychain() {
    let file = RecordingStore::new();
    let keychain = RecordingStore::new();
    keychain.save("oauth_tokens", b"stale").unwrap();
    let service = KeychainService::with_backends(file.clone(), keychain.clone(), false);
    service.delete_all().unwrap();
    assert_eq!(*keychain.delete_all_calls.lock().unwrap(), 1);
    assert_eq!(*file.delete_all_calls.lock().unwrap(), 1);
    assert_eq!(keychain.raw("oauth_tokens"), None);
}

// MARK: - AuthSessionRaceTests.persistenceOwnership

#[test]
fn persistence_checks_generation_inside_write_and_delete() {
    let keychain = keychain_with(RecordingStore::new());
    let session = AuthSessionGeneration::new();
    let old = session.advance();
    let current = session.advance();
    let tokens = OAuthTokens::bearer("new", None, 3600);
    assert_eq!(
        keychain.save_codable_in_session(&tokens, "token", &session, old),
        Err(StoreError::Cancelled)
    );
    keychain
        .save_codable_in_session(&tokens, "token", &session, current)
        .unwrap();
    assert_eq!(
        keychain.delete_in_session("token", &session, old),
        Err(StoreError::Cancelled)
    );
    let loaded: OAuthTokens = keychain.load_codable("token").unwrap().unwrap();
    assert_eq!(loaded.access_token, "new");
}
