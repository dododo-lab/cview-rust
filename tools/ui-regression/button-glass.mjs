// 단추 체계(유리 재질, app-ui/css/controls.css) 회귀 검사 — 모의 IPC·헤드리스 브라우저(실제 서비스는 건드리지 않는다).
// 앱 CSS 를 그대로 읽은 화면에 단추 종류를 모아 넣고(button-gallery.mjs 와 같은 묶음) 다크·라이트에서 잰다.
// BG01 토큰 — controls.css 의 --glass-*·--seg-*·--toggle-*·--btn-* 토큰이 다크·라이트(앱 설정)·라이트(시스템 설정) 모두에서 값이 있고,
//      색을 담은 다크 토큰은 두 라이트 블록에서 각각 다시 정해지며(두 블록 값은 같다) 실제 계산값도 다크와 다르다.
//      규칙 본문(사용자 정의 속성 밖)에 색 리터럴(#hex·rgb()·rgba())이 없다(색은 토큰으로만).
// BG02 모양 — 글자 단추·큰 단추·칩·세그먼트 묶음과 칸은 알약(반경 ≥ 높이/2), 아이콘 단추·둥근 아이콘·작은 닫기는 원. 높이는 --control-* 그대로.
// BG03 글자 대비 — 보조·고스트·위험·기본·칩·고른 칩·고른 세그먼트의 글자색과 실제 바탕(조상 바탕을 겹친 값) 4.5:1 이상,
//      쉼·호버·누름(다크·라이트). 위험 호버는 --danger-fill 위 --on-danger.
// BG04 토글 — 켜진 홈은 둘레 면과 3:1 이상, 꺼진 홈의 테두리도 둘레 면과 3:1 이상(WCAG 1.4.11). 키보드 초점이면 초점 고리(켜짐·꺼짐 모두).
// BG05 제자리 — 보조·기본 단추·칩은 호버에 위치·크기가 그대로(떠오르지 않는다), 누름은 한 번만 줄어든다(.btn .955·칩 .925 이상, .99 이하 — 아예 안 줄면 실패).
// BG06 꺼짐 — :disabled 단추는 호버해도 바탕·테두리·그림자가 쉴 때와 같다.
// BG07 초점 — 키보드 초점(:focus-visible)이면 2px 이상 --primary-text 테두리(보조·기본·아이콘·칩·세그먼트).
// BG09 늘 어두운 면 — 히어로 '멀티라이브'(.btn.hero-ghost)·영상 컨트롤 바·영상 위쪽 줄·영상 위 채팅(머리 단추·닫기·후원 패널 칩·닫기)은
//      테마와 무관하게 어두운 유리: 쉼·호버·누름에서 글자 4.5:1·아이콘 3:1 이상, 히어로·영상 위 단추는 다크·라이트 바탕이 같다.
//      후원 패널의 고른 칩은 면이 패널과 3:1 이상(뒤집힌 알약이 패널에 묻히지 않게). 다크·라이트(앱 설정)·라이트(시스템 설정).
// BG10 상태 단추 — 무장된 글자 단추(.link-btn[data-armed])는 호버에 --danger-fill 위 4.5:1, 눌린 채팅 머리 단추(aria-pressed)는 호버에도 눌린 모양.
// BG12 상태 있는 단추 — 즐겨찾기 별(.fav.on)·채팅 도구(펼침)·후원(열림·새 후원)·합치기(켜짐)·툴바 켜짐·눌린 머리 단추·무장된 고스트 단추는
//      호버·누름 중에도 상태 글자색(과 상태 면)이 쉴 때와 같다. 무장 고스트는 쉴 때 빨간 글자 + 안쪽 테두리, 호버는 위험 채움.
//      보통 채팅 머리 단추는 누르면 호버와 다른 눌림 면.
// BG11 각진 세그먼트 — 멀티라이브 툴바·더보기 메뉴의 세그먼트는 판(--glider-r) 모서리가 홈 모서리를 넘지 않는다(각진 홈에 알약 판 없음).
// 저장소 루트에서: node tools/ui-regression/button-glass.mjs   (Playwright 가 필요하다. CVIEW_UI_BROWSER=webkit 이면 WebKit.)
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, openApp } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const engine = process.env.CVIEW_UI_BROWSER === 'webkit' ? pw.webkit : pw.chromium;
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}

