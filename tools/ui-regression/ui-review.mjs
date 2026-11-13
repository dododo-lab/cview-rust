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
// UR09 [P2-2] 채널 추가 서랍·패인 메뉴 안의 키는 플레이어 단축키로 가지 않는다. 초점이 서랍 밖이어도 Esc 로 서랍이 닫힌다.
// UR10 [P2-3] 포커스 분할선 — 끄는 동안 그리드 칸만 바뀌고(놓기 전 IPC 없음), 위·아래 배치에선 커서가 row-resize.
// UR11 [P2-4] 직접 고른 화질은 고른 높이로 보이고, 자동이면 '자동'이 켜져 보인다(플레이어 levels 의 manual).
// UR12 [P2-4] 영상만 전체 화면 → 영상+채팅 전체 화면으로 바로 넘어간다(예전엔 꺼지기만 했다).
// UR13 [P2-5] 채팅 — 멘션·이모티콘으로 100자를 넘기지 않고 글자 수가 바로 바뀐다, 선택기 검색어·방향키, 불러오기 실패 안내,
//      검색 0건 안내·지우기 뒤 개수, /unblock 없는 이름, /mute 는 붙이기 전에 거른다, TTS 대기 상한.
// UR14 [P2-6] 단축키 도움말에 ⌥⌘1~6 작업 공간, 북마크 줄 한 번. 설정 검색 중엔 탭이 켜져 보이지 않고, 가져오기는 여러 줄 칸,
//      가져오기 뒤 테마·설정 변경을 알린다.
// UR15 [P2-7] 미리보기 소스 조회 오류는 15초 뒤 다시 묻고(없음은 5분), 팔로잉 팔로워 수 조회 실패는 30초 뒤 다시 묻는다.
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
  // 환영 안내를 띄우지 않는다 — 띄운 뒤 지우면 그 Esc 처리기(잡는 단계)가 남아 첫 Esc 를 먹는다.
  try { localStorage.setItem('cview.onboarded', '1'); localStorage.setItem('cview.welcomed.v1', String(Date.now())); } catch { /* 무시 */ }
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

await check('UR09 [P2-2] drawer/menu keys stay out of player shortcuts; Esc outside closes the drawer', async () => {
  const { page, errors } = await openMl({ watch: true });
  const r = await page.evaluate(async () => {
    const { keyBelongsToTarget } = await import('/js/dom.js');
    const ev = (target, key) => ({ key, target, defaultPrevented: false });
    const btn = (cls) => { const box = document.createElement('div'); box.className = cls; const b = document.createElement('button'); box.append(b); document.body.append(box); return b; };
    const out = {};
    for (const cls of ['ml-addbar', 'ml-more-menu', 'ml-pane-menu', 'quality-menu']) {
      const b = btn(cls);
      out[cls] = ['ArrowLeft', 'm', 'l', 'b'].every((k) => keyBelongsToTarget(ev(b, k)));
      b.parentElement.remove();
    }
    out.plain = keyBelongsToTarget(ev(document.body, 'm'));
    return out;
  });
  assert.deepEqual(r, { 'ml-addbar': true, 'ml-more-menu': true, 'ml-pane-menu': true, 'quality-menu': true, plain: false });
  await page.click('[data-ml="add"]');
  await page.waitForSelector('.ml-addbar.open');
  await page.evaluate(() => { document.activeElement?.blur(); document.body.focus(); });
  await page.keyboard.press('Escape');
  await page.waitForTimeout(100);
  assert.equal(await page.locator('.ml-addbar.open').count(), 0, '서랍 밖 초점에서 Esc 로 닫히지 않았다');
  assert.equal(await page.evaluate(() => document.querySelector('.page-multilive').classList.contains('as-watch')), true, 'Esc 가 단독 보기까지 닫았다');
  assert.deepEqual(errors, []);
  await page.close();
});

