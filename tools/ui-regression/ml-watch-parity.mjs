// 라이브 단독 보기(ml-watch) 고급 기능 패리티 회귀 검사 — 예전 시청 화면 고급 패널(PlayerPanel)에만 있던 재생 도구가
// 단독 보기 무대 패인의 ⚙ 에도 있는지, 고르면 그 패인 플레이어에 맞는 호출이 가는지, 저장 값이 다시 걸리는지 본다.
// 모의 IPC·헤드리스 Chromium(실제 서비스·데이터는 건드리지 않는다). 플레이어 호출은 PlayerBridge.call 을 감싸
// "플레이어 문서가 준비돼 실제로 전달된 호출"만 기록한다(준비 전 호출은 플레이어에 닿지 않으므로 세지 않는다).
// W01 단독 보기 ⚙ 에 배속·화면 꽉 채우기·자동 화질 보정·업스케일·픽셀 그대로 확대가 있다(그리드 패인 ⚙ 에 없는 것은 W04).
// W02 배속을 고르면 무대 패인에 setRate·OSD, 메뉴에 고른 배속. LIVE(L)·무대에서 내려오면 1배속으로.
// W03 화면 꽉 채우기·자동 보정·업스케일·픽셀 확대 — 무대 패인에 setForcedAspectRatio·setPictureEnhancement·setSharpPixelScaling,
//     플레이어 설정(player)에 저장. 영상 object-fit 이 실제로 fill.
// W04 저장 값이 다시 걸린다 — 단독 보기에서 나오면 되돌리고, 다른 패인으로 다시 열면·새 패인(새 iframe)이 무대가 되면·
//     새 재생(playStarted)마다 다시 건다. 그리드 패인에는 걸지 않는다.
// W05 이퀄라이저 — macOS(WKWebView)에서는 숨고 호출도 없다. Windows 흉내(UA)에서는 보이고 켜면 setEqualizerEnabled·밴드 10개·저장.
// W06 키보드 — 새 메뉴 항목 위의 M·L·←·B·Space 는 플레이어 단축키로 새지 않는다. ↑↓ 로 항목 사이를 돌고, 다시 그려도
//     초점이 같은 항목에 남고, Esc 는 메뉴를 닫고 ⚙ 로 초점을 돌려준다.
// 저장소 루트에서: node tools/ui-regression/ml-watch-parity.mjs   (Playwright 가 필요하다)
//   CVIEW_UI_SHOTS=<폴더> 면 단독 보기 ⚙ 를 연 장면을 PNG 로 남긴다(검사 전에 찍으므로 고치기 전 판에서도 찍힌다).
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, base } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');
const SHOTS = process.env.CVIEW_UI_SHOTS;
const WINDOWS_UA = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0';
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}
const browser = await chromium.launch();

