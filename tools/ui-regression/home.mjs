// 홈 화면 UI 회귀 검사 + 스크린샷 — 모의 IPC 로 팔로잉·인기·최근·이어 보기·클립을 채워 홈을 그린다.
// 실제 서비스·데이터는 건드리지 않는다. 썸네일·아바타는 이 서버가 그때그때 만드는 SVG 다.
// 저장소 루트에서: node tools/ui-regression/home.mjs   (CVIEW_UI_SHOTS=<폴더> 면 PNG 를 남긴다)
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const { chromium, webkit } = require('playwright');

const ROOT = path.resolve('app-ui');
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.png': 'image/png', '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.json': 'application/json' };
const PALETTE = [['#ff8a5b', '#7b2ff7'], ['#00c2ff', '#00ffa3'], ['#ff5f9e', '#ffb86b'], ['#6a5bff', '#00d4ff'], ['#ffd166', '#ef476f'], ['#06d6a0', '#118ab2'], ['#f72585', '#4361ee'], ['#ff9f1c', '#2ec4b6']];

// /__img/thumb/<n>.svg · /__img/avatar/<n>.svg — 번호별로 색이 다른 그림
function fakeImage(kind, n) {
  const [a, b] = PALETTE[n % PALETTE.length];
  if (kind === 'avatar') {
    return `<svg xmlns="http://www.w3.org/2000/svg" width="120" height="120"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></linearGradient></defs><rect width="120" height="120" fill="url(#g)"/><circle cx="60" cy="48" r="22" fill="#fff" opacity=".85"/><ellipse cx="60" cy="112" rx="38" ry="30" fill="#fff" opacity=".85"/></svg>`;
  }
  const c = (i) => 120 + ((n * 97 + i * 211) % 1040);
  return `<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></linearGradient></defs><rect width="1280" height="720" fill="url(#g)"/><circle cx="${c(1)}" cy="${c(2) % 720}" r="${120 + (n % 5) * 40}" fill="#fff" opacity=".16"/><circle cx="${c(3)}" cy="${c(4) % 720}" r="${80 + (n % 3) * 50}" fill="#000" opacity=".12"/><circle cx="${c(5)}" cy="${c(6) % 720}" r="60" fill="#fff" opacity=".2"/></svg>`;
}

