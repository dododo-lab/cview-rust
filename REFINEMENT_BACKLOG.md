# 고도화 백로그 (2026-10-20)

앱 전체 정밀 점검 결과. 화면·디자인/접근성, Rust 안정성·보안, 화면 JS 안정성·성능, Swift 대응·코드 품질
네 갈래로 코드를 읽어 찾았다(재현이 아니라 코드 읽기로 찾은 경합은 그렇게 적었다). 우선순위:
**P0** 보안·데이터 손실·잘못된 화면 이동 · **P1** 눈에 보이는 버그·UX 불일치 · **P2** 다듬기·견고성·성능 · **P3** 코드 품질.
상태: ⬜ 할 일 · ✅ 완료(커밋) · ⏭ 이번엔 안 함(이유).

## P0

| ID | 상태 | 항목 | 위치 | 고치는 법 |
|---|---|---|---|---|
| S-1 | ⬜ | `open_path` 가 웹뷰가 준 아무 경로나 연다(.app·스크립트 실행 가능) | `crates/cview-app/src/commands/content.rs:599-613` | 다운로드·녹화·스크린샷 폴더 안만 허용, 실행 대신 Finder/탐색기에서 보이기 |
| S-2 | ⬜ | 원격 채팅·이모티콘을 그리는 웹뷰에 CSP 없음(`csp: null`) | `tauri.conf.json:31` | 제한적 CSP(로컬 스크립트만, 이미지 https, 연결 ipc·127.0.0.1·https) |
| S-3 | ⬜ | Windows `open_external` 이 `cmd /C start` — URL 의 `&`·`|` 로 명령 주입 | `commands/content.rs:646-648` | `rundll32 url.dll,FileProtocolHandler` |
| J-1 | ⬜ | `closingForHandoff` 가 비동기 `viewer.close()` 뒤에 늦게 오는 onClose 를 못 막아 무대에서 튕김 | `app-ui/js/app.js:139,283`, `viewer.js:157-165` | `close()` 에 조용히 닫기 옵션·await |
| J-2 | ⬜ | `viewer.close()` 가 `open()` 과 경합 — 낡은 onClose 가 새 방송에서 뒤로 가기·playerStop | `viewer.js:157-165` | 시작 토큰을 잡아 바뀌었으면 onClose 생략 |
| J-3 | ⬜ | 채팅 `connect()/disconnect()` 요청 토큰 없음 — 빠른 패인 전환 때 입력창 비활성·엉뚱한 상태 | `chat.js:263-306` | 연결 순번(connSeq) 확인 |
| D-1 | ⬜ | 최근 시청 기록 "지우기" 한 번에 전부 삭제(확인 없음) | `library.js:178` | 두 번 누르기(armed) 패턴 |
| D-2 | ⬜ | 북마크 삭제 즉시(되돌리기 없음) | `library.js:181` | 토스트 "되돌리기" 뒤 삭제 |

## P1

