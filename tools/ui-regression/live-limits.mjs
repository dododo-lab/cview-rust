// 라이브 단독 보기 ↔ 멀티라이브 채널 수 상한 분리 — 설정 화면 두 슬라이더, 라이브로 여는 추가가 라이브 상한을 따르는지 본다.
// app-ui 를 그대로 띄우고 window.__TAURI__ 를 모의 IPC 로 바꿔 실제 화면 코드를 돌린다(실제 서비스·데이터는 건드리지 않음).
// 저장소 루트에서: node tools/ui-regression/live-limits.mjs   (Playwright 가 필요하다)
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

// 멀티라이브 4/4(가득) · 라이브 단독 보기 상한 8 — 예전엔 둘이 같은 값이라 라이브로 열면 예전 시청 화면으로 빠졌다.
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
  const sync = () => {
    const n = state.sessions.length;
    state.canAddLive = n < state.maxLiveSessions;
    state.maxSessions = state.watchId ? state.maxLiveSessions : state.maxMultiSessions;
    state.canAdd = n < state.maxSessions;
    state.rev++;
    return JSON.parse(JSON.stringify(state));
  };
  const calls = [];
  const lives = ['p1', 'p2'].map((id) => ({ channelId: id, channelName: `이름-${id}`, liveTitle: `방송 ${id}`, isLive: true, viewerCount: 100, openDate: new Date().toISOString() }));
  const handlers = {
    auth_status: () => ({ loggedIn: false, state: 'loggedOut' }),
    settings_get: () => ({ multiLive: { maxConcurrentSessions: 4, maxLiveChannels: 8 }, multiChat: {}, player: {}, chat: {}, general: {} }),
    multilive_state: () => sync(),
    multilive_add: ({ channelId, live }) => {
      calls.push(['add', channelId, live]);
      const cap = live ? state.maxLiveSessions : state.maxMultiSessions;
      if (state.sessions.length >= cap) throw new Error('더 이상 추가할 수 없습니다 (최대 세션 수)');
      state.sessions.push(session(channelId, state.sessions.length));
      return sync();
    },
    multilive_watch: ({ sessionId }) => { calls.push(['watch', sessionId]); state.watchId = sessionId; return sync(); },
    multilive_remove: ({ sessionId }) => { state.sessions = state.sessions.filter((s) => s.id !== sessionId); return sync(); },
    multilive_select: ({ sessionId }) => { state.selectedId = sessionId; return sync(); },
    popular_lives: () => lives,
    following_lives: () => [],
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
const settle = () => page.waitForTimeout(400);
const calls = () => page.evaluate(() => window.__mock.calls.slice());

const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}

await check('설정: 멀티라이브·라이브 단독 보기 상한이 따로 있다', async () => {
  await navigate('settings');
  await page.waitForSelector('[data-group="multi"]');
  await page.click('[data-group="multi"]');
  const v = await page.evaluate(() => ({
    multi: document.querySelector('input[data-domain="multiLive"][data-key="maxConcurrentSessions"]')?.value,
    live: document.querySelector('input[data-domain="multiLive"][data-key="maxLiveChannels"]')?.value,
  }));
  assert.deepEqual(v, { multi: '4', live: '8' });
});

await check('멀티라이브가 가득이어도 라이브로 열면 라이브 상한으로 추가해 단독 보기로 올린다', async () => {
  await navigate('multilive');
  await page.waitForSelector('[data-page="multilive"] .ml-stage');
  await settle();
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:play', { detail: { channelId: 'c9' } })));
  await settle();
  const c = await calls();
  assert.deepEqual(c.find((x) => x[0] === 'add'), ['add', 'c9', true], `추가 호출: ${JSON.stringify(c)}`);
  assert.deepEqual(c.find((x) => x[0] === 'watch'), ['watch', 's-c9']);
});

await check('라이브 단독 보기의 채널 추가(+)도 라이브 상한으로 담고 수 표시는 5 / 8', async () => {
  await page.evaluate(() => { window.__mock.calls.length = 0; });
  await page.evaluate(() => [...document.querySelectorAll('[data-ml="add"]')].find((b) => b.offsetParent).click());
  await page.waitForSelector('.ml-addrow-act[data-channel-id="p1"]');
  const count = await page.evaluate(() => document.querySelector('.ml-addbar-count, .ml-addbar [class*="count"]')?.textContent || '');
  assert.ok(count.includes('5 / 8'), `수 표시: ${count}`);
  await page.click('.ml-addrow-act[data-channel-id="p1"]');
  await settle();
  const c = await calls();
  assert.deepEqual(c.find((x) => x[0] === 'add'), ['add', 'p1', true], `추가 호출: ${JSON.stringify(c)}`);
});

await browser.close();
server.close();
for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : ` — ${r.error}`}`);
if (errors.length) console.log(`page errors:\n${errors.join('\n')}`);
const failed = results.filter((r) => !r.ok).length + errors.length;
console.log(failed ? `${failed} failed` : `all ${results.length} passed`);
process.exit(failed ? 1 : 0);
