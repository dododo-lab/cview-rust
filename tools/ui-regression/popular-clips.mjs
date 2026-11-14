// 방송&클립(사이드바 예전 '인기') UI 회귀 검사 — 보관함에 있던 인기 클립을 이 화면의 '클립' 탭으로 옮겼다(pages.js PopularPage).
// P01 이름 — 사이드바·화면 제목·명령 팔레트·단축키 안내가 모두 '방송&클립'이고 '인기' 메뉴 이름이 남지 않는다.
// P02 탭 구조 — role=tablist/tab/tabpanel, aria-selected·aria-controls·aria-labelledby, 고른 탭만 tabindex 0, 패널은 하나만 보인다.
// P03 키보드 — ←/→(끝에서 돌아감)·Home·End 로 탭을 고르고 초점이 따라간다. 클립 탭은 처음 고를 때 인기 클립을 받는다.
// P04 기억 — 마지막으로 고른 탭을 다시 열 때(앱을 다시 띄워도) 그 탭으로 연다. 매개변수로 준 탭이 기억보다 먼저.
// P05 진입점 — 옛 경로(보관함 clips)·홈 '오늘의 인기 클립' 전체 보기(오늘 기간)·보관함 개요 바로가기·딥링크(cview://popular?tab=clips).
// P06 모션 — 보통이면 들어오는 패널(.entering)·나가는 패널(.leaving)이 잠깐 겹쳐 교차하고 끝나면 하나만 남는다.
//     모션 줄이기(prefers-reduced-motion)면 교차 없이 바로 바뀐다.
// P07 클립 탭 — 기간·정렬을 바꾸면 그 인자로 다시 받고 머리 설명이 따라간다. 받는 동안 세로 스켈레톤. 다른 탭에 다녀와도 다시 받지 않는다.
// P08 보관함 — 인기 클립 탭·카드가 없고, 영상 묶음(카드 셋)이 어느 폭에서도 빈칸 없이 줄을 채운다.
// P09 좁은 창(1000×660)·라이트/다크 — 가로 넘침 없음, 탭 두 개가 다 보임, 탭 전환 중에도 가로 스크롤이 생기지 않는다.
// P10 새로고침(⌘R)·작업 공간 저장은 사용자가 바꾼 탭·기간을 연 때의 매개변수로 되돌리지 않는다.
// P11 나가는 패널 안에 초점이 있으면 고른 탭으로 옮긴다(inert 로 body 에 떨어지지 않게). 탭마다 스크롤 위치를 기억한다.
// 저장소 루트에서: node tools/ui-regression/popular-clips.mjs   (Playwright 가 필요하다.
//   CVIEW_UI_BROWSER=webkit 이면 앱과 같은 WebKit 엔진으로 돈다. CVIEW_UI_SHOTS=<폴더> 면 스크린샷)
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, go, base, mockTauri } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const ENGINE = process.env.CVIEW_UI_BROWSER === 'webkit' ? 'webkit' : 'chromium';
const browser = await pw[ENGINE].launch();
const SHOTS = process.env.CVIEW_UI_SHOTS;
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}

// 부른 명령과 인자, 딥링크 수신기를 남긴다(mockTauri 다음에 돈다).
function spy() {
  window.__args = [];
  window.__listeners = {};
  const inv = window.__TAURI__.core.invoke;
  window.__TAURI__.core.invoke = (cmd, args) => { window.__args.push([cmd, args || {}]); return inv(cmd, args); };
  window.__TAURI__.event.listen = async (name, fn) => { (window.__listeners[name] ||= []).push(fn); return () => {}; };
}
// mock-app 의 openApp 은 초기화 스크립트를 더 받지 않는다 — 같은 순서로 열고 spy·모션 줄이기를 더한다(reload 에도 다시 걸린다).
async function openSpied(opts = {}) {
  const ctx = await browser.newContext({ viewport: { width: opts.width || 1280, height: opts.height || 860 }, colorScheme: opts.theme || 'dark', reducedMotion: opts.reduced ? 'reduce' : 'no-preference' });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, opts.theme || 'dark']);
  await page.addInitScript(spy);
  if (opts.clear !== false) await page.addInitScript(() => { if (!sessionStorage.getItem('kept')) { try { localStorage.removeItem('cview.popular.tab'); } catch { /* 무시 */ } sessionStorage.setItem('kept', '1'); } });
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.waitForFunction(() => { const s = document.getElementById('splash'); return !s || !s.isConnected || getComputedStyle(s).display === 'none' || getComputedStyle(s).opacity === '0'; }, null, { timeout: 8000 }).catch(() => {});
  return { ctx, page, errors };
}

