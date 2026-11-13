// 통합 멀티채팅 렌더러·입력부 공유(UI/UX 리뷰 P2-1 보류분) 회귀 검사 — 모의 IPC·헤드리스 Chromium(실제 서비스·로그인·전송 없음).
// MC01 통합 줄이 단일 채팅과 같은 줄 렌더러(lineHtml)를 쓴다 — 출처 태그·data-uid·아바타·이모티콘 단독 확대, 묶음은 같은 출처끼리만
//      (같은 사람이어도 채널이 다르면 묶지 않는다), 다음 묶음과 이어 붙는다(앞 줄에 has-next).
// MC02 통합 목록 줄 모양 — 기본은 촘촘(아바타·말풍선 없음, 한 줄), '말풍선'을 고르면 아바타·말풍선. 줄 높이를 잰다.
// MC03 통합 입력에 단일 채팅과 같은 입력부(ChatComposer) — 이모티콘 선택기(보낼 채널의 이모티콘), :/@ 자동완성, 글자 수, 입력 기록.
// MC04 통합 줄 동작 — 닉네임 → 말풍선(그 줄 채널로 카드 조회), 우클릭 줄 메뉴(고정 없음), 멘션은 그 줄 채널을 보낼 채널로 고르고 넣는다,
//      이 사람만 보기, 관리자 삭제(blinded).
// MC05 통합 목록은 프레임마다 한 번 붙인다 — 채널 8개가 따로 보내도 목록에는 한 번(예전엔 이벤트마다 붙이고 바닥을 맞췄다).
// MC06 멀티라이브 멀티채팅 — 통합·칸 목록도 같은 렌더러(칸은 태그 없음), 보기 메뉴의 '프로필·말풍선으로 보기',
//      다른 패인 줄의 멘션은 그 패인을 고르고 넣는다, 프레임마다 한 번 붙인다.
// MC07 통합 목록 폭주(채널 8개 합쳐 초당 200개, CPU 4배 감속) — 20ms 넘는 프레임 비율·줄 상한. 촘촘·말풍선 둘 다 잰다(숫자는 출력).
// 저장소 루트에서: node tools/ui-regression/merged-chat.mjs   (Playwright 가 필요하다)
//   CVIEW_UI_SHOTS=<폴더> 면 통합 목록(촘촘·말풍선)을 PNG 로 남긴다. CVIEW_MC_PERF_ONLY=1 이면 MC07 만(옛 코드와 비교용).
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, base, mockTauri } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');
const SHOTS = process.env.CVIEW_UI_SHOTS;
const PERF_ONLY = process.env.CVIEW_MC_PERF_ONLY === '1';
const results = [];
async function check(id, fn) {
  if (PERF_ONLY && !id.startsWith('MC07')) return;
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}
const browser = await chromium.launch();

/** 채팅 이벤트를 흘려 보낼 수 있는 모의 앱 — 멀티채팅에 필요한 명령을 더하고, 부른 명령을 기록한다. */
function chatBus() {
  const m = new Map();
  window.__TAURI__.event = { listen: async (n, f) => { if (!m.has(n)) m.set(n, new Set()); m.get(n).add(f); return () => m.get(n).delete(f); } };
  window.__emit = (n, p) => { for (const f of m.get(n) || []) f({ event: n, payload: p }); };
  window.__sent = [];
  window.__cards = [];
  const extra = {
    live_detail: ({ channelId }) => ({ channelId, channelName: { m: '마뫄', d: '다주' }[channelId] || `채널${channelId}`, chatChannelId: `cc${channelId}`, isLive: true }),
    chat_connect: () => ({ canSend: true }),
    chat_disconnect: () => null,
    chat_avatars: ({ userIds }) => userIds.map(() => ''),
    chat_emoticons: ({ channelId }) => [{ emoticonPackId: `p${channelId}`, emoticonPackName: `${channelId} 팩`, emoticons: [{ emoticonId: `${channelId}_smile`, emoticonName: `${channelId}웃음`, imageUrl: 'https://ssl.pstatic.net/static/nng/glive/icon/a.png' }] }],
    chat_send: ({ text, session }) => { window.__sent.push([text, session]); return null; },
    chat_user_card: ({ userId, chatChannelId }) => { window.__cards.push([userId, chatChannelId]); return { channel: null, following: false }; },
    settings_update: () => null,
  };
  const inv = window.__TAURI__.core.invoke;
  window.__TAURI__.core.invoke = async (c, a) => (extra[c] ? extra[c](a || {}) : inv(c, a));
  try { localStorage.setItem('cview.onboarded', '1'); localStorage.setItem('cview.welcomed.v1', String(Date.now())); } catch { /* 무시 */ }
}

