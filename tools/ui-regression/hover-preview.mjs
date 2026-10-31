// 썸네일 호버 미리보기 회귀 검사 + 스크린샷 — 모의 IPC 로 홈·팔로잉·인기·카테고리·보관함을 채우고,
// preview_source 는 이 서버가 내주는 짧은 WebM(ffmpeg 로 만든 시험 화면)을 돌려준다. 실제 서비스는 건드리지 않는다.
// 저장소 루트에서: node tools/ui-regression/hover-preview.mjs   (CVIEW_UI_SHOTS=<폴더> 면 PNG 를 남긴다)
// ffmpeg 가 필요하다(시험 영상 만들기).
import http from 'node:http';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');

const ROOT = path.resolve('app-ui');
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.png': 'image/png', '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.json': 'application/json' };
const PALETTE = [['#ff8a5b', '#7b2ff7'], ['#00c2ff', '#00ffa3'], ['#ff5f9e', '#ffb86b'], ['#6a5bff', '#00d4ff'], ['#ffd166', '#ef476f'], ['#06d6a0', '#118ab2']];

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'cview-preview-'));
const VIDEO = path.join(tmp, 'preview.webm');
execFileSync('ffmpeg', ['-loglevel', 'error', '-f', 'lavfi', '-i', 'testsrc2=size=640x360:rate=24:duration=6', '-c:v', 'libvpx', '-b:v', '600k', '-an', VIDEO]);

function fakeImage(kind, n) {
  const [a, b] = PALETTE[n % PALETTE.length];
  if (kind === 'avatar') {
    return `<svg xmlns="http://www.w3.org/2000/svg" width="120" height="120"><rect width="120" height="120" fill="${a}"/><circle cx="60" cy="48" r="22" fill="#fff" opacity=".85"/></svg>`;
  }
  return `<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></linearGradient></defs><rect width="1280" height="720" fill="url(#g)"/><circle cx="${300 + (n * 137) % 700}" cy="360" r="160" fill="#fff" opacity=".16"/></svg>`;
}