const P = '[data-page="popular"]';
const state = (page) => page.evaluate((sel) => {
  const root = document.querySelector(sel);
  const tabs = [...root.querySelectorAll('[role="tab"]')];
  const panels = [...root.querySelectorAll('[role="tabpanel"]')];
  return {
    visible: !root.hidden,
    tablist: root.querySelector('[role="tablist"]')?.getAttribute('aria-label'),
    tabs: tabs.map((t) => ({ id: t.dataset.tab, sel: t.getAttribute('aria-selected'), ti: t.tabIndex, ctrl: t.getAttribute('aria-controls'), tid: t.id, text: t.textContent.trim() })),
    panels: panels.map((x) => ({ id: x.id, hidden: x.hidden, by: x.getAttribute('aria-labelledby') })),
    active: document.activeElement?.dataset?.tab || null,
    sub: root.querySelector('.page-subtitle')?.textContent,
  };
}, P);
const selected = async (page) => (await state(page)).tabs.find((t) => t.sel === 'true')?.id;
const shownPanels = async (page) => (await state(page)).panels.filter((x) => !x.hidden).map((x) => x.id);
const settle = (page) => page.waitForFunction((sel) => !document.querySelector(`${sel} .pop-panel.entering, ${sel} .pop-panel.leaving`), P);
const argsOf = (page, cmd) => page.evaluate((c) => window.__args.filter(([n]) => n === c).map(([, a]) => a), cmd);
const shot = async (page, name) => { if (SHOTS) { fs.mkdirSync(SHOTS, { recursive: true }); await page.screenshot({ path: path.join(SHOTS, `${ENGINE}-${name}.png`) }); } };

