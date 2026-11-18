// 화면별 정밀 다듬기 회귀 검사 — 글자 잘림·겹침·넘침과 초점·그림자 결함(모의 IPC, 헤드리스 브라우저).
// FP01 화면 밖에 숨은 '본문 바로가기' 링크는 그림자를 그리지 않는다(창 위 가장자리로 번져 사이드바 머리가 얼룩졌다). 초점을 받으면 그림자.
// FP02 시트 — 열 때 초점을 받는 대화 상자 자체에는 초점 테두리가 없고, 다음 Tab 은 시트 안 단추로 가서 그 단추에 테두리가 선다.
// FP03 시청 리포트 KPI — 1280(사이드바·인사이트 칸)·900 × 라이트·다크에서 값이 잘리지 않는다(말줄임 없음), 단위는 .kpi-unit 로 작게.
//      글자(textContent)는 그대로('14시간 26분').
// FP04 라이브 시청 정보 타일 — 1280·900 에서 값이 타일 밖으로 넘치지 않고, 4개는 3+1 로 남지 않는다(한 줄 또는 2×2).
// FP05 멀티채팅 머리줄 — 1280(인사이트 칸 있음)에서 제목과 검색칸이 겹치지 않고 제목이 다 보인다. 검색칸이 다음 줄이면
//      '모두 닫기'가 같은 줄 옆에 있다(혼자 한 줄을 차지하지 않는다). 넓은 화면(인사이트 칸 접음)에서는 한 줄 그대로.
// FP06 채팅 보내기 단추 — 보낼 수 없을 때(비활성) 반투명이 아닌 회색 면(opacity 1, 스킨의 --cs-fill).
// 저장소 루트에서: node tools/ui-regression/fit-polish.mjs   (Playwright 가 필요하다.
//   CVIEW_UI_BROWSER=webkit 이면 앱(macOS)과 같은 WebKit 엔진으로 돈다.)
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, base, mockTauri, go } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const WEBKIT = process.env.CVIEW_UI_BROWSER === 'webkit';
const engine = WEBKIT ? pw.webkit : pw.chromium;
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message.split('\n').slice(0, 3).join('\n') }); }
}

// 방송 중 채널·멀티라이브 세션(라이브 시청 화면) — live-channel-motion.mjs 와 같은 모양
function liveMock() {
  const open = new Date(Date.now() - 5400e3).toISOString();
  const live = { liveTitle: '오늘은 신작 게임 정주행', categoryName: '마인크래프트', viewerCount: 12400, viewerCountText: '12,400', accumulateCount: 48210, openDate: open, tags: ['게임', '소통'] };
  const channel = { channelId: 'ch1', channelName: '한동숙', channelImageUrl: `${location.origin}/__img/avatar/1.svg`, verifiedMark: true, followerCount: 523000, channelDescription: '매일 저녁 8시 방송합니다.' };
  const session = (id, i, name) => ({ id: `s${i}`, slot: i, channelId: id, channelName: name, liveTitle: live.liveTitle, profileImageUrl: null, viewerCountText: '1만',
    accumulateCount: 0, accumulateCountText: '', openDate: open, isMuted: i > 0, isAudible: i === 0, isAudioOnly: false, state: { kind: 'playing' }, chatChannelId: null, maxHeightCap: 0, decodeSuspended: false });
  const state = { sessions: [session('ch1', 0, '한동숙'), session('ch2', 1, '침착맨')], selectedId: 's0', focusMainId: 's0', canAdd: true, maxSessions: 4,
    layout: 'grid', focusDirection: 'left', focusRatio: 0.7, horizontalRatio: 0.5, verticalRatio: 0.5, multiAudio: false, watchId: null, rev: 1 };
  const over = {
    channel_detail: () => ({ channel: { ...channel }, live: { ...live }, isFavorite: false }), channel_info: () => ({ verified: true, followerCount: 523000 }),
    live_detail: () => ({ ...live, verified: true }), multilive_state: () => state, multilive_stage_size: () => null,
    multilive_set_layout: ({ layout }) => { state.layout = layout; state.rev++; return state; }, multilive_set_focus_layout: () => state,
    multilive_watch: ({ sessionId }) => { state.watchId = sessionId; state.rev++; return state; },
  };
  const orig = window.__TAURI__.core.invoke;
  window.__TAURI__.core.invoke = async (cmd, args) => {
    if (over[cmd]) return over[cmd](args || {});
    try { return await orig(cmd, args); } catch (e) { if (/^(multilive_|chat_|player_)/.test(cmd)) return null; throw e; }
  };
}

