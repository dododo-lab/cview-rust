// 기능 정밀 고도화 21차(2026-11-28) 회귀 검사 — 라이브 영상: 일시정지·잠자기 복귀·방송 재시작·거듭 열기.
// 재연결·지연 제어·대역폭 분배는 Rust(cargo test)가 행동을 검사한다. 여기서는 화면 쪽만 본다.
// 저장소 루트에서: NODE_PATH=<Playwright 경로> node tools/ui-regression/precision-pass-21.mjs
// E01 같은 채널을 거듭 열면(더블클릭) 한 번만 연다 — 두 번째 열기가 첫 열기를 기다린다.
// E02 코드 확인 — 시청 화면 일시정지를 코디네이터에 알림, 잠자기 복귀 점검, 방송 종료 화면 다시 켜짐 이어 보기,
//     가린 창 배경 재생 멈춤 감시.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { base, server, mockTauri } from './mock-app.mjs';

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
const read = (f) => fs.readFileSync(path.resolve(f), 'utf8');
const src = (f) => read(`app-ui/js/${f}`);

function harness() {
  const inv = window.__TAURI__.core.invoke;
  window.__adds = 0;
  window.__TAURI__.core.invoke = async (c, a) => {
    if (c === 'multilive_add') { window.__adds += 1; await new Promise((r) => setTimeout(r, 300)); }
    return inv(c, a);
  };
}

try {
  await check('E01 같은 채널을 거듭 열면 한 번만 연다', async () => {
    const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 } });
    const page = await ctx.newPage();
    const errors = [];
    page.on('pageerror', (e) => errors.push(String(e)));
    await page.addInitScript(mockTauri, [base, 'dark', 12]);
    await page.addInitScript(harness);
    await page.goto(base);
    await page.waitForSelector('[data-page]', { state: 'attached' });
    await page.waitForSelector('[data-page="home"]:not([hidden]) .card-open');
    await page.waitForTimeout(500);
    // 카드를 빠르게 두 번 누른다(더블클릭) — 클릭 이벤트가 둘 간다.
    await page.dblclick('[data-page="home"] .live-card .card-open, [data-page="home"] .card-open');
    await page.waitForTimeout(1500);
    const adds = await page.evaluate(() => window.__adds);
    assert.ok(adds <= 1, `같은 채널 추가를 ${adds}번 보냈다`);
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('E02 코드 확인 — 일시정지·잠자기·방송 재시작·배경 멈춤 감시', async () => {
    assert.match(src('app.js'), /const inflight = openingChannels\.get\(channelId\);/);
    assert.match(src('viewer-controls.js'), /api\.playerSetPaused\(pausing\)/);
    assert.match(src('app.js'), /if \(gap < WAKE_GAP_MS\) return;[\s\S]*?api\.playerSystemWake\(\)/);
    assert.match(src('viewer.js'), /watchForRestart\(\) \{[\s\S]*?this\.open\(id, this\.lastSettings\)/);
    assert.match(read('app-ui/player/player.html'), /const hiddenWatch = document\.hidden && bgTimersActive\(\) && video\.readyState <= 2;/);
  });
} finally {
  await browser.close();
  server.close();
}
console.log(`\n${total - failures.length}/${total} 통과${failures.length ? ` — 실패: ${failures.join(', ')}` : ''}`);
process.exit(failures.length ? 1 : 0);
