// 선명도 통합 회귀 검사 — 예전 '선명한 화면'(채널별 언샤프)과 '업스케일'(전역 GPU 확대)을 '선명도' 하나(끔·약·중·강 +
// '크게 볼 때만 적용')로 합친 뒤, 모든 진입점에서 같은 이름·같은 동작인지, 단계마다 플레이어가 실제로 무엇을 거는지,
// 예전 설정이 알맞은 값으로 옮겨지고 되돌려도 깨지지 않는지 본다. 모의 IPC·헤드리스 Chromium(실제 서비스는 건드리지 않는다).
// S01 진입점마다 통합 항목 하나 — 멀티라이브 그리드 패인 ⚙ · 단독 보기 무대 ⚙ · 시청 화면 고급 패널 '화면' 탭 · 설정 화면.
//     같은 이름 '선명도', 같은 단계(끔·약·중·강), 같은 보조 스위치. 예전 '선명한 화면'·'업스케일'·'모든 패인에 선명하게'는 없다.
// S02 단계별 실제 처리 — 패인 플레이어 문서(player.html)에 캔버스 영상(captureStream)을 넣고, 단계마다 경로·세기·그리기 횟수·
//     실제 화소의 경계 세기를 잰다. 무대(확대): 약 = Catmull-Rom 1패스, 중 = CAS 0.55 2패스(예전 업스케일), 강 = CAS + 언샤프 0.5
//     4패스. 그리드: 언샤프 2패스(세기 0.5·1·1.5), 경계 세기가 끔 < 약 < 중 < 강. 끔은 GPU 캔버스·필터 없음.
//     GPU 를 쓸 수 없으면 SVG 언샤프(url(#cview-unsharp)) k2 = 세기. '크게 볼 때만'이면 그리드는 원본 그대로.
// S03 전역 동작 — 한 패인 ⚙ 에서 바꾸면 모든 패인에 바로 setSharpness(무대만 upscale:true), 저장(sharpnessLevel·구버전
//     거울 videoUpscaling), 메뉴는 열린 채. 무대에서 내려오면 upscale:false, 새 재생(playStarted)·새 플레이어 문서에 다시 건다.
// S04 설정 이전 — 예전 조합(채널별 선명한 화면 켬·끔 수 × 처음 보는 채널 기본값 × 업스케일) 전수표를 순수 함수로,
//     대표 조합을 첫 실행으로 확인한다. 예전 키는 건드리지 않고, 켜져 있었으면 한 번 안내한다.
// S05 되돌리기 — 새 판에서 바꾸면 구버전 거울(videoUpscaling·__default.sharpen·스냅숏, 채널 값은 이름만 바꿔 보존)을 쓰고,
//     구버전이 새 키를 지워도 스냅숏으로 같은 단계(약·강 포함), 구버전에서 바꿨으면 그 선택, 초기화(0)는 거울도 끔.
// S06 상태 줄·⚙ title — 플레이어의 'sharpness' 알림으로 '업스케일 360p → 560p' 같은 지금 처리를 메뉴를 다시 그리지 않고 보인다.
// S07 설정 화면 — '선명도' 고르기·'크게 볼 때만 적용', 검색 '업스케일'·'선명'으로 찾힘, 바꾸면 저장·열린 패인에 바로.
// 저장소 루트에서: node tools/ui-regression/sharpness.mjs   (Playwright 가 필요하다)
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, base } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}
const browser = await chromium.launch();

// ── 멀티라이브 모의(ml-watch-parity.mjs 와 같은 꼴) + 설정 화면에 필요한 명령 ──
function mlMock({ watchId, player, picture }) {
  const names = [['c1', '한동숙'], ['c2', '침착맨'], ['c3', '풍월량'], ['c4', '릴카'], ['c5', '녹두로']];
  const session = ([id, name], i) => ({
    id: `s${i}`, slot: i, channelId: id, channelName: name, liveTitle: '오늘의 방송', viewerCountText: '1,234', openDate: new Date(Date.now() - 3600e3).toISOString(),
    isMuted: i > 0, isAudible: i === 0, state: { kind: 'playing' }, chatChannelId: null,
  });
  const state = {
    sessions: names.slice(0, 4).map(session), selectedId: 's0', focusMainId: 's0', canAdd: true, maxSessions: 5, layout: 'grid', focusDirection: 'left',
    focusRatio: 0.7, horizontalRatio: 0.5, verticalRatio: 0.5, multiAudio: false, watchId, rev: 1,
  };
  const store = { player: { ...(player || {}) } };
  window.__saved = [];
  const settings = () => ({ multiLive: {}, multiChat: {}, chat: { displayMode: 'side' }, general: {}, appearance: {}, network: {}, keyboard: {}, player: { ...store.player } });
  const handlers = {
    auth_status: () => ({ loggedIn: false, state: 'loggedOut' }),
    settings_get: settings,
    settings_update: ({ domain, patch }) => {
      window.__saved.push({ domain, patch });
      if (domain === 'player') {
        store.player = { ...store.player, ...patch };
        // Rust 와 같게 — null 은 키 없음(다시 이전), 숫자는 0..3
        if (store.player.sharpnessLevel === null) delete store.player.sharpnessLevel;
        else if (typeof store.player.sharpnessLevel === 'number') store.player.sharpnessLevel = Math.max(0, Math.min(3, Math.round(store.player.sharpnessLevel)));
      }
      return settings();
    },
    equalizer_presets: () => [[31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000], []],
    multilive_state: () => state, popular_lives: () => [], following_lives: () => [], multilive_stage_size: () => null,
    channel_info: () => ({ verified: false, followerCount: 1000 }), live_detail: () => ({ liveTitle: '오늘의 방송', categoryName: '토크' }),
    channel_detail: ({ channelId }) => ({
      channel: { channelId, channelName: (names.find((n) => n[0] === channelId) || names[0])[1], channelImageUrl: null, verifiedMark: false, followerCount: 1000, channelDescription: '' },
      live: { liveTitle: '오늘의 방송', categoryName: '토크', viewerCount: 1234, viewerCountText: '1,234', openDate: new Date(Date.now() - 3600e3).toISOString(), tags: [] },
    }),
    app_info: () => ({ version: '0.1.97', build: 97, os: 'macos', arch: 'aarch64' }),
    data_health: () => ({ available: true, storeBytes: 1000, freeBytes: 1e9, backups: [], tables: [], backupDir: '/tmp/b', storePath: '/tmp/s' }),
  };
  const listeners = {};
  window.__ml = {
    state, store,
    emit() { state.rev += 1; for (const cb of listeners['multilive://state'] || []) cb({ payload: JSON.parse(JSON.stringify(state)) }); },
  };
  window.__TAURI__ = {
    core: { invoke: async (c, a) => { if (c === 'ui_heartbeat') return null; const h = handlers[c]; if (!h) throw new Error(`mock: ${c}`); return h(a || {}); } },
    event: { listen: async (name, cb) => { (listeners[name] ||= []).push(cb); return () => {}; } },
  };
  try {
    localStorage.setItem('cview.onboarded', '1');
    localStorage.setItem('cview.welcomed.v1', String(Date.now()));
    // 예전 판이 남긴 채널별 화면 보정(선명한 화면 포함) — 맨 위 문서를 처음 열 때만(플레이어 iframe 이 다시 쓰지 않게)
    if (picture && window.top === window && !sessionStorage.getItem('seeded')) {
      localStorage.setItem('cview.picture.v1', JSON.stringify(picture));
      sessionStorage.setItem('seeded', '1');
    }
  } catch { /* 무시 */ }
}

