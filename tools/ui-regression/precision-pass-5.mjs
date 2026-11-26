// 기능 정밀 고도화 5차(2026-11-26) 회귀 검사 — 3·4차에서 뺀 항목 재판단과 새 영역 검토로 찾은 실제 결함을 고친 것.
// 모의 IPC·헤드리스 브라우저(실제 서비스·데이터는 건드리지 않는다). CVIEW_UI_BROWSER=webkit 이면 WebKit.
// 저장소 루트에서: node tools/ui-regression/precision-pass-5.mjs
// T01 채팅 입력 — 이모지(UTF-16 2단위)를 써도 100자(코드포인트)까지 칠 수 있고, 넘게 붙여 넣으면 100자로 자른다.
// T02 네트워크 복구(online) — 채팅만 보기 화면은 다시 열지 않고(채팅 목록 유지), 목록 화면(홈)은 다시 받는다.
// T03 보조 플레이어 창 — 메인 창 멀티라이브에 음소거를 보내지 않는다.
// T04 채팅 창의 '방송 보기'(cview:play)는 그 창에서 재생하지 않고 플레이어 창을 연다.
// T05 채팅만 보기 — 채널 정보를 받는 사이 화면을 떠나면 숨은 화면에서 채팅을 연결하지 않는다.
// T06 시청 화면 — 방송 중이 아니고 채팅 채널도 없는 채널로 바꾸면 앞 채널 채팅을 끊는다.
// T07 외관 이벤트(다른 창·메뉴 막대·가져오기·초기화)의 테마를 이 창 화면도 따른다.
// T08 고른 카테고리 칩은 목록에서 빠져도 남는다(개수 0).
// T09 보관함 사이드바 '최근 본 채널' — 오래된 인기 목록으로 LIVE 를 판정하지 않는다.
// T10 홈 '더 많은 라이브 발견' — '전체' 칩 개수가 펼칠 수 있는 카드 수(36)를 넘지 않는다.
// T11 라이브 메뉴 '최근 시청'은 시청 기록(DB)을 먼저 쓴다.
// T12 상대 날짜 — 오늘·어제는 달력 날짜로, 360~364일 전은 '11개월 전', 갱신 캡션은 '60분 전'이 없다.
// T13 지연 프리셋 '표준'은 Rust 이름(default)으로 보낸다.
// T14 이모지로 시작하는 이름의 이니셜·멘션 알림 자르기·멀티라이브 무대 배율(코드 확인).
// (6차 — 라이브·네트워크)
// T15 맨 글자 단축키(M·L·B)는 한글 입력 상태(e.key 'ㅡ')에서도 물리 키로 알아본다.
// T16 시청 화면 — 다시 연결이 음소거를 되돌리지 않고, 녹화를 먼저 마무리하고, 재생 시작마다 화질을 다시 읽고,
//     무대 위 목록에서 휠은 스크롤하고, 닫을 때 전체 화면을 푼다(코드 확인 — 플레이어 iframe 은 모의 앱에서 돌지 않는다).
// (7차)
// T17 다시보기 — 오류 뒤 화질을 바꾸면 오류 막이 걷히고, '다시 시도'는 재생 주소를 새로 받아 고른 화질로 다시 연다.
// T18 채팅 설정 — 다른 창이 바꾼 설정(settings://chat)을 이 창도 따른다(차단 목록을 옛 값으로 덮지 않게).
// T19 채팅 — /unmute 로 /mute 를 풀고, 말풍선 카드 캐시는 팔로우 변경에 비워진다(코드 확인).
// T20 다시보기 한글 입력 단축키, 다운로드 '다시 받기'의 채널 이름(코드 확인).
// (8차)
// T21 메뉴 막대·트레이 '지금 재생 중'은 창만 열지 않고 재생 중인 화면(show-now-playing)으로 간다.
// T22 카테고리 없는 방송은 '기타'로 묶이고, '기타'를 고르면 그 방송들이 보인다.
// T23 검색 — 같은 검색 결과로 뒤로 돌아오면 다시 찾지 않고, 검색창을 비우면 ⌘R 이 지운 검색어로 다시 찾지 않는다.
// T24 다시보기 — 재생 정보를 광고보다 먼저 받는다(재생할 수 없는 영상은 광고 없이 바로 오류). 인기 클립 새로고침은 refresh 를 보낸다.
// T25 사이드바 사용률 그래프 배율(pixelScale)·시계 역행 처리·광고 볼륨 0(코드 확인).
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { base, server, mockTauri, go } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const WEBKIT = process.env.CVIEW_UI_BROWSER === 'webkit';
const browser = await (WEBKIT ? pw.webkit : pw.chromium).launch();
const failures = [];
let total = 0;
async function check(name, fn) {
  total += 1;
  try { await fn(); console.log(`ok   ${name}`); } catch (e) { failures.push(name); console.log(`FAIL ${name}\n     ${e.message.split('\n').join('\n     ')}`); }
}

