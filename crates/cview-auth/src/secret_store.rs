//! `Sources/CViewAuth/KeychainService.swift` 포팅 — 보안 토큰 저장소.
//!
//! - Swift `AuthStoreBackend` 프로토콜 → [`SecretStore`] trait.
//! - `SystemKeychainBackend`(Security.framework) → [`KeyringSecretStore`] (`keyring` 크레이트 — macOS
//!   Keychain / Windows Credential Manager / Linux keyutils).
//! - `FileAuthStoreBackend` → [`FileSecretStore`] (디렉터리 0700 · 파일 0600, 원자적 쓰기).
//! - 테스트용 [`MemorySecretStore`].
//! - 파사드 `KeychainService` 는 이름을 그대로 두었다. 마이그레이션·fail-closed 규칙도 같다:
//!   - 시스템 저장소 모드의 저장 실패는 **평문 파일로 폴백하지 않는다**.
//!   - 시스템 저장소에 항목이 **없을 때만** 파일 사본을 승격한다(잠김·거부는 오류).
//!   - 삭제는 모드와 무관하게 **두 backend 모두** 지운다(로그아웃이 다른 모드의 잔여물을 남기지 않게).
//!
//! 코드서명 자동감지(`isDeveloperIDSigned`)는 macOS 전용이라 옮기지 않았다. 기본값은
//! env `CVIEW_USE_SYSTEM_KEYCHAIN` → 플랫폼 기본(macOS·Windows 는 시스템 저장소, 그 밖은 파일)이다.
//! Linux keyutils 는 세션 수명이라 재부팅 뒤 남지 않으므로 파일 backend 를 기본으로 둔다.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::generation::{AuthSessionGeneration, Cancelled};

/// 저장소 오류.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    /// 시스템 저장소 오류(잠김·거부·플랫폼 오류).
    #[error("보안 저장소 오류: {0}")]
    Backend(String),
    /// 파일 입출력 오류.
    #[error("파일 저장소 오류: {0}")]
    Io(String),
    /// 저장된 값을 해석하지 못했다(Swift `DecodingError`).
    #[error("저장된 값을 해석하지 못했습니다: {0}")]
    Decoding(String),
    /// 값을 직렬화하지 못했다.
    #[error("값을 저장 형식으로 바꾸지 못했습니다: {0}")]
    Encoding(String),
    /// 세션 세대가 바뀌었다.
    #[error("작업이 취소되었습니다")]
    Cancelled,
}

impl From<Cancelled> for StoreError {
    fn from(_: Cancelled) -> Self {
        StoreError::Cancelled
    }
}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e.to_string())
    }
}

/// 키-값 비밀 저장 backend (Swift `AuthStoreBackend`).
pub trait SecretStore: Send + Sync {
    fn save(&self, key: &str, data: &[u8]) -> Result<(), StoreError>;
    fn load(&self, key: &str) -> Result<Option<Vec<u8>>, StoreError>;
    /// 없는 키 삭제는 성공이다.
    fn delete(&self, key: &str) -> Result<(), StoreError>;
    fn delete_all(&self) -> Result<(), StoreError>;
}

// MARK: - Memory

/// 테스트·임시 세션용 인메모리 저장소.
#[derive(Debug, Default)]
pub struct MemorySecretStore {
    values: Mutex<HashMap<String, Vec<u8>>>,
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Vec<u8>>> {
        self.values.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 저장된 키 개수 (진단·테스트용).
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }
}

impl SecretStore for MemorySecretStore {
    fn save(&self, key: &str, data: &[u8]) -> Result<(), StoreError> {
        self.lock().insert(key.to_string(), data.to_vec());
        Ok(())
    }
    fn load(&self, key: &str) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self.lock().get(key).cloned())
    }
    fn delete(&self, key: &str) -> Result<(), StoreError> {
        self.lock().remove(key);
        Ok(())
    }
    fn delete_all(&self) -> Result<(), StoreError> {
        self.lock().clear();
        Ok(())
    }
}

// MARK: - Keyring

/// OS 자격 증명 저장소(`keyring`) backend.
///
/// `keyring` 은 항목 열거를 지원하지 않으므로, `delete_all` 을 위해 저장한 키 목록을 같은 서비스의
/// 색인 항목([`KeyringSecretStore::INDEX_ACCOUNT`])에 JSON 으로 유지한다.
#[derive(Debug)]
pub struct KeyringSecretStore {
    service_name: String,
    index_lock: Mutex<()>,
}