const server = http.createServer((req, res) => {
  const url = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  const img = url.match(/^\/__img\/(thumb|avatar)\/(\d+)\.svg$/);
  if (img) { res.writeHead(200, { 'content-type': 'image/svg+xml' }); res.end(fakeImage(img[1], Number(img[2]))); return; }
  if (url === '/__video/preview.webm') {
    // 시작 위치 탐색(Range)도 받는다.
    const size = fs.statSync(VIDEO).size;
    const m = /bytes=(\d+)-(\d*)/.exec(req.headers.range || '');
    if (m) {
      const start = Number(m[1]);
      const end = m[2] ? Number(m[2]) : size - 1;
      res.writeHead(206, { 'content-type': 'video/webm', 'content-range': `bytes ${start}-${end}/${size}`, 'accept-ranges': 'bytes', 'content-length': end - start + 1 });
      fs.createReadStream(VIDEO, { start, end }).pipe(res);
    } else {
      res.writeHead(200, { 'content-type': 'video/webm', 'accept-ranges': 'bytes', 'content-length': size });
      fs.createReadStream(VIDEO).pipe(res);
    }
    return;
  }
  const file = path.join(ROOT, url === '/' ? 'index.html' : url);
  if (!file.startsWith(ROOT) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) { res.writeHead(404); res.end(); return; }
  res.writeHead(200, { 'content-type': TYPES[path.extname(file)] || 'application/octet-stream' });
  fs.createReadStream(file).pipe(res);
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${server.address().port}/`;

function mockTauri([origin, previewOn]) {
  const img = (kind, n) => `${origin}__img/${kind}/${n}.svg`;
  const names = ['릴카', '플러리', '따효니', '괴물쥐', '침착맨', '녹두로', '한동숙', '풍월량', '김도', '소풍왔니', '마라맛', '서새봄'];
  const cats = ['마인크래프트', '메이플스토리', '배틀그라운드', '리그 오브 레전드', '저스트 채팅'];
  const titles = ['시청자 참여 내전 + 경품', '신작 첫 플레이! 스포 금지', '혼돈의 서버 생존기', '대회 리뷰 & 분석', '저녁 먹고 수다 한 판'];
  const now = Date.now();
  const live = (i, viewers) => ({
    channelId: `ch${i}`, channelName: names[i % names.length], channelImageUrl: img('avatar', i),
    liveTitle: titles[i % titles.length], thumbnailUrl: img('thumb', i), viewerCount: viewers, viewerCountText: viewers.toLocaleString('ko-KR'),
    categoryName: cats[(i * 3) % cats.length], openDate: new Date(now - (20 + i * 17) * 60000).toISOString(), isLive: true, adult: i === 7,
  });
  const following = [56012, 39120, 37011, 22030, 8210, 6120].map((v, i) => live(i, v));
  const popular = Array.from({ length: 30 }, (_, i) => live(i, Math.round(98000 / (1 + i * 0.35))));
  const cont = Array.from({ length: 4 }, (_, i) => ({ videoNo: 100 + i, title: `${titles[i % titles.length]} 다시보기`, thumbnailUrl: img('thumb', 20 + i), duration: 7200, position: 1200 + i * 900, channelName: names[i + 2] }));
  const clips = Array.from({ length: 8 }, (_, i) => ({ clipUID: `c${i}`, clipTitle: `레전드 순간 모음 #${i + 1}`, thumbnailImageUrl: img('thumb', 30 + i), duration: 40 + i * 7, readCount: 120000 - i * 9000, channel: { channelName: names[(i + 4) % names.length] } }));
  const calls = [];
  const handlers = {
    auth_status: () => ({ loggedIn: true, state: 'loggedIn', nickname: '도도', profileImageUrl: img('avatar', 3) }),
    settings_get: () => ({ multiLive: { maxConcurrentSessions: 4, maxLiveChannels: 5 }, multiChat: {}, player: {}, chat: {}, general: {}, appearance: {} }),
    following_lives: () => following,
    following_channels_all: () => following.map((l) => ({ channelId: l.channelId, channelName: l.channelName, channelImageUrl: l.channelImageUrl, openLive: true, followerCountText: '1만' })),
    popular_lives: () => popular,
    recent_channels: () => [],
    vod_continue_list: () => cont,
    vod_watch_later_list: () => [],
    popular_clips: () => clips,
    following_vods: () => [],
    preview_source: ({ kind }) => ({ url: `${origin}__video/preview.webm`, format: 'mp4', startAt: kind === 'vod' ? 2 : 0, live: kind === 'live' }),
  };
  window.__mock = { handlers, calls };
  window.__TAURI__ = {
    core: { invoke: async (cmd, args) => { calls.push({ cmd, args }); const h = handlers[cmd]; if (!h) throw new Error(`mock: ${cmd}`); return h(args); } },
    event: { listen: async () => () => {} },
  };
  try {
    localStorage.setItem('cview.onboarded', '1');
    localStorage.setItem('cview.thumbPreview', previewOn ? '1' : '0');
  } catch { /* 무시 */ }
}

const SHOTS = process.env.CVIEW_UI_SHOTS;
const browser = await chromium.launch();
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}

async function openApp({ previewOn = true, scheme = 'dark' } = {}) {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, colorScheme: scheme });
  page.setDefaultTimeout(5000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, previewOn]);
  await page.goto(base);
  await page.waitForSelector('.page-home:not([hidden]) .home', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim').forEach((x) => x.remove()));
  await page.waitForSelector('[data-rail="clips"]:not([hidden])');
  await page.waitForTimeout(600);
  return { page, errors };
}

const go = (page, route, params = {}) => page.evaluate(([r, p]) => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: r, params: p } })), [route, params]);
const previewCalls = (page) => page.evaluate(() => window.__mock.calls.filter((c) => c.cmd === 'preview_source').map((c) => c.args));
const activeVideos = (page) => page.evaluate(() => [...document.querySelectorAll('video.thumb-preview')].map((v) => ({
  host: v.parentElement.dataset.preview, muted: v.muted, on: v.parentElement.classList.contains('preview-on'), src: v.getAttribute('src') || '', time: v.currentTime,
})));

async function hover(page, selector) {
  const el = page.locator(selector).first();
  await el.scrollIntoViewIfNeeded();
  await page.waitForTimeout(150); // 스크롤 정리(scroll → stop)가 끝난 뒤
  const box = await el.boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2, { steps: 4 });
}

async function waitPlaying(page) {
  await page.waitForFunction(() => document.querySelector('.preview-on > video.thumb-preview'), null, { timeout: 4000 });
  await page.waitForTimeout(450); // 페이드인 끝
}