/** 모의 앱 위에 명령을 덧씌우고(extra) 부른 명령을 기록한다. */
function overlay(extra) {
  window.__qCalls = [];
  const fns = Object.fromEntries(Object.entries(extra).map(([k, v]) => [k, new Function(`return (${v})`)()]));
  const inv = window.__TAURI__.core.invoke;
  window.__TAURI__.core.invoke = async (c, a) => { window.__qCalls.push([c, a]); return fns[c] ? fns[c](a || {}) : inv(c, a); };
}

/** 이벤트를 모아 두는 listen — 검사가 window.__ev[이름] 처리기를 직접 부른다. */
function captureEvents() {
  window.__ev = {};
  window.__TAURI__.event.listen = async (name, cb) => { (window.__ev[name] ||= []).push(cb); return () => {}; };
}

async function open({ extra = {}, url = '', events = false, liveCount = 12 } = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 }, colorScheme: 'dark' });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, 'dark', liveCount]);
  if (events) await page.addInitScript(captureEvents);
  await page.addInitScript(overlay, Object.fromEntries(Object.entries(extra).map(([k, v]) => [k, v.toString()])));
  await page.goto(`${base}${url}`);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
  return { ctx, page, errors };
}
const settle = (page, ms = 250) => page.waitForTimeout(ms);

