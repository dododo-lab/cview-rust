# Windows Server PowerShell 원격 접속

2026-10-24 설정: `WIN-7SV7UAK6K5B`, Tailscale IP `100.117.247.29`.

## 접속

같은 Tailscale 네트워크에 연결한 Mac 터미널 또는 Windows 터미널에서 실행한다.

```sh
ssh Administrator@100.117.247.29
```

기존 서버 Administrator 비밀번호를 입력한다. 입력 중 문자가 표시되지 않는 것은 정상이다. 로그인하면 Windows PowerShell 5.1이 열린다. `hostname`으로 서버 이름을 확인하고 `exit`로 종료한다.

최초 접속 시 ED25519 서버 키 지문:

```text
SHA256:wOg0YZCPhtyxDl/5zsDFaLaj8rbIyNztDYEJTZa0TqA
```

서버에서 직접 읽은 공개 키와 네트워크를 통해 받은 키가 일치하는 것을 확인했다. 처음 접속하는 PC에서는 위 지문을 확인한 뒤 신뢰 여부를 선택한다.

한 번만 명령을 실행하려면:

```sh
ssh Administrator@100.117.247.29 'hostname; Get-Service sshd'
```

## 업데이트 원격 게시

기존 `CViewUpload$` 공유의 버전 폴더에 완성된 설치 파일과 `notes.txt`를 업로드한다. 다음 명령은 예시 버전 `0.1.21`의 파일 구성과 버전을 검사한다.

```sh
ssh Administrator@100.117.247.29 'powershell -NoProfile -File C:\CViewUpdates\tools\Publish-UploadedRelease.ps1 -Version 0.1.21 -ValidateOnly'
```

검사 후 실제 게시하려면:

```sh
ssh Administrator@100.117.247.29 'powershell -NoProfile -File C:\CViewUpdates\tools\Publish-UploadedRelease.ps1 -Version 0.1.21'
```

실제 릴리스 버전을 사용해야 한다. 파일 구성은 [업로드·게시 안내](README.md)를 따른다.

## 적용 내용

- Windows 기본 OpenSSH Server를 활성화하고 기본 셸을 Windows PowerShell로 설정했다.
- SSH 수신 주소를 `100.117.247.29:22`로 제한하고 로그인 계정을 기존 `administrator` 하나로 제한했다.
- 방화벽 규칙 `CView-Tailscale-PowerShell-SSH`: Tailscale 인터페이스, 로컬 주소 `100.117.247.29`, 원격 주소 `100.64.0.0/10`, TCP 22, OpenSSH 실행 파일에만 허용한다. 기본 OpenSSH 전체 인터페이스 허용 규칙은 비활성화했다.
- 서비스는 지연 자동 시작이며 Tailscale 서비스에 의존한다. 실패 시 10초·30초·60초 간격으로 재시작하도록 설정했다.
- 호스트 개인 키 소유자는 SYSTEM이며 SYSTEM과 Administrators만 접근한다. 기존 사용자 비밀번호는 유지했다. 이후 릴리스 자동화를 위해 현재 Mac의 게시 전용 제한 키를 추가했다. [자동 게시 안내](AUTOMATION.md).
- SSH 에이전트 전달과 TCP 포트 전달을 비활성화했다.
- 기존 WinRM 서비스와 설정은 유지했다. 이 안내는 SSH로 PowerShell 명령 셸에 접속하는 방식이다.

설정 파일: `C:\ProgramData\ssh\sshd_config`

설정 전 상태 백업: `C:\CViewUpdates\tools\remote-powershell-before-20261024\state.json`

설치 소스: `Install-TailscalePowerShell.ps1`. 서버 검증 도구: `Test-TailscalePowerShell.ps1`.

## 검증 결과

- 실제 Windows PowerShell에서 설치 스크립트 실행 성공, SSH 설정 구문 검사 통과.
- SSH 서비스 `Running`, 자동 시작·Tailscale 의존성·수신 주소·방화벽 범위 확인.
- 별도 Mac에서 Tailscale 주소의 SSH 연결과 AES-GCM 암호화, ED25519 호스트 키 일치 확인.
- 서버가 비밀번호/공개 키 인증을 요청하는 단계까지 확인했다. 비밀번호를 제출하지 않은 시험은 예상대로 인증 거부로 종료됐다.
- 일반 LAN 주소 `192.168.0.2:22` 연결은 시간 초과였고, 서버의 실제 SSH 수신 소켓은 Tailscale 주소 하나뿐이었다.
- 사용자가 Mac 터미널에서 기존 비밀번호로 로그인하고 `hostname` 실행 결과 `WIN-7SV7UAK6K5B`가 표시됐음을 직접 확인했다.
- 재부팅 후 자동 복구와 다른 인터넷 회선에서의 연결은 미검증이다.

구체적인 설정·키·클라이언트 검증 기록은 `powershell-verification-2026-10-24.json`에 있다. 호스트 개인 키나 사용자 비밀번호를 포함하지 않는다.

[Microsoft의 SSH 기본 셸 설정 문서](https://learn.microsoft.com/en-us/windows-server/administration/OpenSSH/openssh-server-configuration)를 참고했다.
