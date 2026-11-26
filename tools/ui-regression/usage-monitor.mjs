// 앱 사용률(사이드바 아래 표시·설정 › 성능) 회귀 검사 — 모의 IPC·헤드리스 브라우저(실제 측정·파일 저장은 하지 않는다).
// U01 자리 — 사이드바 아래(계정 칸·새 버전 줄 위)에 하나. 화면을 옮겨도 그대로, 어느 머리줄에도 사용률 칩이 없다.
// U02 값·이름 — 4줄(CPU 는 코어 수로 나눈 전체 대비 %, GPU·메모리·네트워크)과 줄마다 꺾은선. 이름(aria-label)에 네 값, aria-live 없음.
//      CPU 가 높으면 경고 아이콘·경고 바탕 + 이름에 'CPU 높음'(색만으로 알리지 않음). 초점이 있는 동안은 이름을 바꾸지 않는다.
// U03 접은 사이드바 — 레일 가운데 꺾은선 + CPU %, 레일 안에 들어감. 툴팁은 사이드바 툴팁(네 값), 기본 title 은 펼쳤을 때만.
// U04 누르기 — 마우스·키보드(Enter·Space), 접힌 레일에서도 설정 › 성능 탭으로 간다.
// U05 GPU 지원 — 못 읽으면 '—'·이름에 'GPU 지원 안 됨', 전체만 읽으면 전체 GPU 값과 'GPU(전체)'.
// U06 실시간 받기 — 보이면 usage_watch(true). 창이 숨으면 usage_watch(false), 다시 보이면 true 와 그동안 기록 채우기(since).
// U06b 다시 보일 때 채우기 응답보다 실시간 표본이 먼저 와도 숨은 동안의 기록을 잃지 않는다.
// U06c usage_watch 가 실패하면 5초 뒤 다시 보낸다. 아무도 보지 않아도 시작 때 false 를 한 번 보내 Rust 쪽 상태를 맞춘다.
// U07 설정 › 성능 — '연동'과 '고급' 사이 탭, 상태 알약·통계·그래프, 지표·기간 라디오(화살표 키), 표로 보기(1분 평균).
// U07b 표로 보기는 분 묶음이 바뀔 때만 다시 그린다(기록 기간이 짧아 맨 앞 표본이 초마다 잘려도).
// U08 내보내기·지우기 — CSV·JSON 은 usage_export(format) 후 경로 알림, 지우기는 두 번 눌러야(armConfirm) 한 번만 지운다.
// U09 설정 바꾸기 — '사이드바에 표시' 끄면 숨고, '사용률 측정'을 끄면 패널이 '측정 꺼짐'. 설정을 떠나면 실시간 받기를 그만둔다.
// U11 그래프 말풍선(설정 › 성능) — 포인터를 올리면 세로선·말풍선(값·시각), 초점 + ←·End 로 같은 말풍선, Esc 는 말풍선만 닫는다.
// U12 명령 없음(옛 백엔드·브라우저) — 그리지 않고 오류도 없다. 성능 탭은 '잴 수 없습니다'.
// U13 보조 창(?w=chat) — 표시도 usage_* 호출도 없다.
// U14 접근성 — axe(WCAG 2.1 A·AA + best-practice) 펼침·접힘·성능 탭, 보이는 글자 명암비 4.5:1 이상(라이트·다크, 경고 포함).
// U15 배치 — 1000×660(좁아 접힘)·1280×800·1600×900·1280×560(낮은 창): 사이드바 안·계정과 안 겹침·값이 잘리지 않음, 성능 탭 가로 넘침 없음.
//      Windows 레일(72px)에서도 접힌 칸이 레일 안.
// U16 모션 줄이기 — 칸 전환·눌림이 멈춘다.
// U17 갱신 비용(Chromium) — 표본당 스크립트 3ms 미만, 레이아웃은 표본당 한 번 안팎(펼침·접힘).
// U18 라이브 단독 보기 머리줄(채널 칩 6개, 1280·1000) — 사용률 칩이 없고 "+N" 이 칩 줄 안에 다 보인다.
// U20 낮은 창 — 높이에 따라 줄 수를 줄여(1000: 4줄, 900: CPU·메모리, 800: CPU) '방송 중' 목록이 사라지지 않는다(800 에서 줄 2개 반 이상).
// U19 칸 크기·자리가 값("9.8%"·"15%"·"100%")·경고에 따라 바뀌지 않아 아래 계정 칸·옆 글자가 흔들리지 않는다(펼침·접힘).
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
  const appearance = () => ({ theme: opts.theme || 'dark', usageMonitor: cfg.enabled, usageInHeader: cfg.inHeader, usageIntervalMs: cfg.intervalMs, usageHistoryMinutes: cfg.historyMinutes });
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

