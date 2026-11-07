// 넓은 창·좁은 창 UI 회귀 검사 — 폭에 따라 배치가 바뀌는 곳을 본다.
// 넓은 창: 홈 히어로가 띠처럼 늘지 않고 넘김 탭이 오른쪽 세로 목록이 되는지, 설정 카드가 두 단으로 흐르는지.
// 좁은 창: 멀티라이브 채팅이 저절로 접히고 '채팅' 버튼으로 열면 영상 위에 겹치는지, 멀티채팅 머리의 검색칸이 다음 줄로 내려가는지,
//          넘치는 화면 탭 끝이 흐려지는지, 어느 화면도 가로로 넘치지 않는지.
// 저장소 루트에서: node tools/ui-regression/responsive.mjs   (Playwright 가 필요하다)
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import { server, go, openApp } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}
const box = (page, sel) => page.$eval(sel, (n) => { const r = n.getBoundingClientRect(); return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width, height: r.height }; });
const visible = (page, sel) => page.$eval(sel, (n) => Boolean(n.offsetParent) && getComputedStyle(n).display !== 'none').catch(() => false);

// ── 순수 함수 ──
const { tooNarrowForChat, CHAT_WIDTH } = await import(pathToFileURL(path.resolve('app-ui/js/splitter.js')).href);
await check('R00 tooNarrowForChat: chat folds only when chat and player minimums do not fit', () => {
  const edge = CHAT_WIDTH.min + CHAT_WIDTH.minStage;
  assert.equal(tooNarrowForChat(edge - 1), true);
  assert.equal(tooNarrowForChat(edge), false);
  assert.equal(tooNarrowForChat(1200), false);
  assert.equal(tooNarrowForChat(0), false, '아직 배치 전(0)은 접지 않는다');
  assert.equal(tooNarrowForChat(NaN), false);
});

const browser = await chromium.launch();
const ROUTES = [['home', {}], ['following', {}], ['popular', {}], ['categories', {}], ['multichat', {}], ['multilive', {}], ['library', {}], ['stats', {}], ['search', { query: '루다' }], ['settings', {}]];

// ── 넓은 창 ──
{
  const { ctx, page } = await openApp(browser, { width: 2560 });
  await check('R01 wide home: hero keeps ~3:1 and the queue becomes a vertical list on the right', async () => {
    await go(page, 'home');
    await page.waitForSelector('[data-page="home"] .hero-main');
    const hero = await box(page, '[data-page="home"] .hero-main');
    const queue = await box(page, '[data-page="home"] .hero-queue');
    assert.ok(queue.left >= hero.right, `넘김 탭이 히어로 오른쪽(${queue.left} >= ${hero.right})`);
    assert.ok(hero.width / hero.height <= 3.4, `히어로 비율 ${(hero.width / hero.height).toFixed(2)}:1`);
    const tops = await page.$$eval('[data-page="home"] .queue-item', (xs) => xs.map((x) => Math.round(x.getBoundingClientRect().top)));
    assert.ok(tops.length > 2 && tops.every((t, i) => i === 0 || t > tops[i - 1]), `세로로 쌓임 ${tops}`);
  });
  await check('R02 wide settings: groups flow into two columns', async () => {
    await go(page, 'settings');
    await page.waitForSelector('[data-page="settings"] .settings-group');
    const lefts = await page.$$eval('[data-page="settings"] .settings-group', (gs) => [...new Set(gs.filter((g) => g.offsetParent).map((g) => Math.round(g.getBoundingClientRect().left)))]);
    assert.equal(lefts.length, 2, `단 시작 위치 ${lefts}`);
  });
  await check('R03 settings OAuth form keeps the same inner padding as other rows', async () => {
    const g = await box(page, '[data-page="settings"] .oauth-group');
    const input = await box(page, '[data-page="settings"] .oauth-form input');
    assert.ok(input.left - g.left >= 12, `왼쪽 안쪽 여백 ${input.left - g.left}px`);
  });
  await check('R04 wide window: no page scrolls sideways', async () => {
    const bad = [];
    for (const [route, params] of ROUTES) {
      await go(page, route, params);
      await page.waitForTimeout(300);
      if (await page.evaluate(() => document.scrollingElement.scrollWidth > innerWidth + 1)) bad.push(route);
    }
    assert.deepEqual(bad, []);
  });
  await ctx.close();
}
{
  const { ctx, page } = await openApp(browser, { width: 1280 });
  await check('R05 regular home: queue stays below the hero; settings stays one column', async () => {
    await go(page, 'home');
    await page.waitForSelector('[data-page="home"] .hero-main');
    const hero = await box(page, '[data-page="home"] .hero-main');
    const queue = await box(page, '[data-page="home"] .hero-queue');
    assert.ok(queue.top >= hero.bottom, '넘김 탭이 히어로 아래');
    await go(page, 'settings');
    await page.waitForSelector('[data-page="settings"] .settings-group');
    const lefts = await page.$$eval('[data-page="settings"] .settings-group', (gs) => [...new Set(gs.filter((g) => g.offsetParent).map((g) => Math.round(g.getBoundingClientRect().left)))]);
    assert.equal(lefts.length, 1);
  });
  await ctx.close();
}