async function shot(page, name, selector) {
  if (!SHOTS) return;
  fs.mkdirSync(SHOTS, { recursive: true });
  const el = selector ? page.locator(selector).first() : null;
  const clip = el ? await el.boundingBox() : null;
  const pad = 24;
  await page.screenshot({ path: path.join(SHOTS, `${name}.png`), clip: clip ? { x: Math.max(0, clip.x - pad), y: Math.max(0, clip.y - pad), width: clip.width + pad * 2, height: clip.height + pad * 2 } : undefined });
}

// ── 켬 ────────────────────────────────────────────────
const on = await openApp();
const p = on.page;

await check('P01 hovering a live card previews it muted after the delay', async () => {
  const sel = '.popular-grid .card .thumb[data-preview]';
  await hover(p, sel);
  await p.waitForTimeout(250);
  assert.equal((await activeVideos(p)).length, 0, 'started before the delay');
  await waitPlaying(p);
  const vids = await activeVideos(p);
  assert.equal(vids.length, 1);
  assert.ok(vids[0].muted, 'muted');
  assert.ok(vids[0].on, 'faded in');
  assert.match(vids[0].host, /^live:ch\d+$/);
  const callArgs = await previewCalls(p);
  assert.deepEqual(callArgs.at(-1), { kind: 'live', id: vids[0].host.slice(5) });
});
await shot(p, 'after-home-grid', '.popular-grid .card:has(.preview-on)');

await check('P02 moving to another thumbnail keeps only one preview', async () => {
  await hover(p, '.popular-grid .card:nth-child(2) .thumb[data-preview]');
  await waitPlaying(p);
  await p.waitForTimeout(300); // 앞 미리보기의 페이드아웃 정리
  const vids = await activeVideos(p);
  assert.equal(vids.length, 1, JSON.stringify(vids));
});

await check('P03 leaving the thumbnail removes the video and frees it', async () => {
  await p.mouse.move(5, 890);
  await p.waitForTimeout(350);
  assert.equal((await activeVideos(p)).length, 0);
  assert.equal(await p.locator('.preview-loading, .preview-on').count(), 0);
});

await check('P04 passing over quickly does not request a preview', async () => {
  const before = (await previewCalls(p)).length;
  const box = await p.locator('.popular-grid .card:nth-child(5) .thumb').boundingBox();
  await p.mouse.move(box.x + 20, box.y + 20);
  await p.waitForTimeout(200);
  await p.mouse.move(5, 890);
  await p.waitForTimeout(600);
  assert.equal((await previewCalls(p)).length, before);
});

await check('P05 top-10 rail and hero preview too; adult cards do not', async () => {
  assert.ok(await p.locator('.top-shelf .thumb[data-preview^="live:"]').count() > 0, 'top rail');
  assert.ok(await p.locator('.hero-media[data-preview^="live:"]').count() > 0, 'hero');
  assert.equal(await p.locator('[data-channel-id="ch7"] .thumb[data-preview]').count(), 0, 'adult card has preview');
  await hover(p, '.top-shelf .top-item:nth-child(1) .thumb[data-preview]');
  await waitPlaying(p);
});
await shot(p, 'after-home-top10', '.top-shelf .top-item:has(.preview-on)');
await p.mouse.move(5, 890);
await p.waitForTimeout(300);

await check('P06 continue-watching VOD previews from its start offset', async () => {
  await hover(p, '[data-rail="continue"] .thumb[data-preview^="vod:"], .home .thumb[data-preview^="vod:"]');
  await waitPlaying(p);
  const [v] = await activeVideos(p);
  assert.match(v.host, /^vod:/);
  assert.ok(v.time >= 1.9, `currentTime ${v.time}`);
});
await p.mouse.move(5, 890);
await p.waitForTimeout(300);

await check('P07 app motion setting "절약" turns previews off', async () => {
  await p.evaluate(() => { document.documentElement.dataset.motion = 'reduced'; });
  const before = (await previewCalls(p)).length;
  await hover(p, '.popular-grid .card:nth-child(3) .thumb[data-preview]');
  await p.waitForTimeout(900);
  assert.equal((await activeVideos(p)).length, 0);
  assert.equal((await previewCalls(p)).length, before);
  await p.evaluate(() => { document.documentElement.dataset.motion = 'balanced'; });
  await p.mouse.move(5, 890);
});