const browser = await engine.launch();
async function open({ width = 1280, height = 820, theme = 'dark' } = {}) {
  const ctx = await browser.newContext({ viewport: { width, height }, colorScheme: theme, reducedMotion: 'reduce' });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, theme]);
  await page.addInitScript(liveMock);
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.waitForFunction(() => { const s = document.getElementById('splash'); return !s || !s.isConnected || getComputedStyle(s).display === 'none'; }, null, { timeout: 8000 }).catch(() => {});
  return { ctx, page, errors };
}
const box = (page, sel) => page.$eval(sel, (n) => { const r = n.getBoundingClientRect(); return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width, height: r.height }; });

await check('FP01 hidden skip link casts no shadow into the window; focused link has one', async () => {
  const { ctx, page } = await open({ theme: 'light' });
  const idle = await page.$eval('.skip-link', (n) => ({ bottom: n.getBoundingClientRect().bottom, shadow: getComputedStyle(n).boxShadow }));
  assert.ok(idle.bottom <= 0, `숨은 링크는 창 위 밖 (bottom ${idle.bottom})`);
  assert.equal(idle.shadow, 'none', '숨어 있을 때 그림자 없음');
  await page.focus('.skip-link');
  assert.notEqual(await page.$eval('.skip-link', (n) => getComputedStyle(n).boxShadow), 'none', '나타나면 그림자');
  await ctx.close();
});

await check('FP02 sheet dialog itself has no focus outline; next Tab rings the button inside', async () => {
  const { ctx, page } = await open();
  await page.keyboard.press('Tab'); // 키보드 모드(:focus-visible)로 — 단축키로 여는 것과 같다
  await page.evaluate(async () => { const m = await import('/js/app-sheets.js'); m.showShortcuts(); });
  await page.waitForSelector('.app-sheet .sheet');
  const s = await page.$eval('.app-sheet .sheet', (n) => ({ focused: document.activeElement === n, outline: getComputedStyle(n).outlineStyle }));
  assert.equal(s.focused, true, '열면 대화 상자가 초점');
  assert.equal(s.outline, 'none', '대화 상자 자체에는 테두리 없음');
  await page.keyboard.press('Tab');
  const b = await page.evaluate(() => { const a = document.activeElement; return { inSheet: Boolean(a.closest('.sheet')), outline: getComputedStyle(a).outlineStyle }; });
  assert.equal(b.inSheet, true, 'Tab 은 시트 안으로');
  assert.notEqual(b.outline, 'none', '시트 안 단추에는 초점 테두리');
  await ctx.close();
});

for (const [width, theme] of [[1280, 'dark'], [900, 'light']]) {
  await check(`FP03 report KPI values are not truncated (${width} ${theme})`, async () => {
    const { ctx, page, errors } = await open({ width, theme });
    await go(page, 'home', { view: 'report' });
    await page.waitForSelector('.home-report .kpi-card .kpi-value');
    await page.waitForTimeout(300);
    const vals = await page.$$eval('.home-report .kpi-value', (els) => els.map((e) => ({ text: e.textContent, over: e.scrollWidth - e.clientWidth, units: e.querySelectorAll('.kpi-unit').length,
      unitSize: e.querySelector('.kpi-unit') ? parseFloat(getComputedStyle(e.querySelector('.kpi-unit')).fontSize) / parseFloat(getComputedStyle(e).fontSize) : null })));
    assert.ok(vals.length >= 4, `KPI ${vals.length}개`);
    for (const v of vals) {
      assert.ok(v.over <= 1, `'${v.text}' 이 ${v.over}px 넘쳐 말줄임`);
      assert.ok(v.units >= 1, `'${v.text}' 의 단위가 .kpi-unit`);
      assert.ok(v.unitSize < 0.8, `단위가 숫자보다 작다 (${v.unitSize})`);
    }
    assert.match(vals[0].text, /^\d+시간 \d+분$/, '글자는 그대로');
    assert.deepEqual(errors, []);
    await ctx.close();
  });
}

