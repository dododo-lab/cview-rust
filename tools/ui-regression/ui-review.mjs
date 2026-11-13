// UI/UX 리뷰(docs/UI_UX_FEATURE_REVIEW_2026-11-13.md) 회귀 검사 — 리뷰에서 고친 항목이 되살아나지 않는지 본다.
// 모의 IPC·헤드리스 Chromium(실제 서비스·데이터는 건드리지 않는다). 항목 번호는 리뷰 문서를 따른다.
// UR01 [P1-1] 단독 보기 Space — 재생 중이면 OSD '일시정지', 멈춘 뒤 다시 누르면 '재생'(예전엔 반대로 떴다).
// UR02 [P1-2] 단독 보기의 영상 위 채팅 — 무대 안(아래 재생 막대 위)에서 끝나 아래 정보 칸·재생 막대를 덮지 않는다.
// UR03 [P1-3] 그리드의 영상 위 채팅 — 오른쪽 열 패인의 ⚙·⋯·닫기 단추가 채팅에 가리지 않는다.
// UR04 [P1-4] 입장 때 받은 지난 기록(recent) — '여기까지 읽음'·새 메시지 수·TTS·채팅 통계에 들지 않는다. 그 뒤 온 글은 든다.
// UR05 [P1-5] 익명 후원 — 이름이 실려 와도 카드·영상 위 알림·이벤트 칩·TTS 모두 '익명'.
// UR06 [P1-6] 숨긴 시청 화면에서 ⌥⌘R 은 녹화를 켜지 않는다. 녹화 표시는 resetRecording 으로 바로 걷힌다.
// UR07 [P1-7] 백그라운드 절약 상한이 걸린 뒤 PiP 로 띄운 영상도 창이 다시 보이면 상한이 풀린다.
// UR08 [P2-6] ⌘K 팔레트에 '검색'이 한 번만 나온다.
// 저장소 루트에서: node tools/ui-regression/ui-review.mjs   (Playwright 가 필요하다)
//   CVIEW_UI_SHOTS=<폴더> 면 단독 보기·그리드 장면을 PNG 로 남긴다.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, base, openApp } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');
const SHOTS = process.env.CVIEW_UI_SHOTS;
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}
const browser = await chromium.launch();

// ── 멀티라이브(단독 보기·그리드) 모의 ─────────────────────────────
function mlMock([watch, gridOverlay]) {
  const names = [['c1', '한동숙'], ['c2', '침착맨'], ['c3', '풍월량'], ['c4', '릴카']];
  const session = ([id, name], i) => ({
    id: `s${i}`, slot: i, channelId: id, channelName: name, liveTitle: '오늘의 방송', viewerCountText: '1,234', openDate: new Date(Date.now() - 3600e3).toISOString(),
    isMuted: i > 0, isAudible: i === 0, state: { kind: 'playing' }, chatChannelId: null,
  });
  const state = {
    sessions: names.map(session), selectedId: 's0', focusMainId: 's0', canAdd: true, maxSessions: 4, layout: 'grid', focusDirection: 'left',
    focusRatio: 0.7, horizontalRatio: 0.5, verticalRatio: 0.5, multiAudio: false, watchId: watch ? 's0' : null, rev: 1,
  };
  const handlers = {
    auth_status: () => ({ loggedIn: false, state: 'loggedOut' }),
    settings_get: () => ({ multiLive: { chatOverlayInGrid: gridOverlay }, multiChat: {}, chat: { displayMode: 'overlay' } }),
    multilive_state: () => state, popular_lives: () => [], following_lives: () => [], multilive_stage_size: () => null,
    channel_info: () => ({ verified: false, followerCount: 1000 }), live_detail: () => ({ liveTitle: '오늘의 방송', categoryName: '토크' }),
  };
  window.__TAURI__ = {
    core: { invoke: async (c, a) => { if (c === 'ui_heartbeat') return null; const h = handlers[c]; if (!h) throw new Error(`mock: ${c}`); return h(a || {}); } },
    event: { listen: async () => () => {} },
  };
  try { localStorage.setItem('cview.onboarded', '1'); } catch { /* 무시 */ }
}

