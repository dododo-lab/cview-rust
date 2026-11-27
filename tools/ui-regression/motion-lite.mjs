// 모션 경량화(2026-11-28) 회귀 검사 — 창이 포커스를 잃으면 무한 반복 표시를 뺀다(motion.js app-away, motion.css).
// 도는 애니메이션이 하나라도 보이면 창 서버가 매 프레임 창을 다시 합성한다(크기 무관 — VM 4K 에서 4×4px 깜박임 하나가
// WindowServer 코어 약 16%). 다른 앱을 쓰는 동안 버퍼링 로고·채팅 로더·녹화 점·스켈레톤이 계속 돌았다.
// 모의 IPC·헤드리스 브라우저(실제 서비스·데이터는 건드리지 않는다). CVIEW_UI_BROWSER=webkit 이면 WebKit.
// 저장소 루트에서: NODE_PATH=<Playwright 경로> node tools/ui-regression/motion-lite.mjs
// L01 포커스가 있으면 무한 표시가 돈다(모양·타이밍 그대로).
// L02 포커스를 잃으면 모두 animation none(멈춤 paused 가 아니라 제거), 돌아오면 다시 돈다.
// L03 불확정 진행 막대는 멈춘 30% 막대가 아니라 가득 찬 옅은 막대로 보인다.
// L04 같은 출처 iframe(멀티라이브 패인 플레이어)으로 포커스가 옮겨도 떠난 것으로 보지 않는다.
// L05 모션 줄이기 규칙과 겹쳐도 그대로(줄이기 상태에서 포커스를 잃고 돌아와도 정지).
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, openApp } from './mock-app.mjs';

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

const { ctx, page, errors } = await openApp(browser, { welcomed: true });

// 무한 표시를 한데 모은 시험 판 — 실제 클래스 그대로.
await page.evaluate(() => {
  const host = document.createElement('div');
  host.id = 'l-host';
  host.style.cssText = 'position:fixed;left:0;top:0;z-index:99999;background:#000;width:400px';
  host.innerHTML = `
    <div class="spinner" id="l-spin"></div>
    <div class="sk" id="l-sk" style="width:80px;height:10px"></div>
    <button class="icon-btn rec-btn recording"><span class="rec-dot" id="l-rec"></span></button>
    <span class="dl-track" style="width:200px"><i class="dl-fill indeterminate" id="l-dl"></i></span>
    <div class="update-progress-bar indeterminate" style="width:200px"><i id="l-up"></i></div>
    <span class="mwi-dots"><i id="l-dot"></i><i></i><i></i></span>
    <div class="ml-addbar-refresh spinning"><svg id="l-ref" viewBox="0 0 10 10"><rect width="10" height="10"/></svg></div>`;
  document.body.append(host);
});
const IDS = ['l-spin', 'l-sk', 'l-rec', 'l-dl', 'l-up', 'l-dot', 'l-ref'];
const anims = () => page.evaluate((ids) => Object.fromEntries([
  ...ids.map((id) => [id, getComputedStyle(document.getElementById(id)).animationName]),
  ['l-spin::before', getComputedStyle(document.getElementById('l-spin'), '::before').animationName],
  ['l-spin::after', getComputedStyle(document.getElementById('l-spin'), '::after').animationName],
]), IDS);
const setFocus = async (on) => {
  await page.evaluate((v) => { document.hasFocus = () => v; window.dispatchEvent(new Event(v ? 'focus' : 'blur')); }, on);
  await page.waitForFunction((v) => document.documentElement.classList.contains('app-away') === !v, on);
};

await check('L01 포커스가 있으면 무한 표시가 돈다', async () => {
  await setFocus(true);
  const a = await anims();
  for (const [k, v] of Object.entries(a)) assert.notEqual(v, 'none', `${k} 가 돌아야 한다`);
});

