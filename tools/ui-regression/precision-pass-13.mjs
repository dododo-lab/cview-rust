// 기능 정밀 고도화 13차(2026-11-27) 회귀 검사 — 채팅 명령·필터, 보조 창 뒤로·팔레트, 다시보기 화면.
// 모의 IPC·헤드리스 브라우저(실제 서비스·데이터는 건드리지 않는다). CVIEW_UI_BROWSER=webkit 이면 WebKit.
// 저장소 루트에서: NODE_PATH=<Playwright 경로> node tools/ui-regression/precision-pass-13.mjs
// T01 /block·/unblock·/mute — 띄어쓰기 있는 닉네임 전체를 읽고(/mute 는 끝의 숫자가 초), 화면의 다른 목록(멀티채팅 통합 등)에서도
//     그 사람을 찾는다. commandNick 순수 함수.
// T02 /filter 로 넣은 금지어는 이미 보이는 줄에도 걸린다.
// T03 자동 완성이 열린 채 한글 조합 중 Enter 는 후보를 넣지 않는다(조합을 끝내는 키).
// T04 보조 창(채팅 창) — 뒤로 가기(메뉴·⌘[)는 기록이 비면 그대로, 명령 팔레트의 화면 이동은 메인 창으로.
// T05 코드 확인 — 채팅 창 멘션 알림, 내보내기는 보이는 목록, 다시보기 '처음부터'·화질 변경·나중에 보기 연타,
//     메인 창 'navigate live' 는 라이브 메뉴.
// T06 단축키 — 잠금·입력기 키·조합 중 키는 바인딩이 아니고, 다른 동작과 겹치면 저장·기본값 되돌리기를 거부한다.
// T07 설정 — 후원 읽어 주기 음량·속도 기본값, 효과 없는 '화면 갱신'·'스트림 프록시 강제' 줄 없음, 프리셋 알림이 쓰던 입력을 지우지 않음.
// T08 검색·카테고리 — 다른 검색 중이면 같은 검색어도 다시 찾고 입력창을 맞춤, 발견 영역 2분·실패는 기억 안 함, 탭·카테고리 스크롤.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { base, server, mockTauri } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const WEBKIT = process.env.CVIEW_UI_BROWSER === 'webkit';
const browser = await (WEBKIT ? pw.webkit : pw.chromium).launch();
const MAC_UA = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)';
const failures = [];
let total = 0;
async function check(name, fn) {
  total += 1;
  try { await fn(); console.log(`ok   ${name}`); } catch (e) { failures.push(name); console.log(`FAIL ${name}\n     ${e.message.split('\n').join('\n     ')}`); }
}
const src = (f) => fs.readFileSync(path.resolve('app-ui/js', f), 'utf8');

function harness() {
  const inv = window.__TAURI__.core.invoke;
  window.__args = {};
  window.__TAURI__.core.invoke = async (c, a) => {
    (window.__args[c] ||= []).push(a || {});
    if (c === 'chat_connect') return { canSend: true };
    if (c === 'chat_send' || c === 'tray_control' || c === 'window_close_self') return null;
    if (c === 'chat_avatars') return (a.userIds || []).map(() => null);
    try { return await inv(c, a); } catch { return null; }
  };
  const subs = new Map();
  const listen = async (event, cb) => { (subs.get(event) || subs.set(event, []).get(event)).push(cb); return () => {}; };
  window.__TAURI__.event = { listen };
  window.__TAURI__.webviewWindow = { getCurrentWebviewWindow: () => ({ listen }) };
  window.__emit = (event, payload) => { for (const cb of subs.get(event) || []) cb({ payload }); };
  document.hasFocus = () => true;
}

async function open(url = '') {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 }, colorScheme: 'dark', userAgent: MAC_UA });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, 'dark', 12]);
  await page.addInitScript(harness);
  await page.goto(base + url);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
  return { ctx, page, errors };
}

