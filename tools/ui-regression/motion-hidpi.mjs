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
// MH08 외관 변경 알림(settings://appearance, Rust 가 모든 창에) — 열린 설정 화면의 '화면 크기' 슬라이더와 % 표시, 썸네일 배율이 따라온다.
// MH09 devicePixelRatio 에 확대가 이미 들어 있으면(WebView2·확대가 남은 채 다시 불러온 창) 화면 크기를 두 번 곱하지 않는다.
// MH10 ⌘= ⌘- ⌘0 은 단축키 설정의 예약 키다(스크린샷·북마크 등에 묶어 메뉴에 같은 키가 둘 생기지 않게).
//      Windows 는 Ctrl+= · Ctrl+- · Ctrl+0 · Ctrl+(+) · Ctrl+Shift+(+) 도 예약이다(app.js 가 직접 받는다). 맥에서 Ctrl 조합은 비어 있다.
// MH11 저장 알림은 저장한 창에도 되돌아온다 — '화면 크기' 슬라이더에서 → 를 빠르게 세 번 눌러도 앞선 저장의 메아리가 값을
//      되돌리지 않고, 드래그 중에 온 다른 곳의 변경은 놓은 뒤에 반영된다.
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

// 이벤트 모의 — 화면이 듣는 Tauri 이벤트를 모아 두고 window.__emit(이름, 값)으로 보낸다(Rust 가 모든 창에 보내는 것을 흉내).
function eventBus() {
  const subs = {};
  window.__emit = (name, payload) => (subs[name] || []).forEach((cb) => cb({ payload }));
  window.__TAURI__.event.listen = async (name, cb) => { (subs[name] = subs[name] || []).push(cb); return () => {}; };
}

