// 기능 정밀 고도화 16차(2026-11-28) 회귀 검사 — 화면 이동 초점, 단축키·대화상자, 광고 소리, 전체 화면 단추, 화질 전환.
// 모의 IPC·헤드리스 브라우저(실제 서비스·데이터는 건드리지 않는다). CVIEW_UI_BROWSER=webkit 이면 WebKit.
// 저장소 루트에서: NODE_PATH=<Playwright 경로> node tools/ui-regression/precision-pass-16.mjs
// X01 화면 이동 초점 — 키보드로 연 화면은 제목이 초점을 받고, 뒤로 가면 떠날 때 있던 요소로 돌아온다.
// X02 ⇧⌘K 는 팔레트를 열지 않는다(⌘K 는 연다).
// X03 확인 창 — 안의 글자를 눌러도 Esc 로 닫힌다.
// X04 처음 안내 — 다시 보기를 눌러도 두 장이 쌓이지 않고, 위에 뜬 팔레트의 ←/→·Esc 를 가로채지 않는다.
// X05 코드 확인 — Windows 가운데 클릭·기본 우클릭 메뉴·Ctrl+Y 예약, 재생 메뉴가 다시보기·멀티라이브 대상을 쓰고, 보조 창은 사이드바를 뒤집지 않으며, 패인·시청 화면 광고가
//     볼륨·음소거로 시작을 따르고, 화면을 옮기면 칩 미리보기를 닫고, 전체 화면 단추가 상태를 보이고, 화질 메뉴가 버퍼를 비우지
//     않고, v2 지연 맞춤이 토큰을 기다리지 않는다.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { base, server, mockTauri, go } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const WEBKIT = process.env.CVIEW_UI_BROWSER === 'webkit';
const browser = await (WEBKIT ? pw.webkit : pw.chromium).launch();
const MAC_UA = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)';
const failures = [];
let total = 0;
async function check(name, fn) {
  total += 1;
  try { await fn(); console.log(`ok   ${name}`); } catch (e) { failures.push(name); console.log(`FAIL ${name}\n     ${e.message.split('\n').join('\n     ')}`); }
}
const src = (f) => fs.readFileSync(path.resolve('app-ui/js', f), 'utf8');

/** 명령 인자를 모으고, 이벤트 구독을 window.__emit 으로 흉내 낸다. */
function harness() {
  const inv = window.__TAURI__.core.invoke;
  window.__args = {};
  window.__TAURI__.core.invoke = async (c, a) => { (window.__args[c] ||= []).push(a || {}); return inv(c, a); };
  const subs = new Map();
  const listen = async (event, cb) => { (subs.get(event) || subs.set(event, []).get(event)).push(cb); return () => {}; };
  window.__TAURI__.event = { listen };
  window.__TAURI__.webviewWindow = { getCurrentWebviewWindow: () => ({ listen }) };
  window.__emit = (event, payload) => { for (const cb of subs.get(event) || []) cb({ payload }); };
  document.hasFocus = () => true;
}
function overlay(extra) {
  const fns = Object.fromEntries(Object.entries(extra).map(([k, v]) => [k, new Function(`return (${v})`)()]));
  const inv = window.__TAURI__.core.invoke;
  window.__TAURI__.core.invoke = async (c, a) => (fns[c] ? fns[c](a || {}) : inv(c, a));
}
async function open({ extra = {} } = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 }, colorScheme: 'dark', userAgent: MAC_UA });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, 'dark', 12]);
  await page.addInitScript(overlay, Object.fromEntries(Object.entries(extra).map(([k, v]) => [k, v.toString()])));
  await page.addInitScript(harness);
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  return { ctx, page, errors };
}
const frame = (page) => page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));