impl KeyringSecretStore {
    /// 키 목록 색인 항목의 account 이름.
    pub const INDEX_ACCOUNT: &'static str = "__cview_key_index__";

    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            index_lock: Mutex::new(()),
        }
    }

    fn entry(&self, key: &str) -> Result<keyring::Entry, StoreError> {
        keyring::Entry::new(&self.service_name, key).map_err(map_keyring_error)
    }

    fn read_index(&self) -> Result<BTreeSet<String>, StoreError> {
        match self.entry(Self::INDEX_ACCOUNT)?.get_secret() {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes).unwrap_or_default()),
            Err(keyring::Error::NoEntry) => Ok(BTreeSet::new()),
            Err(e) => Err(map_keyring_error(e)),
        }
    }

    fn write_index(&self, index: &BTreeSet<String>) -> Result<(), StoreError> {
        let entry = self.entry(Self::INDEX_ACCOUNT)?;
        if index.is_empty() {
            return match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(map_keyring_error(e)),
            };
        }
        let bytes = serde_json::to_vec(index).map_err(|e| StoreError::Encoding(e.to_string()))?;
        entry.set_secret(&bytes).map_err(map_keyring_error)
    }

    fn update_index(&self, f: impl FnOnce(&mut BTreeSet<String>)) -> Result<(), StoreError> {
        let _guard = self.index_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut index = self.read_index()?;
        f(&mut index);
        self.write_index(&index)
    }
}

fn map_keyring_error(e: keyring::Error) -> StoreError {
    StoreError::Backend(e.to_string())
}

impl SecretStore for KeyringSecretStore {
    fn save(&self, key: &str, data: &[u8]) -> Result<(), StoreError> {
        self.entry(key)?
            .set_secret(data)
            .map_err(map_keyring_error)?;
        self.update_index(|i| {
            i.insert(key.to_string());
        })
    }

    fn load(&self, key: &str) -> Result<Option<Vec<u8>>, StoreError> {
        match self.entry(key)?.get_secret() {
            Ok(bytes) => Ok(Some(bytes)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_keyring_error(e)),
        }
    }

    fn delete(&self, key: &str) -> Result<(), StoreError> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => return Err(map_keyring_error(e)),
        }
        self.update_index(|i| {
            i.remove(key);
        })
    }

    fn delete_all(&self) -> Result<(), StoreError> {
        let _guard = self.index_lock.lock().unwrap_or_else(|e| e.into_inner());
        let index = self.read_index()?;
        for key in &index {
            match self.entry(key)?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => {}
                Err(e) => return Err(map_keyring_error(e)),
            }
        }
        self.write_index(&BTreeSet::new())
    }
}

// MARK: - File

/// 파일 backend — 명시적 파일 모드와 기존 데이터 이관에만 쓴다. 단독으로 쓰지 말 것(항상 파사드 경유).
///
/// `<앱 데이터>/CView/.auth-store/<serviceName>/<key>` 에 저장한다. 기본 서비스명은 legacy 경로
/// (`.auth-store` 바로 아래)를 쓴다.
#[derive(Debug, Clone)]
pub struct FileSecretStore {
    storage_dir: PathBuf,
}

impl FileSecretStore {
    /// 지정 디렉터리를 쓰는 저장소 (테스트용 `init(storageDirectory:)`).
    pub fn with_directory(dir: impl Into<PathBuf>) -> Self {
        Self {
            storage_dir: dir.into(),
        }
    }

    /// 서비스별 기본 위치.
    pub fn for_service(service_name: &str) -> Self {
        let base = app_data_dir()
            .unwrap_or_else(|| std::env::temp_dir().join(".cview-auth"))
            .join("CView")
            .join(".auth-store");
        let dir = if service_name == DEFAULT_SERVICE_NAME {
            base
        } else {
            base.join(service_name)
        };
        Self { storage_dir: dir }
    }

    pub fn storage_dir(&self) -> &Path {
        &self.storage_dir
    }

    fn file_path(&self, key: &str) -> PathBuf {
        self.storage_dir.join(percent_encode_alphanumerics(key))
    }

    fn ensure_dir(&self) -> Result<(), StoreError> {
        std::fs::create_dir_all(&self.storage_dir)?;
        set_permissions(&self.storage_dir, 0o700)?;
        Ok(())
    }
}

