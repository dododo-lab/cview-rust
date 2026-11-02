# CView 보안 점검 (2026-11-03, v0.1.70 + PR #68 기준)

범위: 업데이터·배포 사슬, Tauri 경계(IPC·CSP·창), 웹뷰 XSS, 로컬 스트림 프록시·네트워크, 의존성, 저장소 비밀 노출.
방법: 코드 정독, 로컬 재현(Node·Rust 단위·루프백 통합 테스트), `cargo audit`, git 기록 패턴 검색(값은 출력·기록하지 않음).
운영 업데이트 서버에는 요청을 보내지 않았다.

심각도: **High** 원격에서 바로 악용 · **Medium** 조건 하나로 악용 · **Low** 조건 여럿 또는 영향 작음 · **Info** 심층 방어·설계 메모.
"확인"은 코드 경로를 따라가 재현한 것, "추정"은 외부 동작(치지직 서버·브라우저)을 가정한 것이다.

## 요약

| # | 심각도 | 영역 | 내용 | 상태 |
|---|---|---|---|---|
| F1 | Medium | 프록시 | 로컬 스트림 프록시가 `Host` 머리를 보지 않아 DNS 리바인딩으로 외부 웹페이지가 프록시를 같은 출처로 부를 수 있음 | **수정** |
| F2 | Low | 채팅 | 채팅 이모티콘 주소의 호스트 제한 없음 — 보낸 사람이 정한 서버로 시청자 IP·시각이 새는 추적 픽셀 | **수정** |
| F3 | Low | IPC | `save_file` 이 아무 확장자나 저장 — XSS 와 엮이면 스크린샷 폴더 설정을 LaunchAgents 로 바꿔 자동 실행 파일을 둘 수 있음 | **수정** |
| F4 | Low | CSP | `frame-src https:` 가 모든 https 프레임 허용(쓰는 곳은 치지직 클립 임베드뿐) | **수정** |
| F5 | Low(잠재) | 웹뷰 | 사용자 카드 오류 문구가 이스케이프 없이 `innerHTML` 로 | **수정** |
| F6 | Low | 저장소 | 공개 저장소 git 기록에 tailnet 호스트 이름·내부 IP·서버 호스트 이름·관리자 계정 이름·공개 키가 남음(HEAD 에서는 #51 로 제거) | 제안 |
| F7 | Info | 창 | 메인·보조 창에 `on_navigation` 가드 없음 | 제안 |
| F8 | Info | 업데이트 | 서명 키·SSH 키가 릴리스 Mac 에 암호 없는 파일(0600) | 제안 |
| F9 | Info | 업데이트 | 동결(freeze) 공격 — 서버가 옛 서명 피드를 계속 주면 새 버전을 모름 | 제안 |
| F10 | Info | 광고 | 광고 URL SSRF 검사가 리다이렉트 뒤 주소는 보지 않음 | **개선**(리다이렉트) · 이름 해석은 제안 |
| F11 | Info | 업데이트 | 서명 없는 피드도 "다운로드" 단추(수동 설치)는 경고 없이 보임 | **개선**(경고 문구) |

High 는 없다. 업데이트 서명·해시·설치 경로에서 우회는 찾지 못했다.

## 상세

### F1 [Medium] 로컬 프록시 DNS 리바인딩 — 수정

- 근거: `crates/cview-player/src/proxy/server.rs` `handle` — 루프백 피어·`Origin`(앱 출처 또는 없음)만 검사했다.
  `Origin` 이 없는 요청은 받는다(영상 요소·앱 내부 녹화용).
- 재현 경로(추정 포함): 사용자가 브라우저로 공격자 페이지 `http://rebind.attacker:PORT/` 를 연다 → 이름을 127.0.0.1 로 다시 풀게 한다
  → 같은 출처 `fetch('/…')` 는 `Origin` 을 보내지 않으므로 통과 → 프록시가 허용 CDN·`api.chzzk.naver.com` 키 URL 을 대신 받아
  응답(`Access-Control-Allow-Origin: *`)을 공격자 페이지가 읽는다. 키 URL 에는 로그인 쿠키가 붙으므로 로그인 사용자 권한으로 받은
  라이브 AES 키·재생 목록이 샌다. 쿠키 값 자체는 새지 않는다. 포트는 무작위라 훑어야 하고, 브라우저의 사설망 접근 제한(PNA)이 일부
  막을 수 있다(추정).
- 수정: `host_allowed` — `Host` 이름이 `127.0.0.1`·`localhost`·`[::1]`(포트 숫자) 일 때만 처리, 아니면 403.
  `Host` 가 없는 요청(브라우저가 아닌 로컬 도구)은 그대로 받는다.
- 테스트: `origin_tests::only_loopback_host_names`, `proxy::tests::rebinding_host_header_is_forbidden`(실제 루프백 연결로 403 확인).

### F2 [Low] 채팅 이모티콘 추적 픽셀 — 수정

- 근거: `app-ui/js/dom.js` `renderContent` — `{:name:}` 를 메시지 `extras.emojis[name]` 주소의 `<img>` 로 바꾸며 `safeUrl`(http/https)만 봤다.
  `imageSrc` 는 네이버 호스트가 아니면 원래 주소로 직접 불러온다. CSP `img-src https:` 도 허용한다.
- 재현: 메시지 `{:x:}` + `emojis = {"x": "https://attacker.example/p.png?id=…"}` → 그 채팅을 보는 CView 사용자 모두가 공격자 서버에
  요청(IP·시각 노출). Node 로 확인(`renderContent` 출력에 공격자 주소 그대로). 치지직 서버가 보낸 사람이 만든 `emojis` 를 그대로
  중계하는지는 **추정**.
- 수정: `emojiUrl` — https + 네이버 이미지 호스트(`pstatic.net`·`akamaized.net`·`naver.net`·`naver.com`)만. 그 밖은 원문 텍스트로.
  `isEmoteOnly`·채팅에서 모은 대체 이모티콘 팩(`seenEmoji`)도 같은 기준. 상속 속성(`{:constructor:}`)을 이모티콘으로 보던 화면 버그도 함께 고침.
- 테스트: `node tools/ui-regression/security-helpers.mjs`.

### F3 [Low] `save_file` 확장자 무제한 — 수정

- 근거: `crates/cview-app/src/commands/content.rs` `save_file` — 이름은 경로 구분자만 정리하고 확장자는 무엇이든, `kind` 도 무엇이든 받았다.
  `kind="screenshot"` 은 설정 `screenshotPath`(웹뷰가 `settings` 명령으로 바꿀 수 있음) 폴더에 쓴다.
- 재현 경로(웹뷰 XSS 가 먼저 있어야 함): `settings_update(screenshotPath="~/Library/LaunchAgents")` → `save_file("screenshot", "x.plist", text=…)`
  → 다음 로그인 때 launchd 가 실행. 덮어쓰기는 안 됨(`create_new`).
- 수정: 종류별 확장자 허용 목록 — screenshot: png/jpg/jpeg/webp, chat: txt/json/csv, settings: json. 모르는 종류는 거절.
  앱이 실제로 쓰는 호출(스크린샷 png, 채팅 txt, 설정 json)은 그대로 된다.
- 테스트: `commands::content::tests::save_file_only_allows_known_kinds_and_extensions`.

### F4 [Low] CSP `frame-src https:` — 수정

- 근거: `crates/cview-app/tauri.conf.json`. 원격 프레임은 클립 임베드(`app-ui/js/media.js`, `https://chzzk.naver.com/embed/clip/…`) 하나다.
  HTML 주입이 생기면 임의 https 피싱 프레임을 앱 안에 띄울 수 있었다(원격 프레임에는 IPC 권한이 없다).
- 수정: `frame-src 'self' https://chzzk.naver.com`.
- 미검증: 클립 재생 화면 실제 확인은 릴리스 빌드 설치 뒤 확인 필요.

### F5 [Low·잠재] 사용자 카드 오류 문구 — 수정

- 근거: `app-ui/js/chat-user-pop.js` — `state.error = e.message` 를 그대로 `pop.innerHTML` 에. 지금 오류는 Rust 고정 문구뿐이라 악용 불가(확인).
  서버 문구가 오류에 실리게 바뀌면 바로 주입 경로가 된다.
- 수정: `esc(state.error)`.

### F6 [Low] git 기록의 서버 정보 — 제안

- 근거: `git log --all -p` 에 tailnet MagicDNS 이름(10개 커밋), Tailscale 대역 IP 가 든 `authorized_keys` 제한 줄, Windows 서버 호스트 이름·
  관리자 계정 이름, SSH 공개 키·호스트 키가 남아 있다. 2026-10-31 #51 로 HEAD 에서는 뺐다. 저장소는 공개.
  개인 키·비밀번호·쿠키·토큰 값은 기록 전체에서 찾지 못했다(검색한 `NID_AUT=`·`password=` 는 테스트용 자리 표시 값).
- 영향: 모두 tailnet 안에서만 쓸모 있고, 업로드 키는 `from=` 출발지·`command=` 로 묶여 있다. 정찰 정보 수준.
- 제안: (a) 그대로 두고 tailnet ACL·키 제한을 유지(권장 — 기록 재작성은 모든 클론·PR 참조를 깨뜨린다) 또는
  (b) `git filter-repo` 로 재작성 후 강제 푸시·GitHub 캐시 정리 요청. (b)는 되돌릴 수 없어 사용자 결정 필요.
  서버 호스트 이름은 바꿀 수 있다면 바꾸는 것도 방법.

### F7 [Info] 창 탐색 가드 없음 — 제안

- 메인·보조 창은 `on_navigation` 이 없어, HTML 주입이 생기면 `<meta http-equiv=refresh>`·링크로 창 자체를 원격 페이지로 옮길 수 있다(추정).
  옮겨 간 원격 페이지는 capability 에 없어서 IPC 는 못 쓰지만, 앱 창 모양의 피싱이 된다.
- 제안: 내부 플러그인의 `on_navigation` 으로 `login` 창 외에는 앱 출처(`tauri://localhost`, `http(s)://tauri.localhost`)와
  클립 임베드 프레임만 허용. iframe 탐색에도 불리는지 플랫폼별 확인이 필요해 이번에는 넣지 않았다.

### F8 [Info] 릴리스 키 보관 — 제안

- `~/.config/cview-release/update_signing_ed25519.pem`(피드 서명 키)은 암호 없는 PEM, 0600 검사만 한다(`scripts/publish-update.py`).
  이 Mac 사용자 권한을 얻으면 서명된 악성 업데이트를 만들 수 있다(Windows 설치 파일은 코드 서명이 없어 이 서명이 유일한 출처 증명).
- 제안: 키를 Keychain(접근 시 확인) 또는 암호화 PEM 으로, 오프라인 백업과 교체 절차(`update-signing-keys.txt` 에 다음 키 미리 넣기)를 문서화.

### F9 [Info] 동결·롤백

- 다운그레이드는 막힌다: 앱은 피드 버전이 현재보다 클 때만 설치하고(`updater.rs` `install`), 게이트웨이도 더 낮은·같은 버전 게시를 거절한다.
- 서버가 옛(정상 서명된) 피드를 계속 주면 앱은 새 버전이 없다고 본다. 서명에 만료 시각이 없어서다. 영향은 보안 업데이트 지연.
- 제안: 필요하면 서명 대상에 `expires` 를 넣는 v2 형식(앱·게이트웨이 동시 변경).

### F10 [Info] 광고 URL SSRF 검사

- `crates/cview-networking/src/client_ads.rs` `is_public_ad_url` 은 URL 문자열의 호스트만 본다. 공용 이름이 사설 IP 로 풀리거나
  공용 서버가 사설 주소로 리다이렉트하면 통과할 수 있다(추정). 응답은 VAST(`<` 로 시작)만 웹뷰로 돌아오고, 쿠키는 붙지 않는다.
- 개선: 공용 HTTP 클라이언트(`crates/cview-networking/src/transport.rs`)에 리다이렉트 정책 — http(s) 이고 루프백·사설·링크 로컬·CGNAT
  리터럴 주소·`localhost`·`.local` 이 아닌 곳만 따른다(최대 10회). 테스트 `transport::tests::redirects_only_to_public_http_hosts`.
- 남은 제안: 이름이 사설 IP 로 풀리는 경우(DNS 리바인딩)는 연결 단계 검사가 필요하다. 다만 이 Mac 처럼 프록시·VPN 의 가짜 IP(fake-ip,
  198.18.0.0/15 등)를 쓰는 환경을 깨뜨릴 수 있어 넣지 않았다.

### F11 [Info] 서명 없는 피드의 수동 다운로드

- 서명이 틀리면 자동 설치는 막히지만 "다운로드" 단추는 피드의 https 주소를 그대로 연다. 서버가 털리면 사용자가 직접 받는 파일은
  보호되지 않는다(macOS 는 Gatekeeper·공증이 막고, Windows 는 SmartScreen 뿐).
- 개선: `update_check` 결과에 `signed` 를 넣고, 서명을 확인하지 못한 새 버전이면 업데이트 시트에 경고 문구를 보인다
  (`app-ui/js/update-check.js`). 단추 자체는 남긴다(서명 도입 전 피드·키 교체 사고 때 사용자가 갇히지 않게).

## 확인하고 문제없다고 본 것

**업데이터** (`crates/cview-app/src/updater.rs`, `system.rs`)
- 피드는 앱이 다시 받는다(프런트 주소 불신). https 만, 인증 정보·조각 있는 주소 거절. 다운로드도 `https_only`.
- ed25519 서명 대상: 버전·릴리스 페이지·노트 SHA-256·모든 자산(이름·크기·SHA-256·주소). 이름순 정렬, 공백·제어 문자 필드 거절
  → 필드 경계 조작 불가. 키 ID 는 공개 키 SHA-256 앞 8바이트, 내장 키 목록에서만 찾는다. 서명 없는 피드는 자동 설치 안 함.
- 버전은 `x.y.z` 숫자만 경로에 쓴다(경로 조작 불가). 크기 초과·부족·해시 불일치면 `.part` 를 지우고 멈춘다.
- macOS: 해시 확인한 DMG 를 읽기 전용으로 붙여 복사한 **복사본**에 `codesign --verify --deep --strict` + 팀·번들 요구 사항 + `spctl` + 버전 확인 뒤 교체
  → 검증한 것과 설치하는 것이 같다. Windows: 해시 확인한 파일을 앱 캐시(사용자 전용)에서 바로 실행 — 같은 사용자 권한의 공격자만 바꿀 수 있다.
- 자산 선택: x64 기기에 arm64 판을 고르지 않는다. 선택된 자산도 서명 대상에 있어 바꿔치기 불가.

**게시** (`scripts/publish-update.py`, `tools/windows-update-server/release_gateway.py`)
- SSH: 고정 known_hosts·`StrictHostKeyChecking=yes`·`IdentitiesOnly`·`ssh-ed25519` 만, `-F /dev/null`. 키 파일 권한 검사.
- 게이트웨이: `SSH_ORIGINAL_COMMAND` 고정, 머리 8 KiB 제한, 버전 정규식, 파일 이름은 고정 목록, 링크·접합점 거절, 크기·해시 재검증,
  게시된 버전 불변, 잠금. 서버는 서명을 검증하지 않지만(키 없음) 앱이 검증하므로 서버 탈취로는 설치를 바꿀 수 없다.
- 노트: Tailscale 필요 문구 거부 검사(`check_notes`), 앱에서는 `<pre>` 에 `esc`.

**Tauri 경계**
- capability `default` 는 로컬 창 라벨만. 로그인 창(원격 `nid.naver.com`)에는 IPC 권한이 없다. 원격 URL 은 `remote` 설정이 없어 IPC 불가.
- CSP `script-src 'self' 'unsafe-eval'` — 인라인 스크립트·`on*` 핸들러 실행 불가. `unsafe-eval` 은 플레이어 브리지(`win.eval`)용, 문자열은 Rust 가
  JSON 리터럴로 만든다.
- `open_external`: http(s) 만, Windows 는 셸 없이 `rundll32 url.dll`. `open_path`: 일반 파일 + 미디어·텍스트 확장자만.
  `reveal_path`: 존재하는 경로를 Finder/탐색기에 보이기만(실행 안 함).
- 딥 링크는 정해진 종류만 파싱해 프런트로 넘긴다.

**웹뷰 XSS** — `app-ui/js` 의 HTML 템플릿 보간 약 780곳을 추출해 따라갔다. 채팅(닉네임·배지·칭호 색 정규식)·후원·멀티채팅·검색·홈·채널·
사이드바·VOD/클립 메타·설정·업데이트 노트·새로고침 덮개(상수 SVG) 모두 `esc`/`safeUrl`/`imageSrc`/정규식/상수로 처리돼 있다.
`esc` 는 `& < > " '` 를 바꿔 따옴표 속성에서도 안전하고, 따옴표 없는 속성 보간은 없다. 확인된 XSS 는 없다(F2·F5 는 위).

**네트워크**
- 프록시: 127.0.0.1 바인딩, 루프백 피어만, 앱 출처만, 상류 호스트 허용 목록(네이버·Akamai), 리다이렉트도 허용 목록, 로그인 쿠키는
  `api.chzzk.naver.com/service/v1/encryption/lives/<숫자>/aes_key` GET 에만.
- TLS 검증을 끄는 코드 없음. 쿠키·토큰은 OS 자격 증명 저장소(Keychain·자격 증명 관리자)에, 실패 시 평문 폴백 없음.
  로그에는 쿠키 개수·성공 여부만 남고 값은 남지 않는다.

**의존성**
- `cargo audit`(advisory DB 1,402건, 2026-11-03): 취약점 0. 경고 6 — `glib`(unsound)·`zbus_names`·`zvariant*`(관리 중단)는 Linux 전용
  경로(macOS·Windows 대상 `cargo tree` 에 없음), `proc-macro-error`(빌드 매크로), `async-executor` yanked. 조치 불필요, 다음 Tauri 갱신 때 따라옴.
- `cargo-deny` 는 설치돼 있지 않아 돌리지 않았다(라이선스·중복 검사용, 보안 판단에는 audit 로 충분).
- hls.js 1.7.4-cview.2: 번들 SHA-256 이 `third_party/hlsjs/integrity.json` 과 같다. 로컬 패치 4개는 라이브 동기·재로드·오류 복구 로직만 건드린다.
  JS 패키지 의존성은 없다(바닐라 JS, 번들 하나).

## 검증

- `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, `cargo test --workspace --locked`
- `node --check app-ui/js/*.js`, `node tools/ui-regression/security-helpers.mjs`
- 미검증: 실제 앱에서 클립 임베드(F4)·이모티콘 표시(F2)·스크린샷/채팅 내보내기(F3)·Windows WebView2 프록시 재생(F1) — 릴리스 빌드 설치 후 확인 필요.
