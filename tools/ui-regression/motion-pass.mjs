// 애니메이션·모션 정밀 고도화(2026-11-27) 회귀 검사 — 모션 줄이기, 끝 상태, 다시 그릴 때 되풀이, FLIP 이어짐.
// 모의 IPC·헤드리스 브라우저(실제 서비스·데이터는 건드리지 않는다). CVIEW_UI_BROWSER=webkit 이면 WebKit.
// 저장소 루트에서: NODE_PATH=<Playwright 경로> node tools/ui-regression/motion-pass.mjs
// M01 시스템 모션 줄이기 — 미리보기 받는 중 빛줄기(무한 반복)가 0.01ms 로 돌며 깜빡이지 않고 멈춘다.
// M02 홈 히어로 자동 넘김 — 키보드 초점이 히어로 안에 있으면 진행 막대가 멈춘다(호버와 같게).
// M03 사용률 그래프 — 초점이 있는 동안 표본이 와도 이름(aria-label)을 바꾸지 않는다.
// M04 다운로드 불확정 막대 — 모션 줄이기면 흐르지 않는 30% 막대 대신 가득 찬 옅은 막대.
// M05 메뉴 막대 팝오버 — 새로고침 아이콘 회전이 모션 줄이기·'절약'에서 멈추고, 팝오버가 '절약' 등급을 따른다.
// M06 환영 안내 — 단계를 넘길 때 카드 등장 애니메이션을 다시 돌리지 않고, 양 끝 화살표는 다시 그리지 않는다.
// M07 멀티라이브 FLIP — 이동 중 배치를 또 바꾸면 패인이 지금 보이는 자리에서 이어 움직인다(옛 목적지로 튀지 않음).
// M08 멀티라이브 닫힘 — 격자 칸이 지정된 패인도 마지막 자리에서 사라진다(칸 기준으로 옆에 그려지지 않음).
// M09 새로고침 덮개 — 최소 표시 시간을 기다리는 중 화면을 옮기면 바로 걷힌다.
// M10 시작 화면 — 저장된 '절약' 등급이 첫 페인트 전에 걸린다.
// M11 채팅 줄 — 한 번 나타난 이벤트·멘션 줄과 6단계 흐름은 목록을 다시 보여도 다시 돌지 않는다.
// M12 통합 채팅(멀티채팅·멀티라이브) — 올려 읽는 중 앞줄을 지우면 보던 줄을 제자리에 둔다. 채널 정보 탭 스크롤은 모션 줄이기를 따른다.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { base, server, mockTauri, openApp } from './mock-app.mjs';

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
const src = (f) => fs.readFileSync(path.resolve('app-ui', f), 'utf8');

const { ctx, page, errors } = await openApp(browser, { welcomed: true });

await check('M01 시스템 모션 줄이기 — 미리보기 빛줄기는 멈춘다', async () => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  const r = await page.evaluate(() => {
    const host = document.createElement('div');
    host.innerHTML = '<div class="card"><div class="thumb preview-loading" style="position:relative;width:200px;height:100px"></div></div><div class="ml-peek-thumb preview-loading" style="position:relative;width:200px;height:100px"></div>';
    document.body.append(host);
    const out = [...host.querySelectorAll('.preview-loading')].map((x) => getComputedStyle(x, '::after').animationName);
    host.remove();
    return out;
  });
  await page.emulateMedia({ reducedMotion: 'no-preference' });
  assert.deepEqual(r, ['none', 'none'], `무한 반복이 남았다: ${r}`);
  const normal = await page.evaluate(() => {
    const host = document.createElement('div');
    host.innerHTML = '<div class="card"><div class="thumb preview-loading" style="position:relative;width:200px;height:100px"></div></div>';
    document.body.append(host);
    const n = getComputedStyle(host.querySelector('.preview-loading'), '::after').animationName;
    host.remove();
    return n;
  });
  assert.equal(normal, 'thumb-preview-load', '평소에는 흐른다');
});

