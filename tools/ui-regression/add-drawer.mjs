// 채널 추가 사이드바(멀티라이브·멀티채팅 ml-add-drawer.js) UI 회귀 검사.
// R01 열린 서랍에 axe(WCAG 2.1 A·AA + best-practice) 위반이 없다 — 다크·라이트. 예전엔 <aside role="dialog"> 가 aria-allowed-role 위반.
// R02 남은 자리 칸 — 빈 칸이 서랍 바탕과 구별된다(다크에서 surface-overlay 가 바탕과 같아 사라졌다).
// R03 썸네일 위엔 시청자 수만(+19) — 방송 시간은 꼬리표 줄로 옮겨 "15,000"과 "1시간 11분"이 부딪히지 않는다.
// R04 줄 호버 — 줄 배경이 서랍 바탕과 달라지고 + 가 브랜드색을 띤다.
// R05 키보드 초점 + — 채움(primary)과 외곽선이 있어 호버 틴트와 구별된다.
// R06 추가한 줄 — "추가됨" 꼬리표, 글자는 흐리지 않는다(썸네일만), 방송 시간은 숨김.
// R07 프로필·썸네일이 없는 채널 — 빈 동그라미 대신 이름 첫 글자.
// R08 좁은 서랍(320) — 꼬리표 줄의 카테고리가 잘리지 않고 썸네일이 줄어든다.
// R09 남은 자리 문구는 '추가할 수'(멀티채팅도 같은 서랍), 불러오기 실패 때 바닥줄은 본문 문구를 되풀이하지 않는다.
// 저장소 루트에서: node tools/ui-regression/add-drawer.mjs   (Playwright·axe-core 가 필요하다)
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
const browser = await chromium.launch();

// 모의 IPC 덮어쓰기 — 멀티라이브 상태(added), 실패(fail), 프로필 없는 채널 추가
function patch(o) {
  const inv = window.__TAURI__.core.invoke;
  const sess = (id, i) => ({ id: `s${i}`, slot: i + 1, channelId: id, channelName: id, liveTitle: 't', viewerCountText: '1', openDate: new Date().toISOString(), isMuted: true, state: { kind: 'playing' } });
  const st = { sessions: (o.added || []).map(sess), layout: 'grid', maxSessions: 4, canAdd: (o.added || []).length < 4 };
  window.__TAURI__.core.invoke = async (cmd, args) => {
    if (cmd === 'multilive_state' || cmd === 'multilive_select') return JSON.parse(JSON.stringify(st));
    if (cmd === 'popular_lives' || cmd === 'following_lives') {
      if (o.fail) throw new Error('네트워크 연결을 확인해 주세요');
      const r = await inv(cmd, args);
      return cmd === 'popular_lives' ? r.concat([{ channelId: 'noimg', channelName: '프로필없음', liveTitle: '', isLive: true, viewerCount: 1, viewerCountText: '1' }]) : r;
    }
    return inv(cmd, args);
  };
}
async function openDrawer({ theme = 'dark', width = 1280, route = 'multilive', toggle = '[data-ml="add"]', ...o } = {}) {
  const app = await openApp(browser, { width, height: 820, theme });
  await app.page.evaluate(patch, o);
  await go(app.page, route);
  await app.page.waitForTimeout(500);
  await app.page.evaluate((t) => [...document.querySelectorAll(t)].find((b) => b.offsetParent).click(), toggle);
  await app.page.waitForSelector(o.fail ? '.ml-addbar.open .empty' : '.ml-addbar.open .ml-addrow');
  await app.page.waitForTimeout(800); // 열기 이동·줄 등장 모션이 끝나도록
  return app;
}
// rgb(a) 문자열을 바탕 위에 겹친 색의 상대 휘도로
const lum = ([r, g, b]) => { const f = (c) => { c /= 255; return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; }; return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b); };
const ratio = (a, b) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };
// 화면에 그려진 픽셀로 비교한다(color-mix·투명도 계산을 다시 하지 않게)
async function pixel(page, x, y) {
  const buf = await page.screenshot({ clip: { x, y, width: 1, height: 1 } });
  return page.evaluate(async (b64) => {
    const img = new Image(); img.src = `data:image/png;base64,${b64}`; await img.decode();
    const c = document.createElement('canvas'); c.width = 1; c.height = 1; const g = c.getContext('2d'); g.drawImage(img, 0, 0);
    return [...g.getImageData(0, 0, 1, 1).data.slice(0, 3)];
  }, buf.toString('base64'));
}
const center = (b) => [Math.round(b.x + b.width / 2), Math.round(b.y + b.height / 2)];

