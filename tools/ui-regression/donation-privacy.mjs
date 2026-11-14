// 익명 후원·금액 비공개 후원 표시 회귀 검사(모의 IPC, 헤드리스 Chromium) — donations.js · chat.js · chat-extras.js.
// 백엔드(dto.rs)는 익명 후원자를 userId '' · nickname '익명' · anonymous true 로, 금액 비공개를 donationAmount null 로 보낸다.
// DP01 금액 표시 순수 함수 — 금액이 null·undefined·0·NaN 이면 금액 자리가 빈 문자열이다('0치즈'·'후원 메시지'·'NaN' 없음).
//      읽어 주기 문장도 금액 없이 자연스럽다(빈 칸이 두 번 들어가지 않는다).
// DP02 채팅 카드 네 경우 — 익명+공개, 실명+비공개, 익명+비공개, 일반. 비공개면 .ev-value 가 아예 없고, 익명이면 닉네임 자리에
//      '익명' 하나만(이름·해시·'Unknown' 없음, 누를 수 있는 모양 아님). 일반 후원은 예전처럼 이름과 금액.
// DP03 익명 줄의 사용자 동작 — 닉네임·카드를 누르거나 더블클릭·우클릭해도 말풍선·줄 메뉴·동작 막대가 뜨지 않고 오류도 없다.
//      /block 익명 은 '찾지 못했습니다'로 끝난다. 실명 줄은 예전처럼 말풍선이 뜬다.
// DP04 영상 위 알림·후원 목록(채팅 머리의 후원 단추, donation-list.js — 예전 이벤트 칩 줄) — 비공개면 금액 <b>·목록 금액 칸이 없고,
//      익명은 '익명'.
// DP05 통합(멀티)채팅 줄도 같은 카드 규칙.
// 저장소 루트에서: node tools/ui-regression/donation-privacy.mjs   (Playwright 가 필요하다. CVIEW_UI_SHOTS=<폴더> 면 PNG 를 남긴다)
//   CVIEW_UI_BROWSER=webkit 이면 앱과 같은 WebKit 엔진으로 돈다.
import assert from 'node:assert/strict';
import path from 'node:path';
import { createRequire } from 'node:module';
import { server, base, mockTauri } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const engine = process.env.CVIEW_UI_BROWSER === 'webkit' ? pw.webkit : pw.chromium;
const SHOTS = process.env.CVIEW_UI_SHOTS;
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}
const BAD = /(^|[^\d,])0치즈|undefined|NaN|null|Unknown|후원 메시지/;

// 백엔드가 보내는 모양 그대로(ChatMessageDto). 익명은 userId 를 비운다.
const CASES = {
  anonPublic: { id: 'd-ap', kind: 'donation', userId: '', nickname: '익명', anonymous: true, donationAmount: 2000, donationType: 'chat', content: '익명이지만 금액은 공개' },
  namedPrivate: { id: 'd-np', kind: 'donation', userId: 'u-np', nickname: '비공개후원자', anonymous: false, donationAmount: null, donationType: 'video', content: '금액은 비밀' },
  anonPrivate: { id: 'd-xp', kind: 'donation', userId: '', nickname: '익명', anonymous: true, donationAmount: null, donationType: 'mission', missionText: '노래 한 곡', content: '' },
  normal: { id: 'd-n', kind: 'donation', userId: 'u-n', nickname: '일반후원자', anonymous: false, donationAmount: 5000, donationType: 'chat', content: '늘 응원해요' },
};

const browser = await engine.launch();

