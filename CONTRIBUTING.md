# 기여 안내

CView에 관심을 가져 주셔서 고맙습니다. 이슈와 PR은 한국어로 써 주시면 됩니다.

## 이슈

- **버그 제보**: [버그 제보 양식](https://github.com/dododo-lab/cview-rust/issues/new?template=bug_report.yml)을 써 주세요. 앱 버전(설정 또는 메뉴의 정보), 운영체제, 재현 순서가 있으면 훨씬 빨리 고칠 수 있습니다.
- **기능 제안**: [기능 제안 양식](https://github.com/dododo-lab/cview-rust/issues/new?template=feature_request.yml)을 써 주세요.
- **보안 문제**는 공개 이슈로 올리지 말고 [`SECURITY.md`](SECURITY.md)를 따라 비공개로 알려 주세요.

## 개발 환경

[README의 소스에서 빌드하기](README.md#소스에서-빌드하기)를 따릅니다. 화면은 `app-ui/`의 정적 HTML·CSS·ES 모듈이라 따로 빌드할 필요가 없고, `cargo run -p cview-app`으로 바로 띄울 수 있습니다.

## PR 규칙

1. `main`에서 브랜치를 만들고, 한 PR에는 한 가지 목적만 담습니다.
2. 커밋 메시지와 PR 제목은 무엇이 어떻게 바뀌었는지 한국어로 짧게 씁니다(예: `멀티라이브 채팅 포커스가 풀리던 문제 수정`).
3. 올리기 전에 저장소 루트에서 검사를 모두 통과시킵니다.

   ```sh
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   for f in app-ui/js/*.js; do node --check "$f"; done
   ```

4. 화면이 바뀌면 전후 스크린샷을 PR에 붙입니다. `tools/ui-regression/`의 스크립트로 모의 데이터 화면을 찍을 수 있습니다.
5. 배포 스크립트·업데이트 서버 도구를 바꿨다면 다음도 통과시킵니다.

   ```sh
   python3 -B -m unittest discover -s tools/windows-update-server/tests -v
   zsh -n scripts/release-macos.sh
   zsh -n scripts/release-with-prs.sh
   ```

## 코드 스타일

- Rust는 `rustfmt` 기본 설정과 `clippy -D warnings`를 따릅니다.
- 주석과 문서는 한국어로 씁니다. 원본 Swift 앱에서 옮긴 코드는 모듈 머리 주석에 원본 파일을 적어 둡니다.
- 화면 JS는 번들러 없이 브라우저가 바로 읽는 ES 모듈로 둡니다.

## 배포

배포는 메인테이너가 [`docs/RELEASING.md`](docs/RELEASING.md)의 절차로 진행합니다. PR에서 버전 번호를 직접 올리지 마세요.
