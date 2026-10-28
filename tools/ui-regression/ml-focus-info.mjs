// 멀티라이브 포커스(메인 왼쪽·오른쪽) — 메인 영상이 16:9 높이로 줄고 아래 남는 칸에 채널 정보가 보이는지 본다.
// app-ui 를 그대로 띄우고 window.__TAURI__ 를 모의 IPC 로 바꿔 실제 화면 코드를 돌린다(실제 서비스·데이터는 건드리지 않음).
// 저장소 루트에서: node tools/ui-regression/ml-focus-info.mjs   (Playwright 가 필요하다)
// CVIEW_UI_SHOTS=<폴더> 면 장면을 PNG 로 남긴다. CVIEW_UI_ROOT=<app-ui 폴더> 로 다른 판(예: 고치기 전)을 띄워 찍을 수 있다.
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');

const ROOT = path.resolve(process.env.CVIEW_UI_ROOT || 'app-ui');
const CHECK = !process.env.CVIEW_UI_ROOT; // 다른 판을 찍을 때는 검사 없이 사진만
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

function mockTauri() {
  const names = [['c1', '한동숙', '오늘은 신작 게임 정주행 — 엔딩까지 달립니다', 'Talk', '1.2만'], ['c2', '침착맨', '삼국지 같이 보기', '종합 게임', '3.4만'], ['c3', '풍월량', '저챗', 'Just Chatting', '8,120']];
  const session = ([id, name, title, , viewers], i) => ({
    id: `s${i}`, slot: i, channelId: id, channelName: name, liveTitle: title, profileImageUrl: null,
    viewerCountText: viewers, accumulateCount: 0, accumulateCountText: '', openDate: new Date(Date.now() - 5400e3).toISOString(),
    isMuted: i > 0, isAudible: i === 0, isAudioOnly: false, state: { kind: 'playing' }, chatChannelId: null, maxHeightCap: 0, decodeSuspended: false,
  });
  const state = {
    sessions: names.map(session), selectedId: 's0', focusMainId: 's0', canAdd: true, maxSessions: 4, layout: 'focus',
    focusDirection: 'left', focusRatio: 0.7, horizontalRatio: 0.5, verticalRatio: 0.5, multiAudio: false, watchId: null, rev: 1,
  };
  const detail = (id) => {
    const n = names.find((x) => x[0] === id) || names[0];
    return {
      channel: { channelId: id, channelName: n[1], channelImageUrl: null, verifiedMark: true, followerCount: 523000, channelDescription: '매일 저녁 8시 방송합니다.\n문의: example@example.com' },
      live: { liveTitle: n[2], categoryName: n[3], viewerCount: 12000, viewerCountText: n[4], accumulateCount: 48210, openDate: new Date(Date.now() - 5400e3).toISOString(), tags: ['게임', '소통', '한국어'] },
    };
  };
  const handlers = {
    auth_status: () => ({ loggedIn: false, state: 'loggedOut' }),
    settings_get: () => ({ multiLive: {}, multiChat: {} }),
    multilive_state: () => state,
    multilive_set_focus_layout: ({ direction, ratio }) => { if (direction) state.focusDirection = direction; if (ratio != null) state.focusRatio = ratio; state.rev++; return state; },
    multilive_set_layout: ({ layout }) => { state.layout = layout; state.rev++; return state; },
    channel_detail: ({ channelId }) => detail(channelId),
    channel_info: () => ({ verified: true, followerCount: 523000 }),
    live_detail: ({ channelId }) => ({ ...detail(channelId).live, verified: true }),
    popular_lives: () => [],
    following_lives: () => [],
    multilive_stage_size: () => null,
  };
  window.__mock = { state, handlers };
  window.__TAURI__ = {
    core: { invoke: async (cmd, args) => { const h = handlers[cmd]; if (!h) throw new Error(`mock: ${cmd}`); return h(args || {}); } },
    event: { listen: async () => () => {} },
  };
  try { localStorage.setItem('cview.onboarded', '1'); } catch { /* 무시 */ }
}

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
page.setDefaultTimeout(5000);
const errors = [];
page.on('pageerror', (e) => errors.push(String(e.stack || e)));
await page.addInitScript(mockTauri);
await page.goto(base);
await page.waitForSelector('[data-page]', { state: 'attached' });
await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: 'multilive', params: {} } })));
await page.waitForSelector('.ml-stage.ml-focus');

