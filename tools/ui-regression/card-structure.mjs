// 카드 구조 회귀 검사(UI 리뷰 P3-2) — 초점을 받는 칸 안에 단추가 중첩되지 않고, 카드의 주 동작은 진짜 단추다.
// 예전 라이브 카드는 `<article class="card" tabindex="0">` 자체가 초점을 받고 그 안에 +(멀티라이브 추가) 단추와 클릭되는
// `span[data-open-channel]`(채널 정보)이 들어 있었다 — 화면 낭독기·키보드에 무엇을 누르는지 모호했다(axe 4.10 은 못 잡는다).
// 이제 카드는 초점을 받지 않고, 카드 전체를 덮는 주 동작 단추(.card-open, 이름 = 방송 제목, 채널 이름)와 보조 단추(+·아바타)가 형제다.
// CS01 홈·팔로잉(격자·간결·전체 채널)·인기·검색 — 초점을 받는 요소 안에 대화형 요소가 없고, +·채널 정보는 진짜 <button>.
// CS02 접근 가능한 이름 — 주 동작은 '방송 제목, 채널 이름', +·아바타는 각자 이름. 카드 안 Tab 순서는 주 동작 → + → 아바타.
// CS03 Enter·Space 로 카드를 한 번만 연다(재생 시작 두 번 없음) — 홈 격자·히어로·TOP 10·간결 보기 줄·검색 채널 줄.
// CS04 카드 안 +·아바타는 Enter·Space·클릭 모두 각자 동작하고 카드 열기로 새지 않는다.
// CS05 화살표 — 격자(좌우·아래)·TOP 10 선반·간결 보기 목록에서 다음 카드의 주 동작으로 초점이 옮겨 간다.
// CS06 마우스 — 썸네일을 누르면 한 번 열리고, 주 동작 단추는 마우스를 가로채지 않는다(호버 미리보기·우클릭 메뉴가 카드에서 그대로).
//      키보드 메뉴(초점에서 contextmenu)도 카드 메뉴를 연다.
// CS07 초점 고리 — 주 동작에 초점이면 카드(간결 보기 줄은 안쪽 -2px)에 예전과 같은 외곽선이 그려지고 단추 자체엔 없다.
// CS08 히어로가 자동으로 다음 장으로 넘어가도 키보드 초점이 문서 처음으로 떨어지지 않고 새 장의 주 동작으로 간다.
// 저장소 루트에서: node tools/ui-regression/card-structure.mjs   (Playwright 가 필요하다)
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, go, openApp } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message.split('\n').slice(0, 6).join('\n') }); }
}
const browser = await chromium.launch();

const HOME_CARD = '[data-page="home"] .popular-grid .card';

/** 앱을 열고 홈 카드가 그려질 때까지. 카드 열기·멀티라이브 추가·채널 정보를 세는 계수기를 단다. */
async function open() {
  const app = await openApp(browser, { width: 1280, height: 900 });
  const { page } = app;
  await page.waitForSelector(HOME_CARD);
  await page.evaluate(() => {
    // 카드 열기(bindCardOpen)는 최근 시청에 적는다 — 그 쓰기 수가 연 횟수다.
    const set = Storage.prototype.setItem;
    window.__opens = 0;
    Storage.prototype.setItem = function (k, v) { if (k === 'cview.recentChannels') window.__opens += 1; return set.call(this, k, v); };
    window.__channelPage = 0;
    window.addEventListener('cview:open-channel', () => { window.__channelPage += 1; });
  });
  return app;
}
/** 지금까지 센 값을 돌려주고 0 으로. mlAdd 는 멀티라이브 추가 IPC(카드 열기는 live: true 로, + 는 그냥) 수. */
async function take(page) {
  return page.evaluate(() => {
    const r = { opens: window.__opens, channelPage: window.__channelPage, mlAdd: window.__calls.filter((c) => c === 'multilive_add').length };
    window.__opens = 0; window.__channelPage = 0; window.__calls.length = 0;
    return r;
  });
}
/** 잠깐 기다려 비동기 열기·두 번째 열기(있다면)가 끝나게. */
const settle = (page) => page.waitForTimeout(250);

async function focusIn(page, sel) {
  await page.keyboard.press('Shift'); // 마지막 입력이 키보드 — :focus-visible
  await page.evaluate((s) => { const el = document.querySelector(s); el.scrollIntoView({ block: 'center' }); el.focus(); }, sel);
}
const active = (page) => page.evaluate(() => {
  const a = document.activeElement;
  const card = a && a.closest('[data-channel-id], [data-open-channel-id]');
  return { cls: a ? a.className : '', id: card ? (card.dataset.channelId || card.dataset.openChannelId) : '' };
});

