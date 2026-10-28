// 앱 메뉴 시트 회귀 검사 — 단축키 도움말·앱 정보·업데이트 확인 시트와 채팅 자동 스크롤 메뉴(app-sheets.js).
// app-ui 를 그대로 띄우고 window.__TAURI__ 를 모의 IPC 로 바꿔 실제 화면 코드를 돌린다(실제 서비스·데이터는 건드리지 않음).
// 저장소 루트에서: node tools/ui-regression/app-sheets.mjs   (Playwright 가 필요하다)
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
    settings_get: () => ({ chat: { autoScroll: true } }),
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

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
page.setDefaultTimeout(5000);
const errors = [];
page.on('pageerror', (e) => errors.push(String(e)));
await page.addInitScript(mockTauri);
await page.goto(base);
await page.waitForSelector('[data-page]', { state: 'attached' });
await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));

const menu = (id) => page.evaluate((x) => window.dispatchEvent(new CustomEvent('cview:menu', { detail: x })), id);
const sheetText = () => page.locator('.sheet-scrim.app-sheet .sheet').innerText();
let failed = 0;
async function check(name, fn) {
  try { await fn(); console.log(`PASS ${name}`); } catch (e) { failed++; console.log(`FAIL ${name}: ${e.message}`); }
}

await check('단축키 시트: 세 묶음과 닫기', async () => {
  await menu('shortcuts');
  const t = await sheetText();
  for (const h of ['키보드 단축키', '탐색', '시청', '채팅']) assert.ok(t.includes(h), h);
  await page.locator('.sheet-scrim.app-sheet [data-close]').click();
  await page.waitForSelector('.sheet-scrim.app-sheet', { state: 'detached' });
});

await check('앱 정보 시트: 버전 (빌드) · OS', async () => {
  await menu('about');
  await page.waitForFunction(() => document.querySelector('.sheet-scrim.app-sheet .about'));
  const t = await sheetText();
  assert.ok(t.includes('0.1.40 (빌드 40)'), t);
  assert.ok(t.includes('macos aarch64'), t);
  await page.keyboard.press('Escape');
  await page.waitForSelector('.sheet-scrim.app-sheet', { state: 'detached' });
});

await check('업데이트 확인 시트: 새 버전이면 릴리스 페이지 단추', async () => {
  await menu('check-update');
  await page.waitForSelector('.sheet-scrim.app-sheet [data-update="page"]');
  const t = await sheetText();
  assert.ok(t.includes('0.1.41'), t);
});

await check('업데이트 확인 실패: 오류 문구와 다시 시도', async () => {
  await page.evaluate(() => { window.__mock.handlers.update_check = () => { throw new Error('서버 없음'); }; });
  await menu('check-update');
  await page.waitForSelector('.sheet-scrim.app-sheet [data-update="retry"]');
  assert.ok((await sheetText()).includes('서버 없음'));
});

await check('채팅 자동 스크롤 메뉴: 설정을 뒤집어 저장', async () => {
  await menu('chat-autoscroll');
  await page.waitForFunction(() => window.__mock.calls.some((c) => c.cmd === 'settings_update'));
  const call = await page.evaluate(() => window.__mock.calls.find((c) => c.cmd === 'settings_update'));
  assert.equal(JSON.stringify(call.args).includes('"autoScroll":false'), true, JSON.stringify(call.args));
});

await check('페이지 오류 없음', async () => assert.deepEqual(errors, []));

await browser.close();
server.close();
console.log(failed ? `${failed} failed` : 'all passed');
process.exit(failed ? 1 : 0);
