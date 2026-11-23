// 화면 밖 탐색 페이지 이미지 놓기 회귀 검사(모의 IPC·헤드리스 브라우저) — 메모리 최적화 계획(2026-11-23) Phase 2, MEM-22.
// H01 홈에서 다른 메뉴로 가면 15초 동안은 그대로, 그 뒤 홈의 <img src> 를 모두 놓는다(data-hib-src). 홈으로 돌아오면 그리기 전에
//     같은 주소로 되돌린다 — 이미지 수·주소가 떠나기 전과 같다.
// H02 15초 안에 돌아오면 놓지 않는다.
// H03 OS 메모리 압박(app://memory-pressure)이 오면 숨은 탐색 페이지를 기다리지 않고 바로 놓는다. 보이는 페이지는 건드리지 않는다.
// H04 놓는 대상은 탐색 페이지뿐 — 설정·멀티라이브 등은 그대로.
// 저장소 루트에서: node tools/ui-regression/memory-hibernate.mjs   (CVIEW_UI_BROWSER=webkit 이면 WebKit)
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, base, mockTauri, go } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const WEBKIT = process.env.CVIEW_UI_BROWSER === 'webkit';
const browser = await (WEBKIT ? pw.webkit : pw.chromium).launch();
const results = [];
async function check(id, fn) {
  if (process.env.CVIEW_ONLY && !id.startsWith(process.env.CVIEW_ONLY)) return;
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message.split('\n').slice(0, 10).join('\n') }); }
}

function eventMock() {
  const listeners = {};
  window.__TAURI__.event.listen = async (name, cb) => { (listeners[name] ||= []).push(cb); return () => {}; };
  window.__emit = (name, payload) => { for (const cb of listeners[name] || []) cb({ payload }); };
  try { localStorage.setItem('cview.welcomed.v1', '1'); } catch { /* 없음 */ }
}

async function open() {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 }, reducedMotion: 'reduce' });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => { const m = String(e); if (!/ResizeObserver loop/.test(m)) errors.push(m); });
  await page.clock.install();
  await page.addInitScript(mockTauri, [base, 'dark']);
  await page.addInitScript(eventMock);
  await page.goto(base);
  await page.waitForSelector('[data-page="home"]:not([hidden])', { state: 'attached' });
  await page.clock.runFor(3000);
  return { ctx, page, errors };
}

/** 페이지 안 이미지 — 주소 있는 것, 놓은 것. */
const imgs = (page, id) => page.evaluate((pid) => {
  const root = document.querySelector(`[data-page="${pid}"]`);
  const all = [...root.querySelectorAll('img')];
  return {
    live: all.filter((i) => i.getAttribute('src') && !i.getAttribute('src').startsWith('data:')).map((i) => i.getAttribute('src')),
    held: all.filter((i) => i.dataset.hibSrc).length,
  };
}, id);

await check('H01 hidden home releases <img src> after 15s and restores the same images on return', async () => {
  const { ctx, page, errors } = await open();
  const before = await imgs(page, 'home');
  assert.ok(before.live.length >= 5, `홈 이미지가 너무 적다(${before.live.length}) — 모의 데이터 확인`);
  await go(page, 'settings');
  await page.clock.runFor(10000);
  assert.equal((await imgs(page, 'home')).held, 0, '15초 안에는 놓지 않는다');
  await page.clock.runFor(6000);
  const asleep = await imgs(page, 'home');
  assert.deepEqual([asleep.live.length, asleep.held], [0, before.live.length], `숨은 홈 ${JSON.stringify({ live: asleep.live.length, held: asleep.held })}`);
  await go(page, 'home');
  await page.clock.runFor(50);
  const awake = await imgs(page, 'home');
  assert.equal(awake.held, 0);
  assert.deepEqual(awake.live.slice().sort(), before.live.slice().sort(), '돌아오면 같은 이미지');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('H02 returning within 15s keeps images', async () => {
  const { ctx, page } = await open();
  const before = await imgs(page, 'home');
  await go(page, 'settings');
  await page.clock.runFor(8000);
  await go(page, 'home');
  await page.clock.runFor(20000);
  const now = await imgs(page, 'home');
  assert.deepEqual([now.live.length, now.held], [before.live.length, 0]);
  await ctx.close();
});

await check('H03 OS memory pressure releases hidden explore pages at once; the visible page is untouched', async () => {
  const { ctx, page, errors } = await open();
  const home = await imgs(page, 'home');
  await go(page, 'popular');
  await page.clock.runFor(2000);
  const popular = await imgs(page, 'popular');
  await page.evaluate(() => window.__emit('app://memory-pressure', 'warn'));
  await page.clock.runFor(10);
  assert.deepEqual([(await imgs(page, 'home')).live.length, (await imgs(page, 'home')).held], [0, home.live.length], '숨은 홈은 바로');
  assert.equal((await imgs(page, 'popular')).live.length, popular.live.length, '보이는 인기는 그대로');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('H04 only explore pages are released (settings stays)', async () => {
  const { ctx, page } = await open();
  await go(page, 'settings');
  await page.clock.runFor(1000);
  const settings = await imgs(page, 'settings');
  await go(page, 'home');
  await page.clock.runFor(20000);
  await page.evaluate(() => window.__emit('app://memory-pressure', 'critical'));
  await page.clock.runFor(10);
  const after = await imgs(page, 'settings');
  assert.deepEqual([after.live.length, after.held], [settings.live.length, 0]);
  await ctx.close();
});

await browser.close();
server.close();
let failed = 0;
for (const r of results) {
  console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : ` — ${r.error}`}`);
  if (!r.ok) failed++;
}
console.log(`${results.length - failed}/${results.length} passed`);
process.exit(failed ? 1 : 0);
