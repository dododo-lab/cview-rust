// 검색 결과 화면 UI 회귀 검사 + 스크린샷 — 모의 IPC 로 라이브·채널·다시보기·클립을 채워 검색 화면을 그린다.
// 실제 서비스·데이터는 건드리지 않는다. 썸네일·아바타는 이 서버가 그때그때 만드는 SVG 다(일부는 일부러 깨진 주소).
// 저장소 루트에서: node tools/ui-regression/search.mjs   (CVIEW_UI_SHOTS=<폴더> 면 PNG 를 남긴다)
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');

const ROOT = path.resolve('app-ui');
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.png': 'image/png', '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.json': 'application/json' };
const PALETTE = [['#ff8a5b', '#7b2ff7'], ['#00c2ff', '#00ffa3'], ['#ff5f9e', '#ffb86b'], ['#6a5bff', '#00d4ff'], ['#ffd166', '#ef476f'], ['#06d6a0', '#118ab2'], ['#f72585', '#4361ee'], ['#ff9f1c', '#2ec4b6']];

// /__img/thumb/<n>.svg · /__img/clip/<n>.svg(세로) · /__img/avatar/<n>.svg — 번호별로 색이 다른 그림
function fakeImage(kind, n) {
  const [a, b] = PALETTE[n % PALETTE.length];
  const grad = `<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></linearGradient></defs>`;
  if (kind === 'avatar') return `<svg xmlns="http://www.w3.org/2000/svg" width="120" height="120">${grad}<rect width="120" height="120" fill="url(#g)"/><circle cx="60" cy="48" r="22" fill="#fff" opacity=".85"/><ellipse cx="60" cy="112" rx="38" ry="30" fill="#fff" opacity=".85"/></svg>`;
  const [w, h] = kind === 'clip' ? [720, 1280] : [1280, 720];
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}">${grad}<rect width="${w}" height="${h}" fill="url(#g)"/><circle cx="${(n * 173) % w}" cy="${(n * 97) % h}" r="${160 + (n % 4) * 40}" fill="#fff" opacity=".16"/><circle cx="${(n * 311) % w}" cy="${(n * 211) % h}" r="90" fill="#000" opacity=".12"/></svg>`;
}