// ── 멀티라이브(단독 보기) 모의 — 플레이어 설정 저장·상태 이벤트를 흉내 낸다 ──
function mlMock([watchId]) {
  const names = [['c1', '한동숙'], ['c2', '침착맨'], ['c3', '풍월량'], ['c4', '릴카'], ['c5', '녹두로']];
  const session = ([id, name], i) => ({
    id: `s${i}`, slot: i, channelId: id, channelName: name, liveTitle: '오늘의 방송', viewerCountText: '1,234', openDate: new Date(Date.now() - 3600e3).toISOString(),
    isMuted: i > 0, isAudible: i === 0, state: { kind: 'playing' }, chatChannelId: null,
  });
  const state = {
    sessions: names.slice(0, 4).map(session), selectedId: 's0', focusMainId: 's0', canAdd: true, maxSessions: 5, layout: 'grid', focusDirection: 'left',
    focusRatio: 0.7, horizontalRatio: 0.5, verticalRatio: 0.5, multiAudio: false, watchId, rev: 1,
  };
  const store = { player: {} };
  const presets = [{ name: '평탄', gains: new Array(10).fill(0), preAmp: 0 }, { name: '저음 강조', gains: [6, 5, 4, 2, 0, 0, 0, 0, 0, 0], preAmp: -2 }];
  window.__saved = [];
  const handlers = {
    auth_status: () => ({ loggedIn: false, state: 'loggedOut' }),
    settings_get: () => ({ multiLive: {}, multiChat: {}, chat: { displayMode: 'side' }, player: { ...store.player } }),
    settings_update: ({ domain, patch }) => {
      window.__saved.push({ domain, patch });
      if (domain === 'player') store.player = { ...store.player, ...patch };
      return { multiLive: {}, multiChat: {}, chat: {}, player: { ...store.player } };
    },
    equalizer_presets: () => [[31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000], presets],
    multilive_state: () => state, popular_lives: () => [], following_lives: () => [], multilive_stage_size: () => null,
    channel_info: () => ({ verified: false, followerCount: 1000 }), live_detail: () => ({ liveTitle: '오늘의 방송', categoryName: '토크' }),
    channel_detail: ({ channelId }) => ({
      channel: { channelId, channelName: (names.find((n) => n[0] === channelId) || names[0])[1], channelImageUrl: null, verifiedMark: false, followerCount: 1000, channelDescription: '매일 저녁 방송합니다.' },
      live: { liveTitle: '오늘의 방송', categoryName: '토크', viewerCount: 1234, viewerCountText: '1,234', openDate: new Date(Date.now() - 3600e3).toISOString(), tags: ['소통'] },
    }),
    bookmark_add: () => { window.__bookmarks = (window.__bookmarks || 0) + 1; return null; },
  };
  const listeners = {};
  window.__ml = {
    state, store, session: (i) => session(names[i], i),
    // Rust 가 보내는 multilive://state 이벤트 흉내
    emit() { state.rev += 1; for (const cb of listeners['multilive://state'] || []) cb({ payload: JSON.parse(JSON.stringify(state)) }); },
  };
  window.__TAURI__ = {
    core: { invoke: async (c, a) => { if (c === 'ui_heartbeat') return null; const h = handlers[c]; if (!h) throw new Error(`mock: ${c}`); return h(a || {}); } },
    event: { listen: async (name, cb) => { (listeners[name] ||= []).push(cb); return () => {}; } },
  };
  // 환영 안내를 띄우지 않는다 — 띄운 뒤 지우면 그 Esc 처리기(잡는 단계)가 남아 첫 Esc 를 먹는다.
  try { localStorage.setItem('cview.onboarded', '1'); localStorage.setItem('cview.welcomed.v1', String(Date.now())); } catch { /* 무시 */ }
}

async function openMl({ watchId = 's0', userAgent } = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 }, ...(userAgent ? { userAgent } : {}) });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mlMock, [watchId]);
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
  // 플레이어 호출 기록 — 준비된 플레이어 문서에 실제로 전달된 것만(슬롯별)
  await page.evaluate(async () => {
    const { PlayerBridge } = await import('/js/player.js');
    window.__pcalls = [];
    const call = PlayerBridge.prototype.call;
    PlayerBridge.prototype.call = function (name, ...args) {
      const win = this.iframe.contentWindow;
      if (this.ready && win && typeof win[name] === 'function') {
        const slot = this.iframe.closest('.ml-pane')?.dataset.slot;
        window.__pcalls.push({ slot: slot == null ? null : Number(slot), name, args: JSON.parse(JSON.stringify(args)) });
      }
      return call.call(this, name, ...args);
    };
  });
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: 'multilive', params: {} } })));
  await page.waitForSelector('.ml-pane:not([hidden])');
  await waitPlayers(page);
  await page.waitForTimeout(500); // 무대 크기 재기·설정 읽기가 끝나게
  return { ctx, page, errors };
}

