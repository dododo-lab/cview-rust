// 시스템 '지금 재생 중'(macOS 섬·제어 센터, Windows SMTC) 정보 검사 — 브라우저 없이 Node 로.
// 저장소 루트에서: node tools/ui-regression/now-playing.mjs
import assert from 'node:assert/strict';

const {
  nowPlayingMetadata, applyNowPlaying, clearNowPlaying, liveHandlers, vodHandlers,
  pickNowPlayingSession, multiLiveNowPlayingInfo, SESSION_ACTIONS,
} = await import('../../app-ui/js/now-playing.js');

// ── 메타데이터 ──
const live = nowPlayingMetadata({
  kind: 'live',
  title: '  오늘의   방송 ',
  channelName: '채널',
  thumbnailUrl: 'https://livecloud-thumb.akamaized.net/a/image_480.jpg',
  imageUrl: 'https://nng-phinf.pstatic.net/p.png',
});
assert.equal(live.title, '오늘의 방송', '공백 정리');
assert.equal(live.artist, '채널 · 라이브', '라이브 표시는 채널 줄에');
assert.equal(live.album, 'CView');
assert.equal(live.artwork.length, 2, '썸네일 먼저, 채널 사진 다음');
assert.match(live.artwork[0].src, /image_405\.jpg$/, '라이브 썸네일은 카드와 같은 크기로');
assert.match(live.artwork[1].src, /type=f240_240_na/);

const vod = nowPlayingMetadata({ kind: 'vod', title: '', channelName: '채널' });
assert.equal(vod.title, '채널', '제목이 없으면 채널명');
assert.equal(vod.artist, '채널 · 다시보기');
assert.deepEqual(vod.artwork, []);
assert.equal(nowPlayingMetadata({}).title, 'CView', 'localhost 대신 앱 이름');
assert.equal(nowPlayingMetadata({ channelName: '' }).artist, '라이브');
// 이상한 주소는 그림으로 넘기지 않는다.
assert.deepEqual(nowPlayingMetadata({ thumbnailUrl: 'javascript:alert(1)', imageUrl: 'data:image/png;base64,AA' }).artwork, []);

// ── mediaSession 에 넣기 ──
function fakeWindow() {
  const calls = [];
  class MediaMetadata { constructor(init) { Object.assign(this, init); calls.push(['meta', init.title]); } }
  const handlers = new Map();
  const mediaSession = {
    metadata: null,
    setActionHandler(action, fn) {
      if (action === 'skipad') throw new TypeError('모르는 동작'); // 엔진이 모르는 동작은 건너뛴다
      calls.push(['handler', action, Boolean(fn)]);
      handlers.set(action, fn);
    },
  };
  return { win: { MediaMetadata, navigator: { mediaSession } }, calls, handlers, mediaSession };
}

{
  const { win, calls, handlers, mediaSession } = fakeWindow();
  let played = 0;
  let paused = 0;
  const h = liveHandlers({ play: () => played++, pause: () => paused++ });
  const info = { kind: 'live', title: '방송', channelName: '채널' };
  assert.equal(applyNowPlaying(win, info, h), true);
  assert.equal(mediaSession.metadata.title, '방송');
  assert.equal(mediaSession.metadata.artist, '채널 · 라이브');
  // 라이브는 재생·일시정지만 — 10초 건너뛰기·탐색은 비워서 WebKit 기본 버튼을 숨긴다.
  for (const a of SESSION_ACTIONS) {
    if (a === 'skipad') continue;
    assert.equal(Boolean(handlers.get(a)), a === 'play' || a === 'pause', a);
  }
  handlers.get('play')();
  handlers.get('pause')();
  assert.deepEqual([played, paused], [1, 1]);
  // 같은 내용이면 다시 넣지 않는다(멀티라이브는 1초마다 그린다).
  const n = calls.length;
  applyNowPlaying(win, { ...info }, h);
  assert.equal(calls.length, n, '같은 정보·처리기면 아무것도 안 한다');
  // 제목이 바뀌면 메타데이터만 다시.
  applyNowPlaying(win, { ...info, title: '새 제목' }, h);
  assert.equal(mediaSession.metadata.title, '새 제목');
  assert.equal(calls.slice(n).filter((c) => c[0] === 'handler').length, 0, '처리기는 그대로');
  // 처리기 오류가 밖으로 새지 않는다.
  applyNowPlaying(win, info, liveHandlers({ play: () => { throw new Error('x'); }, pause: () => {} }));
  const warn = console.warn;
  console.warn = () => {};
  try { handlers.get('play')(); } finally { console.warn = warn; }
  // 지우기
  assert.equal(clearNowPlaying(win), true);
  assert.equal(mediaSession.metadata, null);
  for (const a of SESSION_ACTIONS) if (a !== 'skipad') assert.equal(handlers.get(a), null, a);
  // null 정보도 지우기
  applyNowPlaying(win, info, h);
  applyNowPlaying(win, null);
  assert.equal(mediaSession.metadata, null);
}

