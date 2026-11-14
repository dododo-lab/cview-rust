// 위험 글자 대비 회귀 검사 — 모의 IPC·헤드리스 브라우저(실제 로그인·채팅 전송·차단·팔로우 끊기·후원 없음).
// 라이트 위험 글자색 --error(#e1352a)는 흰 메뉴 바탕과 4.43:1 로 4.5:1 에 못 미쳤다(줄 메뉴 '사용자 차단'·계정 메뉴 '로그아웃'이 쉴 때).
// 다크(#ff5a4f)도 호버 바탕 위에서 모자랐다 — 말풍선 '채팅 숨기기(차단)' 4.06·멀티라이브 '모두 닫기' 4.13·'언팔로우'(겹친 면) 4.47.
// DT01 토큰 — --error 와 글자가 놓일 수 있는 면 토큰(메뉴·기본·높인·겹친 면·페이지·덱·사이드바) 모두 4.5:1 이상.
//      다크·라이트(앱 설정)·라이트(시스템 설정) 세 경우 — 라이트 값은 두 블록에 따로 있어 한쪽만 고치면 다른 쪽이 남는다.
// DT02 화면 — 대표 위험 항목의 글자가 실제 바탕과 4.5:1, 아이콘·테두리는 3:1 이상. 쉼·호버·키보드 초점.
//      바탕은 조상 바탕을 겹친 계산값과 그 항목 화면 픽셀의 최빈색 둘 다로 재고 낮은 쪽을 쓴다.
//      줄 메뉴 '사용자 차단' · 계정 메뉴 '로그아웃' · 사용자 말풍선 '채팅 숨기기(차단)'·'언팔로우' · 멀티라이브 '모두 닫기' ·
//      설정 '초기화…'(.btn.danger) · 두 번 누르기 무장('한 번 더 누르면 …') · 채팅 입력 경고 배너 · 채팅 '연결 실패' · 플레이어 오류
// DT03 위험 채움 — 호버 채움(--danger-fill) 위 흰 글자(--on-danger) 4.5:1 이상이고, 호버한 줄의 바탕이 실제로 그 채움이다.
// DT04 빨강으로 읽힘 — --error·--error-on-media 가 빨강 색상각(HSL 350°~12°), 채도 60% 이상, 밝기 30~75%, R 이 G·B 의 두 배 이상
//      (대비를 맞추려 어둡게 하다 갈색·검정으로 보이지 않게).
// 저장소 루트에서: node tools/ui-regression/danger-text-contrast.mjs   (Playwright 가 필요하다.
//   CVIEW_UI_BROWSER=webkit 이면 앱(macOS)과 같은 WebKit 엔진으로 돈다. CVIEW_UI_SHOTS=<폴더> 면 항목 둘레 PNG 를 남긴다.
//   CVIEW_DT_REPORT=1 이면 잰 대비를 모두 찍는다.)
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { server, base, mockTauri, go } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const WEBKIT = process.env.CVIEW_UI_BROWSER === 'webkit';
const engine = WEBKIT ? pw.webkit : pw.chromium;
const SHOTS = process.env.CVIEW_UI_SHOTS;
if (SHOTS) fs.mkdirSync(SHOTS, { recursive: true });
const REPORT = process.env.CVIEW_DT_REPORT ? [] : null;
const W = 1280, H = 820, DPR = 2;
const TEXT = 4.5, NONTEXT = 3;
const fails = { DT01: [], DT02: [], DT03: [], DT04: [] };

const browser = await engine.launch();
const calc = await (await browser.newContext()).newPage(); // 픽셀 계산용 빈 페이지

// ── 색 도구(페이지와 Node 양쪽) ─────────────────────────────────────────────────
const lum = ([R, G, B]) => { const f = (v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; }; return 0.2126 * f(R) + 0.7152 * f(G) + 0.0722 * f(B); };
const ratio = (p, q) => { const [x, y] = [lum(p), lum(q)].sort((m, n) => n - m); return (x + 0.05) / (y + 0.05); };
const over = (top, under) => [0, 1, 2].map((i) => top[i] * top[3] + under[i] * (1 - top[3]));
const hex = (c) => `#${c.slice(0, 3).map((v) => Math.round(v).toString(16).padStart(2, '0')).join('')}`;

