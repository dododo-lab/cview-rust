// 기능 정밀 고도화 14차(2026-11-28) 회귀 검사 — 설정 화면 정리, 채팅 입력·멘션, 보관함 빼기, 카드 메뉴·홈 레일.
// 모의 IPC·헤드리스 브라우저(실제 서비스·데이터는 건드리지 않는다). CVIEW_UI_BROWSER=webkit 이면 WebKit.
// 저장소 루트에서: NODE_PATH=<Playwright 경로> node tools/ui-regression/precision-pass-14.mjs
// T01 설정 — 읽는 곳이 없던 항목(자동 재생·하드웨어 디코딩·따라잡기 배속·연결 시간 제한 등)과 '연동' 탭이 없고, 시도 횟수는 1부터.
// T02 설정 — 네트워크 값을 손으로 바꾸면 프리셋 단추가 저장된 프리셋(자동 → 직접)으로 바뀐다.
// T03 설정 — 다른 곳에서 테마를 바꾸면 개요의 테마 선택이 따라간다.
// T04 설정 — 금지어를 넣으면 금지어 필터도 켠다.
// T05 멘션 — 내 닉네임으로 시작하는 더 긴 닉네임(@도도새)은 나를 부른 것이 아니다.
// T06 보관함 — 이어 보기·나중에 보기 카드의 ✕ 로 항목을 빼고(되돌리기 시간 뒤 저장소에서 지움), 카드는 열리지 않는다.
// T07 카드 메뉴 — 방송 중이 아닌 채널 줄(검색 '채널')은 재생·멀티라이브·채팅만 열기를 띄우지 않는다.
// T08 코드 확인 — 이모티콘 자동 완성은 두 글자부터, 보내기 단추는 후보가 열려 있어도 보내고, 빈 숫자 칸은 0 으로 저장하지 않으며,
//     '음소거로 시작'을 새 무대 패인에 적용하고, 홈 레일은 비면 내리고, 차단하면 멘션 모음에서도 뺀다.
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

/** 명령 인자를 window.__args[명령] 에 모은다. */
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

async function open({ url = '', extra = {}, width = 1280 } = {}) {
  const ctx = await browser.newContext({ viewport: { width, height: 900 }, colorScheme: 'dark', userAgent: MAC_UA });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, 'dark', 12]);
  await page.addInitScript(overlay, Object.fromEntries(Object.entries(extra).map(([k, v]) => [k, v.toString()])));
  await page.addInitScript(harness);
  await page.goto(base + url);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  return { ctx, page, errors };
}
const args = (page, cmd) => page.evaluate((c) => window.__args[c] || [], cmd);
const group = async (page, id) => {
  await go(page, 'settings');
  await page.waitForSelector('[data-page="settings"]:not([hidden]) [data-group]');
  await page.click(`[data-page="settings"] [data-group="${id}"]`);
  await page.waitForTimeout(150);
};

