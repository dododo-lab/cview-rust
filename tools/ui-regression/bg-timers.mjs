// 가려진 창 정밀 타이머(app-ui/player/bg-timers.js) 회귀 검사 — 플레이어 문서를 그대로 띄워 본다.
// 배경: WebKit 은 가려진 페이지의 DOM 타이머를 1초 경계로 모아 hls.js LL 일정이 밀리고 지연이 늘었다. 플레이어는 가려진 채
//   재생 중일 때만 이 페이지의 타이머를 Worker 로 돌린다(백그라운드 지연 2026-11-25).
// T01 플레이어 문서가 bg-timers.js 를 hls.js 보다 먼저 싣는다 — 전역 타이머가 이 모듈 것이고 오류가 없다.
// T02 평소(정밀 모드 꺼짐): setTimeout 인자·순서, clearTimeout, setInterval 반복·clearInterval 이 원래처럼.
// T03 정밀 모드 켜고 끄기: 걸려 있던 반복·한 번 타이머가 남은 시간대로 옮겨 가 계속 돌고, 지우기도 두 모드에서 된다.
// T04 판정(bgTimersReconcile): 가려짐 + 재생 중 + 로딩 중 + hls 있음일 때만 켜고, 정지·로딩 정지·보임이면 끈다.
// T05 안정 우선(LL 끔·세그먼트 + 1초)은 Worker 를 쓸 수 없을 때만 — 쓸 수 있으면 가려져도 웹식 LL 그대로.
// T06 지표의 preciseTimers 와 웹식 따라잡기: 가려졌어도 정밀 모드면 1.03배 따라잡기가 이어진다.
// 저장소 루트에서: node tools/ui-regression/bg-timers.mjs   (CVIEW_UI_BROWSER=webkit 이면 WebKit)
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, base } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const WEBKIT = process.env.CVIEW_UI_BROWSER === 'webkit';
const browser = await (WEBKIT ? pw.webkit : pw.chromium).launch();
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}

const ctx = await browser.newContext({ viewport: { width: 640, height: 360 } });
const page = await ctx.newPage();
page.setDefaultTimeout(8000);
const errors = [];
page.on('pageerror', (e) => errors.push(String(e)));
await page.addInitScript(() => {
  window.__msgs = [];
  window.webkit = { messageHandlers: { player: { postMessage: (m) => window.__msgs.push(JSON.parse(JSON.stringify(m))) } } };
  // 가려짐을 검사에서 정한다(헤드리스 브라우저는 늘 보임).
  window.__hidden = false;
  Object.defineProperty(Document.prototype, 'hidden', { configurable: true, get: () => window.__hidden });
  Object.defineProperty(Document.prototype, 'visibilityState', { configurable: true, get: () => (window.__hidden ? 'hidden' : 'visible') });
});
await page.goto(`${base}player/player.html`);
await page.waitForFunction(() => typeof window.Hls === 'function' && typeof window.bgTimersReconcile === 'function');

