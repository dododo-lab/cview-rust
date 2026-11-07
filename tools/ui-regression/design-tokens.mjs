// 디자인 토큰·글자 대비 UI 회귀 검사.
// 1) CSS 정적 검사 — 반경(4px 이상)·글자 크기(px)를 리터럴로 쓰지 않고 :root 토큰(--radius-* · --fs-*)을 쓰는지,
//    쓰는 토큰이 모두 정의돼 있는지, 같은 토큰을 :root 에서 두 번 정의하지 않는지.
// 2) 화면 검사 — 주요 화면을 다크·라이트로 띄워 axe-core 의 color-contrast 위반이 없는지(이미지 위 글자인 홈 히어로 .hero-main 은 axe 가 배경을 못 읽어 제외).
// app-ui 를 그대로 띄우고 window.__TAURI__ 를 모의 IPC 로 바꿔 실제 화면 코드를 돌린다(실제 서비스·데이터는 건드리지 않음).
// 저장소 루트에서: node tools/ui-regression/design-tokens.mjs
//   Playwright 와 axe-core 가 필요하다(예: npm i playwright@1.49.1 axe-core@4.10.2 를 다른 폴더에 설치하고 NODE_PATH 로 지정).
//   CVIEW_UI_SHOTS=<폴더> 면 화면 × 폭(640·900·1280·1680·2560) × 테마 스크린샷을 남긴다 — 단계별 UI 작업의 전후 비교용.
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');
const AXE = fs.readFileSync(require.resolve('axe-core/axe.min.js'), 'utf8');

const ROOT = path.resolve('app-ui');
const CSS = ['app.css', 'multilive.css', 'motion.css'].map((f) => [f, fs.readFileSync(path.join(ROOT, 'css', f), 'utf8')]);
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}

// ── 1) CSS 정적 검사 ──
const lineOf = (src, index) => src.slice(0, index).split('\n').length;
await check('D01 border-radius uses tokens (literal only for <=3px)', () => {
  const bad = [];
  for (const [file, src] of CSS) {
    for (const m of src.matchAll(/(?<![-\w])border(?:-(?:top|bottom)-(?:left|right))?-radius\s*:\s*([^;}!]+)/g)) {
      for (const px of m[1].matchAll(/(?<![\w.-])(\d+(?:\.\d+)?)px\b/g)) if (Number(px[1]) > 3) bad.push(`${file}:${lineOf(src, m.index)} ${m[0].trim()}`);
    }
  }
  assert.deepEqual(bad, [], '반경은 var(--radius-*) 토큰을 쓴다');
});
await check('D02 font-size uses tokens (literal only for decorative >=40px)', () => {
  const bad = [];
  for (const [file, src] of CSS) {
    // 선언 값 전체를 본다 — calc()·clamp()·var() 대체값 안의 px 도 리터럴로 친다. 값이 40px 이상 리터럴 하나뿐일 때만 허용.
    for (const m of src.matchAll(/(?<![-\w])font-size\s*:\s*([^;}]+)/g)) {
      const value = m[1].replace(/\/\*[\s\S]*?\*\//g, '').replace(/!important/, '').trim();
      const decorative = /^(\d+(?:\.\d+)?)px$/.exec(value);
      if (decorative && Number(decorative[1]) >= 40) continue;
      if (/(?<![\w.-])\d+(?:\.\d+)?px\b/.test(value)) bad.push(`${file}:${lineOf(src, m.index)} font-size: ${value}`);
    }
  }
  assert.deepEqual(bad, [], '글자 크기는 var(--fs-*) 토큰을 쓴다');
});
await check('D03 every used --radius-*/--fs-* token is defined once in :root', () => {
  const all = CSS.map(([, s]) => s).join('\n');
  const used = new Set([...all.matchAll(/var\((--(?:radius|fs)-[\w-]+)/g)].map((m) => m[1]));
  // 세 스타일시트의 모든 :root 블록을 모은다 — 나중에 로드되는 파일의 :root 가 같은 토큰을 다시 정의해도 잡는다.
  const defs = [];
  for (const [file, src] of CSS) {
    for (const block of src.matchAll(/(?:^|\n):root\s*\{([^}]*)\}/g)) {
      for (const d of block[1].matchAll(/(--(?:radius|fs)-[\w-]+)\s*:/g)) defs.push({ token: d[1], at: `${file}:${lineOf(src, block.index + d.index)}` });
    }
  }
  const names = defs.map((d) => d.token);
  const missing = [...used].filter((t) => !names.includes(t));
  const dup = defs.filter((d) => names.indexOf(d.token) !== names.lastIndexOf(d.token)).map((d) => `${d.token} @ ${d.at}`);
  assert.deepEqual(missing, [], '정의 안 된 토큰');
  assert.deepEqual(dup, [], ':root 에서 두 번 정의된 토큰(뒤 정의가 앞 정의를 덮는다)');
  // 검사 자체가 정의를 찾는지 — 0개면 정규식이 깨진 것
  assert.ok(names.includes('--fs-body') && names.includes('--radius-md'), ':root 토큰 정의를 찾지 못함');
});

