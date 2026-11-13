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
// 머리줄 채널 추가 단추의 모양·이름 — 라벨·아이콘·가득 참 표시가 실제로 보이는지
const addBtn = () => page.evaluate(() => {
  const b = document.querySelector('.ml-toolbar .ml-add');
  const shown = (el) => !!el && el.getClientRects().length > 0 && getComputedStyle(el).display !== 'none';
  const cs = getComputedStyle(b);
  return {
    label: b.getAttribute('aria-label'), disabled: b.getAttribute('aria-disabled') === 'true', width: b.offsetWidth,
    long: shown(b.querySelector('.ml-add-long')), short: shown(b.querySelector('.ml-add-label')),
    badge: shown(b.querySelector('.ml-add-full')) ? b.querySelector('.ml-add-full').textContent : '',
    borderStyle: cs.borderTopStyle, bg: cs.backgroundColor, cursor: cs.cursor,
  };
});

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

await check('가득 차면 채널 추가 단추가 점선·흐린 모양으로 바뀌고 n/최대 와 이름으로 이유를 알린다', async () => {
  await page.mouse.move(640, 600);
  await settle();
  const b = await addBtn();
  assert.equal(b.disabled, true);
  assert.equal(b.badge, '5/5');
  assert.equal(b.borderStyle, 'dashed');
  assert.equal(b.cursor, 'not-allowed');
  assert.equal(b.label, '채널 추가 — 가득 참 5/5');
  if (shot) await page.screenshot({ path: path.join(shot, 'add-full.png'), clip: { x: 0, y: 0, width: 1280, height: 70 } });
});

