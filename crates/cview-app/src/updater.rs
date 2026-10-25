//! 앱 안 업데이트 설치(SYS-08) — "업데이트 설치" 한 번으로 새 버전을 받아 검증하고 바꿔 다시 연다.
//!
//! 1. 피드를 다시 받는다(프런트가 넘긴 주소는 믿지 않는다). 새 버전이 아니면 멈춘다.
//! 2. 이 OS 설치 파일을 앱 캐시 폴더로 받는다. 진행률은 [`UPDATE_PROGRESS`] 로 보낸다.
//! 3. 피드에 적힌 크기·SHA-256 과 비교한다. 피드에 해시가 없으면(예전 GitHub 형식) 자동 설치하지 않는다.
//! 4. macOS: DMG 를 Finder 에 띄우지 않고(`-nobrowse`) 붙여 `CView.app` 을 옆 폴더로 복사하고, 복사본이
//!    Developer ID 팀 [`TEAM_ID`]·번들 [`BUNDLE_ID`] 로 서명됐는지(`codesign -R`), Apple 공증을 통과하는지
//!    (`spctl`), 버전이 피드와 같은지 본다. 통과하면 지금 번들과 이름을 맞바꾸고, 앱이 끝나면 새 번들을 연다.
//!    Windows: NSIS 설치 파일을 수동 조작 없는 모드(`/P /R /UPDATE`)로 실행한다 — 설치 파일이 앱을 닫고
//!    설치한 뒤 다시 연다. 설치 파일은 아직 코드 서명이 없어 SHA-256(HTTPS 피드)만 확인한다.
//!
//! 피드 형식은 바꾸지 않는다 — `publish.py` 가 이미 자산마다 `sha256` 을 적는다. 예전 앱은 이 필드를 무시한다.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use cview_app_core::services::update_service::{compare_versions, GitHubAsset};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncWriteExt;

use crate::commands::{CmdResult, CommandError};
use crate::system::{current_feed_url, fetch_release, release_installer};

/// 진행 이벤트 — `{ phase, received, total }`. phase: download · verify · install · restart.
pub const UPDATE_PROGRESS: &str = "update://progress";

/// 업데이트 번들이 가져야 할 서명 — Developer ID Application 인증서, 팀, 번들 ID.
pub const TEAM_ID: &str = "MZ8NB937SB";
pub const BUNDLE_ID: &str = "app.cview.rust";

/// 설치를 한 번에 하나만. 성공하면 앱이 곧 끝나므로 다시 내리지 않는다.
static INSTALLING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressDto {
    phase: &'static str,
    received: u64,
    total: u64,
}

fn emit(app: &AppHandle, phase: &'static str, received: u64, total: u64) {
    let _ = app.emit(
        UPDATE_PROGRESS,
        ProgressDto {
            phase,
            received,
            total,
        },
    );
}

fn err(msg: impl Into<String>) -> CommandError {
    CommandError(msg.into())
}

/// 64자리 16진수 SHA-256 → 바이트. 대소문자는 가리지 않는다. 순수 함수.
pub fn parse_sha256(value: &str) -> Option<[u8; 32]> {
    let v = value.trim();
    if v.len() != 64 || !v.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&v[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// OS 별 자동 설치가 다루는 설치 파일 확장자.
fn installer_ext(os: &str) -> Option<&'static str> {
    match os {
        "macos" => Some(".dmg"),
        "windows" => Some(".exe"),
        _ => None,
    }
}

/// 이 자산을 앱 안에서 받아 설치할 수 있는가 — 지원 OS · 맞는 형식 · 크기 · SHA-256. 순수 함수.
pub fn can_auto_install(asset: &GitHubAsset, os: &str) -> bool {
    let Some(ext) = installer_ext(os) else {
        return false;
    };
    asset.size > 0
        && asset.name.to_lowercase().ends_with(ext)
        && asset.sha256.as_deref().and_then(parse_sha256).is_some()
}

/// 경로에 쓰는 버전은 `1.2.3` 꼴만. 순수 함수.
fn plain_version(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 6 && p.bytes().all(|b| b.is_ascii_digit()))
}