// 화면 안에서 초점을 받는 요소가 대화형 요소를 품는지, 클릭 동작 표지(채널 정보·+)가 진짜 단추인지.
function auditNesting(scope) {
  const root = document.querySelector(scope);
  const FOCUSABLE = 'a[href], button, [tabindex]:not([tabindex="-1"]), [role="button"], [role="link"]';
  const INTERACTIVE = 'a[href], button, input, select, textarea, [tabindex]:not([tabindex="-1"]), [role="button"], [role="link"], [data-open-channel], [data-ml-add]';
  const name = (el) => `${el.tagName.toLowerCase()}.${String(el.className).trim().split(/\s+/).join('.')}`;
  const nested = [];
  for (const el of root.querySelectorAll(FOCUSABLE)) {
    const inner = el.querySelector(INTERACTIVE);
    if (inner) nested.push(`${name(el)} > ${name(inner)}`);
  }
  const fake = [...root.querySelectorAll('[data-open-channel], [data-ml-add]')].filter((el) => el.tagName !== 'BUTTON').map(name);
  // 단추 자체가 동작인 줄(최근 시청)은 카드가 아니다. 홈 "방송 중인 팔로잉" 타일(live-following.js)은 주 동작이
  // 글자를 품은 진짜 단추(.lf-open)이고 + 가 형제 단추다 — 카드와 같은 구조라 그 단추를 주 동작으로 본다.
  const cards = [...root.querySelectorAll('[data-channel-id]:not(button), .dense-row[data-open-channel-id]')];
  const noOpen = cards.filter((c) => !c.querySelector(':scope > button.card-open, :scope > button.lf-open')).map(name);
  return { nested: [...new Set(nested)], fake: [...new Set(fake)], noOpen: [...new Set(noOpen)], cards: cards.length };
}