async function open({ width = 1280, height = 800, theme = 'dark', motion = false, query = '', collapsed = null, platform = null, ...opts } = {}) {
  const ctx = await browser.newContext({ viewport: { width, height }, colorScheme: theme, reducedMotion: motion ? 'no-preference' : 'reduce' });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => { const m = String(e); if (!/ResizeObserver loop/.test(m)) errors.push(m); });
  await page.addInitScript(mockTauri, [base, theme]);
  await page.addInitScript(usageMock, [{ ...opts, theme }]);
  // 사이드바 접기 상태(sidebar.js cview.sidebarCollapsed) — null 이면 창 폭(1100 이하면 접힘)대로.
  await page.addInitScript(([c]) => { localStorage.setItem('cview.welcomed.v1', '1'); if (c != null) localStorage.setItem('cview.sidebarCollapsed', c ? '1' : '0'); }, [collapsed]);
  if (platform) await page.addInitScript(([p]) => { document.documentElement.dataset.platform = p; new MutationObserver(() => { if (document.documentElement.dataset.platform !== p) document.documentElement.dataset.platform = p; }).observe(document.documentElement, { attributes: true, attributeFilter: ['data-platform'] }); }, [platform]);
  await page.goto(`${base}${query}`);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.waitForFunction(() => { const s = document.getElementById('splash'); return !s || !s.isConnected || getComputedStyle(s).display === 'none' || getComputedStyle(s).opacity === '0'; }, null, { timeout: 8000 }).catch(() => {});
  if (!opts.none && !query && opts.inHeader !== false) await page.waitForSelector('.side-usage:not([hidden])');
  await page.waitForTimeout(150);
  return { ctx, page, errors };
}

const calls = (page, name) => page.evaluate((n) => window.__usage.calls.filter((c) => c[0] === n).map((c) => c[1]), name);
const shot = async (page, name, clip) => { if (SHOTS) await page.screenshot({ path: path.join(SHOTS, `usage-${name}-${ENGINE}.png`), ...(clip ? { clip } : {}) }); };
// 사이드바 아래 칸 — 펼침 CPU 값·접힘 CPU 값.
const BOX = '.side-usage';
const CPU_V = `${BOX} .su-row[data-metric="cpu"] .su-v`;
const MINI_V = `${BOX} .su-mini .su-v`;
async function openUsageTab(page) {
  await go(page, 'settings');
  await page.waitForSelector('[data-page="settings"] .settings-tabs');
  await page.click('[data-page="settings"] [data-group="usage"]');
  await page.waitForSelector('[data-usage-panel][data-mounted] .uc-line');
  await page.waitForTimeout(200);
}
/** 사이드바 접기·펼치기(전환 520ms 를 기다린다). */
async function toggleSidebar(page) {
  await page.click('.sidebar-toggle');
  await page.waitForTimeout(650);
}
/** 캔버스에 칠한 화소 수. */
const inked = (page, sel) => page.$eval(sel, (cv) => { const px = cv.getContext('2d').getImageData(0, 0, cv.width, cv.height).data; let n = 0; for (let i = 3; i < px.length; i += 4) if (px[i] > 40) n += 1; return n; });
/** 사이드바 아래쪽 장면(캡처용). */
const sideClip = (page) => page.evaluate(() => { const r = document.querySelector('.sidebar').getBoundingClientRect(); const b = document.querySelector('.side-usage').getBoundingClientRect(); const top = Math.max(0, b.top - 60); return { x: 0, y: top, width: Math.ceil(r.width) + 300, height: Math.min(innerHeight - top, r.bottom - top) }; });

