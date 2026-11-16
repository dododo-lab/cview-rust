// 멀티라이브에서 볼 채널 고르기 — 라이브 단독 보기에 멀티라이브 상한보다 많이 담아 온 채 멀티라이브로 오면 고른 채널만
// 패인으로 보이고, 머리줄 "볼 채널 고르기" 시트에서 어떤 채널을 올릴지(빼고 넣어 바꾸기) 고른다. 상한 이하면 예전 그대로.
// app-ui 를 그대로 띄우고 window.__TAURI__ 를 모의 IPC 로 바꿔 실제 화면 코드를 돌린다(실제 서비스·데이터는 건드리지 않음).
// 저장소 루트에서: node tools/ui-regression/ml-view-picker.mjs   (Playwright 가 필요하다.
//   CVIEW_UI_BROWSER=webkit 이면 앱과 같은 WebKit 엔진으로 돈다. CVIEW_UI_SHOTS=<폴더> 면 화면을 남긴다.)
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const engine = process.env.CVIEW_UI_BROWSER === 'webkit' ? pw.webkit : pw.chromium;
const SHOTS = process.env.CVIEW_UI_SHOTS || '';

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

// 멀티라이브 상한 4 · 라이브 단독 보기 상한 8. 처음엔 4개(상한 이하)로 시작해 예전 동작을 보고, 라이브로 2개를 더 담는다.
// 모의 Rust 는 `multi_view_session_ids` 처럼 선택 → 고른 목록 → 세션 순으로 상한까지 채워 inMultiView 를 단다.
function mockTauri() {
  const session = (id, i) => ({
    id: `s-${id}`, slot: i, channelId: id, channelName: `이름-${id}`, liveTitle: `방송 ${id}`, profileImageUrl: null,
    viewerCountText: '1,000', accumulateCount: 0, accumulateCountText: '', openDate: new Date(Date.now() - 600e3).toISOString(),
    isMuted: i > 0, isAudible: i === 0, isAudioOnly: false, state: { kind: 'playing' }, chatChannelId: null, maxHeightCap: 0, decodeSuspended: false,
  });
  const state = {
    sessions: ['c0', 'c1', 'c2', 'c3'].map(session), selectedId: 's-c0', focusMainId: 's-c0', layout: 'grid',
    canAdd: false, maxSessions: 4, canAddLive: true, maxLiveSessions: 8, maxMultiSessions: 4,
    focusDirection: 'left', focusRatio: 0.7, horizontalRatio: 0.5, verticalRatio: 0.5, multiAudio: false, watchId: null, rev: 1,
  };
  // 모의 Rust — 고른 목록이 있으면 그것만(상한보다 적어도 다시 채우지 않는다), 없으면 자동 보기 → 세션 순으로 상한까지.
  // 어느 쪽이든 선택 세션이 맨 앞.
  // 고른 적이 있으면 고른 수까지만(닫혀 빠진 자리만 채운다), 상한 이하로 줄면 고른 목록을 지운다.
  let picked = [];
  let pickCount = 0;
  let auto = [];
  const sync = () => {
    const n = state.sessions.length;
    const ids = state.sessions.map((s) => s.id);
    let view = null;
    if (!state.watchId && n <= state.maxMultiSessions) { picked = []; pickCount = 0; auto = []; }
    if (!state.watchId && n > state.maxMultiSessions) {
      const target = pickCount ? Math.min(pickCount, state.maxMultiSessions) : state.maxMultiSessions;
      view = [];
      for (const id of [state.selectedId, ...(pickCount ? picked : auto), ...ids]) if (id && ids.includes(id) && !view.includes(id) && view.length < target) view.push(id);
      if (pickCount) picked = view.slice(); else auto = view.slice();
    }
    for (const s of state.sessions) {
      s.inMultiView = !view || view.includes(s.id);
      s.decodeSuspended = !s.inMultiView;
    }
    state.canAddLive = n < state.maxLiveSessions;
    state.maxSessions = state.watchId ? state.maxLiveSessions : state.maxMultiSessions;
    state.canAdd = n < state.maxSessions;
    state.rev++;
    return JSON.parse(JSON.stringify(state));
  };
  const calls = [];
  const handlers = {
    auth_status: () => ({ loggedIn: false, state: 'loggedOut' }),
    settings_get: () => ({ multiLive: { maxConcurrentSessions: 4, maxLiveChannels: 8 }, multiChat: {}, player: {}, chat: {}, general: {} }),
    multilive_state: () => sync(),
    multilive_add: ({ channelId }) => { state.sessions.push(session(channelId, state.sessions.length)); return sync(); },
    multilive_watch: ({ sessionId }) => { calls.push(['watch', sessionId]); state.watchId = sessionId || null; if (sessionId) state.selectedId = sessionId; return sync(); },
    multilive_remove: ({ sessionId }) => { state.sessions = state.sessions.filter((s) => s.id !== sessionId); return sync(); },
    multilive_select: ({ sessionId }) => { state.selectedId = sessionId; return sync(); },
    multilive_set_view: ({ sessionIds }) => {
      calls.push(['view', sessionIds]);
      picked = sessionIds.slice(0, state.maxMultiSessions);
      pickCount = picked.length;
      if (!picked.includes(state.selectedId)) { state.selectedId = picked[0]; state.focusMainId = picked[0]; }
      return sync();
    },
    multilive_stage_size: () => null,
    popular_lives: () => [],
    following_lives: () => [],
  };
  // 라이브 단독 보기에서 담아 온 것처럼 — 세션만 늘리고 상태를 흘린다.
  const listeners = new Map();
  window.__mock = {
    state, handlers, calls,
    resetPick: () => { picked = []; pickCount = 0; auto = []; },
    addLive: (ids) => { for (const id of ids) state.sessions.push(session(id, state.sessions.length)); },
    emit: () => { const s = sync(); for (const [ev, fns] of listeners) if (ev === 'multilive://state') for (const f of fns) f({ payload: s }); },
  };
  window.__TAURI__ = {
    core: { invoke: async (cmd, args) => { const h = handlers[cmd]; if (!h) throw new Error(`mock: ${cmd}`); return h(args || {}); } },
    event: { listen: async (ev, fn) => { if (!listeners.has(ev)) listeners.set(ev, new Set()); listeners.get(ev).add(fn); return () => listeners.get(ev).delete(fn); } },
  };
  try { localStorage.setItem('cview.onboarded', '1'); localStorage.setItem('cview.welcomed.v1', '1'); } catch { /* 무시 */ }
}

