# 백그라운드 라이브 지연 측정

앱이 가려진 동안(창 숨김) 라이브 지연이 웹 공식 플레이어와 같게 유지되는지 잰다. Tart macOS VM(`cview-macos`)에서 돌린다.

- 앱 계측: 디버그 로그(`CVIEW_DEBUG_LOG=1`)를 켜면 플레이어가 초마다 `player.latency` 표본을 `debug.jsonl` 에 남긴다
  (`wall` = Date.now() − hls.playingDate, `latency`, `target`, `ahead`, `rate`, `ll`, `waits`, `seeks`, `gap` = 250ms 타이머 실제 간격, `bgTimers`).
- `web-ref.mjs <채널> <초> <출력.jsonl>`: Playwright WebKit 으로 치지직 웹 라이브를 열어 같은 값을 초마다 남긴다.
  Chrome UA 와 `canPlayType('…mpegurl')` 숨김으로 웹이 Chrome 처럼 hls.js(LL) 경로를 쓰게 한다(안 하면 WebKit 기본 HLS).
- `run.sh <앱> <라벨> <채널> [전경초] [숨김초] [복귀초]`: 앱(두 번째 실행 인자로 딥링크)과 웹을 함께 틀고 전경 → 숨김 → 복귀, CPU 5초 표본.
- `cycles.sh <앱> <라벨> <채널> <숨김초...>`: 숨김/복귀를 되풀이(복귀 점프 확인).
- `appvis.swift`: 접근성 권한 없이 다른 앱 숨기기/되돌리기(`swiftc -O appvis.swift -o appvis`).
- `proxy.py`: VM 이 인터넷에 못 나갈 때 호스트 VM 다리(192.168.64.1)에만 여는 CONNECT 프록시. 치지직 API 는 UA 없는 요청에 답하지 않는다.
- `analyze.py <결과 폴더>`: 구간(전경·숨김·복귀)별 표와 `series.csv`.

VM 에서는 이 폴더를 `~/bglat/vm`, 앱을 `~/bglat/apps/<이름>/CView.app` 에 둔다고 가정한다.
