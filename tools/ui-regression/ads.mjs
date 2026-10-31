// 프리롤 광고(ads.js) 회귀 검사 — 모의 IPC 로 광고 계획·VAST 를 주고, 광고 영상은 ffmpeg 로 만든 짧은 WebM 을
// 가짜 https 주소(page.route)로 내준다. 실제 광고 서버는 건드리지 않는다.
// 저장소 루트에서: node tools/ui-regression/ads.mjs   (ffmpeg 필요)
import http from 'node:http';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');

const ROOT = path.resolve('app-ui');
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' };
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'cview-ads-'));
const VIDEO = path.join(tmp, 'ad.webm');
execFileSync('ffmpeg', ['-loglevel', 'error', '-f', 'lavfi', '-i', 'testsrc2=size=640x360:rate=24:duration=30', '-c:v', 'libvpx', '-b:v', '300k', '-an', VIDEO]);

const PAGE = `<!doctype html><html><head><meta charset="utf-8"><link rel="stylesheet" href="/css/app.css"></head>
<body><div id="host" style="position:relative;width:640px;height:360px"></div></body></html>`;

const server = http.createServer((req, res) => {
  const url = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  if (url === '/') { res.writeHead(200, { 'content-type': 'text/html' }); res.end(PAGE); return; }
  const file = path.join(ROOT, url);
  if (!file.startsWith(ROOT) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) { res.writeHead(404); res.end(); return; }
  res.writeHead(200, { 'content-type': TYPES[path.extname(file)] || 'application/octet-stream' });
  fs.createReadStream(file).pipe(res);
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${server.address().port}/`;

const VAST = `<?xml version="1.0"?><VAST version="3.0"><Ad id="1"><InLine><Impression>https://ads.test/imp</Impression>
<Creatives><Creative><Linear><Duration>00:00:30.000</Duration>
<VideoClicks><ClickThrough>https://ads.test/landing</ClickThrough></VideoClicks>
<MediaFiles><MediaFile type="video/webm" delivery="progressive" width="640" height="360">https://ads.test/ad.webm</MediaFile></MediaFiles>
</Linear></Creative></Creatives></InLine></Ad></VAST>`;

function mockTauri([vast]) {
  const calls = [];
  const handlers = {
    ad_preroll: () => ({ slots: [[{ vastXml: vast, tracking: {}, timeoutMs: 8000 }]], cheatKeyUrl: 'https://game.naver.com/profile#cheat_key' }),
    ad_track: () => null,
    open_external: () => null,
  };
  window.__mock = { calls };
  window.__TAURI__ = {
    core: { invoke: async (cmd, args) => { calls.push({ cmd, args }); const h = handlers[cmd]; if (!h) throw new Error(`mock: ${cmd}`); return h(args); } },
    event: { listen: async () => () => {} },
  };
}

const browser = await chromium.launch({ args: ['--autoplay-policy=no-user-gesture-required'] });
const results = [];
const errors = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}

async function openPage(platform = null) {
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  page.setDefaultTimeout(8000);
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.route('https://ads.test/**', (route) => {
    if (route.request().url().endsWith('/ad.webm')) {
      return route.fulfill({ status: 200, contentType: 'video/webm', body: fs.readFileSync(VIDEO) });
    }
    return route.fulfill({ status: 204, body: '' });
  });
  await page.addInitScript(mockTauri, [VAST]);
  await page.goto(base);
  if (platform) await page.evaluate((p) => { document.documentElement.dataset.platform = p; }, platform);
  return page;
}

// 광고 시작 — 끝나면 window.__adResult 에 결과가 남는다.
async function startAd(page, { host = true } = {}) {
  await page.evaluate(async (inHost) => {
    const { playPreroll } = await import('/js/ads.js');
    window.__adResult = undefined;
    const opts = inHost ? { host: document.getElementById('host') } : {};
    playPreroll('live', 'ch1', opts).then((r) => { window.__adResult = r; });
  }, host);
  await page.waitForFunction(() => { const v = document.querySelector('.ad-overlay video'); return v && v.currentTime > 0.3 && !v.paused; });
}
const adState = (page) => page.evaluate(() => {
  const v = document.querySelector('.ad-overlay video');
  const r = document.querySelector('.ad-resume');
  return { exists: Boolean(v), paused: v ? v.paused : null, resumeShown: r ? !r.hidden : false, result: window.__adResult };
});

await check('자세히 보기로 멈춘 광고는 "광고 계속 보기"로 이어 튼다', async () => {
  const page = await openPage();
  await startAd(page);
  await page.click('.ad-more');
  let s = await adState(page);
  assert.equal(s.paused, true);
  assert.equal(s.resumeShown, true, '계속 보기 단추가 보여야 한다');
  await page.click('.ad-resume');
  await page.waitForFunction(() => !document.querySelector('.ad-overlay video').paused);
  s = await adState(page);
  assert.equal(s.resumeShown, false);
  await page.close();
});

await check('자세히 보기 뒤 창으로 돌아오면(focus) 광고가 이어진다', async () => {
  const page = await openPage();
  await startAd(page);
  await page.click('.ad-more');
  assert.equal((await adState(page)).paused, true);
  await page.evaluate(() => window.dispatchEvent(new Event('focus')));
  await page.waitForFunction(() => !document.querySelector('.ad-overlay video').paused);
  await page.close();
});

await check('재생이 멈춰 진행이 없으면 광고를 넘기고 본 재생으로 간다', async () => {
  const page = await openPage();
  await startAd(page);
  // 네트워크 멈춤 흉내 — 재생 중(paused 아님)인데 시간이 가지 않는다.
  await page.evaluate(() => { document.querySelector('.ad-overlay video').playbackRate = 0; });
  await page.waitForFunction(() => window.__adResult !== undefined, null, { timeout: 20000 });
  const s = await adState(page);
  assert.equal(s.exists, false, '광고 화면이 사라져야 한다');
  assert.equal(s.result, true, '광고 실패는 본 재생을 막지 않는다');
  await page.close();
});

await check('Windows: 창 전체 광고 위에도 창 단추가 있고 바탕으로 창을 끌 수 있다', async () => {
  const page = await openPage('windows');
  await page.evaluate(() => {
    const bar = document.createElement('div');
    bar.className = 'win-controls';
    document.body.append(bar);
  });
  await startAd(page, { host: false });
  const z = await page.evaluate(() => ({
    ctl: Number(getComputedStyle(document.querySelector('.win-controls')).zIndex),
    ad: Number(getComputedStyle(document.querySelector('.ad-overlay')).zIndex),
    drag: document.querySelector('.ad-overlay').hasAttribute('data-tauri-drag-region'),
    stageTop: document.querySelector('.ad-stage').getBoundingClientRect().top,
  }));
  assert.ok(z.ctl > z.ad, `창 단추 z ${z.ctl} ≤ 광고 z ${z.ad}`);
  assert.equal(z.drag, true);
  assert.ok(z.stageTop >= 32, `광고 화면이 창 단추 띠 아래여야 한다: ${z.stageTop}`);
  await page.close();
});

await browser.close();
server.close();
for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : ` — ${r.error}`}`);
if (errors.length) console.log(`page errors:\n${errors.join('\n')}`);
const failed = results.filter((r) => !r.ok).length + errors.length;
console.log(failed ? `${failed} failed` : `all ${results.length} passed`);
process.exit(failed ? 1 : 0);