// 페이지에 심는다 — 계산 스타일 색(rgb()·color(srgb …)·#hex)을 [r, g, b, a] 로 읽고, 조상 바탕을 겹쳐 보이는 바탕을 낸다.
function colorKit() {
  const parse = (s) => {
    s = String(s).trim();
    if (!s || s === 'transparent') return [0, 0, 0, 0];
    let m = s.match(/^rgba?\(([^)]+)\)$/);
    if (m) { const p = m[1].split(/[\s,/]+/).filter(Boolean).map(Number); return [p[0], p[1], p[2], p.length > 3 ? p[3] : 1]; }
    m = s.match(/^color\(srgb\s+([^)]+)\)$/);
    if (m) { const p = m[1].split(/[\s/]+/).filter(Boolean).map(Number); return [p[0] * 255, p[1] * 255, p[2] * 255, p.length > 3 ? p[3] : 1]; }
    const probe = document.createElement('i');
    document.body.append(probe);
    probe.style.color = s;
    const c = getComputedStyle(probe).color;
    probe.remove();
    if (!c || c === s) throw new Error(`색을 읽지 못했다: ${s}`);
    return parse(c);
  };
  const over = (top, under) => [0, 1, 2].map((i) => top[i] * top[3] + under[i] * (1 - top[3]));
  // 조상 바탕을 불투명한 층까지 모아 아래부터 겹친다(아무도 불투명하지 않으면 흰 캔버스). 바탕 그림(그라데이션)은 표시만 한다.
  const backOf = (el) => {
    const layers = [];
    let image = '';
    for (let e = el; e; e = e.parentElement) {
      const cs = getComputedStyle(e);
      if (cs.backgroundImage && cs.backgroundImage !== 'none' && !image) image = e.className || e.tagName;
      const c = parse(cs.backgroundColor);
      if (c[3] > 0) layers.push(c);
      if (c[3] >= 1) break;
    }
    let acc = [255, 255, 255];
    for (let i = layers.length - 1; i >= 0; i -= 1) acc = over(layers[i], acc);
    return { rgb: acc, image };
  };
  const token = (name, el = document.documentElement) => getComputedStyle(el).getPropertyValue(name).trim();
  window.__dtKit = { parse, over, backOf, token };
}

// 항목 둘레를 찍어(테두리 상자 + 여백) 안쪽 최빈색을 바탕으로 읽는다 — 글자 획은 바탕보다 적은 픽셀을 덮는다.
async function pixelBack(page, sel, inset, name) {
  const b = await page.locator(sel).first().boundingBox();
  if (!b) throw new Error(`${sel} 상자를 찾지 못했다`);
  const pad = 10;
  const clip = { x: Math.max(0, Math.floor(b.x) - pad), y: Math.max(0, Math.floor(b.y) - pad), width: Math.ceil(b.width) + pad * 2, height: Math.ceil(b.height) + pad * 2 };
  const buf = await page.screenshot({ clip, animations: 'disabled', caret: 'hide' });
  if (SHOTS && name) fs.writeFileSync(path.join(SHOTS, `${name}.png`), buf);
  const box = { x: (b.x - clip.x + inset) * DPR, y: (b.y - clip.y + inset) * DPR, w: (b.width - inset * 2) * DPR, h: (b.height - inset * 2) * DPR };
  return calc.evaluate(async ({ b64, box }) => {
    const img = new Image();
    img.src = `data:image/png;base64,${b64}`;
    await img.decode();
    const c = document.createElement('canvas');
    c.width = img.width; c.height = img.height;
    const x = c.getContext('2d');
    x.drawImage(img, 0, 0);
    const D = x.getImageData(0, 0, c.width, c.height).data;
    const counts = new Map();
    for (let Y = Math.ceil(box.y); Y < Math.floor(box.y + box.h); Y += 1) {
      for (let X = Math.ceil(box.x); X < Math.floor(box.x + box.w); X += 1) {
        const i = (Y * c.width + X) * 4;
        const k = (D[i] << 16) | (D[i + 1] << 8) | D[i + 2];
        counts.set(k, (counts.get(k) || 0) + 1);
      }
    }
    let best = 0, key = 0;
    for (const [k, n] of counts) if (n > best) { best = n; key = k; }
    return [(key >> 16) & 255, (key >> 8) & 255, key & 255];
  }, { b64: buf.toString('base64'), box });
}

