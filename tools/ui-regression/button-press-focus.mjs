// 단추 누름·초점 테두리 회귀 검사 — 모의 IPC·헤드리스 브라우저(실제 로그인·채팅·후원 전송 없음).
// 예전엔 글자 단추(.btn)를 누르면 motion.css 의 transform: scale(.965)와 "버튼 모션" 기본 scale: .95 가 함께 걸려 .917 로 두 번 줄었다.
// '새 메시지'(.chat-jump)는 등장 애니메이션(mo-pop-up … both)이 끝난 뒤에도 scale: 1 을 붙들어 누름(.93)이 안 보였고,
// 추가 서랍 줄의 +(.ml-addrow-act)는 호버 크기(1.08)가 누르는 동안에도 남았다.
// 초점 테두리는 --primary(라이트 #00d38a)라 밝은 바탕에서 1.5~1.9:1, 초점 고리(--focus-ring, 3px 반투명)는 1.4:1 이었다.
// BF01 누름은 한 번 — 화면마다 보이는 단추를 모두 눌러 transform 의 크기와 scale 속성이 함께 줄지 않는지 본다.
//      .btn 은 한 번만 줄어 .955 이상(.965·넓은 단추 .985·재시도 .96). 화면에 없는 .btn 조합(더 보기·복원·재시도·a.btn)은 만들어 넣어 잰다.
// BF02 '새 메시지' 누름 — 등장이 끝난 뒤 누르면 실제로 줄어든다(.96 미만).
// BF03 서랍 + 누름 — 호버로 커진(1.08) 단추가 누르면 쉴 때보다 작아진다.
// BF04 초점 테두리 대비 — 키보드 초점(:focus-visible)의 테두리·고리가 그려진 자리마다 그 뒤 바탕과 3:1 이상(다크·라이트).
//      outline 이든 box-shadow 고리든 그려진 픽셀과, 테두리만 숨긴 화면의 같은 픽셀을 비교한다(겹친 면·반투명까지 그대로 잰다).
//      outline 과 box-shadow 고리가 한 요소에 함께 서지 않는다(글자 칸에서 둘이 붙어 5px 띠가 됐다).
// BF05 모션 줄이기(시스템 설정·앱 설정 두 경로) — 누름·호버에 크기가 바뀌지 않는다(.btn·'새 메시지'·서랍 +).
// BF06 고대비(forced-colors) — 엔진이 box-shadow 를 지우니 고리로만 초점을 보이던 곳에도 2px 이상 테두리가 선다.
//      Playwright 가 forced-colors 를 흉내 내지 못하는 엔진(WebKit)에서는 건너뛴다.
// BF07 CSS 정적 검사 — :focus·:focus-visible·:focus-within 규칙의 outline·border·box-shadow 가 var(--primary)(채움색)를 쓰지 않는다.
// 저장소 루트에서: node tools/ui-regression/button-press-focus.mjs   (Playwright 가 필요하다.
//   CVIEW_UI_BROWSER=webkit 이면 앱(macOS)과 같은 WebKit 엔진으로 돈다. CVIEW_UI_SHOTS=<폴더> 면 상태별 PNG 를 남긴다.
//   CVIEW_BF_REPORT=1 이면 잰 값(초점 테두리 대비·누름 배율)을 함께 찍는다.
//   CVIEW_BF_ONLY=nav-item,chat-jump 처럼 대상 key 를 주면 그 대상만 돈다 — 고칠 때 빨리 보려고. 이때 BF01 훑기·BF07 은 건너뛴다.)
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, base, mockTauri, go } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const WEBKIT = process.env.CVIEW_UI_BROWSER === 'webkit';
const engine = WEBKIT ? pw.webkit : pw.chromium;
const SHOTS = process.env.CVIEW_UI_SHOTS;
if (SHOTS) fs.mkdirSync(SHOTS, { recursive: true });
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}
const W = 1280, H = 820;
const fails = { BF01: [], BF02: [], BF03: [], BF04: [], BF05: [], BF06: [], BF07: [] };
const ONLY = (process.env.CVIEW_BF_ONLY || '').split(',').map((x) => x.trim()).filter(Boolean);
const want = (key) => !ONLY.length || ONLY.includes(key);
const REPORT = process.env.CVIEW_BF_REPORT ? [] : null;