// ── 이름·구조·키보드·클립 탭 ──
{
  const { ctx, page, errors } = await openSpied();
  await go(page, 'popular');
  await page.waitForSelector(`${P} .pop-panel[data-panel="live"] .grid .card`);

  await check('P01 the menu is named 방송&클립 everywhere', async () => {
    assert.equal(await page.textContent('.sidebar .nav-item[data-route="popular"] .nav-label'), '방송&클립');
    assert.equal(await page.textContent(`${P} .page-title`), '방송&클립');
    assert.ok(!(await page.$$eval('.sidebar .nav-item .nav-label', (ls) => ls.map((l) => l.textContent.trim()))).includes('인기'), '사이드바에 인기 메뉴가 남음');
    const docTitle = await page.evaluate(() => document.title);
    assert.ok(!/^인기$/.test(docTitle), docTitle);
    // 명령 팔레트
    await page.keyboard.press(process.platform === 'darwin' ? 'Meta+k' : 'Control+k');
    await page.waitForSelector('.palette-item');
    const items = await page.$$eval('.palette-item', (xs) => xs.map((x) => x.textContent.replace(/\s+/g, ' ').trim()));
    assert.ok(items.some((t) => t.startsWith('방송&클립')), `팔레트 화면 항목: ${items.slice(0, 10).join(' | ')}`);
    assert.ok(items.some((t) => t.startsWith('인기 클립') && t.includes('방송&클립')), '팔레트 인기 클립 → 방송&클립');
    assert.ok(!items.some((t) => /^인기 이동/.test(t)), '팔레트에 옛 인기 화면 항목');
    await page.keyboard.press('Escape');
    // 단축키 안내(app-sheets.js) — 원본 문자열 확인
    const src = await page.evaluate(async () => (await fetch('/js/app-sheets.js')).text());
    assert.match(src, /홈 · 팔로잉 · 방송&클립 · 멀티라이브/);
    const ws = await page.evaluate(async () => (await fetch('/js/workspace.js')).text());
    assert.match(ws, /\['5', 'popular', '방송&클립'\]/);
  });

  await check('P02 tablist/tab/tabpanel wiring', async () => {
    const s = await state(page);
    assert.equal(s.tablist, '방송&클립');
    assert.deepEqual(s.tabs.map((t) => [t.id, t.text]), [['live', '방송'], ['clips', '클립']]);
    assert.deepEqual(s.tabs.map((t) => [t.sel, t.ti]), [['true', 0], ['false', -1]]);
    for (const t of s.tabs) {
      const panel = s.panels.find((x) => x.id === t.ctrl);
      assert.ok(panel, `${t.id} aria-controls → ${t.ctrl}`);
      assert.equal(panel.by, t.tid, `${t.id} 패널 aria-labelledby`);
    }
    assert.deepEqual(await shownPanels(page), ['pop-panel-live']);
  });

  await check('P03 ←/→ Home End move selection and focus (wraps)', async () => {
    const week = async () => (await argsOf(page, 'popular_clips')).filter((a) => a.filter === 'WITHIN_7_DAYS').length; // 홈 레일(오늘)은 빼고
    assert.equal(await week(), 0);
    await page.focus(`${P} [role="tab"][data-tab="live"]`);
    await page.keyboard.press('ArrowRight');
    assert.equal(await selected(page), 'clips');
    assert.equal((await state(page)).active, 'clips', '초점이 클립 탭으로');
    await page.waitForSelector(`${P} .pop-panel[data-panel="clips"] .clip-grid .card`);
    await settle(page);
    assert.deepEqual(await shownPanels(page), ['pop-panel-clips']);
    assert.equal(await week(), 1, '클립 탭을 처음 고를 때 한 번 받음');
    await page.keyboard.press('ArrowRight'); // 끝에서 처음으로
    assert.equal(await selected(page), 'live');
    await page.keyboard.press('End');
    assert.equal(await selected(page), 'clips');
    await page.keyboard.press('Home');
    assert.equal(await selected(page), 'live');
    await page.keyboard.press('ArrowLeft'); // 처음에서 끝으로
    assert.equal(await selected(page), 'clips');
    assert.equal((await state(page)).active, 'clips');
    await settle(page);
    assert.deepEqual(await shownPanels(page), ['pop-panel-clips']);
    assert.equal(await week(), 1, '오가도 다시 받지 않음(같은 기간·정렬)');
  });

  await check('P07 clip period/order refetch, subtitle follows, vertical skeleton while loading', async () => {
    assert.equal((await state(page)).sub, '최근 7일 · 인기순');
    await page.evaluate(() => {
      const inv = window.__TAURI__.core.invoke;
      window.__TAURI__.core.invoke = async (cmd, args) => { if (cmd === 'popular_clips' && window.__slow) await new Promise((r) => setTimeout(r, 600)); return inv(cmd, args); };
      window.__slow = true;
    });
    await page.click(`${P} [data-clip-filter="WITHIN_1_DAY"]`);
    await page.waitForSelector(`${P} .pop-panel[data-panel="clips"] .sk-card.clip`);
    const ratio = await page.$eval(`${P} .sk-card.clip .thumb-sk`, (n) => { const r = n.getBoundingClientRect(); return r.height / r.width; });
    assert.ok(ratio > 1.6, `스켈레톤 세로 비율 ${ratio}`);
    assert.equal(await page.locator(`${P} .pop-clip-toolbar`).count(), 1, '받는 동안 도구 줄 유지');
    await page.waitForSelector(`${P} .pop-panel[data-panel="clips"] .clip-grid .card`);
    await page.evaluate(() => { window.__slow = false; });
    assert.equal((await state(page)).sub, '오늘 · 인기순');
    await page.click(`${P} [data-clip-order="RECOMMEND"]`);
    await page.waitForFunction(() => window.__args.some(([c, a]) => c === 'popular_clips' && a.order === 'RECOMMEND' && a.filter === 'WITHIN_1_DAY'));
    await page.waitForFunction((sel) => document.querySelector(`${sel} .page-subtitle`).textContent === '오늘 · 추천순', P);
    await page.waitForFunction((sel) => document.querySelector(`${sel} [data-clip-order="RECOMMEND"]`)?.getAttribute('aria-pressed') === 'true', P);
    // 방송 탭으로 가면 머리 설명도 방송 것으로
    await page.click(`${P} [role="tab"][data-tab="live"]`);
    assert.equal((await state(page)).sub, '시청자 많은 순');
    await settle(page);
  });

  await check('P06 tab switch cross-fades (both panels briefly, then one)', async () => {
    // 누른 바로 그 순간을 같은 평가 안에서 읽는다(왕복 시간에 교차가 끝나 버리지 않게)
    const mid = await page.evaluate((sel) => (document.querySelector(`${sel} [role="tab"][data-tab="clips"]`).click(), {
      entering: document.querySelector(`${sel} .pop-panel.entering`)?.dataset.panel,
      leaving: document.querySelector(`${sel} .pop-panel.leaving`)?.dataset.panel,
      leavingInert: document.querySelector(`${sel} .pop-panel.leaving`)?.inert,
      dir: document.querySelector(sel).style.getPropertyValue('--pop-dir'),
      anim: getComputedStyle(document.querySelector(`${sel} .pop-panel.entering`)).animationName,
    }), P);
    assert.deepEqual([mid.entering, mid.leaving, mid.leavingInert, mid.dir], ['clips', 'live', true, '1']);
    assert.equal(mid.anim, 'pop-panel-in');
    await settle(page);
    assert.deepEqual(await shownPanels(page), ['pop-panel-clips']);
    await page.click(`${P} [role="tab"][data-tab="live"]`);
    assert.equal(await page.evaluate((sel) => document.querySelector(sel).style.getPropertyValue('--pop-dir'), P), '-1', '왼쪽 탭으로는 반대 방향');
    await settle(page);
    // 고른 칸 판(.seg-glider)이 고른 탭 아래로
    await page.waitForTimeout(500);
    const g = await page.evaluate((sel) => {
      const gl = document.querySelector(`${sel} .pop-tabs .seg-glider`); const a = document.querySelector(`${sel} .pop-tabs .seg.active`);
      if (!gl) return null; const r = gl.getBoundingClientRect(); const b = a.getBoundingClientRect(); return Math.abs(r.left - b.left) + Math.abs(r.width - b.width);
    }, P);
    assert.ok(g !== null && g < 3, `판 위치 차이 ${g}`);
  });
  await shot(page, 'popular-live');
  await check('P0x no page errors', async () => assert.deepEqual(errors, []));
  await ctx.close();
}