await check('칩은 4개만 보이고 휠로 한 칸씩 넘긴다, 지금 보는 칩은 창 안', async () => {
  const view = () => page.evaluate(() => {
    const p = document.querySelector('.ml-watch-picker');
    const shown = [...p.querySelectorAll('.ml-watch-chip')].filter((c) => !c.closest('.ml-watch-tab').hidden).map((c) => c.textContent.trim().replace('이름-', '') + (c.classList.contains('active') ? '*' : ''));
    const more = (k) => { const m = p.querySelector(`.ml-watch-more.${k}`); return m.hidden ? '' : m.textContent; };
    return { shown, start: more('start'), end: more('end') };
  });
  assert.deepEqual(await view(), { shown: ['p3', 'p4', 'p5', 'p6*'], start: '+1', end: '' });
  const box = await page.locator('.ml-watch-picker').boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.wheel(0, -100);
  await page.waitForTimeout(100);
  assert.deepEqual(await view(), { shown: ['p2', 'p3', 'p4', 'p5'], start: '', end: '+1' });
  // 트랙패드처럼 작은 delta 는 모아서 한 칸
  for (let i = 0; i < 3; i++) await page.mouse.wheel(0, 15);
  await page.waitForTimeout(100);
  assert.deepEqual((await view()).shown, ['p3', 'p4', 'p5', 'p6*']);
  if (shot) await page.screenshot({ path: path.join(shot, 'window.png'), clip: { x: 0, y: 0, width: 1280, height: 70 } });
  await page.click('.ml-watch-more.start');
  assert.deepEqual(await view(), { shown: ['p2', 'p3', 'p4', 'p5'], start: '', end: '+1' });
  await page.mouse.wheel(0, 100);
  await page.waitForTimeout(100);
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

await check('칩의 × 는 아바타 자리에 겹쳐 있다가 마우스를 올리면 뜨고, 누르면 그 채널만 닫는다(고르기 아님)', async () => {
  const geo = (id) => page.evaluate((sid) => {
    const tab = document.querySelector(`.ml-watch-tab[data-session-id="${sid}"]`);
    const x = tab.querySelector('.ml-watch-close');
    const av = tab.querySelector('.ml-watch-chip img, .ml-watch-chip .ml-watch-dot');
    const xr = x.getBoundingClientRect(); const ar = av.getBoundingClientRect();
    return {
      xOpacity: Number(getComputedStyle(x).opacity), avOpacity: Number(getComputedStyle(av).opacity),
      dx: Math.abs((xr.left + xr.right) / 2 - (ar.left + ar.right) / 2), dy: Math.abs((xr.top + xr.bottom) / 2 - (ar.top + ar.bottom) / 2),
      label: x.getAttribute('aria-label'), inChip: !!x.closest('.ml-watch-chip'),
    };
  }, id);
  await page.mouse.move(640, 600);
  await page.waitForFunction(() => Number(getComputedStyle(document.querySelector('.ml-watch-close[data-session-id="s-p3"]')).opacity) === 0, null, { timeout: 2000 });
  let g = await geo('s-p3');
  assert.equal(g.xOpacity, 0, '평소엔 × 가 숨어 있다');
  assert.ok(g.dx <= 1 && g.dy <= 1, `× 가 아바타 위에 겹친다: ${JSON.stringify(g)}`);
  assert.equal(g.label, '이름-p3 채널 닫기');
  assert.ok(!g.inChip, '× 는 칩 단추 밖(단추 안 단추가 아님)');
  await page.locator('.ml-watch-chip[data-session-id="s-p3"]').hover();
  // × 와 아바타가 각자 전환(.15s·.18s)을 마칠 때까지 기다린다.
  await page.waitForFunction(() => {
    const tab = document.querySelector('.ml-watch-tab[data-session-id="s-p3"]');
    const op = (el) => Number(getComputedStyle(el).opacity);
    return op(tab.querySelector('.ml-watch-close')) === 1 && op(tab.querySelector('.ml-watch-chip img, .ml-watch-chip .ml-watch-dot')) === 0;
  }, null, { timeout: 2000 });
  g = await geo('s-p3');
  assert.equal(g.xOpacity, 1, '올리면 × 가 뜬다');
  assert.equal(g.avOpacity, 0, '아바타는 × 에 자리를 내준다');
  if (shot) await page.screenshot({ path: path.join(shot, 'close-hover.png'), clip: { x: 0, y: 0, width: 1280, height: 70 } });
  const before = await page.evaluate(() => window.__mock.calls.length);
  await page.click('.ml-watch-close[data-session-id="s-p3"]');
  await settle();
  const after = await page.evaluate((n) => window.__mock.calls.slice(n), before);
  assert.deepEqual(after, [['remove', 's-p3']], '고르기(watch) 없이 그 채널만 닫는다');
  assert.deepEqual(await chips(), ['p2', 'p4', 'p5*']);
});

await check('키보드: 칩 다음 Tab 이 × 로 가고, 초점이 오면 × 가 보이며 Enter 로 닫는다', async () => {
  await page.mouse.move(640, 600);
  await page.focus('.ml-watch-chip[data-session-id="s-p2"]');
  await page.keyboard.press('Tab');
  // × 는 0 → 1 로 150ms 전환한다 — 중간값을 읽지 않게 다 뜰 때까지 기다린다.
  await page.waitForFunction(() => {
    const a = document.activeElement;
    return a?.classList.contains('ml-watch-close') && Number(getComputedStyle(a).opacity) === 1;
  }, null, { timeout: 2000 });
  assert.equal(await page.evaluate(() => document.activeElement.dataset.sessionId), 's-p2');
  await page.keyboard.press('Enter');
  await settle();
  assert.deepEqual(await chips(), ['p4', 'p5*']);
  const focused = await page.evaluate(() => {
    const a = document.activeElement;
    return { cls: a.className, id: a.dataset.sessionId };
  });
  assert.deepEqual(focused, { cls: 'ml-watch-chip', id: 's-p4' }, '닫은 뒤 초점이 옆 채널 칩으로 간다');
});

await check('라이트·다크에서 보는 칩·다른 칩의 × 가 바탕과 구분된다', async () => {
  for (const theme of ['light', 'dark']) {
    await page.evaluate((t) => { document.documentElement.dataset.theme = t; }, theme);
    for (const id of ['s-p4', 's-p5']) {
      await page.locator(`.ml-watch-chip[data-session-id="${id}"]`).hover();
      await page.waitForTimeout(250);
      const c = await page.evaluate((sid) => {
        const x = document.querySelector(`.ml-watch-close[data-session-id="${sid}"]`);
        const cs = getComputedStyle(x);
        return { bg: cs.backgroundColor, fg: cs.color, ring: cs.boxShadow };
      }, id);
      assert.notEqual(c.bg, c.fg, `${theme} ${id}: × 색이 바탕과 같다`);
      if (id === 's-p5') assert.notEqual(c.ring, 'none', '보는 칩의 × 는 초록 링을 유지');
      if (shot) await page.screenshot({ path: path.join(shot, `close-${theme}-${id}.png`), clip: { x: 0, y: 0, width: 1280, height: 70 } });
    }
  }
  await page.evaluate(() => { delete document.documentElement.dataset.theme; });
});

await check('보는 채널의 × 는 패인 닫기와 같은 길 — 남은 채널로 바꿔 보고 라이브 화면에 머문다', async () => {
  await page.focus('.ml-watch-tab.active .ml-watch-close');
  await page.keyboard.press('Enter');
  await settle();
  const v = await page.evaluate(() => ({
    watch: window.__mock.state.watchId, ids: window.__mock.state.sessions.map((s) => s.id),
    asWatch: document.querySelector('[data-page="multilive"]').classList.contains('as-watch'),
  }));
  assert.deepEqual(v, { watch: 's-p4', ids: ['s-p4'], asWatch: true });
  const f = await page.evaluate(() => ({ ml: document.activeElement.dataset.ml, shown: document.activeElement.offsetParent !== null }));
  assert.deepEqual(f, { ml: 'add', shown: true }, '칩 줄이 숨으면 초점이 보이는 채널 추가 단추로 간다');
});

await check('채널 추가 단추: 넓으면 "채널 추가", 좁으면 "추가", 더 좁으면 + 만 — 이름은 늘 "채널 추가"', async () => {
  await page.mouse.move(640, 600);
  let b = await addBtn();
  assert.deepEqual([b.disabled, b.badge, b.label, b.long, b.short], [false, '', '채널 추가', true, true]);
  assert.equal(b.borderStyle, 'solid');
  await page.setViewportSize({ width: 860, height: 800 });
  await settle();
  b = await addBtn();
  assert.deepEqual([b.long, b.short, b.label], [false, true, '채널 추가'], '좁으면 "+ 추가"');
  assert.ok(b.width > 40, `짧은 라벨이 보일 폭: ${b.width}`);
  await page.setViewportSize({ width: 700, height: 800 });
  await settle();
  b = await addBtn();
  assert.deepEqual([b.short, b.label], [false, '채널 추가'], '아이콘만이어도 이름은 남는다');
  assert.ok(b.width >= 30, `누를 자리 30px 이상: ${b.width}`);
  await page.setViewportSize({ width: 1280, height: 800 });
  await settle();
});

// 바깥으로 번지는 픽셀 자체는 channel-add-hover.mjs CH03 이 잰다. 여기서는 계산값으로 — 위치 그대로, filter 없음, 바깥 그림자 흐림 6px 이하.
await check('채널 추가 단추: 올리면 제자리(위치·filter 그대로)에서 안쪽에 빛이 맺히고 바깥 그림자 흐림 6px 이하, 키보드 초점은 같은 켜짐 + 테두리', async () => {
  const add = page.locator('.ml-toolbar .ml-add');
  const look = () => add.evaluate((b) => {
    const cs = getComputedStyle(b);
    // 바깥 그림자 중 가장 큰 흐림(px) — inset 은 단추 안이라 뺀다
    const blur = Math.max(0, ...cs.boxShadow.split(/,(?![^(]*\))/).filter((s) => !/inset/.test(s))
      .map((s) => parseFloat((s.replace(/rgba?\([^)]*\)|color\([^)]*\)/g, '').match(/-?[\d.]+px/g) || [])[2]) || 0));
    const r = b.getBoundingClientRect();
    return { translate: cs.translate, shadow: cs.boxShadow, outline: cs.outlineStyle, filter: cs.filter, blur, lit: getComputedStyle(b, '::before').opacity, x: r.x, y: r.y };
  });
  await page.evaluate(() => document.activeElement?.blur());   // 앞 검사가 남긴 초점이면 쉼 모양이 아니다(초점 = 호버와 같은 켜짐)
  await page.mouse.move(640, 600);
  await page.waitForTimeout(250);
  const rest = await look();
  await add.hover();
  await page.waitForTimeout(250);
  const hover = await look();
  assert.notEqual(hover.shadow, rest.shadow, '호버하면 턱·그림자가 바뀐다');
  assert.equal(hover.translate, rest.translate, '단추는 제자리(떠오르지 않는다)');
  assert.ok(Math.abs(hover.x - rest.x) < 0.25 && Math.abs(hover.y - rest.y) < 0.25, `위치 그대로: (${rest.x},${rest.y}) → (${hover.x},${hover.y})`);
  assert.equal(hover.filter, 'none', '호버 filter 없음(drop-shadow·brightness 도 아님)');
  assert.ok(hover.blur <= 6, `바깥 그림자 흐림 ${hover.blur}px`);
  if (hover.lit !== '') assert.notEqual(hover.lit, rest.lit, '호버하면 안쪽 빛이 켜진다');   // WebKit 은 가상 요소 계산값이 빌 수 있다
  await page.mouse.move(640, 600);
  await page.keyboard.press('Shift+Tab');
  await page.keyboard.press('Tab');
  await add.focus();
  await page.waitForTimeout(250);
  const focus = await look();
  assert.equal(focus.outline, 'solid', '키보드 초점 테두리');
  assert.equal(focus.lit, hover.lit, '키보드 초점도 호버와 같은 켜짐');
  if (shot) await page.screenshot({ path: path.join(shot, 'add-focus.png'), clip: { x: 0, y: 0, width: 1280, height: 70 } });
  await page.evaluate(() => document.activeElement.blur());
});

await browser.close();
server.close();
for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : ` — ${r.error}`}`);
if (errors.length) console.log(`page errors:\n${errors.join('\n')}`);
const failed = results.filter((r) => !r.ok).length + errors.length;
console.log(failed ? `${failed} failed` : `all ${results.length} passed`);
process.exit(failed ? 1 : 0);
