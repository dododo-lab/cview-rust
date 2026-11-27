# 배포 절차

배포용 릴리스는 저장소 루트에서 한 명령으로 만든다. 개별 빌드 명령(`cargo tauri build` 등)이나 `dist/` 생성만으로는 릴리스가 끝난 것이 아니다.

```sh
scripts/release-macos.sh --dmg
```

포함할 PR이 있거나 작업 트리에 커밋 안 된 작업이 있으면 래퍼를 쓴다. 열린 PR을 squash 병합하고, 작업을 잠시 보관한 뒤 `main`을 갱신해 위 명령을 실행하고, 끝나면 보관한 작업을 되돌린다.

```sh
scripts/release-with-prs.sh [PR번호 ...] [-- release-macos.sh 옵션]
```

`release-macos.sh --dmg`가 하는 일:

1. 빠른 검사(fmt·clippy·JS 문법)와 버전·macOS 빌드 번호 증가. 전체 테스트는 Windows 서버 빌드를 시작한 뒤 겹쳐 돌리며, 실패하면 번호를 되돌린다
2. macOS DMG(Apple Silicon) — Developer ID 서명, Apple 공증, 티켓 첨부·검증
3. Windows x64·ARM64 NSIS 설치 파일 — 같은 버전의 소스를 서버에 보내 MSVC 타깃으로 빌드하고, Mac에서 SHA-256·아키텍처·DLL 의존성 검증, Ubuntu 빌드 서버면 Parallels Windows ARM64에서 x64 테스트와 두 설치 파일의 설치·WebView2 시작 확인(미서명)
4. 업데이트 서버 업로드·게시(`latest.json`과 `release-catalog.json`을 마지막에 원자적으로 갱신)
5. 서버와 Mac 양쪽에서 전체 다운로드·SHA-256 검증
6. 릴리스 커밋·`v<버전>` 태그 푸시
7. GitHub 릴리스 — 같은 설치 파일 세 개, `CView_<버전>_SHA256SUMS.txt`, 앱 자동 업데이트용 서명 피드 `latest.json`을 `v<버전>` 릴리스에 올리고 다시 받아 확인(`scripts/github-release.py`), 게시 후 Actions `github-release`가 업데이트 서버의 서명된 manifest와 재대조

Windows 빌드는 버전을 올린 직후(macOS 앱·DMG 빌드 전) 백그라운드로 시작해 macOS 빌드·서명 및 공증 대기와 겹친다. 버전을 올리기 전에 서버 연결과 도구를 점검하며,
서버 빌드나 두 파일의 검증이 실패하면 게시를 중단하고 버전 증가를 되돌린다.
접속 설정은 저장소 밖 `~/.config/cview-build/`에 둔다([빌드 서버 설정](../tools/build-server/README.md)).
Windows 전용 작업이므로 Mac 자동 작업자 실행 여부와 관계없이 정식 릴리스 스크립트가 처리한다.
Mac에는 검사에 쓰는 `7zz`·`llvm-readobj`가 필요하며 Windows용 MinGW·NSIS·Rust 타깃은 필요하지 않다.
서버 작업 ID·소스 SHA-256·두 설치 파일의 해시 기록은 `dist/CView_<버전>_windows-build.json`, 로그는 `<target>/windows-build-<버전>.log`에 남는다.

macOS와 Windows(x64·arm64) 설치 파일은 늘 같은 버전으로 함께 게시한다. 사용자에게 보일 변경 사항은 UTF-8 파일로 써서 `CVIEW_RELEASE_NOTES_FILE=/절대/경로/notes.txt`로 넘긴다(생략하면 Git 변경 목록을 쓴다). 배포 후 [`CHANGELOG.md`](../CHANGELOG.md)에 항목을 더한다.

게시가 실패하면 버전을 다시 올리지 않고 보존된 파일로 재게시한다.

```sh
python3 scripts/publish-update.py --version <버전> --notes-file dist/CView_<버전>_notes.txt
```

GitHub 릴리스 사본만 실패했으면(업데이트 서버 게시·푸시는 끝남) 같은 버전으로 재실행한다. 이미 올라간 파일은 덮어쓰지 않고, `dist/`에 파일이 없으면 업데이트 서버에서 받아 해시를 확인해 쓴다.

```sh
python3 scripts/github-release.py --version <버전>
```

## 더 자세한 안내

- AI 작업자용 규칙: [`AGENTS.md`](../AGENTS.md)
- 자동 게시·재시도·장애 복구·다른 PC 등록: [`tools/windows-update-server/AUTOMATION.md`](../tools/windows-update-server/AUTOMATION.md)
- 업데이트 서버 구성·수동 게시: [`tools/windows-update-server/README.md`](../tools/windows-update-server/README.md)

## 부록: 업데이트 서버 게시 규칙

**배포용 릴리스 빌드는 매번 업데이트 서버 게시와 검증까지 완료해야 한다.** 설치 파일을 로컬 `dist/`에 만들기만 한 상태는 릴리스 완료로 보지 않는다. 이 규칙은 macOS와 Windows 모두에 적용한다. 개발용 실행·디버그 빌드는 게시 대상이 아니다.

