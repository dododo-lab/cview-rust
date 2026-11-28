// 기능 정밀 고도화 17차(2026-11-28) 회귀 검사 — 단축키·메뉴 기본값 겹침, 멀티채팅 검색, 팔로잉 새로고침 실패, 같은 검색 두 번.
// 모의 IPC·헤드리스 브라우저(실제 서비스·데이터는 건드리지 않는다). CVIEW_UI_BROWSER=webkit 이면 WebKit.
// 저장소 루트에서: NODE_PATH=<Playwright 경로> node tools/ui-regression/precision-pass-17.mjs
// Z01 단축키 — 다른 동작의 남아 있는 메뉴 기본값(맨 S 로 바꾼 스크린샷의 ⌘S)과 겹치면 막는다. 도움말은 실제로 듣는 메뉴 키만 보인다.
// Z02 멀티채팅 검색 — 시각·채널 꼬리표가 아니라 닉네임·본문만 찾는다.
// Z03 팔로잉 — 보이던 목록이 있으면 새로고침이 실패해도 목록을 지킨다(오류 화면으로 바꾸지 않는다).
// Z04 검색 — 같은 검색어를 받는 중 다시 찾으면 첫 결과를 기다린다(빈 결과 화면이 뜨지 않는다).
// Z05 코드 확인 — 팔로잉 19세 표시, 통합 목록 자동 스크롤 설정, 강조를 모든 목록에, 검색 중 이어진 줄 이름, 다시보기 동안 뒤 방송
//     소리 끄기·계정 변경 때 다시보기 유지·탐색 뒤 위치 저장, 손으로 고른 화질 기억, 창 이름 겹침·깨진 프리셋 파일.
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
  const subs = new Map();
  const listen = async (event, cb) => { (subs.get(event) || subs.set(event, []).get(event)).push(cb); return () => {}; };
  window.__TAURI__.event = { listen };
  window.__TAURI__.webviewWindow = { getCurrentWebviewWindow: () => ({ listen }) };
  window.__emit = (event, payload) => { for (const cb of subs.get(event) || []) cb({ payload }); };
  document.hasFocus = () => true;
}
function overlay(extra) {
  const fns = Object.fromEntries(Object.entries(extra).map(([k, v]) => [k, new Function(`return (${v})`)()]));
  const inv = window.__TAURI__.core.invoke;
  window.__TAURI__.core.invoke = async (c, a) => (fns[c] ? fns[c](a || {}) : inv(c, a));
}
async function open({ extra = {}, init = null } = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 }, colorScheme: 'dark', userAgent: MAC_UA });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, 'dark', 12]);
  await page.addInitScript(overlay, Object.fromEntries(Object.entries(extra).map(([k, v]) => [k, v.toString()])));
  await page.addInitScript(harness);
  if (init) await page.addInitScript(init);
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  return { ctx, page, errors };
}