const server = http.createServer((req, res) => {
  const url = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  const img = url.match(/^\/__img\/(thumb|avatar)\/(\d+)\.svg$/);
  if (img) { res.writeHead(200, { 'content-type': 'image/svg+xml' }); res.end(fakeImage(img[1], Number(img[2]))); return; }
  const file = path.join(ROOT, url === '/' ? 'index.html' : url);
  if (!file.startsWith(ROOT) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) { res.writeHead(404); res.end(); return; }
  res.writeHead(200, { 'content-type': TYPES[path.extname(file)] || 'application/octet-stream' });
  fs.createReadStream(file).pipe(res);
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${server.address().port}/`;

function mockTauri([origin, loggedIn]) {
  const img = (kind, n) => `${origin}__img/${kind}/${n}.svg`;
  const names = ['릴카', '플러리', '따효니', '괴물쥐', '침착맨', '녹두로', '한동숙', '풍월량', '김도', '소풍왔니', '마라맛', '서새봄', '우왁굳', '탬탬버린', '강지', '삼식', '얍얍', '비챤', '고세구', '징버거', '주르르', '아이네', '릴파', '해쭈'];
  const cats = ['마인크래프트', '메이플스토리', '배틀그라운드', '리그 오브 레전드', '스타크래프트', '종합 게임', '발로란트', '저스트 채팅'];
  const titles = ['시청자 참여 내전 + 경품', '신작 첫 플레이! 스포 금지', '혼돈의 서버 생존기', '대회 리뷰 & 분석', '저녁 먹고 수다 한 판', '랭크 올리기 마지막 날', '새벽 감성 노래방', '보스 레이드 공략', '뉴비와 함께하는 탐험', '구독자 1만 기념 방송'];
  const now = Date.now();
  const live = (i, viewers) => ({
    channelId: `ch${i}`, channelName: names[i % names.length] + (i >= names.length ? i : ''), channelImageUrl: img('avatar', i),
    liveTitle: titles[i % titles.length], thumbnailUrl: img('thumb', i), viewerCount: viewers, viewerCountText: viewers.toLocaleString('ko-KR'),
    categoryName: cats[(i * 3) % cats.length], openDate: new Date(now - (20 + i * 17) * 60000).toISOString(), isLive: true, adult: i === 13,
  });
  const following = [56012, 39120, 37011, 22030, 8210, 6120, 4410, 2030, 1200].map((v, i) => live(i, v));
  const popular = Array.from({ length: 48 }, (_, i) => live(i, Math.round(98000 / (1 + i * 0.35))));
  const recent = [0, 9, 30, 6].map((i) => ({ channelId: `ch${i}`, channelName: i === 30 ? '오프라인채널' : names[i % names.length], channelImageUrl: img('avatar', i) }));
  const cont = Array.from({ length: 6 }, (_, i) => ({ videoNo: 100 + i, title: `${titles[(i + 3) % titles.length]} 다시보기`, thumbnailUrl: img('thumb', 20 + i), duration: 7200, position: 1200 + i * 900, channelName: names[i + 2] }));
  const clips = Array.from({ length: 10 }, (_, i) => ({ clipUID: `c${i}`, clipTitle: `레전드 순간 모음 #${i + 1}`, thumbnailImageUrl: img('thumb', 30 + i), duration: 40 + i * 7, readCount: 120000 - i * 9000, channel: { channelName: names[(i + 4) % names.length] } }));
  const overview = { sessionCount: 36, streak: 4, last7: 27000, average: 2400, countedSessionCount: 36, uniqueChannelCount: 8,
    recent: [{ channelId: 'ch0', channelName: names[0], seconds: 2400, startedAt: new Date(now - 86400000).toISOString(), thumbnailUrl: img('thumb', 0) }],
    daily: Array.from({ length: 7 }, (_, i) => ({ date: new Date(now - (6 - i) * 86400000).toISOString(), seconds: (i + 1) * 900 })),
    channels: [], categories: [], hourly: Array(24).fill(0) };
  const report = (period) => {
    const total = period === '90d' ? 288000 : 108000;
    return { period, total, previousTotal: total * .8, days: period === '90d' ? 90 : 30, activeDays: 24, sessionCount: 36, averageSession: 3000, dailyAverage: 3600,
      longest: { channelId: 'ch0', channelName: names[0], category: cats[0], startedAt: new Date(now - 86400000).toISOString(), seconds: 14400 },
      channelCount: 8, categoryCount: 5, currentStreak: 4, bestStreak: 12, firstWatchedAt: new Date(now - 100 * 86400000).toISOString(),
      channels: names.slice(0, 8).map((name, i) => ({ channelId: `ch${i}`, name, seconds: total * [0.3, .2, .15, .1, .09, .07, .05, .04][i], sessions: 8 - i, lastWatchedAt: new Date(now - i * 86400000).toISOString() })),
      categories: cats.slice(0, 5).map((name, i) => ({ name, seconds: total * [.4, .25, .2, .1, .05][i] })),
      series: cats.slice(0, 4), otherChannels: { count: 0, seconds: 0 }, otherCategories: { count: 0, seconds: 0 },
      heatmap: Array.from({ length: 7 }, (_, d) => Array.from({ length: 24 }, (_, h) => h >= 18 ? (d + 1) * 120 : 0)),
      lengths: [600, 1800, 3600, 7200, 14400, null].map((maxSeconds) => ({ maxSeconds, count: 6, seconds: 18000 })),
    };
  };
  const health = { available: true, storeBytes: 1230000, freeBytes: 1e11, backups: [], tables: [{ label: '시청 기록', rows: 36 }], backupDir: '/tmp/cview-mock-backup', storePath: '/tmp/cview-mock-store' };
  const handlers = {
    auth_status: () => (loggedIn ? { loggedIn: true, state: 'loggedIn', nickname: '도도', profileImageUrl: img('avatar', 3) } : { loggedIn: false, state: 'loggedOut' }),
    settings_get: () => ({ multiLive: { maxConcurrentSessions: 4, maxLiveChannels: 5 }, multiChat: {}, player: {}, chat: {}, general: {} }),
    following_lives: () => following,
    popular_lives: () => popular,
    recent_channels: () => recent,
    vod_continue_list: () => cont,
    popular_clips: () => clips,
    favorites_list: () => [],
    insights_report: ({ period }) => report(period),
    insights_overview: () => overview,
    data_health: () => health,
    data_cleanup: () => health,
    data_backup_now: () => health,
  };
  window.__mock = { handlers, calls: [] };
  window.__TAURI__ = {
    core: { invoke: async (cmd, args) => { window.__mock.calls.push(cmd); const h = handlers[cmd]; if (!h) throw new Error(`mock: ${cmd}`); return h(args); } },
    event: { listen: async () => () => {} },
  };
  try { localStorage.setItem('cview.welcomed.v1', String(Date.now())); } catch { /* 무시 */ }
}

const SHOTS = process.env.CVIEW_UI_SHOTS;
const browser = await (process.env.CVIEW_UI_BROWSER === 'webkit' ? webkit : chromium).launch();
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}

