// UI 회귀 검사 공용 — app-ui 를 그대로 띄우는 정적 서버와 모의 IPC(window.__TAURI__), 앱 열기·화면 이동 도우미.
// 실제 서비스·데이터는 건드리지 않는다. 쓰는 쪽: design-tokens.mjs, responsive.mjs
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';

const ROOT = path.resolve('app-ui');
const PALETTE = [['#ff6b6b', '#5f27cd'], ['#00d2d3', '#341f97'], ['#feca57', '#ee5253'], ['#48dbfb', '#0abde3'], ['#1dd1a1', '#10ac84'], ['#ff9ff3', '#f368e0']];
function fakeImage(kind, n) {
  const [a, b] = PALETTE[n % PALETTE.length];
  if (kind === 'avatar') return `<svg xmlns="http://www.w3.org/2000/svg" width="120" height="120"><rect width="120" height="120" fill="${a}"/><circle cx="60" cy="48" r="22" fill="#fff" opacity=".85"/></svg>`;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></linearGradient></defs><rect width="1280" height="720" fill="url(#g)"/></svg>`;
}
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.png': 'image/png', '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.json': 'application/json' };
export const server = http.createServer((req, res) => {
  const url = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  const img = url.match(/^\/__img\/(\w+)\/(\d+)\.svg$/);
  if (img) { res.writeHead(200, { 'content-type': 'image/svg+xml' }); res.end(fakeImage(img[1], Number(img[2]))); return; }
  const file = path.join(ROOT, url === '/' ? 'index.html' : url);
  if (!file.startsWith(ROOT) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) { res.writeHead(404); res.end(); return; }
  res.writeHead(200, { 'content-type': TYPES[path.extname(file)] || 'application/octet-stream' });
  fs.createReadStream(file).pipe(res);
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
export const base = `http://127.0.0.1:${server.address().port}/`;

// 모의 IPC — 로그인·데이터 있음. 화면이 쓰는 명령만 채우고 나머지는 실패로 돌려 오류·빈 상태 코드도 지나가게 둔다.
export function mockTauri([origin, theme]) {
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
    channel_detail: () => ({ channel: { ...ch(1), followerCount: 12000, channelDescription: '매일 저녁 방송합니다.\nhttps://example.com/schedule' }, live: null, isFavorite: false }),
    channel_overview: () => ({
      lastLive: { liveTitle: titles[0], categoryName: cats[0], categoryType: 'GAME', tags: ['소통', '게임'], openDate: ago(1800), closeDate: ago(1500), accumulateCount: 4210, adult: false },
      recentVods: vods.slice(0, 4), vodTotal: 37, popularClips: clips.slice(0, 6),
      watch: { totalSeconds: 54000, last30Seconds: 7200, sessionCount: 12, firstWatchedAt: ago(200 * 1440), lastWatchedAt: ago(1440), topCategory: cats[0] },
      follow: { following: true, followDate: ago(400 * 1440) },
    }),
    channel_vods: () => ({ data: vods, totalPages: 1 }),
    channel_clips: () => ({ data: clips, next: null }),
  };
  window.__calls = []; // 부른 명령 기록 — 대기 중 불필요한 요청을 세는 검사가 쓴다
  window.__TAURI__ = {
    core: {
      invoke: async (cmd, args) => {
        // 멈춤 감시 신호(ui-health.js, 5초마다)는 데이터 요청이 아니다 — 대기 중 요청을 세는 검사에 넣지 않는다.
        if (cmd === 'ui_heartbeat') { window.__beats = (window.__beats || 0) + 1; return null; }
        window.__calls.push(cmd); const h = handlers[cmd]; if (!h) throw new Error(`mock: ${cmd}`); return h(args || {});
      },
    },
    event: { listen: async () => () => {} },
  };
  try { localStorage.setItem('cview.onboarded', '1'); localStorage.setItem('cview.theme', theme); } catch { /* 무시 */ }
}

export const go = (page, route, params = {}) => page.evaluate(([r, p]) => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: r, params: p } })), [route, params]);

/** 모의 IPC 로 앱을 열고 시작 화면·환영 안내가 걷힐 때까지 기다린다. */
export async function openApp(browser, { width = 1280, height = 900, theme = 'dark' } = {}) {
  const ctx = await browser.newContext({ viewport: { width, height }, colorScheme: theme });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, theme]);
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
  await page.waitForFunction(() => { const s = document.getElementById('splash'); return !s || !s.isConnected || getComputedStyle(s).display === 'none' || getComputedStyle(s).opacity === '0'; }, null, { timeout: 8000 }).catch(() => {});
  return { ctx, page, errors };
}
