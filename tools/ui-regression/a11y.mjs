// 접근성 UI 회귀 검사.
// A01 주요 화면 11개 × 다크·라이트에 axe-core(WCAG 2.1 A·AA + best-practice)를 돌려 위반이 없는지(글자 대비는 design-tokens.mjs D10 이 본다).
// A02 본문 바로가기 — 처음 Tab 하면 보이고, 누르면 지금 화면으로 초점이 옮겨 다음 Tab 이 화면 안으로 간다.
// A03 선택 묶음(필터 칩·정렬 세그먼트)이 고른 상태를 aria-pressed 로 알리고, 바꾸면 따라 바뀌는지.
// A04 키보드 초점 — 화면마다 Tab 으로 지나는 요소가 초점일 때 눈에 보이는 표시(외곽선·고리·배경)가 생기는지.
// 저장소 루트에서: node tools/ui-regression/a11y.mjs   (Playwright·axe-core 가 필요하다)
import fs from 'node:fs';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, go, openApp } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');
const AXE = fs.readFileSync(require.resolve('axe-core/axe.min.js'), 'utf8');
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}
const ROUTES = [['home', {}], ['following', {}], ['popular', {}], ['categories', {}], ['multichat', {}], ['multilive', {}], ['library', {}], ['stats', {}], ['search', { query: '루다' }], ['settings', {}], ['channel', { channelId: 'ch1' }]];
const browser = await chromium.launch();

// ── A01 axe ──
for (const theme of ['dark', 'light']) {
  const { ctx, page } = await openApp(browser, { width: 1280, theme });
  for (const [route, params] of ROUTES) {
    await go(page, route, params);
    await page.waitForTimeout(700);
    await check(`A01 ${theme} ${route}: no axe violations (WCAG 2.1 A/AA, best practice)`, async () => {
      await page.evaluate(AXE);
      const bad = await page.evaluate(async () => {
        const r = await window.axe.run(document, { runOnly: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'best-practice'], rules: { 'color-contrast': { enabled: false } } });
        return r.violations.flatMap((v) => v.nodes.slice(0, 3).map((n) => `${v.id}: ${n.target.join(' ')}`));
      });
      assert.deepEqual(bad, []);
    });
  }
  await ctx.close();
}

{
  const { ctx, page } = await openApp(browser, { width: 1280 });
  // ── A02 본문 바로가기 ──
  await check('A02 skip link appears on first Tab and moves focus into the current screen', async () => {
    await go(page, 'library');
    await page.waitForSelector('[data-page="library"] .lib-tabs');
    await page.evaluate(() => document.activeElement?.blur());
    await page.keyboard.press('Tab');
    await page.waitForTimeout(300); // 미끄러져 나오는 전환(0.15s)이 끝난 뒤
    const link = await page.evaluate(() => { const a = document.activeElement; const r = a.getBoundingClientRect(); return { cls: a.className, inView: r.top >= 0 && r.bottom <= innerHeight && r.width > 0 }; });
    assert.equal(link.cls, 'skip-link', '처음 Tab 은 바로가기');
    assert.ok(link.inView, '초점을 받으면 화면 안에 보인다');
    await page.keyboard.press('Enter');
    assert.equal(await page.evaluate(() => document.activeElement?.dataset.page), 'library', '지금 화면으로 초점');
    await page.keyboard.press('Tab');
    assert.ok(await page.evaluate(() => Boolean(document.activeElement?.closest('[data-page="library"]'))), '다음 Tab 은 화면 안');
  });

  // ── A03 고른 상태 ──
  await check('A03 filter chips and sort segments expose aria-pressed and follow changes', async () => {
    await go(page, 'following');
    const chips = '[data-page="following"] .chips button.chip';
    await page.waitForSelector(chips);
    const read = (sel) => page.$$eval(sel, (bs) => bs.map((b) => [b.classList.contains('active'), b.getAttribute('aria-pressed')]));
    for (const sel of [chips, '[data-page="following"] .segmented button.seg']) {
      const st = await read(sel);
      assert.ok(st.length > 1 && st.every(([on, p]) => p === String(on)), `${sel}: ${JSON.stringify(st)}`);
    }
    await page.click(`${chips}:nth-child(2)`);
    await page.waitForTimeout(100);
    const after = await read(chips);
    assert.equal(after[1][1], 'true', '누른 칩이 pressed');
    assert.ok(after.every(([on, p]) => p === String(on)));
    // 홈 카테고리 칩은 다시 그리지 않고 .active 만 옮긴다
    await go(page, 'home');
    const home = '[data-page="home"] .home-chips button.chip';
    await page.waitForSelector(home);
    assert.equal(await page.$eval('[data-page="home"] .home-chips', (n) => n.getAttribute('role')), 'group', '필터 묶음은 tablist 가 아니라 group');
    await page.click(`${home}:nth-child(2)`);
    await page.waitForTimeout(100);
    const h = await read(home);
    assert.equal(h[1][1], 'true');
    assert.ok(h.every(([on, p]) => p === String(on)), JSON.stringify(h));
  });

  // ── A04 초점 표시 ──
  for (const route of ['home', 'library', 'settings', 'stats']) {
    await check(`A04 ${route}: every Tab stop shows a visible focus indicator`, async () => {
      await go(page, route);
      await page.waitForTimeout(600);
      await page.evaluate(() => document.activeElement?.blur());
      await page.keyboard.press('Tab'); // 바로가기
      await page.keyboard.press('Enter'); // 화면으로
      const missing = [];
      for (let i = 0; i < 30; i++) {
        await page.keyboard.press('Tab');
        const r = await page.evaluate(() => {
          const el = document.activeElement;
          if (!el || el === document.body) return null;
          const snap = () => [el, ...el.querySelectorAll('*')].slice(0, 6).map((n) => { const cs = getComputedStyle(n); return [cs.outlineStyle, cs.outlineWidth, cs.outlineColor, cs.boxShadow, cs.backgroundColor, cs.borderColor].join('|'); }).join(';');
          const focused = snap();
          el.blur();
          const plain = snap();
          el.focus();
          return { changed: focused !== plain, name: `${el.tagName.toLowerCase()}.${[...el.classList].slice(0, 2).join('.')}` };
        });
        if (!r) break;
        if (!r.changed) missing.push(r.name);
      }
      assert.deepEqual([...new Set(missing)], [], '초점일 때 달라지는 표시가 없는 요소');
    });
  }
  await ctx.close();
}

await browser.close();
server.close();
for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : `\n     ${r.error.split('\n').slice(0, 14).join('\n     ')}`}`);
const failed = results.filter((r) => !r.ok).length;
console.log(`\n${results.length - failed}/${results.length} passed`);
process.exitCode = failed ? 1 : 0;