await check('UR10 [P2-3] focus divider previews grid only while dragging; row-resize cursor for top/bottom', async () => {
  const { page, errors } = await openMl({ watch: false });
  await page.evaluate(() => {
    window.__ipc = [];
    const inv = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async (c, a) => { window.__ipc.push(c); if (c === 'multilive_set_focus_layout') { const st = await inv('multilive_state'); st.layout = 'focus'; if (a.direction) st.focusDirection = a.direction; if (a.ratio != null) st.focusRatio = a.ratio; st.rev += 1; return st; } return inv(c, a); };
  });
  await page.evaluate(() => document.querySelector('[data-direction="top"]').click());
  await page.waitForSelector('.ml-stage.ml-focus.dir-top');
  await page.waitForTimeout(300);
  const d = await rect(page, '.ml-stage > .ml-divider:not(.ml-grid-div)');
  const before = await page.evaluate(() => document.querySelector('.ml-stage').style.gridTemplateRows);
  await page.evaluate(() => { window.__ipc.length = 0; });
  await page.mouse.move(d.x + d.w / 2, d.y + d.h / 2);
  await page.mouse.down();
  await page.mouse.move(d.x + d.w / 2, d.y + d.h / 2 - 60, { steps: 4 });
  await page.waitForTimeout(50);
  const mid = await page.evaluate(() => ({ rows: document.querySelector('.ml-stage').style.gridTemplateRows, cursor: getComputedStyle(document.querySelector('.ml-stage .ml-pane')).cursor, ipc: window.__ipc.slice() }));
  await page.mouse.up();
  await page.waitForTimeout(200);
  const after = await page.evaluate(() => ({ ipc: window.__ipc.slice(), resizing: document.querySelector('.ml-stage').classList.contains('row-resizing') }));
  assert.notEqual(mid.rows, before, '끄는 동안 칸이 바뀌지 않았다');
  assert.equal(mid.cursor, 'row-resize', `위·아래 분할선 커서 ${mid.cursor}`);
  assert.ok(!mid.ipc.includes('multilive_set_focus_layout'), '놓기 전에 저장했다');
  assert.ok(after.ipc.includes('multilive_set_focus_layout'), '놓은 뒤 저장하지 않았다');
  assert.equal(after.resizing, false);
  assert.deepEqual(errors, []);
  await page.close();
});

await check('UR11 [P2-4] chosen quality shows the chosen height; auto shows 자동 active', async () => {
  const { page, errors } = await openMl({ watch: true });
  await page.waitForFunction(() => { const f = document.querySelector('.ml-pane:not([hidden]) iframe'); return f && f.contentDocument?.readyState === 'complete'; });
  const send = (data) => page.evaluate((d) => window.__cviewPlayerBridge({ type: 'levels', data: d }, document.querySelector('.ml-pane.ml-single-target iframe').contentWindow), data);
  const levels = [{ index: 0, height: 480, width: 854, bitrate: 1 }, { index: 1, height: 720, width: 1280, bitrate: 2 }, { index: 2, height: 1080, width: 1920, bitrate: 3 }];
  // ⚙ 를 열면 플레이어에 화질 목록을 다시 묻는다(모의 화면의 플레이어는 빈 목록을 답한다) — 연 뒤에 보낸다.
  const gear = async (data) => {
    await page.evaluate(() => { const p = document.querySelector('.ml-pane.ml-single-target'); p.classList.add('ctl-visible'); p.querySelector('[data-mp="gear"]').click(); });
    await send(data);
    const r = await page.evaluate(() => { const m = document.querySelector('.ml-pane.ml-single-target .ml-gear-menu'); return { head: m.querySelector('.ml-menu-head span')?.textContent || '', active: m.querySelector('.ml-level.active span')?.textContent || '' }; });
    await page.evaluate(() => document.querySelector('.ml-pane.ml-single-target [data-mp="gear"]').click());
    return r;
  };
  const manual = await gear({ levels, current: 2, manual: 1, capped: -1 }); // 720p 를 골랐고 아직 1080p 조각을 재생 중
  const auto = await gear({ levels, current: 2, manual: -1, capped: -1 });
  assert.deepEqual(manual, { head: '720p', active: '720p' }, `고른 화질 ${JSON.stringify(manual)}`);
  assert.equal(auto.active, '자동', `자동인데 ${auto.active} 가 켜져 보인다`);
  assert.deepEqual(errors, []);
  await page.close();
});

