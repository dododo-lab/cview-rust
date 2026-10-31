// 라이브 단독 보기 머리줄의 채널 고르기 — 팔로잉·인기 등에서 연 채널과 채널 추가 사이드바로 담은 채널이 머리줄에 쌓이는지 본다.
// 예전엔 재생으로 연 채널(임시 보기)을 다음 재생이 바꿔 끼워 머리줄에 늘 하나만 남았다.
// app-ui 를 그대로 띄우고 window.__TAURI__ 를 모의 IPC 로 바꿔 실제 화면 코드를 돌린다(실제 서비스·데이터는 건드리지 않음).
// 저장소 루트에서: node tools/ui-regression/live-picker.mjs   (Playwright 가 필요하다)
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');

const ROOT = path.resolve('app-ui');
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.png': 'image/png', '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.json': 'application/json' };
const server = http.createServer((req, res) => {
  const url = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  const file = path.join(ROOT, url === '/' ? 'index.html' : url);
  if (!file.startsWith(ROOT) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) { res.writeHead(404); res.end(); return; }
  res.writeHead(200, { 'content-type': TYPES[path.extname(file)] || 'application/octet-stream' });
  fs.createReadStream(file).pipe(res);
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${server.address().port}/`;

// 비어 있는 멀티라이브 · 멀티라이브 상한 4 · 라이브 단독 보기 상한 5
function mockTauri() {
  let nextSlot = 1;
  const session = (id) => ({
    id: `s-${id}`, slot: nextSlot++, channelId: id, channelName: `이름-${id}`, liveTitle: `방송 ${id}`, profileImageUrl: null,
    viewerCountText: '1,000', accumulateCount: 0, accumulateCountText: '', openDate: new Date(Date.now() - 600e3).toISOString(),
    isMuted: true, isAudible: false, isAudioOnly: false, state: { kind: 'playing' }, chatChannelId: null, maxHeightCap: 0, decodeSuspended: false,
  });
  const state = {
    sessions: [], selectedId: null, focusMainId: null, layout: 'grid',
    canAdd: true, maxSessions: 4, canAddLive: true, maxLiveSessions: 5, maxMultiSessions: 4,
    focusDirection: 'left', focusRatio: 0.7, horizontalRatio: 0.5, verticalRatio: 0.5, multiAudio: false, watchId: null, rev: 1,
  };
  const sync = () => {
    const n = state.sessions.length;
    if (state.watchId && !state.sessions.some((s) => s.id === state.watchId)) state.watchId = null;
    state.canAddLive = n < state.maxLiveSessions;
    state.maxSessions = state.watchId ? state.maxLiveSessions : state.maxMultiSessions;
    state.canAdd = n < state.maxSessions;
    state.rev++;
    return JSON.parse(JSON.stringify(state));
  };
  const calls = [];
  const lives = ['p1', 'p2', 'p3', 'p4', 'p5', 'p6', 'p7'].map((id) => ({
    channelId: id, channelName: `이름-${id}`, liveTitle: `방송 ${id}`, isLive: true, viewerCount: 100, openDate: new Date().toISOString(),
  }));
  const handlers = {
    auth_status: () => ({ loggedIn: false, state: 'loggedOut' }),
    settings_get: () => ({ multiLive: { maxConcurrentSessions: 4, maxLiveChannels: 5 }, multiChat: {}, player: {}, chat: {}, general: {} }),
    multilive_state: () => sync(),
    multilive_add: ({ channelId, live }) => {
      calls.push(['add', channelId, live]);
      const cap = live ? state.maxLiveSessions : state.maxMultiSessions;
      if (state.sessions.length >= cap) throw new Error('더 이상 추가할 수 없습니다 (최대 세션 수)');
      state.sessions.push(session(channelId));
      return sync();
    },
    multilive_watch: ({ sessionId }) => { calls.push(['watch', sessionId]); state.watchId = sessionId; return sync(); },
    multilive_unwatch: () => { state.watchId = null; return sync(); },
    multilive_remove: ({ sessionId }) => { calls.push(['remove', sessionId]); state.sessions = state.sessions.filter((s) => s.id !== sessionId); return sync(); },
    multilive_select: ({ sessionId }) => { state.selectedId = sessionId; return sync(); },
    popular_lives: () => lives,
    following_lives: () => [],
    preview_source: () => null, // 영상 미리보기는 hover-preview.mjs 가 본다 — 여기선 말풍선만
  };
  window.__mock = { state, handlers, calls };
  window.__TAURI__ = {
    core: { invoke: async (cmd, args) => { const h = handlers[cmd]; if (!h) throw new Error(`mock: ${cmd}`); return h(args || {}); } },
    event: { listen: async () => () => {} },
  };
  try { localStorage.setItem('cview.onboarded', '1'); } catch { /* 무시 */ }
}

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
page.setDefaultTimeout(5000);
const errors = [];
page.on('pageerror', (e) => errors.push(String(e.stack || e)));
await page.addInitScript(mockTauri);
await page.goto(base);
await page.waitForSelector('[data-page]', { state: 'attached' });
await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
const navigate = (route) => page.evaluate((r) => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: r, params: {} } })), route);
const play = (id) => page.evaluate((c) => window.dispatchEvent(new CustomEvent('cview:play', { detail: { channelId: c } })), id);
const settle = () => page.waitForTimeout(400);
const chips = () => page.evaluate(() => {
  const p = document.querySelector('.ml-watch-picker');
  return p.hidden ? [] : [...p.querySelectorAll('.ml-watch-chip')].map((b) => b.textContent.trim().replace('이름-', '') + (b.classList.contains('active') ? '*' : ''));
});
// 칩 줄 가운데가 머리줄 가운데와 얼마나 어긋났는지(px) — 머리줄은 좌우 패딩을 뺀 안쪽 폭 기준
const centerOffset = () => page.evaluate(() => {
  const h = document.querySelector('.ml-header'); const cs = getComputedStyle(h);
  const hr = h.getBoundingClientRect(); const pr = document.querySelector('.ml-watch-picker').getBoundingClientRect();
  const mid = (hr.left + parseFloat(cs.paddingLeft) + hr.right - parseFloat(cs.paddingRight)) / 2;
  return Math.abs((pr.left + pr.right) / 2 - mid);
});
const shot = process.env.SHOT_DIR;

const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}

await check('다른 메뉴에서 연 채널이 머리줄에 쌓인다', async () => {
  await navigate('popular');
  await settle();
  await play('p1');
  await settle();
  await navigate('following');
  await settle();
  await play('p2');
  await settle();
  assert.deepEqual(await chips(), ['p1', 'p2*']);
  assert.ok(!(await page.evaluate(() => window.__mock.calls.some((c) => c[0] === 'remove'))), '앞서 연 채널을 빼지 않는다');
});

await check('채널 추가 사이드바 줄을 눌러 연 채널도 머리줄에 쌓인다', async () => {
  await page.evaluate(() => [...document.querySelectorAll('[data-ml="add"]')].find((b) => b.offsetParent).click());
  await page.waitForSelector('.ml-addrow-open[data-channel-id="p3"]');
  await page.click('.ml-addrow-open[data-channel-id="p3"]');
  await settle();
  assert.deepEqual(await chips(), ['p1', 'p2', 'p3*']);
});

await check('사이드바 + 로 담은 채널은 머리줄에 생기고, 채널 줄은 머리줄 가운데에 있다', async () => {
  if (!(await page.isVisible('.ml-addrow-act[data-channel-id="p4"]'))) {
    await page.evaluate(() => [...document.querySelectorAll('[data-ml="add"]')].find((b) => b.offsetParent).click());
  }
  await page.click('.ml-addrow-act[data-channel-id="p4"]');
  await settle();
  assert.deepEqual(await chips(), ['p1', 'p2', 'p3*', 'p4']);
  assert.equal(await page.locator('.ml-watch-rail, .ml-header .rail-item').count(), 0, '머리줄에 다른 방송 줄이 없다');
  await page.keyboard.press('Escape'); // 사이드바 닫기 — 열린 채로는 머리줄 폭이 좁다
  await settle();
  const off = await centerOffset();
  assert.ok(off <= 1, `채널 줄이 가운데에서 ${off}px 어긋남`);
  if (shot) await page.screenshot({ path: path.join(shot, 'picker-4.png'), clip: { x: 0, y: 0, width: 1280, height: 120 } });
});

await check('칩에 마우스를 올리면 미리보기 말풍선이 그 칩 아래에 열리고, 벗어나면 닫힌다', async () => {
  const peek = () => page.evaluate(() => {
    const e = document.querySelector('.ml-peek');
    const r = e.getBoundingClientRect();
    return { hidden: e.hidden, mid: r.left + r.width / 2, top: r.top, title: e.querySelector('.ml-peek-title')?.textContent, described: document.querySelectorAll('.ml-watch-chip[aria-describedby]').length };
  });
  const chip = page.locator('.ml-watch-chip:not(.active)').first();
  await chip.hover();
  await page.waitForTimeout(600);
  const box = await chip.boundingBox();
  let p = await peek();
  assert.ok(!p.hidden, '말풍선이 열린다');
  assert.equal(p.title, '방송 p1');
  assert.ok(p.top >= box.y + box.height, '칩 아래');
  // 옆 칩으로 옮기면 기다리지 않고 바뀐다, 칩 표시는 하나만
  await page.locator('.ml-watch-chip:not(.active)').nth(1).hover();
  await page.waitForTimeout(150);
  p = await peek();
  assert.equal(p.title, '방송 p2');
  assert.equal(p.described, 1);
  // 지금 보는 칩에선 열지 않는다
  await page.locator('.ml-watch-chip.active').hover();
  await page.waitForTimeout(500);
  assert.ok((await peek()).hidden, '보는 칩은 말풍선 없음');
  await chip.hover();
  await page.waitForTimeout(600);
  await page.mouse.move(640, 600);
  await page.waitForTimeout(300);
  p = await peek();
  assert.ok(p.hidden && p.described === 0, '벗어나면 닫힌다');
});

await check('라이브 상한이 차면 가장 오래전에 둘러본 채널을 바꿔 끼운다', async () => {
  await play('p5'); // 5 / 5
  await settle();
  await play('p6'); // 가득 — p1(가장 오래된 둘러본 채널)을 뺀다
  await settle();
  assert.deepEqual(await chips(), ['p2', 'p3', 'p4', 'p5', 'p6*']);
});

await check('머리줄이 넘쳐도 지금 보는 칩이 보인다', async () => {
  await page.setViewportSize({ width: 900, height: 800 });
  await settle();
  await play('p2');
  await settle();
  await play('p6');
  await settle();
  const v = await page.evaluate(() => {
    const p = document.querySelector('.ml-watch-picker');
    const a = p.querySelector('.ml-watch-chip.active');
    const pr = p.getBoundingClientRect(); const ar = a.getBoundingClientRect();
    return { inside: ar.left >= pr.left - 1 && ar.right <= pr.right + 1, over: p.scrollWidth > p.clientWidth };
  });
  assert.ok(v.inside, `보는 칩이 잘림: ${JSON.stringify(v)}`);
  if (shot) await page.screenshot({ path: path.join(shot, 'picker-narrow.png'), clip: { x: 0, y: 0, width: 900, height: 120 } });
});

await check('닫기 단추는 머리줄에 남은 옆 채널로 바꿔 보고 라이브 화면에 머문다', async () => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.evaluate(() => {
    const st = window.__mock.state;
    const pane = [...document.querySelectorAll('.ml-pane')].find((p) => p.classList.contains('ml-single-target'));
    pane.querySelector('[data-ml="remove"]').click();
    return st.watchId;
  });
  await settle();
  assert.deepEqual(await chips(), ['p2', 'p3', 'p4', 'p5*']);
  assert.ok(await page.evaluate(() => document.querySelector('[data-page="multilive"]').classList.contains('as-watch')), '라이브 단독 보기에 머문다');
});

await browser.close();
server.close();
for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : ` — ${r.error}`}`);
if (errors.length) console.log(`page errors:\n${errors.join('\n')}`);
const failed = results.filter((r) => !r.ok).length + errors.length;
console.log(failed ? `${failed} failed` : `all ${results.length} passed`);
process.exit(failed ? 1 : 0);
