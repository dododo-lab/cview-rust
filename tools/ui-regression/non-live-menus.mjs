// 비라이브 메뉴 UI 회귀 검사 — docs/NON_LIVE_UI_UX_VERIFICATION_2026-10-27.md 의 F01–F13 이 고쳐진 채인지 본다.
// app-ui 를 그대로 띄우고 window.__TAURI__ 를 모의 IPC 로 바꿔 실제 화면 코드를 돌린다(실제 서비스·데이터는 건드리지 않음).
// 저장소 루트에서: node tools/ui-regression/non-live-menus.mjs   (Playwright 가 필요하다)
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

// 모의 IPC — 명령별 응답은 테스트가 window.__mock.handlers 로 바꾼다. 부른 명령은 window.__mock.calls 에 남는다.
function mockTauri() {
  const calls = [];
  const health = { available: true, storeBytes: 1000, freeBytes: 1e9, backups: [], tables: [{ label: '시청 기록', rows: 3 }], backupDir: '/tmp/b', storePath: '/tmp/s' };
  const handlers = {
    auth_status: () => ({ loggedIn: false, state: 'loggedOut' }),
    settings_get: () => ({ multiChat: {} }),
    data_health: () => health,
    data_cleanup: () => health,
    data_backup_now: () => health,
    insights_overview: () => ({ sessionCount: 0, recent: [], daily: [], channels: [], categories: [], hourly: Array(24).fill(0) }),
    popular_lives: () => [],
    search: () => ({ lives: [], channels: [] }),
    search_videos: () => ({ items: [], hasMore: false }),
  };
  window.__mock = { calls, handlers };
  window.__TAURI__ = {
    core: {
      invoke: async (cmd, args) => {
        calls.push({ cmd, args });
        const h = handlers[cmd];
        if (!h) throw new Error(`mock: ${cmd}`);
        return h(args);
      },
    },
    event: { listen: async () => () => {} },
  };
  try { localStorage.setItem('cview.onboarded', '1'); } catch { /* 무시 */ }
}

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
page.setDefaultTimeout(5000); // 고장 나면 30초씩 기다리지 않고 바로 실패
const errors = [];
page.on('pageerror', (e) => errors.push(String(e)));
await page.addInitScript(mockTauri);
await page.goto(base);
await page.waitForSelector('[data-page]', { state: 'attached' });
await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));

const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}
const go = (route, params = {}) => page.evaluate(([r, p]) => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: r, params: p } })), [route, params]);
const setHandler = (cmd, src) => page.evaluate(([c, s]) => { window.__mock.handlers[c] = (0, eval)(s); }, [cmd, src]);
const calls = (cmd) => page.evaluate((c) => window.__mock.calls.filter((x) => x.cmd === c), cmd);
const settle = () => page.waitForTimeout(120);
// CVIEW_UI_SHOTS=<폴더> 면 주요 장면을 PNG 로 남긴다(눈으로 확인용)
const SHOTS = process.env.CVIEW_UI_SHOTS;
const shot = async (name) => { if (SHOTS) { fs.mkdirSync(SHOTS, { recursive: true }); await page.screenshot({ path: path.join(SHOTS, `${name}.png`) }); } };

// F01 — 정리 확인: 취소하면 지우지 않고, 확인하면 먼저 백업한 뒤 지운다. 백업이 실패하면 지우지 않는다.
await check('F01 stats cleanup confirmation', async () => {
  await go('stats', { tab: 'data' });
  await page.waitForSelector('[data-stats="cleanup"][data-kind="oldHistory"]');
  await page.click('[data-kind="oldHistory"]');
  await page.waitForSelector('.cleanup-confirm [role="alertdialog"]');
  await page.waitForTimeout(400);
  await shot('f01-confirm');
  assert.equal((await calls('data_cleanup')).length, 0, '첫 클릭에 지움');
  assert.equal(await page.evaluate(() => document.activeElement?.matches('[data-cancel]')), true, '초점이 취소에 있지 않음');
  await page.keyboard.press('Escape');
  await settle();
  assert.equal((await calls('data_cleanup')).length, 0, '취소 후 지움');
  await page.click('[data-kind="recent"]');
  await page.click('.cleanup-confirm button[type="submit"]');
  await settle();
  const order = (await page.evaluate(() => window.__mock.calls.map((x) => x.cmd))).filter((c) => c === 'data_backup_now' || c === 'data_cleanup');
  assert.deepEqual(order, ['data_backup_now', 'data_cleanup']);
  await setHandler('data_backup_now', '() => { throw new Error("disk full"); }');
  await page.click('[data-kind="oldHistory"]');
  await page.click('.cleanup-confirm button[type="submit"]');
  await settle();
  assert.equal((await calls('data_cleanup')).length, 1, '백업 실패인데 지움');
  // 캐시 비우기는 확인 없이 바로
  await page.click('[data-kind="liveCache"]');
  await settle();
  assert.equal((await calls('data_cleanup')).length, 2);
});