async function openMl({ watchId = 's0', player = {}, picture = null, viewport = { width: 1280, height: 800 } } = {}) {
  const ctx = await browser.newContext({ viewport });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript(mlMock, { watchId, player, picture });
  await page.goto(base);
  await page.waitForSelector('[data-page]', { state: 'attached' });
  await page.evaluate(() => document.querySelectorAll('.sheet-scrim.welcome-scrim').forEach((x) => x.remove()));
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
  await page.waitForTimeout(500);
  return { ctx, page, errors };
}

const waitPlayers = (page) => page.waitForFunction(() => [...document.querySelectorAll('.ml-pane')].filter((p) => !p.hidden).every((p) => {
  const f = p.querySelector('iframe');
  return f && f.contentDocument?.readyState === 'complete' && typeof f.contentWindow?.setRate === 'function';
}));
const pane = (slot) => `.ml-pane[data-slot="${slot}"]`;
const calls = (page, slot, name) => page.evaluate(([s, n]) => window.__pcalls.filter((c) => (s == null || c.slot === s) && (!n || c.name === n)), [slot, name]);
const lastCall = async (page, slot, name) => (await calls(page, slot, name)).at(-1) || null;
const clearCalls = (page) => page.evaluate(() => { window.__pcalls.length = 0; window.__saved.length = 0; });
const visibleSlots = (page) => page.evaluate(() => [...document.querySelectorAll('.ml-pane')].filter((p) => !p.hidden && p.dataset.slot != null).map((p) => Number(p.dataset.slot)));
async function openGear(page, slot) {
  await page.evaluate((sel) => {
    const p = document.querySelector(sel);
    p.classList.add('ctl-visible');
    if (p.querySelector('.ml-gear-menu').hidden) p.querySelector('[data-mp="gear"]').click();
  }, pane(slot));
  await page.waitForTimeout(150);
}
const closeGear = (page, slot) => page.evaluate((sel) => { const p = document.querySelector(sel); if (!p.querySelector('.ml-gear-menu').hidden) p.querySelector('[data-mp="gear"]').click(); }, pane(slot));
const clickItem = async (page, slot, sel) => {
  const full = `${pane(slot)} .ml-gear-menu ${sel}`;
  await page.evaluate((s) => { const el = document.querySelector(s); const m = el.closest('.ml-gear-menu'); m.scrollTop = Math.max(0, el.offsetTop - m.clientHeight / 2); }, full);
  await page.click(full);
  await page.waitForTimeout(120);
};
const gearInfo = (page, slot) => page.evaluate((sel) => {
  const m = document.querySelector(`${sel} .ml-gear-menu`);
  const heads = [...m.querySelectorAll('.ml-menu-head')].map((h) => h.childNodes[0]?.textContent.trim());
  return {
    heads,
    levels: [...m.querySelectorAll('[data-mp="sharpness"]')].map((b) => b.textContent.trim()),
    active: m.querySelector('[data-mp="sharpness"][aria-checked="true"]')?.dataset.sharpness ?? null,
    enlarged: m.querySelector('[data-mp="sharpness-enlarged"]')?.getAttribute('aria-checked') ?? null,
    enlargedDisabled: m.querySelector('[data-mp="sharpness-enlarged"]')?.getAttribute('aria-disabled') ?? null,
    note: m.querySelector('.ml-sharpness-note')?.textContent || null,
    legacy: Boolean(m.querySelector('[data-mp="sharpen"], [data-mp="sharpen-all"], [data-key="videoUpscaling"]')),
    text: m.textContent.replace(/\s+/g, ' '),
    open: !m.hidden,
  };
}, pane(slot));
const store = (page) => page.evaluate(() => window.__ml.store.player);
const picture = (page) => page.evaluate(() => JSON.parse(localStorage.getItem('cview.picture.v1') || '{}'));

// ── 플레이어 문서 안 — 캔버스 영상을 넣고 경로·화소를 잰다 ──
async function feedVideo(page, slot) {
  await page.evaluate(async (sel) => {
    const f = document.querySelector(`${sel} iframe`);
    const doc = f.contentDocument, win = f.contentWindow;
    if (win.__feed) return;
    const c = doc.createElement('canvas');
    c.width = 320; c.height = 180; // 무대(단독 보기)가 영상보다 1.15배 넘게 크도록 작게
    const g = c.getContext('2d');
    // 정지 무늬(가는 줄·글자·바둑판) — 프레임마다 다시 그려 새 영상 프레임이 계속 나오게
    const draw = () => {
      g.fillStyle = '#3a4a6a'; g.fillRect(0, 0, 320, 180);
      g.fillStyle = '#d8dde6';
      for (let x = 0; x < 320; x += 6) g.fillRect(x, 0, 2, 60);
      for (let y = 66; y < 126; y += 6) for (let x = (y / 6) % 2 ? 0 : 6; x < 320; x += 12) g.fillRect(x, y, 6, 6);
      g.font = '12px sans-serif'; g.fillStyle = '#ffffff'; g.fillText('선명도 검사 ABC 123 가나다라', 10, 150);
      g.fillStyle = 'rgba(255,255,255,0.004)'; g.fillRect(0, 0, 1, 1);
    };
    draw();
    win.__feed = setInterval(draw, 33);
    const v = doc.getElementById('video');
    v.muted = true;
    v.srcObject = c.captureStream(30);
    await v.play();
  }, pane(slot));
  await page.waitForFunction((sel) => { const v = document.querySelector(`${sel} iframe`).contentDocument.getElementById('video'); return v.videoHeight > 0 && v.currentTime > 0.1; }, pane(slot), { timeout: 8000 });
}

