// 앱 사용률(머리줄 칩·자세히 팝오버·설정 › 성능) 회귀 검사 — 모의 IPC·헤드리스 브라우저(실제 측정·파일 저장은 하지 않는다).
// U01 칩 자리 — 홈(작은 칩)·팔로잉(빈칸 뒤, 새로고침 앞)·설정(머리줄 끝)·라이브 머리줄(툴바 맨 앞, 작은 칩). 문서에 칩은 하나.
// U02 칩 값 — CPU 는 코어 수로 나눈 전체 대비 %, 메모리, 꺾은선. 이름(aria-label)에 값, aria-live 없음.
//      CPU 가 높으면 경고 아이콘 + 이름에 'CPU 높음'(색만으로 알리지 않음). 초점이 칩에 있는 동안은 이름을 바꾸지 않는다.
// U03 팝오버(마우스) — 열면 aria-expanded·대화 상자에 초점(칸에 초점 고리 없음), 칸 4개 값, 바깥 누르면 닫힘.
// U04 팝오버(키보드) — Enter 로 열면 고른 칸(CPU)에 초점, ←/→/Home/End 로 지표를 바꾸면 그래프 색·요약이 따라감, Esc 로 닫고 칩으로.
// U05 GPU 지원 — 못 읽으면 '지원 안 됨'(aria-disabled, 화살표가 건너뜀), 전체만 읽으면 전체 GPU 값과 안내.
// U06 실시간 받기 — 칩이 보이면 usage_watch(true). 창이 숨으면 usage_watch(false), 다시 보이면 true 와 그동안 기록 채우기(since).
// U06b 다시 보일 때 채우기 응답보다 실시간 표본이 먼저 와도 숨은 동안의 기록을 잃지 않는다.
// U06c usage_watch 가 실패하면 5초 뒤 다시 보낸다. 아무도 보지 않아도 시작 때 false 를 한 번 보내 Rust 쪽 상태를 맞춘다.
// U07 설정 › 성능 — '연동'과 '고급' 사이 탭, 상태 알약·통계·그래프, 지표·기간 라디오(화살표 키), 표로 보기(1분 평균).
// U07b 표로 보기는 분 묶음이 바뀔 때만 다시 그린다(기록 기간이 짧아 맨 앞 표본이 초마다 잘려도).
// U08 내보내기·지우기 — CSV·JSON 은 usage_export(format) 후 경로 알림, 지우기는 두 번 눌러야(armConfirm) 한 번만 지운다.
// U09 설정 바꾸기 — '머리줄에 표시' 끄면 칩이 숨고, '사용률 측정'을 끄면 패널이 '측정 꺼짐'. 설정을 떠나면 실시간 받기를 그만둔다.
// U10 팝오버 '기록·내보내기' — 설정 › 성능 탭으로 간다.
// U11 그래프 말풍선 — 포인터를 올리면 세로선·말풍선(값·시각), 초점 + ←·End 로 같은 말풍선, Esc 는 말풍선만 닫는다(팝오버는 그대로).
// U12 명령 없음(옛 백엔드·브라우저) — 칩을 그리지 않고 오류도 없다. 성능 탭은 '잴 수 없습니다'.
// U13 보조 창(?w=chat) — 칩도 usage_* 호출도 없다.
// U14 axe(WCAG 2.1 A·AA + best-practice) — 팝오버·성능 탭, 라이트·다크.
// U15 배치 — 1000×660·1280×800·1600×900: 성능 탭 가로 넘침 없음, 팝오버 창 안, 칩이 머리줄 이웃과 겹치지 않음, 홈 인사말 한 줄(1280↑).
// U16 모션 줄이기 — 실시간 점 깜박임·칩 전환이 멈춘다.
// U17 칩 갱신 비용(Chromium) — 표본당 스크립트 3ms 미만, 레이아웃은 표본당 한 번 안팎.
// U18 라이브 단독 보기 머리줄(채널 칩 6개, 1280·1100·1000) — 칩은 툴바 맨 앞 작은 칩, 채널 칩·전환·⋯·채널 추가와 겹치지 않고 칩 줄 오른쪽,
//      칩 줄 창 크기는 사용률 칩 폭을 빼고 정해 "+N" 이 칩 줄 안에 다 보인다.
// 저장소 루트에서: node tools/ui-regression/usage-monitor.mjs   (Playwright·axe-core 가 필요하다.
//   CVIEW_UI_BROWSER=webkit 이면 WebKit, CVIEW_UI_SHOTS=<폴더> 면 장면 PNG, CVIEW_ONLY=U07 이면 그 검사만.)
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, base, mockTauri, go } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const WEBKIT = process.env.CVIEW_UI_BROWSER === 'webkit';
const engine = WEBKIT ? pw.webkit : pw.chromium;
const AXE = fs.readFileSync(require.resolve('axe-core/axe.min.js'), 'utf8');
const SHOTS = process.env.CVIEW_UI_SHOTS;
if (SHOTS) fs.mkdirSync(SHOTS, { recursive: true });
const ENGINE = WEBKIT ? 'webkit' : 'chromium';
const results = [];
async function check(id, fn) {
  if (process.env.CVIEW_ONLY && !process.env.CVIEW_ONLY.split(',').some((o) => id.startsWith(o))) return;
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message.split('\n').slice(0, 12).join('\n') }); }
}

/**
 * 사용률 명령 흉내 — window.__usage: cfg(측정·머리줄·간격·기간·GPU 지원), calls(명령·인자), samples, push(값) 로 새 표본을 보낸다
 * (Rust 처럼 usage_watch(true) 인 동안만 이벤트). opts.none 이면 usage_* 명령이 없다(옛 백엔드).
 */
