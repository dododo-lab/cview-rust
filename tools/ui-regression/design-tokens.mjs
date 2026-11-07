// 디자인 토큰·글자 대비·공통 컴포넌트 UI 회귀 검사.
// 1) CSS 정적 검사 — 반경(4px 이상)·글자 크기(px)를 리터럴로 쓰지 않고 :root 토큰(--radius-* · --fs-*)을 쓰는지,
//    쓰는 토큰이 모두 정의돼 있는지, 같은 토큰을 :root 에서 두 번 정의하지 않는지.
// 2) 화면 검사 — 주요 화면을 다크·라이트로 띄워 axe-core 의 color-contrast 위반이 없는지(이미지 위 글자인 홈 히어로 .hero-main 은 axe 가 배경을 못 읽어 제외).
// 3) 공통 컴포넌트 — 화면 탭(보관함·통계·설정)은 세그먼트 tablist(필터 칩과 구분), 기본 컨트롤 높이는 --control-* 토큰, 좁은 창에서 탭은 한 줄로 밀린다.
// app-ui 를 그대로 띄우고 window.__TAURI__ 를 모의 IPC 로 바꿔 실제 화면 코드를 돌린다(mock-app.mjs, 실제 서비스·데이터는 건드리지 않음).
// 저장소 루트에서: node tools/ui-regression/design-tokens.mjs
//   Playwright 와 axe-core 가 필요하다(예: npm i playwright@1.49.1 axe-core@4.10.2 를 다른 폴더에 설치하고 NODE_PATH 로 지정).
//   CVIEW_UI_SHOTS=<폴더> 면 화면 × 폭(640·900·1280·1680·2560) × 테마 스크린샷을 남긴다 — 단계별 UI 작업의 전후 비교용.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, go, openApp } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');
const AXE = fs.readFileSync(require.resolve('axe-core/axe.min.js'), 'utf8');

const ROOT = path.resolve('app-ui');
const CSS = ['app.css', 'multilive.css', 'motion.css'].map((f) => [f, fs.readFileSync(path.join(ROOT, 'css', f), 'utf8')]);
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}

// ── 1) CSS 정적 검사 ──
const lineOf = (src, index) => src.slice(0, index).split('\n').length;
await check('D01 border-radius uses tokens (literal only for <=3px)', () => {
  const bad = [];
  for (const [file, src] of CSS) {
    for (const m of src.matchAll(/(?<![-\w])border(?:-(?:top|bottom)-(?:left|right))?-radius\s*:\s*([^;}!]+)/g)) {
      for (const px of m[1].matchAll(/(?<![\w.-])(\d+(?:\.\d+)?)px\b/g)) if (Number(px[1]) > 3) bad.push(`${file}:${lineOf(src, m.index)} ${m[0].trim()}`);
    }
  }
  assert.deepEqual(bad, [], '반경은 var(--radius-*) 토큰을 쓴다');
});
await check('D02 font-size uses tokens (literal only for decorative >=40px)', () => {
  const bad = [];
  for (const [file, src] of CSS) {
    // 선언 값 전체를 본다 — calc()·clamp()·var() 대체값 안의 px 도 리터럴로 친다. 값이 40px 이상 리터럴 하나뿐일 때만 허용.
    for (const m of src.matchAll(/(?<![-\w])font-size\s*:\s*([^;}]+)/g)) {
      const value = m[1].replace(/\/\*[\s\S]*?\*\//g, '').replace(/!important/, '').trim();
      const decorative = /^(\d+(?:\.\d+)?)px$/.exec(value);
      if (decorative && Number(decorative[1]) >= 40) continue;
      if (/(?<![\w.-])\d+(?:\.\d+)?px\b/.test(value)) bad.push(`${file}:${lineOf(src, m.index)} font-size: ${value}`);
    }
  }
  assert.deepEqual(bad, [], '글자 크기는 var(--fs-*) 토큰을 쓴다');
});
await check('D03 every used --radius-*/--fs-* token is defined once in :root', () => {
  const all = CSS.map(([, s]) => s).join('\n');
  const used = new Set([...all.matchAll(/var\((--(?:radius|fs)-[\w-]+)/g)].map((m) => m[1]));
  // 세 스타일시트의 모든 :root 블록을 모은다 — 나중에 로드되는 파일의 :root 가 같은 토큰을 다시 정의해도 잡는다.
  const defs = [];
  for (const [file, src] of CSS) {
    for (const block of src.matchAll(/(?:^|\n):root\s*\{([^}]*)\}/g)) {
      for (const d of block[1].matchAll(/(--(?:radius|fs)-[\w-]+)\s*:/g)) defs.push({ token: d[1], at: `${file}:${lineOf(src, block.index + d.index)}` });
    }
  }
  const names = defs.map((d) => d.token);
  const missing = [...used].filter((t) => !names.includes(t));
  const dup = defs.filter((d) => names.indexOf(d.token) !== names.lastIndexOf(d.token)).map((d) => `${d.token} @ ${d.at}`);
  assert.deepEqual(missing, [], '정의 안 된 토큰');
  assert.deepEqual(dup, [], ':root 에서 두 번 정의된 토큰(뒤 정의가 앞 정의를 덮는다)');
  // 검사 자체가 정의를 찾는지 — 0개면 정규식이 깨진 것
  assert.ok(names.includes('--fs-body') && names.includes('--radius-md'), ':root 토큰 정의를 찾지 못함');
});

