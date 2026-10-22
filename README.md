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

macOS 배포는 `scripts/release-macos.sh` 가 위 검사·버전 올리기·서명·공증까지 한 번에 한다(버전 파일·태그·원격을 바꾼다).
