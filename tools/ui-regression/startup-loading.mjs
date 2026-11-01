// 앱 시작 로딩 화면 회귀 검사 + 스크린샷 — 모의 IPC 응답을 일부러 늦춰, 로딩 화면이 홈 데이터·첫 화면 썸네일을
// 기다렸다가 걷히고 그 뒤에 채워진 홈이 보이는지 본다. 실제 서비스·데이터는 건드리지 않는다.
// 저장소 루트에서: node tools/ui-regression/startup-loading.mjs   (CVIEW_UI_SHOTS=<폴더> 면 PNG 를 남긴다)
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

function mockTauri([origin, loggedIn, delays]) {
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
    core: { invoke: async (cmd, args) => { window.__mock.calls.push(cmd); const h = handlers[cmd]; if (!h) throw new Error(`mock: ${cmd}`); const ms = (delays || {})[cmd]; if (ms) await new Promise((r) => setTimeout(r, ms)); return h(args); } },
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
async function shot(page, name) {
  if (!SHOTS) return;
  fs.mkdirSync(SHOTS, { recursive: true });
  await page.screenshot({ path: path.join(SHOTS, `${name}.png`) });
}

// 로딩 화면이 걷히기 시작하는 순간의 홈 상태를 기록한다(.splash 에 leaving 이 붙는 때).
function watchSplash() {
  const splash = document.getElementById('splash');
  window.__splash = { steps: [], leftAt: 0, homeAtLeave: null };
  if (!splash) return;
  new MutationObserver(() => {
    const t = splash.querySelector('.splash-step').textContent;
    if (window.__splash.steps[window.__splash.steps.length - 1] !== t) window.__splash.steps.push(t);
    if (splash.classList.contains('leaving') && !window.__splash.leftAt) {
      window.__splash.leftAt = performance.now();
      const body = document.querySelector('.page-home .page-body');
      window.__splash.homeAtLeave = {
        visible: !document.querySelector('.page-home').hidden,
        filled: Boolean(body && body.querySelector('.home .hero-main')),
        skeleton: Boolean(body && body.querySelector('.sk')),
      };
    }
  }).observe(splash, { attributes: true, childList: true, subtree: true, characterData: true });
}

async function start({ loggedIn = true, scheme = 'dark', delays = {}, width = 1280, height = 800 }) {
  const page = await browser.newPage({ viewport: { width, height }, colorScheme: scheme });
  page.setDefaultTimeout(15000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, loggedIn, delays]);
  await page.addInitScript((theme) => { try { localStorage.setItem('cview.theme', theme); localStorage.setItem('cview.welcomed.v1', '1'); } catch { /* 무시 */ } }, scheme);
  // 문서가 읽히자마자(앱 스크립트보다 먼저) 로딩 화면 지켜보기를 건다.
  await page.addInitScript({ content: `document.addEventListener('readystatechange', function once() { document.removeEventListener('readystatechange', once); (${watchSplash})(); });` });
  await page.goto(base);
  return { page, errors };
}

const SLOW_DATA = { following_lives: 1400, popular_lives: 1800, recent_channels: 400 };

// 1) 보통 — 데이터가 1.8초 걸리면 로딩 화면이 그동안 단계를 보여 주고, 걷힐 때 홈은 이미 채워져 있다.
for (const scheme of ['dark', 'light']) {
  const { page, errors } = await start({ scheme, delays: SLOW_DATA });
  await page.waitForTimeout(900);
  await check(`S01-${scheme} splash stays while home data loads`, async () => {
    assert.equal(await page.locator('#splash:not(.leaving)').count(), 1, 'splash visible');
    const step = await page.locator('#splash .splash-step').textContent();
    assert.match(step, /인기 라이브/, step);
    assert.doesNotMatch(step, /최근 시청/, `recent finished first: ${step}`);
  });
  await shot(page, `splash-loading-${scheme}`);
  await page.waitForSelector('#splash', { state: 'detached' });
  await check(`S02-${scheme} home is filled (not skeleton) when splash leaves`, async () => {
    const s = await page.evaluate(() => window.__splash);
    assert.ok(s.leftAt >= 1800, `left at ${Math.round(s.leftAt)}ms`);
    assert.deepEqual(s.homeAtLeave, { visible: true, filled: true, skeleton: false });
    assert.ok(s.steps.some((t) => /화면 그리는 중/.test(t)), s.steps.join(' / '));
    assert.deepEqual(errors, []);
  });
  await page.waitForTimeout(600);
  await shot(page, `home-after-${scheme}`);
  await page.close();
}

// 2) 비로그인 — 팔로잉은 묻지 않는다.
{
  const { page, errors } = await start({ loggedIn: false, delays: SLOW_DATA });
  await page.waitForTimeout(700);
  await check('S03 guest splash does not wait for following', async () => {
    const step = await page.locator('#splash .splash-step').textContent();
    assert.doesNotMatch(step, /팔로잉/, step);
  });
  await page.waitForSelector('#splash', { state: 'detached' });
  await check('S04 guest home filled after splash', async () => {
    const s = await page.evaluate(() => window.__splash);
    assert.equal(s.homeAtLeave.filled, true);
    assert.deepEqual(errors, []);
  });
  await page.close();
}

// 3) 느린 네트워크 — 3.5초가 지나면 안내가 바뀌고, 데이터가 오면 걷힌다.
{
  const { page } = await start({ delays: { popular_lives: 5200 } });
  await page.waitForTimeout(4000);
  await check('S05 slow network notice', async () => {
    assert.match(await page.locator('#splash .splash-step').textContent(), /네트워크가 느려요/);
  });
  await shot(page, 'splash-slow-dark');
  await page.waitForSelector('#splash', { state: 'detached' });
  await check('S06 slow network still lands on filled home', async () => {
    assert.equal((await page.evaluate(() => window.__splash)).homeAtLeave.filled, true);
  });
  await page.close();
}

// 4) 응답이 끝내 오지 않으면 8초에서 포기하고 홈(스켈레톤)을 보여 준다 — 로딩 화면에 갇히지 않는다.
{
  const { page } = await start({ delays: { popular_lives: 60000 } });
  await page.waitForSelector('#splash', { state: 'detached', timeout: 12000 });
  await check('S07 splash gives up after the cap', async () => {
    const s = await page.evaluate(() => window.__splash);
    assert.ok(s.leftAt >= 7900 && s.leftAt < 10000, `left at ${Math.round(s.leftAt)}ms`);
    assert.equal(s.homeAtLeave.visible, true);
  });
  await page.close();
}

// 5) 보조 창(?w=stats)은 홈을 기다리지 않는다.
{
  const page = await browser.newPage({ viewport: { width: 900, height: 700 } });
  await page.addInitScript(mockTauri, [base, true, { popular_lives: 60000, following_lives: 60000 }]);
  await page.goto(`${base}?w=stats`);
  await check('S08 aux window skips home loading', async () => {
    await page.waitForSelector('#splash', { state: 'detached', timeout: 3000 });
  });
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
