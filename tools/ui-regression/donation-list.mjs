// 후원 목록(채팅 머리의 후원 단추 + 목록 패널) 회귀 검사 — 모의 IPC·헤드리스 Chromium(실제 로그인·채팅·후원 전송 없음).
// 예전엔 채팅 머리 아래에 후원 칩 띠(.chat-events)·주간 랭킹 줄이 늘 붙어 채팅 자리를 차지했다(donation-list.js 머리말).
// DL01 위쪽 띠 없음 — 후원이 와도 머리 아래에 칩 띠·랭킹 줄이 없고 채팅 목록이 머리 바로 아래서 시작한다. 랭킹은 패널 안에 있다.
// DL02 배지 — 지금 막 온 후원·구독만 센다(입장 때 받은 지난 기록·일반 채팅 제외), 열면 0, 열려 있는 동안은 세지 않는다,
//      100건 넘으면 '99+'. 단추 이름(aria-label)에 새 후원 수, 새 후원이 오면 단추가 반짝인다(.ping).
// DL03 열기·닫기 — role=dialog·aria-modal·이름, aria-expanded, 채팅 머리 아래·채팅 안에 뜬다. ✕·바깥 누르기·Esc 로 닫히고,
//      Esc 는 시청 화면 닫기(문서 Esc 처리)까지 가지 않는다. ✕·Esc 면 단추로 초점이 돌아온다.
// DL04 정렬·거르기 — 종류별 건수, 거르기(후원·미션·구독), 합계 글, 금액순(같으면 최신), aria-pressed.
// DL05 표시 규칙 유지 — 익명은 '익명', 비공개 금액은 금액 칸 없음('0치즈'·NaN 등 없음), 금액 색 단계(ev-lv)가 채팅 카드와 같다.
//      채팅 줄 후원 강조(.chat-line.ev)와 영상 위 알림 호출(onHighlight)도 그대로.
// DL06 키보드 — Enter 로 열면 첫 항목에 초점, ↑/↓·Home/End, Tab·⇧Tab 은 패널 안에서 돈다, Enter 로 항목 → 채팅 줄로 옮겨 강조하고
//      초점은 채팅 목록(그 줄을 고른 상태), Esc 로 닫으면 단추로.
// DL07 채팅 줄 이동 — 이 사람만 보기로 숨은 줄이면 거르기를 풀고 간다. 목록에서 밀려난 줄은 '지나감' 표시·aria-disabled·알림.
// DL08 좁은 폭·테마·오버레이 — 240·360px, 다크·라이트, 영상 위 채팅(라이트)에서 패널이 채팅 안에 들어가고 가로로 넘치지 않으며
//      글자 대비(axe color-contrast)를 지킨다.
// DL09 설정 '위쪽 띠도 보이기'(chat.donationList='strip') — 예전 띠·랭킹 줄이 머리 아래에 돌아오고 단추는 남는다. 되돌리면 걷힌다.
// DL10 멀티라이브 멀티채팅(통합) — 모든 패인의 후원을 채널 이름과 함께 모으고(선택 패인 세션과 겹치지 않게), 패인을 바꿔도
//      비우지 않으며, 항목을 누르면 통합 목록의 그 줄로 간다. 멀티채팅을 끄면 다시 선택 패인 채팅으로.
// 저장소 루트에서: node tools/ui-regression/donation-list.mjs   (Playwright 가 필요하다. axe-core 가 있으면 DL08 대비도 잰다.
//   CVIEW_UI_SHOTS=<폴더> 면 PNG 를 남긴다. CVIEW_UI_BROWSER=webkit 이면 앱과 같은 WebKit 엔진으로 돈다.)
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, base, mockTauri } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const engine = process.env.CVIEW_UI_BROWSER === 'webkit' ? pw.webkit : pw.chromium;
let AXE = null;
try { AXE = fs.readFileSync(require.resolve('axe-core/axe.min.js'), 'utf8'); } catch { /* 대비 검사는 건너뛴다 */ }
const SHOTS = process.env.CVIEW_UI_SHOTS;
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}
const BAD = /(^|[^\d,])0치즈|undefined|NaN|null|Unknown|후원 메시지/;

// 백엔드 ChatMessageDto 모양 — 익명은 userId '' · nickname '익명' · anonymous true, 금액 비공개는 donationAmount null.
const DONATIONS = [
  { id: 'd1', kind: 'donation', userId: '', nickname: '익명', anonymous: true, donationAmount: 2000, donationType: 'video', content: '영상 하나 보고 가요' },
  { id: 'd2', kind: 'donation', userId: 'u-nera', nickname: 'NEra', donationAmount: null, donationType: 'chat', content: '금액은 비밀' },
  { id: 'd3', kind: 'donation', userId: '', nickname: '익명', anonymous: true, donationAmount: 4200, donationType: 'video', content: '' },
  { id: 'd4', kind: 'donation', userId: '', nickname: '익명', anonymous: true, donationAmount: 3600, donationType: 'video', content: '' },
  { id: 's1', kind: 'subscription', userId: 'u-sub', nickname: '구독자', subscriptionMonths: 6, subscriptionTier: '티어 1', content: '6개월째!' },
  { id: 'd5', kind: 'donation', userId: 'u-big', nickname: '큰손', donationAmount: 50000, donationType: 'chat', content: '오늘도 응원합니다' },
  { id: 'd6', kind: 'donation', userId: 'u-mis', nickname: '미션왕', donationAmount: 10000, donationType: 'mission', missionText: '노래 한 곡', content: '' },
];
const RANKING = [
  { rank: 1, nickname: '랭킹1위', amount: 120000, profileImageUrl: null },
  { rank: 2, nickname: '랭킹2위', amount: 80000, profileImageUrl: null },
];