// 장면별 모의 IPC — mock-app 의 로그인된 모의 위에 덮어쓴다.
// in: 로그인(기본) · chat: 채팅 상자 · drawer: 빈 멀티라이브 + 추가 서랍 · watch: 단독 보기(채널 정보란)
function sceneMock([scene]) {
  const inv = window.__TAURI__.core.invoke;
  const loggedOut = () => ({ loggedIn: false, state: 'loggedOut' });
  const lives = Array.from({ length: 4 }, (_, i) => ({ channelId: `p${i}`, channelName: `방송인${i + 1}`, liveTitle: `방송 ${i + 1}`, isLive: true, viewerCount: 100, viewerCountText: '100', openDate: new Date().toISOString() }));
  const names = [['c1', '한동숙'], ['c2', '침착맨'], ['c3', '풍월량'], ['c4', '릴카']];
  const sess = ([id, name], i) => ({ id: `s${i}`, slot: i, channelId: id, channelName: name, liveTitle: '오늘의 방송', viewerCountText: '1,234', openDate: new Date(Date.now() - 3600e3).toISOString(), isMuted: i > 0, isAudible: i === 0, state: { kind: 'playing' }, chatChannelId: null });
  const empty = { sessions: [], selectedId: null, layout: 'grid', canAdd: true, maxSessions: 4, rev: 1 };
  const watch = { sessions: names.map(sess), selectedId: 's0', focusMainId: 's0', canAdd: true, maxSessions: 5, layout: 'grid', focusDirection: 'left', focusRatio: 0.7, horizontalRatio: 0.5, verticalRatio: 0.5, multiAudio: false, watchId: 's0', rev: 1 };
  const over = {
    in: {},
    chat: {
      chat_connect: () => ({ canSend: true }), chat_disconnect: () => null, chat_avatars: (a) => (a.userIds || []).map(() => null),
      chat_donation_ranking: () => [], settings_update: () => null,
    },
    drawer: {
      auth_status: loggedOut, multilive_state: () => JSON.parse(JSON.stringify(empty)), multilive_select: () => JSON.parse(JSON.stringify(empty)),
      multilive_stage_size: () => null, popular_lives: () => lives, following_lives: () => [],
    },
    watch: {
      auth_status: loggedOut, multilive_state: () => JSON.parse(JSON.stringify(watch)), multilive_select: () => JSON.parse(JSON.stringify(watch)),
      multilive_stage_size: () => null, popular_lives: () => [], following_lives: () => [], settings_update: () => null,
      equalizer_presets: () => [[31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000], [{ name: '평탄', gains: new Array(10).fill(0), preAmp: 0 }]],
      channel_info: () => ({ verified: false, followerCount: 1000 }), live_detail: () => ({ liveTitle: '오늘의 방송', categoryName: '토크' }),
      channel_detail: ({ channelId }) => ({
        channel: { channelId, channelName: (names.find((n) => n[0] === channelId) || names[0])[1], channelImageUrl: null, verifiedMark: false, followerCount: 1000, channelDescription: '매일 저녁 방송합니다.' },
        live: { liveTitle: '오늘의 방송', categoryName: '토크', viewerCount: 1234, viewerCountText: '1,234', openDate: new Date(Date.now() - 3600e3).toISOString(), tags: ['소통'] },
      }),
    },
  }[scene] || {};
  window.__TAURI__.core.invoke = async (c, a) => (over[c] ? over[c](a || {}) : inv(c, a));
  try { localStorage.setItem('cview.welcomed.v1', String(Date.now())); localStorage.setItem('cview.onboarded', '1'); } catch { /* 무시 */ }
}

const browser = await engine.launch();
const calc = await (await browser.newContext()).newPage(); // 픽셀 계산용 빈 페이지