await check('M02 홈 히어로 — 키보드 초점이 있으면 자동 넘김 진행 막대가 멈춘다', async () => {
  const r = await page.evaluate(() => {
    const host = document.createElement('div');
    host.innerHTML = '<div class="hero"><button class="card-open">열기</button><button class="queue-item active"><span class="queue-progress"></span></button></div><button class="outside">밖</button>';
    document.body.append(host);
    const bar = host.querySelector('.queue-progress');
    host.querySelector('.outside').focus();
    const before = getComputedStyle(bar).animationPlayState;
    host.querySelector('.card-open').focus();
    const inCard = getComputedStyle(bar).animationPlayState;
    host.querySelector('.queue-item').focus();
    const inTab = getComputedStyle(bar).animationPlayState;
    host.remove();
    return { before, inCard, inTab };
  });
  assert.deepEqual(r, { before: 'running', inCard: 'paused', inTab: 'paused' });
});

await check('M03 사용률 그래프 — 초점이 있는 동안 이름이 매초 바뀌지 않는다', async () => {
  const r = await page.evaluate(async () => {
    const { UsageChart, METRICS } = await import('/js/usage-monitor.js');
    const host = document.createElement('div');
    host.style.width = '400px';
    document.body.append(host);
    const chart = new UsageChart(host);
    const now = Date.now();
    const sample = (t, v) => ({ t, cpu: v, cpuPercent: v, memBytes: 1e8, memoryBytes: 1e8, gpu: v, net: 0 });
    const m = (METRICS || [])[0];
    chart.set([sample(now - 2000, 10), sample(now - 1000, 20)], m);
    const first = chart.plot.getAttribute('aria-label');
    chart.plot.focus();
    chart.set([sample(now - 2000, 10), sample(now - 1000, 20), sample(now, 90)], m);
    const focused = chart.plot.getAttribute('aria-label');
    chart.plot.blur();
    chart.set([sample(now - 1000, 20), sample(now, 90), sample(now + 1000, 95)], m);
    const after = chart.plot.getAttribute('aria-label');
    host.remove();
    return { first, focused, after };
  });
  assert.ok(r.first, '이름이 있다');
  assert.equal(r.focused, r.first, '초점 중에는 그대로');
  assert.notEqual(r.after, r.first, '초점이 떠나면 다시 갱신된다');
});

await check('M04 다운로드 불확정 막대 — 모션 줄이기면 가득 찬 옅은 막대', async () => {
  const read = () => page.evaluate(() => {
    const host = document.createElement('div');
    host.innerHTML = '<div class="dl-bar" style="width:200px;height:4px;position:relative"><div class="dl-fill indeterminate"></div></div>';
    document.body.append(host);
    const f = host.querySelector('.dl-fill');
    const out = { w: f.getBoundingClientRect().width, op: getComputedStyle(f).opacity, anim: getComputedStyle(f).animationName };
    host.remove();
    return out;
  });
  await page.emulateMedia({ reducedMotion: 'reduce' });
  const reduced = await read();
  await page.emulateMedia({ reducedMotion: 'no-preference' });
  await page.evaluate(() => { document.documentElement.dataset.motion = 'reduced'; });
  const setting = await read();
  await page.evaluate(() => { document.documentElement.dataset.motion = 'balanced'; });
  const normal = await read();
  for (const [k, v] of Object.entries({ reduced, setting })) {
    assert.equal(v.anim, 'none', `${k}: 애니메이션 없음`);
    assert.ok(v.w >= 199, `${k}: 가득 찬 막대여야 한다(${v.w}px)`);
    assert.equal(v.op, '0.5', `${k}: 옅게`);
  }
  assert.ok(normal.w < 100 && normal.anim !== 'none', `평소에는 흐르는 짧은 막대(${normal.w}px, ${normal.anim})`);
});

await check('M06 환영 안내 — 단계를 넘겨도 카드가 다시 떠오르지 않는다', async () => {
  const r = await page.evaluate(async () => {
    const { showWelcome } = await import('/js/onboarding.js');
    showWelcome({ force: true });
    const scrim = document.querySelector('.welcome-scrim');
    const firstAnim = getComputedStyle(scrim.querySelector('.sheet')).animationName;
    scrim.querySelector('[data-w=next]').click();
    const secondAnim = getComputedStyle(scrim.querySelector('.sheet')).animationName;
    // 첫 장으로 돌아가 ← 를 한 번 더 — 같은 장이면 다시 그리지 않는다
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true }));
    const sheet = scrim.querySelector('.sheet');
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true }));
    const same = scrim.querySelector('.sheet') === sheet;
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    return { firstAnim, secondAnim, same };
  });
  assert.equal(r.firstAnim, 'mo-rise', '처음 열 때는 떠오른다');
  assert.equal(r.secondAnim, 'none', '다음 장에서는 다시 돌지 않는다');
  assert.ok(r.same, '첫 장에서 ← 는 카드를 다시 만들지 않는다');
  await page.waitForFunction(() => !document.querySelector('.welcome-scrim'));
});