async function openChat({ width = 380 } = {}) {
  const ctx = await browser.newContext({ viewport: { width: width + 400, height: 900 } });
  const page = await ctx.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e.stack || e)));
  await page.addInitScript(mockTauri, [base, 'dark']);
  await page.addInitScript(() => {
    window.__cards = [];
    const inv = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async (c, a) => {
      if (c === 'chat_connect') return { canSend: true };
      if (c === 'chat_avatars') return a.userIds.map(() => null);
      if (c === 'chat_donation_ranking') return [];
      if (c === 'chat_user_card') { window.__cards.push(a.userId); return { userId: a.userId, nickname: '실명', followDate: null }; }
      try { return await inv(c, a); } catch { return null; }
    };
  });
  await page.goto(base);
  await page.waitForFunction(() => !document.getElementById('splash') || !document.getElementById('splash').isConnected, null, { timeout: 20000 });
  await page.evaluate(async (w) => {
    document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove());
    const { ChatPanel } = await import('/js/chat.js');
    const { loadChatPrefs, setMyNickname } = await import('/js/chat-prefs.js');
    await loadChatPrefs();
    setMyNickname('도도');
    const src = new DOMParser().parseFromString(await (await fetch('/index.html')).text(), 'text/html');
    const box = document.createElement('div');
    box.id = 'dpbox';
    box.style.cssText = `position:fixed;left:20px;top:10px;width:${w}px;height:860px;z-index:50;display:flex;flex-direction:column`;
    box.innerHTML = src.querySelector('#viewer .chat').outerHTML;
    document.body.appendChild(box);
    const root = box.firstElementChild;
    root.hidden = false;
    root.style.cssText = 'display:flex;flex-direction:column;height:100%;width:100%';
    const panel = new ChatPanel(root, { isLoggedIn: () => true });
    window.__panel = panel;
    await panel.connect('cc1', 'ch1');
    panel.onEvent({ type: 'connected' });
    panel.active = true;
    const t = Date.now();
    let k = 0;
    window.__say = (list) => { panel.append(list.map((o) => ({ role: 'common_user', badges: [], emojis: {}, timestamp: t + (++k) * 1000, ...o }))); panel.flushLines(); };
  }, width);
  return { ctx, page, errors };
}

// ── DP01 순수 함수 ──
await check('DP01 private amount renders as empty, never 0/NaN/undefined; TTS has no double space', async () => {
  const { ctx, page, errors } = await openChat();
  const r = await page.evaluate(async () => {
    const { formatCheese, eventMeta } = await import('/js/donations.js');
    const { ttsText } = await import('/js/chat-extras.js');
    const amounts = [null, undefined, 0, NaN, -5, Infinity];
    return {
      cheese: amounts.map((a) => formatCheese(a)),
      values: amounts.map((a) => eventMeta({ kind: 'donation', donationAmount: a }).value),
      levels: amounts.map((a) => eventMeta({ kind: 'donation', donationAmount: a }).level),
      shown: eventMeta({ kind: 'donation', donationAmount: 12000 }).value,
      tts: [
        ttsText({ kind: 'donation', nickname: '익명', donationAmount: null, donationType: 'chat', content: '안녕' }),
        ttsText({ kind: 'donation', nickname: '도도', donationAmount: 1000, donationType: 'video', content: '' }),
      ],
    };
  });
  assert.deepEqual(r.cheese, ['', '', '', '', '', '']);
  assert.deepEqual(r.values, ['', '', '', '', '', '']);
  assert.ok(r.levels.every((l) => l === 1), `비공개 금액 색 단계: ${r.levels}`);
  assert.equal(r.shown, '12,000치즈');
  assert.equal(r.tts[0], '익명님이 채팅 후원했습니다. 안녕');
  assert.equal(r.tts[1], '도도님이 1,000치즈 영상 후원했습니다. ');
  assert.deepEqual(errors, []);
  await ctx.close();
});

