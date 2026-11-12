// 라이브 정보란·채널 화면 모션 검사 — 등장 순차·숫자 카운트업/롤링·LIVE 맥동·카드 기울기·배너 패럴랙스·탭 전환이
// 기대대로 움직이고, 움직임 줄이기(설정 "절약"·시스템 prefers-reduced-motion)에서는 꺼지며, 창이 가려지면 멈추는지 본다.
// app-ui 를 그대로 띄우고 window.__TAURI__ 를 모의 IPC 로 바꿔 실제 화면 코드를 돌린다(실제 서비스·데이터는 건드리지 않음).
// 저장소 루트에서: node tools/ui-regression/live-channel-motion.mjs   (Playwright 가 필요하다)
// CVIEW_UI_SHOTS=<폴더> 면 등장 프레임을 PNG 로 남긴다. 다른 판(고치기 전)을 찍으려면 그 판의 저장소 루트에서
// CVIEW_UI_SHOTS_ONLY=1 로 이 파일을 실행한다(검사 없이 사진만).
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, base, mockTauri } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');
const SHOTS = process.env.CVIEW_UI_SHOTS;
const CHECK = !process.env.CVIEW_UI_SHOTS_ONLY;

// mock-app 의 모의 IPC 위에 방송 중 채널·멀티라이브 포커스 상태를 덧씌운다.
function liveMock() {
  const img = (n) => `${location.origin}/__img/avatar/${n}.svg`;
  const thumb = (n) => `${location.origin}/__img/thumb/${n}.svg`;
  const open = new Date(Date.now() - 5400e3).toISOString();
  const live = { liveTitle: '오늘은 신작 게임 정주행 — 엔딩까지 달립니다', categoryName: '마인크래프트', viewerCount: 12400, viewerCountText: '12,400', accumulateCount: 48210, openDate: open, tags: ['게임', '소통', '한국어', '신작', '정주행'] };
  const channel = { channelId: 'ch1', channelName: '한동숙', channelImageUrl: img(1), verifiedMark: true, followerCount: 523000, channelDescription: '매일 저녁 8시 방송합니다.\n문의: example@example.com' };
  const vods = Array.from({ length: 8 }, (_, i) => ({ videoNo: 700 + i, videoTitle: `다시보기 ${i + 1}`, thumbnailImageUrl: thumb(10 + i), duration: 9000, readCount: 800 + i, publishDate: new Date(Date.now() - i * 864e5).toISOString() }));
  const session = (id, i, name, viewers) => ({
    id: `s${i}`, slot: i, channelId: id, channelName: name, liveTitle: live.liveTitle, profileImageUrl: null, viewerCountText: viewers,
    accumulateCount: 0, accumulateCountText: '', openDate: open, isMuted: i > 0, isAudible: i === 0, isAudioOnly: false,
    state: { kind: 'playing' }, chatChannelId: null, maxHeightCap: 0, decodeSuspended: false,
  });
  const state = {
    sessions: [session('ch1', 0, '한동숙', '12,400'), session('ch2', 1, '침착맨', '3.4만')], selectedId: 's0', focusMainId: 's0', canAdd: true, maxSessions: 4,
    layout: 'focus', focusDirection: 'left', focusRatio: 0.7, horizontalRatio: 0.5, verticalRatio: 0.5, multiAudio: false, watchId: null, rev: 1,
  };
  const over = {
    channel_detail: () => ({ channel: { ...channel }, live: { ...live }, isFavorite: false }),
    channel_vods: () => ({ items: vods, totalCount: vods.length, hasMore: false }),
    channel_clips: () => ({ items: [], hasMore: false }),
    channel_posts: () => ({ items: [], hasMore: false }),
    channel_info: () => ({ verified: true, followerCount: 523000 }),
    live_detail: () => ({ ...live, verified: true }),
    favorite_set: () => null,
    multilive_state: () => state,
    multilive_set_focus_layout: ({ direction }) => { if (direction) state.focusDirection = direction; state.rev++; return state; },
    multilive_set_layout: ({ layout }) => { state.layout = layout; state.rev++; return state; },
    multilive_stage_size: () => null,
  };
  window.__mlState = state;
  const orig = window.__TAURI__.core.invoke;
  window.__TAURI__.core.invoke = async (cmd, args) => {
    if (over[cmd]) { window.__calls.push(cmd); return over[cmd](args || {}); }
    return orig(cmd, args);
  };
}

