// 모션 회귀 검사 — 애니메이션 리뷰(2026-10-30)에서 고친 것이 그대로인지 본다.
// 저장소 루트에서: node tools/ui-regression/motion.mjs   (Playwright 가 필요하다)
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

// 모의 IPC — 명령별 응답은 테스트가 window.__mock.handlers 로 바꾼다. 부른 명령은 window.__mock.calls 에 남는다.
function mockTauri() {
  const calls = [];
  const health = { available: true, storeBytes: 1000, freeBytes: 1e9, backups: [], tables: [{ label: '시청 기록', rows: 3 }], backupDir: '/tmp/b', storePath: '/tmp/s' };
  const handlers = {
    auth_status: () => ({ loggedIn: false, state: 'loggedOut' }),
    settings_get: () => ({ multiChat: {} }),
    data_health: () => health,
    data_cleanup: () => health,
    data_backup_now: () => health,
    insights_overview: () => ({ sessionCount: 0, recent: [], daily: [], channels: [], categories: [], hourly: Array(24).fill(0) }),
    popular_lives: () => Array.from({ length: 12 }, (_, i) => ({ channelId: `c${i}`, channelName: `이름-${i}`, liveTitle: `방송 ${i}`, isLive: true, viewerCount: 1000 - i * 10, openDate: new Date(Date.now() - i * 60000).toISOString(), liveCategoryValue: i % 2 ? '게임' : '토크' })),
    search: () => ({ lives: [], channels: [] }),
    search_videos: () => ({ items: [], hasMore: false }),
  };
  window.__mock = { calls, handlers };
  window.__TAURI__ = {
    core: {
      invoke: async (cmd, args) => {
        calls.push({ cmd, args });
        const h = handlers[cmd];
        if (!h) throw new Error(`mock: ${cmd}`);
        return h(args);
      },
    },
    event: { listen: async () => () => {} },
  };
  try { localStorage.setItem('cview.onboarded', '1'); } catch { /* 무시 */ }
}

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
page.setDefaultTimeout(5000);
const errors = [];
page.on('pageerror', (e) => errors.push(String(e)));
await page.addInitScript(mockTauri);
await page.goto(base);
await page.waitForSelector('[data-page]', { state: 'attached' });
await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));

const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}
const go = (route, params = {}) => page.evaluate(([r, p]) => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: r, params: p } })), [route, params]);
const anims = (sel) => page.$$eval(sel, (els) => els.map((e) => getComputedStyle(e).animationName));

// 처음 불러온 목록은 떠오르고, 정렬을 바꿔 다시 그린 목록은 떠오르지 않는다. 정렬 판은 옛 칸에서 미끄러진다.
await check('popular: 처음만 떠오르고 다시 그리면 그대로, 판은 미끄러짐', async () => {
  await go('popular');
  await page.waitForSelector('[data-page="popular"] .grid .card');
  assert.ok((await anims('[data-page="popular"] .page-body > .grid')).every((n) => n === 'content-in'), '처음 목록에 등장 모션이 없음');
  await page.waitForTimeout(400);
  const before = await page.$eval('[data-page="popular"] .seg-glider', (g) => g.style.translate);
  await page.click('[data-page="popular"] [data-sort="recent"]');
  assert.ok((await anims('[data-page="popular"] .page-body > .grid')).every((n) => n === 'none'), '다시 그린 목록이 또 떠오름');
  assert.ok((await anims('[data-page="popular"] .grid > .card')).every((n) => n === 'none'), '카드가 또 차례로 떠오름');
  const g = await page.$eval('[data-page="popular"] .seg-glider', (el) => ({ t: el.style.translate, running: el.getAnimations().some((a) => a.transitionProperty === 'translate') }));
  assert.notEqual(g.t, before, '판이 새 칸으로 가지 않음');
  assert.ok(g.running, '판이 미끄러지지 않고 뚝 옮겨짐');
});

// 모션 줄이기에서도 OSD 가 보인다(사라지는 애니메이션을 0 초로 돌리지 않는다).
await check('reduced: OSD 가 보인다', async () => {
  const r = await page.evaluate(() => {
    document.documentElement.dataset.motion = 'reduced';
    const o = document.createElement('div');
    o.className = 'player-osd fade';
    o.textContent = '볼륨 50%';
    document.body.append(o);
    const cs = getComputedStyle(o);
    const out = { name: cs.animationName, opacity: cs.opacity };
    o.remove();
    document.documentElement.dataset.motion = 'balanced';
    return out;
  });
  assert.equal(r.name, 'none');
  assert.equal(r.opacity, '1');
});

// 멀티라이브 자리 이동은 transform 만 키프레임에 넣는다(transform-origin 이 끼면 합성되지 않는다).
await check('FLIP: 키프레임은 transform 만', async () => {
  const props = await page.evaluate(async () => {
    const { playLayoutMotion } = await import('./js/ml-motion.js');
    const el = document.createElement('div');
    Object.assign(el.style, { position: 'fixed', left: '100px', top: '100px', width: '200px', height: '120px' });
    document.body.append(el);
    const pane = { el };
    playLayoutMotion(new Map([[0, pane]]), new Map([[0, { left: 0, top: 0, width: 100, height: 60 }]]));
    const kf = pane.anim.effect.getKeyframes();
    const origin = el.style.transformOrigin;
    pane.anim.finish();
    await pane.anim.finished;
    await new Promise((r) => setTimeout(r, 0));
    const after = el.style.transformOrigin;
    el.remove();
    return { keys: [...new Set(kf.flatMap((k) => Object.keys(k)).filter((k) => !['offset', 'easing', 'composite', 'computedOffset'].includes(k)))], origin, after };
  });
  assert.deepEqual(props.keys, ['transform']);
  assert.equal(props.origin, '0px 0px');
  assert.equal(props.after, '', '끝난 뒤 기준점을 되돌리지 않음');
});

// 썸네일 자리 밖 그림(채팅 이모티콘 등)에는 is-loaded 를 달지 않는다.
await check('image fade: 썸네일 자리만', async () => {
  const r = await page.evaluate(async () => {
    const png = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=';
    const box = document.createElement('div');
    box.innerHTML = `<span class="chat-line"><img src="${png}"></span><div class="thumb"><img src="${png}"></div>`;
    document.body.append(box);
    await Promise.all([...box.querySelectorAll('img')].map((i) => i.decode().catch(() => {})));
    await new Promise((res) => setTimeout(res, 50));
    const out = [...box.querySelectorAll('img')].map((i) => i.classList.contains('is-loaded'));
    box.remove();
    return out;
  });
  assert.deepEqual(r, [false, true]);
});

console.log(JSON.stringify({ results, pageErrors: errors }, null, 2));
await browser.close();
server.close();
const failed = results.filter((r) => !r.ok);
if (failed.length || errors.length) process.exit(1);
