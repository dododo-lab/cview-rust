// 0004: 멈춤으로 늘어난 지연 목표의 감쇠(liveSyncStallDecayMs).
'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const { Hls, livePlaylist, loadedHls, loadLevel, advance } = require('./harness.cjs');

function stall(hls) {
  hls.trigger(Hls.Events.ERROR, {
    type: Hls.ErrorTypes.MEDIA_ERROR, details: Hls.ErrorDetails.BUFFER_STALLED_ERROR, fatal: false,
  });
}

test('기본값은 업스트림과 같다: 멈춤마다 +1초, 같은 목표 재대입이 초기화하고, 시간이 지나도 줄지 않는다', () => {
  const f = loadedHls({ lowLatencyMode: false });
  try {
    loadLevel(f, livePlaylist());
    f.hls.targetLatency = 3;
    stall(f.hls); stall(f.hls);
    assert.equal(f.hls.targetLatency, 5);
    advance(120_000);
    assert.equal(f.hls.targetLatency, 5, '업스트림은 시간으로 줄지 않는다');
    f.hls.targetLatency = 3;
    assert.equal(f.hls.targetLatency, 3, '업스트림은 같은 값 재대입이 초기화한다');
  } finally { f.hls.destroy(); }
});

test('liveSyncStallDecayMs 이면 같은 목표 재대입은 멈춤 이력을 지우지 않고, 시간이 지나면 하나씩 줄어든다', () => {
  const f = loadedHls({ lowLatencyMode: false, liveSyncStallDecayMs: 20_000 });
  try {
    loadLevel(f, livePlaylist());
    f.hls.targetLatency = 3;
    stall(f.hls); stall(f.hls);
    assert.equal(f.hls.targetLatency, 5);
    f.hls.targetLatency = 3;
    assert.equal(f.hls.targetLatency, 5, '같은 값 재대입이 멈춤 이력을 지웠다');
    advance(19_000);
    assert.equal(f.hls.targetLatency, 5);
    advance(1_000);
    assert.equal(f.hls.targetLatency, 4);
    advance(20_000);
    assert.equal(f.hls.targetLatency, 3);
    advance(60_000);
    assert.equal(f.hls.targetLatency, 3, '목표 아래로 내려갔다');
  } finally { f.hls.destroy(); }
});

test('liveSyncStallDecayMs 여도 목표가 실제로 바뀌면 멈춤 이력을 초기화한다', () => {
  const f = loadedHls({ lowLatencyMode: false, liveSyncStallDecayMs: 20_000 });
  try {
    loadLevel(f, livePlaylist());
    f.hls.targetLatency = 3;
    stall(f.hls);
    assert.equal(f.hls.targetLatency, 4);
    f.hls.targetLatency = 2.5;
    assert.equal(f.hls.targetLatency, 2.5);
  } finally { f.hls.destroy(); }
});
