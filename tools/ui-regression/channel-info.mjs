// 채널 › 정보 탭 검사 — 지금 방송/마지막 방송·채팅 설정·채널·소개(링크)·내 시청 기록·최근 다시보기·인기 클립·메모 카드가
// 라이브·오프라인 채널 모두에서 그려지고, 넓은 창 3열 → 2열 → 1열로 접히며, 불러오는 중·실패·빈 값·일부 실패를 처리하는지 본다.
// app-ui 를 그대로 띄우고 window.__TAURI__ 를 모의 IPC 로 바꿔 실제 화면 코드를 돌린다(실제 서비스·데이터는 건드리지 않음).
// 저장소 루트에서: node tools/ui-regression/channel-info.mjs   (Playwright·axe-core 가 필요하다)
// CVIEW_UI_SHOTS=<폴더> 면 라이브·오프라인 × 폭 × 테마 사진을 남긴다. CVIEW_UI_SHOTS_ONLY=1 이면 검사 없이 사진만(고치기 전 판을 찍을 때).
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, base, mockTauri } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');
const SHOTS = process.env.CVIEW_UI_SHOTS;
const CHECK = !process.env.CVIEW_UI_SHOTS_ONLY;
let AXE = '';
try { AXE = fs.readFileSync(require.resolve('axe-core/axe.min.js'), 'utf8'); } catch { /* 사진만 찍을 때는 없어도 된다 */ }

// 시나리오별 채널 상세·정보 묶음. window.__ov 로 응답을 늦추거나(대기) 실패시킬 수 있다.
function scenarioMock(kind) {
  const img = (k, n) => `${location.origin}/__img/${k}/${n}.svg`;
  const now = Date.now();
  const ago = (min) => new Date(now - min * 60000).toISOString();
  const channel = {
    channelId: 'ch1', channelName: '한동숙', channelImageUrl: img('avatar', 1), verifiedMark: true, verifiedMarkType: 'PARTNER', followerCount: 523431,
    subscriptionAvailability: true, channelDescription: '매일 저녁 8시 방송합니다.\n일정: https://example.com/schedule?a=1&b=<2>\n문의: biz@example.com <script>x</script>',
  };
  const live = {
    liveTitle: '오늘은 신작 게임 정주행 — 엔딩까지 달립니다', categoryName: '마인크래프트', categoryType: 'GAME', viewerCount: 12400, viewerCountText: '12,400', accumulateCount: 48210,
    openDate: ago(95), tags: ['게임', '소통', '한국어', '신작'], thumbnailUrl: img('thumb', 3), adult: true, krOnlyViewing: false, timeMachineActive: true, clipActive: true,
    chatActive: true, chatAvailableGroup: 'FOLLOWER', chatAvailableCondition: 'REAL_NAME', minFollowerMinute: 10, qualities: ['1080p60', '720p60', '480p', '360p', '144p'],
  };
  const vods = Array.from({ length: 4 }, (_, i) => ({ videoNo: 700 + i, videoTitle: `다시보기 ${i + 1} — 어제 방송 전체`, thumbnailImageUrl: img('thumb', 10 + i), duration: 9000 + i * 61, readCount: 12800 + i, publishDate: ago(i * 1440 + 30) }));
  const clips = Array.from({ length: 6 }, (_, i) => ({ clipUID: `c${i}`, clipTitle: `레전드 순간 #${i + 1}`, thumbnailImageUrl: img('thumb', 30 + i), duration: 40, readCount: 94000 - i * 9000 }));
  const overview = {
    lastLive: { liveTitle: '[대회] 결승전 같이 보기', categoryName: '리그 오브 레전드', categoryType: 'GAME', tags: ['대회', '같이보기'], openDate: ago(3 * 1440 + 300), closeDate: ago(3 * 1440), accumulateCount: 1794, adult: false },
    recentVods: vods, vodTotal: 128, popularClips: clips,
    watch: { totalSeconds: 3600 * 42 + 600, last30Seconds: 3600 * 5 + 1200, sessionCount: 57, firstWatchedAt: ago(300 * 1440), lastWatchedAt: ago(180), topCategory: '마인크래프트' },
    follow: { following: true, followDate: ago(640 * 1440) },
  };
  const detail = () => ({ channel: { ...channel }, live: kind === 'live' ? { ...live } : null, isFavorite: false, memo: '' });
  window.__ov = { mode: 'ok', data: overview, calls: 0, release: null };
  // 다시보기·클립 열기는 기록만 하고 재생 화면으로 넘어가지 않게 — 앱보다 먼저 붙는 캡처 단계에서 막는다
  for (const name of ['cview:open-vod', 'cview:open-clip']) window.addEventListener(name, (e) => { (window.__opened ||= []).push(e.detail); e.stopImmediatePropagation(); }, true);
  window.__ext = [];
  const over = {
    channel_detail: detail,
    channel_overview: () => {
      window.__ov.calls++;
      const o = window.__ov;
      if (o.mode === 'fail') return Promise.reject(new Error('network down'));
      if (o.mode === 'wait') return new Promise((res) => { o.release = () => res(o.data); });
      return structuredClone(o.data);
    },
    channel_vods: () => ({ items: vods, totalCount: 128, hasMore: true }),
    channel_clips: () => ({ items: clips, hasMore: false }),
    open_external: ({ url }) => { window.__ext.push(url); return null; },
  };
  const orig = window.__TAURI__.core.invoke;
  window.__TAURI__.core.invoke = async (cmd, args) => {
    if (over[cmd]) { window.__calls.push(cmd); return over[cmd](args || {}); }
    return orig(cmd, args);
  };
}

