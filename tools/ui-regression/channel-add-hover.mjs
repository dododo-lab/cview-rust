// 채널 추가 단추 호버 회귀 검사 — 모의 IPC·헤드리스 브라우저(실제 로그인·채팅·후원 전송 없음).
// 예전엔 채널 추가 단추에 마우스를 올리면 단추 뒤로 브랜드색이 흐릿하게 번졌다(motion.css .btn.primary:hover 의 0 8px 22px 글로우).
// 빈 화면·멀티채팅의 .btn.primary 는 호버 때 공통 .btn:hover 회색 바탕에 덮여 초록 채움이 사라지고 글자가 안 보였다(다크 대비 1.44).
// 대상: 머리줄 .ml-add · 멀티라이브 빈 화면 큰 단추 · 멀티채팅 머리 '+ 채널' · 멀티채팅 빈 화면 단추 · 포커스 배치 서브 줄 끝 추가 칸.
// CH01 호버하면 단추 안쪽 모양이 바뀐다(호버 상태 스타일이 걸린다).
// CH02 블러 글로우 없음(계산 스타일) — 호버·초점에서 단추와 ::before/::after 에 filter: blur 가 없고, 바깥 그림자의 흐림 반경이 6px 이하.
// CH03 바깥 번짐 없음(픽셀) — 단추 경계 3px 바깥 고리에서 호버 전후로 바뀐 픽셀이 3% 이하.
// CH04 대비 — 호버·초점·누름 상태에서 글자색과 단추 안쪽 바탕의 대비가 4.5:1 이상(다크·라이트).
// CH05 키보드 초점 — :focus-visible 이면 2px 이상 테두리가 있고, 안쪽 모양도 호버만큼 바뀐다(키보드도 같은 '켜짐').
// CH06 모션 줄이기(시스템 설정·앱 설정 두 경로) — 호버해도 키프레임 애니메이션이 돌지 않고 단추·아이콘·::before/::after 가 움직이지 않는다.
// CH07 상태 유지 — 가득 참(aria-disabled)은 호버해도 그대로이고 글자는 4.5:1 로 읽힌다(겹친 opacity 까지 셈),
//      추가 서랍이 열리면(aria-expanded) + 가 × 로 돌고, 열린 칩의 글자도 쉼·호버 모두 4.5:1.
// CH08 광택(::after 가 있는 단추만) — 지나가는 도중 커서가 떠나면 띠가 한 프레임에 뚝 사라지지 않고 그 자리에 멈춘 채 옅어지며,
//      곧바로 다시 들어오면 한 프레임에 최대 밝기로 튀지 않고 서서히 밝아진다.
// 저장소 루트에서: node tools/ui-regression/channel-add-hover.mjs   (Playwright 가 필요하다.
//   CVIEW_UI_BROWSER=webkit 이면 앱(macOS)과 같은 WebKit 엔진으로 돈다. CVIEW_UI_SHOTS=<폴더> 면 상태별 PNG 를 남긴다.)
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, base } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const engine = process.env.CVIEW_UI_BROWSER === 'webkit' ? pw.webkit : pw.chromium;
const SHOTS = process.env.CVIEW_UI_SHOTS;
if (SHOTS) fs.mkdirSync(SHOTS, { recursive: true });
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}

