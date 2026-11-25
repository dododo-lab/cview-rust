## 무엇을 바꿨나요

<!-- 바뀐 동작을 사용자 입장에서 짧게. 관련 이슈가 있으면 "Closes #번호" -->

## 확인한 것

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` (Windows 코드를 건드렸으면 `--target x86_64-pc-windows-gnu` 도)
- [ ] `cargo test --workspace`
- [ ] `node --check` (app-ui/js)
- [ ] 화면 변경이면 전후 스크린샷 첨부
- [ ] 실제 앱에서 확인한 플랫폼: macOS / Windows

## 성능 (최적화·핫패스를 건드렸으면)

<!-- 전후를 같은 기계에서 잰 값만 적는다. 재지 못했으면 "재지 못함"이라고 쓴다. -->

- [ ] `python3 scripts/perf-gate.py --markdown perf-gate.md` 결과 표를 아래에 붙임(채팅 파서·저장소·이미지 캐시, 회귀면 종료 코드 1)
- [ ] 화면 검사: `tools/ui-regression` 의 `perf`·`chat-memory`·`ml-offscreen`·`usage-monitor`·`startup-modules` 를 Chromium·WebKit 둘 다 돌려 main 과 같은 실패만 남는지 (`perf` 의 CDP 강제 레이아웃 추적·CPU 감속과 `usage-monitor` U17 은 Chromium 전용)
- [ ] 실앱 CPU·GPU·RSS(해당하면): `perf/tools/suite2.sh` + `perf/tools/analyze-log.py` 전후 비교
- [ ] 감사·문서가 근거면 항목마다 판정(고침 / 틀림 / 보류)과 근거

## 참고

<!-- 리뷰어가 알아야 할 점, 남은 일, 위험한 부분 -->