// F11 — 백업 버튼 글자가 한 줄
await check('F11 backup button single line', async () => {
  await shot('f11-data');
  const h = await page.$eval('[data-stats="backup"]', (b) => ({ h: b.getBoundingClientRect().height, lh: parseFloat(getComputedStyle(b).lineHeight) || 20 }));
  assert.ok(h.h < h.lh * 2 + 4, `버튼 높이 ${h.h}`);
});

// F06 — 탭 선택 상태·패널 연결·방향키
await check('F06 tabs semantics', async () => {
  const sel = await page.$$eval('[data-page="stats"] [role="tab"]', (ts) => ts.map((t) => [t.dataset.tab, t.getAttribute('aria-selected'), t.tabIndex]));
  assert.deepEqual(sel.find((x) => x[1] === 'true'), ['data', 'true', 0]);
  assert.equal(sel.filter((x) => x[1] === 'true').length, 1);
  const panel = await page.$eval('[data-page="stats"] .page-body', (p) => [p.getAttribute('role'), p.getAttribute('aria-labelledby')]);
  assert.deepEqual(panel, ['tabpanel', 'stats-tab-data']);
  await page.focus('[data-page="stats"] [data-tab="data"]');
  await page.keyboard.press('Home');
  await settle();
  assert.equal(await page.evaluate(() => document.activeElement?.dataset.tab), 'overview');
  assert.equal(await page.getAttribute('[data-page="stats"] [data-tab="overview"]', 'aria-selected'), 'true');
  await page.keyboard.press('ArrowLeft');
  await settle();
  assert.equal(await page.evaluate(() => document.activeElement?.dataset.tab), 'data');
  await go('library', { tab: 'recent' });
  await settle();
  assert.equal(await page.getAttribute('[data-page="library"] [data-tab="recent"]', 'aria-selected'), 'true');
});

// F02 — 직접 입력한 검색의 오류 → 다시 시도가 다시 찾는다
await check('F02 search retry', async () => {
  await setHandler('search', '() => { throw new Error("network offline"); }');
  await setHandler('search_videos', '() => { throw new Error("network offline"); }');
  await go('search');
  await page.fill('[data-page="search"] input[type=search]', 'audit');
  await page.press('[data-page="search"] input[type=search]', 'Enter');
  await page.waitForSelector('[data-page="search"] [data-retry]');
  const before = (await calls('search')).length;
  await setHandler('search', '() => ({ lives: [], channels: [{ channelId: "c1", channelName: "채널하나", followerCountText: "1" }] })');
  await setHandler('search_videos', '() => ({ items: [], hasMore: false })');
  await page.click('[data-page="search"] [data-retry]');
  await page.waitForSelector('[data-page="search"] .search-tabs');
  assert.equal((await calls('search')).length, before + 1);
  assert.match(await page.textContent('[data-page="search"] .page-body'), /채널하나/);
});

// F03 — 한쪽 실패는 결과 없음이 아니라 실패 안내 + 그쪽만 다시 받기
await check('F03 partial failure', async () => {
  await setHandler('search_videos', '() => { throw new Error("network offline"); }');
  await page.fill('[data-page="search"] input[type=search]', 'part');
  await page.press('[data-page="search"] input[type=search]', 'Enter');
  await page.waitForSelector('[data-search-retry="videos"]');
  await shot('f03-partial');
  const body = await page.textContent('[data-page="search"] .page-body');
  assert.match(body, /다시보기 검색에 실패했습니다/);
  assert.match(body, /채널하나/);
  const mainCalls = (await calls('search')).length;
  await setHandler('search_videos', '() => ({ items: [{ videoNo: 7, videoTitle: "다시보기칠" }], hasMore: false })');
  await page.click('[data-search-retry="videos"]');
  await page.waitForFunction(() => !document.querySelector('[data-search-retry]'));
  assert.equal((await calls('search')).length, mainCalls, '성공한 쪽까지 다시 받음');
  assert.match(await page.textContent('[data-page="search"] .search-tabs'), /다시보기 1/);
});