const browser = await engine.launch();

/** 실제 index.html 의 채팅 마크업으로 ChatPanel 을 만든다. overlay — 시청 화면 '영상 위 채팅' 안에 둔다. */
async function openChat({ width = 360, height = 720, theme = 'dark', overlay = false, prefs = {} } = {}) {
  const ctx = await browser.newContext({ viewport: { width: width + 400, height: height + 60 }, colorScheme: theme });
  const page = await ctx.newPage();
  page.setDefaultTimeout(5000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e.stack || e)));
  await page.addInitScript(mockTauri, [base, theme]);
  await page.addInitScript((ranking) => {
    try { localStorage.setItem('cview.welcomed.v1', String(Date.now())); localStorage.setItem('cview.onboarded', '1'); } catch { /* 무시 */ }
    window.__highlights = [];
    window.__escapes = 0;
    const inv = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async (c, a) => {
      if (c === 'chat_connect') return { canSend: true };
      if (c === 'chat_disconnect') return null;
      if (c === 'chat_avatars') return a.userIds.map(() => null);
      if (c === 'chat_donation_ranking') return ranking;
      if (c === 'chat_user_card') return { userId: a.userId, nickname: '실명', followDate: null };
      if (c === 'settings_update') return null;
      try { return await inv(c, a); } catch { return null; }
    };
  }, RANKING);
  await page.goto(base);
  await page.waitForFunction(() => !document.getElementById('splash') || !document.getElementById('splash').isConnected, null, { timeout: 20000 });
  await page.evaluate(async ({ w, h, overlay, prefs }) => {
    document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove());
    const { ChatPanel } = await import('/js/chat.js');
    const { loadChatPrefs, setMyNickname, updateChatPrefs } = await import('/js/chat-prefs.js');
    await loadChatPrefs();
    if (Object.keys(prefs).length) await updateChatPrefs(prefs);
    setMyNickname('도도');
    const src = new DOMParser().parseFromString(await (await fetch('/index.html')).text(), 'text/html');
    const box = document.createElement('div');
    box.id = 'dlbox';
    box.style.cssText = `position:fixed;left:20px;top:20px;width:${w}px;height:${h}px;z-index:50;display:flex;flex-direction:column;${overlay ? 'background:linear-gradient(135deg,#3a6,#246 60%,#a54)' : ''}`;
    box.innerHTML = overlay
      ? `<div class="viewer chat-mode-overlay" style="position:absolute;inset:0;display:block">${src.querySelector('#viewer .chat').outerHTML}</div>`
      : src.querySelector('#viewer .chat').outerHTML;
    document.body.appendChild(box);
    const root = box.querySelector('.chat');
    root.hidden = false;
    root.style.cssText = overlay ? `position:absolute;right:12px;top:12px;bottom:12px;width:${w - 24}px` : 'display:flex;flex-direction:column;height:100%;width:100%';
    const panel = new ChatPanel(root, { isLoggedIn: () => true, onHighlight: (m) => window.__highlights.push(m.id) });
    window.__panel = panel;
    // 시청 화면의 Esc 처리(app.js)처럼 문서에서 Esc 를 받는 처리 — 후원 목록의 Esc 가 여기까지 오면 안 된다.
    document.addEventListener('keydown', (e) => { if (e.key === 'Escape') window.__escapes += 1; });
    await panel.connect('cc1', 'ch1');
    panel.onEvent({ type: 'connected' });
    const t = new Date(2026, 10, 13, 21, 0).getTime();
    let k = 0;
    const msg = (o) => ({ role: 'common_user', badges: [], emojis: {}, timestamp: t + (++k) * 30000, ...o });
    /** 채팅 이벤트처럼 흘린다 — recent 는 입장 때 받은 지난 기록. */
    window.__say = (list, recent = false) => { panel.onEvent({ type: 'messages', recent, messages: list.map(msg) }); panel.flushLines(); };
    window.__chat = (n, prefix = 'c') => window.__say(Array.from({ length: n }, (_, i) => ({ id: `${prefix}${i}-${k}`, kind: 'normal', userId: `cu${i % 5}`, nickname: `시청자${i % 5}`, content: `채팅 ${i}` })));
  }, { w: width, h: height, overlay, prefs });
  // 랭킹은 연결과 함께 따로 불러온다
  await page.waitForTimeout(50);
  return { ctx, page, errors };
}