// ── DP02 채팅 카드 네 경우 ──
await check('DP02 chat cards: anonymous shows 익명 only, private amount has no value slot, normal unchanged', async () => {
  const { ctx, page, errors } = await openChat();
  await page.evaluate((cases) => window.__say(Object.values(cases)), CASES);
  const r = await page.evaluate((ids) => Object.fromEntries(ids.map((id) => {
    const line = window.__panel.list.querySelector(`[data-id="${id}"]`);
    const head = line.querySelector('.ev-head');
    return [id, {
      uid: line.dataset.uid,
      value: line.querySelector('.ev-value')?.textContent ?? null,
      headKids: [...head.children].map((c) => c.className),
      emptyKids: [...line.querySelectorAll('.ev-card span, .ev-card b')].filter((el) => !el.children.length && !el.textContent.trim() && !el.closest('.ev-icon')).map((el) => el.className),
      nick: line.querySelector('.ev-from .nick')?.textContent ?? null,
      anon: line.querySelectorAll('.ev-from .nick.anon').length,
      cursor: getComputedStyle(line.querySelector('.ev-from .nick')).cursor,
      text: line.textContent,
    }];
  })), Object.values(CASES).map((c) => c.id));
  if (SHOTS) await page.locator('#dpbox').screenshot({ path: path.join(SHOTS, 'donation-privacy-chat.png') });
  const ap = r['d-ap'], np = r['d-np'], xp = r['d-xp'], n = r['d-n'];
  // 익명 + 금액 공개
  assert.equal(ap.value, '2,000치즈');
  assert.equal(ap.nick, '익명', '익명이면 닉네임 자리에 익명');
  assert.equal(ap.anon, 1);
  assert.equal(ap.cursor, 'auto', '익명 닉네임이 누를 수 있는 모양');
  assert.equal(ap.uid, '');
  // 실명 + 금액 비공개
  assert.equal(np.value, null, '비공개 금액 칸이 남아 있음');
  assert.ok(!np.headKids.includes('ev-value'));
  assert.equal(np.nick, '비공개후원자');
  assert.equal(np.anon, 0);
  // 익명 + 비공개
  assert.equal(xp.value, null);
  assert.equal(xp.anon, 1);
  assert.equal(xp.nick, '익명');
  // 일반
  assert.equal(n.value, '5,000치즈');
  assert.equal(n.nick, '일반후원자');
  assert.equal(n.anon, 0);
  assert.equal(n.cursor, 'pointer');
  for (const [id, x] of Object.entries(r)) {
    assert.ok(!BAD.test(x.text), `${id} 에 잘못된 글자: ${x.text}`);
    assert.deepEqual(x.emptyKids, [], `${id} 에 빈 칸: ${x.emptyKids}`);
  }
  assert.deepEqual(errors, []);
  await ctx.close();
});

// ── DP03 익명 줄의 사용자 동작 ──
await check('DP03 anonymous lines open no user pop, menu or action bar; named lines still do', async () => {
  const { ctx, page, errors } = await openChat();
  await page.evaluate((cases) => window.__say(Object.values(cases)), CASES);
  const menus = () => page.evaluate(() => [...document.querySelectorAll('.ctx-menu, [role="menu"]')].filter((m) => m.offsetParent !== null || getComputedStyle(m).position === 'fixed' && !m.hidden).length);
  const menusBefore = await menus();
  const line = page.locator('#dpbox [data-id="d-ap"]');
  await line.locator('.ev-from .nick').click();
  await line.locator('.ev-card').dblclick();
  // 앱은 ID 없는 줄의 우클릭을 가로채지 않는다(줄 메뉴 없음 — 아래에서 센다). 그러면 WebKit 은 엔진 기본 문맥 메뉴를 띄우고,
  // 그 메뉴가 열린 동안 다음 클릭은 페이지에 닿지 않는다(macOS 메뉴처럼 닫기만 한다 — Esc 로도 안 닫힌다). 아래 실명 줄 클릭이
  // 삼켜지지 않게, 앱 처리기가 모두 돈 뒤(문서 버블 끝) 이번 우클릭의 기본 동작만 막는다. Chromium(헤드리스)은 메뉴가 없다.
  await page.evaluate(() => document.addEventListener('contextmenu', (e) => e.preventDefault(), { once: true }));
  await line.locator('.ev-card').click({ button: 'right' });
  await line.hover();
  const anon = await page.evaluate(() => ({
    pop: document.querySelectorAll('.user-pop').length,
    bar: Boolean(document.querySelector('[data-id="d-ap"] .chat-actions')),
    cards: window.__cards.length,
  }));
  assert.deepEqual(anon, { pop: 0, bar: false, cards: 0 });
  assert.equal(await menus(), menusBefore, '익명 줄 우클릭에 줄 메뉴가 뜸');
  // /block 익명 — 익명 줄에는 ID 가 없어 차단할 사람을 찾지 못한다
  await page.evaluate(() => { const i = window.__panel.input; i.value = '/block 익명'; i.focus(); });
  await page.keyboard.press('Enter');
  await page.waitForTimeout(100);
  const after = await page.evaluate(() => ({
    lines: window.__panel.list.querySelectorAll('[data-id="d-ap"], [data-id="d-xp"]').length,
    toast: document.getElementById('toast-stack')?.textContent || '',
  }));
  assert.equal(after.lines, 2, '익명 후원 줄이 지워짐(차단이 걸림)');
  assert.match(after.toast, /찾지 못했습니다/);
  // 실명 줄은 말풍선이 뜬다
  await page.locator('#dpbox [data-id="d-np"] .ev-from .nick').click();
  await page.waitForSelector('.user-pop', { timeout: 3000 });
  assert.deepEqual(await page.evaluate(() => window.__cards), ['u-np']);
  assert.deepEqual(errors, []);
  await ctx.close();
});

