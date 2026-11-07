// 성능 UI 회귀 검사 — 숫자를 재서 예산을 넘지 않는지 본다(모의 IPC, 헤드리스 Chromium).
// P01 시작 중 앱 코드가 일으키는 강제 동기 레이아웃(스크립트가 크기·위치를 읽어 전체 문서 레이아웃을 당기는 것)이 거의 없는지.
//     — 예전엔 숨은 멀티라이브·멀티채팅 세그먼트 판(motion.js place)과 채팅 분할기(splitter.js apply)가 시작마다 일으켰다.
// P02 화면마다 10초 가만히 둘 때 스크립트·레이아웃·요청이 거의 없는지(주기적 다시 그리기·폴링 회귀).
// P03 첫 화면 아래의 목록 이미지는 지연 로딩(loading="lazy")인지.
// 저장소 루트에서: node tools/ui-regression/perf.mjs   (Playwright 가 필요하다)
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, go, openApp, base, mockTauri } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}
const browser = await chromium.launch();

// ── P01 시작 중 강제 동기 레이아웃 ──
await check('P01 startup: app code forces almost no synchronous layout', async () => {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const page = await ctx.newPage();
  const cdp = await ctx.newCDPSession(page);
  const events = [];
  cdp.on('Tracing.dataCollected', (d) => events.push(...d.value));
  const done = new Promise((r) => cdp.once('Tracing.tracingComplete', r));
  await cdp.send('Tracing.start', { categories: 'devtools.timeline,disabled-by-default-devtools.timeline.stack', transferMode: 'ReportEvents' });
  await page.addInitScript(mockTauri, [base, 'dark']);
  await page.goto(base);
  await page.waitForFunction(() => !document.getElementById('splash') || !document.getElementById('splash').isConnected, null, { timeout: 20000 });
  await cdp.send('Tracing.end');
  await done;
  // 스택이 있는 Layout = 스크립트가 당긴 레이아웃. 환영 안내(onboarding, 처음 한 번)는 모의 실행마다 떠서 빼고,
  // 플레이어 iframe(player.html — hls 포크, isSurfaceHidden 이 숨김 확인에 getClientRects 를 쓴다)은 이 검사 범위 밖이다.
  const forced = events.filter((e) => e.name === 'Layout' && e.ph === 'X' && e.args?.beginData?.stackTrace?.length)
    .map((e) => ({ at: e.args.beginData.stackTrace[0], ms: e.dur / 1000 }))
    .filter((x) => !/onboarding\.js|player\.html/.test(x.at.url || ''));
  const named = forced.map((x) => `${x.at.functionName}@${(x.at.url || '').split('/').pop()}:${x.at.lineNumber + 1} ${x.ms.toFixed(1)}ms`);
  assert.ok(!named.some((n) => /motion\.js|splitter\.js/.test(n)), `세그먼트 판·채팅 분할기가 시작 중 레이아웃을 당김: ${named}`);
  assert.ok(forced.length <= 2, `강제 레이아웃 ${forced.length}회: ${named}`);
  await ctx.close();
});

// ── P02 대기 비용 ──
{
  const { ctx, page } = await openApp(browser, { width: 1280 });
  const cdp = await ctx.newCDPSession(page);
  await cdp.send('Performance.enable');
  const metrics = async () => Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map((m) => [m.name, m.value]));
  // 팔로잉은 썸네일이 빈 카드를 몇 번 다시 채운다(pages.js fillThumbsLater, 횟수 제한) — 그만큼만 허용
  const IPC_BUDGET = { following: 3 };
  for (const route of ['home', 'following', 'popular', 'multichat', 'multilive', 'library', 'stats', 'settings']) {
    await check(`P02 ${route}: stays quiet while idle (10s)`, async () => {
      await go(page, route);
      // 화면이 나눠 불러오기를 마칠 때까지(요청이 2초 동안 멈출 때까지, 최대 8초) 기다린 뒤부터 잰다
      for (let last = -1, still = 0, t = 0; still < 4 && t < 16; t++) {
        await page.waitForTimeout(500);
        const n = await page.evaluate(() => window.__calls.length);
        still = n === last ? still + 1 : 0;
        last = n;
      }
      const a = await metrics();
      const c0 = await page.evaluate(() => window.__calls.length);
      await page.waitForTimeout(10000);
      const b = await metrics();
      const calls = await page.evaluate((n) => window.__calls.slice(n), c0);
      const script = (b.ScriptDuration - a.ScriptDuration) * 1000;
      const layouts = b.LayoutCount - a.LayoutCount;
      assert.ok(script < 60, `스크립트 ${script.toFixed(0)}ms`);
      assert.ok(layouts <= 10, `레이아웃 ${layouts}회`);
      assert.ok(calls.length <= (IPC_BUDGET[route] || 0), `요청 ${calls.length}회: ${calls.join(', ')}`);
    });
  }

  // ── P03 이미지 지연 로딩 ──
  for (const route of ['popular', 'home', 'following']) {
    await check(`P03 ${route}: list images below the first screen load lazily`, async () => {
      await go(page, route);
      await page.waitForTimeout(800);
      const eager = await page.evaluate((r) => {
        const pg = document.querySelector(`[data-page="${r}"]`);
        return [...pg.querySelectorAll('img')].filter((img) => img.getBoundingClientRect().top > innerHeight * 1.5 && img.getAttribute('loading') !== 'lazy')
          .map((img) => `${img.parentElement?.className || ''} > img`).slice(0, 5);
      }, route);
      assert.deepEqual(eager, []);
    });
  }
  await ctx.close();
}

await browser.close();
server.close();
for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : `\n     ${r.error.split('\n').slice(0, 10).join('\n     ')}`}`);
const failed = results.filter((r) => !r.ok).length;
console.log(`\n${results.length - failed}/${results.length} passed`);
process.exitCode = failed ? 1 : 0;
