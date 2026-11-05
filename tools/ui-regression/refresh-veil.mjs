// 직접 새로고침 덮개(refresh-veil.js) 회귀 검사 + 스크린샷 — 모의 IPC 응답을 일부러 늦춰, 메뉴마다 ⌘R(메뉴 동작)·
// 머리줄 새로고침 버튼으로 새로고침할 때 로고 덮개가 뜨고 본문이 흐려졌다가 끝나면 걷히는지 본다. 빨리 끝나는
// 새로고침·자동 갱신(네트워크 복구)에는 뜨지 않고, 다른 화면으로 옮기거나 실패해도 남지 않는지도 본다.
// 저장소 루트에서: node tools/ui-regression/refresh-veil.mjs   (CVIEW_UI_SHOTS=<폴더> 면 PNG 를 남긴다)
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');

const ROOT = path.resolve('app-ui');
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.png': 'image/png', '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.json': 'application/json' };
const PALETTE = [['#ff8a5b', '#7b2ff7'], ['#00c2ff', '#00ffa3'], ['#ff5f9e', '#ffb86b'], ['#6a5bff', '#00d4ff'], ['#ffd166', '#ef476f'], ['#06d6a0', '#118ab2']];

function fakeImage(kind, n) {
  const [a, b] = PALETTE[n % PALETTE.length];
  const size = kind === 'avatar' ? 'width="120" height="120"' : 'width="1280" height="720"';
  return `<svg xmlns="http://www.w3.org/2000/svg" ${size}><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></linearGradient></defs><rect width="100%" height="100%" fill="url(#g)"/><circle cx="30%" cy="40%" r="18%" fill="#fff" opacity=".2"/></svg>`;
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

// 응답 지연은 window.__delay[명령] 을 그때그때 읽는다 — 첫 진입은 빠르게, 새로고침만 느리게.
function mockTauri(origin) {
  const img = (kind, n) => `${origin}__img/${kind}/${n}.svg`;
  const names = ['릴카', '플러리', '따효니', '괴물쥐', '침착맨', '녹두로', '한동숙', '풍월량', '김도', '소풍왔니', '마라맛', '서새봄'];
  const cats = ['마인크래프트', '메이플스토리', '배틀그라운드', '리그 오브 레전드', '저스트 채팅'];
  const now = Date.now();
  const live = (i, viewers) => ({
    channelId: `ch${i}`, channelName: names[i % names.length], channelImageUrl: img('avatar', i), liveTitle: `방송 제목 ${i + 1}`,
    thumbnailUrl: img('thumb', i), viewerCount: viewers, viewerCountText: viewers.toLocaleString('ko-KR'),
    categoryName: cats[i % cats.length], openDate: new Date(now - (20 + i * 17) * 60000).toISOString(), isLive: true,
  });
  const following = [56012, 39120, 37011, 22030, 8210, 6120].map((v, i) => live(i, v));
  const popular = Array.from({ length: 36 }, (_, i) => live(i, Math.round(98000 / (1 + i * 0.35))));
  const clips = Array.from({ length: 8 }, (_, i) => ({ clipUID: `c${i}`, clipTitle: `클립 ${i + 1}`, thumbnailImageUrl: img('thumb', 20 + i), duration: 40, readCount: 1000 * (i + 1), channel: { channelName: names[i] } }));
  const handlers = {
    auth_status: () => ({ loggedIn: true, state: 'loggedIn', nickname: '도도', profileImageUrl: img('avatar', 3) }),
    settings_get: () => ({ multiLive: { maxConcurrentSessions: 4, maxLiveChannels: 5 }, multiChat: {}, player: {}, chat: {}, general: {} }),
    following_lives: () => following,
    following_channels_all: () => following.map((l) => ({ ...l, isLive: true })),
    popular_lives: () => popular,
    recent_channels: () => [],
    vod_continue_list: () => [],
    vod_watch_later_list: () => [],
    following_vods: () => [],
    favorites_list: () => [],
    bookmarks_list: () => [],
    download_list: () => ({ folder: '/tmp/dl', jobs: [], history: [] }),
    popular_clips: () => clips,
    search: () => ({ lives: popular.slice(0, 4), channels: [] }),
    search_videos: () => ({ items: [], hasMore: false }),
    insights_overview: () => ({ sessionCount: 0, daily: [] }),
    insights_report: () => ({ total: 0, firstWatchedAt: null }),
  };
  window.__delay = {};
  window.__listeners = {};
  window.__TAURI__ = {
    core: {
      invoke: async (cmd, args) => {
        const ms = window.__delay[cmd];
        if (ms) await new Promise((r) => setTimeout(r, ms));
        const h = handlers[cmd];
        if (!h) throw new Error(`mock: ${cmd}`);
        return h(args);
      },
    },
    event: { listen: async (name, cb) => { (window.__listeners[name] ||= []).push(cb); return () => {}; } },
  };
  window.__emit = (name, payload) => { for (const cb of window.__listeners[name] || []) cb({ payload }); };
  // 덮개가 한 번이라도 보였는지 기록한다(빨리 끝난 새로고침에 번쩍였는지 보려고).
  window.__veilSeen = 0;
  new MutationObserver(() => {
    const v = document.querySelector('.refresh-veil');
    if (v && !v.hidden) window.__veilSeen += 1;
  }).observe(document, { subtree: true, childList: true, attributes: true, attributeFilter: ['hidden'] });
  try { localStorage.setItem('cview.onboarded', '1'); localStorage.setItem('cview.welcomed.v1', '1'); } catch { /* 무시 */ }
}

const SHOTS = process.env.CVIEW_UI_SHOTS;
const browser = await chromium.launch();
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}
async function shot(page, name) {
  if (!SHOTS) return;
  fs.mkdirSync(SHOTS, { recursive: true });
  await page.screenshot({ path: path.join(SHOTS, `${name}.png`) });
}

