// 홈 화면 UI 회귀 검사 + 스크린샷 — 모의 IPC 로 팔로잉·인기·최근·이어 보기·클립을 채워 홈을 그린다.
// 실제 서비스·데이터는 건드리지 않는다. 썸네일·아바타는 이 서버가 그때그때 만드는 SVG 다.
// 저장소 루트에서: node tools/ui-regression/home.mjs   (CVIEW_UI_SHOTS=<폴더> 면 PNG 를 남긴다)
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
  const handlers = {
    auth_status: () => (loggedIn ? { loggedIn: true, state: 'loggedIn', nickname: '도도', profileImageUrl: img('avatar', 3) } : { loggedIn: false, state: 'loggedOut' }),
    settings_get: () => ({ multiLive: { maxConcurrentSessions: 4, maxLiveChannels: 5 }, multiChat: {}, player: {}, chat: {}, general: {} }),
    following_lives: () => following,
    popular_lives: () => popular,
    recent_channels: () => recent,
    vod_continue_list: () => cont,
    popular_clips: () => clips,
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
  assert.equal(title.trim(), '신작 첫 플레이! 스포 금지');
});
await login.page.locator('[data-hero-index]').first().click();
await login.page.mouse.move(5, 890);
await shotFull(login.page, 'home-login');
await login.page.close();

const guest = await openHome({ loggedIn: false });
await check('H04 guest home shows login banner and popular hero', async () => {
  assert.deepEqual(guest.errors, []);
  assert.ok(await guest.page.locator('[data-action="login"]').count(), 'login banner');
  assert.ok(await guest.page.locator('.hero-main').count(), 'hero');
});
await guest.page.mouse.move(5, 890);
await shotFull(guest.page, 'home-guest');
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

const light = await openHome({ loggedIn: true, scheme: 'light' });
await light.page.evaluate(() => document.documentElement.setAttribute('data-theme', 'light'));
await light.page.mouse.move(5, 890);
await light.page.waitForTimeout(300);
await shotFull(light.page, 'home-light');
await light.page.close();

await browser.close();
server.close();
let failed = 0;
for (const r of results) {
  console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : ` — ${r.error}`}`);
  if (!r.ok) failed++;
}
process.exit(failed ? 1 : 0);
