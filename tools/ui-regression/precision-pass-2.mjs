// 기능 정밀 고도화 2차(2026-11-26) 회귀 검사 — 코드 검토로 새로 찾은 실제 결함을 고친 것.
// 모의 IPC·헤드리스 브라우저(실제 서비스·데이터는 건드리지 않는다). CVIEW_UI_BROWSER=webkit 이면 WebKit.
// 저장소 루트에서: node tools/ui-regression/precision-pass-2.mjs
// Q01 시청 화면 — 재연결·화질 바꾸기·정지 복구로 playStarted 가 다시 와도 사용자가 바꾼 음소거를 지킨다
//     (예전엔 매번 '음소거로 시작' 설정으로 되돌아갔다). 새로 열면 설정을 따른다.
// Q02 시청 화면 — '다시 연결'·'다시 시도'(처음 받은 설정 객체로 다시 열기)는 연 뒤 바꾼 볼륨을 되돌리지 않는다.
// Q03 시청 화면을 녹화 중에 닫으면 녹화를 중지 단추처럼 끝내고(player_record_stop) 저장 위치를 알린다.
// Q04 채팅 — 보내는 동안 입력창을 꺼도 보낸 뒤 초점이 입력창으로 돌아온다(예전엔 문서로 빠졌다).
// Q05 멀티채팅 통합 입력 — /stats 같은 앱 명령은 앱이 처리하고 채팅방으로 보내지 않는다. /clear 는 통합 목록을 지운다.
// Q06 멀티채팅 개요 칩 — 5초마다 다시 그려도 키보드 초점이 같은 칩·닫기 단추에 남는다.
// Q07 멀티채팅 — 복원 때 열지 못한 채널은 저장 목록에서 빠지지 않는다.
// Q08 검색 — 앞 검색의 '다시 시도'가 끝나기 전에 새로 찾고 그 검색도 실패하면 '다시 시도' 단추가 보인다.
// Q09 허브 이동 알림에 '방송&클립'이 그대로 보인다(예전엔 '방송&amp;클립').
// Q10 멀티라이브 포커스 분할선 — 키보드 범위가 메인 비율의 실제 범위(45~85%)다.
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
  window.__qCalls = [];
  const fns = Object.fromEntries(Object.entries(extra).map(([k, v]) => [k, new Function(`return (${v})`)()]));
  const inv = window.__TAURI__.core.invoke;
  window.__TAURI__.core.invoke = async (c, a) => { window.__qCalls.push([c, a]); return fns[c] ? fns[c](a || {}) : inv(c, a); };
}

async function open({ extra = {} } = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 }, colorScheme: 'dark' });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, 'dark']);
  await page.addInitScript(overlay, Object.fromEntries(Object.entries(extra).map(([k, v]) => [k, v.toString()])));
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
  return { ctx, page, errors };
}
const settle = (page, ms = 250) => page.waitForTimeout(ms);