const browser = await chromium.launch();
async function open({ kind = 'offline', theme = 'dark', width = 1280, height = 900, reduce = false, before } = {}) {
  const ctx = await browser.newContext({ viewport: { width, height }, colorScheme: theme, reducedMotion: reduce ? 'reduce' : 'no-preference' });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e.stack || e)));
  await page.addInitScript(mockTauri, [base, theme]);
  await page.addInitScript(scenarioMock, kind);
  if (before) await page.addInitScript(before);
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
  await page.waitForFunction(() => { const s = document.getElementById('splash'); return !s || !s.isConnected || getComputedStyle(s).display === 'none'; }, null, { timeout: 8000 }).catch(() => {});
  return { ctx, page, errors };
}
const go = (page, route, params = {}) => page.evaluate(([r, p]) => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: r, params: p } })), [route, params]);
async function openInfo(page, { settle = true } = {}) {
  await go(page, 'channel', { channelId: 'ch1' });
  await page.waitForSelector('.page-channel:not([hidden]) .ci-grid');
  if (settle) {
    await page.waitForFunction(() => !document.querySelector('.ci-grid .ci-skel'));
    await page.waitForTimeout(700); // 등장 모션이 끝난 뒤
  }
}
const cardIds = (page) => page.$$eval('.ci-grid > .ci-card', (els) => els.map((e) => e.dataset.ci));
const cardText = (page, id) => page.$eval(`[data-ci="${id}"]`, (e) => e.innerText.replace(/\s+/g, ' ').trim());
const columns = (page) => page.$eval('.ci-grid', (g) => getComputedStyle(g).gridTemplateColumns.split(' ').filter(Boolean).length);

const results = [];
async function check(id, fn) {
  if (!CHECK) return;
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}

// ── 사진 ──
if (SHOTS) {
  fs.mkdirSync(SHOTS, { recursive: true });
  for (const kind of ['live', 'offline']) {
    for (const theme of ['dark', 'light']) {
      for (const width of [1920, 1280, 900, 560]) {
        const { ctx, page } = await open({ kind, theme, width, height: width >= 1280 ? 1080 : 1200 });
        await go(page, 'channel', { channelId: 'ch1' });
        await page.waitForSelector('.page-channel:not([hidden]) .channel-tab-body > *');
        await page.waitForTimeout(1200);
        await page.screenshot({ path: path.join(SHOTS, `channel-info-${kind}-${theme}-${width}.png`), fullPage: true });
        await ctx.close();
      }
    }
  }
}