await check('U01 자리', async () => {
  const { ctx, page, errors } = await open();
  const where = () => page.evaluate(() => {
    const c = document.querySelector('.side-usage');
    const sb = document.querySelector('.sidebar').getBoundingClientRect();
    const r = c.getBoundingClientRect();
    const acc = document.querySelector('#account').getBoundingClientRect();
    return { count: document.querySelectorAll('.side-usage').length, parent: c.parentElement.className, prev: c.previousElementSibling?.className || '', next: c.nextElementSibling?.id || '',
      chips: document.querySelectorAll('.usage-chip, .usage-pop').length, inSide: r.left >= sb.left && r.right <= sb.right, aboveAccount: r.bottom <= acc.top + 0.5, gap: Math.round(acc.top - r.bottom), shown: !c.hidden, rect: [r.left, r.top, r.width, r.height].map(Math.round) };
  });
  const first = await where();
  assert.equal(first.count, 1);
  assert.match(first.parent, /sidebar/);
  assert.match(first.prev, /sidebar-spacer/);
  assert.equal(first.next, 'update-pill', '새 버전 줄·계정 칸 바로 위');
  assert.equal(first.chips, 0, '머리줄 칩·팝오버 없음');
  assert.ok(first.inSide && first.aboveAccount && first.gap <= 20, JSON.stringify(first));
  for (const route of ['following', 'settings', 'multilive', 'home']) {
    await go(page, route); await page.waitForTimeout(250);
    const w = await where();
    assert.deepEqual(w.rect, first.rect, `${route} 에서도 같은 자리`);
    assert.equal(w.chips, 0);
    assert.equal(w.count, 1);
  }
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('U02 값·이름', async () => {
  const { ctx, page } = await open();
  await page.evaluate(() => window.__usage.push(123.4));
  await page.waitForTimeout(80);
  const v = await page.evaluate(() => {
    const c = document.querySelector('.side-usage');
    const rows = [...c.querySelectorAll('.su-row')].map((r) => [r.dataset.metric, r.querySelector('.su-k').textContent, r.querySelector('.su-v').textContent]);
    return { rows, label: c.getAttribute('aria-label'), title: c.title, live: c.querySelectorAll('[aria-live]').length + (c.closest('[aria-live]') ? 1 : 0), hidden: [...c.children].every((x) => x.getAttribute('aria-hidden') === 'true'), level: c.dataset.level, warn: getComputedStyle(c.querySelector('.su-row .su-warn')).visibility };
  });
  assert.deepEqual(v.rows.map((r) => r[0]), ['cpu', 'gpu', 'mem', 'net']);
  assert.deepEqual(v.rows.map((r) => r[1]), ['CPU', 'GPU', '메모리', '네트워크']);
  assert.equal(v.rows[0][2], '12%'); // 123.4 / 10코어
  assert.match(v.rows[1][2], /^[\d.]+%$/);
  assert.match(v.rows[2][2], /^\d+ MB$/);
  assert.match(v.rows[3][2], /^[\d.]+ (kbps|Mbps)$/);
  for (const m of ['cpu', 'gpu', 'mem', 'net']) assert.ok(await inked(page, `.su-row[data-metric="${m}"] canvas`) > 30, `${m} 꺾은선`);
  assert.match(v.label, /^앱 사용률 — CPU 12%, GPU [\d.]+%, 메모리 \d+ MB, 네트워크 [\d.]+ (kbps|Mbps)\. 설정 › 성능에서 자세히 보기$/);
  assert.match(v.title, /^CPU 12% · GPU .* — 눌러서 설정 › 성능$/, '펼친 사이드바는 기본 툴팁');
  assert.equal(v.live, 0, '초마다 읽지 않는다');
  assert.ok(v.hidden, '보이는 숫자는 aria-hidden(이름으로만 읽음)');
  assert.equal(v.level, 'ok');
  assert.equal(v.warn, 'hidden');
  await page.evaluate(() => window.__usage.push(900));
  await page.waitForTimeout(80);
  const high = await page.evaluate(() => { const c = document.querySelector('.side-usage'); const row = c.querySelector('.su-row[data-metric="cpu"]'); return { level: c.dataset.level, warn: getComputedStyle(row.querySelector('.su-warn')).visibility, bg: getComputedStyle(row).backgroundColor, label: c.getAttribute('aria-label') }; });
  assert.equal(high.level, 'high');
  assert.equal(high.warn, 'visible', '경고 아이콘');
  assert.notEqual(high.bg, 'rgba(0, 0, 0, 0)', '경고 바탕');
  assert.match(high.label, /CPU 90%.*\(CPU 높음\)/);
  await shot(page, 'side-high', await sideClip(page));
  // 초점이 있는 동안은 이름을 바꾸지 않는다(낭독기가 초마다 다시 읽지 않게).
  await page.focus(BOX);
  const before = await page.getAttribute(BOX, 'aria-label');
  await page.evaluate(() => window.__usage.push(10));
  await page.waitForTimeout(80);
  assert.equal(await page.getAttribute(BOX, 'aria-label'), before);
  assert.equal(await page.textContent(CPU_V), '1.0%', '보이는 값은 바뀐다');
  await page.evaluate(() => document.activeElement.blur());
  await page.waitForTimeout(1);
  await shot(page, 'side', await sideClip(page));
  await ctx.close();
});

await check('U03 접은 사이드바', async () => {
  const { ctx, page, errors } = await open();
  await toggleSidebar(page);
  await page.evaluate(() => window.__usage.push(456));
  await page.waitForTimeout(80);
  const st = await page.evaluate(() => {
    const c = document.querySelector('.side-usage');
    const sb = document.querySelector('.sidebar');
    const r = c.getBoundingClientRect();
    const sr = sb.getBoundingClientRect();
    const mini = c.querySelector('.su-mini').getBoundingClientRect();
    return { collapsed: sb.classList.contains('collapsed'), full: getComputedStyle(c.querySelector('.su-full')).display, mini: getComputedStyle(c.querySelector('.su-mini')).display, v: c.querySelector('.su-mini .su-v').textContent,
      inRail: r.left >= sr.left && r.right <= sr.right, center: Math.abs((mini.left + mini.right) / 2 - (sr.left + sr.right) / 2), title: c.getAttribute('title'), tip: c.dataset.tip, label: c.getAttribute('aria-label') };
  });
  assert.ok(st.collapsed);
  assert.equal(st.full, 'none'); assert.notEqual(st.mini, 'none');
  assert.equal(st.v, '46%');
  assert.ok(await inked(page, `${BOX} .su-mini canvas`) > 20, '접힘 꺾은선');
  assert.ok(st.inRail, JSON.stringify(st));
  assert.ok(st.center < 3, `레일 가운데(${st.center})`);
  assert.equal(st.title, null, '접히면 기본 툴팁 없음(사이드바 툴팁과 겹치지 않게)');
  assert.match(st.tip, /^CPU 46% · GPU .* · 메모리 .* · 네트워크 .* — 눌러서 설정 › 성능$/);
  assert.match(st.label, /CPU 46%/);
  await page.hover(BOX);
  await page.waitForTimeout(80);
  const tip = await page.evaluate(() => { const t = document.querySelector('.side-tip'); return { hidden: t.hidden, text: t.textContent }; });
  assert.equal(tip.hidden, false); assert.match(tip.text, /CPU 46%/);
  await page.evaluate(() => window.__usage.push(950));
  await page.waitForTimeout(80);
  assert.equal(await page.evaluate(() => getComputedStyle(document.querySelector('.su-mini .su-warn')).display), 'block', '접힘 경고 아이콘');
  await page.mouse.move(600, 400);
  await shot(page, 'side-collapsed', await sideClip(page));
  // 다시 펼치면 4줄과 기본 툴팁.
  await toggleSidebar(page);
  const back = await page.evaluate(() => { const c = document.querySelector('.side-usage'); return { full: getComputedStyle(c.querySelector('.su-full')).display, title: c.getAttribute('title'), v: c.querySelector('.su-row .su-v').textContent }; });
  assert.notEqual(back.full, 'none'); assert.match(back.title || '', /CPU 95%/); assert.equal(back.v, '95%');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('U04 누르면 설정 › 성능', async () => {
  const { ctx, page } = await open();
  const onUsageTab = async () => {
    await page.waitForSelector('[data-page="settings"]:not([hidden]) [data-usage-panel]');
    assert.equal(await page.getAttribute('[data-page="settings"] [data-group="usage"]', 'aria-selected'), 'true');
  };
  await page.click(BOX);
  await onUsageTab();
  await go(page, 'home'); await page.waitForTimeout(250);
  await page.focus(BOX); await page.keyboard.press('Enter');
  await onUsageTab();
  await go(page, 'following'); await page.waitForTimeout(250);
  await page.focus(BOX); await page.keyboard.press('Space');
  await onUsageTab();
  await go(page, 'home'); await page.waitForTimeout(250);
  await toggleSidebar(page);
  await page.click(BOX);
  await onUsageTab();
  await ctx.close();
});

await check('U05 GPU 지원', async () => {
  let { ctx, page } = await open({ gpu: { probed: true, app: false, system: false, memoryScope: null, name: null } });
  let t = await page.evaluate(() => { const c = document.querySelector('.side-usage'); return { v: c.querySelector('.su-row[data-metric="gpu"] .su-v').textContent, label: c.getAttribute('aria-label') }; });
  assert.equal(t.v, '—'); assert.match(t.label, /GPU 지원 안 됨/);
  await ctx.close();
  ({ ctx, page } = await open({ gpu: { probed: true, app: false, system: true, memoryScope: 'system', name: null } }));
  t = await page.evaluate(() => { const c = document.querySelector('.side-usage'); return { v: c.querySelector('.su-row[data-metric="gpu"] .su-v').textContent, label: c.getAttribute('aria-label') }; });
  assert.equal(t.v, '18%'); assert.match(t.label, /GPU\(전체\) 18%/);
  await ctx.close();
});

await check('U06 실시간 받기·숨김', async () => {
  const { ctx, page } = await open();
  assert.deepEqual((await calls(page, 'usage_watch')).map((a) => a.on), [true]);
  await page.evaluate(() => { window.__vis = 'hidden'; document.dispatchEvent(new Event('visibilitychange')); });
  await page.waitForTimeout(100);
  assert.deepEqual((await calls(page, 'usage_watch')).map((a) => a.on), [true, false]);
  const last = await page.evaluate(() => window.__usage.samples.at(-1).t);
  const before = await page.textContent(CPU_V);
  await page.evaluate(() => { for (let i = 0; i < 3; i += 1) window.__usage.push(500, { emit: true }); }); // watching=false 라 이벤트 없음
  await page.waitForTimeout(80);
  assert.equal(await page.textContent(CPU_V), before, '숨은 동안 화면 갱신 없음');
  await page.evaluate(() => { window.__vis = 'visible'; document.dispatchEvent(new Event('visibilitychange')); });
  await page.waitForTimeout(150);
  assert.deepEqual((await calls(page, 'usage_watch')).map((a) => a.on), [true, false, true]);
  const hist = await calls(page, 'usage_history');
  assert.equal(hist.at(-1).since, last, '숨은 동안 기록을 since 로 채움');
  assert.equal(await page.textContent(CPU_V), '50%', '채운 뒤 최신 값');
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
  // 사이드바 표시가 꺼져 아무도 보지 않아도 시작 때 false 를 한 번 보내 Rust 쪽(창 새로고침 전 상태)을 맞춘다.
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
  assert.equal(await o.page.textContent(CPU_V), '33%', '재시도 뒤 실시간 표본');
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
  // 표로 보기(1분 평균) — <details> toggle 이벤트가 행을 채울 때까지 기다린 뒤 읽는다.
  await page.click(`${p} .usage-table summary`);
  await page.waitForSelector(`${p} .usage-table tbody tr`);
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
  await page.waitForSelector(`${p} .usage-table tbody tr`);
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
  assert.match(await page.evaluate(() => document.querySelector('[data-key="usageInHeader"]').closest('.setting-row, .row, label, div').textContent), /사이드바에 표시/);
  await page.click('[data-key="usageInHeader"] + span');
  await page.waitForFunction(() => document.querySelector('.side-usage').hidden);
  // 성능 패널이 보고 있어 실시간 받기는 계속
  assert.equal((await calls(page, 'usage_watch')).at(-1).on, true);
  await page.click('[data-key="usageMonitor"] + span');
  await page.waitForFunction(() => document.querySelector('[data-up-state]').textContent === '측정 꺼짐');
  await page.click('[data-key="usageMonitor"] + span');
  await page.waitForFunction(() => document.querySelector('[data-up-state]').textContent.startsWith('실시간'));
  await go(page, 'home');
  await page.waitForTimeout(300);
  assert.equal((await calls(page, 'usage_watch')).at(-1).on, false, '설정을 떠나고 사이드바 표시도 꺼져 있으면 그만 받는다');
  const patches = (await calls(page, 'settings_update')).map((a) => a.patch);
  assert.deepEqual(patches, [{ usageInHeader: false }, { usageMonitor: false }, { usageMonitor: true }]);
  // 측정을 끄면 표시 설정이 켜져 있어도 숨는다.
  await openUsageTab(page);
  await page.click('[data-key="usageInHeader"] + span');
  await page.waitForFunction(() => !document.querySelector('.side-usage').hidden);
  await page.click('[data-key="usageMonitor"] + span');
  await page.waitForFunction(() => document.querySelector('.side-usage').hidden);
  await ctx.close();
});

await check('U11 그래프 말풍선', async () => {
  const { ctx, page } = await open();
  await openUsageTab(page);
  const plot = '[data-usage-panel] .uc-plot';
  const box = await page.locator(plot).boundingBox();
  await page.mouse.move(box.x + box.width * 0.5, box.y + box.height * 0.5);
  await page.waitForTimeout(60);
  const tip = await page.evaluate(() => { const t = document.querySelector('[data-usage-panel] .uc-tip'); return { hidden: t.hidden, value: t.querySelector('strong').textContent, time: t.querySelector('span').textContent, cross: document.querySelector('[data-usage-panel] .uc-cross').style.display }; });
  assert.equal(tip.hidden, false); assert.match(tip.value, /%$/); assert.match(tip.time, /^\d\d:\d\d:\d\d$/); assert.equal(tip.cross, 'block');
  await page.mouse.move(5, 700);
  await page.focus(plot);
  await page.keyboard.press('End');
  await page.keyboard.press('ArrowLeft');
  assert.equal(await page.evaluate(() => document.querySelector('[data-usage-panel] .uc-tip').hidden), false);
  await page.keyboard.press('Escape');
  const after = await page.evaluate(() => ({ tip: document.querySelector('[data-usage-panel] .uc-tip').hidden, settings: !document.querySelector('[data-page="settings"]').hidden }));
  assert.deepEqual(after, { tip: true, settings: true }, 'Esc 는 말풍선만');
  await ctx.close();
});

await check('U12 명령 없음', async () => {
  const { ctx, page, errors } = await open({ none: true });
  await page.waitForTimeout(500);
  assert.equal(await page.$$eval('.side-usage', (c) => c.length), 0);
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
  assert.equal(await page.$$eval('.side-usage, .usage-chip', (c) => c.length), 0);
  assert.equal((await page.evaluate(() => window.__usage.calls.length)), 0);
  await ctx.close();
});

/** 보이는 글자(이름·값·머리)의 명암비 — 칸 바탕(접히면 사이드바 바탕) 기준. 경고 바탕이 깔린 CPU 줄은 그 바탕을 겹쳐 계산한다. */
const textContrast = (page) => page.evaluate(() => {
  const rgb = (s) => { const m = s.match(/[\d.]+/g).map(Number); return { r: m[0], g: m[1], b: m[2], a: m[3] ?? 1 }; };
  const over = (top, bot) => ({ r: top.r * top.a + bot.r * (1 - top.a), g: top.g * top.a + bot.g * (1 - top.a), b: top.b * top.a + bot.b * (1 - top.a), a: 1 });
  const lum = (c) => { const f = (v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; }; return 0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b); };
  const ratio = (a, b) => { const x = lum(a); const y = lum(b); return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05); };
  const box = document.querySelector('.side-usage');
  let bg = rgb(getComputedStyle(document.querySelector('.sidebar')).backgroundColor);
  bg = over(rgb(getComputedStyle(box).backgroundColor), bg);
  const out = [];
  for (const el of box.querySelectorAll('.su-k, .su-v')) {
    if (!el.getClientRects().length) continue;
    const row = el.closest('.su-row');
    const b = row ? over(rgb(getComputedStyle(row).backgroundColor), bg) : bg;
    out.push([el.className + (row ? `:${row.dataset.metric}` : '') + (el.closest('.su-mini') ? ':mini' : ''), Math.round(ratio(over(rgb(getComputedStyle(el).color), b), b) * 100) / 100]);
  }
  return out;
});

for (const theme of ['dark', 'light']) {
  await check(`U14 접근성 ${theme}`, async () => {
    // 높이 1000 — 4줄 모두 보이게(낮은 창은 줄 수를 줄인다, U20).
    const { ctx, page } = await open({ theme, height: 1000 });
    await page.evaluate(AXE);
    const run = (sel) => page.evaluate(async (s) => {
      const r = await window.axe.run(document.querySelector(s), { runOnly: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'best-practice'] });
      return r.violations.flatMap((v) => v.nodes.slice(0, 3).map((n) => `${v.id}: ${n.target.join(' ')}`));
    }, sel);
    const low = (list) => list.filter(([, r]) => r < 4.5);
    assert.deepEqual(await run(BOX), [], '펼침');
    let c = await textContrast(page);
    assert.ok(c.length >= 8, JSON.stringify(c));
    assert.deepEqual(low(c), [], `펼침 명암비 ${JSON.stringify(c)}`);
    // 경고(CPU 높음) — 옅은 경고 바탕 위 글자도 4.5:1.
    await page.evaluate(() => window.__usage.push(950));
    await page.waitForTimeout(80);
    c = await textContrast(page);
    assert.deepEqual(low(c), [], `경고 명암비 ${JSON.stringify(c)}`);
    // 키보드 — Tab 으로 닿고 초점 고리가 보인다.
    await page.focus(BOX);
    await page.keyboard.press('Shift+Tab'); await page.keyboard.press('Tab');
    const ring = await page.evaluate(() => { const a = document.activeElement; return { on: a.classList.contains('side-usage'), shadow: getComputedStyle(a).boxShadow }; });
    assert.ok(ring.on, '탭 순서에 있음'); assert.notEqual(ring.shadow, 'none', '초점 고리');
    await page.evaluate(() => document.activeElement.blur());
    await toggleSidebar(page);
    assert.deepEqual(await run(BOX), [], '접힘');
    c = await textContrast(page);
    assert.deepEqual(low(c), [], `접힘 명암비 ${JSON.stringify(c)}`);
    await shot(page, `side-collapsed-${theme}`, await sideClip(page));
    await toggleSidebar(page);
    await page.evaluate(() => window.__usage.push(123));
    await page.waitForTimeout(80);
    await shot(page, `side-${theme}`, await sideClip(page));
    await openUsageTab(page);
    await page.click('[data-usage-panel] .usage-table summary');
    assert.deepEqual(await run('.settings-groups.usage-tab'), [], '성능 탭');
    await ctx.close();
  });
}

for (const [width, height, platform] of [[1000, 660], [1280, 800], [1600, 900], [1280, 560], [1000, 660, 'windows']]) {
  await check(`U15 배치 ${width}×${height}${platform ? ` ${platform}` : ''}`, async () => {
    const { ctx, page } = await open({ width, height, platform });
    // 긴 값 — 네트워크 "12.5 Mbps"·메모리 "1.95 GB"·CPU "100%" 가 잘리지 않아야 한다.
    await page.evaluate(() => { const s = window.__usage.push(1000); s.netRx = 12.5e6 / 8; s.mem = 1.95 * 1073741824; window.__emit('usage://sample', { sample: s, groups: [] }); });
    await page.waitForTimeout(80);
    const fit = () => page.evaluate(() => {
      const c = document.querySelector('.side-usage');
      const sb = document.querySelector('.sidebar');
      const r = c.getBoundingClientRect();
      const sr = sb.getBoundingClientRect();
      const out = [];
      if (r.left < sr.left - 0.5 || r.right > sr.right + 0.5) out.push('사이드바 밖');
      for (const sel of ['#account', '#update-pill:not([hidden])', '.sidebar-live:not([hidden])', '#nav']) {
        const e = document.querySelector(sel); if (!e) continue;
        const b = e.getBoundingClientRect();
        if (b.height && b.top < r.bottom - 0.5 && r.top < b.bottom - 0.5) out.push(`겹침 ${sel}`);
      }
      for (const v of c.querySelectorAll('.su-v, .su-k')) if (v.getClientRects().length && v.scrollWidth > v.clientWidth + 0.5) out.push(`잘림 ${v.closest('[data-metric]')?.dataset.metric || 'mini'} ${v.textContent}`);
      // 낮은 창 — 사이드바가 스크롤되어 칸에 닿을 수 있다.
      if (sb.scrollHeight > sb.clientHeight && getComputedStyle(sb).overflowY === 'visible') out.push('스크롤 안 됨');
      return { out, collapsed: sb.classList.contains('collapsed') };
    });
    let f = await fit();
    assert.deepEqual(f.out, [], `처음(${f.collapsed ? '접힘' : '펼침'})`);
    assert.equal(f.collapsed, width <= 1100, '1100 이하는 접힌 레일');
    if (SHOTS) await page.screenshot({ path: path.join(SHOTS, `usage-layout-${width}x${height}${platform ? `-${platform}` : ''}-${ENGINE}.png`) });
    await toggleSidebar(page);
    f = await fit();
    assert.deepEqual(f.out, [], `바꾼 뒤(${f.collapsed ? '접힘' : '펼침'})`);
    await openUsageTab(page);
    const of = await page.evaluate(() => {
      const t = document.querySelector('.settings-groups.usage-tab');
      const body = document.querySelector('[data-page="settings"] .page-body');
      return { tab: t.scrollWidth - t.clientWidth, body: body.scrollWidth - body.clientWidth, panel: [...document.querySelectorAll('[data-usage-panel] *')].filter((e) => e.getBoundingClientRect().right > t.getBoundingClientRect().right + 1).map((e) => e.className).slice(0, 4) };
    });
    assert.deepEqual(of, { tab: 0, body: 0, panel: [] });
    await ctx.close();
  });
}

await check('U16 모션 줄이기', async () => {
  const { ctx, page } = await open();
  const dur = (p) => p.evaluate(() => [...document.querySelectorAll('.side-usage')].flatMap((e) => getComputedStyle(e).transitionDuration.split(',').map((x) => parseFloat(x))));
  const m = await dur(page);
  assert.ok(m.every((d) => d < 0.01), `전환 ${m}`);
  await ctx.close();
  const o = await open({ motion: true });
  assert.ok((await dur(o.page)).some((d) => d > 0.05), '모션을 켜면 전환');
  await o.ctx.close();
});

for (const width of [1280, 1000]) {
  await check(`U18 라이브 단독 보기 머리줄 ${width}`, async () => {
    const { ctx, page, errors } = await open({ width, height: 760, channels: 6 });
    await go(page, 'multilive');
    await page.waitForTimeout(300);
    await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:ml-watch', { detail: { sessionId: 's0' } })));
    await page.waitForSelector('.page-multilive.as-watch .ml-watch-picker:not([hidden])');
    await page.waitForTimeout(400);
    const m = await page.evaluate(() => {
      const picker = document.querySelector('.ml-watch-picker').getBoundingClientRect();
      const more = document.querySelector('.ml-watch-more.end:not([hidden])');
      const mr = more && more.getBoundingClientRect();
      return { chip: document.querySelectorAll('.ml-header .usage-chip, .ml-header .side-usage').length, visibleChips: document.querySelectorAll('.ml-watch-picker .ml-watch-tab:not([hidden])').length, more: Boolean(more), moreInside: !mr || (mr.left >= picker.left - 0.5 && mr.right <= picker.right + 0.5) };
    });
    if (SHOTS) await page.screenshot({ path: path.join(SHOTS, `usage-watch-${width}-${ENGINE}.png`), clip: { x: 0, y: 0, width, height: 70 } });
    assert.equal(m.chip, 0, '머리줄에 사용률 없음');
    assert.ok(m.visibleChips >= 2);
    assert.ok(m.more && m.moreInside, `"+N" 이 칩 줄 안에 보임 ${JSON.stringify(m)}`);
    assert.deepEqual(errors, []);
    await ctx.close();
  });
}

for (const [height, rows] of [[1000, ['cpu', 'gpu', 'mem', 'net']], [900, ['cpu', 'mem']], [800, ['cpu']]]) {
  await check(`U20 낮은 창 ${height}`, async () => {
    const { ctx, page } = await open({ width: 1280, height, collapsed: false });
    const st = await page.evaluate(() => {
      const l = document.querySelector('.sidebar-live');
      const row = l.querySelector('.side-live');
      return { rows: [...document.querySelectorAll('.side-usage .su-row')].filter((r) => r.getClientRects().length).map((r) => r.dataset.metric), live: l.hidden ? null : l.clientHeight, row: row ? row.offsetHeight : 0,
        label: document.querySelector('.side-usage').getAttribute('aria-label') };
    });
    assert.deepEqual(st.rows, rows);
    assert.match(st.label, /GPU .*메모리 .*네트워크 /, '빠진 줄도 이름에는 있다');
    if (height === 800) assert.ok(st.live != null && st.live >= st.row * 2.5, `방송 중 목록 ${st.live}px(줄 ${st.row}px)`);
    await ctx.close();
  });
}

for (const [label, collapsed] of [['펼침', false], ['접힘', true]]) {
  await check(`U19 크기·자리가 값·경고에 따라 바뀌지 않음 ${label}`, async () => {
    const { ctx, page } = await open({ width: 1280, height: 760, collapsed });
    // "—"(표본 전)·9.8%·15%·100%·92%(경고) — 칸 크기·값 열 자리·아래 계정 칸 자리가 같아야 한다.
    const rows = [];
    for (const cpu of [98, 150, 1000, 920, 5]) {
      await page.evaluate((v) => window.__usage.push(v), cpu);
      await page.waitForTimeout(60);
      rows.push(await page.evaluate(() => {
        const c = document.querySelector('.side-usage');
        const r = c.getBoundingClientRect();
        const v = c.querySelector('.sidebar.collapsed .su-mini .su-v') || c.querySelector('.su-row[data-metric="cpu"] .su-v');
        const k = c.querySelector('.su-row[data-metric="cpu"] .su-k').getBoundingClientRect();
        const net = c.querySelector('.su-row[data-metric="net"] .su-v').getBoundingClientRect();
        const round = (x) => Math.round(x * 10) / 10;
        return { v: v.textContent, level: c.dataset.level, w: round(r.width), h: round(r.height), top: round(r.top), k: round(k.width), net: round(net.left),
          acc: round(document.querySelector('#account').getBoundingClientRect().top), spark: round(c.querySelector('.sidebar.collapsed .su-mini .su-spark, .sidebar:not(.collapsed) .su-row .su-spark').getBoundingClientRect().left) };
      }));
    }
    assert.deepEqual(rows.map((r) => r.v), ['9.8%', '15%', '100%', '92%', '0.5%']);
    assert.equal(rows[3].level, 'high');
    const one = (k) => new Set(rows.map((r) => r[k])).size === 1;
    for (const k of ['w', 'h', 'top', 'acc', 'spark', ...(collapsed ? [] : ['k', 'net'])]) assert.ok(one(k), `${k} ${JSON.stringify(rows)}`);
    await ctx.close();
  });
}

// U17 갱신 비용(Chromium) — 표본 20개를 받는 동안 스크립트 시간·레이아웃 횟수. 글자·꺾은선만 바꾼다.
if (!WEBKIT) {
  for (const [label, collapsed] of [['펼침', false], ['접힘', true]]) {
    await check(`U17 갱신 비용 ${label}`, async () => {
      const { ctx, page } = await open({ collapsed });
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
      console.log(`    (U17 ${label} 표본당 스크립트 ${script.toFixed(2)}ms, 레이아웃 ${layouts}회 + 바탕 ${base}회)`);
      assert.ok(script < 3, `표본당 스크립트 ${script.toFixed(2)}ms`);
      assert.ok(layouts <= 22, `레이아웃 ${layouts}회(바탕 ${base}회 뺌) — 표본당 한 번 안팎`);
      await ctx.close();
    });
  }
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
