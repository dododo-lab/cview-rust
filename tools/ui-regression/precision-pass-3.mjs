// 기능 정밀 고도화 3차(2026-11-26) 회귀 검사 — 2차에서 뺀 항목 재판단과 새 영역 검토로 찾은 실제 결함을 고친 것.
// 모의 IPC·헤드리스 브라우저(실제 서비스·데이터는 건드리지 않는다). CVIEW_UI_BROWSER=webkit 이면 WebKit.
// 저장소 루트에서: node tools/ui-regression/precision-pass-3.mjs
// R01 설정 — 알림 크기·라이브 되감기 범위·디스크 한도가 Rust 가 저장하는 값과 같은 범위·선택지다.
// R02 채널 알림 팝업 — 설정을 읽는 동안 다시 눌러도 팝업은 하나만 열린다.
// R03 채널 알림 팝업 — 저장에 실패하면 체크가 되돌아간다.
// R04 녹화 — 쓰기 오류로 녹화가 멈추면 마무리하고 이유와 저장된 부분을 알린다.
// R05 앱 시트 — 시트가 떠 있을 때 다른 시트를 열고 닫아도 초점이 처음 연 자리로 돌아간다.
// R06 보관함 '최근 시청 기록 지우기' — 화면 쪽 최근 목록(localStorage)도 비운다.
// R07 다시보기 '다시 시도' — 보던 위치와 배속에서 다시 재생한다.
// R08 시청 화면 음소거 — 광고 중(실제 video.muted 가 켜진 동안)에도 사용자가 고른 음소거를 뒤집는다.
// R09 광고 — 광고 중 다른 채널을 열면 앞 광고의 정리가 새 광고의 소리 막음을 풀지 않는다.
// R10 플레이어 — 일시정지 중 지연 보정(seekToLatency)이 재생을 다시 시작하지 않는다.
// R11 단축키 — ⌘8·⇧⌘Z·⌥⌘H·⌥⌘R 은 예약 키다.
// R12 ⌥⌘R 녹화 — macOS 에서 ⌥ 가 글자를 바꿔도(®) 키 자리로 알아본다.
// R13 ⌘R — 검색 화면에서 새로 낸 검색어, 카테고리 화면에서 고른 카테고리로 다시 불러온다.
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

async function open({ extra = {} } = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 }, colorScheme: 'dark' });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, 'dark']);
  await page.addInitScript(overlay, Object.fromEntries(Object.entries(extra).map(([k, v]) => [k, v.toString()])));
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
  return { ctx, page, errors };
}
const settle = (page, ms = 250) => page.waitForTimeout(ms);

