// 0003: 블로킹 재요청 정책 — 저지연 밖 지시자 금지(blockingReloadOutsideLowLatency), 400 대체(blockingReloadFallback).
'use strict';
const timers = [];
globalThis.setTimeout = (fn, ms) => { timers.push({ fn, ms }); return timers.length; };
globalThis.clearTimeout = () => {};
globalThis.setInterval = () => 0;
globalThis.clearInterval = () => {};
const test = require('node:test');
const assert = require('node:assert/strict');
const { Hls, xhrs, livePlaylist, advance } = require('./harness.cjs');

const MASTER = '#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=8000000,RESOLUTION=1920x1080\nvariant.m3u8?auth=x\n';
const BLOCKING = '#EXT-X-SERVER-CONTROL:CAN-BLOCK-RELOAD=YES,PART-HOLD-BACK=3.051\n';
const playlist = (sequence = 100) => livePlaylist({ sequence, extra: BLOCKING });
const hasDirectives = (url) => /_HLS_msn=/.test(url);

/** 첫 재생목록을 싣는다. 반환 시점 직전의 요청 수를 mark 로 둔다. */
function start(config) {
  timers.length = 0;
  const hls = new Hls(config);
  const errors = [];
  hls.on(Hls.Events.ERROR, (_, d) => errors.push(d));
  hls.loadSource('https://example.invalid/master.m3u8');
  xhrs.at(-1).complete(200, MASTER);
  const f = { hls, errors, mark: 0 };
  respond(f, xhrs.at(-1), playlist(100));
  return f;
}
/** 요청에 응답하고, 그 직후를 다음 요청 탐색의 기준으로 둔다. */
function respond(f, xhr, body, status = 200) {
  timers.length = 0;
  f.mark = xhrs.length;
  xhr.complete(status, body);
}
/**
 * 다음 재생목록 요청: 응답 직후 바로 나갔으면(블로킹 요청) 그것, 아니면 재요청 예약 타이머를 실행한다.
 * 10초 이상 타이머는 대기 중 요청의 응답 시간 초과라서 건드리지 않는다.
 */
function nextRequest(f) {
  if (xhrs.length > f.mark) return xhrs.at(-1);
  const t = timers.filter((x) => x.ms > 0 && x.ms < 9000).at(-1);
  assert.ok(t, `재요청 예약이 없다: ${timers.map((x) => x.ms)}`);
  advance(t.ms);
  const before = xhrs.length;
  t.fn();
  assert.equal(xhrs.length, before + 1, '재요청이 나가지 않았다');
  return xhrs.at(-1);
}

// ⚠ hls.js 의 lowLatencyMode 기본값은 true 다. 앱은 기본 프로필에서 false 를 명시한다.
test('기본값은 업스트림과 같다: 저지연 모드가 아니어도 블로킹 지시자를 붙인다', () => {
  const f = start({ lowLatencyMode: false });
  try { assert.ok(hasDirectives(nextRequest(f).url)); } finally { f.hls.destroy(); }
});

test('blockingReloadOutsideLowLatency=false 이면 저지연 밖에서 지시자를 붙이지 않는다', () => {
  const f = start({ lowLatencyMode: false, blockingReloadOutsideLowLatency: false });
  try {
    const req = nextRequest(f);
    assert.ok(!hasDirectives(req.url), req.url);
    assert.ok(req.url.includes('auth=x'), '서명 쿼리를 잃었다');
  } finally { f.hls.destroy(); }
});

test('lowLatencyMode 에서는 blockingReloadOutsideLowLatency=false 여도 지시자를 쓴다', () => {
  const f = start({ blockingReloadOutsideLowLatency: false, lowLatencyMode: true });
  try { assert.ok(hasDirectives(nextRequest(f).url)); } finally { f.hls.destroy(); }
});

test('기본값은 업스트림과 같다: 지시자 요청의 400 을 지시자 없이 다시 묻지 않는다', () => {
  const f = start({ lowLatencyMode: false });
  try {
    const req = nextRequest(f);
    assert.ok(hasDirectives(req.url));
    const before = xhrs.length;
    req.complete(400);
    const plainRetry = xhrs.slice(before).some((x) => !hasDirectives(x.url));
    assert.ok(!plainRetry, '업스트림 기본값에서 대체 재요청이 나갔다');
    assert.ok(f.errors.some((e) => e.response?.code === 400));
  } finally { f.hls.destroy(); }
});

test('blockingReloadFallback 이면 400 직후 지시자 없이 즉시 다시 묻고, 3번 거절되면 블로킹을 끈다', () => {
  const f = start({ lowLatencyMode: false, blockingReloadFallback: true });
  try {
    let sequence = 100;
    for (let i = 0; i < 3; i++) {
      const req = nextRequest(f);
      assert.ok(hasDirectives(req.url), `${i}: 거절 전 요청에 지시자가 없다`);
      const before = xhrs.length;
      req.complete(400);
      assert.equal(xhrs.length, before + 1, `${i}: 400 뒤 즉시 재요청이 없다`);
      const retry = xhrs.at(-1);
      assert.ok(!hasDirectives(retry.url), `${i}: 재요청에 지시자가 남았다 ${retry.url}`);
      assert.ok(retry.url.includes('auth=x'));
      sequence += 1;
      respond(f, retry, playlist(sequence));
    }
    assert.ok(!f.errors.some((e) => e.fatal), '치명 오류가 났다');
    const after = nextRequest(f);
    assert.ok(!hasDirectives(after.url), `3번 거절 뒤에도 지시자를 붙인다 ${after.url}`);
  } finally { f.hls.destroy(); }
});
