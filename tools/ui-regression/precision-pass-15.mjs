// 기능 정밀 고도화 15차(2026-11-28) 회귀 검사 — 검색 추천어, 멀티채팅 저장 목록 합치기, 탐색 기록·멀티라이브·시청 도구 수정.
// 모의 IPC·헤드리스 브라우저(실제 서비스·데이터는 건드리지 않는다). CVIEW_UI_BROWSER=webkit 이면 WebKit.
// 저장소 루트에서: NODE_PATH=<Playwright 경로> node tools/ui-regression/precision-pass-15.mjs
// V01 검색 추천어 — 입력하면 최근 검색어·내 채널에서 고른 추천이 뜨고, ↓·Enter 로 채널을 고르면 채널 화면을 연다. Esc 는 목록만 닫는다.
// V02 검색 추천어 순수 함수 — 앞부분 일치가 먼저, 방송 중 채널이 먼저, 입력한 그대로는 추천하지 않고 8개까지.
// V03 멀티채팅 저장 — 저장된 목록에서 이 화면이 닫은 채널만 빼고, 다른 창이 연 채널은 남긴다.
// V04 설정 머리말 — 없어진 '연동' 탭을 부제에서도 뺐다.
// V05 코드 확인 — 같은 화면 다시 열기·멀티라이브 복귀는 기록을 쌓지 않고, 프리셋은 구성을 바꾸며 둘러본 채널을 빼고 저장하고,
//     여러 채널 넣기는 자리가 차면 멈추고, LIVE 표시는 사용자 배속·일시정지를 보고, 메뉴가 숨으면 펼침 상태가 따라오며,
//     시청 화면을 닫으면 화질 메뉴·패널도 닫고, 작업 공간은 단독 보기 채널을 기억하고, 멀티채팅 채널이 바뀌면 사이드바를 다시 그린다.
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

function harness() {
  const inv = window.__TAURI__.core.invoke;
  window.__args = {};
  window.__TAURI__.core.invoke = async (c, a) => { (window.__args[c] ||= []).push(a || {}); return inv(c, a); };
  document.hasFocus = () => true;
}
function overlay(extra) {
  const fns = Object.fromEntries(Object.entries(extra).map(([k, v]) => [k, new Function(`return (${v})`)()]));
  const inv = window.__TAURI__.core.invoke;
  window.__TAURI__.core.invoke = async (c, a) => (fns[c] ? fns[c](a || {}) : inv(c, a));
}
async function open({ extra = {}, init = null } = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 }, colorScheme: 'dark', userAgent: MAC_UA });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, 'dark', 12]);
  await page.addInitScript(overlay, Object.fromEntries(Object.entries(extra).map(([k, v]) => [k, v.toString()])));
  await page.addInitScript(harness);
  if (init) await page.addInitScript(init);
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  return { ctx, page, errors };
}