// ── 기억 ──
await check('P04 last tab is remembered (navigation and relaunch); a tab param wins', async () => {
  const { ctx, page } = await openSpied();
  await go(page, 'popular');
  await page.click(`${P} [role="tab"][data-tab="clips"]`);
  await go(page, 'home');
  await go(page, 'popular');
  assert.equal(await selected(page), 'clips', '다른 화면에 다녀와도');
  assert.equal(await page.evaluate(() => localStorage.getItem('cview.popular.tab')), 'clips');
  await page.reload();
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.waitForFunction(() => window.__TAURI__ && document.querySelector('[data-page="popular"] .pop-tabs'));
  await go(page, 'popular');
  assert.equal(await selected(page), 'clips', '다시 띄워도');
  await page.waitForSelector(`${P} .pop-panel[data-panel="clips"] .clip-grid .card`);
  await go(page, 'popular', { tab: 'live' });
  assert.equal(await selected(page), 'live', '매개변수가 기억보다 먼저');
  assert.equal(await page.evaluate(() => localStorage.getItem('cview.popular.tab')), 'live', '매개변수로 연 탭도 기억');
  await ctx.close();
});

// ── 진입점·딥링크 ──
await check('P05 entry points open the clips tab (old library route, home rail, library overview link, deep link)', async () => {
  const { ctx, page } = await openSpied();
  // 옛 경로 — 보관함 › 인기 클립
  await go(page, 'library', { tab: 'clips' });
  await page.waitForFunction(() => !document.querySelector('[data-page="popular"]').hidden);
  assert.equal(await selected(page), 'clips');
  assert.ok(await page.$eval('[data-page="library"]', (n) => n.hidden), '보관함은 숨음');
  assert.equal(await page.getAttribute('.sidebar .nav-item[data-route="popular"]', 'aria-current') ?? (await page.$eval('.sidebar .nav-item[data-route="popular"]', (n) => (n.classList.contains('active') ? 'page' : null))), 'page');
  // 홈 '오늘의 인기 클립' → 클립 탭 · 오늘
  await go(page, 'popular', { tab: 'live' });
  await go(page, 'home');
  const more = `[data-page="home"] [data-rail="clips"] [data-goto="popular"]`;
  await page.waitForSelector(more);
  await page.click(more);
  await page.waitForFunction(() => !document.querySelector('[data-page="popular"]').hidden);
  assert.equal(await selected(page), 'clips');
  await page.waitForFunction(() => window.__args.some(([c, a]) => c === 'popular_clips' && a.filter === 'WITHIN_1_DAY'));
  await page.waitForFunction((sel) => document.querySelector(`${sel} [data-clip-filter="WITHIN_1_DAY"]`)?.getAttribute('aria-pressed') === 'true', P);
  // 보관함 개요 바로가기
  await go(page, 'popular', { tab: 'live' });
  await go(page, 'library', { tab: 'overview' });
  await page.click('[data-page="library"] [data-goto-popular="clips"]');
  await page.waitForFunction(() => !document.querySelector('[data-page="popular"]').hidden);
  assert.equal(await selected(page), 'clips');
  // 딥링크 — Rust 가 보내는 페이로드(system.rs deep_link_payload)
  await go(page, 'popular', { tab: 'live' });
  await go(page, 'home');
  await page.evaluate(() => (window.__listeners['deeplink://open'] || []).forEach((fn) => fn({ payload: { kind: 'popular', tab: 'clips' } })));
  await page.waitForFunction(() => !document.querySelector('[data-page="popular"]').hidden);
  assert.equal(await selected(page), 'clips');
  await page.evaluate(() => (window.__listeners['deeplink://open'] || []).forEach((fn) => fn({ payload: { kind: 'popular', tab: null } })));
  assert.equal(await selected(page), 'clips', '탭 없는 딥링크는 마지막 탭');
  await ctx.close();
});

