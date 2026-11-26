// 기능 정밀 고도화 11차(2026-11-27) 회귀 검사 — 1~10차 상호작용, 지난 제외 재판단, 큰 목록·긴 채팅, 동시 조작, 접근성 낭독.
// 모의 IPC·헤드리스 브라우저(실제 서비스·데이터는 건드리지 않는다). CVIEW_UI_BROWSER=webkit 이면 WebKit.
// 저장소 루트에서: NODE_PATH=<Playwright 경로> node tools/ui-regression/precision-pass-11.mjs
// Q01 채널 화면 — 뒤로 가기가 아닌 재진입은 다시 받음, 뒤로 가기는 그대로, '다시 시도'는 다시 받음, 같은 채널 재진입은 기록을 쌓지 않음.
// Q02 홈 ⌘R 은 인기 클립도 응답 캐시를 건너뛴다(refresh), 채널 화면 ⌘R 은 방송 상태 캐시를 건너뛴다.
// Q03 팔로잉 › 전체 채널 '마지막 방송' — 방송 중이던 행이 꺼지면 다시 묻고, 캐시가 아직 '방송 중'이면 새로 묻는다.
// Q04 긴 채팅 — 위로 올려 읽는 중 앞줄을 지워도 보던 줄이 제자리에 남는다.
// Q05 채팅 '읽기 폭' 메뉴 — 초점이 들어가고 화살표로 옮기며 Esc 는 연 단추로 돌아온다.
// Q06 코드 확인 — 빠른 채널 열기 순번, 채널 전환 시 TTS 멈춤, 지난 구성 실패 채널 정리, 단독 보기 닫을 때 .ml-body 전체 화면 풀기,
//     팔로잉 인사이트 비팔로우 거르기.
// Q07 코드 확인 — 낭독 이름·상태(패인 재생·소리·화질, 채널 별, 녹화, 광고 소리, 접힌 로그인, 비율 슬라이더), Esc 뒤 초점 되돌리기.
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

/** 명령 인자를 window.__args[명령] 에 모은다. */
function recordArgs() {
  const inv = window.__TAURI__.core.invoke;
  window.__args = {};
  window.__TAURI__.core.invoke = async (c, a) => { (window.__args[c] ||= []).push(a || {}); return inv(c, a); };
}
function overlay(extra) {
  const fns = Object.fromEntries(Object.entries(extra).map(([k, v]) => [k, new Function(`return (${v})`)()]));
  const inv = window.__TAURI__.core.invoke;
  window.__TAURI__.core.invoke = async (c, a) => (fns[c] ? fns[c](a || {}) : inv(c, a));
}

async function open({ extra = {} } = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 }, colorScheme: 'dark', userAgent: MAC_UA });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, 'dark', 12]);
  await page.addInitScript(overlay, Object.fromEntries(Object.entries(extra).map(([k, v]) => [k, v.toString()])));
  await page.addInitScript(recordArgs);
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
  return { ctx, page, errors };
}
const count = (page, cmd) => page.evaluate((c) => (window.__args[c] || []).length, cmd);
const menu = (page, action) => page.evaluate((a) => window.dispatchEvent(new CustomEvent('cview:menu', { detail: a })), action);
const visible = (page, route) => page.evaluate((r) => { const el = document.querySelector(`[data-page="${r}"]`); return Boolean(el && !el.hidden); }, route);