async function start({ scheme = 'dark', motion } = {}) {
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 }, colorScheme: scheme });
  page.setDefaultTimeout(15000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, base);
  await page.addInitScript((theme) => { try { localStorage.setItem('cview.theme', theme); } catch { /* 무시 */ } }, scheme);
  await page.goto(base);
  await page.waitForSelector('#splash', { state: 'detached' });
  if (motion) await page.evaluate((m) => { document.documentElement.dataset.motion = m; }, motion);
  return { page, errors };
}

const go = (page, route, params = {}) => page.evaluate(([r, p]) => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: r, params: p } })), [route, params]);
const menuRefresh = (page) => page.evaluate(() => window.__emit('menu://action', { id: 'refresh' }));
const setDelay = (page, delay) => page.evaluate((d) => { window.__delay = d; window.__veilSeen = 0; }, delay);
const veilState = (page, route) => page.evaluate((r) => {
  const v = document.querySelector('.refresh-veil');
  const root = document.querySelector(`[data-page="${r}"]`) || document.querySelector(`.page-${r}`);
  const body = root && root.querySelector('.page-body');
  return {
    shown: Boolean(v && !v.hidden),
    blurred: Boolean(body && body.classList.contains('is-refreshing')),
    filter: body ? getComputedStyle(body).filter : '',
    anyBlurred: document.querySelectorAll('.is-refreshing').length,
    skeleton: Boolean(body && body.querySelector('.sk, .empty.loading')),
    skWhat: body ? [...body.querySelectorAll('.sk, .empty.loading')].slice(0, 3).map((n) => n.className + ' < ' + n.parentElement.className) : [],
    busy: root ? root.getAttribute('aria-busy') : null,
  };
}, route);

// 메뉴마다: [화면, 경로 매개변수, 늦출 명령, 지연(ms)] — 설정은 settings_get 을 두 번 차례로 부른다
const MENUS = [
  ['home', {}, ['following_lives', 'popular_lives']],
  ['following', {}, ['following_lives']],
  ['popular', {}, ['popular_lives']],
  ['categories', {}, ['popular_lives']],
  ['live', {}, ['following_lives', 'popular_lives']],
  ['library', { tab: 'clips' }, ['popular_clips']],
  ['library', { tab: 'overview' }, ['popular_clips', 'bookmarks_list']],
  ['stats', {}, ['insights_report']],
  ['search', { query: '릴카' }, ['search', 'search_videos']],
  ['settings', {}, ['settings_get'], 600],
];

for (const scheme of ['dark', 'light']) {
  const { page, errors } = await start({ scheme });
  for (const [route, params, slow, ms = 1200] of MENUS) {
    await setDelay(page, {}); // 첫 진입은 빠르게 — 앞 메뉴의 지연이 남지 않게
    await go(page, route, params);
    await page.waitForTimeout(700);
    await setDelay(page, Object.fromEntries(slow.map((c) => [c, ms])));
    await menuRefresh(page);
    await page.waitForTimeout(450);
    const during = await veilState(page, route);
    if (route === 'popular' || route === 'home') await shot(page, `refresh-${route}-${scheme}`);
    await page.waitForTimeout(1500);
    const after = await veilState(page, route);
    await check(`R-${scheme}-${route}${params.tab ? `:${params.tab}` : ''} veil while refreshing, gone after`, async () => {
      assert.equal(during.shown, true, 'veil visible during refresh');
      assert.equal(during.blurred, true, 'body blurred during refresh');
      assert.match(during.filter, /blur\(6px\)/, during.filter);
      assert.equal(during.skeleton, false, `old content kept under the veil (no skeleton) ${JSON.stringify(during.skWhat)}`);
      assert.equal(during.busy, 'true');
      assert.deepEqual({ shown: after.shown, any: after.anyBlurred, busy: after.busy }, { shown: false, any: 0, busy: null });
    });
  }
  await check(`R-${scheme} no page errors`, async () => assert.deepEqual(errors, []));
  await page.close();
}

