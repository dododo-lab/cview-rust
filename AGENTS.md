# CView 작업 지침

## 배포용 릴리스

사용자가 배포용 릴리스 빌드를 요청하면 저장소 루트에서 다음 정식 경로를 사용한다.

```sh
scripts/release-macos.sh --dmg
```

릴리스에 포함할 PR이 있거나 작업 트리에 사용자의 커밋 안 된 작업이 있으면 다음 래퍼를 사용한다(Claude 자동 모드 허용 대상).

```sh
scripts/release-with-prs.sh [PR번호 ...] [-- release-macos.sh 옵션]
```

- 열린 PR을 draft 해제·squash 병합하고, 커밋 안 된 작업(추적 안 된 파일 포함)을 stash로 보관한 뒤 `git pull --ff-only origin main` → `scripts/release-macos.sh --dmg`를 실행하고, 성공·실패와 관계없이 보관한 작업을 되돌린다.
- `.idea/`는 `.git/info/exclude`에 넣어 IDE가 다시 만들어도 깨끗함 검사를 막지 않게 한다. `--no-notarize`는 넘기지 않는다.
- main 브랜치에서만 실행한다. 되돌리기에 실패하면 안내대로 `git stash list` 맨 위 항목을 직접 확인한다.

- `release-macos.sh --dmg`는 검사, 버전·빌드 번호 증가, macOS 서명·공증, Windows 설치 파일 생성, 업데이트 서버 업로드·게시, 서버와 Mac의 전체 다운로드·SHA-256 검증, Git 푸시까지 실행한다.
- 개발용 `cargo run`, `cargo tauri build` 자체에는 게시 훅이 없다. 배포 요청을 개별 빌드 명령으로 대체하거나 `dist/` 생성만으로 완료했다고 보고하지 않는다.
- 배포 전 변경 사항을 검토·커밋해서 작업 트리를 깨끗하게 만든다. 다른 작업자의 변경을 무단으로 되돌리거나 포함하지 않는다.
- 먼저 `python3 scripts/publish-update.py --preflight`로 확인할 수 있다. Tailscale과 이 Mac의 전용 키가 필요하다. 비밀번호나 개인 키를 채팅·저장소·로그에 넣지 않는다.
- 키 인증 거부 시 Proxifier의 `CView update SSH (Tailscale)` 규칙(업데이트 서버 SSH, Direct, Default보다 위)을 확인한다. 휴대폰 프록시를 거치면 서버가 다른 출발지로 인식해 정상 키를 거부할 수 있다. 자세한 진단은 `AUTOMATION.md`를 따른다.
- macOS 공증 프로파일 기본값은 `cview-notarize`이며 `CVIEW_NOTARIZE_PROFILE`로 변경한다. `--no-notarize`는 사용자가 공증 생략을 명시적으로 허용한 경우에만 사용한다. 공증 실패를 피하려고 임의로 사용하지 않는다.
- macOS와 Windows 설치 파일을 같은 버전으로 함께 게시한다. 배포에서 `CVIEW_SKIP_WINDOWS`는 금지한다. Windows 미서명/SmartScreen 안내를 설명에 유지한다.
- 사용자에게 보여 줄 변경 사항은 UTF-8 파일로 작성하고 `CVIEW_RELEASE_NOTES_FILE=/절대/경로/notes.txt`로 전달한다. 생략하면 Git 변경 목록이 사용된다.
- 게시 실패 시 버전을 다시 올리지 않는다. 보존된 파일과 `dist/CView_<버전>_notes.txt`로 다음 명령을 재실행한다.

```sh
python3 scripts/publish-update.py --version <버전> --notes-file dist/CView_<버전>_notes.txt
```

- 이미 같은 파일로 게시된 버전은 덮어쓰지 않고 재검증한다. 성공 후 미완료된 `git push origin HEAD`와 `git push origin v<버전>`을 수행한다.
- `dist/CView_<버전>_publish-receipt.json`과 실행 결과로 게시·양쪽 다운로드 검증을 확인한다. 설치·실제 재생 테스트는 별도로 확인하고, 수행하지 않았다면 완료로 주장하지 않는다.
- `--probe`는 1 MiB 전송·재읽기·정리와 현재 릴리스 다운로드를 검사하며 새 릴리스를 만들지 않는다. 가짜 설치 파일을 운영 서버에 게시하지 않는다.

상세 설정·장애 복구·다른 PC 등록: [자동 게시 안내](tools/windows-update-server/AUTOMATION.md).

자동화 코드 변경 시:

```sh
python3 -B -m unittest discover -s tools/windows-update-server/tests -v
zsh -n scripts/release-macos.sh
zsh -n scripts/release-with-prs.sh
```