const browser = await engine.launch();
const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
page.setDefaultTimeout(5000);
const errors = [];
page.on('pageerror', (e) => errors.push(String(e.stack || e)));
await page.addInitScript(mockTauri);
await page.goto(base);
await page.waitForSelector('[data-page]', { state: 'attached' });
await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
const navigate = (route) => page.evaluate((r) => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: r, params: {} } })), route);
const settle = () => page.waitForTimeout(400);
const calls = () => page.evaluate(() => window.__mock.calls.slice());
const shot = async (name) => { if (SHOTS) { fs.mkdirSync(SHOTS, { recursive: true }); await page.screenshot({ path: path.join(SHOTS, `${name}.png`) }); } };
const shownPanes = () => page.evaluate(() => [...document.querySelectorAll('[data-page="multilive"] .ml-pane')]
  .filter((p) => !p.hidden && p.offsetParent).map((p) => p.dataset.perfChannel).sort());
const pickButton = () => page.evaluate(() => {
  const b = document.querySelector('[data-page="multilive"] [data-ml="pick"]');
  return b ? { hidden: b.hidden, visible: Boolean(b.offsetParent), n: b.querySelector('.ml-pick-n')?.textContent || '', label: b.getAttribute('aria-label') || '' } : null;
});

const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}

await check('상한 이하(4개) — 고르기 단추 없이 모든 패인이 보인다(예전 동작)', async () => {
  await navigate('multilive');
  await page.waitForSelector('[data-page="multilive"] .ml-stage');
  await settle();
  assert.deepEqual(await shownPanes(), ['c0', 'c1', 'c2', 'c3']);
  const b = await pickButton();
  assert.ok(b && b.hidden && !b.visible, `단추: ${JSON.stringify(b)}`);
});