{
  const { page, errors } = await start();
  // 빨리 끝나는 새로고침 — 덮개를 아예 띄우지 않는다.
  await go(page, 'popular');
  await page.waitForTimeout(600);
  await setDelay(page, {});
  await menuRefresh(page);
  await page.waitForTimeout(700);
  await check('R10 fast refresh never flashes the veil', async () => {
    assert.equal(await page.evaluate(() => window.__veilSeen), 0);
    assert.equal((await veilState(page, 'popular')).anyBlurred, 0);
  });

  // 머리줄 새로고침 버튼도 같은 덮개.
  await setDelay(page, { popular_lives: 1000 });
  await page.click('[data-page="popular"] [data-action="refresh"]');
  await page.waitForTimeout(400);
  const btn = await veilState(page, 'popular');
  await check('R11 header refresh button shows the veil', async () => assert.equal(btn.shown && btn.blurred, true));
  await page.waitForTimeout(1400);

  // 새로고침 도중 다른 화면으로 — 바로 걷힌다.
  await setDelay(page, { popular_lives: 3000 });
  await menuRefresh(page);
  await page.waitForTimeout(400);
  await go(page, 'following');
  await page.waitForTimeout(50);
  await check('R12 navigating away drops the veil at once', async () => {
    const s = await veilState(page, 'popular');
    assert.deepEqual({ shown: s.shown, any: s.anyBlurred, busy: s.busy }, { shown: false, any: 0, busy: null });
  });
  await page.waitForTimeout(3200);
  await check('R13 late response after leaving does not bring it back', async () => {
    assert.equal((await veilState(page, 'following')).shown, false);
  });

  // 실패해도 걷힌다.
  await go(page, 'popular');
  await page.waitForTimeout(500);
  await page.evaluate(() => { window.__delay = { popular_lives: 800 }; window.__failPopular = true; });
  await page.evaluate(() => {
    const inv = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async (cmd, args) => {
      if (cmd === 'popular_lives' && window.__failPopular) { await new Promise((r) => setTimeout(r, 800)); throw new Error('네트워크 오류'); }
      return inv(cmd, args);
    };
  });
  await menuRefresh(page);
  await page.waitForTimeout(400);
  const failing = await veilState(page, 'popular');
  await page.waitForTimeout(1000);
  await check('R14 failed refresh still clears the veil', async () => {
    assert.equal(failing.shown, true);
    const s = await veilState(page, 'popular');
    assert.deepEqual({ shown: s.shown, any: s.anyBlurred }, { shown: false, any: 0 });
    assert.equal(await page.locator('[data-page="popular"] .page-body .empty').count() > 0, true, 'error state shown');
  });
  await page.evaluate(() => { window.__failPopular = false; });

  // 자동 갱신(네트워크 복구)은 덮개 없이.
  await setDelay(page, { popular_lives: 1000 });
  await page.evaluate(() => window.dispatchEvent(new Event('online')));
  await page.waitForTimeout(1500);
  await check('R15 automatic reload (back online) shows no veil', async () => {
    assert.equal(await page.evaluate(() => window.__veilSeen), 0);
  });
  await check('R16 no page errors', async () => assert.deepEqual(errors, []));
  await page.close();
}

// 움직임 줄이기 — 로고가 멈춰 있다(덮개·흐림은 그대로).
{
  const { page } = await start({ motion: 'reduced' });
  await go(page, 'popular');
  await page.waitForTimeout(600);
  await setDelay(page, { popular_lives: 1000 });
  await menuRefresh(page);
  await page.waitForTimeout(400);
  await check('R17 reduced motion — static logo', async () => {
    const s = await veilState(page, 'popular');
    assert.equal(s.shown && s.blurred, true);
    const anims = await page.evaluate(() => [...document.querySelectorAll('.rv-card, .rv-logo *')].map((n) => getComputedStyle(n).animationName).filter((a) => a !== 'none'));
    assert.deepEqual(anims, []);
  });
  await shot(page, 'refresh-reduced-dark');
  await page.close();
}

await browser.close();
server.close();
let failed = 0;
for (const r of results) {
  console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : ` — ${r.error}`}`);
  if (!r.ok) failed++;
}
process.exit(failed ? 1 : 0);
