# macOS DMG 릴리즈

현재 버전은 `0.1.0`이며 최소 macOS 버전은 13.0이다.
DMG 생성은 macOS에서 실행해야 한다. Linux 크로스 컴파일만으로 Xcode SDK,
Developer ID 서명과 `hdiutil` 패키징을 대체할 수 없다.

## macOS에서 실행

Xcode Command Line Tools, Rust, Python 3와 Tauri CLI 2가 필요하다.
저장소의 `rust-toolchain.toml`과 `Cargo.lock`을 사용한다.
설정의 Developer ID Application 인증서와 개인 키가 해당 Mac의 키체인에 있어야 한다.
다른 인증서는 `APPLE_SIGNING_IDENTITY` 환경 변수로 지정한다.

```sh
xcode-select -p
cargo install tauri-cli --version '^2' --locked
rustup target add aarch64-apple-darwin
CVIEW_NOTARIZE_PROFILE=cview-notarize scripts/release-macos.sh --dmg --target aarch64-apple-darwin
```

`cview-notarize`는 미리 설정한 `notarytool` 키체인 프로파일 이름이다.
공증 자격 증명은 저장소에 넣지 않는다. 기존 스크립트의 App Store Connect API 키 또는
Apple ID 환경 변수 방식도 Tauri 번들러가 사용할 수 있다.
프로파일 방식은 최종 DMG의 공증 결과가 `Accepted`인지 확인한 뒤 티켓을 첨부·검증한다.
공증 설정 없이 실행하면 서명까지만 수행하며, 일반 배포 승인을 확인한 것으로 취급하면 안 된다.

기본 target은 실행하는 Mac의 아키텍처다. Intel용은 `--target x86_64-apple-darwin`,
공용 바이너리는 `--target universal-apple-darwin`을 사용한다.
Universal 빌드에는 `rustup target add aarch64-apple-darwin x86_64-apple-darwin`으로
양쪽 Rust target을 설치해야 한다.

산출물:

- `dist/CView_0.1.0_arm64.dmg` / `_x64.dmg` / `_universal.dmg`
- 동일 이름 뒤에 `.sha256`이 붙은 체크섬 파일
- `<target-dir>/<target>/release/bundle/macos/CView.app`

스크립트는 잠금 파일 고정 빌드, 앱 서명 검증, 이번 실행에서 생성된 DMG 선택,
디스크 이미지 검증, DMG 서명·검증과 SHA-256 생성을 수행한다.
최종 Gatekeeper 평가 결과도 확인해야 한다.

## GitHub Actions 빌드

`.github/workflows/release-macos.yml`은 macOS 러너에서 인증서 가져오기,
빌드·서명·DMG 공증, Gatekeeper 승인과 체크섬 검증 후 DMG 아티팩트를 업로드한다.
작업 종료 시 임시 키체인을 삭제한다. 공개 GitHub Release를 자동 게시하지는 않는다.
현재 변경 내용과 워크플로가 저장소에 반영되어야 해당 소스를 빌드한다.

필요한 저장소 Secrets:

- `APPLE_CERTIFICATE`: 개인 키를 포함한 Developer ID Application `.p12`의 한 줄 Base64 인코딩
- `APPLE_CERTIFICATE_PASSWORD`: 해당 `.p12` 비밀번호
- `APPLE_ID`, `APPLE_PASSWORD`: 공증 계정과 앱 전용 암호
- `APPLE_TEAM_ID`: Apple Developer 팀 ID

인증서 이름이 앱 설정과 다르면 저장소 Variable `APPLE_SIGNING_IDENTITY`도 지정한다.
GitHub API 접근과 저장소·Actions 권한이 있는 연결이 필요하다.
이 클라우드에서는 현재 해당 연결이 없어 워크플로를 원격 등록·실행하지 못했다.

## 이 클라우드에서 확인한 결과

- 실행 환경: Linux. Xcode·`security`·`codesign`·`hdiutil`을 제공하는 macOS 실행 환경이 없다.
- `gh workflow list --repo dododo-lab/cview-rust`는 GitHub API `Forbidden`으로 실패했다.
  원격 macOS 빌드 실행·릴리즈 업로드를 수행하지 못했다.
- `zsh scripts/release-macos.sh --dmg`는 macOS 필요 안내와 함께 종료했다.
  **실제 `.app`/DMG 릴리즈 산출물은 생성하지 않았다.**
- `zsh -n scripts/release-macos.sh` 문법 검사 통과.
- `python3 scripts/tests/test_release_macos.py -v`의 4개 테스트 통과:
  아키텍처별 출력·체크섬, 이전 DMG 거부, 공증 거부 시 실패, 잘못된 서명 시 실패.
  macOS 명령을 대체한 모의 테스트이며 Apple 서명·공증이나 실제 앱 실행을 검증한 것은 아니다.

실제 릴리즈 빌드를 마치려면 최신 작업 내용을 macOS 빌드 환경에서 위 명령으로 실행해야 한다.
