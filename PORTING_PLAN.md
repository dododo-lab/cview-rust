# CView v2 → Rust 포팅 계획

원본: `Sources/` 아래 Swift 6 / SwiftUI macOS 앱 (약 19만 줄, 8개 모듈, 테스트 581개 파일).
대상: 이 저장소의 `rust/` Cargo 워크스페이스 (브랜치 `rust-port`).

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