/** 지금 경로·세기와, 지금 프레임을 한 번 그린 뒤의 경계 세기(라플라시안 RMS)·한 프레임 그리기 횟수. */
const probe = (page, slot) => page.evaluate(async (sel) => {
  const win = document.querySelector(`${sel} iframe`).contentWindow;
  return win.eval(`(async function(){
    _reconcileUpscaler();
    await new Promise((r) => setTimeout(r, 120));
    const up = _upscaler, active = !!(up && up.active);
    let draws = 0, programs = [];
    if (active && up.backend === 'webgl') {
      const gl = up.gl, orig = gl.drawArrays, use = gl.useProgram;
      let cur = null;
      gl.useProgram = function(p){ cur = p; return use.call(gl, p); };
      gl.drawArrays = function(){ draws++; programs.push(cur === up.catmull ? 'catmull' : cur === up.cas ? 'cas' : cur === up.blur ? 'blur' : cur === up.blurUnsharp ? 'unsharp' : '?'); return orig.apply(gl, arguments); };
      try { _drawUpscaledFrame(); } finally { gl.drawArrays = orig; gl.useProgram = use; }
    }
    // 경계 세기 — 화면에 보이는 크기(출력 장치 픽셀)에서 가운데 위쪽 256×128 을 떠서 라플라시안 RMS.
    //   처리 중이면 GPU 캔버스, 끔이면 영상을 같은 출력 크기로 그린 것(브라우저 기본 확대와 같은 쌍선형)을 잰다.
    const W = 256, H = 128;
    const c = document.createElement('canvas'); c.width = W; c.height = H;
    const g = c.getContext('2d', { willReadFrequently: true });
    let src, sx, sy, sw = W, sh = H;
    if (active) {
      const geo = _gpuGeometry(up);
      if (up.backend !== 'webgl') _drawUpscaledFrame();
      src = up.canvas; sx = geo.vx + geo.vw / 2 - W / 2; sy = geo.vy + geo.vh * 0.3;
    } else {
      const r = _containRect(), dpr = window.devicePixelRatio || 1;
      const ow = Math.round(r.w * dpr), oh = Math.round(r.h * dpr);
      const out = document.createElement('canvas'); out.width = ow; out.height = oh;
      const og = out.getContext('2d'); og.imageSmoothingQuality = 'low'; og.drawImage(video, 0, 0, ow, oh);
      src = out; sx = ow / 2 - W / 2; sy = oh * 0.3;
    }
    g.drawImage(src, sx, sy, sw, sh, 0, 0, W, H);
    const d = g.getImageData(0, 0, W, H).data;
    const L = (x, y) => { const i = (y * W + x) * 4; return 0.2126 * d[i] + 0.7152 * d[i + 1] + 0.0722 * d[i + 2]; };
    let s = 0, n = 0;
    for (let y = 1; y < H - 1; y++) for (let x = 1; x < W - 1; x++) { const l = 4 * L(x, y) - L(x - 1, y) - L(x + 1, y) - L(x, y - 1) - L(x, y + 1); s += l * l; n++; }
    return { mode: _levelMode, level: _sharpness.level, plan: _plan, active, backend: up ? up.backend : null,
      canvas: up ? up.canvas.style.display : null, filter: video.style.filter, draws, programs, edge: Math.sqrt(s / n),
      k2: (document.querySelector('#cview-unsharp-svg feComposite[in="edge"]') || { getAttribute(){ return null; } }).getAttribute('k2') };
  })()`);
}, pane(slot));

