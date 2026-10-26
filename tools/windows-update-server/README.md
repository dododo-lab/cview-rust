# Tailscale 외부 PC에서 업데이트 업로드·게시

2026-10-24에 `WIN-7SV7UAK6K5B` (`100.117.247.29`)에 적용한 설정이다.

정식 릴리스 빌드는 이제 `scripts/release-macos.sh --dmg`가 SSH 업로드·게시·검증을 자동으로 수행한다. [자동 게시 안내](AUTOMATION.md)를 우선 사용한다. 아래 SMB 방식은 관리자가 수동 업로드할 때 사용할 수 있다.

## 연결

같은 Tailscale 네트워크에 연결한 뒤 기존 Windows 서버 계정으로 인증한다.

| 항목 | 값 |
| --- | --- |
| Windows 탐색기 주소 | `\\100.117.247.29\CViewUpload$` |
| macOS Finder → 이동 → 서버에 연결 | `smb://100.117.247.29/CViewUpload$` |
| 계정 | `WIN-7SV7UAK6K5B\Administrator` |
| 비밀번호 | 기존 서버 Administrator 비밀번호 |
| 서버 저장 위치 | `C:\CViewUpdates\incoming` |
| 다운로드 사이트 | https://cv.dododo.app/ |

`$`까지 포함해 주소를 입력한다. 공유 목록에서 숨겨지는 전용 공유이며, 공유 권한은 기존 Administrator 계정 하나에만 부여했다. NTFS 권한은 Administrators와 SYSTEM만 허용하고 SMB 암호화를 필수로 적용했다. 새 계정·비밀번호·공유기 포트 개방은 추가하지 않았다. 기존 SMB 서비스와 방화벽 범위를 사용하므로 LAN에서의 기존 접근 가능 범위도 그대로다.

## 업로드

실제 릴리스 버전으로 폴더를 만들고, 완성된 양쪽 운영체제 설치 파일과 UTF-8 릴리스 설명을 넣는다. 아래 `0.1.21`은 예시이며 현재 게시 버전보다 높아야 한다.

```text
\\100.117.247.29\CViewUpload$\0.1.21\
  CView_0.1.21_arm64.dmg
  CView_0.1.21_x64-setup.exe
  notes.txt
```

macOS 서명·공증, Windows 서명 또는 미서명 안내, 앱 내부 버전 일치는 빌드 단계에서 확인한다. 업로드만으로 최신 릴리스가 바뀌지는 않는다.

## 게시

외부 PC에서 `ssh Administrator@100.117.247.29`로 접속해 열린 PowerShell에서 실행한다. [SSH 접속과 원격 게시 명령](POWERSHELL-REMOTE.md)에 자세한 방법이 있다. 원격 데스크톱의 관리자 PowerShell에서도 같은 명령을 사용할 수 있다.

먼저 파일 구성·해시·버전을 확인만 하려면:

```powershell
powershell -NoProfile -File C:\CViewUpdates\tools\Publish-UploadedRelease.ps1 -Version 0.1.21 -ValidateOnly
```

실제 게시하려면:

```powershell
powershell -NoProfile -File C:\CViewUpdates\tools\Publish-UploadedRelease.ps1 -Version 0.1.21
```

도구는 양쪽 설치 파일과 설명 파일을 검사한 뒤 기존 `publish.py`를 실행한다. 이 게시기는 완성된 버전 폴더를 먼저 배치하고 `latest.json`을 마지막에 원자적으로 갱신한다. 이어서 `check_server.py --download`로 HTTPS, 다운로드 페이지, 파일 크기와 SHA-256을 확인한다. 업로드한 원본은 `incoming`에 남는다. 같은 버전 덮어쓰기나 이전 버전 게시를 허용하지 않는다.

## 설치 소스와 확인 결과

- `Install-RemoteUpload.ps1`: 기존 서버를 대상으로 전용 SMB 공유와 게시 보조 도구를 설치한다. `-WhatIf`로 변경 미리보기를 지원한다.
- `Publish-UploadedRelease.ps1`: 서버에 설치된 게시 보조 도구의 원본이다.
- `verification-2026-10-24.json`: 클라이언트·서버 파일 해시, 실제 공유 권한과 설치된 보조 도구 해시를 기록했다.

확인한 항목:

- 실제 Windows PowerShell 5.1에서 두 스크립트 문법 검사 통과, 설치 변경 미리보기와 실제 설치 성공.
- 별도 Mac에서 Tailscale IP를 통한 SMB 연결 성공. 협상 결과 SMB 3.1.1, AES-256-GCM, `ENCRYPTION_REQUIRED=TRUE`.
- 1,048,576바이트 파일 업로드·재읽기 성공, Windows 서버에서 계산한 SHA-256과 일치.
- 업로드 공유 접근 계정 1개와 관리자/SYSTEM 전용 NTFS ACL 확인.
- 설치된 게시 보조 도구에 실제 기존 설치 파일을 제공한 뒤 `-ValidateOnly` 실행: `Version must be newer than 0.1.20.`으로 같은 버전 재게시 차단 확인.
- 기존 `v0.1.20` HTTPS 피드·페이지와 macOS/Windows 설치 파일 2개의 다운로드 및 SHA-256 검사 통과.

검증 파일은 확인 후 제거했다. 이번 작업은 업로드 환경 설정이며 새 앱 릴리스를 게시하지 않았다. Mac과 서버는 같은 물리 LAN에 있었으며, Tailscale IP를 사용한 전송을 검증했다. 다른 인터넷 회선에서의 추가 검증이나 다른 PC의 최초 로그인은 수행하지 않았다.

설정에 사용한 SMB 옵션의 의미는 [Microsoft New-SmbShare 문서](https://learn.microsoft.com/en-us/powershell/module/smbshare/new-smbshare?view=windowsserver2025-ps)에서 확인할 수 있다.
