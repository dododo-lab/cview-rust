// 채팅 사용성 UI 회귀 검사(모의 IPC, 헤드리스 Chromium) — chat.js · chat-actions.js.
// CX01 관리자 삭제(블라인드)가 줄이 붙기 전(프레임·가려진 창의 1초 묶음)에 와도 지워진 글로 그린다.
// CX02 연결이 실패·끊기면 상태 옆에 '다시 연결'이 보이고, 누르면 다시 연결한다. 연결되면 걷힌다. 상태는 role=status.
// CX03 줄 메뉴 — 열면 첫 항목에 초점, ↑/↓ 로 옮기고 Esc 로 닫으면 연 자리로 초점이 돌아간다.
// CX04 이모티콘 선택기 — aria-expanded 를 따르고 Esc 로 닫힌다. 자동완성은 콤보박스(listbox·option·activedescendant).
// CX05 남은 글자 수 — 80자부터 보이고 100자면 경고 색. 전송 실패는 이 패널 배너(경고)에 남고 쓴 글은 지우지 않는다.
// CX06 채팅 제한 — 기간을 알리고, 다른 상태 갱신이 와도 기간 동안 배너가 남는다. penaltyText 는 기간을 사람 말로.
// CX07 입력 기록 — 빈 입력창에서 ↑/↓ 로 보낸 메시지를 되부른다. 쓰던 글이 있으면 ↑ 는 커서 이동 그대로.
// CX08 줄 동작 막대 — 패널에 하나뿐이고 호버한 줄로 옮겨 붙는다(줄 수에 안 든다). 답장은 @닉네임, 복사는 이모티콘을
//      :이름: 으로, 내 줄은 답장이 없다. 고정한 줄 복제본에는 막대가 따라가지 않는다.
// CX09 키보드 줄 고르기 — 목록에서 ↑ 는 최근 줄부터, aria-activedescendant, Shift+F10 메뉴, Enter 말풍선, Esc 풀기.
// CX10 검색 — 닉네임·본문이 맞는 줄만 남기고 개수를 알린다. 새로 붙는 줄도 거르고, Esc 로 닫으면 모두 보인다.
// CX11 넓은 칸에서도 말풍선은 한 줄 길이 상한(560px × 배율)을 넘지 않는다.
// 저장소 루트에서: node tools/ui-regression/chat-ux.mjs   (Playwright 가 필요하다)
//   CVIEW_UI_BROWSER=webkit 이면 앱과 같은 WebKit 엔진으로 돈다(npx playwright install webkit).
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, base, mockTauri } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const engine = process.env.CVIEW_UI_BROWSER === 'webkit' ? pw.webkit : pw.chromium;
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}
const browser = await engine.launch();