// 모의 IPC — scene 'empty'(빈 멀티라이브·멀티채팅), 'focus'(포커스 배치 2채널 → 서브 줄 끝 추가 칸)
function mock([theme, scene]) {
  const sess = (id, i) => ({ id: `s${i}`, slot: i + 1, channelId: id, channelName: `채널${i + 1}`, liveTitle: '방송', profileImageUrl: null, viewerCountText: '1,000', accumulateCount: 0, accumulateCountText: '', openDate: new Date(Date.now() - 600e3).toISOString(), isMuted: true, isAudible: false, isAudioOnly: false, state: { kind: 'playing' }, chatChannelId: null, maxHeightCap: 0, decodeSuspended: false });
  const state = scene === 'focus'
    ? { sessions: ['c1', 'c2'].map(sess), selectedId: 's0', focusMainId: 's0', canAdd: true, maxSessions: 4, layout: 'focus', focusDirection: 'left', focusRatio: 0.7, horizontalRatio: 0.5, verticalRatio: 0.5, multiAudio: false, watchId: null, rev: 1 }
    : { sessions: [], selectedId: null, layout: 'grid', canAdd: true, maxSessions: 4, rev: 1 };
  const lives = Array.from({ length: 4 }, (_, i) => ({ channelId: `p${i}`, channelName: `방송인${i + 1}`, liveTitle: `방송 ${i + 1}`, isLive: true, viewerCount: 100, viewerCountText: '100', openDate: new Date().toISOString() }));
  const handlers = {
    auth_status: () => ({ loggedIn: false, state: 'loggedOut' }),
    settings_get: () => ({ multiLive: { maxConcurrentSessions: 4, maxLiveChannels: 5 }, multiChat: {}, player: {}, chat: {}, general: {}, appearance: { theme } }),
    multilive_state: () => JSON.parse(JSON.stringify(state)),
    multilive_select: () => JSON.parse(JSON.stringify(state)),
    multilive_stage_size: () => null,
    popular_lives: () => lives, following_lives: () => [],
    channel_detail: () => ({ channel: { channelId: 'c1', channelName: '채널1', followerCount: 1000 }, live: null }),
  };
  window.__TAURI__ = {
    core: { invoke: async (cmd, args) => { const h = handlers[cmd]; if (!h) throw new Error(`mock: ${cmd}`); return h(args || {}); } },
    event: { listen: async () => () => {} },
  };
  try { localStorage.setItem('cview.onboarded', '1'); localStorage.setItem('cview.welcomed.v1', '1'); localStorage.setItem('cview.theme', theme); } catch { /* 무시 */ }
}

const TARGETS = [
  { key: 'toolbar', scene: 'empty', route: 'multilive', sel: '.page-multilive .ml-toolbar .ml-add', filled: true },
  { key: 'ml-empty', scene: 'empty', route: 'multilive', sel: '.ml-empty [data-ml="add"]', filled: true },
  { key: 'mc-head', scene: 'empty', route: 'multichat', sel: 'header [data-mc="add"]', filled: true },
  { key: 'mc-empty', scene: 'empty', route: 'multichat', sel: '.mc-empty [data-mc="add"]', filled: true },
  { key: 'focus-tile', scene: 'focus', route: 'multilive', sel: '.ml-focus-add', filled: false },
];

const browser = await engine.launch();
const calc = await (await browser.newContext()).newPage(); // 픽셀 계산용 빈 페이지

async function open(theme, scene, route) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 820 }, colorScheme: theme });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mock, [theme, scene]);
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.waitForFunction(() => { const s = document.getElementById('splash'); return !s || !s.isConnected || getComputedStyle(s).display === 'none' || getComputedStyle(s).opacity === '0'; }, null, { timeout: 8000 }).catch(() => {});
  await page.evaluate((r) => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: r, params: {} } })), route);
  await page.waitForTimeout(900);
  return { ctx, page, errors };
}

// 대상 하나를 data-cah 로 표시하고 위치를 돌려준다(보이는 첫 요소)
async function mark(page, sel) {
  const r = await page.evaluate((s) => {
    const el = [...document.querySelectorAll(s)].find((e) => e.offsetParent !== null && e.getBoundingClientRect().width > 0);
    if (!el) return null;
    document.querySelectorAll('[data-cah]').forEach((x) => x.removeAttribute('data-cah'));
    el.setAttribute('data-cah', '');
    const b = el.getBoundingClientRect();
    return { x: b.x, y: b.y, w: b.width, h: b.height };
  }, sel);
  assert.ok(r, `단추가 보이지 않는다: ${sel}`);
  return r;
}
const rectNow = (page) => page.evaluate(() => { const b = document.querySelector('[data-cah]').getBoundingClientRect(); return { x: b.x, y: b.y, w: b.width, h: b.height }; });
const MARGIN = 20;
const clipOf = (r) => { const x = Math.max(0, Math.floor(r.x - MARGIN)); const y = Math.max(0, Math.floor(r.y - MARGIN)); return { x, y, width: Math.min(1280 - x, Math.ceil(r.w + MARGIN * 2)), height: Math.min(820 - y, Math.ceil(r.h + MARGIN * 2)) }; };