try {
  await check('V01 검색 추천어 — 내 채널·최근 검색어를 띄우고 ↓·Enter 로 채널 화면을 연다', async () => {
    const { ctx, page, errors } = await open({ init: () => { try { localStorage.setItem('cview.recentSearches', JSON.stringify(['릴카 합방', '마인크래프트'])); } catch { /* 무시 */ } } });
    await go(page, 'search');
    const input = '[data-page="search"] input[type=search]';
    await page.waitForSelector('[data-page="search"]:not([hidden]) input[type=search]');
    await page.evaluate(() => { window.__opened = []; window.addEventListener('cview:open-channel', (e) => window.__opened.push(e.detail.channelId)); });
    await page.focus(input);
    await page.keyboard.type('릴');
    await page.waitForSelector('[data-page="search"] .search-suggest:not([hidden]) .sg-item');
    await page.waitForFunction(() => [...document.querySelectorAll('[data-page="search"] .sg-item')].some((x) => x.textContent.includes('릴카') && x.querySelector('.sg-avatar')));
    const texts = await page.$$eval('[data-page="search"] .sg-item .sg-text', (els) => els.map((e) => e.textContent));
    assert.equal(texts[0], '릴카', `채널이 먼저: ${texts}`);
    assert.ok(texts.includes('릴카 합방'), `최근 검색어: ${texts}`);
    assert.equal(await page.$eval(input, (e) => e.getAttribute('aria-expanded')), 'true');
    await page.keyboard.press('ArrowDown');
    assert.equal(await page.$eval(input, (e) => e.getAttribute('aria-activedescendant')), 'search-suggest-0');
    await page.keyboard.press('Enter');
    await page.waitForFunction(() => window.__opened.length > 0);
    assert.deepEqual(await page.evaluate(() => window.__opened), ['ch0']);
    // 다시 열고 Esc — 목록만 닫히고 검색 화면은 그대로다.
    await go(page, 'search');
    await page.focus(input);
    await page.fill(input, '');
    await page.keyboard.type('마인');
    await page.waitForSelector('[data-page="search"] .search-suggest:not([hidden])');
    await page.keyboard.press('Escape');
    assert.equal(await page.$eval('[data-page="search"] .search-suggest', (e) => e.hidden), true);
    assert.equal(await page.$eval('[data-page="search"]', (e) => e.hidden), false);
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('V02 검색 추천어 — 순서·중복·개수', async () => {
    const { ctx, page, errors } = await open();
    const r = await page.evaluate(() => import('/js/search-suggest.js').then(({ buildSuggestions }) => {
      const channels = [
        { id: 'a', name: '김도', live: false }, { id: 'b', name: '도도', live: false }, { id: 'c', name: '도리', live: true },
        { id: 'b', name: '도도', live: false }, { id: 'd', name: '' },
      ];
      const recent = ['도', '도 리뷰', '오늘 도시락', 'ABC'];
      const many = Array.from({ length: 20 }, (_, i) => ({ id: `x${i}`, name: `도${i}` }));
      return {
        mixed: buildSuggestions('도', { recent, channels }).map((x) => `${x.kind}:${x.text}`),
        none: buildSuggestions('  ', { recent, channels }).length,
        caps: buildSuggestions('abc', { recent }).map((x) => x.text),
        limit: buildSuggestions('도', { recent, channels: many }).length,
      };
    }));
    assert.deepEqual(r.mixed, ['channel:도리', 'channel:도도', 'channel:김도', 'query:도 리뷰', 'query:오늘 도시락']);
    assert.equal(r.none, 0);
    assert.deepEqual(r.caps, [], '입력한 그대로(대소문자만 다름)는 추천하지 않는다');
    assert.equal(r.limit, 8);
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('V03 멀티채팅 저장 — 다른 창이 연 채널은 남기고 이 화면이 닫은 채널만 뺀다', async () => {
    const { ctx, page, errors } = await open({
      extra: { settings_get: () => ({ multiLive: {}, multiChat: { savedSessions: [{ channelId: 'a', channelName: 'A' }, { channelId: 'b', channelName: 'B' }, { channelId: 'c', channelName: 'C' }] }, player: {}, chat: {}, general: {}, appearance: { theme: 'dark' } }) },
    });
    const r = await page.evaluate(async () => {
      const { MultiChatPage } = await import('/js/multichat.js');
      const saved = () => (window.__args.settings_update || []).filter((a) => a.domain === 'multiChat').map((a) => a.patch.savedSessions.map((x) => x.channelId));
      // 메인 창 화면은 A·B 만 열었다 — 창이 더한 C 를 지우지 않는다.
      const main = { channels: [{ channelId: 'a', name: 'A' }, { channelId: 'b', name: 'B' }], active: 'b', layout: 'tabs' };
      await MultiChatPage.prototype.persist.call(main);
      // 이 화면이 C 를 닫았다(removedIds) — 그것만 뺀다.
      const win = { channels: [{ channelId: 'a', name: 'A' }], removedIds: new Set(['c']), active: 'a', layout: 'tabs' };
      await MultiChatPage.prototype.persist.call(win);
      return saved();
    });
    assert.deepEqual(r, [['a', 'b', 'c'], ['a', 'b']]);
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('V04 설정 머리말에서 없어진 연동을 뺐다', async () => {
    const html = fs.readFileSync(path.resolve('app-ui/index.html'), 'utf8');
    assert.ok(!/시청 · 채팅 · 멀티라이브 · 연동/.test(html));
  });

  await check('V05 코드 확인 — 기록·프리셋·여러 채널·LIVE·메뉴·닫기·작업 공간·사이드바', async () => {
    const app = src('app.js');
    assert.match(app, /\|\| !Object\.keys\(params\)\.length/);
    assert.match(app, /if \(!transient && !remove\) \{ await navigate\('multilive', \{\}, \{ push: false \}\); return; \}/);
    assert.match(app, /enterWatch\(e\.detail\.sessionId, \{ push: current !== 'multilive' \}\)/);
    assert.match(app, /pages\.multilive\.state\.canAdd === false\) \{ skipped = todo\.length - i; break; \}/);
    assert.match(app, /watching \? pages\.multilive\.watch\?\.session\?\.channelId/);
    assert.match(app, /onChannelsChanged: \(\) => \{ if \(current === 'multichat'\) paintSidebarList\(\); \}/);
    const ml = src('multilive.js');
    assert.match(ml, /const ids = rememberedChannels\(this\.state\.sessions, this\.isTransient\);/);
    assert.match(ml, /async loadPreset\(channels\) \{[\s\S]*?multiLiveRemove\(s\.id\)[\s\S]*?restoreSessions\(channels\.filter/);
    assert.match(src('viewer-controls.js'), /\(this\.viewer\.panel\?\.rate \?\? 1\) === 1\);/);
    const pane = src('ml-pane-controls.js');
    assert.match(pane, /toggle\('at-edge', !badge\.behind && \(!v \|\| !v\.paused\) && this\.rate === 1\)/);
    assert.match(pane, /this\.menuWatch = new MutationObserver/);
    assert.match(src('viewer.js'), /this\.controls\?\.closeQualityMenu\?\.\(\);\n\s+if \(this\.panel\?\.el && !this\.panel\.el\.hidden\) this\.panel\.el\.hidden = true;/);
  });
} finally {
  await browser.close();
  server.close();
}
console.log(`\n${total - failures.length}/${total} 통과${failures.length ? ` — 실패: ${failures.join(', ')}` : ''}`);
process.exit(failures.length ? 1 : 0);
