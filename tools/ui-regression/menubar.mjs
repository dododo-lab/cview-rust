// macOS 메뉴 막대 드롭다운 팝오버(menubar.html) + 메인 창 제어 브리지(tray://control) UI/UX·접근성 회귀 검사.
// MB01 머리줄 라이브 배지·포인터 위치·지금 시청 중 미니 컨트롤러·빠른 동작 6타일 표시
// MB02 미니 컨트롤러 — 재생/일시정지·음소거·볼륨 슬라이더·PiP·시청 화면 열기 명령 전달
// MB03 빠른 동작 & 토글 — 창 열기/숨기기·TOP 4 멀티라이브·무작위 방송·알림 토글 저장·다크/라이트 테마 전환
// MB04 라이브 탐색 — 즐겨찾기 우선 정렬, 탭 화살표 전환, / 검색 필터·Esc 지우기·재Esc 닫기, ↑/↓ 이동, 시청/멀티추가/채팅창
// MB05 비로그인 상태 — 로그인 안내 CTA 및 인기 라이브 탭 전환
// MB06 axe-core(WCAG 2.1 A·AA, color-contrast 포함) — 다크·라이트 위반 0건
// MB07 메인 창 tray://control — 창에 초점이 없어도 화면 이동·멀티 큐 동작 실행
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, base, mockTauri, openApp } from './mock-app.mjs';

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
  try {
    await fn();
    results.push({ id, ok: true });
  } catch (e) {
    results.push({ id, ok: false, error: e.message.split('\n').slice(0, 8).join('\n') });
  }
}

function installMenubarMocks({ loggedIn = true, windowHidden = true, nowPlaying = true, theme = 'dark' } = {}) {
  const img = `${location.origin || ''}`;
  void img;
  const state = window.__mb = {
    controls: [],
    closed: 0,
    themes: [],
    general: { showInMenuBar: true, notificationsEnabled: true },
    snap: {
      open: true,
      loggedIn,
      windowHidden,
      arrowX: 248,
      front: nowPlaying ? {
        nowPlaying: {
          kind: 'live',
          title: '저녁 마인크래프트 생존기',
          channelName: '릴카',
          channelId: 'ch0',
          category: '마인크래프트',
          thumbnailUrl: '',
          imageUrl: '',
        },
        muted: false,
        paused: false,
        volume: 75,
        pip: false,
      } : { nowPlaying: null, muted: null, paused: null, volume: null, pip: null },
    },
    listeners: new Map(),
  };
  const origInvoke = window.__TAURI__.core.invoke;
  window.__TAURI__.core.invoke = async (cmd, args = {}) => {
    if (cmd === 'tray_get_state') return structuredClone(state.snap);
    if (cmd === 'following_lives') { state.followingCalls = (state.followingCalls || 0) + 1; if (window.__followFail) throw new Error('오프라인'); }
    if (cmd === 'tray_popover_close') { state.closed += 1; return null; }
    if (cmd === 'tray_control') { state.controls.push(args.action); return null; }
    if (cmd === 'appearance_set_theme') { state.themes.push(args.theme); return null; }
    if (cmd === 'multilive_state') return { sessions: [{ id: 's1', channelId: 'ch1', slot: 0 }], layout: 'grid' };
    if (cmd === 'auth_status') return loggedIn ? { loggedIn: true, state: 'loggedIn', nickname: '도도' } : { loggedIn: false, state: 'loggedOut' };
    if (cmd === 'settings_get') {
      const s = await origInvoke(cmd, args);
      return { ...s, general: { ...state.general }, appearance: { ...s.appearance, theme } };
    }
    if (cmd === 'settings_update') {
      if (args.domain === 'general') state.general = { ...state.general, ...args.patch };
      const s = await origInvoke('settings_get', {});
      return { ...s, general: { ...state.general }, appearance: { ...s.appearance, theme } };
    }
    return origInvoke(cmd, args);
  };
  const bindListen = async (event, cb) => {
    const list = state.listeners.get(event) || [];
    list.push(cb);
    state.listeners.set(event, list);
    return () => {};
  };
  window.__TAURI__.event = { listen: bindListen };
  window.__TAURI__.webviewWindow = {
    getCurrentWebviewWindow: () => ({ listen: bindListen }),
  };
  window.__emitMenubar = (event, payload) => {
    for (const cb of state.listeners.get(event) || []) cb({ payload });
  };
}