const server = http.createServer((req, res) => {
  const url = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  const img = url.match(/^\/__img\/(thumb|clip|avatar)\/(\d+)\.svg$/);
  if (img) { res.writeHead(200, { 'content-type': 'image/svg+xml' }); res.end(fakeImage(img[1], Number(img[2]))); return; }
  const file = path.join(ROOT, url === '/' ? 'index.html' : url);
  if (!file.startsWith(ROOT) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) { res.writeHead(404); res.end(); return; }
  res.writeHead(200, { 'content-type': TYPES[path.extname(file)] || 'application/octet-stream' });
  fs.createReadStream(file).pipe(res);
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${server.address().port}/`;

function mockTauri([origin, counts]) {
  const img = (kind, n) => `${origin}__img/${kind}/${n}.svg`;
  const broken = `${origin}__img/missing/0.png`; // 404 — 깨진 아바타 처리 확인용
  const now = Date.now();
  const day = 86400000;
  const live = (i, viewers) => ({
    channelId: `lch${i}`, channelName: ['푸르히아하우나나', '아로다루송송'][i % 2], channelImageUrl: img('avatar', i), liveTitle: ['루다 talk', '가여운 토끼가 무서운 살인마를 피해서 도망치는 게임'][i % 2],
    thumbnailUrl: img('thumb', i), viewerCount: viewers, viewerCountText: viewers.toLocaleString('ko-KR'), categoryName: ['talk', '키네틱 언트'][i % 2],
    openDate: new Date(now - (78 + i * 150) * 60000).toISOString(), isLive: true, adult: false,
  });
  const channel = (i) => ({
    channelId: `ch${i}`, channelName: ['루다', '루다니', '루다S2', '루다인데요', '루다대디', 'SIRU 시루다냥', '루다루다', '루다방'][i % 8],
    channelImageUrl: i === 2 ? broken : i === 3 ? '' : img('avatar', i + 3), followerCountText: ['2.4만', '866', '123', '43', '8', '1.8만', '5', '71'][i % 8],
    openLive: i === 0, verifiedMark: i === 0,
  });
  const titles = ['[루다극장]', '[건담 이상형 월드컵] / [루다극장]', '[루다식당] 마파두부편', '루다', '260531 미르컵 김니디팀 (w.루다, 리니링)', '루시아팀 vs 포키팡팀 아브실 내전 (w.루시아, 루코, 루다, 연니비)', '2월2일 아브실 내전 연습 루시아팀', '[25.12.20] 아브 롤 내전 (w.탐 동그레/크랭크 정글 쌍베/유봄냥 미드 철면수심/티뭉 원딜 루다/...'];
  const video = (i) => ({ videoNo: 500 + i, videoTitle: titles[i % titles.length], thumbnailImageUrl: img('thumb', 10 + i), duration: 9000 + i * 1300, readCount: 80 + i * 37, publishDate: new Date(now - (9 + i * 11) * day).toISOString(), channel: { channelName: ['루다', '니니아', '옛따', '동그레'][i % 4] } });
  const clipTitles = ['더보라고', '호두 관찰 캠', '우사기화 되가는 루다', '감웃아져씨 루다', '무..무슨 메이드요?', '루다 치이카와 댄스', 'ㅇㅇ', 'ㅇㅇ'];
  const clip = (i) => ({ clipUID: `c${i}`, clipTitle: clipTitles[i % clipTitles.length], thumbnailImageUrl: img('clip', 30 + i), duration: 20 + (i * 13) % 70, readCount: 4 + i * 19 });
  const handlers = {
    auth_status: () => ({ loggedIn: false, state: 'loggedOut' }),
    settings_get: () => ({ multiLive: { maxConcurrentSessions: 4, maxLiveChannels: 5 }, multiChat: {}, player: {}, chat: {}, general: {} }),
    popular_lives: () => [],
    search: () => ({
      lives: Array.from({ length: counts.lives }, (_, i) => live(i, [324, 260][i % 2])),
      channels: Array.from({ length: counts.channels }, (_, i) => channel(i)),
      clips: Array.from({ length: counts.clips }, (_, i) => clip(i)),
      hasMoreClips: true, errors: {},
    }),
    search_videos: () => ({ items: Array.from({ length: counts.videos }, (_, i) => video(i)), hasMore: true }),
  };
  window.__mock = { handlers, calls: [] };
  window.__TAURI__ = {
    core: { invoke: async (cmd, args) => { window.__mock.calls.push(cmd); const h = handlers[cmd]; if (!h) throw new Error(`mock: ${cmd}`); return h(args); } },
    event: { listen: async () => () => {} },
  };
  try { localStorage.setItem('cview.onboarded', '1'); } catch { /* 무시 */ }
}

const SHOTS = process.env.CVIEW_UI_SHOTS;
const browser = await chromium.launch();
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}

const FULL = { lives: 2, channels: 6, videos: 24, clips: 20 };
async function openSearch({ width = 1680, height = 1000, scheme = 'dark', counts = FULL } = {}) {
  const page = await browser.newPage({ viewport: { width, height }, colorScheme: scheme });
  page.setDefaultTimeout(5000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, counts]);
  await page.goto(base);
  await page.waitForSelector('.nav-item, .side-nav', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim').forEach((x) => x.remove()));
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: 'search', params: { query: '루다' } } })));
  await page.waitForSelector('[data-page="search"]:not([hidden]) [data-stab="all"]');
  await page.waitForTimeout(900);
  return { page, errors };
}

async function shotFull(page, name) {
  if (!SHOTS) return;
  fs.mkdirSync(SHOTS, { recursive: true });
  await page.mouse.move(2, 2);
  await page.waitForTimeout(400);
  const h = await page.evaluate(() => {
    const el = document.querySelector('[data-page="search"]');
    el.scrollTop = 0;
    return Math.ceil(el.scrollHeight + el.getBoundingClientRect().top + 8);
  });
  const vp = page.viewportSize();
  await page.setViewportSize({ width: vp.width, height: Math.max(vp.height, h) });
  await page.waitForTimeout(500);
  await page.screenshot({ path: path.join(SHOTS, `${name}.png`) });
  await page.setViewportSize(vp);
}

const wide = await openSearch();
await check('S01 search results render without errors', async () => {
  assert.deepEqual(wide.errors, []);
  assert.equal(await wide.page.locator('[data-page="search"] .search-section').count(), 4);
});
await check('S02 every "all" preview is a single row', async () => {
  const rows = await wide.page.evaluate(() => [...document.querySelectorAll('[data-page="search"] .search-section')].map((s) => {
    const list = s.querySelector('.search-preview');
    if (!list) return { id: s.dataset.section, rows: -1 };
    const tops = new Set([...list.children].map((c) => Math.round(c.getBoundingClientRect().top)));
    return { id: s.dataset.section, rows: tops.size };
  }));
  for (const r of rows) assert.equal(r.rows, 1, `${r.id} rows ${r.rows}`);
});
await check('S03 sections are evenly spaced', async () => {
  const gaps = await wide.page.evaluate(() => {
    const s = [...document.querySelectorAll('[data-page="search"] .search-section')];
    return s.slice(1).map((x, i) => Math.round(x.getBoundingClientRect().top - s[i].getBoundingClientRect().bottom));
  });
  assert.ok(gaps.every((g) => g === gaps[0] && g >= 24), `gaps ${gaps}`);
});
await check('S04 broken or missing channel avatar falls back to an initial', async () => {
  await wide.page.waitForFunction(() => document.querySelectorAll('[data-page="search"] .cr-avatar.fallback').length >= 2);
  const ok = await wide.page.evaluate(() => [...document.querySelectorAll('[data-page="search"] .cr-avatar.fallback')].every((a) => [...a.querySelectorAll('img')].every((i) => getComputedStyle(i).display === 'none') && a.textContent.trim().length === 1));
  assert.ok(ok);
});
await check('S05 every section head shows a count', async () => {
  const counts = await wide.page.locator('[data-page="search"] .search-section .section-title .count').allTextContents();
  assert.deepEqual(counts, ['2', '6', '24+', '20+']);
});
await check('S06 media titles reserve the same height within a row', async () => {
  const heights = await wide.page.evaluate(() => [...document.querySelectorAll('[data-section="videos"] .card-title')].map((t) => Math.round(t.getBoundingClientRect().height)));
  assert.ok(new Set(heights).size === 1, `heights ${heights}`);
});
await check('S07 shelf arrows scroll the preview', async () => {
  const shelf = wide.page.locator('[data-section="clips"] .search-preview');
  const before = await shelf.evaluate((el) => el.scrollLeft);
  await wide.page.locator('[data-section="clips"] [data-shelf="1"]').click();
  await wide.page.waitForTimeout(700);
  assert.ok(await shelf.evaluate((el) => el.scrollLeft) > before);
  await shelf.evaluate((el) => { el.scrollLeft = 0; });
});
await shotFull(wide.page, 'search-all');
await check('S10 shelves re-measure when the body narrows without a window resize', async () => {
  const nav = wide.page.locator('[data-section="lives"] .shelf-nav');
  assert.equal(await nav.isVisible(), false, 'two lives fit at full width');
  await wide.page.evaluate(() => { document.querySelector('[data-page="search"] .page-body').style.width = '420px'; });
  await wide.page.waitForTimeout(200);
  assert.equal(await nav.isVisible(), true, 'arrows after the body narrows');
  await wide.page.evaluate(() => { document.querySelector('[data-page="search"] .page-body').style.width = ''; });
  await wide.page.waitForTimeout(200);
  assert.equal(await nav.isVisible(), false);
});
await check('S11 filter buttons expose the pressed state', async () => {
  assert.equal(await wide.page.locator('[data-stab="all"]').first().getAttribute('aria-pressed'), 'true');
  assert.equal(await wide.page.locator('[data-page="search"] [role="tab"]').count(), 0);
});
await check('S08 videos tab shows the full grid', async () => {
  await wide.page.locator('[data-stab="videos"]').first().click();
  assert.equal(await wide.page.locator('[data-page="search"] .search-section[data-section="videos"] .grid > .card').count(), 24);
  assert.equal(await wide.page.locator('[data-page="search"] .search-section').count(), 1);
});
await shotFull(wide.page, 'search-videos');
await wide.page.locator('[data-stab="clips"]').first().click();
await shotFull(wide.page, 'search-clips');
await wide.page.close();

const narrow = await openSearch({ width: 900, height: 800 });
await check('S09 narrow window has no horizontal overflow', async () => {
  const over = await narrow.page.evaluate(() => {
    const el = document.querySelector('[data-page="search"]');
    return el.scrollWidth - el.clientWidth;
  });
  assert.ok(over <= 1, `overflow ${over}px`);
  assert.deepEqual(narrow.errors, []);
});
await shotFull(narrow.page, 'search-narrow');
await narrow.page.close();

const light = await openSearch({ scheme: 'light' });
await light.page.evaluate(() => document.documentElement.setAttribute('data-theme', 'light'));
await shotFull(light.page, 'search-light');
await light.page.close();

await browser.close();
server.close();
let failed = 0;
for (const r of results) {
  console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : ` — ${r.error}`}`);
  if (!r.ok) failed++;
}
process.exit(failed ? 1 : 0);
