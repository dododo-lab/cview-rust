// 메뉴 막대(macOS)·알림 영역(Windows)으로 숨기기 설정과 아이콘 메뉴에 보낼 상태 검사 — 모의 IPC·헤드리스 브라우저.
// TR01 설정 › 일반 — '메뉴 막대에 표시' 다음에 '창을 닫으면 메뉴 막대로 숨기기'·'최소화하면 메뉴 막대로 숨기기'·
//      'Dock 아이콘 숨기기'. Dock 줄은 숨기기를 하나라도 켜야 쓸 수 있다. 켜고 끄면 general 패치로 저장한다.
// TR02 Windows — '알림 영역으로'(조사까지), Dock 줄은 없다.
// TR03 검색 — '트레이'·'메뉴바'·'백그라운드'로 찾아도 숨기기 줄이 나온다.
// TR04 지금 시청 중 — 플레이어 문서가 시스템 '지금 재생 중'을 올리면 tray_set_state 로 같은 정보를 보내고, 문서가
//      사라지면(iframe 제거) 다음 확인 때 지운다. 바뀌지 않으면 다시 보내지 않는다. 대상이 없으면 음소거도 null.
// TR05 axe(WCAG 2.1 A·AA) — 일반 탭 라이트·다크.
// 저장소 루트에서: node tools/ui-regression/tray.mjs   (Playwright·axe-core 가 필요하다. CVIEW_UI_BROWSER=webkit 이면 WebKit.
//   CVIEW_UI_SHOTS=<폴더> 면 일반 탭의 숨기기 줄을 PNG 로 남긴다.)
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, base, mockTauri, go } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const WEBKIT = process.env.CVIEW_UI_BROWSER === 'webkit';
const engine = WEBKIT ? pw.webkit : pw.chromium;
const AXE = fs.readFileSync(require.resolve('axe-core/axe.min.js'), 'utf8');
const SHOTS = process.env.CVIEW_UI_SHOTS;
if (SHOTS) fs.mkdirSync(SHOTS, { recursive: true });
const browser = await engine.launch();
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message.split('\n').slice(0, 8).join('\n') }); }
}

const S = '[data-page="settings"]';
const WIN_UA = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0';
const MAC_UA = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)';

/** 일반 설정 저장을 흉내 내고(다음 settings_get 이 돌려준다) tray_set_state 를 기록한다. */
function installTrayMocks() {
  const cfg = window.__tray = { general: { showInMenuBar: true }, calls: [] };
  const inv = window.__TAURI__.core.invoke;
  window.__TAURI__.core.invoke = async (c, a = {}) => {
    cfg.calls.push([c, a]);
    if (c === 'tray_set_state') return null;
    if (c === 'settings_get') { const s = await inv(c, a); return { ...s, general: { ...cfg.general } }; }
    if (c === 'settings_update') {
      if (a.domain === 'general') cfg.general = { ...cfg.general, ...a.patch };
      const s = await inv(c, a);
      return { ...s, general: { ...cfg.general } };
    }
    return inv(c, a);
  };
}

async function open({ theme = 'light', windows = false, settings = true } = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 }, colorScheme: theme, userAgent: windows ? WIN_UA : MAC_UA });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, theme]);
  await page.addInitScript(installTrayMocks);
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.waitForFunction(() => { const s = document.getElementById('splash'); return !s || !s.isConnected || getComputedStyle(s).display === 'none' || getComputedStyle(s).opacity === '0'; }, null, { timeout: 8000 }).catch(() => {});
  if (settings) {
    await go(page, 'settings');
    await page.waitForSelector(`${S} .settings-tabs`);
    await page.waitForTimeout(300);
    await page.click(`${S} .settings-tabs [data-group="general"]`);
    await page.waitForSelector(`${S} [data-key="showInMenuBar"]`, { state: 'attached' }); // 토글 input 은 보이지 않게 그린다
  }
  return { ctx, page, errors };
}