async function openPopover(opts = {}) {
  const theme = opts.theme || 'dark';
  const ctx = await browser.newContext({ viewport: { width: 384, height: 584 }, colorScheme: theme });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, theme, 10]);
  await page.addInitScript(installMenubarMocks, { loggedIn: true, windowHidden: true, nowPlaying: true, ...opts });
  await page.goto(`${base}menubar.html`);
  await page.waitForSelector('.mb-popover');
  await page.waitForFunction(() => !document.querySelector('.mb-empty')?.textContent?.includes('불러오는 중'));
  return { ctx, page, errors };
}

await check('MB01 머리줄·포인터·지금 시청 중·빠른 동작 타일 초기 표시', async () => {
  const { ctx, page, errors } = await openPopover();
  const arrowX = await page.$eval('#mb-stage', (el) => el.style.getPropertyValue('--arrow-x'));
  assert.equal(arrowX, '248px');
  assert.equal(await page.textContent('#mb-live-count'), '팔로잉 방송 중 6');
  assert.equal(await page.isHidden('#mb-now'), false);
  assert.equal(await page.textContent('#mb-now-channel'), '릴카');
  assert.equal(await page.textContent('#mb-vol-val'), '75%');
  assert.equal(await page.textContent('#mb-tile-window-label'), 'CView 열기');
  assert.equal(await page.textContent('#mb-tile-ml-sub'), '1패인 실행 중');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('MB02 지금 시청 중 미니 컨트롤러 — 재생·음소거·볼륨·PiP·창 이동', async () => {
  const { ctx, page, errors } = await openPopover();
  await page.click('#mb-play');
  assert.equal(await page.getAttribute('#mb-play', 'aria-label'), '재생');
  await page.click('#mb-mute');
  assert.equal(await page.getAttribute('#mb-mute', 'aria-label'), '음소거 해제');
  await page.click('#mb-pip');
  assert.equal(await page.getAttribute('#mb-pip', 'aria-pressed'), 'true');
  await page.evaluate(() => {
    const v = document.getElementById('mb-volume');
    v.value = '42';
    v.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await page.waitForTimeout(80);
  await page.click('#mb-now-open');
  const actions = await page.evaluate(() => window.__mb.controls);
  assert.deepEqual(actions, [
    { kind: 'play-pause' },
    { kind: 'mute' },
    { kind: 'pip' },
    { kind: 'volume', value: 42 },
    { kind: 'show-main' },
  ]);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('MB03 빠른 동작 & 토글 — 창 토글·TOP 4 큐·무작위 방송·알림 저장·테마 전환', async () => {
  const { ctx, page, errors } = await openPopover({ theme: 'dark' });
  await page.click('#mb-tile-window');
  await page.click('#mb-tile-queue');
  await page.click('[data-act="random-live"]');
  await page.click('#mb-tile-notify');
  assert.equal(await page.getAttribute('#mb-tile-notify', 'aria-pressed'), 'false');
  assert.equal(await page.evaluate(() => window.__mb.general.notificationsEnabled), false);
  await page.click('#mb-tile-theme');
  assert.equal(await page.evaluate(() => document.documentElement.dataset.theme), 'light');
  const actions = await page.evaluate(() => window.__mb.controls);
  assert.equal(actions[0].kind, 'toggle-window');
  assert.deepEqual(actions[1], { kind: 'queue-multi', channelIds: ['ch0', 'ch1', 'ch2', 'ch3'] });
  assert.equal(actions[2].kind, 'open-live');
  assert.ok(actions[2].channelId.startsWith('ch'));
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('MB04 라이브 탐색 — 즐겨찾기 우선·탭 전환·검색·키보드 내비게이션·채널별 3동작', async () => {
  const { ctx, page, errors } = await openPopover();
  const groups = await page.$$eval('.mb-group-label', (els) => els.map((e) => e.textContent));
  assert.deepEqual(groups, ['즐겨찾기 · 3', '팔로잉 · 3']);
  assert.equal(await page.locator('.mb-item.is-watching[data-channel-id="ch0"]').count(), 1);
  assert.equal(await page.locator('.mb-item[data-channel-id="ch1"] .mb-act-btn.is-in-multi').count(), 1);

  // / 단축키로 검색 초점 → 필터 → ArrowDown 으로 첫 결과 초점 → Esc 로 검색 지우기 → 두 번째 Esc 로 팝오버 닫기
  await page.keyboard.press('/');
  assert.equal(await page.evaluate(() => document.activeElement?.id), 'mb-search');
  await page.fill('#mb-search', '침착맨');
  assert.equal(await page.locator('.mb-item').count(), 1);
  assert.equal(await page.locator('.mb-item').getAttribute('data-channel-id'), 'ch4');
  await page.keyboard.press('ArrowDown');
  assert.equal(await page.evaluate(() => document.activeElement?.dataset.channelId), 'ch4');
  await page.keyboard.press('Escape');
  assert.equal(await page.inputValue('#mb-search'), '');
  assert.equal(await page.locator('.mb-item').count(), 6);
  await page.keyboard.press('Escape');
  assert.equal(await page.evaluate(() => window.__mb.closed), 1);

  // 탭 화살표·Home·End 전환 및 로빙 tabindex(팔로잉 → 인기)
  assert.equal(await page.getAttribute('#mb-tab-following', 'tabindex'), '0');
  assert.equal(await page.getAttribute('#mb-tab-popular', 'tabindex'), '-1');
  await page.focus('#mb-tab-following');
  await page.keyboard.press('ArrowRight');
  assert.equal(await page.getAttribute('#mb-tab-popular', 'aria-selected'), 'true');
  assert.equal(await page.getAttribute('#mb-tab-popular', 'tabindex'), '0');
  assert.equal(await page.getAttribute('#mb-tab-following', 'tabindex'), '-1');
  assert.equal(await page.locator('.mb-item').count(), 10);
  await page.keyboard.press('Home');
  assert.equal(await page.getAttribute('#mb-tab-following', 'aria-selected'), 'true');
  await page.keyboard.press('End');
  assert.equal(await page.getAttribute('#mb-tab-popular', 'aria-selected'), 'true');

  // 채널 줄 동작 3종(시청 / 멀티라이브 추가 / 이미 담긴 채널은 멀티라이브 보기 / 독립 채팅 창)
  await page.click('.mb-item[data-channel-id="ch2"] [data-item-act="watch"]');
  await page.click('.mb-item[data-channel-id="ch2"] [data-item-act="multi"]');
  assert.equal(await page.locator('.mb-item[data-channel-id="ch2"] .mb-act-btn.is-in-multi').count(), 1);
  await page.click('.mb-item[data-channel-id="ch1"] [data-item-act="multi"]');
  await page.click('.mb-item[data-channel-id="ch2"] [data-item-act="chat"]');
  const tail = await page.evaluate(() => window.__mb.controls.slice(-4));
  assert.deepEqual(tail, [
    { kind: 'open-live', channelId: 'ch2' },
    { kind: 'add-multilive', channelId: 'ch2' },
    { kind: 'open-multilive' },
    { kind: 'open-chat-window', channelId: 'ch2' },
  ]);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('MB05 비로그인 상태 — 로그인 안내·인기 탭 전환 및 로그인 직후 즉시 팔로잉 전환', async () => {
  const { ctx, page, errors } = await openPopover({ loggedIn: false, nowPlaying: false });
  assert.equal(await page.isHidden('#mb-now'), true);
  assert.match(await page.textContent('#mb-list'), /로그인이 필요합니다/);
  await page.click('[data-act="login"]');
  assert.deepEqual(await page.evaluate(() => window.__mb.controls), [{ kind: 'login' }]);
  await page.click('[data-act="switch-popular"]');
  assert.equal(await page.getAttribute('#mb-tab-popular', 'aria-selected'), 'true');
  assert.equal(await page.locator('.mb-item').count(), 10);

  // 다시 팔로잉 탭으로 둔 뒤 로그인 성공 이벤트가 오면, 트레이 주기 스냅샷이 아직 비로그인 값이어도 즉시 팔로잉 목록을 표시해야 한다.
  await page.click('#mb-tab-following');
  assert.match(await page.textContent('#mb-list'), /로그인이 필요합니다/);
  await page.evaluate(() => {
    // tray_get_state 의 snap.loggedIn 은 의도적으로 false 인 채로 두어 레이스를 재현한다.
    window.__emitMenubar('auth://changed', { loggedIn: true, state: 'loggedIn', nickname: '도도' });
  });
  await page.waitForFunction(() => document.querySelectorAll('#mb-list .mb-item').length === 6);
  assert.equal(await page.textContent('#mb-live-count'), '팔로잉 방송 중 6');
  assert.doesNotMatch(await page.textContent('#mb-list'), /로그인이 필요합니다/);
  assert.deepEqual(errors, []);
  await ctx.close();
});

for (const theme of ['dark', 'light']) {
  await check(`MB06 axe WCAG 2.1 A·AA (${theme})`, async () => {
    const { ctx, page } = await openPopover({ theme });
    if (SHOTS) {
      await page.screenshot({ path: path.join(SHOTS, `menubar-popover-${theme}-${WEBKIT ? 'webkit' : 'chromium'}.png`), omitBackground: true });
    }
    await page.addScriptTag({ content: AXE });
    const violations = await page.evaluate(async () => {
      const r = await window.axe.run(document.getElementById('mb-popover'), {
        runOnly: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'best-practice'],
      });
      return r.violations.map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(' ')).join(' | ')}`);
    });
    assert.deepEqual(violations, []);
    await ctx.close();
  });
}

await check('MB08 팔로잉 받기 실패는 "방송 중인 팔로잉 없음"이 아니라 실패로 알리고, 다시 열면 다시 받는다(12차)', async () => {
  const ctx = await browser.newContext({ viewport: { width: 384, height: 584 }, colorScheme: 'dark' });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  await page.addInitScript(mockTauri, [base, 'dark', 10]);
  await page.addInitScript(installMenubarMocks, { loggedIn: true, windowHidden: true, nowPlaying: true });
  await page.addInitScript(() => { window.__followFail = true; });
  await page.goto(`${base}menubar.html`);
  await page.waitForSelector('.mb-popover');
  await page.waitForFunction(() => !document.querySelector('.mb-empty')?.textContent?.includes('불러오는 중'));
  const text = await page.textContent('#mb-list');
  assert.ok(text.includes('불러오지 못했습니다'), text);
  assert.ok(!text.includes('방송 중인 팔로잉 채널이 없습니다'), text);
  const before = await page.evaluate(() => window.__mb.followingCalls);
  // 네트워크가 돌아온 뒤 팝오버를 닫았다 다시 열기(30초 안) — 실패한 받기를 '방금 받음'으로 치지 않는다
  await page.evaluate(() => {
    window.__followFail = false;
    window.__emitMenubar('tray://state', { ...window.__mb.snap, open: false });
    window.__emitMenubar('tray://state', { ...window.__mb.snap, open: true });
  });
  await page.waitForFunction((n) => window.__mb.followingCalls > n, before);
  await page.waitForFunction(() => document.querySelectorAll('#mb-list [data-channel-id], #mb-list .mb-item').length > 0);
  await ctx.close();
});

await check('MB07 메인 창 tray://control — 초점 유무와 관계없이 화면 이동 실행', async () => {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 860 }, colorScheme: 'dark' });
  const page = await ctx.newPage();
  await page.addInitScript(mockTauri, [base, 'dark', 8]);
  await page.addInitScript(() => {
    const listeners = new Map();
    const reg = async (ev, cb) => {
      const l = listeners.get(ev) || [];
      l.push(cb);
      listeners.set(ev, l);
      return () => {};
    };
    window.__TAURI__.event = { listen: reg };
    window.__TAURI__.webviewWindow = { getCurrentWebviewWindow: () => ({ listen: reg }) };
    window.__emitHere = (ev, payload) => { for (const cb of listeners.get(ev) || []) cb({ payload }); };
  });
  await page.goto(base);
  await page.waitForSelector('[data-page="settings"]', { state: 'attached' });
  await page.waitForFunction(() => {
    const s = document.getElementById('splash');
    return !s || !s.isConnected || getComputedStyle(s).display === 'none' || getComputedStyle(s).opacity === '0';
  }, null, { timeout: 8000 }).catch(() => {});
  await page.evaluate(() => {
    document.hasFocus = () => false;
    window.__emitHere('tray://control', { kind: 'navigate', route: 'settings', params: {} });
  });
  await page.waitForFunction(() => !document.querySelector('[data-page="settings"]').hidden);
  await ctx.close();
});

await browser.close();
server.close();
let failed = 0;
for (const r of results) {
  console.log(`${r.ok ? '✓' : '✗'} ${r.id}`);
  if (!r.ok) {
    failed += 1;
    console.log(`    ${r.error.replace(/\n/g, '\n    ')}`);
  }
}
console.log(`\n${results.length - failed}/${results.length} 통과 (${WEBKIT ? 'WebKit' : 'Chromium'})`);
process.exit(failed ? 1 : 0);