// 한 항목을 잰다 — text: 글자를 읽을 자식(없으면 자신) · icon: 아이콘 자식 · border: 자기 색으로 그린 테두리(currentColor)
// inset: 픽셀 바탕을 읽을 때 테두리 상자 안쪽으로 뺄 CSS 픽셀(초점 고리·테두리를 피한다)
// scope: CVIEW_UI_SHOTS 일 때 함께 찍어 둘 둘레(메뉴 전체 등) — `<name>-scope.png`
async function measure(page, tag, sel, { text = null, icon = null, border = false, inset = 2, name = null, scope = null } = {}) {
  if (SHOTS && name && scope) {
    const b = await page.locator(scope).first().boundingBox();
    const clip = { x: Math.max(0, Math.floor(b.x) - 8), y: Math.max(0, Math.floor(b.y) - 8), width: Math.ceil(b.width) + 16, height: Math.ceil(b.height) + 16 };
    fs.writeFileSync(path.join(SHOTS, `${name}-scope.png`), await page.screenshot({ clip, animations: 'disabled', caret: 'hide' }));
  }
  const info = await page.locator(sel).first().evaluate((el, { text, icon }) => {
    const K = window.__dtKit;
    const t = text ? el.querySelector(text) : el;
    const ic = icon ? el.querySelector(icon) : null;
    if (!t) throw new Error(`글자 요소 ${text} 가 없다`);
    const back = K.backOf(t);
    return { fg: K.parse(getComputedStyle(t).color), icon: ic ? K.parse(getComputedStyle(ic).color) : null, back: back.rgb, image: back.image, own: K.parse(getComputedStyle(el).color) };
  }, { text, icon });
  const pix = await pixelBack(page, sel, inset, name);
  const out = [];
  const add = (kind, fg, min) => {
    const r1 = ratio(over(fg, info.back), info.back);
    const r2 = ratio(over(fg, pix), pix);
    const r = Math.min(r1, r2);
    const far = info.back.some((v, i) => Math.abs(v - pix[i]) > 10);
    const line = `${tag} ${kind}: ${r.toFixed(2)}:1 (글자 ${hex(fg)} · 바탕 계산 ${hex(info.back)} ${r1.toFixed(2)} / 픽셀 ${hex(pix)} ${r2.toFixed(2)})${far ? ' — 계산·픽셀 바탕이 다름' : ''}${info.image ? ` — 조상 바탕 그림(${info.image})` : ''}`;
    REPORT?.push(`DT02 ${line}`);
    if (r < min) fails.DT02.push(`${line} — ${min}:1 미만`);
    out.push({ kind, r, fg, back: pix });
  };
  add('글자', info.fg, TEXT);
  if (info.icon) add('아이콘', info.icon, NONTEXT);
  if (border) add('테두리', info.own, NONTEXT);
  return { pix, info, out };
}

// 호버 채움 — 바탕이 --danger-fill 이고 글자가 그 위에서 4.5:1 이상
async function measureFill(page, tag, sel, opts = {}) {
  const m = await measure(page, tag, sel, opts);
  const fill = await page.evaluate(() => window.__dtKit.parse(window.__dtKit.token('--danger-fill')));
  if (!m.pix.every((v, i) => Math.abs(v - fill[i]) <= 3)) fails.DT03.push(`${tag}: 호버 바탕 ${hex(m.pix)} — 위험 채움(--danger-fill ${hex(fill)})이 아님`);
  const text = m.out.find((o) => o.kind === '글자');
  if (text && text.r < TEXT) fails.DT03.push(`${tag}: 채움 위 글자 ${text.r.toFixed(2)}:1`);
}

const settle = (page, ms = 420) => page.waitForTimeout(ms); // 바탕·글자 전환(.16~.18s)·메뉴 등장(mo-pop)이 끝나게
const away = (page) => page.mouse.move(W - 4, H - 4);
async function hoverOn(page, sel) {
  const b = await page.locator(sel).first().boundingBox();
  await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2);
  await settle(page);
}

