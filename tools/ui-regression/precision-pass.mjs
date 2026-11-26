// 기능 정밀 고도화(2026-11-26) 회귀 검사 — 감사 문서에 남아 있던 실제 결함을 고친 것.
// 모의 IPC·헤드리스 브라우저(실제 서비스·데이터는 건드리지 않는다). CVIEW_UI_BROWSER=webkit 이면 WebKit.
// 저장소 루트에서: node tools/ui-regression/precision-pass.mjs
// PP01 ⇧ 하나뿐인 사용자 단축키(⇧C 채팅 방식 등)를 시청 화면이 받는다 — 예전엔 저장돼도 아무 데서도 받지 않았다.
//      맨 글자 바인딩은 ⇧ 를 누르면 맞지 않고, ⌘·⌥ 조합은 예전처럼 메뉴 몫(메뉴 동작)이다.
// PP02 좁은 창에서 직접 접어 둔 채팅은 '채팅' 한 번에 열린다(예전엔 두 번).
// PP03 패인이 5개 이상이면 '사용자' 배치는 aria-disabled, 누르면 안내만(배치 명령 없음), 보이는 배치(그리드)가 켜져 보인다.
// PP04 보조 창(플레이어 창)에는 채널 화면·카드의 '멀티라이브에 추가'가 없다 — 누르면 그 창이 멀티라이브로 바뀌었다.
// PP05 라이트(직접·시스템)에서 인사이트 첫 타일 숫자는 단색 브랜드 글자색(대비 4.5:1 이상) — 그라디언트 글자는 1.8~2.5:1 이었다.
// PP06 설정 — 테마·고급 펼치기를 키보드로 누르면 다시 그린 뒤에도 그 단추에 초점(예전엔 문서 처음으로 떨어졌다).
// PP07 채팅 사용자 말풍선 — 키보드로 '하이라이트'·'팔로우'를 누르거나 프로필이 늦게 와 다시 그려도 초점이 그 단추에 남는다.
// PP08 효과 없던 설정(메모리 목표·오프라인이면 새로고침 멈춤·백그라운드 활동 줄이기)은 설정 화면에 없다.
// PP09 멀티채팅 통합 입력의 보낼 채널 선택 상자에 이름(aria-label)이 있다.
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { base, server, mockTauri, go } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const WEBKIT = process.env.CVIEW_UI_BROWSER === 'webkit';
const browser = await (WEBKIT ? pw.webkit : pw.chromium).launch();
const failures = [];
let total = 0;
async function check(name, fn) {
  total += 1;
  try { await fn(); console.log(`ok   ${name}`); } catch (e) { failures.push(name); console.log(`FAIL ${name}\n     ${e.message.split('\n').join('\n     ')}`); }
}

/** 모의 앱 위에 명령을 덧씌우고(extra) 부른 명령을 기록한다. */
function overlay(extra) {
  window.__ppCalls = [];
  const fns = Object.fromEntries(Object.entries(extra).map(([k, v]) => [k, new Function(`return (${v})`)()]));
  const inv = window.__TAURI__.core.invoke;
  window.__TAURI__.core.invoke = async (c, a) => { window.__ppCalls.push([c, a]); return fns[c] ? fns[c](a || {}) : inv(c, a); };
}

async function open({ width = 1280, height = 900, theme = 'dark', query = '', extra = {}, init = null } = {}) {
  const ctx = await browser.newContext({ viewport: { width, height }, colorScheme: theme === 'system-light' ? 'light' : theme });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, theme === 'system-light' ? 'system' : theme]);
  await page.addInitScript(overlay, Object.fromEntries(Object.entries(extra).map(([k, v]) => [k, v.toString()])));
  if (init) await page.addInitScript(init);
  await page.goto(`${base}${query}`);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
  await page.waitForFunction(() => { const s = document.getElementById('splash'); return !s || !s.isConnected || getComputedStyle(s).display === 'none' || getComputedStyle(s).opacity === '0'; }, null, { timeout: 8000 }).catch(() => {});
  return { ctx, page, errors };
}
const settle = (page, ms = 250) => page.waitForTimeout(ms);

/** sRGB 대비 — 'rgb(r, g, b)' 두 개. */
function contrast(a, b) {
  const lum = (c) => {
    const [r, g, bl] = c.match(/[\d.]+/g).slice(0, 3).map(Number).map((v) => { const s = v / 255; return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4; });
    return 0.2126 * r + 0.7152 * g + 0.0722 * bl;
  };
  const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
}

