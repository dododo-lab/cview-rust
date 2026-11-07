// 모의 IPC 로 UI 기준 수치(시작·IPC·화면 전환·스크롤·채팅 폭주)를 잰다. 저장소 루트에서: node perf/tools/ui/measure-ui.mjs "$PWD" [CPU감속배수]  (Playwright 필요, NODE_PATH)
import { createRequire } from 'node:module';
import path from 'node:path';
const repo = process.argv[2];
const RATE = Number(process.argv[3] || 1);
const { base, mockTauri, go } = await import(path.join(repo, 'tools/ui-regression/mock-app.mjs'));
const require = createRequire(import.meta.url);
const { chromium } = require('playwright');
const browser = await chromium.launch();
const out = {};
const q = (a, p) => { const s = [...a].sort((x, y) => x - y); return s.length ? s[Math.min(s.length - 1, Math.floor(p * s.length))] : NaN; };

// 이벤트 버스(모의 listen 은 아무것도 안 함) — mockTauri 뒤에 덮어쓴다
function bus() {
  const m = new Map();
  window.__TAURI__.event = {
    listen: async (n, f) => { if (!m.has(n)) m.set(n, new Set()); const w = (e) => f(e); m.get(n).add(w); return () => m.get(n).delete(w); },
    emit: async (n, p) => { for (const f of m.get(n) || []) f({ event: n, payload: p }); },
  };
  window.__emit = (n, p) => { for (const f of m.get(n) || []) f({ event: n, payload: p }); };
}

async function open({ width = 1280, height = 900 } = {}) {
  const ctx = await browser.newContext({ viewport: { width, height }, colorScheme: 'dark' });
  const page = await ctx.newPage();
  const cdp = await ctx.newCDPSession(page);
  if (RATE > 1) await cdp.send('Emulation.setCPUThrottlingRate', { rate: RATE });
  await page.addInitScript(mockTauri, [base, 'dark']);
  await page.addInitScript(bus);
  const reqs = [];
  page.on('request', (r) => reqs.push({ url: r.url(), type: r.resourceType() }));
  return { ctx, page, cdp, reqs };
}

// ── 시작 ──
{
  const runs = [];
  for (let i = 0; i < 5; i++) {
    const { ctx, page, cdp, reqs } = await open();
    await cdp.send('Performance.enable');
    const t0 = Date.now();
    await page.goto(base);
    await page.waitForFunction(() => !document.getElementById('splash') || !document.getElementById('splash').isConnected, null, { timeout: 30000 });
    const wall = Date.now() - t0;
    await page.waitForTimeout(1500);
    const r = await page.evaluate(() => {
      const nav = performance.getEntriesByType('navigation')[0];
      const fcp = performance.getEntriesByName('first-contentful-paint')[0];
      const res = performance.getEntriesByType('resource');
      const inter = performance.getEntriesByName('cview:interactive')[0];
      const calls = {}; for (const c of window.__calls) calls[c] = (calls[c] || 0) + 1;
      return { dcl: Math.round(nav.domContentLoadedEventEnd), load: Math.round(nav.loadEventEnd), fcp: fcp && Math.round(fcp.startTime), interactive: inter && Math.round(inter.startTime),
        nodes: document.getElementsByTagName('*').length, ipc: window.__calls.length, calls,
        jsKB: Math.round(res.filter((x) => /\.js($|\?)/.test(x.name)).reduce((a, x) => a + (x.decodedBodySize || 0), 0) / 1024),
        heapMB: performance.memory ? +(performance.memory.usedJSHeapSize / 1048576).toFixed(1) : null };
    });
    const pm = Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map((m) => [m.name, m.value]));
    r.splashGone = wall; r.scriptMs = Math.round(pm.ScriptDuration * 1000); r.layoutMs = Math.round(pm.LayoutDuration * 1000); r.styleMs = Math.round(pm.RecalcStyleDuration * 1000);
    r.req = { js: reqs.filter((x) => x.type === 'script').length, css: reqs.filter((x) => x.type === 'stylesheet').length, img: reqs.filter((x) => x.type === 'image').length, font: reqs.filter((x) => x.type === 'font').length, doc: reqs.filter((x) => x.type === 'document').length, all: reqs.length };
    runs.push(r);
    await ctx.close();
  }
  const med = (k) => q(runs.map((r) => r[k]), 0.5);
  out.startup = { dcl: med('dcl'), fcp: med('fcp'), interactive: med('interactive'), splashGone: med('splashGone'), scriptMs: med('scriptMs'), layoutMs: med('layoutMs'), styleMs: med('styleMs'), nodes: med('nodes'), ipc: med('ipc'), jsKB: med('jsKB'), heapMB: med('heapMB'), req: runs[0].req, calls: runs[0].calls };
}