const btn = (page) => page.locator('#dlbox .chat-header .dp-btn');
const state = (page) => page.evaluate(() => {
  const root = document.querySelector('#dlbox .chat');
  const b = root.querySelector('.dp-btn');
  const p = root.querySelector('.dp-panel:not(.exit-ghost)');
  return {
    btn: Boolean(b),
    badge: b && !b.querySelector('.dp-badge').hidden ? b.querySelector('.dp-badge').textContent : '',
    label: b ? b.getAttribute('aria-label') : '',
    expanded: b ? b.getAttribute('aria-expanded') : null,
    open: Boolean(p && !p.hidden),
    active: document.activeElement === b ? 'button' : p && p.contains(document.activeElement) ? `panel:${document.activeElement.className}` : root.querySelector('.chat-list') === document.activeElement ? 'list' : document.activeElement?.tagName,
  };
});
// 닫힐 때 잠깐 남는 모션 복제본(dom.js hideWithExit, .exit-ghost)은 빼고 본다.
const PANEL = '#dlbox .dp-panel:not(.exit-ghost)';
const items = (page) => page.evaluate((sel) => [...document.querySelectorAll(`${sel}:not([hidden]) .dp-item`)].map((b) => ({
  id: b.dataset.dp,
  nick: b.querySelector('.dp-nick').textContent,
  anon: b.querySelector('.dp-nick').classList.contains('anon'),
  value: b.querySelector('.dp-value')?.textContent ?? null,
  kind: b.querySelector('.dp-kind').textContent,
  time: b.querySelector('.dp-time').textContent,
  msg: b.querySelector('.dp-msg')?.textContent ?? '',
  lv: [...b.classList].find((c) => /^ev-lv-\d$/.test(c)),
  gone: b.classList.contains('gone'),
  text: b.textContent,
})), PANEL);

// ── DL01 ──
await check('DL01 no donation strip under the chat header; list starts right below it; ranking lives in the panel', async () => {
  const { ctx, page, errors } = await openChat();
  await page.evaluate((d) => { window.__chat(4); window.__say(d); window.__chat(3); }, DONATIONS);
  await page.waitForTimeout(100);
  const r = await page.evaluate(() => {
    const root = document.querySelector('#dlbox .chat');
    const header = root.querySelector('.chat-header');
    const list = root.querySelector('.chat-list');
    const visible = (el) => Boolean(el && !el.hidden && el.getClientRects().length > 0 && !el.closest('[hidden]'));
    const strip = root.querySelector('.chat-events');
    const rank = root.querySelector('.donation-rank');
    // 머리와 목록 사이에 보이는 것(공지·고정·이 사람만 보기 줄은 지금 숨어 있다)
    const between = [];
    for (let n = header.nextElementSibling; n && n !== list; n = n.nextElementSibling) if (visible(n) && !n.matches('.dp-panel, .exit-ghost')) between.push(n.className);
    return {
      strip: visible(strip),
      chips: [...root.querySelectorAll('.event-chip')].filter(visible).length,
      rankVisible: visible(rank),
      rankInPanel: Boolean(rank && rank.closest('.dp-panel')),
      rankLoaded: Boolean(rank && rank.querySelector('.rank-chip')),
      between,
      gap: Math.round(list.getBoundingClientRect().top - header.getBoundingClientRect().bottom),
      btnInHeader: Boolean(header.querySelector('.dp-btn')),
    };
  });
  if (SHOTS) await page.locator('#dlbox').screenshot({ path: path.join(SHOTS, 'donation-list-closed.png') });
  assert.equal(r.strip, false, '머리 아래에 후원 칩 띠가 보인다');
  assert.equal(r.chips, 0, '보이는 후원 칩이 있다');
  assert.equal(r.rankVisible, false, '주간 랭킹 줄이 머리 아래에 보인다');
  assert.deepEqual(r.between, [], `머리와 채팅 목록 사이에 보이는 띠: ${r.between}`);
  assert.ok(r.gap <= 1, `채팅 목록이 머리 아래 ${r.gap}px 떨어져 시작한다`);
  assert.equal(r.btnInHeader, true, '머리에 후원 단추가 없다');
  assert.equal(r.rankLoaded, true, '랭킹을 불러오지 않았다');
  assert.equal(r.rankInPanel, true, '랭킹이 후원 목록 패널 안에 없다');
  // 열면 랭킹이 패널 안에 보인다
  await btn(page).click();
  const inPanel = await page.evaluate(() => [...document.querySelectorAll('#dlbox .dp-panel .rank-chip .rank-name')].map((x) => x.textContent));
  assert.deepEqual(inPanel, ['랭킹1위', '랭킹2위']);
  assert.deepEqual(errors, []);
  await ctx.close();
});

// ── DL02 ──
await check('DL02 badge counts only fresh donations/subscriptions, resets on open, ignores while open, caps at 99+', async () => {
  const { ctx, page, errors } = await openChat();
  assert.equal((await state(page)).btn, true, '후원 단추가 없다');
  const s0 = await state(page);
  assert.equal(s0.badge, '');
  assert.match(s0.label, /^후원 목록/);
  // 입장 때 받은 지난 기록의 후원은 목록엔 넣되 배지에는 세지 않는다
  await page.evaluate((d) => window.__say(d.slice(0, 2), true), DONATIONS);
  assert.equal((await state(page)).badge, '', '지난 기록 후원을 새 후원으로 셌다');
  await page.evaluate(() => window.__chat(5));
  assert.equal((await state(page)).badge, '', '일반 채팅을 셌다');
  await page.evaluate((d) => window.__say(d.slice(2, 5)), DONATIONS);
  await page.waitForTimeout(60);
  const s1 = await state(page);
  assert.equal(s1.badge, '3');
  assert.match(s1.label, /새 후원 3건/);
  assert.equal(await page.evaluate(() => document.querySelector('#dlbox .dp-btn').classList.contains('ping')), true, '새 후원에 단추가 반짝이지 않는다');
  // 열면 0, 지난 기록 2건 + 새 3건이 모두 목록에
  await btn(page).click();
  const s2 = await state(page);
  assert.equal(s2.badge, '');
  assert.equal(s2.open, true);
  assert.equal((await items(page)).length, 5);
  // 열려 있는 동안 온 후원은 목록에 바로 보이고 배지는 그대로 0
  await page.evaluate((d) => window.__say(d.slice(5, 6)), DONATIONS);
  assert.equal((await state(page)).badge, '');
  assert.equal((await items(page)).length, 6);
  await page.keyboard.press('Escape');
  await page.evaluate((d) => window.__say(d.slice(6)), DONATIONS);
  assert.equal((await state(page)).badge, '1');
  // 99+
  await page.evaluate(() => window.__say(Array.from({ length: 120 }, (_, i) => ({ id: `m${i}`, kind: 'donation', userId: `x${i}`, nickname: `후원자${i}`, donationAmount: 1000, donationType: 'chat', content: '' }))));
  assert.equal((await state(page)).badge, '99+');
  assert.match((await state(page)).label, /새 후원 121건/);
  assert.deepEqual(errors, []);
  await ctx.close();
});

