// 디버그 로그 화면 수집기 — 설정 "디버그 로그"가 켜져 있을 때만 돈다(꺼지면 rAF·타이머를 모두 멈춘다).
// 10초마다 Rust perf_log 로 보낸다: 프레임 시간(10초 중 1초 표본의 p50/p95/최대, 50ms 넘은 긴 프레임 수), DOM 요소 수,
// IPC 이벤트·채팅 렌더 줄 수, 재생 중인 영상마다 해상도·화질 단계·버퍼·라이브 지연·떨어진 프레임·버퍼링 횟수와 시간.
// 시작 표시(조작 가능 시점)와 재생 요청 → 첫 라이브 프레임도 한 번씩 남긴다.
import { api, listen } from './api.js';

const FLUSH_MS = 10000;
let on = false;
let raf = 0;
let timer = 0;
let lastFrame = 0;
let frames = [];
let sentStartup = false;

// 프레임은 10초마다 1초만 잰다 — 매 프레임 rAF 를 계속 돌리면 그 자체가 렌더러를 깨워 CPU 를 쓴다.
const SAMPLE_MS = 1000;
let sampleUntil = 0;

function frameLoop(t) {
  if (!on) return;
  if (lastFrame && !document.hidden) frames.push(t - lastFrame);
  lastFrame = t;
  if (t < sampleUntil) raf = requestAnimationFrame(frameLoop);
  else lastFrame = 0;
}

function sampleFrames() {
  if (!on || document.hidden) return;
  sampleUntil = performance.now() + SAMPLE_MS;
  lastFrame = 0;
  raf = requestAnimationFrame(frameLoop);
}

function pct(sorted, p) {
  return sorted.length ? sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))] : null;
}

const round = (v, d = 1) => (v == null || !Number.isFinite(v) ? null : Math.round(v * 10 ** d) / 10 ** d);

/** 영상 하나의 버퍼링(대기) 횟수·시간 — 처음 볼 때 한 번 건다. */
function watchVideo(v) {
  if (v.__cviewPerf) return v.__cviewPerf;
  const s = { count: 0, ms: 0, since: 0 };
  v.addEventListener('waiting', () => { s.count += 1; if (!s.since) s.since = performance.now(); });
  const end = () => { if (s.since) { s.ms += performance.now() - s.since; s.since = 0; } };
  v.addEventListener('playing', end);
  v.addEventListener('pause', end);
  v.__cviewPerf = s;
  return s;
}

function playerStats() {
  const out = [];
  for (const f of document.querySelectorAll('iframe')) {
    let v = null;
    let w = null;
    try { v = f.contentDocument && f.contentDocument.getElementById('video'); w = f.contentWindow; } catch { continue; }
    if (!v || (!v.videoHeight && v.paused)) continue;
    const s = watchVideo(v);
    let buffer = 0;
    try {
      const b = v.buffered;
      for (let i = 0; i < b.length; i += 1) if (b.start(i) <= v.currentTime && v.currentTime <= b.end(i)) buffer = b.end(i) - v.currentTime;
    } catch { /* 무시 */ }
    let latency = null;
    let level = null;
    try {
      latency = w.eval('typeof hls !== "undefined" && hls && hls.latency != null ? hls.latency : null');
      level = w.eval('typeof hls !== "undefined" && hls ? hls.currentLevel : null');
    } catch { /* 플레이어 준비 전 */ }
    const q = v.getVideoPlaybackQuality ? v.getVideoPlaybackQuality() : null;
    const pane = f.closest('.ml-pane');
    out.push({
      where: f.id === 'player-frame' ? 'viewer' : pane ? `pane${pane.dataset.slot}` : 'other',
      playing: !v.paused,
      height: v.videoHeight,
      level,
      buffer_s: round(buffer, 2),
      latency_s: round(latency, 2),
      dropped: q ? q.droppedVideoFrames : null,
      frames: q ? q.totalVideoFrames : null,
      stalls: s.count,
      stall_ms: Math.round(s.ms + (s.since ? performance.now() - s.since : 0)),
    });
    s.count = 0;
    s.ms = 0;
    if (s.since) s.since = performance.now();
  }
  return out;
}

function sendStartupOnce() {
  if (sentStartup) return;
  const mark = performance.getEntriesByName('cview:interactive')[0];
  if (!mark) return;
  sentStartup = true;
  const nav = performance.getEntriesByType('navigation')[0];
  api.perfLog('startup', {
    page_to_interactive_ms: round(mark.startTime),
    dom_content_loaded_ms: nav ? round(nav.domContentLoadedEventEnd) : null,
    load_ms: nav ? round(nav.loadEventEnd) : null,
  }).catch(() => {});
}

function flush() {
  if (!on) return;
  sendStartupOnce();
  const sorted = frames.sort((a, b) => a - b);
  const perf = globalThis.__cviewPerf;
  const page = document.querySelector('.page:not([hidden])');
  api.perfLog('frames', {
    visible: !document.hidden,
    page: page ? page.dataset.page : (document.getElementById('viewer')?.hidden === false ? 'viewer' : null),
    n: sorted.length,
    p50_ms: round(pct(sorted, 0.5)),
    p95_ms: round(pct(sorted, 0.95)),
    max_ms: round(sorted[sorted.length - 1]),
    long_50ms: sorted.filter((x) => x > 50).length,
    dom_nodes: document.getElementsByTagName('*').length,
    heap_mb: performance.memory ? round(performance.memory.usedJSHeapSize / 1048576) : null,
    ipc_events: perf ? perf.ipc : null,
    chat_lines: perf ? perf.chatLines : null,
  }).catch(() => {});
  if (perf) { perf.ipc = 0; perf.chatLines = 0; }
  frames = [];
  sampleFrames();
  const players = playerStats();
  if (players.length) api.perfLog('player', { players }).catch(() => {});
}

// 재생 요청 → 첫 라이브 프레임
function watchFirstFrame() {
  window.addEventListener('cview:play', () => {
    if (!on) return;
    const t0 = performance.now();
    const f = document.getElementById('player-frame');
    let v = null;
    try { v = f && f.contentDocument.getElementById('video'); } catch { return; }
    if (!v) return;
    const done = () => {
      v.removeEventListener('playing', done);
      api.perfLog('first_live_frame', { ms: round(performance.now() - t0) }).catch(() => {});
    };
    v.addEventListener('playing', done);
  });
}

function start() {
  if (on) return;
  on = true;
  globalThis.__cviewPerf = { ipc: 0, chatLines: 0 };
  lastFrame = 0;
  frames = [];
  sampleFrames();
  timer = setInterval(flush, FLUSH_MS);
  sendStartupOnce();
}

function stop() {
  on = false;
  cancelAnimationFrame(raf);
  clearInterval(timer);
  delete globalThis.__cviewPerf;
}

export async function installPerfCollector() {
  watchFirstFrame();
  listen('perf://enabled', (enabled) => (enabled ? start() : stop()));
  try {
    const s = await api.perfLogStatus();
    if (s && s.enabled) start();
  } catch { /* 앱 밖 */ }
}
