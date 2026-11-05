// 보관함 "개요" 탭 UI 회귀 검사 — 첫 탭이 개요인지, 탭마다 요약 카드(개수·핵심 수치·최근 항목)가 맞게 나오는지,
// 카드를 누르면 그 탭으로 가는지, 빈·로딩·오류·로그인 필요 상태, 폭별 열 수·카드 높이, 개요 옆 인사이트 패널(정리 제안)을 본다.
// app-ui 를 그대로 띄우고 window.__TAURI__ 를 모의 IPC 로 바꿔 실제 화면 코드를 돌린다(실제 서비스·데이터는 건드리지 않음).
// 저장소 루트에서: node tools/ui-regression/library-overview.mjs   (Playwright 가 필요하다. CVIEW_UI_SHOTS=<폴더> 면 스크린샷)
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

// 모의 IPC — mode: 'full'(데이터 있음·로그인) | 'empty'(모두 비었음·로그아웃). window.__mock.handlers 로 바꿀 수 있다.
function mockTauri(mode) {
  const now = Date.now();
  const ago = (min) => new Date(now - min * 60000).toISOString();
  const full = mode === 'full';
  const ch = (i) => ({ channelId: `ch${i}`, channelName: `채널${i}` });
  const calls = [];
  const handlers = {
    auth_status: () => (full ? { loggedIn: true, state: 'loggedIn', nickname: '도도' } : { loggedIn: false, state: 'loggedOut' }),
    settings_get: () => ({ multiChat: {} }),
    insights_overview: () => ({ sessionCount: 0, recent: [], daily: [], channels: [], categories: [], hourly: Array(24).fill(0) }),
    insights_report: () => ({ total: 0, firstWatchedAt: null }),
    following_lives: () => [],
    following_channels_all: () => [],
    popular_lives: () => [],
    popular_clips: () => (full ? Array.from({ length: 5 }, (_, i) => ({ clipUID: `c${i}`, clipTitle: `클립 ${i}`, duration: 30, readCount: (i + 1) * 12000, channel: ch(i % 2) })) : []),
    following_vods: () => (full ? Array.from({ length: 4 }, (_, i) => ({ videoNo: 100 + i, videoTitle: `다시보기 ${i}`, duration: 3600, readCount: 10, publishDate: ago(i * 600), channel: ch(i) })) : []),
    vod_continue_list: () => (full ? [{ videoNo: 1, title: '이어 볼 영상', channelName: '채널1', duration: 3600, position: 1800 }, { videoNo: 2, title: '두 번째', channelName: '채널2', duration: 1200, position: 600 }] : []),
    vod_watch_later_list: () => (full ? [{ videoNo: 3, title: '나중에 볼 영상', channelName: '채널3', duration: 5400, position: 0 }] : []),
    favorites_list: () => (full ? [{ channelId: 'ch1', channelName: '즐찾1', isFavorite: true, lastWatched: ago(60) }, { channelId: 'ch2', channelName: '즐찾2', isFavorite: true, lastWatched: null }] : []),
    recent_channels: ({ limit }) => (full ? Array.from({ length: limit }, (_, i) => ({ channelId: `r${i}`, channelName: `최근${i}`, isFavorite: i === 0, lastWatched: ago(i * 300) })) : []),
    bookmarks_list: () => (full ? [{ id: 'b1', channelId: 'ch1', channelName: '채널1', liveTitle: '옛 장면', liveUptime: 600, capturedAt: ago(3000) }, { id: 'b2', channelId: 'ch2', channelName: '채널2', liveTitle: '새 장면', liveUptime: 1200, capturedAt: ago(5) }] : []),
    download_list: () => (full
      ? { folder: '/tmp/dl', jobs: [{ id: 'j1', kind: 'vod', target: '9', title: '받는 영상', state: 'downloading', progress: 0.4, bytes: 1000 }], history: [{ id: 'clip-c1', title: '받은 클립', completed: true, fileSize: 3 * 1024 * 1024, finishedAt: ago(10), fileStatus: 'ok', filePath: '/tmp/dl/a.mp4' }] }
      : { folder: '/tmp/dl', jobs: [], history: [] }),
  };
  window.__delay = {};
  window.__mock = { calls, handlers };
  window.__TAURI__ = {
    core: {
      invoke: async (cmd, args) => {
        calls.push({ cmd, args });
        const ms = window.__delay[cmd];
        if (ms) await new Promise((r) => setTimeout(r, ms));
        const h = handlers[cmd];
        if (!h) throw new Error(`mock: ${cmd}`);
        return h(args || {});
      },
    },
    event: { listen: async () => () => {} },
  };
  try { localStorage.setItem('cview.onboarded', '1'); } catch { /* 무시 */ }
}