async function openMl({ watch, gridOverlay = false }) {
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mlMock, [watch, gridOverlay]);
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: 'multilive', params: {} } })));
  await page.waitForSelector('.ml-pane:not([hidden])');
  await page.waitForTimeout(700); // 무대 크기 재기(200ms)·FLIP 이 끝나게
  return { page, errors };
}
const rect = (page, sel) => page.evaluate((s) => { const r = document.querySelector(s).getBoundingClientRect(); return { x: r.x, y: r.y, w: r.width, h: r.height, right: r.right, bottom: r.bottom }; }, sel);
/** 패인 막대를 띄우고(마우스 올림과 같은 ctl-visible) 단추 가운데를 맨 위에서 누가 받는지 본다. */
const hitButtons = (page) => page.evaluate(() => {
  const out = [];
  for (const p of document.querySelectorAll('.ml-pane:not([hidden])')) {
    p.classList.add('ctl-visible');
    for (const b of p.querySelectorAll('[data-mp="gear"], [data-mp="more"], [data-ml="remove"], [data-mp="fullscreen"]')) {
      const r = b.getBoundingClientRect();
      if (!r.width) continue;
      const top = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
      out.push({ slot: p.dataset.slot, btn: b.dataset.mp || b.dataset.ml, ok: Boolean(top && (top === b || b.contains(top))), under: top?.closest('.ml-chat') ? 'chat' : top?.className || '' });
    }
  }
  return out;
});
const shot = async (page, name) => { if (!SHOTS) return; fs.mkdirSync(SHOTS, { recursive: true }); await page.screenshot({ path: path.join(SHOTS, `${name}.png`) }); };

await check('UR01 [P1-1] watch Space OSD says 일시정지 when pausing, 재생 when resuming', async () => {
  const { page } = await openMl({ watch: true });
  // 플레이어 페이지(player.html)가 준비되면 play()/pause() 가 곧바로 video.paused 를 바꾼다 — 재생 중인 척 시작한다.
  await page.waitForFunction(() => { const f = document.querySelector('.ml-pane:not([hidden]) iframe'); return f && typeof f.contentWindow?.pause === 'function' && f.contentDocument.readyState === 'complete'; });
  await page.evaluate(() => {
    const v = document.querySelector('.ml-pane:not([hidden]) iframe').contentDocument.getElementById('video');
    let paused = false;
    Object.defineProperty(v, 'paused', { configurable: true, get: () => paused });
    v.play = () => { paused = false; return Promise.resolve(); };
    v.pause = () => { paused = true; };
    document.activeElement?.blur();
  });
  const osd = () => page.evaluate(() => document.querySelector('.player-osd:not([hidden])')?.textContent || '');
  await page.keyboard.press('Space');
  assert.equal(await osd(), '일시정지', '재생 중에 Space');
  await page.keyboard.press('Space');
  assert.equal(await osd(), '재생', '멈춘 뒤 Space');
  await page.close();
});

await check('UR02 [P1-2] watch overlay chat stays inside the stage above the play bar', async () => {
  const { page, errors } = await openMl({ watch: true });
  await page.evaluate(() => document.querySelectorAll('.ml-pane:not([hidden])').forEach((p) => p.classList.add('ctl-visible')));
  await shot(page, 'watch-overlay-chat');
  const stage = await rect(page, '.ml-stage');
  const chat = await rect(page, '.ml-chat');
  assert.ok(chat.h > 150, `채팅이 너무 낮다 ${chat.h}`);
  assert.ok(chat.bottom <= stage.bottom - 50, `채팅 아래 끝 ${chat.bottom} 이 무대 아래(${stage.bottom}) 재생 막대까지 내려온다`);
  const hits = await hitButtons(page);
  const blocked = hits.filter((h) => !h.ok);
  assert.deepEqual(blocked, [], `가린 단추: ${JSON.stringify(blocked)}`);
  assert.deepEqual(errors, []);
  await page.close();
});

await check('UR03 [P1-3] grid overlay chat does not cover right-column pane buttons', async () => {
  const { page, errors } = await openMl({ watch: false, gridOverlay: true });
  await page.evaluate(() => document.querySelectorAll('.ml-pane:not([hidden])').forEach((p) => p.classList.add('ctl-visible')));
  await shot(page, 'grid-overlay-chat');
  const hits = await hitButtons(page);
  assert.ok(hits.length >= 8, `단추를 못 찾았다 ${hits.length}`);
  const blocked = hits.filter((h) => !h.ok);
  assert.deepEqual(blocked, [], `채팅에 가린 단추: ${JSON.stringify(blocked)}`);
  // 채팅을 숨기면(오버레이 끔) 비킴도 풀린다
  await page.evaluate(() => document.querySelector('.ml-body').classList.remove('chat-overlay'));
  await page.setViewportSize({ width: 1281, height: 800 }); // 무대 크기 변경 → 다시 잼
  await page.waitForTimeout(500);
  assert.equal(await page.locator('.ml-pane.under-chat').count(), 0, '오버레이를 꺼도 비킴이 남았다');
  assert.deepEqual(errors, []);
  await page.close();
});

