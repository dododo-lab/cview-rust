# CView 전체 코드 리뷰 (2026-10-28, v0.1.40 기준)

대상: `main` 2527c43 (v0.1.40). Rust 8개 크레이트 약 14만 줄, `app-ui/js` 약 1.4만 줄, CSS 약 3.8천 줄,
플레이어 페이지 `app-ui/player/player.html` 약 6.3천 줄.

## 1. 방법과 범위

- **기계 검사(전체)**: `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`(기준선 통과),
  추가 린트 묶음(`redundant_clone`, `map_unwrap_or`, `or_fun_call`, `manual_let_else`, `unused_async`,
  `needless_pass_by_value`, `redundant_closure_for_method_calls` 등), 테스트 밖 `unwrap()/expect()` 전수 조사,
  `unsafe`·`#[allow]` 목록, JS `node --check` 와 `innerHTML` 보간 이스케이프 조사.
- **정독(위험·규모 순)**: 멀티라이브 매니저 전체, 라이브 플레이어 뷰모델, `app.js`, IPC 명령 중 외부 프로세스·경로를
  다루는 것(`commands/content.rs`), 업데이트 설치기(`updater.rs`), 이미지 프로토콜(`image_service.rs`),
  로컬 HLS 프록시 서버(`proxy/server.rs`), 비밀 저장소(`secret_store.rs`), VOD 보관함 대기열.
  나머지 큰 파일(VOD·팔로잉·클립·검색 뷰모델, 채팅 엔진, API 클라이언트)은 모듈로 나누면서 구조 위주로 읽었다.
- **읽지 않은 것**: `player.html` 의 재생 상태 기계 세부(6.3천 줄), CSS 세부, 테스트 코드 내용.
  실제 앱 동작 확인(맥에서 실행)은 이 리뷰에 포함하지 않는다 — 마지막에 한 번 따로 한다.

## 2. 총평

전체적으로 상태가 좋다.

- 기준선에서 fmt·clippy(`-D warnings`)가 깨끗하고 테스트 1,883개가 모두 통과한다.
- 테스트 밖 `unwrap()/expect()` 가 28곳뿐이고, 대부분 정적 URL·정규식 파싱이나 테스트 대역(`deps.rs` 의 기록용 구현)이다.
  `unsafe` 는 `statvfs`(디스크 여유)와 Dock 아이콘 두 곳뿐이고 둘 다 범위가 좁다.
- 잠금은 대부분 `lock().unwrap_or_else(|e| e.into_inner())` 로 독(poison)을 견디고, `await` 를 넘겨 쥐지 않는다는
  원칙이 문서와 코드에 일관되게 지켜진다. 비동기 작업은 세대(generation) 번호와 `TaskSlot` 으로 늦게 끝난 결과를 버린다.
- 보안 경계가 의식적으로 설계돼 있다: `open_external` 은 http(s)만·셸을 거치지 않는 처리기로, `open_path` 는 일반 파일 +
  확장자 허용 목록으로, 이미지 프로토콜은 https + 허용 호스트 + 리다이렉트 hop 마다 같은 정책으로, 업데이트는
  SHA-256 + (macOS) 팀 ID·번들 ID 서명 요구 사항 + 공증 확인 뒤에만 교체한다.
- JS 는 ES 모듈로 나뉘어 있고, 화면에 넣는 API 문자열은 `esc()` 나 `textContent` 를 거친다(보간 전수 조사에서
  이스케이프 누락을 찾지 못했다).

가장 큰 약점은 **파일 크기**였다. 2천 줄이 넘는 Rust 파일이 셋, 1.2천 줄이 넘는 파일이 여섯 있었고, 대부분 Swift 원본의
`+확장` 파일 여러 개를 한 파일에 합친 모양이었다. 이번 작업에서 이를 Swift 확장 단위와 같은 하위 모듈로 나눴다(4절).

## 3. 발견 사항

심각도: **높음**(데이터 손실·보안·크래시 가능) / **중간**(오동작·자원 누수) / **낮음**(일관성·유지보수).
"고침" 은 이번 PR 에 들어간 것, "제안" 은 남겨 둔 것이다.

