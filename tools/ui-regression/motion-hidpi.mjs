// 모션·고해상도 회귀 검사(모의 IPC, 헤드리스 브라우저).
// MH01 시스템 "동작 줄이기"면 화면 이동·카드 호버·사이드바 접기 내내 시간이 있는 애니메이션·전환이 하나도 돌지 않는다.
// MH02 화면마다 가만히 두면 보이지 않는(화면 밖·숨김) 무한 반복 애니메이션이 없다 — 4K·Retina 에서 보이지 않는 칸을 계속 다시 그렸다.
// MH03 레이아웃을 매 프레임 다시 계산하는 속성(width·height·padding·margin·left·top·font-size·grid-template-columns 등)을
//      움직이는 전환·애니메이션은 정해 둔 곳(사이드바 접기 등)뿐이다. 새로 생기면 transform·opacity 로 바꿀지 먼저 따진다.
// MH04 썸네일 단계 — 1배율 1280 창은 예전과 같다(라이브 image_405, 히어로 image_720). 4K 를 2배율로 쓰면 히어로가 image_1280,
//      화면 크기를 키우면(setImageZoom) 라이브 카드도 물리 픽셀에 맞춰 올라가고, 다시보기는 w750·채팅 아바타는 f120.
// MH05 넓은 창(3840, 4K 1배율·2배율) — 시청 리포트 순위의 이름과 막대 사이가 벌어지지 않고(40px 미만), 카테고리 범례는 460px 이하,
//      홈 '나의 시청 취향' 숫자 칸은 260px 이하로 붙어 있다.
// MH06 설정 → 외관에 '화면 크기'(80~200%)가 있고, 단축키 시트에 ⌘= / ⌘- / ⌘0 이 있다. 창이 뜰 때 저장된 화면 크기를 건다(ui_zoom_sync).
// MH07 Windows(메뉴 막대 없음) — Ctrl+= · Ctrl+- · Ctrl+0 이 화면 크기 단계를 옮긴다(ui_zoom_step).
// 저장소 루트에서: node tools/ui-regression/motion-hidpi.mjs   (Playwright 가 필요하다. CVIEW_UI_BROWSER=webkit 이면 WebKit)
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, base, mockTauri, go } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const WEBKIT = process.env.CVIEW_UI_BROWSER === 'webkit';
const engine = WEBKIT ? pw.webkit : pw.chromium;
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message.split('\n').slice(0, 4).join('\n') }); }
}

// 새로 시작한 애니메이션을 모두 적어 둔다(매 프레임 document.getAnimations).
function recordAnimations() {
  const seen = new WeakSet();
  window.__anims = [];
  const desc = (el) => (el ? `${el.tagName.toLowerCase()}${el.classList.length ? `.${[...el.classList].join('.')}` : ''}` : '?');
  const tick = () => {
    for (const a of document.getAnimations()) {
      if (seen.has(a)) continue;
      seen.add(a);
      const e = a.effect;
      let props = [];
      try { props = [...new Set(e.getKeyframes().flatMap((k) => Object.keys(k).filter((x) => !['offset', 'easing', 'composite', 'computedOffset'].includes(x))))]; } catch { /* 무시 */ }
      if (a.transitionProperty) props = [a.transitionProperty];
      const t = e && e.getTiming ? e.getTiming() : {};
      window.__anims.push({ name: a.animationName || a.transitionProperty || '', props, target: desc(e && e.target) + (e && e.pseudoElement ? e.pseudoElement : ''), dur: typeof t.duration === 'number' ? t.duration : 0 });
    }
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
}

async function open({ width = 1440, height = 900, dpr = 1, reduced = false, ua } = {}) {
  const ctx = await engine.launch().then(async (b) => {
    const c = await b.newContext({ viewport: { width, height }, deviceScaleFactor: dpr, colorScheme: 'dark', reducedMotion: reduced ? 'reduce' : 'no-preference', ...(ua ? { userAgent: ua } : {}) });
    c.__browser = b;
    return c;
  });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mockTauri, [base, 'dark', 24]);
  await page.addInitScript(recordAnimations);
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.waitForFunction(() => { const s = document.getElementById('splash'); return !s || !s.isConnected || getComputedStyle(s).display === 'none'; }, null, { timeout: 8000 }).catch(() => {});
  await page.waitForTimeout(600);
  return { ctx, page, errors, close: () => ctx.__browser.close() };
}

