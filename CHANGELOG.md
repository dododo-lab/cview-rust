# 변경 기록

CView의 버전별 변경 사항이다. 모든 버전은 macOS(Apple Silicon)와 Windows(x64, v0.1.71부터 ARM64 포함) 설치 파일이 같은 번호로 함께 배포되었고,
설치 파일은 `https://cv.dododo.app/releases/<버전>/`(v0.1.73부터는 [GitHub 릴리스](https://github.com/dododo-lab/cview-rust/releases)도)에서 받을 수 있다. 버전 번호는 [유의적 버전](https://semver.org/lang/ko/)을 따르며,
배포할 때마다 버전과 macOS 빌드 번호가 함께 하나씩 오른다(`scripts/release-macos.sh`).

이 파일은 Git 태그와 커밋 기록에서 정리했다. 새 버전을 배포하면 맨 위에 항목을 더한다.

## [0.2.37](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.37) — 2026-11-27

- fix: 애니메이션·모션 정밀 고도화 1·2차 — 실제 결함 25건 ([#177](https://github.com/dododo-lab/cview-rust/pull/177))

## [0.2.36](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.36) — 2026-11-27

- fix: 기능 정밀 고도화 12·13차 — 실제 결함 43건 수정 ([#176](https://github.com/dododo-lab/cview-rust/pull/176))

## [0.2.35](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.35) — 2026-11-27

- fix: 기능 정밀 고도화 11차 — 실제 결함 수정 ([#175](https://github.com/dododo-lab/cview-rust/pull/175))

## [0.2.34](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.34) — 2026-11-27

- fix: 기능 정밀 고도화 9·10차 — 실제 결함 54건 수정 ([#174](https://github.com/dododo-lab/cview-rust/pull/174))

## [0.2.33](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.33) — 2026-11-27

- fix: 기능 정밀 고도화 6·7·8차 — 실제 결함 48건 수정 ([#173](https://github.com/dododo-lab/cview-rust/pull/173))

## [0.2.32](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.32) — 2026-11-26

- feat: 처음 실행 이용 고지·동의 창, 도움말 문의·저장소·라이선스 항목, 오픈소스 표기 ([#172](https://github.com/dododo-lab/cview-rust/pull/172))
- fix: 기능 정밀 고도화 5차 — 실제 결함 24건 수정 ([#171](https://github.com/dododo-lab/cview-rust/pull/171))

## [0.2.31](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.31) — 2026-11-26

- fix: 기능 정밀 고도화 4차 — 실제 결함 19건 수정 (#169 위에 쌓음) ([#170](https://github.com/dododo-lab/cview-rust/pull/170))

## [0.2.30](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.30) — 2026-11-26

- fix: 코드 검토로 새로 찾은 실제 결함 25건 수정(기능 정밀 고도화 2차) ([#168](https://github.com/dododo-lab/cview-rust/pull/168))

## [0.2.29](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.29) — 2026-11-26

- fix: 감사 문서에 남은 실제 결함 14건 수정(기능 정밀 고도화) ([#167](https://github.com/dododo-lab/cview-rust/pull/167))

## [0.2.28](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.28) — 2026-11-26

- feat(usage): 앱 사용률을 머리줄 칩에서 사이드바 아래로 옮기고 새로 설계 ([#166](https://github.com/dododo-lab/cview-rust/pull/166))

## [0.2.27](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.27) — 2026-11-26

- fix: 프로젝트 폴더 리뷰(2026-11-25) 발견 사항 10건 수정 ([#165](https://github.com/dododo-lab/cview-rust/pull/165))

## [0.2.26](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.26) — 2026-11-25

- feat(tray): macOS 메뉴 막대 아이콘 클릭 드롭다운 팝오버 UI/UX 구현

## [0.2.25](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.25) — 2026-11-25

- feat(tray): 창을 메뉴 막대·알림 영역으로 숨기기 + 아이콘·메뉴 새 디자인 ([#164](https://github.com/dododo-lab/cview-rust/pull/164))

## [0.2.24](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.24) — 2026-11-25

- fix(player): 백그라운드에서도 라이브 지연을 웹과 같게 — 숨은 페이지 타이머를 Worker 로·복귀 보정 ([#163](https://github.com/dododo-lab/cview-rust/pull/163))

## [0.2.23](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.23) — 2026-11-24

- perf(mem): 앱 글꼴을 운영체제 글꼴로 — Pretendard 웹폰트(2 MB) 제거 ([#162](https://github.com/dododo-lab/cview-rust/pull/162))

## [0.2.22](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.22) — 2026-11-24

- perf(mem): 기본 화면 상주 메모리 −26% — 등장 애니메이션 채움 해제·앰비언트 빛 작은 그림·앱 할당자 공간 우선 ([#161](https://github.com/dododo-lab/cview-rust/pull/161))

## [0.2.21](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.21) — 2026-11-24

- perf(mem): 메모리 최적화 Phase 3 — 계측 해제·비선택 패인 뒤 버퍼·재생 화면 블러·백그라운드 일시정지 ([#160](https://github.com/dododo-lab/cview-rust/pull/160))
- perf(mem): Phase 2 — OS 메모리 압박 연결·이미지 캐시 축소·TLS/클라이언트 공유·SQLite·숨은 페이지 이미지 놓기 ([#159](https://github.com/dododo-lab/cview-rust/pull/159))
- perf(mem): Phase 1 — 선명도 VRAM 해제·보조 창 iframe 차단·닫힌 패인 회수·누수 4건 ([#158](https://github.com/dododo-lab/cview-rust/pull/158))

## [0.2.20](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.20) — 2026-11-24

- ui: UI/UX 감사 보류 항목 A·B — 단축키 순서·층·반응형·접근성·대비·멀티라이브 ([#157](https://github.com/dododo-lab/cview-rust/pull/157))

## [0.2.19](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.19) — 2026-11-23

- ui: UI/UX 감사 Phase 1~3 — 깨짐·명암비·키보드·멀티라이브·문구 ([#155](https://github.com/dododo-lab/cview-rust/pull/155))

## [0.2.18](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.18) — 2026-11-23

- ci: 최적화 감사 Phase 4 — 성능 회귀 게이트(perf-gate)·checks 워크플로·PR 체크리스트 ([#152](https://github.com/dododo-lab/cview-rust/pull/152))

## [0.2.17](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.17) — 2026-11-23

- perf: 최적화 감사 Phase 3 — 응답 캐시 축출·루프백 TCP_NODELAY·macOS 버전 캐시·창 위치 저장 ([#151](https://github.com/dododo-lab/cview-rust/pull/151))

## [0.2.16](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.16) — 2026-11-23

- perf: 최적화 감사 Phase 2 — 키체인 재시도·이미지 캐시·메인 스레드·채팅 파서·플레이어 VRAM ([#150](https://github.com/dododo-lab/cview-rust/pull/150))

## [0.2.15](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.15) — 2026-11-23

- perf: 최적화 감사 Phase 1 — 부팅·채팅·멀티라이브·프록시·백그라운드 빠른 개선 ([#149](https://github.com/dododo-lab/cview-rust/pull/149))

## [0.2.14](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.14) — 2026-11-23

- feat: 치지직 HEVC(H.265) 1080p — 하드웨어로 풀면 HEVC, 못 풀거나 실패하면 H.264 ([#147](https://github.com/dododo-lab/cview-rust/pull/147))

## [0.2.13](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.13) — 2026-11-23

- fix: VM 검사에서 찾은 앱 결함 3건 — 멀티채팅 지난 줄 사라짐, 그리드 패인 키보드 Tab, 옛 WebKit :has 비용 ([#148](https://github.com/dododo-lab/cview-rust/pull/148))

## [0.2.12](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.12) — 2026-11-22

- 연구: 프레임 보간 5단계 네이티브 재생 실험 도구와 실측 — 치지직 LL-HLS AVPlayer·VT 보간 (앱 변경 없음) ([#146](https://github.com/dododo-lab/cview-rust/pull/146))
- 연구: 프레임 보간 개발 계획과 1·3단계 실측 — 보호 장치·서킷브레이커·RIFE 학습 가중치 (앱 변경 없음) ([#145](https://github.com/dododo-lab/cview-rust/pull/145))

## [0.2.11](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.11) — 2026-11-22

- perf: 멀티라이브 화면을 떠나면 보이지 않는 패인 디코드 중단·소리 패인 360p (실측 32.5 → 14.8) ([#144](https://github.com/dododo-lab/cview-rust/pull/144))

## [0.2.10](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.10) — 2026-11-21

- feat: 앱 실시간 사용률(CPU·GPU·메모리·네트워크) — 머리줄 칩·설정 › 성능 탭 ([#143](https://github.com/dododo-lab/cview-rust/pull/143))

## [0.2.9](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.9) — 2026-11-21

- fix(release): Windows 빌드 로그 전체 cat 출력 시 EAGAIN 실패 방지
- perf: 정지 패인 좀비 로더 차단, 단독 시청 백그라운드 디코드·WindowServer GPU·멀티채팅 DOM 최적화

## [0.2.8](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.8) — 2026-11-21

- feat(ui): 라이브 머리줄 채널 칩 우클릭 메뉴 ([#142](https://github.com/dododo-lab/cview-rust/pull/142))

## [0.2.7](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.7) — 2026-11-20

- feat(ui): 채팅 도구 서랍을 머리 줄 안에서 펼치기 ([#141](https://github.com/dododo-lab/cview-rust/pull/141))
- fix(build): Linux에서 cview-app 빌드가 되도록 rust-ini 0.21.1 고정 ([#140](https://github.com/dododo-lab/cview-rust/pull/140))

## [0.2.6](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.6) — 2026-11-20

- feat(ui): 라이브 머리줄 칩·채널 추가를 오른쪽으로, 칩 줄 슬라이딩 판·FLIP 모션 ([#139](https://github.com/dododo-lab/cview-rust/pull/139))

## [0.2.5](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.5) — 2026-11-20

- Retry both Parallels guest transport failure variants
- Resume matching release builds after Parallels transport errors
- Synchronize Cargo.lock before queuing release builds
- build: move formal Windows releases to Ubuntu and Parallels

## [0.2.4](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.4) — 2026-11-20

- feat(ui): 라이브 시청 상단 바·머리줄 재배치 — 의미별 묶음·넘침 메뉴·폭 단계 접기 ([#138](https://github.com/dododo-lab/cview-rust/pull/138))

## [0.2.3](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.3) — 2026-11-19

- perf: 채팅 붙이기 묶음·셸 채팅 버퍼 제거로 CPU·메모리 줄이기 ([#137](https://github.com/dododo-lab/cview-rust/pull/137))

## [0.2.2](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.2) — 2026-11-19

- fix(ui): 채팅 메뉴 잘림·머리줄 넘침·가림 결함과 입력 상태 다듬기 ([#136](https://github.com/dododo-lab/cview-rust/pull/136))

## [0.2.1](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.1) — 2026-11-19

- fix(ui): 화면 크기 리뷰 결함 6건 — 초기화·가져오기·모든 창 알림·로그인 창 제외·창 생성 시 적용 ([#135](https://github.com/dododo-lab/cview-rust/pull/135))
- feat(ui): 화면 크기 설정·고해상도 썸네일·넓은 창 배치, 모션 회귀 검사 ([#134](https://github.com/dododo-lab/cview-rust/pull/134))

## [0.2.0](https://github.com/dododo-lab/cview-rust/releases/tag/v0.2.0) — 2026-11-19

- fix(ui): 화면별 정밀 다듬기 — 잘림·겹침·넘침과 초점·그림자 결함 ([#133](https://github.com/dododo-lab/cview-rust/pull/133))

## [0.1.116](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.116) — 2026-11-17

- fix(ui): v0.1.112~0.1.114 UI 회귀 검사 실패 정리 ([#132](https://github.com/dododo-lab/cview-rust/pull/132))

## [0.1.115](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.115) — 2026-11-17

- feat(ui): 멀티라이브 채널 추가와 채널 고르기 단추 통합 ([#131](https://github.com/dododo-lab/cview-rust/pull/131))

## [0.1.114](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.114) — 2026-11-16

- feat(ui): 홈과 시청 리포트 통합 및 중복 구성 정리

## [0.1.113](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.113) — 2026-11-16

- fix(ui): 홈 이어보기를 최근 시청 영역으로 이동

## [0.1.112](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.112) — 2026-11-16

- fix: 라이브·채팅 경쟁 상태와 시작 화면 복구 개선

## [0.1.111](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.111) — 2026-11-16

- 멀티라이브 볼 채널 고르기: 시트 줄 넘침, 일부만 보일 때 화질·재생 속도 배분 ([#130](https://github.com/dododo-lab/cview-rust/pull/130))
- build: 릴리스 시간 단축 — Windows 서버 빌드를 일찍 시작하고 세 갈래로 동시에 ([#129](https://github.com/dododo-lab/cview-rust/pull/129))

## [0.1.110](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.110) — 2026-11-16

- 멀티라이브: 라이브에 상한보다 많이 담았을 때 볼 채널 고르기 ([#128](https://github.com/dododo-lab/cview-rust/pull/128))

## [0.1.109](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.109) — 2026-11-16

- ci: 빌드 서버 자동화 테스트를 PR 검사에 포함
- build: Windows 정식 릴리스를 빌드 서버로 연결

## [0.1.108](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.108) — 2026-11-16

- perf: 저장소 행 읽기·통계 개요·채팅 파싱·겹치는 읽기 IPC 최적화 ([#126](https://github.com/dododo-lab/cview-rust/pull/126))

## [0.1.107](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.107) — 2026-11-15

- feat(ui): 버튼 체계 — 유리 재질(Liquid Glass) 토큰과 일관된 모양·상태 ([#125](https://github.com/dododo-lab/cview-rust/pull/125))

## [0.1.106](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.106) — 2026-11-15

- feat(ui): '인기' → '방송&클립' — 방송·클립 탭, 보관함 인기 클립 이동, 탭 전환 모션 ([#124](https://github.com/dododo-lab/cview-rust/pull/124))
- fix(library): remove unsupported 30-day popular clips filter

## [0.1.105](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.105) — 2026-11-15

- 설정 '정보' 탭 — 버전·업데이트·새로운 기능·피드백·진단 보고서·사용 데이터·링크 ([#123](https://github.com/dododo-lab/cview-rust/pull/123))
- WebKit 마우스 닫기 초점(후원 목록·채널 추가 서랍) + 우클릭 뒤 기본 문맥 메뉴(검사) ([#122](https://github.com/dododo-lab/cview-rust/pull/122))

## [0.1.104](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.104) — 2026-11-14

- 홈 "방송 중인 팔로잉" 후속 수정 — 입력 방식 판단·숨은 홈 Esc·더 보기 초점·합성 포인터 이벤트 (+검토 17건) ([#121](https://github.com/dododo-lab/cview-rust/pull/121))

## [0.1.103](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.103) — 2026-11-14

- 홈 "방송 중인 팔로잉" 재설계 — 우선순위 타일 격자 + 미리보기 카드 ([#120](https://github.com/dododo-lab/cview-rust/pull/120))

## [0.1.102](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.102) — 2026-11-14

- 위험 글자색 대비 4.5:1 — 라이트 --error #c4291f·다크 #ff7066, 플레이어 오류는 --error-on-media ([#119](https://github.com/dododo-lab/cview-rust/pull/119))

## [0.1.101](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.101) — 2026-11-14

- 메뉴 줄 키보드 초점 3:1 — 줄 메뉴·계정 메뉴에 틈 둔 고리, 호버와 구별 ([#118](https://github.com/dododo-lab/cview-rust/pull/118))
- test(card-structure): CS05 한 줄 카드 수를 레이아웃 위치(offsetTop)로 — 등장 애니메이션 도중 읽어 부하 때 실패 ([#117](https://github.com/dododo-lab/cview-rust/pull/117))

## [0.1.100](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.100) — 2026-11-14

- 단추 누름 한 번만 · '새 메시지'·서랍 + 누름 보이게 · 초점 테두리 3:1(라이트) ([#116](https://github.com/dododo-lab/cview-rust/pull/116))

## [0.1.99](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.99) — 2026-11-14

- primary 단추 호버: 회색으로 덮이던 버그 + 단추 뒤로 번지던 글로우를 안쪽 빛(빛을 받는 키)으로 ([#115](https://github.com/dododo-lab/cview-rust/pull/115))

## [0.1.98](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.98) — 2026-11-14

- style: 선명도 설정 왕복 테스트 clippy(field_reassign_with_default) 맞춤 ([#113](https://github.com/dododo-lab/cview-rust/pull/113))
- style: 선명도 설정 테스트 cargo fmt 맞춤 ([#113](https://github.com/dododo-lab/cview-rust/pull/113))
- 채널 추가 단추 호버: 단추 뒤로 번지던 글로우 대신 안쪽에 맺히는 빛(빛을 받는 키) ([#114](https://github.com/dododo-lab/cview-rust/pull/114))
- 선명한 화면 + 업스케일을 '선명도' 하나로 — 끔·약·중·강 + 크게 볼 때만 적용 ([#113](https://github.com/dododo-lab/cview-rust/pull/113))

## [0.1.97](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.97) — 2026-11-14

- 채팅 위 후원 칩 띠를 머리의 후원 단추 + 목록 패널로 — 새 후원 배지, 정렬·거르기·합계, 항목 → 채팅 줄 ([#112](https://github.com/dododo-lab/cview-rust/pull/112))

## [0.1.96](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.96) — 2026-11-13

- Windows 설치 파일에 WebView2Loader.dll 동봉 — arm64 판이 시작되지 않던 문제 (v0.1.71~) ([#111](https://github.com/dododo-lab/cview-rust/pull/111))

## [0.1.95](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.95) — 2026-11-13

- 통합 멀티채팅 렌더러·입력부 공유 — lineHtml 출처 태그·ChatComposer·줄 동작·촘촘/말풍선 (UI 리뷰 P2-1) ([#110](https://github.com/dododo-lab/cview-rust/pull/110))
- 지우기·초기화 확인 방식 통일 — 되돌리기 토스트·두 번 누르기·확인 모달 규칙과 공용 도우미 (P3-4) ([#109](https://github.com/dododo-lab/cview-rust/pull/109))
- 카드 구조: 초점 칸 안 단추 중첩 없애기 — 주 동작을 진짜 단추로 (UI 리뷰 P3-2) ([#108](https://github.com/dododo-lab/cview-rust/pull/108))
- 단독 보기 ⚙ 에 배속·화면 비율·자동 보정/업스케일·이퀄라이저 (P2-4 패리티) ([#107](https://github.com/dododo-lab/cview-rust/pull/107))

## [0.1.94](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.94) — 2026-11-13

- 배포 스크립트·내부 정리만 있는 재배포.

## [0.1.93](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.93) — 2026-11-13

- UI 리뷰 중기 개선: 통합 멀티채팅 blinded·모달 포커스 가두기·기호 단추 이름·대비/토큰·좁은 창 (P2-1·P3) ([#106](https://github.com/dododo-lab/cview-rust/pull/106))
- UI 리뷰 즉시 수정: 단독 보기 OSD·오버레이 채팅·지난 채팅 기록·익명·녹화·PiP·팔레트 (P1 전부) ([#104](https://github.com/dododo-lab/cview-rust/pull/104))

## [0.1.92](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.92) — 2026-11-13

- 익명 후원자는 '익명', 금액 비공개 후원은 금액 자리 비우기 ([#103](https://github.com/dododo-lab/cview-rust/pull/103))

## [0.1.91](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.91) — 2026-11-13

- 채널 정보 탭 고도화: 지금/마지막 방송·채팅 설정·채널·소개 링크·내 시청 기록·최근 다시보기·인기 클립 카드 격자 ([#102](https://github.com/dododo-lab/cview-rust/pull/102))
- 배포 래퍼: stash 검색 awk 조기 종료 제거 ([#101](https://github.com/dododo-lab/cview-rust/pull/101))

## [0.1.90](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.90) — 2026-11-13

- 채널 추가 사이드바 목록 UI/UX 정밀 검증·고도화: 다크 자리 칸·호버·키보드 초점·썸네일 배지 겹침·추가됨 표시·첫 글자 대체·좁은 서랍 수정 ([#100](https://github.com/dododo-lab/cview-rust/pull/100))
- 연구: 뉴럴엔진 실시간 프레임 보간 조사·측정 (앱 변경 없음) ([#99](https://github.com/dododo-lab/cview-rust/pull/99))

## [0.1.89](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.89) — 2026-11-13

- 채팅 UI/UX 정밀 검증·수정: 블라인드 누락·남의 채팅 제한 표시·숨은 다시 연결 고침, 줄 동작 막대·키보드 고르기·검색·입력 기록 추가 ([#98](https://github.com/dododo-lab/cview-rust/pull/98))

## [0.1.88](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.88) — 2026-11-13

- 4.17초 세그먼트 채널도 웹과 같은 자리에서 끊김 없이: hls.js LL 파트 로드의 세그먼트 경계 멈춤 수정 ([#97](https://github.com/dododo-lab/cview-rust/pull/97))

## [0.1.87](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.87) — 2026-11-13

- 라이브 지연을 치지직 웹과 같게: LL-HLS·목표 2초·웹 버퍼 기준 따라잡기 ([#96](https://github.com/dododo-lab/cview-rust/pull/96))

## [0.1.86](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.86) — 2026-11-12

- 채팅 메시지 앞에 프로필 사진·사용자별 랜덤 아이콘 표시 ([#95](https://github.com/dododo-lab/cview-rust/pull/95))
- 모션: 라이브 정보란·채널 화면에 등장 순차·숫자 카운트업/롤링·LIVE 맥동·탄성 호버·카드 기울기·배너 패럴랙스 ([#94](https://github.com/dododo-lab/cview-rust/pull/94))
- 라이브 머리줄 "채널 추가" 단추를 눌리는 단추로 더 분명하게 ([#93](https://github.com/dododo-lab/cview-rust/pull/93))

## [0.1.85](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.85) — 2026-11-12

- 성능: 창이 가려진 동안 채팅 줄을 붙일 때 강제 레이아웃 없애기 ([#92](https://github.com/dododo-lab/cview-rust/pull/92))

## [0.1.84](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.84) — 2026-11-09

- 멀티라이브 장시간 웹 콘텐츠 메모리 증가 줄이기(채팅 후원 칩 다시 그리기·창 가림 디코드 정지) ([#91](https://github.com/dododo-lab/cview-rust/pull/91))
- 네트워크 복구 뒤 창 전체가 검게 남는 문제: 멈춘 웹 화면 자동 복구 + 오프라인 복구 자잘한 버그 수정 ([#90](https://github.com/dododo-lab/cview-rust/pull/90))

## [0.1.83](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.83) — 2026-11-08

- 성능 E: 창을 끄는 동안 위치 저장을 미뤄 메인 스레드 디스크 쓰기 없애기 ([#89](https://github.com/dododo-lab/cview-rust/pull/89))
- 성능 3단계: 멀티라이브 패인 폴 live-status 나눠 쓰기·유휴 상태 루프 줄이기 ([#88](https://github.com/dododo-lab/cview-rust/pull/88))
- 성능 2단계: 붐비는 채팅에서 줄 지우기를 묶어 레이아웃 줄이기 ([#87](https://github.com/dododo-lab/cview-rust/pull/87))
- 성능 1단계: 시작 시 팔로잉 대기 제거·팔로잉 폴링 요청 줄이기 ([#86](https://github.com/dododo-lab/cview-rust/pull/86))

## [0.1.82](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.82) — 2026-11-08

- UI 고도화 6단계: 빈 상태를 한 가지 구조로 통일 ([#85](https://github.com/dododo-lab/cview-rust/pull/85))
- UI 고도화 1단계: 디자인 토큰 정리와 보조 글자 대비 개선 ([#80](https://github.com/dododo-lab/cview-rust/pull/80))

## [0.1.81](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.81) — 2026-11-07

- 라이브 채널 탭: 프로필 이미지 위에 채널 닫기(×) 표시 ([#79](https://github.com/dododo-lab/cview-rust/pull/79))

## [0.1.80](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.80) — 2026-11-07

- macOS 섬·제어 센터 '지금 재생 중'에 방송 제목·채널·썸네일 표시 ([#78](https://github.com/dododo-lab/cview-rust/pull/78))

## [0.1.79](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.79) — 2026-11-06

- 보관함 개요 탭 리디자인: 구역 제목·통계식 카드·폭별 1/2/4열, 옆 패널은 정리 제안 ([#77](https://github.com/dododo-lab/cview-rust/pull/77))

## [0.1.78](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.78) — 2026-11-06

- 보관함 개요 탭: 모든 탭을 요약 카드로 한눈에, 카드를 누르면 그 탭으로 ([#76](https://github.com/dododo-lab/cview-rust/pull/76))

## [0.1.77](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.77) — 2026-11-05

- 영상 위 후원 알림을 항상 같은 크기로: 고정 너비·높이·글자 크기, 긴 글은 말줄임 ([#75](https://github.com/dododo-lab/cview-rust/pull/75))

## [0.1.76](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.76) — 2026-11-04

- 치지직 시청 이벤트 전송: 앱 시청을 시청자 수·계정 시청 기록에 반영 ([#74](https://github.com/dododo-lab/cview-rust/pull/74))

## [0.1.75](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.75) — 2026-11-04

- 통계 화면 고도화: 기간 필터·쌓인 추이·요일×시간대 히트맵·카테고리 비중·채널 순위표 ([#73](https://github.com/dododo-lab/cview-rust/pull/73))
- 빌드 시간 단축: dev 의존성 디버그 정보 끄기, Windows 빌드를 공증 대기와 겹치기 ([#72](https://github.com/dododo-lab/cview-rust/pull/72))

## [0.1.74](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.74) — 2026-11-03

- GitHub 릴리스 업로드와 GitHub 경유 앱 자동 업데이트 ([#71](https://github.com/dododo-lab/cview-rust/pull/71))

## [0.1.73](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.73) — 2026-11-03

- 검색 결과 화면 디자인 정밀화 ([#70](https://github.com/dododo-lab/cview-rust/pull/70))

## [0.1.72](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.72) — 2026-11-03

- 보안 점검: 프록시 DNS 리바인딩 차단, 채팅 이모티콘 호스트 제한, 저장 확장자 제한 외 ([#69](https://github.com/dododo-lab/cview-rust/pull/69))

## [0.1.71](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.71) — 2026-11-03

- 배포에 Windows arm64 NSIS 설치 파일 추가 ([#68](https://github.com/dododo-lab/cview-rust/pull/68))

## [0.1.70](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.70) — 2026-11-03

- 검색 병합 뒤 cargo fmt 정리
- 채널 클립 더 보기: clipUID 커서로 다음 쪽 받기 ([#67](https://github.com/dododo-lab/cview-rust/pull/67))
- 검색: 방송 중인 검색 채널을 라이브 탭에, 검색한 채널의 클립 보이기 ([#66](https://github.com/dododo-lab/cview-rust/pull/66))
- 업데이트에 Tailscale이 필요하다는 잘못된 릴리스 노트 안내 제거 ([#63](https://github.com/dododo-lab/cview-rust/pull/63))
- 메뉴 화면 직접 새로고침 때 로고 로딩과 본문 흐림 (공통 덮개) ([#62](https://github.com/dododo-lab/cview-rust/pull/62))
- release-with-prs.sh: 워크트리에서 실행 가능하게, stash는 SHA로 되돌리기 ([#65](https://github.com/dododo-lab/cview-rust/pull/65))

## [0.1.69](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.69) — 2026-11-03

- 업데이트 피드 ed25519 서명 검증 ([#64](https://github.com/dododo-lab/cview-rust/pull/64))

## [0.1.68](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.68) — 2026-11-03

- hls.js 1.7.4-cview.2로 갱신, 오프라인 복구 신호를 재연결에 연결 ([#61](https://github.com/dododo-lab/cview-rust/pull/61))

## [0.1.67](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.67) — 2026-11-02

- 채팅 닉네임을 누르면 그 사람 말풍선(프로필 카드·팔로우·채팅만 보기) ([#60](https://github.com/dododo-lab/cview-rust/pull/60))

## [0.1.66](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.66) — 2026-11-02

- 팔로잉 라이브 카드의 빈 썸네일을 다시 불러 채우기

## [0.1.65](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.65) — 2026-11-02

- 앱 시작 로딩 화면이 홈 데이터를 기다렸다가 걷히게 ([#59](https://github.com/dododo-lab/cview-rust/pull/59))

## [0.1.64](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.64) — 2026-11-02

- 치지직 파트너·챌린저 인증 마크 표시 ([#58](https://github.com/dododo-lab/cview-rust/pull/58))
- 채팅 머리 기능 아이콘을 '⋯' 서랍으로 접기 ([#57](https://github.com/dododo-lab/cview-rust/pull/57))

## [0.1.63](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.63) — 2026-10-31

- 홈 시작·썸네일 로딩 최적화 ([#56](https://github.com/dododo-lab/cview-rust/pull/56))

## [0.1.62](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.62) — 2026-10-31

- 라이브 머리줄 채널 칩: 4개만 보이고 나머지는 휠로 넘겨 보기 ([#55](https://github.com/dododo-lab/cview-rust/pull/55))

## [0.1.61](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.61) — 2026-10-31

- 라이브 머리줄: 다른 방송 바로 전환 줄 제거, 채널 칩 줄 가운데 정렬 ([#52](https://github.com/dododo-lab/cview-rust/pull/52))
- 사이드바 아래 채널 목록을 메뉴마다 다르게, 라이브 메뉴는 팔로잉 시청자 많은 순 ([#54](https://github.com/dododo-lab/cview-rust/pull/54))
- 새 버전 자동 확인과 앱 안 업데이트 알림 ([#53](https://github.com/dododo-lab/cview-rust/pull/53))

## [0.1.60](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.60) — 2026-10-31

- 정밀 검증: 광고 멈춤·Windows 창 단추 가림·미리보기 프록시 경쟁 등 5건 수정 ([#50](https://github.com/dododo-lab/cview-rust/pull/50))
- 저장소 소개 문서 정비, GPL-3.0 라이선스, 배포 서버 정보를 로컬 설정으로 ([#51](https://github.com/dododo-lab/cview-rust/pull/51))

## [0.1.59](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.59) — 2026-10-31

- Windows: 인사이트 패널이 펼쳐진 화면은 머리줄 오른쪽 버튼을 끝까지 붙인다 ([#44](https://github.com/dododo-lab/cview-rust/pull/44))

## [0.1.58](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.58) — 2026-10-31

- macOS 판은 치지직 프리롤 광고를 내지 않음(Windows 판만) ([#49](https://github.com/dododo-lab/cview-rust/pull/49))

## [0.1.57](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.57) — 2026-10-31

- 라이브·멀티라이브 프리롤을 영상 영역 안에만 표시, 광고 닫기(✕) 제거 ([#48](https://github.com/dododo-lab/cview-rust/pull/48))

## [0.1.56](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.56) — 2026-10-31

- 라이브 입장 프리롤 광고가 나오지 않던 문제 수정 ([#47](https://github.com/dododo-lab/cview-rust/pull/47))

## [0.1.55](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.55) — 2026-10-31

- 치지직 규칙 맞추기: Windows 1080p는 커넥터 필요, 치트키 없으면 프리롤 광고 ([#46](https://github.com/dododo-lab/cview-rust/pull/46))

## [0.1.54](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.54) — 2026-10-31

- 배포 스크립트·내부 정리만 있는 재배포.

## [0.1.53](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.53) — 2026-10-31

- Windows 썸네일 미리보기가 재생되지 않던 문제 수정 ([#45](https://github.com/dododo-lab/cview-rust/pull/45))

## [0.1.52](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.52) — 2026-10-31

- Windows: 제목 막대·메뉴 막대를 걷고 창 단추를 화면에 통합 ([#43](https://github.com/dododo-lab/cview-rust/pull/43))

## [0.1.51](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.51) — 2026-10-31

- 썸네일 호버 영상 미리보기 (홈·팔로잉·인기·카테고리·보관함) ([#42](https://github.com/dododo-lab/cview-rust/pull/42))

## [0.1.50](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.50) — 2026-10-31

- 배포 스크립트·내부 정리만 있는 재배포.

## [0.1.49](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.49) — 2026-10-30

- 라이브 머리줄 채널 칩 정리 + 앱 글꼴 Pretendard로 ([#40](https://github.com/dododo-lab/cview-rust/pull/40))
- 채팅 말풍선·후원 카드 정밀 다듬기 ([#41](https://github.com/dododo-lab/cview-rust/pull/41))

## [0.1.48](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.48) — 2026-10-30

- 배포 스크립트·내부 정리만 있는 재배포.

## [0.1.47](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.47) — 2026-10-30

- 홈 화면 리디자인: 앰비언트 스포트라이트, TOP 10 랭킹, 퀵픽 최근 시청, 더 보기 격자 ([#39](https://github.com/dododo-lab/cview-rust/pull/39))

## [0.1.46](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.46) — 2026-10-30

- 애니메이션 리뷰 수정 (1~8, 10~14번) ([#38](https://github.com/dododo-lab/cview-rust/pull/38))

## [0.1.45](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.45) — 2026-10-30

- 영상 재생 리뷰 수정 (1~12번) ([#37](https://github.com/dododo-lab/cview-rust/pull/37))

## [0.1.44](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.44) — 2026-10-29

- 라이브 단독 보기 채널 정보 바 리디자인 + 앱 전체 버튼 모션 ([#36](https://github.com/dododo-lab/cview-rust/pull/36))

## [0.1.43](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.43) — 2026-10-28

- 라이브 머리줄: 다른 메뉴·채널 추가 사이드바에서 연 채널이 쌓이게 ([#35](https://github.com/dododo-lab/cview-rust/pull/35))

## [0.1.42](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.42) — 2026-10-28

- 릴리스 래퍼 스크립트: PR 병합·작업 보관·main 갱신·릴리스·작업 복원 ([#34](https://github.com/dododo-lab/cview-rust/pull/34))
- 라이브 단독 보기와 멀티라이브의 채널 수 상한 분리 ([#33](https://github.com/dododo-lab/cview-rust/pull/33))

## [0.1.41](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.41) — 2026-10-28

- 전체 코드 리뷰 · 관용구 정리 · 큰 파일 모듈화 ([#32](https://github.com/dododo-lab/cview-rust/pull/32))

## [0.1.40](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.40) — 2026-10-28

- 멀티라이브 좌/우 포커스: 메인 영상 16:9 + 아래 채널 정보로 레터박스 줄이기 ([#31](https://github.com/dododo-lab/cview-rust/pull/31))

## [0.1.39](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.39) — 2026-10-27

- 배포 스크립트·내부 정리만 있는 재배포.

## [0.1.38](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.38) — 2026-10-27

- 비라이브 메뉴 UI/UX 검증 13건 반영 ([#30](https://github.com/dododo-lab/cview-rust/pull/30))

## [0.1.37](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.37) — 2026-10-27

- 멀티라이브 채팅 포커스·그리드에서 다른 칸이 멈춰 보이던 문제 수정 ([#28](https://github.com/dododo-lab/cview-rust/pull/28))

## [0.1.36](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.36) — 2026-10-27

- 메뉴 막대를 종류별로 묶고 이동 메뉴 분리 ([#26](https://github.com/dododo-lab/cview-rust/pull/26))
- 라이브 시청 중 채팅 자동 스크롤이 멈추던 문제 수정 ([#27](https://github.com/dododo-lab/cview-rust/pull/27))

## [0.1.35](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.35) — 2026-10-26

- 앱 복귀 후 채팅 자동 스크롤이 멈추던 문제 수정 ([#24](https://github.com/dododo-lab/cview-rust/pull/24))
- 업데이트 게시 기준 주소를 cv.dododo.app 으로 변경 ([#25](https://github.com/dododo-lab/cview-rust/pull/25))

## [0.1.34](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.34) — 2026-10-26

- 앱 기본 글꼴을 나눔고딕으로 — 글꼴 파일을 앱에 넣어 macOS·Windows에서 같게 보이게

## [0.1.33](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.33) — 2026-10-26

- 라이브 ↔ 멀티라이브 전환 때 머리줄은 고정하고 본문만 움직이게
- 라이브·멀티라이브 머리줄 제목과 아이콘을 '라이브'로 통일 — 보기를 바꿔도 머리줄은 그대로
- 라이브 단독 보기 머리줄의 방송 아바타 줄을 30px로 — 머리줄이 6px 높아지지 않게
- 라이브 단독 보기 영상 아래에 채널 정보·다시보기·클립·커뮤니티 추가
- 라이브 정보란 디자인 개선 — 채널 색 카드·알약 메타·붙어 다니는 탭·숫자 타일·카드형 커뮤니티
- 라이브 정보란 디자인 정밀화 — 붙는 탭의 채널 표시·자동 이어 불러오기·사진 크게 보기·뼈대 로딩

## [0.1.32](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.32) — 2026-10-26

- 라이브 목록 머리줄 좌우 여백·제목 폭을 멀티라이브와 같게 — 화면 전환 때 머리줄이 옆으로 뛰지 않게

## [0.1.31](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.31) — 2026-10-26

- 라이브 메뉴 라우팅·머리줄 UX 리뷰 반영 (H-1·H-2·M-1~M-6·C-2·L-1~L-4·DS-1·DT-3)
- 예전 시청 화면·채팅·단축키 UI/UX 다듬기 (H-4 M-7 M-8 M-9 L-6 L-7 L-8 L-9 L-11 DC-1 DC-3 DC-4 D-2 D-4 D-10 DZ-3 DZ-4 DZ-5 DF-1 DT-2 DT-3 DT-5 DS-8)
- 단축키 도움말을 현재 바인딩으로, 라이브 머리줄 빈칸 끌기 (M-9·DZ-4 통합)
- 멀티라이브 UI/UX·디자인 검증 항목 정리
- 멀티라이브 머리줄 빈칸도 창 끌기 (DZ-4)
- 멀티라이브: 무대 왕복 뒤 멈춘 패인을 창 복귀 때 되살리고 무대 전환 기록 추가 (리뷰 C-1)
- 라이브 목록 머리줄 새로고침 버튼을 30px로 — 멀티라이브와 같은 높이 (DS-1)

## [0.1.30](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.30) — 2026-10-26

- 라이트 모드 채팅 기본 배경을 흰색 계열로
- 치지직 이모티콘 API 개편 대응 — 표시·입력·목록·후원 알림 수정
- cargo fmt — 이모티콘 모델·테스트 포맷 정리

## [0.1.29](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.29) — 2026-10-26

- 다크 모드 패인·채팅 그림자를 밝은 파스텔 색조 위주로

## [0.1.28](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.28) — 2026-10-26

- 라이브 영상·채팅 테두리 제거, 채널 색 그림자로 리디자인

## [0.1.27](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.27) — 2026-10-26

- 업데이트: 앱 안에서 받아 검증하고 바꿔 다시 여는 자동 설치
- 문서: 앱 안 자동 설치가 피드 sha256·공증에 기대는 점
- 업데이트 진행 막대: 바탕 트랙이 보이게
- 업데이트: macOS 빌드 오류(가린 dir 이 클로저로 옮겨진 뒤 다시 쓰임) 수정
- 업데이트: 설치 경로를 실제 피드로 끝까지 시험하는 macOS e2e 테스트, 재실행 도우미를 앱과 다른 프로세스 그룹으로

## [0.1.26](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.26) — 2026-10-25

- 멀티라이브 메인 포커스가 이전 패인으로 되돌아가던 경쟁 상태 수정
- 멀티라이브 패인·채팅 테두리 리디자인

## [0.1.25](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.25) — 2026-10-25

- 선명한 화면: GPU 캔버스가 검은 화면만 그리면 다음 백엔드로 폴백
- 선명한 화면: MSE(hls.js) 영상은 처음부터 WebGL 로 그린다

## [0.1.24](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.24) — 2026-10-25

- 라이브 단독 보기: 예전 시청 화면 기능을 무대 패인 위에서 되살림
- 라이브 단독 보기: 앱 메뉴·Esc·닫기를 연결하고 닫으면 보던 화면으로

## [0.1.23](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.23) — 2026-10-25

- Automate release uploads and verified publication over Tailscale
- 사이드바 아이콘 호버 모션과 로고 이펙트 추가

## [0.1.22](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.22) — 2026-10-24

- 업데이트 확인: 자체 서버 피드를 기본값으로, IIS 406 수정

## [0.1.21](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.21) — 2026-10-24

- 업데이트 확인을 자체 HTTPS 서버(CVIEW_UPDATE_URL)로 전환
- 업데이트 확인을 앱 메뉴와 설정 화면에 추가

## [0.1.19](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.19) — 2026-10-24

- 채널 추가 목록: + 는 멀티라이브에 추가·선택, 줄(썸네일·이름)은 라이브로 단독 재생
- 채널 추가 사이드바의 + 가 라이브로 넘어가던 문제
- 채널 추가 사이드바의 + 가 여전히 라이브로 넘어가던 문제
- 멀티라이브 머리줄: 레이아웃·포커스 배치·여러 소리를 ⋯ 메뉴로
- 멀티라이브 채널 추가 버튼: 눌림·호버·열림 상태 정리
- 네트워크 프리셋: 자동 — 멀티라이브 상황에 맞춰 밸런스·고성능·안정 우선을 고름
- 모션 줄이기 빈틈: 설정 "절약"에서 홈 히어로가 매 프레임 넘어가던 문제 외
- CSS 정리: 덮여 죽은 모션 규칙 지우기·다운로드 진행 scaleX·빠진 호버
- 채널 추가 사이드패널 줄: 예전 시청 화면 대신 멀티라이브 라이브 단독 보기로
- Esc·Enter 겹침: 작은 창을 닫으려던 Esc 가 시청 화면을 닫고, 카드 Enter 가 채널을 두 번 열던 문제
- 시청 화면 키·초점: 버튼·선택 상자 키를 단축키가 가로채지 않게, 초점 있는 막대는 숨지 않게
- 멀티라이브 초점: 채널 추가 목록이 상태마다 다시 그려져 초점을 잃던 문제·닫으면 연 버튼으로
- 화면 상태·경합: 설정 검색이 지금 탭만 찾던 문제·단축키 받기가 다음 키를 삼키던 문제 외
- 채팅: ⌘J 가 보이는 패널 수만큼 뒤집히던 문제·TTS 가 화면을 떠나도 읽던 문제·설정 구독 누수
- 모션: 팔레트·메뉴·플레이어 패널이 닫힐 때 한 프레임에 사라지던 것 — 짧게 줄며 흐려지게
- 팔레트 선택 유지·초점 돌려주기, 끌기 놓기/취소 리스너 정리
- 다시 그릴 때 키보드 초점 지키기: 사이드바 팔로잉 폴링·다운로드 진행
- 고도화 백로그: 2차(2026-10-23) 고친 것·남긴 것
- 채널 추가 사이드패널: 마우스로 열고 닫으면 초점이 여는 버튼으로 돌아오지 않던 문제
- 영상 오버레이 LIVE 를 위쪽 정보 칩에만 — 아래 막대의 LIVE 는 늦어졌을 때만
- 채팅: 카카오톡형 말풍선 하나로 통합·고도화 — 나머지 디자인 스타일 7종 제거
- 채널 추가 사이드패널 리디자인 — 남은 자리 칸·출처 개수·풍부한 줄·스켈레톤·추가 피드백
- 라이브 재생은 늘 멀티라이브 라이브 단독 보기로 — 둘러본 채널은 바꿔 끼워 쌓이지 않게
- 모션 2차: 세그먼트 판 미끄러짐·머리줄 등장·빈 화면·기본 등급 카드 차례 등장 외
- 재생 계측: 프록시 세그먼트 수신 요약을 성능 로그에(player.net, 10초 표본)
- 멀티라이브 메인 패인: 패인이 많으면 목표 지연에 여유 — 대역폭 경쟁으로 잦던 멈춤
- 멀티라이브 패인이 단독 시청처럼 돌던 문제 — 패인 플레이어에 is_multi_live 를 켠다
- 고도화 백로그: M-2 의 is_multi_live 부분 해결 표시
- 저지연 백오프: 멈춤이 반복되는 채널은 올린 목표를 120초 지킨다(외딴 멈춤은 그대로 45초)
- 테스트: 만료 음소거 판정이 바쁠 때 가끔 실패하던 문제 — 10ms 대신 0.2초로
- 멀티라이브 포커스: 아래 패인을 더블클릭해 메인으로 올리면 아래 패인이 모두 감춰지던 문제
- 멀티라이브: 아래 패인을 누르면 포커스 전환 대신 라이브 단독 보기로 넘어가던 문제
- GPU: 재생 중인 영상 위 흐림 걷어내기·자동 화질 보정은 메인 패인만
- CView GPU 바쁜 비율 측정 도구(gpu-sample.py) — ioreg accumulatedGPUTime, 관리자 권한 불필요
- gpu-sample.py 에 WindowServer(화면 합성) 몫도 표시 — 웹뷰의 영상·레이어 합성은 CView 가 아니라 여기로 잡힌다
- 멀티라이브: 패인이 4개 이상이면 소리 없는 보조 패인을 360p 로(메인은 1080p 그대로)

## [0.1.18](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.18) — 2026-10-24

- 접기·펼치기 전환을 자연스럽게 — 아이콘 미끄러짐·라벨 페이드
- v0.1.17 로고 줄(44px·로고 36/38)과 motion.css 전환에 맞춤
- 더 스르륵 — 부드러운 출발 곡선, 라벨 미끄러짐, 전환 중 레일 재기 멈춤
- 영상 시청 중 접기·펼치기 끊김 — 사이드바가 본문 위로 미끄러지고 본문은 한 번만 크기 변경
- 영상 재생 중 전환을 합성(transform·opacity)만으로 — 멀티라이브 여러 패인에서도 끊기지 않게
- 영상 위 오버레이·자동 화질 보정의 메인 스레드·합성 부하 줄이기
- 선명한 화면: SVG 필터 대신 WebGL(GPU)로 언샤프 마스크
- 멀티라이브: 패인 추가·제거 모션, 끌어서 메인 바꾸기, 멀티채팅
- 멀티라이브 멀티채팅: 그리드·포커스 보기 추가
- 팔로잉: 팔로우 날짜·알림·인증·팔로워·마지막 방송 등 정보 확장
- 멀티라이브: 포커스(메인) 패인이 바뀌면 채팅도 그 채널로
- 프로필을 누르면 말풍선 계정 메뉴, 로그아웃은 그 안으로
- 멀티라이브를 라이브 메뉴 속 보기로 — 사이드바 항목을 없애고 머리줄 전환 버튼, 제목이 보기를 따른다
- 멀티라이브 → 라이브 전환 시 메인·선택 채널을 그대로 이어 보고, 나머지는 머리줄에서 골라 보기
- 사이드바 위쪽 테마 버튼 제거 — 프로필 말풍선 메뉴와 겹침
- 후원 금액별 색 단계 — 치지직 초록에서 시작해 고액일수록 화려하게
- 검색란 안쪽 네모 초점 테두리 제거 — 감싸는 필드의 둥근 링만 남긴다
- 라이브에 다녀오면 멀티라이브 채팅이 끊긴 채 남던 문제
- 연결·해제 요청 순서 레이스와 이전 채널 이벤트 섞임
- player: 선명한 화면·업스케일 GPU 경로 — WebGPU(Metal 직결) 백엔드 + WebGL 패스 합치기
- ui: 재생 중인 영상 위 흐림(backdrop-filter) 제거, 후원 반짝임을 transform 으로
- metal-bench — 화면에 안 띄우는 WKWebView 로 플레이어 GPU 경로 CPU·GPU 측정
- metal-bench run.sh — zsh 가 $BASE:a 를 경로 변환자로 읽던 것 고침
- metal-bench — 가려짐 감지 끄기를 KVC 대신 세터 직접 호출(macOS 27 에서 예외)

## [0.1.17](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.17) — 2026-10-23

- 배포 스크립트·내부 정리만 있는 재배포.

## [0.1.16](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.16) — 2026-10-23

- 모션·상호작용 다듬기 — 누름 피드백·팝오버/시트 등장·퇴장·썸네일 페이드인·스크롤 머리줄

## [0.1.15](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.15) — 2026-10-23

- 저장·다운로드 파일 이름을 create_new 로 원자적으로 예약 — 번호 소진 시 덮어쓰지 않고 오류
- 토큰 교환 결과를 확인한 뒤 완료/실패 페이지를 보내고, 콜백 연결마다 읽기 시간 제한
- has_more 를 '시작 위치 + 받은 개수 < 전체' 로 계산
- 리다이렉트 hop 마다 이미지 URL 정책 적용 + 포트·사용자 정보 거부
- 채널 추가를 오른쪽 채팅 위 슬라이드 사이드바로 — 정렬·출처 필터·새로고침

## [0.1.14](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.14) — 2026-10-23

- 패인 위 정보도 라이브 화면 상단처럼 — 경과 시간·누적 시청자 칩

## [0.1.13](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.13) — 2026-10-23

- 영상 위 상단 정보 가독성 개선 — 팔로워·경과 시간·누적 시청자 추가

## [0.1.12](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.12) — 2026-10-22

- 채팅이 가끔 끊기던 문제 — 멀티라이브 채팅을 자기 세션(ml)으로
- 방송 종료 그림 — 라이브 화면·멀티라이브 패인 오버레이에 꺼진 화면(달·Zz)
- 후원 알림을 영상 위에서 끌어 옮기기 — 위치는 영상 크기 대비 비율로 저장
- 후원 알림을 채팅 보는 패인 위에 — 더 보기 메뉴에 알림 위치 조정
- 후원 알림 위치 초기화 버튼 — 화면·비디오 설정
- 글자 크기·줄 간격 설정이 채팅 디자인마다 먹지 않던 문제
- 채팅 오버레이 글자 크기에 맞춰 닉네임·아바타·이모티콘도 같은 배율로
- 통합 목록도 차단·금지어·후원 필터·이모티콘 표시 설정을 따른다

## [0.1.11](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.11) — 2026-10-22

- 영상 위 간격을 좌우와 같은 8px로, 포커스 서브 줄 끝에 채널 추가 칸
- 상단 바 글자·버튼을 세로 가운데로 — 머리줄 위아래 8px, sticky 끔
- 채널을 빠르게 바꿔도 마지막 채널에 연결 — 연결 요청 순번, 다른 채널이면 이전 연결 바로 끊기

## [0.1.10](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.10) — 2026-10-22

- notarytool submit 이 올린 뒤 죽으면 history 에서 그 제출을 찾아 이어 감
- 선명한 화면 — 치지직 웹 플레이어와 같은 SVG 언샤프 마스크(setSharpen)
- 패인 ⚙ 메뉴에 선명한 화면·모든 패인에 적용·밝기/대비/채도 — 채널마다 기억, 시청 화면도 같은 선명한 화면
- 선명한 화면·화면 보정 종합 기능 테스트(13개)와 4패인 비용 측정 도구 — 숨긴 실행

## [0.1.9](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.9) — 2026-10-21

- 포커스 메인을 뺀 패인과 채팅 테두리에 연한 무작위 색 — 채팅은 선택 패인 색을 따름

## [0.1.8](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.8) — 2026-10-21

- 썸네일 디스크 캐시 — SQLite 색인 + 파일, 종류별 신선도, LRU 크기 상한
- 썸네일 서비스와 cvimg: 프로토콜 — 메모리·디스크 캐시, 오래된 것 바로 내주고 뒤에서 새로 받기
- 모든 썸네일·아바타를 캐시(cvimg:)로, CDN 에는 칸에 맞는 크기만 — 설정에 "썸네일 캐시 비우기"
- 썸네일 캐시 전후 측정·종합 기능 테스트 — perf/thumbnails.md, 측정 도구
- DMG 를 만들 때 Finder 창이 뜨지 않게 — tauri 번들에 CI=true(--skip-jenkins)
- 공증은 올리기(--no-wait)와 상태 묻기(info, 15초마다·최대 30분)로 나눔 — notarytool --wait 가 매번 SIGBUS

## [0.1.7](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.7) — 2026-10-21

- 패인 볼륨 슬라이더·LIVE·아바타가 마우스를 못 받던 문제 — 라이브·멀티라이브 영상 위 볼륨 조절 불가
- 볼륨 슬라이더가 끄는 중 접히던 문제·영상 위 휠 무시·들리는 패인 음소거 불가
- 라이브 → 멀티라이브 넘기기 뒤 볼륨이 제곱으로 줄던 문제(30% → 9%)
- notarytool 이 기다리는 중 시그널로 죽으면 다시 제출(3번까지)

## [0.1.6](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.6) — 2026-10-21

- 종료(⌘Q·재부팅·로그아웃) 때마다 나던 크래시 — 종료 정리 타이머를 런타임 밖에서 만들어 패닉 → abort
- 재부팅 뒤 "로그인됨"인데 실제로는 비로그인 — 시작·401 때 쿠키 세션을 서버에 확인해 만료로 바꾸기
- 로그인·만료·로그아웃 뒤 열린 채팅을 새 신원으로 다시 연결 — 로그인했는데 채팅을 못 보내던 문제

## [0.1.5](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.5) — 2026-10-21

- 기준선 2 — 시작·a~f·스크롤 프레임·채팅 처리량·네트워크·번들 크기(사용자 요청으로 10분 메모리·멀티채팅 g 는 생략), 측정 도구 확장, 시작 시간 표시
- 릴리스 프로필 — fat LTO·codegen-units=1·strip, 실행 파일 35.5→19.4MB(-45%), 앱 36.8→21.2MB
- 알림 주기를 팔로잉 저장소에 알려 화면 진입 재사용 창을 주기에 맞춤
- 시작 때 계정·설정을 함께 불러오고 120ms 지연 제거, 목록 이미지 지연 로드·비동기 디코딩
- 요청 관찰자 — 디버그 로그가 켜졌을 때만 시도마다 경로(쿼리 제외)·상태·실패 종류·시간·크기·캐시 적중
- 디버그 로그 — 설정·CVIEW_DEBUG_LOG=1 로 켜는 정밀 기록(JSON 줄, 10MB×5 순환, 꺼지면 비용 거의 0)
- 화면 수집기·설정 "디버그 로그" 토글과 "로그 폴더 열기"
- analyze-log.py — 디버그 로그를 시작·프로세스·네트워크·화면 프레임·플레이어·채팅 표로(구간 지정 가능)
- 기록 비용 줄이기 — 프로세스 표본은 찾아 둔 PID 만(60초마다 다시 찾기), 프레임은 10초 중 1초만 표본
- 다른 메뉴 뒤에서 소리만 나는 라이브도 360p 상한, PiP 영상은 상한 제외
- 결과 2 — 기준선 2 대비 시작·CPU(로그 끔)·스크롤·채팅·메모리·네트워크·번들 크기, 디버그 로그 구간 표, 숨은 재생 시나리오(h)

## [0.1.4](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.4) — 2026-10-20

- 웹뷰 CSP, open_path 를 미디어 파일로 제한, Windows 링크 열기에서 셸 제거 (S-1~3)
- 라이브↔멀티 넘기기·시청 화면 닫기/열기·채팅 연결 경합, 기록 전체 삭제 확인·북마크 되돌리기 (J-1~3, D-1~2, J-8~9)
- 낡은 상태가 재사용 슬롯을 멈추던 경합 막기(상태 순번) + 첫 프레임 감시 (M-1, P-2 일부)
- 단축키 — ⌃⌘ 조합·Ctrl+Tab 이 눌리지 않던 것, 안내 문구를 Windows 표기로 (W-1, W-2)
- 전역 초점 링 되살리기, 라이트 테마 대비, 이름 없는 버튼, 접근성 감시 비용 (A-1~3, P-1)
- 실패 화면 — 쉬운 안내·"다시 시도"·원문 접기 (E-1)
- 클립 소리 남음, 멀티채팅 중복 추가·패널 누수·통합 배치 저장, 채팅 창 채널 전환 경합 (J-4~7, J-10)
- 보조 창을 OS 로 닫아도 채팅 세션 정리, 손상된 저장소 복구 (R-1, R-2)
- 측정용 평가 훅(CVIEW_DEBUG_EVAL)이 앞 커밋에 잘못 들어간 것 제거
- 종료 3초 상한, 원자적 저장, 비밀 금고 원본 백업, POST 재시도 금지, 프록시 출처 제한, 잠금 poison (R-3~7, R-10)
- 숨은 화면 타이머·아이콘 반복 재생성·폴링 중복·패인 화질 자동/라이브 이동 어긋남, 죽은 CSS·줄임표·표기 (P-2·3·5, U-1~5)
- 멀티채팅 검색은 새 줄만, 바로 전환 레일은 한 번 재고 계산 (P-6, P-7)

## [0.1.3](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.3) — 2026-10-20

- Windows 빌드 — macOS 전용 창 꾸밈·트레이 템플릿을 나누고 Windows 설정(tauri.windows.conf.json) 추가
- 화면 — 플랫폼 표시(data-platform), Windows 는 신호등 자리·끌기 띠 걷기, Ctrl 을 ⌘ 로 읽기

## [0.1.2](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.2) — 2026-10-20

- 로고 모션 — 스플래시 등장 연출, 로고 로더·버퍼링, 메뉴 전환, 흐린 로고 빈 화면 (motion.css)
- 로딩 상태·버퍼링 연결 — 로더 문구·스켈레톤 위 로더 줄, 스플래시 최소 0.4초, 영상 대기 이벤트로 버퍼링
- DMG 만들기를 앱 빌드와 나누고 임시 이미지 "리소스 사용 중" 실패 때 강제로 떼어 최대 3번 다시 시도
- DMG 재시도 단계는 app,dmg 로 묶는다 — dmg 만 묶으면 tauri 가 .app 번들을 지워 확인 단계가 실패

## [0.1.1](https://github.com/dododo-lab/cview-rust/releases/tag/v0.1.1) — 2026-10-20

- 첫 배포. Swift 6 macOS 앱(CView v2)을 Rust 워크스페이스와 Tauri 2 데스크톱 앱으로 옮겼다.
  홈·팔로잉·인기·카테고리·검색, 라이브 시청(hls.js 저지연 재생), 멀티라이브, 채팅·멀티채팅, VOD·클립, 보관함, 통계, 설정을 포함한다.
- macOS 판은 Developer ID 서명과 Apple 공증을 거친 DMG로 배포했다.