await check('CS01 no interactive element nested in a focusable one; card actions are real buttons (home, following ×3, popular, search)', async () => {
  const { ctx, page, errors } = await open();
  const report = {};
  const audit = async (label, scope) => { report[label] = await page.evaluate(auditNesting, scope); };
  await audit('home', '[data-page="home"]');
  await go(page, 'following');
  await page.waitForSelector('[data-page="following"] .grid .card');
  await audit('following-grid', '[data-page="following"]');
  await page.click('[data-page="following"] [data-dense]');
  await page.waitForSelector('[data-page="following"] .dense-row');
  await audit('following-dense', '[data-page="following"]');
  await page.click('[data-page="following"] [data-ftab="all"]');
  await page.waitForSelector('[data-page="following"] .follow-row');
  await audit('following-all', '[data-page="following"]');
  await go(page, 'popular');
  await page.waitForSelector('[data-page="popular"] .grid .card');
  await audit('popular', '[data-page="popular"]');
  await go(page, 'search', { query: '릴카' });
  await page.waitForSelector('[data-page="search"] .channel-row');
  await audit('search', '[data-page="search"]');
  for (const [label, r] of Object.entries(report)) {
    assert.ok(r.cards > 0, `${label}: 카드가 없다`);
    assert.deepEqual(r.nested, [], `${label}: 초점 요소 안에 대화형 요소`);
    assert.deepEqual(r.fake, [], `${label}: 단추가 아닌 클릭 표지`);
    assert.deepEqual(r.noOpen, [], `${label}: 주 동작 단추가 없는 카드`);
  }
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('CS02 accessible names: main action = "title, channel"; + and avatar named; tab order open → + → avatar → next card', async () => {
  const { ctx, page, errors } = await open();
  const first = await page.evaluate((s) => {
    const c = document.querySelector(s);
    return { title: c.querySelector('.card-title').textContent.trim(), name: c.dataset.channelName, id: c.dataset.channelId };
  }, HOME_CARD);
  const card = page.locator(HOME_CARD).first();
  assert.equal(await card.getByRole('button', { name: `${first.title}, ${first.name}`, exact: true }).count(), 1, '주 동작 이름');
  assert.equal(await card.getByRole('button', { name: '멀티라이브에 추가', exact: true }).count(), 1, '+ 이름');
  assert.equal(await card.getByRole('button', { name: `${first.name} 채널 정보`, exact: true }).count(), 1, '아바타 이름');
  // 히어로·TOP 10 도 같은 이름 규칙
  const hero = await page.evaluate(() => {
    const h = document.querySelector('[data-page="home"] .hero-main');
    return { label: h.querySelector(':scope > .card-open')?.getAttribute('aria-label'), title: h.querySelector('.hero-title').textContent.trim(), name: h.dataset.channelName };
  });
  assert.equal(hero.label, `${hero.title}, ${hero.name}`, '히어로 이름');
  const top = await page.evaluate(() => {
    const t = document.querySelector('[data-page="home"] .top-item');
    return { label: t.querySelector(':scope > .card-open')?.getAttribute('aria-label'), title: t.querySelector('.top-title').textContent.trim(), name: t.dataset.channelName };
  });
  assert.equal(top.label, `${top.title}, ${top.name}`, 'TOP 10 이름');
  // 카드 안 Tab 순서
  await focusIn(page, `${HOME_CARD}:first-child > .card-open`);
  const order = [await active(page)];
  for (let i = 0; i < 3; i += 1) { await page.keyboard.press('Tab'); order.push(await active(page)); }
  assert.deepEqual(order.map((o) => o.cls), ['card-open', 'card-ml-add', 'card-avatar', 'card-open'], `Tab 순서 ${JSON.stringify(order)}`);
  assert.equal(order[2].id, first.id);
  assert.notEqual(order[3].id, first.id, '다음 카드로 넘어가지 않음');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('CS03 Enter and Space open a card exactly once (home grid, hero, top 10, dense row, search channel row)', async () => {
  const { ctx, page, errors } = await open();
  const press = async (label, sel, key) => {
    await focusIn(page, sel);
    await take(page);
    await page.keyboard.press(key);
    await settle(page);
    const r = await take(page);
    assert.deepEqual({ opens: r.opens, mlAdd: r.mlAdd, channelPage: r.channelPage }, { opens: 1, mlAdd: 1, channelPage: 0 }, `${label} ${key}`);
  };
  for (const key of ['Enter', 'Space']) {
    await go(page, 'home');
    await page.waitForSelector(HOME_CARD);
    await press('home grid', `${HOME_CARD}:nth-child(2) > .card-open`, key);
    await go(page, 'home');
    await press('hero', '[data-page="home"] .hero-main > .card-open', key);
    await go(page, 'home');
    await press('top 10', '[data-page="home"] .top-item > .card-open', key);
  }
  await go(page, 'following');
  await page.waitForSelector('[data-page="following"] .grid .card');
  await page.click('[data-page="following"] [data-dense]');
  await page.waitForSelector('[data-page="following"] .dense-row');
  await press('dense row', '[data-page="following"] .dense-row > .card-open', 'Enter');
  // 검색 채널 줄 — 방송 중이면 방송, 아니면 채널 정보를 한 번
  await go(page, 'search', { query: '릴카' });
  await page.waitForSelector('[data-page="search"] .channel-row');
  await press('live channel row', '[data-page="search"] .channel-row.is-live > .card-open', 'Space');
  await focusIn(page, '[data-page="search"] .channel-row:not(.is-live) > .card-open');
  await take(page);
  await page.keyboard.press('Enter');
  await settle(page);
  assert.deepEqual(await take(page), { opens: 0, channelPage: 1, mlAdd: 0 }, 'offline channel row Enter');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('CS04 + and avatar act on their own (Enter, Space, click) and never open the card', async () => {
  const { ctx, page, errors } = await open();
  const card = `${HOME_CARD}:nth-child(2)`;
  for (const how of ['Enter', 'Space', 'click']) {
    await go(page, 'home');
    await page.waitForSelector(card);
    // +
    await take(page);
    if (how === 'click') { await page.hover(card); await page.click(`${card} .card-ml-add`); } else { await focusIn(page, `${card} .card-ml-add`); await page.keyboard.press(how); }
    await settle(page);
    assert.deepEqual(await take(page), { opens: 0, channelPage: 0, mlAdd: 1 }, `+ ${how}`);
    // 아바타(채널 정보)
    await go(page, 'home');
    await page.waitForSelector(card);
    await take(page);
    if (how === 'click') await page.click(`${card} .card-avatar`); else { await focusIn(page, `${card} .card-avatar`); await page.keyboard.press(how); }
    await settle(page);
    assert.deepEqual(await take(page), { opens: 0, channelPage: 1, mlAdd: 0 }, `avatar ${how}`);
  }
  // 히어로의 '멀티라이브'와 TOP 10 의 + 도
  await go(page, 'home');
  await page.waitForSelector(HOME_CARD);
  for (const sel of ['[data-page="home"] .hero-main [data-ml-add]', '[data-page="home"] .top-item [data-ml-add]']) {
    await go(page, 'home');
    await focusIn(page, sel);
    await take(page);
    await page.keyboard.press('Enter');
    await settle(page);
    assert.deepEqual(await take(page), { opens: 0, channelPage: 0, mlAdd: 1 }, sel);
  }
  // 간결 보기 줄의 아바타
  await go(page, 'following');
  await page.waitForSelector('[data-page="following"] .grid .card');
  await page.click('[data-page="following"] [data-dense]');
  await page.waitForSelector('[data-page="following"] .dense-row');
  await focusIn(page, '[data-page="following"] .dense-row .card-avatar');
  await take(page);
  await page.keyboard.press('Space');
  await settle(page);
  assert.deepEqual(await take(page), { opens: 0, channelPage: 1, mlAdd: 0 }, 'dense avatar Space');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('CS05 arrow keys move focus to the next card\'s main action (grid, top 10 shelf, dense list)', async () => {
  const { ctx, page, errors } = await open();
  // 격자 — 인기 화면(홈 격자는 모의 데이터에서 2장뿐)
  const GRID = '[data-page="popular"] .grid .card';
  await go(page, 'popular');
  await page.waitForSelector(GRID);
  // 한 줄 카드 수는 레이아웃 위치(offsetTop)로 센다. 격자가 나타나자마자 카드마다 늦춤이 다른 등장 애니메이션(motion.css mo-rise,
  // translate·scale)이 돌아 getBoundingClientRect().top 은 같은 줄이라도 카드마다 달랐다 — 부하가 걸리면 '한 줄 카드 1' 로 실패.
  // offsetTop 은 transform·translate·scale 의 영향을 받지 않는다(카드는 모두 같은 격자 안이라 offsetParent 도 같다).
  const ids = await page.$$eval(GRID, (cs) => cs.map((c) => [c.dataset.channelId, c.offsetTop]));
  const perRow = ids.filter(([, t]) => t === ids[0][1]).length;
  assert.ok(perRow >= 2, `한 줄 카드 ${perRow}`);
  await focusIn(page, `${GRID}:nth-child(1) > .card-open`);
  await page.keyboard.press('ArrowRight');
  assert.deepEqual(await active(page), { cls: 'card-open', id: ids[1][0] }, 'ArrowRight');
  await page.keyboard.press('ArrowDown');
  assert.deepEqual(await active(page), { cls: 'card-open', id: ids[1 + perRow][0] }, 'ArrowDown');
  await page.keyboard.press('ArrowLeft');
  assert.deepEqual(await active(page), { cls: 'card-open', id: ids[perRow][0] }, 'ArrowLeft');
  await page.keyboard.press('ArrowUp');
  assert.deepEqual(await active(page), { cls: 'card-open', id: ids[0][0] }, 'ArrowUp');
  // + 에 초점이 있어도 화살표는 다음 카드로(예전 카드 안 + 와 같다)
  await focusIn(page, `${GRID}:nth-child(1) .card-ml-add`);
  await page.keyboard.press('ArrowRight');
  assert.deepEqual(await active(page), { cls: 'card-open', id: ids[1][0] }, '+ 에서 ArrowRight');
  // TOP 10 선반
  await go(page, 'home');
  await page.waitForSelector(HOME_CARD);
  const tops = await page.$$eval('[data-page="home"] .top-item', (ts) => ts.map((t) => t.dataset.channelId));
  await focusIn(page, '[data-page="home"] .top-item > .card-open');
  await page.keyboard.press('ArrowRight');
  assert.deepEqual(await active(page), { cls: 'card-open', id: tops[1] }, 'TOP 10 ArrowRight');
  // 간결 보기 목록
  await go(page, 'following');
  await page.waitForSelector('[data-page="following"] .grid .card');
  await page.click('[data-page="following"] [data-dense]');
  await page.waitForSelector('[data-page="following"] .dense-row');
  const rows = await page.$$eval('[data-page="following"] .dense-row', (rs) => rs.map((r) => r.dataset.channelId));
  await focusIn(page, '[data-page="following"] .dense-row > .card-open');
  await page.keyboard.press('ArrowDown');
  assert.deepEqual(await active(page), { cls: 'card-open', id: rows[1] }, 'dense ArrowDown');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('CS06 mouse: thumbnail click opens once; main action lets the pointer through (hover preview, right-click menu); keyboard menu', async () => {
  const { ctx, page, errors } = await open();
  const card = `${HOME_CARD}:nth-child(2)`;
  const hit = await page.evaluate((s) => {
    const c = document.querySelector(s);
    c.scrollIntoView({ block: 'center' });
    const r = c.querySelector('.thumb').getBoundingClientRect();
    const el = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
    return { cls: el.className, inThumb: Boolean(el.closest('.thumb[data-preview]')), pe: getComputedStyle(c.querySelector('.card-open')).pointerEvents };
  }, card);
  assert.equal(hit.inThumb, true, `썸네일 가운데를 가로챈 요소 ${hit.cls}`);
  assert.equal(hit.pe, 'none');
  await take(page);
  await page.click(`${card} .thumb`);
  await settle(page);
  assert.deepEqual(await take(page), { opens: 1, channelPage: 0, mlAdd: 1 }, 'thumb click');
  // 우클릭 메뉴
  await go(page, 'home');
  await page.waitForSelector(card);
  await page.click(`${card} .card-title`, { button: 'right' });
  await page.waitForSelector('.ctx-menu');
  assert.ok((await page.textContent('.ctx-menu')).includes('멀티라이브에 추가'), '카드 메뉴');
  await page.keyboard.press('Escape');
  await page.waitForSelector('.ctx-menu', { state: 'detached' });
  // 키보드 메뉴 — 초점(주 동작 단추)에서 contextmenu
  await focusIn(page, `${card} > .card-open`);
  await page.evaluate(() => { const a = document.activeElement; const r = a.getBoundingClientRect(); a.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: r.left + 10, clientY: r.top + 10 })); });
  await page.waitForSelector('.ctx-menu');
  await page.keyboard.press('Escape');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('CS07 focus ring is drawn on the card (same outline as before), not on the main-action button', async () => {
  const { ctx, page, errors } = await open();
  const ring = async (cardSel) => {
    await focusIn(page, `${cardSel} > .card-open`);
    await page.waitForTimeout(350); // 고리가 조여 드는 애니메이션(.24s)이 끝난 뒤
    return page.evaluate((s) => {
      const c = document.querySelector(s);
      const cs = getComputedStyle(c);
      const bs = getComputedStyle(c.querySelector(':scope > .card-open'));
      return { card: `${cs.outlineStyle} ${cs.outlineWidth} ${cs.outlineOffset}`, button: bs.outlineStyle };
    }, cardSel);
  };
  assert.deepEqual(await ring(`${HOME_CARD}:nth-child(2)`), { card: 'solid 2px 3px', button: 'none' }, 'grid card');
  assert.deepEqual(await ring('[data-page="home"] .hero-main'), { card: 'solid 2px 3px', button: 'none' }, 'hero');
  assert.deepEqual(await ring('[data-page="home"] .top-item'), { card: 'solid 2px 3px', button: 'none' }, 'top 10');
  await go(page, 'following');
  await page.waitForSelector('[data-page="following"] .grid .card');
  await page.click('[data-page="following"] [data-dense]');
  await page.waitForSelector('[data-page="following"] .dense-row');
  assert.deepEqual(await ring('[data-page="following"] .dense-row'), { card: 'solid 2px -2px', button: 'none' }, 'dense row');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('CS08 hero auto-advance keeps keyboard focus on the new slide\'s main action', async () => {
  const { ctx, page, errors } = await open();
  await focusIn(page, '[data-page="home"] .hero-main > .card-open');
  const before = await active(page);
  await page.evaluate(() => document.querySelector('[data-page="home"] .queue-item.active .queue-progress')
    .dispatchEvent(new AnimationEvent('animationend', { animationName: 'queue-progress', bubbles: true })));
  const after = await active(page);
  assert.equal(after.cls, 'card-open', `초점 ${JSON.stringify(after)}`);
  assert.notEqual(after.id, before.id, '다음 장으로 넘어가지 않음');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await browser.close();
server.close();
let failed = 0;
for (const r of results) {
  console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : `\n     ${r.error}`}`);
  if (!r.ok) failed += 1;
}
console.log(`\n${results.length - failed}/${results.length} passed`);
process.exit(failed ? 1 : 0);