// F04 — 다시보기 24개 + hasMore → 더 보기로 다음 구간
await check('F04 vod pagination', async () => {
  await setHandler('search_videos', '({ offset }) => ({ items: Array.from({ length: offset ? 5 : 24 }, (_, i) => ({ videoNo: offset + i, videoTitle: "v" + (offset + i) })), hasMore: !offset })');
  await page.fill('[data-page="search"] input[type=search]', 'many');
  await page.press('[data-page="search"] input[type=search]', 'Enter');
  await page.waitForSelector('[data-page="search"] .search-tabs');
  await page.click('[data-stab="videos"]');
  await shot('f04-more');
  await page.click('[data-vod-more]');
  await page.waitForFunction(() => /다시보기 29/.test(document.querySelector('[data-page="search"] .search-tabs')?.textContent || ''));
  const offs = (await calls('search_videos')).slice(-2).map((c) => c.args.offset);
  assert.deepEqual(offs, [0, 24]);
  assert.equal(await page.$('[data-vod-more]'), null, '마지막 구간인데 더 보기');
});

// F05 — 입력을 비우면 이전 결과가 사라지고, 늦은 응답이 덮지 않는다
await check('F05 clear input', async () => {
  await setHandler('search', '() => new Promise((r) => { window.__late = r; })');
  await page.fill('[data-page="search"] input[type=search]', 'slow');
  await page.press('[data-page="search"] input[type=search]', 'Enter');
  await page.fill('[data-page="search"] input[type=search]', '');
  await settle();
  await page.evaluate(() => window.__late({ lives: [], channels: [{ channelId: 'z', channelName: '늦은결과', followerCountText: '1' }] }));
  await settle();
  const body = await page.textContent('[data-page="search"] .page-body');
  assert.doesNotMatch(body, /늦은결과|다시보기 \d/);
  assert.ok(await page.$('[data-page="search"] .discovery'));
});

// F07 — 고정 별이 초점을 받는 독립 단추, Enter 로 고정해도 상세로 가지 않는다
await check('F07 category pin', async () => {
  await setHandler('popular_lives', '() => [{ channelId: "a", channelName: "A", categoryName: "하스스톤", viewerCount: 10 }, { channelId: "b", channelName: "B", categoryName: "검은사막", viewerCount: 5 }]');
  await go('categories');
  await page.waitForSelector('.category-pin');
  const pin = '.category-pin[data-pin="검은사막"]';
  assert.equal(await page.getAttribute(pin, 'aria-pressed'), 'false');
  await page.focus(pin);
  await page.keyboard.press('Enter');
  await settle();
  assert.equal(await page.getAttribute(pin, 'aria-pressed'), 'true');
  await shot('f07-pin-focus');
  assert.equal(await page.evaluate(() => document.activeElement?.dataset.pin), '검은사막');
  assert.equal(await page.$('[data-cat-back]'), null, '상세로 이동함');
  assert.equal(await page.$eval('.category-item', (x) => x.querySelector('.category-pin').dataset.pin), '검은사막', '고정이 맨 앞으로 오지 않음');
});

// F13 — 인기 정렬 설명
await check('F13 popular subtitle', async () => {
  await go('popular');
  await page.waitForSelector('[data-sort="recent"]');
  await page.click('[data-sort="recent"]');
  assert.equal((await page.textContent('[data-page="popular"] .page-subtitle')).trim(), '최근 시작한 순');
  await page.click('[data-sort="viewers"]');
  assert.equal((await page.textContent('[data-page="popular"] .page-subtitle')).trim(), '시청자 많은 순');
});

// F10 — 설정 검색 0건 안내
await check('F10 settings no result', async () => {
  await go('settings');
  await page.waitForSelector('.settings-search');
  await page.fill('.settings-search', 'zzzz');
  await settle();
  await shot('f10-settings-empty');
  assert.match(await page.textContent('.settings-search-empty'), /"zzzz"와 맞는 설정이 없습니다/);
  await page.click('[data-settings-clear]');
  await settle();
  assert.equal(await page.$eval('.settings-search-empty', (x) => x.hidden), true);
  assert.equal(await page.inputValue('.settings-search'), '');
});