/** index.html 원본 마크업으로 채팅 패널 하나를 띄운다(chat-ux.mjs 와 같은 방식). */
async function openChat() {
  const o = await open();
  await o.page.waitForFunction(() => !document.getElementById('splash') || !document.getElementById('splash').isConnected, null, { timeout: 20000 });
  await o.page.evaluate(async () => {
    const { ChatPanel } = await import('/js/chat.js');
    const { loadChatPrefs, setMyNickname } = await import('/js/chat-prefs.js');
    await loadChatPrefs();
    setMyNickname('도도');
    const doc = new DOMParser().parseFromString(await (await fetch('/index.html')).text(), 'text/html');
    const box = document.createElement('div');
    box.style.cssText = 'position:fixed;left:20px;top:10px;width:380px;height:860px;z-index:50;display:flex;flex-direction:column';
    box.innerHTML = doc.querySelector('#viewer .chat').outerHTML;
    document.body.appendChild(box);
    const root = box.firstElementChild;
    root.hidden = false;
    root.style.cssText = 'display:flex;flex-direction:column;height:100%;width:100%';
    const panel = new ChatPanel(root, { isLoggedIn: () => true });
    window.__panel = panel;
    await panel.connect('cc1', 'ch1');
    panel.onEvent({ type: 'connected' });
    let k = 0;
    const t = Date.now();
    window.__msg = (m) => ({ id: `m${++k}`, kind: 'normal', role: 'common_user', badges: [], emojis: {}, timestamp: t + k * 1000, ...m });
    window.__say = (list) => { panel.append(list.map((m) => window.__msg(m))); panel.flushLines(); };
    window.__send = async (text) => { panel.input.value = text; await panel.send(); };
    window.__prefs = async () => (await import('/js/chat-prefs.js')).chatPrefs();
  });
  return o;
}

