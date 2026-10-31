//! 네이버 라이브 스트리밍 커넥터 — 치지직의 P2P 전용 1080p 트랙 재생 조건.
//!
//! 치지직 웹 플레이어는 재생 JSON 트랙에 `p2pPath` 가 있으면 그 트랙(대개 1080p)을 커넥터
//! (`NLiveConnectorSetup.exe`)가 설치된 Windows 에서만 고를 수 있게 하고, 설치되지 않았으면 일반 트랙의
//! 최고 화질(대개 720p)까지만 보여 준다. 커넥터는 `naverliveconnector:` URL 스킴과 네이버 동영상 플러그인
//! 확장의 네이티브 메시징 호스트로 실행되므로, 둘 중 하나의 등록이나 설치 항목이 레지스트리에 있으면 설치로
//! 본다. 커넥터는 Windows 전용이라 다른 OS 에서는 제한하지 않는다.

use serde::Serialize;

/// 커넥터 설치 파일 — 치지직 웹 플레이어가 여는 주소와 같다.
pub const SETUP_URL: &str =
    "https://appdown.pstatic.net/naver/nliveconnector/NLiveConnectorSetup.exe";

/// UI 용 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorStatus {
    /// 이 OS 에서 1080p(P2P 트랙)에 커넥터가 필요한가.
    pub required: bool,
    pub installed: bool,
    pub download_url: &'static str,
}

/// 지금 상태 — 레지스트리 읽기 몇 번이라 재생을 시작할 때마다 다시 확인한다(설치 직후 바로 반영).
pub fn status() -> ConnectorStatus {
    let required = cfg!(target_os = "windows");
    ConnectorStatus {
        required,
        installed: required && platform::is_installed(),
        download_url: SETUP_URL,
    }
}

/// 엔진에 걸 정책 높이 상한(0 = 없음). `p2p_general_max_height` 는 재생 계획이 찾은 일반 트랙 최고 높이.
pub fn policy_height(p2p_general_max_height: Option<i64>) -> i64 {
    policy_height_for(status(), p2p_general_max_height)
}

fn policy_height_for(status: ConnectorStatus, p2p_general_max_height: Option<i64>) -> i64 {
    match p2p_general_max_height {
        Some(h) if status.required && !status.installed => h.max(1),
        _ => 0,
    }
}

/// 레지스트리 키·값 이름이 커넥터를 가리키는가.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn names_connector(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("liveconnector") || lower.contains("라이브 스트리밍 커넥터")
}

#[cfg(target_os = "windows")]
mod platform {
    use winreg::enums::{HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;

    use super::names_connector;

    /// 확장이 `svc_ver` 이름으로 찾는 네이티브 메시징 호스트가 등록되는 브라우저들.
    const NATIVE_HOST_ROOTS: [&str; 4] = [
        r"Software\Google\Chrome\NativeMessagingHosts",
        r"Software\Microsoft\Edge\NativeMessagingHosts",
        r"Software\Naver\Naver Whale\NativeMessagingHosts",
        r"Software\Chromium\NativeMessagingHosts",
    ];
    const UNINSTALL_ROOTS: [&str; 2] = [
        r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
        r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    ];

    pub fn is_installed() -> bool {
        url_scheme_registered() || native_host_registered() || uninstall_entry_present()
    }

    fn url_scheme_registered() -> bool {
        RegKey::predef(HKEY_CLASSES_ROOT)
            .open_subkey_with_flags("naverliveconnector", KEY_READ)
            .is_ok_and(|k| k.get_value::<String, _>("URL Protocol").is_ok())
    }

    fn hives() -> [RegKey; 2] {
        [
            RegKey::predef(HKEY_CURRENT_USER),
            RegKey::predef(HKEY_LOCAL_MACHINE),
        ]
    }

    fn native_host_registered() -> bool {
        hives().iter().any(|hive| {
            NATIVE_HOST_ROOTS.iter().any(|root| {
                hive.open_subkey_with_flags(root, KEY_READ)
                    .is_ok_and(|k| k.enum_keys().flatten().any(|name| names_connector(&name)))
            })
        })
    }

    fn uninstall_entry_present() -> bool {
        hives().iter().any(|hive| {
            UNINSTALL_ROOTS.iter().any(|root| {
                let Ok(list) = hive.open_subkey_with_flags(root, KEY_READ) else {
                    return false;
                };
                list.enum_keys().flatten().any(|name| {
                    names_connector(&name)
                        || list
                            .open_subkey_with_flags(&name, KEY_READ)
                            .and_then(|k| k.get_value::<String, _>("DisplayName"))
                            .is_ok_and(|display| names_connector(&display))
                })
            })
        })
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    pub fn is_installed() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(required: bool, installed: bool) -> ConnectorStatus {
        ConnectorStatus {
            required,
            installed,
            download_url: SETUP_URL,
        }
    }

    #[test]
    fn caps_only_when_required_and_missing() {
        assert_eq!(policy_height_for(st(true, false), Some(720)), 720);
        assert_eq!(policy_height_for(st(true, true), Some(720)), 0);
        assert_eq!(policy_height_for(st(false, false), Some(720)), 0);
        assert_eq!(policy_height_for(st(true, false), None), 0);
    }

    #[test]
    fn connector_names() {
        assert!(names_connector("NLiveConnector"));
        assert!(names_connector("naverliveconnector_1"));
        assert!(names_connector("네이버 라이브 스트리밍 커넥터"));
        assert!(!names_connector("Naver Whale"));
    }

    #[test]
    fn non_windows_never_requires() {
        if !cfg!(target_os = "windows") {
            assert_eq!(status(), st(false, false));
        }
    }
}