/// 업데이트 받기 → 검증 → 설치 → 재실행. 성공하면 앱이 곧 끝난다.
#[tauri::command]
pub async fn update_install(app: AppHandle) -> CmdResult<()> {
    if INSTALLING.swap(true, Ordering::SeqCst) {
        return Err(err("이미 업데이트를 설치하고 있습니다"));
    }
    let result = install(&app).await;
    match &result {
        Ok(()) => {
            emit(&app, "restart", 0, 0);
            let handle = app.clone();
            tauri::async_runtime::spawn(async move {
                // 프런트가 "다시 시작하는 중" 을 그릴 틈.
                tokio::time::sleep(Duration::from_millis(400)).await;
                handle.exit(0);
            });
        }
        Err(e) => {
            tracing::warn!("업데이트 설치 실패: {}", e.0);
            INSTALLING.store(false, Ordering::SeqCst);
        }
    }
    result
}

async fn install(app: &AppHandle) -> CmdResult<()> {
    let os = std::env::consts::OS;
    if installer_ext(os).is_none() {
        return Err(err("이 운영체제에서는 자동 업데이트를 지원하지 않습니다"));
    }
    let current = app.package_info().version.to_string();
    let release = fetch_release(current_feed_url()?)
        .await?
        .ok_or_else(|| err("공개된 릴리스가 없습니다"))?;
    let version = release.version_string().to_string();
    if !plain_version(&version) {
        return Err(err(format!(
            "릴리스 버전 형식이 올바르지 않습니다: {version}"
        )));
    }
    if compare_versions(&version, &current) != std::cmp::Ordering::Greater {
        return Err(err(format!("이미 최신 버전입니다 ({current})")));
    }
    let asset = release_installer(&release)
        .ok_or_else(|| err("이 운영체제용 설치 파일이 릴리스에 없습니다"))?;
    if !can_auto_install(asset, os) {
        return Err(err(
            "이 릴리스에는 자동 설치에 필요한 검증 정보(SHA-256)가 없습니다 — 다운로드해서 직접 설치해 주세요",
        ));
    }
    let expected = asset
        .sha256
        .as_deref()
        .and_then(parse_sha256)
        .unwrap_or_default();

    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| err(format!("캐시 폴더를 찾지 못했습니다: {e}")))?
        .join("updates");
    // 지난 시도에서 남은 파일(받다 만 파일 · 예전 설치 파일)은 버린다.
    let _ = tokio::fs::remove_dir_all(&dir).await;
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| err(format!("업데이트 폴더를 만들지 못했습니다: {e}")))?;
    let file = dir.join(format!(
        "CView_{version}{}",
        installer_ext(os).unwrap_or_default()
    ));

    let handle = app.clone();
    download(
        &move |received, total| emit(&handle, "download", received, total),
        asset,
        &file,
        &expected,
    )
    .await?;
    emit(app, "verify", 0, 0);

    #[cfg(target_os = "macos")]
    {
        let (file, stage, version) = (file.clone(), dir.clone(), version.clone());
        let handle = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let bundle = mac::current_bundle()?;
            let old = mac::install(&file, &stage, &version, &bundle, &|| {
                emit(&handle, "install", 0, 0)
            })?;
            mac::relaunch_after_exit(std::process::id(), &bundle, &old, "/usr/bin/open")
        })
        .await
        .map_err(|e| err(format!("업데이트 설치 작업이 중단됐습니다: {e}")))??;
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[cfg(target_os = "windows")]
    {
        emit(app, "install", 0, 0);
        windows::launch_installer(&file)?;
    }
    Ok(())
}