async function openHome({ loggedIn, width = 1440, height = 900, scheme = 'dark' }) {
  const page = await browser.newPage({ viewport: { width, height }, colorScheme: scheme });
  page.setDefaultTimeout(5000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, loggedIn]);
  await page.goto(base);
  await page.waitForSelector('.page-home:not([hidden]) .home', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim').forEach((x) => x.remove()));
  await page.waitForSelector('[data-rail="clips"]:not([hidden])');
  await page.waitForTimeout(700);
  return { page, errors };
}

const go = (page, route, params = {}) => page.evaluate(([r, p]) => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: r, params: p } })), [route, params]);
const reportReady = (page) => page.waitForFunction(() => {
  const root = document.querySelector('.home-report');
  return root && !root.hidden && root.querySelector('.stats-body')?.children.length && !root.querySelector('.stats-body .loading');
});

// 화면 전체(스크롤 영역 포함)를 한 장으로 — 스크롤 칸 높이만큼 창을 늘려 찍는다.
async function shotFull(page, name) {
  if (!SHOTS) return;
  fs.mkdirSync(SHOTS, { recursive: true });
  await page.evaluate(() => { for (const el of document.querySelectorAll('.page-home, .content')) el.scrollTop = 0; });
  await page.waitForTimeout(700); // 히어로 페이드(hero-in)가 끝난 뒤
  await page.screenshot({ path: path.join(SHOTS, `${name}.png`) });
  const h = await page.evaluate(() => {
    const el = document.querySelector('.page-home');
    return Math.ceil(el.scrollHeight + el.getBoundingClientRect().top + 8);
  });
  const vp = page.viewportSize();
  await page.setViewportSize({ width: vp.width, height: Math.max(vp.height, h) });
  await page.waitForTimeout(400);
  await page.screenshot({ path: path.join(SHOTS, `${name}-full.png`) });
  await page.setViewportSize(vp);
}

