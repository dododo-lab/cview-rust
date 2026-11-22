// HEVC(H.265) 변형 회귀 검사 — 치지직이 1080p60 을 같은 대역폭의 HEVC·H.264 로 함께 낼 때, 플레이어(player.html)가 이 웹뷰의
// 디코딩 능력에 따라 HEVC 를 쓰거나 H.264 로 자연스럽게 넘어가는지 본다. 플레이어 문서를 그대로 띄우고 MSE 판별·
// MediaCapabilities 를 환경별로 흉내 낸다(재생목록은 가짜, 실제 서비스는 건드리지 않는다).
// H01 판정 표 — hevcPolicyDecision(설정·판별·실행 중 오류) 의 모든 갈래.
// H02 하드웨어 디코딩(지원·부드러움·전력 효율) — HEVC 레벨이 남고 '최고 화질 고정'이 HEVC 1080p 를, H.264 쌍둥이는 대체용으로 남는다.
// H03 원본 우선(기본 설정) — 첫 조각을 HEVC 1080p 로 요청하고, 자동 화질이 코덱 계열을 넘는다(abrSwitchVideoCodec).
// H04 소프트웨어 디코딩(powerEfficient=false) — HEVC 를 빼고 H.264 1080p.
// H05 MSE 미지원(Windows HEVC 확장 없음 등) — hls.js 가 HEVC 를 거르고 H.264 1080p. 판별 요약에 mse=0.
// H06 설정 끔 — 매니페스트에서 HEVC 를 빼고, 재생 중에 끄면 그 자리에서 H.264 로.
// H07 판별이 늦음 — MSE 판별만 믿고 HEVC 로 시작했다가 결과가 '소프트웨어'면 H.264 로 바꾸고 화면에 알린다.
// H08 재생 중 HEVC 디코드 오류(코덱 오류·video 오류 3) — 그 자리에서 H.264 로, 다음 매니페스트에서도 HEVC 를 쓰지 않는다.
// H09 1080p 고정(멀티라이브 메인 1080p/8M) — 하드웨어면 HEVC, 아니면 H.264. 낮은 고정(360p)은 그대로.
// H10 화질 메뉴 항목 — 같은 높이는 하나(HEVC 우선), HEVC 표시, 배지 설명에 코덱.
// H11 레벨·지표·진단 메시지 — levels 의 레벨별 codec, metrics 의 videoCodec, codecDiagnostics 의 hevc 요약.
// 저장소 루트에서: node tools/ui-regression/hevc.mjs   (CVIEW_UI_BROWSER=webkit 이면 WebKit)
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { server, base } from './mock-app.mjs';

const require = createRequire(import.meta.url);
const pw = require('playwright');
const WEBKIT = process.env.CVIEW_UI_BROWSER === 'webkit';
const browser = await (WEBKIT ? pw.webkit : pw.chromium).launch();
const results = [];
async function check(id, fn) {
  try { await fn(); results.push({ id, ok: true }); } catch (e) { results.push({ id, ok: false, error: e.message }); }
}

// 치지직 실제 마스터와 같은 순서·값(2026-11-22 공개 방송에서 확인 — HEVC 가 첫 줄, 1080p 두 변형의 대역폭이 같다)
const MASTER = '#EXTM3U\n'
  + '#EXT-X-STREAM-INF:BANDWIDTH=8192000,CODECS="hvc1.1.2.L123.90,mp4a.40.2",RESOLUTION=1920x1080,FRAME-RATE=59.94\nhevc1080.m3u8\n'
  + '#EXT-X-STREAM-INF:BANDWIDTH=8192000,CODECS="avc1.64002A,mp4a.40.2",RESOLUTION=1920x1080,FRAME-RATE=59.94\navc1080.m3u8\n'
  + '#EXT-X-STREAM-INF:BANDWIDTH=3192000,CODECS="avc1.640028,mp4a.40.2",RESOLUTION=1280x720,FRAME-RATE=59.94\navc720.m3u8\n'
  + '#EXT-X-STREAM-INF:BANDWIDTH=1692000,CODECS="avc1.4D001F,mp4a.40.2",RESOLUTION=852x480,FRAME-RATE=30.00\navc480.m3u8\n'
  + '#EXT-X-STREAM-INF:BANDWIDTH=696000,CODECS="avc1.4D401E,mp4a.40.2",RESOLUTION=640x360,FRAME-RATE=30.00\navc360.m3u8\n';