async function open({ width = 1440, height = 900, dpr = 1, reduced = false, ua, bus = false } = {}) {
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
  if (bus) await page.addInitScript(eventBus);
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

await check('MH08 외관 변경 알림 — 설정 슬라이더·썸네일 배율이 따라온다(⌘= 를 다른 곳에서 눌러도)', async () => {
  const { page, errors, close } = await open({ bus: true });
  try {
    await go(page, 'settings', {});
    await page.waitForSelector('.page[data-page="settings"]:not([hidden])');
    await page.fill('.settings-search', '화면 크기');
    const range = page.locator('input[type="range"][data-domain="appearance"][data-key="uiScale"]');
    await range.waitFor({ state: 'attached' });
    // 모의 settings_get 에는 uiScale 이 없어 처음 값은 정하지 않는다(실제 앱은 늘 있다) — 1.25 가 아니기만 하면 된다
    assert.notEqual(await range.inputValue(), '1.25');
    await page.evaluate(() => window.__emit('settings://appearance', { theme: 'dark', uiScale: 1.25, motionQuality: 'balanced' }));
    await page.waitForFunction(() => document.querySelector('input[data-key="uiScale"]').value === '1.25');
    const out = await range.evaluate((el) => el.parentElement.querySelector('output').textContent);
    assert.equal(out, '125%');
    // 같은 알림이 썸네일 배율(prefs-effects → setImageZoom)에도 닿는다 — 1배율 맥이면 1 × 1.25
    assert.equal(await page.evaluate(async () => (await import('/js/dom.js')).pixelScale()), 1.25);
    assert.deepEqual(errors, []);
  } finally { await close(); }
});

await check('MH09 기기 배율에 확대가 들어 있으면 두 번 곱하지 않는다(WebView2·다시 불러온 창)', async () => {
  const { page, close } = await open();
  try {
    const r = await page.evaluate(async () => {
      const d = await import('/js/dom.js');
      return [d.dprIncludesZoom(3.5, true), d.dprIncludesZoom(2, true), d.dprIncludesZoom(2, false), d.dprIncludesZoom(1, false), d.dprIncludesZoom(3, false), d.dprIncludesZoom(2.5, false)];
    });
    // Windows(WebView2)는 늘 포함, 맥은 1·2 면 화면 배율 그대로(곱한다), 2 초과·소수면 이미 포함
    assert.deepEqual(r, [true, true, false, false, true, true]);
  } finally { await close(); }
  const win = await open({ dpr: 3, ua: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0 Safari/537.36' });
  try {
    // 150% 모니터에서 화면 크기 200% → WebView2 의 devicePixelRatio 3 — 6 으로 부풀리지 않는다
    assert.equal(await win.page.evaluate(async () => { const d = await import('/js/dom.js'); d.setImageZoom(2); return d.pixelScale(); }), 3);
  } finally { await win.close(); }
});

await check('MH10 ⌘= ⌘- ⌘0 은 예약 키 — 다른 동작에 묶을 수 없다', async () => {
  const { page, close } = await open();
  try {
    const r = await page.evaluate(async () => {
      const s = await import('/js/shortcuts.js');
      const cmd = s.RESERVED.find(([k]) => k === 'f')[1];
      return ['=', '-', '0'].map((key) => s.reservedOf({ key, modifiers: cmd }));
    });
    assert.ok(r.every(Boolean), `예약 설명 ${JSON.stringify(r)}`);
  } finally { await close(); }
});

await check('MH10b Windows — Ctrl 확대 키도 예약 키, 맥은 Ctrl 조합을 막지 않는다', async () => {
  const { page, close } = await open();
  try {
    const r = await page.evaluate(async () => {
      const s = await import('/js/shortcuts.js');
      const { control, shift } = s.MOD;
      const keys = [['=', control], ['-', control], ['0', control], ['+', control], ['+', control | shift]];
      // 실제 키 이벤트 → 바인딩 경로로도 확인(Windows 는 Ctrl 을 MOD.control 로 적는다)
      const fromEvent = s.bindingFromEvent({ key: '0', code: 'Digit0', ctrlKey: true, metaKey: false, shiftKey: false, altKey: false });
      return {
        win: keys.map(([key, modifiers]) => s.reservedOf({ key, modifiers }, true)),
        mac: keys.map(([key, modifiers]) => s.reservedOf({ key, modifiers }, false)),
        event: s.reservedOf(fromEvent, true),
      };
    });
    assert.ok(r.win.every(Boolean), `Windows 예약 ${JSON.stringify(r.win)}`);
    assert.ok(r.mac.every((x) => x === null), `맥 ${JSON.stringify(r.mac)}`);
    assert.equal(r.event, '화면 실제 크기');
  } finally { await close(); }
});

await check('MH11 빠른 연속 변경 중 메아리 알림이 화면 크기 슬라이더를 되돌리지 않는다', async () => {
  const { page, errors, close } = await open({ bus: true });
  try {
    // 실제 앱처럼 settings_update(외관)는 늦게 끝나고, 끝나기 전에 모든 창(저장한 창 포함)에 알림을 보낸다
    await page.evaluate(() => {
      const core = window.__TAURI__.core;
      const orig = core.invoke;
      let appearance = { theme: 'dark', uiScale: 1 };
      core.invoke = async (cmd, args) => {
        if (cmd === 'settings_get') { const s = await orig(cmd, args); return { ...s, appearance: { ...appearance } }; }
        if (cmd !== 'settings_update' || !args || args.domain !== 'appearance') return orig(cmd, args);
        appearance = { ...appearance, ...args.patch };
        const echo = { ...appearance };
        await new Promise((r) => setTimeout(r, 120));
        window.__emit('settings://appearance', echo);
        await new Promise((r) => setTimeout(r, 120));
        const s = await orig('settings_get', {});
        return { ...s, appearance: echo };
      };
    });
    await go(page, 'settings', {});
    await page.waitForSelector('.page[data-page="settings"]:not([hidden])');
    await page.fill('.settings-search', '화면 크기');
    const range = page.locator('input[type="range"][data-domain="appearance"][data-key="uiScale"]');
    await range.waitFor({ state: 'attached' });
    await range.evaluate((el) => { el.value = '1'; });
    await range.focus();
    for (let i = 0; i < 3; i++) { await page.keyboard.press('ArrowRight'); await page.waitForTimeout(60); }
    const seen = [];
    for (let i = 0; i < 12; i++) { seen.push(await range.inputValue()); await page.waitForTimeout(120); }
    assert.ok(seen.every((v) => v === '1.15'), `값 흐름 ${seen.join(' ')}`);
    assert.equal(await range.evaluate((el) => el.parentElement.querySelector('output').textContent), '115%');

    // 드래그 중 다른 곳(⌘0)에서 바뀌면 엄지를 건드리지 않고, 놓은 뒤에 맞춘다
    const box = await range.boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.evaluate(() => window.__emit('settings://appearance', { theme: 'dark', uiScale: 1.5 }));
    await page.waitForTimeout(900);
    const during = await range.inputValue();
    assert.notEqual(during, '1.5', '드래그 중에 덮었다');
    await page.mouse.up();
    // 놓으면 change 로 드래그 값이 저장된다(미뤄 둔 1.5 가 드래그 값을 밀어내지 않는다) — 화면과 저장이 같은 드래그 값
    await page.waitForTimeout(1500);
    const after = await range.inputValue();
    const saved = await page.evaluate(async () => (await window.__TAURI__.core.invoke('settings_get', {})).appearance.uiScale);
    assert.equal(Number(after), Number(saved), `화면 ${after} · 저장 ${saved}`);
    assert.equal(after, during, `드래그 값 ${during} → ${after}`);
    assert.deepEqual(errors, []);
  } finally { await close(); }
});

server.close();
for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : `: ${r.error}`}`);
if (wideReport) console.log('넓은 창 측정', JSON.stringify(wideReport));
const failed = results.filter((r) => !r.ok).length;
console.log(failed ? `${failed} failed` : 'all passed');
process.exit(failed ? 1 : 0);