const browser = await chromium.launch();
async function open({ theme = 'dark', motion = null, reduce = false, width = 1280, height = 860 } = {}) {
  const ctx = await browser.newContext({ viewport: { width, height }, colorScheme: theme, reducedMotion: reduce ? 'reduce' : 'no-preference' });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e.stack || e)));
  await page.addInitScript(mockTauri, [base, theme]);
  await page.addInitScript(liveMock);
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
  await page.waitForFunction(() => { const s = document.getElementById('splash'); return !s || !s.isConnected || getComputedStyle(s).display === 'none'; }, null, { timeout: 8000 }).catch(() => {});
  // 설정 "절약" — prefs-effects 가 설정을 받아 다시 덮지 않게 관찰자로 붙잡아 둔다
  if (motion) await page.evaluate((m) => { const r = document.documentElement; r.dataset.motion = m; new MutationObserver(() => { if (r.dataset.motion !== m) r.dataset.motion = m; }).observe(r, { attributes: true, attributeFilter: ['data-motion'] }); }, motion);
  return { ctx, page, errors };
}
const go = (page, route, params = {}) => page.evaluate(([r, p]) => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: r, params: p } })), [route, params]);
const openChannel = async (page) => { await go(page, 'channel', { channelId: 'ch1' }); await page.waitForSelector('.page-channel:not([hidden]) .channel-hero'); };
const openWatchInfo = async (page) => { await go(page, 'multilive'); await page.waitForSelector('.ml-focus-info .mwi-hero:not(.is-skeleton)'); };
/** 요소의 지금 돌고 있는(또는 기다리는) 애니메이션 이름 — CSS 애니메이션·WAAPI 둘 다. */
const runningAnims = (page, sel) => page.$$eval(sel, (els) => els.flatMap((e) => e.getAnimations().filter((a) => a.playState !== 'finished').map((a) => a.animationName || a.id || 'waapi')));

const results = [];
async function check(id, fn) {
  if (!CHECK) return;
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message.split('\n')[0] }); }
}

// ── 사진 — 등장 프레임(0·120·260·520ms·끝), 다크·라이트 ──
if (SHOTS) {
  fs.mkdirSync(SHOTS, { recursive: true });
  for (const theme of ['dark', 'light']) {
    const { ctx, page } = await open({ theme });
    for (const [name, enter, crop] of [['channel', openChannel, '.page-channel .page-body'], ['watch-info', openWatchInfo, '.ml-focus-info']]) {
      await go(page, 'home');
      await page.waitForTimeout(400);
      const t0 = Date.now();
      await enter(page);
      for (const at of [0, 120, 260, 520, 1400]) {
        const wait = at - (Date.now() - t0);
        if (wait > 0) await page.waitForTimeout(wait);
        const el = await page.$(crop);
        await el.screenshot({ path: path.join(SHOTS, `${name}-${theme}-${String(at).padStart(4, '0')}ms.png`) });
      }
    }
    // 카드 기울기·빛 번짐 — 채널 다시보기 첫 카드 오른쪽 위에 마우스
    await openChannel(page);
    await page.click('.channel-tabs [data-tab="vod"]');
    await page.waitForSelector('.channel-tab-body .card .thumb');
    await page.waitForTimeout(900);
    const b = await (await page.$('.channel-tab-body .card')).boundingBox();
    await page.mouse.move(b.x + b.width * 0.5, b.y + b.height * 0.4);
    await page.mouse.move(b.x + b.width * 0.85, b.y + b.height * 0.15, { steps: 5 });
    await page.waitForTimeout(400);
    await page.screenshot({ path: path.join(SHOTS, `channel-tilt-${theme}.png`), clip: { x: b.x - 20, y: b.y - 20, width: b.width * 2 + 60, height: b.height + 40 } });
    await ctx.close();
  }
}

