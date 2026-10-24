# CView v2 — Rust 포팅

Swift 6 macOS 앱(원본 저장소 `dododo-lab/cview-v2` 의 `Sources/`)을 Rust 워크스페이스로 옮기는 작업. 계획과 단계는 [`PORTING_PLAN.md`](PORTING_PLAN.md).

## 데스크톱 앱 실행 (Tauri 2)

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

명령은 저장소 루트(`Cargo.toml` 위치)에서 실행한다. Rust 버전과 개발 도구는
`rust-toolchain.toml` 에 고정되어 있다. npm 설치나 프런트엔드 번들링은 필요하지 않다.

Linux(Debian/Ubuntu)에서는 데스크톱 개발 패키지가 필요하다:

```sh
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libssl-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf
```

GUI 실행에는 X11/Wayland 디스플레이와 D-Bus 세션이 필요하다. 디스플레이가 없는 Linux 에서는
`xvfb`·`xauth` 를 설치하고 `dbus-run-session -- xvfb-run -a cargo run -p cview-app --locked`
로 기동할 수 있다. 실제 로그인·라이브 재생·채팅은 계정과 CHZZK/Naver 네트워크 접근이 필요하다.

로컬 Mac에서는 다음 명령으로 개발 도구를 준비하고 인증서 없이 테스트용 DMG를 만든다:

```sh
./scripts/build-local-macos.sh
```

결과는 `dist/CView_0.1.0_<아키텍처>_local.dmg`에 생성된다.
처음 개발 도구를 설치하는 Mac은 `xcode-select --install`을 먼저 실행한다.
macOS DMG 릴리즈 빌드·서명·공증 명령과 필요한 환경은 [릴리즈 안내](RELEASE.md)에 정리했다.

Windows Server에서 운영하는 자체 HTTPS 업데이트 서버는
[IIS 설치·릴리스 게시 안내](deploy/update-server/README.md)를 참고한다.
앱 빌드 시 `CVIEW_UPDATE_URL`로 해당 서버 주소를 지정할 수 있다.

## 검사

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build -p cview-app --locked
```

UI 의 모듈 연결·인라인 스크립트와 화면 전환 회귀 테스트는 Node.js 24 로 검사한다.
외부 npm 패키지는 사용하지 않는다:

```sh
node --experimental-vm-modules scripts/check-ui.mjs
node --test app-ui/tests/*.test.mjs
```

디자인·반응형·자동 접근성·키보드 회귀 검사는 별도 개발 도구로 실행한다.
테스트 의존성은 앱에 포함되는 `app-ui` 밖의 `ui-tests`에 설치한다.
합성 데이터를 사용하는 검사이므로 실제 로그인·방송 재생 검증과는 구분한다.

```sh
npm --prefix ui-tests ci
cd ui-tests
npx playwright install --with-deps chromium
npm test
```

저장소 루트에서 이미 설치된 Chromium을 사용하려면 브라우저 설치를 생략하고
`PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH=/usr/bin/chromium npm --prefix ui-tests test`
로 실행할 수 있다. 화면 캡처와 검사 결과는 `ui-tests/results/`에 생성된다.
검토 범위와 개선 내용, 실제 앱 검증의 한계는 [디자인 검증 기록](DESIGN_REVIEW.md)에 정리했다.
