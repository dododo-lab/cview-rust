// 0002: 라이브 재생목록 재요청 간격의 기준(liveReloadReference).
// hls.js 가 직접 재생목록을 요청하게 두고, 재요청 타이머를 가로채 지연을 잰다. 가짜 시계를 타이머만큼 진행시킨다.
'use strict';
const timers = [];
globalThis.setTimeout = (fn, ms) => { timers.push({ fn, ms }); return timers.length; };
globalThis.clearTimeout = () => {};
const test = require('node:test');
const assert = require('node:assert/strict');
const { Hls, xhrs, livePlaylist, advance } = require('./harness.cjs');

const MASTER = '#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=8000000,RESOLUTION=1920x1080\nvariant.m3u8\n';

/** 재생목록 두 번(같은 내용) — 반환: [첫 재요청 지연, 안 바뀐 뒤 재요청 지연] */
function reloadDelays(config, second = livePlaylist()) {
  timers.length = 0;
  const hls = new Hls(config);
  hls.loadSource('https://example.invalid/master.m3u8');
  xhrs.at(-1).complete(200, MASTER);
  let mark = timers.length;
  xhrs.at(-1).complete(200, livePlaylist());
  const first = timers.slice(mark).find((t) => t.ms > 0 && t.ms < 15000);
  advance(first.ms);
  const before = xhrs.length;
  first.fn();
  assert.equal(xhrs.length, before + 1, '재요청이 나가지 않았다');
  mark = timers.length;
  xhrs.at(-1).complete(200, second);
  const next = timers.slice(mark).find((t) => t.ms > 0 && t.ms < 15000);
  hls.destroy();
  return [first.ms, next.ms];
}

test('기본값은 업스트림과 같다: 안 바뀐 재생목록 뒤 TARGETDURATION/2(5초) 대기', () => {
  const [first, unchanged] = reloadDelays({});
  assert.ok(Math.abs(first - 2000) <= 100, `first ${first}`);
  assert.ok(Math.abs(unchanged - 5000) <= 100, `unchanged ${unchanged}`);
});

test("'segment' 이면 안 바뀐 재생목록 뒤 세그먼트/2(1초)만 기다린다", () => {
  const [first, unchanged] = reloadDelays({ liveReloadReference: 'segment' });
  assert.ok(Math.abs(first - 2000) <= 100, `first ${first}`);
  assert.ok(Math.abs(unchanged - 1000) <= 100, `unchanged ${unchanged}`);
});

test("'segment' 여도 재생목록이 갱신되면 마지막 세그먼트 길이로 다시 묻는다(업스트림과 같음)", () => {
  const [, updated] = reloadDelays({ liveReloadReference: 'segment' }, livePlaylist({ sequence: 101 }));
  assert.ok(Math.abs(updated - 2000) <= 100, `updated ${updated}`);
});