// mediaSession 이 없거나 다른 출처(접근 시 throw)면 조용히 false.
assert.equal(applyNowPlaying({ navigator: {} }, { title: 'x' }), false);
assert.equal(applyNowPlaying(null, { title: 'x' }), false);
const crossOrigin = { get navigator() { throw new Error('SecurityError'); } };
assert.equal(applyNowPlaying(crossOrigin, { title: 'x' }), false);
assert.equal(clearNowPlaying(crossOrigin), false);

// 새 문서(새 mediaSession)에는 같은 정보라도 다시 넣는다.
{
  const a = fakeWindow();
  const b = fakeWindow();
  const h = liveHandlers({ play() {}, pause() {} });
  applyNowPlaying(a.win, { title: '방송' }, h);
  applyNowPlaying(b.win, { title: '방송' }, h);
  assert.equal(b.mediaSession.metadata.title, '방송');
}

// ── 다시보기 처리기 ──
{
  const video = {
    currentTime: 100, duration: 300, paused: true,
    play() { this.paused = false; return Promise.resolve(); },
    pause() { this.paused = true; },
  };
  const h = vodHandlers(video);
  h.seekbackward({});
  assert.equal(video.currentTime, 90);
  h.seekforward({ seekOffset: 30 });
  assert.equal(video.currentTime, 120);
  h.seekto({ seekTime: 999 });
  assert.equal(video.currentTime, 300, '끝을 넘지 않는다');
  h.seekbackward({ seekOffset: 1000 });
  assert.equal(video.currentTime, 0, '0 아래로 가지 않는다');
  h.seekto({ seekTime: Number.NaN });
  assert.equal(video.currentTime, 0, '숫자가 아니면 무시');
  h.play();
  assert.equal(video.paused, false);
  h.pause();
  assert.equal(video.paused, true);
}

// ── 멀티라이브: 소리 나는 패인 기준 ──
const S = (id, slot, isAudible = false) => ({ id, slot, isAudible, channelId: `c-${id}`, channelName: `채널${id}`, liveTitle: `방송${id}` });
assert.equal(pickNowPlayingSession([], {}), null);
assert.equal(pickNowPlayingSession([S('a', null, true)], {}), null, '플레이어 없는 세션은 고르지 않는다');
assert.equal(pickNowPlayingSession([S('a', 0), S('b', 1, true)], { selectedId: 'a' }).id, 'b', '소리 나는 패인이 먼저');
assert.equal(pickNowPlayingSession([S('a', 0, true), S('b', 1, true)], { selectedId: 'b' }).id, 'b', '여럿이 들리면 선택된 패인');
assert.equal(pickNowPlayingSession([S('a', 0, true), S('b', 1, true)], { watchId: 'a', selectedId: 'b' }).id, 'a', '라이브 무대가 가장 먼저');
assert.equal(pickNowPlayingSession([S('a', 0), S('b', 1)], { selectedId: 'b' }).id, 'b', '모두 무음이면 선택된 패인');
assert.equal(pickNowPlayingSession([S('a', 0), S('b', 1)], {}).id, 'a');
assert.deepEqual(multiLiveNowPlayingInfo({ ...S('a', 0), channelName: '', thumbnailUrl: 't', profileImageUrl: 'p' }), {
  kind: 'live', title: '방송a', channelName: 'c-a', thumbnailUrl: 't', imageUrl: 'p',
});
assert.equal(multiLiveNowPlayingInfo(null), null);

console.log('now-playing: ok');