/** 채팅 패널 하나를 띄운 페이지. chat_connect·chat_send 호출은 window.__connects·__sent 에 남는다. */
async function openChat({ width = 380, loggedIn = true } = {}) {
  const ctx = await browser.newContext({ viewport: { width: width + 400, height: 900 } });
  const page = await ctx.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, 'dark']);
  await page.addInitScript(() => {
    window.__connects = 0;
    window.__sent = [];
    // 클립보드 — 엔진마다 권한이 달라 쓴 글만 기록한다
    window.__clip = null;
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async (t) => { window.__clip = t; }, readText: async () => window.__clip } });
    let hidden = false;
    Object.defineProperty(document, 'hidden', { get: () => hidden, configurable: true });
    window.__setHidden = (v) => { hidden = v; document.dispatchEvent(new Event('visibilitychange')); };
    const inv = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async (c, a) => {
      if (c === 'chat_connect') { window.__connects += 1; return { canSend: true }; }
      if (c === 'chat_send') { window.__sent.push(a.text); if (window.__failSend) throw new Error('보내지 못함(시험)'); return null; }
      if (c === 'chat_avatars') return a.userIds.map(() => null);
      try { return await inv(c, a); } catch { return null; }
    };
  });
  await page.goto(base);
  await page.waitForFunction(() => !document.getElementById('splash') || !document.getElementById('splash').isConnected, null, { timeout: 20000 });
  await page.evaluate(async ([w, loggedIn]) => {
    document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove());
    const { ChatPanel } = await import('/js/chat.js');
    const { loadChatPrefs, setMyNickname } = await import('/js/chat-prefs.js');
    await loadChatPrefs();
    setMyNickname(loggedIn ? '도도' : '');
    // 시청 화면의 패널은 이미 부속을 달았으므로 index.html 원본 마크업에서 새로 만든다.
    const src = new DOMParser().parseFromString(await (await fetch('/index.html')).text(), 'text/html');
    const box = document.createElement('div');
    box.id = 'cxbox';
    box.style.cssText = `position:fixed;left:20px;top:10px;width:${w}px;height:860px;z-index:50;display:flex;flex-direction:column`;
    box.innerHTML = src.querySelector('#viewer .chat').outerHTML;
    document.body.appendChild(box);
    const root = box.firstElementChild;
    root.hidden = false;
    root.style.cssText = 'display:flex;flex-direction:column;height:100%;width:100%';
    const panel = new ChatPanel(root, { isLoggedIn: () => loggedIn });
    window.__panel = panel;
    await panel.connect('cc1', 'ch1');
    panel.onEvent({ type: 'connected' });
    let k = 0;
    const t = Date.now();
    window.__msg = (o) => ({ id: `m${++k}`, kind: 'normal', role: 'common_user', badges: [], emojis: {}, timestamp: t + k * 61000, ...o });
    window.__say = (list) => { panel.append(list.map((o) => window.__msg(o))); panel.flushLines(); };
  }, [width, loggedIn]);
  return { ctx, page, errors };
}

const seed = (page) => page.evaluate(() => window.__say([
  { userId: 'u1', nickname: '사과', content: '안녕하세요 반가워요' },
  { userId: 'u2', nickname: '바나나', content: '오늘 방송 {:d_1:} 최고', emojis: { d_1: 'https://ssl.pstatic.net/static/nng/glive/icon/d_1.png' } },
  { userId: 'u3', nickname: '체리', content: '사과 좋아하세요?' },
  { userId: 'me', nickname: '도도', content: '내 메시지' },
]));

// ── CX01 블라인드 ──
await check('CX01 blind before flush still blinds the line', async () => {
  const { ctx, page } = await openChat();
  const r = await page.evaluate(async () => {
    const p = window.__panel;
    window.__setHidden(true); // 가려진 창 — 줄은 1초 묶음으로 붙는다
    p.append([window.__msg({ id: 'bad1', userId: 'u9', nickname: '나쁜말', content: '지워질 말 {:x:}', emojis: { x: 'https://ssl.pstatic.net/x.png' } })]);
    p.onEvent({ type: 'blinded', messageId: 'bad1' });
    window.__setHidden(false);
    p.flushLines();
    const line = p.list.querySelector('[data-id="bad1"]');
    // 이미 붙은 줄의 블라인드도 그대로
    window.__say([{ id: 'bad2', userId: 'u8', nickname: '또', content: '두 번째' }]);
    p.onEvent({ type: 'blinded', messageId: 'bad2', blindText: '클린봇이 가린 메시지입니다.' });
    const line2 = p.list.querySelector('[data-id="bad2"]');
    return {
      cls: line.className, text: line.querySelector('.text').textContent, imgs: line.querySelectorAll('img.emoji').length,
      cls2: line2.className, text2: line2.querySelector('.text').textContent,
      style: getComputedStyle(line.querySelector('.text')).fontStyle,
    };
  });
  assert.match(r.cls, /blinded/);
  assert.equal(r.text, '관리자에 의해 삭제된 메시지입니다.');
  assert.equal(r.imgs, 0, '지운 메시지의 이모티콘이 남음');
  assert.match(r.cls2, /blinded/);
  assert.equal(r.text2, '클린봇이 가린 메시지입니다.');
  assert.equal(r.style, 'italic', '블라인드 글이 말풍선 스킨에 덮여 구분되지 않음');
  await ctx.close();
});

