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
3. macOS DMG 서명·공증·스테이플 및 Windows x64 NSIS 설치 파일을 만든다.
4. UTF-8 릴리스 설명을 저장하고 로컬 릴리스 커밋·태그를 만든다.
5. 양쪽 설치 파일과 설명을 SSH로 전송하고 서버에서 저장된 파일을 재읽어 SHA-256을 비교한다.
6. 서버가 완성된 버전 폴더를 배치한 뒤 `latest.json`을 원자적으로 변경한다.
7. 서버와 이 Mac에서 HTTPS 설치 파일 전체를 다운로드해 크기·SHA-256을 확인한다.
8. 검증 영수증을 저장하고 Git 커밋·태그를 원격에 푸시한다.

공증 프로파일은 기본 `cview-notarize`이다. 다른 프로파일은 `CVIEW_NOTARIZE_PROFILE`로 지정한다. 사용자용 변경 설명을 따로 준비한 경우:

```sh
CVIEW_RELEASE_NOTES_FILE=/절대/경로/notes.txt scripts/release-macos.sh --dmg
```

설명을 생략하면 이전 태그 이후 Git 변경 목록을 사용한다. Windows 설치 파일은 현재 미서명이며 SmartScreen 안내를 자동 추가한다. `--no-notarize`는 사용자가 명시적으로 공증 생략을 허용할 때만 사용한다. `CVIEW_SKIP_WINDOWS`는 배포에서 거부한다. 개발용 빌드에는 자동 게시를 적용하지 않는다.

## 사전 점검과 전송 시험

```sh
python3 scripts/publish-update.py --preflight
python3 scripts/publish-update.py --probe
```

`--probe`는 1 MiB 임시 파일을 실제 업로드·재읽기·해시 검사 후 삭제하고, 현재 게시된 두 설치 파일을 HTTPS로 다운로드해 검증한다. 최신 버전이나 공개 파일을 변경하지 않는다.

빌드된 실제 파일을 업로드·검사만 하려면:

```sh
python3 scripts/publish-update.py --version <버전> --notes-file dist/CView_<버전>_notes.txt --validate-only
```

## 장애 후 재시도

### SSH 키가 맞는데 인증이 거부되는 경우

2026-10-25에는 Mac의 Proxifier가 서버 SSH 연결까지 iPhone SOCKS5 프록시(`172.20.10.1:8443`)로 보내는 문제가 있었다. Mac의 Tailscale 주소와 키 지문은 정상이어도 서버가 본 출발지는 iOS 장치의 `100.123.134.100`이었다. 서버 OpenSSH 로그에는 `correct key but not from a permitted host`와 `required=100.70.172.3`이 기록됐다.

현재 Mac의 Proxifier `iossocks-simple` 프로필에는 다음 규칙을 Default 규칙보다 위에 적용했다.

| 항목 | 값 |
| --- | --- |
| 이름 | `CView update SSH (Tailscale)` |
| Applications | Any |
| Target Hosts | `100.117.247.29` |
| Target Ports | `22` |
| Action | Direct |

이 규칙은 SSH를 Mac의 Tailscale 연결로 전달한다. 기존 배포 키와 서버의 Mac 출발지 제한을 유지한다. Proxifier 프로필을 바꾸거나 가져온 뒤에는 이 규칙과 순서를 확인하고 `--preflight`, `--probe`를 실행한다. `tailscale status`에 표시된 Mac 주소만으로 서버가 관측한 출발지까지 확인한 것으로 판단하지 않는다.

수정 후 비대화형 키 인증, 실제 1 MiB 전송·재읽기·SHA-256·정리, v0.1.24의 두 설치 파일 전체 HTTPS 다운로드 검증을 통과했다. 새 릴리스는 게시하지 않았다. 검증 기록: `access-verification-2026-10-25.json`.

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

