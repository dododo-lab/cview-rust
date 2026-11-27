// 기능 정밀 고도화 12차(2026-11-27) 회귀 검사 — 보조 창 메뉴·단축키, 오류 복구 표시, 11차 수정의 빈틈.
// 모의 IPC·헤드리스 브라우저(실제 서비스·데이터는 건드리지 않는다). CVIEW_UI_BROWSER=webkit 이면 WebKit.
// 저장소 루트에서: NODE_PATH=<Playwright 경로> node tools/ui-regression/precision-pass-12.mjs
// R01 보조 창(채팅 창)에서 화면 이동 메뉴·⌥⌘숫자·⌘, 는 그 창을 바꾸지 않고 메인 창으로 넘긴다. ⌘W(시청 화면 닫기)는 창을 닫는다.
// R02 인사이트 — 로그인한 채 팔로잉 받기에 실패하면 "로그인하면 …"이 아니라 실패로 알린다(홈·팔로잉).
// R03 인사이트 '많이 보는 팔로잉' — 상위 10위 밖 팔로잉도 후보(channelPool)에서 고르고, 팔로잉 0명이면 비팔로우를 섞지 않는다.
// R04 채팅 '읽기 폭' 메뉴에서 Tab — 연 단추 다음 요소로 간다(문서 처음으로 튀지 않는다).
// R05 코드 확인 — 넣는 중인 둘러본 채널은 지난 구성 실패 채널 정리에서 빼고, 무대에 올리는 사이 다른 채널을 누르면 광고를
//     틀지 않으며, 패인 소리 단추의 첫 낭독 이름이 보이는 이름과 같고, 초점 없는 앱 메뉴를 메인 창이 받는다.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { base, server, mockTauri, go } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const WEBKIT = process.env.CVIEW_UI_BROWSER === 'webkit';
const browser = await (WEBKIT ? pw.webkit : pw.chromium).launch();
const MAC_UA = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)';
const failures = [];
let total = 0;
async function check(name, fn) {
  total += 1;
  try { await fn(); console.log(`ok   ${name}`); } catch (e) { failures.push(name); console.log(`FAIL ${name}\n     ${e.message.split('\n').join('\n     ')}`); }
}
const src = (f) => fs.readFileSync(path.resolve('app-ui/js', f), 'utf8');

/** 명령 인자를 window.__args[명령] 에 모으고, 이벤트 구독을 window.__emit 으로 흉내 낸다. */
function harness() {
  const inv = window.__TAURI__.core.invoke;
  window.__args = {};
  window.__TAURI__.core.invoke = async (c, a) => { (window.__args[c] ||= []).push(a || {}); return inv(c, a); };
  const subs = new Map();
  const listen = async (event, cb) => { (subs.get(event) || subs.set(event, []).get(event)).push(cb); return () => {}; };
  window.__TAURI__.event = { listen };
  window.__TAURI__.webviewWindow = { getCurrentWebviewWindow: () => ({ listen }) };
  window.__emit = (event, payload) => { for (const cb of subs.get(event) || []) cb({ payload }); };
  document.hasFocus = () => true; // 헤드리스는 창 초점이 없을 수 있다 — 앞에 있는 창으로 친다
}
function overlay(extra) {
  const fns = Object.fromEntries(Object.entries(extra).map(([k, v]) => [k, new Function(`return (${v})`)()]));
  const inv = window.__TAURI__.core.invoke;
  window.__TAURI__.core.invoke = async (c, a) => (fns[c] ? fns[c](a || {}) : inv(c, a));
}

async function open({ url = '', extra = {}, width = 1280, waitPage = true } = {}) {
  const ctx = await browser.newContext({ viewport: { width, height: 900 }, colorScheme: 'dark', userAgent: MAC_UA });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, 'dark', 12]);
  await page.addInitScript(overlay, Object.fromEntries(Object.entries(extra).map(([k, v]) => [k, v.toString()])));
  await page.addInitScript(harness);
  await page.goto(base + url);
  if (waitPage) await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
  return { ctx, page, errors };
}
const args = (page, cmd) => page.evaluate((c) => window.__args[c] || [], cmd);
const visible = (page, route) => page.evaluate((r) => { const el = document.querySelector(`[data-page="${r}"]`); return Boolean(el && !el.hidden); }, route);