try {
  await check('R01 알림 크기·되감기 설정이 Rust 저장 범위와 같다', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async () => {
      const { SCHEMA } = await import('/js/settings.js');
      const items = SCHEMA.flatMap(([, , list]) => list);
      const by = (k) => items.find((i) => i.k === k);
      const alert = by('streamAlertScale');
      return {
        alert: [alert.type, alert.min, alert.max],
        window: [by('liveRewindWindowMinutes').type, by('liveRewindWindowMinutes').options.map((o) => o.value)],
        budget: [by('liveRewindDiskBudgetMB').type, by('liveRewindDiskBudgetMB').options.map((o) => o.value)],
      };
    });
    // crates/cview-core/src/models/settings.rs: de_alert_scale(0.7~1.7), LIVE_REWIND_WINDOW_CHOICES, LIVE_REWIND_DISK_BUDGET_CHOICES
    assert.deepEqual(r, { alert: ['range', 0.7, 1.7], window: ['select', [10, 30, 60]], budget: ['select', [2048, 5120, 10240, 20480]] });
    await ctx.close();
  });

  await check('R02 채널 알림 팝업은 빠르게 두 번 눌러도 하나만 열린다', async () => {
    const { ctx, page } = await open({ extra: { settings_get: '() => new Promise((res) => setTimeout(() => res({ channelNotifications: { settings: {} } }), 80))' } });
    const r = await page.evaluate(async () => {
      const { ChannelPage } = await import('/js/channel.js');
      const anchor = document.createElement('button');
      document.body.append(anchor);
      const p = Object.assign(Object.create(ChannelPage.prototype), { channelId: 'c1', detail: { channelName: '채널' } });
      await Promise.all([p.editNotify(anchor), p.editNotify(anchor)]);
      return document.querySelectorAll('.notify-pop').length;
    });
    assert.equal(r, 1);
    await ctx.close();
  });

  await check('R03 채널 알림 저장에 실패하면 체크가 되돌아간다', async () => {
    const { ctx, page } = await open({ extra: { settings_get: '() => ({ channelNotifications: { settings: {} } })', settings_update: '() => { throw new Error("저장 실패"); }' } });
    const r = await page.evaluate(async () => {
      const { ChannelPage } = await import('/js/channel.js');
      const anchor = document.createElement('button');
      document.body.append(anchor);
      const p = Object.assign(Object.create(ChannelPage.prototype), { channelId: 'c1', detail: { channelName: '채널' } });
      await p.editNotify(anchor);
      const box = document.querySelector('.notify-pop input[data-nk="notifyOnLive"]');
      const before = box.checked;
      box.click();
      await new Promise((res) => setTimeout(res, 100));
      return { before, after: box.checked, toast: [...document.querySelectorAll('.toast')].map((t) => t.textContent).join(' ') };
    });
    assert.equal(r.before, true);
    assert.equal(r.after, true, '저장하지 못했는데 꺼진 채로 남았다');
    assert.match(r.toast, /저장 실패/);
    await ctx.close();
  });

  await check('R04 쓰기 오류로 멈춘 녹화는 마무리하고 이유를 알린다', async () => {
    const { ctx, page } = await open({ extra: {
      player_record_status: '() => ({ recording: false, path: "/m/a.ts", bytes: 2097152, startedAt: 1, error: "파일에 쓸 수 없습니다: No space left on device" })',
      player_record_stop: '() => ({ recording: false, path: "/m/a.mp4", bytes: 2097152, startedAt: 1, error: null })',
      player_record_start: '() => ({ recording: true, path: "/m/a.ts", bytes: 0, startedAt: 1 })',
    } });
    const r = await page.evaluate(async () => {
      const { PlayerPanel } = await import('/js/player-panel.js');
      const realSetInterval = window.setInterval;
      let tick = null;
      window.setInterval = (fn) => { tick = fn; return 1; };
      const p = Object.assign(Object.create(PlayerPanel.prototype), {
        viewer: { isPlaying: true, controls: { flash() {} } }, recording: null, recTimer: 0, setRecUi(on) { this.ui = on; }, recButton: document.createElement('button'),
      });
      await p.toggleRecording();
      window.setInterval = realSetInterval;
      await tick();
      return { recording: p.recording, ui: p.ui, stopped: window.__qCalls.some(([c]) => c === 'player_record_stop'), toast: [...document.querySelectorAll('.toast')].map((t) => t.textContent).join(' ') };
    });
    assert.equal(r.recording, null);
    assert.equal(r.ui, false);
    assert.equal(r.stopped, true, '멈춘 녹화를 마무리(확장자 맞추기)하지 않았다');
    assert.match(r.toast, /녹화가 중단됐습니다 · 파일에 쓸 수 없습니다.*2\.0MB.*a\.mp4/);
    await ctx.close();
  });
  await check('R05 시트를 겹쳐 열고 닫아도 초점이 연 자리로 돌아간다', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async () => {
      const { showSheet } = await import('/js/app-sheets.js');
      const opener = document.createElement('button');
      opener.textContent = '연 자리';
      document.body.append(opener);
      opener.focus();
      showSheet('<h2>첫 시트</h2>');
      showSheet('<h2>둘째 시트</h2>'); // 앞 시트를 닫지 않고 메뉴로 다른 시트
      document.querySelector('.sheet-scrim.app-sheet [data-close]').click();
      await new Promise((res) => setTimeout(res, 400));
      return { back: document.activeElement === opener, sheets: document.querySelectorAll('.sheet-scrim.app-sheet').length };
    });
    assert.equal(r.back, true, '초점이 문서 처음으로 떨어졌다');
    await ctx.close();
  });

  await check('R06 최근 시청 기록을 지우면 화면 쪽 최근 목록도 비운다', async () => {
    const { ctx, page } = await open({ extra: { recent_channels_clear: '() => null', data_backup_now: '() => ({ backups: [] })' } });
    await page.evaluate(() => localStorage.setItem('cview.recentChannels', JSON.stringify([{ channelId: 'x1', channelName: '지운 채널', at: 1 }])));
    await go(page, 'library', { tab: 'recent' });
    await page.waitForSelector('[data-lib="recent-clear"]', { state: 'attached' });
    await page.click('[data-lib="recent-clear"]');
    await page.waitForSelector('.cleanup-confirm button[type="submit"]');
    await page.click('.cleanup-confirm button[type="submit"]');
    await page.waitForFunction(() => window.__qCalls.some(([c]) => c === 'recent_channels_clear'));
    await settle(page);
    const left = await page.evaluate(() => localStorage.getItem('cview.recentChannels'));
    assert.equal(left, null, '지운 채널이 홈·라이브의 최근 시청에 다시 나온다');
    await ctx.close();
  });

  await check('R07 다시보기 다시 시도는 보던 위치와 배속에서 다시 재생한다', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async () => {
      const { MediaPage } = await import('/js/media.js');
      const listeners = {};
      const v = {
        currentTime: 4800, playbackRate: 1.5, src: '',
        addEventListener(t, fn) { listeners[t] = fn; },
        load() { this.currentTime = 0; this.playbackRate = 1; }, play() { return Promise.resolve(); },
      };
      const p = Object.assign(Object.create(MediaPage.prototype), { info: { streamUrl: 'https://x/v.m3u8' }, kind: 'vod' });
      Object.defineProperty(p, 'video', { value: v });
      const btn = document.createElement('button');
      btn.dataset.media = 'retry';
      document.body.append(btn);
      await p.onClick({ target: btn });
      listeners.loadedmetadata?.();
      return { at: v.currentTime, rate: v.playbackRate, src: v.src };
    });
    assert.deepEqual(r, { at: 4800, rate: 1.5, src: 'https://x/v.m3u8' });
    await ctx.close();
  });

  await check('R08 광고 중에도 음소거 단추가 음소거를 켠다', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async () => {
      const { Viewer } = await import('/js/viewer.js');
      const calls = [];
      const v = Object.assign(Object.create(Viewer.prototype), {
        // 광고 중 — 실제 video.muted 는 앱이 켜 둔 상태(setAdHold)
        player: { call: (n, x) => calls.push([n, x]), isMuted: () => true },
        muteButton: document.createElement('button'), sessionMuted: false,
      });
      Viewer.prototype.toggleMute.call(v);
      return { session: v.sessionMuted, last: calls.filter(([n]) => n === 'setMuted').at(-1)[1] };
    });
    assert.deepEqual(r, { session: true, last: true }, '광고 중 누르면 음소거 해제만 되어 광고 뒤 소리가 났다');
    await ctx.close();
  });

  await check('R09 광고 중 다른 채널을 열어도 새 광고의 소리 막음이 유지된다', async () => {
    // A 의 광고 계획은 곧 오고(취소됐으니 정리로 빠진다), B 의 계획은 아직 받는 중이다.
    const { ctx, page } = await open({ extra: { ad_preroll: '(a) => (String(a.id || a.target || "").includes("A") ? { slots: [] } : new Promise(() => {}))' } });
    const r = await page.evaluate(async () => {
      const { playPreroll, cancelPreroll } = await import('/js/ads.js');
      const host = document.createElement('div');
      document.body.append(host);
      const log = [];
      const player = { exec: (s) => log.push(s) };
      playPreroll('live', 'A', { host, player });
      cancelPreroll(host);
      playPreroll('live', 'B', { host, player }); // 같은 동기 구간 — 시청 화면 viewer.open 과 같은 순서
      await new Promise((res) => setTimeout(res, 50));
      return log;
    });
    assert.equal(r.at(-1), 'setAdHold(true)', `마지막 명령: ${r.join(', ')}`);
    await ctx.close();
  });

  await check('R10 일시정지 중 지연 보정 탐색은 재생을 다시 시작하지 않는다', async () => {
    const ctx = await browser.newContext();
    const page = await ctx.newPage();
    await page.goto(`${base}player/player.html`);
    const r = await page.evaluate(() => {
      let plays = 0;
      const v = document.getElementById('video');
      v.play = () => { plays += 1; return Promise.resolve(); };
      // 가짜 hls — 지연 메타가 없어 liveSyncPosition 폴백 갈래, 그리고 데드존 갈래.
      window.hls = { latency: 0, liveSyncPosition: 0 };
      hls = window.hls; // eslint-disable-line no-undef
      _wantsPlaying = false; // eslint-disable-line no-undef
      seekToLatency(3); // eslint-disable-line no-undef
      hls.latency = 3.1; // eslint-disable-line no-undef
      seekToLatency(3); // eslint-disable-line no-undef
      const paused = plays;
      _wantsPlaying = true; // eslint-disable-line no-undef
      seekToLatency(3); // eslint-disable-line no-undef
      return { paused, playing: plays - paused };
    });
    assert.deepEqual(r, { paused: 0, playing: 1 });
    await ctx.close();
  });

  await check('R11 메뉴·시스템이 쓰는 키는 예약 키다', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async () => {
      const { RESERVED, MOD } = await import('/js/shortcuts.js');
      const has = (k, m) => RESERVED.some(([key, mods]) => key === k && mods === m);
      return [has('8', MOD.cmd), has('z', MOD.cmd | MOD.shift), has('h', MOD.cmd | MOD.option), has('r', MOD.cmd | MOD.option)];
    });
    assert.deepEqual(r, [true, true, true, true]);
    await ctx.close();
  });

  await check('R12 ⌥⌘R 은 ⌥ 로 바뀐 글자(®)여도 녹화를 켠다', async () => {
    const { ctx, page } = await open();
    const r = await page.evaluate(async () => {
      const { isRecordShortcut } = await import('/js/player-panel.js');
      const ev = (o) => new KeyboardEvent('keydown', o);
      return [
        isRecordShortcut(ev({ key: '®', code: 'KeyR', metaKey: true, altKey: true })),
        isRecordShortcut(ev({ key: 'r', code: 'KeyR', metaKey: true, altKey: true })),
        isRecordShortcut(ev({ key: '®', code: 'KeyR', altKey: true })),
      ];
    });
    assert.deepEqual(r, [true, true, false]);
    await ctx.close();
  });

  await check('R13 ⌘R 은 화면 안에서 바꾼 검색어·카테고리로 다시 불러온다', async () => {
    const { ctx, page } = await open();
    await go(page, 'search', { query: '처음' });
    await page.waitForFunction(() => window.__qCalls.some(([c, a]) => c === 'search' && a.keyword === '처음'));
    await page.fill('.page-search input[type=search], [data-page="search"] input[type=search]', '나중');
    await page.press('.page-search input[type=search], [data-page="search"] input[type=search]', 'Enter');
    await page.waitForFunction(() => window.__qCalls.some(([c, a]) => c === 'search' && a.keyword === '나중'));
    await page.evaluate(() => { window.__qCalls.length = 0; window.dispatchEvent(new CustomEvent('cview:menu', { detail: 'refresh' })); });
    await page.waitForFunction(() => window.__qCalls.some(([c]) => c === 'search'));
    const search = await page.evaluate(() => ({ kw: window.__qCalls.filter(([c]) => c === 'search').map(([, a]) => a.keyword), input: document.querySelector('[data-page="search"] input[type=search], .page-search input[type=search]').value }));
    assert.deepEqual(search, { kw: ['나중'], input: '나중' });

    await go(page, 'categories', { category: '카테고리X' });
    await settle(page, 400);
    await page.evaluate(() => document.querySelector('[data-cat-back]')?.click());
    await settle(page);
    await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:menu', { detail: 'refresh' })));
    await settle(page, 400);
    const sub = await page.evaluate(() => document.querySelector('[data-page="categories"] [data-cat-back]') ? 'detail' : 'all');
    assert.equal(sub, 'all', '⌘R 이 처음 들어온 카테고리 상세로 되돌렸다');
    await ctx.close();
  });
} finally {
  await browser.close();
  server.close();
}
console.log(`\n${total - failures.length}/${total} 통과 (${WEBKIT ? 'webkit' : 'chromium'})`);
if (failures.length) process.exit(1);