const labelOf = (page, key) => page.evaluate((k) => {
  const el = document.querySelector(`[data-page="settings"] input[data-domain="general"][data-key="${k}"]`);
  return el ? el.closest('.setting').querySelector('.setting-label').firstChild.textContent : null;
}, key);
const keysInOrder = (page) => page.evaluate(() => [...document.querySelectorAll('[data-page="settings"] .setting:not([hidden]) input[data-domain="general"]')].map((x) => x.dataset.key));
const lastPatch = (page) => page.evaluate(() => window.__tray.calls.filter(([c, a]) => c === 'settings_update' && a.domain === 'general').at(-1)?.[1].patch);

await check('TR01 macOS 설정 › 일반 — 숨기기 세 줄, Dock 줄은 숨기기를 켜야 쓸 수 있고, 켜고 끄면 저장', async () => {
  const { ctx, page, errors } = await open();
  const keys = await keysInOrder(page);
  const at = keys.indexOf('showInMenuBar');
  assert.deepEqual(keys.slice(at, at + 4), ['showInMenuBar', 'closeToTray', 'minimizeToTray', 'hideDockInTray']);
  assert.equal(await labelOf(page, 'showInMenuBar'), '메뉴 막대에 표시');
  assert.equal(await labelOf(page, 'closeToTray'), '창을 닫으면 메뉴 막대로 숨기기');
  assert.equal(await labelOf(page, 'minimizeToTray'), '최소화하면 메뉴 막대로 숨기기');
  assert.equal(await labelOf(page, 'hideDockInTray'), '숨겨 있는 동안 Dock 아이콘 숨기기');
  const help = await page.evaluate(() => document.querySelector('[data-page="settings"] [data-key="closeToTray"]').closest('.setting').textContent);
  assert.match(help, /재생·채팅·알림은 그대로/, '숨겨도 계속된다는 안내');
  assert.match(help, /CView 종료/, '끝내는 방법 안내');
  const dock = `${S} input[data-key="hideDockInTray"]`;
  assert.equal(await page.isDisabled(dock), true, '숨기기를 켜기 전에는 막힘');
  await page.click(`${S} input[data-key="minimizeToTray"] + span`);
  await page.waitForFunction(() => window.__tray.general.minimizeToTray === true);
  assert.deepEqual(await lastPatch(page), { minimizeToTray: true });
  await page.waitForFunction((s) => !document.querySelector(s).disabled, dock);
  await page.click(`${dock} + span`);
  await page.waitForFunction(() => window.__tray.general.hideDockInTray === true);
  assert.deepEqual(await lastPatch(page), { hideDockInTray: true });
  await page.click(`${S} input[data-key="closeToTray"] + span`);
  await page.waitForFunction(() => window.__tray.general.closeToTray === true);
  await page.click(`${S} input[data-key="minimizeToTray"] + span`);
  await page.click(`${S} input[data-key="closeToTray"] + span`);
  await page.waitForFunction(() => window.__tray.general.closeToTray === false && window.__tray.general.minimizeToTray === false);
  await page.waitForFunction((s) => document.querySelector(s).disabled, dock);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('TR02 Windows — 알림 영역으로(조사), Dock 줄 없음', async () => {
  const { ctx, page, errors } = await open({ windows: true });
  assert.equal(await labelOf(page, 'showInMenuBar'), '알림 영역에 표시');
  assert.equal(await labelOf(page, 'closeToTray'), '창을 닫으면 알림 영역으로 숨기기');
  assert.equal(await labelOf(page, 'minimizeToTray'), '최소화하면 알림 영역으로 숨기기');
  assert.equal(await page.locator(`${S} [data-key="hideDockInTray"]`).count(), 0);
  const help = await page.evaluate(() => document.querySelector('[data-page="settings"] [data-key="minimizeToTray"]').closest('.setting').textContent);
  assert.match(help, /작업 표시줄 대신/);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('TR03 검색 — 트레이·메뉴바·백그라운드로도 찾는다', async () => {
  const { ctx, page } = await open();
  for (const [q, key] of [['트레이', 'closeToTray'], ['메뉴바', 'minimizeToTray'], ['백그라운드', 'closeToTray'], ['숨기기', 'hideDockInTray']]) {
    await page.fill(`${S} .settings-search`, q);
    await page.waitForTimeout(120);
    assert.equal(await page.locator(`${S} .setting:not([hidden]) [data-key="${key}"]`).count(), 1, `"${q}" 로 ${key} 찾기`);
  }
  await ctx.close();
});

await check('TR04 지금 시청 중 — 올리면 보내고, 문서가 사라지면 지우고, 같으면 다시 보내지 않는다', async () => {
  const { ctx, page, errors } = await open({ settings: false });
  const sent = () => page.evaluate(() => window.__tray.calls.filter(([c]) => c === 'tray_set_state').map(([, a]) => a.state));
  // 시작 직후 한 번 — 아무것도 보지 않는다.
  await page.waitForFunction(() => window.__tray.calls.some(([c]) => c === 'tray_set_state'));
  assert.deepEqual((await sent()).at(-1), { nowPlaying: null, muted: null });
  await page.evaluate(async () => {
    const { applyNowPlaying } = await import('/js/now-playing.js');
    const f = document.createElement('iframe');
    f.id = 'tray-test-frame';
    document.body.append(f);
    applyNowPlaying(f.contentWindow, { kind: 'live', title: '저녁 방송', channelName: '채널', thumbnailUrl: '' });
    applyNowPlaying(f.contentWindow, { kind: 'live', title: '저녁 방송', channelName: '채널', thumbnailUrl: '' });
  });
  await page.waitForFunction(() => window.__tray.calls.filter(([c]) => c === 'tray_set_state').at(-1)[1].state.nowPlaying);
  const n = (await sent()).length;
  assert.deepEqual((await sent()).at(-1), { nowPlaying: { kind: 'live', title: '저녁 방송', channelName: '채널' }, muted: null });
  await page.waitForTimeout(2300); // 주기 확인 한 번 — 바뀐 것이 없으면 보내지 않는다
  assert.equal((await sent()).length, n, '같은 상태는 다시 보내지 않는다');
  await page.evaluate(() => document.getElementById('tray-test-frame').remove());
  await page.waitForFunction(() => window.__tray.calls.filter(([c]) => c === 'tray_set_state').at(-1)[1].state.nowPlaying === null, null, { timeout: 5000 });
  assert.deepEqual(errors, []);
  await ctx.close();
});

for (const theme of ['light', 'dark']) {
  await check(`TR05 axe — 일반 탭 ${theme}`, async () => {
    const { ctx, page } = await open({ theme });
    await page.click(`${S} input[data-key="closeToTray"] + span`);
    await page.waitForTimeout(200);
    if (SHOTS) {
      const card = page.locator(`${S} input[data-key="showInMenuBar"]`).locator('xpath=ancestor::*[contains(@class,"settings-group")][1]');
      await card.scrollIntoViewIfNeeded();
      await card.screenshot({ path: path.join(SHOTS, `tray-settings-${theme}-${WEBKIT ? 'webkit' : 'chromium'}.png`) });
    }
    await page.addScriptTag({ content: AXE });
    const v = await page.evaluate(async () => {
      const r = await window.axe.run(document.querySelector('[data-page="settings"]'), { runOnly: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'] });
      return r.violations.map((x) => `${x.id}: ${x.nodes.slice(0, 3).map((n) => n.target.join(' ')).join(' | ')}`);
    });
    assert.deepEqual(v, []);
    await ctx.close();
  });
}

await browser.close();
server.close();
let failed = 0;
for (const r of results) {
  console.log(`${r.ok ? '✓' : '✗'} ${r.id}`);
  if (!r.ok) { failed += 1; console.log(`    ${r.error.replace(/\n/g, '\n    ')}`); }
}
console.log(`\n${results.length - failed}/${results.length} 통과 (${WEBKIT ? 'WebKit' : 'Chromium'})`);
process.exit(failed ? 1 : 0);