const browser = await chromium.launch();
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}
const SHOTS = process.env.CVIEW_UI_SHOTS;

async function open(mode, viewport = { width: 1280, height: 860 }) {
  const page = await browser.newPage({ viewport });
  page.setDefaultTimeout(5000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, mode);
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
  // 시작 화면이 걷힐 때까지 — 스크린샷이 로고만 찍히지 않게
  await page.waitForFunction(() => { const s = document.getElementById('splash'); return !s || !s.isConnected || getComputedStyle(s).display === 'none' || getComputedStyle(s).opacity === '0'; }, null, { timeout: 8000 }).catch(() => {});
  return { page, errors };
}
const go = (page, route, params = {}) => page.evaluate(([r, p]) => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: r, params: p } })), [route, params]);
const calls = (page, cmd) => page.evaluate((c) => window.__mock.calls.filter((x) => x.cmd === c).length, cmd);
const card = (s) => `[data-page="library"] .lib-ov-card[data-ov-section="${s}"]`;
const text = (page, sel) => page.$eval(sel, (n) => n.textContent.replace(/\s+/g, ' ').trim());
const metrics = (page, s) => page.$eval(`${card(s)} .lib-ov-metrics`, (n) => [...n.querySelectorAll('dt, dd')].map((t) => t.textContent.trim()).join(' '));
const shot = async (page, name) => { if (SHOTS) { fs.mkdirSync(SHOTS, { recursive: true }); await page.screenshot({ path: path.join(SHOTS, `${name}.png`) }); } };
const settled = (page) => page.waitForFunction(() => !document.querySelector('[data-page="library"] .lib-ov-card[data-status="loading"]'));