- 업데이트 서버(공개 다운로드·릴리스 페이지): https://cv.dododo.app/
- 최신 릴리스 피드: https://cv.dododo.app/releases/latest.json
- 수동 업로드·원격 게시 방법: [업로드·게시 안내](../tools/windows-update-server/README.md), [원격 PowerShell](../tools/windows-update-server/POWERSHELL-REMOTE.md)
- 버전별 설치 파일과 `notes.txt`를 업로드한 뒤 서버의 관리자 PowerShell에서 `C:\CViewUpdates\tools\Publish-UploadedRelease.ps1 -Version <버전>`으로 게시·다운로드 검증을 실행할 수 있다.
- 앱은 GitHub 릴리스 피드(`https://github.com/dododo-lab/cview-rust/releases/latest/download/latest.json`)로 업데이트를 확인하고 받으며, 받지 못하면 cv.dododo.app 피드를 쓴다. 이전 버전 앱은 cv.dododo.app만 보므로 서버 게시를 계속한다. Tailscale은 게시(SSH 업로드)에만 필요하다.

릴리스마다 다음 순서를 반드시 따른다.

1. 기존 게시 버전보다 높은 새 버전으로 Cargo 작업공간·Tauri 버전과 macOS 빌드 번호를 맞추고 검사·테스트를 통과시킨다. 기존 게시 버전의 파일을 덮어쓰지 않는다.
2. **macOS와 Windows 모두** 릴리스 설치 파일을 빌드한다. 앱에는 GitHub 릴리스 피드가 기본값, 위 최신 릴리스 피드가 대체 피드로 들어 있으며, 다른 서버 하나만 쓸 때만 빌드 환경 변수 `CVIEW_UPDATE_URL`로 바꾼다(실행 시 같은 이름의 환경 변수가 있으면 그 값이 우선한다). macOS는 Developer ID 서명·Apple 공증·티켓 첨부 및 검증을 완료한다. Windows는 코드 서명 인증서가 없어 미서명으로 배포하므로 다운로드 페이지의 릴리스 설명에 이를 명시한다.
3. 같은 버전의 완성된 macOS·Windows 설치 파일과 릴리스 설명을 서버에 준비한다. `_local` 빌드, 소스 압축 파일, 테스트용 가짜 설치 파일은 게시하지 않는다.
4. 서버의 관리자 PowerShell에서 `publish.py`로 게시한다. 여러 설치 파일은 `--artifact`를 반복해 한 릴리스에 함께 등록한다. 버전 폴더와 설치 파일을 먼저 배치하고 `latest.json`을 마지막에 원자적으로 갱신하는 게시 도구를 사용한다. 서버에서 직접 `publish.py`(또는 `Publish-UploadedRelease.ps1`)를 실행하면 `latest.json`에 피드 서명이 들어가지 않아 앱이 자동 설치 대신 다운로드 단추만 보이고 `scripts/github-release.py`의 서명 확인이 실패하므로, 서명은 `scripts/publish-update.py` 경로로 넣는다.
5. `check_server.py --download`로 HTTPS·릴리스 페이지·설치 파일 크기·SHA-256을 검사하고, Tailscale을 끈 외부 네트워크(휴대폰 데이터 등)에서도 접속과 다운로드를 확인한다. 릴리스 Mac은 서버와 같은 공유기 안에 있어 Mac의 검증만으로는 외부 공개 여부까지 확인되지 않는다([공개 다운로드 경로](../tools/windows-update-server/AUTOMATION.md#공개-다운로드-경로)).
6. 게시 주소·버전·검증 결과를 확인한 뒤에만 릴리스 완료로 보고한다. 빌드나 게시가 실패하면 완료로 처리하지 않고 원인과 미완료 단계를 기록한다.

서버 게시 예시(버전·파일 경로는 실제 빌드 결과로 바꾼다):

```powershell
$baseUrl = 'https://cv.dododo.app'
& C:\CViewUpdates\tools\python\python.exe -I -B C:\CViewUpdates\tools\publish.py --root C:\CViewUpdates\site --base-url $baseUrl --version 0.1.21 --artifact C:\Releases\CView_0.1.21_arm64.dmg --artifact C:\Releases\CView_0.1.21_x64-setup.exe --artifact C:\Releases\CView_0.1.21_arm64-setup.exe --notes-file C:\Releases\notes.txt
& C:\CViewUpdates\tools\python\python.exe -I -B C:\CViewUpdates\tools\check_server.py --base-url $baseUrl --download
```

**`scripts/release-macos.sh --dmg`는 위 게시·검증까지 자동 수행한다.** `cargo tauri build`나 개발용 빌드에는 게시를 연결하지 않는다. 자동 게시가 실패하면 같은 버전·파일·설명으로 `scripts/publish-update.py`를 재실행하며, 다운로드 검증에 성공하기 전에는 릴리스 완료로 처리하지 않는다.