// ── DL03 ──
await check('DL03 open/close: dialog semantics, placement under the header inside the chat, ✕ / outside / Esc close', async () => {
  const { ctx, page, errors } = await openChat();
  await page.evaluate((d) => { window.__chat(6); window.__say(d); }, DONATIONS);
  await btn(page).click();
  const r = await page.evaluate(() => {
    const root = document.querySelector('#dlbox .chat');
    const p = root.querySelector('.dp-panel');
    const b = root.querySelector('.dp-btn');
    const rr = root.getBoundingClientRect();
    const pr = p.getBoundingClientRect();
    const hr = root.querySelector('.chat-header').getBoundingClientRect();
    return {
      role: p.getAttribute('role'), modal: p.getAttribute('aria-modal'), name: document.getElementById(p.getAttribute('aria-labelledby'))?.textContent,
      controls: b.getAttribute('aria-controls') === p.id, haspopup: b.getAttribute('aria-haspopup'), expanded: b.getAttribute('aria-expanded'),
      inside: pr.left >= rr.left && pr.right <= rr.right + 0.5 && pr.bottom <= rr.bottom + 0.5, below: pr.top >= hr.bottom - 0.5,
      tall: pr.height > 200,
    };
  });
  if (SHOTS) await page.locator('#dlbox').screenshot({ path: path.join(SHOTS, 'donation-list-open.png') });
  assert.deepEqual(r, { role: 'dialog', modal: 'true', name: '후원 목록', controls: true, haspopup: 'dialog', expanded: 'true', inside: true, below: true, tall: true });
  // ✕ — 닫고 단추로 초점
  await page.locator(`${PANEL} .dp-close`).click();
  let s = await state(page);
  assert.equal(s.open, false);
  assert.equal(s.expanded, 'false');
  assert.equal(s.active, 'button', `✕ 뒤 초점: ${s.active}`);
  // 바깥(채팅 목록) 누르기
  await btn(page).click();
  assert.equal((await state(page)).open, true);
  await page.locator('#dlbox .chat-input input').click();
  s = await state(page);
  assert.equal(s.open, false, '바깥을 눌러도 닫히지 않는다');
  // Esc — 닫고 단추로, 문서의 Esc 처리(시청 화면 닫기)까지 가지 않는다
  await btn(page).click();
  await page.evaluate(() => { window.__escapes = 0; });
  await page.keyboard.press('Escape');
  s = await state(page);
  assert.equal(s.open, false);
  assert.equal(s.active, 'button');
  assert.equal(await page.evaluate(() => window.__escapes), 0, 'Esc 가 문서까지 갔다(시청 화면도 닫힌다)');
  // 단추를 다시 누르면 닫힌다(토글)
  await btn(page).click();
  await btn(page).click();
  assert.equal((await state(page)).open, false);
  assert.deepEqual(errors, []);
  await ctx.close();
});