// 멀티라이브 FLIP — 두 칸 격자에서 자리를 맞바꾸고, 이동 중에 다시 맞바꾼다.
await check('M07 멀티라이브 — 이동 중 다시 바꾸면 보이는 자리에서 이어 움직인다', async () => {
  const r = await page.evaluate(async () => {
    const { snapshotPanes, playLayoutMotion } = await import('/js/ml-motion.js');
    const stage = document.createElement('div');
    stage.style.cssText = 'position:fixed;left:0;top:0;width:1000px;height:300px;display:grid;grid-template-columns:3fr 1fr;z-index:99999';
    const mk = (i) => { const el = document.createElement('div'); el.className = 'ml-pane'; el.style.cssText = 'background:#888;min-width:0'; stage.append(el); return el; };
    const panes = new Map([[0, { el: mk(0) }], [1, { el: mk(1) }]]);
    document.body.append(stage);
    const order = (a, b) => { panes.get(0).el.style.order = String(a); panes.get(1).el.style.order = String(b); };
    order(0, 1);
    let before = snapshotPanes(panes);
    order(1, 0);
    playLayoutMotion(panes, before);
    await new Promise((res) => setTimeout(res, 90));
    before = snapshotPanes(panes); // 지금 보이는 자리
    order(0, 1);
    playLayoutMotion(panes, before);
    const now = snapshotPanes(panes);
    const out = [];
    for (const [slot, a] of before) {
      const b = now.get(slot);
      // 균등 배율이라 크기는 칸 비율에 맞춰 달라질 수 있다 — 가운데가 그대로여야 한다.
      out.push({ dx: Math.abs((a.left + a.width / 2) - (b.left + b.width / 2)), dy: Math.abs((a.top + a.height / 2) - (b.top + b.height / 2)) });
    }
    for (const p of panes.values()) p.anim?.cancel();
    stage.remove();
    return out;
  });
  for (const { dx, dy } of r) assert.ok(dx < 3 && dy < 3, `가운데가 (${dx.toFixed(1)}, ${dy.toFixed(1)})px 튀었다`);
});

await check('M08 멀티라이브 — 격자 칸이 지정된 패인도 제자리에서 사라진다', async () => {
  const r = await page.evaluate(async () => {
    const { playExit } = await import('/js/ml-motion.js');
    const stage = document.createElement('div');
    stage.style.cssText = 'position:fixed;left:0;top:0;width:1000px;height:300px;display:grid;grid-template-columns:700px 300px;grid-template-rows:150px 150px;z-index:99999';
    const main = document.createElement('div'); main.style.cssText = 'grid-row:1 / 3;grid-column:1';
    const sub = document.createElement('div'); sub.style.gridRow = '2'; sub.style.gridColumn = '2';
    stage.append(main, sub);
    document.body.append(stage);
    const rect = sub.getBoundingClientRect();
    const pane = { el: sub };
    let done = false;
    playExit(pane, stage, rect, () => { done = true; });
    const now = sub.getBoundingClientRect();
    pane.anim?.cancel();
    stage.remove();
    return { left: rect.left, top: rect.top, nowLeft: now.left, nowTop: now.top, done };
  });
  assert.ok(Math.abs(r.nowLeft - r.left) < 2 && Math.abs(r.nowTop - r.top) < 2, `제자리(${r.left},${r.top})가 아니라 (${r.nowLeft},${r.nowTop})에 그려졌다`);
});