// F08·F09·F12 — 멀티채팅(비로그인): 통합 입력이 꺼지고, 검색 0건은 결과 없음으로, 좁은 칸은 도구를 접는다.
// 앱의 화면 객체는 밖에서 닿지 않아, 같은 모듈로 시험용 화면을 하나 더 만들어 채널 2개를 붙인다.
await check('F08 F09 F12 multichat', async () => {
  await setHandler('live_detail', '({ channelId }) => ({ channelId, channelName: channelId === "m" ? "마뫄" : "다주", chatChannelId: "cc" + channelId, isLive: true })');
  await setHandler('chat_connect', '() => ({ canSend: true })');
  await setHandler('settings_update', '() => null');
  await page.evaluate(async () => {
    document.querySelectorAll('[data-page]').forEach((x) => { x.hidden = true; });
    const box = document.createElement('div');
    box.id = 'mc-test';
    box.style.cssText = 'position:fixed;inset:0;width:900px;height:700px;z-index:99;background:#111;display:flex';
    document.body.append(box);
    const { MultiChatPage } = await import('./js/multichat.js');
    const mc = new MultiChatPage(box, { isLoggedIn: () => false });
    mc.loaded = true;
    mc.root.hidden = false;
    await mc.add('m', false);
    await mc.add('d', false);
    mc.layout = 'grid';
    mc.render();
    window.__mc = mc;
  });
  await settle();
  // F12 — 좁은 칸: 드문 도구는 접히고 '⋯' 가 보인다. 채널명은 줄지 않는다
  const head = await page.$eval('#mc-test .mc-pane', (p) => ({
    extraShown: [...p.querySelectorAll('.chat-tool-extra')].some((b) => getComputedStyle(b).display !== 'none'),
    more: getComputedStyle(p.querySelector('.mc-tools-more')).display,
    nameClipped: p.querySelector('.mc-name').scrollWidth > p.querySelector('.mc-name').clientWidth,
    stateLines: Math.round(p.querySelector('.chat-state').getBoundingClientRect().height / (parseFloat(getComputedStyle(p.querySelector('.chat-state')).lineHeight) || 14)),
  }));
  assert.equal(head.extraShown, false, '좁은 칸에서 도구가 그대로');
  assert.notEqual(head.more, 'none');
  assert.equal(head.nameClipped, false, '짧은 채널명이 잘림');
  assert.equal(head.stateLines, 1, '연결 상태가 두 줄');
  await shot('f12-grid');
  await page.click('#mc-test .mc-pane .mc-tools-more');
  await shot('f12-grid-tools-open');
  assert.equal(await page.$eval('#mc-test .mc-pane', (p) => getComputedStyle(p.querySelector('.chat-tool-extra')).display !== 'none'), true, '펼쳐도 안 보임');
  // F08 — 통합: 비로그인이면 입력·보내기가 꺼지고 로그인 안내
  await page.evaluate(() => { const mc = window.__mc; mc.layout = 'merged'; mc.mergedList.innerHTML = '<div class="empty">새 메시지를 기다리는 중…</div>'; mc.render(); });
  const merged = await page.$eval('#mc-test .mc-merged', (m) => ({
    input: m.querySelector('.mc-merged-input input').disabled,
    send: m.querySelector('.mc-merged-input button').disabled,
    banner: m.querySelector('.mc-merged-banner').hidden ? '' : m.querySelector('.mc-merged-banner').textContent,
  }));
  assert.equal(merged.input, true);
  assert.equal(merged.send, true);
  assert.match(merged.banner, /로그인하면/);
  // F09 — 첫 메시지로 대기 안내가 걷히고, 검색 0건은 결과 없음으로
  await page.evaluate(() => {
    const mc = window.__mc;
    mc.onSessionEvent({ session: 'mc:m', event: { type: 'messages', messages: [{ kind: 'chat', nickname: 'n', content: '안녕', emojis: {} }] } });
  });
  assert.equal(await page.$('#mc-test .mc-merged-list > .empty'), null, '대기 안내가 남음');
  await page.fill('#mc-test .mc-search', 'cview-no-message-match');
  await settle();
  await shot('f09-merged-no-match');
  assert.match(await page.textContent('#mc-test .mc-search-note'), /일치하는 메시지가 없습니다/);
  await page.fill('#mc-test .mc-search', '안녕');
  await settle();
  assert.match(await page.textContent('#mc-test .mc-search-note'), /일치 1개/);
  await page.click('#mc-test [data-mc="clear-search"]');
  assert.equal(await page.$eval('#mc-test .mc-search-note', (x) => x.hidden), true);
});

console.log(JSON.stringify({ results, pageErrors: errors }, null, 2));
await browser.close();
server.close();
const failed = results.filter((r) => !r.ok);
if (failed.length || errors.length) process.exit(1);