try {
  await check('X01 화면 이동 초점 — 새 화면 제목으로, 뒤로 가면 떠난 자리로', async () => {
    const { ctx, page, errors } = await open();
    await go(page, 'library', { tab: 'favorites' });
    await page.waitForSelector('[data-page="library"]:not([hidden]) .lib-tabs [data-tab="recent"]');
    // 보관함 탭 단추에 초점을 두고(키보드 사용자), 다른 화면으로 옮긴다.
    await page.focus('[data-page="library"] .lib-tabs [data-tab="recent"]');
    await go(page, 'settings');
    await page.waitForSelector('[data-page="settings"]:not([hidden])');
    await frame(page);
    const onTitle = await page.evaluate(() => {
      const a = document.activeElement;
      return Boolean(a && a.classList.contains('page-title') && a.closest('[data-page="settings"]'));
    });
    assert.ok(onTitle, '초점이 숨은 화면에 남았다');
    await page.evaluate(() => window.__emit('menu://action', { id: 'back' }));
    await page.waitForSelector('[data-page="library"]:not([hidden])');
    await frame(page);
    await page.waitForTimeout(100);
    const back = await page.evaluate(() => document.activeElement?.dataset?.tab || document.activeElement?.tagName);
    assert.equal(back, 'recent', '뒤로 가기 뒤 초점이 떠난 자리로 돌아오지 않았다');
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('X02 ⇧⌘K 는 팔레트를 열지 않는다', async () => {
    const { ctx, page, errors } = await open();
    await page.waitForTimeout(300);
    await page.keyboard.press('Meta+Shift+KeyK');
    await page.waitForTimeout(150);
    const shifted = await page.evaluate(() => Boolean(document.querySelector('.palette-scrim:not([hidden]), .palette:not([hidden])') && !document.querySelector('.palette-scrim')?.hidden));
    assert.equal(shifted, false, '⇧⌘K 가 팔레트를 열었다');
    await page.keyboard.press('Meta+KeyK');
    await page.waitForFunction(() => { const el = document.querySelector('.palette-scrim'); return el && !el.hidden; });
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('X03 확인 창 — 글자를 눌러도 Esc 로 닫힌다', async () => {
    const { ctx, page, errors } = await open();
    await page.evaluate(() => import('/js/confirm.js').then(({ confirmDialog }) => {
      window.__confirm = confirmDialog({ title: '지울까요', facts: [['대상', '최근 시청 목록']], confirmLabel: '지우기' }).then((v) => { window.__confirmResult = v === null ? 'closed' : 'ok'; });
    }));
    await page.waitForSelector('.confirm-sheet .sheet');
    await page.click('.confirm-sheet .confirm-facts dd');
    await page.keyboard.press('Escape');
    await page.waitForFunction(() => window.__confirmResult === 'closed', null, { timeout: 3000 });
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('X04 처음 안내 — 두 장 쌓이지 않고 팔레트 키를 가로채지 않는다', async () => {
    const { ctx, page, errors } = await open();
    await page.evaluate(() => import('/js/onboarding.js').then(({ showWelcome }) => { showWelcome({ force: true }); showWelcome({ force: true }); }));
    await page.waitForSelector('.welcome-scrim');
    assert.equal(await page.$$eval('.welcome-scrim', (x) => x.length), 1);
    const step = () => page.evaluate(() => document.querySelector('.welcome-scrim [data-w="next"]')?.textContent + '|' + document.querySelector('.welcome-scrim h2')?.textContent);
    const before = await step();
    await page.keyboard.press('Meta+KeyK');
    await page.waitForFunction(() => { const el = document.querySelector('.palette-scrim'); return el && !el.hidden; });
    await page.keyboard.type('홈');
    await page.keyboard.press('ArrowLeft');
    await page.keyboard.press('ArrowRight');
    assert.equal(await step(), before, '팔레트에서 누른 ←/→ 가 안내 장을 넘겼다');
    await page.keyboard.press('Escape');
    await page.waitForTimeout(250);
    assert.equal(await page.$$eval('.welcome-scrim', (x) => x.length), 1, 'Esc 한 번에 안내까지 닫혔다');
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('X05 코드 확인 — 재생 메뉴·보조 창·광고 소리·칩 미리보기·전체 화면·화질·v2', async () => {
    const app = src('app.js');
    assert.match(app, /else playbackFallback\?\.\('play-pause'\);/);
    assert.match(app, /else if \(playbackFallback\) playbackFallback\('pip'\);/);
    assert.match(app, /'toggle-sidebar': \(\) => \{ if \(!AUX\) sidebar\.toggle\(\); \}/);
    assert.match(app, /\(IS_WINDOWS \? e\.ctrlKey : e\.metaKey\) && !e\.shiftKey && !e\.altKey && e\.key\.toLowerCase\(\) === 'k'/);
    assert.match(app, /if \(pages\.multilive\?\.root\.hidden\) pages\.multilive\.chipPeek\?\.close\(\);/);
    assert.match(src('ads.js'), /video\.muted = adMutedPref\(\) \|\| Boolean\(session\.muted\);/);
    assert.match(src('multilive.js'), /\.\.\.this\.paneAdSound\(pane, st && st\.player\)/);
    assert.match(src('viewer.js'), /target: 'viewer', muted: Boolean\(this\.sessionMuted\)/);
    assert.match(src('ml-pane-controls.js'), /document\.fullscreenElement === this\.el;/);
    assert.match(src('ml-watch.js'), /document\.fullscreenElement === this\.body;/);
    assert.match(src('viewer-controls.js'), /const chatOn = document\.fullscreenElement === this\.root;/);
    for (const f of ['viewer-controls.js', 'ml-pane-controls.js']) assert.ok(!/hls\.currentLevel = -1; hls\.loadLevel = -1;/.test(src(f)), `${f} 의 자동 화질이 버퍼를 비운다`);
    const player = fs.readFileSync(path.resolve('app-ui/player/player.html'), 'utf8');
    assert.match(player, /if \(!wasLocked\) return;/);
    assert.match(app, /document\.addEventListener\('auxclick'/);
    assert.match(app, /if \(!IS_WINDOWS\) return;\n\s+document\.addEventListener\('contextmenu'/);
    assert.match(src('shortcuts.js'), /\['y', C, '다시 실행'\]/);
    assert.match(src('confirm.js'), /role="alertdialog" tabindex="-1"/);
    const profile = fs.readFileSync(path.resolve('crates/cview-player/src/hlsjs/profile.rs'), 'utf8');
    assert.match(profile, /seek_token_required: false,/);
  });
} finally {
  await browser.close();
  server.close();
}
console.log(`\n${total - failures.length}/${total} 통과${failures.length ? ` — 실패: ${failures.join(', ')}` : ''}`);
process.exit(failures.length ? 1 : 0);