// ── 새로고침은 지금 탭·기간 그대로 ──
await check('P10 refresh (⌘R) keeps the tab and period the user changed to, not the ones it was opened with', async () => {
  const { ctx, page } = await openSpied();
  const refresh = () => page.evaluate(() => (window.__listeners['menu://action'] || []).forEach((fn) => fn({ payload: { id: 'refresh' } })));
  await go(page, 'popular', { tab: 'clips', period: 'WITHIN_1_DAY' });
  await page.waitForSelector(`${P} .pop-panel[data-panel="clips"] .clip-grid .card`);
  await page.click(`${P} [data-clip-filter="WITHIN_7_DAYS"]`);
  await page.waitForFunction((sel) => document.querySelector(`${sel} .page-subtitle`).textContent === '최근 7일 · 인기순', P);
  await refresh();
  await page.waitForTimeout(400);
  assert.equal((await state(page)).sub, '최근 7일 · 인기순', '새로고침이 기간을 되돌림');
  await page.click(`${P} [role="tab"][data-tab="live"]`);
  await settle(page);
  await refresh();
  await page.waitForTimeout(400);
  assert.equal(await selected(page), 'live', '새로고침이 탭을 되돌림');
  const ws = await page.evaluate(() => JSON.parse(localStorage.getItem('cview.workspace') || 'null'));
  assert.deepEqual(ws && ws.params, {}, `작업 공간에 연 때의 탭이 남음 ${JSON.stringify(ws)}`);
  await ctx.close();
});