const SRC = fs.readFileSync(path.resolve('app-ui/css/controls.css'), 'utf8');
const ICON = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 5v14M5 12h14"/></svg>';
const KIT = `
<div id="bg-kit" style="position:fixed;inset:0;z-index:9999;overflow:auto;padding:24px;background:var(--deck);display:flex;flex-direction:column;gap:14px;align-items:flex-start">
  <div style="display:flex;gap:10px;align-items:center">
    <button class="btn" data-k="btn">보조</button><button class="btn primary" data-k="primary">기본</button>
    <button class="btn ghost" data-k="ghost">고스트</button><button class="btn danger" data-k="danger">삭제</button>
    <button class="btn lg" data-k="lg">큰 단추</button><button class="btn" disabled data-k="disabled">꺼짐</button>
  </div>
  <div style="display:flex;gap:10px;align-items:center;padding:12px;background:var(--surface-base);border-radius:var(--radius-lg)" data-k="pane">
    <button class="btn" data-k="btn-pane">보조</button><button class="btn danger" data-k="danger-pane">초기화</button>
    <button class="icon-btn" aria-label="추가" data-k="icon">${ICON}</button><button class="icon-btn round" aria-label="추가" data-k="round">${ICON}</button>
    <button class="notice-x" aria-label="닫기" data-k="x">×</button>
    <label class="toggle" data-k="toggle-on"><input type="checkbox" checked aria-label="켬"><span></span></label>
    <label class="toggle" data-k="toggle-off"><input type="checkbox" aria-label="끔"><span></span></label>
  </div>
  <div class="segmented" data-no-glider data-k="segmented"><button class="seg active" data-k="seg-active">개요</button><button class="seg" data-k="seg">클립</button></div>
  <div class="chips"><button class="chip active" data-k="chip-active">전체</button><button class="chip" data-k="chip">게임</button></div>
  <div style="display:flex;gap:10px;align-items:center;padding:12px;background:var(--surface-base)">
    <button class="link-btn" data-armed="1" data-k="armed-link">한 번 더 누르면 삭제</button>
    <div class="chat-header" style="display:flex;gap:4px"><button class="icon-btn chat-hbtn" aria-pressed="true" aria-label="검색" data-k="hbtn-pressed">${ICON}</button></div>
    <div class="channel-actions" style="display:flex"><button class="icon-btn round fav on" aria-label="즐겨찾기 해제" data-k="fav">${ICON}</button></div>
    <div class="chat-header" style="display:flex;gap:4px">
      <button class="icon-btn chat-hbtn" aria-label="보통" data-k="hbtn">${ICON}</button>
      <button class="icon-btn chat-hbtn chat-tools-toggle" aria-expanded="true" aria-label="채팅 도구" data-k="tools">${ICON}</button>
      <button class="icon-btn chat-hbtn dp-btn open" aria-label="후원 목록" data-k="dp-open">${ICON}</button>
      <button class="icon-btn chat-hbtn dp-btn has-new" aria-label="새 후원" data-k="dp-new">${ICON}</button>
      <button class="icon-btn chat-hbtn ml-merge-btn active" aria-label="합치기" data-k="merge">${ICON}</button>
    </div>
    <button class="ml-tb-icon active" aria-label="툴바 켜짐" data-k="tb-active">${ICON}</button>
    <button class="btn ghost" data-armed="1" data-k="ghost-armed">끝난 항목 지우기</button>
    <div class="ml-toolbar" style="display:flex"><div class="segmented" data-k="tb-seg"><button class="seg active">그리드</button><button class="seg">포커스</button></div></div>
  </div>
  <!-- 늘 어두운 면 — 사진·영상 대신 짙은 바탕(hero-shade·막대 그라디언트의 어두운 끝) -->
  <div class="hero-actions" style="display:flex;gap:8px;padding:12px;background:var(--bg-media)" data-k="media">
    <button type="button" class="btn lg hero-ghost" data-k="hero">${ICON}멀티라이브</button>
    <div class="chzzk-bar" style="display:flex;position:static"><button class="icon-btn" aria-label="음소거" data-k="bar-icon">${ICON}</button></div>
    <div class="stage-top" style="display:flex;position:static"><button class="icon-btn" id="bg-viewer-close" aria-label="닫기" data-k="top-icon">${ICON}</button></div>
  </div>
  <div class="viewer chat-mode-overlay" style="position:relative;display:block;width:360px;height:300px;background:var(--bg-media)">
    <div class="chat" style="position:absolute;inset:0;top:0;bottom:0;right:0;width:360px !important;display:flex;flex-direction:column">
      <div class="chat-header" style="display:flex;gap:4px;align-items:center;padding:6px">
        <button class="icon-btn chat-hbtn" aria-label="검색" data-k="ov-hbtn">${ICON}</button>
        <button class="notice-x" aria-label="닫기" data-k="ov-x">×</button>
      </div>
      <div class="dp-panel" style="position:relative;inset:auto;top:auto;left:auto;right:auto;margin:6px">
        <div class="dp-head"><span class="dp-title">후원</span><button class="icon-btn dp-close" aria-label="닫기" data-k="ov-close">${ICON}</button></div>
        <div class="dp-controls"><div class="dp-filter chips"><button class="chip active" data-k="ov-chip-active">전체</button><button class="chip" data-k="ov-chip">영상</button></div></div>
      </div>
    </div>
  </div>
</div>
<style>#bg-kit { --bg-media: rgb(18, 19, 22); }</style>`;

