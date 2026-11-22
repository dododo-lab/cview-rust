// 0006: 자동 화질(ABR)이 영상 코덱 계열을 넘어 오르내리기(abrSwitchVideoCodec).
//   치지직 1080p60 은 같은 대역폭의 HEVC(hvc1)와 H.264(avc1)를 함께 내고, 그 아래 화질은 H.264 뿐이다. 업스트림 ABR 은 처음 고른
//   코덱 묶음 안에서만 고르므로 HEVC 1080p 에서 대역폭이 모자라면 후보가 없고(-1 → 최저 화질 144p 로 떨어짐), H.264 에서는 HEVC 로
//   올라가지 못한다. 켜면 오디오 코덱이 같은 레벨 사이에서는 영상 코덱을 넘어 고른다.
'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const { Hls, xhrs } = require('./harness.cjs');

// 하네스에는 MSE 가 없다 — 코덱이 있는 마스터의 레벨이 모두 미지원으로 빠지지 않게 판별만 흉내 낸다.
globalThis.MediaSource = { isTypeSupported: () => true };

/** 치지직 실제 마스터와 같은 순서·값(HEVC 가 첫 줄). */
const CHZZK_MASTER = '#EXTM3U\n'
  + '#EXT-X-STREAM-INF:BANDWIDTH=8192000,CODECS="hvc1.1.2.L123.90,mp4a.40.2",RESOLUTION=1920x1080,FRAME-RATE=59.94\nhevc1080.m3u8\n'
  + '#EXT-X-STREAM-INF:BANDWIDTH=8192000,CODECS="avc1.64002A,mp4a.40.2",RESOLUTION=1920x1080,FRAME-RATE=59.94\navc1080.m3u8\n'
  + '#EXT-X-STREAM-INF:BANDWIDTH=3192000,CODECS="avc1.640028,mp4a.40.2",RESOLUTION=1280x720,FRAME-RATE=59.94\navc720.m3u8\n'
  + '#EXT-X-STREAM-INF:BANDWIDTH=1692000,CODECS="avc1.4D001F,mp4a.40.2",RESOLUTION=852x480,FRAME-RATE=30.00\navc480.m3u8\n';

function parsed(master, config = {}) {
  const hls = new Hls({ autoStartLoad: false, ...config });
  hls.loadSource('https://example.invalid/master.m3u8');
  xhrs.at(-1).complete(200, master);
  return hls;
}

/** 지금 `current` 레벨을 받고 있을 때 ABR 이 고르는 레벨(버퍼 4초, 대역폭 bw). */
function best(hls, current, bw) {
  const abr = hls.abrController;
  abr.lastLoadedFragLevel = current;
  Object.defineProperty(hls, 'loadLevel', { get: () => current, configurable: true });
  return abr.findBestLevel(bw, 0, hls.levels.length - 1, 4, 0, 0.95, 0.7);
}

const idx = (hls, pred) => hls.levels.findIndex(pred);
const hevc1080 = (l) => l.height === 1080 && l.videoCodec.startsWith('hvc1');
const avc1080 = (l) => l.height === 1080 && l.videoCodec.startsWith('avc1');

test('치지직 마스터: 같은 1080p60 에서 HEVC 가 H.264 보다 위로 정렬된다(앱의 "최고 레벨" 규칙이 HEVC 를 고르는 전제)', () => {
  const hls = parsed(CHZZK_MASTER);
  try {
    assert.deepEqual(hls.levels.map((l) => `${l.height}/${l.videoCodec.slice(0, 4)}`),
      ['480/avc1', '720/avc1', '1080/avc1', '1080/hvc1']);
  } finally { hls.destroy(); }
});

test('기본값은 업스트림과 같다: HEVC 1080p 에서 대역폭이 모자라면 같은 묶음 후보가 없어 -1', () => {
  const hls = parsed(CHZZK_MASTER);
  try {
    assert.equal(best(hls, idx(hls, hevc1080), 5e6), -1);
  } finally { hls.destroy(); }
});

test('기본값은 업스트림과 같다: H.264 720p 에서 대역폭이 넉넉해도 HEVC 로는 올라가지 않는다', () => {
  const hls = parsed(CHZZK_MASTER);
  try {
    assert.equal(best(hls, idx(hls, (l) => l.height === 720), 2e7), idx(hls, avc1080));
  } finally { hls.destroy(); }
});

test('abrSwitchVideoCodec: HEVC 1080p 에서 대역폭이 모자라면 H.264 720p 로 내려간다', () => {
  const hls = parsed(CHZZK_MASTER, { abrSwitchVideoCodec: true });
  try {
    assert.equal(best(hls, idx(hls, hevc1080), 5e6), idx(hls, (l) => l.height === 720));
  } finally { hls.destroy(); }
});

test('abrSwitchVideoCodec: H.264 720p 에서 대역폭이 넉넉하면 맨 위 HEVC 1080p 로 올라간다', () => {
  const hls = parsed(CHZZK_MASTER, { abrSwitchVideoCodec: true });
  try {
    assert.equal(best(hls, idx(hls, (l) => l.height === 720), 2e7), idx(hls, hevc1080));
  } finally { hls.destroy(); }
});

test('abrSwitchVideoCodec: 오디오 코덱이 다른 레벨로는 넘어가지 않는다', () => {
  const master = '#EXTM3U\n'
    + '#EXT-X-STREAM-INF:BANDWIDTH=8192000,CODECS="hvc1.1.2.L123.90,ec-3",RESOLUTION=1920x1080,FRAME-RATE=59.94\nhevc1080.m3u8\n'
    + '#EXT-X-STREAM-INF:BANDWIDTH=3192000,CODECS="avc1.640028,mp4a.40.2",RESOLUTION=1280x720,FRAME-RATE=59.94\navc720.m3u8\n';
  const hls = parsed(master, { abrSwitchVideoCodec: true });
  try {
    const i720 = idx(hls, (l) => l.height === 720);
    assert.equal(best(hls, i720, 2e7), i720);
  } finally { hls.destroy(); }
});