const ROUTES = [['home', {}], ['popular', {}], ['following', {}], ['library', {}], ['search', { query: '마인' }], ['multilive', {}], ['settings', {}], ['home', { view: 'report' }]];
async function tour(page, { idle = 0, onIdle } = {}) {
  for (const [r, p] of ROUTES) {
    await go(page, r, p);
    await page.waitForTimeout(700);
    const card = await page.$('.page:not([hidden]) .card');
    if (card) { await card.hover().catch(() => {}); await page.waitForTimeout(300); await page.mouse.move(2, 400); }
    if (idle) { await page.waitForTimeout(idle); if (onIdle) await onIdle(`${r}${JSON.stringify(p)}`); }
  }
  // 사이드바 접기·펼치기
  const tog = await page.$('.sidebar-toggle');
  if (tog) { await tog.click(); await page.waitForTimeout(500); await tog.click(); await page.waitForTimeout(500); }
}

// MH03 허용 목록 — 레이아웃 속성을 움직여도 되는 곳. 모두 작은 요소이거나(범위가 좁다) 사이드바 접기처럼 일부러 폭을 옮기는 곳이다.
const LAYOUT_PROPS = /^(width|height|min-width|max-width|min-height|max-height|left|top|right|bottom|inset|margin.*|padding.*|font-size|grid-template-columns|grid-template-rows|flex-basis|line-height|border-width|letter-spacing)$/i;
const camel = (p) => p.replace(/[A-Z]/g, (m) => `-${m.toLowerCase()}`);
const LAYOUT_ALLOW = [
  /^aside\.sidebar/, /^button\.nav-item/, /^button\.side-live/, /^span\.side-live-avatar/, /^div\.nav-section/, /^div\.brand-row/, /^div\.brand\b/,
  /^span\.brand-mark/, /^button\.icon-btn\.sidebar-toggle/, /^button\.quick-search/, /^div\.side-quick/, /^button\.icon-btn.*\b(hm|back)\b/,
  /\.seg-glider/, /^div\.chat-tools/, /\.chat-state/, /\.mwi-mini/, /\.ml-vol-range/, /\.vol-group/, /\.ml-peek/, /^span\.toggle/,
];

let wideReport = null;
await check('MH01 동작 줄이기 — 시간이 있는 애니메이션이 없다', async () => {
  const { page, errors, close } = await open({ reduced: true });
  try {
    await tour(page);
    const moving = (await page.evaluate(() => window.__anims)).filter((a) => a.dur > 1);
    assert.deepEqual([...new Set(moving.map((a) => `${a.target} ${a.name} ${a.dur}ms`))].slice(0, 12), []);
    assert.deepEqual(errors, []);
  } finally { await close(); }
});

await check('MH02·MH03 숨은 무한 반복 없음, 레이아웃 속성 애니메이션은 허용 목록뿐', async () => {
  const { page, errors, close } = await open();
  const hidden = [];
  try {
    await tour(page, {
      idle: 1500,
      onIdle: async (route) => {
        const bad = await page.evaluate(() => document.getAnimations().filter((a) => a.playState === 'running' && a.effect && a.effect.getTiming().iterations === Infinity).map((a) => {
          const t = a.effect.target;
          const r = t.getBoundingClientRect();
          const shown = t.checkVisibility ? t.checkVisibility({ opacityProperty: true, visibilityProperty: true }) : true;
          const inView = r.width > 0 && r.height > 0 && r.bottom > 0 && r.top < innerHeight && r.right > 0 && r.left < innerWidth;
          return shown && inView ? null : `${t.tagName.toLowerCase()}.${[...t.classList].join('.')} ${a.animationName}`;
        }).filter(Boolean));
        for (const b of bad) hidden.push(`${route}: ${b}`);
      },
    });
    assert.deepEqual(hidden, [], '보이지 않는 무한 반복 애니메이션');
    const anims = await page.evaluate(() => window.__anims);
    const offenders = [...new Set(anims
      .filter((a) => a.props.map(camel).some((p) => LAYOUT_PROPS.test(p)))
      .filter((a) => !LAYOUT_ALLOW.some((re) => re.test(a.target)))
      .map((a) => `${a.target} ${a.props.map(camel).join(',')}`))];
    assert.deepEqual(offenders, [], '레이아웃 속성을 움직이는 새 전환·애니메이션');
    assert.deepEqual(errors, []);
  } finally { await close(); }
});