await check('라이브에 6개 담아 온 채 멀티라이브 — 4개만 패인으로, 단추에 4/6 과 안내 토스트', async () => {
  await page.evaluate(() => { window.__mock.addLive(['c4', 'c5']); window.__mock.state.selectedId = 's-c5'; window.__mock.emit(); });
  await settle();
  const shown = await shownPanes();
  assert.equal(shown.length, 4, `보이는 패인: ${shown}`);
  assert.ok(shown.includes('c5'), '단독 보기에서 보던 채널은 들어간다');
  const b = await pickButton();
  assert.ok(b && b.visible, `단추: ${JSON.stringify(b)}`);
  assert.equal(b.n, '4/6');
  assert.match(b.label, /6개 중 4개/);
  const count = await page.evaluate(() => document.querySelector('[data-page="multilive"] .ml-count').textContent);
  assert.equal(count, '4 / 4');
  const stageClass = await page.evaluate(() => document.querySelector('[data-page="multilive"] .ml-stage').className);
  assert.match(stageClass, /ml-n4/, `4개 격자로: ${stageClass}`);
  const t = await page.evaluate(() => [...document.querySelectorAll('#toast-stack > *')].map((x) => x.textContent).join(' | '));
  assert.match(t, /6개 중 4개만/);
  await shot('after-limited-grid');
});

await check('고르기 시트 — 대화 상자·체크 4개, 가득 차면 나머지는 막히고 누르면 이유를 알린다', async () => {
  await page.click('[data-page="multilive"] [data-ml="pick"]');
  await page.waitForSelector('.ml-pick-sheet[role="dialog"]');
  const info = await page.evaluate(() => {
    const d = document.querySelector('.ml-pick-sheet');
    const boxes = [...d.querySelectorAll('input[type="checkbox"]')];
    return {
      label: document.getElementById(d.getAttribute('aria-labelledby'))?.textContent,
      modal: d.getAttribute('aria-modal'),
      checked: boxes.filter((b) => b.checked).map((b) => b.value).sort(),
      blocked: boxes.filter((b) => b.getAttribute('aria-disabled') === 'true').map((b) => b.value).sort(),
      focusInside: d.contains(document.activeElement),
      status: d.querySelector('.ml-pick-status').textContent,
    };
  });
  assert.equal(info.label, '멀티라이브에서 볼 채널');
  assert.equal(info.modal, 'true');
  assert.equal(info.checked.length, 4);
  assert.equal(info.blocked.length, 2);
  assert.ok(info.focusInside, '초점이 시트 안으로');
  assert.match(info.status, /4 \/ 4개 선택/);
  await shot('after-picker-full');
  const blockedId = info.blocked[0];
  // aria-disabled 줄은 Playwright 가 "누를 수 없음"으로 본다 — 실제 사용자처럼 그대로 누른다.
  await page.click(`.ml-pick-row[data-session-id="${blockedId}"]`, { force: true });
  const after = await page.evaluate((id) => ({
    checked: document.querySelector(`.ml-pick-sheet input[value="${id}"]`).checked,
    status: document.querySelector('.ml-pick-status').textContent,
  }), blockedId);
  assert.equal(after.checked, false, '가득 차면 체크되지 않는다');
  assert.match(after.status, /최대 4개까지예요/);
});

await check('키보드로 하나 빼고 다른 채널 넣기(바꾸기) → 이대로 보기 — 고른 채널로 패인이 바뀐다', async () => {
  const before = await page.evaluate(() => [...document.querySelectorAll('.ml-pick-sheet input:checked')].map((b) => b.value));
  const out = before.find((id) => id !== 's-c5');
  const into = await page.evaluate(() => [...document.querySelectorAll('.ml-pick-sheet input:not(:checked)')].map((b) => b.value)[0]);
  await page.focus(`.ml-pick-sheet input[value="${out}"]`);
  await page.keyboard.press('Space');
  await page.focus(`.ml-pick-sheet input[value="${into}"]`);
  await page.keyboard.press('Space');
  const st = await page.evaluate(() => document.querySelector('.ml-pick-status').textContent);
  assert.match(st, /4 \/ 4개 선택/);
  await shot('after-picker-swapped');
  await page.evaluate(() => { window.__mock.calls.length = 0; });
  await page.click('.ml-pick-sheet [data-pick-apply]');
  await settle();
  const c = await calls();
  const view = c.find((x) => x[0] === 'view');
  assert.ok(view, `호출: ${JSON.stringify(c)}`);
  assert.equal(view[1].length, 4);
  assert.ok(view[1].includes(into) && !view[1].includes(out), `고른 목록: ${view[1]}`);
  const shown = await shownPanes();
  assert.ok(shown.includes(into.slice(2)) && !shown.includes(out.slice(2)), `보이는 패인: ${shown}`);
  assert.equal(await page.evaluate(() => Boolean(document.querySelector('.ml-pick-scrim'))), false, '시트가 닫힌다');
  await shot('after-applied');
});