// ── 채팅 패널(지난 기록·익명) ──────────────────────────────────
async function openChat() {
  const { ctx, page, errors } = await openApp(browser, { width: 1280, height: 900 });
  await page.evaluate(async () => {
    window.__spoken = [];
    window.speechSynthesis.speak = (u) => window.__spoken.push(u.text);
    const { ChatPanel } = await import('/js/chat.js');
    const { loadChatPrefs, updateChatPrefs } = await import('/js/chat-prefs.js');
    await loadChatPrefs();
    await updateChatPrefs({ ttsEnabled: true });
    const src = new DOMParser().parseFromString(await (await fetch('/index.html')).text(), 'text/html');
    const box = document.createElement('div');
    box.style.cssText = 'position:fixed;left:20px;top:10px;width:380px;height:700px;z-index:50;display:flex;flex-direction:column';
    box.innerHTML = src.querySelector('#viewer .chat').outerHTML;
    document.body.appendChild(box);
    const root = box.firstElementChild;
    root.hidden = false;
    root.style.cssText = 'display:flex;flex-direction:column;height:100%;width:100%';
    const panel = new ChatPanel(root, { isLoggedIn: () => false });
    panel.active = true;
    window.__panel = panel;
    await panel.connect('cc1', 'ch1');
    panel.onEvent({ type: 'connected' });
    let k = 0;
    window.__msg = (o) => ({ id: `m${++k}`, kind: 'normal', role: 'common_user', badges: [], emojis: {}, nickname: `n${k}`, userId: `u${k}`, content: `글 ${k}`, timestamp: Date.now(), ...o });
  });
  return { ctx, page, errors };
}

