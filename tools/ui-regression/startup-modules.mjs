// 시작 모듈 로딩 장애 회귀 검사. 실제 앱과 모의 IPC를 쓰며 창을 띄우지 않는다.
// NODE_PATH=<Playwright 경로> [CVIEW_UI_BROWSER=webkit] node tools/ui-regression/startup-modules.mjs
// 정상 시작 반복: CVIEW_BOOT_CASE=BM11 CVIEW_STARTUP_REPEAT=100 을 함께 지정한다.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { createHash } from 'node:crypto';
import { createRequire } from 'node:module';
import { server, base, mockTauri } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const name = process.env.CVIEW_UI_BROWSER === 'webkit' ? 'webkit' : 'chromium';
const browser = await pw[name].launch({ headless: true });
const results = [];

async function open({ asset = 'app-sheets.js', fault = 'mime', permanent = false, storageBlocked = false, csp = false } = {}) {
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const page = await context.newPage();
  const state = { visits: 0, attempts: 0, blocked: true, errors: [], held: [] };
  page.on('framenavigated', (frame) => { if (frame === page.mainFrame()) state.visits++; });
  page.on('pageerror', (error) => state.errors.push(String(error)));
  await page.addInitScript(mockTauri, [base, 'dark']);
  await page.addInitScript(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async (cmd, args) => {
      try { return await invoke(cmd, args); } catch { return null; }
    };
  });
  if (storageBlocked) await page.addInitScript(() => {
    const session = sessionStorage;
    for (const key of ['getItem', 'setItem', 'removeItem']) {
      const original = Storage.prototype[key];
      Storage.prototype[key] = function (...args) {
        if (this === session) throw new DOMException('검사용 저장소 차단', 'SecurityError');
        return original.apply(this, args);
      };
    }
  });
  if (csp) {
    const html = fs.readFileSync('app-ui/index.html', 'utf8');
    const inline = html.match(/<script>([\s\S]*?)<\/script>/)[1];
    const hash = createHash('sha256').update(inline).digest('base64');
    const config = JSON.parse(fs.readFileSync('crates/cview-app/tauri.conf.json', 'utf8'));
    const policy = config.app.security.csp.replace("script-src 'self'", `script-src 'self' 'sha256-${hash}'`);
    await context.route(base, (route) => route.fulfill({ contentType: 'text/html', headers: { 'content-security-policy': policy }, body: html }));
  }
  if (fault !== 'none') await context.route(`${base}js/${asset}`, async (route) => {
    state.attempts++;
    if (!state.blocked || (!permanent && state.attempts > 1)) return route.continue();
    if (fault === 'stall') { state.held.push(route); return; }
    if (fault === '404') return route.fulfill({ status: 404, body: '' });
    if (fault === 'syntax') return route.fulfill({ contentType: 'text/javascript', body: 'export const = ;' });
    if (fault === 'binding') return route.fulfill({ contentType: 'text/javascript', body: 'export const unrelated = true;' });
    return route.fulfill({ contentType: 'text/css', body: 'body { color: red; }' });
  });
  await page.goto(base, { waitUntil: 'commit' });
  return { context, page, state };
}

async function healthy(page, timeout = 14000) {
  // 시작 화면 제거만으로 성공 처리하지 않고 앱 초기화 끝의 IPC까지 확인한다.
  await page.waitForFunction(() => !document.getElementById('splash')
    && !document.getElementById('boot-error')
    && window.__calls?.includes('deeplink_take_pending'), null, { polling: 50, timeout });
}

async function check(id, fn) {
  if (process.env.CVIEW_BOOT_CASE && !id.startsWith(process.env.CVIEW_BOOT_CASE)) return;
  try { await fn(); results.push({ id, ok: true }); }
  catch (error) { results.push({ id, ok: false, error: error.message }); }
  console.log(`${results.at(-1).ok ? '통과' : '실패'} ${id}${results.at(-1).error ? ` — ${results.at(-1).error}` : ''}`);
}