// ── DP04 영상 위 알림·이벤트 칩 ──
await check('DP04 stream alerts and donation list items drop the amount slot when private', async () => {
  const { ctx, page, errors } = await openChat();
  await page.evaluate((cases) => window.__say(Object.values(cases)), CASES);
  const r = await page.evaluate(async (cases) => {
    const { StreamAlerts, setStreamAlertsEnabled } = await import('/js/donations.js');
    setStreamAlertsEnabled(true);
    const stage = document.createElement('div');
    stage.className = 'player-stage';
    stage.id = 'dpstage';
    stage.style.cssText = 'position:fixed;left:420px;top:10px;width:720px;height:405px;background:#111;z-index:60';
    document.body.append(stage);
    const alerts = new StreamAlerts(stage);
    alerts.show = (item) => alerts.el.append(item); // 자동 닫기 없이 모두 남긴다
    const out = {};
    for (const [k, m] of Object.entries(cases)) {
      alerts.push(m);
      const card = alerts.el.lastElementChild;
      out[k] = { b: card.querySelector('.stream-alert-title b')?.textContent ?? null, nick: card.querySelector('.stream-alert-nick').textContent, text: card.textContent };
    }
    window.__panel.donations.open();
    const chips = [...window.__panel.root.querySelectorAll('.dp-panel .dp-item')].map((c) => ({ nick: c.querySelector('.dp-nick').textContent, value: c.querySelector('.dp-value')?.textContent ?? null }));
    return { out, chips };
  }, CASES);
  if (SHOTS) await page.locator('#dpstage').screenshot({ path: path.join(SHOTS, 'donation-privacy-alerts.png') });
  assert.deepEqual(r.out.anonPublic.b, '2,000치즈');
  assert.equal(r.out.anonPublic.nick, '익명');
  assert.equal(r.out.namedPrivate.b, null);
  assert.equal(r.out.namedPrivate.nick, '비공개후원자');
  assert.equal(r.out.anonPrivate.b, null);
  assert.equal(r.out.anonPrivate.nick, '익명');
  assert.equal(r.out.normal.b, '5,000치즈');
  for (const [k, x] of Object.entries(r.out)) assert.ok(!BAD.test(x.text), `${k} 알림에 잘못된 글자: ${x.text}`);
  // 목록은 최신이 앞
  assert.deepEqual(r.chips, [
    { nick: '일반후원자', value: '5,000치즈' },
    { nick: '익명', value: null },
    { nick: '비공개후원자', value: null },
    { nick: '익명', value: '2,000치즈' },
  ]);
  assert.deepEqual(errors, []);
  await ctx.close();
});

// ── DP05 통합 채팅 ──
await check('DP05 merged (multi) chat cards follow the same rules', async () => {
  const { ctx, page, errors } = await openChat();
  const r = await page.evaluate(async (cases) => {
    const { mergedLinesHtml } = await import('/js/multichat.js');
    const box = document.createElement('div');
    box.innerHTML = mergedLinesHtml(Object.values(cases).map((m) => ({ ...m, role: 'common_user', timestamp: Date.now() })), { name: '채널', style: '' });
    return [...box.querySelectorAll('.chat-line')].map((l) => ({ value: l.querySelector('.ev-value')?.textContent ?? null, nick: l.querySelector('.ev-from .nick')?.textContent ?? null, anon: l.querySelectorAll('.ev-from .nick.anon').length, text: l.textContent }));
  }, CASES);
  assert.deepEqual(r.map(({ value, nick, anon }) => ({ value, nick, anon })), [
    { value: '2,000치즈', nick: '익명', anon: 1 },
    { value: null, nick: '비공개후원자', anon: 0 },
    { value: null, nick: '익명', anon: 1 },
    { value: '5,000치즈', nick: '일반후원자', anon: 0 },
  ]);
  for (const x of r) assert.ok(!BAD.test(x.text), x.text);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await browser.close();
server.close();
for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : `\n     ${r.error}`}`);
const failed = results.filter((r) => !r.ok).length;
console.log(`\n${results.length - failed}/${results.length} passed`);
process.exit(failed ? 1 : 0);