for (const width of [1280, 900]) {
  await check(`FP04 live watch info tiles: values stay inside, no 3+1 orphan (${width})`, async () => {
    const { ctx, page, errors } = await open({ width });
    await go(page, 'multilive');
    await page.waitForTimeout(300);
    await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:ml-watch', { detail: { sessionId: 's0' } })));
    await page.waitForSelector('.mwi-tiles .mwi-tile');
    await page.waitForTimeout(400);
    const tiles = await page.$$eval('.mwi-tiles .mwi-tile', (els) => els.map((t) => {
      const r = t.getBoundingClientRect(); const s = t.querySelector('strong'); const sr = s.getBoundingClientRect();
      return { top: Math.round(r.top), inside: sr.right <= r.right - 4 + 0.5, clipped: s.scrollWidth - s.clientWidth };
    }));
    assert.equal(tiles.length, 4, '팔로워·지금 시청·누적 시청·방송 시간');
    for (const t of tiles) { assert.ok(t.inside, '값이 타일 안'); assert.ok(t.clipped <= 1, `값 말줄임 ${t.clipped}px`); }
    const rows = [...new Set(tiles.map((t) => t.top))].map((top) => tiles.filter((t) => t.top === top).length);
    assert.ok(rows.join() === '4' || rows.join() === '2,2', `줄마다 타일 수 ${rows.join('+')}`);
    assert.deepEqual(errors, []);
    await ctx.close();
  });
}

await check('FP05 multichat header: title and search never overlap; close-all sits beside the wrapped search', async () => {
  const { ctx, page } = await open({ width: 1280, theme: 'light' });
  await go(page, 'multichat');
  await page.waitForSelector('[data-page="multichat"] .mc-search');
  await page.waitForTimeout(200);
  const sel = '[data-page="multichat"]';
  const title = await page.$eval(`${sel} .page-title`, (n) => ({ ...JSON.parse(JSON.stringify(n.getBoundingClientRect())), full: n.scrollWidth <= n.parentElement.clientWidth + 1 }));
  const search = await box(page, `${sel} .mc-search`);
  const close = await box(page, `${sel} [data-mc="close-all"]`);
  const overlap = !(search.left >= title.right || search.top >= title.bottom);
  assert.ok(!overlap, `검색칸이 제목과 겹친다 (제목 오른쪽 ${title.right}, 검색칸 왼쪽 ${search.left})`);
  assert.ok(title.full, '제목 글자가 칸 안에 다 보인다');
  if (search.top >= title.bottom) {
    assert.ok(Math.abs(close.top - search.top) < search.height, "'모두 닫기'가 검색칸과 같은 줄");
    assert.ok(search.width >= 300, `검색칸 폭 ${search.width}`);
  }
  // 인사이트 칸을 접으면(화면 폭 1048px) 한 줄
  await page.evaluate(() => { const r = document.querySelector('.insight-panel'); if (r) r.style.display = 'none'; });
  await page.waitForTimeout(200);
  const wide = await page.$eval(`${sel}`, (n) => n.getBoundingClientRect().width);
  if (wide >= 900) {
    const t2 = await box(page, `${sel} .page-title`);
    const s2 = await box(page, `${sel} .mc-search`);
    assert.ok(s2.top < t2.bottom && s2.left >= t2.right, `넓은 화면(${wide}px)은 한 줄`);
  }
  await ctx.close();
});

await check('FP06 disabled chat send button is a solid grey face, not a faded yellow', async () => {
  const { ctx, page } = await open();
  await go(page, 'multilive');
  await page.waitForTimeout(300);
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:ml-watch', { detail: { sessionId: 's0' } })));
  const sel = '.chat-skin .chat-input .btn.primary';
  await page.waitForSelector(sel, { state: 'attached' });
  const b = await page.$$eval(sel, (els) => els.filter((e) => e.offsetParent).map((e) => {
    e.disabled = true;
    const cs = getComputedStyle(e); const skin = getComputedStyle(e.closest('.chat-skin'));
    const probe = document.createElement('i'); probe.style.background = skin.getPropertyValue('--cs-fill'); document.body.append(probe);
    const fill = getComputedStyle(probe).backgroundColor; probe.remove();
    return { opacity: cs.opacity, bg: cs.backgroundColor, fill };
  }));
  assert.ok(b.length >= 1, '보이는 보내기 단추');
  for (const x of b) { assert.equal(x.opacity, '1'); assert.equal(x.bg, x.fill, '면은 --cs-fill'); }
  await ctx.close();
});

await browser.close();
server.close();
let failed = 0;
for (const r of results) {
  console.log(`${r.ok ? '✓' : '✗'} ${r.id}`);
  if (!r.ok) { failed += 1; console.log(`    ${r.error.replace(/\n/g, '\n    ')}`); }
}
console.log(`\n${results.length - failed}/${results.length} 통과 (${WEBKIT ? 'WebKit' : 'Chromium'})`);
process.exit(failed ? 1 : 0);