// ── DL04 ──
await check('DL04 sort (recent/amount) and kind filter (후원·미션·구독) with counts and totals', async () => {
  const { ctx, page, errors } = await openChat();
  await page.evaluate((d) => window.__say(d), DONATIONS);
  await btn(page).click();
  const chips = () => page.evaluate(() => [...document.querySelectorAll('#dlbox .dp-panel:not(.exit-ghost) [data-dp-filter]')].map((b) => [b.dataset.dpFilter, b.querySelector('.dp-count').textContent, b.getAttribute('aria-pressed')]));
  const sum = () => page.locator(`${PANEL} .dp-sum`).textContent();
  assert.deepEqual(await chips(), [['all', '7', 'true'], ['cheese', '5', 'false'], ['mission', '1', 'false'], ['sub', '1', 'false']]);
  assert.equal(await sum(), '합계 69,800치즈 · 7건');
  // 최신순(기본) — 마지막에 온 것이 위
  assert.deepEqual((await items(page)).map((x) => x.nick), ['미션왕', '큰손', '구독자', '익명', '익명', 'NEra', '익명']);
  // 금액순 — 많은 순, 같으면(비공개·구독은 0) 최신이 위
  await page.locator(`${PANEL} [data-dp-sort="amount"]`).click();
  assert.deepEqual((await items(page)).map((x) => `${x.nick}:${x.value}`), ['큰손:50,000치즈', '미션왕:10,000치즈', '익명:4,200치즈', '익명:3,600치즈', '익명:2,000치즈', '구독자:6개월', 'NEra:null']);
  assert.deepEqual(await page.evaluate(() => [...document.querySelectorAll('#dlbox .dp-panel:not(.exit-ghost) [data-dp-sort]')].map((b) => b.getAttribute('aria-pressed'))), ['false', 'true']);
  // 거르기
  await page.locator(`${PANEL} [data-dp-filter="mission"]`).click();
  assert.deepEqual((await items(page)).map((x) => x.nick), ['미션왕']);
  assert.equal(await sum(), '합계 10,000치즈 · 1건');
  await page.locator(`${PANEL} [data-dp-filter="sub"]`).click();
  assert.deepEqual((await items(page)).map((x) => `${x.nick}:${x.kind}`), ['구독자:구독 · 티어 1 · 6개월 달성']);
  assert.equal(await sum(), '합계 1건');
  await page.locator(`${PANEL} [data-dp-filter="cheese"]`).click();
  assert.deepEqual((await items(page)).map((x) => x.nick), ['큰손', '익명', '익명', '익명', 'NEra']);
  assert.equal(await sum(), '합계 59,800치즈 · 5건');
  assert.deepEqual((await chips()).map((c) => c[2]), ['false', 'true', 'false', 'false']);
  // 다시 열어도 고른 정렬·거르기가 남는다(같은 패널)
  await page.keyboard.press('Escape');
  await btn(page).click();
  assert.equal((await items(page)).length, 5);
  assert.deepEqual(errors, []);
  await ctx.close();
});

// ── DL05 ──
await check('DL05 anonymous → 익명, private amount has no value slot, amount color levels match chat cards; chat highlight & alerts kept', async () => {
  const { ctx, page, errors } = await openChat();
  await page.evaluate((d) => window.__say(d), DONATIONS);
  await btn(page).click();
  const list = await items(page);
  const by = Object.fromEntries(list.map((x) => [`${x.nick}|${x.value}`, x]));
  // 익명 — 이름 자리에 '익명' 하나, 금액은 공개된 대로
  const anon = list.filter((x) => x.anon);
  assert.deepEqual(anon.map((x) => x.value).sort(), ['2,000치즈', '3,600치즈', '4,200치즈']);
  assert.ok(anon.every((x) => x.nick === '익명'));
  // 비공개 금액 — 금액 칸이 없다
  assert.equal(by['NEra|null'].value, null);
  assert.equal(by['NEra|null'].kind, '채팅 후원');
  for (const x of list) assert.ok(!BAD.test(x.text), `잘못된 글자: ${x.text}`);
  // 색 단계 — 채팅 카드(.chat-line)와 같은 ev-lv
  const lines = await page.evaluate(() => Object.fromEntries([...document.querySelectorAll('#dlbox .chat-list .chat-line.ev')].map((l) => [l.dataset.id, [...l.classList].find((c) => /^ev-lv-\d$/.test(c))])));
  const ids = await page.evaluate(() => window.__panel.donations.entries.map((e) => [String(e.seq), e.m.id]));
  const idOf = Object.fromEntries(ids);
  for (const x of list) assert.equal(x.lv, lines[idOf[x.id]], `${x.nick} 색 단계가 채팅 카드와 다르다`);
  assert.equal(by['큰손|50,000치즈'].lv, 'ev-lv-5');
  assert.equal(by['미션왕|10,000치즈'].lv, 'ev-lv-4');
  assert.equal(by['NEra|null'].lv, 'ev-lv-1');
  // 메시지·미션
  assert.equal(by['미션왕|10,000치즈'].msg, '미션 · 노래 한 곡');
  assert.equal(by['큰손|50,000치즈'].msg, '오늘도 응원합니다');
  // 채팅 줄 강조·영상 위 알림은 그대로
  assert.equal(Object.keys(lines).length, 7, '채팅 줄 후원 카드가 그려지지 않았다');
  assert.deepEqual(await page.evaluate(() => window.__highlights), DONATIONS.map((d) => d.id));
  assert.deepEqual(errors, []);
  await ctx.close();
});