/// 설치 파일을 받으며 크기·SHA-256 을 확인한다. 맞지 않으면 파일을 지운다.
async fn download(
    progress: &(dyn Fn(u64, u64) + Send + Sync),
    asset: &GitHubAsset,
    file: &Path,
    expected: &[u8; 32],
) -> CmdResult<()> {
    let total = u64::try_from(asset.size).unwrap_or(0);
    let client = reqwest::Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| err(format!("다운로드 연결 초기화 실패: {e}")))?;
    let mut resp = client
        .get(asset.browser_download_url.clone())
        .header("User-Agent", "CView-Rust")
        .send()
        .await
        .map_err(|e| err(format!("설치 파일 다운로드를 시작하지 못했습니다: {e}")))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(err(format!("설치 파일 다운로드 실패 (HTTP {status})")));
    }
    if resp.content_length().is_some_and(|len| len != total) {
        return Err(err("설치 파일 크기가 릴리스 정보와 다릅니다"));
    }

    let part = file.with_extension("part");
    let mut out = tokio::fs::File::create(&part)
        .await
        .map_err(|e| err(format!("설치 파일을 저장하지 못했습니다: {e}")))?;
    let mut hasher = Sha256::new();
    let mut received: u64 = 0;
    let mut last = Instant::now();
    progress(0, total);
    let outcome: CmdResult<()> = async {
        loop {
            let chunk = tokio::time::timeout(Duration::from_secs(60), resp.chunk())
                .await
                .map_err(|_| err("다운로드가 60초 동안 멈춰 중단했습니다"))?
                .map_err(|e| err(format!("다운로드가 끊겼습니다: {e}")))?;
            let Some(chunk) = chunk else { break };
            received += chunk.len() as u64;
            if received > total {
                return Err(err("설치 파일이 릴리스 정보보다 큽니다"));
            }
            hasher.update(&chunk);
            out.write_all(&chunk)
                .await
                .map_err(|e| err(format!("설치 파일을 저장하지 못했습니다: {e}")))?;
            if last.elapsed() >= Duration::from_millis(120) {
                last = Instant::now();
                progress(received, total);
            }
        }
        out.flush()
            .await
            .and(out.sync_all().await)
            .map_err(|e| err(format!("설치 파일을 저장하지 못했습니다: {e}")))?;
        progress(received, total);
        if received != total {
            return Err(err("설치 파일을 끝까지 받지 못했습니다"));
        }
        if hasher.finalize().as_slice() != expected {
            return Err(err(
                "받은 설치 파일의 SHA-256 이 릴리스 정보와 다릅니다 — 설치하지 않았습니다",
            ));
        }
        Ok(())
    }
    .await;
    drop(out);
    if let Err(e) = outcome {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(e);
    }
    tokio::fs::rename(&part, file)
        .await
        .map_err(|e| err(format!("설치 파일을 저장하지 못했습니다: {e}")))
}

/// 실행 파일 `X.app/Contents/MacOS/bin` → 번들 `X.app`. 순수 함수.
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    (macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.extension()? == "app")
        .then(|| bundle.to_path_buf())
}

/// 교체할 수 없는 위치면 그 이유. Gatekeeper 가 격리 실행한 앱(App Translocation)은 진짜 위치를 모른다. 순수 함수.
pub fn replace_blocker(bundle: &Path) -> Option<&'static str> {
    if bundle
        .components()
        .any(|c| c.as_os_str() == "AppTranslocation")
    {
        return Some(
            "격리된 위치에서 실행 중이라 바꿀 수 없습니다 — CView 를 응용 프로그램 폴더로 옮긴 뒤 다시 시도해 주세요",
        );
    }
    None
}

/// `codesign --test-requirement` 에 넘길 요구 사항 — Apple 이 발급한 Developer ID Application 인증서,
/// 우리 팀, 우리 번들 ID.
pub fn signing_requirement() -> String {
    format!(
        "=identifier \"{BUNDLE_ID}\" and anchor apple generic \
         and certificate 1[field.1.2.840.113635.100.6.2.6] exists \
         and certificate leaf[field.1.2.840.113635.100.6.1.13] exists \
         and certificate leaf[subject.OU] = \"{TEAM_ID}\""
    )
}

#[cfg(target_os = "macos")]
mod mac {
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    use super::{bundle_of, replace_blocker, signing_requirement};
    use crate::commands::CommandError;

    fn err(msg: impl Into<String>) -> CommandError {
        CommandError(msg.into())
    }

