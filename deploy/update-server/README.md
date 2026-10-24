# Windows Server 업데이트 서버

Windows Server 2019/2022/2025의 IIS에서 HTTPS 정적 파일로 CView 릴리스 정보와 설치 파일을 제공한다.
서버 실행에 Node.js·Python·데이터베이스가 필요하지 않다. 릴리스 작성·검증 도구는 Python 3.9 이상이 있는
개발 컴퓨터 또는 서버 관리자 계정에서 실행한다.

현재 앱은 **새 버전 확인 후 다운로드 페이지 열기**를 지원한다. 자동 다운로드·앱 교체 설치 서버는 아니다.
기존 바이너리는 GitHub 주소가 고정되어 있으므로, 자체 서버 주소를 지정한 새 코드로 한 번 재빌드·배포해야 한다.

## 1. Windows 관리자 PowerShell에서 설치

도메인이 서버를 가리키고 해당 도메인의 HTTPS 인증서와 개인 키가
`로컬 컴퓨터 → 개인` 인증서 저장소에 있어야 한다. 클라이언트가 신뢰하는 유효한 인증서를 사용한다.
관리자 PowerShell 5.1에서 이 폴더로 이동한 뒤 실행한다.

```powershell
.\Install-UpdateServer.ps1 -HostName updates.example.com -CertificateThumbprint 'CERTIFICATE_THUMBPRINT'
```

기본 저장 경로는 `C:\CViewUpdates\site`이며 `-RootPath`로 전용 디렉터리를 지정할 수 있다.
IIS 역할·정적 파일 기능, 전용 앱 풀, SNI HTTPS 바인딩, TCP 443 방화벽 규칙을 구성한다.
다른 웹사이트의 경로나 같은 HTTPS 호스트 바인딩은 덮어쓰지 않는다.
게시 폴더 쓰기는 관리자·SYSTEM만, IIS는 읽기만 허용한다. 재시작이 필요한 기능 설치라면 안내 후 중단한다.
외부 접속에는 DNS·공유기/NAT·클라우드 보안 그룹의 443 접근도 별도로 준비해야 한다.

기존 IIS 사이트와 FTP 웹 루트를 사용하는 경우에는 새 사이트 설치 대신 `web.config`, `index.html`, `style.css`, `health.json`과
게시 결과를 해당 사이트의 전용 경로에 배치할 수 있다. 상위 사이트의 IIS 설정 충돌 여부는 관리자가 확인해야 한다.
`Install-UpdateServer.ps1`, Python 도구와 테스트 파일을 공개 웹 루트에 올리지 않는다.

## 2. 릴리스 게시

실제 앱 버전과 일치하는 서명된 배포 설치 파일을 준비한다. `_local.dmg`·소스 ZIP은 게시 도구가 거부한다.
실제 설치 파일이 없으면 테스트용 가짜 버전을 게시하지 않는다. 최초 게시 전 `latest.json`의 404는 정상이다.

```powershell
python .\publish.py --root C:\CViewUpdates\site --base-url https://updates.example.com --version 0.1.1 --artifact C:\Releases\CView_0.1.1_arm64.dmg --notes-file C:\Releases\notes.txt
```

여러 파일은 `--artifact`를 반복한다. 파일명은 영문·숫자·점·밑줄·대시를 사용한다.
게시 버전은 안정 버전 `숫자.숫자.숫자`이며 기존 버전보다 커야 한다.
기존 릴리스는 변경하지 않는다. 완성된 버전 폴더를 옮긴 뒤 최신 피드를 원자적으로 교체한다.
동시 게시를 잠금 파일로 차단하고, 복사·피드 교체 실패 시 기존 피드를 유지한다.
작업이 강제 종료되어 잠금 파일이 남았다면 게시 프로세스가 종료됐는지 확인한 뒤
웹 루트의 상위 폴더에 있는 `.site-publish.lock`을 관리자 계정으로 삭제하고 다시 실행한다.

구조:

```text
C:\CViewUpdates\
  server.json                 # 관리자 설정, 공개 디렉터리 밖
  site\
    web.config
    health.json
    index.html
    style.css
    releases\
      latest.json             # 캐시 금지, GitHub 호환 릴리스 JSON
      0.1.1\
        index.html            # 사용자 다운로드 페이지
        manifest.json
        CView_0.1.1_arm64.dmg
        CView_0.1.1_arm64.dmg.sha256
```

개발 컴퓨터에서 `--root`에 로컬 출력 폴더를 지정해 같은 구조를 만들 수도 있다.
FTP 배포 시 설치 파일·버전 페이지를 먼저 전송하고 `latest.json`을 마지막에 전환해야 한다.
일반 FTP 업로드는 파일시스템의 원자적 교체를 보장하지 않으므로 서버에서 `publish.py`를 실행하는 방식을 권장한다.
FTP는 파일 전송만 제공하며 IIS 설치·인증서 바인딩·프로그램 실행은 할 수 없다.

## 3. 앱 연결

빌드할 때 환경 변수에 주소를 지정하면 앱에 포함된다. 실행 시 같은 변수가 있으면 실행 시 설정을 우선한다.

```sh
CVIEW_UPDATE_URL=https://updates.example.com/releases/latest.json ./scripts/build-local-macos.sh --signed
```

Windows 클라이언트 빌드도 같은 `CVIEW_UPDATE_URL`을 설정하고 빌드한다.
HTTPS만 허용하고 인증 정보가 들어간 URL은 거부한다. 주소가 잘못되면 오류를 표시한다.
주소를 지정하지 않으면 기존 GitHub Releases 확인을 유지한다.
앱 설정의 업데이트 확인 버튼에서 게시 버전이 현재 앱보다 새로우면 해당 서버의 다운로드 페이지를 연다.

## 4. 운영 확인

```powershell
python .\check_server.py --base-url https://updates.example.com --download
```

실제 HTTPS 인증서, health·릴리스 정보·다운로드 페이지, 설치 파일 크기와 SHA-256을 확인한다.
공개 서비스라면 외부 네트워크에서도 검사한다. DNS·인증서 또는 설치 파일이 준비되지 않으면 검증은 실패한다.
IIS 로그는 기본 `%SystemDrive%\inetpub\logs\LogFiles`에 남는다. 서버 설정과 릴리스 폴더를 백업한다.

Tailscale의 `100.64.0.0/10` 주소는 공개 다운로드 주소가 아니다. 관리용 FTP 주소와 앱의 HTTPS 배포 주소를 구분한다.
Tailnet 내부 서비스로만 운영할 경우 앱 클라이언트도 해당 Tailnet에 접속할 수 있어야 한다.
FTP의 제어 포트 외에 수동 모드(PASV) 데이터 포트도 서버 방화벽에서 연결 가능해야 한다.

## 검증 범위

`python3 test_publish.py -v`는 실제 루프백 HTTP 요청으로 JSON·페이지·파일 해시를 검사하고,
동시 게시·경로·중복 이름·버전·게시 실패 시 기존 피드 보존을 검사한다.
IIS 구성 XML과 PowerShell 문법도 검사했다. 현재 연결된 환경은 Linux이므로
실제 Windows IIS 설치·인증서 바인딩·FTP/Tailscale 접속 완료를 뜻하지 않는다.
