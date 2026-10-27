# CView 단계별 정밀 검증 — 2026-10-27

현재 소스의 Rust 테스트 1,885개, JavaScript 채팅 회귀 검사 12개, 배포 자동화 검사 15개가 통과했다. macOS·Windows GNU 대상 Clippy와 현재 소스의 macOS 개발 앱 빌드도 통과했다. 별도 QA 앱에서 실제 CHZZK 단일 시청, 4채널 멀티라이브, 일시정지·재개, 채널 추가·제거·교체, 홈 왕복과 채팅 재연결을 확인했다.

이번 범위에서 새 제품 결함을 확정하지는 않았다. 다만 기본 빌드 디스크의 공간 부족, Linux 의존성 경고 5건, 상시 회귀 CI 부재를 후속 조치로 남긴다. Windows 실기 실행, 실제 설치 앱의 업데이트 교체·재시작, 장시간 안정성 및 모든 화질·오디오 전환까지 검증한 결과는 아니다.

## 1. 검증 기준과 범위

| 항목 | 기준 |
| --- | --- |
| 저장소 | `/Users/dong/Downloads/cview-rust-main` |
| 브랜치·커밋 | `main` · `114a89686d8341e98545321fd19533cfa0e0606f` |
| 소스 버전·빌드 | `0.1.36` · `36` |
| 구조 | Rust 8개 크레이트, 추적 파일 565개, Rust 파일 391개 |
| UI | 앱 ES 모듈 54개, HLS 플레이어·HTML 인라인 스크립트 별도 |
| 증거 | [검사 기록 디렉터리](verification/2026-10-27/) |
| 변경 | 검증 보고서·검사 도구·증거만 추가. 제품 소스 및 HEAD 변경 없음 |

설치된 0.1.36 앱과 현재 소스는 같은 버전 번호여도 동일한 빌드가 아니다. 현재 커밋은 기존 0.1.36 릴리스 이후의 변경을 포함한다. 이를 구분하기 위해 현재 소스를 별도 식별자의 `CView Verification.app`으로 빌드했다. 새 배포, 버전 증가, 운영 서버 게시, Git 커밋·푸시는 수행하지 않았다.

## 2. 환경 및 빌드 경로 검증

기본 Cargo target 경로 `/Volumes/data/project/Rust/CView/target`가 있는 볼륨은 사용률 100%였다. 최초 검사 중 `No space left on device`가 발생했다. 이는 제품 컴파일 결함으로 분류하지 않았다.

기존 캐시·소스·배포 파일을 지우지 않고 다음 격리 경로로 전환해 검사를 완료했다.

```sh
export CARGO_TARGET_DIR=/Users/dong/Downloads/cview-rust-main/target/verification-2026-10-27
export CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
```

최종 확인 시 외장 볼륨 가용 공간은 약 41 MiB, 작업 저장소 볼륨은 약 84 GiB였다. 환경 변수를 생략한 다음 빌드는 기존 target 경로에서 다시 공간 부족을 겪을 수 있다. 이 검증용 환경 설정을 저장소 기본 설정에 반영하지는 않았다.

초기 샌드박스 파일 접근 실패와 공간 부족 로그를 보존했으며, 최종 판정은 격리 경로에서 완료한 로그를 기준으로 한다.

## 3. Rust·플랫폼 검사

| 검사 | 결과 | 증거 |
| --- | --- | --- |
| `cargo fmt --all -- --check` | 통과 | [format.log](verification/2026-10-27/format.log) |
| `cargo test --workspace --locked` | 1,883 통과, 0 실패, 3 제외 | [전체 로그](verification/2026-10-27/rust-tests-isolated.log) |
| 제외된 실서버 업데이트 검사 2개 | 2 통과, 0 실패 | [실서버 로그](verification/2026-10-27/live-update-tests.log) |
| Rust 합계 | **1,885 통과, 0 실패, 문서 예제 1개 미실행** | [집계 JSON](verification/2026-10-27/test-summary.json) |
| macOS workspace/all-targets Clippy, `-D warnings` | 통과 | [macOS 로그](verification/2026-10-27/clippy-macos-isolated.log) |
| Windows GNU workspace/all-targets Clippy, `-D warnings` | 통과 | [Windows 로그](verification/2026-10-27/clippy-windows.log) |
| 현재 소스 Tauri macOS 개발 앱 빌드 | 통과 | [빌드 로그](verification/2026-10-27/build-qa-app.log) |

