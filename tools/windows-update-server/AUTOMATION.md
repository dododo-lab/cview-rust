# 릴리스 자동 업로드·게시

같은 Mac의 Codex, Claude Code, Gemini 등은 프로젝트의 `AGENTS.md`와 정식 릴리스 명령을 사용한다. SSH 개인 키는 저장소 밖에 있으므로 AI를 바꿔도 다시 비밀번호를 입력할 필요가 없다. 새 PC에서는 아래의 1회 등록이 필요하다.

## 정식 명령

Tailscale을 연결하고, 변경 사항을 검토·커밋한 뒤 저장소 루트에서:

```sh
scripts/release-macos.sh --dmg
```

이 명령은 다음 작업을 순서대로 처리한다.

1. 업데이트 서버의 전용 키 인증·HTTPS·버전을 확인하고 Apple 공증 프로파일을 확인한다.
2. 프로젝트 검사와 테스트 후 버전·빌드 번호를 올린다.
3. macOS DMG 서명·공증·스테이플 및 Windows x64·arm64 NSIS 설치 파일을 만든다.
4. UTF-8 릴리스 설명을 저장하고 로컬 릴리스 커밋·태그를 만든다.
5. 세 설치 파일과 설명을 SSH로 전송하고 서버에서 저장된 파일을 재읽어 SHA-256을 비교한다.
6. 서버가 완성된 버전 폴더를 배치한 뒤 `latest.json`을 원자적으로 변경한다.
7. 서버와 이 Mac에서 HTTPS 설치 파일 전체를 다운로드해 크기·SHA-256을 확인한다.
8. 검증 영수증을 저장하고 Git 커밋·태그를 원격에 푸시한다.
9. GitHub 릴리스 사본을 만든다(아래 "GitHub 릴리스 사본").

공증 프로파일은 기본 `cview-notarize`이다. 다른 프로파일은 `CVIEW_NOTARIZE_PROFILE`로 지정한다. 사용자용 변경 설명을 따로 준비한 경우:

```sh
CVIEW_RELEASE_NOTES_FILE=/절대/경로/notes.txt scripts/release-macos.sh --dmg
```

설명을 생략하면 이전 태그 이후 Git 변경 목록을 사용한다. Windows 설치 파일은 현재 미서명이며 SmartScreen 안내를 자동 추가한다. `--no-notarize`는 사용자가 명시적으로 공증 생략을 허용할 때만 사용한다. `CVIEW_SKIP_WINDOWS`는 배포에서 거부하고, Windows arm64만 빼고 게시하는 경로도 없다. 개발용 빌드에는 자동 게시를 적용하지 않는다.

### Windows 교차 빌드 도구