await check('Esc 로 닫으면 바꾸지 않고 초점은 고르기 단추로', async () => {
  await page.evaluate(() => { window.__mock.calls.length = 0; });
  await page.focus('[data-page="multilive"] [data-ml="pick"]');
  await page.keyboard.press('Enter');
  await page.waitForSelector('.ml-pick-sheet');
  await page.keyboard.press('Space'); // 초점 줄을 바꿔도 적용하지 않으면 그대로
  await page.keyboard.press('Escape');
  await settle();
  assert.equal(await page.evaluate(() => Boolean(document.querySelector('.ml-pick-scrim'))), false);
  assert.equal(await page.evaluate(() => document.activeElement?.dataset?.ml), 'pick');
  assert.equal((await calls()).filter((x) => x[0] === 'view').length, 0);
});

await check('라이브 단독 보기로 가면 고르기 단추가 숨고 모든 채널이 머리줄에 있다', async () => {
  await page.evaluate(() => { window.__mock.state.watchId = 's-c0'; window.__mock.state.selectedId = 's-c0'; window.__mock.emit(); });
  await settle();
  const b = await pickButton();
  assert.ok(b && !b.visible, `단추: ${JSON.stringify(b)}`);
  const chips = await page.evaluate(() => document.querySelectorAll('[data-page="multilive"] .ml-watch-picker [data-session-id]:not([data-ml="watch-close"])').length);
  assert.ok(chips >= 6, `머리줄 채널: ${chips}`);
  await page.evaluate(() => { window.__mock.state.watchId = null; window.__mock.emit(); });
  await settle();
});

await check('패인을 닫아 상한 이하가 되면 고르기는 풀리고 모두 보인다', async () => {
  await page.evaluate(() => { const m = window.__mock; m.state.sessions = m.state.sessions.slice(0, 4); if (!m.state.sessions.some((s) => s.id === m.state.selectedId)) m.state.selectedId = 's-c0'; m.emit(); });
  await settle();
  assert.deepEqual(await shownPanes(), ['c0', 'c1', 'c2', 'c3']);
  const b = await pickButton();
  assert.ok(b && !b.visible);
});

await check('상한보다 적게(2개) 고르면 2개만 보이고 다시 채워지지 않는다', async () => {
  await page.evaluate(() => { const m = window.__mock; m.resetPick(); m.addLive(['c4', 'c5']); m.state.selectedId = 's-c0'; m.emit(); });
  await settle();
  await page.click('[data-page="multilive"] [data-ml="pick"]');
  await page.waitForSelector('.ml-pick-sheet');
  await page.evaluate(() => {
    for (const b of document.querySelectorAll('.ml-pick-sheet input[type="checkbox"]')) {
      const want = b.value === 's-c0' || b.value === 's-c1';
      if (b.checked !== want) b.click();
    }
  });
  assert.match(await page.evaluate(() => document.querySelector('.ml-pick-status').textContent), /2 \/ 4개 선택/);
  await page.click('.ml-pick-sheet [data-pick-apply]');
  await settle();
  await page.evaluate(() => window.__mock.emit()); // 주기 상태가 와도 그대로
  await settle();
  assert.deepEqual(await shownPanes(), ['c0', 'c1']);
  const b = await pickButton();
  assert.equal(b.n, '2/6');
  const stageClass = await page.evaluate(() => document.querySelector('[data-page="multilive"] .ml-stage').className);
  assert.match(stageClass, /ml-n2/);
});