async function away(page) {
  await page.evaluate(() => { const a = document.activeElement; if (a && a !== document.body) a.blur(); });
  await page.mouse.move(640, 805);
  await page.waitForTimeout(450);
}
async function hover(page) { const r = await rectNow(page); await page.mouse.move(r.x + r.w / 2, r.y + r.h / 2); await page.waitForTimeout(450); }
// 키보드로 초점 — 앞 요소에 초점을 두고 Tab(그래야 :focus-visible 이 선다)
async function keyFocus(page) {
  await page.evaluate(() => {
    const el = document.querySelector('[data-cah]');
    const all = [...document.querySelectorAll('button, [href], input, select, [tabindex]:not([tabindex="-1"])')].filter((e) => e.offsetParent !== null && !e.disabled);
    (all[all.indexOf(el) - 1] || document.body).focus();
  });
  await page.keyboard.press('Tab');
  const on = await page.evaluate(() => document.activeElement === document.querySelector('[data-cah]'));
  if (!on) await page.evaluate(() => document.querySelector('[data-cah]').focus());
  await page.waitForTimeout(450);
}

// 단추와 ::before/::after·아이콘의 계산된 모양
const look = (page) => page.evaluate(() => {
  const el = document.querySelector('[data-cah]');
  const pick = (cs) => ({ filter: cs.filter, backdrop: cs.backdropFilter || cs.webkitBackdropFilter || 'none', shadow: cs.boxShadow, transform: cs.transform, translate: cs.translate, scale: cs.scale, rotate: cs.rotate, opacity: cs.opacity, content: cs.content });
  let opa = 1; for (let n = el; n && n.nodeType === 1; n = n.parentElement) opa *= Number(getComputedStyle(n).opacity);
  const svg = el.querySelector('svg');
  const sr = svg ? svg.getBoundingClientRect() : null;
  const er = el.getBoundingClientRect();
  return {
    el: pick(getComputedStyle(el)), before: pick(getComputedStyle(el, '::before')), after: pick(getComputedStyle(el, '::after')),
    svg: svg ? pick(getComputedStyle(svg)) : null, svgRect: sr && [sr.x, sr.y, sr.width, sr.height], rect: [er.x, er.y, er.width, er.height],
    color: getComputedStyle(el).color, opa, outline: { style: getComputedStyle(el).outlineStyle, width: parseFloat(getComputedStyle(el).outlineWidth) },
    focusVisible: el.matches(':focus-visible'), expanded: el.getAttribute('aria-expanded'),
    keyframes: el.getAnimations({ subtree: true }).filter((a) => a.playState === 'running' && a.constructor.name === 'CSSAnimation').map((a) => a.animationName),
  };
});

// box-shadow 목록에서 바깥 그림자의 가장 큰 흐림 반경(px). inset 은 단추 안쪽이라 뺀다.
function maxOuterBlur(shadow) {
  if (!shadow || shadow === 'none') return 0;
  const parts = shadow.split(/,(?![^(]*\))/);
  let max = 0;
  for (const p of parts) {
    if (/\binset\b/.test(p)) continue;
    const lens = p.replace(/rgba?\([^)]*\)|color\([^)]*\)|#[0-9a-f]+|oklch\([^)]*\)/gi, '').match(/-?[\d.]+px/g) || [];
    const blur = lens.length >= 3 ? parseFloat(lens[2]) : 0;
    if (blur > max) max = blur;
  }
  return max;
}