const login = await openHome({ loggedIn: true });
await check('H01 logged-in home renders without errors', async () => {
  assert.deepEqual(login.errors, []);
  assert.ok(await login.page.locator('.home .hero-main').count(), 'hero');
  assert.match(await login.page.locator('.home-greeting').textContent(), /도도님/);
});
await check('H02 category chip filters discover grid', async () => {
  const chip = login.page.locator('.home .chip[data-category]:not([data-category=""])').first();
  const name = await chip.getAttribute('data-category');
  await chip.click();
  const metas = await login.page.locator('.popular-grid .card .category').allTextContents();
  assert.ok(metas.length > 0);
  for (const m of metas) assert.ok(m.includes(name), m);
  await login.page.locator('.home .chip[data-category=""]').click();
});
await check('H03 hero pager switches slide', async () => {
  const items = login.page.locator('[data-hero-index]');
  assert.ok(await items.count() >= 2);
  await items.nth(1).click();
  const title = await login.page.locator('.hero-main .hero-title').textContent();
  assert.equal(title.trim(), '시청자 참여 내전 + 경품');
});
await login.page.locator('[data-hero-index]').first().click();
await login.page.mouse.move(5, 890);
// 창이 포커스를 잃으면 넘김 진행 막대 애니메이션을 뺀다 — 멈춤(paused)만으로는 창 서버가 계속 합성했다(motion.js 루트 app-away).
await check('H16 hero progress animation is removed while the window is unfocused', async () => {
  const p = login.page;
  const anim = () => p.evaluate(() => getComputedStyle(document.querySelector('.page-home .queue-item.active .queue-progress')).animationName);
  const setFocus = (on) => p.evaluate((v) => {
    document.hasFocus = () => v;
    window.dispatchEvent(new Event(v ? 'focus' : 'blur'));
  }, on);
  await setFocus(true);
  assert.equal(await anim(), 'queue-progress');
  await setFocus(false);
  await p.waitForFunction(() => document.documentElement.classList.contains('app-away'));
  assert.equal(await anim(), 'none');
  await setFocus(true);
  assert.equal(await anim(), 'queue-progress');
  await p.evaluate(() => { delete document.hasFocus; });
});
await shotFull(login.page, 'home-login');
await check('H06 following, spotlight and discover do not repeat live channels', async () => {
  const groups = await login.page.evaluate(() => ['.lf .lf-item[data-channel-id]', '.hero-main', '.popular-grid .card'].map((sel) => [...document.querySelectorAll(sel)].map((el) => el.dataset.channelId)));
  const all = groups.flat();
  assert.equal(new Set(all).size, all.length);
  assert.equal(await login.page.locator('.top-shelf').count(), 0, 'one discovery area replaces repeated ranked shelves');
  assert.equal(await login.page.locator('.home-history .section-title').count(), 1, 'one recent watching heading');
  assert.equal(await login.page.locator('.home-history [data-rail="continue"]:not([hidden])').count(), 1);
});
await check('H07 recap shares a 30-day report and opens inside home', async () => {
  const p = login.page;
  await p.waitForSelector('[data-home-recap][aria-busy="false"]');
  assert.match(await p.locator('[data-home-recap]').textContent(), /릴카.*마인크래프트.*36회/);
  await p.locator('[data-home-report]').click();
  await reportReady(p);
  assert.equal(await p.locator('.page-home').isVisible(), true);
  assert.equal(await p.locator('.home-live-view').isVisible(), false);
  assert.equal(await p.locator('#nav [data-route="home"]').getAttribute('aria-current'), 'page');
  assert.equal(await p.locator('#nav [data-route="stats"]').count(), 0);
  assert.equal(await p.locator('.side-widget').count(), 0);
  assert.equal(await p.locator('.home-tabs [data-tab="report"]').getAttribute('aria-selected'), 'true');
  assert.equal(await p.locator('.home-report .kpi-card').count(), 4);
  assert.equal(await p.locator('.home-report [data-chart="trend"]').count(), 0);
  assert.doesNotMatch(await p.locator('.home-report .stats-body').textContent(), /연속 시청|오늘 시청/);
  assert.deepEqual(await p.locator('.home-report .stats-period [data-period]').evaluateAll((els) => els.map((el) => el.dataset.period)), ['30d', '90d', 'all']);
  assert.equal(await p.evaluate(() => window.__mock.calls.filter((c) => c === 'insights_report').length), 1, 'preview and detail reuse the same report');
  assert.equal(await p.locator('.insight-panel').isVisible(), true, 'home insights remain available');
});
await shotFull(login.page, 'home-report');
await check('H08 report tabs preserve ranking, time charts, history and data tools', async () => {
  const p = login.page;
  await p.locator('.home-report [data-period="90d"]').click();
  await reportReady(p);
  await p.locator('.home-report [role="tab"][data-tab="ranking"]').click();
  await reportReady(p);
  assert.equal(await p.locator('.home-report .rt-row:not(.rt-head)').count(), 8);
  await p.locator('.home-report [role="tab"][data-tab="time"]').click();
  await reportReady(p);
  assert.equal(await p.locator('.home-report .hm-grid .hm-cell').count(), 168);
  assert.equal(await p.locator('.home-report .len-bars li').count(), 6);
  await p.focus('.home-report [data-chart="heat"]');
  await p.keyboard.press('End');
  assert.match(await p.locator('.home-report [data-stats-live]').textContent(), /^일요일 23시/);
  await p.keyboard.press('Escape');
  assert.equal(await p.locator('.home-report .chart-tip').isVisible(), false);
  await p.locator('.home-report [role="tab"][data-tab="history"]').click();
  await reportReady(p);
  assert.equal(await p.locator('.home-report .history-row').count(), 1);
  await p.locator('.home-report [role="tab"][data-tab="data"]').click();
  await reportReady(p);
  assert.equal(await p.locator('.home-report .stats-period').isVisible(), false);
  await p.locator('.home-report [data-kind="oldHistory"]').click();
  await p.locator('.cleanup-confirm').waitFor();
  await p.keyboard.press('Escape');
  assert.equal(await p.evaluate(() => window.__mock.calls.filter((c) => c === 'data_cleanup').length), 0, 'cancel does not delete data');
  await p.locator('.home-report [data-stats="backup"]').click();
  await p.waitForFunction(() => window.__mock.calls.includes('data_backup_now'));
  assert.deepEqual(login.errors, []);
});
await shotFull(login.page, 'home-report-data');
await check('H09 delayed report cannot overwrite the data tab', async () => {
  const p = login.page;
  await p.evaluate(() => {
    window.__savedReport = window.__mock.handlers.insights_report;
    window.__mock.handlers.insights_report = (args) => new Promise((resolve) => { window.__releaseReport = () => resolve(window.__savedReport(args)); });
  });
  await go(p, 'stats', { tab: 'overview', period: 'all' });
  await p.waitForFunction(() => typeof window.__releaseReport === 'function');
  await p.locator('.home-report [role="tab"][data-tab="data"]').click();
  await reportReady(p);
  await p.evaluate(() => { window.__releaseReport(); window.__mock.handlers.insights_report = window.__savedReport; });
  await p.waitForFunction(() => document.querySelector('.home-report [data-stats="backup"]'));
  assert.equal(await p.locator('.home-report .rank-table').count(), 0);
  assert.equal(await p.locator('.home-report [data-tab="data"]').getAttribute('aria-selected'), 'true');
});
await check('H10 legacy stats links and home keyboard tabs use one home', async () => {
  const p = login.page;
  await go(p, 'stats', { tab: 'time', period: '7d' });
  await reportReady(p);
  assert.equal(await p.locator('.home-report [data-period="30d"]').getAttribute('aria-pressed'), 'true');
  assert.equal(await p.locator('.home-report [data-tab="time"]').getAttribute('aria-selected'), 'true');
  await p.focus('.home-tabs [data-tab="report"]');
  await p.keyboard.press('ArrowLeft');
  await p.locator('.home-live-view').waitFor({ state: 'visible' });
  assert.equal(await p.locator('.home-tabs [data-tab="live"]').getAttribute('aria-selected'), 'true');
  assert.equal(await p.locator('.home-report .chart-tip').isVisible(), false);
  await p.keyboard.press('ArrowRight');
  await reportReady(p);
  assert.equal(await p.locator('.home-tabs [data-tab="report"]').getAttribute('aria-selected'), 'true');
});
await check('H11 refresh clears the cached report and retry recovers an error', async () => {
  const p = login.page;
  await p.evaluate(() => { window.__mock.handlers.insights_report = () => { throw new Error('network offline'); }; });
  await p.locator('.home-header [data-action="refresh"]').click();
  await p.locator('.home-report [data-retry]').waitFor();
  assert.match(await p.locator('.home-report .stats-body').textContent(), /통계를 불러오지 못했습니다/);
  await p.evaluate(() => { window.__mock.handlers.insights_report = window.__savedReport; });
  await p.locator('.home-report [data-retry]').click();
  await p.locator('.home-report .hm-grid').waitFor();
  assert.deepEqual(login.errors, []);
});
await login.page.close();