// ── 좁은 창 ──
{
  const { ctx, page } = await openApp(browser, { width: 640 });
  await check('R10 narrow multilive: chat folds by itself and opens over the stage', async () => {
    await go(page, 'multilive');
    await page.waitForSelector('[data-page="multilive"] .ml-body');
    const chat = '[data-page="multilive"] .ml-chat';
    const reopen = '[data-page="multilive"] .chat-reopen';
    assert.equal(await visible(page, chat), false, '좁으면 채팅이 접힌다');
    assert.equal(await visible(page, reopen), true, "'채팅' 버튼이 보인다");
    const stageBefore = await box(page, '[data-page="multilive"] .ml-main');
    await page.click(reopen);
    assert.equal(await visible(page, chat), true, '버튼으로 연다');
    const st = await page.$eval(chat, (n) => ({ cls: n.classList.contains('narrow-open'), pos: getComputedStyle(n).position }));
    assert.ok(st.cls && st.pos === 'absolute', '영상 위에 겹친다');
    const stageAfter = await box(page, '[data-page="multilive"] .ml-main');
    assert.equal(Math.round(stageAfter.width), Math.round(stageBefore.width), '영상 칸 폭은 그대로');
    await page.click(`${chat} .chat-collapse`);
    assert.equal(await visible(page, chat), false, '접기 버튼으로 다시 접힌다');
    const saved = await page.evaluate(() => JSON.parse(localStorage.getItem('cview.chatPanel.multilive') || 'null'));
    assert.ok(!saved || saved.collapsed !== true, '좁은 창 자동 접기는 저장하지 않는다');
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.waitForTimeout(250);
    const wide = await page.$eval(chat, (n) => ({ shown: Boolean(n.offsetParent), over: n.classList.contains('narrow-open') }));
    assert.deepEqual(wide, { shown: true, over: false }, '넓어지면 원래대로 옆에 붙는다');
    await page.setViewportSize({ width: 640, height: 900 });
  });
  await check('R11 narrow multichat header: search moves to its own row instead of being squeezed', async () => {
    await go(page, 'multichat');
    await page.waitForSelector('[data-page="multichat"] .mc-search');
    const title = await box(page, '[data-page="multichat"] .page-title');
    const search = await box(page, '[data-page="multichat"] .mc-search');
    assert.ok(search.top >= title.bottom, '검색칸이 제목 아래 줄');
    assert.ok(search.width >= 300, `검색칸 폭 ${search.width}px`);
  });
  await check('R12 narrow library: overflowing page tabs fade at the hidden edge', async () => {
    await go(page, 'library');
    const sel = '[data-page="library"] .lib-tabs';
    await page.waitForSelector(sel);
    assert.equal(await page.$eval(sel, (n) => n.dataset.edge), 'end');
    await page.focus(`${sel} [aria-selected="true"]`);
    await page.keyboard.press('End');
    await page.waitForTimeout(150);
    assert.equal(await page.$eval(sel, (n) => n.dataset.edge), 'start');
    await page.keyboard.press('Home');
  });
  await check('R13 narrow window: no page scrolls sideways', async () => {
    const bad = [];
    for (const [route, params] of ROUTES) {
      await go(page, route, params);
      await page.waitForTimeout(300);
      if (await page.evaluate(() => document.scrollingElement.scrollWidth > innerWidth + 1)) bad.push(route);
    }
    assert.deepEqual(bad, []);
  });
  await ctx.close();
}

await browser.close();
server.close();
for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : `\n     ${r.error.split('\n').slice(0, 12).join('\n     ')}`}`);
const failed = results.filter((r) => !r.ok).length;
console.log(`\n${results.length - failed}/${results.length} passed`);
process.exitCode = failed ? 1 : 0;