try {
  await check('T01 bg-timers.js 가 hls.js 보다 먼저, 전역 타이머 소유', async () => {
    const r = await page.evaluate(() => {
      const srcs = [...document.scripts].map((s) => s.getAttribute('src')).filter(Boolean);
      return { srcs, api: !!window.__cviewBgTimers, owned: !/native code/.test(String(window.setTimeout)),
        active: window.__cviewBgTimers.active(), available: window.__cviewBgTimers.available() };
    });
    assert.deepEqual(r.srcs.slice(0, 2), ['bg-timers.js', 'hls.min.js']);
    assert.ok(r.api && r.owned, JSON.stringify(r));
    assert.equal(r.active, false, '처음에는 원래 타이머');
    assert.equal(r.available, true);
    assert.deepEqual(errors, []);
  });

  await check('T02 평소 — 원래 타이머처럼', async () => {
    const r = await page.evaluate(() => new Promise((done) => {
      const log = [];
      setTimeout((a, b) => log.push(`b${a}${b}`), 40, 'x', 'y');
      setTimeout(() => log.push('a'), 10);
      const c = setTimeout(() => log.push('never'), 20);
      clearTimeout(c);
      let n = 0;
      const iv = setInterval(() => { n++; if (n === 3) clearInterval(iv); }, 15);
      setTimeout(() => done({ log, n, pending: window.__cviewBgTimers.pending() }), 200);
    }));
    assert.deepEqual(r.log, ['a', 'bxy']);
    assert.equal(r.n, 3, '반복은 지운 뒤 멈춘다');
  });

  await check('T03 정밀 모드 켜고 끄기 — 걸린 타이머가 옮겨 가 계속 돈다', async () => {
    const r = await page.evaluate(() => new Promise((done) => {
      const T = window.__cviewBgTimers;
      const t0 = performance.now();
      const ticks = [];
      const iv = setInterval(() => ticks.push(T.active() ? 'W' : 'N'), 30);
      let lateAt = 0;
      setTimeout(() => { lateAt = performance.now() - t0; }, 150);   // 켜기 전에 건 한 번 타이머
      const killed = setTimeout(() => ticks.push('never'), 170);
      setTimeout(() => { T.setActive(true); }, 60);
      setTimeout(() => clearTimeout(killed), 100);                     // Worker 쪽으로 옮겨 간 뒤 지우기
      setTimeout(() => { T.setActive(false); }, 260);
      setTimeout(() => {
        clearInterval(iv);
        const after = ticks.length;
        setTimeout(() => done({ ticks, lateAt, after, end: ticks.length, active: T.active() }), 80);
      }, 380);
    }));
    assert.ok(r.ticks.includes('W') && r.ticks.indexOf('W') > 0, `반복 타이머가 Worker 로 옮겨 감: ${r.ticks.join('')}`);
    assert.equal(r.ticks[r.ticks.length - 1], 'N', `끈 뒤 원래 타이머: ${r.ticks.join('')}`);
    assert.ok(!r.ticks.includes('never'), '옮겨 간 타이머도 지워진다');
    assert.ok(r.lateAt >= 140 && r.lateAt < 260, `남은 시간대로 발화: ${r.lateAt}`);
    assert.equal(r.end, r.after, '지운 반복은 멈춘다');
    assert.ok(r.ticks.length >= 9, `반복이 끊기지 않는다: ${r.ticks.length}`);
    assert.equal(r.active, false);
  });

  await check('T04 판정 — 가려진 채 재생·로딩 중일 때만', async () => {
    const r = await page.evaluate(() => {
      const T = window.__cviewBgTimers;
      let paused = false;
      Object.defineProperty(video, 'paused', { configurable: true, get: () => paused });
      const saved = { hls, wants: _wantsPlaying, susp: _suspended };
      const out = {};
      const step = (k) => { bgTimersReconcile(); out[k] = T.active(); };
      hls = { latency: 2.5, targetLatency: 2 };
      _wantsPlaying = true; _suspended = false;
      window.__hidden = false; step('visiblePlaying');
      window.__hidden = true; document.dispatchEvent(new Event('visibilitychange')); out.hiddenEvent = T.active();
      paused = true; video.dispatchEvent(new Event('pause')); out.paused = T.active();
      paused = false; video.dispatchEvent(new Event('timeupdate')); out.resumed = T.active();
      _suspended = true; step('suspended');
      _suspended = false; step('unsuspended');
      _wantsPlaying = false; step('notWanted');
      _wantsPlaying = true; step('wanted');
      hls = null; step('noHls');
      hls = { latency: 2.5, targetLatency: 2 }; step('hlsBack');
      window.__hidden = false; document.dispatchEvent(new Event('visibilitychange')); out.visibleEvent = T.active();
      hls = saved.hls; _wantsPlaying = saved.wants; _suspended = saved.susp;
      delete video.paused;
      bgTimersReconcile();
      return out;
    });
    assert.deepEqual(r, {
      visiblePlaying: false, hiddenEvent: true, paused: false, resumed: true, suspended: false, unsuspended: true,
      notWanted: false, wanted: true, noHls: false, hlsBack: true, visibleEvent: false,
    });
  });

  await check('T05 안정 우선은 Worker 를 쓸 수 없을 때만', async () => {
    const r = await page.evaluate(() => {
      const saved = _webLl;
      _webLl = webLlFreshSession(true);
      const out = {};
      window.__hidden = false; out.visible = webStabilityMode();
      window.__hidden = true; out.hiddenPrecise = webStabilityMode();
      const T = window.__cviewBgTimers;
      const avail = T.available;
      T.available = () => false;
      out.hiddenNoWorker = webStabilityMode();
      T.available = avail;
      window.__hidden = false;
      _webLl = saved;
      return out;
    });
    assert.deepEqual(r, { visible: false, hiddenPrecise: false, hiddenNoWorker: true });
  });

  await check('T06 가려진 정밀 모드에서도 웹식 따라잡기·지표 표시', async () => {
    const r = await page.evaluate(() => {
      const T = window.__cviewBgTimers;
      let paused = false;
      Object.defineProperty(video, 'paused', { configurable: true, get: () => paused });
      Object.defineProperty(video, 'seekable', { configurable: true, get: () => ({ length: 1, start: () => 0, end: () => 10 }) });
      Object.defineProperty(video, 'currentTime', { configurable: true, get: () => 8, set: () => {} });
      Object.defineProperty(video, 'buffered', { configurable: true, get: () => ({ length: 1, start: () => 0, end: () => 13 }) });
      const saved = { hls, wl: _webLl, wants: _wantsPlaying, susp: _suspended, rc: _webRateChangedAt, en: _liveLatencyCorrectionEnabled };
      hls = { latency: 7, targetLatency: 2 };
      _webLl = webLlFreshSession(true);
      _wantsPlaying = true; _suspended = false; _liveLatencyCorrectionEnabled = true;
      _webRateChangedAt = 0; _webSkipUntil = 0;
      video.playbackRate = 1;
      window.__hidden = true;
      bgTimersReconcile();
      webCatchupTick(true);
      const out = { active: T.active(), rateHiddenPrecise: video.playbackRate };
      // 정밀 모드가 아니면(Worker 없음) 종전처럼 가려진 동안 따라잡기를 놓는다.
      T.setActive(false);
      webCatchupTick(true);
      out.rateHiddenThrottled = video.playbackRate;
      video.playbackRate = 1; _webCatchupRate = 0;
      window.__hidden = false;
      hls = saved.hls; _webLl = saved.wl; _wantsPlaying = saved.wants; _suspended = saved.susp;
      _webRateChangedAt = saved.rc; _liveLatencyCorrectionEnabled = saved.en;
      for (const k of ['paused', 'seekable', 'currentTime', 'buffered']) delete video[k];
      bgTimersReconcile();
      return out;
    });
    assert.deepEqual(r, { active: true, rateHiddenPrecise: 1.03, rateHiddenThrottled: 1 });
    const src = await page.evaluate(() => String(collectMetrics));
    assert.match(src, /preciseTimers:\s*bgTimersActive\(\)/, '지표가 정밀 모드를 알린다');
    assert.deepEqual(errors, []);
  });
} finally {
  await browser.close();
  server.close();
}

for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : ` — ${r.error}`}`);
const failed = results.filter((r) => !r.ok).length;
console.log(failed ? `${failed} failed` : `all ${results.length} passed`);
process.exit(failed ? 1 : 0);
