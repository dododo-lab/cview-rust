// 멀티라이브 화면을 떠나면 보이지 않는 패인 디코드를 멈춘다 — 회귀 검사(모의 IPC·헤드리스 브라우저).
// 창이 가려질 때만 보내던 multilive_set_window_occluded(true) 를 멀티라이브(단독 보기 무대 포함) 화면이 숨어도 보낸다.
// Rust 쪽은 선택·PiP·들리는 패인만 남기고 멈춘다(창 가림과 같은 규칙) — 소리는 이어진다.
// 사용률 측정(tools/usage-bench, 4패인 중 1패인 소리, 디버그 빌드, 코어 하나 = 100): 멀티라이브를 뒤에 둔 채 홈
// 32.5 → 19.8(나머지 패인 멈춤) → 14.8(소리 패인 화질 상한, O07). 멀티라이브 화면을 볼 때는 그대로(30~34).
// O01 멀티라이브 → 홈: 5초 안엔 보내지 않고, 5초가 지나면 occluded=true 를 한 번.
// O02 홈 → 멀티라이브: 바로 occluded=false.
// O03 5초 안에 돌아오면 true 를 보내지 않는다(잠깐 다른 메뉴를 보는 정도로는 멈추지 않는다).
// O04 라이브 단독 보기(멀티라이브 무대)는 보이는 화면 — 보내지 않는다. 단독 보기에서 다른 메뉴로 가면 5초 뒤 true.
// O05 창이 가려지면(document.hidden) 예전처럼 5초 뒤 true, 다시 보이면 바로 false(멀티라이브 화면일 때).
// O06 창이 가려진 채 다른 메뉴로 옮겼다가 창이 다시 보여도 멀티라이브 화면이 아니면 false 를 보내지 않는다.
// O07 화면 밖이 되면 패인 플레이어에 화질 상한(360)을 걸고 돌아오면 푼다 — 계속 재생하는 패인(선택·들리는)이 소리만 내는 동안
//     보이지 않는 1080p 를 디코드하지 않게(측정: GPU 처리 6.8 → 4.5, 합 19.8 → 14.8). 시청 집계 표시는 건드리지 않는다.
// O08 화면 밖에 있는 동안 새로 생긴 패인도 페이지가 뜨면 상한을 건다.
// O09 패인이 없으면 화면을 옮겨도 보내지 않는다(빈 멀티라이브로 메뉴를 옮길 때마다 명령이 나가지 않게 — perf P02).
//     다른 메뉴에 있는 동안 패인이 생기면 그때부터 5초 뒤에 보낸다(멀티라이브 화면이 새 상태를 반영한 뒤 다시 맞춘다 —
//     예전엔 상태 알림 처리 순서에 따라 부하 때 다음 메뉴 이동까지 늦었다). 다른 메뉴 사이를 옮겨도 5초는 처음 떠난 때부터 잰다.
// O10 '백그라운드 재생 계속'을 끄면 창이 가려질 때 단독 보기 무대 패인도 멈췄다가 돌아오면 이어 본다(예전엔 옛 시청 화면만 멈췄다).
// 저장소 루트에서: node tools/ui-regression/ml-offscreen.mjs   (CVIEW_UI_BROWSER=webkit 이면 WebKit)
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

