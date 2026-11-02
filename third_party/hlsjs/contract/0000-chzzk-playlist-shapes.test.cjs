// 경량 빌드(fork.json features)가 실제 치지직 재생목록 형태를 그대로 해석하는가.
// 태그 구성은 2026-10-05 인기 라이브 4채널(일반 HLS·LLHLS)에서 확인한 것이다: 마스터는 STREAM-INF 만,
// 미디어는 DATERANGE(nmss)·PDT·MAP·PART·PRELOAD-HINT·SERVER-CONTROL. 일부 채널은 TS + AES-128.
'use strict';
// Node 에는 MediaSource 가 없어 CODECS 가 있는 변형을 전부 '지원 안 함'으로 거른다. 브라우저(WKWebView)에서는
// H.264/AAC 가 지원되므로 그 판정만 흉내 낸다 — 이 파일 프로세스에만 적용된다.
globalThis.MediaSource = { isTypeSupported: (type) => /avc1|mp4a/.test(type) };
const test = require('node:test');
const assert = require('node:assert/strict');
const { Hls, xhrs } = require('./harness.cjs');

const MASTER = '#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-INDEPENDENT-SEGMENTS\n' +
  '#EXT-X-STREAM-INF:BANDWIDTH=8192000,RESOLUTION=1920x1080,CODECS="avc1.64002a,mp4a.40.2",FRAME-RATE=60.0\nhls_chunklist_1080.m3u8\n' +
  '#EXT-X-STREAM-INF:BANDWIDTH=2048000,RESOLUTION=1280x720,CODECS="avc1.4d401f,mp4a.40.2",FRAME-RATE=30.0\nhls_chunklist_720.m3u8\n';

function llMedia(seq = 500) {
  let t = '#EXTM3U\n#EXT-X-VERSION:9\n#EXT-X-INDEPENDENT-SEGMENTS\n#EXT-X-TARGETDURATION:10\n' +
    '#EXT-X-SERVER-CONTROL:CAN-BLOCK-RELOAD=YES,PART-HOLD-BACK=3.051000\n#EXT-X-PART-INF:PART-TARGET=1.000000\n' +
    `#EXT-X-MEDIA-SEQUENCE:${seq}\n#EXT-X-MAP:URI="init.m4v"\n` +
    '#EXT-X-DATERANGE:ID="nmss-daterange",START-DATE="2026-10-04T08:41:51.154Z"\n';
  for (let i = 0; i < 6; i++) {
    t += `#EXT-X-PROGRAM-DATE-TIME:2026-10-05T01:00:${String(i * 2).padStart(2, '0')}.000Z\n`;
    if (i === 5) t += `#EXT-X-PART:DURATION=1.000000,URI="seg${seq + i}.part0.m4v",INDEPENDENT=YES\n#EXT-X-PART:DURATION=1.000000,URI="seg${seq + i}.part1.m4v"\n`;
    t += `#EXTINF:2.000000,\nseg${seq + i}.m4v\n`;
  }
  t += `#EXT-X-PART:DURATION=1.000000,URI="seg${seq + 6}.part0.m4v",INDEPENDENT=YES\n#EXT-X-PRELOAD-HINT:TYPE=PART,URI="seg${seq + 6}.part1.m4v"\n`;
  return t;
}

function load(config, master, media) {
  const hls = new Hls({ autoStartLoad: false, ...config });
  const errors = [], loaded = [], parsed = [];
  hls.on(Hls.Events.ERROR, (_, d) => errors.push(d));
  hls.on(Hls.Events.LEVEL_LOADED, (_, d) => loaded.push(d));
  hls.on(Hls.Events.MANIFEST_PARSED, (_, d) => parsed.push(d));
  hls.loadSource('https://example.invalid/master.m3u8');
  xhrs.at(-1).complete(200, master);
  hls.trigger(Hls.Events.LEVEL_LOADING, { id: 0, level: 0, pathwayId: '', levelInfo: hls.levels[0],
    url: 'https://example.invalid/hls_chunklist_1080.m3u8', deliveryDirectives: null });
  xhrs.at(-1).complete(200, media);
  return { hls, errors, loaded, parsed };
}

test('치지직 LLHLS 형태: 레벨·DATERANGE·PDT·MAP·파트를 오류 없이 해석한다', () => {
  const f = load({ lowLatencyMode: true }, MASTER, llMedia());
  try {
    assert.deepEqual(f.errors.map((e) => e.details), []);
    assert.equal(f.hls.levels.length, 2);
    assert.deepEqual(f.hls.levels.map((l) => l.height), [720, 1080]);
    const details = f.loaded.at(-1).details;
    assert.equal(details.fragments.length, 6);
    assert.ok(details.dateRanges['nmss-daterange'], 'DATERANGE 를 잃었다');
    assert.ok(Number.isFinite(details.fragments[0].programDateTime), 'PDT 를 잃었다');
    assert.ok(details.fragments[0].initSegment, 'MAP(init) 를 잃었다');
    assert.ok(details.partList && details.partList.length >= 3, '파트를 잃었다');
    assert.ok(details.canBlockReload);
  } finally { f.hls.destroy(); }
});

test('TS + AES-128 형태: 키를 해석하고 복호화 정보를 붙인다(DRM 제외 빌드에서도 기본 기능)', () => {
  const media = '#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:2\n#EXT-X-MEDIA-SEQUENCE:10\n' +
    '#EXT-X-KEY:METHOD=AES-128,URI="https://api.example.invalid/aes_key",IV=0x00000000000000000000000000000001\n' +
    '#EXTINF:2.000,\nseg10.ts\n#EXTINF:2.000,\nseg11.ts\n#EXTINF:2.000,\nseg12.ts\n';
  const master = '#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=4000000,RESOLUTION=1280x720,CODECS="avc1.4d401f,mp4a.40.2"\nv.m3u8\n';
  const f = load({ lowLatencyMode: false }, master, media);
  try {
    assert.deepEqual(f.errors.map((e) => e.details), []);
    const frag = f.loaded.at(-1).details.fragments[0];
    assert.equal(frag.decryptdata?.method, 'AES-128');
    assert.ok(frag.decryptdata?.uri.endsWith('/aes_key'));
  } finally { f.hls.destroy(); }
});

test('경량 빌드에서 빠진 기능은 호출해도 안전하다(대체 오디오·자막 트랙 목록)', () => {
  const f = load({ lowLatencyMode: false }, MASTER, llMedia());
  try {
    assert.ok(Array.isArray(f.hls.audioTracks) || f.hls.audioTracks === undefined);
    assert.doesNotThrow(() => f.hls.subtitleTracks);
  } finally { f.hls.destroy(); }
});
