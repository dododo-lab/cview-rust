# CView v2 → Rust 포팅 계획

원본: `dododo-lab/cview-v2` 저장소 `Sources/` 아래 Swift 6 / SwiftUI macOS 앱 (약 19만 줄, 8개 모듈, 테스트 581개 파일).
대상: 이 저장소(`dododo-lab/cview-rust`)의 Cargo 워크스페이스. 2026-10-20 에 `cview-v2` 의 `rust/`(브랜치 `rust-port`)에서 히스토리째 분리했다.

## 원칙

1. **모듈 경계 유지** — SwiftPM 타깃 하나를 Rust 크레이트 하나로 옮긴다. 의존 방향도 그대로 둔다.
2. **아래에서 위로** — UI 없이 검증 가능한 순수 로직(모델·프로토콜·파서·정책)부터 옮기고, 각 단계마다 원본 XCTest 를 Rust 테스트로 옮겨 동작을 고정한다.
3. **플랫폼 종속 코드는 추상화 뒤로** — Keychain, IOKit, AVFoundation, SwiftData, SwiftUI 는 trait 으로 경계를 만들고 크로스플랫폼 구현(`keyring`, SQLite, WebView)으로 대체한다.
4. **런타임** — Swift Concurrency(actor/async) → `tokio` + `async`/`await`. actor 는 `Arc<Mutex<_>>` 또는 메시지 채널 태스크로.

## 크레이트 매핑

| Swift 타깃 | Rust 크레이트 | 핵심 대체 기술 |
|---|---|---|
| `CViewCore` | `cview-core` | `serde`, `thiserror`, `chrono` |
| `CViewNetworking` | `cview-networking` | `reqwest` (rustls), `serde_json` |
| `CViewAuth` | `cview-auth` | `keyring`, `sha2`/`base64` (PKCE) |
| `CViewChat` | `cview-chat` | `tokio-tungstenite` |
| `CViewPersistence` | `cview-persistence` | `rusqlite` (SwiftData 대체) |
| `CViewPlayer` | `cview-player` | `hyper` 로컬 프록시, m3u8 파서; 재생은 hls.js 재사용 |
| `CViewUI` + `CViewApp` | `cview-app` | Tauri 2 (WebView + hls.js) |
| `CViewWidgets`, Intents, PiP | 범위 밖 | macOS 전용 확장 — 추후 판단 |

UI 를 Tauri 로 잡은 이유: 라이브 재생 엔진이 이미 WebView 안의 hls.js(`Sources/CViewPlayer/Resources/hlsjs-player.html`)이므로, 같은 HTML/JS 를 그대로 쓰고 Rust 쪽은 프록시·지연 제어·채팅·API 를 맡으면 된다.

## 단계

| 단계 | 내용 | 완료 기준 |
|---|---|---|
| 0 | 워크스페이스 골격, CI(`cargo test`) | `cargo build` 통과 |
| 1 | `cview-core`: 도메인 모델(Live/Channel/Chat/VOD/Clip/Settings), `AppError`, 순수 유틸(EWMA, PID, Levenshtein, 검색 점수, 날짜 파서 등), 프로토콜 → trait | 대응 Core 테스트 이식·통과 |
| 2 | `cview-networking`: 엔드포인트, `ChzzkApiClient`, 응답 검증·캐시 | 픽스처 기반 디코딩 테스트 통과 |
| 3 | `cview-chat`: 웹소켓 프로토콜, 메시지 파서, 재연결 정책, `ChatEngine` | Chat 테스트 이식·통과 |
| 4 | `cview-auth`: 쿠키, PKCE, OAuth, 토큰 저장 | Auth 테스트 이식·통과 |
| 5 | `cview-persistence`: SQLite 스키마(북마크, 시청 기록, 설정 등) | Persistence 테스트 이식·통과 |
| 6 | `cview-player`: HLS 매니페스트 파서, 로컬 스트림 프록시, 저지연 컨트롤러, 재연결 핸들러 | Player 테스트 이식·통과 |
| 7 | `cview-app`: Tauri 셸, 화면(홈/라이브/채팅/설정) 순차 이식 | 라이브 시청 + 채팅 E2E 동작 |

각 단계는 별도 커밋으로 남기고, 단계가 끝날 때마다 PR 에 진행 상황을 갱신한다.

## 범위에서 제외하거나 미루는 것

- SwiftUI 디자인 시스템(`DesignSystem/`, Liquid Glass), 성능 계측(signpost, RenderBudget) — UI 단계에서 웹 기술로 재설계.
- `AVPlayerEngine`(VOD) — 단계 7 에서 WebView `<video>` 로 대체.
- 위젯, App Intents, PiP, 공증(notarization) 스크립트.