/** 멀티라이브 4패인 상태와 가림 명령 기록(window.__occ). 창 숨김은 window.__vis 로 흉내 낸다. */
function mlMock() {
  window.__occ = [];
  window.__vis = 'visible';
  Object.defineProperty(Document.prototype, 'visibilityState', { configurable: true, get: () => window.__vis });
  Object.defineProperty(Document.prototype, 'hidden', { configurable: true, get: () => window.__vis === 'hidden' });
  const listeners = {};
  window.__TAURI__.event.listen = async (name, cb) => { (listeners[name] ||= []).push(cb); return () => {}; };
  window.__emit = (name, payload) => { for (const cb of listeners[name] || []) cb({ payload }); };
  const open = new Date(Date.now() - 5400e3).toISOString();
  const sessions = Array.from({ length: 4 }, (_, i) => ({ id: `s${i}`, slot: i, channelId: `ch${i + 1}`, channelName: `채널${i + 1}`, liveTitle: '방송', profileImageUrl: null,
    viewerCountText: '1만', accumulateCount: 1, accumulateCountText: '1', openDate: open, isMuted: i > 0, isAudible: i === 0, isAudioOnly: false, state: { kind: 'playing' }, chatChannelId: null, maxHeightCap: 0, decodeSuspended: false }));
  const state = window.__mlState = { sessions, selectedId: 's0', focusMainId: 's0', canAdd: true, maxSessions: 4, layout: 'grid', focusDirection: 'left', focusRatio: 0.7, horizontalRatio: 0.5, verticalRatio: 0.5, multiAudio: false, watchId: null, rev: 1 };
  const live = { liveTitle: '방송', categoryName: '게임', viewerCount: 1, viewerCountText: '1', accumulateCount: 1, openDate: open, tags: [], isLive: true };
  const prev = window.__TAURI__.core.invoke;
  window.__TAURI__.core.invoke = async (c, a = {}) => {
    if (c === 'multilive_set_window_occluded') { window.__occ.push(a.occluded); return null; }
    // 상태는 바꿀 때마다 새 객체(rev 증가)로 — 같은 객체를 고치면 진행 중인 읽기와 섞여 부하 때 흔들렸다.
    if (c === 'multilive_state') return JSON.parse(JSON.stringify(window.__mlState));
    if (c === 'multilive_watch') { window.__mlState = { ...window.__mlState, watchId: a.sessionId, rev: window.__mlState.rev + 1 }; return JSON.parse(JSON.stringify(window.__mlState)); }
    if (c === 'channel_detail') return { channel: { channelId: a.channelId, channelName: '채널' }, live, isFavorite: false };
    if (c === 'live_detail') return { ...live, channelId: a.channelId, channelName: '채널', chatChannelId: null };
    if (c === 'following_channels_all' || c === 'favorites_list' || c === 'chat_donation_ranking') return [];
    try { return await prev(c, a); } catch (e) { if (/^(multilive_|chat_|player_|window_|bookmark_|save_|channel_)/.test(c)) return null; throw e; }
  };
}

async function open() {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 }, reducedMotion: 'reduce' });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => { const m = String(e); if (!/ResizeObserver loop/.test(m)) errors.push(m); });
  await page.clock.install();
  await page.addInitScript(mockTauri, [base, 'dark']);
  await page.addInitScript(mlMock);
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.clock.runFor(500);
  await go(page, 'multilive');
  await page.clock.runFor(300);
  await page.waitForSelector('[data-page="multilive"]:not([hidden])');
  // 시작 때(홈이 첫 화면)의 기록은 지운다 — 멀티라이브 화면에서부터 본다.
  await page.clock.runFor(6000);
  await page.evaluate(() => { window.__occ.length = 0; });
  return { ctx, page, errors };
}
const occ = (page) => page.evaluate(() => window.__occ.slice());
const setVisible = (page, v) => page.evaluate((x) => { window.__vis = x; document.dispatchEvent(new Event('visibilitychange')); }, v);