await check('P11 focus inside the leaving panel moves to the selected tab; scroll is kept per tab', async () => {
  const { ctx, page } = await openSpied({ width: 1280, height: 700 });
  await go(page, 'popular', { tab: 'live' });
  await page.waitForSelector(`${P} .pop-panel[data-panel="live"] .grid .card`);
  await page.focus(`${P} .pop-panel[data-panel="live"] .card .card-open`);
  await page.evaluate((sel) => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: 'popular', params: { tab: 'clips' } } })), P);
  await page.waitForTimeout(50);
  assert.equal(await page.evaluate(() => document.activeElement?.dataset?.tab || document.activeElement?.tagName), 'clips', '초점이 body 로 떨어짐');
  await page.waitForSelector(`${P} .pop-panel[data-panel="clips"] .clip-grid .card`);
  await settle(page);
  const set = await page.evaluate((sel) => { const r = document.querySelector(sel); r.scrollTop = 300; return [r.scrollTop, r.scrollHeight, r.clientHeight]; }, P);
  assert.ok(set[0] > 100, `스크롤 못 함 ${set}`);
  // 탭 줄이 화면 밖이라 누르면 브라우저가 먼저 스크롤한다 — 팔레트·딥링크처럼 경로로 바꾼다
  await go(page, 'popular', { tab: 'live' });
  await settle(page);
  assert.equal(await page.evaluate((sel) => document.querySelector(sel).scrollTop, P), 0, '방송 탭이 클립 탭 스크롤로 열림');
  const trace = await page.evaluate(async (sel) => {
    const root = document.querySelector(sel); const out = [];
    document.querySelector(`${sel} [role="tab"][data-tab="clips"]`).click(); out.push(root.scrollTop);
    for (let i = 0; i < 12; i++) { await new Promise((r) => setTimeout(r, 50)); out.push(root.scrollTop); }
    return out;
  }, P);
  assert.equal(trace.at(-1), set[0], `클립 탭 스크롤을 잃음 ${trace}`);
  await ctx.close();
});

// ── 모션 줄이기 ──
await check('P06b reduced motion switches instantly (no overlapping panels)', async () => {
  const { ctx, page } = await openSpied({ reduced: true });
  await go(page, 'popular');
  await page.waitForSelector(`${P} .pop-panel[data-panel="live"] .grid .card`);
  await page.click(`${P} [role="tab"][data-tab="clips"]`);
  const s = await page.evaluate((sel) => ({ n: document.querySelectorAll(`${sel} .pop-panel.entering, ${sel} .pop-panel.leaving`).length, shown: [...document.querySelectorAll(`${sel} .pop-panel`)].filter((x) => !x.hidden).map((x) => x.dataset.panel) }), P);
  assert.deepEqual(s, { n: 0, shown: ['clips'] });
  await ctx.close();
});