async function open({ width = 1280, height = 900 } = {}) {
  const ctx = await browser.newContext({ viewport: { width, height }, colorScheme: 'dark' });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(process.env.CVIEW_MC_DEBUG ? String(e.stack) : String(e)));
  await page.addInitScript(mockTauri, [base, 'dark']);
  await page.addInitScript(chatBus);
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.waitForFunction(() => { const s = document.getElementById('splash'); return !s || !s.isConnected || getComputedStyle(s).opacity === '0' || getComputedStyle(s).display === 'none'; }, null, { timeout: 10000 }).catch(() => {});
  return { ctx, page, errors };
}

/** 시험용 멀티채팅 화면(채널 2개 m·d, 통합) — 앱의 화면 객체는 밖에서 닿지 않아 같은 모듈로 하나 더 만든다. */
async function mountMultiChat(page, channels = ['m', 'd']) {
  await page.evaluate(async (ids) => {
    document.querySelectorAll('[data-page]').forEach((x) => { x.hidden = true; });
    const box = document.createElement('div');
    box.id = 'mc-test';
    box.style.cssText = 'position:fixed;inset:0;width:900px;height:760px;z-index:99;background:#111;display:flex';
    document.body.append(box);
    const { MultiChatPage } = await import('/js/multichat.js');
    const mc = new MultiChatPage(box, { isLoggedIn: () => true });
    mc.loaded = true;
    mc.root.hidden = false;
    for (const id of ids) await mc.add(id, false);
    // 연결됨 — 패널이 보내기를 켠다(통합 입력은 보낼 채널 패널을 따른다)
    for (const id of ids) window.__emit('chat://session-event', { session: `mc:${id}`, event: { type: 'connected' } });
    mc.layout = 'merged';
    mc.mergedList.innerHTML = '<div class="empty">새 메시지를 기다리는 중…</div>';
    mc.render();
    window.__mc = mc;
  }, channels);
  await page.waitForTimeout(150);
}

const frame = (page) => page.evaluate(() => new Promise((r) => requestAnimationFrame(() => setTimeout(r, 0))));