// ── DL06 ──
await check('DL06 keyboard: Enter opens on first item, arrows/Home/End, Tab trapped, Enter jumps to the chat line, Esc returns focus', async () => {
  const { ctx, page, errors } = await openChat({ height: 560 });
  await page.evaluate((d) => { window.__say(d.slice(0, 2)); window.__chat(40); window.__say(d.slice(2)); window.__chat(6); }, DONATIONS);
  await page.evaluate(() => document.querySelector('#dlbox .dp-btn').focus());
  await page.keyboard.press('Enter');
  const focused = () => page.evaluate(() => { const b = document.activeElement.closest('.dp-item'); return b ? b.querySelector('.dp-nick').textContent + (b.querySelector('.dp-value')?.textContent || '') : document.activeElement.className; });
  assert.equal((await state(page)).open, true, 'Enter 로 열리지 않는다');
  assert.equal(await focused(), '미션왕10,000치즈', '열면 첫 항목에 초점');
  await page.keyboard.press('ArrowDown');
  assert.equal(await focused(), '큰손50,000치즈');
  await page.keyboard.press('End');
  assert.equal(await focused(), '익명2,000치즈');
  await page.keyboard.press('Home');
  assert.equal(await focused(), '미션왕10,000치즈');
  await page.keyboard.press('ArrowUp'); // 맨 위에서 위로 — 그대로
  assert.equal(await focused(), '미션왕10,000치즈');
  // Tab — 패널 안에서만 돈다(항목은 하나만 Tab 정지)
  const stops = [];
  for (let i = 0; i < 12; i += 1) {
    await page.keyboard.press('Tab');
    stops.push(await page.evaluate((sel) => {
      const a = document.activeElement;
      if (!document.querySelector(sel).contains(a)) return `OUT:${a.className}`;
      return a.dataset.dpFilter || a.dataset.dpSort || (a.classList.contains('dp-close') ? 'close' : a.classList.contains('dp-item') ? 'item' : a.closest('.donation-rank') ? 'rank' : a.className);
    }, PANEL));
  }
  assert.ok(stops.every((x) => !x.startsWith('OUT')), `Tab 이 패널 밖으로 나갔다: ${stops}`);
  // 랭킹 칩 줄(가로 스크롤 상자)은 Chromium 만 Tab 정지로 둔다 — 엔진마다 달라 빼고 비교한다.
  const order = stops.filter((x) => x !== 'rank');
  assert.deepEqual(order.slice(0, 9), ['close', 'all', 'cheese', 'mission', 'sub', 'recent', 'amount', 'item', 'close'], `Tab 순서: ${stops}`);
  // ⇧Tab — 맨 앞(✕)에서 거꾸로 가면 마지막 정지(항목)로 돈다
  await page.locator(`${PANEL} .dp-close`).focus();
  await page.keyboard.press('Shift+Tab');
  assert.equal(await page.evaluate(() => document.activeElement.classList.contains('dp-item')), true, '⇧Tab 이 패널 밖으로 나갔다');
  // 큰손 항목에서 Enter → 채팅 줄로
  await page.locator(`${PANEL} .dp-item`).nth(1).focus();
  await page.keyboard.press('Enter');
  // 바닥 따라가기가 되돌리지 않는지 — 잠시 기다리고, 새 채팅이 와도 그 줄에 머문다('새 메시지' 단추가 뜬다)
  await page.waitForTimeout(300);
  const at = () => page.evaluate(() => {
    const list = document.querySelector('#dlbox .chat-list');
    const line = list.querySelector('.chat-line[data-id="d5"]');
    const lr = list.getBoundingClientRect();
    const r = line.getBoundingClientRect();
    return { flash: line.classList.contains('dp-flash'), inView: r.top >= lr.top - 1 && r.bottom <= lr.bottom + 1, kb: line.classList.contains('kb-cur'), active: document.activeElement === list };
  });
  assert.deepEqual(await at(), { flash: true, inView: true, kb: true, active: true });
  await page.evaluate(() => window.__chat(3, 'late'));
  await page.waitForTimeout(150);
  assert.equal((await at()).inView, true, '새 채팅이 오자 바닥으로 내려갔다');
  assert.equal(await page.evaluate(() => !document.querySelector('#dlbox .chat-jump').hidden), true, "'새 메시지' 단추가 없다");
  assert.equal((await state(page)).open, false);
  // 다시 열고 Esc → 단추로
  await page.evaluate(() => document.querySelector('#dlbox .dp-btn').focus());
  await page.keyboard.press(' ');
  assert.equal((await state(page)).open, true, 'Space 로 열리지 않는다');
  await page.keyboard.press('Escape');
  assert.equal((await state(page)).active, 'button');
  assert.deepEqual(errors, []);
  await ctx.close();
});

// ── DL07 ──
await check('DL07 jump clears solo filter when the line is hidden; trimmed lines are marked gone and only toast', async () => {
  const { ctx, page, errors } = await openChat();
  await page.evaluate((d) => { window.__say(d); window.__chat(5); }, DONATIONS);
  // 다른 사람만 보기 → 큰손 줄이 숨는다
  await page.evaluate(() => window.__panel.setSolo('cu1', '시청자1'));
  assert.equal(await page.evaluate(() => document.querySelector('#dlbox .chat-line[data-id="d5"]').offsetParent === null), true);
  await btn(page).click();
  await page.locator(`${PANEL} .dp-item`, { hasText: '큰손' }).click();
  const r = await page.evaluate(() => ({ solo: window.__panel.soloUid, shown: document.querySelector('#dlbox .chat-line[data-id="d5"]').offsetParent !== null, flash: document.querySelector('#dlbox .chat-line[data-id="d5"]').classList.contains('dp-flash') }));
  assert.deepEqual(r, { solo: null, shown: true, flash: true });
  // 밀려난 줄(줄 상한으로 지워짐)
  await page.evaluate(() => document.querySelector('#dlbox .chat-line[data-id="d1"]').remove());
  await btn(page).click();
  const gone = (await items(page)).filter((x) => x.gone).map((x) => x.text.replace(/\s+/g, ' ').trim());
  assert.equal(gone.length, 1);
  assert.match(gone[0], /익명.*2,000치즈.*지나감/);
  assert.equal(await page.locator(`${PANEL} .dp-item.gone`).getAttribute('aria-disabled'), 'true');
  assert.match(await page.locator(`${PANEL} .dp-item.gone`).getAttribute('title'), /이미 지나간/);
  await page.evaluate((sel) => document.querySelector(`${sel} .dp-item.gone`).click(), PANEL); // aria-disabled 라 Playwright click 은 누르지 않는다
  assert.equal((await state(page)).open, true, '밀려난 항목을 누르니 패널이 닫혔다');
  assert.match(await page.evaluate(() => document.getElementById('toast-stack')?.textContent || ''), /지나간 후원/);
  assert.deepEqual(errors, []);
  await ctx.close();
});