| ID | 상태 | 항목 | 위치 | 고치는 법 |
|---|---|---|---|---|
| M-1 | ⬜ | 멀티라이브 패인이 첫 프레임 없이 영원히 "재생 중"(나나양) — 낡은 상태 렌더가 재사용 슬롯을 멈추게 할 수 있음, 첫 프레임 감시 없음 | `multilive.js:323-328`, `crates/cview-app/src/multi_live.rs:235,478`, `session.rs:529` | 상태 순번(rev)으로 낡은 렌더 무시 + 첫 프레임 감시(일정 시간 안 나오면 다시 연결) |
| M-2 | ⬜ | 패인 플레이어가 사용자 플레이어 설정을 안 받음(`player_settings: None`), `is_multi_live` 미설정 | `multi_live.rs:503-508`, `viewmodels/player/mod.rs:268` | 설정 전달·멀티 표시 |
| W-1 | ⬜ | Windows 에서 ⌃⌘ 조합·Ctrl+Tab 이 눌리지 않음(Ctrl→⌘ 바꿔 읽기 부작용) | `index.html:16-22`, `multichat.js:83`, `multilive.js:147`, `viewer-controls.js:376` | Ctrl+Tab 은 바꾸지 않고, ⌃⌘ 는 Windows 에서 Ctrl+Alt |
| W-2 | ⬜ | 단축키 안내가 Windows 에서도 ⌘ | `app.js:312,371-391`, `shortcuts.js:55` 외 10여 곳 | `kbd()` 헬퍼로 Windows 면 Ctrl+/Alt+/Shift+ |
| A-1 | ⬜ | 전역 `:focus-visible { outline: none }` 이 초점 표시를 지움 | `css/app.css:778` | 삭제(뒤쪽 허용 목록이 그린다) |
| A-2 | ⬜ | 아이콘 버튼에 접근 이름 없음(✕ 등), 카드 아바타는 키보드로 못 감 | `multichat.js:56`, `cards.js:21` 외 | aria-label, 버튼으로 |
| A-3 | ⬜ | 라이트 테마 낮은 대비 토큰(3차 글자 3.6:1 등) | `css/app.css:85-176,1673-1696` | 진하게 |
| E-1 | ⬜ | 실패 화면이 백엔드 오류 원문만 보여 주고 다시 시도 버튼 없음 | `categories.js:63`, `pages.js:63,129` 외 | `errorState(제목, 오류, 다시시도)` |
| J-4 | ⬜ | 클립 화면을 떠나도 클립 iframe 소리가 계속 | `media.js:146-156` | 떠날 때 iframe 비우기 |
| J-5 | ⬜ | 멀티채팅 같은 채널 두 번 추가(더블클릭·복원 경합) | `multichat.js:115-139` | 추가 중 집합 |
| J-6 | ⬜ | 멀티채팅 패널 닫아도 ChatPanel 리스너·구독 누수 | `multichat.js:141-150`, `chat.js:136-239` | `ChatPanel.destroy()` |
| J-7 | ⬜ | 채팅 전용 창 빠른 채널 전환 경합 | `chat-only.js:62-76` | 순번 |
| J-8 | ⬜ | 시청 화면이 숨어 있는 동안 플레이어 종료·오류 이벤트를 버림 | `viewer.js:200` | channelId 로 판단 |
| J-9 | ⬜ | 다시 연결이 "음소거로 시작" 설정을 잃음 | `viewer.js:46` | 마지막 설정 재사용 |
| J-10 | ⬜ | 멀티채팅 "통합" 배치가 저장되지 않음 | `multichat.js:99,111` | 배치 문자열 저장 |
| R-1 | ⬜ | 보조 채팅 창을 OS 닫기로 닫으면 채팅 세션이 계속 돎 | `crates/cview-app/src/windows.rs:98`, `state.rs:339` | Destroyed 때 세션 정리 |
| R-2 | ⬜ | 저장소 손상 시 매번 저장소 없이 실행(백업 복구 경로 미사용) | `state.rs:437-445` | 손상 파일 옮기고 백업 복구 |

## P2

