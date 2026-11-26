// 멀티채팅·통합 보기 정밀 검사(기능 정밀 고도화 8차) — 모의 IPC·헤드리스 브라우저. CVIEW_UI_BROWSER=webkit 이면 WebKit.
// 저장소 루트에서: node tools/ui-regression/precision-multichat.mjs
// M01 통합 입력에서 보낼 채널을 고른 뒤 다른 채널을 닫아도 고른 채널로 보낸다.
// M02 한 패널에서 차단하면 다른 패널(그리드)·통합 목록의 그 사람 지난 줄도 걷힌다(통합 줄에서 차단하면 패널에서도).
// M03 통합 입력 /mute — 통합 목록의 지난 줄을 걷고, 그 뒤 온 줄도 통합 목록에 붙이지 않는다.
// M04 통합 입력 /help — 명령어 목록이 통합 목록에 보인다. /stats — 통계가 통합 상자 안에 보인다.
// M05 채널을 닫고 새로 더해도 남은 채널과 색이 겹치지 않는다.
// M06 멀티라이브 멀티채팅(통합·그리드) — /mute 가 보이는 목록에 듣고, /help 가 보이는 목록에 뜬다.
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, base, mockTauri } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const browser = await (process.env.CVIEW_UI_BROWSER === 'webkit' ? pw.webkit : pw.chromium).launch();
const failures = [];
async function check(name, fn) {
  try { await fn(); console.log(`ok   ${name}`); } catch (e) { failures.push(name); console.log(`FAIL ${name}\n     ${e.message.split('\n').join('\n     ')}`); }
}

function chatBus() {
  const m = new Map();
  window.__TAURI__.event = { listen: async (n, f) => { if (!m.has(n)) m.set(n, new Set()); m.get(n).add(f); return () => m.get(n).delete(f); } };
  window.__emit = (n, p) => { for (const f of m.get(n) || []) f({ event: n, payload: p }); };
  window.__sent = [];
  const extra = {
    live_detail: ({ channelId }) => ({ channelId, channelName: `채널${channelId}`, chatChannelId: `cc${channelId}`, isLive: true }),
    chat_connect: () => ({ canSend: true }),
    chat_disconnect: () => null,
    chat_avatars: ({ userIds }) => userIds.map(() => ''),
    chat_emoticons: () => [],
    chat_send: ({ text, session }) => { window.__sent.push([text, session]); return null; },
    settings_update: () => null,
  };
  const inv = window.__TAURI__.core.invoke;
  window.__TAURI__.core.invoke = async (c, a) => (extra[c] ? extra[c](a || {}) : inv(c, a));
  try { localStorage.setItem('cview.onboarded', '1'); localStorage.setItem('cview.welcomed.v1', String(Date.now())); } catch { /* 무시 */ }
}

async function open() {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 }, colorScheme: 'dark' });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, 'dark']);
  await page.addInitScript(chatBus);
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
  return { ctx, page, errors };
}

/** 시험용 멀티채팅 화면 — 앱 화면 객체는 밖에서 닿지 않아 같은 모듈로 하나 더 만든다. */
async function mount(page, ids, layout = 'merged') {
  await page.evaluate(async ([ids, layout]) => {
    document.querySelectorAll('[data-page]').forEach((x) => { x.hidden = true; });
    const box = document.createElement('div');
    box.id = 'mct';
    box.style.cssText = 'position:fixed;inset:0;width:1000px;height:760px;z-index:99;background:#111;display:flex';
    document.body.append(box);
    const { MultiChatPage } = await import('/js/multichat.js');
    const mc = new MultiChatPage(box, { isLoggedIn: () => true });
    mc.loaded = true;
    mc.root.hidden = false;
    for (const id of ids) await mc.add(id, false);
    for (const id of ids) window.__emit('chat://session-event', { session: `mc:${id}`, event: { type: 'connected' } });
    mc.layout = layout;
    mc.mergedList.innerHTML = '<div class="empty">새 메시지를 기다리는 중…</div>';
    mc.render();
    window.__mc = mc;
    const t = Date.now();
    window.__msg = (id, uid, content = `글 ${id}`) => ({ id, kind: 'chat', role: 'common_user', nickname: `닉${uid}`, userId: `user${uid}x`, content, emojis: {}, badges: [], timestamp: t });
    window.__push = (ch, msgs) => window.__emit('chat://session-event', { session: `mc:${ch}`, event: { type: 'messages', messages: msgs } });
  }, [ids, layout]);
  await page.waitForTimeout(150);
}
const frames = (page, ms = 120) => page.evaluate((ms) => new Promise((r) => requestAnimationFrame(() => setTimeout(r, ms))), ms);

/** 통합 입력으로 보낸다(Enter). */
async function sendMerged(page, text) {
  await page.fill('#mct .mc-merged-input input', text);
  await page.press('#mct .mc-merged-input input', 'Enter');
  await frames(page);
}