try {
  await check('BM01 일시적인 CSS 응답 뒤 시작 모듈을 한 번 다시 불러온다', async () => {
    const { context, page, state } = await open();
    try {
      await healthy(page, 6000);
      assert.equal(state.visits, 2);
      assert.equal(state.attempts, 2);
      if (name === 'webkit') assert.ok(state.errors.some((error) => /MIME|module script/i.test(error)), state.errors.join('\n'));
    } finally { await context.close(); }
  });

  await check('BM02 메인 모듈의 일시적인 404도 복구한다', async () => {
    const { context, page, state } = await open({ asset: 'app.js', fault: '404' });
    try { await healthy(page, 6000); assert.equal(state.visits, 2); }
    finally { await context.close(); }
  });

  await check('BM03 반복 오류는 재시작을 멈추고 사용자가 다시 시도할 수 있다', async () => {
    const { context, page, state } = await open({ permanent: true });
    try {
      const retry = page.locator('#boot-error button');
      await retry.waitFor({ timeout: 6000 });
      assert.equal(state.visits, 2);
      assert.equal(await page.locator('#splash').count(), 0);
      assert.equal(await page.locator('#boot-error').getAttribute('role'), 'alert');
      assert.equal(await retry.textContent(), '다시 불러오기');
      await page.waitForTimeout(500);
      assert.equal(state.visits, 2, '자동 재시작은 한 번으로 제한한다');
      state.blocked = false;
      await retry.click();
      await healthy(page);
      assert.equal(state.visits, 3);
      // 복구한 탭에서 다음 시작 오류가 생겨도 이전 실패 횟수가 남지 않는다.
      state.blocked = true;
      const attempt = state.attempts;
      await page.reload({ waitUntil: 'commit' });
      await retry.waitFor({ timeout: 6000 });
      assert.equal(state.attempts - attempt, 2);
      assert.equal(state.visits, 5);
    } finally { await context.close(); }
  });

  await check('BM04 저장소 접근이 차단돼도 재시작 루프 없이 수동 복구한다', async () => {
    const { context, page, state } = await open({ permanent: true, storageBlocked: true });
    try {
      const retry = page.locator('#boot-error button');
      await retry.waitFor({ timeout: 6000 });
      assert.equal(state.visits, 1);
      state.blocked = false;
      await retry.click();
      await healthy(page);
      assert.equal(state.visits, 2);
    } finally { await context.close(); }
  });

  await check('BM05 응답이 멈춘 모듈도 독립적인 제한 시간 뒤 복구한다', async () => {
    const { context, page, state } = await open({ fault: 'stall' });
    try {
      await healthy(page);
      assert.equal(state.visits, 2);
      assert.equal(state.attempts, 2);
    } finally { await context.close(); }
  });

  await check('BM06 응답이 계속 멈추면 오류 화면을 표시하고 재시작을 멈춘다', async () => {
    const { context, page, state } = await open({ fault: 'stall', permanent: true });
    try {
      await page.locator('#boot-error button').waitFor({ timeout: 19000 });
      assert.equal(state.visits, 2);
      assert.equal(await page.locator('#splash').count(), 0);
      state.blocked = false;
      await page.locator('#boot-error button').click();
      await healthy(page);
    } finally { await context.close(); }
  });

  await check('BM07 모듈 구문 오류도 시작 화면에 갇히지 않고 복구한다', async () => {
    const { context, page, state } = await open({ fault: 'syntax' });
    try { await healthy(page); assert.equal(state.visits, 2); }
    finally { await context.close(); }
  });

  await check('BM08 정상 시작 뒤 리소스 오류는 실행 중인 앱을 다시 시작하지 않는다', async () => {
    const { context, page, state } = await open();
    try {
      await healthy(page);
      const visits = state.visits;
      await page.evaluate(() => {
        document.querySelector('script[type="module"]').dispatchEvent(new Event('error'));
        window.dispatchEvent(new ErrorEvent('error', { message: '시작 뒤 오류 검사' }));
      });
      await page.waitForTimeout(500);
      assert.equal(state.visits, visits);
      assert.equal(await page.locator('#boot-error').count(), 0);
    } finally { await context.close(); }
  });

  await check('BM09 다른 모듈 본문으로 인한 가져오기 오류도 복구한다', async () => {
    const { context, page, state } = await open({ fault: 'binding' });
    try { await healthy(page, 6000); assert.equal(state.visits, 2); }
    finally { await context.close(); }
  });

  await check('BM10 앱 보안 정책과 인라인 해시가 적용돼도 복구한다', async () => {
    const { context, page, state } = await open({ csp: true });
    try { await healthy(page, 6000); assert.equal(state.visits, 2); }
    finally { await context.close(); }
  });

  await check('BM11 실제 정적 응답으로 반복 시작해도 오류나 재시도가 없다', async () => {
    const count = Number(process.env.CVIEW_STARTUP_REPEAT || 1);
    assert.ok(Number.isInteger(count) && count >= 1 && count <= 1000);
    for (let i = 0; i < count; i++) {
      const { context, page, state } = await open({ fault: 'none' });
      try {
        await healthy(page);
        assert.deepEqual(state.errors, [], `${i + 1}번째 시작 오류`);
        assert.equal(state.visits, 1, `${i + 1}번째 시작에서 재시도 발생`);
      } finally { await context.close(); }
    }
    console.log(`정상 시작 ${count}회 확인`);
  });
} finally { await browser.close(); server.close(); }

console.log(JSON.stringify({ engine: name, results }, null, 2));
process.exitCode = results.some((result) => !result.ok) ? 1 : 0;