function usageMock([opts]) {
  const listeners = {};
  window.__TAURI__.event.listen = async (name, cb) => { (listeners[name] ||= []).push(cb); return () => {}; };
  window.__emit = (name, payload) => { for (const cb of listeners[name] || []) cb({ payload }); };
  // 창 숨김 흉내 — document.visibilityState 를 __vis 로.
  window.__vis = 'visible';
  Object.defineProperty(Document.prototype, 'visibilityState', { configurable: true, get: () => window.__vis });
  if (opts.channels) {
    // 라이브 단독 보기 머리줄(채널 칩 줄) — viewer-topbar.mjs liveMock 을 줄인 것.
    const open = new Date(Date.now() - 5400e3).toISOString();
    const NAMES = ['한동숙', '침착맨', '서새봄냥 SEOSAEBOM', '똘똘똘이', '귀염1', '쉐리', '풍월량', '릴카'];
    const sessions = Array.from({ length: opts.channels }, (_, i) => ({ id: `s${i}`, slot: i, channelId: `ch${i + 1}`, channelName: NAMES[i % NAMES.length], liveTitle: '오늘 방송', profileImageUrl: `${location.origin}/__img/avatar/${i + 1}.svg`,
      viewerCountText: '1만', accumulateCount: 1, accumulateCountText: '1', openDate: open, isMuted: i > 0, isAudible: i === 0, isAudioOnly: false, state: { kind: 'playing' }, chatChannelId: null, maxHeightCap: 0, decodeSuspended: false }));
    const state = { sessions, selectedId: 's0', focusMainId: 's0', canAdd: true, maxSessions: 8, layout: 'grid', focusDirection: 'left', focusRatio: 0.7, horizontalRatio: 0.5, verticalRatio: 0.5, multiAudio: false, watchId: null, rev: 1 };
    const live = { liveTitle: '오늘 방송', categoryName: '게임', viewerCount: 1, viewerCountText: '1', accumulateCount: 1, openDate: open, tags: [], isLive: true };
    const prev = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async (c, a = {}) => {
      if (c === 'multilive_state') return state;
      if (c === 'multilive_watch') { state.watchId = a.sessionId; state.rev++; return state; }
      if (c === 'channel_detail') return { channel: { channelId: a.channelId, channelName: '한동숙' }, live, isFavorite: false };
      if (c === 'live_detail') return { ...live, channelId: a.channelId, channelName: '한동숙', chatChannelId: null };
      if (c === 'following_channels_all' || c === 'favorites_list' || c === 'chat_donation_ranking') return [];
      try { return await prev(c, a); } catch (e) { if (/^(multilive_|chat_|player_|window_|bookmark_|save_|channel_)/.test(c)) return null; throw e; }
    };
  }
  if (opts.none) return;
  const now = Date.now();
  const cores = 10;
  const mk = (t, i, cpu) => ({ t, cpu: cpu != null ? cpu : 60 + 40 * Math.sin(i / 7), mem: 900 * 1048576 + i * 65536, gpu: 6 + 3 * Math.sin(i / 11), gpuSys: 18, gpuMem: 1.2 * 1073741824, netRx: 600000 + 150000 * Math.sin(i / 5), procs: 5 });
  const samples = Array.from({ length: opts.count ?? 240 }, (_, i) => mk(now - ((opts.count ?? 240) - i) * 1000, i));
  const groups = [{ kind: 'app', cpu: 12, mem: 180e6, count: 1 }, { kind: 'web', cpu: 42, mem: 640e6, count: 2 }, { kind: 'gpu', cpu: 20, mem: 260e6, count: 1 }, { kind: 'network', cpu: 3, mem: 40e6, count: 1 }];
  const cfg = {
    enabled: opts.enabled ?? true, inHeader: opts.inHeader ?? true, intervalMs: 1000, historyMinutes: opts.historyMinutes ?? 15,
    gpu: opts.gpu || { probed: true, app: true, system: true, memoryScope: 'system', name: 'Apple M1 Max' },
  };
  const u = window.__usage = { cfg, calls: [], samples, groups, watching: false, i: samples.length, failWatch: opts.failWatch || 0, historyDelay: 0 };
  const status = () => ({ enabled: cfg.enabled, intervalMs: cfg.intervalMs, historyMinutes: cfg.historyMinutes, cores, platform: 'macos', gpu: cfg.gpu,
    samples: samples.length, capacity: 902, latest: samples.length ? { sample: samples[samples.length - 1], groups } : null });
  u.push = (cpu, { emit = true } = {}) => {
    const s = mk((samples.length ? samples[samples.length - 1].t : Date.now()) + 1000, u.i++, cpu);
    samples.push(s);
    if (emit && u.watching && cfg.enabled) window.__emit('usage://sample', { sample: s, groups });
    return s;
  };
  const appearance = () => ({ theme: 'dark', usageMonitor: cfg.enabled, usageInHeader: cfg.inHeader, usageIntervalMs: cfg.intervalMs, usageHistoryMinutes: cfg.historyMinutes });
  const orig = window.__TAURI__.core.invoke;
  window.__TAURI__.core.invoke = async (c, a = {}) => {
    if (c.startsWith('usage_') || c === 'reveal_path' || (c === 'settings_update' && a.domain === 'appearance')) u.calls.push([c, JSON.parse(JSON.stringify(a))]);
    if (c === 'usage_status') return status();
    if (c === 'usage_watch') {
      if (u.failWatch > 0) { u.failWatch -= 1; throw new Error('usage_watch 실패(모의)'); }
      u.watching = Boolean(a.on); return status();
    }
    if (c === 'usage_history') {
      const out = { intervalMs: cfg.intervalMs, samples: a.since == null ? samples.slice() : samples.filter((s) => s.t > a.since) };
      // historyDelay: 응답을 늦춰 그 사이 실시간 표본이 먼저 오게(채우기 경합 U06b)
      if (u.historyDelay) { const d = u.historyDelay; u.historyDelay = 0; await new Promise((r) => setTimeout(r, d)); }
      return out;
    }
    if (c === 'usage_clear') { samples.length = 0; return null; }
    if (c === 'usage_export') return `/Users/someone/Downloads/CView/CView-사용률-20261121-153005.${a.format}`;
    if (c === 'reveal_path') return null;
    if (c === 'settings_get') { const s = await orig(c, a); return { ...s, appearance: { ...s.appearance, ...appearance() } }; }
    if (c === 'settings_update' && a.domain === 'appearance') {
      const p = a.patch || {};
      if ('usageMonitor' in p) cfg.enabled = Boolean(p.usageMonitor);
      if ('usageInHeader' in p) cfg.inHeader = Boolean(p.usageInHeader);
      if ('usageIntervalMs' in p) cfg.intervalMs = Number(p.usageIntervalMs);
      if ('usageHistoryMinutes' in p) cfg.historyMinutes = Number(p.usageHistoryMinutes);
      const s = await orig('settings_get', {});
      setTimeout(() => window.__emit('settings://appearance', appearance()), 0);
      return { ...s, appearance: { ...s.appearance, ...appearance() } };
    }
    return orig(c, a);
  };
}