const SHOTS = process.env.CVIEW_UI_SHOTS;
const settle = () => page.waitForTimeout(450);
// 가짜 영상 — 패인 안에 16:9 그림을 object-fit: contain 처럼 가운데 놓아 검은 띠(레터박스)가 보이게 한다.
const shot = async (name) => {
  if (!SHOTS) return;
  await page.evaluate(() => {
    for (const p of document.querySelectorAll('.ml-pane:not([hidden])')) {
      let v = p.querySelector('.fake-video');
      if (!v) { v = document.createElement('div'); v.className = 'fake-video'; p.prepend(v); }
      const r = p.getBoundingClientRect();
      const w = Math.min(r.width, (r.height * 16) / 9);
      const h = (w * 9) / 16;
      Object.assign(v.style, { position: 'absolute', left: `${(r.width - w) / 2}px`, top: `${(r.height - h) / 2}px`, width: `${w}px`, height: `${h}px`, background: `linear-gradient(135deg, hsl(${(Number(p.dataset.slot) || 0) * 110 + 200} 60% 45%), hsl(${(Number(p.dataset.slot) || 0) * 110 + 250} 55% 25%))`, pointerEvents: 'none', zIndex: '1' });
    }
  });
  fs.mkdirSync(SHOTS, { recursive: true });
  await page.screenshot({ path: path.join(SHOTS, `${name}.png`) });
};
const setDir = async (dir) => { await page.evaluate((d) => document.querySelector(`[data-direction="${d}"]`).click(), dir); await settle(); };
const measure = () => page.evaluate(() => {
  const stage = document.querySelector('.ml-stage');
  const main = document.querySelector('.ml-pane.ml-first');
  const info = document.querySelector('.ml-focus-info');
  const box = (el) => { const r = el.getBoundingClientRect(); return { x: r.x, y: r.y, w: r.width, h: r.height }; };
  return {
    hasInfo: stage.classList.contains('has-info'), stage: box(stage), main: box(main),
    info: info.hidden || getComputedStyle(info).display === 'none' ? null : box(info),
    name: info.querySelector('.mwi-name h2')?.textContent || '',
  };
});

const results = [];
async function check(id, fn) {
  if (!CHECK) { await fn().catch(() => {}); return; }
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}

for (const dir of ['left', 'right']) {
  await check(`${dir}: 메인 16:9 + 아래 채널 정보`, async () => {
    await setDir(dir);
    await shot(`focus-${dir}-1280x800`);
    if (!CHECK) return;
    const m = await measure();
    assert.ok(m.hasInfo, 'has-info 가 없다');
    assert.ok(Math.abs(m.main.h - (m.main.w * 9) / 16) <= 2, `메인이 16:9 가 아니다 ${m.main.w}x${m.main.h}`);
    assert.ok(m.info, '정보 칸이 안 보인다');
    assert.ok(Math.abs(m.info.x - m.main.x) <= 1 && Math.abs(m.info.w - m.main.w) <= 1, '정보 칸이 메인 열에 없다');
    assert.ok(m.info.y >= m.main.y + m.main.h, '정보 칸이 메인과 겹친다');
    assert.ok(Math.abs(m.info.y + m.info.h - (m.stage.y + m.stage.h)) <= 1, '정보 칸이 무대 아래까지 닿지 않는다');
    assert.equal(m.name, '한동숙');
    if (dir === 'right') assert.ok(Math.abs(m.main.x + m.main.w - (m.stage.x + m.stage.w)) <= 1, '오른쪽인데 메인이 오른쪽 끝에 없다');
  });
}

await check('top: 정보 칸 없음(예전 배치)', async () => {
  await setDir('top');
  await shot('focus-top-1280x800');
  if (!CHECK) return;
  const m = await measure();
  assert.ok(!m.hasInfo && !m.info, '위/아래 배치에 정보 칸이 나왔다');
  await setDir('left');
});

await check('낮은 창: 남는 칸이 작으면 정보 칸 없이 메인이 칸을 다 쓴다', async () => {
  await setDir('left');
  await page.setViewportSize({ width: 1280, height: 420 });
  await settle();
  await shot('focus-left-1280x420');
  if (!CHECK) return;
  const m = await measure();
  assert.ok(!m.hasInfo && !m.info, '낮은 창인데 정보 칸이 나왔다');
  assert.ok(Math.abs(m.main.h - m.stage.h) <= 1, '메인이 무대 높이를 다 쓰지 않는다');
});

await check('창을 키우면 다시 정보 칸', async () => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await settle();
  await shot('focus-left-1440x900');
  if (!CHECK) return;
  const m = await measure();
  assert.ok(m.hasInfo && m.info, '창을 키웠는데 정보 칸이 없다');
});

await check('비율 0.85 로 넓히면 메인 영상이 커지고 정보 칸이 줄어든다', async () => {
  const before = CHECK ? await measure() : null;
  await page.evaluate(() => window.__mock.handlers.multilive_set_focus_layout({ direction: null, ratio: 0.85 }));
  await page.evaluate(async () => { const s = await window.__TAURI__.core.invoke('multilive_state'); window.dispatchEvent(new Event('resize')); return s; });
  await page.evaluate(() => { const r = document.querySelector('.ml-ratio input'); r.value = '85'; r.dispatchEvent(new Event('input')); });
  await settle();
  await shot('focus-left-1440x900-ratio85');
  if (!CHECK) return;
  const m = await measure();
  assert.ok(m.main.h > before.main.h, '메인이 커지지 않았다');
  assert.ok(!m.info || m.info.h < before.info.h, '정보 칸이 줄지 않았다');
  assert.ok(Math.abs(m.main.h - (m.main.w * 9) / 16) <= 2 || Math.abs(m.main.h - m.stage.h) <= 1, '메인 비율이 틀렸다');
});

await browser.close();
server.close();
if (CHECK) {
  for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : ` — ${r.error}`}`);
  if (errors.length) console.log(`page errors:\n${errors.join('\n')}`);
  const failed = results.filter((r) => !r.ok).length + errors.length;
  console.log(failed ? `${failed} failed` : `all ${results.length} passed`);
  process.exit(failed ? 1 : 0);
}
