// 성능 UI 회귀 검사 — 숫자를 재서 예산을 넘지 않는지 본다(모의 IPC, 헤드리스 Chromium).
// P01 시작 중 앱 코드가 일으키는 강제 동기 레이아웃(스크립트가 크기·위치를 읽어 전체 문서 레이아웃을 당기는 것)이 거의 없는지.
//     — 예전엔 숨은 멀티라이브·멀티채팅 세그먼트 판(motion.js place)과 채팅 분할기(splitter.js apply)가 시작마다 일으켰다.
// P02 화면마다 10초 가만히 둘 때 스크립트·레이아웃·요청이 거의 없는지(주기적 다시 그리기·폴링 회귀).
// P03 첫 화면 아래의 목록 이미지는 지연 로딩(loading="lazy")인지.
// P04 붐비는 채팅(초당 200개, CPU 4배 감속)에서 넘친 줄을 모았다가 한 번에 지우는지(trimFront), 프레임이 버티는지.
//     — 예전엔 플러시마다 앞에서 몇 줄씩 지워 남은 줄 전체가 다시 배치됐다(프레임 25~47%가 20ms 초과).
// P05 창이 가려진 동안 채팅 줄을 붙여도 바닥 따라가기(snap)가 레이아웃을 당기지 않는지, 다시 보이면 바닥으로 내리는지.
//     — 예전엔 숨은 창에서도 줄을 붙일 때마다 scrollHeight 를 읽어 목록 전체를 다시 배치했다.
// 저장소 루트에서: node tools/ui-regression/perf.mjs   (Playwright 가 필요하다)
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, go, openApp, base, mockTauri } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}
const browser = await chromium.launch();

// ── P01 시작 중 강제 동기 레이아웃 ──
await check('P01 startup: app code forces almost no synchronous layout', async () => {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const page = await ctx.newPage();
  const cdp = await ctx.newCDPSession(page);
  const events = [];
  cdp.on('Tracing.dataCollected', (d) => events.push(...d.value));
  const done = new Promise((r) => cdp.once('Tracing.tracingComplete', r));
  await cdp.send('Tracing.start', { categories: 'devtools.timeline,disabled-by-default-devtools.timeline.stack', transferMode: 'ReportEvents' });
  await page.addInitScript(mockTauri, [base, 'dark']);
  await page.goto(base);
  await page.waitForFunction(() => !document.getElementById('splash') || !document.getElementById('splash').isConnected, null, { timeout: 20000 });
  await cdp.send('Tracing.end');
  await done;
  // 스택이 있는 Layout = 스크립트가 당긴 레이아웃. 환영 안내(onboarding, 처음 한 번)는 모의 실행마다 떠서 빼고,
  // 플레이어 iframe(player.html — hls 포크, isSurfaceHidden 이 숨김 확인에 getClientRects 를 쓴다)은 이 검사 범위 밖이다.
  const forced = events.filter((e) => e.name === 'Layout' && e.ph === 'X' && e.args?.beginData?.stackTrace?.length)
    .map((e) => ({ at: e.args.beginData.stackTrace[0], ms: e.dur / 1000 }))
    .filter((x) => !/onboarding\.js|player\.html/.test(x.at.url || ''));
  const named = forced.map((x) => `${x.at.functionName}@${(x.at.url || '').split('/').pop()}:${x.at.lineNumber + 1} ${x.ms.toFixed(1)}ms`);
  assert.ok(!named.some((n) => /motion\.js|splitter\.js/.test(n)), `세그먼트 판·채팅 분할기가 시작 중 레이아웃을 당김: ${named}`);
  assert.ok(forced.length <= 2, `강제 레이아웃 ${forced.length}회: ${named}`);
  await ctx.close();
});