async function setup(browser, theme, system = false) {
  const { ctx, page, errors } = await openApp(browser, { width: 1100, height: 700, theme, welcomed: true });
  if (system) await page.evaluate(() => { document.documentElement.dataset.theme = 'system'; });
  await page.evaluate((html) => document.body.insertAdjacentHTML('beforeend', html), KIT);
  await page.waitForTimeout(250);
  return { ctx, page, errors };
}
const away = async (page) => { await page.evaluate(() => document.activeElement?.blur?.()); await page.mouse.move(1090, 690); await page.waitForTimeout(260); };

// 요소의 글자색·실제 바탕(조상 바탕을 겹친 값)·모양
const probe = (page, k) => page.evaluate((key) => {
  const el = document.querySelector(`[data-k="${key}"]`);
  const c = document.createElement('canvas'); c.width = c.height = 1; const g = c.getContext('2d', { willReadFrequently: true });
  const rgba = (col) => { g.clearRect(0, 0, 1, 1); g.fillStyle = '#000'; g.fillStyle = col; g.fillRect(0, 0, 1, 1); const d = g.getImageData(0, 0, 1, 1).data; return [d[0], d[1], d[2], d[3] / 255]; };
  // 바닥부터 겹치기 — 조상들의 바탕색(투명도 포함)을 차례로 얹는다
  const chain = []; for (let n = el; n && n.nodeType === 1; n = n.parentElement) chain.push(n);
  let bg = [0, 0, 0];
  const rootBg = rgba(getComputedStyle(document.documentElement).backgroundColor); if (rootBg[3] > 0) bg = rootBg.slice(0, 3);
  for (const n of chain.reverse()) {
    const [r, gg, b, a] = rgba(getComputedStyle(n).backgroundColor);
    bg = [r * a + bg[0] * (1 - a), gg * a + bg[1] * (1 - a), b * a + bg[2] * (1 - a)];
  }
  const cs = getComputedStyle(el); const r = el.getBoundingClientRect();
  return { fg: rgba(cs.color), bg, radius: parseFloat(cs.borderTopLeftRadius), w: r.width, h: r.height, x: r.x, y: r.y,
    border: cs.borderTopColor, bgColor: cs.backgroundColor, shadow: cs.boxShadow, transform: cs.transform, scale: cs.scale, translate: cs.translate,
    outline: { style: cs.outlineStyle, width: parseFloat(cs.outlineWidth), color: rgba(cs.outlineColor) }, focusVisible: el.matches(':focus-visible') };
}, k);
const lum = ([R, G, B]) => { const f = (v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; }; return 0.2126 * f(R) + 0.7152 * f(G) + 0.0722 * f(B); };
const ratio = (p, q) => { const [a, b] = [lum(p), lum(q)].sort((m, n) => n - m); return (a + 0.05) / (b + 0.05); };
const over = ([r, g, b, a], bg) => [r * a + bg[0] * (1 - a), g * a + bg[1] * (1 - a), b * a + bg[2] * (1 - a)];
const textRatio = (p) => ratio(over(p.fg, p.bg), p.bg);
const center = async (page, k) => { const p = await probe(page, k); return [p.x + p.w / 2, p.y + p.h / 2]; };

