//! `Sources/CViewApp/Services/UpdateService.swift` 포팅 — 릴리스 확인·버전 비교·번들 검증 규칙.
//!
//! 옮긴 것: GitHub Releases 응답 모델, semver(+빌드 번호) 비교, `check_for_updates`(전송 주입),
//! 다운로드한 번들의 식별자·버전 검증 규칙(Info.plist 값을 [`BundleInfo`] 로 받는다), 오류 문구.
//!
//! 건너뛴 것(macOS 전용): asset 다운로드·zip/dmg 풀기·`codesign`/`spctl` 서명·공증 검증·셸 스크립트 교체·
//! 재실행. 자동 설치는 원본에서도 꺼져 있고(`isAutoInstallEnabled = false`), 업데이트 버튼은 릴리스
//! 페이지를 연다 — 셸이 [`UpdateStatus::OpenedReleasePage`] 로 같은 일을 한다.

use std::cmp::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use cview_core::protocols::HttpMethod;
use cview_core::utils::time::Date;
use cview_networking::{HttpRequest, HttpTransport};
use serde::{Deserialize, Serialize};
use url::Url;

use super::support::{lock, system_clock, Clock};

/// 업데이트 상태.
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateStatus {
    Idle,
    Checking,
    UpToDate,
    UpdateAvailable(GitHubRelease),
    Downloading {
        progress: f64,
    },
    /// 로컬 .app 경로(교체 대상).
    ReadyToInstall(String),
    Installing,
    Error(String),
    /// 브라우저에서 릴리스 페이지를 정상적으로 열었다(오류가 아니다).
    OpenedReleasePage {
        version: String,
    },
}

impl UpdateStatus {
    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::Checking | Self::Downloading { .. } | Self::Installing
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitHubAsset {
    pub name: String,
    pub size: i64,
    pub browser_download_url: Url,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitHubRelease {
    pub tag_name: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub html_url: Option<Url>,
    #[serde(default)]
    pub published_at: Option<DateTime<Utc>>,
    pub assets: Vec<GitHubAsset>,
}

impl GitHubRelease {
    /// `v2.0.1` → `"2.0.1"`.
    pub fn version_string(&self) -> &str {
        self.tag_name
            .strip_prefix('v')
            .or_else(|| self.tag_name.strip_prefix('V'))
            .unwrap_or(&self.tag_name)
    }

    /// .zip asset 우선, 없으면 .dmg.
    pub fn preferred_asset(&self) -> Option<&GitHubAsset> {
        self.assets
            .iter()
            .find(|a| a.name.to_lowercase().ends_with(".zip"))
            .or_else(|| {
                self.assets
                    .iter()
                    .find(|a| a.name.to_lowercase().ends_with(".dmg"))
            })
    }

    /// asset 이름(`CView-2.0.0-65.zip`)에서 빌드 번호를 추출.
    pub fn build_number(&self) -> Option<i64> {
        let asset = self.preferred_asset()?;
        let stem = match asset.name.rfind('.') {
            Some(i) if i > 0 => &asset.name[..i],
            _ => asset.name.as_str(),
        };
        let dash = stem.rfind('-')?;
        stem[dash + 1..].parse().ok()
    }

    /// 버전(semver) + 빌드 번호를 함께 비교한다. 버전이 같고 asset 에 번호가 없으면 새롭지 않다.
    pub fn is_newer_than(&self, current_version: &str, current_build: i64) -> bool {
        match compare_versions(self.version_string(), current_version) {
            Ordering::Greater => true,
            Ordering::Less => false,
            Ordering::Equal => self.build_number().is_some_and(|b| b > current_build),
        }
    }
}

/// 점으로 나눈 정수 비교("2.0.1" vs "2.0.0"). 정수가 아닌 조각은 버리고 빠진 자리는 0 으로 본다.
pub fn compare_versions(lhs: &str, rhs: &str) -> Ordering {
    let a: Vec<i64> = lhs.split('.').filter_map(|s| s.parse().ok()).collect();
    let b: Vec<i64> = rhs.split('.').filter_map(|s| s.parse().ok()).collect();
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        match x.cmp(&y) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    Ordering::Equal
}

// MARK: - 오류

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum UpdateError {
    #[error("서버 응답이 올바르지 않습니다.")]
    InvalidResponse,
    #[error("HTTP {0} 오류")]
    HttpStatus(u16),
    #[error("지원하지 않는 파일 형식: {0}")]
    UnsupportedArchive(String),
    #[error("다운로드한 파일에서 .app 번들을 찾을 수 없습니다.")]
    AppNotFoundInArchive,
    #[error("아카이브에 .app 번들이 {0}개 있습니다. 정확히 1개만 허용됩니다.")]
    MultipleAppBundles(usize),
    #[error("다운로드 크기 불일치 (예상 {expected}B, 실제 {actual}B)")]
    DownloadSizeMismatch { expected: i64, actual: i64 },
    #[error("예상하지 않은 앱 이름: {0} (기대: {app})", app = UpdateBundleValidator::EXPECTED_APP_FILE_NAME)]
    UnexpectedAppName(String),
    #[error("Info.plist가 없습니다.")]
    MissingInfoPlist,
    #[error("Info.plist를 읽을 수 없습니다.")]
    InvalidInfoPlist,
    #[error("번들 ID 불일치: {found} (기대: {expected})")]
    InvalidBundleIdentifier { found: String, expected: String },
    #[error("실행 파일명 불일치: {found} (기대: {expected})")]
    InvalidExecutable { found: String, expected: String },
    #[error("실행 파일이 없거나 실행 불가: {name}")]
    MissingExecutableBinary { name: String },
    #[error("버전 불일치: {found} (릴리스: {expected})")]
    VersionMismatch { found: String, expected: String },
    #[error("빌드 번호 불일치: {found} (릴리스: {expected})")]
    BuildMismatch { found: i64, expected: i64 },
    #[error("다운로드한 번들이 현재 버전보다 새롭지 않습니다: {found}")]
    BundleNotNewer { found: String },
    #[error("다운로드한 업데이트의 코드 서명이 유효하지 않습니다 (변조 가능성).")]
    SignatureInvalid,
    #[error("신뢰할 수 없는 서명자: {0} (기대: Developer ID 팀 {team})", team = UpdateBundleValidator::EXPECTED_TEAM_ID)]
    UntrustedSigner(String),
    #[error("다운로드한 업데이트가 Apple 공증되지 않았습니다.")]
    NotNotarized,
    #[error("{0}")]
    Transport(String),
}

// MARK: - 번들 검증

/// 다운로드한 번들의 Info.plist 값(셸이 읽어 넘긴다).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BundleInfo {
    pub app_file_name: String,
    pub bundle_identifier: String,
    pub executable: String,
    /// `Contents/MacOS/<executable>` 이 실행 가능한 파일로 있는가.
    pub executable_exists: bool,
    pub short_version: String,
    pub build: String,
}

pub struct UpdateBundleValidator;

impl UpdateBundleValidator {
    pub const EXPECTED_BUNDLE_ID: &'static str = "com.cview.CView2";
    pub const EXPECTED_EXECUTABLE: &'static str = "CView";
    pub const EXPECTED_APP_FILE_NAME: &'static str = "CView.app";
    pub const EXPECTED_TEAM_ID: &'static str = "MZ8NB937SB";