// 모의 IPC 보탬 — 채팅 연결·말풍선 카드(팔로잉 중인 채널이 있는 사용자)·최근 검색어·멀티라이브 두 패인('모두 닫기'는 패인이 있어야 보인다).
// 시청 화면(viewer)은 재생 시작(player_start)을 실패로 돌려 플레이어 오류 막을 세운다.
function extraMock(scene) {
  const inv = window.__TAURI__.core.invoke;
  const sess = (i) => ({ id: `s${i}`, slot: i, channelId: `ch${i}`, channelName: ['한동숙', '침착맨'][i], liveTitle: '오늘의 방송', viewerCountText: '1,234', openDate: new Date(Date.now() - 3600e3).toISOString(), isMuted: i > 0, isAudible: i === 0, state: { kind: 'playing' }, chatChannelId: null });
  const ml = { sessions: [sess(0), sess(1)], selectedId: 's0', canAdd: true, maxSessions: 4, layout: 'grid', rev: 1 };
  const more = {
    chat_connect: () => ({ canSend: true }), chat_disconnect: () => null, chat_avatars: (a) => (a.userIds || []).map(() => null), chat_donation_ranking: () => [],
    chat_user_card: (a) => ({ userId: a.userId, nickname: '사과', channel: { channelId: 'cu1', channelName: '사과', followerCount: 1200, openLive: false }, following: true }),
    multilive_state: () => JSON.parse(JSON.stringify(ml)), multilive_select: () => JSON.parse(JSON.stringify(ml)), multilive_stage_size: () => null,
    equalizer_presets: () => [[31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000], [{ name: '평탄', gains: new Array(10).fill(0), preAmp: 0 }]],
    channel_info: () => ({ verified: false, followerCount: 1000 }), live_detail: () => ({ liveTitle: '오늘의 방송', categoryName: '토크' }),
  };
  if (scene === 'viewer') {
    for (const k of ['multilive_state', 'multilive_select', 'multilive_stage_size', 'equalizer_presets', 'channel_info', 'live_detail']) delete more[k];
    more.player_start = () => { throw new Error('재생 시험 오류'); };
  }
  window.__TAURI__.core.invoke = async (c, a) => (more[c] ? more[c](a || {}) : inv(c, a));
  try {
    localStorage.setItem('cview.welcomed.v1', String(Date.now()));
    localStorage.setItem('cview.recentSearches', JSON.stringify(['마인크래프트', '침착맨']));
  } catch { /* 무시 */ }
}

async function openApp(theme, { scheme = theme, url = base, scene = 'app' } = {}) {
  const ctx = await browser.newContext({ viewport: { width: W, height: H }, colorScheme: scheme, deviceScaleFactor: DPR });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, theme]);
  await page.addInitScript(extraMock, scene);
  await page.addInitScript(colorKit);
  await page.goto(url);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
  await page.waitForFunction(() => { const s = document.getElementById('splash'); return !s || !s.isConnected || getComputedStyle(s).display === 'none' || getComputedStyle(s).opacity === '0'; }, null, { timeout: 8000 }).catch(() => {});
  await page.waitForTimeout(600);
  return { ctx, page, errors };
}

// ── DT01·DT04 토큰 ───────────────────────────────────────────────────────────
const SURFACES = ['--surface-popover', '--surface-base', '--surface-elevated', '--surface-overlay', '--background', '--deck', '--sidebar'];
function hsl([r, g, b]) {
  r /= 255; g /= 255; b /= 255;
  const mx = Math.max(r, g, b), mn = Math.min(r, g, b), l = (mx + mn) / 2, d = mx - mn;
  let h = 0;
  if (d) { h = mx === r ? ((g - b) / d) % 6 : mx === g ? (b - r) / d + 2 : (r - g) / d + 4; h *= 60; if (h < 0) h += 360; }
  return { h, s: d ? d / (1 - Math.abs(2 * l - 1)) : 0, l };
}
async function tokens(page, tag) {
  const t = await page.evaluate((names) => {
    const K = window.__dtKit;
    const read = (n) => { const v = K.token(n); return v ? K.parse(v) : null; };
    return { theme: document.documentElement.dataset.theme, error: read('--error'), media: read('--error-on-media'), fill: read('--danger-fill'), onFill: read('--on-danger'), surfaces: names.map((n) => [n, read(n)]) };
  }, SURFACES);
  for (const [n, s] of t.surfaces) {
    const r = ratio(t.error, s);
    REPORT?.push(`DT01 ${tag}: --error ${hex(t.error)} / ${n} ${hex(s)} ${r.toFixed(2)}:1`);
    if (r < TEXT) fails.DT01.push(`${tag}: --error ${hex(t.error)} 가 ${n}(${hex(s)}) 와 ${r.toFixed(2)}:1`);
  }
  const rf = ratio(t.onFill, t.fill);
  REPORT?.push(`DT03 ${tag}: --on-danger / --danger-fill ${rf.toFixed(2)}:1`);
  if (rf < TEXT) fails.DT03.push(`${tag}: --danger-fill ${hex(t.fill)} 위 --on-danger ${rf.toFixed(2)}:1`);
  for (const [n, c] of [['--error', t.error], ['--error-on-media', t.media]]) {
    if (!c) continue; // 예전 코드엔 --error-on-media 가 없다 — 플레이어 오류는 DT02 가 실제 화면으로 잰다
    const { h, s, l } = hsl(c);
    REPORT?.push(`DT04 ${tag}: ${n} ${hex(c)} 색상각 ${h.toFixed(1)}° 채도 ${(s * 100).toFixed(0)}% 밝기 ${(l * 100).toFixed(0)}%`);
    const red = (h >= 350 || h <= 12) && s >= 0.6 && l >= 0.3 && l <= 0.75 && c[0] >= 2 * c[1] && c[0] >= 2 * c[2];
    if (!red) fails.DT04.push(`${tag}: ${n} ${hex(c)} 가 빨강으로 읽히지 않는다(색상각 ${h.toFixed(1)}° 채도 ${(s * 100).toFixed(0)}% 밝기 ${(l * 100).toFixed(0)}%)`);
  }
  return t;
}