async function open(theme, scene, route, params = {}, { dpr = 1 } = {}) {
  const ctx = await browser.newContext({ viewport: { width: W, height: H }, colorScheme: theme, deviceScaleFactor: dpr });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, theme]);
  await page.addInitScript(sceneMock, [scene]);
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
  await page.waitForFunction(() => { const s = document.getElementById('splash'); return !s || !s.isConnected || getComputedStyle(s).display === 'none' || getComputedStyle(s).opacity === '0'; }, null, { timeout: 8000 }).catch(() => {});
  await go(page, route, params);
  await page.waitForTimeout(800);
  if (scene === 'chat') {
    // 시청 화면의 채팅 상자를 그대로 떼어 와 띄운다 — '새 메시지' 단추가 보이게
    await page.evaluate(async () => {
      const { ChatPanel } = await import('/js/chat.js');
      const { loadChatPrefs, setMyNickname } = await import('/js/chat-prefs.js');
      await loadChatPrefs();
      setMyNickname('도도');
      const src = new DOMParser().parseFromString(await (await fetch('/index.html')).text(), 'text/html');
      const box = document.createElement('div');
      box.id = 'bfbox';
      box.style.cssText = 'position:fixed;left:420px;top:40px;width:360px;height:600px;z-index:50;display:flex;flex-direction:column';
      box.innerHTML = src.querySelector('#viewer .chat').outerHTML;
      document.body.appendChild(box);
      const root = box.querySelector('.chat');
      root.hidden = false;
      root.style.cssText = 'display:flex;flex-direction:column;height:100%;width:100%';
      const panel = new ChatPanel(root, { isLoggedIn: () => true, onHighlight: () => {} });
      await panel.connect('cc1', 'ch1');
      panel.onEvent({ type: 'connected' });
      panel.jump.hidden = false;
    });
    await page.waitForTimeout(700);   // 등장(mo-pop-up .3s)이 끝나게
  }
  if (scene === 'drawer') {
    await page.click('.page-multilive .ml-toolbar [data-ml="add"]');
    await page.waitForSelector('.ml-addrow-act', { state: 'visible' });
    await page.waitForTimeout(500);
  }
  if (scene === 'watch') {
    await page.waitForSelector('.mwi-hero:not(.is-skeleton) .mwi-btn', { state: 'visible', timeout: 8000 });
    await page.waitForTimeout(900);   // 등장 모션(mo-enter)이 끝나게
  }
  return { ctx, page, errors };
}

const shot = async (page, clip, name) => {
  const buf = await page.screenshot({ clip });
  if (SHOTS && name) fs.writeFileSync(path.join(SHOTS, `${name}.png`), buf);
  return buf;
};
async function away(page) {
  await page.evaluate(() => { const a = document.activeElement; if (a && a !== document.body) a.blur(); });
  await page.mouse.move(W - 4, H - 4);
  await page.waitForTimeout(350);
}
// 대상 하나를 data-bf 로 표시한다(보이는 첫 요소 — 크기 0 인 체크박스처럼 ring 이 따로 있으면 크기를 안 본다)
async function mark(page, sel, { sized = true } = {}) {
  const ok = await page.evaluate(([s, sized]) => {
    const el = [...document.querySelectorAll(s)].find((e) => e.getClientRects().length && (!sized || e.getBoundingClientRect().width > 0) && getComputedStyle(e).visibility !== 'hidden' && !e.disabled);
    document.querySelectorAll('[data-bf]').forEach((x) => x.removeAttribute('data-bf'));
    if (!el) return false;
    el.setAttribute('data-bf', '');
    el.scrollIntoView({ block: 'center' });
    return true;
  }, [sel, sized]);
  assert.ok(ok, `요소가 보이지 않는다: ${sel}`);
}
const rectOf = (page, attr = 'data-bf') => page.evaluate((a) => { const b = document.querySelector(`[${a}]`).getBoundingClientRect(); return { x: b.x, y: b.y, w: b.width, h: b.height }; }, attr);
const clipAround = (r, m = 16) => { const x = Math.max(0, Math.floor(r.x - m)); const y = Math.max(0, Math.floor(r.y - m)); return { x, y, width: Math.min(W - x, Math.ceil(r.w + m * 2)), height: Math.min(H - y, Math.ceil(r.h + m * 2)) }; };
// 키보드로 초점 — 앞 요소에 초점을 두고 Tab(그래야 :focus-visible 이 선다)
async function keyFocus(page) {
  await page.evaluate(() => {
    const el = document.querySelector('[data-bf]');
    const all = [...document.querySelectorAll('button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])')].filter((e) => e.getClientRects().length && !e.disabled);
    (all[all.indexOf(el) - 1] || document.body).focus();
  });
  await page.keyboard.press('Tab');
  const on = await page.evaluate(() => document.activeElement === document.querySelector('[data-bf]'));
  if (!on) await page.evaluate(() => document.querySelector('[data-bf]').focus());
  await page.waitForTimeout(450);   // 초점 고리가 조여 드는 키프레임(.24s)이 끝나게
}
// 눌림 — 클릭 동작(화면 이동·메뉴)은 막고 눌린 상태(:active)만 만든다
async function pressDown(page, attr = 'data-bf') {
  const p = await page.evaluate((a) => {
    const el = document.querySelector(`[${a}]`);
    window.__bfBlock ||= (ev) => { if (ev.target.closest && ev.target.closest('[data-bf-block]')) { ev.preventDefault(); ev.stopImmediatePropagation(); } };
    for (const t of ['click', 'pointerdown', 'mousedown', 'pointerup', 'mouseup']) window.addEventListener(t, window.__bfBlock, true);
    el.setAttribute('data-bf-block', '');
    const b = el.getBoundingClientRect();
    return [b.x + b.width / 2, b.y + b.height / 2];
  }, attr);
  await page.mouse.move(p[0], p[1]);
  await page.mouse.down();
  await page.waitForTimeout(300);
}
async function pressUp(page) {
  await page.mouse.up();
  await page.mouse.move(W - 4, H - 4);
  await page.evaluate(() => document.querySelectorAll('[data-bf-block]').forEach((x) => x.removeAttribute('data-bf-block')));
  await page.waitForTimeout(400);
}
// 요소 하나의 누름 상태 — transform 의 가로 배율(t), scale 속성(s), 실제 너비
const pressState = (page, attr = 'data-bf') => page.evaluate((a) => {
  const el = document.querySelector(`[${a}]`);
  const cs = getComputedStyle(el);
  const t = cs.transform === 'none' ? 1 : Math.hypot(new DOMMatrix(cs.transform).a, new DOMMatrix(cs.transform).b);
  const s = !cs.scale || cs.scale === 'none' ? 1 : parseFloat(cs.scale);
  return { t, s, w: el.getBoundingClientRect().width, active: el.matches(':active') };
}, attr);

