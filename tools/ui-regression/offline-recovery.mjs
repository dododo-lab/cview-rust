// 네트워크 끊김→복구·시작 실패·멈춤 감시 신호 회귀 검사 — 모의 IPC 와 브라우저 오프라인 전환(setOffline)으로
// 네트워크가 끊긴 동안 데이터 요청이 실패하는 상황을 흉내 낸다. 실제 서비스·데이터는 건드리지 않는다.
// 저장소 루트에서: node tools/ui-regression/offline-recovery.mjs
//   (Playwright 가 필요하다 — 예: npm i playwright@1.49.1 를 다른 폴더에 설치하고 NODE_PATH 로 지정)
//
// O01 웹 화면이 멈춤 감시 신호(ui_heartbeat)를 시작 때·핑(__cviewBeat)마다 보낸다
// O02 끊긴 동안 사이드바 팔로잉 갱신이 실패해도 보던 목록·개수를 지우지 않는다
// O03 복구되면(online) 사이드바 팔로잉을 바로 다시 받는다
// O04 끊긴 동안 연결에 실패한 채팅(단독 시청 화면)이 복구되면 저절로 다시 연결된다
// O05 online 이 짧게 여러 번 와도 복구 작업은 한 번만 한다
// O06 끊김·복구 동안 처리되지 않은 페이지 오류가 없다
// O07 시작 중 예외가 나도 로딩 화면이 걷히고 다시 불러오기 안내가 뜬다
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { base, server, mockTauri, go } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');

// 끊김 흉내 — window.__offline 이 참이면 데이터 명령이 네트워크 오류로 실패한다. 채팅 전용 화면에 필요한 명령도 채운다.
function offlineLayer([crashAtStart, failChatOnce]) {
  const inner = window.__TAURI__.core.invoke;
  const extra = {
    live_detail: ({ channelId }) => ({ channelId, channelName: '릴카', chatChannelId: `chat-${channelId}`, isLive: true, liveTitle: '테스트 방송', viewerCountText: '1,000' }),
    chat_connect: () => ({ canSend: true }),
    chat_disconnect: () => null,
  };
  window.__offline = false;
  window.__chatConnects = 0;
  window.__failChatOnce = failChatOnce;
  window.__TAURI__.core.invoke = async (cmd, args) => {
    if (cmd === 'chat_connect') window.__chatConnects += 1;
    // 채널 정보는 받았지만 채팅 연결 요청이 끊기는 순간 실패한 경우.
    if (cmd === 'chat_connect' && window.__failChatOnce) { window.__failChatOnce = false; throw new Error('채팅 서버에 연결하지 못했습니다'); }
    if (window.__offline && cmd !== 'ui_heartbeat' && cmd !== 'chat_disconnect') {
      window.__calls.push(cmd);
      throw new Error('error sending request: connection lost');
    }
    if (crashAtStart && cmd === 'settings_get') {
      window.__calls.push(cmd);
      // 시작 코드가 설정을 읽다가 예외를 내게 한다(예상하지 못한 시작 실패 흉내).
      return { general: {}, appearance: { get theme() { throw new Error('boom'); } } };
    }
    if (extra[cmd]) { window.__calls.push(cmd); return extra[cmd](args || {}); }
    return inner(cmd, args);
  };
}

const browser = await chromium.launch();
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); console.log(`PASS ${id}`); } catch (e) { results.push({ id, ok: false }); console.log(`FAIL ${id}: ${e.message}`); }
}

async function open({ crashAtStart = false, clock = false, failChatOnce = false, query = '' } = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 860 }, colorScheme: 'dark' });
  const page = await ctx.newPage();
  // 2분 주기 타이머(사이드바 팔로잉)를 앞당겨 돌리려고 시계를 바꿔 둔다 — 그 외에는 평소처럼 흐른다.
  if (clock) await page.clock.install();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, 'dark']);
  await page.addInitScript(offlineLayer, [crashAtStart, failChatOnce]);
  await page.addInitScript(() => { try { localStorage.setItem('cview.welcomed.v1', '1'); } catch { /* 무시 */ } });
  await page.goto(base + query);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.waitForFunction(() => !document.getElementById('splash'), null, { timeout: 8000 });
  return { ctx, page, errors };
}

const count = (page, cmd) => page.evaluate((c) => window.__calls.filter((x) => x === c).length, cmd);
const sideItems = (page) => page.evaluate(() => document.querySelectorAll('#sidebar-live [data-channel-id]').length);
const navBadge = (page) => page.evaluate(() => {
  const b = document.querySelector('.nav-item[data-route="following"] .nav-count, .nav-item[data-route="following"] .badge');
  return b ? b.textContent.trim() : '';
});