await check('O01 멀티라이브 → 홈: 5초 뒤 가림', async () => {
  const { ctx, page, errors } = await open();
  await go(page, 'home');
  await page.clock.runFor(4500);
  assert.deepEqual(await occ(page), [], '5초 안에는 보내지 않는다');
  await page.clock.runFor(1000);
  assert.deepEqual(await occ(page), [true]);
  await page.clock.runFor(20000);
  assert.deepEqual(await occ(page), [true], '한 번만');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('O02 홈 → 멀티라이브: 바로 되돌림', async () => {
  const { ctx, page } = await open();
  await go(page, 'home');
  await page.clock.runFor(6000);
  await go(page, 'multilive');
  await page.clock.runFor(50);
  assert.deepEqual(await occ(page), [true, false]);
  await ctx.close();
});

await check('O03 5초 안에 돌아오면 멈추지 않음', async () => {
  const { ctx, page } = await open();
  await go(page, 'following');
  await page.clock.runFor(2000);
  await go(page, 'settings');
  await page.clock.runFor(2000);
  await go(page, 'multilive');
  await page.clock.runFor(8000);
  assert.deepEqual(await occ(page), []);
  await ctx.close();
});

await check('O04 단독 보기 무대는 보이는 화면', async () => {
  const { ctx, page } = await open();
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:ml-watch', { detail: { sessionId: 's0' } })));
  await page.waitForSelector('.page-multilive.as-watch');
  await page.clock.runFor(8000);
  assert.deepEqual(await occ(page), [], '단독 보기 중에는 보내지 않는다');
  await go(page, 'following');
  await page.clock.runFor(5500);
  assert.deepEqual(await occ(page), [true]);
  await ctx.close();
});

await check('O05 창 가림은 예전처럼', async () => {
  const { ctx, page } = await open();
  await setVisible(page, 'hidden');
  await page.clock.runFor(4000);
  assert.deepEqual(await occ(page), []);
  await page.clock.runFor(1500);
  assert.deepEqual(await occ(page), [true]);
  await setVisible(page, 'visible');
  await page.clock.runFor(50);
  assert.deepEqual(await occ(page), [true, false]);
  await ctx.close();
});

await check('O06 가려진 채 메뉴를 옮겼다가 보여도 멀티라이브가 아니면 그대로', async () => {
  const { ctx, page } = await open();
  await setVisible(page, 'hidden');
  await page.clock.runFor(5500);
  await go(page, 'home');
  await setVisible(page, 'visible');
  await page.clock.runFor(8000);
  assert.deepEqual(await occ(page), [true], '홈에서 창이 보여도 패인은 멈춘 채');
  await go(page, 'multilive');
  await page.clock.runFor(50);
  assert.deepEqual(await occ(page), [true, false]);
  await ctx.close();
});

/** 패인 플레이어(iframe)의 setBackgroundSaver·setWatchSurfaceHidden 호출을 기록한다(window.__caps[slot]). */
async function spyPanes(page) {
  await page.waitForFunction(() => document.querySelectorAll('.page-multilive .ml-pane iframe').length >= 4);
  await page.waitForFunction(() => [...document.querySelectorAll('.page-multilive .ml-pane iframe')].every((f) => { try { return typeof f.contentWindow.setBackgroundSaver === 'function'; } catch { return false; } }));
  await page.evaluate(() => {
    window.__caps = {}; window.__surface = 0;
    for (const f of document.querySelectorAll('.page-multilive .ml-pane iframe')) {
      const slot = f.closest('.ml-pane').dataset.slot;
      window.__caps[slot] = [];
      f.contentWindow.setBackgroundSaver = (h) => window.__caps[slot].push(h);
      f.contentWindow.setWatchSurfaceHidden = () => { window.__surface += 1; };
    }
  });
}

await check('O07 화면 밖 패인 화질 상한', async () => {
  const { ctx, page, errors } = await open();
  await spyPanes(page);
  await go(page, 'home');
  await page.clock.runFor(4500);
  assert.deepEqual(await page.evaluate(() => window.__caps), { 0: [], 1: [], 2: [], 3: [] }, '5초 안엔 그대로');
  await page.clock.runFor(1000);
  assert.deepEqual(await page.evaluate(() => window.__caps), { 0: [360], 1: [360], 2: [360], 3: [360] });
  await go(page, 'multilive');
  await page.clock.runFor(50);
  assert.deepEqual(await page.evaluate(() => window.__caps), { 0: [360, 0], 1: [360, 0], 2: [360, 0], 3: [360, 0] });
  assert.equal(await page.evaluate(() => window.__surface), 0, '시청 집계 표시는 건드리지 않는다');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('O08 화면 밖에서 새로 생긴 패인', async () => {
  const { ctx, page } = await open();
  await go(page, 'home');
  await page.clock.runFor(5500);
  // 홈에 있는 동안 다섯 번째 패인이 생긴다(다른 메뉴에서 '멀티라이브에 추가').
  await page.evaluate(() => {
    const st = JSON.parse(JSON.stringify(window.__mlState));
    st.maxSessions = 5;
    st.sessions.push({ ...st.sessions[1], id: 's4', slot: 4, channelId: 'ch5', channelName: '채널5' });
    st.rev = window.__mlState.rev + 1;
    window.__mlState = st;
    window.__emit('multilive://state', JSON.parse(JSON.stringify(st)));
  });
  await page.waitForFunction(() => { const f = document.querySelector('.page-multilive .ml-pane[data-slot="4"] iframe'); try { return f && f.contentDocument.readyState === 'complete' && typeof f.contentWindow.setBackgroundSaver === 'function'; } catch { return false; } });
  // load 처리에서 건 상한은 페이지의 실제 값(_backgroundCapHeight)으로 본다.
  await page.clock.runFor(100);
  const cap = await page.evaluate(() => { const f = document.querySelector('.page-multilive .ml-pane[data-slot="4"] iframe'); return f.contentWindow.eval('_backgroundCapHeight'); });
  assert.equal(cap, 360);
  await ctx.close();
});

await check('O09 패인이 없으면 보내지 않음', async () => {
  const { ctx, page } = await open();
  const emit = (fn) => page.evaluate((src) => {
    const st = JSON.parse(JSON.stringify(window.__mlState));
    new Function('st', 'saved', src)(st, window.__saved || (window.__saved = window.__mlState.sessions));
    st.rev = window.__mlState.rev + 1;
    window.__mlState = st;
    window.__emit('multilive://state', JSON.parse(JSON.stringify(st)));
  }, fn);
  await emit('st.sessions = [];');
  await page.clock.runFor(100);
  await go(page, 'home');
  await page.clock.runFor(8000);
  assert.deepEqual(await occ(page), [], '패인이 없으면 보내지 않는다');
  // 홈에 있는 동안 패인이 생긴다 — 그때부터 5초 뒤(메뉴를 옮겨도 다시 재지 않는다).
  await emit('st.sessions = saved;');
  await page.clock.runFor(3000);
  await go(page, 'following');
  await page.clock.runFor(1500);
  assert.deepEqual(await occ(page), []);
  await page.clock.runFor(1000);
  assert.deepEqual(await occ(page), [true], '처음 화면 밖이 된 때부터 5초');
  await ctx.close();
});

await check('O10 백그라운드 재생 끄기 — 창이 가려지면 단독 보기 무대 패인도 멈췄다가 돌아오면 이어 본다(켜 두면 그대로)', async () => {
  const { ctx, page, errors } = await open();
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:ml-watch', { detail: { sessionId: 's0' } })));
  await page.waitForSelector('.page-multilive.as-watch');
  await page.clock.runFor(2000);
  // 무대 패인 플레이어 — 재생 중인 것처럼 두고, 일시정지·재생 호출을 기록한다.
  await page.waitForFunction(() => {
    const f = document.querySelector('.ml-pane[data-slot="0"] iframe');
    const w = f && f.contentWindow;
    return w && typeof w.pause === 'function' && typeof w.loadSource === 'function' && w.document.readyState === 'complete';
  }, null, { timeout: 15000 });
  await page.clock.runFor(500);
  // 기록은 바깥 창 배열에 — 매번 다시 감싸고(문서가 바뀌어도) 재생 중인 것처럼 둔다.
  const spy = () => page.evaluate(() => {
    const w = document.querySelector('.ml-pane[data-slot="0"] iframe').contentWindow;
    window.__bgCalls ||= [];
    for (const name of ['pause', 'play', 'seekToLiveEdge']) {
      if (w[name] && w[name].__spied) continue;
      const f = w[name];
      const g = (...a) => { window.__bgCalls.push(name); try { return f(...a); } catch { return undefined; } };
      g.__spied = true;
      w[name] = g;
    }
    Object.defineProperty(w.document.getElementById('video'), 'paused', { configurable: true, get: () => false });
    const out = window.__bgCalls.slice(); window.__bgCalls.length = 0; return out;
  });
  await spy();
  // 기본(켜 둠) — 가려져도 멈추지 않는다.
  await setVisible(page, 'hidden');
  await page.clock.runFor(200);
  await setVisible(page, 'visible');
  await page.clock.runFor(200);
  assert.deepEqual(await spy(), [], '켜 두면 멈추지 않는다');
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:settings-changed', { detail: { settings: { player: { continuePlaybackInBackground: false }, appearance: {} } } })));
  await setVisible(page, 'hidden');
  await page.clock.runFor(200);
  assert.deepEqual(await spy(), ['pause'], '가려지면 무대 패인을 멈춘다');
  await setVisible(page, 'visible');
  await page.clock.runFor(200);
  assert.deepEqual(await spy(), ['play', 'seekToLiveEdge'], '돌아오면 이어서(라이브 끝으로)');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await browser.close();
server.close();
let failed = 0;
for (const r of results) {
  console.log(`${r.ok ? '✓' : '✗'} ${r.id}`);
  if (!r.ok) { failed += 1; console.log(`    ${r.error.replace(/\n/g, '\n    ')}`); }
}
console.log(`\n${results.length - failed}/${results.length} 통과 (${WEBKIT ? 'WebKit' : 'Chromium'})`);
process.exit(failed ? 1 : 0);