// ── DT02·DT03 화면 ───────────────────────────────────────────────────────────
// 채팅 패널 — 시청 화면의 패널은 이미 부속을 달았으므로 index.html 원본 마크업에서 하나 새로 만든다(menu-item-focus.mjs 와 같다).
async function chatPanel(page) {
  await page.evaluate(async () => {
    const { ChatPanel } = await import('/js/chat.js');
    const { loadChatPrefs, setMyNickname } = await import('/js/chat-prefs.js');
    await loadChatPrefs();
    setMyNickname('도도');
    const src = new DOMParser().parseFromString(await (await fetch('/index.html')).text(), 'text/html');
    const box = document.createElement('div');
    box.id = 'dtbox';
    box.style.cssText = 'position:fixed;left:420px;top:40px;width:360px;height:600px;z-index:50;display:flex;flex-direction:column';
    box.innerHTML = src.querySelector('#viewer .chat').outerHTML;
    document.body.appendChild(box);
    const root = box.querySelector('.chat');
    root.hidden = false;
    root.style.cssText = 'display:flex;flex-direction:column;height:100%;width:100%';
    const panel = new ChatPanel(root, { isLoggedIn: () => true, onHighlight: () => {} });
    await panel.connect('cc1', 'ch1');
    panel.onEvent({ type: 'connected' });
    let k = 0;
    const t = Date.now();
    const msg = (o) => ({ id: `m${++k}`, kind: 'normal', role: 'common_user', badges: [], emojis: {}, timestamp: t + k * 61000, ...o });
    panel.append([
      { userId: 'u1', nickname: '사과', content: '안녕하세요 반가워요' },
      { userId: 'u2', nickname: '바나나', content: '오늘 방송 최고' },
    ].map(msg));
    panel.flushLines();
    window.__panel = panel;
  });
  await page.waitForTimeout(300);
}

