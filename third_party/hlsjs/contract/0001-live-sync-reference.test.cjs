// 0001: 라이브 기준 단위(liveSyncReference). 치지직형 재생목록(TARGETDURATION 10, 세그먼트 2초).
'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const { livePlaylist, loadedHls, loadLevel } = require('./harness.cjs');

const close = (actual, expected, tolerance = 0.2) =>
  assert.ok(Math.abs(actual - expected) <= tolerance, `${actual} ≉ ${expected}`);

test('기본값(targetduration)은 업스트림과 같다: liveSyncPosition 은 끝 − 10초에서 멈춘다', () => {
  const f = loadedHls({ liveSyncDuration: 3, liveMaxLatencyDuration: 8 });
  const details = loadLevel(f, livePlaylist());
  assert.equal(details.targetduration, 10);
  close(details.averagetargetduration, 2, 0.001);
  close(f.hls.liveSyncPosition, details.edge - 10);
  f.hls.destroy();
});

test("'segment' 이면 liveSyncPosition 이 지연 목표(3초)까지 다가간다", () => {
  const f = loadedHls({ liveSyncDuration: 3, liveMaxLatencyDuration: 8, liveSyncReference: 'segment' });
  const details = loadLevel(f, livePlaylist());
  close(f.hls.liveSyncPosition, details.edge - 3);
  f.hls.destroy();
});

test("'segment' 이면 개수 기반 기본 지연 목표가 3 × 세그먼트(6초)다 (업스트림 30초)", () => {
  const upstream = loadedHls();
  loadLevel(upstream, livePlaylist());
  close(upstream.hls.targetLatency, 30, 0.001);
  upstream.hls.destroy();

  const fork = loadedHls({ liveSyncReference: 'segment' });
  loadLevel(fork, livePlaylist());
  close(fork.hls.targetLatency, 6, 0.001);
  fork.hls.destroy();
});

test("'segment' 여도 세그먼트가 TARGETDURATION 과 같으면 동작이 같다", () => {
  const f = loadedHls({ liveSyncReference: 'segment' });
  loadLevel(f, livePlaylist({ targetDuration: 6, segment: 6 }));
  close(f.hls.targetLatency, 18, 0.001);
  f.hls.destroy();
});