if (CHECK) {
  // ── I01 오프라인 채널 ──
  {
    const { ctx, page, errors } = await open({ kind: 'offline', width: 1680 });
    await openInfo(page);
    await check('I01 offline channel: last broadcast, channel, about, watch, memo, VODs, clips cards in order', async () => {
      assert.deepEqual(await cardIds(page), ['last', 'channel', 'about', 'watch', 'memo', 'vods', 'clips']);
      const last = await cardText(page, 'last');
      assert.match(last, /결승전 같이 보기/);
      assert.match(last, /게임 · 리그 오브 레전드/);
      assert.match(last, /#대회/);
      assert.match(last, /방송 길이 5시간/);
      assert.match(last, /누적 시청자 1,794명/);
      assert.match(last, /3일 전/);
    });
    await check('I02 channel card: exact followers, verified mark kind, subscription, follow-since date', async () => {
      const t = await cardText(page, 'channel');
      assert.match(t, /팔로워 523,431명 · 52\.3만/);
      assert.match(t, /인증 치지직 파트너/);
      assert.match(t, /구독 구독할 수 있음/);
      assert.match(t, /내 팔로우 .+부터 · 1년 전/);
    });
    await check('I03 watch card: total / 30-day / sessions KPIs, last and first watched, top category', async () => {
      const t = await cardText(page, 'watch');
      for (const re of [/42시간 10분 전체 시청/, /5시간 20분 최근 30일/, /57회 시청 횟수/, /마지막 시청 3시간 전/, /처음 본 날/, /자주 본 카테고리 마인크래프트/]) assert.match(t, re);
    });
    await check('I04 VOD and clip previews open the item, "모두 보기" switches tab and shows total', async () => {
      assert.equal(await page.locator('[data-ci="vods"] .ci-mini').count(), 4);
      assert.match(await cardText(page, 'vods'), /모두 보기 128개/);
      assert.equal(await page.locator('[data-ci="clips"] .ci-mini').count(), 6);
      assert.equal(await page.locator('[data-ci="clips"] .ci-mini:visible').count(), 3, '3열에서는 클립 한 줄');
      await page.click('[data-ci="vods"] .ci-mini');
      // 키보드 — 카드에 초점을 두고 Enter
      await page.focus('[data-ci="clips"] .ci-mini');
      await page.keyboard.press('Enter');
      assert.deepEqual(await page.evaluate(() => window.__opened), [{ videoNo: 700 }, { clipUid: 'c0' }]);
      await page.click('[data-ci="vods"] [data-ch-goto="vod"]');
      await page.waitForSelector('.channel-tab-body .grid .card[data-vod-no]');
      assert.equal(await page.$eval('.channel-tabs [aria-selected="true"]', (b) => b.dataset.tab), 'vod');
      await page.click('.channel-tabs [data-tab="info"]');
      await page.waitForSelector('.ci-grid');
    });
    await check('I05 description keeps line breaks, escapes markup, and turns http(s) URLs into external links', async () => {
      const about = page.locator('[data-ci="about"] .ci-about');
      assert.match(await about.innerText(), /8시 방송합니다\.\n일정:/);
      assert.equal(await page.locator('[data-ci="about"] script').count(), 0);
      assert.match(await about.innerText(), /<script>x<\/script>/);
      const links = await page.$$eval('[data-ci="about"] a.ci-link', (as) => as.map((a) => a.href));
      assert.equal(links.length, 1);
      assert.equal(links[0], 'https://example.com/schedule?a=1&b=');
      await page.click('[data-ci="about"] a.ci-link');
      assert.deepEqual(await page.evaluate(() => window.__ext), ['https://example.com/schedule?a=1&b=']);
      assert.ok(page.url().startsWith(base), '앱 안에서 이동하지 않는다');
    });
    await check('I06 overview is fetched once per channel visit (tab switches reuse it)', async () => {
      assert.equal(await page.evaluate(() => window.__ov.calls), 1);
    });
    await check('I07 no page errors', async () => assert.deepEqual(errors, []));
    await ctx.close();
  }

  // ── I08 라이브 채널 ──
  {
    const { ctx, page, errors } = await open({ kind: 'live', width: 1680 });
    await openInfo(page);
    await check('I08 live channel: now-live card with viewers, accumulate, start+uptime, qualities, flags; chat card; memo kept', async () => {
      assert.deepEqual(await cardIds(page), ['live', 'channel', 'about', 'watch', 'chat', 'vods', 'clips', 'memo']);
      const t = await cardText(page, 'live');
      for (const re of [/엔딩까지 달립니다/, /게임 · 마인크래프트/, /지금 시청자 12,400명/, /누적 시청자 48,210명/, /1시간 35분째/, /화질 1080p60 720p60 480p 360p 144p/, /19세/, /타임머신 가능/]) assert.match(t, re);
      const chat = await cardText(page, 'chat');
      assert.match(chat, /채팅 가능 팔로우 10분 지난 팔로워/);
      assert.match(chat, /조건 본인 인증한 사람/);
      assert.equal(await page.locator('[data-ci="last"]').count(), 0, '방송 중이면 마지막 방송 카드는 없다');
    });
    await check('I09 wide cards span two columns at 3 columns', async () => {
      assert.equal(await columns(page), 3);
      const spans = await page.$$eval('.ci-wide', (els) => els.map((e) => Math.round(e.getBoundingClientRect().width)));
      const one = await page.$eval('[data-ci="channel"]', (e) => e.getBoundingClientRect().width);
      for (const w of spans) assert.ok(w > one * 1.9, `넓은 카드 ${w} vs ${one}`);
    });
    await check('I10 no page errors (live)', async () => assert.deepEqual(errors, []));
    await ctx.close();
  }

  // ── I11 폭에 따른 열 수 ──
  for (const [width, cols] of [[1920, 3], [1440, 3], [1280, 2], [1000, 2], [700, 1], [420, 1]]) {
    const { ctx, page } = await open({ kind: 'live', width });
    await openInfo(page);
    await check(`I11 ${width}px: ${cols} column(s), no horizontal overflow, cards fit`, async () => {
      assert.equal(await columns(page), cols);
      assert.equal(await page.evaluate(() => document.scrollingElement.scrollWidth > innerWidth + 1), false, '가로 넘침 없음');
      const over = await page.$$eval('.ci-card', (els) => els.filter((e) => e.scrollWidth > e.clientWidth + 1).map((e) => e.dataset.ci));
      assert.deepEqual(over, [], '카드 안 가로 넘침 없음');
      const clipped = await page.$$eval('.ci-kpi strong', (els) => els.filter((e) => e.scrollWidth > e.clientWidth + 1).map((e) => e.textContent));
      assert.deepEqual(clipped, [], '시청 기록 숫자가 잘리지 않는다');
      if (cols === 2) assert.ok(await page.$eval('[data-ci="clips"]', (e) => e.getBoundingClientRect().width > e.parentElement.getBoundingClientRect().width * 0.9), '2열에서 클립 카드는 전체 폭');
    });
    await ctx.close();
  }

  // ── I12 불러오는 중 → 채움 ──
  {
    const { ctx, page, errors } = await open({ kind: 'offline' });
    await page.evaluate(() => { window.__ov.mode = 'wait'; });
    await openInfo(page, { settle: false });
    await check('I12 while overview loads: immediate cards render, dependent cards show skeleton; filling keeps card nodes', async () => {
      assert.match(await cardText(page, 'channel'), /팔로워 523,431명/);
      assert.match(await cardText(page, 'about'), /8시 방송합니다/);
      for (const id of ['last', 'watch', 'vods', 'clips']) assert.equal(await page.locator(`[data-ci="${id}"] .ci-skel`).count(), 1, `${id} 스켈레톤`);
      await page.evaluate(() => { window.__keep = document.querySelector('[data-ci="watch"]'); });
      await page.waitForFunction(() => typeof window.__ov.release === 'function');
      await page.evaluate(() => window.__ov.release());
      await page.waitForFunction(() => !document.querySelector('.ci-grid .ci-skel'));
      assert.ok(await page.evaluate(() => window.__keep === document.querySelector('[data-ci="watch"]')), '카드 요소를 다시 만들지 않는다');
      assert.match(await cardText(page, 'watch'), /57회/);
    });
    await check('I13 no page errors (loading)', async () => assert.deepEqual(errors, []));
    await ctx.close();
  }

  // ── I14 통째로 실패 → 다시 시도 ──
  {
    const { ctx, page, errors } = await open({ kind: 'offline' });
    await page.evaluate(() => { window.__ov.mode = 'fail'; });
    await openInfo(page, { settle: false });
    await page.waitForSelector('[data-ci="watch"] .ci-failed');
    await check('I14 overview failure: dependent cards show failure + retry, others stay; retry fills them', async () => {
      for (const id of ['last', 'watch', 'vods', 'clips']) assert.equal(await page.locator(`[data-ci="${id}"] .ci-failed [data-ch="overview-retry"]`).count(), 1, id);
      assert.match(await cardText(page, 'channel'), /팔로워/);
      assert.equal(await page.locator('[data-ci="channel"] .sk').count(), 0, '팔로우 줄이 스켈레톤으로 남지 않는다');
      await page.evaluate(() => { window.__ov.mode = 'ok'; });
      await page.click('[data-ci="watch"] [data-ch="overview-retry"]');
      await page.waitForFunction(() => !document.querySelector('.ci-grid :is(.ci-skel, .ci-failed)'));
      assert.match(await cardText(page, 'last'), /결승전/);
    });
    await check('I15 no page errors (failure)', async () => assert.deepEqual(errors, []));
    await ctx.close();
  }

  // ── I16 빈 값·일부 실패 ──
  {
    const { ctx, page, errors } = await open({ kind: 'offline' });
    await page.evaluate(() => {
      Object.assign(window.__ov.data, { lastLive: null, recentVods: null, vodTotal: null, popularClips: [], watch: { totalSeconds: 0, last30Seconds: 0, sessionCount: 0 }, follow: null });
    });
    await page.evaluate(() => {
      const orig = window.__TAURI__.core.invoke;
      window.__TAURI__.core.invoke = async (cmd, args) => {
        if (cmd === 'channel_detail') return { channel: { channelId: 'ch1', channelName: '새 채널', followerCount: 0 }, live: null, isFavorite: false, memo: '첫 방송 보기\n<b>알림</b>' };
        return orig(cmd, args);
      };
    });
    await openInfo(page);
    await check('I16 empty and partial values: friendly empty text per card, VOD failure isolated, no follow row when logged out', async () => {
      assert.match(await cardText(page, 'last'), /최근 방송 기록이 없습니다/);
      assert.match(await cardText(page, 'about'), /소개가 없습니다/);
      assert.match(await cardText(page, 'watch'), /아직 이 채널을 본 기록이 없습니다/);
      assert.match(await cardText(page, 'clips'), /클립이 없습니다/);
      assert.equal(await page.locator('[data-ci="vods"] .ci-failed').count(), 1, '다시보기만 실패');
      assert.equal(await page.$eval('[data-ci="vods"] [data-ch-goto]', (b) => b.textContent.trim()), '모두 보기', '전체 개수를 모르면 숫자 없이');
      const ch = await cardText(page, 'channel');
      assert.match(ch, /인증 인증 없음/);
      assert.match(ch, /구독 상품 없음/);
      assert.doesNotMatch(ch, /내 팔로우/);
      assert.match(await cardText(page, 'memo'), /첫 방송 보기 <b>알림<\/b> 고치기|고치기 첫 방송 보기 <b>알림<\/b>/);
    });
    await check('I17 no page errors (empty)', async () => assert.deepEqual(errors, []));
    await ctx.close();
  }

  // ── I18 접근성·대비 (다크·라이트) ──
  for (const theme of ['dark', 'light']) {
    for (const kind of ['live', 'offline']) {
      const { ctx, page } = await open({ kind, theme, width: 1440 });
      await openInfo(page);
      await check(`I18 ${theme} ${kind}: axe (WCAG 2.1 A/AA + best practice incl. color-contrast) clean in the info tab`, async () => {
        assert.ok(AXE, 'axe-core 가 필요하다');
        await page.evaluate(AXE);
        const bad = await page.evaluate(async () => {
          const r = await window.axe.run(document.querySelector('.channel-tab-body'), { runOnly: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'best-practice'], rules: { region: { enabled: false } } });
          return r.violations.flatMap((v) => v.nodes.slice(0, 3).map((n) => `${v.id}: ${n.target.join(' ')}`));
        });
        assert.deepEqual(bad, []);
      });
      await check(`I19 ${theme} ${kind}: every card is a labelled section with a heading`, async () => {
        const bad = await page.$$eval('.ci-card', (els) => els.filter((e) => { const h = document.getElementById(e.getAttribute('aria-labelledby')); return !h || !h.textContent.trim() || !e.contains(h); }).map((e) => e.dataset.ci));
        assert.deepEqual(bad, []);
      });
      await ctx.close();
    }
  }

  // ── I20 움직임 줄이기 ──
  {
    const { ctx, page } = await open({ kind: 'live', reduce: true });
    await go(page, 'channel', { channelId: 'ch1' });
    await page.waitForSelector('.ci-grid > .ci-card');
    await check('I20 reduced motion: info cards do not animate in', async () => {
      const running = await page.$$eval('.ci-grid > .ci-card', (els) => els.flatMap((e) => e.getAnimations().filter((a) => a.playState !== 'finished')).length);
      assert.equal(running, 0);
    });
    await ctx.close();
  }

  // ── I21 순수 함수 ──
  {
    const { ctx, page } = await open();
    await check('I21 helpers: watch time, chat policy, linkify escaping, span text', async () => {
      const r = await page.evaluate(async () => {
        const m = await import('/js/channel-info.js');
        return {
          w: [m.watchTimeText(30), m.watchTimeText(600), m.watchTimeText(3600 * 2 + 300), m.watchTimeText(3600 * 150 + 60), m.watchTimeText(7200)],
          c1: m.chatPolicyRows({ chatAvailableGroup: 'ALL', chatAvailableCondition: 'NONE' }),
          c2: m.chatPolicyRows({ chatActive: false, chatAvailableGroup: 'ALL' }),
          c3: m.chatPolicyRows({ chatAvailableGroup: 'FOLLOWER', minFollowerMinute: 120 }),
          c4: m.chatPolicyRows(null),
          l: m.linkify('a <i> https://x.y/z. javascript:alert(1) http://q.r/(s)'),
          s: [m.spanText('2026-01-01T00:00:00Z', '2026-01-01T05:01:00Z'), m.spanText('x', 'y'), m.spanText('2026-01-01T05:00:00Z', '2026-01-01T00:00:00Z')],
        };
      });
      assert.deepEqual(r.w, ['1분 미만', '10분', '2시간 5분', '150시간', '2시간']);
      assert.deepEqual(r.c1, [['채팅 가능', '누구나']]);
      assert.deepEqual(r.c2, [['채팅', '꺼 둠']]);
      assert.deepEqual(r.c3, [['채팅 가능', '팔로우 2시간 지난 팔로워']]);
      assert.deepEqual(r.c4, []);
      assert.equal(r.l, 'a &lt;i&gt; <a class="ci-link" href="https://x.y/z" data-ext title="브라우저에서 열기">https://x.y/z</a>. javascript:alert(1) <a class="ci-link" href="http://q.r/" data-ext title="브라우저에서 열기">http://q.r/</a>(s)');
      assert.deepEqual(r.s, ['5시간 1분', '', '']);
    });
    await ctx.close();
  }
}

await browser.close();
server.close();
if (CHECK) {
  for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : `\n     ${r.error}`}`);
  const failed = results.filter((r) => !r.ok).length;
  console.log(`\n${results.length - failed}/${results.length} passed`);
  process.exit(failed ? 1 : 0);
}