const MASTER_URL = 'https://hevc.test/live/master.m3u8';

/**
 * 플레이어 문서를 연다. env: { mse: HEVC MSE 판별, caps: decodingInfo 결과 | 'reject', capsDelay: ms }
 * 재생목록 요청은 마스터만 응답하고 나머지(레벨 재생목록·조각)는 응답하지 않아 레벨 선택만 본다.
 */
async function openPlayer(env) {
  const ctx = await browser.newContext({ viewport: { width: 960, height: 540 } });
  const page = await ctx.newPage();
  page.setDefaultTimeout(8000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.addInitScript((e) => {
    window.__msgs = [];
    window.webkit = { messageHandlers: { player: { postMessage: (m) => window.__msgs.push(JSON.parse(JSON.stringify(m))) } } };
    // 검사 브라우저에는 독점 코덱이 없을 수 있다 — H.264·AAC 는 늘 된다고 하고 HEVC 만 환경대로.
    const MS = window.MediaSource;
    const native = MS && MS.isTypeSupported ? MS.isTypeSupported.bind(MS) : () => false;
    if (MS) MS.isTypeSupported = (t) => (/hvc1|hev1/.test(t) ? e.mse : (/avc1|mp4a/.test(t) ? true : native(t)));
    const answer = () => (e.caps === 'reject' ? Promise.reject(new TypeError('x')) : Promise.resolve(e.caps));
    const decodingInfo = (cfg) => {
      window.__capsCalls = (window.__capsCalls || 0) + 1;
      window.__capsConfig = cfg;
      return e.capsDelay ? new Promise((r) => setTimeout(r, e.capsDelay)).then(answer) : answer();
    };
    Object.defineProperty(navigator, 'mediaCapabilities', { value: { decodingInfo }, configurable: true });
  }, { mse: true, caps: { supported: true, smooth: true, powerEfficient: true }, capsDelay: 0, ...env });
  await page.route('https://hevc.test/**', (route) => {
    if (route.request().url() === MASTER_URL) return route.fulfill({ status: 200, contentType: 'application/vnd.apple.mpegurl', body: MASTER });
    // 레벨 재생목록·조각은 끝내 응답하지 않는다(오류·재시도 없이 선택만 본다)
    return undefined;
  });
  await page.goto(`${base}player/player.html`);
  await page.waitForFunction(() => typeof window.loadSource === 'function' && typeof window.Hls === 'function');
  return { ctx, page, errors };
}

/** loadSource(…, preferSource, lockTop) 뒤 MANIFEST_PARSED 를 기다리고 레벨 상태를 돌려준다. */
async function load(page, { prefer = false, lock = false, setup = '' } = {}) {
  await page.evaluate(([url, p, l, s]) => {
    if (s) window.eval(s);
    window.__parsed = false;
    loadSource(url, 'lowLatency', false, p, l, true);
    hls.on(Hls.Events.MANIFEST_PARSED, () => { window.__parsed = true; });
  }, [MASTER_URL, prefer, lock, setup]);
  await page.waitForFunction(() => window.__parsed === true);
  await page.waitForTimeout(50);
  return state(page);
}

function state(page) {
  return page.evaluate(() => {
    const levels = hls.levels.map((l) => `${l.height}/${videoCodecLabel(l.videoCodec)}`);
    const at = (i) => (i >= 0 && hls.levels[i] ? `${hls.levels[i].height}/${videoCodecLabel(hls.levels[i].videoCodec)}` : String(i));
    return {
      levels, manual: at(hls.manualLevel), next: at(hls.nextLoadLevel), load: at(hls.loadLevel),
      session: { ..._hevcSession }, probe: { ..._hevcProbe }, summary: hevcSummary(), abrSwitch: hls.config.abrSwitchVideoCodec,
    };
  });
}

const msgs = (page, type) => page.evaluate((t) => window.__msgs.filter((m) => m.type === t).map((m) => m.data), type);
const HW = { supported: true, smooth: true, powerEfficient: true };
const SW = { supported: true, smooth: true, powerEfficient: false };
const LADDER_WITH_HEVC = ['360/H.264', '480/H.264', '720/H.264', '1080/H.264', '1080/HEVC'];
const LADDER_H264 = ['360/H.264', '480/H.264', '720/H.264', '1080/H.264'];

await check('H01 decision table (setting, probe, runtime failure)', async () => {
  const { ctx, page, errors } = await openPlayer({});
  try {
    const rows = await page.evaluate(() => {
      const done = (o) => ({ state: 'done', typeSupported: true, supported: true, smooth: true, powerEfficient: true, ...o });
      const cases = [
        ['off', false, done({}), ''],
        ['decode-error', true, done({}), 'bufferAddCodecError'],
        ['pending', true, { state: 'pending' }, ''],
        ['probe-error', true, { state: 'error', typeSupported: true }, ''],
        ['mse', true, done({ typeSupported: false, supported: false, smooth: false, powerEfficient: false }), ''],
        ['decoder', true, done({ supported: false }), ''],
        ['software', true, done({ powerEfficient: false }), ''],
        ['smooth', true, done({ smooth: false }), ''],
        ['hardware', true, done({}), ''],
        ['unknown', true, done({ smooth: null, powerEfficient: null }), ''],
      ];
      return Object.fromEntries(cases.map(([k, en, p, f]) => { const d = hevcPolicyDecision(en, p, f); return [k, `${d.allow}/${d.reason}`]; }));
    });
    assert.deepEqual(rows, {
      off: 'false/off', 'decode-error': 'false/decode-error', pending: 'true/pending', 'probe-error': 'false/probe-error',
      mse: 'false/mse-unsupported', decoder: 'false/decoder-unsupported', software: 'false/software-decoder', smooth: 'false/not-smooth',
      hardware: 'true/hardware', unknown: 'true/capabilities-unknown',
    });
    assert.deepEqual(errors, []);
  } finally { await ctx.close(); }
});

await check('H02 hardware decoder: HEVC 1080p kept and chosen by the top-quality lock, H.264 twin kept as fallback', async () => {
  const { ctx, page, errors } = await openPlayer({ caps: HW });
  try {
    const s = await load(page, { lock: true });
    assert.deepEqual(s.levels, LADDER_WITH_HEVC);
    assert.equal(s.manual, '1080/HEVC', JSON.stringify(s));
    assert.deepEqual([s.session.used, s.session.reason, s.session.available], [true, 'hardware', 1]);
    assert.equal(s.summary.probe, 'mse=1 supported=1 smooth=1 power=1');
    const cfg = await page.evaluate(() => window.__capsConfig);
    assert.deepEqual([cfg.type, cfg.video.contentType, cfg.video.width, cfg.video.height, cfg.video.framerate], ['media-source', 'video/mp4; codecs="hvc1.1.2.L123.90"', 1920, 1080, 59.94]);
    assert.deepEqual(errors, []);
  } finally { await ctx.close(); }
});

await check('H03 prefer-source (default): first request is HEVC 1080p; ABR may cross codec families', async () => {
  const { ctx, page, errors } = await openPlayer({ caps: HW });
  try {
    const s = await load(page, { prefer: true });
    assert.equal(s.next, '1080/HEVC', JSON.stringify(s));
    assert.equal(s.manual, '-1', '원본 우선은 자동 화질을 남긴다');
    assert.equal(s.abrSwitch, true);
    // 포크 0006 이 실린 번들인가 — 코덱 계열을 넘는 ABR 판정(계약 테스트와 같은 상황: HEVC 1080p 에서 5Mbps)
    const best = await page.evaluate(() => {
      const abr = hls.abrController;
      const hevcIdx = hls.levels.findIndex((l) => isHevcCodec(l.videoCodec));
      abr.lastLoadedFragLevel = hevcIdx;
      Object.defineProperty(hls, 'loadLevel', { get: () => hevcIdx, configurable: true });
      const i = abr.findBestLevel(5e6, 0, hls.levels.length - 1, 4, 0, 0.95, 0.7);
      return i >= 0 ? `${hls.levels[i].height}/${videoCodecLabel(hls.levels[i].videoCodec)}` : String(i);
    });
    assert.equal(best, '720/H.264', 'HEVC 1080p 에서 대역폭이 모자라면 H.264 720p 로(144p 로 떨어지지 않는다)');
    assert.deepEqual(errors, []);
  } finally { await ctx.close(); }
});

await check('H04 software decoder (powerEfficient=false): HEVC removed, H.264 1080p', async () => {
  const { ctx, page, errors } = await openPlayer({ caps: SW });
  try {
    const s = await load(page, { lock: true });
    assert.deepEqual(s.levels, LADDER_H264);
    assert.equal(s.manual, '1080/H.264');
    assert.deepEqual([s.session.used, s.session.reason, s.session.available], [false, 'software-decoder', 1]);
    assert.deepEqual(errors, []);
  } finally { await ctx.close(); }
});

await check('H05 MSE says no HEVC (e.g. Windows without the HEVC extension): hls.js filters it, H.264 1080p', async () => {
  const { ctx, page, errors } = await openPlayer({ mse: false, caps: { supported: false, smooth: false, powerEfficient: false } });
  try {
    const s = await load(page, { prefer: true });
    assert.deepEqual(s.levels, LADDER_H264);
    assert.equal(s.next, '1080/H.264');
    assert.equal(s.summary.probe, 'mse=0 supported=0 smooth=0 power=0');
    assert.equal(await page.evaluate(() => window.__capsCalls || 0), 0, 'MSE 가 아니라고 하면 MediaCapabilities 는 묻지 않는다');
    assert.deepEqual(errors, []);
  } finally { await ctx.close(); }
});

await check('H06 setting off: HEVC removed from the manifest; turning it off mid-play switches to H.264 at once', async () => {
  const { ctx, page, errors } = await openPlayer({ caps: HW });
  try {
    const off = await load(page, { lock: true, setup: 'setHevcEnabled(false)' });
    assert.deepEqual(off.levels, LADDER_H264);
    assert.equal(off.manual, '1080/H.264');
    assert.equal(off.session.reason, 'off');
    // 다시 켜면 다음 매니페스트부터
    const on = await load(page, { lock: true, setup: 'setHevcEnabled(true)' });
    assert.equal(on.manual, '1080/HEVC');
    // 재생 중에 끄면 그 자리에서
    await page.evaluate(() => { window.__msgs.length = 0; setHevcEnabled(false); });
    const mid = await state(page);
    assert.deepEqual(mid.levels, LADDER_H264);
    assert.equal(mid.manual, '1080/H.264');
    const hevcMsg = await msgs(page, 'hevc');
    assert.equal(hevcMsg.length, 1);
    assert.deepEqual([hevcMsg[0].reason, hevcMsg[0].used, hevcMsg[0].enabled], ['off', false, false]);
    const lv = (await msgs(page, 'levels')).at(-1);
    assert.deepEqual(lv.levels.map((l) => `${l.height}/${l.codec}`), LADDER_H264, '화면에 바뀐 화질 목록을 다시 보낸다');
    assert.deepEqual(errors, []);
  } finally { await ctx.close(); }
});

await check('H07 late probe: starts on HEVC (MSE says yes), drops to H.264 when the result says software', async () => {
  const { ctx, page, errors } = await openPlayer({ caps: SW, capsDelay: 1500 });
  try {
    const s = await load(page, { lock: true });
    assert.equal(s.probe.state, 'pending', '판별이 아직이어야 한다');
    assert.equal(s.manual, '1080/HEVC');
    assert.equal(s.session.reason, 'pending');
    await page.waitForFunction(() => _hevcProbe.state === 'done', null, { timeout: 5000 });
    await page.waitForTimeout(50);
    const after = await state(page);
    assert.deepEqual(after.levels, LADDER_H264);
    assert.equal(after.manual, '1080/H.264');
    assert.equal(after.session.reason, 'software-decoder');
    assert.equal((await msgs(page, 'hevc')).at(-1)?.reason, 'software-decoder');
    assert.deepEqual(errors, []);
  } finally { await ctx.close(); }
  // 늦게 온 결과가 '하드웨어'면 그대로 HEVC, 사유만 바뀐다
  const hw = await openPlayer({ caps: HW, capsDelay: 1200 });
  try {
    const s = await load(hw.page, { lock: true });
    assert.equal(s.session.reason, 'pending');
    await hw.page.waitForFunction(() => _hevcProbe.state === 'done', null, { timeout: 5000 });
    const after = await state(hw.page);
    assert.deepEqual([after.manual, after.session.reason], ['1080/HEVC', 'hardware']);
    assert.deepEqual(hw.errors, []);
  } finally { await hw.ctx.close(); }
});

await check('H08 HEVC decode failure while playing: falls back to H.264 now and for later manifests', async () => {
  for (const kind of ['codec', 'video']) {
    const { ctx, page, errors } = await openPlayer({ caps: HW });
    try {
      const s = await load(page, { lock: true });
      assert.equal(s.manual, '1080/HEVC');
      await page.evaluate((k) => {
        const idx = hls.levels.findIndex((l) => isHevcCodec(l.videoCodec));
        if (k === 'codec') {
          hls.trigger(Hls.Events.ERROR, { type: Hls.ErrorTypes.MEDIA_ERROR, details: Hls.ErrorDetails.BUFFER_ADD_CODEC_ERROR, fatal: false, sourceBufferName: 'video', frag: { level: idx }, error: new Error('codec') });
        } else {
          // 미디어 요소의 디코드 오류(3) — hls.js 는 듣지 않는다
          Object.defineProperty(video, 'error', { get: () => ({ code: 3, message: 'decode' }), configurable: true });
          hls.currentLevel = idx;
          video.dispatchEvent(new Event('error'));
        }
      }, kind);
      await page.waitForTimeout(50);
      const after = await state(page);
      assert.deepEqual(after.levels, LADDER_H264, `${kind}: ${JSON.stringify(after.levels)}`);
      assert.equal(after.manual, '1080/H.264', `${kind}: ${after.manual}`);
      assert.equal(after.session.reason, 'decode-error');
      assert.match(after.summary.runtimeFailure, kind === 'codec' ? /bufferAddCodecError/ : /mediaError3/);
      // 다음 매니페스트(재연결·채널 전환)에서도 이 페이지는 HEVC 를 쓰지 않는다
      await page.evaluate(() => Object.defineProperty(video, 'error', { get: () => null, configurable: true }));
      const again = await load(page, { lock: true });
      assert.deepEqual([again.manual, again.session.reason], ['1080/H.264', 'decode-error']);
      assert.deepEqual(errors, []);
    } finally { await ctx.close(); }
  }
});

await check('H09 forced height (multi-live main 1080p/8M): HEVC with hardware, H.264 otherwise; 360p stays H.264', async () => {
  for (const [caps, want] of [[HW, '1080/HEVC'], [SW, '1080/H.264']]) {
    const { ctx, page, errors } = await openPlayer({ caps });
    try {
      const s = await load(page, { setup: 'setForcedHeight(1080, 8000)' });
      assert.equal(s.manual, want, JSON.stringify(s));
      await page.evaluate(() => setForcedHeight(360, 600));
      await page.waitForTimeout(50);
      assert.equal((await state(page)).manual, '360/H.264');
      assert.deepEqual(errors, []);
    } finally { await ctx.close(); }
  }
});

await check('H10 quality menu items: one per height (HEVC first), HEVC tag, badge label names the codec', async () => {
  const ctx = await browser.newContext();
  const page = await ctx.newPage();
  try {
    // 앱 화면을 띄우지 않고 같은 출처의 빈 문서에서 모듈만 불러 순수 함수를 본다
    await page.goto(`${base}__blank`);
    const r = await page.evaluate(async () => {
      const m = await import('/js/viewer-controls.js');
      const lv = (index, height, bitrate, codec) => ({ index, height, bitrate, codec });
      const withHevc = m.qualityOptions([lv(0, 360, 696000, 'H.264'), lv(1, 720, 3192000, 'H.264'), lv(2, 1080, 8192000, 'H.264'), lv(3, 1080, 8192000, 'HEVC')]);
      const h264 = m.qualityOptions([lv(0, 720, 3192000, 'H.264'), lv(1, 1080, 8192000, 'H.264')]);
      // 예전 플레이어(코덱 없음) — 같은 높이는 높은 비트레이트
      const legacy = m.qualityOptions([lv(0, 1080, 6000000), lv(1, 1080, 8000000)]);
      return {
        withHevc: withHevc.map((o) => `${o.label}:${o.index}:${o.codec}`), h264: h264.map((o) => `${o.label}:${o.codec}`), legacy: legacy.map((o) => o.index),
        tagHevc: m.codecTagHtml('HEVC'), tagH264: m.codecTagHtml('H.264'),
        badge: m.qualityBadgeLabel({ height: 1080, codec: 'HEVC', auto: true, latency: 2.14 }),
        badgeNoCodec: m.qualityBadgeLabel({ height: 1080, auto: false, latency: 3 }),
        badgeUnknown: m.qualityBadgeLabel({}),
      };
    });
    assert.deepEqual(r.withHevc, ['1080p:3:HEVC', '720p:1:H.264', '360p:0:H.264']);
    assert.deepEqual(r.h264, ['1080p:H.264', '720p:H.264']);
    assert.deepEqual(r.legacy, [1]);
    assert.match(r.tagHevc, /class="codec-tag"[^>]*>HEVC</);
    assert.equal(r.tagH264, '');
    assert.equal(r.badge, '화질 1080p HEVC (자동) · 지연 2.1초');
    assert.equal(r.badgeNoCodec, '화질 1080p · 지연 3.0초');
    assert.equal(r.badgeUnknown, '화질 확인 중 (자동) · 지연 확인 중');
  } finally { await ctx.close(); }
});

await check('H11 levels/metrics/codecDiagnostics carry the codec and the HEVC decision', async () => {
  const { ctx, page, errors } = await openPlayer({ caps: HW });
  try {
    await load(page, { lock: true });
    await page.evaluate(() => { window.__msgs.length = 0; getLevels(); });
    const lv = (await msgs(page, 'levels')).at(-1);
    assert.deepEqual(lv.levels.map((l) => `${l.height}/${l.codec}`), LADDER_WITH_HEVC);
    assert.deepEqual([lv.hevc.reason, lv.hevc.used, lv.hevc.enabled], ['hardware', true, true]);
    // 지표의 지금 코덱 — 레벨 재생목록이 오지 않아 collectMetrics 를 직접 부른다(loadLevel 은 고정한 HEVC)
    await page.evaluate(() => { window.__msgs.length = 0; collectMetrics(); });
    const m = (await msgs(page, 'metrics')).at(-1);
    assert.equal(m && m.videoCodec, 'HEVC');
    // 코덱 진단은 MSE 가 붙고(MEDIA_ATTACHED) 버퍼가 만들어져야(BUFFER_CREATED) 한 번 나간다 — 조각이 오지 않으므로
    // SourceBuffer 생성 알림만 흉내 낸다.
    await page.waitForFunction(() => _attachedMediaSourceKind !== 'unknown');
    await page.evaluate(() => {
      hls.trigger(Hls.Events.BUFFER_CREATED, { tracks: { video: { id: 'main', container: 'video/mp4', codec: 'hvc1.1.2.L123.90' } } });
    });
    const diag = (await msgs(page, 'codecDiagnostics')).at(-1);
    assert.ok(diag, 'codecDiagnostics 가 나가지 않았다');
    assert.match(diag.levels, /1080p\/8192k\/hvc1\.1\.2\.L123\.90/, '매니페스트가 낸 HEVC 변형이 진단 레벨 목록에 있다');
    assert.deepEqual([diag.hevc.reason, diag.hevc.used, diag.hevc.available, diag.hevc.probe], ['hardware', true, 1, 'mse=1 supported=1 smooth=1 power=1']);
    assert.deepEqual(errors, []);
  } finally { await ctx.close(); }
});

await browser.close();
server.close();
for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.id}${r.ok ? '' : ` — ${r.error}`}`);
const failed = results.filter((r) => !r.ok).length;
console.log(failed ? `${failed} failed` : `all ${results.length} passed`);
process.exit(failed ? 1 : 0);