// ── 2) 화면 검사 ──
const PALETTE = [['#ff6b6b', '#5f27cd'], ['#00d2d3', '#341f97'], ['#feca57', '#ee5253'], ['#48dbfb', '#0abde3'], ['#1dd1a1', '#10ac84'], ['#ff9ff3', '#f368e0']];
function fakeImage(kind, n) {
  const [a, b] = PALETTE[n % PALETTE.length];
  if (kind === 'avatar') return `<svg xmlns="http://www.w3.org/2000/svg" width="120" height="120"><rect width="120" height="120" fill="${a}"/><circle cx="60" cy="48" r="22" fill="#fff" opacity=".85"/></svg>`;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></linearGradient></defs><rect width="1280" height="720" fill="url(#g)"/></svg>`;
}
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.png': 'image/png', '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.json': 'application/json' };
const server = http.createServer((req, res) => {
  const url = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  const img = url.match(/^\/__img\/(\w+)\/(\d+)\.svg$/);
  if (img) { res.writeHead(200, { 'content-type': 'image/svg+xml' }); res.end(fakeImage(img[1], Number(img[2]))); return; }
  const file = path.join(ROOT, url === '/' ? 'index.html' : url);
  if (!file.startsWith(ROOT) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) { res.writeHead(404); res.end(); return; }
  res.writeHead(200, { 'content-type': TYPES[path.extname(file)] || 'application/octet-stream' });
  fs.createReadStream(file).pipe(res);
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${server.address().port}/`;

// 모의 IPC — 로그인·데이터 있음. 화면이 쓰는 명령만 채우고 나머지는 실패로 돌려 오류·빈 상태 코드도 지나가게 둔다.
function mockTauri([origin, theme]) {
  const img = (kind, n) => `${origin}__img/${kind}/${n}.svg`;
  const names = ['릴카', '플러리', '따효니', '괴물쥐', '침착맨', '녹두로', '한동숙', '풍월량', '김도', '소풍왔니', '마라맛', '서새봄'];
  const cats = ['마인크래프트', '메이플스토리', '배틀그라운드', '리그 오브 레전드', '스타크래프트', '저스트 채팅'];
  const titles = ['시청자 참여 내전 + 경품', '신작 첫 플레이! 스포 금지', '혼돈의 서버 생존기', '대회 리뷰 & 분석', '저녁 먹고 수다 한 판', '새벽 감성 노래방'];
  const now = Date.now();
  const ago = (min) => new Date(now - min * 60000).toISOString();
  const ch = (i) => ({ channelId: `ch${i}`, channelName: names[i % names.length], channelImageUrl: img('avatar', i) });
  const live = (i, v) => ({ ...ch(i), liveTitle: titles[i % titles.length], thumbnailUrl: img('thumb', i), viewerCount: v, viewerCountText: v.toLocaleString('ko-KR'), categoryName: cats[i % cats.length], openDate: ago(20 + i * 17), isLive: true, adult: false });
  const lives = Array.from({ length: 12 }, (_, i) => live(i, Math.round(60000 / (1 + i))));
  const clips = Array.from({ length: 8 }, (_, i) => ({ clipUID: `c${i}`, clipTitle: `레전드 순간 #${i + 1}`, thumbnailImageUrl: img('thumb', 30 + i), duration: 40 + i * 7, readCount: 120000 - i * 9000, channel: ch(i) }));
  const vods = Array.from({ length: 8 }, (_, i) => ({ videoNo: 500 + i, videoTitle: titles[i % titles.length], thumbnailImageUrl: img('thumb', 10 + i), duration: 9000, readCount: 80 + i, publishDate: ago(i * 600), channel: ch(i) }));
  const cont = Array.from({ length: 4 }, (_, i) => ({ videoNo: 100 + i, title: `${titles[i]} 다시보기`, thumbnailUrl: img('thumb', 20 + i), duration: 7200, position: 1200 + i * 900, channelName: names[i] }));
  const channels = Array.from({ length: 12 }, (_, i) => ({ ...ch(i), followerCountText: `${i + 1}.2만`, openLive: i < 4, isFavorite: i < 3, lastWatched: ago(i * 90) }));
  const trend = Array.from({ length: 7 }, (_, i) => ({ start: ago((7 - i) * 1440), seconds: (i % 3 + 1) * 4000, parts: [2000, 1000, 1000 * (i % 3)] }));
  const total = trend.reduce((a, t) => a + t.seconds, 0);
  const report = (period) => ({
    period, rangeStart: ago(7 * 1440), rangeEnd: new Date().toISOString(), days: 7, unit: 'day', total, previousTotal: total * 0.8, sessionCount: 21, activeDays: 6, averageSession: total / 21, dailyAverage: total / 7,
    longest: { channelId: 'ch0', channelName: names[0], category: cats[0], startedAt: ago(2880), seconds: 20000 }, channelCount: 8, categoryCount: 6, currentStreak: 4, bestStreak: 12, firstWatchedAt: ago(400 * 1440),
    channels: names.slice(0, 8).map((name, i) => ({ channelId: `ch${i}`, name, seconds: total / (i + 2), sessions: 9 - i, lastWatchedAt: ago(i * 400) })), otherChannels: { count: 0, seconds: 0 },
    categories: cats.map((name, i) => ({ name, seconds: total / (i + 2), sessions: 6 - i })), otherCategories: { count: 0, seconds: 0 },
    heatmap: Array.from({ length: 7 }, (_, d) => Array.from({ length: 24 }, (_, h) => ((h + d) % 5) * 600)), series: cats.slice(0, 3), trend,
    lengths: [600, 1800, 3600, 7200, 14400, null].map((max, i) => ({ maxSeconds: max, count: i + 1, seconds: (i + 1) * 3000 })),
  });
  const handlers = {
    auth_status: () => ({ loggedIn: true, state: 'loggedIn', nickname: '도도', profileImageUrl: img('avatar', 3) }),
    settings_get: () => ({ multiLive: { maxConcurrentSessions: 4, maxLiveChannels: 5 }, multiChat: {}, player: {}, chat: {}, general: {}, appearance: { theme } }),
    following_lives: () => lives.slice(0, 6), popular_lives: () => lives, popular_clips: () => clips, following_vods: () => vods,
    following_channels_all: () => channels, recent_channels: ({ limit } = {}) => channels.slice(0, limit || 8), favorites_list: () => channels.slice(0, 3),
    vod_continue_list: () => cont, vod_watch_later_list: () => cont.slice(0, 2),
    bookmarks_list: () => [{ id: 'b1', channelId: 'ch1', channelName: names[1], liveTitle: '옛 장면', liveUptime: 600, capturedAt: ago(3000) }],
    download_list: () => ({ folder: '/tmp/dl', jobs: [], history: [] }),
    insights_report: ({ period }) => report(period),
    insights_overview: () => ({ sessionCount: 5, streak: 3, last7: 9000, average: 1800, recent: [], daily: [], channels: [], categories: [], hourly: Array(24).fill(0) }),
    data_health: () => ({ available: true, storeBytes: 1000, freeBytes: 1e9, backups: [], tables: [{ label: '시청 기록', rows: 3 }], backupDir: '/tmp/b', storePath: '/tmp/s' }),
    search: () => ({ lives: lives.slice(0, 2), channels: channels.slice(0, 6), clips, hasMoreClips: true, errors: {} }),
    search_videos: () => ({ items: vods, hasMore: true }),
    app_info: () => ({ version: '0.1.0', build: 1, os: 'macos', arch: 'aarch64' }),
    multilive_state: () => ({ sessions: [], layout: 'grid' }),
  };
  window.__TAURI__ = {
    core: { invoke: async (cmd, args) => { const h = handlers[cmd]; if (!h) throw new Error(`mock: ${cmd}`); return h(args || {}); } },
    event: { listen: async () => () => {} },
  };
  try { localStorage.setItem('cview.onboarded', '1'); localStorage.setItem('cview.theme', theme); } catch { /* 무시 */ }
}

const ROUTES = [['home', {}], ['following', {}], ['popular', {}], ['categories', {}], ['multichat', {}], ['multilive', {}], ['library', {}], ['stats', {}], ['search', { query: '루다' }], ['settings', {}]];
const SHOTS = process.env.CVIEW_UI_SHOTS;
const WIDTHS = SHOTS ? [640, 900, 1280, 1680, 2560] : [1280];
const browser = await chromium.launch();
const go = (page, route, params) => page.evaluate(([r, p]) => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: r, params: p } })), [route, params]);