검사는 인증, API/네트워크, 채팅, 도메인 모델, 저장소·복구, 플레이어, 앱 상태 및 뷰모델을 포함한다. 테스트 수는 코드 커버리지 비율이나 모든 사용자 시나리오의 완료를 의미하지 않는다. Windows 결과는 교차 컴파일·정적 검사이며 Windows 설치·실행 결과가 아니다.

## 4. UI·IPC·채팅 회귀 검사

| 검사 | 결과 |
| --- | --- |
| JS/HTML 스크립트 문법 | 57개 통과 |
| 상대 import 대상 | 209개 일치 |
| ES 모듈 연결·이름 있는 export | 54개 연결 성공 |
| 리터럴 Tauri `invoke` 이름 ↔ Rust 등록 | 101/101 일치 |
| 실제 채팅 모듈을 실행하는 회귀 검사 | 12/12 통과 |

IPC 검사는 명령 이름의 정적 대조이며 인자 타입, 권한, 모든 실행 분기의 통합 검증을 대신하지 않는다. ES 모듈 연결 검사는 WebKit 렌더링 검사가 아니다.

새로 보존한 [frontend-behavior.mjs](verification/2026-10-27/frontend-behavior.mjs)는 제품의 `chat-follow.js`, `ml-multichat.js`를 직접 불러오고 DOM·시계·타이머·IPC를 제어해 다음 회귀를 검사한다.

- 새 메시지 하단 따라가기, 사용자의 위쪽 읽기 위치 유지, 8초 유휴 뒤 재개, 스크롤바를 잡고 있을 때 자동 복귀 억제.
- 화면 복귀 시 위치 재동기화와 일반 채팅 자동 스크롤 비활성 설정 존중.
- 연결 종료 후 지연 재접속 1회, 메시지 도착 시 중복 재접속 취소, 이전 채널의 늦은 메시지 무시.
- 패인 제거 시 타이머·세션 정리, 재연결 이력 중복 건너뛰기, 보조 창의 메인 창 세션 소유 방지.

초기 검사 도구의 DOM 복제 과정에서 getter/setter가 사라져 3개 검사가 실패했다. 검사 도구의 속성 정의를 고쳐 최종 12개가 통과했으며, 이를 제품 결함 수정으로 계산하지 않았다. [최종 로그](verification/2026-10-27/frontend-behavior.log), [초기 도구 로그](verification/2026-10-27/frontend-behavior-initial-harness.log), [정적 검사](verification/2026-10-27/frontend-static.json), [모듈 연결](verification/2026-10-27/frontend-module-links.log).

## 5. 이전 지적 사항과 주요 경계 재검토

이전 검증의 4개 지적을 현재 소스에서 다시 확인했다. 이번 작업에서 수정한 사항은 아니다.

| 이전 지적 | 현재 확인 |
| --- | --- |
| 저장 파일명 충돌 한도 도달 시 덮어쓰기 | `create_unique_file`이 `create_new(true)`로 원자적 생성. 후보 소진 시 오류. 충돌 소진·동시 생성 테스트 통과 |
| OAuth 검증보다 먼저 성공 HTML 표시 | 콜백 완료 결과를 기다린 후 성공/실패 페이지 선택. state·토큰 처리 실패를 성공으로 표시하지 않는 경로 확인 |
| 페이지 위치를 반영하지 않는 `has_more` | 현재 offset·수신 개수·전체 개수를 반영. 마지막·빈 페이지 테스트 통과 |
| 이미지 리다이렉트 목적지 미검증 | 각 리다이렉트 목적지의 HTTPS·허용 호스트·횟수 제한 확인 |

위 경로는 `crates/cview-app/src/commands/content.rs`, `commands/oauth.rs`, `image_service.rs`와 관련 테스트에서 확인했다. 또한 키체인 실패 시 평문 저장으로 후퇴하지 않는 인증 저장 경로, 원격 로그인 창과 로컬 Tauri 권한의 분리, 스트림 프록시의 루프백·세션·리다이렉트 경계, 업데이트의 크기·SHA·서명·스테이징·복구 경로를 검토했다. 침투 테스트나 전체 보안 감사를 완료했다는 의미는 아니다.

