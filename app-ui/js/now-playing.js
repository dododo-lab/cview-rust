// 시스템 '지금 재생 중' 정보 — macOS 제어 센터·메뉴 막대(27.3 의 섬 포함)·잠금 화면, Windows SMTC.
//
// 이 앱은 네이티브 MPNowPlayingInfoCenter 를 쓰지 않는다. WKWebView(GPU 프로세스)가 소리 나는 <video> 를 보고
// 앱 이름으로 '지금 재생 중' 항목을 직접 올리고, 그 내용은 **그 <video> 가 들어 있는 문서**의
// `navigator.mediaSession` 에서 가져온다(macOS 27.3 MediaRemote 로 실측, PR 설명 참고).
// - 메타데이터가 없으면 제목은 페이지 주인(호스트) — 그래서 섬에 'localhost' 가 떴다. 그림도 없다.
// - 동작 처리기를 하나도 안 두면 WebKit 기본 명령(10초 앞뒤 건너뛰기·탐색·빨리 감기)이 그대로 노출된다.
//   처리기를 두면 **둔 것만** 시스템 명령이 된다 — 라이브는 재생·일시정지만 둔다.
// - 부모 문서의 mediaSession 에 넣으면 iframe 안 영상에는 메타데이터가 붙지 않는다. 플레이어 iframe 의
//   창(contentWindow)에 넣어야 한다. 메타데이터는 문서마다라 iframe 이 새로 뜨면 다시 넣는다(PlayerBridge).
// - 라이브 표시: WebKit 은 MPNowPlayingInfoPropertyIsLiveStream 을 올리지 않는다(무한 길이여도 길이만 뺀다).
//   그래서 채널명 줄에 '라이브'를 붙여 구분한다.
// - 음소거한 영상은 지금 재생 중 후보가 아니다. 볼륨 0 은 후보다(멀티라이브의 무음 패인).
import { imageSrc, safeUrl } from './dom.js';

/** 이 앱이 시스템에 알리는 동작 — 나머지는 처리기를 비워(null) WebKit 기본 버튼을 숨긴다. */
export const SESSION_ACTIONS = ['play', 'pause', 'stop', 'seekbackward', 'seekforward', 'seekto', 'previoustrack', 'nexttrack', 'skipad'];

const KIND_LABEL = { live: '라이브', vod: '다시보기' };

function clean(value) {
  return String(value ?? '').replace(/\s+/g, ' ').trim();
}

/**
 * 시스템에 보일 메타데이터(MediaMetadataInit).
 * @param {{ kind?: 'live'|'vod', title?: string, channelName?: string, thumbnailUrl?: string, imageUrl?: string }} info
 */
export function nowPlayingMetadata(info = {}) {
  const kind = info.kind === 'vod' ? 'vod' : 'live';
  const channel = clean(info.channelName);
  const title = clean(info.title) || channel || 'CView';
  const label = KIND_LABEL[kind];
  const artwork = [];
  // 영상 썸네일이 먼저(무엇을 보는지), 없으면 채널 사진. 네이버 이미지는 앱 이미지 캐시(cvimg)로 받는다.
  const thumb = safeUrl(info.thumbnailUrl);
  if (thumb) artwork.push({ src: imageSrc(thumb, kind), type: 'image/jpeg' });
  const avatar = safeUrl(info.imageUrl);
  if (avatar) artwork.push({ src: imageSrc(avatar, 'avatar-lg') });
  return {
    title,
    artist: channel ? `${channel} · ${label}` : label,
    album: 'CView',
    artwork,
  };
}

function sessionOf(win) {
  try {
    const s = win && win.navigator && win.navigator.mediaSession;
    return s && typeof s.setActionHandler === 'function' ? s : null;
  } catch {
    return null; // 다른 출처 문서·준비 전
  }
}

// mediaSession(문서마다 새 객체) → 마지막으로 넣은 내용. 같은 값이면 다시 넣지 않는다(멀티라이브는 1초마다 그린다).
const applied = new WeakMap();

function setHandlers(session, handlers) {
  for (const action of SESSION_ACTIONS) {
    const fn = handlers && typeof handlers[action] === 'function' ? handlers[action] : null;
    try {
      session.setActionHandler(action, fn ? (details) => { try { fn(details || {}); } catch (e) { console.warn('[now-playing]', action, e); } } : null);
    } catch { /* 이 엔진이 모르는 동작 */ }
  }
}