// ── P02 대기 비용 ──
{
  const { ctx, page } = await openApp(browser, { width: 1280 });
  const cdp = await ctx.newCDPSession(page);
  await cdp.send('Performance.enable');
  const metrics = async () => Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map((m) => [m.name, m.value]));
  // 팔로잉은 썸네일이 빈 카드를 몇 번 다시 채운다(pages.js fillThumbsLater, 횟수 제한) — 그만큼만 허용
  const IPC_BUDGET = { following: 3 };
  for (const route of ['home', 'following', 'popular', 'multichat', 'multilive', 'library', 'stats', 'settings']) {
    await check(`P02 ${route}: stays quiet while idle (10s)`, async () => {
      await go(page, route);
      // 화면이 나눠 불러오기를 마칠 때까지(요청이 2초 동안 멈출 때까지, 최대 8초) 기다린 뒤부터 잰다
      for (let last = -1, still = 0, t = 0; still < 4 && t < 16; t++) {
        await page.waitForTimeout(500);
        const n = await page.evaluate(() => window.__calls.length);
        still = n === last ? still + 1 : 0;
        last = n;
      }
      const a = await metrics();
      const c0 = await page.evaluate(() => window.__calls.length);
      await page.waitForTimeout(10000);
      const b = await metrics();
      const calls = await page.evaluate((n) => window.__calls.slice(n), c0);
      const script = (b.ScriptDuration - a.ScriptDuration) * 1000;
      const layouts = b.LayoutCount - a.LayoutCount;
      assert.ok(script < 60, `스크립트 ${script.toFixed(0)}ms`);
      assert.ok(layouts <= 10, `레이아웃 ${layouts}회`);
      assert.ok(calls.length <= (IPC_BUDGET[route] || 0), `요청 ${calls.length}회: ${calls.join(', ')}`);
    });
  }

  // ── P03 이미지 지연 로딩 ──
  for (const route of ['popular', 'home', 'following']) {
    await check(`P03 ${route}: list images below the first screen load lazily`, async () => {
      await go(page, route);
      await page.waitForTimeout(800);
      const eager = await page.evaluate((r) => {
        const pg = document.querySelector(`[data-page="${r}"]`);
        return [...pg.querySelectorAll('img')].filter((img) => img.getBoundingClientRect().top > innerHeight * 1.5 && img.getAttribute('loading') !== 'lazy')
          .map((img) => `${img.parentElement?.className || ''} > img`).slice(0, 5);
      }, route);
      assert.deepEqual(eager, []);
    });
  }
  await ctx.close();
}

// ── P04 채팅 폭주 ──
await check('P04 chat burst: trims overflow in batches and keeps frames', async () => {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const page = await ctx.newPage();
  const cdp = await ctx.newCDPSession(page);
  await page.addInitScript(mockTauri, [base, 'dark']);
  // 모의 listen 은 아무것도 하지 않는다 — 채팅 이벤트를 흘려 보낼 수 있게 바꾼다.
  await page.addInitScript(() => {
    const m = new Map();
    window.__TAURI__.event = { listen: async (n, f) => { if (!m.has(n)) m.set(n, new Set()); m.get(n).add(f); return () => m.get(n).delete(f); } };
    window.__emit = (n, p) => { for (const f of m.get(n) || []) f({ event: n, payload: p }); };
    const inv = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async (c, a) => { try { return await inv(c, a); } catch { return null; } };
  });
  await page.goto(base);
  await page.waitForFunction(() => !document.getElementById('splash') || !document.getElementById('splash').isConnected, null, { timeout: 20000 });
  await page.evaluate(async () => {
    const { ChatPanel } = await import('/js/chat.js');
    const box = document.createElement('div');
    box.style.cssText = 'position:fixed;right:0;top:0;width:360px;height:860px;z-index:9999;display:flex;flex-direction:column';
    box.innerHTML = document.querySelector('#viewer .chat').outerHTML;
    document.body.appendChild(box);
    const root = box.firstElementChild;
    root.hidden = false;
    root.style.cssText = 'display:flex;flex-direction:column;height:100%;width:100%';
    root.querySelectorAll('.chat-tools,.chat-tools-toggle').forEach((x) => x.remove());
    const panel = new ChatPanel(root, { isLoggedIn: () => true });
    await panel.connect('cc1', 'ch1').catch(() => {});
    panel.active = true;
    panel.chatChannelId = null;
    window.__panel = panel;
    window.__trims = [];
    new MutationObserver((ms) => { const k = ms.reduce((a, r) => a + [...r.removedNodes].filter((x) => !x.classList?.contains('empty')).length, 0); if (k) window.__trims.push(k); }).observe(panel.list, { childList: true });
  });
  await cdp.send('Emulation.setCPUThrottlingRate', { rate: 4 });
  const r = await page.evaluate(async () => {
    let n = 0;
    const mk = () => ({ id: `syn-${++n}`, kind: 'chat', nickname: `테스트${n % 50}`, content: `합성 메시지 ${n} 가나다라마바사 ㅋㅋㅋㅋ`, emojis: {}, role: 'common_user', badges: [], time: Date.now() });
    const d = []; let last = performance.now(); let run = true;
    const tick = (t) => { d.push(t - last); last = t; if (run) requestAnimationFrame(tick); };
    requestAnimationFrame(tick);
    const until = performance.now() + 4000;
    while (performance.now() < until) {
      window.__emit('chat://event', { type: 'messages', messages: Array.from({ length: 10 }, mk) });
      await new Promise((res) => setTimeout(res, 50));
    }
    run = false;
    await new Promise((res) => setTimeout(res, 300));
    d.shift();
    const max = window.__panel.maxLines;
    return { over20: d.filter((x) => x > 20).length / d.length, lines: window.__panel.list.children.length, max, trims: window.__trims };
  });
  await cdp.send('Emulation.setCPUThrottlingRate', { rate: 1 });
  assert.ok(r.lines <= r.max * 1.25 + 1 && r.lines >= r.max, `줄 수 ${r.lines}(상한 ${r.max})`);
  assert.ok(r.trims.length <= 15, `앞에서 지운 횟수 ${r.trims.length} — 붙일 때마다 지우고 있다`);
  assert.ok(r.trims.every((k) => k >= 20), `한 번에 지운 줄 ${r.trims}`);
  // 프레임은 기계에 따라 흔들려 넉넉히 — 고치기 전 25~47%, 고친 뒤 약 4%.
  assert.ok(r.over20 < 0.15, `20ms 넘은 프레임 ${(r.over20 * 100).toFixed(1)}%`);
  await ctx.close();
});

