// 기능 정밀 고도화 4차(2026-11-26) 회귀 검사 — 3차에서 뺀 항목 재판단과 새 영역 검토로 찾은 실제 결함을 고친 것.
// 모의 IPC·헤드리스 브라우저(실제 서비스·데이터는 건드리지 않는다). CVIEW_UI_BROWSER=webkit 이면 WebKit.
// 저장소 루트에서: node tools/ui-regression/precision-pass-4.mjs
// S02 설정 — 효과 없던 '라이브 되감기'(켜기·범위·디스크 한도)가 설정 화면에 없다.
// S03 스크린샷 — 설정의 형식(JPEG)으로 찍고 .jpg 로 저장한다(시청 화면·단독 보기).
// S04 채널 화면 — '더 보기' 응답이 오는 중에 같은 탭을 다시 그려도 다시보기가 겹치지 않는다.
// S05 채널 메모 — 시트를 연 채 다른 채널 화면이 되어도 연 채널에 저장한다(처음 보는 채널은 이름도 함께 보낸다).
// S06 채널 화면 — 다른 곳에서 즐겨찾기를 바꾸면 별이 따라간다.
// S07 팔로잉 목록의 방송 중 줄 — 우클릭 즐겨찾기가 쓸 채널 이름·사진이 줄에 있다.
// S08 채팅 공지 — 하나를 숨겨도 새 공지는 보인다(같은 공지는 다시 받아도 숨긴 채).
// S09 TTS — 이모티콘 코드({:d_1:})를 읽지 않는다.
// S10 TTS — 읽어 주기를 끄면 읽던 문장도 멈춘다.
// S11 /mute 한 사람의 후원은 영상 위 알림으로도 띄우지 않는다.
// S12 후원 목록 — 차단한 사람의 후원은 목록·띠에서 빠지고, 관리자 삭제는 원문을 가린다.
// S13 플레이어 창의 채팅은 창 전용 세션을 쓴다.
// S14 멀티채팅 창은 메인 창 멀티채팅 화면과 다른 세션을 쓴다.
// S15 멀티라이브 — 카드로 잠깐 본(둘러본) 채널은 지난 구성을 덮지 않는다.
// S16 작업 공간 — 다시보기·클립 화면은 보관함으로 복원한다(광고와 함께 자동 재생하지 않는다).
// S01 설정 › 단축키 '바꾸기'로 키를 받는 중 ⌘K — 팔레트가 열리지 않고 예약 키라고 알린다.
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
  await check('S01 단축키를 받는 중 ⌘K 는 팔레트를 열지 않고 예약 키라고 알린다', async () => {
    const { ctx, page } = await open();
    await go(page, 'settings');
    await page.fill('[data-page="settings"] .settings-search', '스크린샷');
    await page.waitForSelector('[data-page="settings"] [data-bind]', { state: 'attached' });
    await page.evaluate(() => { const b = document.querySelector('[data-page="settings"] [data-bind]'); b.scrollIntoView(); b.click(); });
    await page.keyboard.press('Meta+k');
    await settle(page, 300);
    const r = await page.evaluate(() => ({
      palette: Boolean(document.querySelector('.palette-scrim:not([hidden])')),
      saved: window.__qCalls.some(([c, a]) => c === 'settings_update' && a && a.domain === 'keyboard'),
      toast: [...document.querySelectorAll('.toast')].map((t) => t.textContent).join(' '),
    }));
    assert.equal(r.palette, false, '키를 받는 중인데 팔레트가 열렸다');
    assert.equal(r.saved, false);
    assert.match(r.toast, /고정 키/);
    await ctx.close();
  });
  await check('S02 효과 없던 라이브 되감기 설정이 설정 화면에 없다', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async () => {
      const { SCHEMA } = await import('/js/settings.js');
      return SCHEMA.flatMap(([, , list]) => list).filter((i) => /^liveRewind/.test(i.k || '')).map((i) => i.k);
    });
    assert.deepEqual(r, []);
    await go(page, 'settings');
    await page.fill('[data-page="settings"] .settings-search', '되감기');
    await settle(page);
    const text = await page.evaluate(() => document.querySelector('[data-page="settings"]').textContent);
    assert.doesNotMatch(text, /라이브 되감기|되감기 디스크 한도/);
    await ctx.close();
  });

  await check('S03 스크린샷은 설정한 형식(JPEG)으로 찍고 .jpg 로 저장한다', async () => {
    const { ctx, page } = await open({ extra: {
      settings_get: '() => ({ player: { screenshotFormat: "jpeg" }, multiLive: {}, multiChat: {}, chat: {}, general: {}, appearance: {} })',
      save_file: '(a) => "/m/" + a.name',
    } });
    const r = await page.evaluate(async () => {
      const { ViewerControls } = await import('/js/viewer-controls.js');
      const { WatchTools } = await import('/js/ml-watch.js');
      const calls = [];
      const vc = Object.assign(Object.create(ViewerControls.prototype), { viewer: { isPlaying: true, live: { channelName: '채널' }, player: { call: (n, x) => calls.push([n, x]) } }, flash() {} });
      await vc.screenshot();
      await vc.saveScreenshot({ ok: true, base64: 'AAAA', format: 'image/jpeg' });
      const mw = Object.assign(Object.create(WatchTools.prototype), { flash() {} });
      Object.defineProperty(mw, 'pane', { value: { bridge: { call: (n, x) => calls.push([n, x]) } } });
      Object.defineProperty(mw, 'session', { value: { channelName: '패인' } });
      await mw.screenshot();
      await mw.saveScreenshot({ ok: true, base64: 'AAAA', format: 'image/jpeg' });
      const saves = window.__qCalls.filter(([c]) => c === 'save_file').map(([, a]) => a.fileName);
      return { formats: calls.filter(([n]) => n === 'takeScreenshot').map(([, x]) => x), saves };
    });
    assert.deepEqual(r.formats, ['jpeg', 'jpeg']);
    assert.equal(r.saves.length, 2);
    for (const name of r.saves) assert.match(name, /\.jpg/);
    await ctx.close();
  });
  await check('S04 더 보기 중 탭을 다시 그려도 다시보기가 겹치지 않는다', async () => {
    const { ctx, page } = await open({ extra: { channel_vods: '(a) => new Promise((res) => (window.__vod ||= []).push([a, res]))' } });
    const r = await page.evaluate(async () => {
      const { ChannelPage } = await import('/js/channel.js');
      const host = document.createElement('div');
      document.body.append(host);
      const p = new ChannelPage(host, { onBack() {}, onPlay() {} });
      Object.assign(p, { channelId: 'c1', detail: { channel: { channelName: '채널' } }, tab: 'vod' });
      p.body.innerHTML = '<div class="channel-tab-body"></div>';
      const vods = (from) => ({ items: Array.from({ length: 24 }, (_, i) => ({ videoNo: from + i, videoTitle: `v${from + i}`, duration: 60, publishDate: new Date().toISOString() })), hasMore: true });
      const wait = () => new Promise((res) => setTimeout(res, 20));
      const first = p.renderTab(); await wait();
      window.__vod.shift()[1](vods(0)); await first;
      p.pages.vod += 1; const more = p.renderTab(true); await wait(); // '더 보기'
      const again = p.renderTab(); await wait(); // 같은 탭을 다시 누름
      const [[, reMore], [, reFirst]] = [window.__vod.shift(), window.__vod.shift()];
      reMore(vods(24)); await more.catch(() => {}); // 늦게 온 앞 '더 보기'
      reFirst(vods(0)); await again;
      p.pages.vod += 1; const next = p.renderTab(true); await wait();
      const [args, res] = window.__vod.shift();
      res(vods(Number(args.page) * 24)); await next; // 요청한 쪽 그대로 답한다
      const nos = [...p.body.querySelectorAll('[data-vod-no]')].map((e) => e.dataset.vodNo);
      return { count: nos.length, unique: new Set(nos).size };
    });
    assert.deepEqual(r, { count: 48, unique: 48 });
    await ctx.close();
  });

  await check('S05 메모 시트는 연 채널에 저장한다', async () => {
    const { ctx, page } = await open({ extra: { memo_set: '() => null' } });
    const r = await page.evaluate(async () => {
      const { ChannelPage } = await import('/js/channel.js');
      const host = document.createElement('div');
      document.body.append(host);
      const p = new ChannelPage(host, { onBack() {}, onPlay() {} });
      Object.assign(p, { channelId: 'A', detail: { channel: { channelName: '에이', channelImageUrl: 'https://nng-phinf.pstatic.net/a.png' }, memo: '' }, tab: 'vod' });
      p.editMemo();
      document.querySelector('.sheet textarea').value = 'A 메모';
      Object.assign(p, { channelId: 'B', detail: { channel: { channelName: '비' }, memo: '' } }); // ⌘[ 로 다른 채널 화면
      document.querySelector('.sheet form, form.sheet').requestSubmit();
      await new Promise((res) => setTimeout(res, 100));
      return window.__qCalls.filter(([c]) => c === 'memo_set').map(([, a]) => a);
    });
    assert.equal(r.length, 1);
    assert.equal(r[0].channelId, 'A');
    assert.equal(r[0].channelName, '에이');
    await ctx.close();
  });

  await check('S06 다른 곳에서 바꾼 즐겨찾기를 채널 화면 별이 따라간다', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async () => {
      const { ChannelPage } = await import('/js/channel.js');
      const { channelSocialChanged } = await import('/js/channel-actions.js');
      const host = document.createElement('div');
      document.body.append(host);
      const p = new ChannelPage(host, { onBack() {}, onPlay() {} });
      Object.assign(p, { channelId: 'A', detail: { channel: { channelName: '에이' }, isFavorite: true } });
      p.body.innerHTML = '<button class="fav on" data-ch="favorite"></button>';
      channelSocialChanged('A', { favorite: false }); // 사이드바 우클릭으로 해제
      return { model: p.detail.isFavorite, on: p.body.querySelector('[data-ch="favorite"]').classList.contains('on') };
    });
    assert.deepEqual(r, { model: false, on: false });
    await ctx.close();
  });

  await check('S07 팔로잉 방송 중 줄에 채널 이름·사진이 있다', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async () => {
      const { followedRow } = await import('/js/pages.js');
      const box = document.createElement('div');
      box.innerHTML = followedRow({ channelId: 'c1', channelName: '팔로우채널', channelImageUrl: 'https://nng-phinf.pstatic.net/a.png', isLive: true, liveTitle: '방송', categoryName: '게임', concurrentUserCount: 10 });
      const row = box.querySelector('[data-channel-id]');
      return { name: row.dataset.channelName, image: row.dataset.channelImage };
    });
    assert.equal(r.name, '팔로우채널', '이름 없는 줄 — 우클릭 즐겨찾기가 빈 이름으로 저장된다');
    assert.match(r.image, /a\.png/);
    await ctx.close();
  });

  const chatPanel = `async (session) => {
    const { ChatPanel } = await import('/js/chat.js');
    const box = document.createElement('div');
    box.innerHTML = document.querySelector('#viewer .chat').outerHTML;
    document.body.appendChild(box);
    const root = box.firstElementChild;
    root.hidden = false;
    const panel = new ChatPanel(root, { isLoggedIn: () => true, session });
    panel.active = true;
    return panel;
  }`;

  await check('S08 공지 하나를 숨겨도 새 공지는 보인다', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async (mk) => {
      const panel = await (new Function(`return (${mk})`)())('n1');
      const notice = (id, content) => panel.onEvent({ type: 'notice', message: { id, nickname: '스트리머', content, emojis: {} } });
      notice('A', '공지 A');
      panel.notice.querySelector('[data-hide-notice]').click();
      notice('A', '공지 A'); // 다시 받아도 숨긴 채
      const sameHidden = panel.notice.hidden;
      notice('B', '공지 B');
      return { sameHidden, newShown: !panel.notice.hidden && panel.notice.textContent.includes('공지 B') };
    }, chatPanel);
    assert.deepEqual(r, { sameHidden: true, newShown: true });
    await ctx.close();
  });

  await check('S09 TTS 는 이모티콘 코드를 읽지 않는다', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async () => {
      const { ttsText } = await import('/js/chat-extras.js');
      return ttsText({ kind: 'donation', nickname: '팬', donationAmount: 1000, content: '고마워 {:d_10:}{:d_10:} 최고', emojis: { d_10: 'https://x' } });
    });
    assert.doesNotMatch(r, /\{:/);
    assert.match(r, /고마워 최고/);
    await ctx.close();
  });

  await check('S10 읽어 주기를 끄면 읽던 문장도 멈춘다', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async (mk) => {
      let cancels = 0;
      Object.defineProperty(window, 'speechSynthesis', { configurable: true, value: { speak() {}, cancel() { cancels += 1; }, getVoices: () => [], speaking: true, pending: false } });
      const { updateChatPrefs } = await import('/js/chat-prefs.js');
      await updateChatPrefs({ ttsEnabled: true });
      await (new Function(`return (${mk})`)())('t1');
      const before = cancels;
      await updateChatPrefs({ ttsEnabled: false });
      return cancels - before;
    }, chatPanel);
    assert.ok(r >= 1, '끈 뒤에도 읽던 문장을 멈추지 않았다');
    await ctx.close();
  });

  await check('S11 /mute 한 사람의 후원은 영상 위에 띄우지 않는다', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async (mk) => {
      const panel = await (new Function(`return (${mk})`)())('m1');
      const shown = [];
      panel.onHighlight = (m) => shown.push(m.nickname);
      panel.localHidden = (m) => m.nickname === '숨김';
      const don = (nick) => ({ id: `d-${nick}`, kind: 'donation', userId: `u-${nick}`, nickname: nick, content: '후원', donationAmount: 1000, emojis: {}, badges: [], role: 'viewer', timestamp: Date.now() });
      panel.onEvent({ type: 'messages', messages: [don('숨김'), don('보임')] });
      return shown;
    }, chatPanel);
    assert.deepEqual(r, ['보임']);
    await ctx.close();
  });

  await check('S12 후원 목록은 차단한 사람을 빼고 삭제된 원문을 가린다', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async (mk) => {
      const panel = await (new Function(`return (${mk})`)())('d1');
      const { updateChatPrefs } = await import('/js/chat-prefs.js');
      const don = (nick, id) => ({ id, kind: 'donation', userId: `u-${nick}`, nickname: nick, content: `${nick} 원문`, donationAmount: 1000, donationType: 'CHAT', emojis: {}, badges: [], role: 'viewer', timestamp: Date.now() });
      panel.donations.add([don('욕쟁이', 'x1'), don('팬', 'x2'), don('삭제됨', 'x3')]);
      await updateChatPrefs({ blockedUsers: ['u-욕쟁이'] });
      panel.blind('x3', '삭제된 메시지');
      const visible = panel.donations.visibleEntries().map((e) => e.m.nickname);
      const deleted = panel.donations.entries.find((e) => e.m.id === 'x3').m.content;
      await updateChatPrefs({ blockedUsers: [] });
      return { visible, deleted };
    }, chatPanel);
    assert.deepEqual(r.visible.sort(), ['삭제됨', '팬'].sort());
    assert.equal(r.deleted, '삭제된 메시지');
    await ctx.close();
  });

  await check('S13 플레이어 창의 채팅은 창 전용 세션을 쓴다', async () => {
    const ctx = await browser.newContext({ viewport: { width: 1100, height: 700 } });
    const page = await ctx.newPage();
    await page.addInitScript(mockTauri, [base, 'dark']);
    await page.addInitScript(overlay, {});
    await page.goto(`${base}?w=player`);
    await page.waitForSelector('[data-page]', { state: 'attached' });
    await settle(page, 500);
    const r = await page.evaluate(async () => {
      const { ChatPanel } = await import('/js/chat.js');
      // 플레이어 창 시청 화면의 채팅 패널 세션
      const panels = [...(ChatPanel.all ? ChatPanel.all() : [])];
      return panels.length ? panels.map((p) => p.session) : null;
    });
    if (r) assert.ok(r.some((s) => s && s.startsWith('pw-')), `세션: ${JSON.stringify(r)}`);
    else {
      const src = await (await fetch(`${base}js/app.js`)).text();
      assert.match(src, /chatSession: AUX === 'player' \? `pw-/);
    }
    await ctx.close();
  });

  await check('S14 멀티채팅 창은 메인 창과 다른 세션을 쓴다', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async () => {
      const { multichatSession } = await import('/js/multichat.js');
      return [multichatSession('x', null), multichatSession('x', 'multichat')];
    });
    assert.notEqual(r[0], r[1]);
    assert.equal(r[0], 'mc:x');
    await ctx.close();
  });

  await check('S15 둘러본 채널은 멀티라이브 지난 구성을 덮지 않는다', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async () => {
      const { rememberedChannels } = await import('/js/multilive.js');
      const transient = (id) => id === 's-e';
      return [rememberedChannels([{ id: 's-e', channelId: 'e' }], transient), rememberedChannels([{ id: 's-a', channelId: 'a' }, { id: 's-e', channelId: 'e' }], transient)];
    });
    assert.deepEqual(r, [[], ['a']]);
    const app = await (await fetch(`${base}js/app.js`)).text();
    assert.match(app, /pages\.multilive\.isTransient = \(sessionId\) => browsedWatch\.has\(sessionId\)/);
    await ctx.close();
  });

  await check('S16 다시보기 화면은 보관함으로 복원한다', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async () => {
      const { snapshotFor, loadWorkspace } = await import('/js/workspace.js');
      const snap = snapshotFor('media', { kind: 'vod', id: 123 }, null);
      localStorage.setItem('cview.workspace', JSON.stringify({ route: 'media', params: { kind: 'vod', id: 9 }, at: 1 }));
      const old = loadWorkspace();
      return { snap: [snap.route, snap.params], old: [old.route, old.params] };
    });
    assert.deepEqual(r, { snap: ['library', {}], old: ['library', {}] });
    await ctx.close();
  });
} finally {
  await browser.close();
  server.close();
}
console.log(`\n${total - failures.length}/${total} 통과 (${WEBKIT ? 'webkit' : 'chromium'})`);
if (failures.length) process.exit(1);