const guest = await openHome({ loggedIn: false });
await check('H04 guest home shows login banner and popular hero', async () => {
  assert.deepEqual(guest.errors, []);
  assert.ok(await guest.page.locator('[data-action="login"]').count(), 'login banner');
  assert.ok(await guest.page.locator('.hero-main').count(), 'hero');
});
await guest.page.mouse.move(5, 890);
await shotFull(guest.page, 'home-guest');
await check('H12 personal reports work without a platform login', async () => {
  await guest.page.locator('.home-tabs [data-tab="report"]').click();
  await reportReady(guest.page);
  assert.equal(await guest.page.locator('.home-report .kpi-card').count(), 4);
  assert.deepEqual(guest.errors, []);
});
await guest.page.close();

const narrow = await openHome({ loggedIn: true, width: 1000, height: 800 });
await check('H05 narrow window has no horizontal overflow', async () => {
  const over = await narrow.page.evaluate(() => {
    const el = document.querySelector('.page-home');
    return el.scrollWidth - el.clientWidth;
  });
  assert.ok(over <= 1, `overflow ${over}px`);
});
await narrow.page.mouse.move(5, 790);
await shotFull(narrow.page, 'home-narrow');
await narrow.page.close();

const compact = await openHome({ loggedIn: true, width: 700, height: 860 });
await check('H13 compact home and every report tab fit without horizontal overflow', async () => {
  const p = compact.page;
  const overflow = () => p.locator('.page-home').evaluate((el) => el.scrollWidth - el.clientWidth);
  assert.ok(await overflow() <= 1, 'live home fits');
  await p.locator('.home-tabs [data-tab="report"]').click();
  for (const tab of ['overview', 'ranking', 'time', 'history', 'data']) {
    await p.locator(`.home-report [role="tab"][data-tab="${tab}"]`).click();
    await reportReady(p);
    assert.ok(await overflow() <= 1, `${tab} overflow ${await overflow()}`);
  }
  assert.deepEqual(compact.errors, []);
});
await go(compact.page, 'stats', { tab: 'overview' });
await reportReady(compact.page);
await shotFull(compact.page, 'home-report-narrow');
await compact.page.close();