await check('P07b system reduced motion alone does not disable previews (Windows animation effects off)', async () => {
  await p.emulateMedia({ reducedMotion: 'reduce' });
  await hover(p, '.popular-grid .card:nth-child(6) .thumb[data-preview]');
  await waitPlaying(p);
  await p.emulateMedia({ reducedMotion: 'no-preference' });
  await p.mouse.move(5, 890);
  await p.waitForTimeout(300);
});

await check('P08 clicking stops the preview at once', async () => {
  await hover(p, '.popular-grid .card:nth-child(4) .thumb[data-preview]');
  await waitPlaying(p);
  await p.evaluate(() => document.querySelector('.preview-on').dispatchEvent(new PointerEvent('pointerdown', { bubbles: true })));
  await p.waitForTimeout(300);
  assert.equal((await activeVideos(p)).length, 0);
  await p.mouse.move(5, 890);
});

for (const [route, sel, kind] of [
  ['popular', '[data-page="popular"] .thumb[data-preview]', 'live'],
  ['following', '[data-page="following"] .thumb[data-preview]', 'live'],
  ['categories', '[data-page="categories"] .category-art[data-preview]', 'live'],
  ['library', '[data-page="library"] .thumb[data-preview]', 'clip'],
]) {
  await check(`P09 ${route} thumbnails preview (${kind})`, async () => {
    await go(p, route);
    await p.waitForSelector(sel, { timeout: 4000 });
    await hover(p, sel);
    await waitPlaying(p);
    const [v] = await activeVideos(p);
    assert.ok(v && v.host.startsWith(`${kind}:`), JSON.stringify(v));
  });
  await shot(p, `after-${route}`, `[data-page="${route}"] :is(.card, .category-card):has(.preview-on)`);
  await p.mouse.move(5, 890);
  await p.waitForTimeout(300);
}

await check('P10 leaving the screen cleans up', async () => {
  await go(p, 'popular');
  await p.waitForSelector('[data-page="popular"] .thumb[data-preview]');
  await hover(p, '[data-page="popular"] .thumb[data-preview]');
  await waitPlaying(p);
  await go(p, 'library');
  await p.waitForTimeout(1500);
  // 가려진 화면(인기)에 남은 미리보기가 없어야 한다(멈춘 포인터 아래 새 화면 카드가 미리보기를 시작하는 건 정상).
  const stale = await p.evaluate(() => document.querySelectorAll('[data-page][hidden] video.thumb-preview, [data-page][hidden] .preview-on').length);
  assert.equal(stale, 0);
});

await check('P11 no page errors', async () => assert.deepEqual(on.errors, []));
await p.close();

// ── 끔(= 이전 동작) — 설정에서 끄면 미리보기를 묻지도 않는다. 전후 비교 스크린샷의 "전" ──
const off = await openApp({ previewOn: false });
await check('P12 setting off never requests previews', async () => {
  await hover(off.page, '.popular-grid .card .thumb[data-preview]');
  await off.page.waitForTimeout(1200);
  assert.equal((await previewCalls(off.page)).length, 0);
  assert.equal((await activeVideos(off.page)).length, 0);
});
await shot(off.page, 'before-home-grid', '.popular-grid .card:hover');
await hover(off.page, '.top-shelf .top-item:nth-child(1) .thumb[data-preview]');
await off.page.waitForTimeout(500);
await shot(off.page, 'before-home-top10', '.top-shelf .top-item:nth-child(1)');
for (const [route, sel] of [['popular', '[data-page="popular"] .thumb[data-preview]'], ['following', '[data-page="following"] .thumb[data-preview]'], ['categories', '[data-page="categories"] .category-art[data-preview]'], ['library', '[data-page="library"] .thumb[data-preview]']]) {
  await go(off.page, route);
  await off.page.waitForSelector(sel, { timeout: 4000 }).catch(() => {});
  await hover(off.page, sel).catch(() => {});
  await off.page.waitForTimeout(500);
  await shot(off.page, `before-${route}`, `[data-page="${route}"] :is(.card, .category-card):hover`);
}
await off.page.close();

await browser.close();
server.close();
fs.rmSync(tmp, { recursive: true, force: true });
let failed = 0;
for (const r of results) {
  console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : ` — ${r.error}`}`);
  if (!r.ok) failed++;
}
process.exit(failed ? 1 : 0);