async function bg09(page, theme) {
  const bad = [];
  const items = [['hero', 4.5], ['bar-icon', 3], ['top-icon', 3], ['ov-hbtn', 3], ['ov-x', 4.5], ['ov-close', 3], ['ov-chip', 4.5], ['ov-chip-active', 4.5]];
  for (const [k, min] of items) {
    await away(page);
    const rest = await probe(page, k); if (textRatio(rest) < min) bad.push(`${k} 쉼 ${textRatio(rest).toFixed(2)}`);
    const [x, y] = await center(page, k);
    await page.mouse.move(x, y); await page.waitForTimeout(280);
    const hov = await probe(page, k); if (textRatio(hov) < min) bad.push(`${k} 호버 ${textRatio(hov).toFixed(2)} (${hov.bgColor})`);
    await page.mouse.down(); await page.waitForTimeout(220);
    const act = await probe(page, k); if (textRatio(act) < min) bad.push(`${k} 누름 ${textRatio(act).toFixed(2)} (${act.bgColor})`);
    await page.mouse.up();
    if (['hero', 'bar-icon', 'top-icon'].includes(k)) dark09[`${k}`] ??= {}, dark09[k][theme] = [rest.bgColor, hov.bgColor, act.bgColor, rest.fg.join()];
  }
  if (theme !== 'dark') for (const k of Object.keys(dark09)) if (JSON.stringify(dark09[k].dark) !== JSON.stringify(dark09[k][theme])) bad.push(`${k} 다크·라이트 모습이 다르다 ${JSON.stringify(dark09[k])}`);
  // 후원 패널의 고른 칩 — 면이 패널과 3:1 이상(뒤집힌 알약이 패널에 묻히지 않게)
  await away(page);
  const on = await probe(page, 'ov-chip-active');
  const panel = await page.evaluate(() => {
    const cv = document.createElement('canvas'); cv.width = cv.height = 1; const g = cv.getContext('2d');
    g.fillStyle = getComputedStyle(document.querySelector('#bg-kit .dp-panel')).backgroundColor; g.fillRect(0, 0, 1, 1);
    return [...g.getImageData(0, 0, 1, 1).data].slice(0, 3);
  });
  if (ratio(on.bg, panel) < 3) bad.push(`고른 칩 면 ${on.bgColor} 과 패널 rgb(${panel}) 대비 ${ratio(on.bg, panel).toFixed(2)} < 3`);
  assert.deepEqual(bad, []);
}