// ── 데이터 있음 · 로그인 ──
{
  const { page, errors } = await open('full');
  await go(page, 'library');
  await settled(page);
  await shot(page, 'library-overview-full');

  await check('L01 overview is the first and default tab', async () => {
    const tabs = await page.$$eval('[data-page="library"] [role="tab"]', (ts) => ts.map((t) => [t.dataset.tab, t.getAttribute('aria-selected')]));
    assert.deepEqual(tabs[0], ['overview', 'true']);
    assert.equal(tabs.filter((t) => t[1] === 'true').length, 1);
    const order = await page.$$eval('[data-page="library"] .lib-ov-card', (cs) => cs.map((c) => c.dataset.ovSection));
    assert.deepEqual(order, tabs.slice(1).map((t) => t[0]), '카드 순서가 탭 순서와 같음');
  });

  await check('L02 counts, metrics and recent items per tab', async () => {
    const count = (s) => text(page, `${card(s)} .lib-ov-count`);
    assert.equal(await count('clips'), '5개');
    assert.equal(await count('vods'), '4개');
    assert.equal(await count('continue'), '2개');
    assert.equal(await count('later'), '1개');
    assert.equal(await count('favorites'), '2곳');
    assert.equal(await count('recent'), '60+곳');
    assert.equal(await count('bookmarks'), '2개');
    assert.equal(await count('downloads'), '2개');
    assert.match(await metrics(page, 'clips'), /최고 조회 6만/);
    assert.match(await metrics(page, 'continue'), /남은 시간 40분.*평균 진행 50%/);
    assert.match(await metrics(page, 'later'), /전체 길이 1시간 30분/);
    assert.match(await metrics(page, 'downloads'), /받는 중 1개.*용량 3(\.0)? MB/);
    // 최근 항목 — 최대 3개, 북마크는 최신 장면이 먼저
    assert.equal(await page.$$eval(`${card('recent')} .lib-ov-item`, (x) => x.length), 3);
    assert.match(await text(page, `${card('bookmarks')} .lib-ov-item`), /^새 장면/);
    assert.equal(await page.$eval(`${card('continue')} .lib-ov-thumb i`, (i) => i.style.width), '50%');
  });

  await check('L03 clicking a card opens that tab (no refetch for cached clips)', async () => {
    const before = await calls(page, 'popular_clips');
    await page.click(`${card('clips')} .lib-ov-items`);
    await page.waitForSelector('[data-page="library"] .clip-grid');
    assert.equal(await page.getAttribute('[data-page="library"] [data-tab="clips"]', 'aria-selected'), 'true');
    assert.equal(await calls(page, 'popular_clips'), before, '개요에서 받은 클립을 다시 받음');
    await page.click('[data-page="library"] [data-tab="overview"]');
    await settled(page);
    await page.focus(`${card('bookmarks')} .lib-ov-title`);
    await page.keyboard.press('Enter');
    await page.waitForSelector('[data-page="library"] .bookmark-row');
    assert.equal(await page.getAttribute('[data-page="library"] [data-tab="bookmarks"]', 'aria-selected'), 'true');
  });

  await check('L04 one failing section shows its own error and retries alone', async () => {
    await page.evaluate(() => { window.__mock.handlers.favorites_list = () => { throw new Error('network offline'); }; });
    await go(page, 'library', { tab: 'overview' });
    await settled(page);
    await shot(page, 'library-overview-error');
    assert.equal(await page.getAttribute(card('favorites'), 'data-status'), 'error');
    assert.match(await text(page, `${card('favorites')} [role="alert"]`), /인터넷 연결/);
    assert.equal(await page.getAttribute(card('bookmarks'), 'data-status'), 'ok', '다른 카드까지 오류');
    await page.evaluate(() => { window.__mock.handlers.favorites_list = () => [{ channelId: 'x', channelName: '복구', isFavorite: true, lastWatched: null }]; });
    const others = await calls(page, 'bookmarks_list');
    await page.click(`${card('favorites')} [data-ov-retry]`);
    await page.waitForSelector(`${card('favorites')}[data-status="ok"]`);
    assert.equal(await text(page, `${card('favorites')} .lib-ov-count`), '1곳');
    assert.equal(await calls(page, 'bookmarks_list'), others, '다시 시도가 다른 탭까지 다시 받음');
    assert.equal(await page.getAttribute('[data-page="library"] [data-tab="overview"]', 'aria-selected'), 'true', '다시 시도가 탭을 옮김');
  });

  await check('L05 slow section keeps a skeleton while the rest fill in', async () => {
    await page.evaluate(() => { window.__delay.bookmarks_list = 1200; });
    await go(page, 'library', { tab: 'later' });
    await page.waitForTimeout(200);
    await go(page, 'library', { tab: 'overview' });
    await page.waitForSelector(`${card('continue')}[data-status="ok"]`);
    assert.equal(await page.getAttribute(card('bookmarks'), 'aria-busy'), 'true');
    assert.ok(await page.$(`${card('bookmarks')} .sk`), '스켈레톤 없음');
    await page.waitForSelector(`${card('bookmarks')}[data-status="ok"]`, { timeout: 3000 });
    await page.evaluate(() => { window.__delay = {}; });
  });

  await check('L06 leaving mid-load does not paint the overview over another tab', async () => {
    await page.evaluate(() => { window.__delay.bookmarks_list = 600; });
    await go(page, 'library', { tab: 'overview' });
    await page.waitForTimeout(50);
    await go(page, 'library', { tab: 'later' });
    await page.waitForTimeout(900);
    assert.equal(await page.$('[data-page="library"] .lib-ov-card'), null);
    await page.evaluate(() => { window.__delay = {}; });
  });

  // 폭별 열 수 — 묶음(넷)마다 1 · 2 · 4 열만(3열로 한 장이 남지 않게), 같은 줄 카드는 높이가 같다.
  for (const [width, cols] of [[2700, 4], [2000, 4], [1280, 2], [420, 1]]) {
    await check(`L13 ${cols} column(s) and even rows at ${width}px`, async () => {
      await page.setViewportSize({ width, height: 1000 });
      await go(page, 'library', { tab: 'overview' });
      await settled(page);
      await page.waitForTimeout(150);
      if (width === 2000) await shot(page, 'library-overview-wide');
      const r = await page.evaluate(() => [...document.querySelectorAll('[data-page="library"] .lib-ov-grid')].map((g) => {
        const rects = [...g.children].map((c) => c.getBoundingClientRect());
        const tops = [...new Set(rects.map((x) => Math.round(x.top)))];
        return { cols: rects.filter((x) => Math.round(x.top) === tops[0]).length, heights: tops.map((t) => [...new Set(rects.filter((x) => Math.round(x.top) === t).map((x) => Math.round(x.height)))]) };
      }));
      assert.equal(r.length, 2, '묶음 둘');
      for (const g of r) {
        assert.equal(g.cols, cols);
        for (const h of g.heights) assert.equal(h.length, 1, `같은 줄 카드 높이가 다름 ${h}`);
      }
    });
  }
  await page.setViewportSize({ width: 1280, height: 860 });

  await check('L14 insight panel: tidy suggestions on overview, summary on other tabs', async () => {
    await page.evaluate(() => {
      window.__mock.handlers.vod_continue_list = () => [{ videoNo: 1, title: '거의 다 본 영상', channelName: '채널1', duration: 1000, position: 950 }, { videoNo: 2, title: '반쯤', channelName: '채널2', duration: 1200, position: 600 }];
      window.__mock.handlers.favorites_list = () => [{ channelId: 'ch2', channelName: '오래된 즐찾', isFavorite: true, lastWatched: null }];
    });
    await go(page, 'library', { tab: 'later' });
    await page.waitForFunction(() => /보관함 인사이트/.test(document.querySelector('.insight-panel .ip-head')?.textContent || ''));
    await go(page, 'library', { tab: 'overview' });
    await settled(page);
    await page.waitForFunction(() => /보관함 정리/.test(document.querySelector('.insight-panel .ip-head')?.textContent || ''));
    const t = await text(page, '.insight-panel .ip-scroll');
    assert.match(t, /다 본 이어 보기.*거의 다 본 영상 ?95%/);
    assert.doesNotMatch(t, /반쯤/);
    assert.match(t, /30일 넘게 안 본 즐겨찾기.*오래된 즐찾 ?안 봄/);
    assert.equal(await page.$('.insight-panel .ip-tiles'), null, '개요 옆 패널에 개수 타일이 겹침');
    await go(page, 'library', { tab: 'bookmarks' });
    await page.waitForFunction(() => /보관함 인사이트/.test(document.querySelector('.insight-panel .ip-head')?.textContent || ''));
  });

  // 좁은 창 — 카드가 한 줄 한 장으로 접히고 가로로 넘치지 않는다(멀티라이브 옆 화면처럼 좁아져도).
  for (const width of [960, 640, 420]) {
    await check(`L07 no horizontal overflow at ${width}px`, async () => {
      await page.setViewportSize({ width, height: 860 });
      await go(page, 'library', { tab: 'overview' });
      await settled(page);
      await page.waitForTimeout(150);
      if (width === 420) await shot(page, 'library-overview-narrow');
      const r = await page.evaluate(() => {
        const body = document.querySelector('[data-page="library"] .page-body');
        const cards = [...body.querySelectorAll('.lib-ov-card')];
        const b = body.getBoundingClientRect();
        return {
          bodyOverflow: body.scrollWidth - body.clientWidth,
          cardOverflow: cards.filter((c) => c.scrollWidth > c.clientWidth + 1).map((c) => c.dataset.ovSection),
          outside: cards.filter((c) => c.getBoundingClientRect().right > b.right + 1).map((c) => c.dataset.ovSection),
        };
      });
      assert.ok(r.bodyOverflow <= 1, `page-body 가로 넘침 ${r.bodyOverflow}px`);
      assert.deepEqual(r.cardOverflow, []);
      assert.deepEqual(r.outside, []);
    });
  }
  await page.setViewportSize({ width: 1280, height: 860 });

  await check('L08 no page errors (full)', async () => assert.deepEqual(errors, []));
  await page.close();
}