/** 보이는 패인의 플레이어 문서가 모두 준비될 때까지. */
const waitPlayers = (page) => page.waitForFunction(() => [...document.querySelectorAll('.ml-pane')].filter((p) => !p.hidden).every((p) => {
  const f = p.querySelector('iframe');
  return f && f.contentDocument?.readyState === 'complete' && typeof f.contentWindow?.setRate === 'function';
}));
const pane = (slot) => `.ml-pane[data-slot="${slot}"]`;
const calls = (page, slot, name) => page.evaluate(([s, n]) => window.__pcalls.filter((c) => (s == null || c.slot === s) && (!n || c.name === n)), [slot, name]);
const lastCall = async (page, slot, name) => (await calls(page, slot, name)).at(-1) || null;
const clearCalls = (page) => page.evaluate(() => { window.__pcalls.length = 0; window.__saved.length = 0; });
async function openGear(page, slot) {
  await page.evaluate((sel) => {
    const p = document.querySelector(sel);
    p.classList.add('ctl-visible');
    const menu = p.querySelector('.ml-gear-menu');
    if (menu.hidden) p.querySelector('[data-mp="gear"]').click();
  }, pane(slot));
  await page.waitForTimeout(150); // 설정 다시 읽기
}
const closeGear = (page, slot) => page.evaluate((sel) => { const p = document.querySelector(sel); if (!p.querySelector('.ml-gear-menu').hidden) p.querySelector('[data-mp="gear"]').click(); }, pane(slot));
const menuInfo = (page, slot) => page.evaluate((sel) => {
  const m = document.querySelector(`${sel} .ml-gear-menu`);
  const sw = (key) => { const b = m.querySelector(`[data-key="${key}"]`); return b ? b.getAttribute('aria-checked') === 'true' : null; };
  return {
    rates: [...m.querySelectorAll('[data-mp="rate"]')].map((b) => b.dataset.rate),
    activeRate: m.querySelector('[data-mp="rate"].active')?.dataset.rate || null,
    aspect: sw('aspectRatio'), auto: sw('autoPictureEnhancement'), upscale: sw('videoUpscaling'), pixel: sw('sharpPixelScaling'),
    eq: m.querySelector('[data-mp="eq"]') ? m.querySelector('[data-mp="eq"]').getAttribute('aria-checked') === 'true' : null,
    bands: m.querySelectorAll('[data-eq-band]').length, preset: m.querySelector('[data-eq-preset]')?.value ?? null,
    text: m.textContent.replace(/\s+/g, ' '),
  };
}, pane(slot));
// 메뉴는 무대 높이에서 잘려 스크롤된다 — 먼저 메뉴 안에서 보이게 둔 뒤 실제 마우스로 누른다.
const clickItem = async (page, slot, sel) => {
  const full = `${pane(slot)} .ml-gear-menu ${sel}`;
  await page.evaluate((s) => { const el = document.querySelector(s); const m = el.closest('.ml-gear-menu'); m.scrollTop = Math.max(0, el.offsetTop - m.clientHeight / 2); }, full);
  await page.click(full);
  await page.waitForTimeout(80);
};
const objectFit = (page, slot) => page.evaluate((sel) => getComputedStyle(document.querySelector(`${sel} iframe`).contentDocument.getElementById('video')).objectFit, pane(slot));
const shot = async (page, name) => {
  if (!SHOTS) return;
  // 가짜 영상 — 검은 무대 대신 그림을 깔아 메뉴가 영상 위에 뜬 모습을 보이게 한다.
  await page.evaluate(() => {
    for (const p of document.querySelectorAll('.ml-pane:not([hidden])')) {
      let v = p.querySelector('.fake-video');
      if (!v) { v = document.createElement('div'); v.className = 'fake-video'; p.prepend(v); }
      Object.assign(v.style, { position: 'absolute', inset: '0', background: 'linear-gradient(135deg, hsl(210 60% 42%), hsl(265 55% 22%))', pointerEvents: 'none', zIndex: '1' });
    }
  });
  fs.mkdirSync(SHOTS, { recursive: true });
  await page.screenshot({ path: path.join(SHOTS, `${name}.png`) });
};