// ── 2) 화면 검사 ──
const ROUTES = [['home', {}], ['following', {}], ['popular', {}], ['categories', {}], ['multichat', {}], ['multilive', {}], ['library', {}], ['stats', {}], ['search', { query: '루다' }], ['settings', {}]];
const SHOTS = process.env.CVIEW_UI_SHOTS;
const WIDTHS = SHOTS ? [640, 900, 1280, 1680, 2560] : [1280];
const browser = await chromium.launch();

for (const theme of ['dark', 'light']) {
  for (const width of WIDTHS) {
    const { ctx, page } = await openApp(browser, { width, theme });
    for (const [route, params] of ROUTES) {
      await go(page, route, params);
      await page.waitForTimeout(700);
      await page.mouse.move(1, 1);
      if (SHOTS) { fs.mkdirSync(SHOTS, { recursive: true }); await page.screenshot({ path: path.join(SHOTS, `${route}-${theme}-${width}.png`) }); }
      if (width !== 1280) continue;
      await check(`D10 ${theme} ${route}: text contrast meets WCAG AA`, async () => {
        await page.evaluate(AXE);
        const bad = await page.evaluate(async () => {
          const r = await window.axe.run({ include: [['.app']], exclude: [['.hero-main'], ['[hidden]']] }, { runOnly: ['color-contrast'] });
          return r.violations.flatMap((v) => v.nodes.map((n) => `${n.target.join(' ')} ${n.any[0]?.data?.fgColor}/${n.any[0]?.data?.bgColor} ${n.any[0]?.data?.contrastRatio}`));
        });
        assert.deepEqual(bad, []);
      });
    }
    await ctx.close();
  }
}