await check('UR12 [P2-4] video fullscreen switches straight to video+chat fullscreen', async () => {
  const { page, errors } = await openMl({ watch: true });
  await page.evaluate(() => document.activeElement?.blur());
  await page.keyboard.press('Meta+Shift+F');
  await page.waitForFunction(() => document.fullscreenElement?.classList.contains('ml-pane'), null, { timeout: 3000 });
  await page.keyboard.press('Meta+Control+F');
  await page.waitForFunction(() => document.fullscreenElement?.classList.contains('ml-body'), null, { timeout: 3000 });
  await page.keyboard.press('Meta+Control+F');
  await page.waitForFunction(() => !document.fullscreenElement, null, { timeout: 3000 });
  assert.deepEqual(errors, []);
  await page.close();
});

await check('UR13 [P2-5] chat input limit/counter, picker, search empty, /unblock, /mute, TTS cap', async () => {
  const { ctx, page, errors } = await openChat();
  await page.evaluate(() => {
    window.__toasts = [];
    new MutationObserver(() => { for (const t of document.querySelectorAll('.toast')) if (!window.__toasts.includes(t.textContent)) window.__toasts.push(t.textContent); }).observe(document.body, { childList: true, subtree: true });
  });
  const r = await page.evaluate(async () => {
    const p = window.__panel;
    const out = {};
    p.input.disabled = false;
    p.input.value = 'a'.repeat(85);
    p.insertAtCaret('{:d_1:}');
    out.afterEmoji = { len: p.input.value.length, counter: p.counter.textContent, shown: !p.counter.hidden };
    p.input.value = 'a'.repeat(98);
    p.mention('긴닉네임');
    out.afterMention = p.input.value.length;
    p.insertAtCaret('{:d_1:}');
    out.afterOverEmoji = p.input.value.length;
    return out;
  });
  assert.equal(r.afterEmoji.len, 92);
  assert.equal(r.afterEmoji.counter, '92/100', `글자 수가 바로 바뀌지 않았다 ${JSON.stringify(r.afterEmoji)}`);
  assert.equal(r.afterMention, 98, '멘션이 100자를 넘겼다');
  assert.equal(r.afterOverEmoji, 98, '이모티콘이 100자를 넘겼다');
  // 선택기 — 검색어 유지, ↓ 로 첫 칸, → 로 다음 칸
  const picker = await page.evaluate(async () => {
    const p = window.__panel;
    p.emoticons = [{ emoticonPackId: 'a', emoticonPackName: '팩', emoticons: Array.from({ length: 20 }, (_, i) => ({ emoticonId: `e${i}`, emoticonName: `e${i}`, imageUrl: `https://ssl.pstatic.net/e${i}.png` })) }];
    p.emoticonError = false;
    p.togglePicker();
    const search = p.picker.querySelector('.emoji-search');
    search.value = 'e1';
    search.dispatchEvent(new Event('input', { bubbles: true }));
    p.closePicker();
    await new Promise((res) => setTimeout(res, 300));
    p.togglePicker();
    const kept = p.picker.querySelector('.emoji-search').value;
    p.picker.querySelector('.emoji-search').value = '';
    p.renderPicker();
    const s2 = p.picker.querySelector('.emoji-search');
    s2.focus();
    s2.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    const first = document.activeElement?.dataset.emoji;
    document.activeElement.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
    const second = document.activeElement?.dataset.emoji;
    p.closePicker();
    p.emoticons = [];
    p.emoticonError = true;
    p.picker.hidden = false;
    p.renderPicker();
    const err = p.picker.querySelector('.tray-empty')?.textContent || '';
    p.picker.hidden = true;
    return { kept, first, second, err };
  });
  assert.equal(picker.kept, 'e1', '다시 열면 검색어가 지워졌다');
  assert.deepEqual([picker.first, picker.second], ['e0', 'e1'], `방향키 ${picker.first} → ${picker.second}`);
  assert.match(picker.err, /불러오지 못했습니다/);
  // 검색 0건 안내, 지우기 뒤 개수
  const search = await page.evaluate(async () => {
    const p = window.__panel;
    p.onEvent({ type: 'messages', messages: [window.__msg({ content: '안녕' }), window.__msg({ content: '반가워' })] });
    p.flushLines();
    p.openSearch();
    const input = p.root.querySelector('.cs-input');
    input.value = '없는말';
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    const empty = p.root.querySelector('.chat-search-empty');
    const none = { hidden: empty.hidden, text: empty.textContent };
    input.value = '안녕';
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    const one = p.root.querySelector('.cs-count').textContent;
    p.clear(true);
    const afterClear = p.root.querySelector('.cs-count').textContent;
    p.closeSearch();
    return { none, one, afterClear };
  });
  assert.equal(search.none.hidden, false, '0건 안내가 없다');
  assert.match(search.none.text, /없는말/);
  assert.equal(search.one, '1개');
  assert.equal(search.afterClear, '없음', `지운 뒤에도 ${search.afterClear}`);
  // /unblock 없는 이름 · /mute 는 붙이기 전에 거른다
  const cmd = await page.evaluate(async () => {
    const p = window.__panel;
    const run = (t) => p.sendHooks.some((fn) => fn(t));
    run('/unblock 아무개');
    run('/mute 시끄러운 60');
    let flashed = false;
    const mo = new MutationObserver((ms) => { for (const m of ms) for (const n of m.addedNodes) if (n.dataset?.nick === '시끄러운') flashed = true; });
    mo.observe(p.list, { childList: true });
    p.onEvent({ type: 'messages', messages: [window.__msg({ nickname: '시끄러운', content: '도배' }), window.__msg({ nickname: '조용한', content: '안녕' })] });
    p.flushLines();
    mo.disconnect();
    return { flashed, quiet: Boolean(p.list.querySelector('.chat-line[data-nick="조용한"]')) };
  });
  await page.waitForTimeout(100);
  const toasts = await page.evaluate(() => window.__toasts);
  assert.ok(toasts.some((t) => t.includes('차단 목록에') && t.includes('아무개')), `/unblock 안내 ${JSON.stringify(toasts)}`);
  assert.ok(!toasts.some((t) => t.includes('아무개 님 차단을 풀었습니다')), '없는 이름을 풀었다고 했다');
  assert.equal(cmd.flashed, false, '/mute 한 사람 줄이 붙었다가 지워졌다');
  assert.equal(cmd.quiet, true);
  // TTS 대기 상한 — 끝나지 않는 엔진에 후원 10개
  const spoken = await page.evaluate(async () => {
    const p = window.__panel;
    window.__spoken.length = 0;
    Object.defineProperty(window.speechSynthesis, 'speaking', { configurable: true, get: () => window.__spoken.length > 0 });
    Object.defineProperty(window.speechSynthesis, 'pending', { configurable: true, get: () => window.__spoken.length > 1 });
    p.onEvent({ type: 'messages', messages: Array.from({ length: 10 }, (_, i) => window.__msg({ kind: 'donation', donationAmount: 1000, nickname: `후원${i}` })) });
    p.flushLines();
    const { TTS_QUEUE_MAX } = await import('/js/chat-extras.js');
    return { n: window.__spoken.length, max: TTS_QUEUE_MAX };
  });
  assert.equal(spoken.n, spoken.max, `TTS 에 ${spoken.n}개를 쌓았다`);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('UR14 [P2-6] shortcut help hubs/bookmark once; settings tabs while searching; import textarea + broadcast', async () => {
  const { ctx, page, errors } = await openApp(browser, { width: 1280, height: 900 });
  const help = await page.evaluate(async () => {
    const { showShortcuts } = await import('/js/app-sheets.js');
    showShortcuts();
    const rows = [...document.querySelectorAll('.app-sheet .shortcut-row')].map((r) => r.textContent);
    document.querySelector('.app-sheet [data-close]').click();
    return rows;
  });
  assert.ok(help.some((t) => t.includes('⌥⌘1') && t.includes('작업 공간')), '⌥⌘1~6 이 도움말에 없다');
  assert.equal(help.filter((t) => t.includes('북마크') && /^B/.test(t)).length, 1);
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: 'settings', params: {} } })));
  await page.waitForSelector('.settings-search');
  await page.fill('.settings-search', '테마');
  await page.waitForTimeout(300);
  const tabs = await page.evaluate(() => ({ active: document.querySelectorAll('.settings-tabs .seg.active').length, ta: Boolean(document.querySelector('form[data-import] textarea')), note: document.querySelector('[data-search*="테마"] small')?.textContent || '' }));
  assert.equal(tabs.active, 0, '검색 중에 탭이 켜져 보인다');
  assert.match(tabs.note, /macOS|Windows/);
  await page.fill('.settings-search', '');
  await page.waitForTimeout(200);
  await page.click('.settings-tabs [data-group="advanced"]');
  await page.waitForSelector('form[data-import] textarea');
  const imp = await page.evaluate(async () => {
    let changed = null;
    window.addEventListener('cview:settings-changed', (e) => { changed = e.detail; }, { once: true });
    const inv = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async (c, a) => {
      if (c === 'settings_import') return [];
      if (c === 'settings_get') { const s = await inv(c, a); return { ...s, appearance: { theme: 'light' } }; }
      return inv(c, a);
    };
    const form = document.querySelector('form[data-import]');
    form.querySelector('textarea').value = '{\n  "appearance": { "theme": "light" }\n}';
    form.requestSubmit();
    await new Promise((res) => setTimeout(res, 300));
    return { changed: changed && changed.domain, theme: document.documentElement.dataset.theme };
  });
  assert.equal(imp.changed, 'all', '가져오기 뒤 설정 변경을 알리지 않았다');
  assert.equal(imp.theme, 'light', '가져온 테마가 바로 적용되지 않았다');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('UR15 [P2-7] preview source errors retry after 15s; follower lookups retry after 30s', async () => {
  const { ctx, page, errors } = await openApp(browser, { width: 1280, height: 900 });
  await page.evaluate(() => {
    window.__asked = { preview: [], info: 0 };
    const inv = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async (c, a) => {
      if (c === 'preview_source') { window.__asked.preview.push(a.kind); if (a.kind === 'live') throw new Error('네트워크'); return null; }
      if (c === 'channel_info') { window.__asked.info += 1; throw new Error('네트워크'); }
      return inv(c, a);
    };
  });
  await page.clock.install();
  await page.evaluate(async () => {
    const { previewInto } = await import('/js/hover-preview.js');
    const host = document.createElement('div');
    document.body.append(host);
    previewInto(host, 'live', 'x1');
    await new Promise((res) => setTimeout(res, 0));
    previewInto(host, 'vod', 'v1');
    const { FollowExtras } = await import('/js/following-info.js');
    window.__fx = new FollowExtras();
    window.__fx.want('ch9', true);
  });
  await page.clock.runFor(1000);
  await page.clock.runFor(16000);
  await page.evaluate(async () => {
    const { previewInto } = await import('/js/hover-preview.js');
    const host = document.createElement('div');
    document.body.append(host);
    previewInto(host, 'live', 'x1');
    await new Promise((res) => setTimeout(res, 0));
    previewInto(host, 'vod', 'v1');
  });
  await page.clock.runFor(16000); // 팔로워 조회 실패 뒤 32초
  await page.evaluate(() => {
    const list = document.createElement('div');
    list.innerHTML = '<div data-fid="ch9" data-live="1" style="height:20px"><span class="fx-followers"></span></div>';
    document.body.prepend(list);
    window.__fx.attach(list);
  });
  await page.clock.runFor(2000);
  await page.waitForTimeout(400); // IntersectionObserver 는 실제 그리기 때 알린다
  await page.clock.runFor(100);
  const asked = await page.evaluate(() => window.__asked);
  assert.equal(asked.preview.filter((k) => k === 'live').length, 2, `오류 난 미리보기를 다시 묻지 않았다 ${asked.preview}`);
  assert.equal(asked.preview.filter((k) => k === 'vod').length, 1, `미리보기 없음(null)을 다시 물었다 ${asked.preview}`);
  assert.equal(asked.info, 2, `실패한 팔로워 조회를 다시 묻지 않았다 ${asked.info}`);
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