/**
 * `win` 문서의 지금 재생 중 정보를 넣는다. 처리기가 바뀌지 않으면 같은 info 로 다시 불러도 아무것도 안 한다.
 * @param {Window} win 영상(<video>)이 들어 있는 문서의 창
 * @param {object|null} info nowPlayingMetadata 의 입력. null 이면 지운다.
 * @param {Record<string, (details: object) => void>} [handlers] 동작 → 처리기
 * @returns {boolean} 넣었는가(mediaSession 이 없으면 false)
 */
export function applyNowPlaying(win, info, handlers = {}) {
  const session = sessionOf(win);
  if (!session) return false;
  if (!info) return clearNowPlaying(win);
  const meta = nowPlayingMetadata(info);
  const key = JSON.stringify(meta);
  const prev = applied.get(session);
  if (prev && prev.key === key && prev.handlers === handlers) return true;
  try {
    const Meta = win.MediaMetadata;
    session.metadata = typeof Meta === 'function' ? new Meta(meta) : null;
  } catch (e) {
    // 그림 주소 하나가 잘못돼도 글자는 보이게.
    try { session.metadata = new win.MediaMetadata({ ...meta, artwork: [] }); } catch { /* 그대로 */ }
    console.warn('[now-playing] 메타데이터', e);
  }
  if (!prev || prev.handlers !== handlers) setHandlers(session, handlers);
  applied.set(session, { key, handlers });
  return true;
}

/** 지운다 — 재생을 끝내거나 화면을 닫을 때. 시스템 항목은 WebKit 이 영상이 비면 내린다. */
export function clearNowPlaying(win) {
  const session = sessionOf(win);
  if (!session) return false;
  if (!applied.has(session) && !session.metadata) return true;
  try { session.metadata = null; } catch { /* 무시 */ }
  setHandlers(session, null);
  applied.delete(session);
  return true;
}

/** 라이브 처리기 — 재생·일시정지만(라이브에서 10초 건너뛰기·탐색은 뜻이 없다). */
export function liveHandlers({ play, pause }) {
  return { play, pause };
}

/** 다시보기 처리기 — 재생·일시정지·10초 앞뒤·위치 이동. */
export function vodHandlers(video, { step = 10 } = {}) {
  const seek = (t) => {
    if (!video || !Number.isFinite(t)) return;
    const end = Number.isFinite(video.duration) ? video.duration : Infinity;
    video.currentTime = Math.max(0, Math.min(end, t));
  };
  return {
    play: () => { video.play().catch(() => {}); },
    pause: () => video.pause(),
    seekbackward: (d) => seek(video.currentTime - (Number(d.seekOffset) || step)),
    seekforward: (d) => seek(video.currentTime + (Number(d.seekOffset) || step)),
    seekto: (d) => {
      if (d.fastSeek && typeof video.fastSeek === 'function' && Number.isFinite(d.seekTime)) video.fastSeek(d.seekTime);
      else seek(Number(d.seekTime));
    },
  };
}

/**
 * 멀티라이브에서 시스템에 보일 패인 — 소리 나는 패인 중 라이브 무대(watch)·선택·포커스 메인 순, 소리 나는 패인이
 * 없으면 같은 순서로 아무 패인. 플레이어가 없는 세션(slot 없음)·끝난 방송(state.kind 'offline')은 고르지 않는다.
 * @param {Array<{id: string, slot?: number|null, isAudible?: boolean}>} sessions
 * @param {{ watchId?: string|null, selectedId?: string|null, focusMainId?: string|null }} [state]
 */
export function pickNowPlayingSession(sessions, state = {}) {
  // 끝난 방송은 슬롯·소리 라우팅이 남아도 고르지 않는다.
  const list = (sessions || []).filter((s) => s && s.slot != null && !(s.state && s.state.kind === 'offline'));
  if (!list.length) return null;
  const prefer = [state.watchId, state.selectedId, state.focusMainId].filter(Boolean);
  const pick = (pool) => {
    for (const id of prefer) {
      const hit = pool.find((s) => s.id === id);
      if (hit) return hit;
    }
    return pool[0] || null;
  };
  return pick(list.filter((s) => s.isAudible)) || pick(list);
}

/** 멀티라이브 세션 → 지금 재생 중 정보. */
export function multiLiveNowPlayingInfo(s) {
  if (!s) return null;
  return {
    kind: 'live',
    title: s.liveTitle,
    channelName: s.channelName || s.channelId,
    thumbnailUrl: s.thumbnailUrl,
    imageUrl: s.profileImageUrl,
  };
}