await check('UR04 [P1-4] recent history is drawn but not counted as unread, TTS or stats', async () => {
  const { ctx, page, errors } = await openChat();
  const r = await page.evaluate(() => {
    const p = window.__panel;
    // 사용자가 위로 올려 읽는 중(따라가기 꺼짐) — 이때 새 글은 구분선·새 메시지 수를 만든다.
    p.onEvent({ type: 'messages', messages: [window.__msg({}), window.__msg({})], recent: false });
    p.flushLines();
    p.follower.stuck = false;
    p.unread = 0;
    p.list.querySelector('.read-divider')?.remove();
    window.__spoken.length = 0;
    const lines0 = p.list.querySelectorAll('.chat-line').length;
    p.onEvent({ type: 'messages', recent: true, messages: [window.__msg({}), window.__msg({ kind: 'donation', donationAmount: 1000, nickname: '지난후원' })] });
    p.flushLines();
    const afterRecent = { lines: p.list.querySelectorAll('.chat-line, .chat-event').length - lines0, divider: Boolean(p.list.querySelector('.read-divider')), unread: p.unread, spoken: window.__spoken.length };
    p.onEvent({ type: 'messages', messages: [window.__msg({ kind: 'donation', donationAmount: 2000, nickname: '새후원' })] });
    p.flushLines();
    const afterFresh = { divider: Boolean(p.list.querySelector('.read-divider')), unread: p.unread, spoken: window.__spoken.slice() };
    return { afterRecent, afterFresh };
  });
  assert.ok(r.afterRecent.lines >= 2, `지난 기록이 그려지지 않았다 ${JSON.stringify(r.afterRecent)}`);
  assert.equal(r.afterRecent.divider, false, '지난 기록에 "여기까지 읽음"이 붙었다');
  assert.equal(r.afterRecent.unread, 0, '지난 기록이 새 메시지 수에 들었다');
  assert.equal(r.afterRecent.spoken, 0, '지난 후원을 TTS 로 읽었다');
  assert.equal(r.afterFresh.divider, true, '새 글에 구분선이 없다');
  assert.equal(r.afterFresh.unread, 1);
  assert.equal(r.afterFresh.spoken.length, 1, `새 후원 TTS ${JSON.stringify(r.afterFresh.spoken)}`);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('UR05 [P1-5] anonymous donor is "익명" in card, stream alert, chip and TTS', async () => {
  const { ctx, page, errors } = await openChat();
  const r = await page.evaluate(async () => {
    const { ttsText } = await import('/js/chat-extras.js');
    const { donorName, eventCardHtml, StreamAlerts } = await import('/js/donations.js');
    const m = window.__msg({ kind: 'donation', donationAmount: 5000, anonymous: true, nickname: '홍길동', userId: '' });
    const host = document.createElement('div');
    host.style.cssText = 'position:fixed;inset:0;';
    document.body.append(host);
    const alerts = new StreamAlerts(host);
    alerts.push(m);
    await new Promise((res) => setTimeout(res, 50));
    window.__panel.onEvent({ type: 'messages', messages: [m] });
    window.__panel.flushLines();
    return { name: donorName(m), tts: ttsText(m), card: eventCardHtml(m, { nick: '<b>홍길동</b>' }), alert: host.textContent, rail: document.querySelector('.event-chip')?.textContent || '' };
  });
  assert.equal(r.name, '익명');
  for (const [k, v] of Object.entries(r)) assert.ok(!String(v).includes('홍길동'), `${k} 에 이름이 드러난다: ${v}`);
  assert.match(r.tts, /^익명님이/);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('UR06 [P1-6] ⌥⌘R ignored while the viewer is hidden; resetRecording clears the dot', async () => {
  const { ctx, page, errors } = await openApp(browser, { width: 1280, height: 900 });
  const r = await page.evaluate(async () => {
    const { PlayerPanel } = await import('/js/player-panel.js');
    const root = document.createElement('div');
    root.innerHTML = '<div class="player-stage"></div><button id="reload"></button>';
    root.hidden = true;
    document.body.append(root);
    const viewer = { root, isPlaying: true, channelId: 'ch1', player: { onMessage() {}, call() {} }, controls: { flash() {} } };
    const panel = new PlayerPanel(viewer);
    let starts = 0;
    panel.toggleRecording = () => { starts += 1; };
    const press = () => document.dispatchEvent(new KeyboardEvent('keydown', { key: 'r', metaKey: true, altKey: true, bubbles: true }));
    press();
    const hiddenStarts = starts;
    root.hidden = false;
    press();
    panel.recording = { id: 1 };
    panel.setRecUi(true);
    panel.recTimer = setInterval(() => {}, 99999);
    panel.resetRecording();
    return { hiddenStarts, shownStarts: starts - hiddenStarts, dot: panel.recButton.classList.contains('recording'), recording: panel.recording };
  });
  assert.equal(r.hiddenStarts, 0, '숨긴 시청 화면에서 녹화가 켜졌다');
  assert.equal(r.shownStarts, 1, '보이는 시청 화면에서 ⌥⌘R 이 안 듣는다');
  assert.equal(r.dot, false, '녹화 표시가 남았다');
  assert.equal(r.recording, null);
  // viewer.close 가 녹화 표시를 바로 걷는지(시청 화면 전체를 띄우지 않고 코드로 확인)
  const src = fs.readFileSync('app-ui/js/viewer.js', 'utf8');
  const close = src.slice(src.indexOf('async close('), src.indexOf('async close(') + 600);
  assert.match(close, /this\.panel\.resetRecording\(\)/, 'viewer.close 가 녹화 표시를 걷지 않는다');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('UR07 [P1-7] PiP entered while capped is released when the window shows again', async () => {
  const { ctx, page, errors } = await openApp(browser, { width: 1280, height: 900 });
  await page.clock.install();
  const r = await page.evaluate(async () => {
    const { installBackgroundSaver } = await import('/js/background-saver.js?ur07');
    let hidden = false;
    Object.defineProperty(document, 'hidden', { configurable: true, get: () => hidden });
    installBackgroundSaver();
    const f = document.createElement('iframe');
    document.body.append(f);
    const w = f.contentWindow;
    w.document.body.innerHTML = '<video id="video"></video>';
    w.__caps = [];
    w.setBackgroundSaver = (h) => w.__caps.push(h);
    window.__f = f;
    hidden = true;
    document.dispatchEvent(new Event('visibilitychange'));
    window.__show = () => { hidden = false; document.dispatchEvent(new Event('visibilitychange')); };
    return true;
  });
  assert.ok(r);
  await page.clock.runFor(16000);
  await page.evaluate(() => {
    const v = window.__f.contentDocument.getElementById('video');
    Object.defineProperty(v, 'webkitPresentationMode', { configurable: true, get: () => 'picture-in-picture' });
    window.__show();
  });
  const caps = await page.evaluate(() => window.__f.contentWindow.__caps);
  assert.ok(caps.includes(360), `상한이 걸리지 않았다 ${caps}`);
  assert.equal(caps.at(-1), 0, `PiP 영상의 상한이 풀리지 않았다 ${caps}`);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('UR08 [P2-6] command palette lists 검색 once', async () => {
  const { ctx, page, errors } = await openApp(browser, { width: 1280, height: 900 });
  await page.keyboard.press('Meta+k');
  await page.waitForSelector('.palette-item');
  const labels = await page.$$eval('.palette-item .palette-label', (els) => els.map((e) => e.textContent.trim()));
  assert.equal(labels.filter((l) => l === '검색').length, 1, `검색 ${labels.filter((l) => l === '검색').length}번`);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await browser.close();
server.close();
let failed = 0;
for (const r of results) {
  console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : `\n     ${r.error}`}`);
  if (!r.ok) failed += 1;
}
console.log(`\n${results.length - failed}/${results.length} passed`);
process.exit(failed ? 1 : 0);
