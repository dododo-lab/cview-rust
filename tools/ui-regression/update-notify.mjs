// 새 버전 알림 회귀 검사 — 자동 확인 시점·사이드바 "새 버전" 줄·토스트·버전당 한 번·설정 끄기(update-notify.js).
// app-ui 를 그대로 띄우고 window.__TAURI__ 를 모의 IPC 로 바꿔 실제 화면 코드를 돌린다(실제 서비스·데이터는 건드리지 않음).
// 저장소 루트에서: node tools/ui-regression/update-notify.mjs   (Playwright 가 필요하다)
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

function mockTauri() {
  const calls = [];
  const handlers = {
    auth_status: () => ({ loggedIn: false, state: 'loggedOut' }),
    settings_get: () => ({ chat: { autoScroll: true }, general: { autoCheckUpdates: window.__autoCheck !== false, notificationsEnabled: true } }),
    notify_send: () => null,
    settings_update: () => null,
    popular_lives: () => [],
    app_info: () => ({ version: '0.1.40', build: '40', os: 'macos', arch: 'aarch64', dataDir: '/tmp' }),
    update_check: () => ({ current: '0.1.40', latest: '0.1.41', newer: true, url: 'https://example.invalid/r', downloadUrl: null, notes: '' }),
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


const SHOTS = process.env.SHOTS || null; // 스크린샷 폴더(선택)
const browser = await chromium.launch();
let failed = 0;
async function check(name, fn) {
  try { await fn(); console.log(`PASS ${name}`); } catch (e) { failed++; console.log(`FAIL ${name}: ${e.message}`); }
}

async function open(setup = '') {
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  page.setDefaultTimeout(5000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.clock.install();
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
  await page.addInitScript(mockTauri);
  if (setup) await page.addInitScript(setup);
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
  const checks = () => page.evaluate(() => window.__mock.calls.filter((c) => c.cmd === 'update_check').length);
  return { page, errors, checks };
}

await check('실행 직후엔 확인하지 않고 15초 뒤 조용히 확인 → 사이드바 줄·토스트', async () => {
  const { page, errors, checks } = await open();
  await page.clock.fastForward(5000);
  assert.equal(await checks(), 0, '5초 안에 확인하면 안 됨');
  await page.clock.fastForward(11000);
  await page.waitForSelector('#update-pill:not([hidden]) .update-pill-main');
  assert.equal(await checks(), 1);
  assert.ok((await page.locator('#update-pill').innerText()).includes('새 버전 0.1.41'));
  assert.ok((await page.locator('.toast-stack').innerText()).includes('0.1.41'));
  // 토스트는 창 하단 중앙 — 동작 단추가 있는 토스트가 오른쪽으로 밀리던 문제 회귀 방지
  const pos = await page.evaluate(() => { const r = document.querySelector('.toast-stack .toast').getBoundingClientRect(); return { mid: r.left + r.width / 2, gap: innerHeight - r.bottom, w: innerWidth }; });
  assert.ok(Math.abs(pos.mid - pos.w / 2) <= 2, `토스트 가로 중앙 아님: ${JSON.stringify(pos)}`);
  assert.ok(pos.gap >= 8 && pos.gap <= 48, `토스트가 하단에 있지 않음: ${JSON.stringify(pos)}`);
  if (SHOTS) {
    await page.locator('.sidebar').screenshot({ path: `${SHOTS}/sidebar-expanded.png` });
    await page.screenshot({ path: `${SHOTS}/window.png` });
  }
  // 줄을 누르면 업데이트 확인 시트
  await page.locator('#update-pill .update-pill-main').click();
  await page.waitForSelector('.sheet-scrim.app-sheet .update-sheet');
  await page.keyboard.press('Escape');
  // 6시간 뒤 다시 확인
  await page.clock.fastForward(6 * 60 * 60 * 1000 + 1000);
  await page.waitForFunction(() => window.__mock.calls.filter((c) => c.cmd === 'update_check').length >= 3);
  assert.deepEqual(errors, []);
  if (SHOTS) {
    await page.evaluate(() => document.querySelector('.sidebar-toggle')?.click());
    await page.clock.fastForward(1000);
    await page.locator('.sidebar').screenshot({ path: `${SHOTS}/sidebar-collapsed.png` });
  }
  await page.close();
});

await check('같은 버전은 다시 열어도 토스트를 또 띄우지 않고, ✕ 로 줄을 숨기면 그 버전 동안 숨김', async () => {
  const { page } = await open(() => { localStorage.setItem('cview.updateNotified', '0.1.41'); });
  await page.clock.fastForward(16000);
  await page.waitForSelector('#update-pill:not([hidden])');
  assert.equal(await page.locator('.toast-stack .toast').count(), 0);
  await page.locator('#update-pill .update-pill-close').click();
  assert.equal(await page.locator('#update-pill').isHidden(), true);
  assert.equal(await page.evaluate(() => localStorage.getItem('cview.updatePillHidden')), '0.1.41');
  await page.close();
});

await check('최신 버전이면 줄도 토스트도 없음', async () => {
  const { page, checks } = await open(() => { window.__latest = true; });
  await page.evaluate(() => { window.__mock.handlers.update_check = () => ({ current: '0.1.41', latest: '0.1.41', newer: false }); });
  await page.clock.fastForward(16000);
  await page.waitForFunction(() => window.__mock.calls.some((c) => c.cmd === 'update_check'));
  assert.equal(await checks(), 1);
  assert.equal(await page.locator('#update-pill').isHidden(), true);
  assert.equal(await page.locator('.toast-stack .toast').count(), 0);
  await page.close();
});

await check('설정에서 자동 확인을 끄면 서버에 묻지 않음', async () => {
  const { page, checks } = await open(() => { window.__autoCheck = false; });
  await page.clock.fastForward(16000);
  await page.evaluate(() => new Promise((r) => setTimeout(r, 0)));
  assert.equal(await checks(), 0);
  assert.equal(await page.locator('#update-pill').isHidden(), true);
  await page.close();
});

await check('확인 실패는 조용히 넘어가고 30분 뒤 다시 시도', async () => {
  const { page, checks } = await open();
  await page.evaluate(() => { window.__mock.handlers.update_check = () => { throw new Error('오프라인'); }; });
  await page.clock.fastForward(16000);
  await page.waitForFunction(() => window.__mock.calls.some((c) => c.cmd === 'update_check'));
  assert.equal(await page.locator('.toast-stack .toast').count(), 0);
  await page.clock.fastForward(30 * 60 * 1000 + 1000);
  await page.waitForFunction(() => window.__mock.calls.filter((c) => c.cmd === 'update_check').length >= 2);
  assert.ok((await checks()) >= 2);
  await page.close();
});

await browser.close();
server.close();
console.log(failed ? `${failed} failed` : 'all passed');
process.exit(failed ? 1 : 0);