// ── 3) 공통 컴포넌트 — 화면 탭은 세그먼트, 필터는 칩, 기본 컨트롤 높이는 --control-* 토큰 ──
const TABBED = [['library', '[data-page="library"] .lib-tabs'], ['stats', '[data-page="stats"] .lib-tabs'], ['settings', '[data-page="settings"] .settings-tabs']];
{
  const { ctx, page } = await openApp(browser, { width: 1280 });
  const token = (name) => page.evaluate((n) => parseFloat(getComputedStyle(document.documentElement).getPropertyValue(n)), name);
  const [sm, md, lg] = [await token('--control-sm'), await token('--control-md'), await token('--control-lg')];
  for (const [route, sel] of TABBED) {
    await go(page, route, {});
    await page.waitForSelector(sel);
    await check(`D20 ${route}: page tabs are one segmented tablist (not filter chips)`, async () => {
      const t = await page.$eval(sel, (n) => ({
        cls: [...n.classList], role: n.getAttribute('role'), chips: n.querySelectorAll('.chip').length,
        tabs: [...n.querySelectorAll('[role="tab"]')].map((b) => [b.classList.contains('seg'), b.getAttribute('aria-selected'), b.tabIndex]),
        h: Math.round(n.getBoundingClientRect().height),
      }));
      assert.ok(t.cls.includes('segmented') && t.cls.includes('page-tabs'), `클래스 ${t.cls}`);
      assert.equal(t.role, 'tablist');
      assert.equal(t.chips, 0, '탭 안에 칩이 없다');
      assert.ok(t.tabs.length > 2 && t.tabs.every((x) => x[0]), '모든 탭이 .seg');
      assert.equal(t.tabs.filter((x) => x[1] === 'true').length, 1, '고른 탭 하나');
      assert.deepEqual(t.tabs.filter((x) => x[2] === 0).length, 1, '초점 받는 탭은 고른 탭 하나(나머지는 화살표로)');
      assert.equal(t.h, md, `탭 묶음 높이 = --control-md(${md}px)`);
    });
  }
  await check('D21 settings tabs: arrow keys switch group and keep focus on the tab', async () => {
    await go(page, 'settings', {});
    await page.focus('[data-page="settings"] .settings-tabs [aria-selected="true"]');
    await page.keyboard.press('ArrowRight');
    const st = await page.evaluate(() => ({ sel: document.querySelector('[data-page="settings"] .settings-tabs [aria-selected="true"]')?.dataset.group, focus: document.activeElement?.dataset.group }));
    assert.notEqual(st.sel, 'overview');
    assert.equal(st.focus, st.sel, '다시 그린 뒤에도 고른 탭에 초점');
    await page.keyboard.press('Home');
  });
  await check('D22 default controls follow --control-* heights', async () => {
    const h = async (route, sel) => { await go(page, route, {}); await page.waitForSelector(sel); return page.$eval(sel, (n) => Math.round(n.getBoundingClientRect().height)); };
    assert.equal(await h('following', '[data-page="following"] .chip'), sm, '필터 칩 = --control-sm');
    assert.equal(await h('following', '[data-page="following"] .segmented'), md, '세그먼트 = --control-md');
    assert.equal(await h('following', '[data-page="following"] .btn.primary'), md, '버튼 = --control-md');
    assert.equal(await h('settings', '[data-page="settings"] .settings-search'), md, '설정 검색칸 = --control-md');
    assert.equal(await h('multichat', '[data-page="multichat"] .mc-search'), md, '멀티채팅 검색칸 = --control-md');
    assert.equal(await h('home', '[data-page="home"] .btn.lg'), lg, '큰 버튼 = --control-lg');
  });
  await check('D25 empty states share one structure (.empty, title color, token sizes)', async () => {
    const read = (sel) => page.$eval(sel, (n) => {
      const t = n.querySelector(':scope > strong');
      const root = getComputedStyle(document.documentElement);
      return { cls: [...n.classList], color: getComputedStyle(t).color, size: parseFloat(getComputedStyle(t).fontSize), primary: root.getPropertyValue('--text-primary').trim(),
        headline: parseFloat(root.getPropertyValue('--fs-headline')), subhead: parseFloat(root.getPropertyValue('--fs-subhead')) };
    });
    const toRgb = (hex) => { const h = hex.replace('#', ''); return `rgb(${parseInt(h.slice(0, 2), 16)}, ${parseInt(h.slice(2, 4), 16)}, ${parseInt(h.slice(4, 6), 16)})`; };
    const cases = [];
    await go(page, 'multichat'); await page.waitForSelector('[data-page="multichat"] .mc-empty');
    cases.push(['multichat', await read('[data-page="multichat"] .mc-empty'), 'lg']);
    await go(page, 'multilive'); await page.waitForSelector('[data-page="multilive"] .ml-empty');
    cases.push(['multilive', await read('[data-page="multilive"] .ml-empty'), 'lg']);
    await go(page, 'settings'); await page.fill('[data-page="settings"] .settings-search', 'zzzz');
    await page.waitForSelector('[data-page="settings"] .settings-search-empty:not([hidden]) strong');
    cases.push(['settings search', await read('[data-page="settings"] .settings-search-empty'), '']);
    await page.fill('[data-page="settings"] .settings-search', '');
    for (const [name, c, size] of cases) {
      assert.ok(c.cls.includes('empty'), `${name}: .empty 구조`);
      assert.equal(c.color, toRgb(c.primary), `${name}: 제목은 기본 글자색`);
      assert.equal(c.size, size === 'lg' ? c.headline : c.subhead, `${name}: 제목 크기 토큰`);
    }
  });
  await check('D24 channel page: tabs are a segmented tablist synced with the panel', async () => {
    await go(page, 'channel', { channelId: 'ch1' });
    const sel = '[data-page="channel"] .channel-tabs';
    await page.waitForSelector(sel);
    const read = () => page.$eval(sel, (n) => ({ cls: [...n.classList], sel: [...n.querySelectorAll('[aria-selected="true"]')].map((b) => b.dataset.tab), panel: document.querySelector('[data-page="channel"] .channel-tab-body')?.getAttribute('role') }));
    const a = await read();
    assert.ok(a.cls.includes('segmented') && a.cls.includes('page-tabs'));
    assert.deepEqual(a.sel, ['info']);
    assert.equal(a.panel, 'tabpanel');
    await page.click(`${sel} [data-tab="vod"]`);
    assert.deepEqual((await read()).sel, ['vod']);
  });
  await ctx.close();
}
{
  const { ctx, page } = await openApp(browser, { width: 640 });
  await check('D23 narrow window: library tabs stay on one row and scroll the chosen tab into view', async () => {
    await go(page, 'library', {});
    const sel = '[data-page="library"] .lib-tabs';
    await page.waitForSelector(sel);
    const before = await page.$eval(sel, (n) => ({ h: Math.round(n.getBoundingClientRect().height), over: n.scrollWidth > n.clientWidth, doc: document.scrollingElement.scrollWidth <= innerWidth }));
    assert.ok(before.over, '좁은 창에서는 탭이 넘친다(검사 전제)');
    assert.ok(before.h <= 40, `한 줄(${before.h}px)`);
    assert.ok(before.doc, '페이지 가로 넘침 없음');
    await page.focus(`${sel} [aria-selected="true"]`);
    await page.keyboard.press('End');
    await page.waitForTimeout(150);
    const last = await page.$eval(sel, (n) => { const b = n.querySelector('[role="tab"]:last-child'); const r = b.getBoundingClientRect(); const box = n.getBoundingClientRect(); return { sel: b.getAttribute('aria-selected'), inView: r.left >= box.left - 1 && r.right <= box.right + 1 }; });
    assert.equal(last.sel, 'true');
    assert.ok(last.inView, '고른 마지막 탭이 묶음 안에서 보인다');
  });
  await ctx.close();
}

await browser.close();
server.close();
for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : `\n     ${r.error.split('\n').slice(0, 12).join('\n     ')}`}`);
const failed = results.filter((r) => !r.ok).length;
console.log(`\n${results.length - failed}/${results.length} passed`);
process.exitCode = failed ? 1 : 0;