// ═══ S01·S03·S06: 한 페이지(단독 보기로 시작) ═══
{
  const { ctx, page, errors } = await openMl({ watchId: 's0' });

  await check('S01 one unified 선명도 item at every entry point (grid ⚙ · watch ⚙ · viewer panel · settings)', async () => {
    // 단독 보기 무대 ⚙
    await openGear(page, 0);
    const w = await gearInfo(page, 0);
    assert.equal(w.heads.filter((h) => h === '선명도').length, 1, `무대 ⚙ 에 선명도 머리가 하나가 아니다 ${JSON.stringify(w.heads)}`);
    assert.deepEqual(w.levels, ['끔', '약', '중', '강'], `무대 ⚙ 단계 ${JSON.stringify(w.levels)}`);
    assert.equal(w.enlarged, 'false', "무대 ⚙ 에 '크게 볼 때만 적용'이 없다");
    assert.equal(w.legacy, false, '무대 ⚙ 에 예전 선명한 화면·업스케일 항목이 남았다');
    assert.doesNotMatch(w.text, /선명한 화면|업스케일|모든 패인에 선명/, `무대 ⚙ 에 예전 이름이 남았다: ${w.text}`);
    await closeGear(page, 0);
    // 그리드 패인 ⚙
    await page.evaluate(() => { window.__ml.state.watchId = null; window.__ml.emit(); });
    await page.waitForTimeout(300);
    await openGear(page, 1);
    const g = await gearInfo(page, 1);
    assert.equal(g.heads.filter((h) => h === '선명도').length, 1, `그리드 ⚙ 에 선명도 머리가 하나가 아니다 ${JSON.stringify(g.heads)}`);
    assert.deepEqual(g.levels, ['끔', '약', '중', '강']);
    assert.equal(g.enlarged, 'false');
    assert.equal(g.legacy, false, '그리드 ⚙ 에 예전 항목이 남았다');
    assert.doesNotMatch(g.text, /선명한 화면|업스케일|모든 패인에 선명/, `그리드 ⚙ 에 예전 이름이 남았다: ${g.text}`);
    assert.equal(g.heads[0], '화질', '화질 머리가 맨 앞이 아니다');
    await closeGear(page, 1);
    // 시청 화면 고급 패널 '화면' 탭(보조 창·대비책) — 가짜 시청 화면으로 띄운다
    const v = await page.evaluate(async () => {
      const { PlayerPanel } = await import('/js/player-panel.js');
      const root = document.createElement('div');
      root.innerHTML = '<div class="player-stage"></div><button id="reload"></button>';
      document.body.append(root);
      window.__vcalls = [];
      const viewer = { root, isPlaying: true, channelId: 'c1', player: { onMessage() {}, call: (n, ...a) => window.__vcalls.push({ name: n, args: JSON.parse(JSON.stringify(a)) }) }, controls: { flash() {} } };
      const panel = new PlayerPanel(viewer);
      window.__vpanel = panel;
      panel.tab = 'video';
      await panel.loadSettings();
      panel.render();
      panel.el.hidden = false;
      const el = panel.el;
      return {
        segs: [...el.querySelectorAll('[data-sharpness]')].map((b) => b.textContent.trim()),
        labels: [...el.querySelectorAll('.pp-row > span:first-child')].map((s) => s.textContent.trim()),
        legacy: Boolean(el.querySelector('[data-pp="sharpen"], [data-pp="videoUpscaling"]')),
        enlarged: Boolean(el.querySelector('[data-pp="sharpnessEnlarged"]')),
        text: el.textContent.replace(/\s+/g, ' '),
      };
    });
    assert.deepEqual(v.segs, ['끔', '약', '중', '강'], `시청 화면 단계 ${JSON.stringify(v.segs)}`);
    assert.equal(v.labels.filter((l) => l === '선명도').length, 1, `시청 화면에 선명도 줄이 하나가 아니다 ${JSON.stringify(v.labels)}`);
    assert.ok(v.enlarged, "시청 화면에 '크게 볼 때만 적용'이 없다");
    assert.equal(v.legacy, false, '시청 화면에 예전 선명한 화면·업스케일 스위치가 남았다');
    assert.ok(!v.labels.includes('업스케일') && !v.labels.includes('선명한 화면'), `시청 화면 예전 줄 ${JSON.stringify(v.labels)}`);
    // 설정 화면
    await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: 'settings', params: {} } })));
    await page.click('[data-group="watch"]');
    await page.waitForSelector('[data-key="sharpnessLevel"]', { state: 'attached' });
    const s = await page.evaluate(() => {
      const sel = document.querySelector('select[data-key="sharpnessLevel"]');
      const rows = [...document.querySelectorAll('.setting .setting-label')].map((l) => l.childNodes[0]?.textContent.trim());
      return { options: [...sel.options].map((o) => o.textContent.split(' — ')[0]), rows,
        upscaleRow: Boolean(document.querySelector('[data-key="videoUpscaling"]')), enlarged: Boolean(document.querySelector('input[data-key="sharpnessEnlargedOnly"]')) };
    });
    assert.deepEqual(s.options, ['끔', '약', '중', '강'], `설정 선택지 ${JSON.stringify(s.options)}`);
    assert.equal(s.rows.filter((r) => r === '선명도').length, 1, '설정 화면에 선명도 줄이 하나가 아니다');
    assert.equal(s.upscaleRow, false, "설정 화면에 '업스케일' 줄이 남았다");
    assert.ok(s.enlarged, "설정 화면에 '크게 볼 때만 적용'이 없다");
    assert.ok(!s.rows.includes('업스케일') && !s.rows.includes('선명한 화면'), `설정 예전 줄 ${JSON.stringify(s.rows)}`);
    await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: 'multilive', params: {} } })));
    await page.evaluate(() => { window.__ml.state.watchId = 's0'; window.__ml.emit(); });
    await page.waitForTimeout(400);
  });

  await check('S03 one change applies to every pane at once (stage = upscale, grid = sharpen), saves, survives stage/new docs', async () => {
    // 그리드 4패인으로 — 패인 1 의 ⚙ 에서 '강'
    await page.evaluate(() => { window.__ml.state.watchId = null; window.__ml.emit(); });
    await page.waitForTimeout(300);
    await clearCalls(page);
    await openGear(page, 1);
    await clickItem(page, 1, '[data-mp="sharpness"][data-sharpness="3"]');
    const slots = await visibleSlots(page);
    assert.equal(slots.length, 4, `보이는 패인 ${slots}`);
    for (const s of slots) {
      assert.deepEqual((await lastCall(page, s, 'setSharpness'))?.args, [3, { upscale: false, enlargedOnly: false }], `그리드 패인 ${s} 에 '강'이 바로 걸리지 않았다`);
    }
    const g = await gearInfo(page, 1);
    assert.equal(g.open, true, '단계를 누르면 메뉴가 닫혔다');
    assert.equal(g.active, '3', '메뉴에 고른 단계가 켜져 보이지 않는다');
    assert.equal(g.enlargedDisabled, 'false');
    assert.deepEqual(await store(page), { sharpnessLevel: 3, sharpnessEnlargedOnly: false, videoUpscaling: false }, `저장 ${JSON.stringify(await store(page))}`);
    // 다른 패인 ⚙ 도 같은 값
    await closeGear(page, 1);
    await openGear(page, 2);
    assert.equal((await gearInfo(page, 2)).active, '3', '다른 패인 ⚙ 가 같은 단계를 보이지 않는다');
    await closeGear(page, 2);
    // 단독 보기 — 무대 패인만 확대 허용
    await clearCalls(page);
    await page.evaluate(() => { window.__ml.state.watchId = 's2'; window.__ml.emit(); });
    await page.waitForTimeout(400);
    assert.deepEqual((await lastCall(page, 2, 'setSharpness'))?.args, [3, { upscale: true, enlargedOnly: false }], '무대 패인에 확대 허용이 가지 않았다');
    assert.deepEqual((await calls(page, null, 'setSharpness')).filter((c) => c.slot !== 2 && c.args[1].upscale), [], '무대가 아닌 패인이 확대 허용을 받았다');
    // 무대 ⚙ 에서 '크게 볼 때만'
    await openGear(page, 2);
    await clickItem(page, 2, '[data-mp="sharpness-enlarged"]');
    assert.deepEqual((await lastCall(page, 2, 'setSharpness'))?.args, [3, { upscale: true, enlargedOnly: true }]);
    for (const s of [0, 1, 3]) assert.deepEqual((await lastCall(page, s, 'setSharpness'))?.args, [3, { upscale: false, enlargedOnly: true }], `패인 ${s} 에 '크게 볼 때만'이 가지 않았다`);
    assert.deepEqual(await store(page), { sharpnessLevel: 3, sharpnessEnlargedOnly: true, videoUpscaling: true }, '거울 videoUpscaling 이 맞지 않다');
    await clickItem(page, 2, '[data-mp="sharpness-enlarged"]');
    await closeGear(page, 2);
    // 무대에서 내려오면 확대를 끈다(선명도는 그대로)
    await clearCalls(page);
    await page.evaluate(() => { window.__ml.state.watchId = null; window.__ml.emit(); });
    await page.waitForTimeout(300);
    assert.deepEqual((await lastCall(page, 2, 'setSharpness'))?.args, [3, { upscale: false, enlargedOnly: false }], '그리드로 돌아온 패인에 확대가 남았다');
    // 새 재생 — 같은 값을 다시 건다(새 재생은 플레이어가 다시 시작한다)
    await clearCalls(page);
    await page.evaluate((sel) => window.__cviewPlayerBridge({ type: 'playStarted', data: {} }, document.querySelector(`${sel} iframe`).contentWindow), pane(1));
    assert.deepEqual((await lastCall(page, 1, 'setSharpness'))?.args, [3, { upscale: false, enlargedOnly: false }], '새 재생에 다시 걸지 않았다');
    // 새 패인(새 iframe) — 문서가 준비되면 건다
    await clearCalls(page);
    await page.evaluate(() => { const s = { ...window.__ml.state.sessions[0], id: 's4', slot: 4, channelId: 'c5', channelName: '녹두로' }; window.__ml.state.sessions.push(s); window.__ml.emit(); });
    await page.waitForSelector(`${pane(4)}:not([hidden])`);
    await waitPlayers(page);
    await page.waitForFunction(() => window.__pcalls.some((c) => c.slot === 4 && c.name === 'setSharpness'), null, { timeout: 5000 }).catch(() => {});
    assert.deepEqual((await lastCall(page, 4, 'setSharpness'))?.args, [3, { upscale: false, enlargedOnly: false }], '새 패인에 선명도가 걸리지 않았다');
    await page.evaluate(() => { window.__ml.state.sessions.pop(); window.__ml.emit(); });
    await page.waitForTimeout(300);
    // 다른 화면(시청 화면 고급 패널)에서 바꿔도 패인에 바로
    await clearCalls(page);
    // 앞선 클릭(mousedown)이 바깥 클릭으로 패널을 닫았다 — 연 채로 누른다(열려 있으면 이벤트로 다시 그린다)
    await page.evaluate(() => { window.__vpanel.el.hidden = false; window.__vpanel.el.querySelector('[data-sharpness="1"]').click(); });
    await page.waitForTimeout(150);
    for (const s of await visibleSlots(page)) assert.equal((await lastCall(page, s, 'setSharpness'))?.args[0], 1, `시청 화면에서 바꾼 '약'이 패인 ${s} 에 가지 않았다`);
    assert.deepEqual((await page.evaluate(() => window.__vcalls.filter((c) => c.name === 'setSharpness'))).at(-1)?.args, [1, { upscale: true, enlargedOnly: false }], '시청 화면 플레이어는 확대를 허용해야 한다');
    assert.equal(await page.evaluate(() => window.__vpanel.el.querySelector('[data-sharpness="1"]').getAttribute('aria-pressed')), 'true');
  });

  await check('S06 status line + ⚙ title follow the player\'s sharpness report without re-rendering the menu', async () => {
    await page.evaluate(() => { window.__ml.state.watchId = 's0'; window.__ml.emit(); });
    await page.waitForTimeout(300);
    await openGear(page, 0);
    await page.evaluate((sel) => document.querySelector(`${sel} .ml-gear-menu [data-mp="sharpness"][data-sharpness="2"]`).focus(), pane(0));
    await page.keyboard.press('Enter');
    await page.waitForTimeout(150);
    const sel0 = pane(0);
    const send = (data) => page.evaluate(([sel, d]) => window.__cviewPlayerBridge({ type: 'sharpness', data: d }, document.querySelector(`${sel} iframe`).contentWindow), [sel0, data]);
    await send({ level: 2, path: 'upscale', reason: 'enlarged', videoHeight: 720, outHeight: 1440 });
    let g = await gearInfo(page, 0);
    assert.equal(g.note, '지금: 업스케일 720p → 1440p', `상태 줄 ${g.note}`);
    assert.match(await page.evaluate((sel) => document.querySelector(`${sel} .ml-pane-bar [data-mp="gear"]`).title, sel0), /선명도: 중 · 지금 업스케일/);
    assert.equal(await page.evaluate(() => document.activeElement?.dataset.sharpness || ''), '2', '알림이 메뉴를 다시 그려 초점을 잃었다');
    await send({ level: 2, path: 'sharpen', reason: 'stage', videoHeight: 1080, outHeight: 1000 });
    g = await gearInfo(page, 0);
    assert.match(g.note, /윤곽 선명화 — 영상보다 1\.15배 이상 크게 보면 업스케일/);
    await send({ level: 2, path: 'off', reason: 'not-enlarged', videoHeight: 1080, outHeight: 1000 });
    assert.match((await gearInfo(page, 0)).note, /쉬는 중/);
    // ↓ 로 단계 사이를 돈다(메뉴 키보드)
    await page.keyboard.press('ArrowDown');
    assert.equal(await page.evaluate(() => document.activeElement?.dataset.sharpness || ''), '3', '↓ 가 다음 단계로 가지 않았다');
    await page.keyboard.press('Escape');
    await page.waitForTimeout(100);
    assert.equal(await page.evaluate((sel) => document.querySelector(`${sel} .ml-gear-menu`).hidden, sel0), true, 'Esc 로 메뉴가 닫히지 않았다');
  });

  if (errors.length) results.push({ id: 'page errors (S01·S03·S06)', ok: false, error: errors.join('\n') });
  await ctx.close();
}