| 설치 파일 | Rust 타깃 | 도구 |
| --- | --- | --- |
| `CView_<버전>_x64-setup.exe` | `x86_64-pc-windows-gnu` | Homebrew `mingw-w64`, `nsis` |
| `CView_<버전>_arm64-setup.exe` | `aarch64-pc-windows-gnullvm` | [llvm-mingw](https://github.com/mstorsjo/llvm-mingw)(clang·lld·llvm-windres·aarch64 CRT), `nsis` |

Homebrew `mingw-w64`에는 aarch64가 없어 arm64는 llvm-mingw를 쓴다. 기본 위치는 `~/.local/opt/llvm-mingw`(macOS universal 릴리스를 풀고 이 이름으로 링크)이며 `CVIEW_LLVM_MINGW`로 바꾼다. 스크립트는 PATH를 바꾸지 않고 `CARGO_TARGET_AARCH64_PC_WINDOWS_GNULLVM_LINKER`, `CC_`·`AR_`·`RC_aarch64_pc_windows_gnullvm`으로 절대 경로만 넘긴다(macOS 빌드의 clang을 가리지 않게). 도구가 하나라도 없으면 버전을 올리기 전에 무엇을 설치할지 알려 주고 멈춘다.

NSIS 설치 프로그램은 두 판 모두 x86 실행 파일이다. 스크립트는 번들 전 `cview-app.exe`의 PE Machine(x64 `0x8664`, ARM64 `0xaa64`)을 확인하고, `publish-update.py`는 두 Windows 설치 파일이 PE이고 서로 다른지 확인한다. arm64 판 WebView2 로더는 `webview2-com-sys`의 arm64 정적 라이브러리를 쓴다. gnullvm 타깃은 기본으로 `libunwind.dll`을 동적으로 부르는데 Windows에는 없으므로 `-C target-feature=+crt-static`로 정적 링크하고, 빌드 후 `llvm-objdump`로 `lib*.dll`(MinGW 런타임) 가져오기가 없는지 확인한다.

## GitHub 릴리스 사본

설치 파일의 원본은 업데이트 서버이고, GitHub 릴리스에는 같은 파일을 올린다. 앱의 자동 업데이트는 GitHub 피드를 먼저 쓴다.

| 순서 | 피드 | 설치 파일 |
| --- | --- | --- |
| 1 | `https://github.com/dododo-lab/cview-rust/releases/latest/download/latest.json` | `https://github.com/dododo-lab/cview-rust/releases/download/v<버전>/…` |
| 2 (1을 받지 못하거나 404) | `https://cv.dododo.app/releases/latest.json` | `https://cv.dododo.app/releases/<버전>/…` |

`CVIEW_UPDATE_URL`(실행 시 또는 빌드 시)을 정하면 그 주소 하나만 쓴다. GitHub 다운로드는 `release-assets.githubusercontent.com`으로 리다이렉트되며 앱은 HTTPS 리다이렉트만 따른다. 이 기능 이전 버전 앱은 자체 서버 피드만 보므로 업데이트 서버 게시를 계속한다.

```sh
python3 scripts/github-release.py --version <버전>            # release-macos.sh --dmg 마지막 단계
python3 scripts/github-release.py --version <버전> --draft    # 초안으로만 올리고 확인
python3 scripts/github-release.py --version <버전> --verify-only
```

1. `https://cv.dododo.app/releases/<버전>/manifest.json`을 받아 저장소 공개 키로 ed25519 서명을 확인한다. 이 manifest의 이름·크기·SHA-256이 기준이다.
2. `v<버전>` 태그가 origin에 있어야 한다(새 태그를 만들지 않는다).
3. `dist/`의 파일이 manifest와 같으면 쓰고, 없으면 업데이트 서버에서 받아 확인한다. `dist/` 파일이 다르면 멈춘다.
4. 서버 manifest와 버전·노트·크기·SHA-256이 같고 주소만 GitHub인 `latest.json`을 만들어 이 Mac의 피드 서명 키로 서명한다(Ed25519는 결정적이라 재실행해도 같은 바이트). 초안 릴리스를 만들고 설치 파일 세 개, `CView_<버전>_SHA256SUMS.txt`(`shasum -a 256 -c` 형식), `latest.json`을 올린다. 이미 있는 자산은 덮어쓰지 않으며 크기·digest가 다르면 실패한다.
5. GitHub에서 전체를 다시 받아 크기·SHA-256을 확인한 뒤 게시한다. 업데이트 서버의 최신 버전일 때만 GitHub "Latest"로 표시하고, 그때는 앱이 받는 `releases/latest/download/latest.json`이 이 릴리스 피드인지 확인한다.
   - 저장소에 immutable releases가 켜져 있어 게시 뒤에는 파일을 더하거나 바꿀 수 없다. 게시된 릴리스에 파일이 빠졌으면 다음 버전으로 배포한다(v0.1.73은 `latest.json` 없이 게시되어 그동안 앱은 자체 서버 피드를 쓴다).
6. 게시(`release: published`)되면 Actions `github-release` 워크플로가 `--verify-only`로 GitHub 자산을 서버 manifest와 다시 대조하고 결과를 요약에 남긴다. 예전 태그는 Actions에서 `workflow_dispatch`(태그 입력)로 검사한다.

빌드·서명·공증은 이 Mac에서만 하므로 Apple 인증서·공증 자격·피드 서명 키·SSH 키를 GitHub Secrets에 두지 않는다. GitHub 단계가 실패해도 이미 끝난 서버 게시와 푸시는 그대로이며 같은 명령을 재실행하면 된다. 결과는 `dist/CView_<버전>_github-receipt.json`.

이 저장소의 Actions 설정은 `sha_pinning_required`라 액션을 커밋 SHA로 고정해야 한다. 태그(`@v4`)만 쓰면 작업이 시작되지 않고 startup_failure로 끝난다.

## 앱 안 자동 설치와 피드

앱의 "업데이트 설치"는 `latest.json`의 서명을 먼저 확인하고, 이 OS 설치 파일(`.dmg`, `-setup.exe`)을 받아 피드의 `size`·`sha256`과 비교한 뒤 설치한다. 서명이 없거나 맞지 않는 피드, `sha256`이 없는 피드면 앱은 자동 설치 대신 다운로드 단추를 보인다.

- macOS: DMG를 화면에 띄우지 않고 붙여 `CView.app`을 복사하고, Developer ID 팀 `MZ8NB937SB`·번들 `app.cview.rust` 서명(`codesign --test-requirement`), Apple 공증(`spctl`), 버전을 확인한 뒤 교체·재실행한다. `--no-notarize`로 만든 릴리스는 공증 검사에서 멈추므로 자동 설치되지 않는다.
- Windows: NSIS 설치 파일을 `/P /R /UPDATE`로 실행한다(진행 창만 표시, 앱 종료·설치·재실행). 설치 파일 미서명이라 SHA-256만 확인한다.
- Windows 아키텍처: 앱은 자기 빌드 아키텍처에 맞는 `_arm64-setup.exe`·`_x64-setup.exe`를 고른다. ARM64 앱은 피드에 arm64 판이 없으면 x64 판(에뮬레이션)으로 대신한다. x64 앱은 arm64 판을 고르지 않는다. ARM 기기에서 에뮬레이션으로 도는 기존 x64 앱은 x64로 보고하므로 계속 x64 판을 받는다(자동으로 arm64로 옮기지 않는다).
- 이 기능은 이를 포함한 버전부터 동작한다. 그 이전 버전 사용자는 한 번은 직접 설치해야 한다.

### 피드 서명

Windows 설치 파일이 미서명이므로, 업데이트 서버가 침해돼도 설치 파일을 바꿔치기하지 못하게 피드에 ed25519 서명을 싣는다.

- 개인 키: 릴리스 Mac의 `~/.config/cview-release/update_signing_ed25519.pem`(권한 600, `CVIEW_UPDATE_SIGNING_KEY`로 변경). 업데이트 서버에는 두지 않는다. 오프라인 백업을 따로 보관한다.
- 공개 키: 저장소 `crates/cview-app/update-signing-keys.txt`. 앱은 빌드 때 이 목록을 넣고, 목록의 키로 서명된 피드만 자동 설치한다.
- 서명 대상(`release_gateway.signed_payload`, 앱 `updater::signed_payload`와 같은 형식): 버전, 릴리스 페이지 주소, 노트 SHA-256, 자산마다 이름·크기·SHA-256·다운로드 주소.
- `publish-update.py`가 게시 전에 서명하고 게이트웨이에 `signature`로 넘긴다. `publish.py`는 이를 `latest.json`의 `signature`에 그대로 적는다. 게시 후 HTTPS 피드의 서명을 저장소 공개 키로 다시 검증한다.
- 서명에는 OpenSSL 3 이상이 필요하다(`brew install openssl`, 또는 `CVIEW_OPENSSL`). macOS 기본 LibreSSL은 쓰지 않는다.
- `--preflight`는 게이트웨이가 `signed-feed-v1`·`windows-arm64-v1`을 지원하는지, 서명 키가 있고 저장소 공개 키 목록에 있는지 확인한다. 하나라도 빠지면 릴리스를 시작하지 않는다.

처음 한 번(키 생성):

```sh
python3 scripts/publish-update.py --init-signing-key
```

출력된 공개 키 줄을 `update-signing-keys.txt`에 추가해 커밋한다. 기존 키는 덮어쓰지 않는다.

키 교체: 새 공개 키를 목록에 추가한 버전을 먼저 배포하고, 대부분이 업데이트한 뒤 서명 키를 바꾸고, 그다음 옛 키 줄을 지운다. 개인 키가 유출되면 즉시 새 키를 추가하고 옛 키를 지운 버전을 배포한다(옛 키를 믿는 기존 앱은 그 버전을 받을 때까지 위험하다).

예전 앱(서명 검증 전 버전)은 `signature`를 무시하고 SHA-256만 확인한다. 서명 검증을 넣은 첫 버전으로 한 번 업데이트한 뒤부터 보호된다.

## 공개 다운로드 경로

앱의 업데이트 확인·다운로드는 Tailscale 없이 공개 인터넷으로 동작한다. Tailscale은 이 Mac에서 서버로 올리는 SSH 업로드·게시에만 쓴다.

| 경로 | 주소 | 공개 범위 |
| --- | --- | --- |
| 피드(예비, 이전 버전 앱의 기본) | `https://cv.dododo.app/releases/latest.json` | 공개 DNS(A 레코드)·공유기 443 포워딩·Let's Encrypt 인증서 |
| 설치 파일·릴리스 페이지 | `https://cv.dododo.app/releases/<버전>/` | 위와 같음 |
| 업로드·게시(SSH) | 로컬 설정 `server.json`의 Tailscale 주소 | tailnet만(방화벽 `100.64.0.0/10`, 키 출발지 제한) |

무결성: 피드는 HTTPS로만 받고(앱이 `https_only`, 인증 정보 없는 HTTPS 주소만 허용), 설치 파일은 피드의 `size`·`sha256`과 비교한다. macOS는 그 위에 Developer ID 팀 서명과 Apple 공증(`spctl`)을 확인한다. Windows 설치 파일은 미서명이라 피드의 SHA-256과 위 "피드 서명"(ed25519)으로 확인한다. 공개 경로를 쓰더라도 서명 키는 업데이트 서버에 두지 않으므로 서버 침해만으로는 설치 파일을 바꿔치기할 수 없다.

릴리스 Mac은 서버와 같은 공유기 뒤에 있어(같은 공인 IP, 헤어핀 NAT) `publish-update.py`의 다운로드 검증은 외부 공개 여부를 증명하지 않는다. 공유기·방화벽·DNS를 바꾼 뒤나 사용자 보고가 있을 때는 Tailscale을 끈 외부 네트워크에서 확인한다.

```sh
curl -fsS https://cv.dododo.app/health.json
curl -fsS https://cv.dododo.app/releases/latest.json | python3 -m json.tool
```

앱에서는 설정의 업데이트 확인이 최신 버전을 보여 주고, 새 버전이 있을 때 "업데이트 설치"가 다운로드·검증까지 진행되면 된다. 공인 IP가 바뀌면 DNS A 레코드도 바뀌어야 외부 사용자가 접속할 수 있다.

## 사전 점검과 전송 시험

```sh
python3 scripts/publish-update.py --preflight
python3 scripts/publish-update.py --probe
```

`--probe`는 1 MiB 임시 파일을 실제 업로드·재읽기·해시 검사 후 삭제하고, 현재 게시된 설치 파일(arm64 추가 전 릴리스는 두 개, 이후는 세 개)을 HTTPS로 다운로드해 검증한다. 최신 버전이나 공개 파일을 변경하지 않는다.

빌드된 실제 파일을 업로드·검사만 하려면:

```sh
python3 scripts/publish-update.py --version <버전> --notes-file dist/CView_<버전>_notes.txt --validate-only
```

## 장애 후 재시도

### SSH 키가 맞는데 인증이 거부되는 경우

Mac의 프록시 도구가 서버 SSH 연결을 다른 기기(예: 휴대폰 핫스팟 프록시)로 보내면, 키가 맞아도 서버가 다른 출발지로 보고 거부한다. 서버 OpenSSH 로그에는 `correct key but not from a permitted host`가 남는다.

Mac에서 Proxifier 같은 프록시 도구를 쓰면 다음 규칙을 Default 규칙보다 위에 둔다.

| 항목 | 값 |
| --- | --- |
| 이름 | `CView update SSH (Tailscale)` |
| Applications | Any |
| Target Hosts | 업데이트 서버의 Tailscale 주소(로컬 설정 `server.json`의 SSH 대상) |
| Target Ports | `22` |
| Action | Direct |

이 규칙은 SSH를 Mac의 Tailscale 연결로 전달한다. 기존 배포 키와 서버의 Mac 출발지 제한을 유지한다. Proxifier 프로필을 바꾸거나 가져온 뒤에는 이 규칙과 순서를 확인하고 `--preflight`, `--probe`를 실행한다. `tailscale status`에 표시된 Mac 주소만으로 서버가 관측한 출발지까지 확인한 것으로 판단하지 않는다.


### 게시 작업 재시도

빌드·공증 단계 실패 시에는 아직 게시하지 않았으므로 버전 변경을 되돌린다. 로컬 릴리스 커밋이 만들어진 이후에는 버전과 파일을 보존한다. SSH 응답 유실이나 다운로드 검사 실패가 있어도 이미 서버에 게시됐을 수 있기 때문이다.

```sh
python3 scripts/publish-update.py --version <버전> --notes-file dist/CView_<버전>_notes.txt
```

서버의 같은 버전·설치 파일·설명이 모두 일치하면 덮어쓰기 없이 다운로드를 다시 검증한다. 다르면 실패한다. 공증을 명시적으로 생략한 기존 릴리스만 같은 재시도 명령에 `--allow-unnotarized`를 붙이고 설명의 공증 생략 안내를 유지한다.

검증 성공 후 Git 푸시가 남았다면:

```sh
git push origin HEAD
git push origin v<버전>
```

성공 기록은 `dist/CView_<버전>_publish-receipt.json`이다. 파일 크기와 해시·피드·검증 시각을 기록하며, 실제 설치·실행 검증을 대신하지 않는다.

다른 파일로 충돌한 `incoming\<버전>`은 자동으로 덮어쓰거나 삭제하지 않는다. 관리자 계정으로 내용과 사용 중 여부를 확인한 뒤 처리한다. 더 높은 버전이 이미 게시된 경우에는 예전 버전을 재게시하지 않는다.

## 인증과 서버 구성

- 서버 주소·이름·호스트 키는 저장소에 두지 않고 각 Mac의 로컬 설정에 둔다(아래 "로컬 서버 설정"). 업로드에는 Tailscale 연결 필요(사용자 다운로드에는 불필요).
- 자동화 키는 등록된 Mac의 Tailscale 주소에서만 쓸 수 있다.
- 개인 키: `~/.ssh/cview_release_ed25519`, 소유자만 읽기·쓰기(0600). 암호 없는 자동화 전용 키이며 저장소에는 포함하지 않는다.
- 다른 키 위치: `CVIEW_RELEASE_SSH_KEY=/절대/경로/key`.
- 서버 호스트 키는 로컬 설정의 `known_hosts`로 고정한다(`StrictHostKeyChecking=yes`).
- 서버 `administrators_authorized_keys`의 이 키에는 `restrict`, 출발지 제한, 고정 게시 명령을 적용했다. 일반 PowerShell 명령·SFTP·PTY·포트 전달에 사용할 수 없다. 키를 가진 이 Mac 사용자는 새 업데이트를 게시할 수 있으므로 키를 공유하지 않는다.
- 서버 명령: `C:\CViewUpdates\tools\release_gateway.py`. 허용 작업은 사전 점검, 전송 시험, 지정된 버전의 세 설치 파일(macOS arm64, Windows x64·arm64)·설명 전송, 검증, 게시뿐이다.
- 게이트웨이 갱신: 저장소의 `release_gateway.py`·`publish.py`·`check_server.py`·`Publish-UploadedRelease.ps1`을 관리자가 `C:\CViewUpdates\tools\`에 복사한다. 클라이언트는 사전 점검에서 게이트웨이가 알리는 기능(`signed-feed-v1`, `windows-arm64-v1`)을 확인하고, 빠졌으면 `redeploy`를 안내하며 릴리스를 시작하지 않는다. Windows arm64를 넣은 뒤 첫 릴리스 전에 갱신이 필요하다.
- 이 클라이언트는 기존 관리자 SSH 연결을 재사용하지 않고 항상 전용 키를 새로 인증한다. 다른 관리자 비밀번호 접속은 그대로 유지된다.
- Python: `C:\CViewUpdates\tools\python\python.exe`. 공식 Python 3.13.12 embeddable amd64를 전용 폴더에 배치하고 ZIP SHA-256 및 Python Software Foundation Authenticode 서명을 확인했다. 시스템 PATH나 Windows Python 앱 실행 별칭에 의존하지 않는다.
- Python ZIP SHA-256: `76f238f606250c87c6beac75dccd35ee99070a13490555936abb6cb64ecce3d0`.

## 로컬 서버 설정

`scripts/publish-update.py`는 서버 정보를 `~/.config/cview-release/`에서 읽는다(`CVIEW_RELEASE_CONFIG_DIR`로 폴더 변경). 이 폴더는 저장소 밖에 있어 커밋되지 않는다.

```text
~/.config/cview-release/
  server.json    {"ssh_target": "<관리자-계정>@<서버-Tailscale-IP>", "server_name": "<서버 hostname>"}
  known_hosts    <서버-Tailscale-IP> ssh-ed25519 <서버 호스트 공개 키>
```

- `ssh_target`: 게시 전용 키로 접속할 SSH 대상. `server_name`: 사전 점검 때 서버가 돌려주는 hostname과 비교한다.
- `known_hosts`의 호스트 키는 서버 관리자 PowerShell에서 `Get-Content C:\ProgramData\ssh\ssh_host_ed25519_key.pub`로 읽은 값과 맞춰 넣는다. 네트워크로 받은 키를 그대로 믿지 않는다.
- 환경 변수 `CVIEW_RELEASE_SSH_TARGET`, `CVIEW_RELEASE_SERVER_NAME`, `CVIEW_RELEASE_KNOWN_HOSTS`가 있으면 파일보다 우선한다.
- 설정이 없으면 `--preflight`가 무엇이 빠졌는지 알려 주고 멈춘다.

## 다른 PC 1회 등록

새 PC의 SSH 키를 별도로 생성하고 그 공개 키만 서버 관리자가 등록한다. 현재 Mac의 개인 키를 복사하지 않는다. 해당 PC는 같은 Tailscale 네트워크, macOS 빌드·서명·공증 환경, 위 로컬 서버 설정이 필요하다.

```sh
ssh-keygen -t ed25519 -N '' -C cview-release-automation -f ~/.ssh/cview_release_ed25519
chmod 600 ~/.ssh/cview_release_ed25519
```

서버 관리자는 새 공개 키를 `C:\ProgramData\ssh\administrators_authorized_keys`에 `restrict`, 새 PC의 Tailscale 주소로 출발지 제한(`from=`), 고정 게시 명령(`command=`로 `release_gateway.py` 실행)을 붙여 등록한다. 서버 쪽 설치 스크립트는 공개 저장소에 두지 않고 관리자가 따로 보관한다. 설치 후 새 PC에서 `--preflight`와 `--probe`를 확인한다. 현재 키의 출발지 주소를 바꿀 때는 기존 키 줄을 관리자에게 확인받아 수정해야 한다. 키를 폐기할 때는 `C:\ProgramData\ssh\administrators_authorized_keys`에서 해당 공개 키 줄만 제거한다.

공식 참고: [Windows OpenSSH 키 관리](https://learn.microsoft.com/en-us/windows-server/administration/openssh/openssh_keymanagement), [OpenSSH 공개 키 제한 옵션](https://man.openbsd.org/sshd.8), [Python 3.13.12 배포 파일](https://www.python.org/downloads/release/python-31312/).