for (const theme of ['dark', 'light']) {
  for (const width of WIDTHS) {
    const ctx = await browser.newContext({ viewport: { width, height: 900 }, colorScheme: theme });
    const page = await ctx.newPage();
    page.setDefaultTimeout(8000);
    await page.addInitScript(mockTauri, [base, theme]);
    await page.goto(base);
    await page.waitForSelector('[data-page]', { state: 'attached' });
    await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
    await page.waitForFunction(() => { const s = document.getElementById('splash'); return !s || !s.isConnected || getComputedStyle(s).display === 'none' || getComputedStyle(s).opacity === '0'; }, null, { timeout: 8000 }).catch(() => {});
    for (const [route, params] of ROUTES) {
      await go(page, route, params);
      await page.waitForTimeout(700);
      await page.mouse.move(1, 1);
      if (SHOTS) { fs.mkdirSync(SHOTS, { recursive: true }); await page.screenshot({ path: path.join(SHOTS, `${route}-${theme}-${width}.png`) }); }
      if (width !== 1280) continue;
      await check(`D10 ${theme} ${route}: text contrast meets WCAG AA`, async () => {
        await page.evaluate(AXE);
        const bad = await page.evaluate(async () => {
          const r = await window.axe.run({ include: [['.app']], exclude: [['.hero-main'], ['[hidden]']] }, { runOnly: ['color-contrast'] });
          return r.violations.flatMap((v) => v.nodes.map((n) => `${n.target.join(' ')} ${n.any[0]?.data?.fgColor}/${n.any[0]?.data?.bgColor} ${n.any[0]?.data?.contrastRatio}`));
        });
        assert.deepEqual(bad, []);
      });
    }
    await ctx.close();
  }
}

await browser.close();
server.close();
for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : `\n     ${r.error.split('\n').slice(0, 12).join('\n     ')}`}`);
const failed = results.filter((r) => !r.ok).length;
console.log(`\n${results.length - failed}/${results.length} passed`);
process.exitCode = failed ? 1 : 0;