try {
  await check('Z01 단축키 — 남아 있는 메뉴 기본값과 겹치면 막고, 도움말은 듣는 키만', async () => {
    const { ctx, page, errors } = await open();
    const r = await page.evaluate(() => import('/js/shortcuts.js').then((m) => {
      const C = m.MOD.cmd;
      const defaults = Object.fromEntries(m.ACTIONS.map(([a]) => [a, m.defaultBinding(a)]));
      const plainShot = { ...defaults, screenshot: { key: 's', modifiers: 0 } };
      return {
        blocked: m.menuDefaultOf('bookmarkStream', { key: 's', modifiers: C }, plainShot),
        free: m.menuDefaultOf('bookmarkStream', { key: 's', modifiers: C }, defaults),
        own: m.menuDefaultOf('screenshot', { key: 's', modifiers: C }, plainShot),
        keeps: [m.menuKeepsDefault({ key: 'p', modifiers: 0 }), m.menuKeepsDefault({ key: 'p', modifiers: m.MOD.shift }), m.menuKeepsDefault({ key: 'p', modifiers: C })],
      };
    }));
    assert.equal(r.blocked, '스크린샷(메뉴)');
    assert.equal(r.free, null, '스크린샷이 ⌘S 를 직접 쓰면 메뉴 기본값 겹침이 아니라 동작끼리 비교에서 걸린다');
    assert.equal(r.own, null);
    assert.deepEqual(r.keeps, [true, true, false]);
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('Z02 멀티채팅 검색 — 닉네임·본문만 찾는다', async () => {
    const { ctx, page, errors } = await open();
    const r = await page.evaluate(() => import('/js/multichat.js').then(({ lineSearchText }) => {
      const box = document.createElement('div');
      box.innerHTML = '<div class="chat-line msg" data-nick="도도"><span class="mc-tag">릴카 방송</span><time class="chat-time">12:34</time><span class="text">안녕하세요 <img class="emoji" alt="d_1"></span></div>';
      const t = lineSearchText(box.firstElementChild).toLowerCase();
      return { time: t.includes('12'), tag: t.includes('릴카'), nick: t.includes('도도'), body: t.includes('안녕'), emoji: t.includes('d_1') };
    }));
    assert.deepEqual(r, { time: false, tag: false, nick: true, body: true, emoji: true });
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('Z03 팔로잉 — 새로고침이 실패해도 보이던 목록을 지킨다', async () => {
    const { ctx, page, errors } = await open({
      init: () => {
        const inv = window.__TAURI__.core.invoke;
        window.__TAURI__.core.invoke = async (c, a) => {
          if (c === 'following_lives' && window.__failFollow) throw new Error('서버 오류');
          return inv(c, a);
        };
      },
    });
    await go(page, 'following');
    await page.waitForFunction(() => document.querySelectorAll('[data-page="following"] [data-channel-id]').length > 0 || document.querySelector('[data-page="following"] .empty'));
    const before = await page.$$eval('[data-page="following"] [data-channel-id]', (x) => x.length);
    if (!before) { await ctx.close(); throw new Error('첫 목록이 없다(모의 응답 확인 필요)'); }
    await page.evaluate(() => { window.__failFollow = true; });
    await page.evaluate(() => window.__emit('menu://action', { id: 'refresh' }));
    await page.waitForTimeout(900);
    const after = await page.$$eval('[data-page="following"] [data-channel-id]', (x) => x.length);
    const errorShown = await page.$('[data-page="following"] .page-body > .empty.error');
    assert.ok(!errorShown, '오류 화면으로 바뀌었다');
    assert.ok(after > 0, '목록이 사라졌다');
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('Z04 검색 — 같은 검색어를 두 번 찾아도 첫 결과를 쓴다', async () => {
    const { ctx, page, errors } = await open({
      extra: {
        search: () => new Promise((res) => {
          window.__searchCalls = (window.__searchCalls || 0) + 1;
          const n = window.__searchCalls;
          // 둘째 호출은 Rust 처럼 '진행 중인 같은 검색' — 빈 결과를 바로 돌려준다
          if (n === 2) { res({ lives: [], channels: [], clips: [], hasMoreClips: false, errors: {} }); return; }
          setTimeout(() => res({ lives: [], channels: [{ channelId: 'ch1', channelName: '플러리', channelImageUrl: '', openLive: false }], clips: [], hasMoreClips: false, errors: {} }), 400);
        }),
      },
    });
    await go(page, 'search');
    const input = '[data-page="search"] input[type=search]';
    await page.waitForSelector('[data-page="search"]:not([hidden]) input[type=search]');
    await page.fill(input, '플러리');
    await page.press(input, 'Enter');
    await page.waitForTimeout(60);
    await page.press(input, 'Enter');
    await page.waitForSelector('[data-page="search"] .channel-row[data-channel-id="ch1"]', { timeout: 4000 });
    assert.equal(await page.evaluate(() => window.__searchCalls), 1, '같은 검색을 두 번 보냈다');
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('Z05 코드 확인 — 19세·자동 스크롤·강조·이어진 줄·다시보기 소리·계정 변경·위치 저장·화질·창·프리셋', async () => {
    assert.match(read('crates/cview-app/src/dto.rs'), /adult: cview_app_core::caching::following_store::is_adult_live\(&item\.channel_id\)/);
    assert.match(read('crates/cview-app-core/src/caching/following_store.rs'), /note_adult_live\(channel_id, info\.adult\);/);
    assert.match(src('multichat.js'), /chatFollower\(this\.mergedList, mergedAutoScroll\)/);
    assert.match(src('chat.js'), /for \(const uid of new Set\(\[\.\.\.lit, \.\.\.highlightSeen\]\)\)/);
    assert.match(read('app-ui/css/app.css'), /\.chat-skin \.chat-list\.searching \.chat-line\.cont \.msg-head/);
    const app = src('app.js');
    assert.match(app, /else if \(owner === 'media'\) \{\n[\s\S]*?viewer\.suspendAudio\(true\);\n\s+mlAudioSuspended\(true\);/);
    assert.match(app, /if \(current === 'media' && pages\.media\.video && pages\.media\.video\.isConnected\)/);
    assert.match(src('media.js'), /video\.addEventListener\('seeked', \(\) => this\.saveProgress\(\)\);/);
    const player = read('app-ui/player/player.html');
    assert.match(player, /function pickManualLevel\(target\)/);
    assert.match(player, /if \(_userManualLevel >= 0 && hls\.autoLevelEnabled\) restoreUserManualLevel\(\);/);
    assert.match(read('crates/cview-app/src/windows.rs'), /NEXT_WINDOW\.fetch_add/);
    assert.match(read('crates/cview-app-core/src/services/channel_preset_store.rs'), /프리셋 파일 형식이 올바르지 않습니다/);
  });
} finally {
  await browser.close();
  server.close();
}
console.log(`\n${total - failures.length}/${total} 통과${failures.length ? ` — 실패: ${failures.join(', ')}` : ''}`);
process.exit(failures.length ? 1 : 0);