// ── CX02 다시 연결 ──
await check('CX02 failed/disconnected shows a reconnect button next to the state', async () => {
  const { ctx, page } = await openChat();
  const s = await page.evaluate(() => { const p = window.__panel; return { hidden: p.retry.hidden, role: p.state.getAttribute('role') }; });
  assert.equal(s.hidden, true, '연결됨인데 다시 연결이 보임');
  assert.equal(s.role, 'status');
  for (const st of ['failed', 'disconnected']) {
    await page.evaluate((st) => window.__panel.onEvent({ type: 'state', state: st }), st);
    assert.equal(await page.locator('#cxbox .chat-retry').isVisible(), true, `${st} 에서 다시 연결이 안 보임`);
  }
  await page.evaluate(() => window.__panel.onEvent({ type: 'state', state: 'reconnecting', detail: '2' }));
  assert.equal(await page.locator('#cxbox .chat-retry').isVisible(), false, '엔진이 재시도 중인데 버튼이 보임');
  await page.evaluate(() => window.__panel.onEvent({ type: 'state', state: 'failed' }));
  const before = await page.evaluate(() => window.__connects);
  await page.locator('#cxbox .chat-retry').click();
  await page.waitForFunction((n) => window.__connects > n, before);
  await page.evaluate(() => window.__panel.onEvent({ type: 'connected' }));
  assert.equal(await page.locator('#cxbox .chat-retry').isVisible(), false);
  // 사용자가 떠난(비활성) 패널은 끊김이어도 버튼을 꺼내지 않는다
  await page.evaluate(async () => { await window.__panel.disconnect(); });
  assert.equal(await page.locator('#cxbox .chat-retry').isVisible(), false, '떠난 패널에 다시 연결');
  await ctx.close();
});

// ── CX03 줄 메뉴 키보드 ──
await check('CX03 line menu takes focus, arrows move, Esc closes and restores focus', async () => {
  const { ctx, page } = await openChat();
  await seed(page);
  await page.locator('#cxbox .chat-input > input').focus();
  await page.locator('#cxbox .chat-line[data-uid="u1"] .text').click({ button: 'right' });
  const first = await page.evaluate(() => document.activeElement.dataset.ctx);
  assert.equal(first, 'copy-text', '메뉴를 열어도 초점이 메뉴로 가지 않음');
  await page.keyboard.press('ArrowDown');
  assert.equal(await page.evaluate(() => document.activeElement.dataset.ctx), 'copy-nick');
  await page.keyboard.press('ArrowUp');
  await page.keyboard.press('ArrowUp'); // 맨 위에서 올리면 맨 아래(차단)
  assert.equal(await page.evaluate(() => document.activeElement.dataset.ctx), 'block');
  await page.keyboard.press('Escape');
  assert.equal(await page.locator('.ctx-menu').count(), 0, 'Esc 로 닫히지 않음');
  // 우클릭은 목록을 누른 것이라 초점은 목록(또는 입력창)으로 — 문서 본문에 떨어지지 않는다
  assert.notEqual(await page.evaluate(() => document.activeElement.tagName), 'BODY');
  await ctx.close();
});