await check('M09 새로고침 덮개 — 최소 표시를 기다리는 중 화면을 옮기면 바로 걷힌다', async () => {
  const r = await page.evaluate(async () => {
    const { withRefreshVeil, cancelRefreshVeil } = await import('/js/refresh-veil.js');
    const root = document.querySelector('.page:not([hidden])') || document.querySelector('[data-page]');
    const run = withRefreshVeil(root, () => new Promise((res) => setTimeout(res, 230)));
    await run; // 160ms 에 떴고 230ms 에 끝났다 — 걷기는 480ms 까지 기다린다
    const veil = document.querySelector('.refresh-veil');
    const shownAfterFinish = veil && !veil.hidden;
    cancelRefreshVeil(); // 다른 화면으로 이동
    return { shownAfterFinish, hiddenNow: !veil || veil.hidden, busy: root.getAttribute('aria-busy') };
  });
  assert.ok(r.shownAfterFinish, '최소 표시 시간 동안은 보인다(검사 전제)');
  assert.ok(r.hiddenNow, '화면을 옮기면 바로 걷혀야 한다');
  assert.equal(r.busy, null, 'aria-busy 도 걷힌다');
});

await check('M11 채팅 줄 — 목록을 다시 보여도 등장·6단계 흐름이 다시 돌지 않는다', async () => {
  const r = await page.evaluate(async () => {
    const { chatFollower } = await import('/js/chat-follow.js');
    const list = document.createElement('div');
    list.className = 'chat-list';
    list.style.cssText = 'position:fixed;left:0;top:0;width:300px;height:300px;overflow:auto;z-index:99999';
    document.body.append(list);
    chatFollower(list);
    list.innerHTML = '<div class="chat-line mention">멘션</div><div class="chat-line ev ev-lv-6"><div class="ev-card" style="position:relative"><span class="ev-value">10만</span></div></div>';
    const holo = (x) => getComputedStyle(x.querySelector('.ev-card'), '::after').animationName;
    const first = { mention: getComputedStyle(list.firstElementChild).animationName, card: getComputedStyle(list.querySelector('.ev-card')).animationName, holo: holo(list) };
    // 시간을 앞당겨 애니메이션을 끝낸다(9.6초를 기다리지 않게)
    for (const a of list.getAnimations({ subtree: true })) a.finish();
    await new Promise((res) => setTimeout(res, 50));
    list.style.display = 'none';
    list.getBoundingClientRect();
    list.style.display = '';
    const again = { mention: getComputedStyle(list.firstElementChild).animationName, card: getComputedStyle(list.querySelector('.ev-card')).animationName, holo: holo(list), running: list.getAnimations({ subtree: true }).length };
    list.remove();
    return { first, again };
  });
  assert.equal(r.first.mention, 'mo-fade');
  assert.equal(r.first.card, 'mo-pop-up');
  assert.equal(r.first.holo, 'don-holo');
  assert.deepEqual(r.again, { mention: 'none', card: 'none', holo: 'none', running: 0 }, JSON.stringify(r.again));
});

await check('M12 통합 채팅 — 올려 읽는 중 앞줄을 지우면 보던 줄이 제자리', async () => {
  const r = await page.evaluate(async () => {
    const { chatFollower, followAppend } = await import('/js/chat-follow.js');
    const list = document.createElement('div');
    list.style.cssText = 'position:fixed;left:0;top:0;width:300px;height:200px;overflow:auto;overflow-anchor:none;z-index:99999';
    document.body.append(list);
    const f = chatFollower(list);
    const line = (i) => `<div class="chat-line" style="height:20px">${i}</div>`;
    list.innerHTML = Array.from({ length: 100 }, (_, i) => line(i)).join('');
    list.scrollTop = 1000; // 50번째 줄
    f.stuck = false; f.touchedAt = performance.now();
    const before = [...list.children].find((c) => c.offsetTop >= list.scrollTop).textContent;
    let got;
    followAppend(list, (follow) => { got = follow; list.insertAdjacentHTML('beforeend', Array.from({ length: 30 }, (_, i) => line(100 + i)).join('')); }, { resumeMs: 8000 });
    const { trimFront } = await import('/js/chat-follow.js');
    trimFront(list, 80, { keepView: !got });
    const after = [...list.children].find((c) => c.offsetTop >= list.scrollTop).textContent;
    list.remove();
    return { got, before, after };
  });
  assert.equal(r.got, false, 'followAppend 는 붙이기 전 따라가기 여부를 넘긴다');
  assert.equal(r.after, r.before, `보던 줄 ${r.before} → ${r.after}`);
  const mc = src('js/multichat.js');
  assert.equal((mc.match(/trimFront\(this\.mergedList, 600, \{ keepView: !follow \}\)/g) || []).length, 2, '멀티채팅 통합 목록 두 곳');
  assert.match(src('js/ml-multichat.js'), /trimFront\(list, max, \{ keepView: !follow \}\)/, '멀티라이브 통합 목록');
  assert.match(src('js/ml-watch-info.js'), /behavior: motionReduced\(\) \? 'auto' : 'smooth'/, '채널 정보 탭 스크롤');
});