## 6. 의존성 및 자동화

`cargo audit --json`은 RustSec DB 커밋 `3279c99911354c400e1d41e47c7d60e39258b147`의 공지 1,378개와 lockfile 의존성 574개를 대조했다. DB 기록의 마지막 갱신 시각은 `2026-10-26T22:32:25+01:00`이다.

`vulnerabilities` 분류는 0건이지만 정보성 경고가 5건 있으므로 “의존성 문제 없음”으로 판정하지 않는다.

| 패키지 | 분류 | 공지 |
| --- | --- | --- |
| `proc-macro-error 1.0.4` | 유지보수 중단 | [RUSTSEC-2024-0370](https://rustsec.org/advisories/RUSTSEC-2024-0370.html) |
| `zbus_names 4.3.5` | 유지보수 중단 | [RUSTSEC-2026-0371](https://rustsec.org/advisories/RUSTSEC-2026-0371.html) |
| `zvariant 5.15.1` | 유지보수 중단 | [RUSTSEC-2026-0370](https://rustsec.org/advisories/RUSTSEC-2026-0370.html) |
| `zvariant_derive 5.15.1` | 유지보수 중단 | [RUSTSEC-2026-0372](https://rustsec.org/advisories/RUSTSEC-2026-0372.html) |
| `glib 0.18.5` | unsound | [RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html) |

대상별 `cargo tree -i` 결과, 5개 모두 현재 macOS·Windows 그래프에는 없고 Linux의 GTK·알림 관련 Tauri 의존성 경로에 나타났다. `glib` 공지는 `VariantStrIter`의 null 포인터 역참조와 관련되며 수정 범위는 `>=0.20.0`이다. Linux 지원 시 상위 의존성 호환성을 함께 검토해야 한다. [감사 결과](verification/2026-10-27/cargo-audit.json), [대상별 경로](verification/2026-10-27/dependency-warning-paths.json).

배포 자동화 Python 테스트 15개, `release-macos.sh`의 zsh 문법 검사, 추적 Python 10개·셸 16개의 문법 검사가 통과했다. [자동화 로그](verification/2026-10-27/release-automation-tests.log), [문법 결과](verification/2026-10-27/tooling-syntax.json).

저장소의 GitHub workflow는 수동 `workflow_dispatch` Windows 빌드 1개이며 push/PR마다 회귀 검사를 수행하는 workflow는 없다. 이 판단은 저장소 설정에 한정한다. 과거 계정 한도 관련 주석을 현재 GitHub 서비스 상태로 간주하지 않았다.

## 7. 현재 소스의 실제 macOS 재생

QA 앱 식별자는 `app.cview.rust.verification20261027`이며 일반 앱과 별도 설정·키체인 이름을 사용한다. 로그인하지 않은 상태로 실제 공개 방송을 열었다. 빌드는 `--debug --bundles app --no-sign`을 사용한 개발용이며 배포 서명·공증 산출물이 아니다. [정확한 경로·SHA](verification/2026-10-27/qa-app-identity.json).

| 시나리오 | 관찰 |
| --- | --- |
| 홈 → 단일 방송 | 실제 영상과 수신 채팅 표시 |
| 네이티브 재생 메뉴 | 일시정지 표시 후 재개, 이후 영상 프레임 증가 확인 |
| 단일 → 멀티라이브 | 채널 수 1 → 2 → 3 → 4 확인 |
| 홈 ↔ 멀티라이브 | 화면 왕복 및 세션 표시 확인 |
| 포커스 레이아웃 | 주 영상과 보조 영상 구성 확인 |
| 채팅 다시 연결 | 재연결 알림 후 새 메시지 수신 확인 |
| 채널 제거·교체 | 종료된 방송 제거 후 3개, 대체 방송 추가 후 4개 확인 |
| 멀티채팅 그리드 | 4채널별 메시지와 입력 대상 표시 확인 |

최종 채널 조합을 2026-10-27 **10:07:19–10:09:39 KST, 약 140초**, 10초 간격 15개 표본으로 관찰했다. 모든 표본에서 4개 플레이어가 `playing=true`였다.

| 채널 | 시작 프레임 | 마지막 프레임 | 증가 | 마지막 영상 높이 | 마지막 누적 드롭 |
| --- | ---: | ---: | ---: | ---: | ---: |
| 김나성 | 10,559 | 14,731 | 4,172 | 360 | 0 |
| 하쿠텐 후와 | 8,530 | 12,790 | 4,260 | 360 | 8 |
| 아인 슈페너 | 85 | 8,435 | 8,350 | 1080 | 1 |
| 아카캉 Akang | 14,319 | 18,483 | 4,164 | 360 | 2 |

교체 채널인 아인 슈페너는 초기 표본에 stall 1회가 기록됐고, 다른 3개는 해당 관찰 표본의 stall 값이 0이었다. 드롭과 초기 stall을 숨기거나 장시간 무중단으로 일반화하지 않는다. 1080/360은 이 화면에서 관찰한 결과이며, 모든 창 크기·오디오 역할·입력 대상 전환의 화질 정책을 검증한 수치는 아니다.

처음 정지한 것으로 보인 `트수급백수`는 단독 시청에서 “방송이 종료되었습니다”·“오프라인”이 표시됐다. 따라서 이 관찰은 방송 종료로 분류했으며 앱 재생 결함으로 확정하지 않았다.

최종 상태의 비입력 채널에도 채팅 내용이 표시됐지만, 포커스/그리드 각각에서 입력 대상을 반복 변경한 직후 모든 비입력 채널의 새 메시지를 별도 시간 측정한 절차까지 완료하지는 못했다. 최근 멀티채팅 수정의 모든 실서비스 전환 조건을 통과했다고 주장하지 않는다.

[재생 지표 요약](verification/2026-10-27/qa-runtime-summary.json), [선별 지표](verification/2026-10-27/qa-runtime-metrics.jsonl), [최종 화면](verification/2026-10-27/qa-final-multilive.png). 선별 로그에는 네트워크 원문 URL·인증 토큰·채팅 본문을 복사하지 않았다. QA 앱은 검증 후 종료했다.

기존 `/Applications/CView.app`도 초기 확인 과정에서 4채널로 확장했다. 해당 설치 앱은 교체하거나 종료하지 않았으며, 검증으로 추가한 채널이 열린 상태로 남아 있다. 현재 소스 재생 판정은 별도 QA 앱을 기준으로 했다.

## 8. 기존 배포 산출물과 업데이트 검사

| 대상 | 확인 결과 |
| --- | --- |
| 설치 앱 0.1.36/build 36 | `codesign --verify --deep --strict` 통과 |
| 설치 앱 Gatekeeper | `accepted`, `Notarized Developer ID` |
| 기존 0.1.36 DMG | stapled 공증 티켓 검증 통과 |
| 기존 DMG·Windows EXE | 각각 기존 게시 영수증의 크기·SHA-256과 일치 |
| 현재 업데이트 feed | 실서버 HTTPS 조회 테스트 통과 |
| 실서버 DMG 업데이트 E2E | 다운로드·전체 해시·잘못된 해시 거부·DMG 마운트·앱 신원 검사·임시 앱 교체/보호·정리 통과 |

기존 영수증의 검증 시각은 `2026-10-26T16:07:01Z`이며 서버·클라이언트 다운로드 검증 성공이 기록돼 있다. 이는 이번 작업에서 새로 게시한 결과가 아니다. 이번 실서버 테스트는 macOS DMG를 새로 내려받아 검증했으며 Windows EXE의 원격 전체 다운로드를 다시 수행한 것은 아니다.

업데이트 E2E는 **임시 가짜 설치 앱**을 대상으로 하며, 실제 실행 명령은 기록용 스크립트로 대체한다. 실제 `/Applications/CView.app`의 종료 → 교체 → 재시작 → 최종 버전·실재생을 완료한 UI 업데이트 테스트와 구분한다.

Windows 설치 파일은 기존 안내대로 미서명이며 SmartScreen 경고 가능성이 있다. [서명·공증 결과](verification/2026-10-27/artifact-check.json), [산출물 비교](verification/2026-10-27/artifact-hashes.json), [실서버 테스트](verification/2026-10-27/live-update-tests.log).

## 9. 남은 검증 및 후속 조치

| 우선순위 | 항목 | 완료 기준 |
| --- | --- | --- |
| 높음 — 환경 | 기본 target 볼륨 공간 부족 | 캐시·산출물 보존 정책에 따라 공간 확보 또는 영구 경로 재설정 후 기본 명령 재확인 |
| 중간 — Linux | 의존성 경고 5건 | GTK/Tauri/알림 의존성 호환 업그레이드 검토 및 Linux 빌드·실기 확인 |
| 중간 — 자동화 | push/PR 회귀 CI 부재 | Rust·JS·배포 스크립트 검사를 정규 CI에 포함 |
| 실기 미완료 | 입력 대상 전환·화질·오디오 | 각 레이아웃에서 대상 변경 후 비입력 채널 새 채팅, 화질 승격·강등, 실제 출력 소리 확인 |
| 실기 미완료 | Windows 설치·실행 | 실제 Windows에서 설치, WebView2, 라이브·채팅·업데이트 확인 |
| 실기 미완료 | 실제 앱 자동 업데이트 | 실제 설치 앱의 다운로드·교체·재시작·버전·재생 확인 |
| 추가 범위 | OAuth 사용자 로그인, VOD, 다운로드·녹화, 장시간 안정성, GPU | 각 기능의 실서비스 시나리오와 통제된 측정 수행 |

이번 결과만으로 배포 준비가 모두 완료됐다고 판단하지 않는다. 사용자가 배포를 요청하면 저장소 지침의 정식 `scripts/release-macos.sh --dmg` 경로로 별도 수행해야 한다.

## 10. 재실행과 증거 보존

저장소 루트에서 2절의 환경 변수를 설정한 뒤 실행한다.

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu --locked -- -D warnings
node --experimental-vm-modules docs/verification/2026-10-27/frontend-behavior.mjs
python3 -B -m unittest discover -s tools/windows-update-server/tests -v
zsh -n scripts/release-macos.sh
cargo audit --json
```

실서버 테스트는 일반 테스트에서 제외돼 있으므로 별도로 실행해야 한다. 이번에는 빌드된 `cview_app` 테스트 실행 파일에 `--ignored --nocapture --test-threads=1`을 전달해 2개를 실행했다. 실행 파일의 해시는 다음 빌드에서 달라질 수 있다. 로컬 서버 테스트에는 포트 바인딩 권한, 실서버 테스트에는 네트워크·보안 서비스 접근이 필요하다.

[소스 인벤토리·해시](verification/2026-10-27/source-inventory.json)와 [최종 소스 불변 확인](verification/2026-10-27/final-source-check.json)을 보존했다. 제품 소스가 바뀌면 이 결과의 기준 커밋과 구분해 다시 검증해야 한다.

## 11. 후속 조치 반영 (2026-10-27)

| 항목 | 반영 |
| --- | --- |
| push/PR 회귀 CI 부재 | `.github/workflows/ci.yml` 추가. macOS에서 `fmt`·Clippy(`-D warnings`)·`cargo test`, Ubuntu에서 채팅 회귀 12개·배포 자동화 15개·스크립트 문법·`cargo audit` 실행 |
| 채팅 회귀 검사 상시화 | 증거의 `frontend-behavior.mjs`를 `tests/frontend/chat-behavior.mjs`로 옮겨 반복 실행. 증거 파일은 당시 상태로 보존 |
| Linux 의존성 경고 5건 | 4건은 수정판이 없는 유지보수 중단 공지, `glib`는 `>=0.20` 필요(현재 Tauri/wry는 gtk-rs 0.18 사용). `cargo update`로 해소되지 않음을 확인. CI의 `cargo audit --deny warnings`에서 근거와 함께 예외 처리하고 새 공지는 실패로 막음. Linux 지원 시 재검토 |
| 기본 target 볼륨 공간 부족 | 진단만 수행. `/Volumes/data` 가용 44 MiB, CView target 122 GB 중 `debug` 98 GB(`deps` 85 GB). 삭제는 사용자 승인 후 진행 |

CI는 실서버 업데이트 테스트(`#[ignore]`)와 Windows 교차 Clippy를 포함하지 않는다. 기존 주석에 따르면 2026-10-20 당시 이 계정의 Actions 실행이 startup_failure였으므로, 첫 PR 실행 결과로 동작 여부를 확인해야 한다.