const browser = await engine.launch();

async function open({ width = 1280, height = 800, theme = 'dark', motion = false, query = '', ...opts } = {}) {
  const ctx = await browser.newContext({ viewport: { width, height }, colorScheme: theme, reducedMotion: motion ? 'no-preference' : 'reduce' });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => { const m = String(e); if (!/ResizeObserver loop/.test(m)) errors.push(m); });
  await page.addInitScript(mockTauri, [base, theme]);
  await page.addInitScript(usageMock, [opts]);
  await page.goto(`${base}${query}`);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.waitForFunction(() => { const s = document.getElementById('splash'); return !s || !s.isConnected || getComputedStyle(s).display === 'none' || getComputedStyle(s).opacity === '0'; }, null, { timeout: 8000 }).catch(() => {});
  if (!opts.none && !query && opts.inHeader !== false) await page.waitForSelector('.usage-chip:not([hidden])');
  await page.waitForTimeout(150);
  return { ctx, page, errors };
}

const calls = (page, name) => page.evaluate((n) => window.__usage.calls.filter((c) => c[0] === n).map((c) => c[1]), name);
const shot = async (page, name) => { if (SHOTS) await page.screenshot({ path: path.join(SHOTS, `usage-${name}-${ENGINE}.png`) }); };
async function openUsageTab(page) {
  await go(page, 'settings');
  await page.waitForSelector('[data-page="settings"] .settings-tabs');
  await page.click('[data-page="settings"] [data-group="usage"]');
  await page.waitForSelector('[data-usage-panel][data-mounted] .uc-line');
  await page.waitForTimeout(200);
}