try {
  await check('R01 보조 창 — 화면 이동 메뉴·단축키는 메인 창으로, ⌘W 는 창 닫기', async () => {
    const { ctx, page, errors } = await open({ url: '?w=chat&channelId=ch1', extra: { tray_control: () => null, window_close_self: () => null } });
    await page.waitForFunction(() => window.__args.chat_connect || document.querySelector('[data-page="chat"]:not([hidden])'), null, { timeout: 8000 }).catch(() => {});
    await page.waitForTimeout(300);
    const before = await page.evaluate(() => [...document.querySelectorAll('[data-page]')].filter((p) => !p.hidden).map((p) => p.dataset.page));
    for (const id of ['nav-home', 'nav-multilive', 'settings', 'search']) {
      await page.evaluate((i) => window.__emit('menu://action', { id: i }), id);
    }
    await page.keyboard.press('Meta+Alt+Digit1');
    await page.keyboard.press('Meta+Comma');
    await page.waitForTimeout(200);
    const after = await page.evaluate(() => [...document.querySelectorAll('[data-page]')].filter((p) => !p.hidden).map((p) => p.dataset.page));
    assert.deepEqual(after, before, '보조 창의 화면이 바뀌었다');
    assert.equal(await visible(page, 'multilive'), false);
    const routes = (await args(page, 'tray_control')).map((a) => a.action && a.action.kind === 'open-route' && a.action.route);
    assert.deepEqual(routes, ['home', 'multilive', 'settings', 'search', 'home', 'settings']);
    assert.equal((await args(page, 'multi_live_add')).length, 0);
    await page.evaluate(() => window.__emit('menu://action', { id: 'close-viewer' }));
    await page.waitForTimeout(100);
    assert.equal((await args(page, 'window_close_self')).length, 1, '⌘W 가 보조 창을 닫지 않는다');
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('R01b 메인 창 — 화면 이동 메뉴는 그대로, ⌘W 는 창을 닫지 않음', async () => {
    const { ctx, page } = await open({ extra: { window_close_self: () => null } });
    await page.evaluate(() => window.__emit('menu://action', { id: 'nav-following' }));
    await page.waitForFunction(() => !document.querySelector('[data-page="following"]').hidden);
    await page.evaluate(() => window.__emit('menu://action', { id: 'close-viewer' }));
    await page.waitForTimeout(100);
    assert.equal((await args(page, 'window_close_self')).length, 0);
    assert.equal((await args(page, 'tray_control')).length, 0);
    await ctx.close();
  });

  await check('R02 인사이트 — 팔로잉 받기 실패를 "로그인하면 …"으로 말하지 않는다', async () => {
    const { ctx, page } = await open({ width: 1600, extra: { following_lives: () => { throw new Error('오프라인'); } } });
    for (const [route, want] of [['following', '팔로잉 인사이트'], ['home', '오늘의 인사이트']]) {
      await go(page, route);
      await page.waitForFunction((w) => (document.querySelector('.ip-scroll')?.textContent || '').includes(w), want);
      await page.waitForTimeout(150);
      const text = await page.evaluate(() => document.querySelector('.ip-scroll').textContent);
      assert.ok(!text.includes('로그인하면'), `${route}: ${text.slice(0, 120)}`);
      assert.ok(text.includes('불러오지 못했어요'), `${route}: ${text.slice(0, 120)}`);
    }
    await ctx.close();
  });

  await check('R03 인사이트 — 10위 밖 팔로잉도 고르고, 팔로잉 0명이면 비팔로우를 섞지 않는다', async () => {
    const pool = Array.from({ length: 13 }, (_, i) => ({ name: `채널${i}`, seconds: 9000 - i * 300, channelId: `x${i}` }));
    pool[12].channelId = 'ch1';
    const overviewFn = `() => ({ total: 1, last7: 1, last30: 1, streak: 1, average: 1, channels: ${JSON.stringify(pool.slice(0, 10))}, channelPool: ${JSON.stringify(pool)}, categories: [], hourly: [], daily: [], sessionCount: 1, countedSessionCount: 1, uniqueChannelCount: 13, favoriteCount: 0, bookmarkCount: 0, recent: [] })`;
    const run = async (all) => {
      const { ctx, page } = await open({ width: 1600, extra: { insights_overview: overviewFn, following_channels_all: `() => (${JSON.stringify(all)})` } });
      await go(page, 'following');
      await page.waitForFunction(() => (document.querySelector('.ip-scroll')?.textContent || '').includes('많이 보는 팔로잉') || (document.querySelector('.ip-scroll')?.textContent || '').includes('팔로잉 인사이트'));
      await page.waitForTimeout(200);
      const text = await page.evaluate(() => document.querySelector('.ip-scroll').textContent);
      await ctx.close();
      return text;
    };
    const t1 = await run([{ channelId: 'ch1', channelName: '팔로잉' }]);
    assert.ok(t1.includes('채널12'), '12위인 팔로잉 채널이 빠짐');
    assert.ok(!t1.includes('채널0'), '비팔로우 채널이 섞임');
    const t0 = await run([]);
    assert.ok(!t0.includes('채널0') && !t0.includes('채널12'), '팔로잉 0명인데 비팔로우 채널이 섞임');
  });

  await check('R04 읽기 폭 메뉴 Tab — 연 단추 다음으로', async () => {
    const { ctx, page } = await open();
    const ok = await page.evaluate(async () => {
      const { showDesignMenu } = await import('/js/chat-extras.js');
      const wrap = document.createElement('div');
      wrap.innerHTML = '<button id="t-anchor">보기</button><button id="t-next">다음</button>';
      document.body.prepend(wrap);
      const anchor = document.getElementById('t-anchor');
      showDesignMenu(anchor);
      return document.activeElement && document.activeElement.closest('.design-menu') ? true : false;
    });
    assert.ok(ok, '메뉴에 초점이 들어가지 않음');
    await page.keyboard.press('Tab');
    const id = await page.evaluate(() => document.activeElement && document.activeElement.id);
    assert.equal(id, 't-next', `Tab 뒤 초점: ${id}`);
    assert.equal(await page.locator('.design-menu').count(), 0);
    await ctx.close();
  });

  await check('R05 코드 확인 — 둘러본 채널 판정·광고 순번·패인 소리 이름·초점 없는 앱 메뉴', async () => {
    const app = src('app.js');
    assert.match(app, /browsingChannels\.add\(channelId\)[\s\S]*?await pages\.multilive\.add\(/, '넣기 전에 둘러보는 채널로 표시');
    assert.match(app, /isTransient = \(sessionId, channelId\) => browsedWatch\.has\(sessionId\) \|\| \(channelId != null && browsingChannels\.has\(channelId\)\)/);
    assert.match(app, /await enterWatch\(s\.id\);\s*(\/\/.*\s*)*if \(openSeq !== seq \+ 1\) return;\s*pages\.multilive\.playPaneAd/, '무대에 올린 뒤에도 순번 확인');
    assert.match(app, /MAIN_MENU_IDS = new Set\(\['licenses', 'notice', 'about', 'check-update', 'settings', 'notification-settings'\]\)/);
    const ml = src('multilive.js');
    assert.match(ml, /const open = new Set\(kept\);/, '지난 구성 실패 채널 정리는 직접 넣은 세션만 본다');
    assert.match(ml, /isTransient\(x\.id, x\.channelId\)/);
    assert.match(src('ml-pane-controls.js'), /data-ml="audio" title="이 패인 소리 켜기" aria-label="이 패인 소리 켜기"/);
  });
} finally {
  await browser.close();
  server.close();
}
console.log(failures.length ? `\n${failures.length}/${total} 실패: ${failures.join(', ')}` : `\n${total}/${total} 통과 (${WEBKIT ? 'WebKit' : 'Chromium'})`);
process.exit(failures.length ? 1 : 0);
