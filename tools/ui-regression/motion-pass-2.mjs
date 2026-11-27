// 애니메이션·모션 정밀 고도화 2차(2026-11-27) 회귀 검사 — 1차 수정의 회귀, 시청 화면·칩·화면 다시 그리기.
// 모의 IPC·헤드리스 브라우저(실제 서비스·데이터는 건드리지 않는다). CVIEW_UI_BROWSER=webkit 이면 WebKit.
// 저장소 루트에서: NODE_PATH=<Playwright 경로> node tools/ui-regression/motion-pass-2.mjs
// N01 홈 히어로 — 마우스로 누른 탭이 초점을 쥐어도 자동 넘김이 멈추지 않는다(키보드 초점만 멈춤, Chromium 에서 갈리던 것).
// N02 채팅 앞줄 지우기 — 지울 줄·첫 줄이 숨어 있어도(검색·이 사람만 보기) 보던 줄을 제자리에 둔다.
// N03 멀티라이브 — 커지며 나타나는 중인 패인은 다른 배치 변화에 끊기지 않는다(반투명 → 바로 불투명 없음).
// N04 시청 화면 OSD — 알림은 늘 있는 낭독 자리로(보이는 OSD 는 aria-hidden).
// N05 영상 위 후원 알림 — 다시 보여도 등장·6단계 흐름을 되풀이하지 않는다.
// N06 홈 발견 격자 '더 보기' — 보이던 카드는 다시 떠오르지 않고 초점은 새 단추로.
// N07 코드 확인 — 검색 결과·보관함 다시 그리기는 swapContent, 광고 막은 첫 스타일 뒤에 보임, 칩 숨길 때 미끄러짐 표시 제거,
//     팔로잉 미리보기 카드 다시 놓기, 사이드바 사본에 캔버스 그림 복사.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
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
const src = (f) => fs.readFileSync(path.resolve('app-ui', f), 'utf8');

const { ctx, page, errors } = await openApp(browser, { welcomed: true, liveCount: 30 });

await check('N01 홈 히어로 — 마우스로 누른 탭은 자동 넘김을 멈추지 않는다', async () => {
  await page.evaluate(() => {
    const host = document.createElement('div');
    host.id = 'n01';
    host.style.cssText = 'position:fixed;left:0;top:0;z-index:99999;background:#000';
    host.innerHTML = '<div class="hero"><button class="queue-item active" style="width:80px;height:40px"><span class="queue-progress"></span></button></div><button class="outside" style="width:80px;height:40px">밖</button>';
    document.body.append(host);
  });
  const state = () => page.evaluate(() => getComputedStyle(document.querySelector('#n01 .queue-progress')).animationPlayState);
  await page.click('#n01 .queue-item');
  await page.mouse.move(600, 600);
  const afterClick = await state();
  await page.focus('#n01 .outside');
  await page.keyboard.press('Shift+Tab');
  const kbd = await page.evaluate(() => document.activeElement?.classList.contains('queue-item'));
  const afterKey = await state();
  await page.evaluate(() => document.getElementById('n01').remove());
  assert.equal(afterClick, 'running', '마우스로 누른 뒤 떠나면 계속 넘어간다');
  if (kbd) assert.equal(afterKey, 'paused', '키보드 초점이면 멈춘다');
});