await check('U01 칩 자리', async () => {
  const { ctx, page, errors } = await open();
  const where = () => page.evaluate(() => {
    const c = document.querySelector('.usage-chip');
    const h = c.parentElement;
    return { count: document.querySelectorAll('.usage-chip').length, parent: h.className, prev: c.previousElementSibling?.className || '', next: c.nextElementSibling?.className || '', compact: c.classList.contains('compact'), shown: !c.hidden };
  });
  let w = await where();
  assert.equal(w.count, 1);
  assert.match(w.parent, /home-header/);
  assert.match(w.prev, /grow/);
  assert.ok(w.compact, '홈은 작은 칩');
  await go(page, 'following'); await page.waitForTimeout(250);
  w = await where();
  assert.match(w.parent, /page-header/); assert.match(w.prev, /grow/); assert.match(w.next, /icon-btn/); assert.ok(!w.compact);
  await go(page, 'settings'); await page.waitForTimeout(250);
  w = await where();
  assert.match(w.parent, /page-header/); assert.equal(w.next, '', '설정 머리줄 끝');
  const right = await page.evaluate(() => { const c = document.querySelector('.usage-chip').getBoundingClientRect(); const h = document.querySelector('[data-page="settings"] > .page-header').getBoundingClientRect(); return h.right - c.right; });
  assert.ok(right < 60, `오른쪽 끝에 붙음(${right})`);
  await go(page, 'multilive'); await page.waitForTimeout(300);
  w = await where();
  assert.match(w.parent, /ml-toolbar/); assert.match(w.next, /ml-more/); assert.ok(w.compact, '라이브 머리줄은 작은 칩');
  assert.equal(w.count, 1);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('U02 칩 값·이름', async () => {
  const { ctx, page } = await open();
  await page.evaluate(() => window.__usage.push(123.4));
  await page.waitForTimeout(80);
  const v = await page.evaluate(() => {
    const c = document.querySelector('.usage-chip');
    const cv = c.querySelector('canvas.uchip-spark');
    const px = cv.getContext('2d').getImageData(0, 0, cv.width, cv.height).data;
    let inked = 0;
    for (let i = 3; i < px.length; i += 4) if (px[i] > 40) inked += 1;
    return { cpu: c.querySelector('.uchip-v').textContent, label: c.getAttribute('aria-label'), inked, live: c.querySelectorAll('[aria-live]').length + (c.closest('[aria-live]') ? 1 : 0), level: c.dataset.level, warn: getComputedStyle(c.querySelector('.uchip-warn')).display };
  });
  assert.equal(v.cpu, '12%'); // 123.4 / 10코어
  assert.match(v.label, /^앱 사용률 — CPU 12%, 메모리 \d+ MB, GPU [\d.]+%\. 자세히 보기$/);
  assert.ok(v.inked > 40, `꺾은선을 그림(${v.inked}px)`);
  assert.equal(v.live, 0, '초마다 읽지 않는다');
  assert.equal(v.level, 'ok');
  assert.equal(v.warn, 'none');
  await page.evaluate(() => window.__usage.push(900));
  await page.waitForTimeout(80);
  const high = await page.evaluate(() => { const c = document.querySelector('.usage-chip'); return { level: c.dataset.level, warn: getComputedStyle(c.querySelector('.uchip-warn')).display, label: c.getAttribute('aria-label') }; });
  assert.equal(high.level, 'high');
  assert.notEqual(high.warn, 'none', '경고 아이콘');
  assert.match(high.label, /CPU 90%.*\(CPU 높음\)/);
  // 초점이 있는 동안은 이름을 바꾸지 않는다(낭독기가 초마다 다시 읽지 않게).
  await page.focus('.usage-chip');
  const before = await page.getAttribute('.usage-chip', 'aria-label');
  await page.evaluate(() => window.__usage.push(10));
  await page.waitForTimeout(80);
  assert.equal(await page.getAttribute('.usage-chip', 'aria-label'), before);
  assert.equal(await page.textContent('.usage-chip .uchip-v'), '1.0%', '보이는 값은 바뀐다');
  await shot(page, 'chip');
  await ctx.close();
});

await check('U03 팝오버 마우스', async () => {
  const { ctx, page } = await open();
  await page.click('.usage-chip');
  await page.waitForSelector('.usage-pop:not([hidden])');
  const st = await page.evaluate(() => ({
    expanded: document.querySelector('.usage-chip').getAttribute('aria-expanded'),
    controls: document.querySelector('.usage-chip').getAttribute('aria-controls'),
    focus: document.activeElement.id,
    role: document.querySelector('.usage-pop').getAttribute('role'),
    tiles: [...document.querySelectorAll('.up-tile')].map((t) => [t.dataset.metric, t.querySelector('.up-tile-value').textContent, t.getAttribute('aria-checked')]),
    groups: document.querySelectorAll('.up-groups li').length,
    sub: document.querySelector('.up-sub').textContent,
    line: document.querySelector('.usage-pop .uc-line').getAttribute('d') || '',
  }));
  assert.equal(st.expanded, 'true'); assert.equal(st.controls, 'usage-pop'); assert.equal(st.role, 'dialog');
  assert.equal(st.focus, 'usage-pop', '마우스로 열면 대화 상자에 초점');
  assert.deepEqual(st.tiles.map((t) => t[0]), ['cpu', 'gpu', 'mem', 'net']);
  assert.ok(st.tiles.every((t) => t[1] && t[1] !== '—'), JSON.stringify(st.tiles));
  assert.equal(st.tiles[0][2], 'true');
  assert.equal(st.groups, 4);
  assert.equal(st.sub, 'CView와 도우미 4개 · 1초마다');
  assert.ok(st.line.length > 20);
  assert.ok((await calls(page, 'usage_watch')).some((a) => a.on === true));
  await shot(page, 'popover');
  await page.mouse.click(600, 700);
  await page.waitForSelector('.usage-pop', { state: 'hidden' });
  assert.equal(await page.getAttribute('.usage-chip', 'aria-expanded'), 'false');
  await ctx.close();
});

await check('U04 팝오버 키보드', async () => {
  const { ctx, page } = await open();
  await page.focus('.usage-chip');
  await page.keyboard.press('Enter');
  await page.waitForSelector('.usage-pop:not([hidden])');
  const focused = () => page.evaluate(() => document.activeElement.dataset.metric || document.activeElement.className);
  assert.equal(await focused(), 'cpu');
  await page.keyboard.press('ArrowRight');
  assert.equal(await focused(), 'gpu');
  const g = await page.evaluate(() => ({ checked: document.querySelector('.up-tile[data-metric="gpu"]').getAttribute('aria-checked'), color: document.querySelector('.usage-pop .uc').style.getPropertyValue('--uc-color'), label: document.querySelector('.usage-pop .uc-plot').getAttribute('aria-label') }));
  assert.equal(g.checked, 'true'); assert.equal(g.color, 'var(--u-gpu)'); assert.match(g.label, /GPU 최근 2분/);
  await page.keyboard.press('End');
  assert.equal(await focused(), 'net');
  await page.keyboard.press('ArrowRight');
  assert.equal(await focused(), 'cpu', '끝에서 처음으로');
  await page.keyboard.press('Escape');
  await page.waitForSelector('.usage-pop', { state: 'hidden' });
  assert.equal(await page.evaluate(() => document.activeElement.classList.contains('usage-chip')), true, '칩으로 돌아옴');
  await ctx.close();
});

await check('U05 GPU 지원', async () => {
  let { ctx, page } = await open({ gpu: { probed: true, app: false, system: false, memoryScope: null, name: null } });
  await page.focus('.usage-chip'); await page.keyboard.press('Enter');
  await page.waitForSelector('.usage-pop:not([hidden])');
  const t = await page.evaluate(() => { const g = document.querySelector('.up-tile[data-metric="gpu"]'); return { v: g.querySelector('.up-tile-value').textContent, dis: g.getAttribute('aria-disabled') }; });
  assert.equal(t.v, '지원 안 됨'); assert.equal(t.dis, 'true');
  await page.keyboard.press('ArrowRight');
  assert.equal(await page.evaluate(() => document.activeElement.dataset.metric), 'mem', 'GPU 칸을 건너뜀');
  await ctx.close();
  ({ ctx, page } = await open({ gpu: { probed: true, app: false, system: true, memoryScope: 'system', name: null } }));
  await page.click('.usage-chip');
  await page.click('.up-tile[data-metric="gpu"]');
  const s = await page.evaluate(() => { const g = document.querySelector('.up-tile[data-metric="gpu"]'); return { v: g.querySelector('.up-tile-value').textContent, sub: g.querySelector('.up-tile-sub').textContent, label: document.querySelector('.usage-pop .uc-plot').getAttribute('aria-label') }; });
  assert.equal(s.v, '18%'); assert.match(s.sub, /앱별 값은 지원 안 됨/); assert.match(s.label, /GPU\(전체\)/);
  await ctx.close();
});

await check('U06 실시간 받기·숨김', async () => {
  const { ctx, page } = await open();
  assert.deepEqual((await calls(page, 'usage_watch')).map((a) => a.on), [true]);
  await page.evaluate(() => { window.__vis = 'hidden'; document.dispatchEvent(new Event('visibilitychange')); });
  await page.waitForTimeout(100);
  assert.deepEqual((await calls(page, 'usage_watch')).map((a) => a.on), [true, false]);
  const last = await page.evaluate(() => window.__usage.samples.at(-1).t);
  const before = await page.textContent('.usage-chip .uchip-v');
  await page.evaluate(() => { for (let i = 0; i < 3; i += 1) window.__usage.push(500, { emit: true }); }); // watching=false 라 이벤트 없음
  await page.waitForTimeout(80);
  assert.equal(await page.textContent('.usage-chip .uchip-v'), before, '숨은 동안 화면 갱신 없음');
  await page.evaluate(() => { window.__vis = 'visible'; document.dispatchEvent(new Event('visibilitychange')); });
  await page.waitForTimeout(150);
  assert.deepEqual((await calls(page, 'usage_watch')).map((a) => a.on), [true, false, true]);
  const hist = await calls(page, 'usage_history');
  assert.equal(hist.at(-1).since, last, '숨은 동안 기록을 since 로 채움');
  assert.equal(await page.textContent('.usage-chip .uchip-v'), '50%', '채운 뒤 최신 값');
  await ctx.close();
});

await check('U06b 채우기와 실시간 표본이 겹쳐도 기록을 잃지 않음', async () => {
  const { ctx, page } = await open();
  await openUsageTab(page);
  const p = '[data-usage-panel]';
  await page.click(`${p} [data-up-range="60000"]`);
  await page.evaluate(() => { window.__vis = 'hidden'; document.dispatchEvent(new Event('visibilitychange')); });
  await page.waitForTimeout(80);
  // 숨은 동안 CPU 99% 표본 3개(이벤트 없음) — 다시 보일 때 채우기로만 들어온다.
  await page.evaluate(() => { for (let i = 0; i < 3; i += 1) window.__usage.push(990); window.__usage.historyDelay = 400; });
  await page.evaluate(() => { window.__vis = 'visible'; document.dispatchEvent(new Event('visibilitychange')); });
  await page.waitForTimeout(80);
  // 채우기 응답 전에 실시간 표본이 먼저 온다(예전엔 이 표본이 마지막이 되어 채운 표본을 모두 버렸다).
  await page.evaluate(() => window.__usage.push(10));
  await page.waitForTimeout(700);
  assert.equal(await page.textContent(`${p} [data-st="max"]`), '99%');
  assert.equal(await page.textContent(`${p} [data-st="now"]`), '1.0%');
  await ctx.close();
});

await check('U06c 실시간 받기 실패 재시도·다시 불러올 때 맞추기', async () => {
  // 머리줄 표시가 꺼져 아무도 보지 않아도 시작 때 false 를 한 번 보내 Rust 쪽(창 새로고침 전 상태)을 맞춘다.
  let o = await open({ inHeader: false });
  await o.page.waitForTimeout(300);
  assert.deepEqual((await calls(o.page, 'usage_watch')).map((a) => a.on), [false]);
  await o.ctx.close();
  // 첫 usage_watch(true) 가 실패하면 5초 뒤 다시 보낸다.
  o = await open({ failWatch: 1 });
  await o.page.waitForTimeout(5600);
  const on = (await calls(o.page, 'usage_watch')).map((a) => a.on);
  assert.deepEqual(on, [true, true], JSON.stringify(on));
  await o.page.evaluate(() => window.__usage.push(333));
  await o.page.waitForTimeout(80);
  assert.equal(await o.page.textContent('.usage-chip .uchip-v'), '33%', '재시도 뒤 실시간 표본');
  await o.ctx.close();
});

await check('U07 설정 성능 탭', async () => {
  const { ctx, page, errors } = await open();
  await openUsageTab(page);
  const tabs = await page.$$eval('[data-page="settings"] .settings-tabs .seg', (b) => b.map((x) => x.textContent));
  assert.deepEqual(tabs.slice(5, 8), ['연동', '성능', '고급']);
  const p = '[data-usage-panel]';
  const st = await page.evaluate((sel) => {
    const el = document.querySelector(sel);
    return { state: el.querySelector('[data-up-state]').textContent, now: el.querySelector('[data-st="now"]').textContent, max: el.querySelector('[data-st="max"]').textContent, line: el.querySelector('.uc-line').getAttribute('d').length, groups: el.querySelectorAll('.up-groups li').length, note: el.querySelector('[data-up-note]').textContent };
  }, p);
  assert.equal(st.state, '실시간 · 1초마다');
  assert.match(st.now, /%$/); assert.match(st.max, /%$/);
  assert.ok(st.line > 50); assert.equal(st.groups, 4);
  assert.match(st.note, /GPU 메모리는 시스템 전체/);
  // 지표 라디오 — 화살표 키
  await page.focus(`${p} [data-up-metric="cpu"]`);
  await page.keyboard.press('ArrowRight');
  await page.keyboard.press('ArrowRight');
  const m = await page.evaluate((sel) => ({ focus: document.activeElement.dataset.upMetric, checked: document.querySelector(`${sel} [data-up-metric="mem"]`).getAttribute('aria-checked'), now: document.querySelector(`${sel} [data-st="now"]`).textContent }), p);
  assert.equal(m.focus, 'mem'); assert.equal(m.checked, 'true'); assert.match(m.now, /MB|GB/);
  await page.click(`${p} [data-up-range="60000"]`);
  assert.equal(await page.getAttribute(`${p} [data-up-range="60000"]`, 'aria-checked'), 'true');
  assert.match(await page.getAttribute(`${p} .uc-plot`, 'aria-label'), /메모리 최근 60초/);
  // 새 표본이 오면 통계가 따라온다
  await page.click(`${p} [data-up-metric="cpu"]`);
  await page.evaluate(() => window.__usage.push(777));
  await page.waitForTimeout(80);
  assert.equal(await page.textContent(`${p} [data-st="now"]`), '78%');
  // 표로 보기(1분 평균)
  await page.click(`${p} .usage-table summary`);
  const rows = await page.$$eval(`${p} .usage-table tbody tr`, (r) => r.length);
  assert.ok(rows >= 4 && rows <= 6, `1분 평균 줄 ${rows}`);
  assert.equal(await page.$eval(`${p} .usage-table thead`, (t) => t.textContent.replace(/\s+/g, '')), '시각CPUGPU메모리네트워크');
  // 같은 분 안의 새 표본·오래된 표본 잘림으로는 표를 다시 그리지 않는다(분이 바뀔 때만).
  const kept = await page.evaluate((sel) => {
    const row = document.querySelector(`${sel} .usage-table tbody tr`);
    const a = window.__usage.samples;
    const minute = (t) => Math.floor(t / 60000);
    const crosses = minute(a[a.length - 1].t + 1000) !== minute(a[a.length - 1].t) || minute(a[1].t) !== minute(a[0].t);
    window.__usage.push(50);
    return new Promise((r) => setTimeout(() => r({ crosses, same: document.querySelector(`${sel} .usage-table tbody tr`) === row }), 60));
  }, p);
  if (!kept.crosses) assert.ok(kept.same, '표를 초마다 다시 그림');
  await shot(page, 'settings');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('U07b 표는 분이 바뀔 때만', async () => {
  // 기록 기간 2분 — 표본이 들어올 때마다 맨 앞 표본이 잘린다(예전엔 맨 앞 시각을 키로 써서 표를 초마다 다시 그렸다).
  const { ctx, page } = await open({ historyMinutes: 2 });
  await openUsageTab(page);
  const p = '[data-usage-panel]';
  await page.click(`${p} .usage-table summary`);
  const r = await page.evaluate(async (sel) => {
    const minute = (t) => Math.floor(t / 60000);
    let rebuilt = 0; let crossings = 0;
    // 화면 기록(2분 창)의 맨 앞·맨 뒤 분 — 모의 쪽 목록은 잘리지 않으므로 같은 창으로 잘라 본다.
    const span = () => { const a = window.__usage.samples; const last = a[a.length - 1].t; return [minute(a.find((x) => x.t >= last - 120000).t), minute(last)]; };
    for (let i = 0; i < 5; i += 1) {
      const row = document.querySelector(`${sel} .usage-table tbody tr`);
      const before = span();
      window.__usage.push(50);
      await new Promise((ok) => setTimeout(ok, 30));
      const after = span();
      if (after[0] !== before[0] || after[1] !== before[1]) { crossings += 1; continue; }
      if (document.querySelector(`${sel} .usage-table tbody tr`) !== row) rebuilt += 1;
    }
    return { rebuilt, crossings };
  }, p);
  assert.equal(r.rebuilt, 0, JSON.stringify(r));
  assert.ok(r.crossings < 5, '분 경계만 지났다');
  await ctx.close();
});

await check('U08 내보내기·지우기', async () => {
  const { ctx, page } = await open();
  await openUsageTab(page);
  await page.click('[data-setting-action="usage-export-csv"]');
  await page.waitForTimeout(150);
  await page.click('[data-setting-action="usage-export-json"]');
  await page.waitForTimeout(150);
  assert.deepEqual((await calls(page, 'usage_export')).map((a) => a.format), ['csv', 'json']);
  const toast = await page.evaluate(() => [...document.querySelectorAll('.toast')].map((t) => t.textContent).join(' | '));
  assert.match(toast, /내보냈습니다 — \/Users\/someone\/Downloads\/CView\/CView-사용률-20261121-153005\.(csv|json)/);
  const clear = '[data-setting-action="usage-clear"]';
  await page.click(clear);
  await page.waitForTimeout(400);
  assert.equal((await calls(page, 'usage_clear')).length, 0, '첫 누름은 확인만');
  await page.click(clear);
  await page.waitForTimeout(150);
  assert.equal((await calls(page, 'usage_clear')).length, 1);
  assert.equal(await page.evaluate(() => document.querySelector('[data-usage-panel] .uc-empty').hidden), false, '빈 그래프 안내');
  await ctx.close();
});

await check('U09 설정 바꾸기', async () => {
  const { ctx, page } = await open();
  await openUsageTab(page);
  await page.click('[data-key="usageInHeader"] + span');
  await page.waitForFunction(() => document.querySelector('.usage-chip').hidden);
  // 성능 패널이 보고 있어 실시간 받기는 계속
  assert.equal((await calls(page, 'usage_watch')).at(-1).on, true);
  await page.click('[data-key="usageMonitor"] + span');
  await page.waitForFunction(() => document.querySelector('[data-up-state]').textContent === '측정 꺼짐');
  await page.click('[data-key="usageMonitor"] + span');
  await page.waitForFunction(() => document.querySelector('[data-up-state]').textContent.startsWith('실시간'));
  await go(page, 'home');
  await page.waitForTimeout(300);
  assert.equal((await calls(page, 'usage_watch')).at(-1).on, false, '설정을 떠나고 칩도 꺼져 있으면 그만 받는다');
  const patches = (await calls(page, 'settings_update')).map((a) => a.patch);
  assert.deepEqual(patches, [{ usageInHeader: false }, { usageMonitor: false }, { usageMonitor: true }]);
  await ctx.close();
});

await check('U10 팝오버에서 설정으로', async () => {
  const { ctx, page } = await open();
  await page.click('.usage-chip');
  await page.click('.usage-pop [data-up="settings"]');
  await page.waitForSelector('[data-page="settings"]:not([hidden]) [data-usage-panel]');
  assert.equal(await page.getAttribute('[data-page="settings"] [data-group="usage"]', 'aria-selected'), 'true');
  assert.equal(await page.evaluate(() => document.querySelector('.usage-pop').hidden), true);
  await ctx.close();
});

await check('U11 그래프 말풍선', async () => {
  const { ctx, page } = await open();
  await page.click('.usage-chip');
  const plot = '.usage-pop .uc-plot';
  const box = await page.locator(plot).boundingBox();
  await page.mouse.move(box.x + box.width * 0.5, box.y + box.height * 0.5);
  await page.waitForTimeout(60);
  const tip = await page.evaluate(() => { const t = document.querySelector('.usage-pop .uc-tip'); return { hidden: t.hidden, value: t.querySelector('strong').textContent, time: t.querySelector('span').textContent, cross: document.querySelector('.usage-pop .uc-cross').style.display }; });
  assert.equal(tip.hidden, false); assert.match(tip.value, /%$/); assert.match(tip.time, /^\d\d:\d\d:\d\d$/); assert.equal(tip.cross, 'block');
  await page.mouse.move(5, 700);
  await page.focus(plot);
  await page.keyboard.press('End');
  await page.keyboard.press('ArrowLeft');
  assert.equal(await page.evaluate(() => document.querySelector('.usage-pop .uc-tip').hidden), false);
  await page.keyboard.press('Escape');
  const after = await page.evaluate(() => ({ tip: document.querySelector('.usage-pop .uc-tip').hidden, pop: document.querySelector('.usage-pop').hidden }));
  assert.deepEqual(after, { tip: true, pop: false }, 'Esc 는 말풍선만');
  await ctx.close();
});

await check('U12 명령 없음', async () => {
  const { ctx, page, errors } = await open({ none: true });
  await page.waitForTimeout(500);
  assert.equal(await page.$$eval('.usage-chip:not([hidden])', (c) => c.length), 0);
  await go(page, 'settings');
  await page.waitForSelector('[data-page="settings"] .settings-tabs');
  await page.click('[data-page="settings"] [data-group="usage"]');
  await page.waitForFunction(() => /잴 수 없습니다/.test(document.querySelector('[data-usage-panel]')?.textContent || ''));
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('U13 보조 창', async () => {
  const { ctx, page } = await open({ query: '?w=chat' });
  await page.waitForTimeout(600);
  assert.equal(await page.$$eval('.usage-chip', (c) => c.length), 0);
  assert.equal((await page.evaluate(() => window.__usage.calls.length)), 0);
  await ctx.close();
});

for (const theme of ['dark', 'light']) {
  await check(`U14 axe ${theme}`, async () => {
    const { ctx, page } = await open({ theme });
    await page.evaluate(AXE);
    const run = (sel) => page.evaluate(async (s) => {
      const r = await window.axe.run(document.querySelector(s), { runOnly: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'best-practice'] });
      return r.violations.flatMap((v) => v.nodes.slice(0, 3).map((n) => `${v.id}: ${n.target.join(' ')}`));
    }, sel);
    await page.click('.usage-chip');
    await page.waitForTimeout(250);
    assert.deepEqual(await run('.usage-pop'), [], '팝오버');
    assert.deepEqual(await run('.usage-chip'), [], '머리줄 칩');
    await page.keyboard.press('Escape');
    await openUsageTab(page);
    await page.click('[data-usage-panel] .usage-table summary');
    assert.deepEqual(await run('.settings-groups.usage-tab'), [], '성능 탭');
    await ctx.close();
  });
}

for (const [width, height] of [[1000, 660], [1280, 800], [1600, 900]]) {
  await check(`U15 배치 ${width}`, async () => {
    const { ctx, page } = await open({ width, height });
    const greet = await page.evaluate(() => { const g = document.querySelector('.home-greeting'); return { h: g.getBoundingClientRect().height, lh: parseFloat(getComputedStyle(g).lineHeight) }; });
    if (width >= 1280) assert.ok(greet.h < greet.lh * 1.5, `홈 인사말 한 줄(${greet.h}/${greet.lh})`);
    const overlap = async () => page.evaluate(() => {
      const c = document.querySelector('.usage-chip');
      const r = c.getBoundingClientRect();
      const out = [];
      for (const sib of c.parentElement.children) {
        if (sib === c || sib.hidden || getComputedStyle(sib).display === 'none' || sib.matches('.grow, .ml-head-gap')) continue;
        const s = sib.getBoundingClientRect();
        if (s.width && s.height && s.left < r.right - 0.5 && r.left < s.right - 0.5 && s.top < r.bottom && r.top < s.bottom) out.push(sib.className);
      }
      if (r.right > window.innerWidth) out.push('창 밖');
      return out;
    });
    assert.deepEqual(await overlap(), [], '홈');
    await page.click('.usage-chip');
    const pr = await page.evaluate(() => { const r = document.querySelector('.usage-pop').getBoundingClientRect(); return [r.left, r.top, r.right, r.bottom, innerWidth, innerHeight]; });
    assert.ok(pr[0] >= 0 && pr[1] >= 0 && pr[2] <= pr[4] && pr[3] <= pr[5] + 1, `팝오버 창 안 ${pr}`);
    await page.keyboard.press('Escape');
    await go(page, 'multilive'); await page.waitForTimeout(300);
    assert.deepEqual(await overlap(), [], '라이브 머리줄');
    await openUsageTab(page);
    const of = await page.evaluate(() => {
      const t = document.querySelector('.settings-groups.usage-tab');
      const body = document.querySelector('[data-page="settings"] .page-body');
      return { tab: t.scrollWidth - t.clientWidth, body: body.scrollWidth - body.clientWidth, panel: [...document.querySelectorAll('[data-usage-panel] *')].filter((e) => e.getBoundingClientRect().right > t.getBoundingClientRect().right + 1).map((e) => e.className).slice(0, 4) };
    });
    assert.deepEqual(of, { tab: 0, body: 0, panel: [] });
    assert.deepEqual(await overlap(), [], '설정 머리줄');
    if (SHOTS) await page.screenshot({ path: path.join(SHOTS, `usage-settings-${width}-${ENGINE}.png`) });
    await ctx.close();
  });
}

await check('U16 모션 줄이기', async () => {
  const { ctx, page } = await open();
  await page.click('.usage-chip');
  const m = await page.evaluate(() => ({ dot: getComputedStyle(document.querySelector('.up-live i')).animationName, chip: getComputedStyle(document.querySelector('.usage-chip')).transitionDuration.split(',').map((x) => parseFloat(x)) }));
  assert.equal(m.dot, 'none');
  assert.ok(m.chip.every((d) => d < 0.01), `칩 전환 ${m.chip}`);
  await ctx.close();
  const o = await open({ motion: true });
  await o.page.click('.usage-chip');
  assert.notEqual(await o.page.evaluate(() => getComputedStyle(document.querySelector('.up-live i')).animationName), 'none', '모션을 켜면 깜박임');
  await o.ctx.close();
});

for (const width of [1280, 1100, 1000]) {
  await check(`U18 라이브 단독 보기 머리줄 ${width}`, async () => {
    const { ctx, page, errors } = await open({ width, height: 760, channels: 6 });
    await go(page, 'multilive');
    await page.waitForTimeout(300);
    await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:ml-watch', { detail: { sessionId: 's0' } })));
    await page.waitForSelector('.page-multilive.as-watch .ml-watch-picker:not([hidden])');
    await page.waitForTimeout(400);
    const m = await page.evaluate(() => {
      const c = document.querySelector('.usage-chip');
      const r = c.getBoundingClientRect();
      const picker = document.querySelector('.ml-watch-picker').getBoundingClientRect();
      const header = document.querySelector('.ml-header').getBoundingClientRect();
      const hits = [...document.querySelectorAll('.ml-header .ml-watch-chip, .ml-header .ml-more, .ml-header .ml-add, .ml-header .live-mode-seg, .ml-header .page-title')]
        .filter((e) => { const b = e.getBoundingClientRect(); return b.width && b.left < r.right - 0.5 && r.left < b.right - 0.5 && b.top < r.bottom && r.top < b.bottom; })
        .map((e) => e.className);
      // "+N"(가려진 채널 메뉴)은 칩 줄 안에 다 보여야 한다 — 사용률 칩 폭을 빼지 않으면 칩 줄이 넘쳐 끝에서 흐려졌다.
      const more = document.querySelector('.ml-watch-more.end:not([hidden])');
      const mr = more && more.getBoundingClientRect();
      return { parent: c.parentElement.className, compact: c.classList.contains('compact'), hits, inHeader: r.left >= header.left && r.right <= header.right + 0.5, afterPicker: r.left >= picker.right - 0.5,
        visibleChips: document.querySelectorAll('.ml-watch-picker .ml-watch-chip').length, more: Boolean(more), moreInside: !mr || (mr.left >= picker.left - 0.5 && mr.right <= picker.right + 0.5) };
    });
    if (SHOTS) await page.screenshot({ path: path.join(SHOTS, `usage-watch-${width}-${ENGINE}.png`), clip: { x: 0, y: 0, width, height: 70 } });
    assert.match(m.parent, /ml-toolbar/); assert.ok(m.compact);
    assert.deepEqual(m.hits, []);
    assert.ok(m.inHeader && m.afterPicker, JSON.stringify(m));
    assert.ok(m.visibleChips >= 2);
    assert.ok(m.more && m.moreInside, `"+N" 이 칩 줄 안에 보임 ${JSON.stringify(m)}`);
    assert.deepEqual(errors, []);
    await ctx.close();
  });
}

// U17 칩 갱신 비용(Chromium) — 표본 20개를 받는 동안 스크립트 시간·레이아웃 횟수. 칩은 글자·꺾은선만 바꾼다.
if (!WEBKIT) {
  await check('U17 칩 갱신 비용', async () => {
    const { ctx, page } = await open();
    const cdp = await ctx.newCDPSession(page);
    await cdp.send('Performance.enable');
    const metrics = async () => Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map((m) => [m.name, m.value]));
    await page.waitForTimeout(500);
    // 표본 없이 같은 시간 — 화면의 다른 움직임이 내는 레이아웃을 뺀다.
    const z0 = await metrics();
    for (let i = 0; i < 20; i += 1) { await page.evaluate(() => 0); await page.waitForTimeout(60); }
    const z1 = await metrics();
    const base = z1.LayoutCount - z0.LayoutCount;
    const a = await metrics();
    for (let i = 0; i < 20; i += 1) {
      await page.evaluate((k) => window.__usage.push(100 + (k % 5) * 30), i);
      await page.waitForTimeout(60);
    }
    const b = await metrics();
    const script = ((b.ScriptDuration - a.ScriptDuration) * 1000) / 20;
    const layouts = b.LayoutCount - a.LayoutCount - base;
    console.log(`    (U17 표본당 스크립트 ${script.toFixed(2)}ms, 레이아웃 ${layouts}회 + 바탕 ${base}회)`);
    assert.ok(script < 3, `표본당 스크립트 ${script.toFixed(2)}ms`);
    assert.ok(layouts <= 22, `레이아웃 ${layouts}회(바탕 ${base}회 뺌) — 표본당 한 번 안팎`);
    await ctx.close();
  });
}

await browser.close();
server.close();
let failed = 0;
for (const r of results) {
  console.log(`${r.ok ? '✓' : '✗'} ${r.id}`);
  if (!r.ok) { failed += 1; console.log(`    ${r.error.replace(/\n/g, '\n    ')}`); }
}
console.log(`\n${results.length - failed}/${results.length} 통과 (${WEBKIT ? 'WebKit' : 'Chromium'})`);
process.exit(failed ? 1 : 0);
