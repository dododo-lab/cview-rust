// 웹 화면 상태 알림 — Rust 의 멈춤 감시(crates/cview-app/src/ui_watchdog.rs)와 디버그 로그용.
//
// 1) 살아 있다는 신호(heartbeat): 시작할 때·몇 초마다·Rust 의 핑(`window.__cviewBeat()`)을 받을 때 `ui_heartbeat` 를
//    부른다. 웹 콘텐츠 프로세스가 멈추면(GPU 프로세스가 끊긴 뒤 창이 검게 남던 사례) 신호가 끊기고, Rust 가 화면을
//    되살린다. 숨은 창은 타이머가 늦어져도 Rust 가 보이는 창만 판단하므로 괜찮다.
// 2) 처리되지 않은 오류·promise 거부를 디버그 로그(`ui.error`)에 남긴다 — 예전엔 로그에 JS 오류가 하나도 없어
//    화면이 깨진 원인을 알 수 없었다. Rust 는 디버그 로그가 켜져 있을 때만 쓴다.

import { isTauri, invoke } from './api.js';

const BEAT_MS = 5000;
const ERROR_LIMIT_PER_MIN = 10;

let installed = false;

export function installUiHealth() {
  if (installed || !isTauri) return;
  installed = true;
  let inflight = false;
  const beat = () => {
    if (inflight) return; // 앞 신호가 아직 돌아오지 않았다 — 겹쳐 쌓지 않는다
    inflight = true;
    invoke('ui_heartbeat').catch(() => {}).finally(() => { inflight = false; });
  };
  window.__cviewBeat = beat;
  beat();
  setInterval(beat, BEAT_MS);
  document.addEventListener('visibilitychange', () => { if (!document.hidden) beat(); });

  // 같은 오류가 쏟아져도 1분에 몇 개만 — 디버그 로그를 채우지 않게.
  let windowStart = 0;
  let sent = 0;
  const report = (kind, err, extra = {}) => {
    const now = performance.now();
    if (now - windowStart > 60000) { windowStart = now; sent = 0; }
    if (++sent > ERROR_LIMIT_PER_MIN) return;
    const message = String((err && (err.message || err.reason)) || err || '').slice(0, 300);
    const stack = String((err && err.stack) || '').split('\n').slice(0, 4).join(' | ').slice(0, 500);
    invoke('perf_log', { kind: 'error', data: { kind, message, stack, ...extra } }).catch(() => {});
  };
  window.addEventListener('error', (e) => {
    // 그림·스크립트 같은 요소의 읽기 실패(포착 단계로 올라온 것)는 오류가 아니다.
    if (e.target && e.target !== window) return;
    report('error', e.error || e.message, { source: String(e.filename || '').split('/').pop(), line: e.lineno });
  });
  window.addEventListener('unhandledrejection', (e) => report('unhandledrejection', e.reason));
}
