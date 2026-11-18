// 채팅 화면 정밀 다듬기 회귀 검사 — 모의 IPC(실제 서비스·로그인·전송 없음).
// CP01 멀티라이브 채팅 '채팅 보기' 메뉴가 패널(.ml-chat 은 넘침을 자른다) 안에 뜬다 — 패널 300·380·520px.
//      버튼이 도구 서랍 왼쪽 끝이라 오른쪽 정렬 메뉴가 패널 왼쪽 밖으로 나가 항목 앞 글자가 잘렸다(사용자 스크린샷).
//      항목 글자도 잘리지 않는다(scrollWidth ≤ clientWidth).
// CP02 메뉴보다 좁은 패널(200px) — 메뉴가 패널 폭으로 줄고 항목 글자는 줄바꿈된다(잘리지 않는다).
// CP03 fitPopover — 창 왼쪽 끝 버튼의 오른쪽 정렬 메뉴는 반대쪽 정렬로 뒤집고, 오른쪽 끝 왼쪽 정렬 메뉴도 뒤집는다.
//      아래가 모자라면 남은 높이로 줄여 안에서 스크롤한다. 다시 열면 인라인 값을 지우고 다시 잰다.
// CP04 멀티라이브 채팅 도구를 펼쳐도 머리줄 단추가 패널 밖으로 잘리지 않는다 — '채팅 그리드'처럼 긴 제목·380px.
// CP05 입력창 자리 글이 연결 상태를 알린다(연결 중·끊김). 보내는 동안은 흐려지지 않는다(.sending, aria-busy).
// CP06 '새 메시지' 단추가 입력창 위 안내(로그인 안내·'로그인' 링크)를 덮지 않는다.
// CP07 메시지를 기다리는 안내는 목록 한가운데에 있다(통합 목록·칸).
// CP08 멘션 모음(@)은 머리 바로 아래에 열린다 — 좁은 패널에서 도구가 둘째 줄로 내려가도 그 줄(@ 단추)을 덮지 않는다.
// 저장소 루트에서: node tools/ui-regression/chat-polish.mjs   (Playwright 가 필요하다)
//   CVIEW_UI_BROWSER=webkit 이면 앱과 같은 WebKit 엔진으로 돈다. CVIEW_UI_SHOTS=<폴더> 면 화면을 남긴다.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, base, mockTauri } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const engine = process.env.CVIEW_UI_BROWSER === 'webkit' ? pw.webkit : pw.chromium;
const SHOTS = process.env.CVIEW_UI_SHOTS || '';
if (SHOTS) fs.mkdirSync(SHOTS, { recursive: true });
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}
const browser = await engine.launch();

/**
 * 채팅 패널 하나(index.html 원본 마크업)를 띄운다. ml 이면 멀티라이브 채팅(.ml-chat + 채팅 보기 메뉴).
 * window.__panel(ChatPanel)·window.__mlc(MultiLiveMergedChat)·window.__emit(이벤트).
 */