- 서버: `Administrator@100.117.247.29:22`, Tailscale 연결 필요.
- 현재 자동화 키 허용 출발지: 이 Mac의 `100.70.172.3`.
- 개인 키: `~/.ssh/cview_release_ed25519`, 소유자만 읽기·쓰기(0600). 암호 없는 자동화 전용 키이며 저장소에는 포함하지 않는다.
- 다른 키 위치: `CVIEW_RELEASE_SSH_KEY=/절대/경로/key`.
- 서버 키는 저장소의 `known_hosts`로 고정한다. 지문: `SHA256:wOg0YZCPhtyxDl/5zsDFaLaj8rbIyNztDYEJTZa0TqA`.
- 서버 `administrators_authorized_keys`의 이 키에는 `restrict`, 출발지 제한, 고정 게시 명령을 적용했다. 일반 PowerShell 명령·SFTP·PTY·포트 전달에 사용할 수 없다. 키를 가진 이 Mac 사용자는 새 업데이트를 게시할 수 있으므로 키를 공유하지 않는다.
- 서버 명령: `C:\CViewUpdates\tools\release_gateway.py`. 허용 작업은 사전 점검, 전송 시험, 지정된 버전의 두 설치 파일·설명 전송, 검증, 게시뿐이다.
- 이 클라이언트는 기존 관리자 SSH 연결을 재사용하지 않고 항상 전용 키를 새로 인증한다. 다른 관리자 비밀번호 접속은 그대로 유지된다.
- Python: `C:\CViewUpdates\tools\python\python.exe`. 공식 Python 3.13.12 embeddable amd64를 전용 폴더에 배치하고 ZIP SHA-256 및 Python Software Foundation Authenticode 서명을 확인했다. 시스템 PATH나 Windows Python 앱 실행 별칭에 의존하지 않는다.
- Python ZIP SHA-256: `76f238f606250c87c6beac75dccd35ee99070a13490555936abb6cb64ecce3d0`.

## 다른 PC 1회 등록

새 PC의 SSH 키를 별도로 생성하고 그 공개 키만 서버 관리자가 등록한다. 현재 Mac의 개인 키를 복사하지 않는다. 해당 PC는 같은 Tailscale 네트워크와 macOS 빌드·서명·공증 환경이 필요하다.

```sh
ssh-keygen -t ed25519 -N '' -C cview-release-automation -f ~/.ssh/cview_release_ed25519
chmod 600 ~/.ssh/cview_release_ed25519
```

관리자 전용 설치 폴더에 이 디렉터리의 `Install-ReleaseAutomation.ps1`, `release_gateway.py`, `publish.py`, `check_server.py`, `Publish-UploadedRelease.ps1`, 새 공개 키와 검증한 Python ZIP을 준비한다. 서버 관리자 PowerShell에서 실제 파일 경로와 새 PC의 Tailscale 주소로 실행한다.

```powershell
.\Install-ReleaseAutomation.ps1 -PublicKeyPath .\cview_release_ed25519.pub -PythonZip .\python-3.13.12-embed-amd64.zip -ClientAddress <새-PC의-Tailscale-IP>
```

설치기는 기존 다른 키를 유지하고 관리자 전용 백업을 남긴다. `-WhatIf`로 변경을 미리 볼 수 있다. 설치 후 새 PC에서 `--preflight`와 `--probe`를 확인한다. 현재 키의 출발지 주소를 바꿀 때는 기존 키 줄을 관리자에게 확인받아 수정해야 한다. 키를 폐기할 때는 `C:\ProgramData\ssh\administrators_authorized_keys`에서 해당 공개 키 줄만 제거한다.

공식 참고: [Windows OpenSSH 키 관리](https://learn.microsoft.com/en-us/windows-server/administration/openssh/openssh_keymanagement), [OpenSSH 공개 키 제한 옵션](https://man.openbsd.org/sshd.8), [Python 3.13.12 배포 파일](https://www.python.org/downloads/release/python-31312/).

## 이번 설정의 검증 범위

- 전용 키를 사용한 비대화형 접속과 HTTPS 사전 점검 성공.
- 실제 서버에 1 MiB 업로드·디스크 재읽기·SHA-256 일치·임시 파일 정리 성공.
- 전용 키로 `hostname` 등 임의 명령 실행 거부, 경로 이탈 요청 거부 확인.
- 서버와 Mac 양쪽에서 기존 v0.1.20의 macOS/Windows 설치 파일 전체 다운로드·SHA-256 검사 성공.
- 격리된 임시 디렉터리에서 게시·원자적 피드·충돌·재시도·실패 처리 테스트 통과.
- 이 설정 작업에서는 새 버전을 빌드하거나 운영 서버에 새 릴리스를 게시하지 않았다. 새 릴리스의 전체 흐름은 다음 배포 실행 때 검증 영수증으로 확인한다.
