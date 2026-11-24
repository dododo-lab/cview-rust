// 기본 화면 상주 메모리 회귀 검사(모의 IPC·헤드리스 브라우저) — 2026-11-24 기본 메모리 압축.
// 실제 앱(WKWebView) WebContent 메모리의 2/3 가 합성 레이어 그림(IOSurface)이었다. 두 원인을 막는다.
// B01 등장 애니메이션은 끝난 뒤 마지막 장면을 붙들지 않는다(fill backwards) — `both` 면 끝난 애니메이션이 요소를 계속
//     합성 레이어로 두고(페이지 전체·카드 13장·히어로·인사이트 칸), 그 위에 그려지는 것까지 '겹침' 레이어가 됐다.
//     홈·인기·팔로잉·카테고리·라이브러리·통계·멀티라이브·설정을 돌며 정착 뒤 'finished + fill forwards/both' 인 애니메이션이 없어야 한다
//     (나가는 애니메이션·스크롤 연동·대기열 진행 막대처럼 끝 상태가 필요한 것은 제외 목록).
// B02 히어로 앰비언트 빛은 CSS filter·3D 변환 없이 미리 흐린 작은 그림(data:image/png, 가로 48칸)으로 그린다 —
//     filter: blur(56px)+translateZ(0) 은 흐림 반경만큼 넓게 잡힌 레이어 경계로 아래 제목·선반까지 레이어로 끌어들였다.
// B03 히어로 장이 바뀌면 앰비언트도 새 장의 그림으로 바뀐다(앞 장의 늦은 결과가 덮지 않는다).
// B04 앰비언트 그림의 가장자리는 옅고(투명에 가깝고) 가운데는 썸네일 색이다 — CSS blur 처럼 밖을 투명으로 보고 흐렸다.
// 저장소 루트에서: node tools/ui-regression/memory-baseline.mjs   (CVIEW_UI_BROWSER=webkit 이면 WebKit)
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, base, mockTauri, go } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const WEBKIT = process.env.CVIEW_UI_BROWSER === 'webkit';
const browser = await (WEBKIT ? pw.webkit : pw.chromium).launch();
const results = [];
async function check(id, fn) {
  if (process.env.CVIEW_ONLY && !id.startsWith(process.env.CVIEW_ONLY)) return;
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message.split('\n').slice(0, 12).join('\n') }); }
}

async function open() {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 860 } });
  const page = await ctx.newPage();
  page.setDefaultTimeout(10000);
  const errors = [];
  page.on('pageerror', (e) => { const m = String(e); if (!/ResizeObserver loop/.test(m)) errors.push(m); });
  await page.addInitScript(mockTauri, [base, 'dark']);
  await page.addInitScript(() => { try { localStorage.setItem('cview.welcomed.v1', '1'); } catch { /* 없음 */ } });
  await page.goto(base);
  await page.waitForSelector('[data-page="home"]:not([hidden]) .hero-main', { state: 'attached' });
  await page.waitForTimeout(1500);
  return { ctx, page, errors };
}

/** 끝났는데 마지막 장면을 붙든(fill forwards/both) 애니메이션 — 끝 상태가 필요한 것은 뺀다. */
const KEEP = /^(queue-progress|mo-header-glass|mo-header-lift|mo-hero-parallax|mo-timer|osd-fade|ml-added-glow|dp-flash|hm-[\w-]+)$/;
const held = (page) => page.evaluate((keep) => {
  const re = new RegExp(keep);
  const d = (el) => !el ? '?' : `${el.localName}${el.id ? `#${el.id}` : ''}${el.classList.length ? `.${[...el.classList].slice(0, 2).join('.')}` : ''}`;
  return document.getAnimations()
    .filter((a) => a.playState === 'finished')
    .filter((a) => { const f = a.effect?.getComputedTiming?.().fill; return f === 'forwards' || f === 'both'; })
    .map((a) => ({ name: a.animationName || a.id || a.constructor.name, el: d(a.effect?.target) + (a.effect?.pseudoElement || '') }))
    .filter((x) => !re.test(x.name));
}, KEEP.source);

await check('B01 finished entrance animations do not keep filling (no permanent compositing layers)', async () => {
  const { ctx, page, errors } = await open();
  const seen = [];
  for (const route of ['home', 'popular', 'following', 'categories', 'library', 'stats', 'multilive', 'settings', 'home']) {
    if (route !== 'home' || seen.length) await go(page, route);
    await page.waitForTimeout(1200);
    const h = await held(page);
    if (h.length) seen.push(`${route}: ${h.slice(0, 6).map((x) => `${x.name}@${x.el}`).join(', ')}${h.length > 6 ? ` 외 ${h.length - 6}` : ''}`);
  }
  assert.deepEqual(seen, [], `끝난 뒤에도 붙든 애니메이션:\n${seen.join('\n')}`);
  assert.deepEqual(errors, []);
  await ctx.close();
});