| ID | 상태 | 항목 | 위치 |
|---|---|---|---|
| R-3 | ⬜ | 종료가 무한정 기다릴 수 있음 — 3초 상한 | `lib.rs:301-308` |
| R-4 | ⬜ | 레이아웃·창 위치 저장이 원자적이지 않음 | `multi_live.rs:738`, `window_prefs.rs:55,129` |
| R-5 | ⬜ | 비밀 금고 해석 실패 시 조용히 비우고 덮어씀 | `crates/cview-auth/src/secret_store.rs:311,350` |
| R-6 | ⬜ | 팔로우(POST)를 시간 초과·5xx 에 재시도(중복 적용) | `crates/cview-networking/src/client.rs:759-818` |
| R-7 | ⬜ | 로컬 HLS 프록시가 아무 출처에나 ACAO `*` + 로그인 쿠키 | `crates/cview-player/src/proxy/server.rs:165,189` |
| R-8 | ⬜ | 잠자기/깨기·네트워크 변경 처리 없음 | 전반 |
| R-9 | ⬜ | 채팅 WS 큐 무제한 | `crates/cview-chat/src/websocket.rs:329` |
| R-10 | ⬜ | 잠금 poison 이 연쇄 패닉 | `merged_budget.rs:44-95`, `vod_library.rs:76-145` |
| P-1 | ⬜ | 접근성 MutationObserver 가 body 전체를 보며 채팅 묶음마다 전체 문서 재검색 | `a11y.js:39` |
| P-2 | ⬜ | 멀티채팅 5초 개요·멀티라이브 700ms 동기화가 창이 숨어도 돎 | `multichat.js:90`, `multilive.js:154` |
| P-3 | ⬜ | 재생 버튼 아이콘을 700ms·지표마다 다시 그림 | `ml-pane-controls.js:153`, `viewer-controls.js:226` |
| P-4 | ⬜ | 비율 끌기 때마다 전체 렌더(아바타 img 재생성·localStorage 쓰기) | `multilive.js:394-409,347,286` |
| P-5 | ⬜ | 사이드바 팔로잉 폴링이 겹쳐 늘어남 | `app.js:89-104` |
| P-6 | ⬜ | 바로 전환 레일 맞추기가 항목마다 리플로 | `viewer-extras.js:97-103` |
| P-7 | ⬜ | 멀티채팅 검색이 메시지마다 600줄 재검색 | `multichat.js:207` |
| U-1 | ⬜ | 해요/합니다 혼용, 새로고침/새로 고침·전체 화면/전체화면 표기 섞임 | 여러 곳 |
| U-2 | ⬜ | 줄임표 누락(사이드바 메뉴 이름, 채팅 머리 채널 이름) | `app.css:270,526` |
| U-3 | ⬜ | 죽은 CSS(`#toast`, `.ip-body` 오타로 인사이트 등장 애니메이션 미적용, `.ml-pane-info`) | `app.css:1277`, `motion.css:58`, `multilive.css:68` |
| U-4 | ⬜ | 멀티라이브 볼륨 슬라이더 초점 표시 없음 | `multilive.css:246` |
| U-5 | ⬜ | 패인/단독 컨트롤 로직 어긋남(패인 "자동" 화질이 상한 초기화 안 함, 패인 라이브 이동이 재생 속도 안 되돌림) | `ml-pane-controls.js:207,235` |

## P3

| ID | 상태 | 항목 |
|---|---|---|
| Q-1 | ⬜ | 단독·패인 컨트롤 공통 로직(자동 숨김·화질 스크립트·라이브 이동) 공용 모듈로 |
| Q-2 | ⬜ | 디자인 토큰 정리(라이트 값 3중 정의, 덮어쓰기 구역 합치기, 하드코딩 크기·반경) |
| Q-3 | ⬜ | 죽은 코드(쓰이지 않는 상수·함수), 큰 파일 분할(player.html 5.7k줄, multi_live manager 2.4k줄) |
| Q-4 | ⬜ | JS 순수 함수 테스트 없음(node --test 로 시작), 앱 크레이트 명령 테스트 |
| Q-5 | ⬜ | PARITY_BACKLOG 🟡 중 이미 구현된 16개 표시 갱신(CHT-15/17/21, SET-14, SYS-04/08/19/26, PLY-06/10, MUL-08/10, NAV-10 등) |
| Q-6 | ⏭ | 미연결 뷰모델(VodBrowse·Clip), 큰 대응 항목(VOD-01, PLY-03 타임시프트 UI, NAV-04/14) — 별도 작업 |
| Q-7 | ⏭ | 라이브→멀티 넘기기 1~2초 재연결 — 서로 다른 iframe 이라 플레이어 이동 불가, 싸게 고칠 길 없음 |