await check('MC01 merged lines share the single-chat renderer (tag, data-uid, avatar, emote-only, per-source groups)', async () => {
  const { ctx, page, errors } = await open();
  const r = await page.evaluate(async () => {
    const { mergedItemsHtml, mergedLinesHtml } = await import('/js/multichat.js');
    const t = Date.UTC(2026, 10, 13, 3, 4, 5);
    const msg = (id, uid, content, emojis = {}) => ({ id, kind: 'chat', role: 'common_user', nickname: `닉${uid}`, userId: `user${uid}abc`, content, emojis, badges: [], timestamp: t });
    const box = document.createElement('div');
    box.className = 'chat-list';
    document.body.append(box);
    box.innerHTML = mergedItemsHtml([
      { m: msg('a1', 1, '첫 줄'), name: 'A', style: '--mc-color:#0f0', src: 'chA' },
      { m: msg('a2', 1, '같은 사람 같은 분'), name: 'A', style: '--mc-color:#0f0', src: 'chA' },
      { m: msg('b1', 1, '같은 사람 다른 채널'), name: 'B', style: '--mc-color:#00f', src: 'chB' },
      { m: msg('a3', 2, '{:e_1:}', { e_1: 'https://ssl.pstatic.net/static/nng/glive/icon/b.png' }), name: 'A', style: '--mc-color:#0f0', src: 'chA' },
    ]);
    const ls = [...box.children];
    const first = ls[0];
    const out = {
      n: ls.length,
      msg: ls.every((l) => l.classList.contains('msg') && l.classList.contains('merged-line')),
      uid: first.dataset.uid, nick: first.dataset.nick, src: first.dataset.src,
      tagInHead: Boolean(first.querySelector('.msg-head > .mc-tag')), tag: first.querySelector('.mc-tag').textContent,
      avatar: Boolean(first.querySelector('.chat-avatar.av')),
      cont: ls.map((l) => l.classList.contains('cont')), next: ls.map((l) => l.classList.contains('has-next')),
      emote: ls[3].classList.contains('emote-only'),
      color: first.getAttribute('style'),
    };
    // 다음 묶음 — 같은 출처·사람·분이면 앞 줄에 이어 붙인다(앞 줄 has-next)
    const more = mergedLinesHtml([msg('a4', 2, '이어진 말')], { name: 'A', style: '', src: 'chA', prev: box.lastElementChild });
    box.insertAdjacentHTML('beforeend', more);
    out.joined = [box.lastElementChild.classList.contains('cont'), box.children[3].classList.contains('has-next')];
    // 후원 카드도 태그·data-uid
    box.innerHTML = mergedLinesHtml([{ ...msg('d1', 3, '후원 글'), kind: 'donation', payAmount: 1000 }], { name: 'C', style: '', src: 'chC' });
    out.ev = { tag: Boolean(box.querySelector('.ev-card .mc-tag')), uid: box.firstElementChild.dataset.uid, src: box.firstElementChild.dataset.src };
    box.remove();
    return out;
  });
  assert.equal(r.n, 4);
  assert.equal(r.msg, true, '단일 채팅 줄(.msg)이 아니다');
  assert.equal(r.uid, 'user1abc');
  assert.equal(r.nick, '닉1');
  assert.equal(r.src, 'chA');
  assert.equal(r.tagInHead, true, '태그가 머리 줄에 없다');
  assert.equal(r.tag, 'A');
  assert.equal(r.avatar, true, '아바타가 없다');
  assert.deepEqual(r.cont, [false, true, false, false], `묶음(같은 출처만) ${r.cont}`);
  assert.deepEqual(r.next, [true, false, false, false], `묶음 끝 ${r.next}`);
  assert.equal(r.emote, true, '이모티콘 단독 확대(emote-only)가 없다');
  assert.match(r.color, /--mc-color:#0f0/);
  assert.deepEqual(r.joined, [true, true], '다음 묶음이 앞 줄에 이어지지 않았다');
  assert.deepEqual(r.ev, { tag: true, uid: 'user3abc', src: 'chC' });
  assert.deepEqual(errors, []);
  await ctx.close();
});

/** 두 채널이 번갈아 말하는 메시지 n개를 통합 목록에 붙인다. */
async function feedMerged(page, n, prefix = 'f') {
  await page.evaluate(([count, pre]) => {
    const t = Date.now();
    for (let i = 0; i < count; i++) {
      const ch = i % 2 ? 'd' : 'm';
      window.__mc.onSessionEvent({ session: `mc:${ch}`, event: { type: 'messages', messages: [{ id: `${pre}${i}`, kind: 'chat', role: 'common_user', nickname: `시청자${i % 7}`, userId: `viewer${i % 7}xyz`, content: `메시지 ${i} 오늘 방송 재밌네요`, emojis: {}, badges: [], timestamp: t }] } });
    }
  }, [n, prefix]);
  await frame(page);
}

await check('MC02 merged list: compact by default (one line, no avatar), bubble style on demand', async () => {
  const { ctx, page, errors } = await open();
  await mountMultiChat(page);
  await feedMerged(page, 30);
  const measure = () => page.evaluate(() => {
    const lines = [...document.querySelectorAll('#mc-test .mc-merged-list > .merged-line')];
    const hs = lines.map((l) => l.getBoundingClientRect().height);
    const av = lines[0].querySelector('.chat-avatar');
    return {
      compact: document.querySelector('#mc-test .mc-merged').classList.contains('mc-compact'),
      seg: [...document.querySelectorAll('#mc-test [data-mc-style]')].map((b) => [b.dataset.mcStyle, b.classList.contains('active'), b.getAttribute('aria-pressed')]),
      segShown: !document.querySelector('#mc-test .mc-style').hidden,
      avg: hs.reduce((a, b) => a + b, 0) / hs.length,
      avatarShown: Boolean(av) && getComputedStyle(av).display !== 'none',
      bubble: getComputedStyle(lines[0].querySelector('.msg-row > .text')).backgroundColor,
      fs: parseFloat(getComputedStyle(lines[0]).fontSize),
    };
  });
  const c = await measure();
  if (SHOTS) { fs.mkdirSync(SHOTS, { recursive: true }); await page.locator('#mc-test .mc-merged').screenshot({ path: path.join(SHOTS, 'merged-compact.png') }); }
  await page.click('#mc-test [data-mc-style="bubble"]');
  await page.waitForTimeout(100);
  const b = await measure();
  if (SHOTS) await page.locator('#mc-test .mc-merged').screenshot({ path: path.join(SHOTS, 'merged-bubble.png') });
  console.log(`  MC02 줄 높이(메시지당 평균) 촘촘 ${c.avg.toFixed(1)}px · 말풍선 ${b.avg.toFixed(1)}px (글자 ${c.fs}px)`);
  assert.equal(c.compact, true, '기본이 촘촘이 아니다');
  assert.equal(c.segShown, true, '통합에서 줄 모양 고르기가 보이지 않는다');
  assert.deepEqual(c.seg.map((x) => x.slice(0, 2)), [['compact', true], ['bubble', false]]);
  assert.equal(c.avatarShown, false, '촘촘인데 아바타가 보인다');
  assert.equal(c.bubble, 'rgba(0, 0, 0, 0)', '촘촘인데 말풍선 배경이 있다');
  assert.ok(c.avg <= c.fs * 2, `촘촘 줄이 한 줄보다 높다 ${c.avg}`);
  assert.equal(b.compact, false);
  assert.deepEqual(b.seg.map((x) => x.slice(0, 2)), [['compact', false], ['bubble', true]]);
  assert.equal(b.avatarShown, true, '말풍선인데 아바타가 없다');
  assert.ok(c.avg < b.avg * 0.75, `촘촘(${c.avg})이 말풍선(${b.avg})보다 충분히 낮지 않다`);
  // 다른 화면(탭)에서는 줄 모양 고르기를 감춘다
  await page.evaluate(() => { window.__mc.layout = 'tabs'; window.__mc.render(); });
  assert.equal(await page.$eval('#mc-test .mc-style', (x) => x.hidden), true);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('MC03 merged input has the single-chat composer (picker per target, :/@ autocomplete, counter, history)', async () => {
  const { ctx, page, errors } = await open();
  await mountMultiChat(page);
  // 채널 d 패널에 닉네임이 쌓이게(자동완성 @ 후보) — 패널 세션으로 메시지를 보낸다
  await page.evaluate(() => {
    window.__emit('chat://session-event', { session: 'mc:d', event: { type: 'messages', messages: [{ id: 'q1', kind: 'chat', role: 'common_user', nickname: '다주팬', userId: 'fanofdzz', content: '안녕', emojis: {}, badges: [] }] } });
  });
  await page.waitForTimeout(300);
  const form = '#mc-test .mc-merged-input';
  const s = await page.evaluate((f) => {
    const el = document.querySelector(f);
    return { btn: Boolean(el.querySelector('.chat-emoji-btn')), disabled: el.querySelector('.chat-emoji-btn')?.disabled, role: el.querySelector('input').getAttribute('role'), target: window.__mc.target.value };
  }, form);
  assert.equal(s.btn, true, '이모티콘 단추가 없다');
  assert.equal(s.disabled, false);
  assert.equal(s.role, 'combobox', '자동완성 콤보박스가 아니다');
  assert.equal(s.target, 'mc:m');
  await page.click(`${form} .chat-emoji-btn`);
  const cellsM = await page.$$eval(`${form} .emoji-cell`, (cs) => cs.map((c) => c.dataset.emoji));
  assert.deepEqual(cellsM, ['m_smile'], `보낼 채널(m) 이모티콘이 아니다 ${cellsM}`);
  await page.selectOption('#mc-test .mc-target', 'mc:d');
  const cellsD = await page.$$eval(`${form} .emoji-cell`, (cs) => cs.map((c) => c.dataset.emoji));
  assert.deepEqual(cellsD, ['d_smile'], `보낼 채널을 바꿨는데 이모티콘이 그대로 ${cellsD}`);
  await page.click(`${form} .emoji-cell`);
  assert.equal(await page.inputValue(`${form} input[type="text"]`), '{:d_smile:}');
  await page.fill(`${form} input[type="text"]`, '');
  await page.type(`${form} input[type="text"]`, '@다주');
  const ac = await page.$$eval(`${form} .chat-autocomplete [role=option]`, (os) => os.map((o) => o.textContent));
  assert.deepEqual(ac, ['@다주팬'], `@ 자동완성 ${ac}`);
  await page.keyboard.press('Enter');
  assert.equal(await page.inputValue(`${form} input[type="text"]`), '@다주팬 ');
  assert.equal(await page.evaluate(() => window.__sent.length), 0, '자동완성 Enter 가 보내 버렸다');
  await page.fill(`${form} input[type="text"]`, '');
  await page.type(`${form} input[type="text"]`, ':d_');
  assert.equal(await page.$$eval(`${form} .chat-autocomplete [role=option]`, (os) => os.length), 1, ': 자동완성 후보가 없다');
  await page.keyboard.press('Escape');
  await page.fill(`${form} input[type="text"]`, 'ㅋ'.repeat(85));
  const cnt = await page.$eval(`${form} .chat-count`, (c) => ({ t: c.textContent, hidden: c.hidden }));
  assert.deepEqual(cnt, { t: '85/100', hidden: false }, '글자 수가 보이지 않는다');
  await page.fill(`${form} input[type="text"]`, '통합에서 보냄');
  await page.press(`${form} input[type="text"]`, 'Enter');
  await page.waitForTimeout(100);
  assert.deepEqual(await page.evaluate(() => window.__sent), [['통합에서 보냄', 'mc:d']]);
  assert.equal(await page.inputValue(`${form} input[type="text"]`), '');
  await page.press(`${form} input[type="text"]`, 'ArrowUp');
  assert.equal(await page.inputValue(`${form} input[type="text"]`), '통합에서 보냄', '입력 기록(↑)이 없다');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('MC04 merged line actions: nickname pop per source, line menu, mention picks the source channel, solo, blinded', async () => {
  const { ctx, page, errors } = await open();
  await mountMultiChat(page);
  await feedMerged(page, 6);
  // 채널 d 줄(홀수)의 닉네임 → 말풍선, 카드는 d 채팅 채널로 묻는다
  const nick = '#mc-test .mc-merged-list > .merged-line[data-src="d"] .nick';
  await page.click(nick);
  await page.waitForSelector('.user-pop');
  assert.deepEqual(await page.evaluate(() => window.__cards.at(-1)), ['viewer1xyz', 'ccd'], '말풍선이 줄의 채널로 묻지 않았다');
  await page.click('.user-pop [data-up="solo"]');
  const solo = await page.evaluate(() => {
    const list = document.querySelector('#mc-test .mc-merged-list');
    const shown = [...list.querySelectorAll('.merged-line')].filter((l) => l.offsetParent !== null).map((l) => l.dataset.uid);
    return { on: list.classList.contains('solo'), shown: [...new Set(shown)], bar: !document.querySelector('#mc-test .mc-merged .chat-solo').hidden };
  });
  assert.deepEqual(solo, { on: true, shown: ['viewer1xyz'], bar: true }, `이 사람만 보기 ${JSON.stringify(solo)}`);
  await page.click('#mc-test .chat-solo [data-solo-off]');
  // 우클릭 줄 메뉴 — 고정은 없다(통합 목록에는 고정 줄이 없다)
  await page.click(nick, { button: 'right' });
  const items = await page.$$eval('.ctx-menu [data-ctx]', (bs) => bs.map((b) => b.dataset.ctx));
  assert.ok(items.includes('mention') && items.includes('profile') && items.includes('block'), `줄 메뉴 ${items}`);
  assert.ok(!items.includes('pin'), '통합 목록에 고정이 있다');
  await page.click('.ctx-menu [data-ctx="mention"]');
  const m = await page.evaluate(() => ({ target: window.__mc.target.value, value: document.querySelector('#mc-test .mc-merged-input input[type="text"]').value }));
  assert.deepEqual(m, { target: 'mc:d', value: '@시청자1 ' }, `멘션 ${JSON.stringify(m)}`);
  // 관리자 삭제 — 말풍선 모양 줄에서도 글이 바뀐다
  await page.evaluate(() => window.__mc.onSessionEvent({ session: 'mc:d', event: { type: 'blinded', messageId: 'f1', blindText: '' } }));
  const bl = await page.$eval('#mc-test .merged-line[data-id="f1"]', (l) => ({ cls: l.classList.contains('blinded'), t: l.querySelector('.text').textContent }));
  assert.equal(bl.cls, true);
  assert.match(bl.t, /삭제된 메시지/);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('MC05 merged list appends once per frame for many channel events', async () => {
  const { ctx, page, errors } = await open();
  await mountMultiChat(page);
  const r = await page.evaluate(async () => {
    const list = window.__mc.mergedList;
    const recs = [];
    const mo = new MutationObserver((ms) => recs.push(ms.filter((x) => x.addedNodes.length).length));
    mo.observe(list, { childList: true });
    for (let i = 0; i < 8; i++) {
      window.__mc.onSessionEvent({ session: `mc:${i % 2 ? 'd' : 'm'}`, event: { type: 'messages', messages: [{ id: `b${i}`, kind: 'chat', nickname: `n${i}`, userId: `user${i}aaa`, content: `묶음 ${i}`, emojis: {}, badges: [] }] } });
    }
    const sync = list.querySelectorAll('.merged-line').length;
    await new Promise((res) => requestAnimationFrame(() => setTimeout(res, 0)));
    mo.disconnect();
    return { sync, after: list.querySelectorAll('.merged-line').length, inserts: recs.reduce((a, b) => a + b, 0) };
  });
  assert.equal(r.sync, 0, '이벤트마다 바로 붙였다');
  assert.equal(r.after, 8);
  assert.ok(r.inserts <= 2, `붙인 횟수 ${r.inserts}`); // 안내(.empty) 걷기 + 한 번
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('MC06 multilive merged chat: shared renderer, bubble toggle in view menu, mention selects the pane', async () => {
  const { ctx, page, errors } = await open();
  const r = await page.evaluate(async () => {
    localStorage.setItem('cview.ml.chatView', 'merged');
    const el = document.createElement('section');
    el.className = 'chat ml-chat';
    el.style.cssText = 'position:fixed;right:0;top:0;width:360px;height:700px;z-index:99;display:flex;flex-direction:column;background:#111';
    el.innerHTML = '<header class="chat-header"><strong>채팅</strong><div class="chat-tools-inner"></div></header><div class="chat-list"></div><form class="chat-input"><input type="text" maxlength="100"><button class="btn primary">채팅</button></form>';
    document.body.append(el);
    const { MultiLiveMergedChat } = await import('/js/ml-multichat.js');
    const selected = [];
    const state = { selectedId: 's0', sessions: [{ id: 's0', channelId: 'c0', channelName: '첫째', chatChannelId: 'cc0' }, { id: 's1', channelId: 'c1', channelName: '둘째', chatChannelId: 'cc1' }] };
    const mc = new MultiLiveMergedChat(el, { getState: () => state, getPanes: () => new Map(), onSelect: (id) => selected.push(id) });
    mc.sync();
    const t = Date.now();
    const mk = (id, uid) => ({ id, kind: 'chat', role: 'common_user', nickname: `닉${uid}`, userId: `mluser${uid}x`, content: `글 ${id}`, emojis: {}, badges: [], timestamp: t });
    window.__emit('chat://session-event', { session: 'mlx:s0', event: { type: 'messages', messages: [mk('a', 1), mk('b', 1)] } });
    window.__emit('chat://session-event', { session: 'mlx:s1', event: { type: 'messages', messages: [mk('c', 2)] } });
    const sync = mc.list.querySelectorAll('.merged-line').length;
    await new Promise((res) => requestAnimationFrame(() => setTimeout(res, 0)));
    const lines = [...mc.list.querySelectorAll('.merged-line')];
    const cell = mc.conns.get('s1').list.querySelector('.merged-line');
    const out = {
      sync,
      n: lines.length,
      msg: lines.every((l) => l.classList.contains('msg')),
      tags: lines.map((l) => l.querySelector('.mc-tag')?.textContent),
      src: lines.map((l) => l.dataset.src),
      cont: lines.map((l) => l.classList.contains('cont')),
      cellTag: Boolean(cell.querySelector('.mc-tag')), cellUid: cell.dataset.uid,
      compact: mc.list.classList.contains('mc-compact'),
      item: mc.menu.querySelector('[data-mc-bubble]')?.getAttribute('role'),
      checked: mc.menu.querySelector('[data-mc-bubble]')?.getAttribute('aria-checked'),
    };
    mc.menu.querySelector('[data-mc-bubble]').click();
    out.after = { compact: mc.list.classList.contains('mc-compact'), cells: mc.grid.classList.contains('mc-compact'), checked: mc.menu.querySelector('[data-mc-bubble]').getAttribute('aria-checked') };
    // 다른 패인(s1) 줄의 멘션 — 그 패인을 고르고 입력에 넣는다
    lines[2].dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 50, clientY: 50 }));
    document.querySelector('.ctx-menu [data-ctx="mention"]').click();
    out.mention = { selected, value: el.querySelector('.chat-input input').value };
    return out;
  });
  assert.equal(r.sync, 0, '이벤트마다 바로 붙였다');
  assert.equal(r.n, 3);
  assert.equal(r.msg, true, '단일 채팅 줄(.msg)이 아니다');
  assert.deepEqual(r.tags, ['첫째', '첫째', '둘째']);
  assert.deepEqual(r.src, ['mlx:s0', 'mlx:s0', 'mlx:s1']);
  assert.deepEqual(r.cont, [false, true, false]);
  assert.equal(r.cellTag, false, '칸 줄에 태그가 있다');
  assert.equal(r.cellUid, 'mluser2x');
  assert.equal(r.compact, true);
  assert.equal(r.item, 'menuitemcheckbox');
  assert.equal(r.checked, 'false');
  assert.deepEqual(r.after, { compact: false, cells: false, checked: 'true' });
  assert.deepEqual(r.mention, { selected: ['s1'], value: '@닉2 ' });
  assert.deepEqual(errors, []);
  await ctx.close();
});

// 통합 목록 폭주 — 채널 8개, 합쳐 초당 200개(50ms 마다 채널마다 1~2개), CPU 4배 감속, 4초.
async function mergedBurst(style) {
  const { ctx, page } = await open();
  const cdp = await ctx.newCDPSession(page);
  const ids = ['m', 'd', 'a', 'b', 'c', 'e', 'f', 'g'];
  await mountMultiChat(page, ids);
  await page.evaluate(async (st) => {
    if (st === 'bubble') { const { updateChatPrefs } = await import('/js/chat-prefs.js'); await updateChatPrefs({ mergedChatStyle: 'bubble' }); }
    window.__trims = [];
    new MutationObserver((ms) => { const k = ms.reduce((a, r) => a + [...r.removedNodes].filter((x) => !x.classList?.contains('empty')).length, 0); if (k) window.__trims.push(k); }).observe(window.__mc.mergedList, { childList: true });
  }, style);
  await cdp.send('Emulation.setCPUThrottlingRate', { rate: 4 });
  const r = await page.evaluate(async (chs) => {
    let n = 0;
    const mk = () => ({ id: `syn-${++n}`, kind: 'chat', userId: `synuser${n % 300}`, nickname: `테스트${n % 50}`, content: `합성 메시지 ${n} 가나다라마바사 ㅋㅋㅋㅋ`, emojis: {}, role: 'common_user', badges: [], timestamp: Date.now() });
    const d = []; let last = performance.now(); let run = true;
    const tick = (t) => { d.push(t - last); last = t; if (run) requestAnimationFrame(tick); };
    requestAnimationFrame(tick);
    const until = performance.now() + 4000;
    while (performance.now() < until) {
      // 50ms 마다 10개를 채널 8개에 나눠 — 채널마다 따로 온다
      for (let i = 0; i < chs.length; i++) {
        const k = i < 2 ? 2 : 1;
        window.__mc.onSessionEvent({ session: `mc:${chs[i]}`, event: { type: 'messages', messages: Array.from({ length: k }, mk) } });
      }
      await new Promise((res) => setTimeout(res, 50));
    }
    run = false;
    await new Promise((res) => setTimeout(res, 300));
    d.shift();
    return { over20: d.filter((x) => x > 20).length / d.length, frames: d.length, lines: window.__mc.mergedList.querySelectorAll('.chat-line').length, trims: window.__trims.length };
  }, ids);
  await cdp.send('Emulation.setCPUThrottlingRate', { rate: 1 });
  await ctx.close();
  return r;
}

for (const style of ['compact', 'bubble']) {
  await check(`MC07 merged burst (${style}): frames hold, lines capped`, async () => {
    const r = await mergedBurst(style);
    console.log(`  MC07 ${style}: 20ms 넘은 프레임 ${(r.over20 * 100).toFixed(1)}% (${r.frames}프레임), 줄 ${r.lines}, 앞에서 지운 횟수 ${r.trims}`);
    assert.ok(r.lines <= 600 * 1.25 + 1, `줄 수 ${r.lines}`);
    assert.ok(r.over20 < 0.15, `20ms 넘은 프레임 ${(r.over20 * 100).toFixed(1)}%`);
  });
}

await browser.close();
server.close();
for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : `\n     ${r.error.split('\n').join('\n     ')}`}`);
const passed = results.filter((r) => r.ok).length;
console.log(`\n${passed}/${results.length} passed`);
process.exit(passed === results.length ? 0 : 1);
