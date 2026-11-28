// 기능 정밀 고도화 18차(2026-11-28) 회귀 검사 — 방송&클립 '더 보기', 녹화 중복 클릭, 채팅 묶음·새 메시지 수, 채팅 전용 화면 등.
// 모의 IPC·헤드리스 브라우저(실제 서비스·데이터는 건드리지 않는다). CVIEW_UI_BROWSER=webkit 이면 WebKit.
// 저장소 루트에서: NODE_PATH=<Playwright 경로> node tools/ui-regression/precision-pass-18.mjs
// B01 방송&클립 — '더 보기'가 한 줄도 더 받지 못했는데 다음 쪽이 남았으면 단추를 남긴다(잠깐 실패). 끝이면 숨긴다.
// B02 채팅 전용 화면 — 바꾼 채널을 경로 값으로 돌려준다(⌘R 이 처음 채널로 돌아가지 않는다).
// B03 코드 확인 — 메뉴 막대 재생 제어가 다시보기를 먼저, 줄을 지운 뒤 묶음 바로잡기, 바닥이 보이면 새 메시지 수 지우기,
//     녹화 중복 클릭 막기, 인사이트 시간 표기·뒤에 있을 때 받기 쉬기, 홈 인사·날짜 갱신, 채팅 전용 실패 시 앞 채팅 끊기,
//     끝난 방송 북마크 거절, 재생목록을 잃은 녹화 멈추기, 녹화 이름 겹침, 자동 실행 실패 되돌리기.
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
  window.__TAURI__.core.invoke = async (c, a) => { (window.__args[c] ||= []).push(a || {}); return inv(c, a); };
  document.hasFocus = () => true;
}
async function open({ init = null, liveCount = 12 } = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 }, colorScheme: 'dark', userAgent: MAC_UA });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, 'dark', liveCount]);
  await page.addInitScript(harness);
  if (init) await page.addInitScript(init);
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  return { ctx, page, errors };
}

/** 모의 popular_lives — 처음 30개까지만 준다(다음 쪽 실패 흉내). popular_has_more 는 window.__hasMore. */
function popularStub() {
  const inv = window.__TAURI__.core.invoke;
  window.__TAURI__.core.invoke = async (c, a) => {
    if (c === 'popular_lives') { const all = await inv(c, a); return all.slice(0, 30); }
    if (c === 'popular_has_more') return Boolean(window.__hasMore);
    return inv(c, a);
  };
}

try {
  for (const [label, more, expectButton] of [['다음 쪽이 남았으면 단추를 남긴다', true, true], ['끝이면 숨긴다', false, false]]) {
    await check(`B01 방송&클립 '더 보기' — ${label}`, async () => {
      const { ctx, page, errors } = await open({ init: popularStub, liveCount: 60 });
      await page.evaluate((m) => { window.__hasMore = m; }, more);
      await go(page, 'popular', { tab: 'live' });
      await page.waitForSelector('[data-page="popular"]:not([hidden]) [data-more]');
      await page.click('[data-page="popular"] [data-more]');
      await page.waitForFunction(() => (window.__args.popular_lives || []).some((a) => a.size === 60));
      await page.waitForTimeout(400);
      const shown = Boolean(await page.$('[data-page="popular"]:not([hidden]) [data-more]'));
      assert.equal(shown, expectButton);
      if (expectButton) {
        // 다른 화면(홈·인사이트)도 50개를 묻는다 — 방송&클립이 보낸 30·60 만 본다.
        const sizes = await page.evaluate(() => window.__args.popular_lives.map((a) => a.size).filter((n) => n === 30 || n === 60));
        assert.ok(sizes.includes(30) && sizes.includes(60), `더 보기 요청: ${sizes}`);
      }
      assert.deepEqual(errors, []);
      await ctx.close();
    });
  }

  await check('B02 채팅 전용 화면 — 바꾼 채널을 경로 값으로', async () => {
    const { ctx, page, errors } = await open();
    const r = await page.evaluate(() => import('/js/chat-only.js').then(({ ChatOnlyPage }) => {
      const fake = { channelId: 'ch7' };
      return [ChatOnlyPage.prototype.routeParams.call(fake), ChatOnlyPage.prototype.routeParams.call({ channelId: null })];
    }));
    assert.deepEqual(r, [{ channelId: 'ch7' }, {}]);
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('B03 코드 확인 — 재생 제어·채팅 묶음·새 메시지·녹화·인사이트·홈·채팅 전용·북마크·자동 실행', async () => {
    const app = src('app.js');
    assert.match(app, /const mediaFront = \(\) => \(current === 'media' && !viewer\.isOpen && pages\.media\?\.video\) \|\| null;/);
    assert.match(app, /case 'play-pause':\n\s+if \(mediaFront\(\)\)/);
    const chat = src('chat.js');
    assert.match(chat, /function regroupLines\(panel\)/);
    assert.match(chat, /if \(added\.length \|\| fresh\.length\) for \(const panel of PANELS\) regroupLines\(panel\);/);
    assert.match(chat, /if \(filtered && this\.atBottom\) this\.unread = 0; else this\.unread \+= visible;/);
    assert.match(src('player-panel.js'), /if \(this\.recBusy\) return;/);
    const insight = src('insight-panel.js');
    assert.match(insight, /if \(m < 1\) return '방금 시작';/);
    assert.match(insight, /if \(!document\.hasFocus\(\)\) \{ this\.awayStale = true; return; \}/);
    assert.match(src('home.js'), /if \(this\.greet && this\.greet\.isConnected\) this\.renderGreeting\(\);/);
    assert.match(src('chat-only.js'), /if \(this\.chat\.active\) this\.chat\.disconnect\(\);\n\s+this\.root\.querySelector\('\.chatonly-avatar'\)\.innerHTML = '';/);
    assert.match(read('crates/cview-app/src/commands/content.rs'), /방송 중이 아니라 북마크할 수 없습니다/);
    assert.match(read('crates/cview-player/src/recording/segments.rs'), /self\.stop_for_lost_playlist\(\);/);
    assert.match(read('crates/cview-app/src/commands/recording.rs'), /pub fn unique_recording_path/);
    assert.match(read('crates/cview-app/src/commands/settings.rs'), /if !crate::window_prefs::apply_general\(app, &services\.settings\.general\(\)\)/);
  });
} finally {
  await browser.close();
  server.close();
}
console.log(`\n${total - failures.length}/${total} 통과${failures.length ? ` — 실패: ${failures.join(', ')}` : ''}`);
process.exit(failures.length ? 1 : 0);