// ── W01~W04, W06: macOS(기본 UA) ──
{
  const { ctx, page, errors } = await openMl({ watchId: 's0' });

  await check('W01 watch gear lists rate, aspect, auto/upscale/pixel (grid gear: W04)', async () => {
    // 사진은 큰 창에서 — 메뉴가 무대 높이에서 잘리지 않고 한 번에 보이게
    if (SHOTS) { await page.setViewportSize({ width: 1680, height: 1050 }); await page.waitForTimeout(500); }
    await openGear(page, 0);
    await shot(page, 'watch-gear');
    const m = await menuInfo(page, 0);
    assert.deepEqual(m.rates, ['0.5', '0.75', '1', '1.25', '1.5', '2'], `배속 항목 ${JSON.stringify(m.rates)}`);
    assert.equal(m.activeRate, '1', '처음엔 1배속');
    assert.equal(m.aspect, false, '화면 꽉 채우기 스위치가 없다');
    assert.equal(m.auto, false, '자동 화질 보정 스위치가 없다');
    assert.equal(m.upscale, false, '업스케일 스위치가 없다');
    assert.equal(m.pixel, false, '픽셀 그대로 확대 스위치가 없다');
    assert.match(m.text, /화질/, '기존 화질 항목이 사라졌다');
    assert.match(m.text, /선명한 화면/, '기존 화면 보정 항목이 사라졌다');
    await closeGear(page, 0);
    if (SHOTS) { await page.setViewportSize({ width: 1280, height: 800 }); await page.waitForTimeout(500); }
  });

  await check('W02 rate goes to the stage pane (setRate + OSD); L / leaving watch resets to 1x', async () => {
    await clearCalls(page);
    await openGear(page, 0);
    await clickItem(page, 0, '[data-mp="rate"][data-rate="1.5"]');
    assert.deepEqual((await lastCall(page, 0, 'setRate'))?.args, [1.5], 'setRate(1.5) 가 무대 패인에 가지 않았다');
    assert.equal(await page.evaluate(() => document.querySelector('.player-osd:not([hidden])')?.textContent || ''), '1.5배속');
    assert.equal(await page.evaluate((sel) => document.querySelector(`${sel} iframe`).contentDocument.getElementById('video').playbackRate, pane(0)), 1.5, '영상 배속이 바뀌지 않았다');
    assert.deepEqual((await calls(page, null, 'setRate')).filter((c) => c.slot !== 0), [], '다른 패인에 배속이 갔다');
    await openGear(page, 0);
    assert.equal((await menuInfo(page, 0)).activeRate, '1.5', '메뉴에 고른 배속이 켜져 보이지 않는다');
    await closeGear(page, 0);
    // L(라이브로) — 1배속으로
    await page.evaluate(() => document.activeElement?.blur());
    await page.keyboard.press('l');
    assert.deepEqual((await lastCall(page, 0, 'setRate'))?.args, [1], 'L 이 1배속으로 되돌리지 않았다');
    await openGear(page, 0);
    assert.equal((await menuInfo(page, 0)).activeRate, '1', 'L 뒤 메뉴가 1배속이 아니다');
    // 새 재생(playStarted)은 1배속 — 2배속으로 둔 뒤 새 재생
    await clickItem(page, 0, '[data-mp="rate"][data-rate="2"]');
    await page.evaluate((sel) => window.__cviewPlayerBridge({ type: 'playStarted', data: {} }, document.querySelector(`${sel} iframe`).contentWindow), pane(0));
    assert.deepEqual((await lastCall(page, 0, 'setRate'))?.args, [1], '새 재생이 1배속으로 돌아가지 않았다');
    // 무대에서 내려오면(그리드로) 1배속
    await openGear(page, 0);
    await clickItem(page, 0, '[data-mp="rate"][data-rate="1.25"]');
    await page.evaluate(() => { window.__ml.state.watchId = null; window.__ml.emit(); });
    await page.waitForTimeout(200);
    assert.deepEqual((await lastCall(page, 0, 'setRate'))?.args, [1], '단독 보기를 나왔는데 배속이 남았다');
    await page.evaluate(() => { window.__ml.state.watchId = 's0'; window.__ml.emit(); });
    await page.waitForTimeout(300);
  });

  await check('W03 aspect/auto/upscale/pixel apply to the stage pane and save to player settings', async () => {
    await clearCalls(page);
    await openGear(page, 0);
    await clickItem(page, 0, '[data-key="aspectRatio"]');
    assert.deepEqual((await lastCall(page, 0, 'setForcedAspectRatio'))?.args, [true]);
    assert.equal(await objectFit(page, 0), 'fill', '영상이 꽉 채워지지 않았다');
    await clickItem(page, 0, '[data-key="autoPictureEnhancement"]');
    await clickItem(page, 0, '[data-key="videoUpscaling"]');
    assert.deepEqual((await lastCall(page, 0, 'setPictureEnhancement'))?.args, [{ auto: true, upscale: true }]);
    await clickItem(page, 0, '[data-key="sharpPixelScaling"]');
    assert.deepEqual((await lastCall(page, 0, 'setSharpPixelScaling'))?.args, [true]);
    const m = await menuInfo(page, 0);
    assert.deepEqual([m.aspect, m.auto, m.upscale, m.pixel], [true, true, true, true], '스위치가 켜져 보이지 않는다');
    assert.equal(await page.evaluate(() => !document.querySelector('.ml-pane[data-slot="0"] .ml-gear-menu').hidden), true, '스위치를 누르면 메뉴가 닫혔다');
    const store = await page.evaluate(() => window.__ml.store.player);
    assert.deepEqual(store, { aspectRatio: 'fill', autoPictureEnhancement: true, videoUpscaling: true, sharpPixelScaling: true }, `저장 ${JSON.stringify(store)}`);
    assert.deepEqual((await calls(page)).filter((c) => c.slot !== 0 && c.name !== 'getLevels'), [], '다른 패인에 호출이 갔다');
    await closeGear(page, 0);
  });

  await check('W04 saved tools re-apply: leave watch reverts; another pane / new pane / playStarted re-applies', async () => {
    await clearCalls(page);
    await page.evaluate(() => { window.__ml.state.watchId = null; window.__ml.emit(); });
    await page.waitForTimeout(250);
    assert.deepEqual((await lastCall(page, 0, 'setForcedAspectRatio'))?.args, [false], '그리드로 돌아와도 꽉 채우기가 남았다');
    assert.deepEqual((await lastCall(page, 0, 'setPictureEnhancement'))?.args, [{ auto: false, upscale: false }], '그리드 패인에 업스케일이 남았다');
    assert.equal(await objectFit(page, 0), 'contain');
    // 그리드 패인 ⚙ 에는 새 항목이 없다
    await openGear(page, 1);
    const g = await menuInfo(page, 1);
    assert.deepEqual([g.rates.length, g.aspect, g.auto], [0, null, null], `그리드 패인 ⚙ 에 단독 보기 항목이 있다 ${JSON.stringify(g)}`);
    await closeGear(page, 1);
    // 다른 패인으로 단독 보기 — 저장 값이 그 패인에 걸린다
    await clearCalls(page);
    await page.evaluate(() => { window.__ml.state.watchId = 's1'; window.__ml.emit(); });
    await page.waitForTimeout(300);
    assert.deepEqual((await lastCall(page, 1, 'setForcedAspectRatio'))?.args, [true], '다시 연 단독 보기에 꽉 채우기가 걸리지 않았다');
    assert.deepEqual((await lastCall(page, 1, 'setPictureEnhancement'))?.args, [{ auto: true, upscale: true }]);
    assert.deepEqual((await lastCall(page, 1, 'setSharpPixelScaling'))?.args, [true]);
    assert.deepEqual((await calls(page)).filter((c) => c.slot !== 1 && ['setForcedAspectRatio', 'setPictureEnhancement'].includes(c.name) && c.args[0] !== false && c.args[0]?.upscale !== false), [], '무대가 아닌 패인에 도구가 걸렸다');
    await openGear(page, 1);
    const m = await menuInfo(page, 1);
    assert.deepEqual([m.aspect, m.auto, m.upscale, m.pixel], [true, true, true, true], '다시 연 단독 보기 메뉴가 저장 값을 보이지 않는다');
    await closeGear(page, 1);
    // 새 재생(playStarted) — 다시 건다
    await clearCalls(page);
    await page.evaluate((sel) => window.__cviewPlayerBridge({ type: 'playStarted', data: {} }, document.querySelector(`${sel} iframe`).contentWindow), pane(1));
    assert.deepEqual((await lastCall(page, 1, 'setForcedAspectRatio'))?.args, [true], '새 재생에 다시 걸지 않았다');
    // 새 패인(새 iframe)이 무대 — 플레이어 문서가 준비되면 건다
    await clearCalls(page);
    await page.evaluate(() => { const s = window.__ml.session(4); window.__ml.state.sessions.push(s); window.__ml.state.watchId = s.id; window.__ml.emit(); });
    await page.waitForSelector(`${pane(4)}:not([hidden])`);
    await waitPlayers(page);
    await page.waitForFunction(() => window.__pcalls.some((c) => c.slot === 4 && c.name === 'setForcedAspectRatio'), null, { timeout: 5000 }).catch(() => {});
    assert.deepEqual((await lastCall(page, 4, 'setForcedAspectRatio'))?.args, [true], '새 패인에 꽉 채우기가 걸리지 않았다');
    assert.deepEqual((await lastCall(page, 4, 'setPictureEnhancement'))?.args, [{ auto: true, upscale: true }], '새 패인에 업스케일이 걸리지 않았다');
    assert.equal(await objectFit(page, 4), 'fill');
    assert.deepEqual((await lastCall(page, 1, 'setForcedAspectRatio'))?.args, [false], '앞 무대 패인을 되돌리지 않았다');
    // macOS — 이퀄라이저는 메뉴에도 없고 호출도 없다
    await openGear(page, 4);
    assert.equal((await menuInfo(page, 4)).eq, null, 'macOS 에서 이퀄라이저가 보인다');
    await closeGear(page, 4);
    assert.deepEqual(await calls(page, null, 'setEqualizerEnabled'), [], 'macOS 에서 이퀄라이저를 켰다');
    // 단독 보기를 s0 으로 되돌린다
    await page.evaluate(() => { window.__ml.state.sessions.pop(); window.__ml.state.watchId = 's0'; window.__ml.emit(); });
    await page.waitForTimeout(300);
  });

  await check('W06 keys on new menu items stay out of player shortcuts; arrows/Esc/re-render keep focus sane', async () => {
    await clearCalls(page);
    await openGear(page, 0);
    await page.evaluate((sel) => document.querySelector(`${sel} .ml-gear-menu [data-mp="rate"][data-rate="1.25"]`).focus(), pane(0));
    const before = await page.evaluate((sel) => ({ gain: document.querySelector(`${sel} .ml-vol-range`).value, bookmarks: window.__bookmarks || 0 }), pane(0));
    for (const k of ['m', 'l', 'ArrowLeft', 'b']) await page.keyboard.press(k);
    const after = await page.evaluate((sel) => ({ gain: document.querySelector(`${sel} .ml-vol-range`).value, bookmarks: window.__bookmarks || 0, osd: document.querySelector('.player-osd:not([hidden])')?.textContent || '' }), pane(0));
    assert.equal(after.gain, before.gain, 'M 이 음소거로 샜다');
    assert.equal(after.bookmarks, before.bookmarks, 'B 가 북마크로 샜다');
    assert.ok(!/라이브|음소거|되감|뒤/.test(after.osd), `플레이어 단축키가 돌았다: ${after.osd}`);
    assert.deepEqual((await calls(page, 0)).filter((c) => ['seek', 'seekToLiveEdge', 'setRate'].includes(c.name)), [], '메뉴 위 키가 되감기·라이브로·배속으로 샜다');
    // ↓ 는 다음 항목으로
    await page.keyboard.press('ArrowDown');
    assert.equal(await page.evaluate(() => document.activeElement?.dataset.rate || ''), '1.5', '↓ 가 다음 배속 항목으로 가지 않았다');
    // 메뉴가 다시 그려져도(늦게 온 화질 목록) 초점이 같은 항목에
    await page.evaluate((sel) => window.__cviewPlayerBridge({ type: 'levels', data: { levels: [{ index: 0, height: 720, width: 1280, bitrate: 2 }], current: 0, manual: -1 } }, document.querySelector(`${sel} iframe`).contentWindow), pane(0));
    assert.equal(await page.evaluate(() => document.activeElement?.dataset.rate || ''), '1.5', '다시 그린 뒤 초점이 떨어졌다');
    // 꽉 채우기 스위치에서 Space 는 스위치를 누르고(재생/일시정지로 새지 않는다)
    await page.evaluate((sel) => document.querySelector(`${sel} .ml-gear-menu [data-key="aspectRatio"]`).focus(), pane(0));
    await clearCalls(page);
    await page.keyboard.press('Space');
    await page.waitForTimeout(100);
    assert.deepEqual((await calls(page, 0)).filter((c) => c.name === 'play' || c.name === 'pause'), [], 'Space 가 재생/일시정지로 샜다');
    assert.ok((await calls(page, 0, 'setForcedAspectRatio')).length > 0, 'Space 로 스위치가 눌리지 않았다');
    assert.equal(await page.evaluate(() => document.activeElement?.dataset.key || ''), 'aspectRatio', '스위치를 누른 뒤 초점이 떨어졌다');
    // Esc — 메뉴를 닫고 ⚙ 로
    await page.keyboard.press('Escape');
    await page.waitForTimeout(100);
    const esc = await page.evaluate((sel) => ({ open: !document.querySelector(`${sel} .ml-gear-menu`).hidden, focus: document.activeElement?.dataset.mp || '', watch: document.querySelector('.page-multilive').classList.contains('as-watch') }), pane(0));
    assert.deepEqual(esc, { open: false, focus: 'gear', watch: true }, `Esc ${JSON.stringify(esc)}`);
  });

  if (errors.length) results.push({ id: 'page errors (macOS)', ok: false, error: errors.join('\n') });
  await ctx.close();
}

