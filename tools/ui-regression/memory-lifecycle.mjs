// 메모리 수명주기 회귀 검사(모의 IPC·헤드리스 브라우저) — 메모리 최적화 계획(2026-11-23) Phase 1.
// L01 보조 창(?w=chat·?w=player)은 멀티라이브를 그리지 않는다 — 메인 창에서 4패인을 보는 동안 멀티라이브 상태가 모든 창으로
//     오지만(lib.rs MULTILIVE_STATE) 보조 창의 숨은 멀티라이브 페이지는 player.html iframe 을 만들지 않는다
//     (예전엔 채팅 창 하나에 패인 수만큼 플레이어 문서가 더 떴다).
// L02 닫힌 패인은 30초 동안 그대로(같은 슬롯을 다시 쓰면 바로 재생) 두고, 그 뒤엔 2개만 남기고 문서에서 지운다.
//     지운 슬롯을 다시 쓰면 그 사이 온 명령이 새 패인에 흘러간다. 패인을 지우면 그 패인이 창·문서에 건 리스너도 풀린다.
// L03 30초 안에 같은 슬롯을 다시 쓰면 지우지 않는다(같은 iframe 을 그대로 쓴다).
// 저장소 루트에서: node tools/ui-regression/memory-lifecycle.mjs   (CVIEW_UI_BROWSER=webkit 이면 WebKit)
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

/** 멀티라이브 상태(window.__setSessions 로 패인 수를 바꾼다)와 명령 기록, 문서·창 리스너 수(window.__docListeners). */
function mlMock() {
  const listeners = {};
  window.__TAURI__.event.listen = async (name, cb) => { (listeners[name] ||= []).push(cb); return () => {}; };
  window.__emit = (name, payload) => { for (const cb of listeners[name] || []) cb({ payload }); };
  const live = new Map();
  for (const target of [Document.prototype, Window.prototype]) {
    const add = target.addEventListener;
    const remove = target.removeEventListener;
    target.addEventListener = function (type, fn, opts) {
      const key = `${type}:${Boolean(typeof opts === 'object' ? opts && opts.capture : opts)}`;
      if (!live.has(this)) live.set(this, new Set());
      const set = live.get(this);
      const entry = `${key}#${fn && (fn.__id ||= Math.random())}`;
      if (opts && typeof opts === 'object' && opts.signal) {
        if (opts.signal.aborted) return add.call(this, type, fn, opts);
        opts.signal.addEventListener('abort', () => set.delete(entry), { once: true });
      }
      set.add(entry);
      return add.call(this, type, fn, opts);
    };
    target.removeEventListener = function (type, fn, opts) {
      const key = `${type}:${Boolean(typeof opts === 'object' ? opts && opts.capture : opts)}`;
      live.get(this)?.delete(`${key}#${fn && fn.__id}`);
      return remove.call(this, type, fn, opts);
    };
  }
  window.__docListeners = () => (live.get(document)?.size || 0) + (live.get(window)?.size || 0);
  const open = new Date(Date.now() - 5400e3).toISOString();
  const mk = (i) => ({ id: `s${i}`, slot: i, channelId: `ch${i + 1}`, channelName: `채널${i + 1}`, liveTitle: '방송', profileImageUrl: null,
    viewerCountText: '1만', accumulateCount: 1, accumulateCountText: '1', openDate: open, isMuted: i > 0, isAudible: i === 0, isAudioOnly: false, state: { kind: 'playing' }, chatChannelId: null, maxHeightCap: 0, decodeSuspended: false });
  window.__mlState = { sessions: [0, 1, 2, 3].map(mk), selectedId: 's0', focusMainId: 's0', canAdd: true, maxSessions: 9, layout: 'grid', focusDirection: 'left', focusRatio: 0.7, horizontalRatio: 0.5, verticalRatio: 0.5, multiAudio: false, watchId: null, rev: 1 };
  window.__setSessions = (slots) => {
    window.__mlState = { ...window.__mlState, sessions: slots.map(mk), selectedId: slots.length ? `s${slots[0]}` : null, focusMainId: slots.length ? `s${slots[0]}` : null, rev: window.__mlState.rev + 1 };
    window.__emit('multilive://state', JSON.parse(JSON.stringify(window.__mlState)));
  };
  const info = { liveTitle: '방송', categoryName: '게임', viewerCount: 1, viewerCountText: '1', accumulateCount: 1, openDate: open, tags: [], isLive: true };
  const prev = window.__TAURI__.core.invoke;
  window.__TAURI__.core.invoke = async (c, a = {}) => {
    if (c === 'multilive_state') return JSON.parse(JSON.stringify(window.__mlState));
    if (c === 'channel_detail') return { channel: { channelId: a.channelId, channelName: '채널' }, live: info, isFavorite: false };
    if (c === 'live_detail') return { ...info, channelId: a.channelId, channelName: '채널', chatChannelId: null };
    if (c === 'following_channels_all' || c === 'favorites_list' || c === 'chat_donation_ranking') return [];
    try { return await prev(c, a); } catch (e) { if (/^(multilive_|chat_|player_|window_|bookmark_|save_|channel_)/.test(c)) return null; throw e; }
  };
}