// ── 화면 전환·노드·힙 ──
{
  const { ctx, page, cdp } = await open();
  await page.goto(base);
  await page.waitForFunction(() => !document.getElementById('splash') || !document.getElementById('splash').isConnected, null, { timeout: 30000 });
  await page.waitForTimeout(1000);
  await cdp.send('Performance.enable');
  const routes = ['following', 'popular', 'categories', 'library', 'stats', 'settings', 'multichat', 'multilive', 'search', 'home'];
  out.nav = {};
  for (let pass = 0; pass < 3; pass++) for (const r of routes) {
    const m0 = Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map((m) => [m.name, m.value]));
    const ms = await page.evaluate(async (route) => {
      const t0 = performance.now();
      window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route, params: {} } }));
      await new Promise((res) => requestAnimationFrame(() => setTimeout(res, 0)));
      return performance.now() - t0;
    }, r);
    await page.waitForTimeout(700);
    const m1 = Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map((m) => [m.name, m.value]));
    const nodes = await page.evaluate(() => document.getElementsByTagName('*').length);
    (out.nav[r] ||= []).push({ ms: Math.round(ms), script: Math.round((m1.ScriptDuration - m0.ScriptDuration) * 1000), layout: Math.round((m1.LayoutDuration - m0.LayoutDuration) * 1000), nodes });
  }
  for (const r of routes) { const a = out.nav[r]; out.nav[r] = { first: a[0], warmMs: q(a.slice(1).map((x) => x.ms), 0.5), nodes: a[2].nodes }; }
  out.heapAfterTourMB = await page.evaluate(() => performance.memory && +(performance.memory.usedJSHeapSize / 1048576).toFixed(1));
  // 화면을 30바퀴 돌린 뒤 노드·리스너 증가(누수)
  const before = await cdp.send('Memory.getDOMCounters');
  for (let i = 0; i < 30; i++) for (const r of routes) await go(page, r);
  await page.waitForTimeout(1500);
  await cdp.send('HeapProfiler.collectGarbage');
  const after = await cdp.send('Memory.getDOMCounters');
  out.leak30 = { before, after };
  out.heapAfter30MB = await page.evaluate(() => performance.memory && +(performance.memory.usedJSHeapSize / 1048576).toFixed(1));

  // ── 스크롤 프레임(팔로잉·인기, 모의 데이터는 짧아 목록을 복제해 늘린다) ──
  out.scroll = {};
  for (const r of ['popular', 'following']) {
    await go(page, r); await page.waitForTimeout(800);
    out.scroll[r] = await page.evaluate(async (route) => {
      const pg = document.querySelector(`[data-page="${route}"]`) || document.querySelector('.page:not([hidden])');
      if (!pg) return null;
      const grid = pg.querySelector('.grid, .card-grid, [class*="grid"]');
      if (grid) { const kids = [...grid.children]; for (let k = 0; k < 15; k++) for (const c of kids) grid.appendChild(c.cloneNode(true)); }
      let sc = pg; while (sc && sc.scrollHeight <= sc.clientHeight + 10) sc = sc.parentElement;
      sc = sc || document.scrollingElement;
      const d = []; let last = performance.now(); let run = true; let y = 0;
      const tick = (t) => { d.push(t - last); last = t; y += 40; sc.scrollTop = y % (sc.scrollHeight - sc.clientHeight); if (run) requestAnimationFrame(tick); };
      requestAnimationFrame(tick); await new Promise((r) => setTimeout(r, 3000)); run = false; d.shift();
      const s = d.sort((a, b) => a - b); const qq = (p) => s[Math.floor(p * (s.length - 1))];
      return { frames: s.length, p50: +qq(0.5).toFixed(1), p95: +qq(0.95).toFixed(1), max: +s[s.length - 1].toFixed(1), over20: +(100 * s.filter((x) => x > 20).length / s.length).toFixed(1), cards: grid ? grid.children.length : 0 };
    }, r);
  }

  // ── 채팅 폭주 처리량(본 시청 화면 채팅 패널) ──
  await page.evaluate(async () => {
    const inv = window.__TAURI__.core.invoke; window.__TAURI__.core.invoke = async (c, a) => { try { return await inv(c, a); } catch { return null; } };
    const { ChatPanel } = await import('/js/chat.js');
    const src = document.querySelector('#viewer .chat') || document.querySelector('.chat');
    const box = document.createElement('div'); box.style.cssText = 'position:fixed;right:0;top:0;width:360px;height:860px;z-index:9999;display:flex;flex-direction:column;background:#111';
    box.innerHTML = src.outerHTML; document.body.appendChild(box);
    const root = box.firstElementChild; root.hidden = false; root.style.cssText = 'display:flex;flex-direction:column;height:100%;width:100%';
    root.querySelectorAll('.chat-tools,.chat-tools-toggle').forEach((x) => x.remove());
    window.__panel = new ChatPanel(root, { isLoggedIn: () => true });
    await window.__panel.connect('cc1', 'ch1').catch(() => {});
    window.__panel.active = true; window.__panel.chatChannelId = null;
  });
  out.chat = [];
  for (const rate of [50, 200, 800]) {
    const r = await page.evaluate(async (rate) => {
      let n = 0;
      const mk = () => ({ id: `syn-${++n}`, kind: 'chat', nickname: `테스트${n % 50}`, content: `합성 메시지 ${n} 가나다라마바사 아자차카타파하 ㅋㅋㅋㅋ {:d_1:}`, emojis: { d_1: '/__img/avatar/1.svg' }, role: 'common_user', badges: [], time: Date.now() });
      const d = []; let last = performance.now(); let run = true;
      const tick = (t) => { d.push(t - last); last = t; if (run) requestAnimationFrame(tick); }; requestAnimationFrame(tick);
      const per = Math.max(1, Math.round(rate / 20)); const until = performance.now() + 4000; let busy = 0;
      while (performance.now() < until) { const a = performance.now(); window.__emit('chat://event', { type: 'messages', messages: Array.from({ length: per }, mk) }); busy += performance.now() - a; await new Promise((r) => setTimeout(r, 50)); }
      run = false; await new Promise((r) => setTimeout(r, 200)); d.shift();
      const s = d.sort((a, b) => a - b); const qq = (p) => s[Math.floor(p * (s.length - 1))];
      return { rate, frames: s.length, p50: +qq(0.5).toFixed(1), p95: +qq(0.95).toFixed(1), max: +s[s.length - 1].toFixed(1), over20: +(100 * s.filter((x) => x > 20).length / s.length).toFixed(1), emitMs: +busy.toFixed(0), lines: window.__panel.list.children.length };
    }, rate);
    out.chat.push(r);
  }
  const lt = await cdp.send('Performance.getMetrics');
  await ctx.close();
}
console.log(JSON.stringify(out, null, 1));
await browser.close();
process.exit(0);
