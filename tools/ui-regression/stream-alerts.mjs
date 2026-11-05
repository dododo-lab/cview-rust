// 영상 위 후원·구독 알림(donations.js StreamAlerts) 크기 회귀 검사 — 알림 카드는 메시지 길이·이모티콘·금액 자릿수,
// 창(단독 시청 무대) 크기, 멀티라이브 패인 크기와 상관없이 같은 픽셀 크기여야 한다. 패인이 카드보다 좁을 때만
// 패인 안으로 줄어들고(높이는 그대로), 설정의 '알림 크기' 배율을 키워도 틀 밖으로 넘치지 않는지 본다.
// 저장소 루트에서: node tools/ui-regression/stream-alerts.mjs   (CVIEW_UI_SHOTS=<폴더> 면 PNG 를 남긴다)
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');

const ROOT = path.resolve('app-ui');
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.woff2': 'font/woff2' };
const EMOJI = 'https://nng-phinf.pstatic.net/glive/emoji/test.png';

// 실제 화면처럼 단독 시청 무대(.player-stage)와 멀티라이브 패인(.ml-pane, 컨테이너 쿼리 기준)을 크기별로 깐다.
const PAGE = `<!doctype html><html data-theme="dark" data-motion="reduced"><head><meta charset="utf-8">
<link rel="stylesheet" href="/css/app.css"><link rel="stylesheet" href="/css/multilive.css">
<style>body { margin: 0; padding: 16px; display: flex; flex-wrap: wrap; gap: 16px; align-items: flex-start; } .player-stage, .ml-pane { flex: none; }</style>
</head><body>
<div class="player-stage" id="stage-big" style="width:1600px;height:900px"></div>
<div class="player-stage" id="stage-small" style="width:720px;height:405px"></div>
<div class="ml-pane" id="pane-mid" style="width:640px;height:360px"></div>
<div class="ml-pane" id="pane-quad" style="width:400px;height:225px"></div>
<div class="ml-pane" id="pane-tiny" style="width:300px;height:200px"></div>
<script type="module">
  import { StreamAlerts } from '/js/donations.js';
  window.__alerts = {};
  for (const el of document.querySelectorAll('.player-stage, .ml-pane')) window.__alerts[el.id] = new StreamAlerts(el);
  window.__ready = true;
</script></body></html>`;