// ── W05: Windows 흉내 — 이퀄라이저 ──
await check('W05 Windows: EQ shows in watch gear; toggling applies + saves; bands/preset follow', async () => {
  const { ctx, page, errors } = await openMl({ watchId: 's0', userAgent: WINDOWS_UA });
  assert.equal(await page.evaluate(() => document.documentElement.dataset.platform), 'windows');
  await clearCalls(page);
  await openGear(page, 0);
  let m = await menuInfo(page, 0);
  assert.equal(m.eq, false, 'Windows 에서 이퀄라이저 스위치가 없다');
  assert.equal(m.bands, 0, '꺼져 있을 때 밴드가 보인다');
  await clickItem(page, 0, '[data-mp="eq"]');
  const on = await lastCall(page, 0, 'setEqualizerEnabled');
  assert.deepEqual(on?.args, [true, new Array(10).fill(0), 0], `켜기 호출 ${JSON.stringify(on)}`);
  m = await menuInfo(page, 0);
  assert.deepEqual([m.eq, m.bands, m.preset], [true, 10, '평탄'], `켠 뒤 메뉴 ${JSON.stringify(m)}`);
  assert.equal((await page.evaluate(() => window.__ml.store.player)).equalizerPreset, '평탄', '이퀄라이저를 저장하지 않았다');
  if (SHOTS) {
    await page.setViewportSize({ width: 1680, height: 1050 });
    await page.waitForTimeout(500);
    await page.evaluate((sel) => { const m = document.querySelector(`${sel} .ml-gear-menu`); m.scrollTop = m.scrollHeight; }, pane(0));
    await shot(page, 'watch-gear-windows-eq');
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.waitForTimeout(300);
  }
  // 프리셋 바꾸기 — 밴드·프리앰프가 그 값으로
  await page.selectOption(`${pane(0)} [data-eq-preset]`, '저음 강조');
  await page.waitForTimeout(100);
  assert.deepEqual((await lastCall(page, 0, 'setEqualizerEnabled'))?.args, [true, [6, 5, 4, 2, 0, 0, 0, 0, 0, 0], -2]);
  // 밴드 슬라이더 — 끄는 동안 setEqualizerBand, 놓으면 '사용자 지정'으로 저장. 슬라이더 위 ← 는 되감기로 새지 않는다.
  await page.focus(`${pane(0)} [data-eq-band="9"]`);
  await page.keyboard.press('ArrowUp');
  await page.keyboard.press('ArrowLeft');
  assert.ok((await calls(page, 0, 'setEqualizerBand')).length > 0, '밴드 조절이 플레이어에 가지 않았다');
  assert.deepEqual((await calls(page, 0)).filter((c) => c.name === 'seek'), [], '슬라이더 위 ← 가 되감기로 샜다');
  await page.evaluate((sel) => document.querySelector(`${sel} [data-eq-band="9"]`).dispatchEvent(new Event('change', { bubbles: true })), pane(0));
  await page.waitForTimeout(100);
  assert.equal((await page.evaluate(() => window.__ml.store.player)).equalizerPreset, '사용자 지정');
  // 그리드로 나오면 이퀄라이저를 평탄하게 끄고, 다시 무대에 오르면 다시 켠다
  await clearCalls(page);
  await page.evaluate(() => { window.__ml.state.watchId = null; window.__ml.emit(); });
  await page.waitForTimeout(250);
  assert.deepEqual((await lastCall(page, 0, 'setEqualizerEnabled'))?.args, [false], '그리드로 나와도 이퀄라이저가 남았다');
  await page.evaluate(() => { window.__ml.state.watchId = 's0'; window.__ml.emit(); });
  await page.waitForTimeout(300);
  assert.equal((await lastCall(page, 0, 'setEqualizerEnabled'))?.args?.[0], true, '다시 연 단독 보기에 이퀄라이저가 걸리지 않았다');
  // 끄기
  await openGear(page, 0);
  await clickItem(page, 0, '[data-mp="eq"]');
  assert.deepEqual((await lastCall(page, 0, 'setEqualizerEnabled'))?.args, [false]);
  assert.equal((await page.evaluate(() => window.__ml.store.player)).equalizerPreset, null);
  assert.deepEqual(errors, []);
  await ctx.close();
});

await browser.close();
server.close();
for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : ` — ${r.error}`}`);
const failed = results.filter((r) => !r.ok).length;
console.log(failed ? `${failed} failed` : `all ${results.length} passed`);
process.exit(failed ? 1 : 0);