// ═══ S02: 단계별 실제 처리 — 패인 플레이어 문서 ═══
await check('S02 per-level processing in the real player (stage: 1/2/4 passes; grid: unsharp 0.5/1/1.5; off: none; css fallback k2)', async () => {
  const { ctx, page, errors } = await openMl({ watchId: 's0', viewport: { width: 1280, height: 800 } });
  try {
    await feedVideo(page, 0);
    const pick = async (slot, level) => {
      await openGear(page, slot);
      await clickItem(page, slot, `[data-mp="sharpness"][data-sharpness="${level}"]`);
      await closeGear(page, slot);
      await page.waitForTimeout(200);
      return probe(page, slot);
    };
    // 무대(단독 보기) — 영상 360p, 무대는 그보다 크다 → 확대 경로
    const st = {};
    for (const L of [0, 1, 2, 3]) st[L] = await pick(0, L);
    assert.equal(st[0].mode, true, '플레이어가 선명도(level) 모드가 아니다');
    assert.deepEqual([st[0].plan.path, st[0].active, st[0].filter], ['off', false, ''], `끔 ${JSON.stringify(st[0])}`);
    if (st[1].backend !== 'webgl') throw new Error(`이 브라우저에서 WebGL2 를 쓸 수 없어 GPU 경로를 확인하지 못했다 (${JSON.stringify(st[1].plan)})`);
    assert.deepEqual([st[1].plan.path, st[1].plan.cas, st[1].programs.join(',')], ['upscale', null, 'catmull'], `약 무대 ${JSON.stringify(st[1])}`);
    assert.deepEqual([st[2].plan.path, st[2].plan.cas, st[2].programs.join(',')], ['upscale', 0.55, 'catmull,cas'], `중 무대 ${JSON.stringify(st[2])}`);
    assert.deepEqual([st[3].plan.path, st[3].plan.cas, st[3].plan.post, st[3].programs.join(',')], ['upscale', 0.55, 0.5, 'catmull,cas,blur,unsharp'], `강 무대 ${JSON.stringify(st[3])}`);
    for (const L of [1, 2, 3]) assert.equal(st[L].canvas, '', `무대 ${L}: GPU 캔버스가 보이지 않는다`);
    assert.ok(st[1].edge < st[2].edge && st[2].edge < st[3].edge, `무대 경계 세기가 단계마다 커지지 않는다 ${[1, 2, 3].map((L) => st[L].edge.toFixed(1))}`);
    // 그리드 — 패인이 영상보다 크지 않고 확대도 허용하지 않는다 → 언샤프
    await page.evaluate(() => { window.__ml.state.watchId = null; window.__ml.emit(); });
    await page.waitForTimeout(500);
    const gr = {};
    for (const L of [0, 1, 2, 3]) gr[L] = await pick(0, L);
    assert.deepEqual([gr[0].plan.path, gr[0].active, gr[0].filter], ['off', false, ''], `그리드 끔 ${JSON.stringify(gr[0])}`);
    for (const [L, amount] of [[1, 0.5], [2, 1], [3, 1.5]]) {
      assert.deepEqual([gr[L].plan.path, gr[L].plan.reason, gr[L].plan.amount, gr[L].programs.join(',')], ['sharpen', 'grid', amount, 'blur,unsharp'], `그리드 ${L} ${JSON.stringify(gr[L])}`);
    }
    const e = [0, 1, 2, 3].map((L) => gr[L].edge);
    assert.ok(e[0] < e[1] && e[1] < e[2] && e[2] < e[3], `그리드 경계 세기가 끔 < 약 < 중 < 강 이 아니다 ${e.map((x) => x.toFixed(1))}`);
    // GPU 를 쓸 수 없으면 SVG 언샤프 — 세기가 k2 로
    const css = await page.evaluate((sel) => document.querySelector(`${sel} iframe`).contentWindow.eval(`(function(){
      _sharpenGpuFailed = true; _deactivateUpscaler(); _reconcileUpscaler();
      const k2 = document.querySelector('#cview-unsharp-svg feComposite[in="edge"]').getAttribute('k2');
      const r = { path: _plan.path, filter: video.style.filter, k2 };
      _sharpenGpuFailed = false; _reconcileUpscaler();
      return r;
    })()`), pane(0));
    assert.deepEqual([css.path, css.filter.startsWith('url('), css.k2], ['sharpen-css', true, '1.5'], `GPU 없음 ${JSON.stringify(css)}`);
    // '크게 볼 때만' — 그리드는 원본 그대로
    await openGear(page, 0);
    await clickItem(page, 0, '[data-mp="sharpness-enlarged"]');
    await closeGear(page, 0);
    const eo = await probe(page, 0);
    assert.deepEqual([eo.plan.path, eo.plan.reason, eo.active, eo.filter], ['off', 'grid-enlarged-only', false, ''], `크게 볼 때만 그리드 ${JSON.stringify(eo)}`);
    // 무대로 가면 확대
    await page.evaluate(() => { window.__ml.state.watchId = 's0'; window.__ml.emit(); });
    await page.waitForTimeout(500);
    const es = await probe(page, 0);
    assert.deepEqual([es.plan.path, es.active], ['upscale', true], `크게 볼 때만 무대 ${JSON.stringify(es)}`);
    assert.deepEqual(errors, []);
  } finally { await ctx.close(); }
});