await check('N02 채팅 앞줄 지우기 — 숨긴 줄이 섞여도 보던 줄이 제자리', async () => {
  const r = await page.evaluate(async () => {
    const { trimFront } = await import('/js/chat-follow.js');
    const out = [];
    for (const hide of [[0], [30], [0, 30]]) {
      const wrap = document.createElement('div');
      wrap.style.cssText = 'position:fixed;left:0;top:120px;padding-top:40px;z-index:99999';
      const list = document.createElement('div');
      list.style.cssText = 'width:300px;height:200px;overflow:auto;overflow-anchor:none';
      wrap.append(list);
      document.body.append(wrap);
      list.innerHTML = Array.from({ length: 130 }, (_, i) => `<div class="chat-line" style="height:20px${hide.includes(i) ? ';display:none' : ''}">${i}</div>`).join('');
      list.scrollTop = 1500;
      const top = () => [...list.children].find((c) => c.offsetParent && c.getBoundingClientRect().top >= list.getBoundingClientRect().top - 1).textContent;
      const before = top();
      trimFront(list, 100, { keepView: true }); // 130 > 100 + 25 → 앞 30줄을 지운다
      out.push({ hide: hide.join('+'), before, after: top() });
      wrap.remove();
    }
    return out;
  });
  for (const x of r) assert.equal(x.after, x.before, `숨긴 줄 ${x.hide}: ${x.before} → ${x.after}`);
});

await check('N03 멀티라이브 — 나타나는 중인 패인은 끊기지 않는다', async () => {
  const r = await page.evaluate(async () => {
    const { snapshotPanes, playLayoutMotion } = await import('/js/ml-motion.js');
    const stage = document.createElement('div');
    stage.style.cssText = 'position:fixed;left:0;top:0;width:900px;height:300px;display:grid;grid-template-columns:1fr 1fr 1fr;z-index:99999';
    const mk = () => { const el = document.createElement('div'); el.style.background = '#888'; stage.append(el); return el; };
    const panes = new Map([[0, { el: mk() }], [1, { el: mk() }]]);
    for (const [i, p] of panes) { p.el.style.gridRow = '1'; p.el.style.gridColumn = String(i + 1); }
    document.body.append(stage);
    playLayoutMotion(panes, new Map([[0, panes.get(0).el.getBoundingClientRect()]])); // 1번은 새 패인 — 나타난다
    const enter = panes.get(1).anim;
    await new Promise((res) => setTimeout(res, 140)); // 기다림(90ms)은 지났고 커지며 나타나는 중
    const before = snapshotPanes(panes);
    panes.get(0).el.style.gridColumn = '3'; // 다른 패인만 옮긴다(1번 칸은 그대로)
    playLayoutMotion(panes, before);
    const out = { same: panes.get(1).anim === enter, state: enter.playState };
    for (const p of panes.values()) p.anim?.cancel();
    stage.remove();
    return out;
  });
  assert.ok(r.same && r.state === 'running', `나타나기가 끊겼다(${JSON.stringify(r)})`);
});

