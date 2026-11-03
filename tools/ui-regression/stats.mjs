// 통계 화면 UI 회귀 검사 + 스크린샷 — 모의 IPC 의 기간 리포트(insights_report)로 개요·채널·시간대 탭을 그린다.
// 실제 서비스·데이터는 건드리지 않는다. 리포트 숫자는 씨앗 고정 의사난수라 매번 같다.
// 저장소 루트에서: node tools/ui-regression/stats.mjs   (CVIEW_UI_SHOTS=<폴더> 면 PNG 를 남긴다)
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');

const ROOT = path.resolve('app-ui');
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.png': 'image/png', '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.json': 'application/json' };
const server = http.createServer((req, res) => {
  const url = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  const file = path.join(ROOT, url === '/' ? 'index.html' : url);
  if (!file.startsWith(ROOT) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) { res.writeHead(404); res.end(); return; }
  res.writeHead(200, { 'content-type': TYPES[path.extname(file)] || 'application/octet-stream' });
  fs.createReadStream(file).pipe(res);
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${server.address().port}/`;

// mode: full(기록 있음) · empty(첫 실행) · gap(전체에만 기록이 있고 다른 기간은 비어 있음)
function mockTauri(mode) {
  const HOUR = 3600;
  const CATS = ['Just Chatting', '리그 오브 레전드', '발로란트', '마인크래프트', '음악', '스타크래프트', '토크', '스포츠'];
  const NAMES = ['루다', '니니아', '옛따', '동그레', '아로다루송송', '푸르히아하우나나', '시루', '탐', '크랭크', '유봄냥', '철면수심', '티뭉', '연니비', '루코', '리니링', '김니디', '미르', '포키팡', '루시아', '아브실'];
  let seed = 7;
  const rnd = () => { seed = (seed * 1103515245 + 12345) % 2147483648; return seed / 2147483648; };
  const startOfDay = (d) => { const x = new Date(d); x.setHours(0, 0, 0, 0); return x; };
  const addDays = (d, n) => { const x = new Date(d); x.setDate(x.getDate() + n); return x; };

  function buckets(period, now) {
    const today = startOfDay(now);
    if (period === 'today') return { unit: 'hour', days: 1, start: today, list: Array.from({ length: 24 }, (_, h) => new Date(today.getTime() + h * HOUR * 1000)) };
    if (period === '7d' || period === '30d') {
      const n = period === '7d' ? 7 : 30;
      return { unit: 'day', days: n, start: addDays(today, -(n - 1)), list: Array.from({ length: n }, (_, i) => addDays(today, i - n + 1)) };
    }
    if (period === '90d') {
      const start = addDays(today, -89);
      const monday = addDays(start, -((start.getDay() + 6) % 7));
      const list = [];
      for (let w = monday; w <= now; w = addDays(w, 7)) list.push(w);
      return { unit: 'week', days: 90, start, list };
    }
    const first = new Date(today.getFullYear(), today.getMonth() - 13, 1);
    const list = [];
    for (let m = first; m <= now; m = new Date(m.getFullYear(), m.getMonth() + 1, 1)) list.push(m);
    return { unit: 'month', days: Math.round((today - first) / 86400000) + 1, start: first, list };
  }

  function report(period) {
    const now = new Date();
    const b = buckets(period, now);
    const empty = mode === 'empty' || (mode === 'gap' && period !== 'all');
    const scale = { hour: 0.25, day: 2.2, week: 12, month: 48 }[b.unit] * HOUR;
    const weights = [0.42, 0.24, 0.14, 0.1, 0.1];
    const trend = b.list.map((start, i) => {
      const future = start > now;
      const zero = empty || future || (b.unit === 'day' && i % 6 === 2) || (b.unit === 'hour' && (start.getHours() < 9 && start.getHours() > 2));
      const seconds = zero ? 0 : Math.round(scale * (0.3 + rnd() * 1.4));
      const parts = weights.map((w) => Math.round(seconds * w * (0.7 + rnd() * 0.6)));
      const sum = parts.reduce((a, x) => a + x, 0) || 1;
      const norm = parts.map((p) => (seconds * p) / sum);
      return { start: start.toISOString(), seconds, parts: norm };
    });
    const total = trend.reduce((a, x) => a + x.seconds, 0);
    const hourW = Array.from({ length: 24 }, (_, h) => (h >= 19 || h <= 1 ? 3 : h >= 12 ? 1.4 : h >= 3 && h <= 8 ? 0.05 : 0.6));
    const dayW = [0.8, 0.7, 0.9, 0.8, 1.1, 1.6, 1.5];
    const hw = hourW.reduce((a, x) => a + x, 0);
    const dw = dayW.reduce((a, x) => a + x, 0);
    const heatmap = dayW.map((d) => hourW.map((h) => Math.round((total * d * h) / (hw * dw) * (0.6 + rnd() * 0.8))));
    const chWeights = NAMES.map((_, i) => 1 / (i + 1.4));
    const cw = chWeights.reduce((a, x) => a + x, 0);
    const channels = empty ? [] : NAMES.map((name, i) => ({
      channelId: `ch${i}`, name, seconds: (total * chWeights[i]) / cw, sessions: Math.max(1, Math.round(30 / (i + 1))),
      lastWatchedAt: new Date(now - (i * 7 + 1) * 3600000).toISOString(),
    }));
    const catW = [0.4, 0.22, 0.13, 0.09, 0.06, 0.05, 0.03, 0.02];
    const categories = empty ? [] : CATS.map((name, i) => ({ name, seconds: total * catW[i], sessions: Math.round(40 * catW[i]) + 1 }));
    const sessionCount = empty ? 0 : Math.max(1, Math.round(total / 4100));
    const counts = [0.08, 0.2, 0.27, 0.25, 0.15, 0.05].map((x) => Math.round(sessionCount * x));
    return {
      period,
      rangeStart: b.start.toISOString(),
      rangeEnd: now.toISOString(),
      days: b.days,
      unit: b.unit,
      total,
      previousTotal: period === 'all' ? null : total * 0.82,
      sessionCount,
      activeDays: empty ? 0 : Math.max(1, Math.round(b.days * 0.8)),
      averageSession: sessionCount ? total / sessionCount : 0,
      dailyAverage: total / b.days,
      longest: empty ? null : { channelId: 'ch0', channelName: NAMES[0], category: CATS[0], startedAt: new Date(now - 2 * 86400000).toISOString(), seconds: 5.6 * HOUR },
      channelCount: channels.length,
      categoryCount: categories.length,
      currentStreak: empty ? 0 : 4,
      bestStreak: empty ? 0 : 12,
      firstWatchedAt: mode === 'empty' ? null : new Date(now - 400 * 86400000).toISOString(),
      channels: channels.slice(0, 15),
      otherChannels: { count: Math.max(0, channels.length - 15), seconds: channels.slice(15).reduce((a, c) => a + c.seconds, 0) },
      categories,
      otherCategories: { count: 0, seconds: 0 },
      heatmap,
      series: categories.slice(0, 4).map((c) => c.name),
      trend: trend.map((t) => (empty ? { ...t, parts: [0] } : t)),
      lengths: [600, 1800, 3600, 7200, 14400, null].map((max, i) => ({ maxSeconds: max, count: counts[i], seconds: counts[i] * (max || 20000) * 0.6 })),
    };
  }

  const health = { available: true, storeBytes: 1000, freeBytes: 1e9, backups: [], tables: [{ label: '시청 기록', rows: 3 }], backupDir: '/tmp/b', storePath: '/tmp/s' };
  const handlers = {
    auth_status: () => ({ loggedIn: false, state: 'loggedOut' }),
    settings_get: () => ({ multiLive: { maxConcurrentSessions: 4, maxLiveChannels: 5 }, multiChat: {}, player: {}, chat: {}, general: {} }),
    popular_lives: () => [],
    data_health: () => health,
    insights_report: ({ period }) => report(period),
    insights_overview: () => ({ sessionCount: 0, recent: [], daily: [], channels: [], categories: [], hourly: Array(24).fill(0) }),
  };
  window.__mock = { handlers, calls: [] };
  window.__TAURI__ = {
    core: { invoke: async (cmd, args) => { window.__mock.calls.push({ cmd, args }); const h = handlers[cmd]; if (!h) throw new Error(`mock: ${cmd}`); return h(args); } },
    event: { listen: async () => () => {} },
  };
  try { localStorage.setItem('cview.onboarded', '1'); } catch { /* 무시 */ }
}

const SHOTS = process.env.CVIEW_UI_SHOTS;
const browser = await chromium.launch();
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}

async function openStats({ width = 1440, height = 960, scheme = 'dark', mode = 'full', params = {} } = {}) {
  const page = await browser.newPage({ viewport: { width, height }, colorScheme: scheme });
  page.setDefaultTimeout(5000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, mode);
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim').forEach((x) => x.remove()));
  await go(page, 'stats', params);
  await page.waitForSelector('[data-page="stats"]:not([hidden]) .stats-body > :not(.loading)');
  await page.waitForTimeout(300);
  return { page, errors };
}
const go = (page, route, params = {}) => page.evaluate(([r, p]) => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: r, params: p } })), [route, params]);
const S = '[data-page="stats"]';
const reportCalls = (page) => page.evaluate(() => window.__mock.calls.filter((c) => c.cmd === 'insights_report').map((c) => c.args.period));
async function waitReport(page) {
  await page.waitForFunction(() => !document.querySelector('[data-page="stats"] .stats-body .empty.loading'));
  await page.waitForTimeout(150);
}

async function shotFull(page, name) {
  if (!SHOTS) return;
  fs.mkdirSync(SHOTS, { recursive: true });
  await page.mouse.move(2, 2);
  await page.evaluate(() => document.activeElement?.blur());
  await page.waitForTimeout(200);
  const h = await page.evaluate(() => {
    const el = document.querySelector('[data-page="stats"]');
    el.scrollTop = 0;
    return Math.ceil(el.scrollHeight + el.getBoundingClientRect().top + 8);
  });
  const vp = page.viewportSize();
  await page.setViewportSize({ width: vp.width, height: Math.max(vp.height, h) });
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(SHOTS, `${name}.png`) });
  await page.setViewportSize(vp);
}

const noOverflow = (page) => page.evaluate(() => {
  const el = document.querySelector('[data-page="stats"]');
  return el.scrollWidth - el.clientWidth;
});

// ── 개요 ─────────────────────
const wide = await openStats({ params: { tab: 'overview', period: '30d' } });
await check('T01 overview renders KPIs, stacked trend, top channels and category donut', async () => {
  assert.deepEqual(wide.errors, []);
  const p = wide.page;
  assert.equal(await p.locator(`${S} .kpi-card`).count(), 6);
  assert.equal(await p.locator(`${S} [data-chart="trend"] .chart-col`).count(), 30);
  assert.ok(await p.locator(`${S} [data-chart="trend"] .chart-stack > i.s1`).count() > 10, 'stacked series');
  assert.equal(await p.locator(`${S} .share-bars li`).count(), 5);
  assert.ok(await p.locator(`${S} [data-chart="cats"] .donut-arc`).count() >= 5);
  assert.match(await p.locator(`${S} .kpi-card`).first().textContent(), /▲ \d+% · 직전 30일 대비/);
  assert.equal(await p.locator(`${S} .chart-avg`).count(), 1, 'average line');
});
await check('T02 bar heights follow values on a nice axis', async () => {
  const r = await wide.page.evaluate(() => {
    const chart = document.querySelector('[data-chart="trend"]');
    const plot = chart.querySelector('.chart-plot').getBoundingClientRect().height;
    const stacks = [...chart.querySelectorAll('.chart-col')].map((c) => c.querySelector('.chart-stack')?.getBoundingClientRect().height || 0);
    const labels = [...chart.querySelectorAll('.chart-y span')].map((s) => s.textContent);
    return { plot, stacks, labels };
  });
  const tallest = Math.max(...r.stacks);
  assert.ok(tallest > r.plot * 0.5 && tallest <= r.plot + 0.5, `tallest ${tallest} / ${r.plot}`);
  assert.ok(r.stacks.some((h) => h === 0), 'zero days have no bar');
  assert.ok(r.labels.length >= 3 && r.labels.every((t) => /^(0|\d+(\.\d)?시간|\d+분)$/.test(t)), r.labels.join(','));
});
await check('T03 hovering a column shows a tooltip with date, total and series', async () => {
  const p = wide.page;
  const col = p.locator(`${S} [data-chart="trend"] .chart-col:has(.chart-stack)`).nth(15);
  await col.hover();
  const tip = p.locator(`${S} .chart-tip`);
  await tip.waitFor({ state: 'visible' });
  const text = await tip.textContent();
  assert.match(text, /\d+월 \d+일 \([월화수목금토일]\)/);
  assert.match(text, /(시간|분)/);
  assert.ok(await tip.locator('li').count() >= 2, 'per-series rows');
  const box = await tip.boundingBox();
  const vp = p.viewportSize();
  assert.ok(box.x >= 0 && box.y >= 0 && box.x + box.width <= vp.width && box.y + box.height <= vp.height, JSON.stringify(box));
  assert.equal(await col.evaluate((el) => el.classList.contains('is-active')), true);
  await p.mouse.move(2, 2);
  await p.waitForTimeout(100);
  assert.equal(await tip.isVisible(), false, 'tooltip hides when the pointer leaves');
});
await check('T04 keyboard moves through columns and announces values', async () => {
  const p = wide.page;
  await p.focus(`${S} [data-chart="trend"]`);
  const first = await p.getAttribute(`${S} [data-chart="trend"]`, 'data-active');
  await p.keyboard.press('ArrowLeft');
  const second = await p.getAttribute(`${S} [data-chart="trend"]`, 'data-active');
  assert.equal(Number(second), Number(first) - 1);
  assert.match(await p.locator(`${S} [data-stats-live]`).textContent(), /\d+월 \d+일/);
  await p.keyboard.press('Home');
  assert.equal(await p.getAttribute(`${S} [data-chart="trend"]`, 'data-active'), '0');
  await p.keyboard.press('Escape');
  assert.equal(await p.locator(`${S} .chart-tip`).isVisible(), false);
});
await shotFull(wide.page, 'stats-overview-30d');

await check('T05 period buttons refetch, mark pressed and remember the choice', async () => {
  const p = wide.page;
  await p.locator(`${S} [data-period="7d"]`).click();
  await waitReport(p);
  assert.equal(await p.locator(`${S} [data-chart="trend"] .chart-col`).count(), 7);
  assert.equal(await p.getAttribute(`${S} [data-period="7d"]`, 'aria-pressed'), 'true');
  assert.equal(await p.getAttribute(`${S} [data-period="30d"]`, 'aria-pressed'), 'false');
  assert.equal(await p.evaluate(() => localStorage.getItem('cview.stats.period')), '7d');
  await p.locator(`${S} [data-period="today"]`).click();
  await waitReport(p);
  assert.equal(await p.locator(`${S} [data-chart="trend"] .chart-col`).count(), 24);
  assert.equal(await p.locator(`${S} .chart-avg`).count(), 0, 'no average line for hours');
  assert.match(await p.locator(`${S} .kpi-card`).first().textContent(), /어제 이 시각/);
  await p.locator(`${S} [data-period="all"]`).click();
  await waitReport(p);
  assert.equal(await p.locator(`${S} [data-chart="trend"] .chart-col`).count(), 14);
  assert.match(await p.locator(`${S} [data-chart="trend"] .chart-x`).textContent(), /\d{2}년 1월/);
  assert.deepEqual((await reportCalls(p)).slice(-3), ['7d', 'today', 'all']);
});
await shotFull(wide.page, 'stats-overview-all');

// ── 채널 · 카테고리 ─────────────────────
await check('T06 ranking tab lists channels with share, sessions and last watched', async () => {
  const p = wide.page;
  await p.locator(`${S} [data-period="90d"]`).click();
  await waitReport(p);
  await p.locator(`${S} [role="tab"][data-tab="ranking"]`).click();
  await waitReport(p);
  assert.equal(await p.locator(`${S} .rank-table .rt-row:not(.rt-head)`).count(), 15);
  assert.match(await p.locator(`${S} .rank-others`).first().textContent(), /그 밖의 채널 5개/);
  const row = await p.locator(`${S} .rank-table .rt-row:not(.rt-head)`).first().textContent();
  assert.match(row, /루다/);
  assert.match(row, /\d+%/);
  assert.match(row, /\d+회/);
  assert.match(row, /(분|시간|일) 전/);
  assert.equal(await p.locator(`${S} [data-open-channel-id="ch0"]`).count(), 1);
  assert.equal(await p.locator(`${S} [data-chart="cats"] .donut-legend li`).count(), 5, 'top 4 + 기타');
  await p.locator(`${S} [data-chart="cats"] .donut-legend li`).nth(1).hover();
  assert.match(await p.locator(`${S} .chart-tip`).textContent(), /리그 오브 레전드.*전체의 \d+%/);
});
await shotFull(wide.page, 'stats-ranking');

// ── 시간대 ─────────────────────
await check('T07 time tab shows heatmap, hourly, weekday and session lengths', async () => {
  const p = wide.page;
  await p.locator(`${S} [role="tab"][data-tab="time"]`).click();
  await waitReport(p);
  assert.equal(await p.locator(`${S} [data-chart="heat"] .hm-grid .hm-cell`).count(), 168);
  assert.ok(await p.locator(`${S} [data-chart="heat"] .hm-cell.l4`).count() >= 1);
  assert.equal(await p.locator(`${S} [data-chart="hours"] .chart-col`).count(), 24);
  assert.equal(await p.locator(`${S} [data-chart="days"] .chart-col`).count(), 7);
  assert.equal(await p.locator(`${S} .len-bars li`).count(), 6);
  assert.equal(await p.locator(`${S} .kpi-card`).count(), 5);
  await p.focus(`${S} [data-chart="heat"]`);
  await p.keyboard.press('Home');
  await p.keyboard.press('ArrowDown');
  assert.equal(await p.getAttribute(`${S} [data-chart="heat"]`, 'data-active'), '24');
  assert.match(await p.locator(`${S} [data-stats-live]`).textContent(), /^화요일 0시 ~ 1시/);
  await p.locator(`${S} [data-chart="heat"] [data-i="${5 * 24 + 21}"]`).hover();
  assert.match(await p.locator(`${S} .chart-tip`).textContent(), /토요일 21시 ~ 22시/);
});
await shotFull(wide.page, 'stats-time');
await check('T08 period bar hides on history and data tabs', async () => {
  const p = wide.page;
  await p.locator(`${S} [role="tab"][data-tab="history"]`).click();
  await waitReport(p);
  assert.equal(await p.locator(`${S} .stats-period`).isVisible(), false);
  await p.locator(`${S} [role="tab"][data-tab="overview"]`).click();
  await waitReport(p);
  assert.equal(await p.locator(`${S} .stats-period`).isVisible(), true);
  assert.deepEqual(wide.errors, []);
});
await wide.page.close();

// ── 빈 상태 ─────────────────────
const first = await openStats({ mode: 'empty' });
await check('T09 first run shows an empty state, not empty charts', async () => {
  assert.match(await first.page.locator(`${S} .stats-body .empty`).textContent(), /아직 시청 기록이 없습니다/);
  assert.equal(await first.page.locator(`${S} .chart`).count(), 0);
  assert.deepEqual(first.errors, []);
});
await first.page.close();
const gap = await openStats({ mode: 'gap', params: { period: '7d' } });
await check('T10 empty period offers the whole history', async () => {
  const p = gap.page;
  assert.match(await p.locator(`${S} .stats-body .empty`).textContent(), /최근 7일에는 시청 기록이 없습니다/);
  await p.locator(`${S} .stats-body [data-period="all"]`).click();
  await waitReport(p);
  assert.equal(await p.getAttribute(`${S} .stats-period [data-period="all"]`, 'aria-pressed'), 'true');
  assert.ok(await p.locator(`${S} [data-chart="trend"]`).count() === 1);
});
await gap.page.close();

// ── 좁은 창 ─────────────────────
for (const width of [900, 700]) {
  const narrow = await openStats({ width, height: 860, params: { tab: 'overview', period: '30d' } });
  await check(`T11-${width} narrow window has no horizontal overflow on every tab`, async () => {
    const p = narrow.page;
    for (const tab of ['overview', 'ranking', 'time']) {
      await p.locator(`${S} [role="tab"][data-tab="${tab}"]`).click();
      await waitReport(p);
      const over = await noOverflow(p);
      assert.ok(over <= 1, `${tab} overflow ${over}px`);
    }
    // 좁으면 순위표의 횟수·마지막 시청 열을 접는다
    await p.locator(`${S} [role="tab"][data-tab="ranking"]`).click();
    await waitReport(p);
    if (width === 700) assert.equal(await p.locator(`${S} .rt-opt`).first().isVisible(), false);
    assert.deepEqual(narrow.errors, []);
  });
  if (width === 700) {
    await narrow.page.locator(`${S} [role="tab"][data-tab="overview"]`).click();
    await waitReport(narrow.page);
    await shotFull(narrow.page, 'stats-narrow');
  }
  await narrow.page.close();
}

// ── 라이트 테마 ─────────────────────
const light = await openStats({ scheme: 'light', params: { tab: 'overview', period: '30d' } });
await light.page.evaluate(() => document.documentElement.setAttribute('data-theme', 'light'));
await check('T12 light theme keeps readable chart colors', async () => {
  const bg = await light.page.evaluate(() => getComputedStyle(document.querySelector('[data-chart="trend"] .chart-stack > i.s0')).backgroundColor);
  assert.notEqual(bg, 'rgba(0, 0, 0, 0)');
  assert.deepEqual(light.errors, []);
});
await shotFull(light.page, 'stats-light');
await light.page.locator(`${S} [role="tab"][data-tab="time"]`).click();
await waitReport(light.page);
await shotFull(light.page, 'stats-time-light');
await light.page.close();

await browser.close();
server.close();
let failed = 0;
for (const r of results) {
  console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : ` — ${r.error}`}`);
  if (!r.ok) failed++;
}
process.exit(failed ? 1 : 0);