assert.deepEqual(errors, [], errors.join('\n'));
await ctx.close();

// M05 메뉴 막대 팝오버 — 따로 연다(menubar.html).
await check('M05 메뉴 막대 — 새로고침 회전은 모션 줄이기·절약에서 멈추고 절약 등급을 따른다', async () => {
  const c = await browser.newContext({ viewport: { width: 384, height: 584 } });
  const p = await c.newPage();
  await p.addInitScript(mockTauri, [base, 'dark', 4]);
  await p.addInitScript(() => {
    const inv = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async (cmd, args) => {
      if (cmd === 'settings_get') { let s = {}; try { s = await inv(cmd, args); } catch { /* 없음 */ } return { ...s, appearance: { ...(s.appearance || {}), theme: 'dark', motionQuality: 'reduced' } }; }
      if (cmd === 'tray_get_state') return { following: [], loggedIn: false };
      return inv(cmd, args);
    };
  });
  await p.goto(`${base}menubar.html`);
  await p.waitForFunction(() => document.documentElement.dataset.motion === 'reduced', null, { timeout: 8000 });
  const spin = () => p.evaluate(() => {
    const b = document.createElement('button');
    b.className = 'mb-icon-btn is-spinning';
    b.innerHTML = '<svg viewBox="0 0 10 10"></svg>';
    document.body.append(b);
    const n = getComputedStyle(b.querySelector('svg')).animationName;
    b.remove();
    return n;
  });
  assert.equal(await spin(), 'none', "'절약'이면 돌지 않는다");
  await p.evaluate(() => { document.documentElement.dataset.motion = 'balanced'; });
  assert.equal(await spin(), 'mb-spin', '평소에는 돈다');
  await p.emulateMedia({ reducedMotion: 'reduce' });
  assert.equal(await spin(), 'none', '시스템 모션 줄이기면 돌지 않는다');
  await c.close();
});

// M10 시작 화면 — 저장된 등급이 첫 페인트 전에.
await check("M10 시작 화면 — 저장된 '절약'이 설정을 받기 전부터 걸린다", async () => {
  const c = await browser.newContext({ viewport: { width: 1000, height: 700 } });
  const p = await c.newPage();
  await p.addInitScript(mockTauri, [base, 'dark', 4]);
  await p.addInitScript(() => {
    try { localStorage.setItem('cview.motion', 'reduced'); localStorage.setItem('cview.welcomed.v1', '1'); } catch { /* 무시 */ }
    // 문서를 다 읽은 순간(모듈 실행·설정 응답 전)의 등급
    window.__motionSeen = [];
    document.addEventListener('readystatechange', () => { if (document.readyState === 'interactive') window.__motionSeen.push(document.documentElement.dataset.motion || ''); });
  });
  await p.goto(base);
  await p.waitForSelector('[data-page]', { state: 'attached' });
  await p.waitForFunction(() => window.__motionSeen.length >= 1);
  const seen = await p.evaluate(() => window.__motionSeen);
  assert.equal(seen[0], 'reduced', `첫 값 ${seen}`);
  // 설정을 받은 뒤 값을 다시 저장한다(모의 설정은 등급이 없어 균형)
  await p.waitForFunction(() => localStorage.getItem('cview.motion') === 'balanced', null, { timeout: 8000 });
  await c.close();
});

await browser.close();
server.close();
console.log(`\n${total - failures.length}/${total} 통과${failures.length ? ` — 실패: ${failures.join(', ')}` : ''}`);
process.exit(failures.length ? 1 : 0);