    pub fn validate(
        info: &BundleInfo,
        release: &GitHubRelease,
        current_version: &str,
        current_build: i64,
    ) -> Result<(), UpdateError> {
        if info.app_file_name != Self::EXPECTED_APP_FILE_NAME {
            return Err(UpdateError::UnexpectedAppName(info.app_file_name.clone()));
        }
        if info.bundle_identifier != Self::EXPECTED_BUNDLE_ID {
            return Err(UpdateError::InvalidBundleIdentifier {
                found: info.bundle_identifier.clone(),
                expected: Self::EXPECTED_BUNDLE_ID.into(),
            });
        }
        if info.executable != Self::EXPECTED_EXECUTABLE {
            return Err(UpdateError::InvalidExecutable {
                found: info.executable.clone(),
                expected: Self::EXPECTED_EXECUTABLE.into(),
            });
        }
        if !info.executable_exists {
            return Err(UpdateError::MissingExecutableBinary {
                name: info.executable.clone(),
            });
        }
        let build: i64 = info.build.parse().unwrap_or(0);
        if info.short_version != release.version_string() {
            return Err(UpdateError::VersionMismatch {
                found: info.short_version.clone(),
                expected: release.version_string().into(),
            });
        }
        if let Some(expected) = release.build_number() {
            if build != expected {
                return Err(UpdateError::BuildMismatch {
                    found: build,
                    expected,
                });
            }
        }
        let newer = match compare_versions(&info.short_version, current_version) {
            Ordering::Greater => true,
            Ordering::Less => false,
            Ordering::Equal => build > current_build,
        };
        if !newer {
            return Err(UpdateError::BundleNotNewer {
                found: format!("{} ({build})", info.short_version),
            });
        }
        Ok(())
    }
}

// MARK: - 서비스

#[derive(Debug)]
struct State {
    status: UpdateStatus,
    latest_release: Option<GitHubRelease>,
    last_checked_at: Option<Date>,
}

/// 업데이트 확인 서비스. `Clone` 은 같은 서비스를 가리킨다.
#[derive(Clone)]
pub struct UpdateService {
    state: Arc<Mutex<State>>,
    transport: Arc<dyn HttpTransport>,
    current_version: String,
    current_build: i64,
    now: Clock,
}

impl UpdateService {
    /// GitHub 저장소(owner/repo).
    pub const REPOSITORY: &'static str = "dododo-lab/cview-v2";
    /// 자동 설치 — 원본과 같이 꺼 둔다(릴리스 페이지를 연다).
    pub const IS_AUTO_INSTALL_ENABLED: bool = false;