// ── R01 axe ──
for (const theme of ['dark', 'light']) {
  const { ctx, page } = await openDrawer({ theme, added: ['ch0'] });
  await check(`R01 ${theme}: 열린 채널 추가 서랍에 axe 위반 없음`, async () => {
    const tag = await page.evaluate(() => document.querySelector('.ml-addbar.open').tagName);
    assert.notEqual(tag, 'ASIDE', 'role=dialog 를 aside 에 달면 aria-allowed-role 위반');
    await page.evaluate(AXE);
    const bad = await page.evaluate(async () => (await axe.run(document.querySelector('.ml-addbar.open'), { runOnly: ['wcag2a', 'wcag2aa', 'best-practice'] }))
      .violations.map((v) => `${v.id}: ${v.nodes.slice(0, 2).map((n) => n.target.join(' ')).join(', ')}`));
    assert.deepEqual(bad, []);
  });
  await check(`R02 ${theme}: 빈 자리 칸이 서랍 바탕과 구별된다`, async () => {
    const box = await page.locator('.ml-addbar-slots i:not(.on)').first().boundingBox();
    const bg = await pixel(page, Math.round(box.x + box.width / 2), Math.round(box.y - 4));
    const slot = await pixel(page, ...center(box));
    assert.ok(ratio(slot, bg) >= 1.25, `빈 칸 ${slot} vs 바탕 ${bg} = ${ratio(slot, bg).toFixed(2)}:1`);
  });
  await check(`R04 ${theme}: 줄 호버 — 줄 배경이 바뀌고 + 가 브랜드색을 띤다`, async () => {
    const row = page.locator('.ml-addrow:not(.added)').nth(1);
    const act = row.locator('.ml-addrow-act');
    const rb = await row.boundingBox();
    const spot = [Math.round(rb.x + rb.width - 50), Math.round(rb.y + 6)];
    const before = await pixel(page, ...spot);
    const actBefore = await act.evaluate((e) => getComputedStyle(e).color);
    await row.hover();
    await page.waitForTimeout(300);
    const after = await pixel(page, ...spot);
    assert.ok(ratio(before, after) >= 1.08, `줄 배경 ${before} → ${after} = ${ratio(before, after).toFixed(2)}:1`);
    const actAfter = await act.evaluate((e) => getComputedStyle(e).color);
    const primary = await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--primary-text').trim());
    assert.notEqual(actBefore, actAfter, '+ 색이 그대로');
    const probe = await page.evaluate((c) => { const s = document.createElement('i'); s.style.color = c; document.body.append(s); const v = getComputedStyle(s).color; s.remove(); return v; }, primary);
    assert.equal(actAfter, probe, '+ 가 --primary-text 가 아니다');
    await page.mouse.move(5, 5);
  });
  await check(`R05 ${theme}: 키보드 초점 + 는 채움과 외곽선으로 호버와 구별된다`, async () => {
    await page.focus('.ml-addbar-search input');
    await page.keyboard.press('ArrowDown');
    await page.keyboard.press('ArrowDown');
    await page.keyboard.press('ArrowRight');
    await page.waitForTimeout(300);
    const s = await page.evaluate(() => {
      const a = document.activeElement; const cs = getComputedStyle(a);
      const probe = document.createElement('i'); probe.style.color = 'var(--primary)'; a.append(probe); const primary = getComputedStyle(probe).color; probe.remove();
      return { cls: a.className, bg: cs.backgroundColor, outline: cs.outlineStyle, ow: cs.outlineWidth, primary };
    });
    assert.equal(s.cls, 'ml-addrow-act');
    assert.equal(s.bg, s.primary, `초점 + 배경 ${s.bg}`);
    assert.ok(s.outline !== 'none' && parseFloat(s.ow) >= 2, `외곽선 ${s.outline} ${s.ow}`);
  });
  await ctx.close();
}