/// 영숫자 외 모든 바이트를 `%XX` 로 바꾼다 (Swift `addingPercentEncoding(withAllowedCharacters: .alphanumerics)`).
fn percent_encode_alphanumerics(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    for b in key.bytes() {
        if b.is_ascii_alphanumeric() {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(unix)]
fn set_permissions(path: &Path, mode: u32) -> Result<(), StoreError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_permissions(_path: &Path, _mode: u32) -> Result<(), StoreError> {
    Ok(())
}

/// 플랫폼별 사용자 앱 데이터 디렉터리 (macOS Application Support 대응).
fn app_data_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join("Library").join("Application Support"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    }
}

impl SecretStore for FileSecretStore {
    fn save(&self, key: &str, data: &[u8]) -> Result<(), StoreError> {
        // 임시 파일이 자격 증명을 담기 전에 디렉터리 권한부터 좁힌다.
        self.ensure_dir()?;
        let path = self.file_path(key);
        let tmp = self
            .storage_dir
            .join(format!(".tmp-{}", uuid::Uuid::new_v4().simple()));
        {
            use std::io::Write;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&tmp)?;
            file.write_all(data)?;
            file.sync_all()?;
        }
        if let Err(e) = std::fs::rename(&tmp, &path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
        set_permissions(&path, 0o600)
    }

    fn load(&self, key: &str) -> Result<Option<Vec<u8>>, StoreError> {
        match std::fs::read(self.file_path(key)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn delete(&self, key: &str) -> Result<(), StoreError> {
        match std::fs::remove_file(self.file_path(key)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    fn delete_all(&self) -> Result<(), StoreError> {
        let entries = match std::fs::read_dir(&self.storage_dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        for entry in entries {
            let entry = entry?;
            // 기본 디렉터리에는 다른 서비스의 하위 디렉터리도 있다 — 일반 파일만 지운다.
            if entry.file_type()?.is_file() {
                std::fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    }
}

// MARK: - Facade

/// 기본 서비스명.
pub const DEFAULT_SERVICE_NAME: &str = "com.cview.auth";

/// 보안 저장소 파사드 (Swift `KeychainService` actor).
pub struct KeychainService {
    file_backend: Arc<dyn SecretStore>,
    keychain_backend: Arc<dyn SecretStore>,
    use_system_keychain: bool,
}

impl std::fmt::Debug for KeychainService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeychainService")
            .field("use_system_keychain", &self.use_system_keychain)
            .finish_non_exhaustive()
    }
}

impl Default for KeychainService {
    fn default() -> Self {
        Self::new(DEFAULT_SERVICE_NAME)
    }
}

impl KeychainService {
    /// 서비스명별 실제 저장소 (파일 + OS keyring).
    pub fn new(service_name: &str) -> Self {
        Self::with_mode(service_name, Self::default_use_system_keychain())
    }

    pub fn with_mode(service_name: &str, use_system_keychain: bool) -> Self {
        Self {
            file_backend: Arc::new(FileSecretStore::for_service(service_name)),
            keychain_backend: Arc::new(KeyringSecretStore::new(service_name)),
            use_system_keychain,
        }
    }

    /// backend 를 주입한다 (테스트·대체 저장소).
    pub fn with_backends(
        file_backend: Arc<dyn SecretStore>,
        keychain_backend: Arc<dyn SecretStore>,
        use_system_keychain: bool,
    ) -> Self {
        Self {
            file_backend,
            keychain_backend,
            use_system_keychain,
        }
    }

    /// 파일·시스템 양쪽 모두 메모리인 저장소 (파일 모드). 테스트용.
    pub fn in_memory() -> Self {
        Self::with_backends(
            Arc::new(MemorySecretStore::new()),
            Arc::new(MemorySecretStore::new()),
            false,
        )
    }

    /// 시스템 저장소 사용 여부 기본값. env `CVIEW_USE_SYSTEM_KEYCHAIN` 이 우선한다.
    pub fn default_use_system_keychain() -> bool {
        Self::use_system_keychain_from(std::env::var("CVIEW_USE_SYSTEM_KEYCHAIN").ok().as_deref())
    }

    fn use_system_keychain_from(env: Option<&str>) -> bool {
        if let Some(v) = env.map(str::to_lowercase) {
            if ["1", "true", "yes"].contains(&v.as_str()) {
                return true;
            }
            if ["0", "false", "no"].contains(&v.as_str()) {
                return false;
            }
        }
        cfg!(any(target_os = "macos", windows))
    }

    pub fn uses_system_keychain(&self) -> bool {
        self.use_system_keychain
    }

    // MARK: CRUD

    /// 시스템 저장소 모드는 실패해도 평문 파일로 폴백하지 않는다.
    pub fn save(&self, key: &str, data: &[u8]) -> Result<(), StoreError> {
        if !self.use_system_keychain {
            return self.file_backend.save(key, data);
        }
        self.keychain_backend.save(key, data)?;
        self.file_backend.delete(key)
    }

    /// 시스템 저장소에 항목이 **없을 때만** 파일 사본을 승격한다. 잠김·거부는 오류다.
    pub fn load(&self, key: &str) -> Result<Option<Vec<u8>>, StoreError> {
        if !self.use_system_keychain {
            return self.file_backend.load(key);
        }
        if let Some(data) = self.keychain_backend.load(key)? {
            return Ok(Some(data));
        }
        let Some(data) = self.file_backend.load(key)? else {
            return Ok(None);
        };
        self.keychain_backend.save(key, &data)?;
        self.file_backend.delete(key)?;
        Ok(Some(data))
    }

    /// 두 backend 를 모두 지우고, 실패는 숨기지 않는다.
    ///
    /// ⚠ `use_system_keychain` 조건으로 감싸면 로그아웃이 다른 모드의 자격 증명을 남기는 회귀가 돌아온다.
    pub fn delete(&self, key: &str) -> Result<(), StoreError> {
        let keychain_result = self.keychain_backend.delete(key);
        self.file_backend.delete(key)?;
        keychain_result
    }

    /// 전체 삭제 — 두 backend 모두.
    pub fn delete_all(&self) -> Result<(), StoreError> {
        let keychain_result = self.keychain_backend.delete_all();
        self.file_backend.delete_all()?;
        keychain_result?;
        tracing::info!("Auth store cleared");
        Ok(())
    }

    // MARK: Convenience

    pub fn save_string(&self, key: &str, value: &str) -> Result<(), StoreError> {
        self.save(key, value.as_bytes())
    }

    pub fn load_string(&self, key: &str) -> Result<Option<String>, StoreError> {
        Ok(self.load(key)?.and_then(|d| String::from_utf8(d).ok()))
    }

    pub fn save_codable<T: Serialize>(&self, key: &str, value: &T) -> Result<(), StoreError> {
        let data = serde_json::to_vec(value).map_err(|e| StoreError::Encoding(e.to_string()))?;
        self.save(key, &data)
    }

    pub fn load_codable<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, StoreError> {
        let Some(data) = self.load(key)? else {
            return Ok(None);
        };
        serde_json::from_slice(&data)
            .map(Some)
            .map_err(|e| StoreError::Decoding(e.to_string()))
    }

    /// 세대가 `generation` 일 때만 저장한다(확인과 저장이 원자적).
    pub fn save_session_data(
        &self,
        data: &[u8],
        key: &str,
        session: &AuthSessionGeneration,
        generation: u64,
    ) -> Result<(), StoreError> {
        session.with_current(generation, || self.save(key, data))
    }

    pub fn save_codable_in_session<T: Serialize>(
        &self,
        value: &T,
        key: &str,
        session: &AuthSessionGeneration,
        generation: u64,
    ) -> Result<(), StoreError> {
        session.with_current(generation, || self.save_codable(key, value))
    }

    pub fn delete_in_session(
        &self,
        key: &str,
        session: &AuthSessionGeneration,
        generation: u64,
    ) -> Result<(), StoreError> {
        session.with_current(generation, || self.delete(key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encoding_matches_swift_alphanumerics() {
        assert_eq!(percent_encode_alphanumerics("abc123"), "abc123");
        assert_eq!(
            percent_encode_alphanumerics("com.cview/auth:token@2024"),
            "com%2Ecview%2Fauth%3Atoken%402024"
        );
        assert_eq!(percent_encode_alphanumerics("한"), "%ED%95%9C");
    }

    #[test]
    fn env_override_for_system_keychain() {
        assert!(KeychainService::use_system_keychain_from(Some("1")));
        assert!(KeychainService::use_system_keychain_from(Some("YES")));
        assert!(!KeychainService::use_system_keychain_from(Some("0")));
        assert!(!KeychainService::use_system_keychain_from(Some("false")));
        assert_eq!(
            KeychainService::use_system_keychain_from(Some("maybe")),
            cfg!(any(target_os = "macos", windows))
        );
    }
}