const server = http.createServer((req, res) => {
  const url = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  if (url === '/__alerts.html') { res.writeHead(200, { 'content-type': 'text/html' }); res.end(PAGE); return; }
  const file = path.join(ROOT, url);
  if (!file.startsWith(ROOT) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) { res.writeHead(404); res.end(); return; }
  res.writeHead(200, { 'content-type': TYPES[path.extname(file)] || 'application/octet-stream' });
  fs.createReadStream(file).pipe(res);
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${server.address().port}/`;

const long = '정말 긴 후원 메시지입니다 '.repeat(12);
const CASES = {
  empty: { kind: 'donation', nickname: '도도', donationAmount: 1000, donationType: 'CHAT', content: '' },
  short: { kind: 'donation', nickname: '도도', donationAmount: 1000, donationType: 'CHAT', content: '안녕' },
  long: { kind: 'donation', nickname: '아주아주아주아주긴닉네임을쓰는시청자님'.repeat(2), donationAmount: 1000, donationType: 'CHAT', content: long },
  unbroken: { kind: 'donation', nickname: '도도', donationAmount: 1000, donationType: 'CHAT', content: 'ㅋ'.repeat(300) },
  emoji: { kind: 'donation', nickname: '도도', donationAmount: 5000, donationType: 'CHAT', content: '{:a:}{:a:}{:a:} 이모티콘 {:a:} '.repeat(6), emojis: { a: EMOJI } },
  huge: { kind: 'donation', nickname: '큰손', donationAmount: 123456789, donationType: 'CHAT', content: '초대형 후원' },
  video: { kind: 'donation', nickname: '도도', donationAmount: 60000, donationType: 'VIDEO', content: long },
  mission: { kind: 'donation', nickname: '도도', donationAmount: 20000, donationType: 'MISSION', missionText: '미션 문구 '.repeat(20), content: long },
  subscription: { kind: 'subscription', nickname: '구독자', subscriptionMonths: 24, subscriptionTier: 'TIER 2', content: '' },
  gift: { kind: 'subscription', nickname: '선물왕', giftType: 'SUBSCRIPTION_GIFT', giftCount: 50, content: long },
};

const browser = await chromium.launch();
try {
  const page = await browser.newPage({ viewport: { width: 1800, height: 1500 } });
  // 이모티콘은 실제 네이버 서버 대신 큰(64px) 그림으로 — 줄 높이를 늘리지 않는지 본다.
  await page.route(`${EMOJI}*`, (r) => r.fulfill({ contentType: 'image/svg+xml', body: '<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><rect width="64" height="64" fill="#0f0"/></svg>' }));
  await page.goto(`${base}__alerts.html`);
  await page.waitForFunction(() => window.__ready);

  // 한 틀에 알림 하나를 띄우고 카드 크기(레이아웃 크기·화면 상자)와 틀 상자를 잰다.
  const measure = (frame, name) => page.evaluate(async ({ frame, m }) => {
    const alerts = window.__alerts[frame];
    alerts.clear();
    alerts.push(m);
    const card = alerts.el.lastElementChild;
    await Promise.all([...card.querySelectorAll('img')].map((img) => (img.complete ? null : new Promise((r) => { img.onload = img.onerror = r; }))));
    // 등장 애니메이션(모션 절약이라 0.01ms)이 끝난 뒤 화면 상자를 잰다.
    await new Promise((r) => setTimeout(r, 120));
    const r = card.getBoundingClientRect();
    const f = alerts.el.parentElement.getBoundingClientRect();
    return { w: card.offsetWidth, h: card.offsetHeight, box: { l: r.left, t: r.top, r: r.right, b: r.bottom }, frame: { l: f.left, t: f.top, r: f.right, b: f.bottom } };
  }, { frame, m: CASES[name] });

  const W = 360;
  const H = 98;
  const expectWidth = { 'stage-big': W, 'stage-small': W, 'pane-mid': W, 'pane-quad': W, 'pane-tiny': 300 - 16 };
  for (const frame of Object.keys(expectWidth)) {
    for (const name of Object.keys(CASES)) {
      const got = await measure(frame, name);
      assert.equal(got.w, expectWidth[frame], `${frame}/${name} 너비 ${got.w}`);
      assert.equal(got.h, H, `${frame}/${name} 높이 ${got.h}`);
      assert.ok(got.box.l >= got.frame.l && got.box.r <= got.frame.r, `${frame}/${name} 가로로 틀 밖: ${JSON.stringify(got)}`);
    }
  }

  // 한 번에 여럿(최대 3개)이 쌓여도 카드마다 같은 크기.
  const stack = await page.evaluate((cases) => {
    const a = window.__alerts['pane-mid'];
    a.clear();
    for (const k of ['empty', 'long', 'mission']) a.push(cases[k]);
    return [...a.el.children].map((c) => [c.offsetWidth, c.offsetHeight]);
  }, CASES);
  assert.deepEqual(stack, [[W, H], [W, H], [W, H]], `쌓인 알림 크기 ${JSON.stringify(stack)}`);

  // 위치 맞추기 견본도 같은 크기.
  const ghost = await page.evaluate(() => { const a = window.__alerts['stage-big']; a.clear(); a.preview(); const c = a.el.lastElementChild; const s = [c.offsetWidth, c.offsetHeight]; a.endPreview(); return s; });
  assert.deepEqual(ghost, [W, H], `견본 알림 크기 ${ghost}`);

  // 설정의 '알림 크기' 배율 — 넓은 틀에서는 배율대로 커지고, 좁은 패인에서는 배율을 곱해도 틀 안에 든다.
  await page.evaluate(() => document.documentElement.style.setProperty('--alert-scale', '1.5'));
  const scaledBig = await measure('stage-big', 'long');
  assert.equal(Math.round(scaledBig.box.r - scaledBig.box.l), W * 1.5, `배율 1.5 무대 너비 ${JSON.stringify(scaledBig)}`);
  for (const frame of ['pane-quad', 'pane-tiny']) {
    const got = await measure(frame, 'long');
    assert.ok(got.box.l >= got.frame.l - 0.5 && got.box.r <= got.frame.r + 0.5, `배율 1.5 ${frame} 가로로 틀 밖: ${JSON.stringify(got)}`);
  }
  await page.evaluate(() => document.documentElement.style.removeProperty('--alert-scale'));

  if (process.env.CVIEW_UI_SHOTS) {
    fs.mkdirSync(process.env.CVIEW_UI_SHOTS, { recursive: true });
    await page.evaluate((cases) => {
      const show = { 'stage-big': ['empty', 'long', 'huge'], 'stage-small': ['emoji', 'mission', 'gift'], 'pane-mid': ['video', 'short'], 'pane-quad': ['subscription'], 'pane-tiny': ['long'] };
      for (const [frame, names] of Object.entries(show)) {
        const a = window.__alerts[frame];
        a.clear();
        for (const n of names) a.push(cases[n]);
      }
    }, CASES);
    await page.screenshot({ path: path.join(process.env.CVIEW_UI_SHOTS, 'stream-alerts.png'), fullPage: true });
  }
  console.log('stream-alerts: 모든 크기·메시지에서 알림 크기 같음 — 통과');
} finally {
  await browser.close();
  server.close();
}