// ── R03·R06·R07 줄 내용 ──
{
  const { ctx, page } = await openDrawer({ added: ['ch0'] });
  await check('R03 썸네일 위엔 시청자 수만 — 방송 시간은 꼬리표 줄에, 겹침 없음', async () => {
    const r = await page.evaluate(() => [...document.querySelectorAll('.ml-addrow:not(.added):not([data-channel-id="noimg"])')].map((row) => {
      const thumb = row.querySelector('.ml-addrow-thumb');
      const up = row.querySelector('.ml-addrow-uptime');
      const v = row.querySelector('.ml-addrow-viewers').getBoundingClientRect(); const t = thumb.getBoundingClientRect();
      return { inThumb: Boolean(thumb.querySelector('.ml-addrow-uptime')), inMeta: Boolean(up?.closest('.ml-addrow-meta')), viewersInside: v.right <= t.right && v.left >= t.left };
    }));
    assert.ok(r.length >= 5);
    for (const x of r) { assert.ok(!x.inThumb, '썸네일 위에 방송 시간'); assert.ok(x.inMeta, '꼬리표 줄에 방송 시간 없음'); assert.ok(x.viewersInside); }
  });
  await check('R06 추가한 줄 — "추가됨" 꼬리표, 글자는 흐리지 않고 방송 시간은 숨김', async () => {
    const r = await page.evaluate(() => {
      const row = document.querySelector('.ml-addrow.added');
      const text = row.querySelector('.ml-addrow-text');
      return { chip: row.querySelector('.ml-addrow-added')?.textContent, op: getComputedStyle(text).opacity, up: row.querySelector('.ml-addrow-uptime') && getComputedStyle(row.querySelector('.ml-addrow-uptime')).display, label: row.querySelector('.ml-addrow-act').getAttribute('aria-label') };
    });
    assert.equal(r.chip, '추가됨');
    assert.equal(r.op, '1');
    assert.ok(!r.up || r.up === 'none', `방송 시간 ${r.up}`);
    assert.match(r.label, /이미 추가/);
  });
  await check('R07 프로필·썸네일 없는 채널은 이름 첫 글자', async () => {
    const r = await page.evaluate(() => {
      const row = document.querySelector('.ml-addrow[data-channel-id="noimg"]');
      return [...row.querySelectorAll('.ml-addrow-initial')].map((e) => ({ t: e.textContent, w: e.getBoundingClientRect().width }));
    });
    assert.equal(r.length, 2, JSON.stringify(r));
    for (const x of r) { assert.equal(x.t, '프'); assert.ok(x.w >= 16); }
  });
  await ctx.close();
}

// ── R08 좁은 서랍 ──
{
  const { ctx, page } = await openDrawer({ route: 'multichat', toggle: '[data-mc="add"]' });
  await check('R08 좁은 서랍(320) — 카테고리가 잘리지 않고 썸네일이 줄어든다', async () => {
    const r = await page.evaluate(() => ({
      w: Math.round(document.querySelector('.ml-addbar.open').getBoundingClientRect().width),
      thumb: Math.round(document.querySelector('.ml-addbar.open .ml-addrow-thumb').getBoundingClientRect().width),
      cut: [...document.querySelectorAll('.ml-addbar.open .ml-addrow-cat')].filter((e) => e.scrollWidth > e.clientWidth + 1).map((e) => e.textContent),
    }));
    assert.ok(r.w <= 330, `서랍 폭 ${r.w}`);
    assert.ok(r.thumb < 116, `썸네일 ${r.thumb}`);
    assert.deepEqual(r.cut, []);
  });
  await check('R09 멀티채팅 남은 자리 문구는 "추가할 수"', async () => {
    const t = await page.textContent('.ml-addbar.open .ml-addbar-count');
    assert.match(t, /더 추가할 수 있어요/);
  });
  await ctx.close();
}
{
  const { ctx, page } = await openDrawer({ fail: true });
  await check('R09 불러오기 실패 — 바닥줄은 본문 오류 문구를 되풀이하지 않는다', async () => {
    const r = await page.evaluate(() => ({ body: document.querySelector('.ml-addbar.open .ml-addbar-list .empty span')?.textContent, foot: document.querySelector('.ml-addbar.open .ml-addbar-status').textContent }));
    assert.ok(r.body);
    assert.notEqual(r.foot, r.body);
  });
  await ctx.close();
}
{
  // 큰 서랍(340)은 썸네일을 줄이지 않는다
  const { ctx, page } = await openDrawer({});
  await check('R08 기본 서랍(340)은 썸네일 116', async () => {
    const w = await page.evaluate(() => Math.round(document.querySelector('.ml-addrow-thumb').getBoundingClientRect().width));
    assert.equal(w, 116);
  });
  await ctx.close();
}

await browser.close();
server.close();
let failed = 0;
for (const r of results) { console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : `\n     ${r.error}`}`); if (!r.ok) failed++; }
console.log(`\n${results.length - failed}/${results.length} passed`);
process.exit(failed ? 1 : 0);