// ── BF01 누름은 한 번 ────────────────────────────────────────────────────────────
// 화면마다 보이는 단추(클래스 조합마다 하나)를 눌러 본다
const SCAN = ONLY.length ? [] : [['home', {}], ['following', {}], ['channel', { channelId: 'ch1' }], ['settings', {}], ['search', { q: '릴카' }], ['stats', {}], ['library', {}]];
let btnSeen = 0;
for (const [route, params] of SCAN) {
  const { ctx, page } = await open('dark', 'in', route, params);
  const n = await page.evaluate(() => {
    const seen = new Set();
    const els = [...document.querySelectorAll('button, a.btn, [role="button"]')].filter((e) => {
      const b = e.getBoundingClientRect();
      if (!(b.width > 4 && b.height > 4 && b.x >= 0 && b.y >= 0 && b.right <= innerWidth && b.bottom <= innerHeight) || e.disabled) return false;
      const h = document.elementFromPoint(b.x + b.width / 2, b.y + b.height / 2);
      if (!h || !(h === e || e.contains(h))) return false;
      const key = `${e.tagName}.${[...e.classList].sort().join('.')}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
    els.forEach((e, i) => e.setAttribute('data-bf-scan', i));
    return els.length;
  });
  for (let i = 0; i < n; i++) {
    const attr = `data-bf-scan="${i}"`;
    const name = await page.evaluate((a) => { const e = document.querySelector(`[${a}]`); return e && `${e.tagName.toLowerCase()}.${[...e.classList].join('.')}`; }, attr);
    if (!name) continue;   // 앞 단추를 누르다 화면이 다시 그려졌다
    const rest = await pressState(page, attr);
    await pressDown(page, attr);
    const st = await pressState(page, attr).catch(() => null);
    await pressUp(page);
    await page.keyboard.press('Escape').catch(() => {});
    if (!st || !st.active) continue;
    if (st.t < 0.999 && st.s < 0.999) fails.BF01.push(`${route} ${name}: 두 번 줄어듦 transform ${st.t.toFixed(3)} × scale ${st.s.toFixed(3)} = ${(st.t * st.s).toFixed(3)}`);
    if (/^(button|a)\.(.*\.)?btn(\.|$)/.test(name)) {
      btnSeen++;
      const k = (st.t * st.s) / (rest.t * rest.s);
      if (k < 0.955) fails.BF01.push(`${route} ${name}: .btn 누름 배율 ${k.toFixed(3)} (.955 미만)`);
    }
  }
  await ctx.close();
}
if (!ONLY.length && btnSeen < 4) fails.BF01.push(`눌러 본 .btn 이 ${btnSeen}개뿐 — 화면 구성이 바뀌어 검사가 비었다`);
// 화면에 없는 .btn 조합 — 만들어 넣고 누른다. 기대 배율: 더 보기·복원 .985, 재시도 .96
if (want('made')) {
  const { ctx, page } = await open('dark', 'in', 'home');
  const made = [
    ['<button class="btn load-more">더 보기</button>', 0.985], ['<button class="btn lg ml-restore">복원</button>', 0.985],
    ['<button class="btn primary overlay-retry">다시 시도</button>', 0.96],
    ['<button class="btn ghost">고스트</button>', 0.965], ['<button class="btn primary">기본</button>', 0.965],
  ];
  for (const [html, want] of made) {
    await page.evaluate((h) => {
      document.getElementById('bf-made')?.remove();
      const box = document.createElement('div');
      box.id = 'bf-made';
      box.style.cssText = 'position:fixed;left:600px;top:380px;z-index:9999;padding:20px;background:var(--background)';
      box.innerHTML = h;
      box.firstElementChild.setAttribute('data-bf', '');
      document.body.appendChild(box);
    }, html);
    await page.waitForTimeout(100);
    const rest = await pressState(page);
    await pressDown(page);
    const st = await pressState(page);
    await pressUp(page);
    const k = st.w / rest.w;
    if (st.t < 0.999 && st.s < 0.999) fails.BF01.push(`만든 ${html}: 두 번 줄어듦 ${st.t.toFixed(3)} × ${st.s.toFixed(3)}`);
    if (Math.abs(k - want) > 0.006) fails.BF01.push(`만든 ${html}: 누름 배율 ${k.toFixed(3)} (기대 ${want})`);
  }
  await ctx.close();
}

// ── BF02 '새 메시지' 누름 · BF03 서랍 + 누름 · BF05 모션 줄이기 ─────────────────────────────
// 대상마다: 쉼 너비 → (호버) → 누름 너비. motion: null(보통) | 'media'(시스템) | 'app'(앱 설정 data-motion="reduced")
async function pressRatio(page, { hoverFirst = false } = {}) {
  const rest = (await rectOf(page)).w;
  let hov = rest;
  if (hoverFirst) {
    const r = await rectOf(page);
    await page.mouse.move(r.x + r.w / 2, r.y + r.h / 2);
    await page.waitForTimeout(450);
    hov = (await rectOf(page)).w;
  }
  await pressDown(page);
  const st = await pressState(page);
  await pressUp(page);
  return { hover: hov / rest, press: st.w / rest, active: st.active };
}
const PRESS = [
  { key: 'btn', scene: 'in', route: 'following', sel: '.page-following .btn.ghost, .btn.ghost' },
  { key: 'chat-jump', scene: 'chat', route: 'home', sel: '#bfbox .chat-jump' },
  { key: 'addrow-act', scene: 'drawer', route: 'multilive', sel: '.ml-addrow:not(.added):not(.full) .ml-addrow-act', hoverFirst: true },
];
for (const T of PRESS.filter((t) => want(t.key))) {
  for (const motion of [null, 'media', 'app']) {
    const { ctx, page } = await open('dark', T.scene, T.route);
    try {
      if (motion === 'media') await page.emulateMedia({ reducedMotion: 'reduce' });
      if (motion === 'app') await page.evaluate(() => { document.documentElement.dataset.motion = 'reduced'; });
      await mark(page, T.sel);
      await away(page);
      if (SHOTS && !motion) {
        const r = await rectOf(page);
        await shot(page, clipAround(r, 24), `press-${T.key}-rest`);
        await pressDown(page);
        await shot(page, clipAround(r, 24), `press-${T.key}-down`);
        await pressUp(page);
      }
      const m = await pressRatio(page, T);
      const tag = `${T.key}${motion ? ` 모션 줄이기(${motion})` : ''}`;
      REPORT?.push(`누름 ${tag}: 호버 ${m.hover.toFixed(3)} · 누름 ${m.press.toFixed(3)}`);
      if (!m.active) { (motion ? fails.BF05 : fails[T.key === 'chat-jump' ? 'BF02' : T.key === 'addrow-act' ? 'BF03' : 'BF01']).push(`${tag}: 눌린 상태가 서지 않았다`); continue; }
      if (motion) {
        if (Math.abs(m.press - 1) > 0.003 || Math.abs(m.hover - 1) > 0.003) fails.BF05.push(`${tag}: 크기가 바뀜(호버 ${m.hover.toFixed(3)}, 누름 ${m.press.toFixed(3)})`);
      } else if (T.key === 'chat-jump') {
        if (!(m.press < 0.96)) fails.BF02.push(`${tag}: 눌러도 ${m.press.toFixed(3)}배 — 누름이 보이지 않는다`);
      } else if (T.key === 'addrow-act') {
        if (!(m.hover > 1.02)) fails.BF03.push(`${tag}: 호버 크기 ${m.hover.toFixed(3)}(호버 반응이 사라졌다)`);
        if (!(m.press < 0.99 && m.press < m.hover)) fails.BF03.push(`${tag}: 눌러도 ${m.press.toFixed(3)}배(호버 ${m.hover.toFixed(3)}) — 누름이 보이지 않는다`);
      }
    } catch (e) {
      (motion ? fails.BF05 : fails.BF02).push(`${T.key} ${motion || ''}: ${e.message.split('\n')[0]}`);
    }
    await ctx.close();
  }
}

// ── BF04 초점 테두리 대비 · BF06 고대비 ─────────────────────────────────────────────
// ring — 테두리를 그리는 요소: self(초점 받은 요소) | next(바로 뒤 형제 — 토글의 막대) | card(감싼 카드) | field(감싼 검색칸)
// shadow — 보통 때 초점이 box-shadow 고리라 고대비에서 따로 테두리가 있어야 하는 대상
const FOCUS = [
  { key: 'nav-item', scene: 'in', route: 'home', sel: '.nav-item' },
  { key: 'sidebar-toggle', scene: 'in', route: 'home', sel: '.icon-btn.sidebar-toggle' },
  { key: 'account-trigger', scene: 'in', route: 'home', sel: '.account-trigger', shadow: true },
  { key: 'hero-card', scene: 'in', route: 'home', sel: '.hero-main.card > .card-open', ring: 'card' },   // 스스로 --primary-text 를 바꾸는 히어로
  { key: 'card', scene: 'in', route: 'home', sel: '.card:not(.hero-main) > .card-open', ring: 'card' },
  { key: 'link-btn', scene: 'in', route: 'home', sel: '.link-btn' },
  { key: 'btn-ghost', scene: 'in', route: 'following', sel: '.btn.ghost' },
  { key: 'btn-primary', scene: 'in', route: 'following', sel: '[data-queue-multi]' },
  { key: 'seg', scene: 'in', route: 'following', sel: '.seg' },
  { key: 'chip', scene: 'in', route: 'following', sel: '.chip' },
  { key: 'toggle', scene: 'in', route: 'settings', prep: '.settings-tabs [data-group="watch"]', sel: '.toggle input', ring: 'next', shadow: true },
  { key: 'settings-search', scene: 'in', route: 'settings', sel: '.settings-search' },
  { key: 'lib-ov-title', scene: 'in', route: 'library', sel: '.lib-ov-title', shadow: true },
  { key: 'chart', scene: 'in', route: 'stats', sel: '.chart[tabindex]', shadow: true },
  { key: 'channel-btn', scene: 'in', route: 'channel', params: { channelId: 'ch1' }, sel: '.btn.ghost.sm' },
  { key: 'ci-link', scene: 'in', route: 'channel', params: { channelId: 'ch1' }, sel: '.ci-link' },
  { key: 'discover-item', scene: 'in', route: 'search', params: { q: '릴카' }, sel: '.discover-item' },
  { key: 'search-field', scene: 'in', route: 'search', params: { q: '릴카' }, sel: '.search-field input', ring: 'field', shadow: true },
  { key: 'ml-tb-icon', scene: 'drawer', route: 'multilive', sel: '.ml-tb-icon' },
  { key: 'ml-addrow-open', scene: 'drawer', route: 'multilive', sel: '.ml-addrow-open' },
  { key: 'mwi-btn', scene: 'watch', route: 'multilive', sel: '.mwi-hero:not(.is-skeleton) .mwi-btn.icon' },
  { key: 'mwi-tab', scene: 'watch', route: 'multilive', sel: '.segmented.mwi-tabs .seg' },
];
// 테두리 모양 — outline 이면 굵기·띄움, 아니면 box-shadow 의 흐림 0·퍼짐 2px 이상 고리(퍼짐·안쪽 여부)
const ringGeom = (page, ring) => page.evaluate((ring) => {
  const el = document.querySelector('[data-bf]');
  const r = ring === 'next' ? el.nextElementSibling : ring === 'card' ? el.closest('.card, .top-item') : ring === 'field' ? el.closest('.search-field') : el;
  document.querySelectorAll('[data-bf-ring]').forEach((x) => x.removeAttribute('data-bf-ring'));
  r.setAttribute('data-bf-ring', '');
  const cs = getComputedStyle(r);
  const b = r.getBoundingClientRect();
  const box = { x: b.x, y: b.y, w: b.width, h: b.height };
  const fv = el.matches(':focus-visible');
  const parts = (cs.boxShadow === 'none' ? '' : cs.boxShadow).split(/,(?![^(]*\))/).map((p) => p.trim()).filter(Boolean);
  let shadowRing = null;
  for (const p of parts) {
    const color = (p.match(/(?:rgba?|color|oklch|oklab|lab|lch|hsla?)\([^)]*\)|#[0-9a-f]{3,8}\b/i) || [''])[0];
    const lens = (p.replace(color, '').match(/-?[\d.]+px/g) || []).map(parseFloat);
    const [x = 0, y = 0, blur = 0, spread = 0] = lens;
    if (x === 0 && y === 0 && blur === 0 && spread >= 2) {   // 1px 는 쉴 때부터 있는 머리카락 테두리(inset 0 0 0 1px 등)
      const inset = /\binset\b/.test(p);
      shadowRing = { kind: 'shadow', w: spread, off: inset ? -spread : 0, box, fv };
      break;
    }
  }
  if (cs.outlineStyle !== 'none' && parseFloat(cs.outlineWidth) >= 1) return { kind: 'outline', w: parseFloat(cs.outlineWidth), off: parseFloat(cs.outlineOffset) || 0, box, fv, double: !!shadowRing };
  return shadowRing || { kind: 'none', box, fv };
}, ring);
// 그려진 테두리 띠의 가운데(네 변의 가운데)에서 초점 화면과 테두리만 숨긴 화면의 픽셀 대비
async function ringContrast(a, b, clip, g) {
  return calc.evaluate(async ({ a, b, clip, g }) => {
    const load = async (b64) => { const img = new Image(); img.src = `data:image/png;base64,${b64}`; await img.decode(); const c = document.createElement('canvas'); c.width = img.width; c.height = img.height; const x = c.getContext('2d'); x.drawImage(img, 0, 0); return { x, s: img.width / clip.width, w: img.width, h: img.height }; };
    const A = await load(a); const B = await load(b);
    const lum = ([R, G, Bl]) => { const f = (v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; }; return 0.2126 * f(R) + 0.7152 * f(G) + 0.0722 * f(Bl); };
    const ratio = (p, q) => { const [x, y] = [lum(p), lum(q)].sort((m, n) => n - m); return (x + 0.05) / (y + 0.05); };
    const d = g.off + g.w / 2;   // 테두리 상자 가장자리에서 띠 가운데까지(+ 바깥, - 안쪽)
    const { x, y, w, h } = g.box;
    const pts = { 위: [x + w / 2, y - d], 아래: [x + w / 2, y + h + d], 왼쪽: [x - d, y + h / 2], 오른쪽: [x + w + d, y + h / 2] };
    const out = {};
    for (const [side, [px, py]] of Object.entries(pts)) {
      const X = Math.floor((px - clip.x) * A.s), Y = Math.floor((py - clip.y) * A.s);
      if (X < 0 || Y < 0 || X >= A.w || Y >= A.h) continue;
      const pa = [...A.x.getImageData(X, Y, 1, 1).data.slice(0, 3)];
      const pb = [...B.x.getImageData(X, Y, 1, 1).data.slice(0, 3)];
      out[side] = ratio(pa, pb);
    }
    return out;
  }, { a: a.toString('base64'), b: b.toString('base64'), clip, g });
}
let forcedOk = true;
for (const theme of ['dark', 'light']) {
  for (const T of FOCUS.filter((t) => want(t.key))) {
    const tag = `${T.key} ${theme}`;
    const { ctx, page, errors } = await open(theme, T.scene, T.route, T.params || {}, { dpr: 2 });
    try {
      if (T.prep) { await page.click(T.prep); await page.waitForTimeout(400); }
      await mark(page, T.sel, { sized: !T.ring });
      await away(page);
      await keyFocus(page);
      const g = await ringGeom(page, T.ring);
      if (!g.fv) { fails.BF04.push(`${tag}: 키보드 초점이 서지 않았다(:focus-visible 아님)`); continue; }
      if (g.kind === 'none') { fails.BF04.push(`${tag}: 초점 테두리가 없다`); continue; }
      if (g.w < 2) fails.BF04.push(`${tag}: 테두리 ${g.w}px — 2px 미만`);
      if (g.double) fails.BF04.push(`${tag}: outline 과 box-shadow 고리가 함께 — 초점 테두리가 두 겹`);
      const clip = clipAround(g.box, Math.max(12, Math.ceil(Math.abs(g.off) + g.w + 6)));
      const on = await shot(page, clip, `focus-${T.key}-${theme}`);
      await page.addStyleTag({ content: '[data-bf-ring], [data-bf-ring]:focus-visible { outline-color: transparent !important; box-shadow: none !important; animation: none !important; }' });
      await page.waitForTimeout(80);
      const off = await shot(page, clip, null);
      const c = await ringContrast(on, off, clip, g);
      const drawn = Object.entries(c).filter(([, v]) => v >= 1.1);
      REPORT?.push(`초점 ${tag}: ${g.kind} ${g.w}px 최저 ${Math.min(...drawn.map(([, v]) => v)).toFixed(2)}:1`);   // 1.1 미만은 그 변이 잘려 안 그려진 것(부모 overflow 등)
      if (drawn.length < 2) fails.BF04.push(`${tag}: 테두리가 그려진 변이 ${drawn.length}개뿐 ${JSON.stringify(c)}`);
      const low = drawn.filter(([, v]) => v < 3);
      if (low.length) fails.BF04.push(`${tag}: 대비 ${low.map(([s, v]) => `${s} ${v.toFixed(2)}`).join(', ')} (3:1 미만, ${g.kind} ${g.w}px)`);
      if (errors.length) fails.BF04.push(`${tag}: 페이지 오류 ${errors[0]}`);
    } catch (e) {
      fails.BF04.push(`${tag}: ${e.message.split('\n')[0]}`);
    }
    await ctx.close();
  }
}
// 고대비 — 고리로 초점을 보이던 대상만. 흉내 내지 못하는 엔진은 건너뛴다
for (const T of FOCUS.filter((t) => t.shadow && want(t.key))) {
  const { ctx, page } = await open('light', T.scene, T.route, T.params || {});
  try {
    await page.emulateMedia({ forcedColors: 'active' });
    if (!(await page.evaluate(() => matchMedia('(forced-colors: active)').matches))) { forcedOk = false; await ctx.close(); break; }
    if (T.prep) { await page.click(T.prep); await page.waitForTimeout(400); }
    await mark(page, T.sel, { sized: !T.ring });
    await away(page);
    await keyFocus(page);
    const g = await ringGeom(page, T.ring);
    if (g.kind !== 'outline' || g.w < 2) fails.BF06.push(`${T.key}: 고대비 초점 테두리 없음(${g.kind} ${g.w || 0}px)`);
  } catch (e) {
    fails.BF06.push(`${T.key}: ${e.message.split('\n')[0]}`);
  }
  await ctx.close();
}

// ── BF07 CSS 정적 검사 ────────────────────────────────────────────────────────────
for (const file of ONLY.length ? [] : ['app.css', 'multilive.css', 'motion.css']) {
  const src = fs.readFileSync(path.join('app-ui/css', file), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '));
  for (const m of src.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const sel = m[1].replace(/\s+/g, ' ').trim();
    if (!/:focus(-visible|-within)?\b/.test(sel)) continue;
    for (const decl of m[2].split(';')) {
      const [prop, ...v] = decl.split(':');
      const name = (prop || '').trim();
      if (/^(outline|outline-color|border|border-color|box-shadow)$/.test(name) && /var\(--primary\)/.test(v.join(':'))) {
        const line = src.slice(0, m.index + m[0].indexOf(decl)).split('\n').length;
        fails.BF07.push(`${file}:${line} ${sel.slice(0, 80)} { ${decl.trim()} }`);
      }
    }
  }
}

const NAMES = {
  BF01: '.btn 은 한 번만 줄어든다(transform × scale 겹침 없음)', BF02: "'새 메시지' 누름이 보인다", BF03: '서랍 + 누름이 호버 크기를 놓는다',
  BF04: '초점 테두리 2px·뒤 바탕과 3:1 이상(다크·라이트)', BF05: '모션 줄이기에서 누름·호버 크기 변화 없음', BF06: '고대비에서 고리 대신 테두리',
  BF07: '초점 규칙의 테두리에 채움색 --primary 없음(CSS)',
};
for (const [id, list] of Object.entries(fails)) {
  if (id === 'BF06' && !forcedOk) { results.push({ id: `${id} ${NAMES[id]}`, ok: true, note: '건너뜀 — 이 엔진은 forced-colors 를 흉내 내지 못한다' }); continue; }
  await check(`${id} ${NAMES[id]}`, async () => assert.deepEqual(list, []));
}

await browser.close();
server.close();
if (REPORT) console.log(REPORT.join('\n'));
for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.note ? ` (${r.note})` : ''}${r.ok ? '' : `\n     ${r.error.split('\n').filter((l) => /^[+-] /.test(l.trim()) || /^\s+'/.test(l)).slice(0, 24).join('\n     ') || r.error.slice(0, 900)}`}`);
const failed = results.filter((r) => !r.ok).length;
console.log(`\n${results.length - failed}/${results.length} passed (${WEBKIT ? 'webkit' : 'chromium'})`);
process.exit(failed ? 1 : 0);
