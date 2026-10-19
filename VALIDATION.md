# 프로젝트 검증 및 개선 기록

검증일: 2026-10-20 (한국 시간). Debian 13 x86_64, Rust 1.95.0, Node.js 24.22.0.

## 검토 범위

8개 Rust 크레이트의 전체 테스트·정적 검사와 UI 모듈 연결을 검사하고,
네트워크·인증·파일 저장·미디어 처리·창 생성·화면 전환 경계를 중심으로 코드를 검토했다.
테스트 통과는 실제 계정 로그인·방송 재생이나 모든 플랫폼의 기능 동등성을 보증하지 않는다.

| 영역 | 확인 내용 |
| --- | --- |
| core | 모델·정책·스케줄링 테스트, 카탈로그 부팅 조건 |
| networking | 캐시·인증 세대·재시도·요청 제한·API 엔드포인트 테스트 |
| chat | 메시지 파싱·moderation·연결 시도 세대·WebSocket 테스트 |
| auth | 쿠키 정책·PKCE·CSRF·저장소·로그인 상태 테스트 |
| persistence | SQLite CRUD·시청 기록·설정·백업·복구·동기화 큐 테스트 |
| player | HLS·프록시·녹화·코디네이터와 다운로드 무결성·취소 테스트 |
| app-core | 서비스·뷰모델·멀티라이브 상태 전이 테스트 |
| app/UI | 명령 등록·파일 내보내기·창 생성·화면 수명, 실제 Linux UI 기동 |

## 수정한 문제

- Linux 빌드: `tauri-plugin-deep-link 2.6.2`와 호환되는 `rust-ini 0.21.1`을 Linux 의존성에 고정했다.
  `0.21.3`의 `ParseOption` 필드 추가로 발생하던 E0063을 해결했다.
- Linux 창 생성: macOS 전용 제목 막대 API 호출을 플랫폼 조건으로 한정했다.
- 전체 Clippy 오류 4건의 불리언 조건을 같은 의미의 간결한 조건으로 정리했다.
- 파일 내보내기: 이름 조회 후 `write`하던 경합을 `create_new` 기반의 원자적 이름 확보로 바꿨다.
  이름 999개가 모두 사용 중이면 원본을 덮어쓰는 대신 오류를 반환한다. 기존 심볼릭 링크도 따라가지 않는다.
- 긴 한글 파일명: UTF-8 바이트 길이도 제한하고 확장자를 보존한다.
- 다운로드: 고유 임시 파일과 `persist_noclobber`로 기존 완성본을 보존한다.
  실패·취소·태스크 abort 시 임시 파일을 정리하고, 서버가 응답을 멈춘 동안에도 취소를 확인한다.
- HLS 다운로드: 기존 `HlsByteSpan` 정책으로 범위의 유효성·응답 길이·Content-Range를 검증한다.
  서버가 Range를 무시한 200 응답은 요청 구간만 추출한다. 동일 URL의 init 구간 변경도 반영한다.
  서명 쿼리는 같은 출처(스킴·호스트·포트)에만 상속한다.
- VOD/클립 화면: 화면을 떠날 때 진행 중 요청을 무효화하고 클립 iframe을 내린다.
  늦게 돌아온 응답이 숨겨진 화면에서 재생·저장 타이머를 시작하지 못하게 했다.
- Windows 외부 URL: `cmd /C start` 대신 `ShellExecuteW`를 사용한다.
  URL의 `&` 등이 셸 명령으로 해석되는 경로를 제거했다.
- 재현성: Rust 도구 버전, Linux/macOS CI, UI 연결 검사와 Node 회귀 테스트를 추가했다.
  README의 오래된 `cd rust` 경로와 저장소를 수정하던 포맷 검사 안내도 정리했다.

## 실행 결과

| 검사 | 결과 |
| --- | --- |
| `cargo fmt --all -- --check` | 통과 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 통과 |
| `cargo test --workspace --locked` | 1,816 통과, 0 실패, 기존 doctest 1개 ignored |
| `cargo build -p cview-app --locked` | 통과 |
| `node --experimental-vm-modules scripts/check-ui.mjs` | 39개 UI 모듈, 인라인 스크립트 2개, IPC 명령 93개 통과 |
| `node --test app-ui/tests/*.test.mjs` | 5개 통과 |
| Windows URL 호출 부분 교차 컴파일 | `x86_64-pc-windows-gnu` 대상 `cargo check` 통과; 앱 전체 빌드는 아님 |
| Linux GUI | 홈·첫 실행 안내·설정 렌더링, 키보드로 설정 이동, 테마 변경의 SQLite 저장, 통계 보조 창 생성 확인 |

다운로드 회귀 테스트는 외부 서비스 대신 루프백 HTTP 서버를 사용한다. 동시 파일 내보내기,
이름 소진, 심볼릭 링크, 긴 한글 이름, 다운로드 취소·abort·덮어쓰기 방지와 잘못된 범위 응답을 검사한다.
CI 설정은 추가했으나 원격 GitHub Actions 실행 결과를 의미하지 않는다.

## 의존성 보안 검사와 남은 제한

`cargo-audit 0.22.2`로 잠금 파일의 의존성 572개를 검사했다.
RustSec DB 커밋은 `999ec89d5d3f10f20be14750ec8c2d8b44e0ac9b` (2026-10-17)이다.
`cargo audit --json`은 취약점 분류 0건과 아래 경고 5건을 보고했다. 경고를 무시 목록에 넣지 않았다.

| advisory | 의존성 | 내용 |
| --- | --- | --- |
| RUSTSEC-2024-0429 | glib 0.18.5 | `VariantStrIter` iterator 구현의 메모리 안전성 문제(unsound); 수정 버전 ≥0.20.0 |
| RUSTSEC-2024-0370 | proc-macro-error 1.0.4 | 유지보수 중단 |
| RUSTSEC-2026-0371 | zbus_names 4.3.5 | 유지보수 중단 |
| RUSTSEC-2026-0370 | zvariant 5.15.1 | 유지보수 중단 |
| RUSTSEC-2026-0372 | zvariant_derive 5.15.1 | 유지보수 중단 |

`glib 0.18`은 Tauri → GTK 0.18/WebKitGTK 등에서 요구한다. 직접 `glib 0.20`을 추가해도
기존 의존성이 교체되지 않는다. GTK/Tauri 상위 의존성의 호환 가능한 전환 또는 검토된 백포트가
필요하며 이번 변경에서 GTK 스택을 강제로 교체하지 않았다. 나머지 경고도 상위 의존성 갱신을 추적해야 한다.

- CHZZK/Naver 외부 요청은 이 클라우드에서 시간 초과가 발생했다.
  실계정 로그인·실제 라이브/VOD/채팅은 검증하지 않았다.
- macOS/Windows 실제 실행, 서명·공증·배포 패키징은 해당 플랫폼에서 별도 확인해야 한다.
- 클라우드 GUI는 사용자 디렉터리에 설치한 WebKit 실행 파일 경로를 PRoot로 연결하고 Xvfb/D-Bus로 구동했다.
  TLS 및 WebKit 샌드박스 검증을 끄지 않았다. 원격 검사기 기반 자동 IPC 점검은 WebKit 프로세스 종료로
  완료하지 못했으며, 별도 재기동 후 일반 X11 입력으로 위 GUI 동작을 확인했다.