    pub fn new(
        transport: Arc<dyn HttpTransport>,
        current_version: impl Into<String>,
        current_build: i64,
        now: Option<Clock>,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                status: UpdateStatus::Idle,
                latest_release: None,
                last_checked_at: None,
            })),
            transport,
            current_version: current_version.into(),
            current_build,
            now: now.unwrap_or_else(system_clock),
        }
    }

    pub fn status(&self) -> UpdateStatus {
        lock(&self.state).status.clone()
    }
    pub fn latest_release(&self) -> Option<GitHubRelease> {
        lock(&self.state).latest_release.clone()
    }
    pub fn last_checked_at(&self) -> Option<Date> {
        lock(&self.state).last_checked_at
    }
    pub fn current_version(&self) -> &str {
        &self.current_version
    }
    pub fn current_build(&self) -> i64 {
        self.current_build
    }

    /// 캐시버스터가 붙은 최신 릴리스 URL.
    pub fn latest_release_url(&self) -> Url {
        let ts = (self.now)().timestamp();
        Url::parse(&format!(
            "https://api.github.com/repos/{}/releases/latest?_ts={ts}",
            Self::REPOSITORY
        ))
        .expect("고정 URL")
    }

    /// 업데이트 확인. 404 는 릴리스 없음 → 최신으로 본다. `silent` 면 실패를 `Idle` 로 둔다.
    pub async fn check_for_updates(&self, silent: bool) {
        {
            let mut st = lock(&self.state);
            if st.status.is_busy() {
                return;
            }
            if !silent {
                st.status = UpdateStatus::Checking;
            }
        }
        let result = self.fetch_latest().await;
        let mut st = lock(&self.state);
        match result {
            Ok(None) => {
                st.last_checked_at = Some((self.now)());
                st.status = UpdateStatus::UpToDate;
            }
            Ok(Some(release)) => {
                st.last_checked_at = Some((self.now)());
                st.latest_release = Some(release.clone());
                if release.is_newer_than(&self.current_version, self.current_build) {
                    tracing::info!(
                        "Update available: {} (current: {} build {})",
                        release.version_string(),
                        self.current_version,
                        self.current_build
                    );
                    st.status = UpdateStatus::UpdateAvailable(release);
                } else {
                    st.status = UpdateStatus::UpToDate;
                }
            }
            Err(e) => {
                tracing::error!("Update check failed: {e}");
                st.status = if silent {
                    UpdateStatus::Idle
                } else {
                    UpdateStatus::Error(format!("업데이트 확인 실패: {e}"))
                };
            }
        }
    }

    async fn fetch_latest(&self) -> Result<Option<GitHubRelease>, UpdateError> {
        let mut request = HttpRequest::new(HttpMethod::Get, self.latest_release_url());
        request.set_header("Accept", Some("application/vnd.github+json"));
        request.set_header(
            "User-Agent",
            Some(&format!("CView/{}", self.current_version)),
        );
        request.set_header("Cache-Control", Some("no-cache"));
        request.set_header("Pragma", Some("no-cache"));
        request.timeout = Some(Duration::from_secs(10));
        let response = self
            .transport
            .send(request)
            .await
            .map_err(|e| UpdateError::Transport(e.to_string()))?;
        match response.status {
            200 => serde_json::from_slice(&response.body)
                .map(Some)
                .map_err(|_| UpdateError::InvalidResponse),
            404 => Ok(None),
            other => Err(UpdateError::HttpStatus(other)),
        }
    }

    /// 릴리스 페이지를 열었다(셸이 브라우저를 띄운 뒤 부른다).
    pub fn note_opened_release_page(&self, version: impl Into<String>) {
        lock(&self.state).status = UpdateStatus::OpenedReleasePage {
            version: version.into(),
        };
    }

    pub fn dismiss_error(&self) {
        let mut st = lock(&self.state);
        if matches!(st.status, UpdateStatus::Error(_)) {
            st.status = UpdateStatus::Idle;
        }
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/UpdateServiceTests.swift`(번들 검증) + 버전 비교.
    use super::*;
    use async_trait::async_trait;
    use cview_networking::{HttpResponse, TransportError};

    fn release(tag: &str, asset: &str) -> GitHubRelease {
        GitHubRelease {
            tag_name: tag.into(),
            name: None,
            body: None,
            html_url: None,
            published_at: None,
            assets: vec![GitHubAsset {
                name: asset.into(),
                size: 1,
                browser_download_url: Url::parse("https://example.com/a").unwrap(),
            }],
        }
    }

    fn info(version: &str, build: &str) -> BundleInfo {
        BundleInfo {
            app_file_name: "CView.app".into(),
            bundle_identifier: UpdateBundleValidator::EXPECTED_BUNDLE_ID.into(),
            executable: "CView".into(),
            executable_exists: true,
            short_version: version.into(),
            build: build.into(),
        }
    }

    #[test]
    fn version_and_build_parsing() {
        assert_eq!(compare_versions("2.0.1", "2.0.0"), Ordering::Greater);
        assert_eq!(compare_versions("2.0", "2.0.0"), Ordering::Equal);
        assert_eq!(compare_versions("1.9.9", "2.0"), Ordering::Less);
        let r = release("v2.0.0", "CView-2.0.0-65.zip");
        assert_eq!(r.version_string(), "2.0.0");
        assert_eq!(r.build_number(), Some(65));
        assert!(r.is_newer_than("2.0.0", 64));
        assert!(!r.is_newer_than("2.0.0", 65));
        assert!(!release("2.0.0", "CView.zip").is_newer_than("2.0.0", 1));
    }

    #[test]
    fn bundle_validation_rules() {
        let r = release("v2.0.1", "CView-2.0.1-70.zip");
        assert!(UpdateBundleValidator::validate(&info("2.0.1", "70"), &r, "2.0.0", 65).is_ok());

        let mut wrong = info("2.0.1", "70");
        wrong.bundle_identifier = "com.evil".into();
        assert!(matches!(
            UpdateBundleValidator::validate(&wrong, &r, "2.0.0", 65),
            Err(UpdateError::InvalidBundleIdentifier { .. })
        ));

        let down = release("v1.0.0", "CView-1.0.0-1.zip");
        assert!(matches!(
            UpdateBundleValidator::validate(&info("1.0.0", "1"), &down, "2.0.0", 65),
            Err(UpdateError::BundleNotNewer { .. })
        ));

        let mut missing = info("2.0.1", "70");
        missing.executable_exists = false;
        assert!(matches!(
            UpdateBundleValidator::validate(&missing, &r, "2.0.0", 65),
            Err(UpdateError::MissingExecutableBinary { .. })
        ));
        assert!(matches!(
            UpdateBundleValidator::validate(&info("2.0.1", "71"), &r, "2.0.0", 65),
            Err(UpdateError::BuildMismatch { .. })
        ));
    }

    struct Fixed(u16, Vec<u8>);

    #[async_trait]
    impl HttpTransport for Fixed {
        async fn send(&self, _: HttpRequest) -> Result<HttpResponse, TransportError> {
            Ok(HttpResponse::new(self.0, self.1.clone()))
        }
    }

    #[tokio::test]
    async fn check_for_updates_paths() {
        let body = serde_json::to_vec(&release("v2.1.0", "CView-2.1.0-80.zip")).unwrap();
        let s = UpdateService::new(Arc::new(Fixed(200, body)), "2.0.0", 65, None);
        s.check_for_updates(false).await;
        assert!(matches!(s.status(), UpdateStatus::UpdateAvailable(_)));

        let s = UpdateService::new(Arc::new(Fixed(404, vec![])), "2.0.0", 65, None);
        s.check_for_updates(false).await;
        assert_eq!(s.status(), UpdateStatus::UpToDate);

        let s = UpdateService::new(Arc::new(Fixed(500, vec![])), "2.0.0", 65, None);
        s.check_for_updates(true).await;
        assert_eq!(s.status(), UpdateStatus::Idle);
        s.check_for_updates(false).await;
        assert_eq!(
            s.status(),
            UpdateStatus::Error("업데이트 확인 실패: HTTP 500 오류".into())
        );
        s.dismiss_error();
        assert_eq!(s.status(), UpdateStatus::Idle);
    }
}