const ambient = (page) => page.evaluate(() => {
  const box = document.querySelector('.hero-ambient');
  const img = box?.querySelector('img');
  const cs = (el) => (el ? getComputedStyle(el) : null);
  return {
    box: !!box,
    boxFilter: cs(box)?.filter, boxTransform: cs(box)?.transform, boxWill: cs(box)?.willChange,
    src: img?.getAttribute('src') || '', imgFilter: cs(img)?.filter, imgTransform: cs(img)?.transform,
    natural: img ? [img.naturalWidth, img.naturalHeight] : null,
  };
});

await check('B02 hero ambient is a pre-blurred tiny image without CSS filter or 3D transform', async () => {
  const { ctx, page, errors } = await open();
  await page.waitForFunction(() => document.querySelector('.hero-ambient img')?.getAttribute('src')?.startsWith('data:'), null, { timeout: 8000 });
  await page.waitForFunction(() => document.querySelector('.hero-ambient img')?.naturalWidth > 0);
  const a = await ambient(page);
  assert.ok(a.src.startsWith('data:image/png'), `앰비언트 src: ${a.src.slice(0, 40)}`);
  assert.equal(a.natural[0], 48, `가로 48칸이어야 한다: ${a.natural}`);
  for (const [k, v] of Object.entries({ boxFilter: a.boxFilter, imgFilter: a.imgFilter })) assert.equal(v, 'none', `${k} = ${v}`);
  for (const [k, v] of Object.entries({ boxTransform: a.boxTransform, imgTransform: a.imgTransform })) assert.equal(v, 'none', `${k} = ${v}`);
  assert.equal(a.boxWill, 'auto');
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('B03 ambient follows the hero slide', async () => {
  const { ctx, page, errors } = await open();
  await page.waitForFunction(() => document.querySelector('.hero-ambient img')?.getAttribute('src')?.startsWith('data:'), null, { timeout: 8000 });
  const first = (await ambient(page)).src;
  const before = await page.evaluate(() => document.querySelector('.hero-ambient').dataset.src);
  await page.locator('.queue-item').nth(2).click();
  await page.waitForFunction((s) => document.querySelector('.hero-ambient').dataset.src !== s, before);
  await page.waitForFunction((s) => { const v = document.querySelector('.hero-ambient img')?.getAttribute('src'); return v && v !== s; }, first, { timeout: 8000 });
  const after = await page.evaluate(() => {
    const box = document.querySelector('.hero-ambient');
    return { src: box.dataset.src, thumb: document.querySelector('.hero-main img')?.getAttribute('src') || '' };
  });
  // 모의 썸네일 주소는 채널마다 다르다 — 앰비언트가 지금 장의 썸네일 번호를 따라간다.
  const num = (u) => (u.match(/__img\/thumb\/(\d+)/) || [])[1];
  assert.equal(num(after.src), num(after.thumb), `앰비언트 ${after.src} / 히어로 ${after.thumb}`);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await check('B04 ambient image fades to transparent at the edges like CSS blur', async () => {
  const { ctx, page, errors } = await open();
  await page.waitForFunction(() => document.querySelector('.hero-ambient img')?.naturalWidth > 0 && document.querySelector('.hero-ambient img').getAttribute('src').startsWith('data:'), null, { timeout: 8000 });
  const px = await page.evaluate(async () => {
    const img = document.querySelector('.hero-ambient img');
    const c = document.createElement('canvas');
    c.width = img.naturalWidth; c.height = img.naturalHeight;
    const ctx = c.getContext('2d');
    ctx.drawImage(img, 0, 0);
    const d = ctx.getImageData(0, 0, c.width, c.height).data;
    const a = (x, y) => d[(y * c.width + x) * 4 + 3];
    return { corner: a(0, 0), center: a(c.width >> 1, c.height >> 1) };
  });
  assert.ok(px.center >= 250, `가운데는 불투명: ${px.center}`);
  assert.ok(px.corner < 160, `모서리는 옅다: ${px.corner}`);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await browser.close();
server.close();
let failed = 0;
for (const r of results) {
  console.log(`${r.ok ? '✓' : '✗'} ${r.id}${r.ok ? '' : `\n    ${r.error.replace(/\n/g, '\n    ')}`}`);
  if (!r.ok) failed++;
}
console.log(`\n${results.length - failed}/${results.length} passed (${WEBKIT ? 'webkit' : 'chromium'})`);
process.exit(failed ? 1 : 0);