try {
  await check('PP01 ⇧ 단축키', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async () => {
      const s = await import('/js/shortcuts.js');
      const ev = (key, code, o = {}) => new KeyboardEvent('keydown', { key, code, ...o });
      s.applyKeyboardSettings({ bindings: ['toggleChat', { key: 'c', modifiers: s.MOD.shift }, 'togglePiP', { key: 'p', modifiers: 0 }, 'toggleFullscreen', { key: 'f', modifiers: s.MOD.shift }] });
      const out = {
        shiftC: s.matchPlainAction(ev('C', 'KeyC', { shiftKey: true })),
        plainC: s.matchPlainAction(ev('c', 'KeyC')),
        shiftF: s.matchPlainAction(ev('F', 'KeyF', { shiftKey: true })),
        plainP: s.matchPlainAction(ev('p', 'KeyP')),
        shiftP: s.matchPlainAction(ev('P', 'KeyP', { shiftKey: true })),
        cmdShiftC: s.matchPlainAction(ev('c', 'KeyC', { shiftKey: true, metaKey: true })),
      };
      // ⌘⇧ 바인딩은 메뉴가 없는 동작만 여기서 받는다(예전 그대로).
      s.applyKeyboardSettings({ bindings: ['toggleChat', { key: 'y', modifiers: s.MOD.cmd | s.MOD.shift }, 'screenshot', { key: 'u', modifiers: s.MOD.cmd | s.MOD.shift }] });
      out.cmdShiftY = s.matchPlainAction(ev('y', 'KeyY', { shiftKey: true, metaKey: true }));
      out.cmdShiftU = s.matchPlainAction(ev('u', 'KeyU', { shiftKey: true, metaKey: true }));
      s.applyKeyboardSettings({ bindings: [] });
      return out;
    });
    assert.deepEqual(r, { shiftC: 'toggleChat', plainC: null, shiftF: 'toggleFullscreen', plainP: 'togglePiP', shiftP: null, cmdShiftC: null, cmdShiftY: 'toggleChat', cmdShiftU: null });
    await ctx.close();
  });

  await check('PP02 좁은 창에서 접어 둔 채팅은 한 번에 열린다', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async () => {
      const { installChatSplitter } = await import('/js/splitter.js');
      localStorage.removeItem('cview.split.pp');
      const container = document.createElement('div');
      container.style.cssText = 'position:fixed;left:0;top:0;width:1000px;height:400px;display:flex;z-index:99';
      container.innerHTML = '<div style="flex:1"></div><section class="chat"><header class="chat-header"></header></section>';
      document.body.append(container);
      const chat = container.querySelector('.chat');
      installChatSplitter({ container, chat, key: 'pp' });
      const frame = () => new Promise((res) => requestAnimationFrame(() => requestAnimationFrame(res)));
      await frame();
      chat.querySelector('.chat-collapse, .chat-header button').click(); // 넓은 창에서 직접 접기
      await frame();
      const collapsedWide = chat.classList.contains('collapsed');
      container.style.width = '600px'; // 채팅이 저절로 접히는 폭
      await frame();
      container.querySelector('.chat-reopen').click();
      await frame();
      return { collapsedWide, afterOne: chat.classList.contains('collapsed'), reopenHidden: container.querySelector('.chat-reopen').hidden };
    });
    assert.deepEqual(r, { collapsedWide: true, afterOne: false, reopenHidden: true });
    await ctx.close();
  });

  await check('PP03 패인 5개 이상이면 사용자 배치는 막히고 안내만', async () => {
    const sessions = Array.from({ length: 5 }, (_, i) => ({ id: `s${i}`, slot: i + 1, channelId: `ch${i}`, channelName: `이름${i}`, liveTitle: `방송${i}`, state: 'playing', inMultiView: true }));
    const { ctx, page, errors } = await open({
      extra: {
        multilive_state: `() => ({ sessions: ${JSON.stringify(sessions)}, selectedId: 's0', canAdd: true, maxSessions: 6, layout: 'custom' })`,
        settings_get: `() => ({ multiLive: { maxConcurrentSessions: 6, maxLiveChannels: 6 }, multiChat: {}, player: {}, chat: {}, general: {}, appearance: { theme: 'dark' } })`,
        multilive_set_layout: '() => null',
      },
    });
    await go(page, 'multilive');
    await settle(page, 800);
    const r = await page.evaluate(() => {
      const root = document.querySelector('.page-multilive');
      const btn = (l) => root.querySelector(`[data-layout="${l}"]`);
      const out = { panes: root.querySelectorAll('.ml-pane').length, disabled: btn('custom').getAttribute('aria-disabled'), active: [...root.querySelectorAll('[data-layout].active')].map((b) => b.dataset.layout) };
      window.__ppCalls.length = 0;
      btn('custom').click();
      return out;
    });
    await settle(page, 150);
    const after = await page.evaluate(() => ({
      calls: window.__ppCalls.filter(([c]) => c === 'multilive_set_layout').length,
      toast: [...document.querySelectorAll('.toast')].map((t) => t.textContent).join(' '),
    }));
    assert.equal(r.disabled, 'true', `패인 ${r.panes}개`);
    assert.deepEqual(r.active, ['grid'], '보이는 배치(그리드)가 켜져 보인다');
    assert.equal(after.calls, 0, '배치 명령을 보냈다');
    assert.match(after.toast, /사용자 배치는 패인 4개까지/);
    // 그리드는 그대로 고를 수 있다
    await page.evaluate(() => { window.__ppCalls.length = 0; document.querySelector('.page-multilive [data-layout="focus"]').click(); });
    await settle(page, 100);
    assert.equal(await page.evaluate(() => window.__ppCalls.filter(([c, a]) => c === 'multilive_set_layout' && a.layout === 'focus').length), 1);
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('PP04 플레이어 창에는 멀티라이브에 추가가 없다', async () => {
    const live = { liveTitle: '지금 방송', categoryName: '게임', viewerCount: 100, openDate: new Date().toISOString(), isLive: true };
    const extra = { channel_detail: `() => ({ channel: { channelId: 'ch1', channelName: '플러리', channelImageUrl: null, followerCount: 1, openLive: true }, live: ${JSON.stringify(live)}, isFavorite: false })` };
    const shown = {};
    for (const [name, query] of [['main', ''], ['player', '?w=player']]) {
      const { ctx, page } = await open({ query, extra });
      await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:open-channel', { detail: { channelId: 'ch1' } })));
      await page.waitForSelector('[data-ch="play"]', { state: 'attached' }).catch(() => {});
      await settle(page, 300);
      shown[name] = await page.evaluate(() => {
        const card = document.createElement('div');
        card.innerHTML = '<article data-channel-id="ch2"><button type="button" class="card-ml-add" data-ml-add>+</button></article>';
        document.querySelector('[data-page]:not([hidden])')?.append(card);
        const plus = card.querySelector('[data-ml-add]');
        return { multi: Boolean(document.querySelector('[data-ch="multi"]')), play: Boolean(document.querySelector('[data-ch="play"]')), plus: getComputedStyle(plus).display !== 'none' };
      });
      await ctx.close();
    }
    assert.deepEqual(shown.main, { multi: true, play: true, plus: true });
    assert.deepEqual(shown.player, { multi: false, play: true, plus: false });
  });

  await check('PP05 라이트 인사이트 첫 타일 숫자 대비', async () => {
    for (const theme of ['light', 'system-light', 'dark']) {
      const { ctx, page } = await open({ theme });
      const r = await page.evaluate(() => {
        const wrap = document.createElement('div');
        wrap.innerHTML = '<div class="ip-tiles"><div class="ip-tile"><strong>12시간</strong><span>이번 주</span></div><div class="ip-tile"><strong>3</strong></div></div>';
        document.body.append(wrap);
        const tile = wrap.querySelector('.ip-tile');
        const cs = getComputedStyle(tile.querySelector('strong'));
        return { color: cs.color, clip: cs.backgroundClip || cs.webkitBackgroundClip, image: cs.backgroundImage, bg: getComputedStyle(tile).backgroundColor };
      });
      if (theme === 'dark') {
        assert.notEqual(r.image, 'none', '다크는 그라디언트 그대로');
      } else {
        assert.equal(r.image, 'none', `${theme}: 그라디언트 글자`);
        const c = contrast(r.color, r.bg);
        assert.ok(c >= 4.5, `${theme}: 대비 ${c.toFixed(2)} (${r.color} on ${r.bg})`);
      }
      await ctx.close();
    }
  });

  await check('PP06 설정 — 테마·고급 펼치기를 키보드로 눌러도 초점이 남는다', async () => {
    const { ctx, page } = await open();
    await go(page, 'settings');
    await page.waitForSelector('[data-theme-choice]');
    await page.keyboard.press('Shift');
    await page.focus('[data-theme-choice="light"]');
    await page.keyboard.press('Space');
    await settle(page, 200);
    const t = await page.evaluate(() => ({ choice: document.activeElement?.dataset.themeChoice, checked: document.querySelector('[data-theme-choice="light"]').getAttribute('aria-checked') }));
    assert.deepEqual(t, { choice: 'light', checked: 'true' });
    // 고급 보기 — 그 단추에 초점이 남는다(다시 그린 새 단추)
    const adv = await page.$('[data-adv]');
    if (adv) {
      await adv.focus();
      await page.keyboard.press('Enter');
      await settle(page, 150);
      assert.equal(await page.evaluate(() => document.activeElement?.hasAttribute('data-adv')), true, '고급 펼치기 뒤 초점');
    }
    await ctx.close();
  });

  await check('PP07 채팅 사용자 말풍선 — 다시 그려도 초점이 그 단추에', async () => {
    const { ctx, page, errors } = await open({
      extra: {
        chat_user_card: `async () => { await new Promise((r) => setTimeout(r, 300)); return { channel: { channelId: 'u1', channelName: '유저', followerCount: 3 }, following: false, profile: null }; }`,
        channel_follow: `async () => { await new Promise((r) => setTimeout(r, 150)); return null; }`,
        settings_update: `() => null`,
      },
    });
    const r = await page.evaluate(async () => {
      const { openUserPop } = await import('/js/chat-user-pop.js');
      const host = document.createElement('div');
      host.innerHTML = '<div class="chat-list"><div class="chat-line" data-uid="u1" data-nick="유저"><span class="nick">유저</span> 안녕</div></div>';
      document.body.append(host);
      const line = host.querySelector('.chat-line');
      const panel = { input: { disabled: false }, channelId: 'c0', chatChannelId: 'cc0', soloUid: null, list: host.querySelector('.chat-list'), isLoggedIn: () => true, toggleHighlight() {}, setSolo() {}, mention() {}, showProfile() {}, block() {} };
      openUserPop(panel, line.querySelector('.nick'), line);
      const pop = document.querySelector('.user-pop');
      const wait = (ms) => new Promise((res) => setTimeout(res, ms));
      // 프로필이 오기 전에 키보드로 '하이라이트' 에 둔다 → 프로필 도착으로 다시 그려도 그대로
      pop.querySelector('[data-up="highlight"]').focus();
      await wait(450);
      const afterLoad = document.activeElement?.dataset.up;
      pop.querySelector('[data-up="highlight"]').click(); // 다시 그림
      const afterHighlight = document.activeElement?.dataset.up;
      const follow = pop.querySelector('[data-up="follow"]');
      follow.focus();
      follow.click(); // 바쁨(disabled) → 끝나면 언팔로우 단추
      await wait(400);
      return { afterLoad, afterHighlight, afterFollow: document.activeElement?.dataset.up, inPop: pop.contains(document.activeElement) };
    });
    assert.deepEqual(r, { afterLoad: 'highlight', afterHighlight: 'highlight', afterFollow: 'unfollow', inPop: true });
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('PP08 효과 없던 설정 세 줄이 설정 화면에 없다', async () => {
    const { ctx, page } = await open();
    await go(page, 'settings');
    await page.waitForSelector('[data-theme-choice]');
    const text = await page.evaluate(async () => {
      const groups = [...document.querySelectorAll('[data-group]')].map((g) => g.dataset.group);
      let all = '';
      for (const g of groups) {
        document.querySelector(`[data-group="${g}"]`)?.click();
        await new Promise((res) => setTimeout(res, 60));
        document.querySelector('[data-adv]')?.textContent.includes('보기') && document.querySelector('[data-adv]').click();
        await new Promise((res) => setTimeout(res, 60));
        all += document.querySelector('[data-page="settings"]').textContent;
      }
      return all;
    });
    assert.ok(text.includes('초당 요청 한도'), '고급 네트워크 항목이 보여야 한다(검사 전제)');
    for (const gone of ['메모리 목표', '오프라인이면 새로고침 멈춤', '백그라운드 활동 줄이기']) assert.ok(!text.includes(gone), `${gone} 이 남았다`);
    await ctx.close();
  });

  await check('PP09 멀티채팅 보낼 채널 선택 상자 이름', async () => {
    const { ctx, page } = await open();
    await go(page, 'multichat');
    await settle(page, 300);
    const label = await page.evaluate(() => document.querySelector('.mc-target')?.getAttribute('aria-label'));
    assert.equal(label, '보낼 채널');
    await ctx.close();
  });
} finally {
  await browser.close();
  server.close();
}
if (failures.length) { console.log(`\n실패 ${failures.length}/${total}: ${failures.join(', ')}`); process.exit(1); }
console.log(`\n모두 통과 ${total}`);