try {
  await check('Q01 채널 화면 — 재진입은 다시 받고, 뒤로 가기는 그대로, 다시 시도는 다시 받고, 같은 채널 재진입은 기록을 쌓지 않는다', async () => {
    const { ctx, page, errors } = await open();
    await go(page, 'channel', { channelId: 'ch1' });
    await page.waitForSelector('[data-page="channel"] .channel-tabs');
    const first = await count(page, 'channel_detail');
    await go(page, 'home');
    await go(page, 'channel', { channelId: 'ch1' });
    await page.waitForFunction((n) => (window.__args.channel_detail || []).length > n, first);
    const reopened = await count(page, 'channel_detail');
    assert.equal(reopened, first + 1, '카드·말풍선으로 다시 열면 방송 상태를 다시 받는다');
    await go(page, 'home');
    await menu(page, 'back');
    await page.waitForFunction(() => !document.querySelector('[data-page="channel"]').hidden);
    await page.waitForTimeout(150);
    assert.equal(await count(page, 'channel_detail'), reopened, '뒤로 가기로 돌아오면 다시 받지 않는다');
    // 실패 화면의 '다시 시도'(전역 처리기 → page.retry)
    await page.evaluate(() => {
      const b = document.createElement('button');
      b.dataset.retry = '';
      b.id = 'q01-retry';
      document.querySelector('[data-page="channel"]').append(b);
      b.click();
    });
    await page.waitForFunction((n) => (window.__args.channel_detail || []).length > n, reopened);
    const retryArgs = await page.evaluate(() => window.__args.channel_detail.at(-1));
    assert.equal(retryArgs.refresh, true, '다시 시도는 캐시를 건너뛴다');
    // 기록 중복: ch1 → ch2 → 뒤로(ch1) → 같은 ch1 다시 열기 → 뒤로 = ch1 이전 화면(홈)
    await go(page, 'channel', { channelId: 'ch2' });
    await page.waitForFunction(() => (window.__args.channel_detail || []).some((a) => a.channelId === 'ch2'));
    await menu(page, 'back');
    await page.waitForTimeout(150);
    await go(page, 'channel', { channelId: 'ch1' });
    await page.waitForTimeout(150);
    await menu(page, 'back');
    await page.waitForTimeout(200);
    assert.ok(await visible(page, 'home'), '같은 채널을 다시 열어 쌓인 기록 때문에 뒤로 가기가 한 번 헛돌았다');
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('Q02 ⌘R — 홈 인기 클립·채널 방송 상태가 응답 캐시를 건너뛴다', async () => {
    const { ctx, page, errors } = await open();
    await page.waitForFunction(() => (window.__args.popular_clips || []).length > 0);
    assert.equal(await page.evaluate(() => window.__args.popular_clips[0].refresh), false);
    await menu(page, 'refresh');
    await page.waitForFunction(() => (window.__args.popular_clips || []).some((a) => a.refresh === true));
    await go(page, 'channel', { channelId: 'ch1' });
    await page.waitForSelector('[data-page="channel"] .channel-tabs');
    assert.equal(await page.evaluate(() => window.__args.channel_detail.at(-1).refresh), false);
    await menu(page, 'refresh');
    await page.waitForFunction(() => window.__args.channel_detail.at(-1).refresh === true);
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check("Q03 '마지막 방송' — 방송 중이던 행이 꺼지면 다시 묻고, 캐시가 '방송 중'이면 새로 묻는다", async () => {
    const { ctx, page, errors } = await open({
      extra: {
        channel_info: () => ({ channelId: 'z1', followerCount: 10, followerCountText: '10' }),
        // 캐시(refresh false)는 아직 방송 중, 새로 물으면 끝남
        live_detail: ({ refresh }) => (refresh ? { isLive: false, closeDate: new Date(Date.now() - 60000).toISOString(), liveTitle: '끝난 방송', categoryName: '게임' } : { isLive: true }),
      },
    });
    const r = await page.evaluate(async () => {
      const { FollowExtras } = await import('/js/following-info.js');
      const fx = new FollowExtras();
      const list = document.createElement('div');
      list.style.cssText = 'position:fixed;top:0;left:0;width:400px;z-index:99999;background:#000';
      const row = (live) => `<div data-fid="z1" data-live="${live ? 1 : 0}" style="height:40px"><span class="fx-followers"></span><span class="fx-last"></span></div>`;
      document.body.append(list);
      list.innerHTML = row(true);
      fx.attach(list);
      const wait = (f) => new Promise((res) => { const t = setInterval(() => { if (f()) { clearInterval(t); res(); } }, 20); setTimeout(() => { clearInterval(t); res(); }, 3000); });
      await wait(() => list.querySelector('.fx-followers').textContent);
      const liveCalls = (window.__args.live_detail || []).length;
      list.innerHTML = row(false); // 방송이 끝나 목록을 다시 그림
      fx.attach(list);
      await wait(() => list.querySelector('.fx-last').textContent);
      return { liveCalls, calls: window.__args.live_detail || [], last: list.querySelector('.fx-last').textContent };
    });
    assert.equal(r.liveCalls, 0, '방송 중 행은 라이브 상세를 묻지 않는다');
    assert.ok(r.calls.some((a) => a.refresh === true), JSON.stringify(r.calls));
    assert.match(r.last, /마지막 방송/);
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('Q04 긴 채팅 — 위로 올려 읽는 중 앞줄을 지워도 보던 줄이 제자리에 남는다', async () => {
    const { ctx, page, errors } = await open();
    const r = await page.evaluate(async () => {
      const { trimFront } = await import('/js/chat-follow.js');
      const list = document.createElement('div');
      list.style.cssText = 'position:fixed;top:0;left:0;width:300px;height:400px;overflow:auto;z-index:99999';
      list.innerHTML = Array.from({ length: 400 }, (_, i) => `<div data-i="${i}" style="height:20px">줄 ${i}</div>`).join('');
      document.body.append(list);
      list.scrollTop = 250 * 20; // 250번째 줄을 맨 위에(바닥보다 한참 위)
      const topRow = () => { const r0 = list.getBoundingClientRect(); return document.elementFromPoint(r0.left + 5, r0.top + 5)?.dataset.i; };
      const before = topRow();
      const cut = trimFront(list, 300, { keepView: true });
      await new Promise((res) => requestAnimationFrame(res));
      return { cut, before, after: topRow(), scrollTop: list.scrollTop };
    });
    assert.equal(r.cut, true);
    assert.equal(r.after, r.before, JSON.stringify(r));
    assert.ok(src('chat.js').includes('this.trimLines(!follow);'), '채팅 패널이 따라가지 않는 중에 보던 자리를 지킨다');
    assert.match(src('chat.js'), /!follow && fresh\.length && !this\.unread && !this\.list\.querySelector\('\.read-divider'\)/);
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check("Q05 '읽기 폭' 메뉴 — 초점·화살표·Esc 로 연 단추로", async () => {
    const { ctx, page, errors } = await open();
    const r = await page.evaluate(async () => {
      const { showDesignMenu } = await import('/js/chat-extras.js');
      const anchor = document.createElement('button');
      anchor.textContent = '읽기 폭';
      anchor.style.cssText = 'position:fixed;top:10px;right:10px;z-index:99999';
      document.body.append(anchor);
      anchor.focus();
      showDesignMenu(anchor);
      const menuEl = document.querySelector('.design-menu');
      const items = [...menuEl.querySelectorAll('[data-width]')];
      const roles = items.map((b) => [b.getAttribute('role'), b.getAttribute('aria-checked')]);
      const inMenu = menuEl.contains(document.activeElement);
      const firstFocus = document.activeElement.dataset.width;
      document.activeElement.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true }));
      const moved = document.activeElement.dataset.width !== firstFocus && menuEl.contains(document.activeElement);
      document.activeElement.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
      return { roles, inMenu, moved, closed: !document.querySelector('.design-menu'), back: document.activeElement === anchor };
    });
    assert.ok(r.roles.every(([role]) => role === 'menuitemradio'), JSON.stringify(r.roles));
    assert.equal(r.roles.filter(([, c]) => c === 'true').length, 1);
    assert.ok(r.inMenu && r.moved && r.closed && r.back, JSON.stringify(r));
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('Q06 코드 확인 — 열기 순번·TTS 멈춤·지난 구성 실패 채널·단독 보기 전체 화면·팔로잉 인사이트', async () => {
    const app = src('app.js');
    assert.match(app, /const seq = \+\+openSeq;/);
    assert.match(app, /if \(seq !== openSeq\) return; \/\/ 그새 다른 채널을 눌렀다/);
    assert.match(app, /async function enterWatch\(sessionId, \{ push = true \} = \{\}\) \{\n\s*openSeq\+\+;/);
    assert.match(src('chat.js'), /!\[\.\.\.PANELS\]\.some\(\(p\) => p !== this && p\.connected\)\) speechSynthesis\.cancel\(\);/);
    const ml = src('multilive.js');
    assert.match(ml, /this\.unopened = this\.unopened\.filter\(\(id\) => !open\.has\(id\)\);/);
    assert.match(ml, /if \(!this\.restoring\) this\.unopened = \[\];/);
    assert.match(ml, /wasWatch && this\.watchSlot == null && document\.fullscreenElement\s*\n\s*&& document\.fullscreenElement === this\.root\.querySelector\('\.ml-body'\)/);
    assert.match(src('insight-panel.js'), /c\.seconds >= 60 && isFollowed\(c\)/);
  });

  await check('Q07 코드 확인 — 낭독 이름·상태와 Esc 뒤 초점', async () => {
    const pane = src('ml-pane-controls.js');
    assert.match(pane, /this\.playButton\.setAttribute\('aria-label', this\.playButton\.title\);/);
    assert.match(pane, /this\.audioButton\.setAttribute\('aria-label', audioTitle\);/);
    assert.match(pane, /role="menuitemradio" aria-checked="\$\{o\.index === this\.manualIndex\}" data-mp="level"/);
    assert.match(src('channel.js'), /star\.setAttribute\('aria-label', star\.title\)/);
    assert.match(src('player-panel.js'), /this\.recButton\.setAttribute\('aria-label', on \? '녹화 중지' : '녹화 시작'\);/);
    assert.doesNotMatch(src('ads.js'), /setAttribute\('aria-pressed'/);
    assert.match(src('sidebar.js'), /account-login" data-action="login" data-tip="네이버 로그인" aria-label="네이버 로그인"/);
    assert.match(src('settings.js'), /aria-valuetext="\$\{esc\(fmt\(item, v\)\)\}"/);
    const app = src('app.js');
    assert.match(app, /const inside = \[\.\.\.popups\]\.some\(\(p\) => p\.contains\(active\)\);/);
    assert.match(app, /if \(inside && back && back\.isConnected/);
  });
} finally {
  await browser.close();
  server.close();
}
console.log(`${total - failures.length}/${total} passed`);
if (failures.length) { console.log(`failed: ${failures.join(', ')}`); process.exit(1); }