// ── 검사 ──
if (CHECK) {
  const { ctx, page, errors } = await open();

  await check('L01 channel: 머리 요소가 차례로 떠오른다(등장 순차)', async () => {
    await openChannel(page);
    const delays = await page.$$eval('.channel-hero :is(.channel-avatar, .channel-id, .channel-actions > *, .channel-live-line)', (els) => els.map((e) => {
      const a = e.getAnimations().find((x) => /^mo-/.test(x.animationName || ''));
      return a ? Number(a.effect.getTiming().delay) : null;
    }));
    assert.ok(delays.length >= 5 && delays.every((d) => d != null), `등장 애니메이션 없음: ${delays}`);
    assert.ok(delays.some((d) => d > delays[0]), `순차 지연이 없음: ${delays}`);
    const props = await page.$$eval('.channel-hero *', (els) => [...new Set(els.flatMap((e) => e.getAnimations().flatMap((a) => (a.effect?.getKeyframes?.() || []).flatMap((k) => Object.keys(k)))))]);
    const bad = props.filter((p) => !['offset', 'easing', 'composite', 'computedOffset', 'opacity', 'transform', 'translate', 'scale', 'rotate'].includes(p));
    assert.deepEqual(bad, [], `transform·opacity 밖의 속성을 움직임: ${bad}`);
  });

  await check('L02 channel: 팔로워·시청자 수가 0 에서 올라와 원래 글로 끝난다', async () => {
    await go(page, 'home');
    await page.waitForTimeout(300);
    await openChannel(page);
    const early = await page.$$eval('.channel-id [data-mo-count]', (els) => els.map((e) => e.textContent));
    await page.waitForTimeout(1100);
    const done = await page.$$eval('.channel-id [data-mo-count]', (els) => els.map((e) => e.textContent));
    assert.deepEqual(done, ['52.3만', '12,400']);
    assert.notDeepEqual(early, done, `카운트업이 없다: ${early}`);
  });

  await check('L03 channel: 즐겨찾기로 다시 그려도 등장 모션을 되풀이하지 않는다', async () => {
    await page.waitForTimeout(800);
    await page.click('.channel-actions [data-ch="favorite"]');
    await page.waitForSelector('.channel-actions .fav.on');
    const replay = await runningAnims(page, '.channel-hero :is(.channel-avatar, .channel-id, .channel-live-line)');
    assert.deepEqual(replay, [], `다시 떠오름: ${replay}`);
    assert.equal(await page.$eval('.channel-id [data-mo-count]', (e) => e.textContent), '52.3만', '숫자가 다시 0 부터 셈');
  });

  await check('L04 channel: 카드 위에서 마우스를 움직이면 썸네일이 기울고 빛이 따라온다, 떠나면 제자리', async () => {
    await page.click('.channel-tabs [data-tab="vod"]');
    await page.waitForSelector('.channel-tab-body .card .thumb');
    await page.waitForTimeout(700);
    const card = await page.$('.channel-tab-body .card');
    const b = await card.boundingBox();
    await page.mouse.move(b.x + b.width * 0.5, b.y + 20);
    await page.mouse.move(b.x + b.width * 0.9, b.y + 30, { steps: 4 });
    await page.waitForTimeout(250);
    const on = await page.$eval('.channel-tab-body .card .thumb', (t) => ({ ry: t.style.getPropertyValue('--mo-ry'), glare: Boolean(t.querySelector('.mo-glare')), tf: getComputedStyle(t).transform }));
    assert.ok(on.glare, '빛 번짐 층이 없다');
    assert.ok(parseFloat(on.ry) > 0, `기울기 변수: ${on.ry}`);
    assert.notEqual(on.tf, 'none', '썸네일이 기울지 않음');
    await page.mouse.move(5, 5);
    await page.waitForTimeout(100);
    assert.equal(await page.$eval('.channel-tab-body .card .thumb', (t) => t.style.getPropertyValue('--mo-ry')), '', '떠나도 기울기가 남음');
  });

  await check('L05 channel: 탭을 바꾸면 내용이 가는 쪽에서 미끄러져 들어온다', async () => {
    await page.click('.channel-tabs [data-tab="info"]');
    const a = await runningAnims(page, '.channel-tab-body');
    assert.ok(a.includes('mo-tab-swap'), `탭 전환 모션 없음: ${a}`);
  });

  await check('L06 channel: 배너가 스크롤·마우스에 따라 천천히 움직인다(패럴랙스)', async () => {
    const r = await page.evaluate(() => {
      const img = document.querySelector('.channel-hero-bg img');
      return { timeline: CSS.supports('animation-timeline: scroll()') ? getComputedStyle(img).animationName : 'unsupported' };
    });
    if (r.timeline !== 'unsupported') assert.match(r.timeline, /mo-hero-parallax/, `스크롤 패럴랙스 없음: ${r.timeline}`);
    const hero = await (await page.$('.channel-hero')).boundingBox();
    await page.mouse.move(hero.x + hero.width * 0.9, hero.y + hero.height * 0.2, { steps: 3 });
    await page.waitForTimeout(120);
    const px = await page.$eval('.channel-hero', (h) => h.style.getPropertyValue('--mo-px'));
    assert.ok(px && parseFloat(px) !== 0, `마우스 패럴랙스 변수: ${px}`);
    await page.mouse.move(5, 5);
  });

  await check('L07 channel: LIVE 배지가 맥동하고, 창이 가려지면 멈춘다', async () => {
    const state = () => page.$$eval('.channel-avatar .strip-live, .channel-live-line .badge-live', (els) => els.map((e) => getComputedStyle(e, '::after').animationName + ':' + getComputedStyle(e, '::after').animationPlayState));
    const on = await state();
    assert.ok(on.length === 2 && on.every((s) => s.startsWith('mo-live-halo:running')), `맥동: ${on}`);
    await page.evaluate(() => { Object.defineProperty(document, 'hidden', { configurable: true, get: () => true }); document.dispatchEvent(new Event('visibilitychange')); });
    const off = await state();
    assert.ok(off.every((s) => s.endsWith(':paused')), `가려져도 돈다: ${off}`);
    await page.evaluate(() => { Object.defineProperty(document, 'hidden', { configurable: true, get: () => false }); document.dispatchEvent(new Event('visibilitychange')); });
    assert.ok((await state()).every((s) => s.endsWith(':running')), '다시 보여도 멈춰 있다');
  });

  await check('L08 channel: 가만히 둘 때(5초) 맥동이 스크립트·레이아웃을 일으키지 않는다', async () => {
    const cdp = await ctx.newCDPSession(page);
    await cdp.send('Performance.enable');
    const m = async () => Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map((x) => [x.name, x.value]));
    await page.waitForTimeout(1200);
    const a = await m();
    await page.waitForTimeout(5000);
    const b = await m();
    const script = (b.ScriptDuration - a.ScriptDuration) * 1000;
    const layouts = b.LayoutCount - a.LayoutCount;
    assert.ok(script < 30, `스크립트 ${script.toFixed(0)}ms`);
    assert.ok(layouts <= 3, `레이아웃 ${layouts}회`);
    await cdp.detach();
  });

  await check('L09 라이브 정보란: 머리·통계 타일·태그가 차례로 떠오르고 숫자가 올라온다', async () => {
    await openWatchInfo(page);
    const n = await page.$$eval('.ml-focus-info :is(.mwi-avatar, .mwi-name, .mwi-title, .mwi-meta > *, .mwi-actions > *, .mwi-tile, .mwi-tags > *)', (els) => els.filter((e) => e.getAnimations().some((a) => /^mo-/.test(a.animationName || ''))).length);
    assert.ok(n >= 12, `등장 애니메이션이 붙은 요소 ${n}개`);
    await page.waitForTimeout(1100);
    const nums = await page.$$eval('.ml-focus-info [data-mo-count]', (els) => els.map((e) => e.textContent));
    assert.ok(nums.includes('52.3만') && nums.includes('12,400'), `끝 숫자: ${nums}`);
  });

  await check('L10 라이브 정보란: 시청자 수가 바뀌면 숫자가 굴러 바뀐다', async () => {
    await page.evaluate(() => { window.__mlState.sessions[0].viewerCountText = '12,950'; });
    await page.evaluate(() => document.querySelector('[data-direction="right"]').click());
    await page.waitForFunction(() => document.querySelector('.ml-focus-info .mwi-pill-live .mo-num')?.textContent === '12,950');
    const a = await runningAnims(page, '.ml-focus-info .mwi-pill-live .mo-num');
    assert.ok(a.includes('mo-roll'), `롤링 없음: ${a}`);
    await page.evaluate(() => document.querySelector('[data-direction="left"]').click());
  });

  await check('L11 라이브 정보란: LIVE 점이 맥동하고, 정보란이 화면 밖이면 멈춘다', async () => {
    const st = () => page.$eval('.ml-focus-info .mwi-pill-live i', (e) => `${getComputedStyle(e, '::after').animationName}:${getComputedStyle(e, '::after').animationPlayState}`);
    assert.equal(await st(), 'mo-live-ping:running');
    await page.$eval('.ml-focus-info', (el) => { el.querySelector('.mwi-body').style.minHeight = '3000px'; el.scrollTop = 2000; });
    await page.waitForFunction(() => document.querySelector('.ml-focus-info .mwi-hero').classList.contains('mo-off'));
    assert.equal(await st(), 'mo-live-ping:paused');
    await page.$eval('.ml-focus-info', (el) => { el.scrollTop = 0; el.querySelector('.mwi-body').style.minHeight = ''; });
    await page.waitForFunction(() => !document.querySelector('.ml-focus-info .mwi-hero').classList.contains('mo-off'));
  });

  assert.deepEqual(errors, []);
  await ctx.close();

  // 움직임 줄이기 — 설정 "절약"과 시스템 설정 둘 다
  for (const [label, opts] of [['data-motion=reduced', { motion: 'reduced' }], ['prefers-reduced-motion', { reduce: true }]]) {
    const r = await open(opts);
    await check(`L20 ${label}: 등장·맥동·패럴랙스가 없고 숫자는 바로 끝 값`, async () => {
      await openChannel(r.page);
      const nums = await r.page.$$eval('.channel-id [data-mo-count]', (els) => els.map((e) => e.textContent));
      assert.deepEqual(nums, ['52.3만', '12,400'], `카운트업이 돈다: ${nums}`);
      const live = await r.page.$$eval('.channel-hero *', (els) => els.flatMap((e) => [...e.getAnimations(), ...['::after', '::before'].map((p) => getComputedStyle(e, p).animationName).filter((n) => n !== 'none').map((n) => ({ animationName: n, playState: 'css' }))])
        .filter((a) => a.playState !== 'finished' && /^mo-/.test(a.animationName || '')).map((a) => a.animationName));
      assert.deepEqual([...new Set(live)], [], `모션이 남음: ${live}`);
    });
    await check(`L21 ${label}: 카드 기울기·호버 크기 변화가 없다`, async () => {
      await r.page.click('.channel-tabs [data-tab="vod"]');
      await r.page.waitForSelector('.channel-tab-body .card .thumb');
      assert.deepEqual(await runningAnims(r.page, '.channel-tab-body'), [], '탭 전환 모션이 돈다');
      const b = await (await r.page.$('.channel-tab-body .card')).boundingBox();
      await r.page.mouse.move(b.x + b.width * 0.5, b.y + 20);
      await r.page.mouse.move(b.x + b.width * 0.9, b.y + 30, { steps: 4 });
      await r.page.waitForTimeout(150);
      const t = await r.page.$eval('.channel-tab-body .card .thumb', (el) => ({ ry: el.style.getPropertyValue('--mo-ry'), glare: Boolean(el.querySelector('.mo-glare')) }));
      assert.deepEqual(t, { ry: '', glare: false });
      await r.page.hover('.channel-avatar');
      assert.equal(await r.page.$eval('.channel-avatar', (e) => getComputedStyle(e).scale), 'none', '아바타 호버 크기');
    });
    await check(`L22 ${label}: 라이브 정보란 — 등장·LIVE 점·롤링 없음`, async () => {
      await openWatchInfo(r.page);
      const a = await runningAnims(r.page, '.ml-focus-info :is(.mwi-avatar, .mwi-name, .mwi-title, .mwi-meta > *, .mwi-tile, .mwi-tags > *)');
      assert.deepEqual(a.filter((n) => /^mo-/.test(n)), [], `등장: ${a}`);
      assert.equal(await r.page.$eval('.ml-focus-info .mwi-pill-live i', (e) => getComputedStyle(e, '::after').animationName), 'none');
      await r.page.evaluate(() => { window.__mlState.sessions[0].viewerCountText = '13,100'; });
      await r.page.evaluate(() => document.querySelector('[data-direction="right"]').click());
      await r.page.waitForFunction(() => document.querySelector('.ml-focus-info .mwi-pill-live .mo-num')?.textContent === '13,100');
      assert.deepEqual(await runningAnims(r.page, '.ml-focus-info .mwi-pill-live .mo-num'), [], '롤링이 돈다');
    });
    assert.deepEqual(r.errors, []);
    await r.ctx.close();
  }
}

await browser.close();
server.close();
if (CHECK) {
  for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : ` — ${r.error}`}`);
  const failed = results.filter((r) => !r.ok).length;
  console.log(`\n${results.length - failed}/${results.length} passed`);
  process.exit(failed ? 1 : 0);
}