await check("여러 소리에서 숨긴 채널은 '지금 재생 중'으로 고르지 않는다", async () => {
  const supported = await page.evaluate(() => 'mediaSession' in navigator);
  // 보이는 선택 패인(c0)은 무음, 숨긴 c5 는 들림으로 둔 상태 — 예전엔 c5 를 골랐다.
  await page.evaluate(() => {
    const m = window.__mock;
    m.state.multiAudio = true;
    for (const s of m.state.sessions) s.isAudible = s.channelId === 'c5';
    // emit 이 isAudible 을 덮지 않게 한 번 더 맞춘다
    const orig = m.handlers.multilive_state;
    m.emit();
    for (const s of m.state.sessions) s.isAudible = s.channelId === 'c5';
    m.emit();
    return orig && true;
  });
  await settle();
  const titles = await page.evaluate(() => [...document.querySelectorAll('[data-page="multilive"] .ml-pane:not([hidden]) iframe')]
    .map((f) => { try { return f.contentWindow.navigator.mediaSession?.metadata?.title || ''; } catch { return 'x'; } }).filter(Boolean));
  if (supported && titles.length) assert.ok(titles.every((t) => !t.includes('c5')), `지금 재생 중: ${titles}`);
  const slot = await page.evaluate(() => {
    const m = window.__mock.state.sessions.find((s) => s.channelId === 'c5');
    const pane = document.querySelector(`[data-page="multilive"] .ml-pane[data-slot="${m.slot}"] iframe`);
    try { return pane?.contentWindow.navigator.mediaSession?.metadata?.title || ''; } catch { return ''; }
  });
  assert.ok(!slot.includes('c5'), `숨긴 패인 문서의 지금 재생 중: ${slot}`);
  await page.evaluate(() => { const m = window.__mock; m.state.multiAudio = false; for (const s of m.state.sessions) s.isAudible = s.id === m.state.selectedId; m.emit(); });
  await settle();
});

await check('안내 토스트는 실제 멀티라이브 화면에서 처음 제한될 때 한 번만', async () => {
  const toasts = () => page.evaluate(() => [...document.querySelectorAll('#toast-stack > *')].filter((x) => /함께 재생해요/.test(x.textContent)).length);
  const clear = () => page.evaluate(() => document.querySelectorAll('#toast-stack > *').forEach((x) => x.remove()));
  // 상한 아래로 내려 기억을 지운다.
  await page.evaluate(() => { const m = window.__mock; m.resetPick(); m.state.sessions = m.state.sessions.slice(0, 4); m.state.selectedId = 's-c0'; m.emit(); });
  await settle();
  await clear();
  // 라이브 단독 보기(같은 화면을 무대로) 중에 더 담는다 — 띄우지 않는다.
  await navigate('live');
  await page.evaluate(() => { const m = window.__mock; m.state.watchId = 's-c0'; m.addLive(['c4', 'c5']); m.emit(); });
  await settle();
  assert.equal(await toasts(), 0, '단독 보기 중');
  // 단독 보기를 나가는 길 — 상태가 먼저 와 아직 라이브 메뉴(무대 화면이 보이는 채)로 그리는 순간에는 띄우지 않는다.
  const during = await page.evaluate(() => {
    const m = window.__mock; m.state.watchId = null; m.emit();
    return [...document.querySelectorAll('#toast-stack > *')].filter((x) => /함께 재생해요/.test(x.textContent)).length;
  });
  assert.equal(during, 0, '단독 보기를 나가는 길');
  // 앱이 남은 패인이 있어 멀티라이브 화면으로 옮긴다 — 거기서 한 번.
  await settle();
  assert.equal(await page.evaluate(() => !document.querySelector('[data-page="multilive"]').hidden), true);
  assert.equal(await toasts(), 1, '멀티라이브 화면에서 처음');
  await clear();
  // 단독 보기를 다녀와도 다시 뜨지 않는다.
  await page.evaluate(() => { const m = window.__mock; m.state.watchId = 's-c1'; m.emit(); });
  await settle();
  await page.evaluate(() => { const m = window.__mock; m.state.watchId = null; m.emit(); });
  await settle();
  assert.equal(await toasts(), 0, '두 번째는 띄우지 않는다');
});

await browser.close();
server.close();
for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : ` — ${r.error}`}`);
if (errors.length) console.log(`page errors:\n${errors.join('\n')}`);
const failed = results.filter((r) => !r.ok).length + errors.length;
console.log(failed ? `${failed} failed` : `all ${results.length} passed`);
process.exit(failed ? 1 : 0);
