# 변경 기록

CView의 버전별 변경 사항이다. 모든 버전은 macOS(Apple Silicon)와 Windows(x64) 설치 파일이 같은 번호로 함께 배포되었고,
설치 파일은 `https://cv.dododo.app/releases/<버전>/`에서 받을 수 있다. 버전 번호는 [유의적 버전](https://semver.org/lang/ko/)을 따르며,
배포할 때마다 버전과 macOS 빌드 번호가 함께 하나씩 오른다(`scripts/release-macos.sh`).

이 파일은 Git 태그와 커밋 기록에서 정리했다. 새 버전을 배포하면 맨 위에 항목을 더한다.

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

