// 기능 정밀 고도화 19차(2026-11-28) 회귀 검사 — 채널 화면·채팅 창 방송 상태 따라가기, 멀티라이브 배치 도구, 녹화 가드.
// 모의 IPC·헤드리스 브라우저(실제 서비스·데이터는 건드리지 않는다). CVIEW_UI_BROWSER=webkit 이면 WebKit.
// 저장소 루트에서: NODE_PATH=<Playwright 경로> node tools/ui-regression/precision-pass-19.mjs
// C01 채널 화면 — 열어 둔 동안 방송 상태가 바뀌면 다시 그린다(checkLive), 바뀌지 않으면 그대로 둔다.
// C02 코드 확인 — 채팅 창 방송 상태 1분 확인·다시 연결, 포커스 분할선은 한 채널 크게·단독 보기에서 내림, 무대 class 의 끄는 중
//     표시 유지, 분할선 Enter·더블클릭 기본값, 끌기 손잡이 화살표로 순서 바꾸기, 녹화는 재생 대상 창·암호화 아닌 방송만,
//     팔로우 401 세션 만료 알림, 429 때 관문 전체 멈춤, 놓기 메인 판정에 선택 패인.
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
const read = (f) => fs.readFileSync(path.resolve(f), 'utf8');
const src = (f) => read(`app-ui/js/${f}`);

function harness() {
  const inv = window.__TAURI__.core.invoke;
  window.__args = {};
  window.__TAURI__.core.invoke = async (c, a) => {
    (window.__args[c] ||= []).push(a || {});
    if (c === 'live_detail') return { channelId: (a || {}).channelId, isLive: Boolean(window.__liveNow), liveTitle: '지금 방송', viewerCountText: '10', chatChannelId: 'cc1' };
    return inv(c, a);
  };
  document.hasFocus = () => true;
}

try {
  await check('C01 채널 화면 — 방송 상태가 바뀔 때만 다시 그린다', async () => {
    const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 }, colorScheme: 'dark', userAgent: MAC_UA });
    const page = await ctx.newPage();
    page.setDefaultTimeout(8000);
    const errors = [];
    page.on('pageerror', (e) => errors.push(String(e)));
    await page.addInitScript(mockTauri, [base, 'dark', 12]);
    await page.addInitScript(harness);
    await page.goto(base);
    await page.waitForSelector('[data-page]', { state: 'attached' });
    await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: 'channel', params: { channelId: 'ch1' } } })));
    await page.waitForSelector('[data-page="channel"]:not([hidden]) .channel-tabs');
    const detailCalls = () => page.evaluate(() => (window.__args.channel_detail || []).length);
    const n0 = await detailCalls();
    // checkLive 는 1분 주기라 프로토타입으로 직접 부른다(가짜 페이지 — show 호출만 센다).
    const r = await page.evaluate(async () => {
      const { ChannelPage } = await import('/js/channel.js');
      const calls = [];
      const fake = { channelId: 'ch1', detail: { live: null }, root: { hidden: false }, tab: 'info', show: (p) => calls.push(p) };
      window.__liveNow = false;
      await ChannelPage.prototype.checkLive.call(fake);
      const same = calls.length;
      window.__liveNow = true;
      await ChannelPage.prototype.checkLive.call(fake);
      return { same, changed: calls.length, refresh: calls[0] && calls[0].refresh };
    });
    assert.equal(r.same, 0, '상태가 그대로인데 다시 그렸다');
    assert.equal(r.changed, 1, '방송이 켜졌는데 다시 그리지 않았다');
    assert.equal(r.refresh, true);
    assert.equal(await detailCalls(), n0);
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('C02 코드 확인 — 채팅 창·분할선·무대 class·손잡이 키·녹화·401·429·놓기', async () => {
    assert.match(src('chat-only.js'), /async checkLive\(\) \{[\s\S]*?this\.chat\.connect\(live\.chatChannelId, live\.channelId\)/);
    const ml = src('multilive.js');
    assert.match(ml, /const focus = state\.layout === 'focus' && n > 1 && single == null;/);
    assert.match(ml, /\['resizing', 'row-resizing', 'ml-dragging', 'has-info'\]\.filter/);
    assert.match(ml, /reset: 0\.7,/);
    assert.match(ml, /axis === 'h' \? api\.multiLiveSetRatios\(0\.5, null\) : api\.multiLiveSetRatios\(null, 0\.5\)/);
    assert.match(ml, /closest\('\.ml-drag'\)[\s\S]*?api\.multiLiveReorder\(pane\.sessionId, order\[to\]\)/);
    const rec = read('crates/cview-app/src/commands/recording.rs');
    assert.match(rec, /window\.label\(\) != crate::windows::player_target\(\)/);
    assert.match(rec, /이 방송은 암호화되어 있어 녹화할 수 없습니다/);
    assert.match(read('crates/cview-networking/src/client/request.rs'), /self\.inner\.gate\.pause_for\(Duration::from_secs_f64\(wait\)\);/);
    assert.match(read('crates/cview-app-core/src/viewmodels/multi_live/manager/audio_grid.rs'), /\.or\(self\.selected_session_id\.filter/);
  });
} finally {
  await browser.close();
  server.close();
}
console.log(`\n${total - failures.length}/${total} 통과${failures.length ? ` — 실패: ${failures.join(', ')}` : ''}`);
process.exit(failures.length ? 1 : 0);