async function mount({ width = 380, kind = 'viewer', theme = 'dark', loggedIn = true, mode = 'off', title = null } = {}) {
  const ctx = await browser.newContext({ viewport: { width: width + 300, height: 700 }, colorScheme: theme });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(process.env.CVIEW_CP_DEBUG ? String(e.stack) : String(e)));
  await page.addInitScript(mockTauri, [base, theme]);
  await page.addInitScript((m) => {
    try { localStorage.setItem('cview.welcomed.v1', '1'); localStorage.setItem('cview.onboarded', '1'); localStorage.setItem('cview.ml.chatView', m); } catch { /* 무시 */ }
    const subs = new Map();
    window.__TAURI__.event = { listen: async (n, f) => { if (!subs.has(n)) subs.set(n, new Set()); subs.get(n).add(f); return () => subs.get(n).delete(f); } };
    window.__emit = (n, p) => { for (const f of subs.get(n) || []) f({ event: n, payload: p }); };
    window.__sendGate = null;
    const inv = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async (c, a) => {
      if (c === 'chat_connect') return { canSend: true };
      if (c === 'chat_disconnect') return null;
      if (c === 'chat_avatars') return a.userIds.map(() => null);
      if (c === 'chat_send') { if (window.__sendGate) await window.__sendGate; return null; }
      return inv(c, a);
    };
  }, mode);
  await page.goto(base);
  await page.waitForFunction(() => !document.getElementById('splash') || !document.getElementById('splash').isConnected, null, { timeout: 20000 });
  await page.evaluate(async ([w, kind, loggedIn, title]) => {
    document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove());
    const { ChatPanel } = await import('/js/chat.js');
    const { loadChatPrefs, setMyNickname } = await import('/js/chat-prefs.js');
    await loadChatPrefs();
    setMyNickname(loggedIn ? '도도' : '');
    const src = new DOMParser().parseFromString(await (await fetch('/index.html')).text(), 'text/html');
    const box = document.createElement('div');
    box.id = 'cpbox';
    box.style.cssText = `position:fixed;left:260px;top:10px;width:${w}px;height:660px;z-index:50;display:flex;flex-direction:column`;
    box.innerHTML = src.querySelector('#viewer .chat').outerHTML;
    document.body.append(box);
    const root = box.firstElementChild;
    root.hidden = false;
    root.style.cssText = 'display:flex;flex-direction:column;height:100%;width:100%';
    if (kind === 'ml') root.classList.add('ml-chat');
    const panel = new ChatPanel(root, { isLoggedIn: () => loggedIn, session: kind === 'ml' ? 'ml' : null });
    window.__panel = panel;
    if (kind === 'ml') {
      const { MultiLiveMergedChat } = await import('/js/ml-multichat.js');
      const state = { selectedId: 's0', sessions: [{ id: 's0', channelId: 'c0', channelName: '첫째', chatChannelId: 'cc0' }, { id: 's1', channelId: 'c1', channelName: '둘째', chatChannelId: 'cc1' }] };
      window.__mlc = new MultiLiveMergedChat(root, { getState: () => state, getPanes: () => new Map(), onSelect: () => {} });
      window.__mlc.sync();
      if (title) window.__mlc.title.textContent = title;
    }
    await panel.connect('cc1', 'ch1');
    panel.onEvent({ type: 'connected' });
  }, [width, kind, loggedIn, title]);
  return { ctx, page, errors };
}

/** 도구 서랍을 펴고 다 펼쳐질 때까지 기다린다. */
async function openTools(page) {
  await page.click('#cpbox [data-chat="tools"]');
  await page.waitForFunction(() => document.querySelector('#cpbox .chat-tools').classList.contains('settled'), null, { timeout: 3000 }).catch(() => {});
  await page.waitForTimeout(150);
}

/** 채팅 보기 메뉴의 자리 — 패널·메뉴·항목 사각형과 항목 글자 넘침. */
const viewMenuGeometry = (page) => page.evaluate(() => {
  const panel = document.querySelector('#cpbox .chat').getBoundingClientRect();
  const menu = document.querySelector('#cpbox .ml-mc-menu');
  const m = menu.getBoundingClientRect();
  const items = [...menu.querySelectorAll('.ml-menu-item')].map((b) => {
    const r = b.getBoundingClientRect();
    return { text: b.textContent.trim(), left: r.left, right: r.right, clipped: b.scrollWidth > b.clientWidth + 1 };
  });
  return { hidden: menu.hidden, panel: [panel.left, panel.right], menu: [m.left, m.right, m.top, m.bottom], items, fit: menu.dataset.fit || '' };
});

for (const width of [300, 380, 520]) {
  await check(`CP01 view menu stays inside the multilive chat panel (${width}px)`, async () => {
    const { ctx, page, errors } = await mount({ width, kind: 'ml' });
    await openTools(page);
    await page.click('#cpbox .ml-merge-btn');
    const g = await viewMenuGeometry(page);
    if (SHOTS) await page.screenshot({ path: `${SHOTS}/cp01-view-menu-${width}.png` });
    assert.equal(g.hidden, false, '메뉴가 열리지 않았다');
    assert.ok(g.menu[0] >= g.panel[0] - 0.5, `메뉴 왼쪽 ${g.menu[0]} < 패널 왼쪽 ${g.panel[0]}`);
    assert.ok(g.menu[1] <= g.panel[1] + 0.5, `메뉴 오른쪽 ${g.menu[1]} > 패널 오른쪽 ${g.panel[1]}`);
    assert.equal(g.items.length, 5);
    for (const it of g.items) {
      assert.ok(it.left >= g.panel[0] && it.right <= g.panel[1], `항목이 패널 밖: ${it.text}`);
      assert.equal(it.clipped, false, `항목 글자가 잘림: ${it.text}`);
    }
    // 키보드로 닫고 다시 열어도 같은 자리(인라인 값을 다시 잰다)
    await page.keyboard.press('Escape');
    await page.click('#cpbox .ml-merge-btn');
    const again = await viewMenuGeometry(page);
    assert.deepEqual(again.menu.map(Math.round), g.menu.map(Math.round));
    assert.deepEqual(errors, []);
    await ctx.close();
  });
}