// ── CX04 이모티콘 선택기·자동완성 ──
await check('CX04 emoji picker Esc/aria-expanded; autocomplete is an ARIA combobox', async () => {
  const { ctx, page } = await openChat();
  await page.evaluate(() => { window.__panel.emoticons = [{ emoticonPackId: 'p', emoticonPackName: '기본', emoticons: [{ emoticonId: 'd_1', emoticonName: 'd_1', imageUrl: 'https://ssl.pstatic.net/a.png' }, { emoticonId: 'd_2', emoticonName: 'd_2', imageUrl: 'https://ssl.pstatic.net/b.png' }] }]; });
  const btn = page.locator('#cxbox .chat-emoji-btn');
  await btn.click();
  assert.equal(await btn.getAttribute('aria-expanded'), 'true');
  await page.keyboard.press('Escape');
  assert.equal(await page.locator('#cxbox .emoji-picker:not(.exit-ghost)').isHidden(), true, 'Esc 로 선택기가 닫히지 않음');
  assert.equal(await btn.getAttribute('aria-expanded'), 'false');
  assert.equal(await page.evaluate(() => document.activeElement === window.__panel.input), true, '닫은 뒤 입력창으로 돌아가지 않음');
  const input = page.locator('#cxbox .chat-input > input');
  assert.equal(await input.getAttribute('role'), 'combobox');
  await input.pressSequentially(':d_');
  const a = await page.evaluate(() => { const p = window.__panel; return { exp: p.input.getAttribute('aria-expanded'), ad: p.input.getAttribute('aria-activedescendant'), role: p.autocomplete.getAttribute('role'), opts: [...p.autocomplete.querySelectorAll('[role=option]')].map((o) => [o.id, o.getAttribute('aria-selected')]) }; });
  assert.equal(a.exp, 'true');
  assert.equal(a.role, 'listbox');
  assert.equal(a.opts.length, 2);
  assert.equal(a.ad, a.opts[0][0]);
  await page.keyboard.press('ArrowDown');
  const ad2 = await page.evaluate(() => window.__panel.input.getAttribute('aria-activedescendant'));
  assert.equal(ad2, a.opts[1][0], '고른 후보가 activedescendant 로 따라가지 않음');
  await page.keyboard.press('Escape');
  assert.equal(await input.getAttribute('aria-expanded'), 'false');
  await ctx.close();
});

// ── CX05 글자 수·전송 실패 ──
await check('CX05 char counter near the limit; send failure stays on this panel and keeps the text', async () => {
  const { ctx, page } = await openChat();
  const input = page.locator('#cxbox .chat-input > input');
  await input.fill('가'.repeat(79));
  await input.press('End');
  assert.equal(await page.locator('#cxbox .chat-count').isVisible(), false, '79자에 글자 수가 보임');
  await input.pressSequentially('나');
  assert.equal(await page.locator('#cxbox .chat-count').textContent(), '80/100');
  await input.fill('가'.repeat(100));
  await input.dispatchEvent('input');
  const full = await page.evaluate(() => { const c = window.__panel.counter; return { t: c.textContent, full: c.classList.contains('full'), color: getComputedStyle(c).color }; });
  assert.equal(full.t, '100/100');
  assert.ok(full.full, '100자인데 경고가 아님');
  await page.evaluate(() => { window.__failSend = true; });
  await input.fill('실패할 메시지');
  await input.press('Enter');
  await page.waitForFunction(() => !window.__panel.banner.hidden);
  const r = await page.evaluate(() => { const p = window.__panel; return { v: p.input.value, banner: p.banner.textContent, warn: p.banner.classList.contains('warn'), role: p.banner.getAttribute('role') }; });
  assert.equal(r.v, '실패할 메시지', '실패한 글이 지워짐');
  assert.match(r.banner, /보내지 못함/);
  assert.ok(r.warn);
  assert.equal(r.role, 'alert');
  await ctx.close();
});

// ── CX06 채팅 제한 ──
await check('CX06 penalty shows duration and survives state updates until it ends', async () => {
  const { ctx, page } = await openChat();
  const r = await page.evaluate(async () => {
    const { penaltyText } = await import('/js/chat.js');
    const p = window.__panel;
    p.onEvent({ type: 'penalized', userId: 'me', duration: 1 });
    p.onEvent({ type: 'state', state: 'connected' }); // 상태 갱신이 배너를 걷지 않는다
    const during = p.banner.hidden ? '' : p.banner.textContent;
    p.flushLines();
    const sys = p.list.lastElementChild.textContent;
    await new Promise((res) => setTimeout(res, 1200));
    return { during, sys, after: p.banner.hidden, texts: [penaltyText(0), penaltyText(30), penaltyText(600), penaltyText(5400)] };
  });
  assert.equal(r.during, '1초 동안 채팅 이용이 제한되었습니다');
  assert.equal(r.sys, '1초 동안 채팅 이용이 제한되었습니다');
  assert.equal(r.after, true, '제한이 끝났는데 배너가 남음');
  assert.deepEqual(r.texts, ['채팅 이용이 잠시 제한되었습니다', '30초 동안 채팅 이용이 제한되었습니다', '10분 동안 채팅 이용이 제한되었습니다', '1.5시간 동안 채팅 이용이 제한되었습니다']);
  await ctx.close();
});