async function screens(theme) {
  const { ctx, page, errors } = await openApp(theme);
  const T = (s) => `${s} ${theme}`;
  const shot = (s) => `${theme}-${s}`;
  const step = async (label, fn) => {
    try { await fn(); } catch (e) { fails.DT02.push(`${T(label)}: ${e.message.split('\n')[0]}`); }
    await page.keyboard.press('Escape').catch(() => {});
    await away(page);
  };
  await tokens(page, theme);
  await go(page, 'home');
  await page.waitForTimeout(400);

  await step('채팅', async () => {
    await chatPanel(page);
    // 줄 메뉴 '사용자 차단' — 키보드로 연다(Shift+F10), 초점은 첫 항목에 두고 마우스는 치운다
    await page.locator('#dtbox .chat-list').focus();
    await page.keyboard.press('ArrowUp');
    await page.keyboard.press('Shift+F10');
    await page.waitForSelector('.ctx-menu .ctx-item.danger', { state: 'visible' });
    await away(page);
    await settle(page);
    await measure(page, T('줄 메뉴 사용자 차단 쉼'), '.ctx-menu .ctx-item.danger', { name: shot('ctx-danger-rest'), scope: '.ctx-menu' });
    await hoverOn(page, '.ctx-menu .ctx-item.danger');
    await measureFill(page, T('줄 메뉴 사용자 차단 호버'), '.ctx-menu .ctx-item.danger', { name: shot('ctx-danger-hover') });
    await page.keyboard.press('Escape');
    await away(page);
    // 사용자 말풍선 — 고른 줄에서 Enter. 카드(팔로잉 중)가 오면 '언팔로우' 단추가 생긴다.
    await page.locator('#dtbox .chat-list').focus();
    await page.keyboard.press('ArrowUp');
    await page.keyboard.press('Enter');
    await page.waitForSelector('.user-pop .up-item.danger', { state: 'visible' });
    await page.waitForSelector('.user-pop .up-following', { state: 'visible' });
    await away(page);
    await settle(page);
    await measure(page, T('말풍선 채팅 숨기기(차단) 쉼'), '.user-pop .up-item.danger', { name: shot('up-danger-rest') });
    await hoverOn(page, '.user-pop .up-item.danger');
    await measure(page, T('말풍선 채팅 숨기기(차단) 호버'), '.user-pop .up-item.danger', { name: shot('up-danger-hover') });
    await hoverOn(page, '.user-pop .up-following');
    if (!(await page.locator('.user-pop .up-following .up-off').isVisible())) throw new Error("팔로잉 호버에 '언팔로우'가 보이지 않는다");
    await measure(page, T('말풍선 언팔로우(팔로잉 호버)'), '.user-pop .up-following', { inset: 3, name: shot('up-unfollow-hover'), scope: '.user-pop' });
    await page.keyboard.press('Escape');
    await away(page);
    // 연결 실패 상태와 입력 경고 배너 — setState 가 배너를 지우므로 상태를 먼저 둔다
    await page.evaluate(() => { window.__panel.setState('failed'); window.__panel.updateInput('메시지를 보내지 못했습니다. 잠시 뒤 다시 보내 주세요.'); });
    await page.waitForSelector('#dtbox .chat-input-banner.warn', { state: 'visible' });
    await settle(page);
    await measure(page, T('채팅 상태 연결 실패'), '#dtbox .chat-state.bad', { inset: 0, name: shot('chat-state-bad') });
    await measure(page, T('채팅 입력 경고 배너'), '#dtbox .chat-input-banner.warn', { inset: 3, name: shot('chat-banner-warn'), scope: '#dtbox .chat-input-banner.warn' });
    await page.evaluate(() => document.getElementById('dtbox')?.remove());
  });

  await step('계정 메뉴', async () => {
    await page.locator('.account-trigger').focus();
    await page.keyboard.press('Enter');
    await page.waitForSelector('.account-pop .ap-item.danger', { state: 'visible' });
    await away(page);
    await settle(page);
    await measure(page, T('계정 메뉴 로그아웃 쉼'), '.account-pop .ap-item.danger', { text: '.ap-label', icon: '.ap-icon', name: shot('ap-danger-rest'), scope: '.account-pop' });
    await hoverOn(page, '.account-pop .ap-item.danger');
    await measureFill(page, T('계정 메뉴 로그아웃 호버'), '.account-pop .ap-item.danger', { text: '.ap-label', icon: '.ap-icon', name: shot('ap-danger-hover') });
  });

  await step('멀티라이브 더 보기', async () => {
    await go(page, 'multilive');
    await page.waitForSelector('[data-ml-more]', { state: 'visible' });
    await page.locator('[data-ml-more]').focus();
    await page.keyboard.press('Enter');   // 키보드로 열면 첫 항목에 초점
    await page.waitForSelector('.ml-more-menu .ml-menu-item.danger', { state: 'visible' });
    await away(page);
    await settle(page);
    await measure(page, T('멀티라이브 모두 닫기 쉼'), '.ml-more-menu .ml-menu-item.danger', { icon: 'svg', name: shot('ml-danger-rest') });
    await hoverOn(page, '.ml-more-menu .ml-menu-item.danger');
    await measure(page, T('멀티라이브 모두 닫기 호버'), '.ml-more-menu .ml-menu-item.danger', { icon: 'svg', name: shot('ml-danger-hover'), scope: '.ml-more-menu' });
    await away(page);
    await page.keyboard.press('End');
    await settle(page);
    if (!(await page.evaluate(() => document.activeElement?.matches('.ml-menu-item.danger:focus-visible')))) throw new Error("End 로 '모두 닫기'에 키보드 초점이 서지 않았다");
    await measure(page, T('멀티라이브 모두 닫기 초점'), '.ml-more-menu .ml-menu-item.danger', { icon: 'svg', inset: 3, name: shot('ml-danger-focus') });
  });

  await step('설정 초기화', async () => {
    await go(page, 'settings');
    await page.locator('[data-page="settings"] .settings-tabs [data-group="advanced"]').click();
    await page.waitForSelector('[data-reset-all]', { state: 'visible' });
    await page.locator('[data-reset-all]').scrollIntoViewIfNeeded();
    await away(page);
    await settle(page);
    await measure(page, T('설정 초기화… 쉼'), '[data-reset-all]', { inset: 3, name: shot('btn-danger-rest'), scope: '.setting:has([data-reset-all])' });
    await hoverOn(page, '[data-reset-all]');
    await measureFill(page, T('설정 초기화… 호버'), '[data-reset-all]', { inset: 3, name: shot('btn-danger-hover') });
  });

  await step('두 번 누르기 무장', async () => {
    const CLEAR = '[data-page="search"] [data-recent-clear]';
    await go(page, 'search');
    const input = '[data-page="search"] input[type=search]';
    await page.fill(input, '침착맨');
    await page.press(input, 'Enter');
    await page.waitForTimeout(200);
    await page.fill(input, '');
    await page.waitForSelector(CLEAR, { state: 'visible' });
    await page.click(CLEAR);
    await away(page);
    await settle(page, 300);   // 무장은 3초 — 그 안에 잰다
    if ((await page.locator(CLEAR).getAttribute('data-armed')) !== '1') throw new Error('첫 누름에 무장하지 않았다');
    await measure(page, T('두 번 누르기 무장'), CLEAR, { border: true, inset: 2, name: shot('armed'), scope: '[data-page="search"] .section-head:has([data-recent-clear])' });
  });

  if (errors.length) fails.DT02.push(`${T('페이지 오류')}: ${errors[0]}`);
  await ctx.close();

  // 플레이어 오류 — 보조 플레이어 창(?w=player)으로 연 시청 화면. 모의 IPC 가 player_start 를 실패로 돌려 오류 막이 선다.
  const v = await openApp(theme, { url: `${base}?w=player&channelId=ch2`, scene: 'viewer' });
  try {
    await v.page.waitForSelector('#viewer .player-overlay.error:not([hidden]) > span', { state: 'visible' });
    await settle(v.page);
    await measure(v.page, T('플레이어 오류'), '#viewer .player-overlay.error > span', { inset: 0, name: shot('player-error'), scope: '#viewer .player-overlay.error > span' });
  } catch (e) {
    fails.DT02.push(`${T('플레이어 오류')}: ${e.message.split('\n')[0]}`);
  }
  await v.ctx.close();
}