| # | 심각도 | 위치 | 내용 | 상태 |
|---|---|---|---|---|
| F1 | 중간 | `cview-app/src/commands/content.rs` | `reveal_path`·`open_path`·`open_notification_settings`·`open_external` 이 `spawn()` 한 `Child` 를 그냥 버려, macOS/Linux 에서 끝난 `open` 프로세스가 앱이 끝날 때까지 좀비로 남았다(누를 때마다 하나씩). | 고침 — `launch()` 가 따로 `wait()` 로 거둔다 |
| F2 | 낮음 | `viewmodels/vod_library.rs` `is_current` | 같은 대기열의 다른 메서드는 독을 견디는데 이것만 `lock().unwrap()` 이라, 앞선 작업이 패닉하면 이 경로만 연쇄 패닉했다. | 고침 — `guard()` 하나로 |
| F3 | 낮음 | 멀티라이브 매니저 `schedule_session_start`/`retry`/`schedule_menu_session_lifecycle` | 세대 증가·앞선 작업 취소 코드가 세 벌 복사돼 있었다(한 곳만 고치면 어긋날 위험). | 고침 — `next_generation` + `StartupJob` |
| F4 | 낮음 | 매니저 `clear_stage_handoff_warm` | `live_zap` 계측 로그가 `complete_watch_zap` 과 같은 코드로 한 벌 더 있었다. | 고침 — 그것을 부른다 |
| F5 | 낮음 | `app.js` / `update-check.js` | "버전 (빌드 N)" 문구 함수가 두 벌. `app.js` 가 이미 정적으로 불러온 `chat-prefs.js` 를 메뉴에서 다시 동적 `import()`. | 고침 |
| F6 | 낮음 | 전역 | `map(..).unwrap_or(..)` 류 88곳, 불필요한 `clone` 5곳, 즉시 평가되는 `unwrap_or(구조체)` 11곳 등. | 고침 — 66개 파일 기계적 정리 |
| F7 | 중간 | `cview-app/src/image_service.rs` | 이미지 응답 크기 상한이 없다. 허용 호스트의 응답이 비정상적으로 크면 그대로 메모리에 올리고(핫 캐시 48MB 상한은 넣은 뒤에 적용) 디스크에 쓴다. | 제안 — 본문 상한(예: 8MB)과 `Content-Length` 선검사 |
| F8 | 낮음 | 같은 파일 `ALLOWED_HOST_SUFFIXES` | `.akamaized.net` 은 Akamai 의 공용 도메인이라 다른 회사 호스트도 허용된다. 쿠키·인증 없이 이미지 형식만 받으므로 피해는 작다. `image/svg+xml` 도 통과한다(`<img>` 안에서는 스크립트가 돌지 않음). | 제안 — 실제 쓰는 호스트로 좁히고 SVG 제외 검토 |
| F9 | 낮음 | `cview-player/src/proxy/server.rs` `origin_allowed` | `Origin` 이 없는 요청을 받는다. 브라우저의 no-cors GET(`<img>`·`<video>`)은 `Origin` 을 보내지 않으므로, 사용자가 연 다른 웹페이지가 루프백 포트를 알면 프록시에 세그먼트 요청을 시킬 수 있다(응답은 읽지 못함, 업스트림은 대상 호스트로 고정). | 제안 — 경로에 세션 토큰을 넣거나 `Host` 검사 추가 |
| F10 | 낮음 | `cview-app/src/updater.rs` (Windows) | Windows 설치 파일은 코드 서명이 없어 HTTPS 피드의 SHA-256 만으로 검증한다. 피드 서버가 뚫리면 설치 파일과 해시를 함께 바꿀 수 있다. 이미 문서화된 한계다. | 제안 — Authenticode 서명 또는 피드 서명(minisign 등) |
| F11 | 낮음 | `cview-auth/src/secret_store.rs` `with_map` | 키체인 읽기 오류를 캐시에 `Err` 로 남겨 앱을 다시 켤 때까지 다시 시도하지 않는다. 반복 키체인 프롬프트를 피하려는 의도로 보이나 주석이 없다. | 제안 — 의도를 주석으로, 또는 일정 시간 뒤 재시도 |
| F12 | 낮음 | 7곳 | `await` 가 없는 `async fn`(`ChzzkApiClient::invalidate_authentication_cache`, `StreamCoordinator::set_player_engine`·`stop_stream`·`pause`·`resume`, `AppState::sync_authentication` 등). 호출자는 불필요하게 `.await` 한다. | 제안 — 공개 API 라 한 번에 바꿀 때 |
| F13 | 낮음 | `app.js` 의 ⌥← 뒤로 가기 | 입력 중 판정이 `INPUT`/`TEXTAREA` 만 보고, `dom.js keyBelongsToTarget` 은 `SELECT`·`contentEditable` 도 본다. 선택 상자에서 ⌥← 를 누르면 뒤로 간다. | 제안 — 같은 판정 함수를 쓴다 |
| F14 | 낮음 | `stats.js` / `insight-panel.js` / `following-info.js` | `durationText`·`bytesText`·`agoText` 가 파일마다 따로 있고 결과도 조금씩 다르다("1시간" vs "1시간 0분", "KB" 반올림 등). 의도된 차이인지 알 수 없다. | 제안 — `format.js` 로 모으고 변형은 이름으로 구분 |

