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
        calls.push({ cmd, args, at: Date.now() }); // at — 모의 시계 기준 호출 시각
        const h = handlers[cmd];
        if (!h) throw new Error(`mock: ${cmd}`);
        return h(args);
      },
    },
    event: { listen: async () => () => {} },
  };
  // 환영 안내를 띄우지 않는다 — 키는 cview.welcomed.v1(onboarding.js). 옛 키만 넣으면 시계를 돌린 뒤 환영 막이 늦게 떠
  // 업데이트 줄 클릭을 가로챘다(실행마다 다른 항목이 시간 초과).
  try { localStorage.setItem('cview.onboarded', '1'); localStorage.setItem('cview.welcomed.v1', String(Date.now())); } catch { /* 무시 */ }
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
  // 앱은 확인 타이머를 건(installUpdateNotifier) 바로 뒤에 deeplink_take_pending 을 부른다 — 그때가 '실행 직후' 기준점이다.
  // 이보다 먼저 시계를 앞당기면 타이머가 늦게 걸려 첫 확인 시점을 잴 수 없다.
  await page.waitForFunction(() => window.__mock.calls.some((c) => c.cmd === 'deeplink_take_pending'));
  const readyAt = await page.evaluate(() => window.__mock.calls.find((c) => c.cmd === 'deeplink_take_pending').at);
  const checks = () => page.evaluate(() => window.__mock.calls.filter((c) => c.cmd === 'update_check').length);
  // 기준점에서 첫 확인까지(모의 시계 ms) — 15초(FIRST_DELAY_MS) 안팎이어야 한다
  const firstDelay = () => page.evaluate((t0) => window.__mock.calls.find((c) => c.cmd === 'update_check').at - t0, readyAt);
  return { page, errors, checks, firstDelay };
}

// 시계를 조금씩 앞당기며 조건을 기다린다. 앱은 비동기 시작(설정 읽기 등)을 마친 뒤에야 확인 타이머를 걸고, 실패 뒤 재시도
// 타이머도 응답을 받은 다음 걸린다 — 그 전에 시계를 한 번에 앞당기면 타이머가 늦게 걸려 영영 울리지 않았다(실행마다 다른 항목이
// 시간 초과).
async function advanceUntil(page, pred, stepMs, maxSteps, what) {
  for (let i = 0; i < maxSteps; i++) {
    if (await page.evaluate(pred)) return;
    await page.clock.fastForward(stepMs);
  }
  assert.ok(await page.evaluate(pred), `${what}: 시계를 ${(stepMs * maxSteps) / 1000}초 앞당겨도 오지 않음`);
}
const pillShown = () => Boolean(document.querySelector('#update-pill:not([hidden]) .update-pill-main'));
const checkCount = () => window.__mock.calls.filter((c) => c.cmd === 'update_check').length;
// 첫 확인은 실행 직후가 아니라 약 15초 뒤 — 시계를 1초씩 앞당겨 기다리되 늦게 와도 통과하지 않게 상한을 둔다
async function assertFirstCheckAt15s(firstDelay) {
  const ms = await firstDelay();
  assert.ok(ms >= 14000 && ms <= 17000, `첫 확인이 실행 ${ms / 1000}초 뒤(15초 안팎이어야 함)`);
}

await check('실행 직후엔 확인하지 않고 15초 뒤 조용히 확인 → 사이드바 줄·토스트', async () => {
  const { page, errors, checks, firstDelay } = await open();
  await page.clock.fastForward(5000);
  assert.equal(await checks(), 0, '5초 안에 확인하면 안 됨');
  await advanceUntil(page, pillShown, 1000, 20, '업데이트 줄');
  assert.equal(await checks(), 1);
  await assertFirstCheckAt15s(firstDelay);
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
  const { page, firstDelay } = await open(() => { localStorage.setItem('cview.updateNotified', '0.1.41'); });
  await advanceUntil(page, pillShown, 1000, 20, '업데이트 줄');
  await assertFirstCheckAt15s(firstDelay);
  assert.equal(await page.locator('.toast-stack .toast').count(), 0);
  await page.locator('#update-pill .update-pill-close').click();
  assert.equal(await page.locator('#update-pill').isHidden(), true);
  assert.equal(await page.evaluate(() => localStorage.getItem('cview.updatePillHidden')), '0.1.41');
  await page.close();
});

await check('최신 버전이면 줄도 토스트도 없음', async () => {
  const { page, checks, firstDelay } = await open(() => { window.__latest = true; });
  await page.evaluate(() => { window.__mock.handlers.update_check = () => ({ current: '0.1.41', latest: '0.1.41', newer: false }); });
  await advanceUntil(page, checkCount, 1000, 20, '첫 확인');
  await assertFirstCheckAt15s(firstDelay);
  await page.evaluate(() => new Promise((r) => setTimeout(r, 0)));
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

await check('자동 확인을 켜면 곧 확인하고, 끄면 줄을 내리고 다음 확인을 멈춤(12차)', async () => {
  const { page, checks } = await open(() => { window.__autoCheck = false; });
  await page.clock.fastForward(16000);
  assert.equal(await checks(), 0);
  const toggle = (on) => page.evaluate((v) => {
    window.__autoCheck = v;
    window.dispatchEvent(new CustomEvent('cview:settings-changed', { detail: { domain: 'general', key: 'autoCheckUpdates', value: v, settings: { general: { autoCheckUpdates: v } } } }));
  }, on);
  await toggle(true);
  // 예전엔 꺼진 채 시작하면 다음 확인이 6시간 뒤로 잡혀 켜도 묻지 않았다
  for (let i = 0; i < 10 && (await checks()) < 1; i++) await page.clock.fastForward(1000);
  assert.equal(await checks(), 1, '켠 뒤 10초 안에 확인하지 않음');
  await page.waitForSelector('#update-pill:not([hidden])');
  await toggle(false);
  assert.equal(await page.locator('#update-pill').isHidden(), true, '끈 뒤에도 새 버전 줄이 남음');
  await page.clock.fastForward(7 * 60 * 60 * 1000);
  assert.equal(await checks(), 1, '끈 뒤에도 확인함');
  await page.close();
});

await check('확인 실패는 조용히 넘어가고 30분 뒤 다시 시도', async () => {
  const { page, checks, firstDelay } = await open();
  await page.evaluate(() => { window.__mock.handlers.update_check = () => { throw new Error('오프라인'); }; });
  await advanceUntil(page, checkCount, 1000, 20, '첫 확인');
  await assertFirstCheckAt15s(firstDelay);
  assert.equal(await page.locator('.toast-stack .toast').count(), 0);
  // 실패 직후(6시간 주기가 아니라) 30분 안팎에 다시 묻는다 — 1분씩 앞당겨 29~32분 사이에 와야 한다
  let minutes = 0;
  while ((await checks()) < 2 && minutes < 40) { await page.clock.fastForward(60 * 1000); minutes++; }
  assert.ok((await checks()) >= 2, '40분 안에 다시 시도하지 않음');
  assert.ok(minutes >= 29 && minutes <= 32, `다시 시도까지 ${minutes}분`);
  await page.close();
});

await browser.close();
server.close();
console.log(failed ? `${failed} failed` : 'all passed');
process.exit(failed ? 1 : 0);
