// 기능 정밀 고도화 20차(2026-11-28) 회귀 검사 — 네트워크·라이브 영상 복구.
// 대부분 Rust(재연결·프록시·채팅 토큰) 변경이라 cargo test 가 행동을 검사한다. 여기서는 화면 쪽만 본다.
// 저장소 루트에서: NODE_PATH=<Playwright 경로> node tools/ui-regression/precision-pass-20.mjs
// D01 재연결 중 시청 화면 — fatalError 가 와도 '재연결 중…'을 유지하고, 엔진 오류 원문을 알림으로 띄우지 않는다.
// D02 코드 확인 — 온라인 복귀 플레이어 신호는 잠잠해진 뒤, 재연결 대기 채팅도 바로 다시 연결.
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
const src = (f) => fs.readFileSync(path.resolve('app-ui/js', f), 'utf8');

try {
  await check('D01 재연결 중 시청 화면 — 오류 카드·원문 알림이 끼어들지 않는다', async () => {
    const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 } });
    const page = await ctx.newPage();
    const errors = [];
    page.on('pageerror', (e) => errors.push(String(e)));
    await page.addInitScript(mockTauri, [base, 'dark', 12]);
    await page.goto(base);
    await page.waitForSelector('[data-page]', { state: 'attached' });
    const r = await page.evaluate(async () => {
      const { Viewer } = await import('/js/viewer.js');
      const shown = [];
      const fake = { channelId: 'ch1', phase: null, showOverlay: (t, err) => shown.push([t, Boolean(err)]), player: { setNowPlaying() {} } };
      Viewer.prototype.onPlayerEvent.call(fake, { type: 'phase', phase: 'reconnecting' });
      const before = document.querySelectorAll('.toast').length;
      Viewer.prototype.onPlayerEvent.call(fake, { type: 'error', message: 'networkError: manifestLoadError' });
      const toasts = document.querySelectorAll('.toast').length - before;
      return { phase: fake.phase, shown, toasts };
    }).catch((e) => ({ err: String(e) }));
    assert.ok(!r.err, r.err);
    assert.equal(r.phase, 'reconnecting');
    assert.equal(r.toasts, 0, '엔진 오류 원문이 알림으로 떴다');
    assert.match(src('viewer.js'), /if \(this\.phase === 'reconnecting'\) return;/);
    assert.deepEqual(errors, []);
    await ctx.close();
  });

  await check('D02 코드 확인 — 온라인 복귀·채팅 재연결', async () => {
    const app = src('app.js');
    assert.ok(!/window\.addEventListener\('online', \(\) => \{\n\s+banner\.hidden = true;\n\s+if \(isTauri\) api\.playerNetworkPath\(true\)/.test(app), 'online 직후 바로 보낸다');
    assert.match(app, /onlineTimer = setTimeout\(\(\) => \{\n\s+if \(navigator\.onLine === false\) return;[\s\S]*?api\.playerNetworkPath\(true\)/);
    assert.match(src('chat.js'), /p\.stateName === 'failed' \|\| p\.stateName === 'reconnecting'/);
  });
} finally {
  await browser.close();
  server.close();
}
console.log(`\n${total - failures.length}/${total} 통과${failures.length ? ` — 실패: ${failures.join(', ')}` : ''}`);
process.exit(failures.length ? 1 : 0);