const dark09 = {};   // BG09 — 다크·라이트 모습 비교
const browser = await engine.launch();
try {
  await check('BG01 tokens defined in every theme, no color literals in rule bodies', async () => {
    const names = [...new Set([...SRC.matchAll(/^\s*(--(?:glass|seg|toggle|btn|chip|pk-sheen|danger-hi|fill-inset)[\w-]*)\s*:/gm)].map((m) => m[1]))];
    assert.ok(names.length >= 20, `토큰이 너무 적다: ${names.length}`);
    for (const [theme, system] of [['dark', false], ['light', false], ['light', true]]) {
      const { ctx, page } = await setup(browser, theme, system);
      const missing = await page.evaluate((list) => list.filter((n) => !getComputedStyle(document.documentElement).getPropertyValue(n).trim()), names);
      assert.deepEqual(missing, [], `${theme}${system ? '(시스템)' : ''} 에서 값이 없는 토큰`);
      await ctx.close();
    }
    // 테마 블록마다 — 색을 담은 다크 토큰은 라이트(앱 설정)·라이트(시스템) 블록에서 각각 다시 정한다(두 블록 값은 같다)
    const noComments = SRC.replace(/\/\*[\s\S]*?\*\//g, '');
    const block = (head) => { const i = noComments.indexOf(head); assert.ok(i >= 0, `블록이 없다: ${head}`); const j = noComments.indexOf('{', i); const k = noComments.indexOf('}', j); return noComments.slice(j + 1, k); };
    const decls = (b) => Object.fromEntries([...b.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)].map((m) => [m[1], m[2].trim().replace(/\s+/g, ' ')]));
    const dark = decls(block(':root, .viewer.chat-mode-overlay .chat'));
    const light = decls(block(':root[data-theme="light"] {'));
    const sys = decls(block(':root[data-theme="system"] {'));
    const SAME = new Set(['--toggle-knob', '--danger-hi', '--fill-inset', '--glass-tint']);   // 테마와 무관한 색
    const themed = Object.keys(dark).filter((n) => /#[0-9a-f]{3,8}\b|rgba?\(|var\(--surface/i.test(dark[n]) && !SAME.has(n));
    assert.ok(themed.length >= 15, `색 토큰이 너무 적다: ${themed.length}`);
    assert.deepEqual(themed.filter((n) => !(n in light)), [], '라이트(앱 설정) 블록에 없는 색 토큰');
    assert.deepEqual(themed.filter((n) => !(n in sys)), [], '라이트(시스템) 블록에 없는 색 토큰');
    assert.deepEqual(Object.keys(light).filter((n) => light[n] !== sys[n]), [], '두 라이트 블록 값이 다르다');
    // 실제 계산값 — 라이트에서 다크와 달라야 한다(블록이 이름만 있고 안 걸리는 경우)
    const vals = {};
    for (const [theme, system] of [['dark', false], ['light', false], ['light', true]]) {
      const { ctx, page } = await setup(browser, theme, system);
      if (system) await page.emulateMedia({ colorScheme: 'light' });
      vals[`${theme}${system ? '-sys' : ''}`] = await page.evaluate((list) => Object.fromEntries(list.map((n) => [n, getComputedStyle(document.documentElement).getPropertyValue(n).trim()])), themed);
      await ctx.close();
    }
    for (const t of ['light', 'light-sys']) {
      const same = themed.filter((n) => vals[t][n] === vals.dark[n] && !/var\(--surface/.test(dark[n]));
      assert.deepEqual(same, [], `${t} 에서 다크와 같은 값`);
    }
    // 규칙 본문 — 주석과 사용자 정의 속성 선언(--x: …;)을 지운 나머지에서 색 리터럴을 찾는다
    const body = SRC.replace(/\/\*[\s\S]*?\*\//g, '').replace(/--[\w-]+\s*:[^;{}]*;/g, '');
    const bad = body.split('\n').filter((l) => /(?<![\w-])(#[0-9a-f]{3,8}\b|rgba?\()/i.test(l)).map((l) => l.trim());
    assert.deepEqual(bad, [], '색은 토큰(var(--…))으로만');
  });

  for (const theme of ['dark', 'light']) {
    const { ctx, page, errors } = await setup(browser, theme);
    await away(page);

    await check(`BG02 [${theme}] capsules and circles`, async () => {
      const bad = [];
      for (const k of ['btn', 'primary', 'lg', 'danger', 'chip', 'chip-active', 'segmented', 'seg-active']) {
        const p = await probe(page, k); if (p.radius + 0.5 < p.h / 2) bad.push(`${k} 반경 ${p.radius} < 높이/2 ${p.h / 2}`);
      }
      for (const k of ['icon', 'round', 'x']) {
        const p = await probe(page, k); if (Math.abs(p.w - p.h) > 1 || p.radius + 0.5 < p.w / 2) bad.push(`${k} 원이 아니다 ${p.w}×${p.h} r${p.radius}`);
      }
      const h = await page.evaluate(() => { const v = (n) => parseFloat(getComputedStyle(document.documentElement).getPropertyValue(n)); return { md: v('--control-md'), lg: v('--control-lg') }; });
      const btn = await probe(page, 'btn'); const lg = await probe(page, 'lg');
      if (Math.round(btn.h) !== h.md) bad.push(`.btn 높이 ${btn.h} ≠ --control-md ${h.md}`);
      if (Math.round(lg.h) !== h.lg) bad.push(`.btn.lg 높이 ${lg.h} ≠ --control-lg ${h.lg}`);
      assert.deepEqual(bad, []);
    });

    await check(`BG03 [${theme}] text contrast at rest/hover/press`, async () => {
      const bad = [];
      for (const k of ['btn', 'primary', 'ghost', 'danger', 'btn-pane', 'danger-pane', 'chip', 'chip-active', 'seg-active', 'seg']) {
        await away(page);
        const rest = await probe(page, k); if (textRatio(rest) < 4.5) bad.push(`${k} 쉼 ${textRatio(rest).toFixed(2)}`);
        const [x, y] = await center(page, k);
        await page.mouse.move(x, y); await page.waitForTimeout(260);
        const hov = await probe(page, k); if (textRatio(hov) < 4.5) bad.push(`${k} 호버 ${textRatio(hov).toFixed(2)}`);
        await page.mouse.down(); await page.waitForTimeout(200);
        const act = await probe(page, k); if (textRatio(act) < 4.5) bad.push(`${k} 누름 ${textRatio(act).toFixed(2)}`);
        await page.mouse.up();
        if (k.startsWith('danger')) {
          const fill = await page.evaluate(() => { const d = document.createElement('div'); d.style.background = 'var(--danger-fill)'; document.body.append(d); const c = getComputedStyle(d).backgroundColor; d.remove(); return c; });
          if (hov.bgColor !== fill) bad.push(`${k} 호버 바탕 ${hov.bgColor} ≠ --danger-fill ${fill}`);
        }
      }
      assert.deepEqual(bad, []);
    });

    await check(`BG04 [${theme}] toggle track and edge contrast, focus ring`, async () => {
      await away(page);
      const surround = (await probe(page, 'pane')).bg;
      const tr = await page.evaluate(() => {
        const span = (k) => getComputedStyle(document.querySelector(`[data-k="${k}"] span`));
        return { on: span('toggle-on').backgroundColor, offShadow: span('toggle-off').boxShadow };
      });
      const c = await page.evaluate((cols) => cols.map((col) => { const cv = document.createElement('canvas'); cv.width = cv.height = 1; const g = cv.getContext('2d'); g.fillStyle = col; g.fillRect(0, 0, 1, 1); const d = g.getImageData(0, 0, 1, 1).data; return [d[0], d[1], d[2], d[3] / 255]; }), [tr.on, (tr.offShadow.match(/(?:rgba?|color)\([^)]*\)|#[0-9a-f]{3,8}/i) || ['transparent'])[0]]);
      const onR = ratio(over(c[0], surround), surround); const edgeR = ratio(over(c[1], surround), surround);
      assert.ok(onR >= 3, `켜진 홈 대비 ${onR.toFixed(2)}`);
      assert.ok(edgeR >= 3, `꺼진 홈 테두리 대비 ${edgeR.toFixed(2)} (${tr.offShadow})`);
      for (const k of ['toggle-on', 'toggle-off']) {
        await page.evaluate((key) => { document.querySelector(`[data-k="${key}"]`).previousElementSibling?.focus?.(); }, k);
        await page.focus(`[data-k="${k}"] input`); await page.keyboard.press('Shift+Tab'); await page.keyboard.press('Tab');
        await page.waitForTimeout(250);
        const sh = await page.evaluate((key) => ({ fv: document.querySelector(`[data-k="${key}"] input`).matches(':focus-visible'), s: getComputedStyle(document.querySelector(`[data-k="${key}"] span`)).boxShadow }), k);
        assert.ok(sh.fv, `${k} 키보드 초점이 아니다`);
        assert.ok(/0px 0px 0px 2px/.test(sh.s), `${k} 초점 고리가 없다: ${sh.s}`);
        // 초점 + 호버 — 호버 테두리가 초점 고리를 덮지 않는다
        const [hx, hy] = await center(page, k); await page.mouse.move(hx, hy); await page.waitForTimeout(250);
        const hs = await page.evaluate((key) => getComputedStyle(document.querySelector(`[data-k="${key}"] span`)).boxShadow, k);
        assert.ok(/0px 0px 0px 2px/.test(hs), `${k} 초점 + 호버에서 초점 고리가 없다: ${hs}`);
        await page.mouse.move(1090, 690);
      }
    });

    await check(`BG05 [${theme}] hover stays in place, press shrinks once`, async () => {
      const bad = [];
      for (const k of ['btn', 'primary', 'chip']) {
        await away(page);
        const a = await probe(page, k);
        const [x, y] = await center(page, k);
        await page.mouse.move(x, y); await page.waitForTimeout(450);
        const b = await probe(page, k);
        if (Math.abs(a.y - b.y) > 0.25 || Math.abs(a.h - b.h) > 0.25) bad.push(`${k} 호버에 움직임 y ${a.y}→${b.y} h ${a.h}→${b.h}`);
        await page.mouse.down(); await page.waitForTimeout(450);
        const c = await probe(page, k);
        const s = c.w / a.w;
        const min = k === 'chip' ? 0.925 : 0.955;   // 글자 단추 .965 · 작은 단추(칩) .93 — 한 번만
        if (s < min) bad.push(`${k} 누름 ${s.toFixed(3)} — 두 번 줄었다`);
        if (s > 0.99) bad.push(`${k} 누름 ${s.toFixed(3)} — 줄어들지 않는다`);
        await page.mouse.up();
      }
      assert.deepEqual(bad, []);
    });

    await check(`BG06 [${theme}] disabled ignores hover`, async () => {
      await away(page);
      const a = await probe(page, 'disabled');
      const [x, y] = await center(page, 'disabled');
      await page.mouse.move(x, y); await page.waitForTimeout(300);
      const b = await probe(page, 'disabled');
      assert.deepEqual([b.bgColor, b.border, b.shadow, b.y], [a.bgColor, a.border, a.shadow, a.y]);
    });

    await check(`BG07 [${theme}] keyboard focus outline`, async () => {
      const bad = [];
      const want = await page.evaluate(() => { const d = document.createElement('div'); d.style.color = 'var(--primary-text)'; document.body.append(d); const c = getComputedStyle(d).color; d.remove(); return c; });
      for (const k of ['btn', 'primary', 'icon', 'round', 'chip', 'seg']) {
        await away(page);
        await page.focus(`[data-k="${k}"]`); await page.keyboard.press('Shift+Tab'); await page.keyboard.press('Tab');
        await page.waitForTimeout(320);
        const p = await probe(page, k);
        const wantC = await page.evaluate((col) => { const cv = document.createElement('canvas'); cv.width = cv.height = 1; const g = cv.getContext('2d'); g.fillStyle = col; g.fillRect(0, 0, 1, 1); return [...g.getImageData(0, 0, 1, 1).data].slice(0, 3); }, want);
        if (!p.focusVisible) { bad.push(`${k} 초점이 :focus-visible 이 아니다`); continue; }
        if (p.outline.style === 'none' || p.outline.width < 2) bad.push(`${k} 테두리 ${p.outline.style} ${p.outline.width}`);
        else if (p.outline.color.slice(0, 3).some((v, i) => Math.abs(v - wantC[i]) > 2)) bad.push(`${k} 테두리색 ${p.outline.color} ≠ --primary-text`);
      }
      assert.deepEqual(bad, []);
    });

    await check(`BG09 [${theme}] always-dark surfaces keep dark glass`, () => bg09(page, theme));

    await check(`BG10 [${theme}] armed text button and pressed header button on hover`, async () => {
      const bad = [];
      await away(page);
      let [x, y] = await center(page, 'armed-link');
      await page.mouse.move(x, y); await page.waitForTimeout(280);
      const armed = await probe(page, 'armed-link');
      const fill = await page.evaluate(() => { const d = document.createElement('div'); d.style.background = 'var(--danger-fill)'; document.body.append(d); const c = getComputedStyle(d).backgroundColor; d.remove(); return c; });
      if (armed.bgColor !== fill) bad.push(`무장 글자 단추 호버 바탕 ${armed.bgColor} ≠ --danger-fill ${fill}`);
      if (textRatio(armed) < 4.5) bad.push(`무장 글자 단추 호버 대비 ${textRatio(armed).toFixed(2)}`);
      await away(page);
      const pr = await probe(page, 'hbtn-pressed');
      [x, y] = await center(page, 'hbtn-pressed');
      await page.mouse.move(x, y); await page.waitForTimeout(280);
      const ph = await probe(page, 'hbtn-pressed');
      if (ph.bgColor !== pr.bgColor || ph.fg.join() !== pr.fg.join()) bad.push(`눌린 머리 단추 호버에 모양이 풀린다 ${pr.bgColor}/${pr.fg} → ${ph.bgColor}/${ph.fg}`);
      assert.deepEqual(bad, []);
    });

    await check(`BG12 [${theme}] stateful buttons keep their state on hover and press`, async () => {
      const bad = [];
      const errC = await page.evaluate(() => { const d = document.createElement('div'); d.style.color = 'var(--error)'; document.body.append(d); const c = getComputedStyle(d).color; d.remove(); return c; });
      const fill = await page.evaluate(() => { const d = document.createElement('div'); d.style.background = 'var(--danger-fill)'; document.body.append(d); const c = getComputedStyle(d).backgroundColor; d.remove(); return c; });
      // [키, 면도 같아야 하나]
      for (const [k, keepBg] of [['fav', false], ['tools', true], ['dp-open', true], ['dp-new', false], ['merge', true], ['tb-active', true], ['hbtn-pressed', true]]) {
        await away(page);
        const rest = await probe(page, k);
        const [x, y] = await center(page, k);
        await page.mouse.move(x, y); await page.waitForTimeout(280);
        const hov = await probe(page, k);
        await page.mouse.down(); await page.waitForTimeout(220);
        const act = await probe(page, k);
        await page.mouse.up();
        for (const [st, p] of [['호버', hov], ['누름', act]]) {
          if (p.fg.join() !== rest.fg.join()) bad.push(`${k} ${st} 글자색 ${rest.fg} → ${p.fg}`);
          if (keepBg && p.bgColor !== rest.bgColor) bad.push(`${k} ${st} 상태 면 ${rest.bgColor} → ${p.bgColor}`);
        }
      }
      await away(page);
      const ga = await probe(page, 'ghost-armed');
      const gaC = `rgb(${ga.fg.slice(0, 3).join(', ')})`;
      if (gaC !== errC) bad.push(`무장 고스트 쉼 글자 ${gaC} ≠ --error ${errC}`);
      if (!/inset/.test(ga.shadow)) bad.push(`무장 고스트 쉼에 안쪽 테두리가 없다: ${ga.shadow}`);
      let [x, y] = await center(page, 'ghost-armed');
      await page.mouse.move(x, y); await page.waitForTimeout(280);
      const gh = await probe(page, 'ghost-armed');
      if (gh.bgColor !== fill) bad.push(`무장 고스트 호버 바탕 ${gh.bgColor} ≠ --danger-fill ${fill}`);
      // 보통 머리 단추 — 누름은 호버와 다른 면
      await away(page);
      [x, y] = await center(page, 'hbtn');
      await page.mouse.move(x, y); await page.waitForTimeout(280);
      const hh = await probe(page, 'hbtn');
      await page.mouse.down(); await page.waitForTimeout(220);
      const ha = await probe(page, 'hbtn');
      await page.mouse.up();
      if (hh.bgColor === ha.bgColor && hh.shadow === ha.shadow) bad.push(`보통 머리 단추 누름이 호버와 같다 ${ha.bgColor}`);
      assert.deepEqual(bad, []);
    });

    await check(`BG11 [${theme}] square segmented thumb matches track`, async () => {
      const r = await page.evaluate(() => { const el = document.querySelector('[data-k="tb-seg"]'); const cs = getComputedStyle(el); return { track: parseFloat(cs.borderTopLeftRadius), glider: parseFloat(cs.getPropertyValue('--glider-r')), seg: parseFloat(getComputedStyle(el.querySelector('.seg')).borderTopLeftRadius) }; });
      assert.ok(r.track < 20, `툴바 세그먼트 홈이 각지지 않다(반경 ${r.track}) — 검사 전제`);
      assert.ok(r.glider <= r.track, `판 모서리 ${r.glider} > 홈 모서리 ${r.track}`);
      assert.ok(Math.abs(r.glider - r.seg) <= 1, `판 모서리 ${r.glider} ≠ 칸 모서리 ${r.seg}`);
    });

    await check(`BG08 [${theme}] no page errors`, async () => assert.deepEqual(errors, []));
    await ctx.close();
  }
  {
    const { ctx, page } = await setup(browser, 'light', true);
    await page.emulateMedia({ colorScheme: 'light' }); await page.waitForTimeout(200);
    await check('BG09 [light-system] always-dark surfaces keep dark glass', () => bg09(page, 'light-system'));
    await ctx.close();
  }
} finally { await browser.close(); server.close(); }

for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : `\n     ${r.error.split('\n').slice(0, 80).join('\n     ')}`}`);
const failed = results.filter((r) => !r.ok).length;
console.log(`\n${results.length - failed}/${results.length} passed`);
process.exitCode = failed ? 1 : 0;