try {
  await check('T01 설정 — 읽히지 않던 항목·연동 탭이 없고 시도 횟수는 1부터', async () => {
    const { ctx, page, errors } = await open();
    await go(page, 'settings');
    await page.waitForSelector('[data-page="settings"]:not([hidden]) [data-group]');
    const groups = await page.$$eval('[data-page="settings"] [data-group]', (b) => b.map((x) => x.dataset.group));
    assert.ok(!groups.includes('link'), `'연동' 탭이 남았다: ${groups}`);
    const keys = new Set();
    for (const g of ['watch', 'multi', 'advanced']) {
      await page.click(`[data-page="settings"] [data-group="${g}"]`);
      await page.waitForTimeout(120);
      const adv = await page.$('[data-page="settings"] [data-adv]');
      if (adv && /고급 보기/.test(await adv.textContent())) { await adv.click(); await page.waitForTimeout(120); }
      for (const k of await page.$$eval('[data-page="settings"] [data-key]', (els) => els.map((e) => `${e.dataset.domain}.${e.dataset.key}`))) keys.add(k);
    }
    for (const dead of ['player.autoPlay', 'player.hardwareDecoding', 'player.catchupRate', 'network.connectionTimeout', 'network.streamConnectionTimeout',
      'network.maxConnectionsPerHost', 'network.autoReconnect', 'network.maxReconnectAttempts', 'network.reconnectBaseDelay', 'network.cacheExpiry',
      'multiLive.autoReconnect', 'multiLive.autoReconnectMaxRetries', 'sync.explicitlyOptedIn']) {
      assert.ok(!keys.has(dead), `${dead} 가 아직 보인다`);
    }
    assert.ok(keys.has('player.startMuted') && keys.has('network.retryCount'), '남겨야 할 항목이 빠졌다');
    await page.click('[data-page="settings"] [data-group="advanced"]');
    const min = await page.$eval('[data-page="settings"] [data-domain="network"][data-key="retryCount"]', (e) => e.min);
    assert.equal(min, '1');
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('T02 설정 — 네트워크 값을 손으로 바꾸면 프리셋 단추가 따라간다', async () => {
    const settings = () => {
      const s = window.__net14 || (window.__net14 = { preset: 'auto' });
      return { multiLive: {}, multiChat: {}, player: {}, chat: {}, general: {}, appearance: { theme: 'dark' }, network: { retryCount: 3, autoPreset: s.preset === 'auto' }, networkPreset: s.preset };
    };
    const { ctx, page, errors } = await open({
      extra: {
        settings_get: settings,
        settings_update: new Function(`return function ({ domain }) { const get = ${settings.toString()}; get(); if (domain === 'network') window.__net14.preset = 'custom'; return get(); }`)(),
      },
    });
    await group(page, 'advanced');
    const active = () => page.$$eval('[data-page="settings"] [data-net-preset].active', (b) => b.map((x) => x.dataset.netPreset));
    assert.deepEqual(await active(), ['auto']);
    await page.$eval('[data-page="settings"] [data-domain="network"][data-key="retryCount"]', (el) => { el.value = '5'; el.dispatchEvent(new Event('change', { bubbles: true })); });
    await page.waitForFunction(() => !document.querySelector('[data-page="settings"] [data-net-preset="auto"].active'));
    assert.deepEqual(await active(), [], '자동 단추가 켜진 채 남았다');
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('T03 설정 — 다른 곳에서 바꾼 테마를 개요의 테마 선택이 따라간다', async () => {
    const { ctx, page, errors } = await open({ extra: { appearance_set_theme: () => null } });
    await group(page, 'overview');
    await page.waitForSelector('[data-theme-choice]');
    await page.evaluate(() => import('/js/theme.js').then((m) => m.setTheme('light')));
    const on = await page.$$eval('[data-theme-choice].active', (b) => b.map((x) => x.dataset.themeChoice));
    assert.deepEqual(on, ['light']);
    assert.equal(await page.$eval('[data-theme-choice="light"]', (b) => b.getAttribute('aria-checked')), 'true');
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('T04 설정 — 금지어를 넣으면 금지어 필터도 켠다', async () => {
    const { ctx, page, errors } = await open();
    await group(page, 'chat');
    await page.fill('[data-page="settings"] [data-word-form] input', '광고');
    await page.click('[data-page="settings"] [data-word-form] button');
    await page.waitForFunction(() => (window.__args.settings_update || []).some((a) => a.domain === 'chat' && a.patch && a.patch.blockedWords));
    const patch = (await args(page, 'settings_update')).find((a) => a.domain === 'chat' && a.patch.blockedWords).patch;
    assert.deepEqual(patch.blockedWords, ['광고']);
    assert.equal(patch.chatFilterEnabled, true);
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('T05 멘션 — @도도새 는 도도를 부른 것이 아니다', async () => {
    const { ctx, page, errors } = await open();
    const r = await page.evaluate(() => import('/js/chat-prefs.js').then(({ mentionsMe }) => {
      const m = (content) => ({ content, nickname: '남' });
      return [
        mentionsMe(m('@도도 안녕'), '도도'), mentionsMe(m('안녕 @도도'), '도도'), mentionsMe(m('@도도! 왔어'), '도도'),
        mentionsMe(m('@도도새 안녕'), '도도'), mentionsMe(m('@도도1 ㅋ'), '도도'), mentionsMe(m('@도도새 @도도 둘 다'), '도도'),
      ];
    }));
    assert.deepEqual(r, [true, true, true, false, false, true]);
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('T06 보관함 — 이어 보기·나중에 보기에서 한 항목 빼기', async () => {
    const { ctx, page, errors } = await open({ extra: { vod_progress_remove: () => null, vod_watch_later_set: () => null } });
    await go(page, 'library', { tab: 'continue' });
    await page.waitForSelector('[data-page="library"] .media-card [data-lib="shelf-remove"]');
    const before = await page.$$eval('[data-page="library"] .media-card:not([hidden])', (c) => c.length);
    let opened = 0;
    await page.exposeFunction('__vodOpened', () => { opened += 1; });
    await page.evaluate(() => window.addEventListener('cview:open-vod', () => window.__vodOpened()));
    const card = page.locator('[data-page="library"] .media-card').first();
    await card.hover();
    await card.locator('[data-lib="shelf-remove"]').click();
    assert.equal(await page.$$eval('[data-page="library"] .media-card:not([hidden])', (c) => c.length), before - 1);
    await page.waitForFunction(() => (window.__args.vod_progress_remove || []).length > 0, null, { timeout: 12000 });
    assert.deepEqual((await args(page, 'vod_progress_remove')).map((a) => a.videoNo), [100]);
    assert.equal(opened, 0, '✕ 가 다시보기도 열었다');
    await go(page, 'library', { tab: 'later' });
    await page.waitForSelector('[data-page="library"] .media-card [data-shelf="later"]');
    const later = page.locator('[data-page="library"] .media-card').first();
    await later.hover();
    await later.locator('[data-lib="shelf-remove"]').click();
    await page.waitForFunction(() => (window.__args.vod_watch_later_set || []).length > 0, null, { timeout: 12000 });
    const w = (await args(page, 'vod_watch_later_set'))[0];
    assert.equal(w.videoNo, 100);
    assert.equal(w.on, false);
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('T07 카드 메뉴 — 방송 중이 아닌 채널 줄은 재생·멀티라이브·채팅을 띄우지 않는다', async () => {
    const { ctx, page, errors } = await open();
    await go(page, 'search', { query: '도' });
    await page.waitForSelector('[data-page="search"] .channel-row[data-open-live=""]');
    const items = async (sel) => {
      await page.click(sel, { button: 'right' });
      await page.waitForTimeout(150);
      const list = await page.evaluate(() => [...document.querySelectorAll('[role="menuitem"][data-ctx]')].filter((b) => b.offsetParent !== null).map((b) => b.textContent.trim()));
      await page.keyboard.press('Escape');
      await page.waitForTimeout(100);
      return list;
    };
    const off = await items('[data-page="search"] .channel-row[data-open-live=""]');
    assert.ok(off.length, '메뉴가 뜨지 않았다');
    for (const bad of ['재생', '멀티라이브에 추가', '채팅만 열기']) assert.ok(!off.includes(bad), `오프라인 채널 메뉴에 '${bad}': ${off}`);
    assert.ok(off.includes('채널 정보'));
    const on = await items('[data-page="search"] .channel-row[data-open-live="1"]');
    assert.ok(on.includes('재생'), `방송 중 채널 메뉴: ${on}`);
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('T08 코드 확인 — 자동 완성·보내기·빈 숫자 칸·음소거로 시작·홈 레일·차단 멘션', async () => {
    const composer = src('chat-composer.js');
    assert.match(composer, /m\[2\] === ':' && m\[3\]\.length < 2\) return null/);
    assert.match(composer, /acceptSubmit\(e\) \{[\s\S]*?e\.submitter[\s\S]*?setAutocompleteOpen\(false\)/);
    assert.match(src('chat.js'), /this\.composer\.acceptSubmit\(e\)\) this\.send\(\)/);
    assert.match(src('multichat.js'), /this\.composer\.acceptSubmit\(e\)\) this\.sendMerged\(\)/);
    assert.match(src('settings.js'), /target\.value\.trim\(\) === '' \? NaN : Number\(target\.value\)/);
    assert.match(src('app.js'), /await enterWatch\(s\.id\);\n\s+await applyStartMuted\(\);/);
    assert.match(src('home.js'), /cont\.status === 'fulfilled' && !cont\.value\.length\) clear\('continue'\)/);
    assert.match(src('chat.js'), /panel\.mentions = panel\.mentions\.filter\(\(m\) => !gone\.has\(m\.userId\)\)/);
  });
} finally {
  await browser.close();
  server.close();
}
console.log(`\n${total - failures.length}/${total} 통과${failures.length ? ` — 실패: ${failures.join(', ')}` : ''}`);
process.exit(failures.length ? 1 : 0);