// ── 모두 비었음 · 로그아웃 ──
{
  const { page, errors } = await open('empty');
  await go(page, 'library');
  await settled(page);
  await shot(page, 'library-overview-empty');

  await check('L09 empty sections show a short empty note with zero count', async () => {
    assert.equal(await text(page, `${card('clips')} .lib-ov-count`), '0개');
    assert.match(await text(page, `${card('bookmarks')} .lib-ov-note`), /북마크가 없습니다/);
    assert.match(await text(page, `${card('downloads')} .lib-ov-note`), /받은 파일이 없습니다/);
    // 빈 카드도 데이터 있는 카드와 같은 자리를 채운다(아이콘 · 안내 · 도움말)
    assert.match(await text(page, `${card('bookmarks')} .lib-ov-note small`), /B 키/);
    const hs = await page.$$eval('[data-page="library"] .lib-ov-card', (cs) => [...new Set(cs.map((c) => Math.round(c.getBoundingClientRect().height)))]);
    assert.equal(hs.length, 1, `빈 카드 높이가 제각각 ${hs}`);
    assert.equal(await page.$(`${card('clips')} .lib-ov-metrics`), null);
  });

  await check('L10 logged out: follow VODs card asks to log in without fetching', async () => {
    assert.equal(await page.getAttribute(card('vods'), 'data-status'), 'login');
    assert.ok(await page.$(`${card('vods')} [data-action="login"]`));
    assert.equal(await calls(page, 'following_vods'), 0);
    // 로그인 단추는 탭을 옮기지 않는다(앱의 로그인 흐름만)
    await page.evaluate(() => { window.__mock.handlers.auth_login = () => ({ loggedIn: false }); });
    await page.click(`${card('vods')} [data-action="login"]`).catch(() => {});
    await page.waitForTimeout(100);
    assert.equal(await page.getAttribute('[data-page="library"] [data-tab="overview"]', 'aria-selected'), 'true');
  });

  await check('L11 unknown tab parameter falls back to overview', async () => {
    await go(page, 'library', { tab: 'nope' });
    await settled(page);
    assert.equal(await page.getAttribute('[data-page="library"] [data-tab="overview"]', 'aria-selected'), 'true');
  });

  await check('L12 no page errors (empty)', async () => assert.deepEqual(errors, []));
  await page.close();
}

await browser.close();
server.close();
let failed = 0;
for (const r of results) {
  console.log(`${r.ok ? 'PASS' : 'FAIL'}  ${r.id}${r.ok ? '' : `\n      ${r.error}`}`);
  if (!r.ok) failed += 1;
}
console.log(`\n${results.length - failed}/${results.length} passed`);
process.exit(failed ? 1 : 0);