await check('N04 시청 화면 OSD — 늘 있는 낭독 자리로 알린다', async () => {
  const r = await page.evaluate(async () => {
    const { announcePolite } = await import('/js/a11y.js');
    announcePolite('볼륨 50%');
    const live = document.getElementById('polite-live');
    const first = { text: live.textContent, role: live.getAttribute('role'), shown: getComputedStyle(live).display !== 'none' };
    announcePolite('볼륨 50%');
    return { first, again: live.textContent, count: document.querySelectorAll('#polite-live').length };
  });
  assert.deepEqual(r.first, { text: '볼륨 50%', role: 'status', shown: true });
  assert.equal(r.again, '볼륨 50%');
  assert.equal(r.count, 1);
  for (const f of ['js/viewer-controls.js', 'js/ml-watch.js']) {
    const s = src(f);
    assert.match(s, /this\.osd\.setAttribute\('aria-hidden', 'true'\)/, `${f}: 보이는 OSD 는 꾸밈`);
    assert.match(s, /announcePolite\(text\);\n\s+this\.osd\.textContent = text;/, `${f}: flash 가 낭독 자리에 쓴다`);
    assert.doesNotMatch(s, /this\.osd\.setAttribute\('role'/, `${f}: 숨었다 나타나는 live 영역 없음`);
  }
});

await check('N05 영상 위 후원 알림 — 다시 보여도 등장을 되풀이하지 않는다', async () => {
  const r = await page.evaluate(async () => {
    const { StreamAlerts } = await import('/js/donations.js');
    const stage = document.createElement('div');
    stage.style.cssText = 'position:fixed;left:0;top:0;width:800px;height:450px;z-index:99999';
    document.body.append(stage);
    const alerts = new StreamAlerts(stage);
    const item = document.createElement('div');
    item.className = 'stream-alert ev-lv-6';
    item.innerHTML = '<span class="stream-alert-icon">★</span><span class="stream-alert-title"><b>10만</b></span>';
    alerts.show(item, 60000);
    const first = getComputedStyle(item).animationName;
    for (const a of stage.getAnimations({ subtree: true })) a.finish();
    await new Promise((res) => setTimeout(res, 50));
    stage.style.display = 'none';
    stage.getBoundingClientRect();
    stage.style.display = '';
    const again = { card: getComputedStyle(item).animationName, after: getComputedStyle(item, '::after').animationName, icon: getComputedStyle(item.querySelector('.stream-alert-icon')).animationName, running: stage.getAnimations({ subtree: true }).length };
    stage.remove();
    return { first, again };
  });
  assert.equal(r.first, 'alert-in');
  assert.deepEqual(r.again, { card: 'none', after: 'none', icon: 'none', running: 0 }, JSON.stringify(r.again));
});

await check("N06 홈 발견 격자 '더 보기' — 보이던 카드는 다시 떠오르지 않고 초점은 새 단추로", async () => {
  await page.waitForSelector('[data-grid-more]', { state: 'attached', timeout: 8000 });
  await page.evaluate(() => { document.documentElement.dataset.motion = 'rich'; });
  await page.focus('[data-grid-more]');
  await page.keyboard.press('Enter');
  const r = await page.evaluate(() => {
    const box = document.querySelector('.popular-grid');
    const cards = [...box.querySelectorAll('.grid > *')];
    return { n: cards.length, anims: cards.slice(0, 12).map((c) => getComputedStyle(c).animationName).filter((x) => x !== 'none').length, focusIn: box.contains(document.activeElement) };
  });
  await page.evaluate(() => { document.documentElement.dataset.motion = 'balanced'; });
  assert.ok(r.n > 12, `카드가 늘었다(${r.n})`);
  assert.equal(r.anims, 0, '보이던 카드는 다시 떠오르지 않는다');
  assert.ok(r.focusIn, '초점이 격자 안(새 더 보기·새 카드)에 남는다');
});

await check('N07 코드 확인 — 검색·보관함·광고·칩·팔로잉 카드·사이드바 사본', async () => {
  const pages = src('js/pages.js');
  assert.match(pages, /swapContent\(this\.body, `<div class="search-results">/, '검색 결과는 swapContent');
  assert.match(pages, /this\.renderResults\(\{ enter: changed \}\)/, '탭을 바꾸면 떠오른다');
  assert.match(src('js/library.js'), /if \(seq === this\.seq\) swapContent\(this\.body, html\);/, '보관함');
  assert.match(src('js/ads.js'), /void getComputedStyle\(this\.root\)\.opacity;\n\s+requestAnimationFrame\(\(\) => this\.root\.classList\.add\('shown'\)\)/, '광고 막 페이드');
  assert.match(src('js/multilive.js'), /if \(c\.hidden\) c\.classList\.remove\('enter-start', 'enter-end'\);/, '칩 미끄러짐 표시');
  assert.match(src('js/live-following.js'), /this\.fillPeek\(this\.peekKey, \{ keepMedia: true \}\);\n[^\n]*\n\s+this\.place\(this\.items\.get\(this\.peekKey\)\);/, '팔로잉 카드 다시 놓기');
  assert.match(src('js/sidebar.js'), /drawImage\(srcCanvas\[i\], 0, 0\)/, '사이드바 사본 캔버스');
});

assert.deepEqual(errors, [], errors.join('\n'));
await ctx.close();
await browser.close();
server.close();
console.log(`\n${total - failures.length}/${total} 통과${failures.length ? ` — 실패: ${failures.join(', ')}` : ''}`);
process.exit(failures.length ? 1 : 0);