// ── DL08 ──
await check('DL08 narrow widths, light/dark and overlay chat: panel fits inside the chat, no horizontal overflow, contrast ok', async () => {
  const cases = [
    { width: 240, theme: 'dark' }, { width: 240, theme: 'light' },
    { width: 360, theme: 'dark' }, { width: 360, theme: 'light' },
    { width: 340, theme: 'light', overlay: true }, { width: 340, theme: 'dark', overlay: true },
  ];
  const bad = [];
  for (const c of cases) {
    const { ctx, page, errors } = await openChat({ ...c, height: 600 });
    await page.evaluate((d) => { window.__chat(3); window.__say(d.map((m) => (m.id === 'd5' ? { ...m, nickname: '아주아주긴닉네임을가진큰손후원자님', content: '아주 긴 메시지 '.repeat(12) } : m))); }, DONATIONS);
    await btn(page).click();
    await page.waitForTimeout(260); // 떠오르는 모션이 끝난 뒤 잰다
    const r = await page.evaluate(() => {
      const root = document.querySelector('#dlbox .chat');
      const p = root.querySelector('.dp-panel');
      const rr = root.getBoundingClientRect();
      const pr = p.getBoundingClientRect();
      const b = root.querySelector('.dp-btn').getBoundingClientRect();
      const hr = root.querySelector('.chat-header').getBoundingClientRect();
      const over = [...p.querySelectorAll('*')].filter((el) => { if (el.parentElement.closest('.donation-rank-list')) return false; const x = el.getBoundingClientRect(); return x.width && (x.right > pr.right + 0.5 || x.left < pr.left - 0.5); }).map((el) => el.className);
      return {
        inside: pr.left >= rr.left - 0.5 && pr.right <= rr.right + 0.5 && pr.top >= hr.bottom - 0.5 && pr.bottom <= rr.bottom + 0.5,
        btn: b.left >= hr.left && b.right <= hr.right + 0.5 && b.width > 0,
        hscroll: p.scrollWidth > p.clientWidth + 1 || root.scrollWidth > root.clientWidth + 1,
        over: over.slice(0, 5),
      };
    });
    const tag = `${c.width}${c.theme}${c.overlay ? '-overlay' : ''}`;
    if (SHOTS) await page.locator('#dlbox').screenshot({ path: path.join(SHOTS, `donation-list-${tag}.png`) });
    if (!r.inside) bad.push(`${tag}: 패널이 채팅 밖으로 나간다`);
    if (!r.btn) bad.push(`${tag}: 단추가 머리 밖`);
    if (r.hscroll) bad.push(`${tag}: 가로 넘침`);
    if (r.over.length) bad.push(`${tag}: 넘친 요소 ${r.over}`);
    if (AXE) {
      await page.addScriptTag({ content: AXE });
      const v = await page.evaluate(async () => {
        const res = await window.axe.run(document.querySelector('#dlbox .dp-panel'), { runOnly: ['color-contrast'] });
        return res.violations.flatMap((x) => x.nodes.map((n) => `${n.target.join(' ')} ${n.any[0]?.message || ''}`));
      });
      if (v.length) bad.push(`${tag}: 대비 ${v.slice(0, 3).join(' / ')}`);
    }
    if (errors.length) bad.push(`${tag}: ${errors[0]}`);
    await ctx.close();
  }
  assert.deepEqual(bad, []);
});

// ── DL09 ──
await check('DL09 setting "strip" brings back the old top strip and ranking row (button stays); switching back removes them', async () => {
  const { ctx, page, errors } = await openChat({ prefs: { donationList: 'strip' } });
  await page.evaluate((d) => window.__say(d), DONATIONS);
  await page.waitForTimeout(60);
  const look = () => page.evaluate(() => {
    const root = document.querySelector('#dlbox .chat');
    const header = root.querySelector('.chat-header');
    const strip = root.querySelector('.chat-events');
    const rank = root.querySelector('.donation-rank');
    return {
      strip: Boolean(strip && !strip.hidden && strip.isConnected && !strip.closest('.dp-panel')),
      chips: strip && strip.isConnected ? [...strip.querySelectorAll('.event-chip b')].map((b) => b.textContent).slice(0, 3) : [],
      rankUnderHeader: Boolean(rank && rank.previousElementSibling === header && !rank.hidden),
      btn: Boolean(header.querySelector('.dp-btn')),
    };
  });
  const a = await look();
  assert.deepEqual(a, { strip: true, chips: ['미션왕', '큰손', '구독자'], rankUnderHeader: true, btn: true });
  // 띠가 있어도 목록 패널은 열린다
  await btn(page).click();
  assert.equal((await items(page)).length, 7);
  await page.keyboard.press('Escape');
  await page.evaluate(async () => { const { updateChatPrefs } = await import('/js/chat-prefs.js'); await updateChatPrefs({ donationList: 'button' }); });
  const b = await look();
  assert.deepEqual(b, { strip: false, chips: [], rankUnderHeader: false, btn: true });
  assert.deepEqual(errors, []);
  await ctx.close();
});

