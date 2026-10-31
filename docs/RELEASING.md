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

1. 검사(fmt·clippy·테스트·JS 문법)와 버전·macOS 빌드 번호 증가
2. macOS DMG(Apple Silicon) — Developer ID 서명, Apple 공증, 티켓 첨부·검증
3. Windows x64 NSIS 설치 파일 — 같은 Mac에서 교차 빌드(미서명)
4. 업데이트 서버 업로드·게시(`latest.json`은 마지막에 원자적으로 갱신)
5. 서버와 Mac 양쪽에서 전체 다운로드·SHA-256 검증
6. 릴리스 커밋·`v<버전>` 태그 푸시

macOS와 Windows 설치 파일은 늘 같은 버전으로 함께 게시한다. 사용자에게 보일 변경 사항은 UTF-8 파일로 써서 `CVIEW_RELEASE_NOTES_FILE=/절대/경로/notes.txt`로 넘긴다(생략하면 Git 변경 목록을 쓴다). 배포 후 [`CHANGELOG.md`](../CHANGELOG.md)에 항목을 더한다.

게시가 실패하면 버전을 다시 올리지 않고 보존된 파일로 재게시한다.

```sh
python3 scripts/publish-update.py --version <버전> --notes-file dist/CView_<버전>_notes.txt
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
$baseUrl = 'https://cv.dododo.app'
& C:\CViewUpdates\tools\python\python.exe -I -B C:\CViewUpdates\tools\publish.py --root C:\CViewUpdates\site --base-url $baseUrl --version 0.1.21 --artifact C:\Releases\CView_0.1.21_arm64.dmg --artifact C:\Releases\CView_0.1.21_x64-setup.exe --notes-file C:\Releases\notes.txt
& C:\CViewUpdates\tools\python\python.exe -I -B C:\CViewUpdates\tools\check_server.py --base-url $baseUrl --download
```

**`scripts/release-macos.sh --dmg`는 위 게시·검증까지 자동 수행한다.** `cargo tauri build`나 개발용 빌드에는 게시를 연결하지 않는다. 자동 게시가 실패하면 같은 버전·파일·설명으로 `scripts/publish-update.py`를 재실행하며, 다운로드 검증에 성공하기 전에는 릴리스 완료로 처리하지 않는다.