await screens('dark');
await screens('light');
// 시스템 설정을 따르는 라이트 — 앱 설정 라이트와 다른 블록이라 토큰만 다시 본다
{
  const { ctx, page } = await openApp('system', { scheme: 'light' });
  const t = await tokens(page, 'light(시스템)');
  if (t.theme !== 'system') fails.DT01.push(`light(시스템): data-theme 이 system 이 아니다(${t.theme})`);
  await ctx.close();
}

await browser.close();
server.close();
const titles = {
  DT01: '--error 와 면 토큰 4.5:1', DT02: '위험 항목 글자 4.5:1·아이콘·테두리 3:1(쉼·호버·초점)',
  DT03: '위험 채움 위 흰 글자 4.5:1', DT04: '위험 색이 빨강으로 읽힘',
};
if (REPORT) console.log(REPORT.join('\n'));
let failed = 0;
for (const [id, list] of Object.entries(fails)) {
  console.log(`${list.length ? '✗' : '✓'} ${id} ${titles[id]}`);
  for (const m of list) console.log(`    ${m}`);
  if (list.length) failed += 1;
}
console.log(`\n${Object.keys(fails).length - failed}/${Object.keys(fails).length} 통과 (${WEBKIT ? 'WebKit' : 'Chromium'})`);
process.exit(failed ? 1 : 0);
