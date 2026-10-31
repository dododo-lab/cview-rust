# Tailscale 외부 PC에서 업데이트 업로드·게시

정식 릴리스 빌드는 이제 `scripts/release-macos.sh --dmg`가 SSH 업로드·게시·검증을 자동으로 수행한다. [자동 게시 안내](AUTOMATION.md)를 우선 사용한다. 아래 SMB 방식은 관리자가 수동 업로드할 때 사용할 수 있다.

## 연결

같은 Tailscale 네트워크에 연결한 뒤 기존 Windows 서버 계정으로 인증한다.

| 항목 | 값 |
| --- | --- |
| Windows 탐색기 주소 | `\\<서버-Tailscale-IP>\CViewUpload$` |
| macOS Finder → 이동 → 서버에 연결 | `smb://<서버-Tailscale-IP>/CViewUpload$` |
| 계정 | 서버 관리자 계정 |
| 서버 저장 위치 | `C:\CViewUpdates\incoming` |
| 다운로드 사이트 | https://cv.dododo.app/ |

`$`까지 포함해 주소를 입력한다. 공유 목록에서 숨겨지는 전용 공유이며, 공유와 NTFS 권한은 관리자 계정에만 주고 SMB 암호화를 필수로 적용한다.

## 업로드

실제 릴리스 버전으로 폴더를 만들고, 완성된 양쪽 운영체제 설치 파일과 UTF-8 릴리스 설명을 넣는다. 아래 `0.1.21`은 예시이며 현재 게시 버전보다 높아야 한다.

```text
\\<서버-Tailscale-IP>\CViewUpload$\0.1.21\
  CView_0.1.21_arm64.dmg
  CView_0.1.21_x64-setup.exe
  notes.txt
```

macOS 서명·공증, Windows 서명 또는 미서명 안내, 앱 내부 버전 일치는 빌드 단계에서 확인한다. 업로드만으로 최신 릴리스가 바뀌지는 않는다.

## 게시

외부 PC에서 서버 관리자 계정으로 SSH 접속해 열린 PowerShell에서 실행한다. [SSH 접속과 원격 게시 명령](POWERSHELL-REMOTE.md)에 자세한 방법이 있다. 원격 데스크톱의 관리자 PowerShell에서도 같은 명령을 사용할 수 있다.

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

설정에 사용한 SMB 옵션의 의미는 [Microsoft New-SmbShare 문서](https://learn.microsoft.com/en-us/powershell/module/smbshare/new-smbshare?view=windowsserver2025-ps)에서 확인할 수 있다.