// ── CX07 입력 기록 ──
await check('CX07 input history recalls sent messages with ArrowUp/Down', async () => {
  const { ctx, page } = await openChat();
  const input = page.locator('#cxbox .chat-input > input');
  for (const t of ['첫째', '둘째', '둘째', '셋째']) { await input.fill(t); await input.press('Enter'); await page.waitForFunction(() => window.__panel.input.value === '' && !window.__panel.input.disabled); }
  await input.press('ArrowUp');
  assert.equal(await input.inputValue(), '셋째');
  await input.press('ArrowUp');
  assert.equal(await input.inputValue(), '둘째');
  await input.press('ArrowUp');
  assert.equal(await input.inputValue(), '첫째', '연달아 같은 말은 한 번만 기억');
  await input.press('ArrowUp');
  assert.equal(await input.inputValue(), '첫째', '맨 앞에서 멈춘다');
  await input.press('ArrowDown');
  await input.press('ArrowDown');
  await input.press('ArrowDown');
  assert.equal(await input.inputValue(), '', '끝을 지나면 빈 칸');
  await input.fill('쓰는 중');
  await input.press('ArrowUp');
  assert.equal(await input.inputValue(), '쓰는 중', '쓰던 글을 기록이 덮음');
  await ctx.close();
});

// ── CX08 동작 막대 ──
await check('CX08 one hover action bar per panel: reply, copy (emoji as :name:), own line has no reply', async () => {
  const { ctx, page } = await openChat();
  await seed(page);
  const count0 = await page.evaluate(() => window.__panel.list.children.length);
  await page.locator('#cxbox .chat-line[data-uid="u1"] .text').hover();
  assert.equal(await page.locator('#cxbox .chat-actions').count(), 1);
  await page.locator('#cxbox .chat-line[data-uid="u2"] .text').hover();
  const r = await page.evaluate(() => { const p = window.__panel; return { n: document.querySelectorAll('#cxbox .chat-actions').length, at: p.list.querySelector('.chat-actions').closest('.chat-line').dataset.uid, lines: p.list.children.length }; });
  assert.equal(r.n, 1, '줄마다 막대가 생김');
  assert.equal(r.at, 'u2', '막대가 호버한 줄로 옮겨 가지 않음');
  assert.equal(r.lines, count0, '막대가 목록 줄 수에 들어감(줄 상한이 틀어진다)');
  await page.locator('#cxbox .chat-actions [data-ca="copy"]').click();
  assert.equal(await page.evaluate(() => navigator.clipboard.readText()), '오늘 방송 :d_1: 최고');
  await page.locator('#cxbox .chat-actions [data-ca="reply"]').click();
  assert.equal(await page.evaluate(() => window.__panel.input.value), '@바나나 ');
  await page.locator('#cxbox .chat-line[data-uid="me"] .text').hover();
  assert.equal(await page.locator('#cxbox .chat-actions [data-ca="reply"]').isVisible(), false, '내 줄에 답장');
  await page.locator('#cxbox .chat-actions [data-ca="more"]').click();
  assert.equal(await page.locator('.ctx-menu').count(), 1, '더보기가 줄 메뉴를 열지 않음');
  await page.keyboard.press('Escape');
  // 고정 — 복제본에는 막대가 따라가지 않는다
  await page.evaluate(() => { const p = window.__panel; const line = p.list.querySelector('.chat-line[data-uid="u1"]'); line.append(p.list.querySelector('.chat-actions') || document.createElement('i')); p.pin(line); });
  assert.equal(await page.locator('#cxbox .chat-pin .chat-actions').count(), 0, '고정 줄에 막대가 복제됨');
  await ctx.close();
});