## 진행 현황 (2026-10, `rust-final` 브랜치)

> **이식 시점(2026-10-20) 기록이다.** 아래 "남은 일"·테스트 수는 그 뒤 바뀌었다. 2026-10-23(v0.1.14) 트리 기준으로
> 그사이 생긴 것: 채널 상세·VOD·클립 화면(`app-ui/js/channel.js`, `commands/content.rs`), 멀티라이브 화면
> (`app-ui/js/multilive.js`, `commands/multi_live.rs`), 미디어 다운로더(`commands/download.rs`, `app-ui/js/downloads.js`),
> 녹화(`commands/recording.rs`), 파일 저장·토스트, 공증까지 하는 배포 스크립트(`scripts/release-macos.sh`).
> 인증서 피닝은 아직 미이식이다(`crates/cview-networking/src/transport.rs` 의 `TODO(cert-pinning)`).
> 테스트 수는 1,770 → 1,840(2026-10-23 전체 실행). 나머지 항목은 현재 코드로 다시 확인해야 한다.

### 단계별 상태

| 단계 | 상태 | 비고 |
|---|---|---|
| 0 골격 | 완료 | `cargo fmt --check` · `clippy -D warnings` · `cargo test --workspace` 통과 |
| 1 `cview-core` | 완료 | |
| 2 `cview-networking` | 완료 | |
| 3 `cview-chat` | 완료 | |
| 4 `cview-auth` | 완료 | 인증서 피닝 제외 |
| 5 `cview-persistence` | 완료 | |
| 6 `cview-player` | 완료 | 코디네이터·로컬 프록시·저지연 PID·재연결 |
| 6.5 `cview-app-core` | 완료 | AppState, 서비스, 뷰모델(홈·팔로잉·인기·검색·채널 상세·클립·채팅·멀티라이브·플레이어·VOD 탐색/재생/보관함) |
| 7 `cview-app` | 1차 완료 | Tauri 셸이 app-core 뷰모델을 사용: 재생 `PlayerViewModel`(저지연 제어 다시 켬), 채팅 `ChatViewModel`, 팔로잉 `FollowingStore`, 인기 `AllBroadcastsViewModel`, 검색 `SearchViewModel`. 화면은 홈/라이브/채팅/설정까지 |

### 크레이트별 테스트 수

| 크레이트 | 테스트 |
|---|---|
| `cview-core` | 369 |
| `cview-networking` | 71 |
| `cview-chat` | 156 |
| `cview-auth` | 110 |
| `cview-persistence` | 253 |
| `cview-player` | 283 |
| `cview-app-core` | 501 (lib 252 + 통합 249) |
| `cview-app` | 27 |
| **합계** | **1770** |

### 일부러 이식하지 않은 것 / 남은 일

- 플랫폼 확장: PiP, 위젯, App Intents, 공증 스크립트.
- 인증서 피닝, 미디어 다운로더(VOD 저장), 저장/열기 패널, 토스트.
- 로컬 타임시프트 녹화기 팩토리 — 설정값은 계산하지만 코디네이터에 녹화기를 등록하지 않는다(서버 타임머신만 동작).
- VOD 재생 엔진 — `VodPlayerEngine` 트레잇만 있고 AVPlayer 대체(WebView `<video>`) 구현과 셸 화면은 없다.
- 셸 화면: VOD·클립·채널 상세·멀티라이브·이모티콘 선택기·화질 선택기 UI 는 아직 없다(뷰모델은 이식됨).
- WebView 엔진의 이퀄라이저·비디오 보정·녹화·스크린샷 명령 연결, 매치웹 v2 시크 토큰 응답(기본 꺼짐).
- `HomeViewModel` 레거시 캐시 마이그레이션, `LiveCatalogStore` 영속화(+Persistence/+Checkpoint/promote 는 no-op), `WatchHeartbeatCenter`, 가림(occlusion) 상한 타이머.
- `PlayerViewModel` 의 `MultiLivePlayer` 구현(선택 사항), VOD 탐색 동기화 패스는 합류(join) 대신 직렬화로 단순화.
- 셸은 app-core `AppState` 의 인증 경계만 쓴다 — 워크스페이스 복원·저장소 복구 UI 는 미연결.

### 검증 범위

앱은 CHZZK 에 접근할 수 없는 샌드박스에서 `xvfb-run` 으로 헤드리스 기동만 확인했다(패닉 없음, 네트워크 오류는 예상대로 표시). 실제 로그인·라이브 재생(저지연 제어 포함)·채팅 송수신은 macOS 실기기에서 반드시 확인해야 한다.