// 두 PNG 비교 — inner: 단추 안쪽(가장자리 3px 안) 평균 차이, ring: 단추 3px 바깥 고리에서 바뀐 픽셀 %
async function compare(a, b, r, clip) {
  return calc.evaluate(async ({ a, b, r, clip }) => {
    const load = async (b64) => { const img = new Image(); img.src = `data:image/png;base64,${b64}`; await img.decode(); const c = document.createElement('canvas'); c.width = img.width; c.height = img.height; const g = c.getContext('2d'); g.drawImage(img, 0, 0); return g.getImageData(0, 0, c.width, c.height); };
    const A = await load(a); const B = await load(b);
    const w = Math.min(A.width, B.width); const h = Math.min(A.height, B.height);
    const s = A.width / clip.width; // 기기 배율
    const ox = (r.x - clip.x) * s, oy = (r.y - clip.y) * s, ow = r.w * s, oh = r.h * s;
    let ring = 0, changed = 0, inner = 0, innerSum = 0;
    for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4;
      const d = Math.max(Math.abs(A.data[i] - B.data[i]), Math.abs(A.data[i + 1] - B.data[i + 1]), Math.abs(A.data[i + 2] - B.data[i + 2]));
      const inside3 = x >= ox + 3 * s && x < ox + ow - 3 * s && y >= oy + 3 * s && y < oy + oh - 3 * s;
      const outside3 = x < ox - 3 * s || x >= ox + ow + 3 * s || y < oy - 3 * s || y >= oy + oh + 3 * s;
      if (inside3) { inner++; innerSum += d; }
      if (outside3) { ring++; if (d > 6) changed++; }
    }
    return { inner: inner ? innerSum / inner : 0, ringPct: ring ? 100 * changed / ring : 0 };
  }, { a: a.toString('base64'), b: b.toString('base64'), r, clip });
}

// 글자색과 단추 안쪽 바탕(가장자리 근처 네 점)의 최저 대비. opa(단추와 조상의 opacity 곱)가 1 보다 작으면 글자를 바탕에 섞어 잰다.
// pts 를 주면 그 점들만 본다(서랍에 가린 곳을 빼려고).
async function contrast(png, r, clip, color, opa = 1, pts = null) {
  return calc.evaluate(async ({ png, r, clip, color, opa, pts: only }) => {
    const img = new Image(); img.src = `data:image/png;base64,${png}`; await img.decode();
    const c = document.createElement('canvas'); c.width = img.width; c.height = img.height; const g = c.getContext('2d'); g.drawImage(img, 0, 0);
    const s = img.width / clip.width;
    const at = (x, y) => [...g.getImageData(Math.round((x - clip.x) * s), Math.round((y - clip.y) * s), 1, 1).data.slice(0, 3)];
    const lum = ([R, G, B]) => { const f = (v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; }; return 0.2126 * f(R) + 0.7152 * f(G) + 0.0722 * f(B); };
    const ratio = (p, q) => { const [x, y] = [lum(p), lum(q)].sort((m, n) => n - m); return (x + 0.05) / (y + 0.05); };
    // 계산된 글자색을 캔버스에 한 점 칠해 sRGB 바이트로 — color-mix 결과는 color(srgb 0.28 0.99 0.74) 처럼 0~1 로 나온다
    const one = document.createElement('canvas'); one.width = one.height = 1; const g1 = one.getContext('2d');
    g1.fillStyle = color; g1.fillRect(0, 0, 1, 1);
    const px = g1.getImageData(0, 0, 1, 1).data; const fg = [px[0], px[1], px[2]]; const alpha = (px[3] / 255) * opa;
    const pts = only || [[r.x + 5, r.y + r.h / 2], [r.x + r.w - 5, r.y + r.h / 2], [r.x + r.w / 2, r.y + 4], [r.x + r.w / 2, r.y + r.h - 4]];
    if (!pts.length) return 0;
    return Math.min(...pts.map(([x, y]) => { const bg = at(x, y); return ratio(fg.map((v, i) => v * alpha + bg[i] * (1 - alpha)), bg); }));
  }, { png: png.toString('base64'), r, clip, color, opa, pts });
}
// 위 네 점 중 실제로 단추가 보이는 점(다른 것에 가리지 않은 점). 서랍이 단추 오른쪽을 덮으면 덮인 경계 바로 앞(서랍 그림자가
// 떨어져 가장 어두운 곳)도 위·아래 줄에서 잰다 — 글자가 없는 줄이라 바탕만 잡힌다.
const visiblePts = (page) => page.evaluate(() => {
  const el = document.querySelector('[data-cah]'); const b = el.getBoundingClientRect();
  const on = (x, y) => { const h = document.elementFromPoint(x, y); return !!h && (h === el || el.contains(h)); };
  const pts = [[b.x + 5, b.y + b.height / 2], [b.x + b.width - 5, b.y + b.height / 2], [b.x + b.width / 2, b.y + 4], [b.x + b.width / 2, b.y + b.height - 4]].filter(([x, y]) => on(x, y));
  for (const y of [b.y + 4, b.y + b.height - 4]) {
    if (on(b.x + b.width - 5, y)) continue;
    for (let x = b.x + b.width - 6; x > b.x + 5; x -= 1) if (on(x, y)) { pts.push([x - 2, y]); break; }
  }
  return pts;
});
// 열린 칩 글자 대비 — 쉼과 호버(윗물들임이 글자 윗부분에 닿는다). 서랍이 단추 일부를 덮으면 보이는 점만.
async function openChipContrast(page, tag, key, theme) {
  const or = await rectNow(page); const oclip = clipOf(or);
  const pts = await visiblePts(page);
  if (!pts.length) { fails.CH07.push(`${tag}: 열림 상태에서 단추가 보이지 않음`); return; }
  const ol = await look(page);
  const oc = await contrast(await shot(page, oclip, `${key}-${theme}-open`), or, oclip, ol.color, ol.opa, pts);
  if (oc < 4.5) fails.CH07.push(`${tag} 열림 글자: ${oc.toFixed(2)}`);
  const vis = pts[0]; await page.mouse.move(vis[0], vis[1]); await page.waitForTimeout(450);
  const hl = await look(page);
  const hc = await contrast(await shot(page, oclip, `${key}-${theme}-open-hover`), or, oclip, hl.color, hl.opa, await visiblePts(page));
  if (hc < 4.5) fails.CH07.push(`${tag} 열림+호버 글자: ${hc.toFixed(2)}`);
}