try {
  await check('T01 채팅 입력은 이모지를 써도 100자(코드포인트)까지 받는다', async () => {
    const { ctx, page } = await open();
    await page.evaluate(async () => {
      const { ChatComposer } = await import('/js/chat-composer.js');
      const form = document.createElement('form');
      form.className = 'chat-input';
      form.innerHTML = '<input type="text" maxlength="100" id="t01">';
      document.body.append(form);
      window.__t01 = new ChatComposer(form, { source: () => null });
    });
    await page.focus('#t01');
    await page.keyboard.insertText('😀'.repeat(60));
    for (let i = 0; i < 40; i += 1) await page.keyboard.insertText('가');
    await page.keyboard.insertText('나');
    let r = await page.evaluate(() => { const i = document.getElementById('t01'); return { n: [...i.value].length, last: [...i.value].at(-1), count: i.form.querySelector('.chat-count')?.textContent }; });
    assert.deepEqual(r, { n: 100, last: '가', count: '100/100' });
    // 한도를 넘는 붙여넣기는 100자로 자른다(엔진마다 maxlength 셈이 달라도).
    await page.evaluate(() => { const i = document.getElementById('t01'); i.value = ''; i.dispatchEvent(new Event('input', { bubbles: true })); });
    await page.keyboard.insertText('😀'.repeat(70) + 'a'.repeat(70));
    r = await page.evaluate(() => [...document.getElementById('t01').value].length);
    assert.ok(r <= 100, `붙여넣기 뒤 ${r}자`);
    await ctx.close();
  });

  await check('T02 네트워크가 돌아와도 채팅만 보기는 다시 열지 않고 홈은 다시 받는다', async () => {
    const { ctx, page } = await open({ extra: {
      live_detail: ({ channelId }) => ({ channelId, channelName: '릴카', chatChannelId: `chat-${channelId}`, isLive: true, liveTitle: '방송', viewerCountText: '1,000' }),
      chat_connect: () => ({ canSend: true }),
      chat_disconnect: () => null,
    } });
    await go(page, 'chat', { channelId: 'ch1' });
    await settle(page, 600);
    const count = (c) => page.evaluate((cmd) => window.__qCalls.filter(([x]) => x === cmd).length, c);
    const before = await count('chat_connect');
    assert.ok(before >= 1, '채팅을 연결하지 않았다');
    await page.evaluate(() => window.dispatchEvent(new Event('online')));
    await settle(page, 1300);
    assert.equal(await count('chat_connect'), before, '채팅 화면을 다시 열었다');
    await go(page, 'home');
    await settle(page, 600);
    const home = await count('following_lives');
    await page.evaluate(() => window.dispatchEvent(new Event('online')));
    await settle(page, 1300);
    assert.ok(await count('following_lives') > home, '홈은 다시 받아야 한다');
    await ctx.close();
  });

  await check('T03 보조 플레이어 창은 메인 창 멀티라이브에 음소거를 보내지 않는다', async () => {
    const { ctx, page } = await open({ url: '?w=player&channelId=ch2', extra: {
      player_start: ({ channelId }) => ({ live: { channelId, channelName: '플러리', chatChannelId: `chat-${channelId}`, isLive: true, liveTitle: '방송', viewerCountText: '1' } }),
      chat_connect: () => ({ canSend: true }),
      chat_disconnect: () => null,
    } });
    await settle(page, 800);
    const r = await page.evaluate(() => window.__qCalls.filter(([c]) => c === 'multilive_set_audio_suspended' || c === 'multilive_park').map(([c, a]) => [c, a]));
    assert.deepEqual(r, []);
    await ctx.close();
  });

  await check('T04 채팅 창의 방송 보기는 플레이어 창을 연다', async () => {
    const { ctx, page } = await open({ url: '?w=chat&channelId=ch2', extra: {
      live_detail: ({ channelId }) => ({ channelId, channelName: '플러리', chatChannelId: `chat-${channelId}`, isLive: true, liveTitle: '방송', viewerCountText: '1' }),
      chat_connect: () => ({ canSend: true }),
      chat_disconnect: () => null,
      window_open: () => 'player-1',
    } });
    await settle(page, 500);
    await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:play', { detail: { channelId: 'ch5' } })));
    await settle(page, 500);
    const r = await page.evaluate(() => ({
      open: window.__qCalls.filter(([c]) => c === 'window_open').map(([, a]) => [a.kind, a.params && a.params.channelId]),
      start: window.__qCalls.some(([c]) => c === 'player_start'),
    }));
    assert.deepEqual(r, { open: [['player', 'ch5']], start: false });
    await ctx.close();
  });

  await check('T05 채널 정보를 받는 사이 떠나면 채팅만 보기는 연결하지 않는다', async () => {
    const { ctx, page } = await open({ extra: {
      live_detail: ({ channelId }) => new Promise((r) => { window.__releaseDetail = () => r({ channelId, channelName: '릴카', chatChannelId: `chat-${channelId}`, isLive: true, liveTitle: '방송', viewerCountText: '1' }); }),
      chat_connect: () => ({ canSend: true }),
      chat_disconnect: () => null,
    } });
    await go(page, 'chat', { channelId: 'ch1' });
    await settle(page, 300);
    await go(page, 'home');
    await settle(page, 300);
    await page.evaluate(() => window.__releaseDetail && window.__releaseDetail());
    await settle(page, 400);
    assert.equal(await page.evaluate(() => window.__qCalls.some(([c]) => c === 'chat_connect')), false, '떠난 화면에서 채팅을 연결했다');
    await ctx.close();
  });

  await check('T06 방송이 없고 채팅 채널도 없는 채널로 바꾸면 앞 채팅을 끊는다', async () => {
    const { ctx, page } = await open({ url: '?w=player&channelId=ch2', extra: {
      player_start: ({ channelId }) => { if (channelId === 'ch7') throw new Error('방송 중이 아닙니다'); return { live: { channelId, channelName: '플러리', chatChannelId: `chat-${channelId}`, isLive: true, liveTitle: '방송', viewerCountText: '1' } }; },
      live_detail: ({ channelId }) => ({ channelId, channelName: '꺼진 채널', chatChannelId: null, isLive: false }),
      chat_connect: () => ({ canSend: true }),
      chat_disconnect: () => null,
    } });
    await settle(page, 800);
    assert.ok(await page.evaluate(() => window.__qCalls.some(([c]) => c === 'chat_connect')), '첫 채널 채팅을 연결하지 않았다');
    const n = await page.evaluate(() => window.__qCalls.length);
    await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:play', { detail: { channelId: 'ch7' } })));
    await settle(page, 800);
    const after = await page.evaluate((from) => window.__qCalls.slice(from).map(([c]) => c), n);
    assert.ok(after.includes('chat_disconnect'), `앞 채팅을 끊지 않았다: ${after.join(',')}`);
    await ctx.close();
  });

  await check('T07 외관 이벤트의 테마를 이 창 화면도 따른다', async () => {
    const { ctx, page } = await open({ events: true });
    assert.equal(await page.evaluate(() => document.documentElement.dataset.theme), 'dark');
    await page.evaluate(() => (window.__ev['settings://appearance'] || []).forEach((cb) => cb({ payload: { theme: 'light', uiScale: 1 } })));
    await settle(page, 200);
    assert.equal(await page.evaluate(() => document.documentElement.dataset.theme), 'light');
    await ctx.close();
  });

  await check('T08 고른 카테고리 칩은 목록에서 빠져도 남는다', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async () => {
      const { categoryChipGroups } = await import('/js/pages.js');
      const lives = [{ categoryName: 'A', viewerCount: 9 }, { categoryName: 'B', viewerCount: 5 }, { categoryName: 'C', viewerCount: 1 }];
      return [
        categoryChipGroups(lives, '사라진 분야', 2).map((c) => [c.name, c.count]),
        categoryChipGroups(lives, 'C', 2).map((c) => [c.name, c.count]),
        categoryChipGroups(lives, '', 2).map((c) => c.name),
      ];
    });
    assert.deepEqual(r, [[['A', 1], ['B', 1], ['사라진 분야', 0]], [['A', 1], ['B', 1], ['C', 1]], ['A', 'B']]);
    await ctx.close();
  });

  await check('T09 최근 본 채널은 오래된 인기 목록으로 LIVE 를 판정하지 않는다', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async () => {
      const { buildSidebarList } = await import('/js/sidebar-lists.js');
      const now = Date.parse('2026-11-26T14:00:00+09:00');
      const data = (popularAt) => ({ following: [], favorites: new Set(), popular: [{ channelId: 'a', channelName: 'A', viewerCount: 9, viewerCountText: '9', liveTitle: 't' }], popularAt, recent: [{ channelId: 'a', channelName: 'A', lastWatched: now - 3600e3 }] });
      const live = (d) => { const l = buildSidebarList('library', d, now); return (l.items || l.rows || []).map((i) => i.live); };
      return { stale: live(data(now - 4 * 3600e3)), fresh: live(data(now - 30e3)) };
    });
    assert.deepEqual(r, { stale: [false], fresh: [true] });
    await ctx.close();
  });

  await check('T10 홈 전체 칩 개수는 펼칠 수 있는 카드 수를 넘지 않는다', async () => {
    const { ctx, page } = await open({ liveCount: 60 });
    await go(page, 'home');
    await page.waitForSelector('.home-chips .chip', { state: 'attached' });
    const n = await page.evaluate(() => Number(document.querySelector('.home-chips .chip .chip-count').textContent));
    assert.ok(n <= 36, `전체 ${n}`);
    await ctx.close();
  });

  await check('T11 라이브 메뉴 최근 시청은 시청 기록(DB)을 먼저 쓴다', async () => {
    const { ctx, page } = await open({ extra: { recent_channels: () => [{ channelId: 'dbch', channelName: '기록채널', channelImageUrl: '' }] } });
    await page.evaluate(() => { try { localStorage.removeItem('cview.recentChannels'); } catch { /* 무시 */ } });
    await go(page, 'live');
    await page.waitForSelector('.recent-row .recent-item', { state: 'attached' });
    const ids = await page.evaluate(() => [...document.querySelectorAll('[data-page="live"] .recent-row .recent-item')].map((b) => b.dataset.channelId));
    assert.deepEqual(ids, ['dbch']);
    await ctx.close();
  });

  await check('T12 상대 날짜와 갱신 캡션의 경계', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async () => {
      const fi = await import('/js/following-info.js');
      const ci = await import('/js/channel-info.js');
      const ch = await import('/js/channel.js');
      const cards = await import('/js/cards.js');
      const at = (y, m, d, h, mi = 0) => new Date(y, m - 1, d, h, mi).getTime();
      const iso = (t) => new Date(t).toISOString();
      const now = at(2026, 11, 26, 9);
      return {
        lastNight: fi.agoText(at(2026, 11, 25, 23, 30), now),
        today: fi.agoText(at(2026, 11, 26, 1), now),
        twoDays: ci.agoText(iso(at(2026, 11, 24, 23)), at(2026, 11, 26, 22)),
        yesterday: ci.agoText(iso(at(2026, 11, 25, 7)), at(2026, 11, 26, 22)),
        year362: ch.relativeDate(iso(now - 362 * 86400e3), now),
        info362: ci.agoText(iso(now - 362 * 86400e3), now),
        year400: ch.relativeDate(iso(now - 400 * 86400e3), now),
        fresh3571: cards.freshnessText(now - 3571e3, now),
        fresh5400: cards.freshnessText(now - 5400e3, now),
      };
    });
    assert.deepEqual(r, { lastNight: '1일 전', today: '오늘', twoDays: '2일 전', yesterday: '어제', year362: '11개월 전', info362: '11개월 전', year400: '1년 전', fresh3571: '59분 전 갱신', fresh5400: '1시간 전 갱신' });
    await ctx.close();
  });

  await check('T13 지연 프리셋 표준은 default 로 보낸다', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async () => {
      const { SCHEMA } = await import('/js/settings.js');
      const items = SCHEMA.flatMap((g) => g[2] || []);
      const preset = items.find((i) => i.k === 'latencyPreset');
      return preset.options.map((o) => (Array.isArray(o) ? o[0] : o.value));
    });
    assert.deepEqual(r, ['matchWeb', 'webSync', 'default', 'ultraLow', 'custom']);
    await ctx.close();
  });

  await check('T14 이모지 이니셜·멘션 알림 자르기·무대 배율', async () => {
    const src = async (f) => (await fetch(`${base}js/${f}`)).text();
    for (const f of ['ml-watch-info.js', 'viewer-extras.js', 'menubar.js']) {
      const t = await src(f);
      assert.doesNotMatch(t, /channelName \|\| '\?'\)\.slice\(0, 1\)|\.trim\(\)\.charAt\(0\)/, `${f} 이 UTF-16 한 단위로 이니셜을 자른다`);
    }
    assert.match(await src('chat.js'), /Array\.from\(String\(m\.content \|\| ''\)\)\.slice\(0, 140\)/);
    assert.match(await src('multilive.js'), /const scale = pixelScale\(\);/);
  });

  await check('T15 한글 입력 상태에서도 M·L·B 를 물리 키로 알아본다', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async () => {
      const { plainLetter } = await import('/js/dom.js');
      return [
        plainLetter({ key: 'ㅡ', code: 'KeyM' }), plainLetter({ key: 'ㅣ', code: 'KeyL' }), plainLetter({ key: 'ㅠ', code: 'KeyB' }),
        plainLetter({ key: 'M', code: 'KeyM' }), plainLetter({ key: ' ', code: 'Space' }), plainLetter({ key: 'ArrowUp', code: 'ArrowUp' }),
      ];
    });
    assert.deepEqual(r, ['m', 'l', 'b', 'm', '', '']);
    const vc = await (await fetch(`${base}js/viewer-controls.js`)).text();
    assert.doesNotMatch(vc, /e\.key\.toLowerCase\(\) === '[mlb]' && !mod/);
    const mw = await (await fetch(`${base}js/ml-watch.js`)).text();
    assert.match(mw, /plainLetter\(e\) \|\| e\.key/);
    await ctx.close();
  });

  await check('T16 시청 화면 다시 연결·녹화·화질·휠·전체 화면', async () => {
    const v = await (await fetch(`${base}js/viewer.js`)).text();
    const vc = await (await fetch(`${base}js/viewer-controls.js`)).text();
    assert.match(v, /if \(!reopened\) this\.sessionMuted = this\.startMuted;/, '다시 연결이 음소거를 설정값으로 되돌린다');
    assert.ok(v.indexOf('await this.panel.resetRecording();') > -1 && v.indexOf('await this.panel.resetRecording();') < v.indexOf('await api.playerStart(channelId)'), '녹화를 마무리하기 전에 재생을 시작한다');
    assert.match(v, /this\.controls\.resetStream\(\);/);
    assert.match(v, /document\.exitFullscreen\(\)\.catch/);
    assert.match(vc, /if \(type === 'playStarted'\) this\.viewer\.player\.call\('getLevels'\);/);
    assert.match(vc, /closest\('\.player-panel, \.vt-menu, \.quality-menu'\)\) return;/);
  });

  await check('T17 다시보기 오류 막·다시 시도', async () => {
    const { ctx, page } = await open({ extra: {
      vod_play: () => { window.__vodPlays = (window.__vodPlays || 0) + 1; return { videoNo: 500, title: '다시보기', channelName: '릴카', duration: 100, resumeAt: 0, streamUrl: `${location.origin}/auto-${window.__vodPlays}.mp4`, qualities: [{ url: `${location.origin}/q1080-${window.__vodPlays}.mp4`, resolution: '1920x1080', name: '1080p' }, { url: `${location.origin}/q720-${window.__vodPlays}.mp4`, resolution: '1280x720', name: '720p' }] }; },
      vod_progress_save: () => null,
      ad_preroll_plan: () => null,
    } });
    await go(page, 'media', { kind: 'vod', id: 500 });
    await page.waitForSelector('[data-page="media"] video', { state: 'attached', timeout: 8000 });
    const r = await page.evaluate(async () => {
      const root = document.querySelector('[data-page="media"]');
      const v = root.querySelector('video');
      const sel = root.querySelector('[data-media-select="quality"]');
      v.dispatchEvent(new Event('error'));
      await new Promise((res) => setTimeout(res, 50));
      const shown = Boolean(root.querySelector('.media-error'));
      sel.value = '1'; sel.dispatchEvent(new Event('change', { bubbles: true }));
      const cleared = !root.querySelector('.media-error');
      v.dispatchEvent(new Event('error'));
      await new Promise((res) => setTimeout(res, 50));
      root.querySelector('[data-media="retry"]').click();
      await new Promise((res) => setTimeout(res, 300));
      return { shown, cleared, plays: window.__vodPlays, src: v.getAttribute('src') || v.src };
    });
    assert.equal(r.shown, true, '오류 막이 뜨지 않았다(검사 전제)');
    assert.equal(r.cleared, true, '화질을 바꿨는데 오류 막이 남았다');
    assert.equal(r.plays, 2, '다시 시도가 재생 주소를 새로 받지 않았다');
    assert.match(r.src, /q720-2\.mp4$/, `고른 화질(720p)의 새 주소로 열지 않았다: ${r.src}`);
    await ctx.close();
  });

  await check('T18 다른 창이 바꾼 채팅 설정을 따른다', async () => {
    const { ctx, page } = await open({ events: true });
    const r = await page.evaluate(async () => {
      const { loadChatPrefs, chatPrefs } = await import('/js/chat-prefs.js');
      await loadChatPrefs();
      (window.__ev['settings://chat'] || []).forEach((cb) => cb({ payload: { blockedUsers: ['A'], blockedUserNames: { A: '가' } } }));
      return chatPrefs().blockedUsers;
    });
    assert.deepEqual(r, ['A']);
    await ctx.close();
  });

  await check('T19 /unmute·말풍선 캐시', async () => {
    const ex = await (await fetch(`${base}js/chat-extras.js`)).text();
    assert.match(ex, /case '\/unmute': \{/);
    assert.doesNotMatch(ex, /SERVER_ONLY = new Set\(\[[^\]]*'\/unmute'/);
    const pop = await (await fetch(`${base}js/chat-user-pop.js`)).text();
    assert.match(pop, /addEventListener\(CHANNEL_SOCIAL_EVENT, \(\) => cardCache\.clear\(\)\)/);
    assert.match(pop, /listen\(Events\.authChanged, \(\) => cardCache\.clear\(\)\)/);
  });

  await check('T20 다시보기 한글 단축키·다시 받기 채널 이름', async () => {
    const media = await (await fetch(`${base}js/media.js`)).text();
    assert.match(media, /plainLetter\(e\) \|\| e\.key/);
    const dl = await (await fetch(`${base}js/downloads.js`)).text();
    assert.match(dl, /startDownload\(b\.dataset\.kind, b\.dataset\.target, b\.dataset\.title, b\.dataset\.channel \|\| undefined\)/);
    assert.equal((dl.match(/data-channel="\$\{esc\(/g) || []).length, 2);
  });

  await check('T21 지금 재생 중 카드는 재생 화면으로 간다', async () => {
    const { ctx, page } = await open({ events: true });
    await go(page, 'settings');
    await settle(page, 200);
    const r = await page.evaluate(async () => {
      // 다시보기가 아닌 화면에서 재생 중 칸이 없으면 그대로(오류 없이) — 실제 이동은 시청 화면이 있어야 해서 코드로 확인한다.
      (window.__ev['tray://control'] || []).forEach((cb) => cb({ payload: { kind: 'show-now-playing' } }));
      const src = await (await fetch('/js/app.js')).text();
      return { handled: /case 'show-now-playing':/.test(src), route: document.querySelector('[data-page="settings"]:not([hidden])') !== null };
    });
    assert.deepEqual(r, { handled: true, route: true });
    await ctx.close();
  });

  await check('T22 기타 카테고리를 고르면 카테고리 없는 방송이 보인다', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async () => {
      const { categoryGroups, arrangeLives } = await import('/js/pages.js');
      const lives = [{ categoryName: 'LoL', viewerCount: 3 }, { categoryName: null, viewerCount: 2 }, { viewerCount: 1 }];
      return { groups: categoryGroups(lives).map((g) => `${g.name}:${g.count}`), etc: arrangeLives(lives, { category: '기타' }).length };
    });
    assert.deepEqual(r, { groups: ['LoL:1', '기타:2'], etc: 2 });
    await ctx.close();
  });

  await check('T23 검색 뒤로 가기·비운 뒤 ⌘R', async () => {
    const { ctx, page } = await open();
    const count = () => page.evaluate(() => window.__qCalls.filter(([c]) => c === 'search').length);
    await go(page, 'search', { query: '릴카' });
    await settle(page, 600);
    const first = await count();
    assert.ok(first >= 1);
    await go(page, 'home');
    await settle(page, 300);
    await go(page, 'search', { query: '릴카' });
    await settle(page, 400);
    assert.equal(await count(), first, '같은 검색 결과로 돌아왔는데 다시 찾았다');
    await page.evaluate(() => { const i = document.querySelector('[data-page="search"] input'); i.value = ''; i.dispatchEvent(new Event('input', { bubbles: true })); });
    await settle(page, 200);
    await page.keyboard.press('Meta+r');
    await settle(page, 600);
    const r = await page.evaluate(() => document.querySelector('[data-page="search"] input').value);
    assert.equal(await count(), first, '비운 뒤 ⌘R 이 지운 검색어로 다시 찾았다');
    assert.equal(r, '');
    await ctx.close();
  });

  await check('T24 다시보기는 재생 정보를 광고보다 먼저, 클립 새로고침은 refresh', async () => {
    const { ctx, page } = await open({ extra: {
      vod_play: () => { throw new Error('성인 인증이 필요한 영상입니다'); },
    } });
    await go(page, 'media', { kind: 'vod', id: 77 });
    await settle(page, 600);
    const r = await page.evaluate(() => ({
      calls: window.__qCalls.map(([c]) => c).filter((c) => c === 'vod_play' || c.startsWith('ad_')),
      err: Boolean(document.querySelector('[data-page="media"] .empty.error')),
    }));
    assert.equal(r.calls[0], 'vod_play', `광고를 먼저 불렀다: ${r.calls.join(',')}`);
    assert.ok(!r.calls.some((c) => c.startsWith('ad_')), '재생할 수 없는 영상에 광고를 요청했다');
    assert.equal(r.err, true);
    const pages = await (await fetch(`${base}js/pages.js`)).text();
    assert.match(pages, /api\.popularClips\(this\.clipFilter, this\.clipOrder, Boolean\(refresh\)\)/);
    await ctx.close();
  });

  await check('T25 사용률 그래프 배율·시계 역행·광고 볼륨 0', async () => {
    const su = await (await fetch(`${base}js/sidebar-usage.js`)).text();
    assert.match(su, /Math\.min\(3, pixelScale\(\)\)/);
    const um = await (await fetch(`${base}js/usage-monitor.js`)).text();
    assert.match(um, /if \(last\.t - s\.t <= BACKWARD_TOLERANCE_MS\) return;/);
    const ads = await (await fetch(`${base}js/ads.js`)).text();
    assert.match(ads, /Number\.isFinite\(v\) && v >= 0 \? Math\.min\(1, v \/ 100\) : 1/);
  });
} finally {
  await browser.close();
  server.close();
}
console.log(`\n${total - failures.length}/${total} 통과 (${WEBKIT ? 'webkit' : 'chromium'})`);
if (failures.length) process.exit(1);