    /// 도구를 실행하고 실패하면 stderr 첫 줄을 붙여 돌려준다.
    fn run(tool: &str, args: &[&std::ffi::OsStr], what: &str) -> Result<String, CommandError> {
        let out = Command::new(tool)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| err(format!("{what}: {e}")))?;
        if !out.status.success() {
            let msg = String::from_utf8_lossy(&out.stderr);
            let line = msg
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("")
                .trim();
            return Err(err(format!("{what}: {line}")));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// 붙인 DMG — 놓아도(오류로 빠져나가도) 떼어 낸다.
    struct Mounted(PathBuf);

    impl Drop for Mounted {
        fn drop(&mut self) {
            let _ = Command::new("/usr/bin/hdiutil")
                .arg("detach")
                .arg(&self.0)
                .args(["-force", "-quiet"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let _ = std::fs::remove_dir(&self.0);
        }
    }

    /// 실행 중인 앱 번들 — 바꿀 수 없는 위치면 그 이유로 실패.
    pub fn current_bundle() -> Result<PathBuf, CommandError> {
        let exe = std::env::current_exe()
            .map_err(|e| err(format!("실행 중인 앱 위치를 알 수 없습니다: {e}")))?;
        let bundle = bundle_of(&exe)
            .ok_or_else(|| err("앱 번들(.app) 안에서 실행 중이 아니라 바꿀 수 없습니다"))?;
        if let Some(why) = replace_blocker(&bundle) {
            return Err(err(why));
        }
        Ok(bundle)
    }

    /// DMG 속 새 버전을 검증해 `bundle` 자리에 넣는다. 돌려주는 값은 옮겨 둔 예전 번들(재실행 뒤 지운다).
    pub fn install(
        dmg: &Path,
        work: &Path,
        version: &str,
        bundle: &Path,
        on_install: &dyn Fn(),
    ) -> Result<PathBuf, CommandError> {
        let parent = bundle
            .parent()
            .ok_or_else(|| err("앱이 있는 폴더를 알 수 없습니다"))?;
        let staged = parent.join(format!(".CView-update-{version}.app"));
        let old = parent.join(format!(".CView-old-{}.app", std::process::id()));
        let _ = std::fs::remove_dir_all(&staged);

        // 1) DMG 를 화면에 띄우지 않고 붙여 새 번들을 앱 옆으로 복사한다.
        let mount_point = work.join("mnt");
        std::fs::create_dir_all(&mount_point).map_err(|e| {
            err(format!(
                "디스크 이미지를 붙일 폴더를 만들지 못했습니다: {e}"
            ))
        })?;
        {
            run(
                "/usr/bin/hdiutil",
                &[
                    "attach".as_ref(),
                    "-nobrowse".as_ref(),
                    "-noautoopen".as_ref(),
                    "-readonly".as_ref(),
                    "-quiet".as_ref(),
                    "-mountpoint".as_ref(),
                    mount_point.as_os_str(),
                    dmg.as_os_str(),
                ],
                "디스크 이미지를 열지 못했습니다",
            )?;
            let mounted = Mounted(mount_point);
            let apps: Vec<PathBuf> = std::fs::read_dir(&mounted.0)
                .map_err(|e| err(format!("디스크 이미지를 읽지 못했습니다: {e}")))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "app") && p.is_dir())
                .collect();
            let [source] = apps.as_slice() else {
                return Err(err(format!(
                    "디스크 이미지에 앱이 {}개 있습니다 (1개여야 합니다)",
                    apps.len()
                )));
            };
            if source.file_name().is_none_or(|n| n != "CView.app") {
                return Err(err("디스크 이미지 속 앱 이름이 CView.app 이 아닙니다"));
            }
            if let Err(e) = run(
                "/usr/bin/ditto",
                &[source.as_os_str(), staged.as_os_str()],
                "새 버전을 복사하지 못했습니다",
            ) {
                let _ = std::fs::remove_dir_all(&staged);
                return Err(if e.0.contains("ermission") || e.0.contains("Read-only") {
                    err("앱 폴더에 쓸 권한이 없습니다 — 다운로드해서 직접 설치해 주세요")
                } else {
                    e
                });
            }
        }

        // 2) 실제로 설치할 복사본을 검사한다: 서명(팀·번들 ID) · 공증 · 버전.
        let checked = (|| {
            run(
                "/usr/bin/codesign",
                &[
                    "--verify".as_ref(),
                    "--deep".as_ref(),
                    "--strict".as_ref(),
                    "--test-requirement".as_ref(),
                    signing_requirement().as_ref(),
                    staged.as_os_str(),
                ],
                "새 버전의 코드 서명이 CView 개발자 서명과 맞지 않습니다",
            )?;
            run(
                "/usr/sbin/spctl",
                &[
                    "--assess".as_ref(),
                    "--type".as_ref(),
                    "execute".as_ref(),
                    staged.as_os_str(),
                ],
                "새 버전이 Apple 공증 검사를 통과하지 못했습니다",
            )?;
            let plist = staged.join("Contents/Info.plist");
            let found = run(
                "/usr/bin/plutil",
                &[
                    "-extract".as_ref(),
                    "CFBundleShortVersionString".as_ref(),
                    "raw".as_ref(),
                    "-o".as_ref(),
                    "-".as_ref(),
                    plist.as_os_str(),
                ],
                "새 버전 정보를 읽지 못했습니다",
            )?;
            if found != version {
                return Err(err(format!(
                    "디스크 이미지 속 앱 버전({found})이 릴리스({version})와 다릅니다"
                )));
            }
            Ok(())
        })();
        if let Err(e) = checked {
            let _ = std::fs::remove_dir_all(&staged);
            return Err(e);
        }

        // 3) 이름 맞바꾸기 — 실패하면 원래대로 돌린다. 실행 중인 번들은 옮겨도 이 프로세스는 계속 돈다.
        on_install();
        std::fs::rename(bundle, &old).map_err(|e| {
            let _ = std::fs::remove_dir_all(&staged);
            err(format!("지금 앱을 옮기지 못했습니다: {e}"))
        })?;
        if let Err(e) = std::fs::rename(&staged, bundle) {
            let _ = std::fs::rename(&old, bundle);
            let _ = std::fs::remove_dir_all(&staged);
            return Err(err(format!("새 버전을 넣지 못했습니다: {e}")));
        }

        Ok(old)
    }

    /// `pid` 가 끝나면(최대 60초 기다림) `opener` 로 새 번들을 열고 예전 번들을 지운다.
    /// 앱과 다른 프로세스 그룹에서 돌아 앱이 끝나도 같이 끝나지 않는다.
    pub fn relaunch_after_exit(
        pid: u32,
        bundle: &Path,
        old: &Path,
        opener: &str,
    ) -> Result<(), CommandError> {
        use std::os::unix::process::CommandExt;
        let script = r#"i=0; while /bin/kill -0 "$1" 2>/dev/null && [ $i -lt 300 ]; do /bin/sleep 0.2; i=$((i+1)); done; "$4" "$2"; /bin/rm -rf "$3""#;
        Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .arg("cview-relaunch")
            .arg(pid.to_string())
            .arg(bundle)
            .arg(old)
            .arg(opener)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .map(|_| ())
            .map_err(|e| {
                err(format!(
                    "새 버전은 설치했지만 다시 열지 못했습니다 — CView 를 직접 다시 열어 주세요 ({e})"
                ))
            })
    }

    /// 실제 업데이트 서버로 끝까지 — 받기 · SHA-256 · DMG · 서명 · 공증 · 교체 · 재실행 신호.
    /// 진짜 앱은 건드리지 않는다: 임시 폴더의 가짜 `CView.app` 을 바꾸고, `open` 대신 기록 스크립트를 부른다.
    /// Tailscale 필요 — `cargo test -p cview-app --lib -- --ignored updater_e2e_real_feed`.
    #[cfg(test)]
    mod e2e {
        use std::path::Path;
        use std::time::Duration;

        use crate::system::{current_feed_url, fetch_release, release_installer};
        use crate::updater::{can_auto_install, download, parse_sha256};

        fn plist_version(app: &Path) -> String {
            let out = std::process::Command::new("/usr/bin/plutil")
                .args(["-extract", "CFBundleShortVersionString", "raw", "-o", "-"])
                .arg(app.join("Contents/Info.plist"))
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }

        #[tokio::test]
        #[ignore]
        async fn updater_e2e_real_feed() {
            let release = fetch_release(current_feed_url().unwrap())
                .await
                .unwrap()
                .expect("공개된 릴리스");
            let version = release.version_string().to_string();
            let asset = release_installer(&release)
                .expect("macOS 설치 파일")
                .clone();
            assert!(can_auto_install(&asset, "macos"), "{asset:?}");
            let expected = parse_sha256(asset.sha256.as_deref().unwrap()).unwrap();

            let tmp = tempfile::tempdir().unwrap();
            let work = tmp.path().join("updates");
            std::fs::create_dir_all(&work).unwrap();
            let dmg = work.join(format!("CView_{version}.dmg"));
            let seen = std::sync::Mutex::new((0u64, 0u64, 0usize));
            download(
                &|r, t| {
                    let mut s = seen.lock().unwrap();
                    *s = (r, t, s.2 + 1);
                },
                &asset,
                &dmg,
                &expected,
            )
            .await
            .unwrap();
            let (received, total, calls) = *seen.lock().unwrap();
            assert_eq!((received, total), (asset.size as u64, asset.size as u64));
            assert!(calls >= 2);

            // 받은 파일을 1바이트 바꾸면 SHA-256 에서 막혀야 한다.
            let mut wrong = expected;
            wrong[0] ^= 1;
            let bad = work.join("bad.dmg");
            assert!(download(&|_, _| {}, &asset, &bad, &wrong).await.is_err());
            assert!(!bad.exists() && !bad.with_extension("part").exists());

            // 가짜 "지금 앱".
            let apps = tmp.path().join("Applications");
            let bundle = apps.join("CView.app");
            std::fs::create_dir_all(bundle.join("Contents")).unwrap();
            std::fs::write(bundle.join("Contents/marker"), "old").unwrap();

            let old = super::install(&dmg, &work, &version, &bundle, &|| {}).unwrap();
            assert_eq!(plist_version(&bundle), version);
            assert_eq!(
                std::fs::read_to_string(old.join("Contents/marker")).unwrap(),
                "old"
            );
            assert!(!work.join("mnt").exists(), "DMG 를 떼야 한다");
            assert!(!apps.join(format!(".CView-update-{version}.app")).exists());

            // 버전이 다르면 교체하지 않고 원래 번들을 남긴다.
            let before = plist_version(&bundle);
            assert!(super::install(&dmg, &work, "9.9.9", &bundle, &|| {}).is_err());
            assert_eq!(plist_version(&bundle), before);

            // 재실행: 프로세스가 끝나야 opener 를 부르고 예전 번들을 지운다.
            let opened = tmp.path().join("opened");
            let opener = tmp.path().join("opener.sh");
            std::fs::write(
                &opener,
                format!("#!/bin/sh\necho \"$1\" > '{}'\n", opened.display()),
            )
            .unwrap();
            std::process::Command::new("/bin/chmod")
                .arg("+x")
                .arg(&opener)
                .status()
                .unwrap();
            let mut child = std::process::Command::new("/bin/sleep")
                .arg("1")
                .spawn()
                .unwrap();
            super::relaunch_after_exit(child.id(), &bundle, &old, opener.to_str().unwrap())
                .unwrap();
            tokio::time::sleep(Duration::from_millis(500)).await;
            assert!(!opened.exists(), "프로세스가 끝나기 전에 열면 안 된다");
            child.wait().unwrap();
            for _ in 0..50 {
                if opened.exists() && !old.exists() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            assert_eq!(
                std::fs::read_to_string(&opened).unwrap().trim(),
                bundle.to_str().unwrap()
            );
            assert!(!old.exists(), "예전 번들을 지워야 한다");
        }
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use std::os::windows::process::CommandExt;
    use std::path::Path;
    use std::process::Command;

    use crate::commands::CommandError;

    const DETACHED_PROCESS: u32 = 0x0000_0008;

    /// Tauri NSIS 설치 파일: `/P` 진행 창만 · `/R` 설치 후 다시 실행 · `/UPDATE` 바로 가기·자동 시작 유지.
    /// 설치 파일이 실행 중인 CView 를 닫고 설치한다.
    pub fn launch_installer(installer: &Path) -> Result<(), CommandError> {
        Command::new(installer)
            .args(["/P", "/R", "/UPDATE"])
            .creation_flags(DETACHED_PROCESS)
            .spawn()
            .map(|_| ())
            .map_err(|e| CommandError(format!("설치 파일을 실행하지 못했습니다: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str, size: i64, sha: Option<&str>) -> GitHubAsset {
        GitHubAsset {
            name: name.into(),
            size,
            browser_download_url: url::Url::parse(&format!("https://example.com/{name}")).unwrap(),
            sha256: sha.map(Into::into),
        }
    }

    const SHA: &str = "76f238f606250c87c6beac75dccd35ee99070a13490555936abb6cb64ecce3d0";

    #[test]
    fn sha256_parsing() {
        let b = parse_sha256(SHA).unwrap();
        assert_eq!(b[0], 0x76);
        assert_eq!(b[31], 0xd0);
        assert_eq!(parse_sha256(&SHA.to_uppercase()), Some(b));
        assert_eq!(parse_sha256(&format!(" {SHA}\n")), Some(b));
        assert!(parse_sha256(&SHA[1..]).is_none());
        assert!(parse_sha256(&format!("{}zz", &SHA[2..])).is_none());
        assert!(parse_sha256(&format!("{}가", &SHA[3..])).is_none());
        assert!(parse_sha256("").is_none());
    }

    #[test]
    fn auto_install_needs_hash_and_matching_installer() {
        assert!(can_auto_install(
            &asset("CView_0.1.24_arm64.dmg", 10, Some(SHA)),
            "macos"
        ));
        assert!(can_auto_install(
            &asset("CView_0.1.24_x64-setup.exe", 10, Some(SHA)),
            "windows"
        ));
        // 예전 GitHub 형식(해시 없음) · 잘못된 해시 · 크기 0 · 다른 형식 · 지원하지 않는 OS
        assert!(!can_auto_install(
            &asset("CView_0.1.24_arm64.dmg", 10, None),
            "macos"
        ));
        assert!(!can_auto_install(
            &asset("CView_0.1.24_arm64.dmg", 10, Some("abc")),
            "macos"
        ));
        assert!(!can_auto_install(
            &asset("CView_0.1.24_arm64.dmg", 0, Some(SHA)),
            "macos"
        ));
        assert!(!can_auto_install(
            &asset("CView_0.1.24_x64.msi", 10, Some(SHA)),
            "windows"
        ));
        assert!(!can_auto_install(
            &asset("CView_0.1.24_arm64.dmg", 10, Some(SHA)),
            "windows"
        ));
        assert!(!can_auto_install(
            &asset("CView_0.1.24.tar.gz", 10, Some(SHA)),
            "linux"
        ));
    }

    #[test]
    fn feed_asset_hash_is_read() {
        let json = format!(
            r#"{{"tag_name":"v0.1.24","assets":[{{"name":"CView_0.1.24_arm64.dmg","size":5,
            "browser_download_url":"https://example.com/a.dmg","sha256":"{SHA}"}},
            {{"name":"old.dmg","size":5,"browser_download_url":"https://example.com/b.dmg"}}]}}"#
        );
        let r: cview_app_core::services::update_service::GitHubRelease =
            serde_json::from_str(&json).unwrap();
        assert_eq!(r.assets[0].sha256.as_deref(), Some(SHA));
        assert_eq!(r.assets[1].sha256, None);
    }

    #[test]
    fn version_shape() {
        assert!(plain_version("0.1.24"));
        assert!(!plain_version("0.1"));
        assert!(!plain_version("0.1.24-beta"));
        assert!(!plain_version("../1.2"));
        assert!(!plain_version("1..2"));
    }

    #[test]
    fn bundle_paths() {
        assert_eq!(
            bundle_of(Path::new(
                "/Applications/CView.app/Contents/MacOS/cview-app"
            )),
            Some(PathBuf::from("/Applications/CView.app"))
        );
        assert_eq!(bundle_of(Path::new("/usr/local/bin/cview-app")), None);
        assert_eq!(bundle_of(Path::new("/x/target/release/cview-app")), None);
        assert!(replace_blocker(Path::new("/Applications/CView.app")).is_none());
        assert!(replace_blocker(Path::new(
            "/private/var/folders/x/AppTranslocation/ABC/d/CView.app"
        ))
        .is_some());
    }

    #[test]
    fn requirement_pins_team_and_bundle() {
        let r = signing_requirement();
        assert!(r.starts_with('='));
        assert!(r.contains("identifier \"app.cview.rust\""));
        assert!(r.contains("subject.OU] = \"MZ8NB937SB\""));
        assert!(r.contains("1.2.840.113635.100.6.1.13"));
    }
}
