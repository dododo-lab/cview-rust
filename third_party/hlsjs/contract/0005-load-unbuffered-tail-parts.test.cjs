// 0005: 버퍼 판정 여유(0.2초) 안쪽의 받지 않은 꼬리 파트(loadUnbufferedTailParts).
//   4.17초 세그먼트 채널에서 세그먼트 마지막 파트가 0.03~0.15초이거나 버퍼 끝이 파트 끝보다 몇 ms 모자라면, fragment-tracker 는
//   앞 파트만 받은 세그먼트를 OK 로 본다. 그때 isLoopLoading 이 "반복 로드"로 보고 아무것도 받지 않아 12초 남짓 멈췄다.
'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const { loadedHls, loadLevel } = require('./harness.cjs');

/** 4.17초 세그먼트 LL 재생목록 — 마지막 완성 세그먼트(106)의 꼬리 파트가 0.033초. */
function llPlaylist() {
  let text = '#EXTM3U\n#EXT-X-VERSION:10\n#EXT-X-TARGETDURATION:10\n'
    + '#EXT-X-SERVER-CONTROL:CAN-BLOCK-RELOAD=YES,PART-HOLD-BACK=3.051\n#EXT-X-PART-INF:PART-TARGET=1.000000\n'
    + '#EXT-X-MEDIA-SEQUENCE:100\n';
  for (let sn = 100; sn < 106; sn++) text += `#EXTINF:4.166667,\nseg${sn}.m4v\n`;
  for (let i = 0; i < 4; i++) text += `#EXT-X-PART:DURATION=1.000000,URI="seg106_${i}.m4v"${i === 0 ? ',INDEPENDENT=YES' : ''}\n`;
  text += '#EXT-X-PART:DURATION=0.033333,URI="seg106_4.m4v"\n#EXTINF:4.033333,\nseg106.m4v\n';
  text += '#EXT-X-PRELOAD-HINT:TYPE=PART,URI="seg107_0.m4v"\n';
  return text;
}

/** 꼬리 파트만 남기고 106 의 앞 파트 네 개를 받은 상태 — 트래커는 106 을 OK 로 본다. */
function tailState(config) {
  const f = loadedHls({ lowLatencyMode: true, ...config });
  const details = loadLevel(f, llPlaylist());
  const sc = f.hls.streamController;
  const parts = details.partList.filter((p) => p.fragment.sn === 106);
  assert.equal(parts.length, 5);
  parts.slice(0, 4).forEach((p) => { p.elementaryStreams.video = { startPTS: p.start, endPTS: p.end }; });
  const frag = parts[0].fragment;
  sc.loadingParts = true;
  sc.fragmentTracker.getState = () => 'OK';
  // 버퍼 끝이 넷째 파트 끝보다 14ms 모자라다(오디오가 먼저 끝남).
  sc.nextLoadPosition = parts[3].end;
  return { f, sc, frag, tail: parts[4], target: parts[3].end - 0.014 };
}

test('기본값은 업스트림과 같다: 꼬리 파트가 남아도 OK 세그먼트는 반복 로드로 본다', () => {
  const s = tailState({});
  try {
    assert.equal(s.sc.isLoopLoading(s.frag, s.target), true);
  } finally { s.f.hls.destroy(); }
});

test('loadUnbufferedTailParts 이면 받지 않은 꼬리 파트가 있는 OK 세그먼트는 반복 로드가 아니다', () => {
  const s = tailState({ loadUnbufferedTailParts: true });
  try {
    assert.equal(s.sc.hasUnbufferedTailPart(s.frag), true);
    assert.equal(s.sc.isLoopLoading(s.frag, s.target), false);
  } finally { s.f.hls.destroy(); }
});

test('한 번 받은 꼬리 파트는 다시 받지 않는다(진짜 반복 로드 방지)', () => {
  const s = tailState({ loadUnbufferedTailParts: true });
  try {
    s.tail.stats.loading.end = 1;
    assert.equal(s.sc.hasUnbufferedTailPart(s.frag), false);
    assert.equal(s.sc.isLoopLoading(s.frag, s.target), true);
  } finally { s.f.hls.destroy(); }
});

/** 106 의 파트를 꼬리까지 다 받았고 107 첫 파트가 올라온 상태 — 버퍼 끝은 106 끝보다 7ms 모자라다. */
function doneState(config) {
  const f = loadedHls({ lowLatencyMode: true, ...config });
  const details = loadLevel(f, llPlaylist().replace(
    '#EXT-X-PRELOAD-HINT:TYPE=PART,URI="seg107_0.m4v"\n',
    '#EXT-X-PART:DURATION=1.000000,URI="seg107_0.m4v",INDEPENDENT=YES\n#EXT-X-PRELOAD-HINT:TYPE=PART,URI="seg107_1.m4v"\n',
  ));
  const sc = f.hls.streamController;
  const parts = details.partList.filter((p) => p.fragment.sn === 106);
  parts.forEach((p) => { p.elementaryStreams.video = { startPTS: p.start, endPTS: p.end }; });
  const frag = parts[0].fragment;
  sc.loadingParts = true;
  sc.fragmentTracker.getState = (fr) => (fr.sn === 106 ? 'OK' : 'NOT_LOADED');
  sc.nextLoadPosition = frag.end;
  const bufferInfo = { len: 2, start: frag.end - 2.007, end: frag.end - 0.007, nextStart: undefined, buffered: [] };
  return { f, sc, frag, details, bufferInfo };
}

test('기본값은 업스트림과 같다: 다 받은 세그먼트의 버퍼 끝이 몇 ms 모자라면 반복 로드로 보고 아무것도 고르지 않는다', () => {
  const s = doneState({});
  try {
    assert.equal(s.sc.isLoopLoading(s.frag, s.bufferInfo.end), true);
    assert.equal(s.sc.getNextFragmentLoopLoading(s.frag, s.details, s.bufferInfo, 'main', 12), null);
  } finally { s.f.hls.destroy(); }
});

test('loadUnbufferedTailParts 이면 받은 끝에서 다음 세그먼트를 고른다', () => {
  const s = doneState({ loadUnbufferedTailParts: true });
  try {
    assert.equal(s.sc.isLoopLoading(s.frag, s.bufferInfo.end), true);
    const next = s.sc.getNextFragmentLoopLoading(s.frag, s.details, s.bufferInfo, 'main', 12);
    assert.ok(next, '다음 조각을 고르지 못했다');
    assert.equal(next.sn, 107);
  } finally { s.f.hls.destroy(); }
});

test('버퍼 끝이 받은 끝보다 0.2초 넘게 모자라면 반올림이 아니므로 업스트림 판정 그대로다', () => {
  const s = doneState({ loadUnbufferedTailParts: true });
  try {
    s.bufferInfo.end = s.frag.end - 0.5;
    assert.equal(s.sc.getNextFragmentLoopLoading(s.frag, s.details, s.bufferInfo, 'main', 12), null);
  } finally { s.f.hls.destroy(); }
});

test('파트로 받고 있지 않으면(저지연 밖) 업스트림 판정 그대로다', () => {
  const s = tailState({ loadUnbufferedTailParts: true });
  try {
    s.sc.loadingParts = false;
    assert.equal(s.sc.hasUnbufferedTailPart(s.frag), false);
    assert.equal(s.sc.isLoopLoading(s.frag, s.target), true);
  } finally { s.f.hls.destroy(); }
});