// ── DL10 ──
await check('DL10 multilive multi-chat (merged): every pane feeds the list with channel names, no doubles, survives pane switch, jumps to merged line', async () => {
  const { ctx, page, errors } = await openChat();
  // 채팅 이벤트를 패인 세션별로 흘릴 수 있게 — 모의 listen 을 바꾼 뒤 멀티라이브 채팅 상자를 따로 만든다.
  const r = await page.evaluate(async (d) => {
    const m = new Map();
    window.__TAURI__.event = { listen: async (n, f) => { if (!m.has(n)) m.set(n, new Set()); m.get(n).add(f); return () => m.get(n).delete(f); } };
    const emit = (n, p) => { for (const f of m.get(n) || []) f({ event: n, payload: p }); };
    localStorage.setItem('cview.ml.chatView', 'merged');
    const el = document.createElement('section');
    el.className = 'chat ml-chat';
    el.id = 'mlbox';
    el.style.cssText = 'position:fixed;right:0;top:0;width:360px;height:700px;z-index:99;display:flex;flex-direction:column';
    el.innerHTML = '<header class="chat-header"><strong>채팅</strong><span class="chat-state">연결 끊김</span></header><div class="chat-notice" hidden></div><div class="chat-list"></div><form class="chat-input"><input type="text" maxlength="100"><button class="btn primary" type="submit">채팅</button></form>';
    document.body.append(el);
    const { ChatPanel } = await import('/js/chat.js');
    const { MultiLiveMergedChat } = await import('/js/ml-multichat.js');
    const panel = new ChatPanel(el, { isLoggedIn: () => true, session: 'ml' });
    await panel.connect('cc0', 'c0');
    panel.onEvent({ type: 'connected' });
    const state = { selectedId: 's0', sessions: [{ id: 's0', channelId: 'c0', channelName: '첫째방', chatChannelId: 'cc0' }, { id: 's1', channelId: 'c1', channelName: '둘째방', chatChannelId: 'cc1' }] };
    const mc = new MultiLiveMergedChat(el, { getState: () => state, getPanes: () => new Map(), onSelect: () => {} });
    mc.sync();
    const t = Date.now();
    const mk = (o) => ({ role: 'common_user', badges: [], emojis: {}, timestamp: t, ...o });
    // 선택 패인(s0)의 후원은 ChatPanel('ml')과 추가 세션(mlx:s0) 양쪽으로 온다 — 목록에는 한 번만
    panel.onEvent({ type: 'messages', messages: [mk(d[5])] });
    panel.flushLines();
    emit('chat://session-event', { session: 'mlx:s0', event: { type: 'messages', messages: [mk(d[5]), mk({ id: 'n0', kind: 'normal', userId: 'u0', nickname: '시청자', content: '안녕' })] } });
    emit('chat://session-event', { session: 'mlx:s1', event: { type: 'messages', messages: [mk(d[0]), mk(d[1])] } });
    await new Promise((res) => requestAnimationFrame(() => setTimeout(res, 0)));
    const dl = panel.donations;
    const out = { external: dl.external, badge: el.querySelector('.dp-badge').hidden ? '' : el.querySelector('.dp-badge').textContent };
    el.querySelector('.dp-btn').click();
    out.items = [...el.querySelectorAll('.dp-item')].map((b) => `${b.querySelector('.dp-nick').textContent}@${b.querySelector('.dp-src')?.textContent}`);
    el.querySelector('.dp-close').click();
    // 다른 패인을 고르면 선택 패인 채팅(ml)이 다시 연결된다 — 목록은 그대로
    await panel.connect('cc1', 'c1');
    out.afterSwitch = dl.entries.length;
    // 항목 → 통합 목록의 그 줄
    el.querySelector('.dp-btn').click();
    [...el.querySelectorAll('.dp-item')].find((b) => b.textContent.includes('NEra')).click();
    const line = mc.list.querySelector('.chat-line[data-id="d2"][data-src="mlx:s1"]');
    await new Promise((res) => setTimeout(res, 200));
    const lr = mc.list.getBoundingClientRect();
    const rr = line.getBoundingClientRect();
    out.jump = { flash: line.classList.contains('dp-flash'), open: !el.querySelector('.dp-panel:not(.exit-ghost)').hidden, inView: rr.top >= lr.top - 1 && rr.bottom <= lr.bottom + 1, focus: document.activeElement === el.querySelector('.dp-btn') };
    // 멀티채팅을 끄면 다시 선택 패인 채팅으로(비운다)
    mc.setMode('off');
    out.off = { external: dl.external, n: dl.entries.length };
    panel.onEvent({ type: 'messages', messages: [mk({ ...d[6], id: 'own1' })] });
    panel.flushLines();
    out.own = dl.entries.map((e) => e.m.id);
    return out;
  }, DONATIONS);
  assert.equal(r.external, 'ml');
  assert.equal(r.badge, '3');
  assert.deepEqual(r.items, ['NEra@둘째방', '익명@둘째방', '큰손@첫째방']);
  assert.equal(r.afterSwitch, 3, '패인을 바꾸자 목록이 비었다');
  assert.deepEqual(r.jump, { flash: true, open: false, inView: true, focus: true });
  assert.deepEqual(r.off, { external: null, n: 0 });
  assert.deepEqual(r.own, ['own1']);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await browser.close();
server.close();
for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : `\n     ${r.error}`}`);
const failed = results.filter((r) => !r.ok).length;
console.log(`\n${results.length - failed}/${results.length} passed`);
process.exit(failed ? 1 : 0);
