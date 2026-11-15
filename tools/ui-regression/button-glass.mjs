// 단추 체계(유리 재질, app-ui/css/controls.css) 회귀 검사 — 모의 IPC·헤드리스 브라우저(실제 서비스는 건드리지 않는다).
// 앱 CSS 를 그대로 읽은 화면에 단추 종류를 모아 넣고(button-gallery.mjs 와 같은 묶음) 다크·라이트에서 잰다.
// BG01 토큰 — controls.css 의 --glass-*·--seg-*·--toggle-*·--btn-* 토큰이 다크·라이트(앱 설정)·라이트(시스템 설정) 모두에서 값이 있고,
//      규칙 본문(사용자 정의 속성 밖)에 색 리터럴(#hex·rgb()·rgba())이 없다(색은 토큰으로만).
// BG02 모양 — 글자 단추·큰 단추·칩·세그먼트 묶음과 칸은 알약(반경 ≥ 높이/2), 아이콘 단추·둥근 아이콘·작은 닫기는 원. 높이는 --control-* 그대로.
// BG03 글자 대비 — 보조·고스트·위험·기본·칩·고른 칩·고른 세그먼트의 글자색과 실제 바탕(조상 바탕을 겹친 값) 4.5:1 이상,
//      쉼·호버·누름(다크·라이트). 위험 호버는 --danger-fill 위 --on-danger.
// BG04 토글 — 켜진 홈은 둘레 면과 3:1 이상, 꺼진 홈의 테두리도 둘레 면과 3:1 이상(WCAG 1.4.11). 키보드 초점이면 초점 고리(켜짐·꺼짐 모두).
// BG05 제자리 — 보조·기본 단추·칩은 호버에 위치·크기가 그대로(떠오르지 않는다), 누름은 한 번만 줄어든다(.btn .955·칩 .925 이상).
// BG06 꺼짐 — :disabled 단추는 호버해도 바탕·테두리·그림자가 쉴 때와 같다.
// BG07 초점 — 키보드 초점(:focus-visible)이면 2px 이상 --primary-text 테두리(보조·기본·아이콘·칩·세그먼트).
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
</div>`;

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

    await check(`BG08 [${theme}] no page errors`, async () => assert.deepEqual(errors, []));
    await ctx.close();
  }
} finally { await browser.close(); server.close(); }

for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : `\n     ${r.error.split('\n').slice(0, 12).join('\n     ')}`}`);
const failed = results.filter((r) => !r.ok).length;
console.log(`\n${results.length - failed}/${results.length} passed`);
process.exitCode = failed ? 1 : 0;
