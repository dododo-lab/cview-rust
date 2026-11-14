// primary 단추 호버 회귀 검사 — 모의 IPC·헤드리스 브라우저(실제 로그인·채팅·후원 전송 없음).
// 예전엔 .btn.primary 에 마우스를 올리면 공통 .btn:hover 의 회색 바탕(--surface-overlay)과 회색 테두리가 초록 채움을 덮어
// 다크 테마 글자 대비가 1.44 였고(6d80f5e~), 쉼·호버·누름 모두 단추 뒤로 브랜드색 글로우(0 6px 18px·0 8px 22px·0 3px 10px)가 번졌다.
// '새 메시지'(.chat-jump)·추가 서랍 줄의 +(.ml-addrow-act)·단독 보기 '채널 홈'(.mwi-hero .mwi-btn[data-mwi=channel])도 호버 때 번졌다.
// 채널 추가 단추(.ch-add)는 channel-add-hover.mjs 가 본다 — 여기서는 그 밖의 primary 단추.
// 대상: 홈 로그인(큰 단추) · 사이드바 로그인 · 팔로잉 '이 목록 멀티라이브로' · 채팅 보내기(카카오 스킨) · 채팅 '새 메시지' ·
//       추가 서랍 줄의 + · 단독 보기 '채널 홈'.
// PB01 대비 — 쉼·호버·초점·누름에서 글자(아이콘)색과 단추 안쪽 바탕의 대비가 4.5:1 이상(다크·라이트).
// PB02 번지는 글로우 없음(계산 스타일) — 단추와 ::before/::after 에 filter blur·drop-shadow 가 없고, 흐림 3px 넘는 바깥 그림자는
//      무채색이면서 쉴 때부터 있던 것(떠 있는 '새 메시지'의 높이 그림자)뿐. 브랜드색 그림자·호버·누름에 새로 생기는 번짐은 실패.
// PB03 바깥 번짐 없음(픽셀) — 단추 경계 3px 바깥에서 호버 전후 바뀐 픽셀 3% 이하. 채움 단추는 쉼 모양도(단추를 감춘 화면과 비교).
// PB04 키보드 초점 — :focus-visible 이면 2px 이상 테두리, 테두리색이 뒤 바탕과 3:1 이상, 빛을 받는 단추는 안쪽도 호버만큼 켜진다.
// PB05 채움 유지 — 채움 단추는 호버·초점에서 바탕색·테두리색이 쉴 때와 같다(회색으로 바뀌지 않는다).
//      aria-disabled(가득 참처럼 탭 순서에 남지만 누를 수 없는 단추)는 호버·누름에 모양이 바뀌지 않는다.
// PB06 모션 줄이기(시스템 설정·앱 설정) — 호버·누름에 단추·::before 가 움직이지 않고 키프레임·전환이 돌지 않는다.
// PB07 고대비(forced-colors, Chromium 만) — 흰빛 층(::before)·바깥 그림자가 그려지지 않고, 호버가 테두리·윤곽으로 보인다.
// PB08 color-mix 없는 엔진(Safari 16.0 흉내 — color-mix 선언을 버리고 color-mix 를 품은 변수는 무효로) — 대비 4.5:1,
//      테두리가 글자색(currentColor)으로 떨어지지 않고, 빛을 받는 단추는 쉴 때 윗빛·턱이 남는다.
// 저장소 루트에서: node tools/ui-regression/primary-button-hover.mjs   (Playwright 가 필요하다.
//   CVIEW_UI_BROWSER=webkit 이면 앱(macOS)과 같은 WebKit 엔진으로 돈다. CVIEW_UI_SHOTS=<폴더> 면 상태별 PNG 를 남긴다 —
//   CVIEW_UI_DPR=2 면 레티나 배율로 찍는다.)
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