{
  const { ctx, page, errors } = await open({ clock: true });

  await check('O01 heartbeat at start and on every ping', async () => {
    const b0 = await page.evaluate(() => window.__beats || 0);
    assert.ok(b0 >= 1, `시작 신호 ${b0}`);
    assert.equal(typeof (await page.evaluate(() => typeof window.__cviewBeat)), 'string');
    await page.evaluate(() => window.__cviewBeat());
    await page.waitForFunction((n) => (window.__beats || 0) > n, b0);
  });

  await go(page, 'home');
  await page.waitForFunction(() => document.querySelectorAll('#sidebar-live [data-channel-id]').length > 0);
  const itemsBefore = await sideItems(page);
  const badgeBefore = await navBadge(page);

  // 사이드바 팔로잉 갱신은 즐겨찾기 목록(favorites_list)을 함께 부른다 — 그 호출로 사이드바 갱신이 돌았는지 안다.
  await check('O02 failed periodic refresh while offline keeps the sidebar following list', async () => {
    const s0 = await count(page, 'favorites_list');
    await page.evaluate(() => { window.__offline = true; });
    await ctx.setOffline(true);
    await page.clock.fastForward('02:05'); // 2분 주기 갱신이 끊긴 동안 돈다 → 실패
    await page.waitForFunction((n) => window.__calls.filter((x) => x === 'favorites_list').length > n, s0, { timeout: 4000 });
    await page.waitForTimeout(200);
    assert.equal(await sideItems(page), itemsBefore, '사이드바 팔로잉 목록이 지워졌다');
    assert.equal(await navBadge(page), badgeBefore, '팔로잉 개수가 지워졌다');
  });

  await check('O03 coming back online refetches sidebar following right away', async () => {
    const s0 = await count(page, 'favorites_list');
    await page.evaluate(() => { window.__offline = false; });
    await ctx.setOffline(false);
    // 다음 주기(2분)·실패 재시도를 기다리지 않고 복구 직후(잠잠해진 뒤 0.8초) 바로.
    await page.waitForFunction((n) => window.__calls.filter((x) => x === 'favorites_list').length > n, s0, { timeout: 3000 });
    assert.ok(await sideItems(page) > 0);
  });

  await check('O05 flapping online events run recovery once', async () => {
    await go(page, 'home');
    await page.waitForTimeout(300);
    const f0 = await count(page, 'following_lives');
    for (let i = 0; i < 4; i++) { await ctx.setOffline(true); await ctx.setOffline(false); }
    await page.waitForTimeout(1600);
    const n = (await count(page, 'following_lives')) - f0;
    // 사이드바 갱신 한 번 + 홈 다시 불러오기(팔로잉 방송) 한 번 = 2. 흔들린 횟수(4)만큼 늘면 실패.
    assert.ok(n >= 1 && n <= 2, `팔로잉 요청 ${n}회`);
    const toasts = await page.evaluate(() => [...document.querySelectorAll('.toast')].filter((t) => /다시 연결되었습니다/.test(t.textContent)).length);
    assert.ok(toasts <= 1, `복구 알림 ${toasts}개`);
  });

  await check('O06 no unhandled page errors during offline/online', async () => {
    assert.deepEqual(errors, []);
  });
  await ctx.close();
}

{
  // 단독 시청 화면(보조 플레이어 창의 viewer)은 복구 때 화면을 다시 불러오지 않는다 — 채팅은 따로 다시 붙어야 한다.
  const { ctx, page, errors } = await open({ failChatOnce: true, query: '?w=player&channelId=ch2' });
  await check('O04 viewer chat that failed to connect while offline reconnects when back online', async () => {
    await page.waitForFunction(() => !document.getElementById('viewer').hidden);
    await page.waitForFunction(() => [...document.querySelectorAll('.chat-state')].some((s) => s.offsetParent && /실패/.test(s.textContent)));
    const c0 = await page.evaluate(() => window.__chatConnects);
    await ctx.setOffline(true);
    await ctx.setOffline(false);
    await page.waitForFunction((n) => window.__chatConnects > n, c0, { timeout: 4000 });
    await page.waitForFunction(() => ![...document.querySelectorAll('.chat-state')].some((s) => s.offsetParent && /실패/.test(s.textContent)));
    assert.deepEqual(errors, []);
  });
  await ctx.close();
}

{
  const { ctx, page } = await open({ crashAtStart: true }).catch(async (e) => { throw new Error(`시작 화면이 걷히지 않았다: ${e.message}`); });
  await check('O07 startup exception hides the splash and offers a reload', async () => {
    assert.equal(await page.evaluate(() => Boolean(document.getElementById('splash'))), false);
    const t = await page.evaluate(() => [...document.querySelectorAll('.toast')].map((x) => x.textContent).join(' | '));
    assert.match(t, /화면을 준비하지 못했습니다/);
    assert.match(t, /다시 불러오기/);
  });
  await ctx.close();
}

await browser.close();
server.close();
const failed = results.filter((r) => !r.ok);
console.log(failed.length ? `${failed.length} failed` : `all ${results.length} passed`);
process.exit(failed.length ? 1 : 0);