try {
  await check('M01 보낼 채널을 고른 뒤 다른 채널을 닫아도 고른 채널로 보낸다', async () => {
    const { ctx, page, errors } = await open();
    await mount(page, ['a', 'b', 'c']);
    await page.selectOption('#mct .mc-target', 'mc:b');
    await page.evaluate(() => window.__mc.remove('c'));
    await frames(page);
    const target = await page.$eval('#mct .mc-target', (s) => s.value);
    await sendMerged(page, '안녕');
    const sent = await page.evaluate(() => window.__sent);
    assert.equal(target, 'mc:b', '채널을 닫은 뒤에도 고른 보낼 채널이 남는다');
    assert.deepEqual(sent, [['안녕', 'mc:b']]);
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('M02 차단하면 다른 패널·통합 목록의 지난 줄도 걷힌다', async () => {
    const { ctx, page, errors } = await open();
    await mount(page, ['a', 'b'], 'grid');
    await page.evaluate(() => { window.__push('a', [window.__msg('a1', 7)]); window.__push('b', [window.__msg('b1', 7), window.__msg('b2', 8)]); });
    await frames(page, 300);
    const r = await page.evaluate(async () => {
      const mc = window.__mc;
      const n = (ch) => mc.channels.find((c) => c.channelId === ch).panel.list.querySelectorAll('.chat-line[data-uid="user7x"]').length;
      const before = [n('a'), n('b')];
      mc.channels[0].panel.block('user7x', '닉7');
      const afterGrid = [n('a'), n('b')];
      return { before, afterGrid };
    });
    assert.deepEqual(r.before, [1, 1]);
    assert.deepEqual(r.afterGrid, [0, 0], '패널 a 에서 차단하면 패널 b 의 그 사람 줄도 걷힌다');
    // 통합 목록 줄 메뉴로 차단 → 패널 목록에서도
    await page.evaluate(() => { const mc = window.__mc; mc.layout = 'merged'; mc.mergedList.innerHTML = ''; mc.render(); });
    await page.evaluate(() => { window.__push('a', [window.__msg('a2', 9)]); window.__push('b', [window.__msg('b3', 9)]); });
    await frames(page, 300);
    const r2 = await page.evaluate(() => {
      const mc = window.__mc;
      const merged0 = mc.mergedList.querySelectorAll('.chat-line[data-uid="user9x"]').length;
      mc.lines.view('a').block('user9x', '닉9');
      return { merged0, merged: mc.mergedList.querySelectorAll('.chat-line[data-uid="user9x"]').length, panes: mc.channels.map((c) => c.panel.list.querySelectorAll('.chat-line[data-uid="user9x"]').length) };
    });
    assert.equal(r2.merged0, 2);
    assert.equal(r2.merged, 0);
    assert.deepEqual(r2.panes, [0, 0], '통합 줄에서 차단하면 패널의 지난 줄도 걷힌다');
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('M03 통합 입력 /mute 가 통합 목록에 듣는다', async () => {
    const { ctx, page, errors } = await open();
    await mount(page, ['a', 'b']);
    await page.evaluate(() => { window.__push('a', [window.__msg('a1', 5), window.__msg('a2', 6)]); window.__push('b', [window.__msg('b1', 5)]); });
    await frames(page, 300);
    await sendMerged(page, '/mute 닉5');
    const mid = await page.evaluate(() => [...window.__mc.mergedList.querySelectorAll('.merged-line')].map((l) => `${l.dataset.src}:${l.dataset.nick}`));
    await page.evaluate(() => { window.__push('a', [window.__msg('a3', 5), window.__msg('a4', 6)]); });
    await frames(page, 300);
    const after = await page.evaluate(() => [...window.__mc.mergedList.querySelectorAll('.merged-line')].map((l) => `${l.dataset.src}:${l.dataset.nick}`));
    const sent = await page.evaluate(() => window.__sent);
    assert.deepEqual(sent, [], '명령은 채팅방으로 보내지 않는다');
    assert.deepEqual(mid, ['a:닉6', 'b:닉5'], '보낼 채널(a)의 그 사람 지난 줄만 걷힌다');
    assert.deepEqual(after, ['a:닉6', 'b:닉5', 'a:닉6'], '숨긴 뒤 온 그 사람 줄은 통합 목록에 붙지 않는다');
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('M04 통합 입력 /help·/stats 결과가 통합 보기에서 보인다', async () => {
    const { ctx, page, errors } = await open();
    await mount(page, ['a', 'b']);
    await sendMerged(page, '/help');
    const help = await page.evaluate(() => [...window.__mc.mergedList.querySelectorAll('.chat-line.system')].filter((l) => l.offsetParent).map((l) => l.textContent));
    assert.equal(help.length, 1, '명령어 목록이 통합 목록에 한 줄 보인다');
    assert.match(help[0], /\/mute/);
    await sendMerged(page, '/stats');
    const stats = await page.evaluate(() => [...document.querySelectorAll('.chat-tray')].filter((t) => !t.hidden && t.offsetParent && t.textContent.includes('채팅 통계')).length);
    assert.equal(stats, 1, '채팅 통계가 보이는 곳에 열린다');
    await page.mouse.click(400, 400); // 바깥을 누르면 닫힌다
    const open2 = await page.evaluate(() => [...document.querySelectorAll('.chat-tray')].filter((t) => !t.hidden && t.offsetParent).length);
    assert.equal(open2, 0);
    assert.deepEqual(await page.evaluate(() => window.__sent), []);
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('M05 채널을 닫고 더해도 색이 겹치지 않는다', async () => {
    const { ctx, page, errors } = await open();
    await mount(page, ['a', 'b', 'c']);
    const colors = await page.evaluate(async () => {
      const mc = window.__mc;
      await mc.remove('a');
      await mc.add('d', false);
      return mc.channels.map((c) => c.color);
    });
    assert.equal(new Set(colors).size, colors.length, `색 ${colors.join(',')}`);
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('M06 멀티라이브 멀티채팅 — /mute·/help 가 보이는 통합·칸 목록에 듣는다', async () => {
    const { ctx, page, errors } = await open();
    const r = await page.evaluate(async () => {
      localStorage.setItem('cview.ml.chatView', 'merged');
      const el = document.createElement('section');
      el.className = 'chat ml-chat';
      el.style.cssText = 'position:fixed;right:0;top:0;width:360px;height:700px;z-index:99;display:flex;flex-direction:column;background:#111';
      el.innerHTML = '<header class="chat-header"><strong>채팅</strong><span class="chat-state">연결 끊김</span></header><div class="chat-notice" hidden></div><div class="chat-list"></div><form class="chat-input"><input type="text" maxlength="100"><button class="btn primary" type="submit">채팅</button></form>';
      document.body.append(el);
      const { ChatPanel } = await import('/js/chat.js');
      const { MultiLiveMergedChat } = await import('/js/ml-multichat.js');
      const panel = new ChatPanel(el, { isLoggedIn: () => true, session: 'ml' });
      const state = { selectedId: 's0', sessions: [{ id: 's0', channelId: 'c0', channelName: '첫째', chatChannelId: 'cc0' }, { id: 's1', channelId: 'c1', channelName: '둘째', chatChannelId: 'cc1' }] };
      const mc = new MultiLiveMergedChat(el, { getState: () => state, getPanes: () => new Map(), onSelect: () => {} });
      mc.sync();
      const t = Date.now();
      const mk = (id, uid) => ({ id, kind: 'chat', role: 'common_user', nickname: `닉${uid}`, userId: `mluser${uid}x`, content: `글 ${id}`, emojis: {}, badges: [], timestamp: t });
      const send = (sid, list) => window.__emit('chat://session-event', { session: `mlx:${sid}`, event: { type: 'messages', messages: list } });
      const tick = (ms = 300) => new Promise((res) => requestAnimationFrame(() => setTimeout(res, ms)));
      send('s0', [mk('a', 1), mk('b', 2)]); send('s1', [mk('c', 1)]);
      await tick();
      const nicks = (box) => [...box.querySelectorAll('.merged-line')].map((l) => l.dataset.nick);
      const before = nicks(mc.list);
      for (const fn of panel.sendHooks) if (fn('/mute 닉1')) break;
      const mid = nicks(mc.list);
      send('s1', [mk('d', 1), mk('e', 2)]);
      await tick();
      const after = nicks(mc.list);
      mc.setMode('grid');
      await tick(600);
      const grid = nicks(mc.grid);
      for (const fn of panel.sendHooks) if (fn('/help')) break;
      await tick(300);
      const helpGrid = [...mc.grid.querySelectorAll('.chat-line.system')].filter((l) => l.textContent.includes('/mute')).length;
      mc.setMode('merged');
      for (const fn of panel.sendHooks) if (fn('/help')) break;
      await tick(300);
      const helpMerged = [...mc.list.querySelectorAll('.chat-line.system')].filter((l) => l.textContent.includes('/mute')).length;
      return { before, mid, after, grid, helpGrid, helpMerged };
    });
    assert.deepEqual(r.before, ['닉1', '닉2', '닉1']);
    assert.deepEqual(r.mid, ['닉2'], '/mute 가 통합 목록의 지난 줄을 걷는다');
    assert.deepEqual(r.after, ['닉2', '닉2'], '숨긴 뒤 온 줄도 통합 목록에 붙지 않는다');
    assert.deepEqual(r.grid, ['닉2', '닉2'], '칸 보기로 바꿔 다시 그려도 숨긴 사람 줄이 없다');
    assert.equal(r.helpGrid, 1, '/help 가 칸 보기(선택 패인 칸)에 보인다');
    assert.equal(r.helpMerged, 1, '/help 가 통합 목록에 보인다');
    assert.deepEqual(errors, []);
    await ctx.close();
  });
} finally {
  await browser.close();
  server.close();
}
console.log(failures.length ? `\n${failures.length}개 실패` : '\n모두 통과');
process.exit(failures.length ? 1 : 0);