async function open(query = '') {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 }, reducedMotion: 'reduce' });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => { const m = String(e); if (!/ResizeObserver loop/.test(m)) errors.push(m); });
  await page.clock.install();
  await page.addInitScript(mockTauri, [base, 'dark']);
  await page.addInitScript(mlMock);
  await page.goto(`${base}${query}`);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.clock.runFor(500);
  return { ctx, page, errors };
}
const frames = (page) => page.evaluate(() => document.querySelectorAll('[data-page="multilive"] iframe').length);
const paneSlots = (page) => page.evaluate(() => [...document.querySelectorAll('[data-page="multilive"] .ml-pane')].map((p) => Number(p.dataset.slot)).sort((a, b) => a - b));

for (const w of ['chat', 'player']) {
  await check(`L01 aux window ?w=${w} never builds multi-live player iframes`, async () => {
    const { ctx, page, errors } = await open(`?w=${w}&channelId=ch9`);
    await page.evaluate(() => window.__setSessions([0, 1, 2, 3]));
    await page.clock.runFor(2000);
    await page.evaluate(() => window.__setSessions([0, 1, 2, 3, 4, 5]));
    await page.clock.runFor(2000);
    assert.equal(await frames(page), 0, '보조 창에 멀티라이브 플레이어 iframe 이 생겼다');
    assert.deepEqual(errors, []);
    await ctx.close();
  });
}

await check('L02 closed panes: kept 30s, then all but 2 are removed; reused slots come back with queued commands; listeners released', async () => {
  const { ctx, page, errors } = await open();
  await go(page, 'multilive');
  await page.clock.runFor(300);
  await page.waitForSelector('[data-page="multilive"]:not([hidden])');
  await page.evaluate(() => window.__setSessions([0, 1, 2, 3, 4, 5]));
  await page.clock.runFor(1000);
  assert.equal(await frames(page), 6);
  const with6 = await page.evaluate(() => window.__docListeners());
  await page.evaluate(() => window.__setSessions([]));
  await page.clock.runFor(25000);
  assert.equal(await frames(page), 6, '30초 안에는 그대로(다시 쓰면 바로 재생)');
  await page.clock.runFor(6000);
  assert.deepEqual(await paneSlots(page), [0, 1], '번호가 작은 2개만 남긴다');
  const after = await page.evaluate(() => window.__docListeners());
  assert.ok(after <= with6 - 4 * 3, `지운 패인의 리스너가 남았다 ${with6} → ${after}`);
  // 지운 슬롯(4)을 Rust 가 다시 쓰면 — 상태보다 먼저 온 명령이 새 패인에 흘러간다.
  await page.evaluate(() => window.__emit('multilive://command', { slot: 4, script: 'window.__queued = 1;' }));
  await page.evaluate(() => window.__setSessions([4]));
  await page.clock.runFor(1500);
  assert.deepEqual(await paneSlots(page), [0, 1, 4]);
  await page.waitForFunction(() => {
    const f = document.querySelector('[data-page="multilive"] .ml-pane[data-slot="4"] iframe');
    try { return f && f.contentWindow && f.contentWindow.__queued === 1; } catch { return false; }
  }, null, { timeout: 8000 }).catch(() => { throw new Error('다시 만든 패인에 대기 명령이 가지 않았다'); });
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('L03 a slot reused within 30s keeps its iframe', async () => {
  const { ctx, page, errors } = await open();
  await go(page, 'multilive');
  await page.clock.runFor(300);
  await page.evaluate(() => window.__setSessions([0, 1, 2, 3, 4]));
  await page.clock.runFor(1000);
  await page.evaluate(() => { window.__f4 = document.querySelector('.ml-pane[data-slot="4"] iframe'); });
  await page.evaluate(() => window.__setSessions([0]));
  await page.clock.runFor(10000);
  await page.evaluate(() => window.__setSessions([0, 4]));
  await page.clock.runFor(40000);
  const same = await page.evaluate(() => document.querySelector('.ml-pane[data-slot="4"] iframe') === window.__f4);
  assert.ok(same, '다시 쓴 슬롯의 iframe 이 바뀌었다');
  assert.deepEqual(await paneSlots(page), [0, 1, 2, 4], '닫힌 1·2·3 중 2개(1·2)만 남는다');
  assert.deepEqual(errors, []);
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