// ── P05 가려진 창의 채팅 ──
await check('P05 hidden window: chat appends force no layout, snaps back to bottom when shown', async () => {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const page = await ctx.newPage();
  const cdp = await ctx.newCDPSession(page);
  await page.addInitScript(mockTauri, [base, 'dark']);
  await page.goto(base);
  await page.waitForFunction(() => !document.getElementById('splash') || !document.getElementById('splash').isConnected, null, { timeout: 20000 });
  await page.evaluate(async () => {
    const { followAppend, trimFront } = await import('/js/chat-follow.js');
    const list = document.createElement('div');
    list.className = 'chat-list';
    list.style.cssText = 'position:fixed;right:0;top:0;width:320px;height:400px;overflow:auto;z-index:9999';
    document.body.appendChild(list);
    let hidden = false;
    Object.defineProperty(document, 'hidden', { get: () => hidden, configurable: true });
    window.__setHidden = (v) => { hidden = v; document.dispatchEvent(new Event('visibilitychange')); };
    let n = 0;
    // 멀티라이브 패인 채팅처럼 이벤트마다 붙인다
    window.__burst = (k) => { for (let i = 0; i < k; i++) followAppend(list, () => { list.insertAdjacentHTML('beforeend', `<div class="chat-line">줄 ${++n} 가나다라마바사</div>`); trimFront(list, 300); }); };
    window.__list = list;
    window.__burst(50);
    window.__setHidden(true);
  });
  const evs = [];
  cdp.on('Tracing.dataCollected', (d) => evs.push(...d.value));
  const done = new Promise((r) => cdp.once('Tracing.tracingComplete', r));
  await cdp.send('Tracing.start', { categories: 'devtools.timeline,disabled-by-default-devtools.timeline.stack', transferMode: 'ReportEvents' });
  await page.evaluate(() => window.__burst(400));
  await cdp.send('Tracing.end');
  await done;
  const forced = evs.filter((e) => e.name === 'Layout' && e.ph === 'X' && e.args?.beginData?.stackTrace?.some((f) => /chat-follow\.js|chat\.js/.test(f.url || '')));
  // 고치기 전: 줄마다 snap 이 scrollHeight 를 읽어 400번 강제 레이아웃
  assert.equal(forced.length, 0, `숨은 창에서 채팅이 레이아웃을 ${forced.length}번 당김`);
  const gap = await page.evaluate(async () => {
    window.__setHidden(false);
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    const l = window.__list;
    return l.scrollHeight - l.clientHeight - l.scrollTop;
  });
  assert.ok(gap <= 1, `다시 보였는데 바닥에서 ${gap}px 떨어져 있음`);
  await ctx.close();
});

await browser.close();
server.close();
for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : `\n     ${r.error.split('\n').slice(0, 10).join('\n     ')}`}`);
const failed = results.filter((r) => !r.ok).length;
console.log(`\n${results.length - failed}/${results.length} passed`);
process.exitCode = failed ? 1 : 0;