// ── CX09 키보드 줄 고르기 ──
await check('CX09 keyboard line navigation: arrows, activedescendant, Shift+F10 menu, Enter user pop, Esc', async () => {
  const { ctx, page } = await openChat();
  await seed(page);
  const list = page.locator('#cxbox .chat-list');
  assert.equal(await list.getAttribute('role'), 'log');
  assert.equal(await list.getAttribute('aria-live'), 'off', '붐비는 채팅을 줄마다 읽어 줌');
  await list.focus();
  await page.keyboard.press('ArrowUp');
  const cur = () => page.evaluate(() => { const p = window.__panel; const id = p.list.getAttribute('aria-activedescendant'); const el = id && document.getElementById(id); return el ? el.dataset.uid : null; });
  assert.equal(await cur(), 'me', '처음 ↑ 는 가장 최근 줄');
  await page.keyboard.press('ArrowUp');
  assert.equal(await cur(), 'u3');
  await page.keyboard.press('Home');
  assert.equal(await cur(), 'u1');
  assert.equal(await page.locator('#cxbox .chat-line.kb-cur .chat-actions').count(), 1, '고른 줄에 동작 막대가 없음');
  await page.keyboard.press('Shift+F10');
  assert.equal(await page.locator('.ctx-menu').count(), 1, 'Shift+F10 메뉴');
  await page.keyboard.press('Escape');
  assert.equal(await page.evaluate(() => document.activeElement === window.__panel.list), true, '메뉴를 닫은 뒤 목록으로 돌아가지 않음');
  await page.keyboard.press('Enter');
  await page.waitForSelector('.user-pop', { timeout: 3000 });
  await page.keyboard.press('Escape');
  await list.focus();
  await page.keyboard.press('Escape');
  assert.equal(await cur(), null, 'Esc 로 고르기가 풀리지 않음');
  await ctx.close();
});

// ── CX10 검색 ──
await check('CX10 search filters by nick/text, counts, filters new lines, Esc restores', async () => {
  const { ctx, page } = await openChat();
  await seed(page);
  await page.locator('#cxbox [data-chat="tools"]').click();
  await page.locator('#cxbox [data-chat-search]').click();
  const q = page.locator('#cxbox .chat-search .cs-input');
  await q.fill('사과');
  await page.waitForFunction(() => window.__panel.searchQuery === '사과');
  const shown = () => page.evaluate(() => [...window.__panel.list.querySelectorAll('.chat-line[data-uid]')].filter((l) => l.offsetParent !== null).map((l) => l.dataset.uid));
  assert.deepEqual(await shown(), ['u1', 'u3'], '닉네임(사과)·본문(사과 좋아하세요) 줄만 남아야 함');
  assert.equal(await page.locator('#cxbox .cs-count').textContent(), '2개');
  await page.evaluate(() => window.__say([{ userId: 'u4', nickname: '무관', content: '다른 말' }, { userId: 'u5', nickname: '새사람', content: '사과 맛있다' }]));
  assert.deepEqual(await shown(), ['u1', 'u3', 'u5'], '새로 붙은 줄을 거르지 않음');
  assert.equal(await page.locator('#cxbox .cs-count').textContent(), '3개');
  await q.fill('d_1'); // 이모티콘 이름으로도
  await page.waitForFunction(() => window.__panel.searchQuery === 'd_1');
  assert.deepEqual(await shown(), ['u2']);
  await q.press('Escape');
  assert.equal(await page.locator('#cxbox .chat-search').isHidden(), true);
  assert.equal((await shown()).length, 6, '검색을 닫아도 줄이 숨어 있음');
  await ctx.close();
});

// ── CX11 넓은 칸 말풍선 상한 ──
await check('CX11 bubbles stay readable in a wide chat column', async () => {
  const { ctx, page } = await openChat({ width: 900 });
  await page.evaluate(() => window.__say([{ userId: 'u1', nickname: '긴글', content: '아주 긴 문장 '.repeat(40) }, { userId: 'me', nickname: '도도', content: '내 긴 문장 '.repeat(40) }]));
  const w = await page.evaluate(() => [...window.__panel.list.querySelectorAll('.msg-main')].map((m) => m.getBoundingClientRect().width));
  assert.ok(w.every((x) => x <= 561), `말풍선이 너무 넓음: ${w}`);
  await ctx.close();
});

await browser.close();
server.close();
for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : `\n     ${r.error}`}`);
const failed = results.filter((r) => !r.ok).length;
console.log(`\n${results.length - failed}/${results.length} passed`);
process.exit(failed ? 1 : 0);