## 4. 이번에 한 모듈화 (동작 변경 없음)

옮긴 코드가 원본과 같은지 매번 기계적으로 비교했다(옮긴 파일들의 코드 줄 다중집합이 원본과 같은지 — `pub(super)` 추가와
rustfmt 줄바꿈만 달라야 통과). 형제 모듈에서 부르는 비공개 메서드만 `pub(super)` 로 넓혔다.

| 파일 | 전 | 후 | 새 하위 모듈 |
|---|---:|---:|---|
| `viewmodels/multi_live/manager/mod.rs` | 2,398 | 697 | lifecycle · scheduling · stage · menu_surfaces · decode_budget · zap · workspace_host |
| `viewmodels/player/mod.rs` | 2,319 | 1,049 | stream · events · rewind · recording · screenshot · adjust |
| `viewmodels/vod_browse.rs` | 2,168 | 731 | library · search · feed · sync · visibility |
| `viewmodels/following.rs` | 1,668 | 916 | watch_prep · dock · hub_presets · origin |
| `viewmodels/clip.rs` | 1,554 | 916 | player(재수출로 경로 유지) · trending · channel · library |
| `viewmodels/search.rs` | 1,414 | 941 | results · fetch |
| `cview-chat/src/engine/mod.rs` | 1,285 | 722 | connection · listeners · messages · reconnect |
| `cview-networking/src/client.rs` | 1,202 | 630 | request · shared_get |
| `app-ui/js/app.js` | 942 | 845 | `app-sheets.js` (단축키·앱 정보·업데이트 확인 시트) |

또 300줄이 넘는 인라인 `#[cfg(test)] mod tests { .. }` 10개를 같은 이름 디렉터리의 `tests.rs` 로 옮겼다
(live_detail_refresh · app_state · bandwidth_coordinator · following_store · hlsjs bridge · refresh_coordinator ·
live_catalog_store · image_service · manifest parser · live_catalog_promotion).

## 5. 남은 제안 (이번 PR 밖)

1. **`player.html` 의 6.3천 줄 인라인 스크립트** — 가장 큰 단일 파일이다. 구획 주석(상태 · Stall Recovery · 프로파일 ·
   네이티브 브리지 · 소스 로드 · 재생 제어 · 선명한 화면 · WebGPU · matchWeb v2 · 메트릭 · 정리)이 이미 있어
   `player/*.js` 고전 스크립트로 나누기 쉽다. 다만 ① 최상위에서 다른 구획 함수를 바로 부르는 곳이 있으면 로드 순서가
   중요하고, ② `cview-player/src/policy/latency_pairing.rs` 테스트가 `include_str!` 로 이 HTML 을 읽고,
   ③ 재생 회귀는 실제 스트림이 있어야 확인된다. 그래서 실제 앱 검사와 함께 따로 하는 것이 맞다.
2. **`app.css`(2,372줄)** — 사이드바·카드·시청 화면·설정 등 화면별 파일로 나누되 `index.html` 의 `<link>` 순서로
   지금 캐스케이드 순서를 그대로 지킨다.
3. **`MultiLivePage` 생성자(약 225줄)** — 이벤트 바인딩 묶음을 `bind*()` 메서드로 쪼갠다.
4. **`ManagerState` 필드 60여 개** — 스테이지(`stage_*`)·메뉴(`menu_*`)·디코드(`decode_*`)·대역폭(`bandwidth_*`)
   필드를 하위 구조체로 묶으면 `MultiLiveManager::new` 의 긴 초기화가 `Default` 로 줄고, 모듈 경계도 타입으로 드러난다.
5. `redundant_closure_for_method_calls`(193곳, `.map(|s| s.to_string())` → `.map(ToString::to_string)`) 는
   가독성 의견이 갈려 손대지 않았다. 팀 규칙으로 정하면 `cargo clippy --fix` 로 한 번에 바꿀 수 있다.
6. F7–F14 의 제안 항목.

## 6. 검증

- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` 통과.
- `cargo test --workspace`: 1,883개 통과(기준선과 같은 수).
- `node --check` : `app-ui/js/*.js`, `tools/ui-regression/*.mjs` 전부 통과.
- Playwright(모의 IPC) 회귀: `ml-focus-info.mjs` 6/6, `non-live-menus.mjs` 전부 ok·페이지 오류 0,
  새 `app-sheets.mjs` 6/6.
- 실제 앱에서의 기능 확인은 아직 하지 않았다(맥에서 마지막에 한 번).