// 장면별 모의 IPC — mock-app 의 로그인된 모의 위에 덮어쓴다.
// out: 로그아웃(홈 로그인 띠·사이드바 로그인) · in: 로그인(팔로잉) · chat: 채팅 상자 · drawer: 빈 멀티라이브 + 추가 서랍 · watch: 단독 보기
function sceneMock([scene]) {
  const inv = window.__TAURI__.core.invoke;
  const loggedOut = () => ({ loggedIn: false, state: 'loggedOut' });
  const lives = Array.from({ length: 4 }, (_, i) => ({ channelId: `p${i}`, channelName: `방송인${i + 1}`, liveTitle: `방송 ${i + 1}`, isLive: true, viewerCount: 100, viewerCountText: '100', openDate: new Date().toISOString() }));
  const names = [['c1', '한동숙'], ['c2', '침착맨'], ['c3', '풍월량'], ['c4', '릴카']];
  const sess = ([id, name], i) => ({ id: `s${i}`, slot: i, channelId: id, channelName: name, liveTitle: '오늘의 방송', viewerCountText: '1,234', openDate: new Date(Date.now() - 3600e3).toISOString(), isMuted: i > 0, isAudible: i === 0, state: { kind: 'playing' }, chatChannelId: null });
  const empty = { sessions: [], selectedId: null, layout: 'grid', canAdd: true, maxSessions: 4, rev: 1 };
  const watch = { sessions: names.map(sess), selectedId: 's0', focusMainId: 's0', canAdd: true, maxSessions: 5, layout: 'grid', focusDirection: 'left', focusRatio: 0.7, horizontalRatio: 0.5, verticalRatio: 0.5, multiAudio: false, watchId: 's0', rev: 1 };
  const over = {
    out: { auth_status: loggedOut },
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

// kind — primary: 빛을 받는 키(.btn.primary) · skin: 카카오 스킨 보내기(납작한 노랑 그대로) · float: 떠 있는 '새 메시지' ·
//        icon: 추가 서랍 줄의 둥근 + · pill: 단독 보기 '채널 홈'(글자·바탕을 뒤집은 알약)
// rest — 'away'(커서를 치움) 또는 'row'(줄에 올림 — 줄 호버가 이미 + 를 물들인다, 거기서 단추로 옮겨 간 차이를 본다)
const TARGETS = [
  { key: 'home-login', scene: 'out', route: 'home', sel: '.home-banner [data-action="login"]', kind: 'primary' },
  { key: 'sidebar-login', scene: 'out', route: 'home', sel: '.account-login', kind: 'primary' },
  { key: 'queue-multi', scene: 'in', route: 'following', sel: '[data-queue-multi]', kind: 'primary' },
  { key: 'chat-send', scene: 'chat', route: 'home', sel: '#pbbox .chat-input .btn.primary', kind: 'skin' },
  { key: 'chat-jump', scene: 'chat', route: 'home', sel: '#pbbox .chat-jump', kind: 'float' },
  { key: 'addrow-act', scene: 'drawer', route: 'multilive', sel: '.ml-addrow:not(.added):not(.full) .ml-addrow-act', kind: 'icon', rest: 'row' },
  { key: 'mwi-channel', scene: 'watch', route: 'multilive', sel: '.mwi-hero:not(.is-skeleton) .mwi-btn[data-mwi="channel"]', kind: 'pill' },
];
const LIT = new Set(['primary', 'float', 'icon', 'pill']);   // 안쪽 빛으로 켜지는 단추
const FILLED_REST = new Set(['primary', 'skin', 'pill']);    // 쉴 때부터 채운 단추 — 쉼 모양의 바깥 번짐·채움 유지도 본다

// Safari 16.0 흉내 — color-mix 를 모르는 엔진처럼 CSS 를 바꿔 보낸다.
// 일반 선언은 구문 오류라 통째로 버려지고(앞 규칙 값이 남는다), 변수 값은 그대로 들고 있다가 쓰는 자리에서 무효(unset)가 된다.
function safari16(css) {
  const MIX = 'nocolor-mix';
  css = css.replace(/color:\s*color-mix\(in srgb, red 50%, blue\)/g, `color: ${MIX}`);   // @supports 조건 — 이제 'not' 쪽이 참
  // 변수(--x: … color-mix(…) …) — 값 전체를 무효 토큰으로
  css = css.replace(/(--[\w-]+)\s*:([^;{}]*color-mix\([^;{}]*)(;|(?=\}))/g, (m, name) => `${name}: ${MIX};`);
  // 일반 선언 — 버린다
  css = css.replace(/(^|[;{\s])([a-z-]+)\s*:([^;{}]*color-mix\([^;{}]*)(;|(?=\}))/g, (m, pre) => pre);
  return css;
}

const browser = await engine.launch();
const calc = await (await browser.newContext()).newPage(); // 픽셀 계산용 빈 페이지

async function open(theme, T, { legacy = false } = {}) {
  const ctx = await browser.newContext({ viewport: { width: W, height: H }, colorScheme: theme, deviceScaleFactor: Number(process.env.CVIEW_UI_DPR) || 1 });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  if (legacy) {
    await page.route('**/css/*.css', async (route) => {
      const res = await route.fetch();
      route.fulfill({ response: res, body: safari16(await res.text()), headers: { ...res.headers(), 'content-type': 'text/css' } });
    });
  }
  await page.addInitScript(mockTauri, [base, theme]);
  await page.addInitScript(sceneMock, [T.scene]);
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
  await page.waitForFunction(() => { const s = document.getElementById('splash'); return !s || !s.isConnected || getComputedStyle(s).display === 'none' || getComputedStyle(s).opacity === '0'; }, null, { timeout: 8000 }).catch(() => {});
  await go(page, T.route);
  await page.waitForTimeout(700);
  if (T.scene === 'chat') {
    await page.evaluate(async () => {
      const { ChatPanel } = await import('/js/chat.js');
      const { loadChatPrefs, setMyNickname } = await import('/js/chat-prefs.js');
      await loadChatPrefs();
      setMyNickname('도도');
      const src = new DOMParser().parseFromString(await (await fetch('/index.html')).text(), 'text/html');
      const box = document.createElement('div');
      box.id = 'pbbox';
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
      window.__panel = panel;
    });
    await page.waitForTimeout(500);
  }
  if (T.scene === 'drawer') {
    await page.click('.page-multilive .ml-toolbar [data-ml="add"]');
    await page.waitForSelector(T.sel, { state: 'visible' });
    await page.waitForTimeout(500);
  }
  if (T.scene === 'watch') {
    await page.waitForSelector(T.sel, { state: 'visible', timeout: 8000 });
    await page.waitForTimeout(900);   // 등장 모션(mo-enter)이 끝나게
  }
  return { ctx, page, errors };
}

// 대상 하나를 data-pbh 로 표시하고 위치를 돌려준다(보이는 첫 요소)
async function mark(page, sel) {
  const r = await page.evaluate((s) => {
    const el = [...document.querySelectorAll(s)].find((e) => e.getClientRects().length && e.getBoundingClientRect().width > 0 && getComputedStyle(e).visibility !== 'hidden');
    if (!el) return null;
    document.querySelectorAll('[data-pbh]').forEach((x) => x.removeAttribute('data-pbh'));
    el.setAttribute('data-pbh', '');
    el.scrollIntoView({ block: 'center' });
    const b = el.getBoundingClientRect();
    return { x: b.x, y: b.y, w: b.width, h: b.height, disabled: !!el.disabled };
  }, sel);
  assert.ok(r, `단추가 보이지 않는다: ${sel}`);
  assert.ok(!r.disabled, `단추가 꺼져 있다: ${sel}`);
  return r;
}
const rectNow = (page) => page.evaluate(() => { const b = document.querySelector('[data-pbh]').getBoundingClientRect(); return { x: b.x, y: b.y, w: b.width, h: b.height }; });
const MARGIN = 20;
const clipOf = (r) => { const x = Math.max(0, Math.floor(r.x - MARGIN)); const y = Math.max(0, Math.floor(r.y - MARGIN)); return { x, y, width: Math.min(W - x, Math.ceil(r.w + MARGIN * 2)), height: Math.min(H - y, Math.ceil(r.h + MARGIN * 2)) }; };

async function away(page) {
  await page.evaluate(() => { const a = document.activeElement; if (a && a !== document.body) a.blur(); });
  await page.mouse.move(W - 4, H - 4);
  await page.waitForTimeout(450);
}
// 쉼 자리 — 'row' 면 같은 줄의 왼쪽(썸네일)에 커서를 둔다
async function toRest(page, T) {
  await away(page);
  if (T.rest === 'row') {
    const p = await page.evaluate(() => { const row = document.querySelector('[data-pbh]').closest('.ml-addrow'); const b = row.getBoundingClientRect(); return [b.x + 12, b.y + b.height / 2]; });
    await page.mouse.move(p[0], p[1]);
    await page.waitForTimeout(450);
  }
}
async function hover(page) { const r = await rectNow(page); await page.mouse.move(r.x + r.w / 2, r.y + r.h / 2); await page.waitForTimeout(450); }
// 키보드로 초점 — 앞 요소에 초점을 두고 Tab(그래야 :focus-visible 이 선다)
async function keyFocus(page) {
  await page.evaluate(() => {
    const el = document.querySelector('[data-pbh]');
    const all = [...document.querySelectorAll('button, [href], input, select, [tabindex]:not([tabindex="-1"])')].filter((e) => e.getClientRects().length && !e.disabled);
    (all[all.indexOf(el) - 1] || document.body).focus();
  });
  await page.keyboard.press('Tab');
  const on = await page.evaluate(() => document.activeElement === document.querySelector('[data-pbh]'));
  if (!on) await page.evaluate(() => document.querySelector('[data-pbh]').focus());
  await page.waitForTimeout(450);
}

// 단추와 ::before/::after 의 계산된 모양
const look = (page) => page.evaluate(() => {
  const el = document.querySelector('[data-pbh]');
  const pick = (cs) => ({ filter: cs.filter, shadow: cs.boxShadow, transform: cs.transform, translate: cs.translate, scale: cs.scale, opacity: cs.opacity, content: cs.content, display: cs.display });
  const cs = getComputedStyle(el);
  const er = el.getBoundingClientRect();
  return {
    el: pick(cs), before: pick(getComputedStyle(el, '::before')), after: pick(getComputedStyle(el, '::after')),
    rect: [er.x, er.y, er.width, er.height], color: cs.color, bg: cs.backgroundColor, border: cs.borderTopColor, borderW: parseFloat(cs.borderTopWidth),
    outline: { style: cs.outlineStyle, width: parseFloat(cs.outlineWidth), color: cs.outlineColor, offset: parseFloat(cs.outlineOffset) || 0 },
    focusVisible: el.matches(':focus-visible'),
    keyframes: el.getAnimations({ subtree: true }).filter((a) => a.playState === 'running' && a.constructor.name === 'CSSAnimation').map((a) => a.animationName),
    transitions: el.getAnimations({ subtree: true }).filter((a) => a.playState === 'running' && a.constructor.name === 'CSSTransition' && (a.effect?.getTiming().duration || 0) > 1).map((a) => `${a.effect?.pseudoElement || ''}${a.transitionProperty}`),
  };
});

// 색 문자열 → [r, g, b, a] (0~255, a 0~1). color-mix 결과(Chromium 은 color(srgb …))도 캔버스로 바꾼다.
const rgba = (page, list) => page.evaluate((cols) => {
  const c = document.createElement('canvas'); c.width = c.height = 1; const g = c.getContext('2d', { willReadFrequently: true });
  return cols.map((col) => { g.clearRect(0, 0, 1, 1); g.fillStyle = '#000'; g.fillStyle = col; g.fillRect(0, 0, 1, 1); const d = g.getImageData(0, 0, 1, 1).data; return [d[0], d[1], d[2], d[3] / 255]; });
}, list);

// box-shadow 목록 → [{ inset, blur, color, raw }]
function shadows(str) {
  if (!str || str === 'none') return [];
  return str.split(/,(?![^(]*\))/).map((p) => {
    const raw = p.trim();
    const color = (raw.match(/(?:rgba?|color|oklch|oklab|lab|lch|hsla?)\([^)]*\)|#[0-9a-f]{3,8}\b/i) || ['rgba(0, 0, 0, 0)'])[0];
    const lens = raw.replace(color, '').match(/-?[\d.]+px/g) || [];
    return { inset: /\binset\b/.test(raw), blur: lens.length >= 3 ? parseFloat(lens[2]) : 0, color, raw };
  });
}
// 흐림 3px 넘는 바깥 그림자 중 문제 있는 것 — 채도 있는 색(브랜드 글로우)이거나 쉴 때 없던 것
async function badGlows(page, L, rest) {
  const out = [];
  for (const part of ['el', 'before', 'after']) {
    const f = L[part].filter || 'none';
    if (/blur\(|drop-shadow\(/.test(f)) out.push(`${part} filter ${f}`);
    const restOuter = new Set(shadows(rest ? rest[part].shadow : '').filter((s) => !s.inset && s.blur > 3).map((s) => s.raw));
    const list = shadows(L[part].shadow).filter((s) => !s.inset && s.blur > 3);
    const cols = await rgba(page, list.map((s) => s.color));
    list.forEach((s, i) => {
      const [r, g, b, a] = cols[i];
      if (a < 0.03) return;
      if (Math.max(r, g, b) - Math.min(r, g, b) > 28) out.push(`${part} 색 있는 번짐 ${s.raw}`);
      else if (rest && !restOuter.has(s.raw)) out.push(`${part} 새 번짐 ${s.raw}`);
    });
  }
  return out;
}

const shot = async (page, clip, name) => {
  const buf = await page.screenshot({ clip });
  if (SHOTS) fs.writeFileSync(path.join(SHOTS, `${name}.png`), buf);
  return buf;
};
// 두 PNG 비교 — inner: 단추 안쪽(가장자리 3px 안) 평균 차이, ringPct: 단추 3px 바깥에서 바뀐 픽셀 %, boxPct: 단추와 그 2px 둘레에서 바뀐 픽셀 %
async function compare(a, b, r, clip) {
  return calc.evaluate(async ({ a, b, r, clip }) => {
    const load = async (b64) => { const img = new Image(); img.src = `data:image/png;base64,${b64}`; await img.decode(); const c = document.createElement('canvas'); c.width = img.width; c.height = img.height; const g = c.getContext('2d'); g.drawImage(img, 0, 0); return g.getImageData(0, 0, c.width, c.height); };
    const A = await load(a); const B = await load(b);
    const w = Math.min(A.width, B.width); const h = Math.min(A.height, B.height);
    const s = A.width / clip.width;
    const ox = (r.x - clip.x) * s, oy = (r.y - clip.y) * s, ow = r.w * s, oh = r.h * s;
    let ring = 0, changed = 0, inner = 0, innerSum = 0, box = 0, boxChanged = 0;
    for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4;
      const d = Math.max(Math.abs(A.data[i] - B.data[i]), Math.abs(A.data[i + 1] - B.data[i + 1]), Math.abs(A.data[i + 2] - B.data[i + 2]));
      const inside3 = x >= ox + 3 * s && x < ox + ow - 3 * s && y >= oy + 3 * s && y < oy + oh - 3 * s;
      const outside3 = x < ox - 3 * s || x >= ox + ow + 3 * s || y < oy - 3 * s || y >= oy + oh + 3 * s;
      const inBox2 = x >= ox - 2 * s && x < ox + ow + 2 * s && y >= oy - 2 * s && y < oy + oh + 2 * s;
      if (inside3) { inner++; innerSum += d; }
      if (outside3) { ring++; if (d > 6) changed++; }
      if (inBox2) { box++; if (d > 24) boxChanged++; }
    }
    return { inner: inner ? innerSum / inner : 0, ringPct: ring ? 100 * changed / ring : 0, boxPct: box ? 100 * boxChanged / box : 0 };
  }, { a: a.toString('base64'), b: b.toString('base64'), r, clip });
}
// 글자색과 단추 안쪽 바탕(가장자리 근처 네 점)의 최저 대비
async function contrast(png, r, clip, color) {
  return calc.evaluate(async ({ png, r, clip, color }) => {
    const img = new Image(); img.src = `data:image/png;base64,${png}`; await img.decode();
    const c = document.createElement('canvas'); c.width = img.width; c.height = img.height; const g = c.getContext('2d'); g.drawImage(img, 0, 0);
    const s = img.width / clip.width;
    const at = (x, y) => [...g.getImageData(Math.round((x - clip.x) * s), Math.round((y - clip.y) * s), 1, 1).data.slice(0, 3)];
    const lum = ([R, G, B]) => { const f = (v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; }; return 0.2126 * f(R) + 0.7152 * f(G) + 0.0722 * f(B); };
    const ratio = (p, q) => { const [x, y] = [lum(p), lum(q)].sort((m, n) => n - m); return (x + 0.05) / (y + 0.05); };
    const one = document.createElement('canvas'); one.width = one.height = 1; const g1 = one.getContext('2d');
    g1.fillStyle = color; g1.fillRect(0, 0, 1, 1);
    const px = g1.getImageData(0, 0, 1, 1).data; const fg = [px[0], px[1], px[2]]; const alpha = px[3] / 255;
    const pts = [[r.x + 5, r.y + r.h / 2], [r.x + r.w - 5, r.y + r.h / 2], [r.x + r.w / 2, r.y + 4], [r.x + r.w / 2, r.y + r.h - 4]];
    return Math.min(...pts.map(([x, y]) => { const bg = at(x, y); return ratio(fg.map((v, i) => v * alpha + bg[i] * (1 - alpha)), bg); }));
  }, { png: png.toString('base64'), r, clip, color });
}
// 초점 테두리색과 그 자리 뒤 바탕(초점 전 화면)의 대비 — 테두리 위·아래 가운데 두 점 중 낮은 쪽
async function outlineContrast(restPng, r, clip, L) {
  return calc.evaluate(async ({ png, r, clip, o }) => {
    const img = new Image(); img.src = `data:image/png;base64,${png}`; await img.decode();
    const c = document.createElement('canvas'); c.width = img.width; c.height = img.height; const g = c.getContext('2d'); g.drawImage(img, 0, 0);
    const s = img.width / clip.width;
    const at = (x, y) => [...g.getImageData(Math.round((x - clip.x) * s), Math.round((y - clip.y) * s), 1, 1).data.slice(0, 3)];
    const lum = ([R, G, B]) => { const f = (v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; }; return 0.2126 * f(R) + 0.7152 * f(G) + 0.0722 * f(B); };
    const ratio = (p, q) => { const [x, y] = [lum(p), lum(q)].sort((m, n) => n - m); return (x + 0.05) / (y + 0.05); };
    const one = document.createElement('canvas'); one.width = one.height = 1; const g1 = one.getContext('2d');
    g1.fillStyle = o.color; g1.fillRect(0, 0, 1, 1);
    const px = g1.getImageData(0, 0, 1, 1).data;
    const d = o.offset + o.width / 2;   // 테두리 굵기의 가운데
    return Math.min(...[[r.x + r.w / 2, r.y - d], [r.x + r.w / 2, r.y + r.h + d]].map(([x, y]) => { const bg = at(x, y); const a = px[3] / 255; return ratio([0, 1, 2].map((i) => px[i] * a + bg[i] * (1 - a)), bg); }));
  }, { png: restPng.toString('base64'), r, clip, o: L.outline });
}

const fails = { PB01: [], PB02: [], PB03: [], PB04: [], PB05: [], PB06: [], PB07: [], PB08: [] };

for (const theme of ['dark', 'light']) {
  for (const T of TARGETS) {
    const tag = `${T.key}/${theme}`;
    const { ctx, page, errors } = await open(theme, T);
    try {
      await mark(page, T.sel);
      await toRest(page, T);
      const r0 = await rectNow(page);
      const clip = clipOf(r0);
      const restLook = await look(page);
      const rest = await shot(page, clip, `${T.key}-${theme}-rest`);

      // 쉼 — 채움 단추는 쉴 때도 단추 밖에 번짐이 없어야 한다(단추를 감춘 화면과 비교)
      if (FILLED_REST.has(T.kind)) {
        await page.evaluate(() => { document.querySelector('[data-pbh]').style.visibility = 'hidden'; });
        await page.waitForTimeout(60);
        const gone = await shot(page, clip, `${T.key}-${theme}-hidden`);
        await page.evaluate(() => { document.querySelector('[data-pbh]').style.visibility = ''; });
        await page.waitForTimeout(60);
        const d = await compare(gone, rest, r0, clip);
        if (d.ringPct > 3) fails.PB03.push(`${tag} 쉼: 바깥 고리 ${d.ringPct.toFixed(1)}% 가 단추 그림자로 물듦`);
      }
      const rc = await contrast(rest, r0, clip, restLook.color);
      if (FILLED_REST.has(T.kind) && rc < 4.5) fails.PB01.push(`${tag} rest: ${rc.toFixed(2)}`);
      // 쉼에는 비교할 앞 상태가 없다 — 색 있는 번짐만 본다('새 메시지'의 무채색 높이 그림자는 그대로 둔다)
      for (const g of await badGlows(page, restLook, null)) fails.PB02.push(`${tag} rest ${g}`);

      // 호버
      await hover(page);
      const hoverLook = await look(page);
      const hov = await shot(page, clip, `${T.key}-${theme}-hover`);
      const hc = await compare(rest, hov, r0, clip);
      const hcon = await contrast(hov, r0, clip, hoverLook.color);
      if (hcon < 4.5) fails.PB01.push(`${tag} hover: ${hcon.toFixed(2)}`);
      for (const g of await badGlows(page, hoverLook, restLook)) fails.PB02.push(`${tag} hover ${g}`);
      if (hc.ringPct > 3) fails.PB03.push(`${tag} hover: 바깥 고리 ${hc.ringPct.toFixed(1)}% 변함`);
      if (FILLED_REST.has(T.kind)) {
        if (hoverLook.bg !== restLook.bg && T.kind !== 'pill') fails.PB05.push(`${tag} hover 바탕 ${restLook.bg} → ${hoverLook.bg}`);
        if (hoverLook.border !== restLook.border) fails.PB05.push(`${tag} hover 테두리 ${restLook.border} → ${hoverLook.border}`);
      }

      // 누름 — 단추 밖에서 떼어 click 은 일으키지 않는다
      // 눌린 단추는 작아진다 — 대비는 지금(줄어든) 자리의 안쪽에서 잰다
      await page.mouse.down(); await page.waitForTimeout(220);
      const actLook = await look(page);
      const act = await shot(page, clip, `${T.key}-${theme}-active`);
      const ra = await rectNow(page);
      if (process.env.CVIEW_UI_DEBUG) console.log(tag, 'press', (ra.w / r0.w).toFixed(3), actLook.el.transform, actLook.el.scale);
      const acon = await contrast(act, ra, clip, actLook.color);
      if (acon < 4.5) fails.PB01.push(`${tag} active: ${acon.toFixed(2)}`);
      for (const g of await badGlows(page, actLook, restLook)) fails.PB02.push(`${tag} active ${g}`);
      await page.mouse.move(W - 4, H - 4); await page.mouse.up();
      await toRest(page, T);

      // 키보드 초점
      await keyFocus(page);
      const focusLook = await look(page);
      const foc = await shot(page, clip, `${T.key}-${theme}-focus`);
      const fc = await compare(rest, foc, r0, clip);
      if (!focusLook.focusVisible) fails.PB04.push(`${tag}: :focus-visible 아님`);
      if (!(focusLook.outline.style !== 'none' && focusLook.outline.width >= 2)) fails.PB04.push(`${tag}: 초점 테두리 ${focusLook.outline.style} ${focusLook.outline.width}px`);
      else {
        const oc = await outlineContrast(rest, r0, clip, focusLook);
        if (oc < 3) fails.PB04.push(`${tag}: 초점 테두리 대비 ${oc.toFixed(2)} (${focusLook.outline.color})`);
      }
      if (LIT.has(T.kind) && fc.inner < Math.min(2, hc.inner * 0.5)) fails.PB04.push(`${tag}: 초점 안쪽 변화 ${fc.inner.toFixed(2)} < 호버 ${hc.inner.toFixed(2)}의 절반`);
      const fcon = await contrast(foc, r0, clip, focusLook.color);
      if (fcon < 4.5) fails.PB01.push(`${tag} focus: ${fcon.toFixed(2)}`);
      for (const g of await badGlows(page, focusLook, restLook)) fails.PB02.push(`${tag} focus ${g}`);
      if (FILLED_REST.has(T.kind) && T.kind !== 'pill' && focusLook.bg !== restLook.bg) fails.PB05.push(`${tag} focus 바탕 ${restLook.bg} → ${focusLook.bg}`);
      await toRest(page, T);

      // aria-disabled — 글자 단추 하나로(홈 로그인). 호버·누름에 안쪽·바깥 모두 그대로여야 한다
      if (T.key === 'home-login') {
        await page.evaluate(() => document.querySelector('[data-pbh]').setAttribute('aria-disabled', 'true'));
        await toRest(page, T);
        const drest = await shot(page, clip, `${T.key}-${theme}-ariadisabled-rest`);
        await hover(page);
        const dh = await compare(drest, await shot(page, clip, `${T.key}-${theme}-ariadisabled-hover`), r0, clip);
        if (dh.inner > 1.5 || dh.ringPct > 1) fails.PB05.push(`${tag} aria-disabled 인데 호버로 바뀜(안 ${dh.inner.toFixed(2)}, 바깥 ${dh.ringPct.toFixed(1)}%)`);
        await page.mouse.down(); await page.waitForTimeout(220);
        const dp = await rectNow(page);
        if (Math.abs(dp.w - r0.w) > 0.5) fails.PB05.push(`${tag} aria-disabled 인데 눌림(폭 ${r0.w.toFixed(1)} → ${dp.w.toFixed(1)})`);
        await page.mouse.move(W - 4, H - 4); await page.mouse.up();
        await page.evaluate(() => document.querySelector('[data-pbh]').removeAttribute('aria-disabled'));
        await toRest(page, T);
      }

      // 모션 줄이기 — 시스템 설정, 앱 설정(data-motion="reduced")
      for (const mode of ['media', 'app']) {
        if (mode === 'media') await page.emulateMedia({ reducedMotion: 'reduce' });
        else await page.evaluate(() => { document.documentElement.dataset.motion = 'reduced'; });
        await toRest(page, T);
        const a = await look(page);
        const r = await rectNow(page);
        await page.mouse.move(r.x + r.w / 2, r.y + r.h / 2);
        await page.waitForTimeout(40);
        const early = await look(page);
        await page.waitForTimeout(300);
        const b = await look(page);
        await page.mouse.down(); await page.waitForTimeout(40);
        const p = await look(page);
        await page.mouse.move(W - 4, H - 4); await page.mouse.up();
        const kf = [...early.keyframes, ...b.keyframes, ...p.keyframes];
        if (kf.length) fails.PB06.push(`${tag} ${mode}: 키프레임 ${kf.join(',')}`);
        const tr = [...early.transitions, ...p.transitions];
        if (tr.length) fails.PB06.push(`${tag} ${mode}: 전환 ${[...new Set(tr)].join(',')}`);
        const moved = (u, v) => u.some((x, i) => Math.abs(x - v[i]) > 0.25);
        if (moved(a.rect, b.rect)) fails.PB06.push(`${tag} ${mode}: 호버에 움직임 ${a.rect.map((v) => v.toFixed(1))} → ${b.rect.map((v) => v.toFixed(1))}`);
        if (moved(a.rect, p.rect)) fails.PB06.push(`${tag} ${mode}: 누름에 크기 변화 ${a.rect.map((v) => v.toFixed(1))} → ${p.rect.map((v) => v.toFixed(1))}`);
        const k = ['transform', 'translate', 'scale'];
        if (a.before.content !== 'none' && k.some((q) => a.before[q] !== b.before[q])) fails.PB06.push(`${tag} ${mode}: ::before 가 움직임`);
        if (mode === 'media') await page.emulateMedia({ reducedMotion: null });
        else await page.evaluate(() => { delete document.documentElement.dataset.motion; });
      }

      // 고대비(Windows 고대비 테마 — WebView2 forced-colors). WebKit 은 흉내 내지 못한다.
      if (!WEBKIT) {
        await page.emulateMedia({ forcedColors: 'active' });
        await toRest(page, T);
        const fr = await rectNow(page);
        const fclip = clipOf(fr);
        const frest = await shot(page, fclip, `${T.key}-${theme}-forced-rest`);
        await hover(page);
        const fl = await look(page);
        const fhov = await shot(page, fclip, `${T.key}-${theme}-forced-hover`);
        const fd = await compare(frest, fhov, fr, fclip);
        const drawn = (x) => x.content !== 'none' && x.content !== 'normal' && x.display !== 'none' && Number(x.opacity) > 0;
        if (drawn(fl.before)) fails.PB07.push(`${tag}: 고대비에서 ::before 빛 층이 그려짐`);
        if (shadows(fl.el.shadow).some((s) => !s.inset)) fails.PB07.push(`${tag}: 고대비에서 바깥 그림자 ${fl.el.shadow}`);
        if (fd.boxPct < 2) fails.PB07.push(`${tag}: 고대비에서 호버가 보이지 않음(바뀐 픽셀 ${fd.boxPct.toFixed(1)}%)`);
        await page.emulateMedia({ forcedColors: 'none' });
      }
      if (errors.length) fails.PB01.push(`${tag}: 페이지 오류 ${errors[0]}`);
    } catch (e) {
      fails.PB01.push(`${tag}: ${e.message.split('\n')[0]}`);
    }
    await ctx.close();

    // Safari 16.0 흉내 — 따로 연다(CSS 를 바꿔 보낸다)
    const L16 = await open(theme, T, { legacy: true });
    try {
      const page = L16.page;
      await mark(page, T.sel);
      await toRest(page, T);
      const r = await rectNow(page);
      const clip = clipOf(r);
      const states = [['rest', async () => {}], ['hover', () => hover(page)], ['active', async () => { await hover(page); await page.mouse.down(); await page.waitForTimeout(220); }]];
      for (const [name, enter] of states) {
        await enter();
        const L = await look(page);
        const png = await shot(page, clip, `${T.key}-${theme}-s16-${name}`);
        const c = await contrast(png, await rectNow(page), clip, L.color);
        if ((name !== 'rest' || FILLED_REST.has(T.kind)) && c < 4.5) fails.PB08.push(`${tag} ${name}: 대비 ${c.toFixed(2)}`);
        if (L.borderW > 0) {
          const [bc, fg] = await rgba(page, [L.border, L.color]);
          if (bc[3] > 0.05 && bc.slice(0, 3).every((v, i) => Math.abs(v - fg[i]) < 3)) fails.PB08.push(`${tag} ${name}: 테두리가 글자색(${L.border})`);
        }
        if (name === 'rest' && (T.kind === 'primary' || T.kind === 'pill') && !shadows(L.el.shadow).some((s) => s.inset)) fails.PB08.push(`${tag} rest: 윗빛·턱이 없음(${L.el.shadow})`);
        if (name === 'active') { await page.mouse.move(W - 4, H - 4); await page.mouse.up(); }
        await toRest(page, T);
      }
      if (L16.errors.length) fails.PB08.push(`${tag}: 페이지 오류 ${L16.errors[0]}`);
    } catch (e) {
      fails.PB08.push(`${tag}: ${e.message.split('\n')[0]}`);
    }
    await L16.ctx.close();
  }
}

const NAMES = {
  PB01: '쉼·호버·초점·누름 글자 대비 4.5:1', PB02: '번지는 글로우 없음(blur·색 있는 그림자·새로 생기는 번짐)', PB03: '단추 바깥으로 번지는 픽셀 없음(쉼·호버)',
  PB04: '키보드 초점 = 3:1 테두리 + 호버만큼의 켜짐', PB05: '채움 단추는 호버·초점에도 바탕·테두리 그대로, aria-disabled 는 반응 없음', PB06: '모션 줄이기에서 움직임·전환·키프레임 없음',
  PB07: '고대비에서 빛 층·그림자 없이 호버가 보임', PB08: 'color-mix 없는 엔진(Safari 16.0)에서 대비·테두리·키 모양 유지',
};
for (const [id, list] of Object.entries(fails)) await check(`${id} ${NAMES[id]}`, async () => assert.deepEqual(list, []));

await browser.close();
server.close();
for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : `\n     ${r.error.split('\n').filter((l) => /^[+-] /.test(l.trim()) || /^\s+'/.test(l)).slice(0, 16).join('\n     ') || r.error.slice(0, 600)}`}`);
const failed = results.filter((r) => !r.ok).length;
console.log(`\n${results.length - failed}/${results.length} passed (${WEBKIT ? 'webkit' : 'chromium'})`);
process.exit(failed ? 1 : 0);
