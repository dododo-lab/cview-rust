# CView v2 — Rust 포팅

Swift 6 macOS 앱(원본 저장소 `dododo-lab/cview-v2` 의 `Sources/`)을 Rust 워크스페이스로 옮기는 작업. 계획과 단계는 [`PORTING_PLAN.md`](PORTING_PLAN.md).

## 데스크톱 앱 실행 (Tauri 2)

저장소 루트(이 `README.md` 가 있는 폴더)가 Cargo 워크스페이스다. 루트에서 실행한다.

```sh
cargo run -p cview-app
```

- 화면은 `app-ui` 의 정적 HTML/CSS/ES 모듈이다 — npm 이나 번들러 빌드 단계가 없다.
- 라이브 재생은 기존 hls.js 플레이어(`app-ui/player/player.html`, 원본 `hlsjs-player.html`)를 그대로 쓰고,
  Rust(`cview-player`)의 로컬 프록시·코디네이터가 스트림 URL 과 수명을 맡는다.
- 데이터(SQLite 스토어 `CView_v2.store`, 설정 미러 `settings.json`)는 OS 앱 데이터 디렉터리
  (`app.cview.rust`) 아래에 저장된다.
- 로그 수준은 `RUST_LOG=debug cargo run -p cview-app` 처럼 조절한다.

선택: Tauri CLI 가 있으면 `cd crates/cview-app && cargo tauri dev` 로도 실행할 수 있다
(`cargo install tauri-cli --version "^2"`).

Linux 에서는 `webkit2gtk-4.1`, `libsoup-3.0` 개발 패키지가 필요하다.

## 검사

저장소 루트에서:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
for f in app-ui/js/*.js; do node --check "$f"; done
```

배포는 `scripts/release-macos.sh --dmg`가 검사·버전 증가·macOS 서명/공증·Windows 설치 파일 생성·업데이트 서버 업로드/게시·양쪽 다운로드 검증·Git 푸시까지 실행한다.

```sh
scripts/release-macos.sh --dmg
```

같은 Mac에서는 다른 AI도 이 명령을 사용하면 전용 SSH 키로 자동 게시한다. 공증 프로파일 기본값은 `cview-notarize`이다. Tailscale 연결과 깨끗한 작업 트리가 필요하다. [자동 게시·재시도·다른 PC 등록](tools/windows-update-server/AUTOMATION.md), [AI 작업 지침](AGENTS.md).

## 릴리스 빌드 시 업데이트 서버 게시 필수

**배포용 릴리스 빌드는 매번 업데이트 서버 게시와 검증까지 완료해야 한다.** 설치 파일을 로컬 `dist/`에 만들기만 한 상태는 릴리스 완료로 보지 않는다. 이 규칙은 macOS와 Windows 모두에 적용한다. 개발용 실행·디버그 빌드는 게시 대상이 아니다.

- 업데이트 서버: https://win-7sv7uak6k5b.horse-solfege.ts.net/
- 최신 릴리스 피드: https://win-7sv7uak6k5b.horse-solfege.ts.net/releases/latest.json
- 서버 게시 경로: `C:\CViewUpdates\site`
- 서버 게시·검증 도구: `C:\CViewUpdates\tools\publish.py`, `C:\CViewUpdates\tools\check_server.py` (전용 `tools\python\python.exe`로 실행)
- 외부 PC 업로드 공유: `\\100.117.247.29\CViewUpload$` (기존 서버 Administrator 계정, SMB 암호화 필수). macOS에서는 `smb://100.117.247.29/CViewUpload$`로 연결한다. [업로드·게시 사용 방법](tools/windows-update-server/README.md).
- 원격 PowerShell: Tailscale 연결 후 `ssh Administrator@100.117.247.29`로 접속한다. [접속·원격 게시 명령](tools/windows-update-server/POWERSHELL-REMOTE.md).
- 버전별 설치 파일과 `notes.txt`를 업로드한 뒤 서버의 관리자 PowerShell에서 `C:\CViewUpdates\tools\Publish-UploadedRelease.ps1 -Version <버전>`으로 게시·다운로드 검증을 실행할 수 있다.
- 앱의 업데이트 확인과 다운로드에는 같은 Tailscale 네트워크 연결이 필요하다.

릴리스마다 다음 순서를 반드시 따른다.

1. 기존 게시 버전보다 높은 새 버전으로 Cargo 작업공간·Tauri 버전과 macOS 빌드 번호를 맞추고 검사·테스트를 통과시킨다. 기존 게시 버전의 파일을 덮어쓰지 않는다.
2. **macOS와 Windows 모두** 릴리스 설치 파일을 빌드한다. 위 최신 릴리스 피드 주소는 앱에 기본값으로 들어 있으며, 다른 서버를 쓸 때만 빌드 환경 변수 `CVIEW_UPDATE_URL`로 바꾼다(실행 시 같은 이름의 환경 변수가 있으면 그 값이 우선한다). macOS는 Developer ID 서명·Apple 공증·티켓 첨부 및 검증을 완료한다. Windows는 코드 서명을 적용하며, 인증서가 없어 미서명으로 배포할 때에는 다운로드 페이지의 릴리스 설명에 이를 명시한다.
3. 같은 버전의 완성된 macOS·Windows 설치 파일과 릴리스 설명을 서버에 준비한다. `_local` 빌드, 소스 압축 파일, 테스트용 가짜 설치 파일은 게시하지 않는다.
4. 서버의 관리자 PowerShell에서 `publish.py`로 게시한다. 여러 설치 파일은 `--artifact`를 반복해 한 릴리스에 함께 등록한다. 버전 폴더와 설치 파일을 먼저 배치하고 `latest.json`을 마지막에 원자적으로 갱신하는 게시 도구를 사용한다.
5. `check_server.py --download`로 HTTPS·릴리스 페이지·설치 파일 크기·SHA-256을 검사하고, 별도 Tailscale 클라이언트에서도 접속과 다운로드를 확인한다.
6. 게시 주소·버전·검증 결과를 확인한 뒤에만 릴리스 완료로 보고한다. 빌드나 게시가 실패하면 완료로 처리하지 않고 원인과 미완료 단계를 기록한다.

서버 게시 예시(버전·파일 경로는 실제 빌드 결과로 바꾼다):

```powershell
$baseUrl = 'https://win-7sv7uak6k5b.horse-solfege.ts.net'
& C:\CViewUpdates\tools\python\python.exe -I -B C:\CViewUpdates\tools\publish.py --root C:\CViewUpdates\site --base-url $baseUrl --version 0.1.21 --artifact C:\Releases\CView_0.1.21_arm64.dmg --artifact C:\Releases\CView_0.1.21_x64-setup.exe --notes-file C:\Releases\notes.txt
& C:\CViewUpdates\tools\python\python.exe -I -B C:\CViewUpdates\tools\check_server.py --base-url $baseUrl --download
```

**`scripts/release-macos.sh --dmg`는 위 게시·검증까지 자동 수행한다.** `cargo tauri build`나 개발용 빌드에는 게시를 연결하지 않는다. 자동 게시가 실패하면 같은 버전·파일·설명으로 `scripts/publish-update.py`를 재실행하며, 다운로드 검증에 성공하기 전에는 릴리스 완료로 처리하지 않는다.