await check('CP02 view menu narrower panel: shrinks to the panel and wraps item text', async () => {
  const { ctx, page, errors } = await mount({ width: 200, kind: 'ml', title: '채팅' });
  await openTools(page);
  await page.evaluate(() => document.querySelector('#cpbox .ml-merge-btn').click());
  const g = await viewMenuGeometry(page);
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/cp02-view-menu-200.png` });
  assert.ok(g.menu[0] >= g.panel[0] - 0.5 && g.menu[1] <= g.panel[1] + 0.5, `메뉴 ${g.menu} 패널 ${g.panel}`);
  assert.ok(g.menu[1] - g.menu[0] <= g.panel[1] - g.panel[0] - 16 + 0.5, '패널 여백(8px)을 남기지 않았다');
  for (const it of g.items) assert.equal(it.clipped, false, `항목 글자가 잘림: ${it.text}`);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('CP03 fitPopover flips at window edges and limits height', async () => {
  const { ctx, page, errors } = await mount({ width: 300 });
  const r = await page.evaluate(async () => {
    const { fitPopover } = await import('/js/dom.js');
    // 정렬은 CSS 로 준다 — fitPopover 는 열 때마다 인라인 left·right 를 지우고 CSS 자리에서 잰다.
    document.head.insertAdjacentHTML('beforeend', '<style>.cp-left { left: 0; right: auto; }</style>');
    const mk = (css, menuClass, items = 3) => {
      const wrap = document.createElement('div');
      wrap.style.cssText = `position:fixed;z-index:99;${css}`;
      wrap.innerHTML = `<button style="width:28px;height:28px">x</button><div class="ml-more-menu ${menuClass}" role="menu">${'<button class="ml-menu-item">항목 이름</button>'.repeat(items)}</div>`;
      document.body.append(wrap);
      return wrap.lastElementChild;
    };
    const rect = (el) => { const b = el.getBoundingClientRect(); return [Math.round(b.left), Math.round(b.right), Math.round(b.top), Math.round(b.bottom)]; };
    // 오른쪽 정렬(기본 .ml-more-menu) — 창 왼쪽 끝 버튼
    const a = mk('left:12px;top:40px', '');
    fitPopover(a);
    const aOut = { rect: rect(a), fit: a.dataset.fit || '' };
    // 왼쪽 정렬 메뉴 — 창 오른쪽 끝 버튼
    const b = mk(`left:${window.innerWidth - 40}px;top:40px`, 'cp-left');
    fitPopover(b);
    // 아래가 모자란 메뉴 — 창 아래 가까이
    const c = mk(`right:20px;top:${window.innerHeight - 160}px`, '', 12);
    fitPopover(c);
    const cOut = { rect: rect(c), scroll: getComputedStyle(c).overflowY, maxH: c.style.maxHeight };
    // 자리를 옮긴 뒤 다시 열면 지난 인라인 값 없이 다시 잰다
    a.parentElement.style.left = '300px';
    fitPopover(a);
    return { w: window.innerWidth, h: window.innerHeight, a: aOut.rect, aFit: aOut.fit, b: rect(b), bFit: b.dataset.fit || '', c: cOut, aAgain: { left: a.style.left, right: a.style.right, fit: a.dataset.fit || '' } };
  });
  assert.equal(r.aFit, 'flip');
  assert.ok(r.a[0] >= 12 && r.a[1] <= r.w - 8, `왼쪽 끝 메뉴 ${r.a}`);
  assert.equal(r.bFit, 'flip');
  assert.ok(r.b[0] >= 8 && r.b[1] <= r.w - 8 + 0.5, `오른쪽 끝 메뉴 ${r.b}`);
  assert.ok(r.c.rect[3] <= r.h - 8 + 0.5, `아래로 넘침 ${r.c.rect} / ${r.h}`);
  assert.equal(r.c.scroll, 'auto');
  assert.ok(r.c.maxH, '높이 상한이 없다');
  assert.deepEqual(r.aAgain, { left: '', right: '', fit: '' }, '다시 잴 때 지난 값이 남았다');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('CP04 multilive chat header keeps every tool inside the panel when tools open (380px, long title)', async () => {
  for (const [width, title] of [[380, '채팅 그리드'], [460, '채팅 포커스'], [300, '멀티채팅']]) {
    const { ctx, page, errors } = await mount({ width, kind: 'ml', mode: 'grid', title });
    await openTools(page);
    const r = await page.evaluate(() => {
      const panel = document.querySelector('#cpbox .chat').getBoundingClientRect();
      const header = document.querySelector('#cpbox .chat-header');
      const out = [...header.querySelectorAll('button')].filter((b) => b.getClientRects().length && !b.closest('[hidden]'))
        .map((b) => ({ label: b.getAttribute('aria-label') || b.textContent.trim(), r: b.getBoundingClientRect() }))
        .filter((x) => x.r.width && (x.r.right > panel.right + 0.5 || x.r.left < panel.left - 0.5))
        .map((x) => x.label);
      const t = header.querySelector('strong');
      return { out, titleClipped: t.scrollWidth > t.clientWidth + 1, title: t.textContent };
    });
    if (SHOTS) await page.screenshot({ path: `${SHOTS}/cp04-header-${width}.png` });
    assert.deepEqual(r.out, [], `${width}px: 패널 밖 단추 ${r.out}`);
    if (width < 460) assert.equal(r.titleClipped, false, `${width}px: 제목 '${r.title}'이 잘렸다`);
    assert.deepEqual(errors, []);
    await ctx.close();
  }
});

await check('CP05 input placeholder tells the connection state; sending does not dim the input', async () => {
  const { ctx, page, errors } = await mount({ width: 360 });
  const r = await page.evaluate(async () => {
    const p = window.__panel;
    const ph = () => p.input.placeholder;
    const out = { connected: ph() };
    p.onEvent({ type: 'reconnecting' });
    out.reconnecting = [ph(), p.input.disabled];
    p.onEvent({ type: 'disconnected' });
    out.disconnected = [ph(), p.input.disabled];
    p.onEvent({ type: 'connected' });
    out.back = [ph(), p.input.disabled];
    // 보내는 중 — 왕복을 붙잡아 둔 채 모양을 본다
    let release;
    window.__sendGate = new Promise((res) => { release = res; });
    p.input.value = '안녕';
    const sending = p.send();
    const form = p.input.form;
    out.sending = { cls: form.classList.contains('sending'), busy: form.getAttribute('aria-busy'), disabled: p.input.disabled,
      opacity: getComputedStyle(p.input).opacity, btn: getComputedStyle(p.sendButton).opacity };
    release();
    await sending;
    out.sent = { cls: form.classList.contains('sending'), busy: form.getAttribute('aria-busy'), disabled: p.input.disabled };
    return out;
  });
  assert.equal(r.connected, '채팅을 입력하세요…');
  assert.deepEqual(r.reconnecting, ['채팅에 연결하는 중…', true]);
  assert.deepEqual(r.disconnected, ['채팅 연결이 끊겼습니다', true]);
  assert.deepEqual(r.back, ['채팅을 입력하세요…', false]);
  assert.deepEqual(r.sending, { cls: true, busy: 'true', disabled: true, opacity: '1', btn: '1' });
  assert.deepEqual(r.sent, { cls: false, busy: null, disabled: false });
  // 로그인 전에는 연결 상태와 관계없이 로그인 안내
  const { ctx: c2, page: p2 } = await mount({ width: 360, loggedIn: false });
  const out2 = await p2.evaluate(() => { window.__panel.onEvent({ type: 'reconnecting' }); return window.__panel.input.placeholder; });
  assert.equal(out2, '로그인 후 채팅 참여 가능');
  await c2.close();
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('CP06 new-message pill does not cover the input banner (login prompt)', async () => {
  const { ctx, page, errors } = await mount({ width: 360, loggedIn: false });
  const r = await page.evaluate(async () => {
    const p = window.__panel;
    const mk = (i) => ({ id: `x${i}`, kind: 'normal', role: 'common_user', badges: [], emojis: {}, userId: `u${i}`, nickname: `n${i}`, content: `글 ${i}`, timestamp: Date.now() });
    p.append(Array.from({ length: 40 }, (_, i) => mk(i)));
    p.flushLines();
    p.list.scrollTop = 0;
    p.follower.stuck = false;
    p.append([mk(100)]);
    p.flushLines();
    await new Promise((res) => setTimeout(res, 450));
    const j = p.jump.getBoundingClientRect();
    const b = p.banner.getBoundingClientRect();
    const f = p.input.form.getBoundingClientRect();
    return { jumpHidden: p.jump.hidden, bannerHidden: p.banner.hidden, jumpBottom: j.bottom, bannerTop: b.top, formTop: f.top };
  });
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/cp06-jump-banner.png` });
  assert.equal(r.jumpHidden, false);
  assert.equal(r.bannerHidden, false);
  assert.ok(r.jumpBottom <= r.bannerTop - 4, `단추 아래 ${r.jumpBottom} > 안내 위 ${r.bannerTop}`);
  // 안내가 걷히면 원래 자리(입력창 바로 위)로 내려온다
  await page.evaluate(() => { window.__panel.banner.hidden = true; });
  await page.waitForTimeout(150);
  const down = await page.evaluate(() => {
    const p = window.__panel;
    return { jumpBottom: p.jump.getBoundingClientRect().bottom, formTop: p.input.form.getBoundingClientRect().top, v: p.root.style.getPropertyValue('--chat-banner-h') };
  });
  assert.equal(down.v, '');
  assert.ok(down.jumpBottom <= down.formTop - 4 && down.jumpBottom >= down.formTop - 40, `안내 없는 자리 ${JSON.stringify(down)}`);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('CP07 waiting placeholder sits in the middle of the list', async () => {
  const { ctx, page, errors } = await mount({ width: 380, kind: 'ml', mode: 'merged' });
  const r = await page.evaluate(() => {
    const list = window.__mlc.list;
    const e = list.querySelector(':scope > .empty');
    const lr = list.getBoundingClientRect();
    const t = e.firstChild;
    const range = document.createRange();
    range.selectNodeContents(e);
    const tr = range.getBoundingClientRect();
    return { mid: (lr.top + lr.bottom) / 2, text: (tr.top + tr.bottom) / 2, h: lr.height, has: Boolean(t) };
  });
  assert.ok(r.has);
  assert.ok(Math.abs(r.text - r.mid) < r.h * 0.1, `안내 가운데 ${r.text} / 목록 가운데 ${r.mid}`);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('CP08 mention tray opens below the header even when tools wrap to a second row', async () => {
  for (const width of [300, 420]) {
    const { ctx, page, errors } = await mount({ width });
    await openTools(page);
    await page.click('#cpbox .mention-btn');
    const r = await page.evaluate(() => {
      const tray = document.querySelector('#cpbox .chat-tray');
      const head = document.querySelector('#cpbox .chat-header').getBoundingClientRect();
      const t = tray.getBoundingClientRect();
      const at = document.querySelector('#cpbox .mention-btn').getBoundingClientRect();
      const hit = document.elementFromPoint(at.left + at.width / 2, at.top + at.height / 2);
      const panel = document.querySelector('#cpbox .chat').getBoundingClientRect();
      return { hidden: tray.hidden, trayTop: t.top, trayBottom: t.bottom, headBottom: head.bottom, panelBottom: panel.bottom, atCovered: !hit?.closest('.mention-btn') };
    });
    if (SHOTS) await page.screenshot({ path: `${SHOTS}/cp08-tray-${width}.png` });
    assert.equal(r.hidden, false);
    assert.ok(r.trayTop >= r.headBottom, `${width}px: 모음 위 ${r.trayTop} < 머리 아래 ${r.headBottom}`);
    assert.ok(r.trayBottom <= r.panelBottom, `${width}px: 모음이 패널 아래로 넘친다`);
    assert.equal(r.atCovered, false, `${width}px: @ 단추가 가려졌다`);
    // 다시 누르면 닫힌다(가려지지 않아 같은 단추로 닫을 수 있다)
    await page.click('#cpbox .mention-btn');
    assert.equal(await page.$eval('#cpbox .chat-tray', (t) => t.hidden), true);
    assert.deepEqual(errors, []);
    await ctx.close();
  }
});

await browser.close();
server.close();
let failed = 0;
for (const r of results) {
  console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : `\n     ${r.error}`}`);
  if (!r.ok) failed++;
}
console.log(`\n${results.length - failed}/${results.length} passed`);
process.exit(failed ? 1 : 0);
