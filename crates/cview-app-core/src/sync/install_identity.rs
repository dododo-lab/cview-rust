//! `Sources/CViewApp/Sync/InstallIdentity.swift` 포팅.
//!
//! 한 설치당 고유한 익명 UUID. `KeychainService`(파일 기반 기본)에 보관한다. 재설치하면 새 ID 가
//! 발급되어 이전 데이터와 단절된다(예상된 동작).
//!
//! 원본은 키체인 await 가 actor 재진입 지점이라 in-flight Task memo 로 중복 발급을 막았다. 여기서는
//! 비동기 뮤텍스가 조회·발급 전체를 직렬화한다 — 두 호출자가 서로 다른 UUID 를 저장하는 경합이 없다.

use std::sync::{Arc, OnceLock};

use cview_auth::secret_store::KeychainService;
use tokio::sync::Mutex;
use uuid::Uuid;

pub struct InstallIdentity {
    keychain: Arc<KeychainService>,
    cached: Mutex<Option<String>>,
}

impl InstallIdentity {
    pub const KEYCHAIN_KEY: &'static str = "com.cview.sync.installID";
    pub const SERVICE_NAME: &'static str = "com.cview.sync";

    pub fn new(keychain: Arc<KeychainService>) -> Self {
        Self {
            keychain,
            cached: Mutex::new(None),
        }
    }

    /// 앱 전역 인스턴스.
    pub fn shared() -> Arc<InstallIdentity> {
        static SHARED: OnceLock<Arc<InstallIdentity>> = OnceLock::new();
        SHARED
            .get_or_init(|| {
                Arc::new(InstallIdentity::new(Arc::new(KeychainService::new(
                    Self::SERVICE_NAME,
                ))))
            })
            .clone()
    }

    /// 설치 ID — 없으면 새로 발급해 키체인에 저장한다. 대문자 UUID 문자열(Swift `uuidString`).
    pub async fn install_id(&self) -> String {
        let mut cached = self.cached.lock().await;
        if let Some(id) = cached.as_ref() {
            return id.clone();
        }
        let loaded = self
            .keychain
            .load(Self::KEYCHAIN_KEY)
            .ok()
            .flatten()
            .and_then(|data| String::from_utf8(data).ok())
            .filter(|id| !id.is_empty());
        let id = match loaded {
            Some(id) => id,
            None => {
                let new_id = Uuid::new_v4().to_string().to_uppercase();
                let _ = self.keychain.save(Self::KEYCHAIN_KEY, new_id.as_bytes());
                new_id
            }
        };
        *cached = Some(id.clone());
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn concurrent_callers_share_one_id_and_reuse_keychain() {
        let keychain = Arc::new(KeychainService::in_memory());
        let identity = Arc::new(InstallIdentity::new(keychain.clone()));
        let (a, b) = tokio::join!(identity.install_id(), identity.install_id());
        assert_eq!(a, b);
        assert_eq!(a.len(), 36);
        assert_eq!(a, a.to_uppercase());
        // 새 프로세스(새 인스턴스)도 키체인 값을 읽는다.
        let again = InstallIdentity::new(keychain);
        assert_eq!(again.install_id().await, a);
    }
}