try {
  await check('T01 /block·/mute — 띄어쓰기 닉네임 전체, 다른 목록의 사람도 찾음', async () => {
    const { ctx, page } = await openChat();
    const r = await page.evaluate(async () => {
      const { commandNick } = await import('/js/chat-extras.js');
      const pure = [commandNick(['김', '철수']), commandNick(['@김', '철수', '60'], { seconds: true }), commandNick(['60'], { seconds: true }), commandNick([])];
      window.__say([{ userId: 'u-kim', nickname: '김', content: '나는 김' }, { userId: 'u-kimcs', nickname: '김 철수', content: '나는 김 철수' }]);
      await window.__send('/block 김 철수');
      const p1 = await window.__prefs();
      // 화면의 다른 목록(멀티채팅 통합 목록 등)에만 있는 사람
      const other = document.createElement('div');
      other.className = 'chat-list';
      other.innerHTML = '<div class="chat-line" data-nick="바깥 사람" data-uid="u-out"><span class="text">안녕</span></div>';
      document.body.append(other);
      await window.__send('/block 바깥 사람');
      const p2 = await window.__prefs();
      window.__say([{ userId: 'u-lee', nickname: '이 영희', content: '하나' }]);
      await window.__send('/mute 이 영희 60');
      const toasts = [...document.querySelectorAll('.toast')].map((t) => t.textContent).join(' | ');
      const leeLeft = document.querySelectorAll('.chat-line[data-nick="이 영희"]').length;
      return { pure, b1: p1.blockedUsers || [], b2: p2.blockedUsers || [], toasts, leeLeft };
    });
    assert.deepEqual(r.pure, [{ nick: '김 철수', seconds: null }, { nick: '김 철수', seconds: 60 }, { nick: '60', seconds: null }, { nick: '', seconds: null }]);
    assert.ok(r.b1.includes('u-kimcs') && !r.b1.includes('u-kim'), `차단: ${r.b1}`);
    assert.ok(r.b2.includes('u-out'), `다른 목록의 사람: ${r.b2}`);
    assert.ok(r.toasts.includes('이 영희 님 메시지를 60초'), r.toasts);
    assert.equal(r.leeLeft, 0);
    await ctx.close();
  });

  await check('T02 /filter — 이미 보이는 줄에도 걸린다', async () => {
    const { ctx, page } = await openChat();
    const r = await page.evaluate(async () => {
      window.__say([
        { userId: 'a', nickname: '가', content: '광고링크 여기' },
        { userId: 'b', nickname: '나', content: '평범한 말' },
        { userId: 'c', nickname: '다', content: '또 광고링크' },
      ]);
      await window.__send('/filter 광고링크');
      await new Promise((r) => setTimeout(r, 50));
      return [...window.__panel.list.querySelectorAll('.chat-line:not(.system)')].map((l) => l.dataset.nick);
    });
    assert.deepEqual(r, ['나']);
    await ctx.close();
  });

  await check('T03 자동 완성 — 한글 조합 중 Enter 는 후보를 넣지 않음', async () => {
    const { ctx, page } = await openChat();
    const r = await page.evaluate(async () => {
      window.__say([{ userId: 'd', nickname: '도리도리', content: '안녕' }]);
      const input = window.__panel.input;
      input.focus();
      input.value = '@도리';
      input.dispatchEvent(new Event('input', { bubbles: true }));
      const ac = window.__panel.root.querySelector('.chat-autocomplete, [role="listbox"]');
      const opened = ac && !ac.hidden;
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', keyCode: 229, isComposing: true, bubbles: true, cancelable: true }));
      const during = input.value;
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
      return { opened, during, after: input.value };
    });
    assert.ok(r.opened, '자동 완성이 열리지 않음');
    assert.equal(r.during, '@도리', '조합 중 Enter 에 후보를 넣음');
    assert.ok(r.after.startsWith('@도리도리'), `조합 뒤 Enter: ${r.after}`);
    await ctx.close();
  });

  await check('T04 보조 창 — 뒤로 가기는 그대로, 팔레트 화면 이동은 메인 창으로', async () => {
    const { ctx, page, errors } = await open('?w=chat&channelId=ch1');
    await page.waitForTimeout(400);
    const shown = () => page.evaluate(() => [...document.querySelectorAll('[data-page]')].filter((p) => !p.hidden).map((p) => p.dataset.page));
    const before = await shown();
    await page.evaluate(() => window.__emit('menu://action', { id: 'back' }));
    await page.keyboard.press('Meta+BracketLeft');
    await page.waitForTimeout(200);
    assert.deepEqual(await shown(), before, '뒤로 가기로 보조 창이 홈이 됨');
    await page.keyboard.press('Meta+k');
    await page.waitForSelector('.palette-input input');
    await page.fill('.palette-input input', '멀티라이브');
    await page.waitForTimeout(150);
    await page.keyboard.press('Enter');
    await page.waitForTimeout(200);
    assert.deepEqual(await shown(), before, '팔레트로 보조 창 화면이 바뀜');
    const routes = await page.evaluate(() => (window.__args.tray_control || []).map((a) => a.action && a.action.route));
    assert.deepEqual(routes, ['multilive']);
    assert.equal(await page.evaluate(() => (window.__args.multi_live_add || []).length), 0);
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('T05 코드 확인 — 채팅 창 멘션 알림·보이는 목록 내보내기·다시보기·라이브 메뉴', async () => {
    assert.match(src('chat-only.js'), /\{ isLoggedIn, session, mentionNotify: true \}\);/);
    assert.match(src('chat-extras.js'), /const src = panel\.list\.hidden\s*\? \[\.\.\.panel\.root\.querySelectorAll\('\.chat-line'\)\]\.filter\(\(l\) => !panel\.list\.contains\(l\)\)/);
    const media = src('media.js');
    assert.match(media, /run: \(\) => \{ video\.removeEventListener\('loadedmetadata', resume\); video\.currentTime = 0; \}/);
    assert.match(media, /const wasPaused = keepPosition && v\.paused;[\s\S]*?if \(!wasPaused\) v\.play\(\)/);
    assert.match(media, /if \(a\.disabled\) break;[\s\S]*?a\.disabled = true;[\s\S]*?finally \{ a\.disabled = false; \}/);
    assert.match(src('app.js'), /if \(p\.route === 'live' && !Object\.keys\(params\)\.length\) openLiveMenu\(\);/);
    assert.match(src('update-notify.js'), /if \(!force && autoOn === false\) return;\s*renderUpdatePill\(u\);/);
  });
  await check('T06 단축키 — 잠금·입력기 키 무시, 겹치면 저장 거부', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async () => {
      const { bindingFromEvent, defaultBinding } = await import('/js/shortcuts.js');
      const ev = (o) => ({ metaKey: false, shiftKey: false, altKey: false, ctrlKey: false, ...o });
      return {
        caps: bindingFromEvent(ev({ key: 'CapsLock', code: 'CapsLock' })),
        hangul: bindingFromEvent(ev({ key: 'HangulMode', code: 'Lang1' })),
        process: bindingFromEvent(ev({ key: 'Process', code: 'KeyA', isComposing: false })),
        composingDigit: bindingFromEvent(ev({ key: '1', code: 'Digit1', isComposing: true })),
        composingLetter: bindingFromEvent(ev({ key: 'ㅊ', code: 'KeyC', isComposing: true })),
        f: bindingFromEvent(ev({ key: 'f', code: 'KeyF' })),
        def: defaultBinding('togglePiP'),
        none: defaultBinding('없는동작'),
      };
    });
    assert.equal(r.caps, null);
    assert.equal(r.hangul, null);
    assert.equal(r.process, null);
    assert.equal(r.composingDigit, null);
    assert.deepEqual(r.composingLetter, { key: 'c', modifiers: 0 });
    assert.deepEqual(r.f, { key: 'f', modifiers: 0 });
    assert.deepEqual(r.def, { key: 'p', modifiers: 0 });
    assert.equal(r.none, null);
    const set = src('settings.js');
    assert.ok(!set.includes('그 동작도 같은 키로 실행됩니다'), '겹치는데 저장하는 안내가 남음');
    assert.match(set, /if \(clash\) \{\s*toast\(`\$\{bindingLabel\(b\)\} 은\(는\) 이미 "\$\{clash\.label\}"에 쓰는 키입니다[\s\S]*?return;\s*\}\s*try \{ await saveBinding\(action, b\)/);
    assert.match(set, /const clash = def && conflictOf\(act, def\);\s*if \(clash\) \{ toast\(/);
    await ctx.close();
  });

  await check('T07 설정 — TTS 기본값·효과 없는 줄·프리셋 알림', async () => {
    const { ctx, page } = await open();
    const d = await page.evaluate(async () => { const { CHAT_DEFAULTS } = await import('/js/chat-prefs.js'); return [CHAT_DEFAULTS.ttsVolume, CHAT_DEFAULTS.ttsRate]; });
    assert.deepEqual(d, [0.8, 200]);
    const set = src('settings.js');
    assert.ok(!/k: 'frameRateMode'/.test(set), '화면 갱신 줄이 남음');
    assert.ok(!/k: 'forceStreamProxy'/.test(set), '스트림 프록시 강제 줄이 남음');
    assert.match(set, /if \(!typing && !drafted[^\n]*\) \{ this\.render\(\); return; \}/);
    await ctx.close();
  });

  await check('T08 검색·카테고리 — 다른 검색 중 재선택·발견 영역·스크롤', async () => {
    const pages = src('pages.js');
    assert.match(pages, /String\(params\.query\)\.trim\(\) === this\.query && this\.lastQuery === this\.query && \(this\.result \|\| this\.videos\)\) \{\s*this\.input\.value = params\.query;/);
    assert.match(pages, /if \(this\.discovery && Date\.now\(\) - this\.discoveryAt < 120_000\) return this\.discovery;/);
    assert.match(pages, /catch \{ return this\.discovery \|\| \{ cats: \[\], top: \[\] \}; \}/);
    assert.match(pages, /if \(changed\) this\.root\.scrollTop = 0;/);
    const cats = src('categories.js');
    assert.match(cats, /if \(enter && this\.view === ''\) this\.listScroll = this\.root\.scrollTop;/);
    assert.match(cats, /this\.root\.scrollTop = view \? 0 : \(this\.listScroll \|\| 0\);/);
  });
} finally {
  await browser.close();
  server.close();
}
console.log(failures.length ? `\n${failures.length}/${total} 실패: ${failures.join(', ')}` : `\n${total}/${total} 통과 (${WEBKIT ? 'WebKit' : 'Chromium'})`);
process.exit(failures.length ? 1 : 0);