await check('L02 포커스를 잃으면 모두 뺀다(none), 돌아오면 다시 돈다', async () => {
  await setFocus(false);
  const off = await anims();
  for (const [k, v] of Object.entries(off)) assert.equal(v, 'none', `${k} 는 빠져야 한다`);
  assert.equal(await page.evaluate(() => document.getAnimations().filter((x) => x.effect?.target?.closest?.('#l-host') && x.playState === 'running').length), 0);
  await setFocus(true);
  const on = await anims();
  for (const [k, v] of Object.entries(on)) assert.notEqual(v, 'none', `${k} 가 다시 돌아야 한다`);
});

await check('L03 불확정 막대 — 떠난 동안 가득 찬 옅은 막대', async () => {
  await setFocus(false);
  const r = await page.evaluate(() => ['l-dl', 'l-up'].map((id) => {
    const el = document.getElementById(id);
    return { w: el.getBoundingClientRect().width, pw: el.parentElement.getBoundingClientRect().width, o: Number(getComputedStyle(el).opacity) };
  }));
  for (const x of r) { assert.ok(Math.abs(x.w - x.pw) < 1.5, `막대 폭 ${x.w} ≈ ${x.pw}`); assert.equal(x.o, 0.5); }
  await setFocus(true);
});

await check('L04 같은 출처 iframe 으로 포커스가 옮겨도 떠난 것으로 보지 않는다', async () => {
  await page.evaluate(() => { delete document.hasFocus; });
  await page.evaluate(() => new Promise((res) => {
    const f = document.createElement('iframe');
    f.id = 'l-frame';
    f.style.cssText = 'position:fixed;left:500px;top:0;width:200px;height:100px;z-index:99999';
    f.srcdoc = '<button id="b">b</button>';
    f.onload = res;
    document.body.append(f);
  }));
  await page.bringToFront();
  await page.frameLocator('#l-frame').locator('#b').click();
  await page.waitForTimeout(150);
  const st = await page.evaluate(() => ({ away: document.documentElement.classList.contains('app-away'), has: document.hasFocus(), act: document.activeElement?.id }));
  assert.equal(st.act, 'l-frame', 'iframe 이 초점을 가져야 검사가 의미 있다');
  assert.equal(st.away, !st.has, 'app-away 는 document.hasFocus() 를 따른다');
  if (st.has) assert.equal(st.away, false);
  // 초점이 iframe 안에 있을 때 다른 앱으로 가면 blur 는 iframe 창에만 온다 — 그것도 받아 떠난 것으로 본다.
  await page.evaluate(() => {
    document.hasFocus = () => false;
    document.getElementById('l-frame').contentWindow.dispatchEvent(new Event('blur'));
  });
  await page.waitForFunction(() => document.documentElement.classList.contains('app-away'), null, { timeout: 2000 });
  // 돌아올 때 focus 도 iframe 창에만 온다.
  if (st.has) {
    await page.evaluate(() => {
      document.hasFocus = () => true;
      document.getElementById('l-frame').contentWindow.dispatchEvent(new Event('focus'));
    });
    await page.waitForFunction(() => !document.documentElement.classList.contains('app-away'), null, { timeout: 2000 });
  }
  await page.evaluate(() => { delete document.hasFocus; document.getElementById('l-frame').remove(); });
});

await check('L05 모션 줄이기 — 포커스를 잃고 돌아와도 정지 그대로', async () => {
  await page.evaluate(() => { document.documentElement.dataset.motion = 'reduced'; });
  await setFocus(false);
  await setFocus(true);
  const a = await anims();
  for (const k of ['l-spin', 'l-sk', 'l-rec', 'l-dl', 'l-up', 'l-ref', 'l-spin::before', 'l-spin::after']) assert.equal(a[k], 'none', `${k}`);
  await page.evaluate(() => { delete document.documentElement.dataset.motion; delete document.hasFocus; });
});

assert.deepEqual(errors, []);
await ctx.close();
await browser.close();
server.close();
console.log(`\n${total - failures.length}/${total} 통과${failures.length ? ` — 실패: ${failures.join(', ')}` : ''}`);
process.exit(failures.length ? 1 : 0);