const light = await openHome({ loggedIn: true, scheme: 'light' });
await light.page.evaluate(() => document.documentElement.setAttribute('data-theme', 'light'));
await light.page.mouse.move(5, 890);
await light.page.waitForTimeout(300);
await shotFull(light.page, 'home-light');
await light.page.locator('.home-tabs [data-tab="report"]').click();
await reportReady(light.page);
await shotFull(light.page, 'home-report-light');
await check('H14 first-run and failed previews have recoverable states', async () => {
  const p = light.page;
  await p.evaluate(() => { window.__savedReport = window.__mock.handlers.insights_report; window.__mock.handlers.insights_report = () => ({ total: 0, firstWatchedAt: null }); });
  await p.locator('.home-header [data-action="refresh"]').click();
  await p.locator('.home-report .stats-body .empty').waitFor();
  assert.match(await p.locator('.home-report .stats-body').textContent(), /아직 시청 기록이 없습니다/);
  await go(p, 'home', { view: 'live', refresh: true });
  await p.waitForSelector('[data-home-recap][aria-busy="false"]');
  assert.match(await p.locator('[data-home-recap]').textContent(), /시청 취향이 여기에 쌓여요/);
  await p.evaluate(() => { window.__mock.handlers.insights_report = () => { throw new Error('offline'); }; });
  await p.locator('.home-header [data-action="refresh"]').click();
  await p.locator('[data-recap-retry]').waitFor();
  await p.evaluate(() => { window.__mock.handlers.insights_report = window.__savedReport; });
  await p.locator('[data-recap-retry]').click();
  await p.locator('.home-recap-metric').first().waitFor();
  assert.deepEqual(light.errors, []);
});
await check('H15 live retries and delayed thumbnail fills bypass the view cache', async () => {
  const p = light.page;
  await p.evaluate(() => {
    window.__savedPopular = window.__mock.handlers.popular_lives;
    window.__mock.handlers.popular_lives = () => { throw new Error('offline'); };
  });
  await p.locator('.home-header [data-action="refresh"]').click();
  await p.locator('.home-live-view .empty [data-retry]').waitFor();
  await p.evaluate(() => { window.__mock.handlers.popular_lives = window.__savedPopular; });
  await p.locator('.home-live-view .empty [data-retry]').click();
  await p.locator('.popular-grid .card').first().waitFor();
  await p.evaluate(() => {
    const saved = window.__mock.handlers.following_lives;
    window.__thumbCalls = 0;
    window.__mock.handlers.following_lives = () => {
      window.__thumbCalls++;
      return saved().map((live) => ({ ...live, thumbnailUrl: window.__thumbCalls === 1 ? '' : live.thumbnailUrl }));
    };
  });
  await p.locator('.home-header [data-action="refresh"]').click();
  await p.waitForFunction(() => window.__thumbCalls >= 2, null, { timeout: 10000 });
  assert.deepEqual(light.errors, []);
});
await light.page.close();

await browser.close();
server.close();
let failed = 0;
for (const r of results) {
  console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : ` — ${r.error}`}`);
  if (!r.ok) failed++;
}
process.exit(failed ? 1 : 0);