await check('MH04 썸네일 단계 — 1배율은 그대로, 고해상도·화면 크기 확대는 물리 픽셀에 맞춰', async () => {
  const LIVE = 'https://livecloud-thumb.akamaized.net/chzzk/x/thumbnail/image_720.jpg';
  const VOD = 'https://video-phinf.pstatic.net/a/b.jpg';
  const AV = 'https://nng-phinf.pstatic.net/a/b.png';
  const probe = (page) => page.evaluate(async ([live, vod, av]) => {
    const d = await import('/js/dom.js');
    // 모의 IPC 에서는 캐시 주소(cvimg://localhost/<종류>%2F<URL>)로 나온다 — 원래 URL 로 풀어 본다.
    const src = (u, kind) => { const s = d.imageSrc(u, kind); const m = /^(?:cvimg:\/\/localhost|http:\/\/cvimg\.localhost)\/(.*)$/.exec(s); return m ? decodeURIComponent(m[1]).replace(/^[^/]+\//, '') : s; };
    const tier = (u) => (u.match(/image_(\d+)\.jpg/) || [])[1];
    const out = { live: tier(src(live, 'live')), hero: tier(src(live, 'hero')), vod: new URL(src(vod, 'vod')).searchParams.get('type'), sm: new URL(src(av, 'avatar-sm')).searchParams.get('type') };
    d.setImageZoom(1.75);
    out.zoomedLive = tier(src(live, 'live'));
    d.setImageZoom(1);
    return out;
  }, [LIVE, VOD, AV]);
  const one = await open({ width: 1280, height: 800, dpr: 1 });
  try {
    const r = await probe(one.page);
    assert.equal(r.live, '405'); assert.equal(r.hero, '720'); assert.equal(r.vod, 'w640'); assert.equal(r.sm, 'f60_60_na');
    // 1배율 1280 창에서 175% 는 물리 세로 약 350 — 405 로 충분하다
    assert.equal(r.zoomedLive, '405');
  } finally { await one.close(); }
  const uhd = await open({ width: 3840, height: 2160, dpr: 2 });
  try {
    const r = await probe(uhd.page);
    assert.equal(r.hero, '1280', '4K 2배율 히어로');
    assert.equal(r.sm, 'f120_120_na', '2배율 채팅 아바타');
    // 3840 창에서 카드는 약 260px → 2배율 물리 세로 약 292 → 405 로 충분하다(괜히 키우지 않는다)
    assert.equal(r.live, '405');
  } finally { await uhd.close(); }
  const retina = await open({ width: 1100, height: 760, dpr: 2 });
  try {
    const r = await probe(retina.page);
    // 1100 창: 인사이트 칸 없음 → 본문 약 830 → 카드 3열 약 266 → 물리 세로 약 300 → 405
    assert.equal(r.live, '405');
    assert.ok(['w640', 'w750'].includes(r.vod));
    // 같은 칸을 175% 로 키우면(WebKit 은 devicePixelRatio 가 그대로) 물리 세로 약 525 — 10% 모자란 것까지 받아 주어 480
    assert.equal(r.zoomedLive, '480', `화면 크기 175% 라이브 단계 ${r.zoomedLive}`);
  } finally { await retina.close(); }
});

await check('MH05 넓은 창 — 리포트 순위·범례, 홈 취향 숫자가 벌어지지 않는다', async () => {
  for (const dpr of [1, 2]) {
    const { page, errors, close } = await open({ width: 3840, height: 2160, dpr });
    try {
      await go(page, 'home', {});
      await page.waitForSelector('.page-home .home-recap-metric', { state: 'visible' });
      const recap = await page.$$eval('.page-home .home-recap-metric', (els) => els.map((e) => Math.round(e.getBoundingClientRect().width)));
      assert.ok(recap.length && recap.every((w) => w <= 261), `취향 숫자 칸 폭 ${recap}`);
      await go(page, 'home', { view: 'report' });
      await page.waitForSelector('.share-bars li', { state: 'visible' });
      const gaps = await page.$$eval('.share-bars li', (lis) => lis.map((li) => {
        const name = li.querySelector('.rank-name'); const track = li.querySelector('.rank-track');
        const range = document.createRange(); range.selectNodeContents(name);
        return Math.round(track.getBoundingClientRect().left - range.getBoundingClientRect().right);
      }));
      // 이름 칸은 11em(약 143px) 까지 — 막대와의 간격은 그 안 — 예전엔 1000px 가까이 벌어졌다
      assert.ok(gaps.every((g) => g < 150), `이름–막대 간격 ${gaps}`);
      const legend = await page.$eval('.donut-legend', (e) => e.getBoundingClientRect().width).catch(() => 0);
      assert.ok(legend <= 461, `범례 폭 ${legend}`);
      wideReport = { recap, gaps, legend };
      assert.deepEqual(errors, []);
    } finally { await close(); }
  }
});

await check('MH06 설정 화면 크기·단축키 시트·창 시작 시 적용', async () => {
  const { page, errors, close } = await open();
  try {
    assert.ok((await page.evaluate(() => window.__calls)).includes('ui_zoom_sync'), '시작 때 ui_zoom_sync 를 부르지 않음');
    await go(page, 'settings', {});
    await page.waitForSelector('.page[data-page="settings"]:not([hidden])');
    await page.fill('.settings-search', '화면 크기');
    const range = page.locator('input[type="range"][data-domain="appearance"][data-key="uiScale"]');
    await range.waitFor({ state: 'attached' });
    assert.equal(await range.getAttribute('min'), '0.8');
    assert.equal(await range.getAttribute('max'), '2');
    await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: 'home', params: {} } })));
    await page.keyboard.press(process.platform === 'darwin' ? 'Meta+Slash' : 'Control+Slash');
    await page.waitForSelector('.shortcut-row');
    const rows = await page.$$eval('.shortcut-row', (r) => r.map((x) => x.textContent));
    assert.ok(rows.some((t) => t.includes('화면 크기')), '단축키 시트에 화면 크기 없음');
    assert.deepEqual(errors, []);
  } finally { await close(); }
});

await check('MH07 Windows — Ctrl+= · Ctrl+- · Ctrl+0 이 화면 크기를 옮긴다', async () => {
  const { page, close } = await open({ ua: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0 Safari/537.36' });
  try {
    assert.equal(await page.evaluate(() => document.documentElement.dataset.platform), 'windows');
    await page.evaluate(() => { window.__calls.length = 0; });
    await page.mouse.click(700, 600);
    for (const k of ['Control+Equal', 'Control+Minus', 'Control+Digit0']) await page.keyboard.press(k);
    await page.waitForTimeout(200);
    const n = (await page.evaluate(() => window.__calls)).filter((c) => c === 'ui_zoom_step').length;
    assert.equal(n, 3, `ui_zoom_step ${n}번`);
  } finally { await close(); }
});

server.close();
for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : `: ${r.error}`}`);
if (wideReport) console.log('넓은 창 측정', JSON.stringify(wideReport));
const failed = results.filter((r) => !r.ok).length;
console.log(failed ? `${failed} failed` : 'all passed');
process.exit(failed ? 1 : 0);