const fails = { CH01: [], CH02: [], CH03: [], CH04: [], CH05: [], CH06: [], CH07: [], CH08: [] };
const shot = async (page, clip, name) => {
  const buf = await page.screenshot({ clip });
  if (SHOTS) fs.writeFileSync(path.join(SHOTS, `${name}.png`), buf);
  return buf;
};

for (const theme of ['dark', 'light']) {
  for (const T of TARGETS) {
    const tag = `${T.key}/${theme}`;
    const { ctx, page, errors } = await open(theme, T.scene, T.route);
    try {
      await mark(page, T.sel);
      await away(page);
      const r0 = await rectNow(page);
      const clip = clipOf(r0);
      const restLook = await look(page);
      const rest = await shot(page, clip, `${T.key}-${theme}-rest`);

      // 호버
      await hover(page);
      const hoverLook = await look(page);
      const hov = await shot(page, clip, `${T.key}-${theme}-hover`);
      const hc = await compare(rest, hov, r0, clip);
      if (hc.inner < 2) fails.CH01.push(`${tag}: 안쪽 변화 ${hc.inner.toFixed(2)}`);
      for (const part of ['el', 'before', 'after']) {
        const L = hoverLook[part];
        if (/blur\(/.test(L.filter) || /blur\(/.test(L.backdrop)) fails.CH02.push(`${tag} hover ${part}: filter ${L.filter} ${L.backdrop}`);
        const b = maxOuterBlur(L.shadow);
        if (b > 6) fails.CH02.push(`${tag} hover ${part}: 그림자 흐림 ${b}px (${L.shadow.slice(0, 80)})`);
      }
      if (hc.ringPct > 3) fails.CH03.push(`${tag}: 바깥 고리 ${hc.ringPct.toFixed(1)}% 변함`);
      if (T.filled) {
        const c = await contrast(hov, r0, clip, hoverLook.color);
        if (c < 4.5) fails.CH04.push(`${tag} hover: ${c.toFixed(2)}`);
      } else {
        const c = await contrast(hov, r0, clip, hoverLook.color);
        if (c < 4.5) fails.CH04.push(`${tag} hover(칸 글자): ${c.toFixed(2)}`);
      }

      // 누름 — 단추 밖에서 떼어 click 은 일으키지 않는다
      if (T.filled) {
        await page.mouse.down(); await page.waitForTimeout(200);
        const act = await shot(page, clip, `${T.key}-${theme}-active`);
        const c = await contrast(act, r0, clip, (await look(page)).color);
        if (c < 4.5) fails.CH04.push(`${tag} active: ${c.toFixed(2)}`);
        await page.mouse.move(640, 805); await page.mouse.up();
      }
      await away(page);

      // 키보드 초점
      await keyFocus(page);
      const focusLook = await look(page);
      const foc = await shot(page, clip, `${T.key}-${theme}-focus`);
      const fc = await compare(rest, foc, r0, clip);
      if (!focusLook.focusVisible) fails.CH05.push(`${tag}: :focus-visible 아님`);
      if (!(focusLook.outline.style !== 'none' && focusLook.outline.width >= 2)) fails.CH05.push(`${tag}: 초점 테두리 ${focusLook.outline.style} ${focusLook.outline.width}px`);
      if (fc.inner < Math.min(2, hc.inner * 0.5)) fails.CH05.push(`${tag}: 초점 안쪽 변화 ${fc.inner.toFixed(2)} < 호버 ${hc.inner.toFixed(2)}의 절반`);
      for (const part of ['el', 'before', 'after']) {
        const L = focusLook[part];
        if (/blur\(/.test(L.filter)) fails.CH02.push(`${tag} focus ${part}: filter ${L.filter}`);
        const b = maxOuterBlur(L.shadow);
        if (b > 6) fails.CH02.push(`${tag} focus ${part}: 그림자 흐림 ${b}px`);
      }
      if (T.filled) {
        const c = await contrast(foc, r0, clip, focusLook.color);
        if (c < 4.5) fails.CH04.push(`${tag} focus: ${c.toFixed(2)}`);
      }
      await away(page);

      // 광택 도중 떠나기 — 지연 90ms + .65s 의 가운데쯤(들어온 지 380ms)에 커서를 빼고 40ms 뒤를 본다.
      // 띠(::after)가 옅어지는 중이어야 한다: ::after 의 opacity 전환이 돌고 있거나 계산값이 0 과 1 사이.
      if (T.filled) {
        await away(page);
        const rr = await rectNow(page);
        await page.mouse.move(rr.x + rr.w / 2, rr.y + rr.h / 2);
        await page.waitForTimeout(380);
        await page.mouse.move(640, 805);
        await page.waitForTimeout(40);
        // ::after 의 전환 상태 — WebKit 은 전환 중인 가상 요소의 계산값을 '' 로 주므로 getAnimations 로 본다
        const sheenState = () => page.evaluate(() => {
          const el = document.querySelector('[data-cah]');
          const tr = el.getAnimations({ subtree: true }).filter((a) => a.playState === 'running' && (a.effect?.pseudoElement || '') === '::after');
          const of = (p) => tr.filter((a) => (a.transitionProperty || '') === p);
          const ca = getComputedStyle(el, '::after').content;
          return {
            sheen: !!ca && ca !== 'none' && ca !== 'normal', opacity: getComputedStyle(el, '::after').opacity,
            fading: of('opacity').length,
            // 띠가 멈춰 있다 = transform 전환이 아직 지연 구간(제자리 복귀 대기)
            hold: of('transform').filter((a) => (a.currentTime ?? 0) < (a.effect?.getTiming().delay || 0)).length,
          };
        });
        const st = await sheenState();
        const o = parseFloat(st.opacity);
        if (st.sheen && !st.fading && !(o > 0.02 && o < 1)) fails.CH08.push(`${tag}: 떠난 뒤 40ms 에 띠가 이미 없음(::after opacity ${st.opacity || '빈 값'}, 전환 ${st.fading})`);
        if (st.sheen && !st.hold) fails.CH08.push(`${tag}: 떠난 뒤 띠가 그 자리에 멈춰 있지 않음(바로 제자리로 돌아감)`);
        // 옅어지는 중(.18s 안)에 다시 들어오기 — 띠 opacity 가 전환으로 돌아와야 한다(0s 면 한 프레임에 1 로 튄다)
        if (st.sheen) {
          await page.waitForTimeout(20);
          await page.mouse.move(rr.x + rr.w / 2, rr.y + rr.h / 2);
          await page.waitForTimeout(20);
          const re = await sheenState();
          if (!re.fading) fails.CH08.push(`${tag}: 다시 들어오자 띠가 한 프레임에 켜짐(::after opacity 전환 없음, 값 ${re.opacity || '빈 값'})`);
        }
        // 광택 도중 눌러 서랍을 열면(머리줄) 띠가 바로 꺼진다 — 어두운 열린 칩 위에 흰 띠가 남지 않게
        if (st.sheen && T.key === 'toolbar') {
          await away(page);
          await page.mouse.move(rr.x + rr.w / 2, rr.y + rr.h / 2);
          await page.waitForTimeout(380);
          await page.mouse.down(); await page.mouse.up();
          await page.waitForTimeout(30);
          const op = await page.evaluate(() => {
            const el = document.querySelector('[data-cah]');
            return { expanded: el.getAttribute('aria-expanded'), left: el.getAnimations({ subtree: true }).filter((a) => a.playState === 'running' && (a.effect?.pseudoElement || '') === '::after').map((a) => a.transitionProperty || a.animationName) };
          });
          if (op.expanded !== 'true') fails.CH08.push(`${tag}: 눌렀는데 서랍이 열리지 않음`);
          else if (op.left.length) fails.CH08.push(`${tag}: 광택 도중 열었더니 띠가 남음(${op.left.join(',')})`);
          await page.mouse.down(); await page.mouse.up();   // 같은 단추로 닫는다
          await page.waitForTimeout(300);
        }
        await away(page);
      }

      // 가득 참 글자 — 빈 화면 큰 단추(겹치는 opacity 규칙이 있다)
      if (T.key === 'ml-empty') {
        await page.evaluate(() => document.querySelector('[data-cah]').setAttribute('aria-disabled', 'true'));
        await away(page);
        const fl = await look(page);
        const fr = await rectNow(page);
        const fpng = await shot(page, clipOf(fr), `${T.key}-${theme}-full`);
        const c = await contrast(fpng, fr, clipOf(fr), fl.color, fl.opa);
        if (c < 4.5) fails.CH07.push(`${tag} 가득 참 글자: ${c.toFixed(2)}(opacity ${fl.opa})`);
        await page.evaluate(() => document.querySelector('[data-cah]').removeAttribute('aria-disabled'));
        await away(page);
      }

      // 모션 줄이기 — 시스템 설정, 앱 설정(data-motion="reduced")
      for (const mode of ['media', 'app']) {
        if (mode === 'media') await page.emulateMedia({ reducedMotion: 'reduce' });
        else await page.evaluate(() => { document.documentElement.dataset.motion = 'reduced'; });
        await away(page);
        const a = await look(page);
        const r = await rectNow(page);
        await page.mouse.move(r.x + r.w / 2, r.y + r.h / 2);
        await page.waitForTimeout(60);
        const early = await look(page);
        await page.waitForTimeout(400);
        const b = await look(page);
        if (early.keyframes.length || b.keyframes.length) fails.CH06.push(`${tag} ${mode}: 키프레임 ${[...early.keyframes, ...b.keyframes].join(',')}`);
        const moved = (p, q) => p.some((v, i) => Math.abs(v - q[i]) > 0.25);
        if (moved(a.rect, b.rect)) fails.CH06.push(`${tag} ${mode}: 단추가 움직임 ${a.rect.map((v) => v.toFixed(1))} → ${b.rect.map((v) => v.toFixed(1))}`);
        if (a.svgRect && moved(a.svgRect, b.svgRect)) fails.CH06.push(`${tag} ${mode}: 아이콘이 움직임`);
        for (const part of ['before', 'after']) {
          if (a[part].content === 'none' && b[part].content === 'none') continue;   // 그려지지 않는 가상 요소는 움직일 수 없다
          const k = ['transform', 'translate', 'scale', 'rotate'];
          if (k.some((p) => a[part][p] !== b[part][p])) fails.CH06.push(`${tag} ${mode}: ::${part} 이 움직임 ${k.map((p) => b[part][p]).join(' ')}`);
        }
        if (mode === 'media') await page.emulateMedia({ reducedMotion: null });
        else await page.evaluate(() => { delete document.documentElement.dataset.motion; });
      }

      // 상태 유지 — 머리줄 단추만(가득 참·서랍 열림)
      if (T.key === 'toolbar') {
        await away(page);
        await page.evaluate(() => { const el = document.querySelector('[data-cah]'); el.setAttribute('aria-disabled', 'true'); const b = el.querySelector('.ml-add-full'); if (b) { b.hidden = false; b.textContent = '4/4'; } });
        await away(page);
        const r = await rectNow(page);
        const fclip = clipOf(r);
        const full = await shot(page, fclip, `${T.key}-${theme}-full`);
        const fl = await look(page);
        const fcon = await contrast(full, r, fclip, fl.color, fl.opa);
        if (fcon < 4.5) fails.CH07.push(`${tag} 가득 참 글자: ${fcon.toFixed(2)}(opacity ${fl.opa})`);
        await hover(page);
        const fullHov = await shot(page, fclip, `${T.key}-${theme}-full-hover`);
        const d = await compare(full, fullHov, r, fclip);
        if (d.inner > 1.5 || d.ringPct > 1) fails.CH07.push(`${tag}: 가득 참인데 호버로 바뀜(안 ${d.inner.toFixed(2)}, 바깥 ${d.ringPct.toFixed(1)}%)`);
        await page.evaluate(() => { const el = document.querySelector('[data-cah]'); el.removeAttribute('aria-disabled'); const b = el.querySelector('.ml-add-full'); if (b) b.hidden = true; });
        await away(page);
        await page.evaluate(() => document.querySelector('[data-cah]').click());
        await page.waitForTimeout(700);
        await away(page);
        const op = await look(page);
        const deg = await page.evaluate(() => {
          const svg = document.querySelector('[data-cah] svg');
          const cs = getComputedStyle(svg);
          if (cs.rotate && cs.rotate !== 'none') return parseFloat(cs.rotate);
          const m = new DOMMatrix(cs.transform === 'none' ? undefined : cs.transform);
          return Math.round(Math.atan2(m.b, m.a) * 180 / Math.PI);
        });
        if (op.expanded !== 'true') fails.CH07.push(`${tag}: 서랍이 열리지 않음`);
        if (Math.abs(Math.abs(deg) % 90 - 45) > 1) fails.CH07.push(`${tag}: 열림 아이콘 회전 ${deg}°(× 가 아님)`);
        await openChipContrast(page, tag, T.key, theme);
        await page.keyboard.press('Escape'); await page.waitForTimeout(400);
      }
      // 빈 화면·멀티채팅 단추도 서랍을 열면 열린 칩이 된다 — 기본 창(1280)에서는 멀티채팅 빈 화면 단추 위로 서랍이 겹친다
      if (T.filled && T.key !== 'toolbar') {
        await away(page);
        await page.evaluate(() => document.querySelector('[data-cah]').click());
        await page.waitForTimeout(700);
        await page.evaluate(() => { const a = document.activeElement; if (a && a !== document.body) a.blur(); });
        if ((await look(page)).expanded !== 'true') fails.CH07.push(`${tag}: 서랍이 열리지 않음`);
        else await openChipContrast(page, tag, T.key, theme);
      }
      if (errors.length) fails.CH01.push(`${tag}: 페이지 오류 ${errors[0]}`);
    } catch (e) {
      fails.CH01.push(`${tag}: ${e.message}`);
    }
    await ctx.close();
  }
}

const NAMES = {
  CH01: '호버하면 안쪽 모양이 바뀐다', CH02: '블러 글로우 없음(filter blur·흐림 6px 넘는 바깥 그림자)', CH03: '단추 바깥으로 번지는 픽셀 없음',
  CH04: '호버·초점·누름 글자 대비 4.5:1', CH05: '키보드 초점 = 테두리 + 호버만큼의 켜짐', CH06: '모션 줄이기에서 움직임·키프레임 없음', CH07: '가득 참·서랍 열림 상태 유지(글자 4.5:1)',
  CH08: '광택 도중 떠나면 띠가 옅어지며 사라짐',
};
for (const [id, list] of Object.entries(fails)) await check(`${id} ${NAMES[id]}`, async () => assert.deepEqual(list, []));

await browser.close();
server.close();
for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : `\n     ${r.error.split('\n').filter((l) => /^[+-] /.test(l.trim()) || /^\s+'/.test(l)).slice(0, 12).join('\n     ') || r.error.slice(0, 600)}`}`);
const failed = results.filter((r) => !r.ok).length;
console.log(`\n${results.length - failed}/${results.length} passed (${process.env.CVIEW_UI_BROWSER === 'webkit' ? 'webkit' : 'chromium'})`);
process.exit(failed ? 1 : 0);