// ── 보관함 정리 ──
await check('P08 library has no popular-clips tab/card; 3-card media group fills its rows at every width', async () => {
  const { ctx, page } = await openSpied();
  await go(page, 'library');
  await page.waitForFunction(() => !document.querySelector('[data-page="library"] .lib-ov-card[data-status="loading"]'));
  const tabs = await page.$$eval('[data-page="library"] [role="tab"]', (ts) => ts.map((t) => t.dataset.tab));
  assert.ok(!tabs.includes('clips'), tabs.join());
  assert.equal(await page.locator('[data-page="library"] [data-ov-section="clips"]').count(), 0);
  assert.ok(!(await page.textContent('[data-page="library"] .page-subtitle')).includes('클립'));
  for (const width of [1000, 1280, 1700, 2300]) {
    await page.setViewportSize({ width, height: 900 });
    await page.waitForTimeout(150);
    const rows = await page.$$eval('[data-page="library"] .lib-ov-grid', (gs) => gs.map((g) => {
      const cards = [...g.children].map((c) => c.getBoundingClientRect());
      const gr = g.getBoundingClientRect();
      const byRow = new Map();
      for (const r of cards) { const k = Math.round(r.top); const v = byRow.get(k) || { w: 0, n: 0 }; v.w += r.width; v.n += 1; byRow.set(k, v); }
      const gap = parseFloat(getComputedStyle(g).columnGap) || 0;
      return [...byRow.values()].map(({ w, n }) => gr.width - w - gap * (n - 1));
    }));
    for (const [gi, g] of rows.entries()) for (const left of g) assert.ok(left < 2, `폭 ${width} 묶음 ${gi}: 줄 끝 빈자리 ${left.toFixed(1)}px`);
  }
  await ctx.close();
});

// ── 좁은 창·테마 ──
for (const theme of ['light', 'dark']) {
  await check(`P09 ${theme} 1000×660 — no horizontal overflow, both tabs visible, none during transition`, async () => {
    const { ctx, page, errors } = await openSpied({ width: 1000, height: 660, theme });
    await go(page, 'popular', { tab: 'live' });
    await page.waitForSelector(`${P} .pop-panel[data-panel="live"] .grid .card`);
    const fits = () => page.evaluate((sel) => { const p = document.querySelector(sel); return p.scrollWidth - p.clientWidth; }, P);
    assert.ok(await fits() <= 0, `가로 넘침 ${await fits()}`);
    const tabsBox = await page.$$eval(`${P} [role="tab"]`, (ts) => ts.map((t) => { const r = t.getBoundingClientRect(); const l = t.parentElement.getBoundingClientRect(); return r.left >= l.left - 1 && r.right <= l.right + 1 && r.width > 40; }));
    assert.deepEqual(tabsBox, [true, true]);
    await page.click(`${P} [role="tab"][data-tab="clips"]`);
    let worst = 0;
    for (let i = 0; i < 8; i++) { worst = Math.max(worst, await fits()); await page.waitForTimeout(30); }
    assert.ok(worst <= 0, `전환 중 가로 넘침 ${worst}`);
    await page.waitForSelector(`${P} .pop-panel[data-panel="clips"] .clip-grid .card`);
    await settle(page);
    const cols = await page.$eval(`${P} .pop-clip-grid`, (g) => getComputedStyle(g).gridTemplateColumns.split(' ').length);
    assert.ok(cols >= 2, `클립 열 ${cols}`);
    const bar = await page.$eval(`${P} .pop-clip-toolbar`, (b) => b.scrollWidth <= b.clientWidth + 1);
    assert.ok(bar, '클립 도구 줄 넘침');
    await shot(page, `popular-clips-1000-${theme}`);
    assert.deepEqual(errors, []);
    await ctx.close();
  });
}

await browser.close();
server.close();
let failed = 0;
for (const r of results) { console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : `\n     ${r.error}`}`); if (!r.ok) failed++; }
console.log(`\n${results.length - failed}/${results.length} passed (${ENGINE})`);
process.exit(failed ? 1 : 0);