// ═══ S04: 설정 이전 ═══
await check('S04 legacy → 선명도 migration table (pure) and first-run boots (legacy keys untouched, one-time notice)', async () => {
  const { ctx, page, errors } = await openMl({ watchId: null });
  try {
    // 순수 함수 전수표 — 채널 분포 × 처음 보는 채널 기본값 × 업스케일
    const rows = await page.evaluate(async () => {
      const { sharpnessFromLegacy } = await import('/js/sharpness.js');
      const out = [];
      for (const [on, off] of [[0, 0], [2, 0], [1, 3], [2, 2], [0, 2]]) {
        for (const def of [undefined, true, false]) {
          for (const upscale of [false, true]) {
            const r = sharpnessFromLegacy({ on, off, def, upscale });
            out.push({ on, off, def: def === undefined ? '-' : def, upscale, level: r.level, E: r.enlargedOnly, notice: r.notice });
          }
        }
      }
      return out;
    });
    assert.equal(rows.length, 30);
    for (const r of rows) {
      const s = r.on > r.off ? true : r.on < r.off ? false : (r.def === true || (r.def === '-' && r.on > 0));
      const want = { level: s || r.upscale ? 2 : 0, E: !s && r.upscale };
      assert.deepEqual({ level: r.level, E: r.E }, want, `행 ${JSON.stringify(r)}`);
    }
    // 대표 행(사람 말로)
    const row = (on, off, def, upscale) => rows.find((r) => r.on === on && r.off === off && r.def === def && r.upscale === upscale);
    assert.deepEqual([row(0, 0, '-', false).level, row(0, 0, '-', false).E], [0, false], '아무것도 안 켠 사용자 → 끔');
    assert.deepEqual([row(0, 0, '-', true).level, row(0, 0, '-', true).E], [2, true], '업스케일만 → 중 · 크게 볼 때만(예전 업스케일과 같은 화소)');
    assert.deepEqual([row(0, 0, true, false).level, row(0, 0, true, false).E], [2, false], "'모든 패인에 선명하게' → 중");
    assert.deepEqual([row(0, 0, true, true).level, row(0, 0, true, true).E], [2, false], '둘 다 → 중(삼중 선명화 대신 하나)');
    assert.deepEqual([row(2, 0, '-', false).level], [2], '켠 채널이 많으면 → 중');
    assert.deepEqual([row(1, 3, '-', false).level], [0], '끈 채널이 많으면 → 끔');
    assert.deepEqual([row(1, 3, '-', true).level, row(1, 3, '-', true).E], [2, true], '끈 채널이 많고 업스케일 → 중 · 크게 볼 때만');
  } finally { await ctx.close(); }
  // 첫 실행 — 저장본에 sharpnessLevel 이 없을 때 한 번 옮겨 쓴다
  const cases = [
    { name: '업스케일만', player: { videoUpscaling: true }, picture: null, want: { sharpnessLevel: 2, sharpnessEnlargedOnly: true }, notice: true },
    { name: '채널 둘 켬', player: {}, picture: { c1: { sharpen: true }, c2: { sharpen: true, brightness: 1.2 } }, want: { sharpnessLevel: 2, sharpnessEnlargedOnly: false }, notice: true },
    { name: '모두 끔·기본 끔', player: { videoUpscaling: false }, picture: { c1: { sharpen: false }, __default: { sharpen: false } }, want: { sharpnessLevel: 0, sharpnessEnlargedOnly: false }, notice: false },
    { name: '이미 새 값', player: { sharpnessLevel: 1, sharpnessEnlargedOnly: false, videoUpscaling: true }, picture: { c1: { sharpen: true } }, want: { sharpnessLevel: 1, sharpnessEnlargedOnly: false }, notice: false },
  ];
  for (const c of cases) {
    const { ctx: cx, page: p, errors: errs } = await openMl({ watchId: null, player: c.player, picture: c.picture });
    try {
      await p.waitForFunction(() => typeof window.__ml.store.player.sharpnessLevel === 'number', null, { timeout: 4000 });
      const st = await store(p);
      assert.deepEqual({ sharpnessLevel: st.sharpnessLevel, sharpnessEnlargedOnly: st.sharpnessEnlargedOnly }, c.want, `${c.name}: ${JSON.stringify(st)}`);
      assert.equal(st.videoUpscaling, c.player.videoUpscaling, `${c.name}: 이전이 예전 업스케일 값을 바꿨다`);
      if (c.picture) assert.deepEqual(await picture(p), c.picture, `${c.name}: 이전이 예전 채널 값을 바꿨다`);
      // 모든 보이는 패인에 이전 결과가 걸렸다
      await p.waitForTimeout(300);
      for (const s of await visibleSlots(p)) assert.equal((await lastCall(p, s, 'setSharpness'))?.args[0], c.want.sharpnessLevel, `${c.name}: 패인 ${s}`);
      const notice = await p.evaluate(() => [...document.querySelectorAll('.toast')].some((t) => /선명도' 하나로 합쳐졌어요/.test(t.textContent)));
      assert.equal(notice, c.notice, `${c.name}: 1회 안내 ${notice}`);
      assert.deepEqual(errs, [], `${c.name}: ${errs}`);
    } finally { await cx.close(); }
  }
});

// ═══ S05: 되돌리기(구버전 거울·스냅숏·초기화) ═══
await check('S05 downgrade-safe: legacy mirrors + snapshot restore (약·강), old-version edits win, reset turns mirrors off', async () => {
  // 새 판에서 '강' — 구버전 거울
  const seed = { c1: { sharpen: false, brightness: 1.1 }, c2: { sharpen: true } };
  let ls;
  {
    const { ctx, page } = await openMl({ watchId: null, picture: seed });
    try {
      await page.waitForFunction(() => typeof window.__ml.store.player.sharpnessLevel === 'number');
      await openGear(page, 0);
      await clickItem(page, 0, '[data-mp="sharpness"][data-sharpness="3"]');
      await closeGear(page, 0);
      assert.deepEqual(await store(page), { sharpnessLevel: 3, sharpnessEnlargedOnly: false, videoUpscaling: false });
      const pic = await picture(page);
      assert.equal(pic.__default.sharpen, true, '구버전 기본값 거울(선명한 화면 켬)이 없다');
      assert.deepEqual(pic.__default.sharpness, { level: 3, enlargedOnly: false, d: true, u: false }, `스냅숏 ${JSON.stringify(pic.__default)}`);
      assert.deepEqual(pic.c1, { brightness: 1.1, sharpenBeforeEnhance: false }, '채널 값(구버전이 기본값보다 앞세우는 값)을 보존하며 비키지 않았다');
      assert.deepEqual(pic.c2, { sharpenBeforeEnhance: true });
      ls = pic;
    } finally { await ctx.close(); }
  }
  // 구버전이 새 키를 지웠다(구버전 저장) — 거울이 그대로면 스냅숏으로 '강'
  {
    const { ctx, page } = await openMl({ watchId: null, player: { videoUpscaling: false }, picture: ls });
    try {
      await page.waitForFunction(() => typeof window.__ml.store.player.sharpnessLevel === 'number');
      assert.deepEqual(await store(page), { videoUpscaling: false, sharpnessLevel: 3, sharpnessEnlargedOnly: false }, '되돌렸다 다시 올리니 강이 아니다');
    } finally { await ctx.close(); }
  }
  // 구버전에서 채널 하나의 선명한 화면을 껐다 → 구버전의 선택을 따른다(다시 계산)
  {
    const edited = JSON.parse(JSON.stringify(ls));
    edited.c3 = { sharpen: false };
    const { ctx, page } = await openMl({ watchId: null, player: { videoUpscaling: false }, picture: edited });
    try {
      await page.waitForFunction(() => typeof window.__ml.store.player.sharpnessLevel === 'number');
      // 채널 끔 1 · 켬 0 → 끔
      assert.equal((await store(page)).sharpnessLevel, 0, '구버전에서 바꾼 값을 따르지 않았다');
    } finally { await ctx.close(); }
  }
  // '약·크게 볼 때만' → 거울은 업스케일만(둘을 같이 켜지 않는다) → 되돌렸다 와도 약·크게 볼 때만
  {
    const { ctx, page } = await openMl({ watchId: null });
    try {
      await page.waitForFunction(() => typeof window.__ml.store.player.sharpnessLevel === 'number');
      await openGear(page, 0);
      await clickItem(page, 0, '[data-mp="sharpness"][data-sharpness="1"]');
      await clickItem(page, 0, '[data-mp="sharpness-enlarged"]');
      await closeGear(page, 0);
      assert.deepEqual(await store(page), { sharpnessLevel: 1, sharpnessEnlargedOnly: true, videoUpscaling: true });
      const pic = await picture(page);
      assert.equal(pic.__default.sharpen, false, '크게 볼 때만인데 구버전 선명한 화면도 켰다(이중 선명화)');
      ls = pic;
    } finally { await ctx.close(); }
    const { ctx: c2, page: p2 } = await openMl({ watchId: null, player: { videoUpscaling: true }, picture: ls });
    try {
      await p2.waitForFunction(() => typeof window.__ml.store.player.sharpnessLevel === 'number');
      const st = await store(p2);
      assert.deepEqual([st.sharpnessLevel, st.sharpnessEnlargedOnly], [1, true], `되돌렸다 와서 ${JSON.stringify(st)}`);
    } finally { await c2.close(); }
  }
  // 초기화(Rust 기본값 0) — 스냅숏이 있으면 거울도 끔으로 맞춘다
  {
    const snap = { __default: { sharpen: true, sharpness: { level: 3, enlargedOnly: false, d: true, u: false } } };
    const { ctx, page } = await openMl({ watchId: null, player: { sharpnessLevel: 0, sharpnessEnlargedOnly: false, videoUpscaling: false }, picture: snap });
    try {
      await page.waitForTimeout(600);
      const pic = await picture(page);
      assert.deepEqual([pic.__default.sharpen, pic.__default.sharpness.level], [false, 0], `초기화 뒤 거울 ${JSON.stringify(pic)}`);
      assert.equal((await store(page)).sharpnessLevel, 0);
    } finally { await ctx.close(); }
  }
});

// ═══ S07: 설정 화면 ═══
await check('S07 settings: 선명도 select + 크게 볼 때만, found by search (업스케일·선명), saves and re-applies to open panes', async () => {
  const { ctx, page, errors } = await openMl({ watchId: null });
  try {
    await page.waitForFunction(() => typeof window.__ml.store.player.sharpnessLevel === 'number');
    await page.evaluate(() => window.dispatchEvent(new CustomEvent('cview:navigate', { detail: { route: 'settings', params: {} } })));
    await page.click('[data-group="watch"]');
    await page.waitForSelector('select[data-key="sharpnessLevel"]', { state: 'attached' });
    const visible = async (q) => {
      await page.fill('.settings-search', q);
      await page.waitForTimeout(150);
      return page.evaluate(() => !document.querySelector('select[data-key="sharpnessLevel"]').closest('.setting').hidden);
    };
    assert.ok(await visible('업스케일'), "검색 '업스케일'로 선명도를 찾지 못한다");
    assert.ok(await visible('선명'), "검색 '선명'으로 선명도를 찾지 못한다");
    assert.equal(await page.evaluate(() => document.querySelector('input[data-key="sharpnessEnlargedOnly"]').disabled), true, "끔인데 '크게 볼 때만'을 누를 수 있다");
    await clearCalls(page);
    await page.selectOption('select[data-key="sharpnessLevel"]', '2');
    await page.waitForTimeout(250);
    assert.deepEqual(await store(page), { sharpnessLevel: 2, sharpnessEnlargedOnly: false, videoUpscaling: false });
    for (const s of await page.evaluate(() => [...document.querySelectorAll('.ml-pane')].filter((p) => p.dataset.slot != null && p.querySelector('iframe')).map((p) => Number(p.dataset.slot)))) {
      assert.equal((await lastCall(page, s, 'setSharpness'))?.args[0], 2, `설정에서 바꾼 값이 패인 ${s} 에 가지 않았다`);
    }
    assert.equal(await page.evaluate(() => document.querySelector('input[data-key="sharpnessEnlargedOnly"]').disabled), false, "단계를 켰는데 '크게 볼 때만'이 막혀 있다");
    await page.evaluate(() => document.querySelector('input[data-key="sharpnessEnlargedOnly"]').click()); // 꾸민 토글이라 입력은 숨어 있다
    await page.waitForTimeout(200);
    assert.deepEqual(await store(page), { sharpnessLevel: 2, sharpnessEnlargedOnly: true, videoUpscaling: true });
    // 다른 곳(이벤트)에서 바꾸면 설정 화면 칸도 따라온다
    await page.evaluate(async () => { const { setSharpness } = await import('/js/sharpness.js'); await setSharpness(3, false); });
    await page.waitForTimeout(150);
    assert.deepEqual(await page.evaluate(() => [document.querySelector('select[data-key="sharpnessLevel"]').value, document.querySelector('input[data-key="sharpnessEnlargedOnly"]').checked]), ['3', false]);
    assert.deepEqual(errors, []);
  } finally { await ctx.close(); }
});

await browser.close();
server.close();
for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : ` — ${r.error}`}`);
const failed = results.filter((r) => !r.ok).length;
console.log(failed ? `${failed} failed` : `all ${results.length} passed`);
process.exit(failed ? 1 : 0);