try {
  await check('Q01 재시작한 재생도 사용자가 바꾼 음소거를 지킨다', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async () => {
      const { Viewer } = await import('/js/viewer.js');
      const calls = [];
      let muted = true;
      const v = Object.assign(Object.create(Viewer.prototype), {
        player: { call: (n, x) => { calls.push([n, x]); if (n === 'setMuted') muted = x; }, isMuted: () => muted },
        muteButton: document.createElement('button'), volume: { value: '50' }, showOverlay() {}, audioSuspended: false,
        startMuted: false, sessionMuted: false,
      });
      const lastMute = () => calls.filter(([n]) => n === 'setMuted').at(-1)[1];
      Viewer.prototype.onPlayerMessage.call(v, 'playStarted', {});
      const first = lastMute();
      Viewer.prototype.toggleMute.call(v); // 사용자가 M — 음소거
      Viewer.prototype.onPlayerMessage.call(v, 'playStarted', {}); // 재연결·정지 복구
      const afterReload = lastMute();
      Viewer.prototype.toggleMute.call(v); // 다시 풀기
      Viewer.prototype.onPlayerMessage.call(v, 'playStarted', {});
      return { first, afterReload, afterUnmute: lastMute() };
    });
    assert.deepEqual(r, { first: false, afterReload: true, afterUnmute: false });
    await ctx.close();
  });

  await check('Q02 다시 연결은 연 뒤 바꾼 볼륨을 되돌리지 않는다', async () => {
    const { ctx, page } = await open({ extra: { player_start: '() => { throw new Error("방송 중이 아닙니다"); }', live_detail: '() => { throw new Error("x"); }' } });
    const r = await page.evaluate(async () => {
      const { Viewer } = await import('/js/viewer.js');
      // 화면 구성 요소는 아무것도 하지 않는 대역으로 — open() 의 설정·볼륨 처리만 본다.
      const noop = () => new Proxy(function () {}, { get: (t, k) => (k in t ? t[k] : noop()), apply: () => undefined });
      const volume = document.createElement('input');
      volume.type = 'range'; volume.max = '100';
      const v = new Proxy({ startToken: 0, channelId: null, volume, root: { hidden: true }, topbar: { loadedFor: 'c1', load() {}, closeMenu() {} }, status: {} }, {
        get: (t, k) => (k in t ? t[k] : noop()),
      });
      const settings = { volumeLevel: 0.5, startMuted: false };
      await Viewer.prototype.open.call(v, 'c1', settings);
      const opened = volume.value;
      volume.value = '20'; // 연 뒤 사용자가 20% 로(저장됨)
      await Viewer.prototype.open.call(v, 'c1', v.lastSettings); // '다시 연결'
      const reopened = volume.value;
      await Viewer.prototype.open.call(v, 'c1', { volumeLevel: 0.7 }); // 새로 열기 — 새 설정을 따른다
      return { opened, reopened, fresh: volume.value };
    });
    assert.deepEqual(r, { opened: '50', reopened: '20', fresh: '70' });
    await ctx.close();
  });

  await check('Q03 녹화 중 시청 화면을 닫으면 녹화를 끝내고 저장 위치를 알린다', async () => {
    const { ctx, page } = await open({ extra: { player_record_stop: '() => ({ recording: false, path: "/Users/x/Movies/CView/채널_20261126_101010.mp4", bytes: 3145728, startedAt: 1 })' } });
    const r = await page.evaluate(async () => {
      const { PlayerPanel } = await import('/js/player-panel.js');
      const p = { recording: { recording: true }, recTimer: 0, setRecUi() {} };
      await PlayerPanel.prototype.resetRecording.call(p);
      const idle = { recording: null, recTimer: 0, setRecUi() {} };
      window.__qCalls.length = 0;
      await PlayerPanel.prototype.resetRecording.call(idle); // 녹화 중이 아니면 아무것도 하지 않는다
      return { recording: p.recording, toast: [...document.querySelectorAll('.toast')].map((t) => t.textContent).join(' '), idleCalls: window.__qCalls.length };
    });
    assert.equal(r.recording, null);
    assert.match(r.toast, /녹화를 저장했습니다 \(3\.0MB\).*\.mp4/);
    assert.equal(r.idleCalls, 0);
    await ctx.close();
  });

  await check('Q04 채팅을 보낸 뒤 초점이 입력창으로 돌아온다', async () => {
    const { ctx, page } = await open({ extra: { chat_send: '() => new Promise((res) => setTimeout(() => res(null), 60))' } });
    const r = await page.evaluate(async () => {
      const { ChatPanel } = await import('/js/chat.js');
      const root = document.createElement('div');
      root.innerHTML = '<input class="q-input" maxlength="100">';
      document.body.append(root);
      const input = root.querySelector('input');
      const panel = {
        root, input, sendHooks: [], canSendMessage: true, pendingSend: null, connSeq: 1, chatChannelId: 'cc', active: true, destroyed: false, session: null,
        updateInput() { this.input.disabled = Boolean(this.pendingSend); }, renderCount() {}, remember() {}, flashInput() {},
      };
      input.focus();
      input.value = 'ㅎㅇ';
      await ChatPanel.prototype.send.call(panel);
      return { focused: document.activeElement === input, value: input.value, disabled: input.disabled };
    });
    assert.deepEqual(r, { focused: true, value: '', disabled: false });
    await ctx.close();
  });

  await check('Q05 통합 입력의 앱 명령은 채팅방으로 보내지 않는다', async () => {
    const { ctx, page } = await open({ extra: { chat_send: '() => null' } });
    const r = await page.evaluate(async () => {
      const { MultiChatPage } = await import('/js/multichat.js');
      const input = document.createElement('input');
      document.body.append(input);
      const list = document.createElement('div');
      list.innerHTML = '<div class="chat-line">옛 줄</div>';
      const handled = [];
      const entry = { session: 'mc:a', panel: { canSendMessage: true, connSeq: 1, chatChannelId: 'cc', active: true, destroyed: false, sendHooks: [(t) => { if (t.startsWith('/')) { handled.push(t); return true; } return false; }] } };
      const mc = {
        mergedInput: input, mergedList: list, layout: 'merged', pendingSend: null, lines: { reset() {} },
        targetEntry: () => entry, updateMergedInput() {}, composer: { renderCount() {}, remember() {} },
      };
      window.__qCalls.length = 0;
      input.value = '/stats';
      await MultiChatPage.prototype.sendMerged.call(mc);
      input.value = '/clear';
      await MultiChatPage.prototype.sendMerged.call(mc);
      const sentCommands = window.__qCalls.filter(([c]) => c === 'chat_send').length;
      input.value = '안녕';
      await MultiChatPage.prototype.sendMerged.call(mc);
      return { handled, sentCommands, sentText: window.__qCalls.filter(([c]) => c === 'chat_send').map(([, a]) => a.text), list: list.textContent, value: input.value };
    });
    assert.deepEqual(r.handled, ['/stats', '/clear']);
    assert.equal(r.sentCommands, 0, '명령을 채팅방으로 보냈다');
    assert.deepEqual(r.sentText, ['안녕']);
    assert.equal(r.list, '채팅을 지웠습니다');
    assert.equal(r.value, '');
    await ctx.close();
  });

  await check('Q06 개요 칩을 다시 그려도 키보드 초점이 남는다', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async () => {
      const { MultiChatPage } = await import('/js/multichat.js');
      const overview = document.createElement('div');
      document.body.append(overview);
      const chan = (id) => ({ channelId: id, name: `이름${id}`, color: '#0c8', image: null, rate: [], panel: { state: { className: 'ok' } } });
      const mc = { overview, active: 'a', channels: [chan('a'), chan('b')] };
      MultiChatPage.prototype.renderOverview.call(mc);
      overview.querySelector('[data-mc-remove="b"]').focus();
      mc.channels[1].rate.push(Date.now()); // 분당 메시지 수가 바뀌어 다시 그린다
      MultiChatPage.prototype.renderOverview.call(mc);
      const afterRemove = document.activeElement?.dataset.mcRemove;
      overview.querySelector('[data-mc-focus="a"]').focus();
      const before = overview.querySelector('[data-mc-focus="a"]');
      MultiChatPage.prototype.renderOverview.call(mc); // 바뀐 것이 없으면 그대로 둔다
      const same = overview.querySelector('[data-mc-focus="a"]') === before;
      mc.channels[0].rate.push(Date.now());
      MultiChatPage.prototype.renderOverview.call(mc);
      return { afterRemove, afterChip: document.activeElement?.dataset.mcFocus, same, rate: overview.querySelector('[data-mc-focus="a"] .mc-rate').textContent };
    });
    assert.deepEqual(r, { afterRemove: 'b', afterChip: 'a', same: true, rate: '1/분' });
    await ctx.close();
  });

  await check('Q07 복원하지 못한 멀티채팅 채널은 저장 목록에 남는다', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async () => {
      const { MultiChatPage } = await import('/js/multichat.js');
      const mc = { channels: [{ channelId: 'a', name: 'A' }], unrestored: [{ channelId: 'b', channelName: 'B' }, { channelId: 'a', channelName: 'A' }], active: 'a', layout: 'tabs' };
      window.__qCalls.length = 0;
      MultiChatPage.prototype.persist.call(mc);
      await new Promise((res) => setTimeout(res, 20));
      return window.__qCalls.filter(([c, a]) => c === 'settings_update' && a.domain === 'multiChat').map(([, a]) => a.patch.savedSessions.map((x) => x.channelId));
    });
    assert.deepEqual(r, [['a', 'b']]);
    await ctx.close();
  });

  await check('Q08 앞 검색의 재시도 중 새 검색이 실패해도 다시 시도 단추가 있다', async () => {
    const { ctx, page } = await open({
      extra: {
        search: '({ keyword }) => { window.__searchN = (window.__searchN || 0) + 1; if (window.__searchN === 2) return new Promise((_, rej) => { window.__releaseRetry = () => rej(new Error("느림")); }); throw new Error("검색 실패 " + keyword); }',
      },
    });
    const run = (q) => page.evaluate((query) => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: 'search', params: { query } } })), q);
    await run('가');
    await page.waitForSelector('[data-page="search"]:not([hidden]) [data-search-retry="main"]');
    await page.click('[data-page="search"] [data-search-retry="main"]'); // 두 번째 search — 끝나지 않고 걸린다
    await settle(page, 150);
    await run('나'); // 세 번째 search — 실패
    await settle(page, 300);
    await page.evaluate(() => window.__releaseRetry && window.__releaseRetry());
    await settle(page, 200);
    const r = await page.evaluate(() => ({
      retry: Boolean(document.querySelector('[data-page="search"] [data-search-retry="main"]')),
      retrying: /다시 찾는 중/.test(document.querySelector('[data-page="search"]').textContent),
    }));
    assert.deepEqual(r, { retry: true, retrying: false });
    await ctx.close();
  });

  await check('Q09 허브 알림 문구가 이스케이프되지 않는다', async () => {
    const { ctx, page } = await open();
    const text = await page.evaluate(async () => {
      const { hubFlash } = await import('/js/workspace.js');
      hubFlash('방송&클립');
      return [...document.querySelectorAll('.toast')].map((t) => t.textContent).join(' ');
    });
    assert.match(text, /허브 · 방송&클립/);
    assert.doesNotMatch(text, /&amp;/);
    await ctx.close();
  });

  await check('Q10 포커스 분할선 키보드 범위가 메인 비율 범위와 같다', async () => {
    const { ctx, page } = await open();
    await go(page, 'multilive');
    await page.waitForSelector('.ml-divider', { state: 'attached' });
    const r = await page.evaluate(() => {
      const d = document.querySelector('.ml-divider');
      const slider = document.querySelector('.page-multilive input[type="range"][aria-label="메인 패인 크기"]');
      return { min: d.getAttribute('aria-valuemin'), max: d.getAttribute('aria-valuemax'), sliderMin: slider?.min, sliderMax: slider?.max };
    });
    assert.deepEqual(r, { min: '45', max: '85', sliderMin: '45', sliderMax: '85' });
    await ctx.close();
  });
} finally {
  await browser.close();
  server.close();
}
console.log(`\n${total - failures.length}/${total} 통과 (${WEBKIT ? 'webkit' : 'chromium'})`);
if (failures.length) process.exit(1);
