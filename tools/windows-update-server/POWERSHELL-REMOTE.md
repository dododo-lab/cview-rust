# Windows Server PowerShell 원격 접속

## 접속

같은 Tailscale 네트워크에 연결한 Mac 터미널 또는 Windows 터미널에서 실행한다.

```sh
ssh <관리자-계정>@<서버-Tailscale-IP>
```

관리자 비밀번호를 입력한다. 입력 중 문자가 표시되지 않는 것은 정상이다. 로그인하면 Windows PowerShell 5.1이 열린다. `hostname`으로 서버 이름을 확인하고 `exit`로 종료한다.

최초 접속 시 ED25519 서버 키 지문:

```text
SHA256:wOg0YZCPhtyxDl/5zsDFaLaj8rbIyNztDYEJTZa0TqA
```

서버에서 직접 읽은 공개 키와 네트워크를 통해 받은 키가 일치하는 것을 확인했다. 처음 접속하는 PC에서는 위 지문을 확인한 뒤 신뢰 여부를 선택한다.

한 번만 명령을 실행하려면:

```sh
ssh <관리자-계정>@<서버-Tailscale-IP> 'hostname; Get-Service sshd'
```

## 업데이트 원격 게시

기존 `CViewUpload$` 공유의 버전 폴더에 완성된 설치 파일과 `notes.txt`를 업로드한다. 다음 명령은 예시 버전 `0.1.21`의 파일 구성과 버전을 검사한다.

```sh
ssh <관리자-계정>@<서버-Tailscale-IP> 'powershell -NoProfile -File C:\CViewUpdates\tools\Publish-UploadedRelease.ps1 -Version 0.1.21 -ValidateOnly'
```

검사 후 실제 게시하려면:

```sh
ssh <관리자-계정>@<서버-Tailscale-IP> 'powershell -NoProfile -File C:\CViewUpdates\tools\Publish-UploadedRelease.ps1 -Version 0.1.21'
```

실제 릴리스 버전을 사용해야 한다. 파일 구성은 [업로드·게시 안내](README.md)를 따른다.

## 적용 내용

- Windows 기본 OpenSSH Server를 활성화하고 기본 셸을 Windows PowerShell로 설정했다.
- SSH 수신 주소를 서버의 Tailscale 주소(22번 포트)로 제한하고 로그인 계정을 관리자 계정 하나로 제한했다.
- 방화벽 규칙 `CView-Tailscale-PowerShell-SSH`: Tailscale 인터페이스, 로컬 주소는 서버의 Tailscale 주소, 원격 주소 `100.64.0.0/10`, TCP 22, OpenSSH 실행 파일에만 허용한다. 기본 OpenSSH 전체 인터페이스 허용 규칙은 비활성화했다.
- 서비스는 지연 자동 시작이며 Tailscale 서비스에 의존한다. 실패 시 10초·30초·60초 간격으로 재시작하도록 설정했다.
- 호스트 개인 키 소유자는 SYSTEM이며 SYSTEM과 Administrators만 접근한다. 기존 사용자 비밀번호는 유지했다. 이후 릴리스 자동화를 위해 현재 Mac의 게시 전용 제한 키를 추가했다. [자동 게시 안내](AUTOMATION.md).
- SSH 에이전트 전달과 TCP 포트 전달을 비활성화했다.
- 기존 WinRM 서비스와 설정은 유지했다. 이 안내는 SSH로 PowerShell 명령 셸에 접속하는 방식이다.

설정 파일: `C:\ProgramData\ssh\sshd_config`


설치 소스: `Install-TailscalePowerShell.ps1`. 서버 검증 도구: `Test-TailscalePowerShell.ps1`.


[Microsoft의 SSH 기본 셸 설정 문서](https://learn.microsoft.com/en-us/windows-server/administration/OpenSSH/openssh-server-configuration)를 참고했다.
